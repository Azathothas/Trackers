//! Shared machinery for the verbs that need a live torrent session.
//!
//! `download`, `seed`, `peers`, `trackers`, and `bench` all do the same four
//! things: resolve a source, start a session, attach HTTP sources to it, and
//! then watch until a stop condition fires. That is what lives here, so a
//! flag means one thing across every verb rather than one thing per command.

use std::collections::{BTreeMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bit_cli_core::engine::{Engine, EngineOptions, Handle, PeerSnapshot, TorrentSnapshot};
use bit_cli_core::error::{Error, Result};
use bit_cli_core::layout::Layout;
use bit_cli_core::torrent::Metainfo;
use bit_cli_core::units::{format_rate, format_size, parse_duration, parse_rate};
use bit_cli_core::webseed::binding::{BindingSet, SourceSpec};
use bit_cli_core::webseed::bridge::{BridgeParams, BridgeState, BridgeStatus};
use bit_cli_core::webseed::fetch::{Fetcher, Verify};
use bit_cli_core::webseed::ledger::{BlockLedger, Conviction};
use serde::Serialize;

use crate::cli::{Global, LimitArgs, TrackerArgs, WebSeedArgs};
use crate::env::Env;
use crate::output::Renderer;

/// A tokio runtime for the commands that do I/O.
///
/// One per invocation. Nothing here outlives the process, so the runtime is
/// built where it is used rather than held in a global.
pub fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::generic(format!("cannot start the async runtime: {e}")))
}

/// Parse a duration flag, naming the flag when it is wrong.
pub fn duration_flag(value: &str, flag: &str) -> Result<Duration> {
    parse_duration(value)
        .map_err(|e| Error::usage(format!("--{flag}: {e}")).with("value", value.to_string()))
}

/// Parse an optional duration flag.
pub fn optional_duration(value: &Option<String>, flag: &str) -> Result<Option<Duration>> {
    value.as_deref().map(|v| duration_flag(v, flag)).transpose()
}

/// Parse a rate flag, naming the flag when it is wrong.
pub fn rate_flag(value: &Option<String>, flag: &str) -> Result<Option<u64>> {
    let Some(text) = value else { return Ok(None) };
    parse_rate(text)
        .map(Some)
        .map_err(|e| Error::usage(format!("--{flag}: {e}")).with("value", text.clone()))
}

/// Parse a size flag, naming the flag when it is wrong.
pub fn size_flag(value: &Option<String>, flag: &str) -> Result<Option<u64>> {
    let Some(text) = value else { return Ok(None) };
    bit_cli_core::units::parse_size(text)
        .map(Some)
        .map_err(|e| Error::usage(format!("--{flag}: {e}")).with("value", text.clone()))
}

/// Parse a `--port` value, which is either `N` or `START-END`.
pub fn port_range(values: &[String]) -> Result<std::ops::RangeInclusive<u16>> {
    if values.is_empty() {
        return Ok(6881..=6889);
    }
    let mut low = u16::MAX;
    let mut high = u16::MIN;
    for value in values {
        let (start, end) = match value.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (value.trim(), value.trim()),
        };
        let parse = |text: &str| -> Result<u16> {
            text.parse::<u16>().map_err(|_| {
                Error::usage(format!(
                    "--port `{value}` is not a port or a START-END range"
                ))
                .with("value", value.clone())
            })
        };
        let (start, end) = (parse(start)?, parse(end)?);
        if start > end {
            return Err(Error::usage(format!("--port `{value}` runs backwards"))
                .with("value", value.clone()));
        }
        low = low.min(start);
        high = high.max(end);
    }
    Ok(low..=high)
}

/// Parse `--peer` values into addresses.
///
/// A name is resolved here rather than at dial time, so `--peer nope:6881`
/// fails before the session starts instead of showing up later as a peer that
/// never connected. A name that resolves to several addresses contributes all
/// of them, because any of them may be the one that answers.
/// Parse `--block-peer` values into inclusive address ranges.
///
/// Three spellings, because a blocklist is written in all three: one address,
/// an inclusive `START-END` range, and a CIDR block. A `HOST:PORT` is refused
/// rather than truncated to its address, because the session blocks an address
/// and accepting a port would silently block every port on that host.
///
/// Nothing is resolved. `--peer` takes a name because a caller naming a peer
/// wants to reach it; a blocklist entry that resolved would block whatever the
/// name pointed at when the run started, which is not what a block means. See
/// `TODO/peers.md`, T-164.
pub fn blocked_ranges(values: &[String]) -> Result<Vec<(IpAddr, IpAddr)>> {
    let mut out = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        out.push(blocked_range(value)?);
    }
    Ok(out)
}

fn blocked_range(value: &str) -> Result<(IpAddr, IpAddr)> {
    if let Some((base, prefix)) = value.split_once('/') {
        let base: IpAddr = base.parse().map_err(|_| block_error(value))?;
        let prefix: u32 = prefix.parse().map_err(|_| block_error(value))?;
        return cidr_range(value, base, prefix);
    }
    // IPv6 uses colons and never a hyphen, so a hyphen is unambiguously the
    // range separator in both families.
    if let Some((start, end)) = value.split_once('-') {
        let start: IpAddr = start.trim().parse().map_err(|_| block_error(value))?;
        let end: IpAddr = end.trim().parse().map_err(|_| block_error(value))?;
        return match (start, end) {
            (IpAddr::V4(a), IpAddr::V4(b)) if a <= b => Ok((start, end)),
            (IpAddr::V6(a), IpAddr::V6(b)) if a <= b => Ok((start, end)),
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)) => Err(Error::usage(
                format!("--block-peer `{value}` runs backwards"),
            )
            .with("value", value.to_string())),
            _ => Err(Error::usage(format!(
                "--block-peer `{value}` mixes IPv4 and IPv6 in one range"
            ))
            .with("value", value.to_string())),
        };
    }
    if let Ok(one) = value.parse::<IpAddr>() {
        return Ok((one, one));
    }
    if let Ok(addr) = value.parse::<std::net::SocketAddr>() {
        return Err(Error::usage(format!(
            "--block-peer `{value}` carries a port; a block is by address, so write `{}`",
            addr.ip()
        ))
        .with("value", value.to_string()));
    }
    Err(block_error(value))
}

/// Expand a CIDR block into the first and last address it holds.
fn cidr_range(value: &str, base: IpAddr, prefix: u32) -> Result<(IpAddr, IpAddr)> {
    let width = match base {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    };
    if prefix > width {
        return Err(Error::usage(format!(
            "--block-peer `{value}`: a /{prefix} is longer than the {width} bits an address has"
        ))
        .with("value", value.to_string()));
    }
    Ok(match base {
        IpAddr::V4(addr) => {
            // Shifting by the full width is undefined, so a /0 is spelled out
            // rather than computed.
            let mask = match prefix {
                0 => 0,
                _ => u32::MAX << (32 - prefix),
            };
            let start = addr.to_bits() & mask;
            (
                std::net::Ipv4Addr::from_bits(start).into(),
                std::net::Ipv4Addr::from_bits(start | !mask).into(),
            )
        }
        IpAddr::V6(addr) => {
            let mask = match prefix {
                0 => 0,
                _ => u128::MAX << (128 - prefix),
            };
            let start = addr.to_bits() & mask;
            (
                std::net::Ipv6Addr::from_bits(start).into(),
                std::net::Ipv6Addr::from_bits(start | !mask).into(),
            )
        }
    })
}

fn block_error(value: &str) -> Error {
    Error::usage(format!(
        "--block-peer `{value}` is not an address, a START-END range, or a CIDR block"
    ))
    .with("value", value.to_string())
}

