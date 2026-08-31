//! Tracker announce and scrape, over HTTP(S) and UDP.
//!
//! `librqbit` announces on its own while a torrent runs, but it does not
//! expose the result: which tier answered, what interval it asked for, how
//! many seeders and leechers it reported, and why a tracker failed. That is
//! exactly what `bit-cli trackers` exists to show, so the protocol is
//! implemented here rather than inferred from the session's behaviour.
//!
//! - HTTP announce and response: BEP 3, with compact peers from BEP 23.
//! - HTTP scrape: BEP 48.
//! - UDP announce and scrape: BEP 15.
//!
//! Announcing is a read-only operation from `bit-cli`'s point of view. The
//! `.torrent` is never rewritten and no state is stored between runs.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

use crate::error::{Error, Result};
use crate::torrent::bencode::{self, Value};

/// The magic connection id every BEP 15 exchange starts from.
const UDP_PROTOCOL_ID: u64 = 0x0417_2710_1980;

/// BEP 15 action numbers.
const ACTION_CONNECT: u32 = 0;
const ACTION_ANNOUNCE: u32 = 1;
const ACTION_SCRAPE: u32 = 2;
const ACTION_ERROR: u32 = 3;

/// Largest UDP response worth reading. An announce reply is 20 bytes plus six
/// per peer, so this holds well over a thousand peers.
const UDP_BUFFER: usize = 8192;

/// What the client tells the tracker it is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// A regular interval announce.
    #[default]
    None,
    /// The first announce of a session.
    Started,
    /// The last announce before stopping.
    Stopped,
    /// The download just finished.
    Completed,
}

impl Event {
    /// The `event` query parameter, or `None` when there is none.
    pub const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Started => Some("started"),
            Self::Stopped => Some("stopped"),
            Self::Completed => Some("completed"),
        }
    }

    /// The BEP 15 numeric event.
    pub const fn as_udp(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Completed => 1,
            Self::Started => 2,
            Self::Stopped => 3,
        }
    }
}

/// What `left` carries when the length is not known.
///
/// A magnet before its metadata arrives has no total length, so there is no
/// true answer and the question is which untrue one does least harm. Three
/// candidates, and two of them have a named tracker that refuses them:
///
/// - **Zero** is the one to avoid. It is a well-formed answer that means "I am
///   a seed", so the tracker hands this client to every peer asking for one
///   and none of them can be served. It fails silently, at other people's
///   expense, which is the worst of the three.
/// - **A negative**, which some clients send and BEP 15's signed field allows,
///   is refused by real trackers. `torrent/tracker/http/http.go:36` records
///   one: the AWS S3 tracker answers `400 Bad Request: left(-1) was not in the
///   valid range 0 - 9223372036854775807`.
/// - **Omitting the key** is refused by the same tracker with a `500`, which
///   that comment also records.
///
/// So: the largest value that tracker names as valid, which is `i64::MAX`. It
/// is not zero, it is not negative, it is present, and a tracker parsing the
/// field as signed or unsigned reads the same number. `anacrolix/torrent`
/// clamps to exactly this for the same reason.
pub const UNKNOWN_LEFT: u64 = i64::MAX as u64;

/// What one tracker said, for `--trace tracker`.
///
/// The inbound half of the exchange, and it is one function because an
/// announce and a scrape return the same type and a caller reading a trace
/// wants the two side by side. `invalid_peers` is in it because an entry the
/// tracker sent that is not a peer is exactly the thing a report of a smaller
/// swarm than expected turns out to be. See `TODO/trackers.md`, T-180.
fn trace_response(kind: &str, result: &TrackerResult) {
    tracing::trace!(
        target: "bit_cli::tracker",
        kind,
        url = %result.url,
        tier = result.tier,
        protocol = %result.protocol,
        ok = result.ok,
        elapsed_ms = result.elapsed_ms,
        http_status = ?result.http_status,
        seeders = ?result.seeders,
        leechers = ?result.leechers,
        completed = ?result.completed,
        interval_s = ?result.interval_s,
        min_interval_s = ?result.min_interval_s,
        peers = result.peers.len(),
        invalid_peers = result.invalid_peers.len(),
        warning = ?result.warning,
        failure = ?result.failure,
        "response"
    );
}

/// What to tell the tracker about this client.
#[derive(Debug, Clone)]
pub struct Announce {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    /// Bytes still wanted, or `None` when the length is not known yet.
    ///
    /// `None` is a magnet before its metadata arrives, which is a normal path
    /// here rather than an edge case. What goes on the wire for it is
    /// [`UNKNOWN_LEFT`], never zero: zero means seed. See `TODO/trackers.md`,
    /// T-180.
    pub left: Option<u64>,
    pub event: Event,
    pub numwant: u32,
    /// A stable per-run key, which lets a tracker recognise a client whose
    /// address changed.
    pub key: u32,
}

impl Announce {
    /// An announce for a torrent nothing has been downloaded from yet.
    ///
    /// `left` is `None` when the length is not known, which is a magnet whose
    /// metadata has not arrived.
    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20], port: u16, left: Option<u64>) -> Self {
        Self {
            info_hash,
            peer_id,
            port,
            uploaded: 0,
            downloaded: 0,
            left,
            event: Event::Started,
            numwant: 50,
            // A run-scoped key. It has to differ between runs and stay fixed
            // within one, which is exactly what a random value gives.
            key: fastrand_u32(),
        }
    }
}

/// What one tracker said.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerResult {
    /// The URL as it was announced to.
    pub url: String,
    /// Which BEP 12 tier this tracker is in. Zero when the list is flat.
    pub tier: usize,
    /// `http`, `https`, or `udp`.
    pub protocol: String,
    /// Whether the tracker answered with a usable response.
    pub ok: bool,
    /// Round trip time for the whole exchange.
    pub elapsed_ms: u64,
    /// Seeders, from `complete`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seeders: Option<u64>,
    /// Leechers, from `incomplete`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leechers: Option<u64>,
    /// Completed downloads, which only a scrape reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
    /// Seconds the tracker asked the client to wait before announcing again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_s: Option<u64>,
    /// The shortest interval the tracker will accept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_interval_s: Option<u64>,
    /// HTTP status, for an HTTP tracker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    /// Peers the tracker returned.
    pub peers: Vec<String>,
    /// Entries in the peer list that are not peers, one line each.
    ///
    /// A tracker list comes out of a `.torrent`, which is untrusted input, so
    /// one malformed entry must not cost the whole response and must not
    /// vanish either: dropping it silently reports a smaller swarm than the
    /// tracker described and says nothing about why. See `TODO/trackers.md`,
    /// T-180.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invalid_peers: Vec<String>,
    /// The tracker's `warning message`, if it sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Why the announce failed. `failure reason` from the tracker, or the
    /// transport error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// Which address family this announce went out over, when it was pinned to
    /// one. Absent when the family was left to the resolver, which is what an
    /// announce with no family asked for.
    ///
    /// This is the whole point of announcing twice. A tracker records the
    /// source address of the connection it was announced over, so an announce
    /// that only ever went out over one family registers a peer only reachable
    /// on that family. See `TODO/peers.md`, T-022.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<Family>,
    /// The address this announce was actually sent to, once the URL's host was
    /// resolved and filtered to the family. Absent when nothing was dialled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

