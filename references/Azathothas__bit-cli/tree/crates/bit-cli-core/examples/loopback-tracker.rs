//! A minimal BEP 3 HTTP tracker bound to loopback.
//!
//! It exists so two different BitTorrent clients running on this machine can
//! find each other without the DHT, without local service discovery, and
//! without touching the network. `scripts/interop-roundtrip.ps1` uses it to
//! prove `bit-cli create` and `bit-cli seed` interoperate with another client
//! (`TODO/create-seed.md`, T-084).
//!
//! It is a test fixture, not a product. It keeps peers in memory, never
//! expires them, answers `announce` and `scrape`, and speaks the compact peer
//! format from BEP 23 plus the dictionary format when `compact=0` is asked
//! for. That is the whole surface a client needs to join a swarm.
//!
//! ```text
//! cargo run -p bit-cli-core --example loopback-tracker -- --port 6969
//! ```
//!
//! Port `0` asks the OS for a free one. The chosen announce URLs are printed
//! to stdout before the first request is served, so a script can read one and
//! pass it to `--announce`. The IPv4 HTTP URL is always the **first** line,
//! because `scripts/soak.ps1` and `scripts/interop-roundtrip.ps1` read only
//! that one; the IPv6 HTTP URL and the UDP URL follow it. Every announce is
//! logged to stderr with an ISO 8601 UTC millisecond timestamp.
//!
//! `--announce-log <PATH>` additionally appends one JSON object per announce,
//! carrying the **raw query string** and the request headers as received. That
//! is what `scripts/check-announce.ps1` reads to decide whether the numbers a
//! tracker sees are the numbers `bit-cli` reports (`TODO/trackers.md`, T-235).
//! The raw query is kept rather than a re-serialisation of the parsed map,
//! because parameter order is part of what a real tracker fingerprints and the
//! `BTreeMap` the parser produces sorts it away.
//!
//! # The same three numbers over the other two announce paths
//!
//! `TODO/trackers.md` T-237 is why the next three exist. An announce that is
//! redirected and an announce that is rejected at HTTP 200 are both ordinary,
//! and a check that reads only the status calls the second one a success.
//!
//! - **`--redirect-announce <N>`** answers the first `N` requests to
//!   `/announce` with `302 Found` and a `Location` of `/announce-r` carrying
//!   the same query. `/announce-r` is served exactly like `/announce`, so the
//!   log holds the request that was redirected and the one that followed it,
//!   and the three numbers can be compared across the hop.
//! - **`--fail-announce <REASON>`** answers every announce with `REASON` in a
//!   `failure reason` key: HTTP 200 over TCP, BEP 15 action 3 over UDP.
//!
//! **UDP is always on** and speaks BEP 15: a connect exchange, then an
//! announce. A connection id this tracker did not issue is refused with
//! action 3, so a client that skips the connect is caught rather than served.
//! It asks for the HTTP port and does not always get it, so the `udp://` line
//! on stdout is the one to read rather than the HTTP port with the scheme
//! changed. See the bind below for why.

use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bit_cli_core::time::now_iso;
use bit_cli_core::torrent::bencode::{Value, encode};

/// One peer in one swarm, as last announced.
#[derive(Debug, Clone)]
struct Peer {
    /// The address other peers are told to connect to. The port comes from
    /// the announce, not from the TCP source port, because a seeding client
    /// listens on a different port than it announces from.
    addr: SocketAddr,
    /// Peer id, kept so the dictionary response can carry it.
    id: Vec<u8>,
    /// Bytes still wanted. Zero means a seeder.
    left: u64,
}

/// One peer record's key: the peer id, and which family it announced over.
///
/// **Not the peer id alone.** A dual-stack client announces once per family,
/// because a tracker records the source address of the connection it was
/// announced over. Keyed by peer id alone the second announce overwrites the
/// first, the client is left reachable on one family, and which one depends on
/// the order the announces landed in. That is the exact failure
/// `TODO/peers.md` T-022 is about, and BEP 7 is the reason to key per family
/// instead: a peer has one address in each and both are worth keeping.
type PeerKey = (Vec<u8>, bool);

/// Where `--announce-log` writes, or `None` when it was not asked for.
type AnnounceLog = Arc<Mutex<Option<String>>>;