pub fn peer_addrs(values: &[String]) -> Result<Vec<std::net::SocketAddr>> {
    use std::net::ToSocketAddrs;

    let mut out = Vec::new();
    for value in values {
        let resolved: Vec<std::net::SocketAddr> = value
            .to_socket_addrs()
            .map_err(|e| {
                Error::usage(format!(
                    "--peer `{value}` is not a reachable HOST:PORT: {e}"
                ))
                .with("value", value.clone())
            })?
            .collect();
        if resolved.is_empty() {
            return Err(
                Error::usage(format!("--peer `{value}` resolved to no address"))
                    .with("value", value.clone()),
            );
        }
        out.extend(resolved);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Where payloads are written for this run.
pub fn download_directory(global: &Global, env: &Env) -> PathBuf {
    match &global.dir {
        Some(dir) => env.resolve(dir),
        None => env.cwd.clone(),
    }
}

/// Build the session options from the global flags and one command's limits.
pub struct SessionSetup<'a> {
    pub global: &'a Global,
    pub trackers: &'a TrackerArgs,
    pub limits: &'a LimitArgs,
    pub web_seeds: &'a WebSeedArgs,
    pub listen_ports: std::ops::RangeInclusive<u16>,
    pub no_dht: bool,
    pub no_lsd: bool,
    /// How space is reserved for each payload file. `seed` and `peers` never
    /// create one, so they leave it at the default.
    pub allocation: bit_cli_core::alloc::Allocation,
}

impl SessionSetup<'_> {
    /// Turn the flags into engine options.
    ///
    /// `--web-seed-only` is the one flag that reaches across subsystems: it
    /// turns off peers, DHT, LSD, and trackers together, because "HTTP only"
    /// with a tracker still running is not HTTP only.
    pub fn engine_options(&self, env: &Env) -> Result<EngineOptions> {
        let http_only = self.web_seeds.web_seed_only;
        Ok(EngineOptions {
            download_directory: download_directory(self.global, env),
            listen_ports: self.listen_ports.clone(),
            // A real run has to be reachable from the swarm, so the listener
            // binds the wildcard address. `--web-seed-only` needs a port only
            // for the loopback bridge, so it stays on loopback: nothing
            // outside the machine has any reason to reach it, and a host
            // firewall has no reason to ask about it.
            listen_ip: http_only.then(|| std::net::Ipv4Addr::LOCALHOST.into()),
            enable_dht: !http_only && !self.no_dht,
            enable_lsd: !http_only && !self.no_lsd,
            enable_trackers: !http_only && !self.trackers.no_tracker,
            enable_peers: !http_only,
            max_peers: self.limits.max_peers,
            // The session caps, which is what "overall" means. The per-torrent
            // pair goes on the add, through [`Self::torrent_rates`]. Before
            // T-181 both pairs aimed here and only one of them arrived, so
            // `--max-download-rate` capped the whole run and
            // `--max-overall-download-rate` capped nothing.
            download_rate: rate_flag(
                &self.limits.max_overall_download_rate,
                "max-overall-download-rate",
            )?,
            upload_rate: rate_flag(
                &self.limits.max_overall_upload_rate,
                "max-overall-upload-rate",
            )?,
            // Peers only. The session cap above reaches an attached HTTP
            // source too, because the source arrives as a peer over loopback;
            // this one skips it. See `TODO/multi-source.md`, T-132.
            peer_download_rate: rate_flag(&self.limits.max_peer_rate, "max-peer-rate")?,
            extra_trackers: Vec::new(),
            ipv4_only: false,
            client_name: Some(format!("bit-cli {}", bit_cli_core::VERSION)),
            allocation: self.allocation,
            max_open_files: self.limits.max_open_files,
            blocked_peers: blocked_ranges(&self.limits.block_peer)?,
            encryption: self.limits.encryption.into(),
            transport: self.limits.transport.into(),
            // Off unless a command turns it on. `bit-cli seed --fastresume`
            // is the only one that does, and it sets this after this call
            // because the default location is derived from where the payload
            // turned out to be. See `TODO/disk-io.md`, T-016.
            resume_cache: None,
        })
    }

    /// The per-torrent rate caps, as `(download, upload)` bytes per second.
    ///
    /// These go on `AddOptions` rather than on the session, which is the whole
    /// difference between `--max-download-rate` and
    /// `--max-overall-download-rate`. Both are parsed here so a command cannot
    /// wire one and forget the other. See `TODO/cli-surface.md`, T-181.
    pub fn torrent_rates(&self) -> Result<(Option<u64>, Option<u64>)> {
        Ok((
            rate_flag(&self.limits.max_download_rate, "max-download-rate")?,
            rate_flag(&self.limits.max_upload_rate, "max-upload-rate")?,
        ))
    }

    /// The tracker list for one torrent, after the runtime edits.
    ///
    /// `--tracker`, `--tracker-file`, and `--tracker-list-url` add;
    /// `--exclude-tracker` removes; `--replace-trackers` drops the torrent's
    /// own list first. The `.torrent` is never rewritten.
    ///
    /// `fetch_list` fetches a `--tracker-list-url`, injected the way
    /// [`crate::webseed_args::collect`] takes its own, so the assembly is
    /// testable without a network and a command that must not reach out passes
    /// [`crate::webseed_args::no_network`]. See `TODO/cli-surface.md`, T-181.
    pub fn tracker_list(
        &self,
        meta: Option<&Metainfo>,
        env: &Env,
        fetch_list: impl Fn(&str) -> Result<String>,
    ) -> Result<Option<Vec<String>>> {
        let args = self.trackers;
        if args.no_tracker || self.web_seeds.web_seed_only {
            return Ok(Some(Vec::new()));
        }
        let mut out: Vec<String> = Vec::new();
        if !args.replace_trackers
            && let Some(meta) = meta
        {
            out.extend(meta.trackers());
        }
        out.extend(args.tracker.iter().cloned());
        for path in &args.tracker_file {
            let path = env.resolve(path);
            let text = std::fs::read_to_string(&path).map_err(|e| {
                bit_cli_core::error::from_io(e, format!("cannot read {}", path.display()))
            })?;
            out.extend(bit_cli_core::webseed::table::parse_url_list(&text));
        }
        // Read with the same parser `--tracker-file` uses, so two flags that
        // read identically in `--help` behave identically. That parser
        // flattens: a blank line does not start a new BEP 12 tier here any
        // more than it does in a file, and announcing in tier order is
        // [T-063](../TODO/trackers.md) rather than this.
        for url in &args.tracker_list_url {
            let text = fetch_list(url)?;
            out.extend(bit_cli_core::webseed::table::parse_url_list(&text));
        }

        let excluded: HashSet<&str> = args.exclude_tracker.iter().map(String::as_str).collect();
        if excluded.contains("*") {
            return Ok(Some(Vec::new()));
        }
        out.retain(|url| !excluded.contains(url.as_str()));

        // Keep declaration order but drop repeats, so a tracker listed in both
        // the torrent and a file is announced to once.
        let mut seen = HashSet::new();
        out.retain(|url| seen.insert(url.clone()));

        match out.is_empty()
            && args.tracker.is_empty()
            && args.tracker_file.is_empty()
            && args.tracker_list_url.is_empty()
        {
            true => Ok(None),
            false => Ok(Some(out)),
        }
    }
}

/// One HTTP source attached to a running torrent.
///
/// A source may be presented over more than one connection, each of which is
/// its own peer to the session and its own bridge here. They share one
/// fetcher, so they share the window cache and the concurrency budget: the
/// point of the second connection is a second receive path, not a second set
/// of requests at the mirror. See `TODO/webseed.md`, T-009.
///
/// The accounting stays one row per source regardless. A caller asked for one
/// mirror and wants to know what that mirror served.
pub struct AttachedSource {
    pub index: usize,
    pub url: String,
    pub origin: &'static str,
    pub scope: String,
    /// Pieces this source can serve on its own.
    pub whole_pieces: usize,
    statuses: Vec<Arc<BridgeStatus>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    /// The one fetcher every connection shares, kept so the run can report
    /// what went over HTTP against what reached the session.
    fetcher: Arc<Fetcher>,
    /// Blocks this source was convicted of serving wrong, in the order they
    /// were resolved. See `TODO/webseed.md`, T-179.
    convictions: std::sync::Mutex<Vec<Conviction>>,
}

impl AttachedSource {
    /// Stop every bridge for this source.
    pub fn stop(&self) {
        for task in &self.tasks {
            task.abort();
        }
    }

    /// Convict this source of serving the wrong bytes for one block.
    ///
    /// The first conviction retires the source; the rest are still recorded,
    /// because a mirror that got six blocks wrong and a mirror that got one
    /// wrong are different mirrors and the report should say which it was.
    fn convict(&self, conviction: Conviction) {
        self.fetcher
            .stats()
            .ban(format!("{} {conviction}", self.url));
        self.convictions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(conviction);
    }