/// One address family, as an announce is pinned to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    V4,
    V6,
}

impl Family {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V4 => "v4",
            Self::V6 => "v6",
        }
    }

    const fn matches(self, addr: &SocketAddr) -> bool {
        matches!(
            (self, addr),
            (Self::V4, SocketAddr::V4(_)) | (Self::V6, SocketAddr::V6(_))
        )
    }

    /// The unspecified address to bind a local socket of this family to.
    const fn unspecified(self) -> SocketAddr {
        match self {
            Self::V4 => SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            Self::V6 => SocketAddr::new(std::net::IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        }
    }
}

impl std::fmt::Display for Family {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TrackerResult {
    fn failed(url: &str, tier: usize, elapsed: Duration, reason: impl Into<String>) -> Self {
        Self {
            protocol: protocol_of(url).to_string(),
            url: url.to_string(),
            tier,
            ok: false,
            elapsed_ms: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
            seeders: None,
            leechers: None,
            completed: None,
            interval_s: None,
            min_interval_s: None,
            http_status: None,
            peers: Vec::new(),
            invalid_peers: Vec::new(),
            warning: None,
            failure: Some(reason.into()),
            family: None,
            endpoint: None,
        }
    }
}

/// The scheme part of a tracker URL, lower-cased.
pub fn protocol_of(url: &str) -> &'static str {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("udp://") {
        "udp"
    } else if lower.starts_with("https://") {
        "https"
    } else if lower.starts_with("http://") {
        "http"
    } else {
        "unknown"
    }
}

/// A tracker client for one run.
pub struct Client {
    http: reqwest::Client,
    /// Kept so a family-pinned client can be built with the same settings.
    user_agent: String,
    connect_timeout: Duration,
    timeout: Duration,
}