/// Every swarm the tracker has seen, keyed by info hash then by peer record.
type Swarms = Arc<Mutex<HashMap<Vec<u8>, HashMap<PeerKey, Peer>>>>;

/// What every request handler needs and none of it changes after startup.
///
/// One struct rather than six parameters, because the UDP loop and the two
/// TCP accept loops all want the same set and a seventh would otherwise have
/// to be threaded through four call sites.
#[derive(Clone)]
struct Fixture {
    swarms: Swarms,
    log: AnnounceLog,
    interval: i64,
    /// How many more announces to answer with a redirect, from
    /// `--redirect-announce`. Shared across both families and both accept
    /// loops, so `N` is a count of announces and not a count per listener.
    redirects_left: Arc<AtomicU32>,
    /// The `failure reason` every announce gets, from `--fail-announce`.
    fail: Option<String>,
    /// Connection ids this tracker has handed out over UDP. BEP 15 exists to
    /// make a spoofed source address cost a round trip, and a fixture that
    /// accepts any id at all cannot show that the client paid it.
    connections: Arc<Mutex<HashSet<u64>>>,
    /// The next connection id, so two clients never share one.
    next_connection: Arc<AtomicU64>,
}

fn main() {
    let mut port: u16 = 0;
    let mut interval: i64 = 5;
    let mut announce_log: Option<String> = None;
    let mut redirect_announce: u32 = 0;
    let mut fail_announce: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => port = next_value(&mut args, "--port").parse().expect("--port"),
            "--interval" => {
                interval = next_value(&mut args, "--interval")
                    .parse()
                    .expect("--interval")
            }
            "--announce-log" => announce_log = Some(next_value(&mut args, "--announce-log")),
            "--redirect-announce" => {
                redirect_announce = next_value(&mut args, "--redirect-announce")
                    .parse()
                    .expect("--redirect-announce")
            }
            "--fail-announce" => fail_announce = Some(next_value(&mut args, "--fail-announce")),
            "--help" | "-h" => {
                println!(
                    "usage: loopback-tracker [--port PORT] [--interval SECONDS] [--announce-log PATH]"
                );
                println!(
                    "                        [--redirect-announce N] [--fail-announce REASON]"
                );
                return;
            }
            other => {
                eprintln!("loopback-tracker: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("bind loopback");
    let bound = listener.local_addr().expect("local addr");
    // The same port on IPv6 loopback, so a client can announce to this
    // tracker over either family and the tracker sees a different source
    // address for each. Two listeners rather than one dual-stack socket,
    // because the standard library leaves IPV6_V6ONLY on and turning it off
    // portably is what `TODO/peers.md` T-023 is about.
    //
    // A host with no IPv6 at all keeps the IPv4 listener and says so, because
    // a fixture that refuses to start is worse than one that covers less.
    let listener6 = TcpListener::bind((Ipv6Addr::LOCALHOST, bound.port())).ok();

    // The same port over UDP, which is what BEP 15 assumes: a tracker
    // reachable as `http://host:port/announce` is reachable as
    // `udp://host:port/announce`. IPv4 only, because a BEP 15 announce reply
    // packs peers six bytes each and there is no room in that shape for an
    // IPv6 address. Bound after the TCP listeners so the port is already
    // known when the port asked for was 0.
    //
    // The matching UDP port is asked for and is not required, because on
    // Windows a freely chosen TCP port is not always available over UDP.
    // `netsh int ipv4 show excludedportrange udp` lists twelve reserved bands
    // on this machine, Hyper-V and WinNAT hold them, and a bind inside one
    // fails with `os error 10013` rather than with "address in use". Both
    // failures seen on 2026-08-24 were inside a listed band: 53502 and 53521
    // in 53495-53594, and 65389 and 65390 in 65356-65455. So the fallback is
    // an OS-chosen port, and the URL printed below is what a caller reads
    // either way.
    let udp = match UdpSocket::bind((Ipv4Addr::LOCALHOST, bound.port())) {
        Ok(socket) => Some(socket),
        Err(err) => {
            eprintln!(
                "{} udp/{} is taken ({err}), asking for any free port",
                now_iso(),
                bound.port()
            );
            UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).ok()
        }
    };
    let udp_port = udp
        .as_ref()
        .and_then(|socket| socket.local_addr().ok())
        .map(|addr| addr.port());

    // The script reads the first line to learn the port, so it goes out before
    // anything else and is flushed immediately. Anything added here goes
    // after it: `scripts/soak.ps1` and `scripts/interop-roundtrip.ps1` read
    // line one and nothing else.
    println!("http://127.0.0.1:{}/announce", bound.port());
    if listener6.is_some() {
        println!("http://[::1]:{}/announce", bound.port());
    }
    if let Some(port) = udp_port {
        println!("udp://127.0.0.1:{port}/announce");
    }
    std::io::stdout().flush().ok();
    eprintln!("{} tracker listening on {bound}", now_iso());
    match &listener6 {
        Some(socket) => eprintln!(
            "{} tracker listening on {}",
            now_iso(),
            socket.local_addr().expect("local addr")
        ),
        None => eprintln!("{} no IPv6 loopback: announcing over IPv4 only", now_iso()),
    }
    match udp_port {
        Some(port) => eprintln!("{} tracker listening on udp 127.0.0.1:{port}", now_iso()),
        None => eprintln!("{} no UDP socket at all: HTTP only", now_iso()),
    }
    if redirect_announce > 0 {
        eprintln!(
            "{} answering the first {redirect_announce} announce(s) with a 302 to /announce-r",
            now_iso()
        );
    }
    if let Some(reason) = &fail_announce {
        eprintln!("{} refusing every announce with: {reason}", now_iso());
    }

    let fixture = Fixture {
        swarms: Swarms::default(),
        // The path rather than an open handle, behind a mutex: a run that
        // never announces leaves no file, and two accept threads cannot
        // interleave a line into one.
        log: Arc::new(Mutex::new(announce_log)),
        interval,
        redirects_left: Arc::new(AtomicU32::new(redirect_announce)),
        fail: fail_announce,
        connections: Arc::new(Mutex::new(HashSet::new())),
        // Any non-zero start does. Zero is worth avoiding because a client
        // that never read the connect reply would send zero and be served.
        next_connection: Arc::new(AtomicU64::new(0x0417_2710_1980)),
    };

    if let Some(socket) = udp {
        let fixture = fixture.clone();
        std::thread::spawn(move || serve_udp(&socket, &fixture));
    }
    for listener in [Some(listener), listener6].into_iter().flatten() {
        let fixture = fixture.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let fixture = fixture.clone();
                std::thread::spawn(move || {
                    if let Err(err) = serve(stream, &fixture) {
                        eprintln!("{} connection failed: {err}", now_iso());
                    }
                });
            }
        });
    }
    // Both accept loops are on their own threads, so the main thread parks
    // rather than returning and taking the process with it. In a loop, because
    // `park` is allowed to return without anything having unparked it, and a
    // spurious wake here would end the run and every script driving it.
    loop {
        std::thread::park();
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> String {
    match args.next() {
        Some(value) => value,
        None => {
            eprintln!("loopback-tracker: {flag} needs a value");
            std::process::exit(2);
        }
    }
}

/// Answer one HTTP/1.1 request and close. No keep-alive: a tracker announce is
/// infrequent enough that the extra connection costs nothing, and closing
/// keeps the parser to the few lines below.
fn serve(mut stream: TcpStream, fixture: &Fixture) -> std::io::Result<()> {
    let peer_ip = stream.peer_addr()?.ip();
    let local = stream.local_addr()?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // The headers are kept now rather than drained. `User-Agent` is part of
    // what a tracker sees of a client, so a check that asks whether the
    // advertised identity reached the wire needs them.
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    let target = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    };
    let params = parse_query(query);

    let body = match path {
        // `/announce-r` is where `--redirect-announce` sends a client, and it
        // is served identically. A redirect target that behaved differently
        // would measure the target rather than the hop.
        "/announce" | "/announce-r" => {
            record_announce(fixture, "http", path, query, &headers, &params, peer_ip);
            // The redirect is issued after the record is written, so the log
            // holds the request that was redirected as well as the one that
            // followed it. Comparing the two across the hop is the whole
            // point of the case in `scripts/check-announce.ps1`.
            if path == "/announce" && take_redirect(&fixture.redirects_left) {
                let target = format!("http://{local}/announce-r?{query}");
                eprintln!("{} redirecting an announce to {target}", now_iso());
                write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )?;
                return stream.flush();
            }
            match &fixture.fail {
                // BEP 3 puts a rejection in the body at HTTP 200, which is
                // the case a check reading only the status calls a success.
                Some(reason) => failure(reason),
                None => announce(&params, peer_ip, &fixture.swarms, fixture.interval),
            }
        }
        "/scrape" => scrape(&params, &fixture.swarms),
        _ => failure("unknown path"),
    };

    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}