    /// Blocks this source was convicted of serving wrong.
    pub fn convictions(&self) -> Vec<Conviction> {
        self.convictions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Whether this source has been convicted of serving wrong bytes.
    pub fn is_banned(&self) -> bool {
        self.fetcher.stats().banned().is_some()
    }

    /// How many connections this source is presented over.
    pub fn connections(&self) -> usize {
        self.statuses.len()
    }

    /// Bytes handed to the session, across every connection.
    pub fn served_bytes(&self) -> u64 {
        self.statuses.iter().map(|s| s.served_bytes()).sum()
    }

    /// Blocks handed to the session, across every connection.
    pub fn blocks(&self) -> u64 {
        self.statuses.iter().map(|s| s.blocks()).sum()
    }

    /// What the source is doing, taken as the best of its connections.
    ///
    /// One connection failing while another serves is a source that is
    /// serving. A source is failed only when every connection has given up,
    /// which is also when it has nothing left to try. Cooling ranks above
    /// failed and below everything else: it is out now and coming back, so a
    /// caller waiting on it should keep waiting.
    pub fn state(&self) -> BridgeState {
        let rank = |state: BridgeState| match state {
            BridgeState::Active => 4,
            BridgeState::Connecting => 3,
            BridgeState::Idle => 2,
            BridgeState::Cooling => 1,
            BridgeState::Failed => 0,
        };
        self.statuses
            .iter()
            .map(|s| s.state())
            .max_by_key(|state| rank(*state))
            .unwrap_or(BridgeState::Failed)
    }

    /// The first problem any connection reported.
    pub fn error(&self) -> Option<String> {
        self.statuses.iter().find_map(|s| s.error())
    }

    /// Bytes pulled over HTTP, and the requests that pulled them.
    ///
    /// Against [`Self::served_bytes`] this is the amplification: the two are
    /// equal when every byte fetched reached the session, and the first is
    /// larger when the same range was fetched more than once. Sources sharing
    /// a fetcher share its window cache, so a source over four connections
    /// fetches a window once; four separate sources at the same URL fetch it
    /// up to four times.
    pub fn http(&self) -> (u64, u64) {
        let stats = self.fetcher.stats();
        (
            stats.bytes.load(std::sync::atomic::Ordering::Relaxed),
            stats.requests.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Requests retried, and what status each retry was spent on.
    ///
    /// Separate from `error`, which names why a source stopped. These are the
    /// failures it recovered from, which is the only place a status policy
    /// shows its work: with `--web-seed-retry-status 403` a run that would
    /// have died reports the 403s it rode out instead.
    pub fn retries(&self) -> (u64, BTreeMap<u16, u64>) {
        let stats = self.fetcher.stats();
        (
            stats.retries.load(std::sync::atomic::Ordering::Relaxed),
            stats.retries_by_status(),
        )
    }

    /// Every loopback port this source's connections have dialled from.
    pub fn local_ports(&self) -> Vec<u16> {
        self.statuses.iter().flat_map(|s| s.local_ports()).collect()
    }

    /// Reconnects across every connection, the milliseconds spent waiting to
    /// make them, and what ended the attempt before each one.
    ///
    /// Summed rather than maxed: each connection waits on its own backoff, and
    /// what a run lost is what all of them lost. A source that served the
    /// whole payload without a break reports zero, which is what makes a
    /// non-zero number worth reading. See `TODO/performance.md`, T-037.
    pub fn reconnects(&self) -> (u64, u64, BTreeMap<&'static str, u64>) {
        let mut count = 0;
        let mut waited = 0;
        let mut reasons: BTreeMap<&'static str, u64> = BTreeMap::new();
        for status in &self.statuses {
            let (one, ms) = status.reconnects();
            count += one;
            waited += ms;
            for (reason, times) in status.reconnect_reasons() {
                *reasons.entry(reason).or_default() += times;
            }
        }
        (count, waited, reasons)
    }

    /// Files this source turned out not to hold, deduplicated by index.
    ///
    /// Every connection to one source finds the same missing file
    /// independently, because each has its own bitfield to narrow, so the
    /// per-connection lists repeat. A caller asked about a mirror and wants
    /// the mirror's answer, which is the same rule the byte accounting
    /// follows: one row per source however many connections it uses.
    pub fn gone_files(&self) -> Vec<bit_cli_core::webseed::bridge::GoneFile> {
        let mut seen = std::collections::BTreeMap::new();
        for status in &self.statuses {
            for gone in status.gone_files() {
                seen.entry(gone.file).or_insert(gone);
            }
        }
        seen.into_values().collect()
    }

    /// The request pipeline across every connection.
    ///
    /// The depths are summed rather than maxed: each connection is its own
    /// peer with its own request window, and what bounds the source is the
    /// total the session keeps outstanding to it.
    pub fn pipeline(&self) -> bit_cli_core::webseed::bridge::BridgePipeline {
        let mut total = bit_cli_core::webseed::bridge::BridgePipeline::default();
        for status in &self.statuses {
            let one = status.pipeline();
            total.in_flight += one.in_flight;
            total.peak_in_flight += one.peak_in_flight;
            total.requests += one.requests;
            total.blocks += one.blocks;
            total.service_nanos += one.service_nanos;
        }
        total
    }
}

/// What one source looks like in a report.
#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub index: usize,
    pub url: String,
    pub origin: &'static str,
    pub scope: String,
    pub whole_pieces: usize,
    /// Peer connections this source is presented over.
    pub connections: usize,
    pub state: BridgeState,
    pub served_bytes: u64,
    pub served_human: String,
    pub blocks: u64,
    /// Bytes pulled over HTTP, and the requests that pulled them.
    ///
    /// Larger than `served_bytes` when a range was fetched more than once.
    pub http_bytes: u64,
    pub http_requests: u64,
    /// Requests that failed and were tried again.
    pub retries: u64,
    /// Those retries broken down by the HTTP status that caused them, keyed
    /// by the code as a string because JSON object keys are strings.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub retries_by_status: BTreeMap<String, u64>,
    /// How many times the connection to the session ended and was made again,
    /// across every connection this source is presented over.
    ///
    /// A bridge waits between attempts on a delay that doubles from one second
    /// to thirty, and `reconnect_wait_ms` is what that cost. A stalled run and
    /// a slow one look the same in the byte counts and different here. See
    /// `TODO/performance.md`, T-037.
    #[serde(skip_serializing_if = "is_zero")]
    pub reconnects: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub reconnect_wait_ms: u64,
    /// Those reconnects by what ended the attempt before each one:
    /// `disconnected`, `link`, `stalled`, or `cooldown`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub reconnect_reasons: BTreeMap<String, u64>,
    /// Files this source turned out not to hold, with why.
    ///
    /// A permanent failure on one file narrows the source rather than retiring
    /// it, so a mirror serving eleven files of twelve stays a usable mirror.
    /// The byte counts cannot tell that from a mirror that holds all twelve,
    /// which is why the narrowing is reported rather than only logged. See
    /// `TODO/webseed.md`, T-005.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gone_files: Vec<bit_cli_core::webseed::bridge::GoneFile>,
    /// Pieces given up across every file lost. Subtract it from
    /// `whole_pieces` for what the source still announces.
    #[serde(skip_serializing_if = "is_zero")]
    pub pieces_dropped: u64,
    /// How many times this source spent its error budget.
    ///
    /// One with `--web-seed-cooldown` at zero is the source being retired.
    /// More than one means it came back and went out again, which the state
    /// alone cannot say. See `TODO/multi-source.md`, T-137.
    #[serde(skip_serializing_if = "is_zero")]
    pub cooldowns: u64,
    /// When it may be used again, while it is cooling down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown_until: Option<String>,
    /// Milliseconds left of that wait.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown_remaining_ms: Option<u64>,
    /// Blocks this source was proved to have served wrong.
    ///
    /// Proved rather than suspected: each row is a block whose recorded hash
    /// differs from the bytes the session went on to verify, so it names the
    /// mirror that broke the piece and not merely one that contributed to it.
    /// A source with one of these is retired. See `TODO/webseed.md`, T-179.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub convictions: Vec<Conviction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `serde` cannot take `u64::eq` by reference in a `skip_serializing_if`.
fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl AttachedSource {
    /// A snapshot of this source for the report.
    pub fn report(&self) -> SourceReport {
        let served = self.served_bytes();
        let (http_bytes, http_requests) = self.http();
        let (retries, retries_by_status) = self.retries();
        let (reconnects, reconnect_wait_ms, reconnect_reasons) = self.reconnects();
        SourceReport {
            index: self.index,
            url: self.url.clone(),
            origin: self.origin,
            scope: self.scope.clone(),
            whole_pieces: self.whole_pieces,
            connections: self.connections(),
            state: self.state(),
            served_bytes: served,
            served_human: format_size(served),
            blocks: self.blocks(),
            http_bytes,
            http_requests,
            retries,
            retries_by_status: retries_by_status
                .into_iter()
                .map(|(code, count)| (code.to_string(), count))
                .collect(),
            reconnects,
            reconnect_wait_ms,
            reconnect_reasons: reconnect_reasons
                .into_iter()
                .map(|(reason, count)| (reason.to_string(), count))
                .collect(),
            gone_files: self.gone_files(),
            pieces_dropped: self
                .statuses
                .iter()
                .map(|status| status.pieces_dropped())
                .max()
                .unwrap_or(0),
            cooldowns: self.fetcher.stats().cooldowns(),
            cooldown_until: self
                .fetcher
                .stats()
                .cooldown_remaining()
                .and(self.fetcher.stats().cooldown_until())
                .map(|at| at.iso()),
            cooldown_remaining_ms: self
                .fetcher
                .stats()
                .cooldown_remaining()
                .map(|left| left.as_millis().min(u128::from(u64::MAX)) as u64),
            convictions: self.convictions(),
            error: self.error(),
        }
    }
}

/// Attach every declared HTTP source to a running torrent.
///
/// The torrent's metadata has to have resolved first, because the layout is
/// what scopes and URLs are computed against. Nothing here touches the
/// `.torrent`: the sources exist for the length of this invocation only.
#[derive(Debug, Clone, Copy)]
pub struct AttachOptions {
    /// Fail the run when the sources leave a gap.
    pub require: bool,
    /// Whether peers can cover what the sources cannot.
    pub peers_available: bool,
    /// Windows each source caches.
    pub cache_windows: usize,
    /// Keep a request record for `--trace http`.
    pub trace: bool,
    /// When to hash-check what a source serves.
    pub verify: Verify,
}

pub async fn attach_sources(
    engine: &Engine,
    handle: &Handle,
    layout: &Arc<Layout>,
    specs: &[SourceSpec],
    options: &AttachOptions,
) -> Result<(Vec<AttachedSource>, BindingSet)> {
    attach_sources_with(engine, handle, layout, specs, options, None)
        .await
        .map(|(sources, set, _)| (sources, set))
}

/// [`attach_sources`], recording every block served in a shared ledger.
///
/// One ledger for the torrent rather than one per source: which mirror sent a
/// block is a statement none of them can make about itself. The ledger is
/// returned so the caller can resolve it against pieces the session has
/// verified. See `TODO/webseed.md`, T-179.
pub async fn attach_sources_tracked(
    engine: &Engine,
    handle: &Handle,
    layout: &Arc<Layout>,
    specs: &[SourceSpec],
    options: &AttachOptions,
) -> Result<(Vec<AttachedSource>, BindingSet, Arc<BlockLedger>)> {
    let ledger = Arc::new(BlockLedger::new(layout.piece_length));
    attach_sources_with(engine, handle, layout, specs, options, Some(ledger)).await
}

async fn attach_sources_with(
    engine: &Engine,
    handle: &Handle,
    layout: &Arc<Layout>,
    specs: &[SourceSpec],
    options: &AttachOptions,
    ledger: Option<Arc<BlockLedger>>,
) -> Result<(Vec<AttachedSource>, BindingSet, Arc<BlockLedger>)> {
    let AttachOptions {
        require,
        peers_available,
        ..
    } = *options;
    // A ledger exists either way, so the returned type does not change with
    // the caller. Only a tracked attach hands it to the bridges, which is what
    // decides whether anything is recorded: `bench` measures the fetch path
    // and must not pay a hash per block to do it.
    let tracked = ledger.is_some();
    let ledger = ledger.unwrap_or_else(|| Arc::new(BlockLedger::new(layout.piece_length)));
    let info_hash = handle.info_hash().as_string();
    let mut set = BindingSet::resolve(layout, &info_hash, specs)?;
    if require {
        set.require_coverage(peers_available)?;
    }
    if specs.is_empty() {
        return Ok((Vec::new(), set, ledger));
    }
    // Decide the wire style before the first real request. Only a
    // command-line source with `--web-seed-style auto` costs a probe: a
    // metainfo source is keyed by which list it came from, and a declared
    // style is taken as given. Getting this wrong produces a 404 or a
    // wrong-length body from a healthy server, which reads as a broken mirror.
    // See `TODO/webseed.md`, T-004.
    bit_cli_core::webseed::probe::resolve_auto_styles(&mut set, &info_hash).await;
    let set = set;

    let attached = attach_bindings(
        engine,
        handle,
        layout,
        &set.bindings,
        options,
        tracked.then(|| ledger.clone()),
    )?;
    Ok((attached, set, ledger))
}

/// Attach one more source to a torrent that is already running.
///
/// Everything [`attach_sources_tracked`] does for one binding, for a source
/// that did not exist when the run started: a file an earlier torrent in the
/// same invocation has only just finished writing. See
/// `TODO/multi-source.md`, T-143.
///
/// Coverage is not re-checked. `--web-seed-require` asks whether the sources a
/// run declared cover the payload, which was answered when the run started; a
/// source that turns up later can only add.
///
/// `index` is the caller's, not this function's. The ledger is keyed on the
/// source index, so two sources sharing one index would convict each other,
/// and a binding set resolved on its own always numbers from zero.
pub async fn attach_late(
    engine: &Engine,
    handle: &Handle,
    layout: &Arc<Layout>,
    spec: &SourceSpec,
    index: usize,
    options: &AttachOptions,
    ledger: &Arc<BlockLedger>,
) -> Result<AttachedSource> {
    let info_hash = handle.info_hash().as_string();
    let mut set = BindingSet::resolve(layout, &info_hash, std::slice::from_ref(spec))?;
    bit_cli_core::webseed::probe::resolve_auto_styles(&mut set, &info_hash).await;
    let mut binding = set
        .bindings
        .into_iter()
        .next()
        .ok_or_else(|| Error::generic(format!("{}: resolved to no binding", spec.url)))?;
    binding.index = index;
    attach_bindings(
        engine,
        handle,
        layout,
        std::slice::from_ref(&binding),
        options,
        Some(ledger.clone()),
    )?
    .pop()
    .ok_or_else(|| Error::generic(format!("{}: attached no bridge", spec.url)))
}

/// Build the fetcher, the connections and the accounting row for each binding.
///
/// The piece hashes are read once for the whole call rather than once per
/// binding, because reading them copies every hash in the torrent.
fn attach_bindings(
    engine: &Engine,
    handle: &Handle,
    layout: &Arc<Layout>,
    bindings: &[bit_cli_core::webseed::binding::Binding],
    options: &AttachOptions,
    ledger: Option<Arc<BlockLedger>>,
) -> Result<Vec<AttachedSource>> {
    let AttachOptions {
        cache_windows,
        trace,
        verify,
        ..
    } = *options;
    let info_hash = handle.info_hash().as_string();
    let target = engine.bridge_target().ok_or_else(|| {
        Error::network(
            "web seeds need an incoming peer port and none was bound, so no HTTP source can attach",
        )
    })?;
    let session_peer_id = handle.shared().peer_id;
    let piece_hashes = engine.piece_hashes(handle);

    let mut attached = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let limits = &binding.spec.limits;
        let mut params = BridgeParams::for_binding(
            target,
            handle.info_hash(),
            session_peer_id,
            layout,
            binding,
            limits.per_connection_concurrency(),
        );
        if let Some(ledger) = &ledger {
            params = params.with_ledger(ledger.clone());
        }
        let whole_pieces = params.pieces.len();
        // One fetcher for the whole source, however many connections it is
        // presented over. That is what shares the window cache between them
        // and what keeps the concurrency budget a budget: the fetcher's own
        // semaphore is the cap on requests actually in flight at the mirror.
        let fetcher = Arc::new(
            Fetcher::new(
                binding.clone(),
                layout.clone(),
                info_hash.clone(),
                cache_windows,
                trace,
            )?
            .with_verification(verify, piece_hashes.clone()),
        );
        let mut statuses = Vec::with_capacity(limits.connections());
        let mut tasks = Vec::with_capacity(limits.connections());
        for _ in 0..limits.connections() {
            let status = Arc::new(BridgeStatus::default());
            tasks.push(tokio::spawn(bit_cli_core::webseed::bridge::run(
                params.clone(),
                fetcher.clone(),
                status.clone(),
            )));
            statuses.push(status);
        }
        attached.push(AttachedSource {
            index: binding.index,
            url: binding.spec.url.clone(),
            origin: binding.spec.origin.as_str(),
            scope: binding.scope.selector.clone(),
            whole_pieces,
            statuses,
            tasks,
            fetcher,
            convictions: std::sync::Mutex::new(Vec::new()),
        });
    }
    Ok(attached)
}

/// Resolve every disputed piece the session has verified, and retire the
/// sources the verified bytes convict.
///
/// The correct bytes come off the disk, from the piece the session has already
/// hash-checked, so nothing is fetched a second time. Only a block two sources
/// disagreed about is ever read, which in a healthy run is none of them: the
/// whole pass costs one `have` dump and nothing else.
///
/// Returns the convictions made on this pass, so the caller can report them as
/// they happen rather than only at the end.
pub fn resolve_convictions(
    ledger: &BlockLedger,
    sources: &[AttachedSource],
    have: &[bool],
    mut read: impl FnMut(u64, u32) -> Option<Vec<u8>>,
) -> Vec<Conviction> {
    let convictions = ledger.resolve(have, &mut read);
    // Settled pieces are dropped after the disputed ones are resolved rather
    // than before, so a pass never spends its budget forgetting pieces it
    // still needed.
    ledger.forget_settled(have);
    for conviction in &convictions {
        if let Some(source) = sources.iter().find(|s| s.index == conviction.source) {
            source.convict(conviction.clone());
        }
    }
    convictions
}

/// Loopback ports the attached bridges have connected from.
///
/// This is what tells a bridge apart from a real peer in the peer list, so
/// an HTTP source is never counted as a swarm member. It is every port each
/// bridge has used rather than the one it holds now: the session keeps a dead
/// peer's row after the connection closes, and a bridge that reconnected is
/// still not a swarm member under its old port.
pub fn bridge_ports(sources: &[AttachedSource]) -> HashSet<u16> {
    sources
        .iter()
        .flat_map(AttachedSource::local_ports)
        .collect()
}

/// When to stop watching.
#[derive(Debug, Clone, Default)]
pub struct StopConditions {
    /// Overall deadline for the whole operation.
    pub timeout: Option<Duration>,
    /// Stop after this long regardless of state.
    pub stop_after: Option<Duration>,
    /// Give up when nothing has progressed for this long.
    pub stall: Option<Duration>,
    /// Abort when the rate falls below this, in bytes per second.
    pub lowest_rate: Option<u64>,
    /// Stop seeding at this ratio. Zero means do not seed at all.
    pub seed_ratio: Option<f64>,
    /// Stop seeding after this long.
    pub seed_time: Option<Duration>,
    /// Exit after this long with no connected peers.
    pub exit_when_idle: Option<Duration>,
    /// Stop when the process holds more than this many handles.
    ///
    /// A long `seed` run leaks a socket for every peer that connects and
    /// closes before it handshakes, which is upstream and measured in
    /// `TODO/peers.md` under T-020. Nothing here closes those sockets. What
    /// this does is bound them: a supervised deployment gets a loud exit and a
    /// restart instead of a process that quietly runs the machine out of
    /// descriptors.
    pub max_handles: Option<u64>,
    /// Stop when the process holds more than this much resident memory.
    ///
    /// The same shape as [`Self::max_handles`] and for the neighbouring
    /// defect. A seeder grows about 0.8 MiB an hour under load, and
    /// `TODO/memory.md` T-040 attributes most of that to the peer row
    /// `librqbit` keeps for every peer it has ever accepted and never
    /// reclaims: measured at 2,907 bytes a row over 2,000 of them. Nothing
    /// here frees one. What this does is bound the growth.
    pub max_rss: Option<u64>,
    /// Stop when this run's own listener has stopped answering.
    ///
    /// The other half of T-020, and the half no ceiling catches. The same
    /// backlog that strands sockets also holds up every incoming handshake,
    /// so a seeder can accept connections, answer none of them, and go on
    /// reporting a ratio. The state here is written by the probe task that
    /// [`spawn_listener_probe`] starts and read on each tick.
    pub listener: Option<ListenerCheck>,
}

/// Failed probes in a row before the run gives up.
///
/// Derived rather than picked. The session's accept loop clears one queued
/// check per connection it accepts, so N queued checks need N connections to
/// clear, and a probe is one connection. One failure therefore means a
/// backlog of at least one, which a real peer would have cleared for itself.
/// Three in a row means the backlog outlived three connections, so the next
/// three peers to arrive get nothing either. That is an outage, and one
/// failure is not.
pub const LISTENER_FAILURES_ALLOWED: u32 = 3;

/// How the run watches its own listener.
#[derive(Debug, Clone)]
pub struct ListenerCheck {
    /// Written by the probe task, read by the watch loop.
    pub state: Arc<ListenerState>,
    /// Consecutive failures tolerated before [`Stopped::ListenerUnhealthy`].
    pub allowed: u32,
}

/// What the listener probe has found so far.
///
/// Counters rather than a channel: the watch loop samples on its own tick, and
/// a probe that came and went between two ticks is not an event anybody has to
/// see. Only the consecutive count decides anything; the rest is reported.
#[derive(Debug)]
pub struct ListenerState {
    probes: AtomicU64,
    failed: AtomicU64,
    consecutive: AtomicU32,
    /// Round trip of the last answered probe, in milliseconds. `u64::MAX`
    /// until one is answered.
    last_rtt_ms: AtomicU64,
    /// The last probe's failure class, or `None` when it was answered.
    last_failure: Mutex<Option<&'static str>>,
    /// Loopback ports the probe has connected from. Each one is a peer row
    /// the session keeps and never reclaims, so the peer list drops them
    /// rather than counting this process as its own swarm member.
    ports: Mutex<HashSet<u16>>,
}

impl Default for ListenerState {
    /// Written out rather than derived, because zero is a measurement here.
    /// A derived `last_rtt_ms` of 0 reads as a probe answered instantly, and
    /// that is what the first report said before this existed.
    fn default() -> Self {
        Self {
            probes: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            consecutive: AtomicU32::new(0),
            last_rtt_ms: AtomicU64::new(u64::MAX),
            last_failure: Mutex::new(None),
            ports: Mutex::new(HashSet::new()),
        }
    }
}

impl ListenerState {
    /// Fold one probe in.
    pub fn record(&self, probe: &bit_cli_core::listener::Probe) {
        self.probes.fetch_add(1, Ordering::Relaxed);
        if let Some(port) = probe.local_port
            && let Ok(mut ports) = self.ports.lock()
        {
            ports.insert(port);
        }
        if let Ok(mut last) = self.last_failure.lock() {
            *last = probe.failure;
        }
        if probe.healthy {
            self.consecutive.store(0, Ordering::Relaxed);
            if let Some(ms) = probe.rtt_ms {
                self.last_rtt_ms.store(ms, Ordering::Relaxed);
            }
        } else {
            self.failed.fetch_add(1, Ordering::Relaxed);
            self.consecutive.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Failed probes since the last answered one.
    #[must_use]
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive.load(Ordering::Relaxed)
    }

    /// The loopback ports the probe has dialled from.
    #[must_use]
    pub fn ports(&self) -> HashSet<u16> {
        self.ports.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// What the run reports about its own listener.
    #[must_use]
    pub fn report(&self) -> ListenerReport {
        let rtt = self.last_rtt_ms.load(Ordering::Relaxed);
        ListenerReport {
            healthy: self.consecutive.load(Ordering::Relaxed) == 0,
            probes: self.probes.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            consecutive_failures: self.consecutive.load(Ordering::Relaxed),
            last_rtt_ms: (rtt != u64::MAX).then_some(rtt),
            last_failure: self.last_failure.lock().ok().and_then(|f| *f),
        }
    }
}

/// The listener's health, as the report and the event stream carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListenerReport {
    /// False once a probe has failed and no later one has been answered.
    pub healthy: bool,
    /// Probes made.
    pub probes: u64,
    /// Probes that were not answered.
    pub failed: u64,
    /// Failures since the last answered probe.
    pub consecutive_failures: u32,
    /// Round trip of the last answered probe. Null until one is answered.
    ///
    /// Present whatever it holds, unlike the block as a whole. Whether the
    /// listener was watched at all is a question about the run, and a caller
    /// answers it by the presence of `listener`. What the last probe found is
    /// a question about the listener, and a key that comes and goes would
    /// make a consumer write the same null check twice.
    pub last_rtt_ms: Option<u64>,
    /// The last probe's failure class, or null when it was answered.
    pub last_failure: Option<&'static str>,
}

/// The peer list with this run's own listener probe taken out of it.
#[derive(Debug, Default)]
pub struct PeerView {
    /// Everything that is not this process.
    pub rows: Vec<PeerSnapshot>,
    /// Probe rows dropped. One per probe the session answered, because a
    /// probe that was answered became a peer.
    pub dropped: u32,
    /// Of those, the ones connected right now, which is at most the one probe
    /// in flight.
    pub dropped_live: u32,
}

/// Drop the peer rows this run's own listener probe left behind.
///
/// A probe completes a real handshake, so the session records it as a peer and
/// keeps the row after the socket closes: measured at 24 probes, 24 rows, in a
/// terminal state and never reclaimed. It is this process talking to itself.
/// Not a swarm member, not a web seed, and not something a caller should have
/// to filter out of `peer_detail` on every tick.
///
/// The counts come back with the rows because the aggregate the session keeps
/// counts them too, and two decisions are made on that aggregate: `seed` exits
/// 14 when it stopped idle having **seen** no peer, and `--exit-when-idle`
/// measures how long it has had none **live**. A probe every minute would
/// answer both questions wrong.
#[must_use]
pub fn without_probe_rows(rows: Vec<PeerSnapshot>, ports: &HashSet<u16>) -> PeerView {
    if ports.is_empty() {
        return PeerView {
            rows,
            ..Default::default()
        };
    }
    let mut view = PeerView::default();
    for row in rows {
        if bit_cli_core::engine::is_own_loopback_port(&row.addr, ports) {
            view.dropped += 1;
            // `librqbit`'s own name for the state, which is one of "queued",
            // "connecting", "live", "dead" and "not needed". A closed probe
            // settles on "not needed", and none of the reported buckets
            // counts that one.
            if row.state == "live" {
                view.dropped_live += 1;
            }
            continue;
        }
        view.rows.push(row);
    }
    view
}

/// The rows for peers this session still holds, in the order they were in.
///
/// A peer row survives the connection it describes: the session keeps it so
/// that the run's final document can say who ever connected, and `librqbit`
/// settles a closed one on `not needed`. That is history, and history belongs
/// in a document written once rather than in every report tick.
///
/// Measured before this existed, on the operator's six hour soak of
/// 2026-08-24: a `seed --jsonl` tick carried **873** rows, every one of them
/// `not needed`, **zero** of them connected, in a 270 KB object emitted every
/// 30 seconds. The row count plateaus rather than growing, so the cost is a
/// constant 32 MB an hour for as long as the process runs, for a 16 MiB
/// payload. See `TODO/cli-surface.md`, T-258.
///
/// **What is left is what the event's own counts already describe**:
/// `live`, `connecting` and `queued` are the three buckets a session reports,
/// and the two it does not report a bucket for, `dead` and `not needed`, are
/// the two this drops. So the length of a tick's `peer_detail` is
/// `peers.live + peers.connecting + peers.queued` from the same event, which
/// is a cross-check a consumer could not make against the old array.
#[must_use]
pub fn currently_held(rows: &[PeerSnapshot]) -> Vec<&PeerSnapshot> {
    rows.iter()
        .filter(|row| !matches!(row.state.as_str(), "dead" | "not needed"))
        .collect()
}

/// Take this run's own probe out of a snapshot's peer counts.
///
/// The rows are dropped from the reported list by [`without_probe_rows`]; this
/// is the same correction applied to the aggregate the stop conditions read.
pub fn discount_probe_peers(snapshot: &mut TorrentSnapshot, view: &PeerView) {
    snapshot.peers.seen = snapshot.peers.seen.saturating_sub(view.dropped);
    snapshot.peers.live = snapshot.peers.live.saturating_sub(view.dropped_live);
}

/// Probe this run's own listener every `interval`, until the task is dropped.
///
/// `target` is the loopback address of this run's listener, which
/// [`bit_cli_core::engine::Engine::bridge_target`] already works out, and
/// `info_hash` must be one the run is serving. The first probe fires one
/// interval in: a session that has just gone live has nothing wrong with it
/// yet, and a probe during initialisation would measure the hash check.
pub fn spawn_listener_probe(
    target: std::net::SocketAddr,
    info_hash: [u8; 20],
    interval: Duration,
) -> Arc<ListenerState> {
    let state = Arc::new(ListenerState::default());
    let writer = Arc::clone(&state);
    let timeout = bit_cli_core::listener::timeout_for(interval);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let probe = bit_cli_core::listener::probe(target, info_hash, timeout).await;
            writer.record(&probe);
        }
    });
    state
}

