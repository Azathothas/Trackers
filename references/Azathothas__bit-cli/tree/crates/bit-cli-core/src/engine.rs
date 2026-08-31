//! The torrent engine: a `librqbit` session wrapped for one-shot commands.
//!
//! Every `bit-cli` verb runs in the foreground, does its work, and exits.
//! There is no daemon and no stored session, so the engine owns a session for
//! the length of one invocation and nothing outlives the process. That is what
//! keeps this module small: no persistence, no restore, no id stability across
//! runs.
//!
//! Everything `librqbit` hands back is translated into the plain types in this
//! module before it leaves. A command never sees a `librqbit` type, which is
//! what lets the rendering layer be written once and stay stable if the engine
//! underneath changes.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use librqbit::api::TorrentIdOrHash;
use librqbit::http_api_types::{PeerStatsFilter, PeerStatsFilterState};
use librqbit::limits::LimitsConfig;
use librqbit::storage::StorageFactoryExt;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, DhtSessionConfig, ListenerOptions,
    ManagedTorrent, Session, SessionOptions, TorrentStats, TorrentStatsState,
};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::paths::PathPlan;
use crate::storage::{PlanHandle, SafeStorageFactory};
use crate::torrent::InfoHash;

/// Incoming connections allowed to be mid-handshake-check at once.
///
/// `librqbit` defaults this to 256, and at exactly 256 its accept loop
/// panics. The loop is a `tokio::select!` over two branches, both with
/// preconditions: `accept` is enabled only while the pending set is under the
/// cap, and draining the pending set is enabled only while it is not empty.
/// A pending check that resolves to `Err` fails the second branch's pattern,
/// which disables it for that iteration, and when the set is at the cap the
/// first branch is already disabled. Every branch disabled is a panic, and the
/// panic kills the accept task while the process carries on: the port stops
/// answering and the run still reports itself as seeding.
///
/// A connection that closes before it handshakes is exactly the `Err` that
/// triggers it, and enough of them at once fill the set. Measured, 3000 such
/// connections at 64 at a time killed a seeder's listener in 79 seconds.
///
/// Raising the cap does not paper over the bug, it removes the branch that
/// carries it: the first branch's precondition never goes false, so the pair
/// can never both be disabled. What the cap was protecting against is still
/// bounded, by the operating system's own limit on sockets and by
/// `--max-peers` on what reaches the swarm. See `TODO/peers.md`, T-020.
const PENDING_HANDSHAKE_CHECKS: usize = usize::MAX;

/// How the session is configured for one run.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// Where payloads are written.
    pub download_directory: PathBuf,
    /// Inclusive port range to try for incoming peer connections.
    pub listen_ports: std::ops::RangeInclusive<u16>,
    /// Bind the peer listener to this address only.
    ///
    /// `None` binds the wildcard address, which is what a real run wants:
    /// peers have to be able to reach it. Setting it to loopback confines the
    /// session to the machine, which is what a test wants. It also keeps a
    /// host firewall quiet, because a loopback-only listener is not an
    /// incoming connection as far as Windows Firewall is concerned.
    pub listen_ip: Option<IpAddr>,
    /// Use the DHT.
    pub enable_dht: bool,
    /// Use local service discovery.
    pub enable_lsd: bool,
    /// Announce to trackers.
    pub enable_trackers: bool,
    /// Accept incoming peer connections at all. `false` still binds a port,
    /// because the web seed bridge needs one, but disables discovery.
    pub enable_peers: bool,
    /// Peer connections per torrent.
    pub max_peers: Option<usize>,
    /// Download rate cap in bytes per second, **across the whole session**.
    ///
    /// `librqbit` has two rate limits and they are different fields:
    /// `SessionOptions::ratelimits` caps the session and
    /// `AddTorrentOptions::ratelimits` caps one torrent. This is the session
    /// one, so it is where `--max-overall-download-rate` belongs. The
    /// per-torrent flag is [`AddOptions::download_rate`]. See
    /// `TODO/cli-surface.md`, T-181.
    pub download_rate: Option<u64>,
    /// Upload rate cap in bytes per second, across the whole session.
    pub upload_rate: Option<u64>,
    /// Download rate cap in bytes per second for **swarm peers only**.
    ///
    /// [`Self::download_rate`] bounds everything the session pulls in, and an
    /// HTTP source reaches the session as a peer over loopback, so it bounds
    /// that too. This one skips the bridge this process runs, by peer id
    /// prefix, so the swarm can be capped while an HTTP mirror is capped
    /// separately by `--web-seed-speed-limit` or not at all. See
    /// `TODO/multi-source.md`, T-132.
    ///
    /// There is no upload counterpart. The bridge is a seed: it never sends
    /// `Interested` and never requests, so nothing is uploaded to it and the
    /// upload caps already reach peers alone.
    pub peer_download_rate: Option<u64>,
    /// Trackers added to every torrent in this run.
    pub extra_trackers: Vec<String>,
    /// Restrict to IPv4.
    pub ipv4_only: bool,
    /// The client name announced to peers and trackers.
    pub client_name: Option<String>,
    /// How space is reserved for each payload file.
    pub allocation: crate::alloc::Allocation,
    /// How many payload files stay open at once. Zero means the default.
    pub max_open_files: usize,
    /// Where a resume cache lives, when one is wanted.
    ///
    /// `None` disables it, which is the default and the previous behaviour:
    /// every add re-hashes the whole payload. `Some` supplies a `BitVFactory`
    /// to the session, which is what `librqbit`'s `fastresume` needs and what
    /// it could only get from a session persistence store before the vendored
    /// tree took one here. See [`crate::resume`] and `TODO/disk-io.md` T-016.
    pub resume_cache: Option<PathBuf>,
    /// What this session does about peer encryption.
    ///
    /// `prefer` is the default and it is what mainline clients default to:
    /// dial with MSE, dial again in plaintext when the peer does not speak it,
    /// and accept both. `require` refuses a plaintext peer in either
    /// direction, which is what reaches a peer that requires encryption at the
    /// cost of every peer that cannot. See [`crate::mse`] and `TODO/peers.md`,
    /// T-163.
    pub encryption: crate::mse::Encryption,
    /// Which transports this session listens on and dials.
    ///
    /// [`Transport::Tcp`] is the default and is what every run did before
    /// 2026-08-24. See `TODO/bep-coverage.md`, T-101.
    pub transport: Transport,
    /// Peer addresses this run refuses, as inclusive ranges.
    ///
    /// The session checks these before it reads an incoming handshake and
    /// before it dials, so a blocked address never takes a connection slot.
    /// They are fixed for the life of the session: `librqbit` 9.0.0 loads its
    /// blocklist once and holds it in a plain field, so nothing can be added
    /// after the session starts. See `TODO/peers.md`, T-164.
    pub blocked_peers: Vec<(IpAddr, IpAddr)>,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            download_directory: PathBuf::from("."),
            listen_ports: 6881..=6889,
            listen_ip: None,
            enable_dht: true,
            enable_lsd: true,
            enable_trackers: true,
            enable_peers: true,
            max_peers: None,
            download_rate: None,
            upload_rate: None,
            peer_download_rate: None,
            resume_cache: None,
            extra_trackers: Vec::new(),
            ipv4_only: false,
            client_name: Some(format!("bit-cli {}", crate::VERSION)),
            allocation: crate::alloc::Allocation::default(),
            max_open_files: crate::storage::DEFAULT_MAX_OPEN_FILES,
            encryption: crate::mse::Encryption::default(),
            transport: Transport::default(),
            blocked_peers: Vec::new(),
        }
    }
}

/// Which transports a session listens on and dials.
///
/// BEP 29 uTP carries the same peer wire protocol as TCP does, over UDP, under
/// LEDBAT congestion control. What that buys is not throughput: LEDBAT targets
/// a fixed one-way queueing delay and backs off when it rises, so a seeder
/// yields to other traffic on the same link rather than competing with it.
///
/// The default is [`Transport::Tcp`] and it stays there. `librqbit`'s own
/// `ListenerOptions` says "once uTP is stable upgrade default to both", and
/// this repository has no measurement that would justify moving first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    /// TCP only.
    #[default]
    Tcp,
    /// uTP only. Nothing reaches a TCP-only peer.
    Utp,
    /// Both, on the same port number, and the peer chooses.
    Both,
}