/// Take one redirect from the budget, or report that there is none left.
///
/// A compare and exchange loop rather than a load and a store, because both
/// accept loops and every connection thread share the counter and
/// `--redirect-announce 1` has to mean one announce in total rather than one
/// per thread that looks.
///
/// Written out rather than as `fetch_update`, which does exactly this and is
/// deprecated on beta in favour of `try_update`. CI's beta clippy job caught
/// it the day it was written; `try_update` is not on stable yet, so neither
/// name is portable and the loop is.
fn take_redirect(left: &AtomicU32) -> bool {
    let mut current = left.load(Ordering::SeqCst);
    loop {
        if current == 0 {
            return false;
        }
        match left.compare_exchange_weak(current, current - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
}

/// Append one JSON object describing the announce exactly as it arrived.
///
/// Written by hand rather than through a serialiser, because this is an
/// example and the crate's `Value` encoder is bencode. Every string that can
/// carry a byte outside the JSON grammar goes through `json_string`.
///
/// `query_order` is derived from the **raw** query rather than from `params`,
/// which is a `BTreeMap` and has already sorted it. Order is the thing
/// `scripts/check-announce.ps1` cannot recover any other way.
///
/// `protocol` and `path` are what separate the three announce paths in one
/// log: `http` at `/announce`, `http` at `/announce-r` for the request that
/// followed a redirect, and `udp`, which has neither a query nor a header and
/// records both as empty. Every other field is spelled the same way for all
/// three, so a check written against one reads all of them.
fn record_announce(
    fixture: &Fixture,
    protocol: &str,
    path: &str,
    query: &str,
    headers: &[(String, String)],
    params: &BTreeMap<String, Vec<u8>>,
    peer_ip: std::net::IpAddr,
) {
    let guard = fixture.log.lock().expect("announce log lock");
    let Some(log_path) = guard.as_ref() else {
        return;
    };

    let order: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| pair.split('=').next().unwrap_or(""))
        .collect();

    let mut out = String::new();
    out.push('{');
    out.push_str(&format!("\"at\":{}", json_string(&now_iso())));
    out.push_str(&format!(",\"protocol\":{}", json_string(protocol)));
    out.push_str(&format!(",\"path\":{}", json_string(path)));
    out.push_str(&format!(",\"from\":{}", json_string(&peer_ip.to_string())));
    out.push_str(&format!(
        ",\"family\":\"ip{}\"",
        if peer_ip.is_ipv4() { "v4" } else { "v6" }
    ));
    out.push_str(&format!(",\"raw_query\":{}", json_string(query)));
    out.push_str(",\"query_order\":[");
    for (i, name) in order.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(name));
    }
    out.push(']');
    out.push_str(",\"headers\":[");
    for (i, (name, value)) in headers.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":{},\"value\":{}}}",
            json_string(name),
            json_string(value)
        ));
    }
    out.push(']');

    // The fields a fidelity check reads. `info_hash` and `peer_id` are twenty
    // arbitrary bytes, so they go out as hex rather than as text.
    if let Some(v) = params.get("info_hash") {
        out.push_str(&format!(",\"info_hash\":{}", json_string(&hex(v))));
    }
    if let Some(v) = params.get("peer_id") {
        out.push_str(&format!(",\"peer_id_hex\":{}", json_string(&hex(v))));
        out.push_str(&format!(",\"peer_id\":{}", json_string(&printable(v))));
    }
    for name in [
        "port",
        "uploaded",
        "downloaded",
        "left",
        "event",
        "numwant",
        "key",
        "compact",
        "corrupt",
        "no_peer_id",
        "supportcrypto",
        "redundant",
        "ipv6",
    ] {
        if let Some(value) = text(params, name) {
            out.push_str(&format!(",{}:{}", json_string(name), json_string(&value)));
        }
    }
    out.push_str("}\n");

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = file.write_all(out.as_bytes());
    }
}

