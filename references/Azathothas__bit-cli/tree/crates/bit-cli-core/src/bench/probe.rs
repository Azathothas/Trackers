//! One-shot reachability and capability probe.
//!
//! `bench leech`, `bench seed`, and `bench webseed` all answer "how fast".
//! This answers the question that comes before it: is the thing there, and
//! what does it speak. It moves no payload, runs for as long as one exchange
//! takes, and reports what the other end said about itself.
//!
//! Two kinds of target, decided from the address:
//!
//! - `HOST:PORT` is a peer. Connect, send a BitTorrent handshake, read theirs,
//!   and listen for as long as `--duration` allows. The reserved bytes say
//!   which extensions it claims, the peer id says which client it is, and the
//!   messages it volunteers say what it is holding.
//! - An `http://` or `https://` URL is a mirror. One ranged `GET` for a single
//!   byte, redirects followed by hand, and the TLS parameters read from the
//!   connection.
//!
//! Nothing here is a benchmark of throughput, so the report carries no time
//! series. It carries the environment, like every other `bench` report, and
//! the facts.
//!
//! See `TODO/bench.md`, T-090, step 5.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use librqbit_core::Id20;
use librqbit_peer_protocol::{Handshake, Message};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::error::{Error, Result};
use crate::time::Timestamp;
use crate::units::Millis;
use crate::webseed::bridge::Framer;

/// What a probe found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeReport {
    /// `peer` or `http`.
    pub kind: String,
    /// The target as the caller wrote it.
    pub target: String,
    /// Whether the exchange completed. A refusal that arrives quickly is still
    /// an answer, and this is what separates it from silence.
    pub reachable: bool,
    /// Time to the established connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect: Option<Millis>,
    /// Time from the connection to the first thing the other end said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_response: Option<Millis>,
    /// Time for the whole probe.
    pub elapsed: Millis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<PeerFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub at: String,
}

/// What a peer said about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerFacts {
    /// The peer id it sent, printable characters kept and the rest escaped.
    pub peer_id: String,
    /// The client the peer id names, when the prefix is one of the conventions
    /// in BEP 20.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// The eight reserved bytes, as hex.
    pub reserved: String,
    /// Extensions the reserved bytes claim, by their BEP names.
    pub extensions: Vec<String>,
    /// Whether it echoed the info hash that was asked for. A peer that does
    /// not is answering about a different torrent.
    pub info_hash_matches: bool,
    /// The BEP 10 extended handshake, when it sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended: Option<ExtendedFacts>,
    /// Message types it volunteered, in order, deduplicated.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub messages: Vec<String>,
    /// Pieces its bitfield claims, when it sent one.
    ///
    /// Absent when the peer said `have all` instead, which BEP 6 lets it do
    /// and which carries no count. `messages` names which of the three it
    /// used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pieces_advertised: Option<u32>,
}

/// The BEP 10 extended handshake, as far as this needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtendedFacts {
    /// The `v` key: the client's own name for itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// The `reqq` key: how many block requests it will queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_queue: Option<u32>,
    /// The extension names in `m`, sorted.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extensions: Vec<String>,
    /// The BEP 21 `upload_only` flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_only: Option<bool>,
}

/// What an HTTP endpoint said about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpFacts {
    pub status: u16,
    /// Whether the answer to a one-byte range was a `206`.
    pub range_support: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_length: Option<u64>,
    /// The entity length from `Content-Range`, which is the whole file rather
    /// than the byte that was asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    pub http_version: String,
    /// Every redirect hop, as `status url`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub redirects: Vec<String>,
    /// Where the request ended up, when that differs from where it started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<crate::webseed::probe::TlsReport>,
}

/// Whether a target is a peer address or an HTTP endpoint.
///
/// A URL scheme decides it. Anything else is parsed as a socket address, which
/// is what makes `127.0.0.1:51413` and `[::1]:51413` both work and a hostname
/// with no port an error rather than a silent HTTP probe.
pub fn classify(target: &str) -> Result<Probe> {
    let lower = target.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(Probe::Http(target.trim().to_string()));
    }
    match target.trim().parse::<SocketAddr>() {
        Ok(addr) => Ok(Probe::Peer(addr)),
        Err(_) => Err(Error::usage(format!(
            "`{target}` is neither an http(s) URL nor a HOST:PORT address"
        ))
        .with("value", target.to_string())),
    }
}