impl Transport {
    /// The name this appears under in `--json` and in a trace line.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Utp => "utp",
            Self::Both => "both",
        }
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<Transport> for librqbit::ListenerMode {
    fn from(transport: Transport) -> Self {
        match transport {
            Transport::Tcp => Self::TcpOnly,
            Transport::Utp => Self::UtpOnly,
            Transport::Both => Self::TcpAndUtp,
        }
    }
}

/// The peer id a session announces under.
///
/// Here rather than inline in the options, so the test that holds "one binary,
/// one identity" can assert the same twenty bytes the session is given rather
/// than a second construction that happens to agree today. See
/// `TODO/peers.md`, T-236.
pub fn session_peer_id() -> [u8; 20] {
    crate::peer_id::generate(&crate::peer_id::PREFIX)
}

/// How one torrent is added.
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    /// Start paused.
    pub paused: bool,
    /// Write here instead of the session default.
    pub output_folder: Option<String>,
    /// Only these file indices.
    pub only_files: Option<Vec<usize>>,
    /// Write on top of existing files. Required to resume or to seed.
    pub overwrite: bool,
    /// Read the metadata and stop, without starting the torrent.
    pub list_only: bool,
    /// Trackers for this torrent only.
    pub trackers: Option<Vec<String>>,
    /// Skip tracker announces for this torrent.
    pub disable_trackers: bool,
    /// Override the announce interval.
    pub tracker_interval: Option<Duration>,
    /// Peers to try before any are discovered.
    pub initial_peers: Vec<SocketAddr>,
    /// Peer connections for this torrent.
    pub peer_limit: Option<usize>,
    /// Download rate cap in bytes per second, **for this torrent alone**.
    ///
    /// `AddTorrentOptions::ratelimits` rather than `SessionOptions::ratelimits`,
    /// which is what makes `--max-download-rate` a per-torrent cap and
    /// `--max-overall-download-rate` a session one. Before T-181 both flags
    /// aimed at the session field and only one of them reached it.
    pub download_rate: Option<u64>,
    /// Upload rate cap in bytes per second, for this torrent alone.
    pub upload_rate: Option<u64>,
    /// `-O`/`--index-out`: a file index to the path the caller wants it at,
    /// relative to the output directory.
    ///
    /// Applied inside the path plan, so a requested path is sanitised,
    /// truncated and disambiguated exactly as a torrent path is: `-O` renames
    /// a file and cannot be used to escape the output directory. See
    /// `TODO/cli-surface.md`, T-116.
    pub index_out: BTreeMap<usize, String>,
}

/// Coarse state of one torrent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Reading the metadata or hash-checking existing data.
    Initializing,
    /// Connected and transferring.
    Live,
    /// Stopped on request.
    Paused,
    /// Stopped by a failure. The failure is in `error`.
    Error,
}

impl State {
    /// The stable name used in JSON and text output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initializing => "initializing",
            Self::Live => "live",
            Self::Paused => "paused",
            Self::Error => "error",
        }
    }
}

/// How many peers are in each state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerCounts {
    /// Connected and usable.
    pub live: u32,
    /// Connecting right now.
    pub connecting: u32,
    /// Known but not yet tried.
    pub queued: u32,
    /// Seen at any point in this run.
    pub seen: u32,
    /// Tried and given up on.
    pub dead: u32,
}

/// One torrent, as every command reports it.
///
/// Byte counts and durations are raw integers. A formatted string may sit
/// beside one in the rendering layer, but never instead of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentSnapshot {
    /// Position in this run. Not stable across runs, by design: there is no
    /// stored session for an id to be stable against.
    pub id: usize,
    pub info_hash: String,
    pub name: String,
    pub state: State,
    pub total_bytes: u64,
    pub progress_bytes: u64,
    pub uploaded_bytes: u64,
    pub finished: bool,
    pub download_rate: u64,
    pub upload_rate: u64,
    /// Estimated time to completion. An estimate, which is why the name says
    /// so and why `eta_confidence` sits beside it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_ms: Option<u64>,
    /// How much the estimate is worth: `none`, `low`, or `measured`.
    pub eta_confidence: &'static str,
    pub peers: PeerCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TorrentSnapshot {
    /// Progress as a fraction in `0.0..=1.0`.
    pub fn fraction(&self) -> f64 {
        match self.total_bytes {
            0 => 0.0,
            total => (self.progress_bytes as f64 / total as f64).clamp(0.0, 1.0),
        }
    }

    /// Uploaded over downloaded. Zero when nothing has been downloaded.
    pub fn ratio(&self) -> f64 {
        match self.progress_bytes {
            0 => 0.0,
            progress => self.uploaded_bytes as f64 / progress as f64,
        }
    }
}

/// One peer, with the accounting a seeding operator needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerSnapshot {
    pub addr: String,
    /// `live`, `connecting`, `queued`, `dead`, or `not needed`.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// `tcp`, `utp`, or `socks`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// `rc4` when the connection settled on message stream encryption,
    /// `plaintext` when it did not, absent for a peer this session has not
    /// completed a connection with. See `TODO/peers.md`, T-163.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption: Option<String>,
    /// `incoming` when the peer dialled us, `outgoing` when we dialled it.
    pub direction: &'static str,
    /// Bytes this peer sent us.
    pub downloaded_bytes: u64,
    /// Bytes we sent this peer. The number that answers "is my server
    /// actually serving".
    pub uploaded_bytes: u64,
    /// Pieces received from this peer and verified.
    pub verified_pieces: u32,
    /// Blocks received from this peer.
    pub chunks: u32,
    pub errors: u32,
    /// Total time spent establishing connections to this peer.
    pub connect_ms: u64,
    /// Mean time to download one piece from this peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_piece_ms: Option<u64>,
    /// How often this peer choked us, and how often it unchoked us again.
    ///
    /// A peer that chokes goes quiet and looks exactly like one that is slow,
    /// so these are the two numbers that tell "stopped sending" from "stopped
    /// being allowed to send". See `TODO/peers.md`, T-024.
    pub choked: u32,
    pub unchoked: u32,
    /// Why each connection to this peer ended, newest last.
    ///
    /// Empty for a peer that has not disconnected. Bounded: a flapping peer
    /// produces one per flap and the session keeps a thousand peer rows.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub disconnects: Vec<PeerDisconnect>,
    /// Whether this is one of our own web seed bridges rather than a swarm
    /// member. A bridge is not a peer and must never be counted as one.
    pub web_seed: bool,
}

/// Why one connection to a peer ended.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDisconnect {
    /// When, in ISO 8601 UTC with millisecond precision.
    pub at: String,
    /// What ended it. A connection the peer closed cleanly has no error and
    /// reports `closed by the peer`, which is a real reason rather than a
    /// stand-in for a missing one.
    pub reason: String,
}

/// A running torrent.
///
/// Re-exported so a caller can name the type without depending on `librqbit`
/// directly. Everything useful about it is reachable through [`Engine`].
pub type Handle = Arc<ManagedTorrent>;

/// The session, for the length of one invocation.
pub struct Engine {
    session: Arc<Session>,
    /// The resume cache this session was given, when it was given one. Held so
    /// a caller can describe a payload to it before adding the torrent, which
    /// is the only point at which anything knows what the payload should look
    /// like. See [`Self::expect_resume`].
    resume: Option<Arc<crate::resume::FileResumeCache>>,
    api: Api,
    listen_addr: Option<SocketAddr>,
    warnings: Vec<String>,
    download_directory: PathBuf,
    /// One path plan per added torrent, by torrent id. A torrent's files are
    /// not written where the metainfo says when the metainfo says something
    /// the filesystem cannot do, and this is where the caller reads what
    /// happened instead.
    plans: Mutex<HashMap<usize, PlanHandle>>,
    allocation: crate::alloc::Allocation,
    max_open_files: usize,
    /// What storage needed the caller to know, gathered across every torrent.
    storage_notes: Mutex<Vec<Arc<Mutex<Vec<String>>>>>,
    /// What storage did, across every torrent in this session.
    storage_metrics: Arc<crate::storage::StorageMetrics>,
    /// The encryption policy, and what each peer settled on.
    ///
    /// Held here as well as inside the session because `librqbit` takes the
    /// transform as a trait object and gives no way to read it back, and the
    /// negotiated mode per peer is what `--json` reports.
    encryption: Arc<crate::mse::MseTransform>,
}