/// Why the watch loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stopped {
    /// Every wanted piece is present and verified.
    Completed,
    /// `--seed-ratio` was reached.
    SeedRatio,
    /// `--seed-time` elapsed.
    SeedTime,
    /// `--exit-when-idle` elapsed with no peers.
    Idle,
    /// `--timeout` or `--stop-after` elapsed.
    Deadline,
    /// `--stop-timeout` elapsed with no progress.
    Stalled,
    /// The rate fell below `--lowest-speed-limit`.
    TooSlow,
    /// The user interrupted the run.
    Interrupted,
    /// The torrent failed.
    Failed,
    /// `--max-handles` was exceeded.
    HandleCeiling,
    /// `--max-rss` was exceeded.
    RssCeiling,
    /// `--listener-check` found this run's own listener no longer answering.
    ListenerUnhealthy,
}

impl Stopped {
    /// The stable name used in JSON and text output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::SeedRatio => "seed_ratio",
            Self::SeedTime => "seed_time",
            Self::Idle => "idle",
            Self::Deadline => "deadline",
            Self::Stalled => "stalled",
            Self::TooSlow => "too_slow",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::HandleCeiling => "handle_ceiling",
            Self::RssCeiling => "rss_ceiling",
            Self::ListenerUnhealthy => "listener_unhealthy",
        }
    }

    /// The exit code this reason produces.
    pub const fn code(self) -> bit_cli_core::ExitCode {
        use bit_cli_core::ExitCode as E;
        match self {
            Self::Completed | Self::SeedRatio | Self::SeedTime | Self::Idle => E::Success,
            Self::Deadline | Self::Stalled => E::Timeout,
            Self::TooSlow => E::ThresholdNotMet,
            Self::Interrupted => E::Interrupted,
            Self::Failed => E::Generic,
            // Not a threshold the caller set on the payload, and not a
            // timeout. The run hit a resource ceiling, which is the same
            // shape of failure as running out of them.
            Self::HandleCeiling | Self::RssCeiling => E::ResourceCeiling,
            // Nothing about the process or the port says this, which is why
            // it gets a code rather than folding into `Generic`.
            Self::ListenerUnhealthy => E::ListenerUnhealthy,
        }
    }
}