/// A JSON string literal, with the five escapes JSON requires and a `\u` form
/// for every other control byte.
///
/// A raw control byte in a JSON document is invalid however forgiving the
/// reader is, and a tracker sees whatever a client chose to send.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Record the announcing peer and answer with the rest of the swarm.
fn announce(
    params: &BTreeMap<String, Vec<u8>>,
    peer_ip: std::net::IpAddr,
    swarms: &Swarms,
    interval: i64,
) -> Vec<u8> {
    let Some(info_hash) = params.get("info_hash") else {
        return failure("no info_hash");
    };
    let Some(peer_id) = params.get("peer_id") else {
        return failure("no peer_id");
    };
    let port: u16 = match text(params, "port").and_then(|p| p.parse().ok()) {
        Some(port) => port,
        None => return failure("no port"),
    };
    let left: u64 = text(params, "left")
        .and_then(|l| l.parse().ok())
        .unwrap_or(u64::MAX);
    let event = text(params, "event").unwrap_or_default();
    let compact = text(params, "compact").as_deref() != Some("0");

    let (others, complete, incomplete) = record_peer(
        swarms,
        info_hash,
        peer_id,
        SocketAddr::new(peer_ip, port),
        left,
        &event,
    );

    // The source family is logged because it is the thing an announce over
    // one family and an announce over the other differ in, and it is what a
    // script checks to see that both arrived.
    eprintln!(
        "{} announce info_hash={} peer_id={} from={peer_ip} family=ip{} port={port} left={left} event={} -> {} peer(s)",
        now_iso(),
        hex(info_hash),
        printable(peer_id),
        if peer_ip.is_ipv4() { "v4" } else { "v6" },
        if event.is_empty() { "-" } else { &event },
        others.len(),
    );

    // BEP 23 packs IPv4 peers six bytes each into `peers`; BEP 7 packs IPv6
    // peers eighteen bytes each into `peers6`. Both are sent, because a
    // client announcing over one family still wants to hear about the other:
    // which family it reached the tracker over decides what the tracker
    // records about it, not what it is told back.
    let mut packed6 = Vec::with_capacity(others.len() * 18);
    for peer in &others {
        if let IpAddr::V6(ip) = peer.addr.ip() {
            packed6.extend_from_slice(&ip.octets());
            packed6.extend_from_slice(&peer.addr.port().to_be_bytes());
        }
    }
    let peers = if compact {
        let mut packed = Vec::with_capacity(others.len() * 6);
        for peer in &others {
            if let IpAddr::V4(ip) = peer.addr.ip() {
                packed.extend_from_slice(&ip.octets());
                packed.extend_from_slice(&peer.addr.port().to_be_bytes());
            }
        }
        Value::Bytes(packed)
    } else {
        Value::List(
            others
                .iter()
                .map(|peer| {
                    Value::Dict(BTreeMap::from([
                        (b"peer id".to_vec(), Value::Bytes(peer.id.clone())),
                        (
                            b"ip".to_vec(),
                            Value::Bytes(peer.addr.ip().to_string().into_bytes()),
                        ),
                        (b"port".to_vec(), Value::Int(peer.addr.port() as i64)),
                    ]))
                })
                .collect(),
        )
    };

    let mut response = BTreeMap::from([
        (b"interval".to_vec(), Value::Int(interval)),
        // Same as `interval`. A one-second announce storm on loopback buries
        // the log this fixture exists to produce.
        (b"min interval".to_vec(), Value::Int(interval)),
        (b"complete".to_vec(), Value::Int(complete)),
        (b"incomplete".to_vec(), Value::Int(incomplete)),
        (b"peers".to_vec(), peers),
    ]);
    // Only when there is one. An empty `peers6` is a key every client has to
    // parse to learn nothing.
    if compact && !packed6.is_empty() {
        response.insert(b"peers6".to_vec(), Value::Bytes(packed6));
    }
    encode(&Value::Dict(response))
}

