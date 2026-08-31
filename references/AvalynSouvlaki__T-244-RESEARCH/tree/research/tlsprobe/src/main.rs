//! tlsprobe — a TLS + HTTP/2 fingerprint oracle.
//!
//! Stands up a local HTTPS listener, captures what a client actually puts on
//! the wire, and reports JA3, JA4, JA4_r, the Akamai HTTP/2 fingerprint and the
//! header order. Intended to be run in CI and asserted against a golden value.
//!
//!   tlsprobe                          # :8443, TLS terminated, full report
//!   tlsprobe --port 9000 --json       # machine-readable, one object per line
//!   tlsprobe --raw                    # no TLS termination; ClientHello only
//!   tlsprobe --expect-ja4 t13d1516h2_8daaf6152771_e5627efa2ab1 --once
//!
//! Exit status is 1 if any --expect assertion fails, so it drops straight into
//! a test target.

mod h2fp;
mod huffman;
mod tlsfp;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

struct Args {
    port: u16,
    json: bool,
    raw: bool,
    once: bool,
    expect_ja4: Option<String>,
    expect_ja3: Option<String>,
    expect_akamai: Option<String>,
}

fn parse_args() -> Args {
    let mut a = Args {
        port: 8443,
        json: false,
        raw: false,
        once: false,
        expect_ja4: None,
        expect_ja3: None,
        expect_akamai: None,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        let next = |i: &mut usize| {
            *i += 1;
            argv.get(*i).cloned().unwrap_or_else(|| {
                eprintln!("error: {} needs a value", argv[*i - 1]);
                std::process::exit(2);
            })
        };
        match argv[i].as_str() {
            "--port" | "-p" => a.port = next(&mut i).parse().unwrap_or(8443),
            "--json" => a.json = true,
            "--raw" => a.raw = true,
            "--once" => a.once = true,
            "--expect-ja4" => a.expect_ja4 = Some(next(&mut i)),
            "--expect-ja3" => a.expect_ja3 = Some(next(&mut i)),
            "--expect-akamai" => a.expect_akamai = Some(next(&mut i)),
            "-h" | "--help" => {
                println!("{}", HELP);
                std::process::exit(0);
            }
            other => {
                eprintln!("error: unknown argument {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    a
}

const HELP: &str = "\
tlsprobe — TLS + HTTP/2 fingerprint oracle

USAGE:
    tlsprobe [OPTIONS]

OPTIONS:
    -p, --port <N>          listen port (default 8443)
        --raw               do not terminate TLS; capture the ClientHello only
        --json              emit one JSON object per connection
        --once              exit after the first connection
        --expect-ja4 <S>    assert the JA4 string, else exit 1
        --expect-ja3 <S>    assert the JA3 hash, else exit 1
        --expect-akamai <S> assert the Akamai HTTP/2 fingerprint, else exit 1
    -h, --help              this text";

/// A throwaway CA-less leaf. Clients must be pointed at this with verification
/// off — the certificate is camouflage so the handshake completes, nothing more.
fn tls_config() -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error>> {
    let cert = rcgen::generate_simple_self_signed(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ])?;
    let der = rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());
    let key = rustls::pki_types::PrivateKeyDer::try_from(cert.signing_key.serialize_der())?;

    let mut cfg = rustls::ServerConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(vec![der], key)?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(cfg))
}

fn esc(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            c if (c as u32) < 0x20 => vec![' '],
            c => vec![c],
        })
        .collect()
}

fn jarr(v: &[String]) -> String {
    format!(
        "[{}]",
        v.iter().map(|s| format!("\"{}\"", esc(s))).collect::<Vec<_>>().join(",")
    )
}