impl Client {
    /// Build a client.
    pub fn new(user_agent: &str, timeout: Duration, connect_timeout: Duration) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(timeout)
            .connect_timeout(connect_timeout)
            .build()
            .map_err(|e| Error::network(format!("cannot build an HTTP client: {e}")))?;
        Ok(Self {
            http,
            user_agent: user_agent.to_string(),
            connect_timeout,
            timeout,
        })
    }

    /// Announce to one tracker, letting the resolver pick the address family.
    pub async fn announce(&self, url: &str, tier: usize, request: &Announce) -> TrackerResult {
        self.announce_on(url, tier, request, None).await
    }

    /// Announce to one tracker over one address family.
    ///
    /// `None` is the old behaviour: resolve the host and use whatever comes
    /// back first, which on a dual-stack host is whichever family the resolver
    /// happened to order first and is not a choice anyone made.
    ///
    /// A tracker under BEP 3 records the **source address of the connection**,
    /// so the family an announce goes out over decides which of this host's
    /// addresses the swarm is told about. Registering both takes two
    /// announces. See `TODO/peers.md`, T-022.
    pub async fn announce_on(
        &self,
        url: &str,
        tier: usize,
        request: &Announce,
        family: Option<Family>,
    ) -> TrackerResult {
        let started = Instant::now();
        // What `--trace tracker` promises, outbound half. The request in full
        // means the fields that decide what the tracker records: `left`, whose
        // unknown case goes out as `UNKNOWN_LEFT` rather than zero, the event,
        // the port it is registering, and the family the connection will go
        // out over, which is the address the tracker learns. See
        // `TODO/trackers.md` T-180 and T-022, and `TODO/cli-surface.md` T-219.
        tracing::trace!(
            target: "bit_cli::tracker",
            url = %url,
            tier,
            protocol = protocol_of(url),
            family = ?family,
            event = ?request.event.as_str(),
            port = request.port,
            left = ?request.left,
            uploaded = request.uploaded,
            downloaded = request.downloaded,
            numwant = request.numwant,
            "announce"
        );
        let outcome = match protocol_of(url) {
            "udp" => self.udp(url, request, false, family).await,
            "http" | "https" => self.http_announce(url, request, family).await,
            other => Err(Error::usage(format!(
                "{url}: `{other}` is not a tracker protocol"
            ))),
        };
        let result = match outcome {
            Ok(mut result) => {
                result.url = url.to_string();
                result.tier = tier;
                result.family = family;
                result.elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                result
            }
            Err(error) => {
                let mut result =
                    TrackerResult::failed(url, tier, started.elapsed(), error.to_string());
                result.family = family;
                result
            }
        };
        trace_response("announce", &result);
        result
    }

    /// Scrape one tracker.
    /// Scrape one tracker.
    ///
    /// `at` is the endpoint to ask, for a tracker whose path does not follow
    /// the BEP 48 convention and so has no endpoint to derive. It replaces the
    /// derivation entirely, including the protocol: a caller may point an
    /// `http://` announce at a `udp://` scrape if that is what the tracker
    /// runs. See `TODO/trackers.md`, T-065.
    pub async fn scrape(
        &self,
        url: &str,
        tier: usize,
        request: &Announce,
        at: Option<&str>,
    ) -> TrackerResult {
        let started = Instant::now();
        let endpoint = at.unwrap_or(url);
        tracing::trace!(
            target: "bit_cli::tracker",
            url = %url,
            endpoint = %endpoint,
            tier,
            protocol = protocol_of(endpoint),
            named = at.is_some(),
            "scrape"
        );
        let outcome = match protocol_of(endpoint) {
            "udp" => self.udp(endpoint, request, true, None).await,
            "http" | "https" => match at.map(str::to_string).or_else(|| scrape_url(endpoint)) {
                Some(scrape) => self.http_scrape(&scrape, request).await,
                None => Err(Error::usage(format!(
                    "{endpoint} does not follow the BEP 48 convention, so its scrape URL cannot be derived. Name it with --scrape-url"
                ))),
            },
            other => Err(Error::usage(format!(
                "{endpoint}: `{other}` is not a tracker protocol"
            ))),
        };
        let result = match outcome {
            Ok(mut result) => {
                result.url = url.to_string();
                result.tier = tier;
                result.elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                result
            }
            Err(error) => TrackerResult::failed(url, tier, started.elapsed(), error.to_string()),
        };
        trace_response("scrape", &result);
        result
    }

    async fn http_announce(
        &self,
        url: &str,
        request: &Announce,
        family: Option<Family>,
    ) -> Result<TrackerResult> {
        let full = format!("{}{}", url, announce_query(url, request));
        // A client pinned to one family, or the shared one when no family was
        // asked for. Pinning is a fresh client because the override is a
        // property of the builder; that costs about a millisecond and this is
        // a diagnostic that announces twice per tracker, not a session.
        let pinned = match family {
            None => None,
            Some(family) => Some(self.pinned_http(url, family)?),
        };
        let http = pinned
            .as_ref()
            .map(|(client, _)| client)
            .unwrap_or(&self.http);
        let response = http
            .get(&full)
            .send()
            .await
            .map_err(|e| Error::network(format!("{url}: {e}")))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| Error::network(format!("{url}: body was cut short: {e}")))?;
        let mut result = parse_http_response(&body)?;
        result.http_status = Some(status.as_u16());
        result.endpoint = pinned.map(|(_, endpoint)| endpoint);
        if !status.is_success() && result.failure.is_none() {
            result.ok = false;
            result.failure = Some(format!("HTTP {status}"));
        }
        Ok(result)
    }

    /// An HTTP client that will only ever reach this tracker over one family.
    ///
    /// `ClientBuilder::local_address` does **not** do this. `hyper-util` binds
    /// the local address only when it already matches the destination's family
    /// and falls through to the unspecified address of the destination's own
    /// family otherwise, so setting `0.0.0.0` still connects over IPv6. The
    /// mechanism that does work is overriding the resolution: the host is
    /// resolved here, filtered to the family, and handed to the builder, so
    /// there is no address of the other family for it to choose.
    fn pinned_http(&self, url: &str, family: Family) -> Result<(reqwest::Client, String)> {
        let (host, port) = http_authority(url)?;
        let addrs = resolve_family(&host, port, family, url)?;
        let endpoint = addrs
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let client = reqwest::Client::builder()
            .user_agent(self.user_agent.clone())
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .resolve_to_addrs(&host, &addrs)
            .build()
            .map_err(|e| Error::network(format!("{url}: cannot build an HTTP client: {e}")))?;
        Ok((client, endpoint))
    }

    async fn http_scrape(&self, url: &str, request: &Announce) -> Result<TrackerResult> {
        let full = format!(
            "{}{}info_hash={}",
            url,
            if url.contains('?') { "&" } else { "?" },
            percent_encode(&request.info_hash)
        );
        let response = self
            .http
            .get(&full)
            .send()
            .await
            .map_err(|e| Error::network(format!("{url}: {e}")))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| Error::network(format!("{url}: body was cut short: {e}")))?;
        let mut result = parse_scrape_response(&body, &request.info_hash)?;
        result.http_status = Some(status.as_u16());
        if !status.is_success() && result.failure.is_none() {
            result.ok = false;
            result.failure = Some(format!("HTTP {status}"));
        }
        Ok(result)
    }

    /// One BEP 15 exchange: connect, then announce or scrape.
    async fn udp(
        &self,
        url: &str,
        request: &Announce,
        scrape: bool,
        family: Option<Family>,
    ) -> Result<TrackerResult> {
        let target = udp_target(url, family)?;
        // The local socket is bound in the destination's family either way.
        // With a family asked for, `udp_target` has already made sure the
        // destination is in it.
        let bind: SocketAddr = match target.is_ipv4() {
            true => Family::V4.unspecified(),
            false => Family::V6.unspecified(),
        };
        let socket = UdpSocket::bind(bind)
            .await
            .map_err(|e| Error::network(format!("{url}: cannot open a UDP socket: {e}")))?;
        socket
            .connect(target)
            .await
            .map_err(|e| Error::network(format!("{url}: cannot reach {target}: {e}")))?;

        let transaction = fastrand_u32();
        let reply = self
            .udp_exchange(
                &socket,
                url,
                &connect_request(transaction),
                ACTION_CONNECT,
                transaction,
            )
            .await?;
        if reply.len() < 16 {
            return Err(Error::network(format!("{url}: short connect response")));
        }
        let connection_id = u64::from_be_bytes(reply[8..16].try_into().unwrap_or([0; 8]));

        let transaction = fastrand_u32();
        let (payload, action) = match scrape {
            true => (
                scrape_request(connection_id, transaction, &request.info_hash),
                ACTION_SCRAPE,
            ),
            false => (
                announce_request(connection_id, transaction, request),
                ACTION_ANNOUNCE,
            ),
        };
        let reply = self
            .udp_exchange(&socket, url, &payload, action, transaction)
            .await?;
        let mut result = match scrape {
            true => parse_udp_scrape(&reply)?,
            false => parse_udp_announce(&reply)?,
        };
        result.endpoint = Some(target.to_string());
        Ok(result)
    }

    /// Send one UDP request and read the matching reply.
    ///
    /// BEP 15 says to retry with an exponential backoff. Three attempts inside
    /// the configured timeout is enough to ride out a dropped datagram without
    /// making a dead tracker cost a minute.
    async fn udp_exchange(
        &self,
        socket: &UdpSocket,
        url: &str,
        payload: &[u8],
        expect_action: u32,
        transaction: u32,
    ) -> Result<Vec<u8>> {
        let per_attempt = (self.timeout / 3).max(Duration::from_secs(1));
        let mut last = String::new();
        for _ in 0..3 {
            socket
                .send(payload)
                .await
                .map_err(|e| Error::network(format!("{url}: cannot send: {e}")))?;
            let mut buf = vec![0u8; UDP_BUFFER];
            match tokio::time::timeout(per_attempt, socket.recv(&mut buf)).await {
                Err(_) => last = "timed out waiting for a reply".to_string(),
                Ok(Err(e)) => last = format!("cannot read: {e}"),
                Ok(Ok(n)) => {
                    buf.truncate(n);
                    if n < 8 {
                        last = format!("short reply of {n} bytes");
                        continue;
                    }
                    let action = u32::from_be_bytes(buf[0..4].try_into().unwrap_or([0; 4]));
                    let echoed = u32::from_be_bytes(buf[4..8].try_into().unwrap_or([0; 4]));
                    if echoed != transaction {
                        // A reply to an earlier attempt. Keep waiting rather
                        // than treating a stale datagram as this answer.
                        last = "reply carried a different transaction id".to_string();
                        continue;
                    }
                    if action == ACTION_ERROR {
                        let text = String::from_utf8_lossy(&buf[8..]).trim().to_string();
                        return Err(Error::network(format!("{url}: {text}")));
                    }
                    if action != expect_action {
                        return Err(Error::network(format!(
                            "{url}: expected action {expect_action}, got {action}"
                        )));
                    }
                    return Ok(buf);
                }
            }
        }
        Err(Error::network(format!("{url}: {last}")))
    }
}