impl Engine {
    /// Start a session.
    pub async fn start(options: &EngineOptions) -> Result<Self> {
        let (listen_addr, listen_warning) = match options.listen_ip {
            Some(ip) => (bind_on(ip, &options.listen_ports), None),
            None => resolve_listen_addr(&options.listen_ports),
        };
        let mut warnings = Vec::new();
        // Built before the session, because the session takes it by argument.
        // The blocking spawner is the one `DiskBackedBitV` flushes through and
        // is sized the way the session sizes its own.
        let resume = options.resume_cache.as_ref().map(|root| {
            std::sync::Arc::new(crate::resume::FileResumeCache::new(
                root.clone(),
                librqbit::spawn_utils::BlockingSpawner::new(8),
            ))
        });
        warnings.extend(listen_warning);

        let trackers = options
            .extra_trackers
            .iter()
            .filter_map(|t| url::Url::parse(t).ok())
            .collect();

        // `librqbit` reads its peer blocklist from a URL at session start and
        // offers no in-memory door: `Session::blocklist` is a plain `IpRanges`
        // field behind an `Arc` and `IpRanges` lives in a private module. A
        // `file:` URL is accepted, so this is the whole seam. The file exists
        // for the length of this call and is deleted when `_blocklist` drops,
        // which makes it a scratch file rather than the state decision 7.4
        // rules out. See `TODO/peers.md`, T-164.
        let _blocklist = write_blocklist(&options.blocked_peers)?;

        // Message stream encryption. The session calls this on both sides of
        // every peer connection before a protocol byte crosses it, and it is
        // also what `peers()` joins against to say which mode each peer
        // settled on. See `TODO/peers.md`, T-163.
        let encryption = Arc::new(crate::mse::MseTransform::new(options.encryption));

        trace_dht(options.enable_dht);
        let opts = SessionOptions {
            blocklist_url: _blocklist.as_ref().map(|(_, url)| url.clone()),
            dht: options.enable_dht.then(dht_config),
            disable_trackers: !options.enable_trackers,
            disable_local_service_discovery: !options.enable_lsd,
            // No persistence, ever. A stored session is Phase C, and writing
            // one from a foreground command would leave state behind that
            // nothing in this process will read back.
            persistence: None,
            listen: Some(ListenerOptions {
                listen_addr,
                mode: options.transport.into(),
                ipv4_only: options.ipv4_only,
                max_pending_incoming_handshake_checks: PENDING_HANDSHAKE_CHECKS,
                ..Default::default()
            }),
            // Which transports this run **dials**, which is a separate setting
            // from which it listens on and defaults to TCP whatever the
            // listener says.
            //
            // Setting only the listener is what `--transport` did when it was
            // first written, and it produced two wrong answers on the same
            // run: `--transport utp` against a TCP-only peer connected anyway,
            // over TCP, and `--transport utp` against a uTP-only peer timed
            // out. The dialer tries TCP first and only reaches uTP a second
            // later, so a flag that leaves `enable_tcp` true is a flag that
            // says nothing about what the connection turns out to be. See
            // `TODO/bep-coverage.md`, T-101.
            connect: Some(librqbit::ConnectionOptions {
                enable_tcp: !matches!(options.transport, Transport::Utp),
                ..Default::default()
            }),
            ratelimits: LimitsConfig {
                download_bps: rate_to_bps(options.download_rate),
                upload_bps: rate_to_bps(options.upload_rate),
            },
            // `fastresume` on its own does nothing: `librqbit` only reads it
            // where a persistence store already exists. What makes it work
            // here is the factory below, which is a resume cache and no more.
            fastresume: resume.is_some(),
            bitv_factory: resume
                .clone()
                .map(|r| r as std::sync::Arc<dyn librqbit::BitVFactory>),
            trackers,
            peer_limit: options.max_peers,
            ipv4_only: options.ipv4_only,
            client_name_and_version: options.client_name.clone(),
            // The identity this session announces under. Left `None` it was
            // `librqbit`'s own, `-rQ9010-`, so every tracker was told this
            // client is rqbit and the version moved when the vendored tree
            // did. See `TODO/peers.md`, T-236.
            peer_id: Some(librqbit_core::hash_id::Id20::new(session_peer_id())),
            stream_transform: Some(encryption.clone() as Arc<dyn librqbit::StreamTransform>),
            ..Default::default()
        };

        let session = Session::new_with_opts(options.download_directory.clone(), opts)
            .await
            .map_err(|e| {
                Error::generic(format!("cannot start the torrent session: {e}")).with(
                    "download_directory",
                    options.download_directory.display().to_string(),
                )
            })?;
        // The swarm-only download cap, and the one peer it does not apply
        // to. Set here rather than in `SessionOptions` because `LimitsConfig`
        // is a two-field serialized type in `librqbit` and this is a third
        // limiter beside it; `Limits` takes it through a setter, the same way
        // `set_rates` reaches the other two.
        //
        // The exemption is registered whether or not a cap is set, because it
        // costs nothing when the peer limiter is off and forgetting it later
        // would make the flag quietly wrong.
        session
            .ratelimits
            .set_exempt_peer_prefixes(vec![crate::webseed::bridge::PEER_ID_PREFIX]);
        session
            .ratelimits
            .set_peer_download_bps(rate_to_bps(options.peer_download_rate));

        let api = Api::new(session.clone(), None);
        let listen_addr = session.listen_addr();
        if listen_addr.is_none() {
            warnings.push(
                "no peer port was bound, so incoming connections and web seed bridges are unavailable"
                    .to_string(),
            );
        }

        Ok(Self {
            session,
            api,
            resume,
            listen_addr,
            warnings,
            download_directory: options.download_directory.clone(),
            plans: Mutex::new(HashMap::new()),
            allocation: options.allocation,
            max_open_files: options.max_open_files,
            storage_notes: Mutex::new(Vec::new()),
            storage_metrics: Arc::new(crate::storage::StorageMetrics::default()),
            encryption,
        })
    }

    /// What this session's storage has done so far: reads, writes, and the
    /// piece checks that read a piece back and hash it.
    ///
    /// Read it twice and diff to get an interval. See
    /// [`crate::storage::StorageCounts::since`].
    pub fn storage_counts(&self) -> crate::storage::StorageCounts {
        self.storage_metrics.read()
    }

    /// Non-fatal problems found while starting.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// What storage needed the caller to know, across every torrent added.
    ///
    /// Storage cannot report to a stream itself: it runs on the session's
    /// threads and the streams belong to the caller. So it collects, and the
    /// caller reads this when it is ready to report. The only thing that
    /// appears here today is an allocation method that could not be used, with
    /// what ran instead.
    pub fn storage_notes(&self) -> Vec<String> {
        let Ok(handles) = self.storage_notes.lock() else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for handle in handles.iter() {
            let Ok(notes) = handle.lock() else { continue };
            for note in notes.iter() {
                if !out.contains(note) {
                    out.push(note.clone());
                }
            }
        }
        out
    }

