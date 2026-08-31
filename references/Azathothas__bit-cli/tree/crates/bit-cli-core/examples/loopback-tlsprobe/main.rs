//! A TLS and HTTP/2 fingerprint oracle on loopback.
//!
//! It exists so that what `bit-cli` puts on the wire can be **asserted**
//! rather than eyeballed. `TODO/cli-surface.md` T-244 ships a client that
//! fetches a web page, and its acceptance asks for the header set and the TLS
//! fingerprint to be checked against a recorded capture. Nothing in this tree
//! could see either: a client's own view of its handshake is the view it
//! intended, and the only honest reading is off the wire.
//!
//! So this stands up a listener, captures the `ClientHello` by `peek`ing the
//! socket, optionally terminates TLS with a throwaway certificate, negotiates
//! `h2` over ALPN, and reads the client's opening flight. It reports JA3,
//! JA4, JA4_r, the Akamai HTTP/2 fingerprint and the HPACK-decoded header
//! order.
//!
//! It is a test fixture, not a product. It serves one connection at a time,
//! answers nothing useful, and its certificate is camouflage: a client has to
//! be pointed at it with verification off.
//!
//! ```text
//! cargo run -p bit-cli-core --example loopback-tlsprobe -- --port 0
//! ```
//!
//! Port `0` asks the operating system for a free one. The base URL is printed
//! to stdout as a single line before the first connection is accepted, the
//! same as the other loopback fixtures, so a script can read it and point a
//! client at it. Everything else goes to stderr unless `--json` is passed,
//! which puts one JSON object per connection on stdout after that first line.
//!
//! **Assert JA4, never JA3.** JA4 sorts ciphers and extensions before hashing,
//! so it survives a client that shuffles its extension order; JA3 preserves
//! wire order and will flake. A survey of one impersonating client measured
//! three distinct JA3 hashes and one stable JA4 across six captures of the
//! same binary.
//!
//! **`--ca-out` is how a client that verifies certificates completes a
//! handshake here.** The probe mints its own certificate authority, signs the
//! leaf with it, and writes the authority to the path given. A client told to
//! trust that one file then verifies the chain and connects, which is what the
//! HTTP/2 half of a fingerprint needs: the Akamai fingerprint and the
//! HPACK-decoded header order only exist after ALPN picks `h2`. Nothing is
//! disabled to get there. `bit-cli` reads such a file from
//! `BIT_CLI_EXTRA_CA_FILE`, which **adds** a root and never replaces the
//! usual ones.
//!
//! **`--plain` reads the same header order off cleartext HTTP/1.1.** It needs
//! no handshake and no certificate at all, so it is the capture that still
//! works when a client cannot be told to trust anything.
//!
//! **Capture JA4 in `--raw` mode.** Terminating the handshake can change what
//! a client offers: a client told to skip certificate verification may fall
//! back to a different `signature_algorithms` list, and a JA4 read through
//! that handshake is not the JA4 it ships. `--raw` never completes a
//! handshake, so nothing has to be disabled to reach it. The Akamai HTTP/2
//! fingerprint needs the opposite: it only exists after ALPN picks `h2`.
//!
//! **`--bind` is how a browser that is not on this machine reaches it.** The
//! default is `127.0.0.1` and nothing else changes it. A WSL2 distro in NAT
//! mode cannot reach the Windows loopback at all, so a capture from a browser
//! installed in a throwaway distro needs the probe on the address that distro
//! reaches the host at, which `wsl-ephemeral.ps1 -Action HostAddress` prints.
//! The leaf certificate carries whatever `--bind` names, so a client that
//! trusts `--ca-out` still verifies the name it dialled. The unspecified
//! address is refused by name: a fixture that terminates TLS and records
//! header values does not belong on every interface, and the specific address
//! is always knowable.
//!
//! Exit status is 1 when any `--expect` assertion fails, 2 when it could not
//! run at all, so it drops into an acceptance script unchanged.

mod h2fp;
mod huffman;
mod tlsfp;

use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

/// Every switch, parsed by hand because an example takes no `clap`.
struct Args {
    bind: IpAddr,
    port: u16,
    json: bool,
    raw: bool,
    plain: bool,
    ca_out: Option<String>,
    header_values: bool,
    hello_out: Option<String>,
    once: bool,
    expect_ja4: Option<String>,
    expect_ja3: Option<String>,
    expect_akamai: Option<String>,
    expect_file: Option<String>,
    write_golden: Option<String>,
    until_h2: bool,
}