/// Put one peer in one swarm and report the rest of it.
///
/// Shared by the HTTP and UDP announce paths, which differ in how they pack
/// the answer and in nothing else. Two copies of this would be two chances
/// for a UDP announce and an HTTP announce to disagree about the same swarm,
/// which is the one thing a fixture serving both must not do.
///
/// The peers come back owned rather than borrowed because the lock ends here.
fn record_peer(
    swarms: &Swarms,
    info_hash: &[u8],
    peer_id: &[u8],
    addr: SocketAddr,
    left: u64,
    event: &str,
) -> (Vec<Peer>, i64, i64) {
    let mut swarms = swarms.lock().expect("swarm lock");
    let swarm = swarms.entry(info_hash.to_vec()).or_default();
    let key: PeerKey = (peer_id.to_vec(), addr.is_ipv4());
    if event == "stopped" {
        swarm.remove(&key);
    } else {
        swarm.insert(
            key,
            Peer {
                addr,
                id: peer_id.to_vec(),
                left,
            },
        );
    }

    // A peer never gets itself back, which is what makes a two-client swarm on
    // one machine behave like a real one. By peer id and not by the record's
    // key, so a client announcing over both families is not handed its own
    // other address.
    let others: Vec<Peer> = swarm
        .values()
        .filter(|p| p.id != peer_id)
        .cloned()
        .collect();
    let (complete, incomplete) = counts(swarm);
    (others, complete, incomplete)
}