/// The query string for an HTTP announce, including the leading separator.
pub fn announce_query(url: &str, request: &Announce) -> String {
    let mut query = String::from(match url.contains('?') {
        true => "&",
        false => "?",
    });
    query.push_str(&format!("info_hash={}", percent_encode(&request.info_hash)));
    query.push_str(&format!("&peer_id={}", percent_encode(&request.peer_id)));
    query.push_str(&format!("&port={}", request.port));
    query.push_str(&format!("&uploaded={}", request.uploaded));
    query.push_str(&format!("&downloaded={}", request.downloaded));
    query.push_str(&format!("&left={}", request.left.unwrap_or(UNKNOWN_LEFT)));
    query.push_str("&compact=1&no_peer_id=1");
    query.push_str(&format!("&numwant={}", request.numwant));
    query.push_str(&format!("&key={:08x}", request.key));
    if let Some(event) = request.event.as_str() {
        query.push_str(&format!("&event={event}"));
    }
    query
}

/// The BEP 48 scrape URL for an announce URL, when one can be derived.
///
/// The convention is that the last path component is `announce` and the scrape
/// endpoint replaces it with `scrape`. A tracker whose path does not end that
/// way has no defined scrape URL, and guessing one produces a 404 that reads
/// like a tracker failure.
pub fn scrape_url(announce: &str) -> Option<String> {
    let (base, query) = match announce.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (announce, None),
    };
    let (head, last) = base.rsplit_once('/')?;
    let rest = last.strip_prefix("announce")?;
    let mut out = format!("{head}/scrape{rest}");
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    Some(out)
}

/// One count from a tracker response, where a negative means "not known".
///
/// Clamping a negative to zero is the inbound half of the same mistake
/// [`UNKNOWN_LEFT`] describes: zero seeders is a fact about the swarm, and a
/// tracker that sent `-1` did not state one. `None` is the honest reading and
/// is what an absent key already produces, so a caller has one case to handle
/// rather than two. See `TODO/trackers.md`, T-180.
fn count_of(value: &Value, key: &str) -> Option<u64> {
    match value.get(key).and_then(Value::as_int) {
        Some(n) if n >= 0 => Some(n as u64),
        _ => None,
    }
}

/// Parse a bencoded HTTP announce response.
pub fn parse_http_response(body: &[u8]) -> Result<TrackerResult> {
    let value = bencode::decode(body)
        .map_err(|e| Error::network(format!("the tracker did not send bencode: {e}")))?;

    let mut result = TrackerResult {
        url: String::new(),
        tier: 0,
        protocol: String::new(),
        ok: true,
        elapsed_ms: 0,
        seeders: count_of(&value, "complete"),
        leechers: count_of(&value, "incomplete"),
        completed: count_of(&value, "downloaded"),
        interval_s: count_of(&value, "interval"),
        min_interval_s: count_of(&value, "min interval")
            .or_else(|| count_of(&value, "min_interval")),
        http_status: None,
        peers: Vec::new(),
        invalid_peers: Vec::new(),
        warning: value.get("warning message").and_then(Value::as_text),
        failure: value.get("failure reason").and_then(Value::as_text),
        family: None,
        endpoint: None,
    };
    if result.failure.is_some() {
        result.ok = false;
        return Ok(result);
    }

    // A response with no `peers` key at all is a well-formed empty swarm and
    // not an error, which is what an announce to a tracker that knows the
    // torrent and has nobody on it looks like.
    let mut invalid = Vec::new();
    let mut peers = Vec::new();
    if let Some(list) = value.get("peers") {
        peers.extend(parse_peers(list, false, &mut invalid));
    }
    if let Some(list) = value.get("peers6") {
        peers.extend(parse_peers(list, true, &mut invalid));
    }
    result.peers = peers;
    result.invalid_peers = invalid;
    Ok(result)
}

/// Parse a bencoded BEP 48 scrape response.
pub fn parse_scrape_response(body: &[u8], info_hash: &[u8; 20]) -> Result<TrackerResult> {
    let value = bencode::decode(body)
        .map_err(|e| Error::network(format!("the tracker did not send bencode: {e}")))?;

    if let Some(reason) = value
        .get("failure reason")
        .and_then(Value::as_text)
        .or_else(|| value.get("failure_reason").and_then(Value::as_text))
    {
        return Ok(TrackerResult::failed("", 0, Duration::ZERO, reason));
    }

    let entry = value
        .get("files")
        .and_then(Value::as_dict)
        .and_then(|files| files.get(info_hash.as_slice()));
    let Some(entry) = entry else {
        return Ok(TrackerResult::failed(
            "",
            0,
            Duration::ZERO,
            "the tracker does not know this info hash",
        ));
    };

    Ok(TrackerResult {
        url: String::new(),
        tier: 0,
        protocol: String::new(),
        ok: true,
        elapsed_ms: 0,
        seeders: count_of(entry, "complete"),
        leechers: count_of(entry, "incomplete"),
        completed: count_of(entry, "downloaded"),
        interval_s: None,
        min_interval_s: None,
        http_status: None,
        peers: Vec::new(),
        invalid_peers: Vec::new(),
        warning: None,
        failure: None,
        family: None,
        endpoint: None,
    })
}