    /// The address incoming peer connections arrive on.
    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.listen_addr
    }

    /// Where a web seed bridge should dial to reach this session.
    pub fn bridge_target(&self) -> Option<SocketAddr> {
        self.listen_addr.map(loopback_target)
    }

    /// Tell the resume cache what the payload for `info_hash` should look
    /// like, before the torrent is added.
    ///
    /// A no-op when no cache was configured. It has to happen before the add,
    /// because the session loads the cached bitfield during the add and this
    /// is the only thing that knows what the payload was supposed to be. A
    /// hash nobody described is never served from the cache.
    pub fn expect_resume(&self, info_hash: &str, fingerprint: crate::resume::Fingerprint) {
        if let Some(cache) = self.resume.as_ref() {
            cache.expect(info_hash, fingerprint);
        }
    }

    /// Where the resume cache is, when there is one.
    pub fn resume_root(&self) -> Option<&std::path::Path> {
        self.resume.as_ref().map(|c| c.root())
    }

    /// The underlying session, for the few callers that need it.
    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    /// Add a torrent and return its handle.
    ///
    /// `source` is anything `librqbit` accepts on a command line: a path, a
    /// URL, a magnet, or a bare info hash.
    pub async fn add(&self, source: &str, options: &AddOptions) -> Result<Arc<ManagedTorrent>> {
        let add = AddTorrent::from_cli_argument(source).map_err(|e| {
            Error::source_resolution(format!("{source}: {e}")).with("source", source.to_string())
        })?;
        let response = self.add_inner(source, add, options).await?;
        response.into_handle().ok_or_else(|| {
            Error::source_resolution(format!("{source}: the torrent was listed but not added"))
        })
    }

    /// Add a torrent from bytes the caller already holds.
    ///
    /// `source` is what the caller called it, and is used only in errors and
    /// reports. This exists for the sources the session cannot resolve itself:
    /// a Metalink names its `.torrent` by URL, and this run has already
    /// fetched and parsed that URL. Handing the session the URL again would
    /// fetch it a second time, and two fetches of one URL can return two
    /// different documents. See `TODO/cli-surface.md`, T-113.
    pub async fn add_bytes(
        &self,
        source: &str,
        torrent: Vec<u8>,
        options: &AddOptions,
    ) -> Result<Arc<ManagedTorrent>> {
        let response = self
            .add_inner(source, AddTorrent::from_bytes(torrent), options)
            .await?;
        response.into_handle().ok_or_else(|| {
            Error::source_resolution(format!("{source}: the torrent was listed but not added"))
        })
    }

    /// Where this torrent's files are actually written.
    ///
    /// A torrent path that cannot exist on the filesystem, or that would leave
    /// the output directory, is rewritten before anything is opened. The plan
    /// records every such change with the reason. `None` until the metadata
    /// has resolved and storage has been created, and
    /// [`PathPlan::is_clean`] for the ordinary torrent that needed nothing.
    pub fn path_plan(&self, handle: &ManagedTorrent) -> Option<PathPlan> {
        let plans = self.plans.lock().ok()?;
        plans.get(&handle.id())?.get().cloned()
    }

    /// Read a torrent's metadata without starting it.
    ///
    /// This resolves a magnet against the swarm, which is the one way to turn
    /// a magnet into a layout.
    pub async fn resolve(&self, source: &str) -> Result<ResolvedTorrent> {
        self.resolve_with(source, &AddOptions::default()).await
    }

    /// [`Engine::resolve`] against the swarm the caller is about to add into.
    ///
    /// Which swarm a magnet resolves against depends on its trackers and on
    /// the peers the caller was given, so reading the metadata with the
    /// defaults would look somewhere the add that follows does not. Nothing is
    /// written and nothing is started, so the options that describe writing are
    /// dropped rather than carried. See `TODO/cli-surface.md`, T-185.
    pub async fn resolve_with(
        &self,
        source: &str,
        options: &AddOptions,
    ) -> Result<ResolvedTorrent> {
        let options = AddOptions {
            list_only: true,
            only_files: None,
            paused: false,
            overwrite: false,
            output_folder: None,
            ..options.clone()
        };
        let add = AddTorrent::from_cli_argument(source).map_err(|e| {
            Error::source_resolution(format!("{source}: {e}")).with("source", source.to_string())
        })?;
        match self.add_inner(source, add, &options).await? {
            AddTorrentResponse::ListOnly(list) => {
                let name = list
                    .info
                    .name()
                    .map(|n| n.into_owned())
                    .unwrap_or_else(|| list.info_hash.as_string());
                let multi_file = list.info.info().files.is_some();
                let files = list
                    .info
                    .iter_file_details()
                    .map(|f| (join_components(f.filename.iter_components()), f.len))
                    .collect::<Vec<_>>();
                let layout = Layout::from_lengths(
                    name,
                    multi_file,
                    list.info.lengths().default_piece_length(),
                    files,
                );
                Ok(ResolvedTorrent {
                    info_hash: InfoHash(list.info_hash.0),
                    layout,
                    torrent_bytes: list.torrent_bytes.to_vec(),
                })
            }
            _ => Err(Error::source_resolution(format!(
                "{source}: the torrent started instead of being listed"
            ))),
        }
    }

    async fn add_inner(
        &self,
        source: &str,
        add: AddTorrent<'_>,
        options: &AddOptions,
    ) -> Result<AddTorrentResponse> {
        // A torrent's own file names decide where its bytes go, and a torrent
        // is untrusted input. The session's default storage joins those names
        // onto the output directory as given, which on Windows is enough to
        // leave it. This factory plans safe paths first. See `crate::storage`.
        //
        // `AddOptions::output_folder` is the per-add override, not the run's
        // output directory: `--dir` is `download_directory` and so takes the
        // `None` branch. An add that sets it gets exactly that directory, and
        // `subfolder: false` is what stops a second copy of the torrent's name
        // being appended to a root that already ends in it. `seed` is the only
        // caller that sets it, for the payload root it resolved. Otherwise the
        // session's rule applies and a multi-file torrent goes into a
        // directory named after itself, which the factory reproduces: a
        // download with `--dir out` lands at `out/<name>/`. See
        // `TODO/disk-io.md`, T-190.
        let (output_folder, subfolder) = match &options.output_folder {
            Some(folder) => (PathBuf::from(folder), false),
            None => (self.download_directory.clone(), true),
        };
        let storage = SafeStorageFactory::new(output_folder, options.overwrite, subfolder)
            .with_allocation(self.allocation)
            .with_max_open_files(self.max_open_files)
            .with_index_out(options.index_out.clone())
            .with_metrics(self.storage_metrics.clone());
        let plan = storage.plan_handle();
        if let Ok(mut notes) = self.storage_notes.lock() {
            notes.push(storage.notes_handle());
        }
        let opts = AddTorrentOptions {
            paused: options.paused,
            output_folder: options.output_folder.clone(),
            only_files: options.only_files.clone(),
            overwrite: options.overwrite,
            list_only: options.list_only,
            trackers: options.trackers.clone(),
            disable_trackers: options.disable_trackers,
            force_tracker_interval: options.tracker_interval,
            initial_peers: (!options.initial_peers.is_empty())
                .then(|| options.initial_peers.clone()),
            peer_limit: options.peer_limit,
            ratelimits: LimitsConfig {
                download_bps: rate_to_bps(options.download_rate),
                upload_bps: rate_to_bps(options.upload_rate),
            },
            storage_factory: Some(storage.boxed()),
            ..Default::default()
        };
        let response = self
            .session
            .add_torrent(add, Some(opts))
            .await
            .map_err(|e| classify_add_error(source, &e))?;
        if let AddTorrentResponse::Added(id, _) | AddTorrentResponse::AlreadyManaged(id, _) =
            &response
            && let Ok(mut plans) = self.plans.lock()
        {
            plans.insert(*id, plan);
        }
        Ok(response)
    }

    /// A snapshot of one torrent. No I/O.
    pub fn snapshot(&self, handle: &ManagedTorrent) -> TorrentSnapshot {
        let stats = handle.stats();
        let (download_rate, upload_rate, peers) = live_rates(&stats);
        let eta = handle
            .live()
            .and_then(|l| l.down_speed_estimator().time_remaining());
        TorrentSnapshot {
            id: handle.id(),
            info_hash: handle.info_hash().as_string(),
            name: handle
                .name()
                .unwrap_or_else(|| handle.info_hash().as_string()),
            state: to_state(&stats.state),
            total_bytes: stats.total_bytes,
            progress_bytes: stats.progress_bytes,
            uploaded_bytes: stats.uploaded_bytes,
            finished: stats.finished,
            download_rate,
            upload_rate,
            eta_ms: eta.map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64),
            eta_confidence: match (eta, download_rate) {
                (None, _) => "none",
                (Some(_), 0) => "low",
                (Some(_), _) => "measured",
            },
            peers,
            error: stats.error,
        }
    }

    /// Per-peer accounting for one torrent.
    ///
    /// `bridge_ports` are the loopback ports this run's web seed bridges are
    /// connected from. A bridge is our own HTTP source wearing a peer's
    /// clothes, so it is labelled rather than counted as a swarm member.
    pub fn peers(&self, handle: &ManagedTorrent, bridge_ports: &HashSet<u16>) -> Vec<PeerSnapshot> {
        let Some(live) = handle.live() else {
            return Vec::new();
        };
        let snapshot = live.per_peer_stats_snapshot(all_peers_filter());
        // One lock rather than one per row: the map is shared with every
        // connection task that is negotiating right now.
        let negotiated = self.encryption.negotiated_all();
        let mut rows: Vec<PeerSnapshot> = snapshot
            .peers
            .into_iter()
            .map(|(addr, peer)| {
                let counters = peer.counters;
                let mean_piece_ms = (counters.downloaded_and_checked_pieces > 0).then(|| {
                    counters.total_piece_download_ms
                        / u64::from(counters.downloaded_and_checked_pieces)
                });
                let encryption = negotiated.get(&addr).map(|m| (*m).to_string());
                let disconnects = peer
                    .disconnects
                    .iter()
                    .map(|d| PeerDisconnect {
                        at: crate::time::Timestamp::from_system_time(
                            std::time::UNIX_EPOCH + std::time::Duration::from_millis(d.at_unix_ms),
                        )
                        .iso(),
                        reason: d
                            .reason
                            .clone()
                            .unwrap_or_else(|| "closed by the peer".to_string()),
                    })
                    .collect();
                PeerSnapshot {
                    web_seed: is_own_loopback_port(&addr, bridge_ports),
                    addr,
                    state: peer.state.to_string(),
                    client: peer.client_name,
                    connection: peer.conn_kind.map(|k| format!("{k:?}").to_lowercase()),
                    encryption,
                    choked: counters.times_choked,
                    unchoked: counters.times_unchoked,
                    disconnects,
                    direction: match counters.incoming_connections > 0 {
                        true => "incoming",
                        false => "outgoing",
                    },
                    downloaded_bytes: counters.fetched_bytes,
                    uploaded_bytes: counters.uploaded_bytes,
                    verified_pieces: counters.downloaded_and_checked_pieces,
                    chunks: counters.fetched_chunks,
                    errors: counters.errors,
                    connect_ms: counters.total_time_connecting_ms,
                    mean_piece_ms,
                }
            })
            .collect();
        rows.sort_by(|a, b| a.addr.cmp(&b.addr));
        rows
    }

    /// How many connections `--block-peer` refused, incoming and outgoing.
    ///
    /// This is the number the flag moves, and it is the session's own count
    /// rather than one this crate keeps: `librqbit` bumps it at both check
    /// sites. See `TODO/peers.md`, T-164.
    pub fn blocked(&self) -> BlockedPeers {
        let stats = self.api.api_session_stats();
        BlockedPeers {
            incoming: stats.counters.blocked_incoming,
            outgoing: stats.counters.blocked_outgoing,
        }
    }

    /// Which pieces are present, one bool per piece.
    ///
    /// The wire bitfield is byte aligned, so it carries spare bits past the
    /// last piece that must not be reported as pieces.
    pub fn have_pieces(&self, handle: &ManagedTorrent) -> Option<Vec<bool>> {
        let (have, total) = self
            .api
            .api_dump_haves(TorrentIdOrHash::Id(handle.id()))
            .ok()?;
        Some(have.iter().map(|bit| *bit).take(total as usize).collect())
    }

    /// The torrent's piece hashes, once its metadata has resolved.
    ///
    /// This is what lets an HTTP source be checked at the source rather than
    /// only by the session, which is the difference between "a peer served
    /// bad data" and "this mirror served piece 4108 wrong".
    pub fn piece_hashes(&self, handle: &ManagedTorrent) -> Option<Arc<Vec<[u8; 20]>>> {
        handle
            .with_metadata(|metadata| {
                let raw = metadata.info.info().pieces.as_ref();
                // `as_chunks` gives the array type directly, so there is no
                // fallible conversion per hash to write and none to discard.
                // A trailing partial hash is not a hash and is dropped, which
                // is what `chunks_exact` did too.
                let (hashes, _) = raw.as_chunks::<20>();
                Arc::new(hashes.to_vec())
            })
            .ok()
    }

    /// The trackers this torrent announces to, in sorted order.
    pub fn trackers(&self, handle: &ManagedTorrent) -> Vec<String> {
        let mut out: Vec<String> = handle
            .shared()
            .trackers
            .iter()
            .map(|u| u.to_string())
            .collect();
        out.sort();
        out
    }

    /// The torrent's layout, once its metadata has resolved.
    pub fn layout(&self, handle: &ManagedTorrent) -> Option<Layout> {
        let info_hash = handle.info_hash();
        handle
            .with_metadata(|metadata| {
                let name = metadata
                    .info
                    .name()
                    .map(|n| n.into_owned())
                    .unwrap_or_else(|| info_hash.as_string());
                // The on-disk relative filename is sanitized for the platform,
                // so path separators come back as the OS uses them. The layout
                // is `/`-separated everywhere, which is what BEP 19 URL
                // composition needs.
                let files = metadata
                    .file_infos
                    .iter()
                    .map(|f| {
                        let path = f
                            .relative_filename
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join("/");
                        (path, f.len)
                    })
                    .collect::<Vec<_>>();
                Layout::from_lengths(
                    name,
                    metadata.info.info().files.is_some(),
                    metadata.lengths().default_piece_length(),
                    files,
                )
            })
            .ok()
    }

    /// Wait until the torrent's metadata has resolved and any hash check has
    /// finished.
    pub async fn wait_until_initialized(&self, handle: &ManagedTorrent) -> Result<()> {
        handle
            .wait_until_initialized()
            .await
            .map_err(|e| Error::generic(format!("torrent failed to initialize: {e}")))
    }

    /// Wait for initialisation, giving up after `timeout`.
    ///
    /// Initialisation is reading the metadata and hash-checking whatever is
    /// already on disk, and it is where a torrent can stop making progress
    /// without failing: upstream reports roughly one add in twenty of a
    /// torrent with existing files sticking at "checking files" and never
    /// leaving. See `TODO/disk-io.md`, T-015.
    ///
    /// A run bounded only by `--timeout` survives that, but reports a deadline
    /// rather than the reason, so the error carries the phase, how far the
    /// check had got, and how long it waited. "It hung hashing at 43%" is
    /// something an operator can act on; "it timed out" is not.
    pub async fn wait_until_initialized_within(
        &self,
        handle: &ManagedTorrent,
        timeout: Duration,
    ) -> Result<()> {
        match tokio::time::timeout(timeout, self.wait_until_initialized(handle)).await {
            Ok(result) => result,
            Err(_) => {
                let snapshot = self.snapshot(handle);
                let checked = crate::units::format_percent(snapshot.fraction());
                Err(Error::timeout(format!(
                    "{}: still initializing after {}ms, hash-checked {checked}",
                    snapshot.name,
                    timeout.as_millis()
                ))
                .with("phase", "initializing")
                .with("info_hash", snapshot.info_hash)
                .with(
                    "waited_ms",
                    timeout.as_millis().min(u128::from(u64::MAX)) as u64,
                )
                .with("checked_bytes", snapshot.progress_bytes)
                .with("total_bytes", snapshot.total_bytes)
                .with("checked_percent", checked)
                .with("state", snapshot.state.as_str()))
            }
        }
    }

    /// Wait until every wanted piece is present and verified.
    pub async fn wait_until_completed(&self, handle: &ManagedTorrent) -> Result<()> {
        handle
            .wait_until_completed()
            .await
            .map_err(|e| Error::generic(format!("torrent failed: {e}")))
    }

    /// Change the live rate limits.
    pub fn set_rates(&self, download: Option<u64>, upload: Option<u64>) {
        self.session
            .ratelimits
            .set_download_bps(rate_to_bps(download));
        self.session.ratelimits.set_upload_bps(rate_to_bps(upload));
    }

    /// Drop every peer connection and dial the peer list again.
    ///
    /// A peer that dies is retried on a backoff with a minimum of 10 seconds
    /// and a factor of 6, so attempts land at about 10s, 70s, 430s, and then
    /// 36 minutes (`librqbit` 9.0.0,
    /// `torrent_state/live/peer/stats/atomic.rs`). A peer that comes back one
    /// second after an attempt fails waits six times the last wait. On a swarm
    /// of one, which is what `--peer` builds, that is the difference between
    /// finishing and timing out.
    ///
    /// The backoff itself is not reachable: it is built in `pub(crate)` code
    /// from constants and `SessionOptions` does not carry it. What is
    /// reachable is throwing the peer state away. Pausing a live torrent moves
    /// it to `Paused`, which drops `TorrentStateLive` and with it the set of
    /// peers already seen and their backoff counters, while keeping the chunk
    /// tracker. Starting it again builds a fresh peer stream carrying
    /// `initial_peers` and a fresh tracker announce.
    ///
    /// It costs no hash check: `Paused` to `Live` is a direct transition, and
    /// only a fresh add or an error goes through `Initializing`. What it does
    /// cost is every live connection, so it is worth doing when nothing is
    /// arriving and not otherwise. See `TODO/peers.md`, T-138.
    pub async fn redial(&self, handle: &Handle) -> Result<()> {
        self.session
            .pause(handle)
            .await
            .map_err(|e| Error::generic(format!("could not pause the torrent to re-dial: {e}")))?;
        self.session.unpause(handle).await.map_err(|e| {
            Error::generic(format!(
                "could not restart the torrent after a re-dial: {e}"
            ))
        })
    }

    /// Stop the session and everything running under it.
    pub async fn stop(self) {
        self.session.stop().await;
    }
}