/// A change the watch loop noticed between two ticks.
///
/// Piece and file completions are derived by comparing consecutive snapshots
/// rather than pushed by the engine, so the report interval bounds how
/// precisely they are timed. The counts are exact; the timestamps are as
/// precise as the interval.
#[derive(Debug, Default)]
pub struct Tick {
    pub verified_pieces: Vec<u32>,
    pub completed_files: Vec<usize>,
}

/// Tracks what has already been reported, so each event fires once.
pub struct Progress {
    have: Vec<bool>,
    files_done: Vec<bool>,
    file_lengths: Vec<u64>,
    best_progress: u64,
    last_progress_at: Instant,
    started: Instant,
    complete_since: Option<Instant>,
    idle_since: Option<Instant>,
}

impl Progress {
    /// Start tracking a torrent with the given file lengths.
    pub fn new(piece_count: u32, file_lengths: Vec<u64>) -> Self {
        let files = file_lengths.len();
        Self {
            have: vec![false; piece_count as usize],
            files_done: vec![false; files],
            file_lengths,
            best_progress: 0,
            last_progress_at: Instant::now(),
            started: Instant::now(),
            complete_since: None,
            idle_since: None,
        }
    }

    /// How long the run has been going.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// How long since the byte count last went up.
    ///
    /// Zero on a run that has just made progress. This is the same clock
    /// `--stop-timeout` reads, so a caller comparing the two is comparing like
    /// with like. See `TODO/peers.md`, T-138.
    pub fn stalled_for(&self) -> Duration {
        self.last_progress_at.elapsed()
    }

    /// Fold one observation in, returning what changed.
    pub fn observe(
        &mut self,
        snapshot: &TorrentSnapshot,
        have: Option<&[bool]>,
        file_progress: &[u64],
    ) -> Tick {
        let mut tick = Tick::default();

        if let Some(have) = have {
            for (index, present) in have.iter().enumerate() {
                if *present && !self.have.get(index).copied().unwrap_or(true) {
                    self.have[index] = true;
                    tick.verified_pieces.push(index as u32);
                }
            }
        }

        for (index, done) in file_progress.iter().enumerate() {
            let length = self.file_lengths.get(index).copied().unwrap_or(0);
            let already = self.files_done.get(index).copied().unwrap_or(true);
            if !already && length > 0 && *done >= length {
                self.files_done[index] = true;
                tick.completed_files.push(index);
            }
        }

        if snapshot.progress_bytes > self.best_progress {
            self.best_progress = snapshot.progress_bytes;
            self.last_progress_at = Instant::now();
        }
        if snapshot.finished && self.complete_since.is_none() {
            self.complete_since = Some(Instant::now());
        }
        match snapshot.peers.live {
            0 => self.idle_since.get_or_insert_with(Instant::now),
            _ => {
                self.idle_since = None;
                &mut self.started
            }
        };

        tick
    }

    /// Whether a stop condition has fired.
    pub fn should_stop(
        &self,
        snapshot: &TorrentSnapshot,
        stop: &StopConditions,
        seeding: bool,
    ) -> Option<Stopped> {
        if snapshot.state == bit_cli_core::engine::State::Error {
            return Some(Stopped::Failed);
        }
        for limit in [stop.timeout, stop.stop_after].into_iter().flatten() {
            if self.started.elapsed() >= limit {
                return Some(Stopped::Deadline);
            }
        }
        if let Some(stall) = stop.stall
            && !snapshot.finished
            && self.last_progress_at.elapsed() >= stall
        {
            return Some(Stopped::Stalled);
        }
        if let Some(floor) = stop.lowest_rate
            && !snapshot.finished
            && self.started.elapsed() >= Duration::from_secs(10)
            && snapshot.download_rate < floor
        {
            return Some(Stopped::TooSlow);
        }
        if let Some(idle) = stop.exit_when_idle
            && let Some(since) = self.idle_since
            && since.elapsed() >= idle
        {
            return Some(Stopped::Idle);
        }
        // Sampled here rather than on its own timer, so the ceilings cost one
        // reading per report interval and nothing between them. One reading
        // for both, because two would report two different instants.
        if stop.max_handles.is_some() || stop.max_rss.is_some() {
            let process = bit_cli_core::sysinfo::Process::sample();
            if stop.max_handles.is_some_and(|c| process.open_handles > c) {
                return Some(Stopped::HandleCeiling);
            }
            if stop.max_rss.is_some_and(|c| process.rss_bytes > c) {
                return Some(Stopped::RssCeiling);
            }
        }
        // Read here rather than acted on by the probe task itself, so the run
        // stops the same way every other stop condition stops it: on a tick,
        // with a reason, through one report.
        if let Some(check) = &stop.listener
            && check.state.consecutive_failures() >= check.allowed
        {
            return Some(Stopped::ListenerUnhealthy);
        }
        if !seeding {
            return snapshot.finished.then_some(Stopped::Completed);
        }
        // Seeding: completion is not a stop condition, the seeding limits are.
        if let Some(ratio) = stop.seed_ratio
            && snapshot.ratio() >= ratio
        {
            return Some(Stopped::SeedRatio);
        }
        if let Some(limit) = stop.seed_time
            && let Some(since) = self.complete_since
            && since.elapsed() >= limit
        {
            return Some(Stopped::SeedTime);
        }
        None
    }
}

/// One line of plain progress, written to stderr.
///
/// Progress is never data, so it never reaches stdout. This is the same
/// information the `progress` event carries, which is the parity requirement:
/// nothing a person can read here is missing from the machine surface.
pub fn progress_line(snapshot: &TorrentSnapshot, sources: &[AttachedSource]) -> String {
    let mut line = format!(
        "{:>6.2}%  {} / {}  down {}  up {}  peers {}",
        snapshot.fraction() * 100.0,
        format_size(snapshot.progress_bytes),
        format_size(snapshot.total_bytes),
        format_rate(snapshot.download_rate),
        format_rate(snapshot.upload_rate),
        snapshot.peers.live,
    );
    let served: u64 = sources.iter().map(AttachedSource::served_bytes).sum();
    if !sources.is_empty() {
        line.push_str(&format!("  http {}", format_size(served)));
    }
    if let Some(eta) = snapshot.eta_ms {
        line.push_str(&format!(
            "  eta {}",
            bit_cli_core::units::format_duration_ms(eta)
        ));
    }
    line
}