/// Peers from either the compact form (BEP 23) or the dictionary form.
///
/// Anything in the list that is not a peer is described into `invalid` and
/// skipped. Nothing here fails the response: a tracker that returns one bad
/// entry beside forty good ones has told the caller about forty peers, and
/// refusing all of them because of the forty-first is the failure mode this
/// was written against.
fn parse_peers(value: &Value, ipv6: bool, invalid: &mut Vec<String>) -> Vec<String> {
    let stride = match ipv6 {
        true => 18,
        false => 6,
    };
    let key = match ipv6 {
        true => "peers6",
        false => "peers",
    };
    if let Some(bytes) = value.as_bytes() {
        // A compact list is a whole number of fixed-size addresses. A
        // remainder is a truncated address, and `chunks_exact` drops it
        // without a word, which is the silent half of the same problem.
        let whole = bytes.len() - bytes.len() % stride;
        if whole != bytes.len() {
            invalid.push(format!(
                "`{key}` carried {} bytes, which is {} address(es) of {stride} bytes and {} left over",
                bytes.len(),
                whole / stride,
                bytes.len() - whole
            ));
        }
        return bytes[..whole]
            .chunks_exact(stride)
            .map(|chunk| match ipv6 {
                true => {
                    let mut octets = [0u8; 16];
                    octets.copy_from_slice(&chunk[..16]);
                    let port = u16::from_be_bytes([chunk[16], chunk[17]]);
                    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::from(octets), port, 0, 0))
                        .to_string()
                }
                false => {
                    let ip = Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
                    let port = u16::from_be_bytes([chunk[4], chunk[5]]);
                    SocketAddr::V4(SocketAddrV4::new(ip, port)).to_string()
                }
            })
            .collect();
    }
    let Some(list) = value.as_list() else {
        invalid.push(format!(
            "`{key}` is neither a compact byte string nor a list of peer dictionaries"
        ));
        return Vec::new();
    };
    list.iter()
        .enumerate()
        .filter_map(|(index, peer)| {
            // `peers: [42]` is the shape this exists for: a list whose entries
            // are integers rather than dictionaries. Naming what the entry is
            // costs one branch and is the difference between a caller who can
            // report the tracker and one who counts peers twice looking for
            // the missing one.
            if peer.as_dict().is_none() {
                invalid.push(format!("`{key}` entry {index} is not a peer dictionary"));
                return None;
            }
            let Some(ip) = peer.get("ip").and_then(Value::as_text) else {
                invalid.push(format!("`{key}` entry {index} has no `ip`"));
                return None;
            };
            let Some(port) = peer.get("port").and_then(Value::as_int) else {
                invalid.push(format!("`{key}` entry {index} ({ip}) has no `port`"));
                return None;
            };
            // The dictionary form's `port` is a bencoded integer, so it can be
            // negative or past 65535, and either one formats into an address
            // string nothing can dial.
            let Ok(port) = u16::try_from(port) else {
                invalid.push(format!(
                    "`{key}` entry {index} ({ip}) has port {port}, which is not a port"
                ));
                return None;
            };
            Some(match ip.contains(':') {
                true => format!("[{ip}]:{port}"),
                false => format!("{ip}:{port}"),
            })
        })
        .collect()
}

/// The `host:port` a `udp://` tracker URL points at.
fn udp_target(url: &str, family: Option<Family>) -> Result<SocketAddr> {
    let rest = url
        .trim()
        .strip_prefix("udp://")
        .ok_or_else(|| Error::usage(format!("{url} is not a udp:// URL")))?;
    let authority = rest.split(['/', '?']).next().unwrap_or(rest);
    let mut resolved = std::net::ToSocketAddrs::to_socket_addrs(&authority)
        .map_err(|e| Error::network(format!("{url}: cannot resolve {authority}: {e}")))?;
    match family {
        // What this used to do, always: take whatever the resolver put first.
        // On a dual-stack host that is not a choice, it is an ordering.
        None => resolved
            .next()
            .ok_or_else(|| Error::network(format!("{url}: {authority} resolved to no address"))),
        Some(family) => resolved.find(|addr| family.matches(addr)).ok_or_else(|| {
            Error::network(format!(
                "{url}: {authority} has no IP{family} address to announce over"
            ))
        }),
    }
}

/// Which address families a tracker URL resolves to, in a stable order.
///
/// This is what decides how many announces a tracker gets. A host with both an
/// A and an AAAA record is two announces, because a tracker records the source
/// address of the connection and one announce registers one of this host's
/// addresses. A host with one is one, and a host that resolves to nothing is
/// the error the caller reports.
pub fn families_of(url: &str) -> Result<Vec<Family>> {
    let (host, port) = match protocol_of(url) {
        "udp" => {
            let rest = url
                .trim()
                .strip_prefix("udp://")
                .ok_or_else(|| Error::usage(format!("{url} is not a udp:// URL")))?;
            let authority = rest.split(['/', '?']).next().unwrap_or(rest);
            match authority.rsplit_once(':') {
                Some((host, port)) => (
                    host.trim_matches(['[', ']']).to_string(),
                    port.parse()
                        .map_err(|_| Error::usage(format!("{url}: `{port}` is not a port")))?,
                ),
                None => {
                    return Err(Error::usage(format!(
                        "{url}: a udp:// tracker needs a port"
                    )));
                }
            }
        }
        "http" | "https" => http_authority(url)?,
        other => {
            return Err(Error::usage(format!(
                "{url}: `{other}` is not a tracker protocol"
            )));
        }
    };
    let mut families: Vec<Family> =
        std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
            .map_err(|e| Error::network(format!("{url}: cannot resolve {host}: {e}")))?
            .map(|addr| match addr {
                SocketAddr::V4(_) => Family::V4,
                SocketAddr::V6(_) => Family::V6,
            })
            .collect();
    families.sort();
    families.dedup();
    match families.is_empty() {
        true => Err(Error::network(format!(
            "{url}: {host} resolved to no address"
        ))),
        false => Ok(families),
    }
}

/// The host and port of an HTTP tracker URL, for resolving it by hand.
///
/// Written out rather than pulled from a URL crate because this file already
/// parses these URLs by hand everywhere else, and the shape it has to handle
/// is the same one `protocol_of` and `scrape_url` handle.
fn http_authority(url: &str) -> Result<(String, u16)> {
    let trimmed = url.trim();
    let (scheme, rest) = match trimmed.split_once("://") {
        Some(pair) => pair,
        None => return Err(Error::usage(format!("{url} is not an http:// URL"))),
    };
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        other => return Err(Error::usage(format!("{url}: `{other}` is not HTTP"))),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    // Userinfo is legal in a URL and is not part of the host.
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    // A bracketed IPv6 literal carries colons of its own, so the port split
    // has to happen after the bracket rather than at the last colon.
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| Error::usage(format!("{url}: unclosed [ in the host")))?;
        let port = match tail.strip_prefix(':') {
            Some(port) => port
                .parse()
                .map_err(|_| Error::usage(format!("{url}: `{port}` is not a port")))?,
            None => default_port,
        };
        return Ok((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Ok((
            host.to_string(),
            port.parse()
                .map_err(|_| Error::usage(format!("{url}: `{port}` is not a port")))?,
        )),
        None => Ok((authority.to_string(), default_port)),
    }
}