/// A torrent whose metadata has been resolved without starting it.
pub struct ResolvedTorrent {
    pub info_hash: InfoHash,
    pub layout: Layout,
    /// The `.torrent` bytes, so a magnet can be turned into a file.
    pub torrent_bytes: Vec<u8>,
}

/// A peer filter that keeps every peer, not only the connected ones.
///
/// A peer that sent two gigabytes and then disconnected still belongs in the
/// accounting, so the default filter (connected peers only) is wrong here.
/// Every peer, including one that took two gigabytes and left.
///
/// Built through the filter's own `Deserialize` from a fixed literal until
/// 2026-08-22, because `librqbit` exported `PeerStatsFilter` and not the enum
/// its one field holds. The vendored tree exports both. See `TODO/peers.md`,
/// T-025.
fn all_peers_filter() -> PeerStatsFilter {
    PeerStatsFilter {
        state: PeerStatsFilterState::All,
    }
}

/// Join a torrent path's components with `/`, on every platform.
fn join_components<'a>(components: impl Iterator<Item = std::borrow::Cow<'a, str>>) -> String {
    components
        .map(|c| c.into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Turn a bytes-per-second cap into what `librqbit` wants.
///
/// Zero and `None` both mean unlimited. A cap above `u32::MAX` bytes per
/// second is past any real link, so it saturates rather than wrapping.
fn rate_to_bps(rate: Option<u64>) -> Option<NonZeroU32> {
    let rate = rate?;
    NonZeroU32::new(rate.min(u64::from(u32::MAX)) as u32)
}

fn live_rates(stats: &TorrentStats) -> (u64, u64, PeerCounts) {
    match &stats.live {
        Some(live) => {
            let peers = &live.snapshot.peer_stats;
            (
                live.download_speed.as_bytes(),
                live.upload_speed.as_bytes(),
                PeerCounts {
                    live: peers.live,
                    connecting: peers.connecting,
                    queued: peers.queued,
                    seen: peers.seen,
                    dead: peers.dead,
                },
            )
        }
        None => (0, 0, PeerCounts::default()),
    }
}

fn to_state(state: &TorrentStatsState) -> State {
    match state {
        // The `paused` flag here is what the torrent will do once
        // initialization finishes. While it runs, it really is initializing.
        TorrentStatsState::Initializing { .. } => State::Initializing,
        TorrentStatsState::Live => State::Live,
        TorrentStatsState::Paused => State::Paused,
        TorrentStatsState::Error => State::Error,
    }
}

/// Give an add failure the exit code that matches what actually went wrong.
///
/// A caller branches on the exit code, so "could not write the file" and "the
/// tracker is unreachable" must not both arrive as a generic failure.
///
/// Anything `bit-cli`'s own storage raises is classified by type, because an
/// exit code decided by a string is an exit code that changes when somebody
/// rewords an error. `librqbit` reports its own failures as one opaque chain
/// with no types to match on, so those still go by text, and every phrase
/// matched there is pinned by a test.
fn classify_add_error(source: &str, err: &anyhow::Error) -> Error {
    let text = format!("{err:#}");
    let code = match classify_by_type(err) {
        Some(code) => code,
        None => classify_by_text(&text),
    };
    Error::new(code, format!("{source}: {text}")).with("source", source.to_string())
}

/// The classification `bit-cli`'s own errors carry with them.
fn classify_by_type(err: &anyhow::Error) -> Option<crate::exit::ExitCode> {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<crate::storage::AlreadyExists>())
        .map(|_| crate::exit::ExitCode::Disk)
}

/// The classification for an error that arrived as prose.
fn classify_by_text(text: &str) -> crate::exit::ExitCode {
    let lower = text.to_lowercase();
    if lower.contains("no such file")
        || lower.contains("cannot find the file")
        || lower.contains("permission denied")
        || lower.contains("already exists")
        // `EEXIST`. `librqbit`'s own cache files hit this when a previous run
        // left them behind.
        || lower.contains("os error 17")
    {
        crate::exit::ExitCode::Disk
    } else if lower.contains("dns") || lower.contains("connect") || lower.contains("tls") {
        crate::exit::ExitCode::Network
    } else {
        crate::exit::ExitCode::SourceResolution
    }
}

/// Address families tried when binding the peer listener.
///
/// IPv6 first: `librqbit` clears `IPV6_V6ONLY` for an unspecified v6 address,
/// so `[::]` is a genuine dual-stack socket on Windows as well as Linux.
const LISTEN_FAMILIES: [IpAddr; 2] = [
    IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    IpAddr::V4(Ipv4Addr::UNSPECIFIED),
];

/// Pick the address to listen on for incoming peer connections.
///
/// `librqbit` binds one address and fails the session if it is taken, so the
/// configured range is walked here and an OS-assigned port is used as a last
/// resort. A port clash costs the preferred port rather than the run.
pub fn resolve_listen_addr(ports: &std::ops::RangeInclusive<u16>) -> (SocketAddr, Option<String>) {
    choose_listen_addr(ports, &bindable)
}

/// The port-selection decision, with the socket probe injected.
///
/// Separating the decision from the probe is what makes it testable: a test
/// describes which addresses are taken and asserts the choice without binding
/// anything. That also keeps the test suite from opening wildcard listeners,
/// which a host firewall asks the user about once per binary.
fn choose_listen_addr(
    ports: &std::ops::RangeInclusive<u16>,
    free: &dyn Fn(&SocketAddr) -> bool,
) -> (SocketAddr, Option<String>) {
    // Probing only `[::]` is not enough. On Windows the standard library
    // leaves `IPV6_V6ONLY` on, so a successful `[::]` bind says nothing about
    // IPv4, and the dual-stack socket `librqbit` then builds fails on a port
    // that is only taken on the IPv4 side.
    let dual_stack_free = |port: u16| {
        LISTEN_FAMILIES
            .iter()
            .all(|ip| free(&SocketAddr::new(*ip, port)))
    };

    if let Some(port) = ports.clone().find(|port| dual_stack_free(*port)) {
        return (SocketAddr::new(LISTEN_FAMILIES[0], port), None);
    }
    // No port in the range is free on both stacks. Try one family at a time
    // before giving the port choice to the operating system.
    for ip in LISTEN_FAMILIES {
        if let Some(addr) = ports
            .clone()
            .map(|port| SocketAddr::new(ip, port))
            .find(|a| free(a))
        {
            return (
                addr,
                Some(format!(
                    "port {} is only free on {}, so the peer listener is not dual-stack",
                    addr.port(),
                    match ip.is_ipv6() {
                        true => "IPv6",
                        false => "IPv4",
                    }
                )),
            );
        }
    }

    let warning = format!(
        "ports {}-{} are unavailable, letting the operating system choose the peer port",
        ports.start(),
        ports.end()
    );
    for ip in LISTEN_FAMILIES {
        let any = SocketAddr::new(ip, 0);
        if free(&any) {
            return (any, Some(warning));
        }
    }
    // Nothing binds at all. Hand back the configured port so `librqbit`
    // reports the real reason rather than this function guessing at it.
    (
        SocketAddr::new(LISTEN_FAMILIES[0], *ports.start()),
        Some(warning),
    )
}

/// The first free port in `ports` on one specific address.
///
/// Port zero is not probed: it means "let the operating system choose", and
/// probing it would only prove that the OS can hand out a port.
fn bind_on(ip: IpAddr, ports: &std::ops::RangeInclusive<u16>) -> SocketAddr {
    if *ports.start() == 0 {
        return SocketAddr::new(ip, 0);
    }
    match ports
        .clone()
        .map(|port| SocketAddr::new(ip, port))
        .find(bindable)
    {
        Some(addr) => addr,
        None => SocketAddr::new(ip, 0),
    }
}

/// Whether `addr` can be bound right now.
///
/// The probe socket closes immediately, so `librqbit` binds it moments later.
/// This is a race in principle; in practice a port that was free a moment ago
/// is the best answer available, and losing the race produces a clear bind
/// error rather than a wrong result.
fn bindable(addr: &SocketAddr) -> bool {
    std::net::TcpListener::bind(addr).is_ok()
}

/// Connections `--block-peer` refused, in each direction.
///
/// Kept apart because they answer different questions: `outgoing` is a peer the
/// run was told about and refused to dial, and `incoming` is one that dialled
/// this session and was refused before its handshake was read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedPeers {
    pub incoming: u64,
    pub outgoing: u64,
}