/// A classified target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Peer(SocketAddr),
    Http(String),
}

/// Probe one target.
/// The identities are plain byte arrays rather than `librqbit` types, because
/// every caller of this is outside that crate and the conversion is one line.
pub async fn run(
    probe: &Probe,
    target: &str,
    info_hash: [u8; 20],
    peer_id: [u8; 20],
    timeout: Duration,
) -> ProbeReport {
    match probe {
        Probe::Peer(addr) => peer(*addr, target, info_hash, peer_id, timeout).await,
        Probe::Http(url) => http(url, timeout).await,
    }
}

/// Connect to a peer, handshake, and listen.
async fn peer(
    addr: SocketAddr,
    target: &str,
    info_hash: [u8; 20],
    peer_id: [u8; 20],
    timeout: Duration,
) -> ProbeReport {
    let at = Timestamp::now();
    let started = Instant::now();
    let mut report = ProbeReport {
        kind: "peer".to_string(),
        target: target.to_string(),
        reachable: false,
        connect: None,
        first_response: None,
        elapsed: Millis(0),
        peer: None,
        http: None,
        error: None,
        at: at.iso(),
    };

    let stream = match tokio::time::timeout(timeout, TcpStream::connect(addr)).await {
        Err(_) => {
            report.error = Some(format!("no connection within {}", Millis::from(timeout)));
            report.elapsed = Millis::from(started.elapsed());
            return report;
        }
        Ok(Err(e)) => {
            report.error = Some(format!("connect: {e}"));
            report.elapsed = Millis::from(started.elapsed());
            return report;
        }
        Ok(Ok(stream)) => stream,
    };
    report.connect = Some(Millis::from(started.elapsed()));
    let connected = Instant::now();

    let (mut read, mut write) = stream.into_split();
    let ours = Handshake::new(Id20::new(info_hash), Id20::new(peer_id));
    let mut buf = [0u8; 68];
    let len = ours.serialize_unchecked_len(&mut buf);
    if let Err(e) = write.write_all(&buf[..len]).await {
        report.error = Some(format!("write handshake: {e}"));
        report.elapsed = Millis::from(started.elapsed());
        return report;
    }

    // Their handshake, then whatever they volunteer until the deadline. A peer
    // that says nothing after handshaking is still reachable and still
    // reported: silence is a fact about it.
    let mut frames = Framer::default();
    let theirs = match read_handshake(&mut read, &mut frames, timeout).await {
        Ok(theirs) => theirs,
        Err(e) => {
            report.error = Some(e);
            report.elapsed = Millis::from(started.elapsed());
            return report;
        }
    };
    report.first_response = Some(Millis::from(connected.elapsed()));
    report.reachable = true;

    let mut facts = PeerFacts {
        peer_id: printable(&theirs.peer_id.0),
        client: client_of(&theirs.peer_id.0),
        reserved: hex(&theirs.reserved.to_be_bytes()),
        extensions: extensions_of(theirs.reserved),
        info_hash_matches: theirs.info_hash.0 == info_hash,
        extended: None,
        messages: Vec::new(),
        pieces_advertised: None,
    };
    listen(&mut read, &mut frames, &mut facts, timeout).await;

    report.peer = Some(facts);
    report.elapsed = Millis::from(started.elapsed());
    report
}

/// Read one handshake, or say why not.
async fn read_handshake(
    read: &mut (impl tokio::io::AsyncRead + Unpin),
    frames: &mut Framer,
    timeout: Duration,
) -> std::result::Result<Handshake, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok((theirs, size)) = Handshake::deserialize(frames.buffered()) {
            let owned = Handshake {
                info_hash: theirs.info_hash,
                peer_id: theirs.peer_id,
                reserved: theirs.reserved,
            };
            frames.consume(size);
            return Ok(owned);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("no handshake before the deadline".to_string());
        }
        match tokio::time::timeout(left, frames.fill(read)).await {
            Err(_) => return Err("no handshake before the deadline".to_string()),
            Ok(Err(e)) => return Err(format!("read handshake: {e}")),
            Ok(Ok(0)) => return Err("closed during the handshake".to_string()),
            Ok(Ok(_)) => {}
        }
    }
}