/// Resolve a host to every address it has in one family.
fn resolve_family(host: &str, port: u16, family: Family, url: &str) -> Result<Vec<SocketAddr>> {
    let addrs: Vec<SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(&(host, port))
        .map_err(|e| Error::network(format!("{url}: cannot resolve {host}: {e}")))?
        .filter(|addr| family.matches(addr))
        .collect();
    match addrs.is_empty() {
        true => Err(Error::network(format!(
            "{url}: {host} has no IP{family} address to announce over"
        ))),
        false => Ok(addrs),
    }
}

fn connect_request(transaction: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&UDP_PROTOCOL_ID.to_be_bytes());
    out.extend_from_slice(&ACTION_CONNECT.to_be_bytes());
    out.extend_from_slice(&transaction.to_be_bytes());
    out
}

fn announce_request(connection_id: u64, transaction: u32, request: &Announce) -> Vec<u8> {
    let mut out = Vec::with_capacity(98);
    out.extend_from_slice(&connection_id.to_be_bytes());
    out.extend_from_slice(&ACTION_ANNOUNCE.to_be_bytes());
    out.extend_from_slice(&transaction.to_be_bytes());
    out.extend_from_slice(&request.info_hash);
    out.extend_from_slice(&request.peer_id);
    out.extend_from_slice(&request.downloaded.to_be_bytes());
    out.extend_from_slice(&request.left.unwrap_or(UNKNOWN_LEFT).to_be_bytes());
    out.extend_from_slice(&request.uploaded.to_be_bytes());
    out.extend_from_slice(&request.event.as_udp().to_be_bytes());
    // IP address zero means "use the source address of this datagram", which
    // is right for every case that is not an explicit override.
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&request.key.to_be_bytes());
    out.extend_from_slice(&request.numwant.to_be_bytes());
    out.extend_from_slice(&request.port.to_be_bytes());
    out
}

fn scrape_request(connection_id: u64, transaction: u32, info_hash: &[u8; 20]) -> Vec<u8> {
    let mut out = Vec::with_capacity(36);
    out.extend_from_slice(&connection_id.to_be_bytes());
    out.extend_from_slice(&ACTION_SCRAPE.to_be_bytes());
    out.extend_from_slice(&transaction.to_be_bytes());
    out.extend_from_slice(info_hash);
    out
}

/// Parse a BEP 15 announce reply.
pub fn parse_udp_announce(reply: &[u8]) -> Result<TrackerResult> {
    if reply.len() < 20 {
        return Err(Error::network(format!(
            "short announce reply of {} bytes",
            reply.len()
        )));
    }
    let interval = u32::from_be_bytes(reply[8..12].try_into().unwrap_or([0; 4]));
    let leechers = u32::from_be_bytes(reply[12..16].try_into().unwrap_or([0; 4]));
    let seeders = u32::from_be_bytes(reply[16..20].try_into().unwrap_or([0; 4]));
    // Six bytes per peer, four of address and two of port. A trailing partial
    // entry is not a peer and is dropped.
    let (entries, _) = reply[20..].as_chunks::<6>();
    let peers = entries
        .iter()
        .map(|chunk| {
            let ip = Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
            let port = u16::from_be_bytes([chunk[4], chunk[5]]);
            SocketAddr::V4(SocketAddrV4::new(ip, port)).to_string()
        })
        .collect();

    Ok(TrackerResult {
        url: String::new(),
        tier: 0,
        protocol: "udp".to_string(),
        ok: true,
        elapsed_ms: 0,
        seeders: Some(u64::from(seeders)),
        leechers: Some(u64::from(leechers)),
        completed: None,
        interval_s: Some(u64::from(interval)),
        min_interval_s: None,
        http_status: None,
        peers,
        // A UDP announce carries peers as a fixed-size array and nothing
        // else, so there is no shape here for an entry to be wrong in.
        invalid_peers: Vec::new(),
        warning: None,
        failure: None,
        family: None,
        endpoint: None,
    })
}

/// Parse a BEP 15 scrape reply for one info hash.
pub fn parse_udp_scrape(reply: &[u8]) -> Result<TrackerResult> {
    if reply.len() < 20 {
        return Err(Error::network(format!(
            "short scrape reply of {} bytes",
            reply.len()
        )));
    }
    let seeders = u32::from_be_bytes(reply[8..12].try_into().unwrap_or([0; 4]));
    let completed = u32::from_be_bytes(reply[12..16].try_into().unwrap_or([0; 4]));
    let leechers = u32::from_be_bytes(reply[16..20].try_into().unwrap_or([0; 4]));

    Ok(TrackerResult {
        url: String::new(),
        tier: 0,
        protocol: "udp".to_string(),
        ok: true,
        elapsed_ms: 0,
        seeders: Some(u64::from(seeders)),
        leechers: Some(u64::from(leechers)),
        completed: Some(u64::from(completed)),
        interval_s: None,
        min_interval_s: None,
        http_status: None,
        peers: Vec::new(),
        invalid_peers: Vec::new(),
        warning: None,
        failure: None,
        family: None,
        endpoint: None,
    })
}