fn report(ch: &tlsfp::ClientHello, h2: Option<&h2fp::H2Fingerprint>, http1: &[String], a: &Args) -> bool {
    let (ja3_str, ja3_hash) = ch.ja3(true);
    let (_, ja3_raw_hash) = ch.ja3(false);
    let ja4 = ch.ja4();
    let akamai = h2.map(|h| h.akamai());
    let headers: Vec<String> = h2
        .map(|h| h.headers.clone())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| http1.to_vec());

    if a.json {
        let hexlist = |v: &[u16]| v.iter().map(|x| format!("0x{x:04x}")).collect::<Vec<_>>();
        println!(
            "{{\"ja4\":\"{}\",\"ja4_r\":\"{}\",\"ja3\":\"{}\",\"ja3_nogrease_hash\":\"{}\",\
             \"ja3_raw_hash\":\"{}\",\"sni\":{},\"alpn\":{},\"akamai\":{},\"settings\":{},\
             \"header_order\":{},\"extensions\":{},\"curves\":{}}}",
            esc(&ja4),
            esc(&ch.ja4_r()),
            esc(&ja3_str),
            ja3_hash,
            ja3_raw_hash,
            ch.sni.as_ref().map(|s| format!("\"{}\"", esc(s))).unwrap_or("null".into()),
            jarr(&ch.alpn),
            akamai.as_ref().map(|s| format!("\"{}\"", esc(s))).unwrap_or("null".into()),
            jarr(&h2.map(|h| h.settings_pretty()).unwrap_or_default()),
            jarr(&headers),
            jarr(&hexlist(&ch.extensions)),
            jarr(&hexlist(&ch.curves)),
        );
    } else {
        println!("{}", "=".repeat(72));
        println!("  JA4      {ja4}");
        println!("  JA4_r    {}", ch.ja4_r());
        println!("  JA3      {ja3_hash}  (GREASE-filtered)");
        println!("  JA3 raw  {ja3_raw_hash}  (unfiltered, per original spec)");
        println!("  JA3 str  {ja3_str}");
        println!();
        println!("  SNI      {}", ch.sni.clone().unwrap_or_else(|| "(none — IP literal)".into()));
        println!("  ALPN     {:?}", ch.alpn);
        println!("  TLS ver  0x{:04x}", ch.effective_version());
        println!(
            "  ciphers  {} ({} GREASE)",
            ch.ciphers.len(),
            ch.ciphers.iter().filter(|&&c| tlsfp::is_grease(c)).count()
        );
        println!(
            "  ext order {}",
            ch.extensions.iter().map(|e| format!("0x{e:04x}")).collect::<Vec<_>>().join(" ")
        );
        println!(
            "  curves    {}",
            ch.curves.iter().map(|c| format!("0x{c:04x}")).collect::<Vec<_>>().join(" ")
        );
        println!("\n  browser markers:");
        for (name, present) in ch.browser_markers() {
            println!("    [{}] {name}", if present { "x" } else { " " });
        }
        if let Some(h) = h2 {
            println!("\n  --- HTTP/2 ---");
            println!("  Akamai   {}", h.akamai());
            for s in h.settings_pretty() {
                println!("    SETTINGS  {s}");
            }
            if let Some(w) = h.window_update {
                println!("    WINDOW_UPDATE  {w}");
            }
            for (s, e, d, wt) in &h.priorities {
                println!("    PRIORITY  stream={s} excl={e} dep={d} weight={wt}");
            }
        }
        if !headers.is_empty() {
            println!("\n  header order ({}):", headers.len());
            for (i, h) in headers.iter().enumerate() {
                println!("    {:2}. {h}", i + 1);
            }
        }
        println!("{}", "=".repeat(72));
    }

    let mut ok = true;
    let mut check = |label: &str, want: &Option<String>, got: Option<&String>| {
        if let Some(w) = want {
            match got {
                Some(g) if g == w => eprintln!("PASS {label}: {g}"),
                Some(g) => {
                    eprintln!("FAIL {label}\n  want {w}\n  got  {g}");
                    ok = false;
                }
                None => {
                    eprintln!("FAIL {label}: not captured");
                    ok = false;
                }
            }
        }
    };
    check("ja4", &a.expect_ja4, Some(&ja4));
    check("ja3", &a.expect_ja3, Some(&ja3_hash));
    check("akamai", &a.expect_akamai, akamai.as_ref());
    ok
}