/// Seeders and leechers, counted by distinct peer rather than by record.
///
/// A dual-stack peer holds one record per family, and `complete` and
/// `incomplete` are counts of clients: counting records would report one peer
/// announcing over both families as two, and a swarm of one as a swarm of two.
fn counts(swarm: &HashMap<PeerKey, Peer>) -> (i64, i64) {
    let mut seeds: BTreeMap<&[u8], bool> = BTreeMap::new();
    for peer in swarm.values() {
        // A peer that is a seed on any of its records is a seed. It is the
        // same client either way.
        let entry = seeds.entry(&peer.id).or_insert(false);
        *entry |= peer.left == 0;
    }
    let complete = seeds.values().filter(|seed| **seed).count() as i64;
    (complete, seeds.len() as i64 - complete)
}

/// BEP 48 scrape for one or more info hashes.
fn scrape(params: &BTreeMap<String, Vec<u8>>, swarms: &Swarms) -> Vec<u8> {
    let swarms = swarms.lock().expect("swarm lock");
    let mut files = BTreeMap::new();
    let wanted: Vec<Vec<u8>> = match params.get("info_hash") {
        Some(hash) => vec![hash.clone()],
        None => swarms.keys().cloned().collect(),
    };
    for hash in wanted {
        let swarm = swarms.get(&hash).cloned().unwrap_or_default();
        let (complete, incomplete) = counts(&swarm);
        files.insert(
            hash,
            Value::Dict(BTreeMap::from([
                (b"complete".to_vec(), Value::Int(complete)),
                (b"downloaded".to_vec(), Value::Int(0)),
                (b"incomplete".to_vec(), Value::Int(incomplete)),
            ])),
        );
    }
    encode(&Value::Dict(BTreeMap::from([(
        b"files".to_vec(),
        Value::Dict(files),
    )])))
}

fn failure(reason: &str) -> Vec<u8> {
    eprintln!("{} refused: {reason}", now_iso());
    encode(&Value::Dict(BTreeMap::from([(
        b"failure reason".to_vec(),
        Value::Bytes(reason.as_bytes().to_vec()),
    )])))
}

/// The BEP 15 magic every connect request opens with.
const UDP_PROTOCOL_ID: u64 = 0x0417_2710_1980;
const UDP_CONNECT: u32 = 0;
const UDP_ANNOUNCE: u32 = 1;
const UDP_SCRAPE: u32 = 2;
const UDP_ERROR: u32 = 3;

/// BEP 15 on one datagram socket: connect, then announce or scrape.
///
/// One thread and one socket rather than a thread per exchange. A datagram
/// server has no connection to hand off, and the work per request is a map
/// lookup, so a second thread would only add a lock to contend on.
fn serve_udp(socket: &UdpSocket, fixture: &Fixture) {
    // An announce reply is 20 bytes plus six per peer, and a request is 98.
    // 4 KiB is more than either will ever be and costs one page.
    let mut buf = [0u8; 4096];
    loop {
        let Ok((n, from)) = socket.recv_from(&mut buf) else {
            continue;
        };
        // Every request in BEP 15 carries its action at byte 8 and its
        // transaction id at byte 12, whether the first eight bytes are the
        // protocol magic or a connection id.
        if n < 16 {
            eprintln!("{} udp: {n} byte request, which is not BEP 15", now_iso());
            continue;
        }
        let request = &buf[..n];
        let action = be32(&request[8..12]);
        let transaction = be32(&request[12..16]);
        let reply = match action {
            UDP_CONNECT => udp_connect(request, transaction, fixture),
            UDP_ANNOUNCE => udp_announce(request, transaction, from, fixture),
            UDP_SCRAPE => udp_scrape(request, transaction, fixture),
            other => udp_error(transaction, &format!("action {other} is not one of mine")),
        };
        let _ = socket.send_to(&reply, from);
    }
}