/// Percent-encode raw bytes for a tracker query string.
///
/// A tracker query carries the twenty raw bytes of an info hash, not its hex
/// rendering, so this encodes everything outside the unreserved set from
/// RFC 3986. Getting this wrong produces a tracker that answers "torrent not
/// found" for a torrent it is tracking.
pub fn percent_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for byte in bytes {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// A random `u32` without pulling in a random number generator.
///
/// Transaction ids and the announce key only have to be unpredictable enough
/// that two runs do not collide, which the system clock plus the address of a
/// stack local provides.
fn fastrand_u32() -> u32 {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    );
    hasher.finish() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Announce {
        Announce {
            info_hash: [
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
                0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
            ],
            peer_id: *b"-BC0100-abcdefghijkl",
            port: 6881,
            uploaded: 10,
            downloaded: 20,
            left: Some(30),
            event: Event::Started,
            numwant: 50,
            key: 0xdead_beef,
        }
    }

    #[test]
    fn raw_bytes_are_percent_encoded_not_hex_encoded() {
        assert_eq!(percent_encode(&[0x01, 0x02]), "%01%02");
        assert_eq!(percent_encode(b"abcXYZ019-_.~"), "abcXYZ019-_.~");
        assert_eq!(percent_encode(b"/?&="), "%2F%3F%26%3D");
        assert_eq!(percent_encode(&[0xff]), "%FF");
    }

    #[test]
    fn an_announce_query_carries_every_required_parameter() {
        let query = announce_query("http://t.example/announce", &request());
        assert!(query.starts_with('?'), "{query}");
        for key in [
            "info_hash=",
            "peer_id=",
            "port=6881",
            "uploaded=10",
            "downloaded=20",
            "left=30",
            "compact=1",
            "event=started",
        ] {
            assert!(query.contains(key), "{key} missing from {query}");
        }
    }

    #[test]
    fn an_announce_url_that_already_has_a_query_gets_an_ampersand() {
        let query = announce_query("http://t.example/announce?pk=abc", &request());
        assert!(query.starts_with('&'), "{query}");
    }

    #[test]
    fn a_regular_interval_announce_sends_no_event() {
        let mut request = request();
        request.event = Event::None;
        let query = announce_query("http://t.example/announce", &request);
        assert!(!query.contains("event="), "{query}");
    }

    #[test]
    fn scrape_urls_follow_the_bep_48_convention() {
        assert_eq!(
            scrape_url("http://t.example/announce").as_deref(),
            Some("http://t.example/scrape")
        );
        assert_eq!(
            scrape_url("http://t.example/announce.php").as_deref(),
            Some("http://t.example/scrape.php")
        );
        assert_eq!(
            scrape_url("http://t.example/x/announce?pk=abc").as_deref(),
            Some("http://t.example/x/scrape?pk=abc")
        );
    }

    #[test]
    fn a_tracker_with_no_announce_path_has_no_derivable_scrape_url() {
        // Guessing here would produce a 404 that reads like the tracker being
        // down, which is a worse answer than saying it cannot be derived.
        assert_eq!(scrape_url("http://t.example/track"), None);
        assert_eq!(scrape_url("http://t.example/"), None);
    }

    #[test]
    fn a_normal_announce_response_parses() {
        let body = b"d8:completei12e10:incompletei3e8:intervali1800e12:min intervali900e5:peers6:\x7f\x00\x00\x01\x1a\xe1e";
        let result = parse_http_response(body).unwrap();
        assert!(result.ok);
        assert_eq!(result.seeders, Some(12));
        assert_eq!(result.leechers, Some(3));
        assert_eq!(result.interval_s, Some(1800));
        assert_eq!(result.min_interval_s, Some(900));
        assert_eq!(result.peers, vec!["127.0.0.1:6881"]);
    }

    #[test]
    fn a_failure_reason_is_reported_rather_than_parsed_past() {
        let body = b"d14:failure reason17:torrent not founde";
        let result = parse_http_response(body).unwrap();
        assert!(!result.ok);
        assert_eq!(result.failure.as_deref(), Some("torrent not found"));
        assert!(result.peers.is_empty());
    }

    #[test]
    fn a_warning_message_does_not_make_the_announce_a_failure() {
        let body = b"d15:warning message21:tracker is overloaded8:completei1ee";
        let result = parse_http_response(body).unwrap();
        assert!(result.ok);
        assert_eq!(result.warning.as_deref(), Some("tracker is overloaded"));
        assert_eq!(result.seeders, Some(1));
    }

    #[test]
    fn the_dictionary_peer_form_parses_as_well_as_the_compact_one() {
        let body = b"d5:peersld2:ip9:127.0.0.14:porti6881eeee";
        let result = parse_http_response(body).unwrap();
        assert_eq!(result.peers, vec!["127.0.0.1:6881"]);
    }

    #[test]
    fn ipv6_peers_come_back_bracketed() {
        let mut body = Vec::from(&b"d6:peers618:"[..]);
        body.extend_from_slice(&[0u8; 15]);
        body.push(1);
        body.extend_from_slice(&6881u16.to_be_bytes());
        body.push(b'e');
        let result = parse_http_response(&body).unwrap();
        assert_eq!(result.peers, vec!["[::1]:6881"]);
    }

    #[test]
    fn a_truncated_compact_peer_list_drops_the_partial_entry() {
        // Seven bytes is one peer and one stray byte. The stray byte is not a
        // peer and inventing an address from it would be worse than losing it.
        let body = b"d5:peers7:\x7f\x00\x00\x01\x1a\xe1\x00e";
        let result = parse_http_response(body).unwrap();
        assert_eq!(result.peers, vec!["127.0.0.1:6881"]);
    }

    #[test]
    fn a_scrape_response_is_read_for_the_hash_that_was_asked_for() {
        let hash = request().info_hash;
        let mut body = Vec::from(&b"d5:filesd20:"[..]);
        body.extend_from_slice(&hash);
        body.extend_from_slice(b"d8:completei7e10:downloadedi42e10:incompletei2eeee");
        let result = parse_scrape_response(&body, &hash).unwrap();
        assert!(result.ok);
        assert_eq!(result.seeders, Some(7));
        assert_eq!(result.leechers, Some(2));
        assert_eq!(result.completed, Some(42));
    }

    #[test]
    fn a_scrape_for_an_unknown_hash_says_so() {
        let body = b"d5:filesdee";
        let result = parse_scrape_response(body, &request().info_hash).unwrap();
        assert!(!result.ok);
        assert!(result.failure.unwrap().contains("does not know"));
    }

    #[test]
    fn a_udp_connect_request_carries_the_protocol_magic() {
        let payload = connect_request(0x1234_5678);
        assert_eq!(payload.len(), 16);
        assert_eq!(
            u64::from_be_bytes(payload[0..8].try_into().unwrap()),
            UDP_PROTOCOL_ID
        );
        assert_eq!(
            u32::from_be_bytes(payload[8..12].try_into().unwrap()),
            ACTION_CONNECT
        );
        assert_eq!(
            u32::from_be_bytes(payload[12..16].try_into().unwrap()),
            0x1234_5678
        );
    }

    #[test]
    fn a_udp_announce_request_is_ninety_eight_bytes_in_the_documented_order() {
        let payload = announce_request(0xaabb_ccdd_eeff_0011, 7, &request());
        assert_eq!(payload.len(), 98, "BEP 15 fixes the announce request size");
        assert_eq!(
            u32::from_be_bytes(payload[8..12].try_into().unwrap()),
            ACTION_ANNOUNCE
        );
        assert_eq!(&payload[16..36], &request().info_hash);
        assert_eq!(&payload[36..56], &request().peer_id);
        assert_eq!(
            u64::from_be_bytes(payload[56..64].try_into().unwrap()),
            20,
            "downloaded"
        );
        assert_eq!(
            u64::from_be_bytes(payload[64..72].try_into().unwrap()),
            30,
            "left"
        );
        assert_eq!(
            u64::from_be_bytes(payload[72..80].try_into().unwrap()),
            10,
            "uploaded"
        );
        assert_eq!(
            u32::from_be_bytes(payload[80..84].try_into().unwrap()),
            2,
            "started"
        );
        assert_eq!(
            u16::from_be_bytes(payload[96..98].try_into().unwrap()),
            6881
        );
    }

    #[test]
    fn a_udp_scrape_request_is_thirty_six_bytes() {
        let payload = scrape_request(1, 2, &request().info_hash);
        assert_eq!(payload.len(), 36);
        assert_eq!(
            u32::from_be_bytes(payload[8..12].try_into().unwrap()),
            ACTION_SCRAPE
        );
        assert_eq!(&payload[16..36], &request().info_hash);
    }

    #[test]
    fn udp_event_numbers_follow_bep_15() {
        assert_eq!(Event::None.as_udp(), 0);
        assert_eq!(Event::Completed.as_udp(), 1);
        assert_eq!(Event::Started.as_udp(), 2);
        assert_eq!(Event::Stopped.as_udp(), 3);
    }

    #[test]
    fn a_udp_announce_reply_parses_its_counts_and_peers() {
        let mut reply = Vec::new();
        reply.extend_from_slice(&ACTION_ANNOUNCE.to_be_bytes());
        reply.extend_from_slice(&7u32.to_be_bytes());
        reply.extend_from_slice(&1800u32.to_be_bytes());
        reply.extend_from_slice(&3u32.to_be_bytes());
        reply.extend_from_slice(&12u32.to_be_bytes());
        reply.extend_from_slice(&[127, 0, 0, 1]);
        reply.extend_from_slice(&6881u16.to_be_bytes());

        let result = parse_udp_announce(&reply).unwrap();
        assert_eq!(result.interval_s, Some(1800));
        assert_eq!(result.leechers, Some(3));
        assert_eq!(result.seeders, Some(12));
        assert_eq!(result.peers, vec!["127.0.0.1:6881"]);
    }

    #[test]
    fn a_short_udp_reply_is_an_error_rather_than_zeroes() {
        assert!(parse_udp_announce(&[0; 12]).is_err());
        assert!(parse_udp_scrape(&[0; 12]).is_err());
    }

    #[test]
    fn a_udp_scrape_reply_reads_seeders_completed_leechers_in_that_order() {
        let mut reply = Vec::new();
        reply.extend_from_slice(&ACTION_SCRAPE.to_be_bytes());
        reply.extend_from_slice(&7u32.to_be_bytes());
        reply.extend_from_slice(&12u32.to_be_bytes());
        reply.extend_from_slice(&42u32.to_be_bytes());
        reply.extend_from_slice(&3u32.to_be_bytes());

        let result = parse_udp_scrape(&reply).unwrap();
        assert_eq!(result.seeders, Some(12));
        assert_eq!(result.completed, Some(42));
        assert_eq!(result.leechers, Some(3));
    }

    #[test]
    fn protocols_are_recognised_case_insensitively() {
        assert_eq!(protocol_of("UDP://t.example:451/announce"), "udp");
        assert_eq!(protocol_of("HTTPS://t.example/announce"), "https");
        assert_eq!(protocol_of("http://t.example/announce"), "http");
        assert_eq!(protocol_of("wss://t.example"), "unknown");
    }

    #[test]
    fn a_udp_url_resolves_to_its_authority() {
        let addr = udp_target("udp://127.0.0.1:451/announce", None).unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:451");
        assert!(udp_target("http://t.example/announce", None).is_err());
    }

    /// A family that was asked for and is not there is an error naming it,
    /// not a silent fallback to the other one.
    ///
    /// Falling back would be the worst answer available: the caller asked to
    /// announce over one family, and announcing over the other registers an
    /// address they did not ask to publish and reports it as if they had.
    #[test]
    fn a_udp_url_with_no_address_in_the_family_is_refused() {
        let v4 = udp_target("udp://127.0.0.1:451/announce", Some(Family::V4)).unwrap();
        assert_eq!(v4.to_string(), "127.0.0.1:451");
        let err = udp_target("udp://127.0.0.1:451/announce", Some(Family::V6)).unwrap_err();
        assert!(
            err.to_string().contains("IPv6"),
            "the error should name the family: {err}"
        );
        let v6 = udp_target("udp://[::1]:451/announce", Some(Family::V6)).unwrap();
        assert_eq!(v6.to_string(), "[::1]:451");
    }

    #[test]
    fn an_http_url_splits_into_a_host_and_a_port() {
        assert_eq!(
            http_authority("http://t.example/announce").unwrap(),
            ("t.example".to_string(), 80)
        );
        assert_eq!(
            http_authority("https://t.example/announce").unwrap(),
            ("t.example".to_string(), 443)
        );
        assert_eq!(
            http_authority("http://t.example:6969/announce?x=1").unwrap(),
            ("t.example".to_string(), 6969)
        );
        // Userinfo is not the host.
        assert_eq!(
            http_authority("http://user:pw@t.example:8080/a").unwrap(),
            ("t.example".to_string(), 8080)
        );
        assert!(http_authority("udp://t.example:451").is_err());
    }

    /// An IPv6 literal carries colons of its own, so the port cannot be split
    /// off at the last one.
    #[test]
    fn a_bracketed_ipv6_host_keeps_its_colons() {
        assert_eq!(
            http_authority("http://[::1]:6969/announce").unwrap(),
            ("::1".to_string(), 6969)
        );
        assert_eq!(
            http_authority("http://[2001:db8::1]/announce").unwrap(),
            ("2001:db8::1".to_string(), 80)
        );
        assert!(http_authority("http://[::1:6969/announce").is_err());
    }

    /// Literals need no resolver, so this says the same thing on every host.
    #[test]
    fn a_literal_address_resolves_to_its_own_family_only() {
        assert_eq!(
            families_of("udp://127.0.0.1:451/announce").unwrap(),
            vec![Family::V4]
        );
        assert_eq!(
            families_of("udp://[::1]:451/announce").unwrap(),
            vec![Family::V6]
        );
        assert_eq!(
            families_of("http://127.0.0.1:6969/announce").unwrap(),
            vec![Family::V4]
        );
        assert_eq!(
            families_of("http://[::1]:6969/announce").unwrap(),
            vec![Family::V6]
        );
        assert!(families_of("wss://t.example/announce").is_err());
    }

    #[test]
    fn two_random_keys_differ() {
        // The key only has to be unpredictable enough that two runs do not
        // collide, but zero every time would defeat the point.
        let values: std::collections::HashSet<u32> = (0..8).map(|_| fastrand_u32()).collect();
        assert!(values.len() > 1, "the key generator returned a constant");
    }
}