/// How long a peer that has already said something gets to say more.
///
/// A peer volunteers its greeting in one burst: the extended handshake, the
/// bitfield, and an unchoke arrive together. Waiting out the whole deadline
/// after that makes every probe cost `--timeout` and tells nobody anything, so
/// the listening ends when the peer goes quiet. A peer that has said nothing
/// at all still gets the full deadline, because that is the case where
/// something might yet arrive.
const QUIET: Duration = Duration::from_millis(400);

/// Collect what the peer volunteers, until it goes quiet or the deadline.
async fn listen(
    read: &mut (impl tokio::io::AsyncRead + Unpin),
    frames: &mut Framer,
    facts: &mut PeerFacts,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    loop {
        match frames.take_frame() {
            Err(_) => return,
            Ok(Some(frame)) => {
                observe(&frame, facts);
                continue;
            }
            Ok(None) => {}
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        let wait = match facts.messages.is_empty() {
            true => left,
            false => left.min(QUIET),
        };
        match tokio::time::timeout(wait, frames.fill(read)).await {
            Ok(Ok(n)) if n > 0 => {}
            // A closed connection ends it, and so does a quiet peer that has
            // already had its say. Neither is an error: everything already
            // learned is kept.
            _ => return,
        }
    }
}

/// Fold one message into the facts.
fn observe(frame: &[u8], facts: &mut PeerFacts) {
    // A zero-length frame is a keep-alive, which carries no message id.
    if frame.len() <= 4 {
        note(facts, "keep-alive");
        return;
    }
    let body = &frame[4..];
    let Ok((message, _)) = Message::deserialize(frame, &[]) else {
        note(facts, "unrecognised");
        return;
    };
    match message {
        Message::Bitfield(bits) => {
            note(facts, "bitfield");
            facts.pieces_advertised = Some(bits.as_ref().iter().map(|b| b.count_ones()).sum());
        }
        Message::Have(_) => note(facts, "have"),
        Message::Choke => note(facts, "choke"),
        Message::Unchoke => note(facts, "unchoke"),
        Message::Interested => note(facts, "interested"),
        Message::NotInterested => note(facts, "not-interested"),
        Message::Request(_) => note(facts, "request"),
        Message::Cancel(_) => note(facts, "cancel"),
        Message::Piece(_) => note(facts, "piece"),
        // BEP 6. `have all` and `have none` stand in for a bitfield, so a
        // target that negotiated the fast extension announces what it holds in
        // two bytes and there are no bits to count. `have all` therefore
        // leaves `pieces_advertised` absent and says so in `messages`, which
        // is a stronger statement than a count anyway: every piece, whatever
        // the torrent turns out to be. See `TODO/bep-coverage.md`, T-100.
        Message::HaveAll => note(facts, "have-all"),
        Message::HaveNone => {
            note(facts, "have-none");
            facts.pieces_advertised = Some(0);
        }
        Message::SuggestPiece(_) => note(facts, "suggest-piece"),
        Message::RejectRequest(_) => note(facts, "reject-request"),
        Message::AllowedFast(_) => note(facts, "allowed-fast"),
        Message::Extended(_) => {
            note(facts, "extended");
            // The extended handshake is extension id 0, and its payload is the
            // bencode dictionary. `librqbit`'s parser exposes the message but
            // not the raw dictionary, so this reads it from the frame: id, then
            // extension id, then the dictionary.
            if body.len() > 2 && body[1] == 0 {
                facts.extended = extended_facts(&body[2..]);
            }
        }
        _ => note(facts, "other"),
    }
}

/// Add a message type once.
fn note(facts: &mut PeerFacts, what: &str) {
    if !facts.messages.iter().any(|seen| seen == what) {
        facts.messages.push(what.to_string());
    }
}

/// Read the parts of an extended handshake worth reporting.
fn extended_facts(dict: &[u8]) -> Option<ExtendedFacts> {
    use crate::torrent::bencode::{Value, decode};

    let value = decode(dict).ok()?;
    let mut extensions: Vec<String> = value
        .get("m")
        .and_then(|m| match m {
            Value::Dict(entries) => Some(
                entries
                    .keys()
                    .map(|key| String::from_utf8_lossy(key).into_owned())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    extensions.sort();
    Some(ExtendedFacts {
        client: value.get("v").and_then(Value::as_text),
        request_queue: value
            .get("reqq")
            .and_then(Value::as_int)
            .map(|n| n.max(0) as u32),
        extensions,
        upload_only: value
            .get("upload_only")
            .and_then(Value::as_int)
            .map(|n| n != 0),
    })
}

/// Probe an HTTP endpoint with a one-byte ranged GET.
async fn http(url: &str, timeout: Duration) -> ProbeReport {
    use crate::webseed::probe::{resolve_redirect, tls_report_within};

    let at = Timestamp::now();
    let started = Instant::now();
    let mut report = ProbeReport {
        kind: "http".to_string(),
        target: url.to_string(),
        reachable: false,
        connect: None,
        first_response: None,
        elapsed: Millis(0),
        peer: None,
        http: None,
        error: None,
        at: at.iso(),
    };

    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .connect_timeout(timeout)
        .user_agent(crate::webseed::fetch::default_user_agent())
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            report.error = Some(format!("cannot build an HTTP client: {e}"));
            report.elapsed = Millis::from(started.elapsed());
            return report;
        }
    };

    // Redirects by hand, so the chain is reported rather than collapsed. Five
    // hops is what every browser allows before calling it a loop.
    let mut current = url.to_string();
    let mut redirects = Vec::new();
    for _ in 0..6 {
        let response = match client
            .get(&current)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                report.error = Some(format!("{current}: {e}"));
                report.elapsed = Millis::from(started.elapsed());
                return report;
            }
        };
        report
            .first_response
            .get_or_insert(Millis::from(started.elapsed()));
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let Ok(next) = resolve_redirect(&current, &location) else {
                report.error = Some(format!("{current}: {status} with no usable Location"));
                report.elapsed = Millis::from(started.elapsed());
                return report;
            };
            redirects.push(format!("{} {next}", status.as_u16()));
            current = next;
            continue;
        }

        let headers = response.headers().clone();
        let header = |name: reqwest::header::HeaderName| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        let entity_length = header(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.rsplit('/').next().map(str::to_string))
            .and_then(|total| total.trim().parse::<u64>().ok());
        report.reachable = status.is_success();
        report.http = Some(HttpFacts {
            status: status.as_u16(),
            range_support: status.as_u16() == 206,
            content_length: response.content_length(),
            entity_length,
            server: header(reqwest::header::SERVER),
            http_version: format!("{:?}", response.version()),
            redirects,
            resolved_url: (current != url).then(|| current.clone()),
            tls: match current.to_ascii_lowercase().starts_with("https://") {
                true => tls_report_within(&current, timeout).await.ok(),
                false => None,
            },
        });
        report.elapsed = Millis::from(started.elapsed());
        return report;
    }

    report.error = Some(format!("{url}: more than five redirects"));
    report.elapsed = Millis::from(started.elapsed());
    report
}