impl BlockedPeers {
    /// Whether anything was refused at all.
    pub const fn any(self) -> bool {
        self.incoming > 0 || self.outgoing > 0
    }
}

/// DHT configuration, with the routing table cache turned off.
///
/// `DhtSessionConfig::default()` enables persistence, and persistence is a
/// JSON routing table under the OS cache directory, rewritten every sixty
/// seconds. Two things are wrong with that here, and the second is the one
/// that bites.
///
/// It is state a foreground one-shot leaves behind, which decision 7.4 rules
/// out. The `persistence: None` a few lines above says "no persistence, ever"
/// about the session, and the DHT was writing anyway.
///
/// And the path is not this program's. `dht::persistence` builds it from
/// `get_configuration_directory("dht")`, which is `com.rqbit.dht`, so this
/// program was rewriting the routing table of any `rqbit` install on the same
/// machine. There is one on this machine, and this repository runs it for
/// interop. Measured: one 90 second run took that file from 95,248 bytes to
/// 81,752 and moved its timestamp.
///
/// What it costs to turn off is bootstrapping from the well-known nodes on
/// every run, which is what a tool that keeps no state has to do anyway. See
/// `TODO/dht.md`, T-050.
fn dht_config() -> DhtSessionConfig {
    DhtSessionConfig {
        persistence: None,
        ..Default::default()
    }
}