const HELP: &str = "\
loopback-tlsprobe: a TLS and HTTP/2 fingerprint oracle on loopback

USAGE:
    loopback-tlsprobe [OPTIONS]

OPTIONS:
    -p, --port <N>            listen port, 0 for one the OS picks (default 0)
    -b, --bind <ADDR>         listen address (default 127.0.0.1). Use it to
                              reach a browser that is not on this machine; the
                              unspecified address is refused
        --raw                 do not terminate TLS, capture the ClientHello only
        --plain               speak cleartext HTTP/1.1, capture the header order
        --ca-out <PATH>       write the generated CA certificate here, as PEM,
                              so a verifying client can be told to trust it
        --header-values       record header values as well as names. Only for
                              a browser this repository launched itself
        --hello-out <PATH>    write the raw ClientHello here, as one hex line,
                              which is how a parser test vector is recorded
        --json                one JSON object per connection on stdout
        --once                exit after the first connection
        --until-h2            exit after the first connection that completed
                              HTTP/2. A browser opens sockets it abandons, so
                              --once can capture one of those and nothing else
        --expect-ja4 <S>      assert the JA4 string, else exit 1
        --expect-ja3 <S>      assert the JA3 hash, else exit 1
        --expect-akamai <S>   assert the Akamai HTTP/2 fingerprint, else exit 1
        --expect-file <PATH>  assert every field a golden manifest carries
        --write-golden <PATH> write the first capture as a golden manifest
    -h, --help                this text

The base URL is printed to stdout as one line before the first connection.";

fn next_value(argv: &[String], i: &mut usize) -> String {
    *i += 1;
    match argv.get(*i) {
        Some(v) => v.clone(),
        None => {
            eprintln!("loopback-tlsprobe: {} needs a value", argv[*i - 1]);
            std::process::exit(2);
        }
    }
}

/// The address the listener binds, refusing the two shapes that are a mistake
/// rather than a choice.
///
/// A hostname is refused because the leaf certificate and the announced URL
/// both need a literal, and resolving one here would put a name in the
/// certificate that does not match what the caller dialled. The unspecified
/// address is refused because this fixture terminates TLS with a throwaway
/// authority and records header values, and neither belongs on every interface
/// a machine has.
fn parse_bind(raw: &str) -> IpAddr {
    let Ok(addr) = raw.parse::<IpAddr>() else {
        eprintln!(
            "loopback-tlsprobe: --bind {raw} is not an IP address. \
             A hostname is not accepted: the certificate and the announced URL \
             both carry the literal this binds."
        );
        std::process::exit(2);
    };
    if addr.is_unspecified() {
        eprintln!(
            "loopback-tlsprobe: --bind {raw} would listen on every interface, \
             which this fixture does not do. Name the one address the client \
             reaches this host at; for a WSL2 distro in NAT mode that is \
             `wsl-ephemeral.ps1 -Action HostAddress`."
        );
        std::process::exit(2);
    }
    addr
}

fn parse_args() -> Args {
    parse_argv(&std::env::args().skip(1).collect::<Vec<String>>())
}