/// Peers rendered as an aligned table.
pub fn peer_table(peers: &[PeerSnapshot]) -> Vec<String> {
    let rows: Vec<Vec<String>> = peers
        .iter()
        .map(|p| {
            vec![
                p.addr.clone(),
                p.state.clone(),
                p.direction.to_string(),
                p.connection.clone().unwrap_or_else(|| "-".into()),
                p.client.clone().unwrap_or_else(|| "-".into()),
                format_size(p.downloaded_bytes),
                format_size(p.uploaded_bytes),
                p.verified_pieces.to_string(),
                match p.web_seed {
                    true => "web seed".to_string(),
                    false => "-".to_string(),
                },
            ]
        })
        .collect();
    crate::output::table(
        &[
            "ADDRESS", "STATE", "DIR", "CONN", "CLIENT", "DOWN", "UP", "PIECES", "KIND",
        ],
        &rows,
    )
}

/// Run a hook, passing everything through the environment.
///
/// Nothing torrent-supplied is ever interpolated into a command line. The
/// command is run as written and the facts arrive as `BIT_CLI_*` variables, so
/// a file named `; rm -rf /` is a file name and not a command.
pub fn run_hook(command: &str, vars: &BTreeMap<String, String>) -> Result<i32> {
    let mut cmd = shell_command(command);
    for (key, value) in vars {
        cmd.env(key, value);
    }
    let status = cmd.status().map_err(|e| {
        bit_cli_core::error::from_io(e, format!("hook `{command}` could not be run"))
    })?;
    Ok(status.code().unwrap_or(-1))
}

/// `sh -c <command>`, and the argument reaches `sh` as written.
#[cfg(not(windows))]
fn shell_command(command: &str) -> std::process::Command {
    let mut c = std::process::Command::new("sh");
    c.arg("-c").arg(command);
    c
}

/// `cmd /C <command>`, and the argument reaches `cmd` as written.
///
/// **`raw_arg`, not `arg`.** Rust quotes an argument for the C runtime's own
/// parser before handing it to `CreateProcess`, and `cmd.exe` does not use that
/// parser: it re-reads the command line with rules of its own. A hook whose
/// command contains a quoted path, a redirect, or an `&&` therefore reached
/// `cmd` mangled, and the process exited with "The filename, directory name, or
/// volume label syntax is incorrect" rather than doing what it was told.
///
/// Found by T-115's own acceptance, whose hook is
/// `mkdir "<dir>\%BIT_CLI_HOOK%-%BIT_CLI_INFO_HASH%"`: the hook fired twice,
/// as the entry asked, and failed twice. `raw_arg` hands the string to `cmd`
/// unchanged, which is what a shell command needs and what `sh -c` has always
/// done on the other platform.
#[cfg(windows)]
fn shell_command(command: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("cmd");
    c.arg("/C");
    c.raw_arg(command);
    c
}

/// The environment a hook receives.
pub fn hook_vars(
    snapshot: &TorrentSnapshot,
    directory: &std::path::Path,
) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    vars.insert("BIT_CLI_VERSION".into(), bit_cli_core::VERSION.to_string());
    vars.insert("BIT_CLI_INFO_HASH".into(), snapshot.info_hash.clone());
    vars.insert("BIT_CLI_NAME".into(), snapshot.name.clone());
    vars.insert("BIT_CLI_DIR".into(), directory.display().to_string());
    vars.insert(
        "BIT_CLI_TOTAL_BYTES".into(),
        snapshot.total_bytes.to_string(),
    );
    vars.insert(
        "BIT_CLI_PROGRESS_BYTES".into(),
        snapshot.progress_bytes.to_string(),
    );
    vars.insert(
        "BIT_CLI_UPLOADED_BYTES".into(),
        snapshot.uploaded_bytes.to_string(),
    );
    vars.insert("BIT_CLI_STATE".into(), snapshot.state.as_str().to_string());
    vars.insert("BIT_CLI_FINISHED".into(), snapshot.finished.to_string());
    vars
}