/// What this process decided about the DHT, for `--trace dht`.
///
/// The subsystem's other target is `librqbit_dht`, which carries the queries,
/// the responses and the routing table. This is the one fact that target
/// cannot carry: whether there is a DHT at all. A run with `--no-dht` or
/// `--web-seed-only` emits nothing from the vendored crate because nothing
/// runs, and "no records" is indistinguishable from "the flag does nothing",
/// which is the state `TODO/cli-surface.md` T-219 was filed about. So the
/// decision is recorded here, once per session, before it is acted on.
fn trace_dht(enabled: bool) {
    tracing::trace!(
        target: "bit_cli::dht",
        enabled,
        persistence = false,
        "session dht"
    );
}

/// Write the blocked ranges where `librqbit` will read them, as a `file:` URL.
///
/// The format is PeerGuardian's, which is what `IpRanges` parses: one
/// `name:start-end` per line, `#` for a comment. Its parser splits an IPv4 line
/// at the **last** colon and an IPv6 line at the first, so the name must not
/// contain one; `blocked` does not.
///
/// `None` when nothing is blocked, so an ordinary run writes no file at all.
fn write_blocklist(
    ranges: &[(IpAddr, IpAddr)],
) -> Result<Option<(tempfile::NamedTempFile, String)>> {
    use std::io::Write;

    if ranges.is_empty() {
        return Ok(None);
    }
    let mut body = String::from("# bit-cli --block-peer, for this invocation only\n");
    for (start, end) in ranges {
        body.push_str(&format!("blocked:{start}-{end}\n"));
    }
    let mut file = tempfile::Builder::new()
        .prefix("bit-cli-blocklist-")
        .suffix(".txt")
        .tempfile()
        .map_err(|e| crate::error::from_io(e, "cannot write the peer blocklist"))?;
    file.write_all(body.as_bytes())
        .and_then(|()| file.flush())
        .map_err(|e| crate::error::from_io(e, "cannot write the peer blocklist"))?;
    let url = url::Url::from_file_path(file.path())
        .map_err(|()| {
            Error::generic("the peer blocklist landed on a path that is not a file URL")
                .with("path", file.path().display().to_string())
        })?
        .to_string();
    Ok(Some((file, url)))
}

/// Where a bridge dials to reach the session's own peer listener.
///
/// An unspecified bind address is not connectable, so it becomes loopback.
/// Anything else is already an address the session answers on.
pub fn loopback_target(listen: SocketAddr) -> SocketAddr {
    if !listen.ip().is_unspecified() {
        return listen;
    }
    let ip = match listen.ip() {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
    };
    SocketAddr::new(ip, listen.port())
}