/// The extensions eight reserved bytes claim.
fn extensions_of(reserved: u64) -> Vec<String> {
    let bytes = reserved.to_be_bytes();
    let mut out: Vec<String> = Vec::new();
    if bytes[5] & 0x10 != 0 {
        out.push("extension-protocol".to_string());
    }
    if bytes[7] & 0x01 != 0 {
        out.push("dht".to_string());
    }
    if bytes[7] & 0x04 != 0 {
        out.push("fast".to_string());
    }
    if bytes[7] & 0x02 != 0 {
        out.push("extension-negotiation".to_string());
    }
    if bytes[0] & 0x80 != 0 {
        out.push("azureus-messaging".to_string());
    }
    out
}

/// A peer id with the unprintable bytes escaped, the way a tracker log shows
/// one.
fn printable(id: &[u8; 20]) -> String {
    let mut out = String::with_capacity(20);
    for byte in id {
        match byte.is_ascii_graphic() {
            true => out.push(*byte as char),
            false => out.push_str(&format!("%{byte:02x}")),
        }
    }
    out
}

/// The client an Azureus-style peer id names, per BEP 20.
///
/// Only the prefix is decoded. The version digits differ between clients in
/// ways that are not worth guessing at, so they are reported as written.
fn client_of(id: &[u8; 20]) -> Option<String> {
    if id[0] != b'-' || !id[1].is_ascii_alphabetic() || !id[2].is_ascii_alphanumeric() {
        return None;
    }
    let code = std::str::from_utf8(&id[1..3]).ok()?;
    let version = std::str::from_utf8(&id[3..7]).ok()?;
    let name = match code {
        // This client's own, so a probe of a `bit-cli` seeder reads as one.
        // It said `BC` until T-236, which is BitComet's code and not this
        // one's, so a probe of a real BitComet peer reported `bit-cli`. See
        // `TODO/peers.md`.
        "CL" => "bit-cli",
        "BC" => "BitComet",
        "rQ" | "RQ" => "rqbit",
        "qB" => "qBittorrent",
        "lt" | "LT" => "libtorrent",
        "TR" => "Transmission",
        "UT" => "uTorrent",
        "DE" => "Deluge",
        "AZ" => "Azureus",
        "aria" => "aria2",
        other => other,
    };
    Some(format!("{name} {version}"))
}