/// Report a warning about a source that failed, once per source.
pub fn report_failed_sources(
    sources: &[AttachedSource],
    reported: &mut HashSet<usize>,
    renderer: &Renderer,
    env: &mut Env,
) -> Vec<SourceReport> {
    let mut newly_failed = Vec::new();
    for source in sources {
        if source.state() != BridgeState::Failed || !reported.insert(source.index) {
            continue;
        }
        let report = source.report();
        renderer.warn(
            env,
            format!(
                "web seed {} is unusable: {}",
                report.url,
                report.error.as_deref().unwrap_or("no reason given")
            ),
        );
        newly_failed.push(report);
    }
    newly_failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use bit_cli_core::ExitCode;
    use bit_cli_core::engine::{PeerCounts, State};

    fn blocked(values: &[&str]) -> Vec<(IpAddr, IpAddr)> {
        blocked_ranges(&values.iter().map(ToString::to_string).collect::<Vec<_>>()).unwrap()
    }

    fn blocked_err(value: &str) -> Error {
        blocked_ranges(&[value.to_string()]).unwrap_err()
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("an address")
    }

    /// The three spellings a blocklist entry is written in, in both families.
    /// See `TODO/peers.md`, T-164.
    #[test]
    fn an_address_a_range_and_a_cidr_all_resolve_to_the_same_shape() {
        assert_eq!(
            blocked(&["203.0.113.5"]),
            [(ip("203.0.113.5"), ip("203.0.113.5"))]
        );
        assert_eq!(
            blocked(&["203.0.113.0-203.0.113.255"]),
            [(ip("203.0.113.0"), ip("203.0.113.255"))]
        );
        assert_eq!(
            blocked(&["203.0.113.0/24"]),
            [(ip("203.0.113.0"), ip("203.0.113.255"))]
        );
        // A CIDR is expanded from its block, not from the address given, so a
        // host address inside one still names the whole block.
        assert_eq!(
            blocked(&["203.0.113.77/24"]),
            [(ip("203.0.113.0"), ip("203.0.113.255"))]
        );
        assert_eq!(
            blocked(&["2001:db8::1"]),
            [(ip("2001:db8::1"), ip("2001:db8::1"))]
        );
        assert_eq!(
            blocked(&["2001:db8::-2001:db8::ffff"]),
            [(ip("2001:db8::"), ip("2001:db8::ffff"))]
        );
        assert_eq!(
            blocked(&["2001:db8::/112"]),
            [(ip("2001:db8::"), ip("2001:db8::ffff"))]
        );
    }

    /// The two ends of the prefix range, because both are computed by a shift
    /// and one of them would be undefined if it were.
    #[test]
    fn the_widest_and_narrowest_cidr_blocks_are_both_exact() {
        assert_eq!(
            blocked(&["0.0.0.0/0"]),
            [(ip("0.0.0.0"), ip("255.255.255.255"))]
        );
        assert_eq!(
            blocked(&["203.0.113.5/32"]),
            [(ip("203.0.113.5"), ip("203.0.113.5"))]
        );
        assert_eq!(
            blocked(&["::/0"]),
            [(ip("::"), ip("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"))]
        );
        assert_eq!(
            blocked(&["2001:db8::1/128"]),
            [(ip("2001:db8::1"), ip("2001:db8::1"))]
        );
    }

    /// A port is refused rather than dropped. Blocking is by address, so
    /// taking `127.0.0.1:6881` and blocking `127.0.0.1` would block every port
    /// on that host without saying so.
    #[test]
    fn an_address_with_a_port_is_refused_and_says_what_to_write_instead() {
        let err = blocked_err("127.0.0.1:6881");
        assert_eq!(err.code(), ExitCode::Usage);
        assert!(err.message().contains("127.0.0.1"), "{}", err.message());
        assert!(err.message().contains("port"), "{}", err.message());
    }

    #[test]
    fn a_range_that_is_backwards_or_mixes_families_is_refused() {
        let err = blocked_err("203.0.113.9-203.0.113.1");
        assert_eq!(err.code(), ExitCode::Usage);
        assert!(err.message().contains("backwards"), "{}", err.message());

        let err = blocked_err("203.0.113.1-2001:db8::1");
        assert!(err.message().contains("IPv4 and IPv6"), "{}", err.message());

        let err = blocked_err("203.0.113.0/40");
        assert!(err.message().contains("32 bits"), "{}", err.message());

        assert_eq!(blocked_err("not-an-address").code(), ExitCode::Usage);
    }

    /// Nothing is resolved. A name would block whatever it pointed at when the
    /// run started, which is not what blocking an address means.
    #[test]
    fn a_hostname_is_refused_rather_than_resolved() {
        let err = blocked_err("localhost");
        assert_eq!(err.code(), ExitCode::Usage);
    }

    fn snapshot(progress: u64, total: u64) -> TorrentSnapshot {
        TorrentSnapshot {
            id: 0,
            info_hash: "0".repeat(40),
            name: "t".into(),
            state: State::Live,
            total_bytes: total,
            progress_bytes: progress,
            uploaded_bytes: 0,
            finished: progress >= total && total > 0,
            download_rate: 0,
            upload_rate: 0,
            eta_ms: None,
            eta_confidence: "none",
            peers: PeerCounts::default(),
            error: None,
        }
    }

    /// Build a `SessionSetup` from a `download` command line.
    ///
    /// The whole point of these is the flags, so they are parsed rather than
    /// hand-built: a struct filled in by hand cannot catch a flag that stopped
    /// reaching the struct.
    fn setup_from(argv: &[&str]) -> (crate::cli::DownloadArgs, crate::cli::Global) {
        use clap::Parser;
        let mut full = vec!["bit-cli", "download"];
        full.extend_from_slice(argv);
        full.push("x.torrent");
        let cli = crate::cli::Cli::try_parse_from(full).expect("parse");
        let Some(crate::cli::Command::Download(args)) = cli.command else {
            panic!("expected download");
        };
        (args, cli.global)
    }

    fn session_setup<'a>(
        args: &'a crate::cli::DownloadArgs,
        global: &'a crate::cli::Global,
    ) -> SessionSetup<'a> {
        SessionSetup {
            global,
            trackers: &args.trackers,
            limits: &args.limits,
            web_seeds: &args.web_seeds,
            listen_ports: 6881..=6889,
            no_dht: args.no_dht,
            no_lsd: args.no_lsd,
            allocation: bit_cli_core::alloc::Allocation::default(),
        }
    }

    /// `TODO/cli-surface.md` T-181. `--tracker-list-url` fetched three
    /// trackers and every one of them is announced to.
    #[test]
    fn a_tracker_list_url_contributes_every_tracker_it_names() {
        let (args, global) = setup_from(&["--tracker-list-url", "https://e.com/trackers.txt"]);
        let setup = session_setup(&args, &global);
        let env = Env::test(&[], "/work").0;
        let trackers = setup
            .tracker_list(None, &env, |url| {
                assert_eq!(url, "https://e.com/trackers.txt");
                Ok("# mirrors
udp://a.example:80

udp://b.example:80
udp://c.example:80
"
                .to_string())
            })
            .unwrap()
            .expect("a list");
        assert_eq!(
            trackers,
            vec![
                "udp://a.example:80".to_string(),
                "udp://b.example:80".to_string(),
                "udp://c.example:80".to_string(),
            ],
            "comments and blank lines are dropped, the rest are announced to"
        );
    }

    /// A list URL and the two flags beside it compose, and a tracker named
    /// twice is announced to once.
    #[test]
    fn a_tracker_list_url_composes_with_the_flags_beside_it() {
        let (args, global) = setup_from(&[
            "--tracker",
            "udp://cli.example:80",
            "--tracker-list-url",
            "https://e.com/trackers.txt",
            "--exclude-tracker",
            "udp://b.example:80",
        ]);
        let setup = session_setup(&args, &global);
        let env = Env::test(&[], "/work").0;
        let trackers = setup
            .tracker_list(None, &env, |_| {
                Ok("udp://a.example:80
udp://b.example:80
udp://cli.example:80
"
                .to_string())
            })
            .unwrap()
            .expect("a list");
        assert_eq!(
            trackers,
            vec![
                "udp://cli.example:80".to_string(),
                "udp://a.example:80".to_string(),
            ],
            "--tracker first, the excluded one gone, and the repeat dropped"
        );
    }

    /// A command that must not reach the network says so rather than fetching.
    #[test]
    fn a_tracker_list_url_on_a_no_network_command_fails_clearly() {
        let (args, global) = setup_from(&["--tracker-list-url", "https://e.com/trackers.txt"]);
        let setup = session_setup(&args, &global);
        let env = Env::test(&[], "/work").0;
        let err = setup
            .tracker_list(None, &env, crate::webseed_args::no_network)
            .unwrap_err();
        assert_eq!(err.code(), bit_cli_core::ExitCode::Usage);
        assert!(
            err.message().contains("needs the network"),
            "{}",
            err.message()
        );
    }

    /// `TODO/cli-surface.md` T-181. The two rate scopes are two different
    /// `librqbit` fields, and this is the test that says which is which.
    ///
    /// `--max-overall-*` is the session cap and reaches `EngineOptions`.
    /// `--max-*` is the per-torrent cap and reaches the add. Before T-181 both
    /// aimed at the session field, so capping one torrent capped the whole run
    /// and capping the whole run did nothing at all.
    #[test]
    fn the_overall_rate_caps_the_session_and_the_plain_one_caps_a_torrent() {
        let (args, global) = setup_from(&[
            "--max-download-rate",
            "1MiB/s",
            "--max-upload-rate",
            "2MiB/s",
            "--max-overall-download-rate",
            "4MiB/s",
            "--max-overall-upload-rate",
            "8MiB/s",
        ]);
        let setup = session_setup(&args, &global);
        let env = Env::test(&[], "/work").0;

        let options = setup.engine_options(&env).unwrap();
        assert_eq!(options.download_rate, Some(4 * 1024 * 1024));
        assert_eq!(options.upload_rate, Some(8 * 1024 * 1024));

        let (down, up) = setup.torrent_rates().unwrap();
        assert_eq!(down, Some(1024 * 1024));
        assert_eq!(up, Some(2 * 1024 * 1024));
    }

    /// Neither scope is filled in from the other. A run that caps only one
    /// torrent has no session cap, and a run that caps the session has no
    /// per-torrent one.
    #[test]
    fn one_rate_scope_never_stands_in_for_the_other() {
        let (args, global) = setup_from(&["--max-download-rate", "1MiB/s"]);
        let setup = session_setup(&args, &global);
        let env = Env::test(&[], "/work").0;
        assert_eq!(setup.engine_options(&env).unwrap().download_rate, None);
        assert_eq!(setup.torrent_rates().unwrap().0, Some(1024 * 1024));

        let (args, global) = setup_from(&["--max-overall-download-rate", "4MiB/s"]);
        let setup = session_setup(&args, &global);
        assert_eq!(
            setup.engine_options(&env).unwrap().download_rate,
            Some(4 * 1024 * 1024)
        );
        assert_eq!(setup.torrent_rates().unwrap().0, None);
    }

    #[test]
    fn a_bare_port_and_a_range_both_parse() {
        assert_eq!(port_range(&[]).unwrap(), 6881..=6889);
        assert_eq!(port_range(&["51413".into()]).unwrap(), 51413..=51413);
        assert_eq!(port_range(&["6881-6891".into()]).unwrap(), 6881..=6891);
    }

    #[test]
    fn several_port_flags_widen_the_range() {
        let range = port_range(&["7000".into(), "6881-6889".into()]).unwrap();
        assert_eq!(range, 6881..=7000);
    }

    #[test]
    fn a_backwards_or_malformed_port_range_is_a_usage_error() {
        for bad in ["6891-6881", "notaport", "6881-", "-6889"] {
            let err = port_range(&[bad.to_string()]).unwrap_err();
            assert_eq!(err.code(), bit_cli_core::ExitCode::Usage, "{bad}");
        }
    }

    #[test]
    fn a_piece_is_only_reported_verified_once() {
        let mut progress = Progress::new(4, vec![1024]);
        let snap = snapshot(2048, 4096);

        let first = progress.observe(&snap, Some(&[true, true, false, false]), &[]);
        assert_eq!(first.verified_pieces, vec![0, 1]);

        let second = progress.observe(&snap, Some(&[true, true, true, false]), &[]);
        assert_eq!(
            second.verified_pieces,
            vec![2],
            "already-known pieces do not repeat"
        );
    }

    #[test]
    fn a_file_completes_once_and_a_zero_length_file_never_does() {
        let mut progress = Progress::new(4, vec![100, 0, 50]);
        let snap = snapshot(150, 150);

        let first = progress.observe(&snap, None, &[100, 0, 20]);
        assert_eq!(first.completed_files, vec![0]);

        let second = progress.observe(&snap, None, &[100, 0, 50]);
        assert_eq!(
            second.completed_files,
            vec![2],
            "a zero-length file never completes"
        );

        let third = progress.observe(&snap, None, &[100, 0, 50]);
        assert!(third.completed_files.is_empty());
    }

    #[test]
    fn completion_stops_a_download_but_not_a_seed() {
        let mut progress = Progress::new(1, vec![10]);
        let done = snapshot(10, 10);
        progress.observe(&done, None, &[10]);

        let stop = StopConditions::default();
        assert_eq!(
            progress.should_stop(&done, &stop, false),
            Some(Stopped::Completed)
        );
        assert_eq!(
            progress.should_stop(&done, &stop, true),
            None,
            "a seed keeps going"
        );
    }

    #[test]
    fn a_failed_torrent_stops_before_any_other_condition() {
        let progress = Progress::new(1, vec![10]);
        let mut snap = snapshot(0, 10);
        snap.state = State::Error;
        let stop = StopConditions {
            timeout: Some(Duration::ZERO),
            ..Default::default()
        };
        assert_eq!(
            progress.should_stop(&snap, &stop, false),
            Some(Stopped::Failed)
        );
    }

    #[test]
    fn a_deadline_that_has_already_passed_stops_immediately() {
        let progress = Progress::new(1, vec![10]);
        let snap = snapshot(0, 10);
        let stop = StopConditions {
            timeout: Some(Duration::ZERO),
            ..Default::default()
        };
        assert_eq!(
            progress.should_stop(&snap, &stop, false),
            Some(Stopped::Deadline)
        );
    }

    #[test]
    fn a_seed_ratio_stops_a_seed() {
        let mut progress = Progress::new(1, vec![10]);
        let mut snap = snapshot(10, 10);
        snap.uploaded_bytes = 25;
        progress.observe(&snap, None, &[10]);

        let stop = StopConditions {
            seed_ratio: Some(2.0),
            ..Default::default()
        };
        assert_eq!(
            progress.should_stop(&snap, &stop, true),
            Some(Stopped::SeedRatio)
        );

        let stop = StopConditions {
            seed_ratio: Some(3.0),
            ..Default::default()
        };
        assert_eq!(progress.should_stop(&snap, &stop, true), None);
    }

    #[test]
    fn stop_reasons_map_to_distinct_exit_codes() {
        use bit_cli_core::ExitCode as E;
        assert_eq!(Stopped::Completed.code(), E::Success);
        assert_eq!(Stopped::Deadline.code(), E::Timeout);
        assert_eq!(Stopped::Stalled.code(), E::Timeout);
        assert_eq!(Stopped::TooSlow.code(), E::ThresholdNotMet);
        assert_eq!(Stopped::Interrupted.code(), E::Interrupted);
        assert_eq!(Stopped::HandleCeiling.code(), E::ResourceCeiling);
        // A finished seed exits zero however it was told to stop, because
        // being told to stop is not a failure.
        for reason in [Stopped::SeedRatio, Stopped::SeedTime, Stopped::Idle] {
            assert_eq!(reason.code(), E::Success, "{reason:?}");
        }
    }

    #[test]
    fn every_stop_reason_has_a_stable_name() {
        for reason in [
            Stopped::Completed,
            Stopped::SeedRatio,
            Stopped::SeedTime,
            Stopped::Idle,
            Stopped::Deadline,
            Stopped::Stalled,
            Stopped::TooSlow,
            Stopped::Interrupted,
            Stopped::Failed,
            Stopped::HandleCeiling,
        ] {
            let name = reason.as_str();
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{name}"
            );
        }
    }

    #[test]
    fn a_progress_line_carries_every_number_the_event_does() {
        let mut snap = snapshot(512, 2048);
        snap.download_rate = 1024 * 1024;
        snap.peers.live = 7;
        let line = progress_line(&snap, &[]);
        assert!(line.contains("25.00%"), "{line}");
        assert!(line.contains("512 B"), "{line}");
        assert!(
            line.contains("2.00 MiB/s") || line.contains("1.00 MiB/s"),
            "{line}"
        );
        assert!(line.contains("peers 7"), "{line}");
    }

    #[test]
    fn hook_variables_never_carry_a_shell_string() {
        let snap = snapshot(1, 2);
        let vars = hook_vars(&snap, std::path::Path::new("/data"));
        assert_eq!(vars["BIT_CLI_INFO_HASH"], snap.info_hash);
        assert_eq!(vars["BIT_CLI_TOTAL_BYTES"], "2");
        assert_eq!(vars["BIT_CLI_DIR"], "/data");
        for key in vars.keys() {
            assert!(key.starts_with("BIT_CLI_"), "{key}");
        }
    }

    #[test]
    fn a_handle_ceiling_the_process_is_already_over_stops_the_run() {
        // Zero is under any real process's handle count, so this fires on the
        // first sample without having to leak anything to get there.
        let progress = Progress::new(1, vec![10]);
        let snap = snapshot(0, 10);
        let stop = StopConditions {
            max_handles: Some(0),
            ..Default::default()
        };
        assert_eq!(
            progress.should_stop(&snap, &stop, false),
            Some(Stopped::HandleCeiling)
        );
    }

    #[test]
    fn a_handle_ceiling_nothing_is_near_does_not_stop_the_run() {
        let progress = Progress::new(1, vec![10]);
        let snap = snapshot(0, 10);
        let stop = StopConditions {
            max_handles: Some(u64::MAX),
            ..Default::default()
        };
        assert_eq!(progress.should_stop(&snap, &stop, false), None);
    }

    #[test]
    fn an_rss_ceiling_the_process_is_already_over_stops_the_run() {
        let progress = Progress::new(1, vec![10]);
        let stop = StopConditions {
            max_rss: Some(0),
            ..Default::default()
        };
        assert_eq!(
            progress.should_stop(&snapshot(0, 10), &stop, false),
            Some(Stopped::RssCeiling)
        );
    }

    #[test]
    fn an_rss_ceiling_nothing_is_near_does_not_stop_the_run() {
        let progress = Progress::new(1, vec![10]);
        let stop = StopConditions {
            max_rss: Some(u64::MAX),
            ..Default::default()
        };
        assert_eq!(progress.should_stop(&snapshot(0, 10), &stop, false), None);
    }

    #[test]
    fn the_handle_ceiling_is_reported_before_the_memory_one() {
        // Both from one sample, and both crossed. Handles come first because
        // a process out of descriptors has already stopped working, where one
        // over a memory line is still serving.
        let progress = Progress::new(1, vec![10]);
        let stop = StopConditions {
            max_handles: Some(0),
            max_rss: Some(0),
            ..Default::default()
        };
        assert_eq!(
            progress.should_stop(&snapshot(0, 10), &stop, false),
            Some(Stopped::HandleCeiling)
        );
    }

    #[test]
    fn no_handle_ceiling_means_the_process_is_never_sampled_against_one() {
        let progress = Progress::new(1, vec![10]);
        let snap = snapshot(0, 10);
        assert_eq!(
            progress.should_stop(&snap, &StopConditions::default(), false),
            None
        );
    }

    fn answered(port: u16) -> bit_cli_core::listener::Probe {
        bit_cli_core::listener::Probe {
            healthy: true,
            rtt_ms: Some(1),
            local_port: Some(port),
            failure: None,
        }
    }

    fn unanswered(port: u16) -> bit_cli_core::listener::Probe {
        bit_cli_core::listener::Probe {
            healthy: false,
            rtt_ms: None,
            local_port: Some(port),
            failure: Some("handshake_timeout"),
        }
    }

    fn listener_stop(state: &Arc<ListenerState>) -> StopConditions {
        StopConditions {
            listener: Some(ListenerCheck {
                state: Arc::clone(state),
                allowed: LISTENER_FAILURES_ALLOWED,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn one_unanswered_probe_does_not_stop_a_seeder() {
        // A backlog of one is one a real peer clears for itself by arriving.
        let state = Arc::new(ListenerState::default());
        state.record(&unanswered(40001));
        let progress = Progress::new(1, vec![10]);
        assert_eq!(
            progress.should_stop(&snapshot(0, 10), &listener_stop(&state), true),
            None
        );
    }

    #[test]
    fn three_unanswered_probes_in_a_row_stop_the_run() {
        let state = Arc::new(ListenerState::default());
        for port in 40001..40004 {
            state.record(&unanswered(port));
        }
        let progress = Progress::new(1, vec![10]);
        assert_eq!(
            progress.should_stop(&snapshot(0, 10), &listener_stop(&state), true),
            Some(Stopped::ListenerUnhealthy)
        );
    }

    #[test]
    fn an_answered_probe_clears_the_run_of_failures_before_it() {
        // Not a count of failures, a count since the last answer: a listener
        // that answered a moment ago is one a peer can reach now.
        let state = Arc::new(ListenerState::default());
        state.record(&unanswered(40001));
        state.record(&unanswered(40002));
        state.record(&answered(40003));
        state.record(&unanswered(40004));
        assert_eq!(state.consecutive_failures(), 1);
        let progress = Progress::new(1, vec![10]);
        assert_eq!(
            progress.should_stop(&snapshot(0, 10), &listener_stop(&state), true),
            None
        );
        let report = state.report();
        assert_eq!(report.probes, 4);
        assert_eq!(report.failed, 3);
        assert!(!report.healthy);
        assert_eq!(report.last_rtt_ms, Some(1));
        assert_eq!(report.last_failure, Some("handshake_timeout"));
    }

    #[test]
    fn a_listener_nothing_has_probed_yet_reports_no_round_trip() {
        // Zero is a measurement: a report that says the last probe came back
        // in 0 ms when none has been made is worse than one that says none
        // has. The derived `Default` said exactly that.
        let report = ListenerState::default().report();
        assert_eq!(report.probes, 0);
        assert_eq!(report.last_rtt_ms, None);
        assert_eq!(report.last_failure, None);
        assert!(report.healthy, "nothing has failed yet");
    }

    #[test]
    fn a_run_with_no_listener_check_is_never_stopped_for_one() {
        let progress = Progress::new(1, vec![10]);
        assert_eq!(
            progress.should_stop(&snapshot(0, 10), &StopConditions::default(), true),
            None
        );
    }

    #[test]
    fn the_probes_own_peer_rows_are_dropped_and_nothing_else_is() {
        let state = Arc::new(ListenerState::default());
        state.record(&answered(40001));
        state.record(&unanswered(40002));
        let closed = |addr: &str| {
            let mut row = peer_row(addr);
            row.state = "not needed".into();
            row
        };
        let rows = vec![
            closed("127.0.0.1:40001"),
            closed("[::1]:40002"),
            peer_row("127.0.0.1:40003"),
            peer_row("203.0.113.7:40001"),
        ];
        let kept = without_probe_rows(rows, &state.ports());
        let addrs: Vec<&str> = kept.rows.iter().map(|r| r.addr.as_str()).collect();
        // The dial that failed still connected, so it left a row too, and a
        // real peer that happens to share a port number with one of ours does
        // not.
        assert_eq!(addrs, vec!["127.0.0.1:40003", "203.0.113.7:40001"]);
        assert_eq!(kept.dropped, 2);
        assert_eq!(kept.dropped_live, 0, "both rows are closed");
    }

    #[test]
    fn the_probes_own_peers_come_out_of_the_counts_the_run_decides_on() {
        // `seed` exits 14 when it stopped idle having seen no peer, and
        // `--exit-when-idle` measures how long it has had none live. A probe
        // a minute would answer both wrong: it is seen, and for the moment it
        // is connected it is live.
        let state = Arc::new(ListenerState::default());
        state.record(&answered(40001));
        state.record(&answered(40002));
        let mut closed = peer_row("127.0.0.1:40001");
        closed.state = "not needed".into();
        let mut in_flight = peer_row("127.0.0.1:40002");
        in_flight.state = "live".into();
        let view = without_probe_rows(vec![closed, in_flight], &state.ports());
        assert_eq!((view.dropped, view.dropped_live), (2, 1));

        let mut snap = snapshot(0, 10);
        snap.peers.seen = 3;
        snap.peers.live = 1;
        discount_probe_peers(&mut snap, &view);
        assert_eq!(snap.peers.seen, 1, "one real peer was seen, not three");
        assert_eq!(snap.peers.live, 0, "the only live peer was ours");
    }

    #[test]
    fn discounting_more_probes_than_the_session_counted_floors_at_zero() {
        // The two numbers come from different places, so nothing guarantees
        // the session's own count is the larger one on any given tick.
        let view = PeerView {
            rows: Vec::new(),
            dropped: 9,
            dropped_live: 9,
        };
        let mut snap = snapshot(0, 10);
        snap.peers.seen = 1;
        snap.peers.live = 1;
        discount_probe_peers(&mut snap, &view);
        assert_eq!((snap.peers.seen, snap.peers.live), (0, 0));
    }

    #[test]
    fn no_probe_means_the_peer_list_is_returned_untouched() {
        let rows = vec![peer_row("127.0.0.1:40001")];
        let kept = without_probe_rows(rows, &HashSet::new());
        assert_eq!(kept.rows.len(), 1);
        assert_eq!(kept.dropped, 0);
    }

    /// A report tick carries the peers this session holds, not its history.
    ///
    /// The operator's six hour soak of 2026-08-24 sent 873 rows every 30
    /// seconds, every one of them `not needed` and none of them connected, in
    /// a 270 KB object. The two terminal states are the two the session
    /// reports no bucket for, so what is left is exactly
    /// `peers.live + peers.connecting + peers.queued` from the same event. See
    /// `TODO/cli-surface.md`, T-258.
    #[test]
    fn a_tick_carries_the_peers_a_session_holds_and_not_the_ones_it_has_lost() {
        let mut rows = Vec::new();
        for (addr, state) in [
            ("10.0.0.1:1", "live"),
            ("10.0.0.2:2", "connecting"),
            ("10.0.0.3:3", "queued"),
            ("10.0.0.4:4", "dead"),
            ("10.0.0.5:5", "not needed"),
        ] {
            let mut row = peer_row(addr);
            row.state = state.into();
            rows.push(row);
        }

        let held = currently_held(&rows);
        let addrs: Vec<&str> = held.iter().map(|row| row.addr.as_str()).collect();
        assert_eq!(
            addrs,
            vec!["10.0.0.1:1", "10.0.0.2:2", "10.0.0.3:3"],
            "the three reported buckets survive and the two terminal states do not"
        );
        assert_eq!(rows.len(), 5, "the rows themselves are untouched");
    }

    /// A run that has lost every peer sends an empty array rather than its
    /// whole history, which is the shape the soak was in at its last sample.
    #[test]
    fn a_tick_with_nothing_connected_carries_nothing() {
        let mut rows = Vec::new();
        for addr in ["10.0.0.1:1", "10.0.0.2:2"] {
            let mut row = peer_row(addr);
            row.state = "not needed".into();
            rows.push(row);
        }
        assert!(currently_held(&rows).is_empty());
    }

    fn peer_row(addr: &str) -> PeerSnapshot {
        PeerSnapshot {
            addr: addr.to_string(),
            state: "live".into(),
            client: None,
            connection: None,
            encryption: None,
            choked: 0,
            unchoked: 0,
            disconnects: Vec::new(),
            direction: "incoming",
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            verified_pieces: 0,
            chunks: 0,
            errors: 0,
            connect_ms: 0,
            mean_piece_ms: None,
            web_seed: false,
        }
    }
}