/// Whether a peer address is this process connected to itself over loopback
/// on one of `ports`.
///
/// Two things dial this run's own listener: a web seed bridge, which is
/// labelled as one rather than counted as a swarm member, and the listener
/// health probe, whose rows are dropped outright. Both ask this.
pub fn is_own_loopback_port(addr: &str, ports: &HashSet<u16>) -> bool {
    if ports.is_empty() {
        return false;
    }
    let Some((host, port)) = addr.rsplit_once(':') else {
        return false;
    };
    if !matches!(host, "127.0.0.1" | "[::1]" | "::1" | "localhost") {
        return false;
    }
    port.parse().is_ok_and(|port| ports.contains(&port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unspecified_listen_address_becomes_loopback() {
        let v4: SocketAddr = "0.0.0.0:6881".parse().unwrap();
        assert_eq!(loopback_target(v4), "127.0.0.1:6881".parse().unwrap());

        let v6: SocketAddr = "[::]:6881".parse().unwrap();
        assert_eq!(loopback_target(v6), "[::1]:6881".parse().unwrap());
    }

    #[test]
    fn a_specific_listen_address_is_dialled_as_is() {
        let addr: SocketAddr = "192.0.2.10:51413".parse().unwrap();
        assert_eq!(loopback_target(addr), addr);
    }

    /// A probe that reports the listed addresses as taken and everything else
    /// as free. Nothing is bound, so the test suite never opens a wildcard
    /// listener and never trips a host firewall.
    fn taken(addrs: &[&str]) -> impl Fn(&SocketAddr) -> bool + use<> {
        let taken: Vec<SocketAddr> = addrs.iter().map(|a| a.parse().unwrap()).collect();
        move |addr: &SocketAddr| !taken.contains(addr)
    }

    #[test]
    fn a_free_port_range_yields_its_first_port_on_the_dual_stack_address() {
        let (addr, warning) = choose_listen_addr(&(6881..=6889), &taken(&[]));
        assert_eq!(addr.port(), 6881);
        assert!(addr.is_ipv6() && addr.ip().is_unspecified(), "{addr}");
        assert_eq!(warning, None);
    }

    #[test]
    fn a_busy_port_is_skipped_for_the_next_one_in_the_range() {
        let busy = taken(&["[::]:6881", "0.0.0.0:6881"]);
        let (addr, warning) = choose_listen_addr(&(6881..=6889), &busy);
        assert_eq!(addr.port(), 6882);
        assert_eq!(warning, None);
    }

    #[test]
    fn a_port_taken_on_ipv4_alone_is_not_chosen_for_a_dual_stack_listener() {
        // This is the Windows trap: `[::]` binds IPv6-only there, so probing
        // it alone reports a port free that a dual-stack bind will then fail
        // on. Only the IPv4 side of 6881 is held here.
        let busy = taken(&["0.0.0.0:6881"]);
        let (addr, warning) = choose_listen_addr(&(6881..=6882), &busy);
        assert_eq!(addr.port(), 6882, "6881 is not dual-stack free");
        assert_eq!(warning, None);
    }

    #[test]
    fn a_single_family_fallback_says_which_family_it_settled_for() {
        // Every port in the range is held on IPv4, so no dual-stack bind is
        // possible and the listener settles for IPv6 with a warning.
        let busy = taken(&["0.0.0.0:6881", "0.0.0.0:6882"]);
        let (addr, warning) = choose_listen_addr(&(6881..=6882), &busy);
        assert!(addr.is_ipv6());
        assert_eq!(addr.port(), 6881);
        assert!(warning.unwrap().contains("IPv6"));
    }

    #[test]
    fn an_exhausted_range_falls_back_to_an_os_chosen_port() {
        let busy = taken(&["[::]:6881", "0.0.0.0:6881"]);
        let (addr, warning) = choose_listen_addr(&(6881..=6881), &busy);
        assert_eq!(
            addr.port(),
            0,
            "an OS-assigned port is the documented fallback"
        );
        assert!(warning.unwrap().contains("6881-6881"));
    }

    #[test]
    fn nothing_bindable_at_all_hands_the_configured_port_back() {
        // With no address usable, guessing is worse than letting `librqbit`
        // fail on the port the caller actually asked for and say why.
        let (addr, warning) = choose_listen_addr(&(6881..=6881), &|_| false);
        assert_eq!(addr.port(), 6881);
        assert!(warning.is_some());
    }

    #[test]
    fn a_loopback_only_listener_walks_the_range_on_that_address_alone() {
        assert_eq!(
            bind_on(Ipv4Addr::LOCALHOST.into(), &(0..=0)),
            "127.0.0.1:0".parse().unwrap(),
            "port zero is never probed; it means the OS chooses"
        );
    }

    #[test]
    fn rate_limits_saturate_rather_than_wrapping() {
        assert_eq!(rate_to_bps(None), None);
        assert_eq!(rate_to_bps(Some(0)), None, "zero means unlimited");
        assert_eq!(rate_to_bps(Some(1024)).unwrap().get(), 1024);
        assert_eq!(rate_to_bps(Some(u64::MAX)).unwrap().get(), u32::MAX);
    }

    #[test]
    fn only_loopback_addresses_on_a_known_bridge_port_count_as_web_seeds() {
        let ports: HashSet<u16> = [40001].into_iter().collect();
        assert!(is_own_loopback_port("127.0.0.1:40001", &ports));
        assert!(is_own_loopback_port("[::1]:40001", &ports));
        assert!(
            !is_own_loopback_port("127.0.0.1:40002", &ports),
            "a different port is a real peer"
        );
        assert!(
            !is_own_loopback_port("203.0.113.7:40001", &ports),
            "a routable address is a real peer"
        );
        assert!(!is_own_loopback_port("127.0.0.1:40001", &HashSet::new()));
        assert!(!is_own_loopback_port("garbage", &ports));
    }

    #[test]
    fn states_have_stable_names() {
        for state in [
            State::Initializing,
            State::Live,
            State::Paused,
            State::Error,
        ] {
            assert!(
                state.as_str().chars().all(|c| c.is_ascii_lowercase()),
                "{state:?}"
            );
        }
    }

    #[test]
    fn progress_and_ratio_never_divide_by_zero() {
        let mut snapshot = TorrentSnapshot {
            id: 0,
            info_hash: "0".repeat(40),
            name: "t".into(),
            state: State::Live,
            total_bytes: 0,
            progress_bytes: 0,
            uploaded_bytes: 100,
            finished: false,
            download_rate: 0,
            upload_rate: 0,
            eta_ms: None,
            eta_confidence: "none",
            peers: PeerCounts::default(),
            error: None,
        };
        assert_eq!(snapshot.fraction(), 0.0);
        assert_eq!(snapshot.ratio(), 0.0);

        snapshot.total_bytes = 200;
        snapshot.progress_bytes = 50;
        assert_eq!(snapshot.fraction(), 0.25);
        assert_eq!(snapshot.ratio(), 2.0);

        // Progress past the total, which a re-check can briefly report, must
        // not produce a fraction above one.
        snapshot.progress_bytes = 500;
        assert_eq!(snapshot.fraction(), 1.0);
    }
    /// A file that is already there is a disk failure, not a bad source.
    ///
    /// A caller branches on the exit code, and `download` over an existing
    /// payload without `--allow-overwrite` is exactly the case where a generic
    /// failure is useless: the fix is a flag, and the code has to say so. See
    /// `TODO/disk-io.md`, T-014.
    #[test]
    fn an_existing_file_is_classified_by_type_rather_than_by_its_wording() {
        let error = anyhow::Error::new(crate::storage::AlreadyExists {
            path: std::path::PathBuf::from("out/payload.bin"),
        })
        .context("could not create storage");
        let classified = classify_add_error("payload.torrent", &error);
        assert_eq!(classified.code(), crate::exit::ExitCode::Disk);
        assert!(classified.message().contains("payload.bin"), "{classified}");
        assert!(
            classified.message().contains("--allow-overwrite"),
            "the error names the fix: {classified}"
        );
    }

    /// The phrases the text classifier matches on, pinned.
    ///
    ///  reports its own failures as prose with no type to match, so
    /// these are matched by string. A test is what keeps a reworded phrase from
    /// silently changing an exit code.
    #[test]
    fn every_text_classification_maps_to_the_code_it_is_there_for() {
        for (text, expected) in [
            (
                "No such file or directory (os error 2)",
                crate::exit::ExitCode::Disk,
            ),
            (
                "The system cannot find the file specified",
                crate::exit::ExitCode::Disk,
            ),
            (
                "Permission denied (os error 13)",
                crate::exit::ExitCode::Disk,
            ),
            ("File exists (os error 17)", crate::exit::ExitCode::Disk),
            ("out/a.bin already exists", crate::exit::ExitCode::Disk),
            (
                "dns error: failed to lookup address",
                crate::exit::ExitCode::Network,
            ),
            ("tcp connect error", crate::exit::ExitCode::Network),
            ("invalid TLS certificate", crate::exit::ExitCode::Network),
            (
                "torrent file is not bencode",
                crate::exit::ExitCode::SourceResolution,
            ),
        ] {
            assert_eq!(classify_by_text(text), expected, "{text}");
        }
    }

    /// The DHT writes no routing table, anywhere.
    ///
    /// The default enables it, so this is one field away from coming back on a
    /// version bump, and what it wrote was another program's file. See
    /// `TODO/dht.md`, T-050.
    #[test]
    fn the_dht_keeps_no_cache_on_disk() {
        assert!(
            dht_config().persistence.is_none(),
            "the DHT is persisting a routing table again"
        );
        // And the default really is the other way, so this test is about a
        // choice rather than about a tautology.
        assert!(
            librqbit::DhtSessionConfig::default().persistence.is_some(),
            "upstream stopped persisting by default: this test no longer says anything"
        );
    }

    #[test]
    fn a_type_beats_the_text_when_both_could_match() {
        // The message says "connect", which the text classifier would call a
        // network failure. The type says otherwise and the type wins.
        let error = anyhow::Error::new(crate::storage::AlreadyExists {
            path: std::path::PathBuf::from("connect/payload.bin"),
        });
        assert_eq!(
            classify_add_error("x.torrent", &error).code(),
            crate::exit::ExitCode::Disk
        );
    }
}