/// Lower-case hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_is_an_http_probe_and_an_address_is_a_peer() {
        assert!(matches!(
            classify("https://mirror.example.com/pub/").unwrap(),
            Probe::Http(_)
        ));
        assert!(matches!(
            classify("HTTP://mirror.example.com/").unwrap(),
            Probe::Http(_)
        ));
        assert!(matches!(
            classify("127.0.0.1:51413").unwrap(),
            Probe::Peer(_)
        ));
        assert!(matches!(classify("[::1]:51413").unwrap(), Probe::Peer(_)));
    }

    #[test]
    fn a_hostname_without_a_port_is_a_usage_error() {
        let error = classify("mirror.example.com").unwrap_err();
        assert_eq!(error.code(), crate::ExitCode::Usage);
        assert!(error.message().contains("HOST:PORT"), "{}", error.message());
    }

    #[test]
    fn the_reserved_bytes_name_the_extensions_they_claim() {
        // BEP 10 is bit 0x10 of byte five; DHT and the fast extension are in
        // byte seven.
        let mut bytes = [0u8; 8];
        bytes[5] = 0x10;
        bytes[7] = 0x05;
        let found = extensions_of(u64::from_be_bytes(bytes));
        assert_eq!(found, ["extension-protocol", "dht", "fast"]);
        assert!(extensions_of(0).is_empty());
    }

    #[test]
    fn an_azureus_style_peer_id_names_its_client() {
        let mut id = [b'0'; 20];
        id[..8].copy_from_slice(b"-rQ9000-");
        assert_eq!(client_of(&id).as_deref(), Some("rqbit 9000"));

        id[..8].copy_from_slice(b"-CL0200-");
        assert_eq!(client_of(&id).as_deref(), Some("bit-cli 0200"));

        // And the code this used to answer to belongs to somebody else.
        id[..8].copy_from_slice(b"-BC0100-");
        assert_eq!(client_of(&id).as_deref(), Some("BitComet 0100"));

        // A peer id that is not Azureus style is left alone rather than
        // guessed at.
        let plain = [b'x'; 20];
        assert_eq!(client_of(&plain), None);
    }

    #[test]
    fn a_peer_id_keeps_its_printable_bytes_and_escapes_the_rest() {
        let mut id = [0u8; 20];
        id[..8].copy_from_slice(b"-CL0200-");
        assert_eq!(
            printable(&id),
            "-CL0200-%00%00%00%00%00%00%00%00%00%00%00%00"
        );
    }

    #[test]
    fn an_extended_handshake_yields_the_client_and_the_queue_depth() {
        let dict = b"d1:md11:ut_metadatai1ee4:reqqi250e1:v13:bit-cli/0.1.0e";
        let facts = extended_facts(dict).expect("a dictionary");
        assert_eq!(facts.client.as_deref(), Some("bit-cli/0.1.0"));
        assert_eq!(facts.request_queue, Some(250));
        assert_eq!(facts.extensions, ["ut_metadata"]);
        assert_eq!(facts.upload_only, None);
    }

    #[test]
    fn an_upload_only_flag_is_read_as_a_bool() {
        let dict = b"d1:mde11:upload_onlyi1ee";
        let facts = extended_facts(dict).expect("a dictionary");
        assert_eq!(facts.upload_only, Some(true));
    }
}