/// Drain the client's opening flight. We stop as soon as we have HEADERS,
/// otherwise the read timeout ends it — a client that is waiting on our
/// response would otherwise hold the connection open indefinitely.
fn read_h2_flight(stream: &mut impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if h2fp::parse(&buf).saw_headers {
                    break;
                }
                if buf.len() > 1 << 20 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    buf
}

fn handle(tcp: TcpStream, cfg: Option<&Arc<rustls::ServerConfig>>, a: &Args) -> Option<bool> {
    tcp.set_read_timeout(Some(Duration::from_millis(2500))).ok()?;

    // Peek leaves the bytes in the kernel buffer, so rustls can still read the
    // same ClientHello afterwards. This is what lets one connection yield both
    // the TLS and the HTTP/2 fingerprint.
    let mut peek = vec![0u8; 16384];
    let n = tcp.peek(&mut peek).ok()?;
    peek.truncate(n);

    let ch = match tlsfp::parse(&peek) {
        Some(ch) => ch,
        None => {
            eprintln!("note: {n} bytes that are not a ClientHello — ignoring");
            return None;
        }
    };

    let Some(cfg) = cfg else {
        return Some(report(&ch, None, &[], a));
    };

    let conn = match rustls::ServerConnection::new(cfg.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("note: rustls setup failed ({e}); reporting TLS only");
            return Some(report(&ch, None, &[], a));
        }
    };
    let mut tls = rustls::StreamOwned::new(conn, tcp);

    // A browser-shaped ClientHello can legitimately fail against this throwaway
    // cert (pinning, ECH, a required cipher we lack). That is not a probe error
    // — fall back to the TLS-only report rather than losing the capture.
    if let Err(e) = tls.conn.complete_io(&mut tls.sock) {
        eprintln!("note: handshake did not complete ({e}); reporting TLS only");
        return Some(report(&ch, None, &[], a));
    }

    let alpn = tls.conn.alpn_protocol().map(|p| p.to_vec());
    if alpn.as_deref() == Some(b"h2") {
        // Our own SETTINGS unblocks clients that wait for one before sending HEADERS.
        let _ = tls.write_all(&[0, 0, 0, 0x4, 0, 0, 0, 0, 0]);
        let _ = tls.flush();
        let buf = read_h2_flight(&mut tls);
        let fp = h2fp::parse(&buf);
        let ok = report(&ch, Some(&fp), &[], a);
        let _ = tls.write_all(&[0, 0, 8, 0x7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        Some(ok)
    } else {
        // HTTP/1.1: the request line and header order are still worth having.
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = tls.read(&mut chunk) {
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let names: Vec<String> = text
            .lines()
            .skip(1)
            .take_while(|l| !l.is_empty())
            .filter_map(|l| l.split(':').next().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        let _ = tls.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
        Some(report(&ch, None, &names, a))
    }
}

fn main() {
    let a = parse_args();
    let cfg = if a.raw {
        None
    } else {
        match tls_config() {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("error: could not build a TLS config ({e}); falling back to --raw");
                None
            }
        }
    };

    let listener = match TcpListener::bind(("127.0.0.1", a.port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot bind 127.0.0.1:{} — {e}", a.port);
            std::process::exit(2);
        }
    };
    eprintln!(
        "tlsprobe listening on 127.0.0.1:{} ({})",
        a.port,
        if cfg.is_some() { "TLS terminated, ALPN h2/http1.1" } else { "raw ClientHello only" }
    );

    let mut all_ok = true;
    for s in listener.incoming().flatten() {
        if let Some(ok) = handle(s, cfg.as_ref(), &a) {
            all_ok &= ok;
            if a.once {
                break;
            }
        }
    }
    if !all_ok {
        std::process::exit(1);
    }
}