/// Hand out a connection id, so an announce can be asked for one back.
fn udp_connect(request: &[u8], transaction: u32, fixture: &Fixture) -> Vec<u8> {
    let magic = u64::from_be_bytes(request[0..8].try_into().unwrap_or([0; 8]));
    if magic != UDP_PROTOCOL_ID {
        return udp_error(transaction, "connect without the protocol id");
    }
    let id = fixture.next_connection.fetch_add(1, Ordering::SeqCst);
    fixture
        .connections
        .lock()
        .expect("connection lock")
        .insert(id);
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&UDP_CONNECT.to_be_bytes());
    out.extend_from_slice(&transaction.to_be_bytes());
    out.extend_from_slice(&id.to_be_bytes());
    out
}

/// A BEP 15 announce: the same swarm the HTTP path uses, packed six bytes to
/// a peer.
fn udp_announce(request: &[u8], transaction: u32, from: SocketAddr, fixture: &Fixture) -> Vec<u8> {
    if let Some(refusal) = udp_connection_refusal(request, transaction, fixture) {
        return refusal;
    }
    // 8 + 4 + 4 + 20 + 20 + 8 + 8 + 8 + 4 + 4 + 4 + 4 + 2.
    if request.len() < 98 {
        return udp_error(
            transaction,
            &format!("announce of {} bytes, and BEP 15 says 98", request.len()),
        );
    }
    let info_hash = request[16..36].to_vec();
    let peer_id = request[36..56].to_vec();
    let downloaded = be64(&request[56..64]);
    let left = be64(&request[64..72]);
    let uploaded = be64(&request[72..80]);
    // BEP 15's numbering, which is not the order the strings are usually
    // written in: 0 none, 1 completed, 2 started, 3 stopped.
    let event = match be32(&request[80..84]) {
        1 => "completed",
        2 => "started",
        3 => "stopped",
        _ => "",
    };
    let key = be32(&request[88..92]);
    let numwant = be32(&request[92..96]);
    let port = u16::from_be_bytes(request[96..98].try_into().unwrap_or([0; 2]));

    // The same field names the HTTP path records, so one check reads both.
    // The query and the headers are empty because a datagram has neither, and
    // an invented query would be a claim about a request that never existed.
    let mut params: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    params.insert("info_hash".to_string(), info_hash.clone());
    params.insert("peer_id".to_string(), peer_id.clone());
    params.insert("port".to_string(), port.to_string().into_bytes());
    params.insert("uploaded".to_string(), uploaded.to_string().into_bytes());
    params.insert(
        "downloaded".to_string(),
        downloaded.to_string().into_bytes(),
    );
    params.insert("left".to_string(), left.to_string().into_bytes());
    params.insert("key".to_string(), key.to_string().into_bytes());
    params.insert("numwant".to_string(), numwant.to_string().into_bytes());
    if !event.is_empty() {
        params.insert("event".to_string(), event.as_bytes().to_vec());
    }
    record_announce(fixture, "udp", "/announce", "", &[], &params, from.ip());

    if let Some(reason) = &fixture.fail {
        return udp_error(transaction, reason);
    }

    let (others, complete, incomplete) = record_peer(
        &fixture.swarms,
        &info_hash,
        &peer_id,
        SocketAddr::new(from.ip(), port),
        left,
        event,
    );
    eprintln!(
        "{} udp announce info_hash={} peer_id={} from={} port={port} left={left} event={} -> {} peer(s)",
        now_iso(),
        hex(&info_hash),
        printable(&peer_id),
        from.ip(),
        if event.is_empty() { "-" } else { event },
        others.len(),
    );

    let mut out = Vec::with_capacity(20 + others.len() * 6);
    out.extend_from_slice(&UDP_ANNOUNCE.to_be_bytes());
    out.extend_from_slice(&transaction.to_be_bytes());
    out.extend_from_slice(&(fixture.interval as u32).to_be_bytes());
    out.extend_from_slice(&(incomplete as u32).to_be_bytes());
    out.extend_from_slice(&(complete as u32).to_be_bytes());
    // IPv4 only. A BEP 15 announce reply has six bytes per peer and no room
    // for anything else, so an IPv6 peer in this swarm is not offered here.
    for peer in &others {
        if let IpAddr::V4(ip) = peer.addr.ip() {
            out.extend_from_slice(&ip.octets());
            out.extend_from_slice(&peer.addr.port().to_be_bytes());
        }
    }
    out
}