/// The parser, over a slice a test can supply.
///
/// `parse_args` reads the process's own command line, which a test cannot set,
/// so the loop lives here and that one is the two lines that fetch it.
fn parse_argv(argv: &[String]) -> Args {
    let mut a = Args {
        bind: IpAddr::from([127, 0, 0, 1]),
        port: 0,
        json: false,
        raw: false,
        plain: false,
        ca_out: None,
        header_values: false,
        hello_out: None,
        once: false,
        expect_ja4: None,
        expect_ja3: None,
        expect_akamai: None,
        expect_file: None,
        write_golden: None,
        until_h2: false,
    };
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--port" | "-p" => {
                let raw = next_value(argv, &mut i);
                a.port = match raw.parse() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("loopback-tlsprobe: --port {raw} is not a port number");
                        std::process::exit(2);
                    }
                };
            }
            "--bind" | "-b" => {
                let raw = next_value(argv, &mut i);
                a.bind = parse_bind(&raw);
            }
            "--json" => a.json = true,
            "--raw" => a.raw = true,
            "--plain" => a.plain = true,
            "--ca-out" => a.ca_out = Some(next_value(argv, &mut i)),
            "--header-values" => a.header_values = true,
            "--hello-out" => a.hello_out = Some(next_value(argv, &mut i)),
            "--once" => a.once = true,
            "--until-h2" => a.until_h2 = true,
            "--expect-ja4" => a.expect_ja4 = Some(next_value(argv, &mut i)),
            "--expect-ja3" => a.expect_ja3 = Some(next_value(argv, &mut i)),
            "--expect-akamai" => a.expect_akamai = Some(next_value(argv, &mut i)),
            "--expect-file" => a.expect_file = Some(next_value(argv, &mut i)),
            "--write-golden" => a.write_golden = Some(next_value(argv, &mut i)),
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            other => {
                eprintln!("loopback-tlsprobe: unknown argument {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    a
}

/// A throwaway certificate authority and one leaf signed by it.
///
/// Both are generated per run and neither is written anywhere except the path
/// `--ca-out` names. A client that trusts that one file verifies the chain
/// normally: the point is to reach the HTTP/2 half of a fingerprint **without**
/// anything on either side skipping verification, because a client that skips
/// it can offer a different `signature_algorithms` list and the fingerprint
/// read through that handshake is not the one it ships.
///
/// The leaf carries `localhost`, `127.0.0.1` and `::1`, which is every name a
/// loopback fixture is reached by, plus whatever `--bind` named when that is
/// something else. Without that last one a client dialling the WSL adapter
/// address would fail on the name rather than on the chain, which is the same
/// error message for a different cause and would send a session looking at the
/// authority.
fn tls_config(
    ca_out: Option<&str>,
    bind: IpAddr,
) -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error>> {
    let mut ca_params = rcgen::CertificateParams::new(Vec::new())?;
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "loopback-tlsprobe throwaway CA");
    ca_params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    let ca = rcgen::CertifiedIssuer::self_signed(ca_params, rcgen::KeyPair::generate()?)?;

    let mut names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    let bound = bind.to_string();
    if !names.contains(&bound) {
        names.push(bound);
    }
    let mut leaf_params = rcgen::CertificateParams::new(names)?;
    leaf_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "localhost");
    leaf_params.use_authority_key_identifier_extension = true;
    let leaf_key = rcgen::KeyPair::generate()?;
    let leaf_cert = leaf_params.signed_by(&leaf_key, &ca)?;

    if let Some(path) = ca_out {
        // An absolute path, because a relative one handed to a .NET or std
        // file API is relative to the process's own directory rather than to
        // wherever a script thinks it is. See TODO/RULES.md section 5.
        let absolute = std::path::absolute(path)?;
        std::fs::write(&absolute, ca.pem())?;
        eprintln!(
            "loopback-tlsprobe: wrote the CA certificate to {}",
            absolute.display()
        );
    }

    // The leaf alone, without the CA behind it. That is what a server is
    // supposed to send, and it is also what makes the client's extra-root
    // path fire: Windows' own verifier only consults roots handed to it when
    // the chain it built is partial, so sending the CA makes it build a
    // complete chain to an authority nobody trusts and stop there.
    let chain = vec![rustls::pki_types::CertificateDer::from(
        leaf_cert.der().to_vec(),
    )];
    let key = rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der())?;

    let mut cfg = rustls::ServerConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(chain, key)?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(cfg))
}