/// A BEP 15 scrape for one info hash.
fn udp_scrape(request: &[u8], transaction: u32, fixture: &Fixture) -> Vec<u8> {
    if let Some(refusal) = udp_connection_refusal(request, transaction, fixture) {
        return refusal;
    }
    if request.len() < 36 {
        return udp_error(
            transaction,
            &format!("scrape of {} bytes, and BEP 15 says 36", request.len()),
        );
    }
    let mut out = Vec::new();
    out.extend_from_slice(&UDP_SCRAPE.to_be_bytes());
    out.extend_from_slice(&transaction.to_be_bytes());
    let swarms = fixture.swarms.lock().expect("swarm lock");
    // Every twenty bytes after the header is one more info hash, which is how
    // BEP 15 asks for up to 74 of them in one datagram.
    let (hashes, _) = request[16..].as_chunks::<20>();
    for hash in hashes {
        let swarm = swarms.get(hash.as_slice()).cloned().unwrap_or_default();
        let (complete, incomplete) = counts(&swarm);
        out.extend_from_slice(&(complete as u32).to_be_bytes());
        // Completed downloads. This fixture does not count them, and zero is
        // the honest answer rather than a number nothing measured.
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(incomplete as u32).to_be_bytes());
    }
    out
}

/// Refuse a request whose connection id this tracker never issued.
///
/// This is the whole point of BEP 15's connect exchange: an announce is only
/// served to a source address that has already answered a datagram. A fixture
/// that skipped the check could not tell a client that does the round trip
/// from one that invents an id.
fn udp_connection_refusal(request: &[u8], transaction: u32, fixture: &Fixture) -> Option<Vec<u8>> {
    let id = u64::from_be_bytes(request[0..8].try_into().unwrap_or([0; 8]));
    match fixture
        .connections
        .lock()
        .expect("connection lock")
        .contains(&id)
    {
        true => None,
        false => {
            eprintln!("{} udp: connection id {id:#x} was never issued", now_iso());
            Some(udp_error(transaction, "connection id expired"))
        }
    }
}

fn udp_error(transaction: u32, message: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + message.len());
    out.extend_from_slice(&UDP_ERROR.to_be_bytes());
    out.extend_from_slice(&transaction.to_be_bytes());
    out.extend_from_slice(message.as_bytes());
    out
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap_or([0; 4]))
}

fn be64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]))
}

/// A query parameter as UTF-8, for the ones that are always ASCII digits or
/// words. `info_hash` and `peer_id` are raw bytes and never go through this.
fn text(params: &BTreeMap<String, Vec<u8>>, key: &str) -> Option<String> {
    params
        .get(key)
        .map(|value| String::from_utf8_lossy(value).into_owned())
}

/// Split a query string into raw byte values.
///
/// `info_hash` and `peer_id` are twenty arbitrary bytes percent-encoded, so
/// decoding to `String` first would corrupt them. Everything stays as bytes.
fn parse_query(query: &str) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(key.to_string(), percent_decode(value));
    }
    out
}

fn percent_decode(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&value[i + 1..i + 3], 16) {
                    Ok(byte) => out.push(byte),
                    // A stray `%` is passed through rather than dropped, so a
                    // malformed request produces a wrong info hash and a
                    // visible failure instead of a silent near-match.
                    Err(_) => out.push(b'%'),
                }
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Peer ids are mostly ASCII with a random tail. Show the readable part and
/// escape the rest, so the log identifies which client announced.
fn printable(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() {
                (b as char).to_string()
            } else {
                format!("%{b:02x}")
            }
        })
        .collect()
}