/// Escape a string for a JSON scalar. Control bytes become a space rather than
/// an escape, because a header name holding one is a defect to see, not text
/// to round trip.
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
        v.iter()
            .map(|s| format!("\"{}\"", esc(s)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// What one connection yielded, in the shape a golden manifest carries.
struct Capture {
    ja4: String,
    ja4_r: String,
    ja3: String,
    akamai: Option<String>,
    headers: Vec<String>,
    /// Header values beside their names, and only under `--header-values`.
    /// Empty is the shipping case and the default.
    header_pairs: Vec<(String, String)>,
}

impl Capture {
    fn to_json(&self) -> String {
        format!(
            "{{\"ja4\":\"{}\",\"ja4_r\":\"{}\",\"ja3\":\"{}\",\"akamai\":{},\"headers\":{}}}",
            esc(&self.ja4),
            esc(&self.ja4_r),
            esc(&self.ja3),
            self.akamai
                .as_ref()
                .map(|s| format!("\"{}\"", esc(s)))
                .unwrap_or_else(|| "null".into()),
            jarr(&self.headers),
        )
    }

    /// The same object with the header values beside the names.
    ///
    /// A second method rather than a field in `to_json`, so the shipping
    /// shape cannot grow values by accident: a caller has to ask.
    fn to_json_with_values(&self) -> String {
        let pairs = self
            .header_pairs
            .iter()
            .map(|(n, v)| format!("[\"{}\",\"{}\"]", esc(n), esc(v)))
            .collect::<Vec<_>>()
            .join(",");
        let base = self.to_json();
        format!(
            "{},\"header_pairs\":[{}]}}",
            base.trim_end_matches('}'),
            pairs
        )
    }
}

/// One expected value read out of a golden manifest.
///
/// The manifest is read with a scanner rather than a JSON crate, because an
/// example carries no `serde` of its own and the document is one flat object
/// this program wrote. A field that is absent is not asserted, so a manifest
/// carrying only `ja4` checks only that.
fn golden_field(text: &str, name: &str) -> Option<String> {
    let key = format!("\"{name}\"");
    let start = text.find(&key)? + key.len();
    let rest = text.get(start..)?.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if rest.starts_with("null") {
        return None;
    }
    let body = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

/// Every string in a golden manifest's `headers` array, in order.
fn golden_headers(text: &str) -> Option<Vec<String>> {
    let start = text.find("\"headers\"")? + "\"headers\"".len();
    let rest = text.get(start..)?.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let body = rest.strip_prefix('[')?;
    let end = body.find(']')?;
    let mut out = Vec::new();
    let mut chars = body[..end].chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut item = String::new();
        while let Some(c) = chars.next() {
            match c {
                // An escaped quote is part of the name, not the end of it.
                // Header names off the wire do not carry one, which is why
                // this is here: a reader that stops at the first backslash
                // quote would silently truncate a name that did.
                '\\' => item.push(chars.next()?),
                '"' => break,
                c => item.push(c),
            }
        }
        out.push(item);
    }
    Some(out)
}

fn describe(
    ch: &tlsfp::ClientHello,
    h2: Option<&h2fp::H2Fingerprint>,
    http1: &[String],
) -> Capture {
    let (_, ja3_hash) = ch.ja3(true);
    Capture {
        ja4: ch.ja4(),
        ja4_r: ch.ja4_r(),
        ja3: ja3_hash,
        akamai: h2.map(h2fp::H2Fingerprint::akamai),
        headers: h2
            .map(|h| h.headers.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| http1.to_vec()),
        header_pairs: h2.map(|h| h.header_pairs.clone()).unwrap_or_default(),
    }
}

fn print_human(ch: &tlsfp::ClientHello, h2: Option<&h2fp::H2Fingerprint>, cap: &Capture) {
    let (ja3_str, _) = ch.ja3(true);
    let (_, ja3_raw) = ch.ja3(false);
    eprintln!("{}", "=".repeat(72));
    eprintln!("  JA4      {}", cap.ja4);
    eprintln!("  JA4_r    {}", cap.ja4_r);
    // Never asserted, for the same reason JA3 is not: it keeps the wire order
    // and so it moves when nothing is wrong. It is what to read when a JA4_r
    // matches and the capture still looks unlike the client it claims to be.
    eprintln!("  JA4_ro   {}  (wire order, diagnostic only)", ch.ja4_ro());
    eprintln!("  JA3      {}  (GREASE filtered)", cap.ja3);
    eprintln!("  JA3 raw  {ja3_raw}  (unfiltered, per the original spec)");
    eprintln!("  JA3 str  {ja3_str}");
    eprintln!(
        "  SNI      {}",
        ch.sni
            .clone()
            .unwrap_or_else(|| "(none, IP literal)".into())
    );
    eprintln!("  ALPN     {:?}", ch.alpn);
    eprintln!("  TLS ver  0x{:04x}", ch.effective_version());
    eprintln!(
        "  ciphers  {} ({} GREASE)",
        ch.ciphers.len(),
        ch.ciphers.iter().filter(|&&c| tlsfp::is_grease(c)).count()
    );
    eprintln!(
        "  ext order {}",
        ch.extensions
            .iter()
            .map(|e| format!("0x{e:04x}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    eprintln!("  browser markers:");
    for (name, present) in ch.browser_markers() {
        eprintln!("    [{}] {name}", if present { "x" } else { " " });
    }
    if let Some(h) = h2 {
        eprintln!("  HTTP/2");
        eprintln!("    Akamai  {}", h.akamai());
        for s in h.settings_pretty() {
            eprintln!("    SETTINGS  {s}");
        }
        if let Some(w) = h.window_update {
            eprintln!("    WINDOW_UPDATE  {w}");
        }
        for (s, e, d, wt) in &h.priorities {
            eprintln!("    PRIORITY  stream={s} excl={e} dep={d} weight={wt}");
        }
    }
    if !cap.headers.is_empty() {
        eprintln!("  header order ({}):", cap.headers.len());
        for (i, h) in cap.headers.iter().enumerate() {
            eprintln!("    {:2}. {h}", i + 1);
        }
    }
    eprintln!("{}", "=".repeat(72));
}

/// Check one field, reporting both sides when they differ.
fn check(label: &str, want: Option<&String>, got: Option<&String>, ok: &mut bool) {
    let Some(want) = want else { return };
    match got {
        Some(g) if g == want => eprintln!("PASS {label}: {g}"),
        Some(g) => {
            eprintln!("FAIL {label}\n  want {want}\n  got  {g}");
            *ok = false;
        }
        None => {
            eprintln!("FAIL {label}: not captured");
            *ok = false;
        }
    }
}

fn assert_all(cap: &Capture, a: &Args) -> bool {
    let mut ok = true;
    check("ja4", a.expect_ja4.as_ref(), Some(&cap.ja4), &mut ok);
    check("ja3", a.expect_ja3.as_ref(), Some(&cap.ja3), &mut ok);
    check(
        "akamai",
        a.expect_akamai.as_ref(),
        cap.akamai.as_ref(),
        &mut ok,
    );

    if let Some(path) = &a.expect_file {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                check(
                    "golden ja4",
                    golden_field(&text, "ja4").as_ref(),
                    Some(&cap.ja4),
                    &mut ok,
                );
                check(
                    "golden akamai",
                    golden_field(&text, "akamai").as_ref(),
                    cap.akamai.as_ref(),
                    &mut ok,
                );
                if let Some(want) = golden_headers(&text)
                    && !want.is_empty()
                {
                    if want == cap.headers {
                        eprintln!("PASS golden headers: {} in order", want.len());
                    } else {
                        eprintln!(
                            "FAIL golden headers\n  want {}\n  got  {}",
                            want.join(", "),
                            cap.headers.join(", ")
                        );
                        ok = false;
                    }
                }
            }
            Err(e) => {
                eprintln!("FAIL golden {path}: {e}");
                ok = false;
            }
        }
    }
    ok
}

/// Drain the client's opening flight.
///
/// It stops as soon as HEADERS has arrived, because a client waiting on a
/// response would otherwise hold the connection until the read deadline.
fn read_h2_flight(stream: &mut impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if h2fp::parse(&buf, false).saw_headers || buf.len() > 1 << 20 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    buf
}

fn handle(tcp: TcpStream, cfg: Option<&Arc<rustls::ServerConfig>>, a: &Args) -> Option<Capture> {
    tcp.set_read_timeout(Some(Duration::from_millis(2500)))
        .ok()?;

    // `peek` leaves the bytes in the kernel buffer, so rustls reads the same
    // ClientHello afterwards. That is what lets one connection yield both the
    // TLS and the HTTP/2 fingerprint.
    let mut peek = vec![0u8; 16384];
    let n = tcp.peek(&mut peek).ok()?;
    peek.truncate(n);

    // Written before the parse, so a hello this parser gets **wrong** is
    // still recorded. A test vector that only exists for the inputs already
    // handled is a test vector that proves nothing.
    if let Some(path) = &a.hello_out {
        let hex: String = peek.iter().map(|b| format!("{b:02x}")).collect();
        match std::path::absolute(path).and_then(|p| std::fs::write(&p, hex + "\n").map(|_| p)) {
            Ok(p) => eprintln!(
                "loopback-tlsprobe: wrote the ClientHello to {}",
                p.display()
            ),
            Err(e) => eprintln!("loopback-tlsprobe: cannot write {path}: {e}"),
        }
    }

    let ch = match tlsfp::parse(&peek) {
        Some(ch) => ch,
        None => {
            eprintln!("loopback-tlsprobe: {n} bytes that are not a ClientHello, ignoring");
            return None;
        }
    };

    let Some(cfg) = cfg else {
        return Some(describe(&ch, None, &[]));
    };

    let conn = match rustls::ServerConnection::new(cfg.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("loopback-tlsprobe: rustls setup failed ({e}), reporting TLS only");
            return Some(describe(&ch, None, &[]));
        }
    };
    let mut tls = rustls::StreamOwned::new(conn, tcp);

    // A browser-shaped ClientHello can legitimately fail against a throwaway
    // certificate. That is not a probe error: fall back to the TLS-only
    // capture rather than losing it.
    if let Err(e) = tls.conn.complete_io(&mut tls.sock) {
        eprintln!("loopback-tlsprobe: handshake did not complete ({e}), reporting TLS only");
        return Some(describe(&ch, None, &[]));
    }

    if tls.conn.alpn_protocol() == Some(b"h2") {
        // Our own empty SETTINGS unblocks a client that waits for one before
        // sending HEADERS.
        let _ = tls.write_all(&[0, 0, 0, 0x4, 0, 0, 0, 0, 0]);
        let _ = tls.flush();
        let buf = read_h2_flight(&mut tls);
        let fp = h2fp::parse(&buf, a.header_values);
        let cap = describe(&ch, Some(&fp), &[]);
        if !a.json {
            print_human(&ch, Some(&fp), &cap);
        }
        // GOAWAY, so the client sees the connection end rather than a reset.
        let _ = tls.write_all(&[0, 0, 8, 0x7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        return Some(cap);
    }

    // HTTP/1.1: the request line and the header order are still worth having.
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
    let cap = describe(&ch, None, &names);
    if !a.json {
        print_human(&ch, None, &cap);
    }
    Some(cap)
}

/// Read one cleartext HTTP/1.1 request and record its header order.
///
/// No TLS, so nothing has to be trusted or disabled. The capture carries no
/// JA4, because there is no handshake to read one from.
fn handle_plain(mut tcp: TcpStream) -> Option<Capture> {
    tcp.set_read_timeout(Some(Duration::from_millis(2500)))
        .ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while let Ok(n) = tcp.read(&mut chunk) {
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 1 << 16 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let names: Vec<String> = text
        .lines()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split(':').next().map(|s| s.trim().to_ascii_lowercase()))
        .filter(|s| !s.is_empty())
        .collect();
    let _ = tcp.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
    Some(Capture {
        ja4: String::new(),
        ja4_r: String::new(),
        ja3: String::new(),
        akamai: None,
        headers: names,
        header_pairs: Vec::new(),
    })
}

fn main() {
    let a = parse_args();
    let cfg = if a.raw || a.plain {
        None
    } else {
        match tls_config(a.ca_out.as_deref(), a.bind) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("loopback-tlsprobe: cannot build a TLS config ({e})");
                std::process::exit(2);
            }
        }
    };

    let listener = match TcpListener::bind((a.bind, a.port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("loopback-tlsprobe: cannot bind {}:{}: {e}", a.bind, a.port);
            std::process::exit(2);
        }
    };
    let bound = match listener.local_addr() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("loopback-tlsprobe: cannot read the bound address: {e}");
            std::process::exit(2);
        }
    };

    // One line on stdout before the first connection, the same contract the
    // other loopback fixtures keep, so a script can read the port it got. The
    // scheme is the one a client should actually use, so a script never has to
    // know which mode it started, and the host is what was bound rather than a
    // literal, so a caller can hand the line to a browser that is not here.
    // An IPv6 literal is bracketed, which is the one part of a URL a `format!`
    // over an address gets wrong.
    let host = match a.bind {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    println!(
        "{}://{host}:{}",
        if a.plain { "http" } else { "https" },
        bound.port()
    );
    let _ = std::io::stdout().flush();
    eprintln!(
        "loopback-tlsprobe: listening on {bound} ({})",
        match (a.plain, cfg.is_some()) {
            (true, _) => "cleartext HTTP/1.1, header order only",
            (false, true) => "TLS terminated, ALPN h2 and http/1.1",
            (false, false) => "raw ClientHello only",
        }
    );
    if !a.bind.is_loopback() {
        eprintln!(
            "loopback-tlsprobe: {} is not a loopback address, so this fixture is \
             reachable by anything on that network for as long as it runs",
            a.bind
        );
    }

    let mut all_ok = true;
    let mut wrote_golden = false;
    for stream in listener.incoming().flatten() {
        let captured = match a.plain {
            true => handle_plain(stream),
            false => handle(stream, cfg.as_ref(), &a),
        };
        let Some(cap) = captured else {
            continue;
        };
        if a.plain && !a.json {
            eprintln!("  header order ({}):", cap.headers.len());
            for (i, h) in cap.headers.iter().enumerate() {
                eprintln!("    {:2}. {h}", i + 1);
            }
        }
        if a.json {
            println!(
                "{}",
                match a.header_values {
                    true => cap.to_json_with_values(),
                    false => cap.to_json(),
                }
            );
            let _ = std::io::stdout().flush();
        }
        if let Some(path) = &a.write_golden
            && !wrote_golden
        {
            match std::fs::write(path, format!("{}\n", cap.to_json())) {
                Ok(()) => {
                    wrote_golden = true;
                    eprintln!("loopback-tlsprobe: wrote {path}");
                }
                Err(e) => {
                    eprintln!("loopback-tlsprobe: cannot write {path}: {e}");
                    all_ok = false;
                }
            }
        }
        all_ok &= assert_all(&cap, &a);
        if a.once {
            break;
        }
        // Wait on the condition rather than on a connection count. A browser
        // opens sockets it then abandons: measured on 2026-08-30, a Chrome 152
        // driven at this fixture made 13 connections and the **first** carried
        // no HTTP/2 at all. `--once` captured that one and stopped, so the run
        // reported a TLS-only capture and no Akamai fingerprint, which reads
        // as a failed handshake rather than as the wrong socket.
        if a.until_h2 && cap.akamai.is_some() {
            break;
        }
    }
    if !all_ok {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_golden_field_is_read_out_of_a_flat_object() {
        let text =
            r#"{"ja4":"t13d1516h2_abc_def","ja4_r":"x","ja3":"y","akamai":null,"headers":[]}"#;
        assert_eq!(
            golden_field(text, "ja4").as_deref(),
            Some("t13d1516h2_abc_def")
        );
        assert_eq!(golden_field(text, "ja3").as_deref(), Some("y"));
        // A null field is absent rather than the string "null", so a manifest
        // written from a --raw capture does not assert an Akamai fingerprint
        // that a raw capture cannot have.
        assert_eq!(golden_field(text, "akamai"), None);
        assert_eq!(golden_field(text, "nothing"), None);
    }

    #[test]
    fn golden_headers_keep_their_order() {
        let text = r#"{"headers":[":method",":authority","user-agent","accept"]}"#;
        assert_eq!(
            golden_headers(text),
            Some(vec![
                ":method".to_string(),
                ":authority".to_string(),
                "user-agent".to_string(),
                "accept".to_string(),
            ])
        );
    }

    #[test]
    fn an_absent_headers_array_is_none_and_an_empty_one_is_empty() {
        assert_eq!(golden_headers(r#"{"ja4":"x"}"#), None);
        assert_eq!(golden_headers(r#"{"headers":[]}"#), Some(Vec::new()));
    }

    #[test]
    fn a_capture_round_trips_through_its_own_json() {
        let cap = Capture {
            ja4: "t13d1516h2_aaaa_bbbb".into(),
            ja4_r: "t13d1516h2_1301,1302_002b".into(),
            ja3: "cd08e31494f9531f560d64c695473da9".into(),
            akamai: Some("1:65536;2:0|15663105|0|m,a,s,p".into()),
            headers: vec![":method".into(), ":authority".into()],
            header_pairs: Vec::new(),
        };
        let text = cap.to_json();
        assert_eq!(
            golden_field(&text, "ja4").as_deref(),
            Some(cap.ja4.as_str())
        );
        assert_eq!(
            golden_field(&text, "akamai").as_deref(),
            cap.akamai.as_deref()
        );
        assert_eq!(golden_headers(&text), Some(cap.headers.clone()));
    }

    #[test]
    fn a_capture_with_no_http2_writes_a_null_akamai() {
        let cap = Capture {
            ja4: "t13i1515h2_a_b".into(),
            ja4_r: "t13i1515h2_c_d".into(),
            ja3: "e".into(),
            akamai: None,
            headers: Vec::new(),
            header_pairs: Vec::new(),
        };
        let text = cap.to_json();
        assert!(text.contains("\"akamai\":null"), "{text}");
        assert_eq!(golden_field(&text, "akamai"), None);
    }

    #[test]
    fn header_values_are_absent_from_the_shipping_shape() {
        let cap = Capture {
            ja4: "a".into(),
            ja4_r: "b".into(),
            ja3: "c".into(),
            akamai: None,
            headers: vec!["user-agent".into()],
            header_pairs: vec![("user-agent".into(), "Mozilla/5.0".into())],
        };
        // The default shape carries names and never values, whatever the
        // capture happens to hold. Asking is what produces them.
        let plain = cap.to_json();
        assert!(!plain.contains("header_pairs"), "{plain}");
        assert!(!plain.contains("Mozilla"), "{plain}");

        let asked = cap.to_json_with_values();
        assert!(asked.contains("\"header_pairs\""), "{asked}");
        assert!(asked.contains("Mozilla/5.0"), "{asked}");
        assert_eq!(golden_headers(&asked), Some(cap.headers.clone()));
    }

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_command_line_with_no_bind_listens_on_loopback() {
        // The default is the whole safety argument for the switch: every other
        // script in this tree passes nothing and must keep the address it has.
        assert_eq!(parse_argv(&argv(&[])).bind, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(
            parse_argv(&argv(&["--port", "0", "--raw"])).bind,
            IpAddr::from([127, 0, 0, 1])
        );
    }

    #[test]
    fn until_h2_is_off_unless_asked_for_and_is_not_once() {
        // Two switches that both end the loop and end it on different facts.
        // `--once` stops at the first connection; `--until-h2` stops at the
        // first one that reached HTTP/2, which a browser's preconnect is not.
        let plain = parse_argv(&argv(&[]));
        assert!(!plain.until_h2 && !plain.once);
        let a = parse_argv(&argv(&["--until-h2"]));
        assert!(a.until_h2 && !a.once);
        let b = parse_argv(&argv(&["--once"]));
        assert!(b.once && !b.until_h2);
    }

    #[test]
    fn bind_takes_a_literal_under_either_spelling() {
        for flag in ["--bind", "-b"] {
            let a = parse_argv(&argv(&[flag, "172.23.96.1", "--once"]));
            assert_eq!(a.bind, IpAddr::from([172, 23, 96, 1]));
            assert!(a.once, "{flag} swallowed the switch after it");
        }
        assert_eq!(
            parse_argv(&argv(&["--bind", "::1"])).bind,
            "::1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn a_bound_address_that_is_already_a_san_is_not_added_twice() {
        // What `tls_config` does with the bind address, asserted on the list
        // rather than on the certificate, because the certificate needs a key
        // and this is the part that could be wrong.
        for (bind, expected) in [
            ("127.0.0.1", 3),
            ("::1", 3),
            ("172.23.96.1", 4),
            ("10.0.0.5", 4),
        ] {
            let mut names = vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
            ];
            let bound = bind.parse::<IpAddr>().unwrap().to_string();
            if !names.contains(&bound) {
                names.push(bound);
            }
            assert_eq!(names.len(), expected, "{bind} produced {names:?}");
        }
    }

    #[test]
    fn an_ipv6_bind_is_bracketed_in_the_announced_url_and_ipv4_is_not() {
        for (bind, expected) in [
            ("127.0.0.1", "127.0.0.1"),
            ("172.23.96.1", "172.23.96.1"),
            ("::1", "[::1]"),
            ("fd00::1", "[fd00::1]"),
        ] {
            let addr: IpAddr = bind.parse().unwrap();
            let host = match addr {
                IpAddr::V4(v4) => v4.to_string(),
                IpAddr::V6(v6) => format!("[{v6}]"),
            };
            assert_eq!(host, expected);
        }
    }

    #[test]
    fn a_quote_in_a_header_name_does_not_break_the_json() {
        let cap = Capture {
            ja4: "a".into(),
            ja4_r: "b".into(),
            ja3: "c".into(),
            akamai: None,
            headers: vec!["x\"y".into()],
            header_pairs: Vec::new(),
        };
        let text = cap.to_json();
        assert!(text.contains("x\\\"y"), "the quote is not escaped: {text}");
        assert_eq!(golden_headers(&text), Some(vec!["x\"y".to_string()]));
    }
}
