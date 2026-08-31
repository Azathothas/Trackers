//! A loopback BitTorrent peer backed by an HTTP source.
//!
//! `librqbit` has no notion of an HTTP source: its only entry point for
//! torrent data is the peer protocol. So a web seed is presented to the
//! session as an ordinary peer. The bridge dials the session's own listen
//! port, announces the pieces its source can serve, unchokes, and answers each
//! `request` with bytes fetched over ranged GETs.
//!
//! Nothing here verifies piece hashes. Fetched bytes reach the session as
//! normal peer blocks, so the session's own verification applies and a source
//! serving wrong data is dropped exactly like a lying peer.
//!
//! Two things separate this from a naive "claim everything" bridge:
//!
//! - The announced bitfield carries only the pieces the source's scope covers
//!   **in full**. A source holding half a piece cannot satisfy that piece's
//!   hash on its own, so claiming it would make the session request bytes the
//!   bridge has to refuse.
//! - When that is not the whole torrent, the bridge advertises BEP 21
//!   `upload_only`. A partial seed that does not say so reads to the session
//!   as a leecher that happens to be missing pieces.
//!
//! The bridge only ever seeds. It never sends `interested`, so it cannot
//! consume the session's upload bandwidth.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use librqbit::ByteBuf;
use librqbit_core::Id20;
use librqbit_peer_protocol::{Handshake, Message, Piece};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;

use crate::layout::Layout;
use crate::webseed::fetch::{FetchError, Fetcher};

/// Azureus-style client prefix for a bridge's peer id, from the one place the
/// client identity lives.
///
/// It has to differ from the session's own id, or the session drops the
/// connection as a self-connect. See `TODO/peers.md`, T-236.
pub(crate) const PEER_ID_PREFIX: [u8; 8] = crate::peer_id::role(*b"ws", *b"01");

/// Serialized keep-alive: a bare zero length prefix.
const KEEP_ALIVE: [u8; 4] = [0, 0, 0, 0];

/// Wire size of a BitTorrent v1 handshake.
const HANDSHAKE_LEN: usize = 68;

/// Bytes a message needs beyond its variable-length payload: the length
/// prefix, the message id, and a `piece` message's index and offset.
const MESSAGE_OVERHEAD: usize = 13;

/// How often to send a keep-alive. `librqbit` drops a peer that is silent for
/// longer than its read timeout, which defaults to ten seconds.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(5);

/// Largest block the bridge serves. Real clients ask for 16 KiB; far above
/// that is a malformed request.
const MAX_REQUEST_LEN: u32 = 128 * 1024;

/// Longest frame accepted from the session, which bounds the read buffer.
const MAX_FRAME_LEN: usize = 1024 * 1024;

/// Requests the bridge tells the session it will keep queued.
const REQUEST_QUEUE: u32 = 250;

/// First delay before reconnecting. Doubles per failure.
const RECONNECT_BASE: Duration = Duration::from_secs(1);

/// Longest delay between reconnection attempts.
const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// How many loopback ports a bridge remembers having connected from.
const MAX_REMEMBERED_PORTS: usize = 64;

/// A `request`, `cancel` or `reject request` body: three big-endian `u32`s.
///
/// [`serialize`] sizes its buffer as the overhead plus whatever
/// variable-length body the message carries, and a rejection's body is longer
/// than the piece preamble that overhead is built for. Passing zero produced
/// `NoSpaceInBuffer` and ended the connection the rejection existed to keep.
const REQUEST_BODY_LEN: usize = 12;

/// BitTorrent message id for the BEP 10 extension protocol.
const MSGID_EXTENDED: u8 = 20;

/// Extension message id 0 is the extended handshake.
const EXTENDED_HANDSHAKE: u8 = 0;

/// Extension ids this bridge advertises in its own `m`, as `(name, our id)`.
///
/// BEP 10 carries two independent numberings, and this is one of them: the id
/// a **peer** must use when it sends us that extension. The other direction,
/// the id **we** must use when sending to a peer, is read out of the peer's
/// own extended handshake and is never this table. Indexing one with the
/// other's key is the defect vortex PR 103 found, where extensions had never
/// once worked against qBittorrent because an incoming id was checked against
/// the local numbering.
///
/// This is empty, and the empty `m` in [`extended_handshake`] is the same
/// statement on the wire: the bridge only seeds and implements no extension
/// messages. So no incoming extension id can be one of ours.
const OUR_EXTENSIONS: &[(&str, u8)] = &[];

/// Whether an incoming extension id is one this bridge advertised.
///
/// The id in an incoming extension message is only meaningful in **our**
/// numbering, so [`OUR_EXTENSIONS`] is the only table it may be looked up in.
/// Reading it against anyone else's numbering is what T-166 is about.
fn is_our_extension(id: u8) -> bool {
    OUR_EXTENSIONS.iter().any(|(_, ours)| *ours == id)
}

/// What a bridge is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeState {
    /// Dialling the session, or waiting to retry.
    Connecting,
    /// Connected, unchoked, and serving.
    Active,
    /// Nothing to do: the torrent is not live, or it is already complete.
    Idle,
    /// Out for now, and coming back. The source spent its error budget and
    /// `--web-seed-cooldown` is non-zero, so the bridge is sleeping until the
    /// deadline and will reconnect. A cooling source is not a failed one: a
    /// caller waiting on it should keep waiting. See `TODO/multi-source.md`,
    /// T-137.
    Cooling,
    /// The source is unusable and the bridge has given up on it.
    Failed,
}

/// Live state of one bridge, readable while it runs.
#[derive(Debug)]
pub struct BridgeStatus {
    state: Mutex<BridgeState>,
    error: Mutex<Option<String>>,
    served_bytes: AtomicU64,
    blocks: AtomicU64,
    local_port: AtomicU16,
    /// Every loopback port this bridge has connected from.
    ports: Mutex<Vec<u16>>,
    /// Blocks the session has asked for and not yet been given.
    ///
    /// This is the session's request window seen from the other end, and it
    /// is the number that says whether the pipeline is deep enough to keep
    /// the link busy. `bench leech` samples it.
    in_flight: AtomicU64,
    peak_in_flight: AtomicU64,
    requests: AtomicU64,
    /// Total time from a request arriving to its block going out, over every
    /// block served. Divided by [`Self::blocks`] it is the mean service time,
    /// which with the depth above bounds throughput at depth over service
    /// time.
    service_nanos: AtomicU64,
    /// How many times the connection to the session ended and was made again.
    ///
    /// A bridge that reconnects is not serving, and it waits between attempts
    /// on a delay that doubles from one second to thirty. Nothing in the
    /// report said so, which is why a run that spent four and a half minutes
    /// waiting looked identical to one that was slow. See
    /// `TODO/performance.md`, T-037.
    reconnects: AtomicU64,
    /// Milliseconds spent asleep between those attempts. This is the number
    /// that says where a stalled run's time went.
    reconnect_wait_ms: AtomicU64,
    /// Those reconnects by why the last attempt ended, newest count last.
    reconnect_reasons: Mutex<std::collections::BTreeMap<&'static str, u64>>,
    /// Files this source turned out not to hold, with why, in the order they
    /// were found.
    ///
    /// A permanent failure on one file narrows the source rather than retiring
    /// it, and a caller has to be able to see that happen: a mirror serving
    /// eleven files of twelve is a different thing from one serving all
    /// twelve, and the byte counts alone cannot tell them apart. See
    /// `TODO/webseed.md`, T-005.
    gone_files: Mutex<Vec<GoneFile>>,
    /// Pieces given up across every file lost.
    pieces_dropped: AtomicU64,
    /// Of those, the ones retracted with BEP 54 rather than by reconnecting.
    pieces_retracted: AtomicU64,
    /// Requests answered with BEP 6 `reject request` rather than by ending the
    /// connection.
    rejections: AtomicU64,
}

/// One file a source turned out not to hold.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoneFile {
    /// File index within the torrent.
    pub file: usize,
    /// Pieces the source stopped announcing because of it.
    pub pieces_dropped: usize,
    /// What the mirror said, which is the status and the URL.
    pub reason: String,
}

impl Default for BridgeStatus {
    fn default() -> Self {
        Self {
            state: Mutex::new(BridgeState::Connecting),
            error: Mutex::new(None),
            served_bytes: AtomicU64::new(0),
            blocks: AtomicU64::new(0),
            local_port: AtomicU16::new(0),
            ports: Mutex::new(Vec::new()),
            in_flight: AtomicU64::new(0),
            peak_in_flight: AtomicU64::new(0),
            requests: AtomicU64::new(0),
            service_nanos: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            reconnect_wait_ms: AtomicU64::new(0),
            reconnect_reasons: Mutex::new(std::collections::BTreeMap::new()),
            gone_files: Mutex::new(Vec::new()),
            pieces_dropped: AtomicU64::new(0),
            pieces_retracted: AtomicU64::new(0),
            rejections: AtomicU64::new(0),
        }
    }
}

/// What one bridge's request pipeline is doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BridgePipeline {
    /// Blocks requested and not yet served.
    pub in_flight: u64,
    /// The most that were ever outstanding at once.
    pub peak_in_flight: u64,
    /// Blocks the session asked for.
    pub requests: u64,
    /// Blocks served.
    pub blocks: u64,
    /// Total request-to-answer time across those blocks.
    pub service_nanos: u64,
}

impl BridgePipeline {
    /// Mean time to answer one block, in microseconds. `None` when nothing
    /// has been served.
    pub fn mean_service_us(&self) -> Option<u64> {
        match self.blocks {
            0 => None,
            blocks => Some(self.service_nanos / blocks / 1000),
        }
    }

    /// What happened between an earlier reading and this one.
    ///
    /// The two gauges are levels rather than counts, so they are taken from
    /// the later reading rather than subtracted.
    pub fn since(&self, earlier: &Self) -> Self {
        Self {
            in_flight: self.in_flight,
            peak_in_flight: self.peak_in_flight,
            requests: self.requests.saturating_sub(earlier.requests),
            blocks: self.blocks.saturating_sub(earlier.blocks),
            service_nanos: self.service_nanos.saturating_sub(earlier.service_nanos),
        }
    }
}

impl BridgeStatus {
    /// What the bridge is doing.
    pub fn state(&self) -> BridgeState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The last problem reported, if any.
    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Bytes handed to the session.
    pub fn served_bytes(&self) -> u64 {
        self.served_bytes.load(Ordering::Relaxed)
    }

    /// Blocks handed to the session.
    pub fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }

    /// The loopback port this bridge is connected from right now, if it is
    /// connected.
    pub fn local_port(&self) -> Option<u16> {
        match self.local_port.load(Ordering::Relaxed) {
            0 => None,
            port => Some(port),
        }
    }

    /// Every loopback port this bridge has connected from, newest last.
    ///
    /// This is what tells a bridge apart from a real peer in the peer list,
    /// and it has to be the history rather than the current port: the session
    /// keeps a dead peer's row after the connection closes, and a bridge that
    /// disconnected is still not a swarm member.
    pub fn local_ports(&self) -> Vec<u16> {
        self.ports.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// How many times this bridge reconnected, and how long it waited to.
    ///
    /// A stalled run and a slow one look the same in the byte counts. They do
    /// not look the same here: a bridge that spent four minutes asleep between
    /// attempts says so. See `TODO/performance.md`, T-037.
    pub fn reconnects(&self) -> (u64, u64) {
        (
            self.reconnects.load(Ordering::Relaxed),
            self.reconnect_wait_ms.load(Ordering::Relaxed),
        )
    }

    /// Those reconnects by why the attempt before each one ended.
    pub fn reconnect_reasons(&self) -> std::collections::BTreeMap<&'static str, u64> {
        self.reconnect_reasons
            .lock()
            .map(|reasons| reasons.clone())
            .unwrap_or_default()
    }

    /// Charge one reconnect to a reason, with the wait that preceded it.
    /// Record that a file turned out not to be there.
    fn record_file_gone(&self, file: usize, pieces_dropped: usize, reason: &str) {
        self.pieces_dropped
            .fetch_add(pieces_dropped as u64, Ordering::Relaxed);
        if let Ok(mut gone) = self.gone_files.lock() {
            gone.push(GoneFile {
                file,
                pieces_dropped,
                reason: reason.to_string(),
            });
        }
    }

    /// Record that pieces were retracted on the wire rather than by
    /// reconnecting.
    ///
    /// Separate from `pieces_dropped`, which counts pieces given up however it
    /// happened. This counts the ones a `lt_donthave` carried, which is the
    /// number that says BEP 54 was used rather than the reconnect.
    fn record_retraction(&self, pieces: usize) {
        self.pieces_retracted
            .fetch_add(pieces as u64, Ordering::Relaxed);
    }

    /// Pieces retracted with BEP 54 `lt_donthave` on a live connection.
    pub fn pieces_retracted(&self) -> u64 {
        self.pieces_retracted.load(Ordering::Relaxed)
    }

    /// Record a request refused with BEP 6 rather than by hanging up.
    fn record_rejection(&self) {
        self.rejections.fetch_add(1, Ordering::Relaxed);
    }

    /// Requests answered with BEP 6 `reject request`.
    ///
    /// Every one of these was a connection the source would have lost before
    /// the fast extension, because the only way to refuse was to stop talking.
    pub fn rejections(&self) -> u64 {
        self.rejections.load(Ordering::Relaxed)
    }

    /// Files this source turned out not to hold, in the order they were found.
    pub fn gone_files(&self) -> Vec<GoneFile> {
        self.gone_files
            .lock()
            .map(|gone| gone.clone())
            .unwrap_or_default()
    }

    /// Pieces given up across every file lost.
    pub fn pieces_dropped(&self) -> u64 {
        self.pieces_dropped.load(Ordering::Relaxed)
    }

    fn record_reconnect(&self, reason: &'static str, waited: Duration) {
        self.reconnects.fetch_add(1, Ordering::Relaxed);
        self.reconnect_wait_ms.fetch_add(
            waited.as_millis().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
        if let Ok(mut reasons) = self.reconnect_reasons.lock() {
            *reasons.entry(reason).or_default() += 1;
        }
    }

    fn set_state(&self, state: BridgeState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
    }

    fn set_error(&self, reason: Option<String>) {
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) = reason;
    }

    fn set_local_port(&self, port: u16) {
        self.local_port.store(port, Ordering::Relaxed);
        if port == 0 {
            return;
        }
        let mut ports = self.ports.lock().unwrap_or_else(|e| e.into_inner());
        if ports.last() == Some(&port) {
            return;
        }
        // A run that reconnects for hours would otherwise keep one port per
        // attempt. The cap is generous against the number of dead peer rows a
        // session holds and small enough to be free.
        if ports.len() >= MAX_REMEMBERED_PORTS {
            ports.remove(0);
        }
        ports.push(port);
    }

    fn add_served(&self, bytes: u64) {
        self.served_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.blocks.fetch_add(1, Ordering::Relaxed);
    }

    /// Everything the request pipeline is doing right now.
    pub fn pipeline(&self) -> BridgePipeline {
        BridgePipeline {
            in_flight: self.in_flight.load(Ordering::Relaxed),
            peak_in_flight: self.peak_in_flight.load(Ordering::Relaxed),
            requests: self.requests.load(Ordering::Relaxed),
            blocks: self.blocks.load(Ordering::Relaxed),
            service_nanos: self.service_nanos.load(Ordering::Relaxed),
        }
    }

    /// The session asked for a block.
    fn request_received(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        let now = self.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak_in_flight.fetch_max(now, Ordering::Relaxed);
    }

    /// A requested block is no longer outstanding, whether it was served, was
    /// cancelled, or failed.
    fn request_settled(&self, elapsed: Duration) {
        saturating_decrement(&self.in_flight);
        self.service_nanos.fetch_add(
            elapsed.as_nanos().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    /// Drop every outstanding request. Called when the connection ends: the
    /// session will ask again on the next one, and counting the old requests
    /// as still in flight would report a depth that no longer exists.
    fn reset_in_flight(&self) {
        self.in_flight.store(0, Ordering::Relaxed);
    }
}

/// Take one off a counter, and stop at zero.
///
/// This is `fetch_update` with `saturating_sub`, written as the loop that
/// method is. `fetch_update` is deprecated from `rustc` 1.99, renamed to
/// `try_update`, and under CI's `-D warnings` a deprecation is an error rather
/// than a warning. The new name cannot be taken: it does not exist on the
/// pinned toolchain and it does not exist on the MSRV, 1.88, which
/// `TODO/RULES.md` section 6 says is measured rather than chosen. Silencing it
/// with `#[allow(deprecated)]` would hide the next rename in this file too.
///
/// The saturation is the part worth keeping. Every settle is paired with a
/// receive, so a plain `fetch_sub` would be correct in every path there is;
/// it would also wrap to `u64::MAX` the first time one is not, and the number
/// it is counting is reported. See `TODO/cli-surface.md`, T-218.
fn saturating_decrement(counter: &AtomicU64) {
    let mut current = counter.load(Ordering::Relaxed);
    while current > 0 {
        match counter.compare_exchange_weak(
            current,
            current - 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(seen) => current = seen,
        }
    }
}

/// Everything a bridge needs to present one source as a peer.
#[derive(Debug, Clone)]
pub struct BridgeParams {
    /// Where the session accepts incoming peer connections.
    pub listen_addr: SocketAddr,
    /// The torrent to attach to.
    pub info_hash: Id20,
    /// The session's own peer id, so the bridge avoids colliding with it.
    pub session_peer_id: Id20,
    /// Length of a non-final piece.
    pub piece_length: u32,
    /// Pieces in the torrent.
    pub total_pieces: u32,
    /// Piece indices this source can serve in full.
    pub pieces: Vec<u32>,
    /// Concurrent HTTP fetches.
    pub concurrency: usize,
    /// Client string sent in the extended handshake.
    pub client: String,
    /// Byte range of each file in the torrent, in index order.
    ///
    /// Kept so a source that loses one file can work out which pieces that
    /// file touches without carrying the whole layout. See
    /// `TODO/webseed.md`, T-005.
    pub file_spans: Vec<std::ops::Range<u64>>,
    /// Index of this source in the binding set, so a block can be attributed
    /// to the mirror that served it.
    pub source: usize,
    /// Where blocks this source puts on the wire are recorded.
    ///
    /// Shared with every other source on the same torrent: attribution is a
    /// statement about which of them sent a block, so it cannot be made from
    /// inside one of them. `None` leaves the bridge exactly as it was, which
    /// is what the protocol tests and `bench` use. See `TODO/webseed.md`,
    /// T-179.
    pub ledger: Option<Arc<crate::webseed::ledger::BlockLedger>>,
}

impl BridgeParams {
    /// Build the parameters for one binding against one torrent.
    ///
    /// The piece list is the binding's scope narrowed to whole pieces, which
    /// is the only set the source can serve without help.
    pub fn for_binding(
        listen_addr: SocketAddr,
        info_hash: Id20,
        session_peer_id: Id20,
        layout: &Layout,
        binding: &crate::webseed::binding::Binding,
        concurrency: usize,
    ) -> Self {
        Self {
            listen_addr,
            info_hash,
            session_peer_id,
            piece_length: layout.piece_length,
            total_pieces: layout.piece_count(),
            pieces: binding.scope.whole_pieces(layout),
            concurrency,
            client: format!("bit-cli/{}", crate::VERSION),
            file_spans: layout
                .files
                .iter()
                .map(|file| file.offset..file.offset + file.length)
                .collect(),
            source: binding.index,
            ledger: None,
        }
    }

    /// Record every block this source serves in a shared ledger.
    pub fn with_ledger(mut self, ledger: Arc<crate::webseed::ledger::BlockLedger>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    /// Whether piece `piece` overlaps file `file` by even one byte.
    ///
    /// One byte is the right threshold: a piece is verified against its whole
    /// hash, so a source missing any part of it cannot serve it at all. That
    /// is the same rule the announced bitfield already uses, which carries
    /// only pieces a scope covers **in full**.
    pub fn piece_touches(&self, piece: u32, file: usize) -> bool {
        let Some(span) = self.file_spans.get(file) else {
            return false;
        };
        if span.start >= span.end {
            // A zero-length file occupies no bytes, so no piece touches it and
            // losing it costs nothing.
            return false;
        }
        let start = u64::from(piece) * u64::from(self.piece_length);
        let end = start + u64::from(self.piece_length);
        start < span.end && span.start < end
    }

    /// Whether this source can serve the whole torrent.
    pub fn is_complete(&self) -> bool {
        self.pieces.len() as u32 == self.total_pieces
    }

    /// Size of the piece bitfield in bytes, as the wire format requires.
    pub fn bitfield_bytes(&self) -> usize {
        (self.total_pieces as usize).div_ceil(8)
    }
}

/// Why a bridge connection ended.
enum BridgeError {
    /// The source is unusable. Give up on it.
    Source(String),
    /// One file is permanently gone from this source, and the rest may still
    /// be there. Drop the pieces that file touches and reconnect with the
    /// smaller bitfield. See `TODO/webseed.md`, T-005.
    FileGone { file: usize, reason: String },
    /// The connection to the session failed. Reconnect later.
    Link(String),
    /// One request failed in a way that could still recover: the mirror is
    /// down, not wrong. Reconnect and let the source's own error budget decide
    /// when it has had enough. See [`retryable_failure`].
    Stalled(String),
}

/// Run a bridge until the source fails or the task is dropped.
///
/// Link failures retry with backoff, because a torrent that is not live yet
/// looks exactly like one from here. A source failure is terminal: the bridge
/// cannot retract a bitfield it has already sent, so staying connected while
/// refusing requests would only make the session wait out request timeouts.
///
/// A request that failed transiently and ran out of its own retries is
/// neither. The mirror answered, wrongly, and might answer correctly next
/// time: a 503 during a restart, or a 403 from a signature the caller told
/// `bit-cli` to retry. Those reconnect like a link failure, and what stops the
/// loop is the source's own budget: `max_errors` consecutive failed requests
/// trip its cooldown, and a cooling source's next fetch is permanent, which
/// retires it. Without that, one four-second outage killed a mirror for the
/// rest of the run and `--web-seed-max-errors` could never be reached. See
/// `TODO/multi-source.md`, T-130.
pub async fn run(params: BridgeParams, fetcher: Arc<Fetcher>, status: Arc<BridgeStatus>) {
    if params.pieces.is_empty() {
        status.set_state(BridgeState::Idle);
        status.set_error(Some(
            "the source's scope does not cover any whole piece, so it has nothing to serve"
                .to_string(),
        ));
        return;
    }

    // Owned and mutable, because a source can lose a file mid-run and the
    // piece list it announces has to shrink with it. See `TODO/webseed.md`,
    // T-005.
    let mut params = params;
    let mut delay = RECONNECT_BASE;
    loop {
        // Checked before dialling as well as inside the connection, because a
        // conviction landing while this bridge sits in its reconnect backoff
        // would otherwise be answered by connecting again.
        if let Some(reason) = fetcher.stats().banned() {
            status.set_error(Some(reason));
            status.set_state(BridgeState::Failed);
            return;
        }
        status.set_state(BridgeState::Connecting);
        let outcome = serve(&mut params, &fetcher, &status).await;
        status.set_local_port(0);
        status.reset_in_flight();
        // Named before the reason string is moved into the error slot, and
        // taken from the variant rather than the text, so a report groups by
        // what happened rather than by how it was worded.
        let ended = match &outcome {
            Ok(()) => "disconnected",
            Err(BridgeError::Source(_)) => "source",
            Err(BridgeError::Link(_)) => "link",
            Err(BridgeError::Stalled(_)) => "stalled",
            Err(BridgeError::FileGone { .. }) => "file_gone",
        };
        match outcome {
            Ok(()) => delay = RECONNECT_BASE,
            Err(BridgeError::Source(reason)) => {
                status.set_error(Some(reason));
                status.set_state(BridgeState::Failed);
                return;
            }
            Err(BridgeError::FileGone { file, reason }) => {
                // The source is healthy and smaller. Drop every piece the file
                // touches, because a piece needs all of its bytes and this
                // source can no longer supply that file's share of them, then
                // reconnect straight away with the smaller bitfield.
                //
                // The wire has no way to retract a bit already announced,
                // which is why this is a reconnect rather than a message. BEP
                // 54 `lt_donthave` is that message and
                // `TODO/bep-coverage.md` T-167 records why it cannot be used
                // here: `librqbit` 9.0.0 has no receive side for it.
                let before = params.pieces.len();
                // Computed against a copy of the piece list, because
                // `piece_touches` reads the rest of `params` while `retain`
                // holds the piece list mutably.
                let keep: Vec<u32> = params
                    .pieces
                    .iter()
                    .copied()
                    .filter(|piece| !params.piece_touches(*piece, file))
                    .collect();
                params.pieces = keep;
                let dropped = before - params.pieces.len();
                status.record_file_gone(file, dropped, &reason);
                if dropped == 0 {
                    // Cannot happen as the code stands: `serve` refuses a
                    // request for a piece this source did not announce, so any
                    // request that can fail is for an announced piece, and a
                    // request that names a file overlaps it. Guarded anyway,
                    // because the alternative to a guard here is a reconnect
                    // loop with no delay in it, and a hot loop is a worse way
                    // to find out that the invariant moved than an error is.
                    status.set_error(Some(format!(
                        "file {file} is gone and no announced piece touches it, so this source cannot narrow: {reason}"
                    )));
                    status.set_state(BridgeState::Failed);
                    return;
                }
                if params.pieces.is_empty() {
                    status.set_error(Some(format!(
                        "every piece this source covered is gone; the last was file {file}: {reason}"
                    )));
                    status.set_state(BridgeState::Failed);
                    return;
                }
                status.set_error(Some(reason));
                // No backoff. Nothing is wrong with the mirror or the link,
                // and the sooner the session sees the smaller bitfield the
                // sooner it stops waiting on pieces this source will not send.
                delay = RECONNECT_BASE;
                status.record_reconnect(ended, Duration::ZERO);
                continue;
            }
            Err(BridgeError::Link(reason)) => status.set_error(Some(reason)),
            Err(BridgeError::Stalled(reason)) => {
                // The budget running out is decided here rather than on the
                // next fetch, so the reported reason is the run of errors and
                // not the refusal that followed it.
                status.set_error(Some(reason.clone()));
                if fetcher.stats().budget_spent() {
                    // The deadline is read once and the wait derived from it,
                    // rather than reading the deadline and the remaining time
                    // separately. Connections sharing a source share one
                    // `SourceStats`, so another one clearing the cooldown
                    // between two reads would otherwise give this bridge a
                    // deadline with no wait, or a wait with no deadline.
                    let waiting = fetcher.stats().cooldown_until().and_then(|deadline| {
                        let left = deadline.epoch_ms() - crate::time::Timestamp::now().epoch_ms();
                        (left > 0).then(|| (deadline, Duration::from_millis(left as u64)))
                    });
                    match waiting {
                        // Nothing to wait for. `--web-seed-cooldown 0`, the
                        // default, means the source does not come back.
                        None => {
                            status.set_state(BridgeState::Failed);
                            return;
                        }
                        Some((deadline, remaining)) => {
                            status.set_state(BridgeState::Cooling);
                            tokio::time::sleep(remaining).await;
                            fetcher.stats().end_cooldown(deadline);
                            status.record_reconnect("cooldown", remaining);
                            // The mirror has had its time. Dial straight away
                            // rather than adding the link backoff on top of a
                            // wait the caller already chose.
                            delay = RECONNECT_BASE;
                            continue;
                        }
                    }
                }
            }
        }
        status.set_state(BridgeState::Connecting);
        tokio::time::sleep(delay).await;
        status.record_reconnect(ended, delay);
        delay = (delay * 2).min(RECONNECT_MAX);
    }
}

/// Connect to the session and serve requests until the connection ends.
///
/// `params` is taken by mutable reference because a source can lose a file
/// while this connection is up, and with BEP 54 that narrows the scope here
/// rather than on the next reconnect: the piece list the caller holds has to
/// shrink with it, or a later reconnect would announce pieces this source has
/// already retracted. See `TODO/bep-coverage.md`, T-167.
async fn serve(
    params: &mut BridgeParams,
    fetcher: &Arc<Fetcher>,
    status: &Arc<BridgeStatus>,
) -> Result<(), BridgeError> {
    let mut stream = TcpStream::connect(params.listen_addr)
        .await
        .map_err(|e| BridgeError::Link(format!("connect: {e}")))?;
    let _ = stream.set_nodelay(true);
    if let Ok(addr) = stream.local_addr() {
        status.set_local_port(addr.port());
    }

    let (mut read, mut write) = stream.split();
    let mut frames = Framer::default();

    let fast = handshake(params, &mut read, &mut write, &mut frames).await?;
    send_greeting(params, fast, &mut write).await?;
    status.set_error(None);
    status.set_state(BridgeState::Active);

    // Requests the session is still waiting on. Serving a piece it cancelled
    // makes it drop the peer, so a cancel has to be honoured.
    let pending: Arc<Mutex<HashSet<BlockKey>>> = Arc::default();
    let limiter = Arc::new(Semaphore::new(params.concurrency.max(1)));
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
    let mut tasks: JoinSet<Result<(), BlockFailure>> = JoinSet::new();
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    let mut served: HashSet<u32> = params.pieces.iter().copied().collect();
    // The id the session gave `lt_donthave`, once its extended handshake has
    // arrived. `None` until then, and `None` for good against a session that
    // does not speak BEP 54, which is what keeps sending one a no-op rather
    // than a message the far end warns about and ignores.
    let mut donthave: Option<u8> = None;
    // Pieces retracted with `lt_donthave` on this connection. A request for
    // one of them was already on the wire when the retraction went out, so it
    // is dropped rather than refused: refusing ends the connection, and the
    // whole point of BEP 54 is that this one does not end. The session's own
    // request timeout takes it from here, and `on_donthave` has already put
    // the piece back on the queue for another peer.
    let mut retracted: HashSet<u32> = HashSet::new();
    // Files already retracted on this connection. Every block that was in
    // flight against a file when it turned out to be gone fails the same way,
    // so the second failure and the tenth are the same news as the first, and
    // narrowing on them again would report a file twice and then retire the
    // source for being unable to narrow.
    let mut gone: HashSet<usize> = HashSet::new();

    loop {
        // A source convicted of serving wrong bytes stops here rather than at
        // the next fetch: the conviction is made outside this task, and a
        // mirror that is already known bad must not answer one more request
        // while it waits to be asked. `Source` is the variant that retires a
        // bridge for good, which is what a conviction means.
        if let Some(reason) = fetcher.stats().banned() {
            return Err(BridgeError::Source(reason));
        }

        // Drain what is already buffered before waiting for more.
        while let Some(frame) = frames.take_frame().map_err(BridgeError::Link)? {
            // A frame's message id follows the four byte length prefix, and a
            // keep-alive has neither. An extension message carries its
            // extension id one byte further on.
            //
            // Extension frames are dropped here rather than deserialized,
            // because this bridge advertised an empty `m` and so has nothing
            // to route one to. That has to be decided against `OUR_EXTENSIONS`
            // and nowhere else: `librqbit`'s decoder maps an incoming id
            // against its own constants, `MY_EXTENDED_UT_METADATA = 3` and
            // `MY_EXTENDED_UT_PEX = 1`, which this bridge never advertised, so
            // letting it decode reads the peer's id through a table neither
            // end agreed on. A body that then fails to parse as that type ends
            // the connection, which is a web seed lost to a message it had
            // already said it does not speak. See `TODO/peers.md`, T-166.
            if frame.get(4) == Some(&MSGID_EXTENDED) {
                let extension = frame.get(5).copied().unwrap_or(EXTENDED_HANDSHAKE);
                // The one exception, and BEP 10 is what makes it safe: the
                // extended handshake is id 0 in both directions, so it is the
                // only frame that can be read before a numbering is agreed.
                // What is kept out of it is the id to address `lt_donthave`
                // to, and nothing else.
                if extension == EXTENDED_HANDSHAKE {
                    if let Some(dict) = frame.get(6..) {
                        donthave = peer_donthave_id(dict);
                    }
                    continue;
                }
                if !is_our_extension(extension) {
                    continue;
                }
            }
            let message = Message::deserialize(&frame, &[])
                .map_err(|e| BridgeError::Link(format!("bad message: {e:?}")))?
                .0;
            // The inbound half of `--trace peer`: every message the session
            // sent this bridge, with its length, before anything decides what
            // to do about it. A message the arms below drop is still a message
            // that arrived, and a trace that only showed the handled ones
            // would be the wrong answer to "why did nothing happen".
            tracing::trace!(
                target: "bit_cli::peer",
                message = ?message,
                len = frame.len(),
                direction = "in",
                "message"
            );
            match message {
                Message::Request(request) => {
                    if request.length > MAX_REQUEST_LEN {
                        return Err(BridgeError::Link(format!(
                            "session asked for {} bytes in one block",
                            request.length
                        )));
                    }
                    // A request for a piece this source never announced is a
                    // session bug, and answering it would fetch bytes outside
                    // the scope. Refuse loudly rather than silently.
                    // BEP 6. A request this source cannot answer is refused
                    // with `reject request` rather than by ending the
                    // connection, which is the whole point of the message: a
                    // partial seed being asked for a piece it does not hold is
                    // a normal thing rather than a protocol error. Two cases
                    // reach it: a piece retracted with `lt_donthave` while the
                    // request was already on the wire, and a piece this source
                    // never announced.
                    //
                    // Without the extension a retracted piece is dropped
                    // silently, because there is no way to say no and refusing
                    // costs the connection.
                    if retracted.contains(&request.index) {
                        if fast {
                            out_tx
                                .send(serialize(
                                    &Message::RejectRequest(request),
                                    REQUEST_BODY_LEN,
                                )?)
                                .await
                                .map_err(|e| BridgeError::Link(format!("reject request: {e}")))?;
                        }
                        continue;
                    }
                    if !served.contains(&request.index) {
                        if fast {
                            out_tx
                                .send(serialize(
                                    &Message::RejectRequest(request),
                                    REQUEST_BODY_LEN,
                                )?)
                                .await
                                .map_err(|e| BridgeError::Link(format!("reject request: {e}")))?;
                            status.record_rejection();
                            continue;
                        }
                        return Err(BridgeError::Link(format!(
                            "session asked for piece {}, which this source did not announce",
                            request.index
                        )));
                    }
                    let key = (request.index, request.begin, request.length);
                    pending
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(key);
                    status.request_received();
                    tasks.spawn(serve_block(
                        key,
                        offset_of(params, request.index, request.begin),
                        limiter.clone(),
                        fetcher.clone(),
                        status.clone(),
                        pending.clone(),
                        out_tx.clone(),
                        params.ledger.clone().map(|ledger| (params.source, ledger)),
                    ));
                }
                Message::Cancel(request) => {
                    pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&(
                        request.index,
                        request.begin,
                        request.length,
                    ));
                }
                // The bridge only seeds, so what the session says about its
                // own progress or interest changes nothing here. Extension
                // messages never reach this arm: they are dropped above, at
                // the frame, against the numbering this bridge advertised.
                _ => {}
            }
        }

        tokio::select! {
            read = frames.fill(&mut read) => {
                match read {
                    Ok(0) => return Err(BridgeError::Link("session closed the connection".into())),
                    Ok(_) => {}
                    Err(e) => return Err(BridgeError::Link(format!("read: {e}"))),
                }
            }
            Some(message) = out_rx.recv() => {
                write.write_all(&message).await
                    .map_err(|e| BridgeError::Link(format!("write: {e}")))?;
            }
            Some(finished) = tasks.join_next(), if !tasks.is_empty() => {
                match finished {
                    Ok(Ok(())) => {}
                    Ok(Err(failure)) => {
                        let ended = retryable_failure(failure);
                        // BEP 54. A file that turned out not to be there
                        // narrows this source, and the wire has a way to say
                        // so now: one `lt_donthave` per piece given up, on the
                        // connection that is already open. Without it the only
                        // way to retract a bit is to drop the connection and
                        // announce a smaller bitfield, which is what `run`
                        // still does when the session does not speak BEP 54.
                        // See `TODO/bep-coverage.md`, T-167.
                        let BridgeError::FileGone { file, reason } = &ended else {
                            return Err(ended);
                        };
                        let Some(extension) = donthave else {
                            return Err(ended);
                        };
                        if gone.contains(file) {
                            continue;
                        }
                        let (keep, dropped) = split_on_file(params, *file);
                        // Both terminal cases are the caller's: it has the
                        // reporting and the state machine for a source that
                        // cannot narrow and one that has nothing left.
                        if dropped.is_empty() || keep.is_empty() {
                            return Err(ended);
                        }
                        status.record_file_gone(*file, dropped.len(), reason);
                        for piece in &dropped {
                            let message = serialize_donthave(*piece, extension);
                            write.write_all(&message).await.map_err(|e| {
                                BridgeError::Link(format!("lt_donthave: {e}"))
                            })?;
                        }
                        // Announced and served have to move together, or a
                        // request for a retracted piece would be answered from
                        // a file this source no longer has.
                        served = keep.iter().copied().collect();
                        retracted.extend(dropped.iter().copied());
                        gone.insert(*file);
                        params.pieces = keep;
                        status.record_retraction(dropped.len());
                    }
                    Err(e) if e.is_panic() => {
                        return Err(BridgeError::Source(format!("bridge task panicked: {e}")));
                    }
                    Err(_) => {}
                }
            }
            _ = keep_alive.tick() => {
                write.write_all(&KEEP_ALIVE).await
                    .map_err(|e| BridgeError::Link(format!("keep-alive: {e}")))?;
            }
        }
    }
}

/// Exchange handshakes and confirm the session routed us to the right torrent.
///
/// Returns whether the session set BEP 6's reserved bit. `Handshake::new` sets
/// ours, so this is the negotiation: both ends or neither.
async fn handshake(
    params: &BridgeParams,
    read: &mut (impl tokio::io::AsyncRead + Unpin),
    write: &mut (impl tokio::io::AsyncWrite + Unpin),
    frames: &mut Framer,
) -> Result<bool, BridgeError> {
    let mut peer_id = Id20::new(crate::peer_id::generate(&PEER_ID_PREFIX));
    while peer_id == params.session_peer_id {
        peer_id = Id20::new(crate::peer_id::generate(&PEER_ID_PREFIX));
    }

    // `Handshake::new` sets the BEP 10 extension bit, which is what carries
    // the BEP 21 `upload_only` flag in the extended handshake below.
    let ours = Handshake::new(params.info_hash, peer_id);
    let mut buf = [0u8; HANDSHAKE_LEN];
    let len = ours.serialize_unchecked_len(&mut buf);
    // What `--trace handshake` promises, outbound half. This bridge is a real
    // peer as far as the session is concerned, so this is a real handshake:
    // the id it will be known by, the info hash it claims, and the extension
    // bits it sets. See `TODO/cli-surface.md`, T-219.
    tracing::trace!(
        target: "bit_cli::handshake",
        peer_id = %peer_id.as_string(),
        info_hash = %params.info_hash.as_string(),
        direction = "out",
        "handshake"
    );
    write
        .write_all(&buf[..len])
        .await
        .map_err(|e| BridgeError::Link(format!("write handshake: {e}")))?;

    loop {
        match Handshake::deserialize(frames.buffered()) {
            Ok((theirs, size)) => {
                tracing::trace!(
                    target: "bit_cli::handshake",
                    peer_id = %theirs.peer_id.as_string(),
                    info_hash = %theirs.info_hash.as_string(),
                    supports_fast = theirs.supports_fast(),
                    supports_extended = theirs.supports_extended(),
                    direction = "in",
                    "handshake"
                );
                if theirs.info_hash != params.info_hash {
                    return Err(BridgeError::Link(
                        "session sent a different infohash".into(),
                    ));
                }
                frames.consume(size);
                return Ok(theirs.supports_fast());
            }
            Err(_) => {
                let n = frames
                    .fill(read)
                    .await
                    .map_err(|e| BridgeError::Link(format!("read handshake: {e}")))?;
                if n == 0 {
                    return Err(BridgeError::Link("session closed during handshake".into()));
                }
            }
        }
    }
}

/// Announce what this source holds, then unchoke.
///
/// The order matters: the extended handshake carries the BEP 21 flag and has
/// to arrive before the bitfield, so the session knows it is looking at a
/// partial seed rather than a peer that is still downloading.
async fn send_greeting(
    params: &BridgeParams,
    fast: bool,
    write: &mut (impl tokio::io::AsyncWrite + Unpin),
) -> Result<(), BridgeError> {
    let bits = bitfield(params);
    let mut out = extended_handshake(params);
    // BEP 6. A source whose scope is the whole torrent says so in two bytes
    // rather than one bit per piece, which on a million piece torrent is
    // 128 KiB saved on every connection. Only when the session negotiated it:
    // a `have all` to a peer that does not know the message is a dropped
    // connection. See TODO/bep-coverage.md, T-100.
    let announce = match fast && params.is_complete() {
        true => Message::HaveAll,
        false => Message::Bitfield(ByteBuf(&bits)),
    };
    // The extension negotiation half of what `--trace handshake` promises.
    // `upload_only` is the BEP 21 flag the dictionary carries and it is the
    // one field of it that changes what the session does with this peer.
    tracing::trace!(
        target: "bit_cli::handshake",
        upload_only = !params.is_complete(),
        extensions = 0,
        bytes = out.len(),
        direction = "out",
        "extended handshake"
    );
    for message in [announce, Message::Unchoke] {
        // The greeting is wire traffic, so it goes to `peer` rather than to
        // `handshake`: the two subsystems split at the point where the
        // connection is negotiated and messages start.
        tracing::trace!(
            target: "bit_cli::peer",
            message = ?message,
            direction = "out",
            "message"
        );
        out.extend_from_slice(&serialize(&message, bits.len())?);
    }
    write
        .write_all(&out)
        .await
        .map_err(|e| BridgeError::Link(format!("write bitfield: {e}")))
}

/// The piece bitfield for this source's scope.
///
/// Bit `i` is set when the source covers piece `i` in full. Spare bits past
/// the last piece stay zero, as the wire format requires.
fn bitfield(params: &BridgeParams) -> Vec<u8> {
    let mut bits = vec![0u8; params.bitfield_bytes()];
    for &piece in &params.pieces {
        if piece >= params.total_pieces {
            continue;
        }
        let index = piece as usize;
        bits[index / 8] |= 0x80 >> (index % 8);
    }
    bits
}

/// The BEP 10 extended handshake, carrying the BEP 21 partial seed flag.
///
/// The dictionary is written by hand because the bridge supports no extension
/// messages at all, and an empty `m` is the honest way to say so. Keys are in
/// ascending byte order, as bencode requires.
fn extended_handshake(params: &BridgeParams) -> Vec<u8> {
    let client = &params.client;
    let mut dict = Vec::new();
    dict.push(b'd');
    dict.extend_from_slice(b"1:mde");
    dict.extend_from_slice(format!("4:reqqi{REQUEST_QUEUE}e").as_bytes());
    // BEP 21. A source that holds only part of the payload says so, so the
    // session treats it as a partial seed rather than as a leecher.
    if !params.is_complete() {
        dict.extend_from_slice(b"11:upload_onlyi1e");
    }
    dict.extend_from_slice(format!("1:v{}:{client}", client.len()).as_bytes());
    dict.push(b'e');

    let mut out = Vec::with_capacity(dict.len() + 6);
    out.extend_from_slice(&((dict.len() as u32) + 2).to_be_bytes());
    out.push(MSGID_EXTENDED);
    out.push(EXTENDED_HANDSHAKE);
    out.extend_from_slice(&dict);
    out
}

/// The pieces that survive losing one file, and the pieces that do not.
///
/// A piece needs all of its bytes, so a piece that touches a file this source
/// cannot serve any more is one this source cannot serve any more.
fn split_on_file(params: &BridgeParams, file: usize) -> (Vec<u32>, Vec<u32>) {
    let mut keep = Vec::with_capacity(params.pieces.len());
    let mut dropped = Vec::new();
    for &piece in &params.pieces {
        if params.piece_touches(piece, file) {
            dropped.push(piece);
        } else {
            keep.push(piece);
        }
    }
    (keep, dropped)
}

/// The extension id the session gave `lt_donthave`, from its own handshake.
///
/// BEP 10 numbers extension messages **per receiver**: the id in a message is
/// the one the receiver advertised, not the sender's. So sending one costs a
/// second table, read out of the peer's `m` rather than out of
/// [`OUR_EXTENSIONS`], and this is that table for the one extension the bridge
/// sends. See `TODO/peers.md` T-166 and `TODO/bep-coverage.md` T-167.
///
/// The extended handshake is the one frame that can be decoded without
/// agreeing a numbering first, because BEP 10 fixes it at id 0 in both
/// directions. Everything after it still goes through `OUR_EXTENSIONS`.
fn peer_donthave_id(dict: &[u8]) -> Option<u8> {
    use crate::torrent::bencode::{Value, decode};

    let value = decode(dict).ok()?;
    let m = value.get("m")?;
    let id = m.get("lt_donthave").and_then(Value::as_int)?;
    u8::try_from(id).ok().filter(|id| *id != EXTENDED_HANDSHAKE)
}

/// One `lt_donthave`, addressed with the id the session advertised.
///
/// BEP 54: the payload is a four byte big-endian piece index and nothing else,
/// which is why it is built here rather than through the bencode path every
/// other extension message takes.
fn serialize_donthave(piece: u32, extension: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 6);
    out.extend_from_slice(&6u32.to_be_bytes());
    out.push(MSGID_EXTENDED);
    out.push(extension);
    out.extend_from_slice(&piece.to_be_bytes());
    out
}

/// Serialize one message into a fresh buffer.
///
/// `librqbit` serializes into a caller-sized slice, so `payload` is the size
/// of whatever variable-length body the message carries.
fn serialize(message: &Message<'_>, payload: usize) -> Result<Vec<u8>, BridgeError> {
    let mut buf = vec![0u8; MESSAGE_OVERHEAD + payload];
    let len = message
        .serialize(&mut buf, &Default::default)
        .map_err(|e| BridgeError::Link(format!("serialize: {e}")))?;
    buf.truncate(len);
    Ok(buf)
}

/// A block the session asked for, as `(piece, offset in piece, length)`.
type BlockKey = (u32, u32, u32);

/// Why one block could not be served, and whether the source is finished.
///
/// The fetcher has already spent this request's `retries` by the time an error
/// gets here. What it has not spent is the source's error budget, so a failure
/// that could recover is kept apart from one that cannot.
struct BlockFailure {
    reason: String,
    /// Whether the source could still answer a later request.
    recoverable: bool,
    /// The file the failing request was addressed to, when it was addressed to
    /// one. A permanent failure with a file named narrows the source to what
    /// it can still serve rather than retiring it. See `TODO/webseed.md`,
    /// T-005.
    file: Option<usize>,
}

impl From<crate::webseed::fetch::ReadFailure> for BlockFailure {
    fn from(failure: crate::webseed::fetch::ReadFailure) -> Self {
        let file = failure.file;
        let mut out = Self::from(failure.error);
        out.file = file;
        out
    }
}

impl From<FetchError> for BlockFailure {
    fn from(err: FetchError) -> Self {
        // A stall is recoverable here even though it is not retryable inside
        // the request. The two questions are different: the retry ladder asks
        // whether asking this mirror again in half a second could work, and
        // this asks whether the source is finished. A stall goes down the
        // `Stalled` path so the bridge consults the error budget, which the
        // fetcher has already tripped. See `TODO/webseed.md`, T-007.
        let recoverable = err.is_retryable() || err.is_stall();
        let reason = match err {
            FetchError::Transient { reason, .. }
            | FetchError::Permanent { reason, .. }
            | FetchError::Stalled { reason, .. }
            | FetchError::HashMismatch { reason } => reason,
        };
        Self {
            reason,
            recoverable,
            file: None,
        }
    }
}

impl BlockFailure {
    /// A failure inside the bridge that has nothing to do with the source.
    fn local(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            recoverable: false,
            file: None,
        }
    }
}

/// Turn a block failure into the reason a connection ended.
fn retryable_failure(failure: BlockFailure) -> BridgeError {
    match (failure.recoverable, failure.file) {
        (true, _) => BridgeError::Stalled(failure.reason),
        // A permanent failure on a request addressed to one file is that
        // file's, not the source's. `bit-cli` exists to treat a mirror holding
        // part of a payload as a first-class case, and retiring the whole
        // source over one file contradicts that in the one place it matters.
        (false, Some(file)) => BridgeError::FileGone {
            file,
            reason: failure.reason,
        },
        (false, None) => BridgeError::Source(failure.reason),
    }
}

/// The text inside a [`BridgeError`], whichever kind it is.
fn reason_of(err: BridgeError) -> String {
    match err {
        BridgeError::Source(reason)
        | BridgeError::Link(reason)
        | BridgeError::Stalled(reason)
        | BridgeError::FileGone { reason, .. } => reason,
    }
}

/// Fetch one block over HTTP and queue it, unless the session cancelled it.
/// Which source a block is recorded against, and where it is recorded.
///
/// `None` for a bridge with no ledger, which is every caller but `download`.
type Attribution = Option<(usize, Arc<crate::webseed::ledger::BlockLedger>)>;

#[allow(clippy::too_many_arguments)]
async fn serve_block(
    key: BlockKey,
    offset: u64,
    limiter: Arc<Semaphore>,
    fetcher: Arc<Fetcher>,
    status: Arc<BridgeStatus>,
    pending: Arc<Mutex<HashSet<BlockKey>>>,
    out: mpsc::Sender<Vec<u8>>,
    attribution: Attribution,
) -> Result<(), BlockFailure> {
    // The clock starts when the request was taken off the wire, not when this
    // task gets a permit: time spent waiting for the concurrency limit is
    // time the session was waiting, and hiding it would report a pipeline
    // that answers faster than it does.
    let started = std::time::Instant::now();
    let outcome = fetch_and_send(
        key,
        offset,
        limiter,
        fetcher,
        &status,
        pending,
        out,
        attribution,
    )
    .await;
    status.request_settled(started.elapsed());
    // What `--trace piece` promises: the request, its receipt, and the timing,
    // for the one piece path this repository's own code decides. The clock is
    // the one `request_settled` is charged, so the number here and the
    // pipeline number in the report are the same number. See
    // `TODO/cli-surface.md`, T-219.
    let (index, begin, length) = key;
    tracing::trace!(
        target: "bit_cli::piece",
        piece = index,
        begin,
        length,
        offset,
        micros = started.elapsed().as_micros() as u64,
        error = ?outcome.as_ref().err().map(|e| e.reason.as_str()),
        "served a block"
    );
    outcome
}

#[allow(clippy::too_many_arguments)]
async fn fetch_and_send(
    key: BlockKey,
    offset: u64,
    limiter: Arc<Semaphore>,
    fetcher: Arc<Fetcher>,
    status: &BridgeStatus,
    pending: Arc<Mutex<HashSet<BlockKey>>>,
    out: mpsc::Sender<Vec<u8>>,
    attribution: Attribution,
) -> Result<(), BlockFailure> {
    let (index, begin, length) = key;
    let _permit = limiter
        .acquire()
        .await
        .map_err(|e| BlockFailure::local(e.to_string()))?;
    let block = match fetcher.read_block(offset, u64::from(length)).await {
        Ok(block) => block,
        Err(failure) => return Err(BlockFailure::from(failure)),
    };

    if !pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&key)
    {
        return Ok(());
    }

    // Recorded after the cancel race is won and before the bytes go out, so
    // the ledger holds exactly the blocks that reached the session. A block
    // fetched and then dropped never entered a piece and must not be able to
    // convict anyone. See `TODO/webseed.md`, T-179.
    if let Some((source, ledger)) = &attribution {
        ledger.record(*source, key, &block);
    }

    let message = Message::Piece(Piece::from_data(index, begin, &block));
    let buf = serialize(&message, block.len()).map_err(|e| BlockFailure::local(reason_of(e)))?;
    status.add_served(block.len() as u64);
    let _ = out.send(buf).await;
    Ok(())
}

/// Torrent byte offset of a block within a piece.
fn offset_of(params: &BridgeParams, piece: u32, begin: u32) -> u64 {
    u64::from(piece) * u64::from(params.piece_length) + u64::from(begin)
}

/// Length-prefixed message framing over a byte stream.
///
/// Buffered bytes live outside the read future, which keeps [`Framer::fill`]
/// cancel-safe and usable directly in a `select!`.
#[derive(Default)]
pub(crate) struct Framer {
    buf: Vec<u8>,
}

impl Framer {
    /// Bytes received but not yet consumed.
    pub(crate) fn buffered(&self) -> &[u8] {
        &self.buf
    }

    /// Drop the first `n` buffered bytes.
    pub(crate) fn consume(&mut self, n: usize) {
        self.buf.drain(..n);
    }

    /// Read whatever is available. Zero means end of stream.
    pub(crate) async fn fill(
        &mut self,
        read: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> std::io::Result<usize> {
        let mut chunk = [0u8; 8192];
        let n = read.read(&mut chunk).await?;
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(n)
    }

    /// Take one complete length-prefixed frame, if the buffer holds one.
    pub(crate) fn take_frame(&mut self) -> Result<Option<Vec<u8>>, String> {
        let Some(prefix) = self.buf.get(..4) else {
            return Ok(None);
        };
        let len = u32::from_be_bytes(prefix.try_into().unwrap_or([0; 4])) as usize;
        if len > MAX_FRAME_LEN {
            return Err(format!("session sent a {len} byte frame"));
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        Ok(Some(self.buf.drain(..4 + len).collect()))
    }
}

#[cfg(test)]
mod tests {
    /// The counter stops at zero rather than wrapping.
    ///
    /// The saturation is the whole reason this is not `fetch_sub`, and nothing
    /// held it: the closure it replaced was never covered either. A wrapped
    /// counter is reported as `in_flight`, so the failure is a number a reader
    /// believes. See `TODO/cli-surface.md`, T-218.
    #[test]
    fn a_settled_request_never_takes_the_counter_below_zero() {
        let counter = AtomicU64::new(2);
        saturating_decrement(&counter);
        saturating_decrement(&counter);
        assert_eq!(counter.load(Ordering::Relaxed), 0);
        saturating_decrement(&counter);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            0,
            "the counter wrapped instead of stopping"
        );
    }

    use super::*;
    use crate::webseed::binding::{BindingSet, Origin, SourceSpec};
    use crate::webseed::scope::Scope;

    const HASH: &str = "0102030405060708090a0b0c0d0e0f1011121314";

    fn layout() -> Layout {
        // Four pieces of 1024 bytes, then a short fifth.
        Layout::from_lengths("payload", false, 1024, [("payload".to_string(), 4500u64)])
    }

    fn params(scope: &str) -> BridgeParams {
        let layout = layout();
        let spec = SourceSpec::new("https://e.example/payload", Origin::CommandLine)
            .with_scope(Scope::parse(scope).unwrap());
        let set = BindingSet::resolve(&layout, HASH, &[spec]).unwrap();
        BridgeParams::for_binding(
            "127.0.0.1:1".parse().unwrap(),
            Id20::new([0u8; 20]),
            Id20::new([1u8; 20]),
            &layout,
            &set.bindings[0],
            4,
        )
    }

    #[test]
    fn block_offsets_are_absolute() {
        let p = params("*");
        assert_eq!(offset_of(&p, 0, 0), 0);
        assert_eq!(offset_of(&p, 0, 512), 512);
        assert_eq!(offset_of(&p, 3, 16), 3 * 1024 + 16);
    }

    #[test]
    fn a_whole_torrent_source_announces_every_piece() {
        let p = params("*");
        assert_eq!(p.total_pieces, 5);
        assert_eq!(p.pieces, vec![0, 1, 2, 3, 4]);
        assert!(p.is_complete());
        // Five pieces occupy one byte, so the low three bits stay clear.
        assert_eq!(bitfield(&p), vec![0b1111_1000]);
    }

    #[test]
    fn a_scoped_source_announces_only_the_pieces_it_covers_in_full() {
        let p = params("byte:0-2048");
        assert!(!p.is_complete());
        assert_eq!(
            p.pieces,
            vec![0, 1],
            "bytes 0-2047 are exactly pieces 0 and 1"
        );
        assert_eq!(bitfield(&p), vec![0b1100_0000]);
    }

    #[test]
    fn a_partially_covered_piece_is_never_announced() {
        // Bytes 0 to 1535 cover piece 0 in full and only half of piece 1.
        // Announcing piece 1 would make the session request bytes this source
        // cannot serve, and the piece would never verify.
        let p = params("byte:0-1536");
        assert_eq!(p.pieces, vec![0]);
        assert_eq!(bitfield(&p), vec![0b1000_0000]);
    }

    #[test]
    fn the_bitfield_is_the_right_length_for_the_piece_count() {
        let layout = Layout::from_lengths("t", false, 1024, [("t".to_string(), 12 * 1024u64)]);
        let spec = SourceSpec::new("https://e.example/t", Origin::CommandLine);
        let set = BindingSet::resolve(&layout, HASH, &[spec]).unwrap();
        let p = BridgeParams::for_binding(
            "127.0.0.1:1".parse().unwrap(),
            Id20::new([0u8; 20]),
            Id20::new([1u8; 20]),
            &layout,
            &set.bindings[0],
            1,
        );
        assert_eq!(p.total_pieces, 12);
        assert_eq!(p.bitfield_bytes(), 2);
        // Twelve pieces in two bytes, so the low four bits are spare.
        assert_eq!(bitfield(&p), vec![0xFF, 0xF0]);
    }

    #[test]
    fn a_complete_source_does_not_claim_to_be_upload_only() {
        let dict = extended_handshake(&params("*"));
        let text = String::from_utf8_lossy(&dict).into_owned();
        assert!(!text.contains("upload_only"), "{text}");
    }

    #[test]
    fn a_partial_source_advertises_bep_21() {
        let dict = extended_handshake(&params("byte:0-2048"));
        let text = String::from_utf8_lossy(&dict).into_owned();
        assert!(text.contains("11:upload_onlyi1e"), "{text}");
    }

    #[test]
    fn the_extended_handshake_is_a_well_formed_frame() {
        let frame = extended_handshake(&params("*"));
        let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(
            len,
            frame.len() - 4,
            "the length prefix covers the rest of the frame"
        );
        assert_eq!(frame[4], MSGID_EXTENDED);
        assert_eq!(frame[5], EXTENDED_HANDSHAKE);
        assert_eq!(frame[6], b'd', "the payload is a bencoded dictionary");
        assert_eq!(frame[frame.len() - 1], b'e');
    }

    #[test]
    fn the_extended_handshake_advertises_no_extension_messages() {
        // An empty `m` is what says "I speak the extension protocol but
        // implement none of its messages", which is exactly true here.
        let frame = extended_handshake(&params("*"));
        let text = String::from_utf8_lossy(&frame).into_owned();
        assert!(text.contains("1:mde"), "{text}");
        assert!(!text.contains("ut_metadata"), "{text}");
        assert!(!text.contains("ut_pex"), "{text}");
    }

    /// The two BEP 10 numberings, and the rule that keeps them apart.
    ///
    /// An incoming extension id is decided by [`OUR_EXTENSIONS`] and by
    /// nothing else, and the table has to say the same thing the advertised
    /// `m` says on the wire. An entry added to one without the other is the
    /// drift this guards against, and `librqbit`'s own receive-side numbering
    /// is the table it must never be read as. See `TODO/peers.md`, T-166.
    #[test]
    fn an_incoming_extension_id_is_only_read_against_our_own_map() {
        for id in 0..=u8::MAX {
            assert_eq!(
                is_our_extension(id),
                OUR_EXTENSIONS.iter().any(|(_, ours)| *ours == id),
                "id {id} is decided by a table other than OUR_EXTENSIONS"
            );
        }

        // `MY_EXTENDED_UT_PEX` is 1 and `MY_EXTENDED_UT_METADATA` is 3 in
        // `librqbit-peer-protocol` 9.0.0. Those are that crate's ids for what
        // it advertises, not this bridge's, and reading an incoming id as one
        // of them is what cost a connection before T-166.
        assert!(!is_our_extension(1));
        assert!(!is_our_extension(3));

        let text = String::from_utf8_lossy(&extended_handshake(&params("*"))).into_owned();
        for (name, _) in OUR_EXTENSIONS {
            assert!(text.contains(name), "`m` does not advertise {name}: {text}");
        }
        assert_eq!(
            OUR_EXTENSIONS.is_empty(),
            text.contains("1:mde"),
            "an empty table and an empty `m` are the same statement: {text}"
        );
    }

    #[test]
    fn the_extended_handshake_keys_are_in_bencode_order() {
        let frame = extended_handshake(&params("byte:0-2048"));
        let text = String::from_utf8_lossy(&frame).into_owned();
        let m = text.find("1:m").unwrap();
        let reqq = text.find("4:reqq").unwrap();
        let upload = text.find("11:upload_only").unwrap();
        let v = text.rfind("1:v").unwrap();
        assert!(m < reqq && reqq < upload && upload < v, "{text}");
    }

    /// BEP 10 numbers extension messages per receiver, so the id to send
    /// `lt_donthave` to comes out of the session's handshake and nowhere else.
    #[test]
    fn the_peers_donthave_id_is_read_from_its_own_m() {
        let dict = b"d1:md11:lt_donthavei7e11:ut_metadatai3ee4:reqqi250ee";
        assert_eq!(peer_donthave_id(dict), Some(7));
    }

    #[test]
    fn a_session_that_does_not_speak_bep_54_gives_no_id() {
        let dict = b"d1:md11:ut_metadatai3e6:ut_pexi1ee4:reqqi250ee";
        assert_eq!(peer_donthave_id(dict), None);
        // An empty `m`, which is what this bridge itself sends.
        assert_eq!(peer_donthave_id(b"d1:mde4:reqqi250ee"), None);
        // Not a dictionary at all.
        assert_eq!(peer_donthave_id(b"le"), None);
    }

    /// Id 0 is the extended handshake in both directions. A peer that
    /// advertises `lt_donthave` there is either broken or trying, and sending
    /// to id 0 would be sending a handshake.
    #[test]
    fn extension_id_zero_is_never_taken_as_a_message_id() {
        assert_eq!(peer_donthave_id(b"d1:md11:lt_donthavei0eee"), None);
    }

    /// BEP 54's payload is four big-endian bytes and no bencode, which is the
    /// detail every other extension message in the protocol does not share.
    #[test]
    fn a_donthave_is_ten_bytes_on_the_wire() {
        let frame = serialize_donthave(0x0102_0304, 7);
        assert_eq!(frame.len(), 10);
        assert_eq!(&frame[..4], &6u32.to_be_bytes(), "length prefix");
        assert_eq!(frame[4], MSGID_EXTENDED);
        assert_eq!(frame[5], 7, "the id the session advertised");
        assert_eq!(&frame[6..], &[0x01, 0x02, 0x03, 0x04], "big endian piece");
    }

    /// A file this source cannot serve takes every piece that touches it, and
    /// leaves every piece that does not.
    #[test]
    fn losing_a_file_splits_the_piece_list_on_it() {
        let params = params("*");
        let (keep, dropped) = split_on_file(&params, 0);
        assert!(!dropped.is_empty(), "file 0 has to touch some piece");
        assert_eq!(
            keep.len() + dropped.len(),
            params.pieces.len(),
            "every announced piece is in exactly one of the two"
        );
        for piece in &dropped {
            assert!(params.piece_touches(*piece, 0));
        }
        for piece in &keep {
            assert!(!params.piece_touches(*piece, 0));
        }
    }

    #[test]
    fn framer_yields_whole_frames_only() {
        let mut framer = Framer::default();
        framer.buf.extend_from_slice(&[0, 0, 0, 2, 9]);
        assert_eq!(framer.take_frame().unwrap(), None);
        framer.buf.push(7);
        assert_eq!(framer.take_frame().unwrap(), Some(vec![0, 0, 0, 2, 9, 7]));
        assert_eq!(framer.take_frame().unwrap(), None);
    }

    #[test]
    fn framer_handles_keep_alives_and_back_to_back_frames() {
        let mut framer = Framer::default();
        framer
            .buf
            .extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0]);
        assert_eq!(framer.take_frame().unwrap(), Some(vec![0, 0, 0, 0]));
        assert_eq!(framer.take_frame().unwrap(), Some(vec![0, 0, 0, 1, 1]));
        assert_eq!(framer.take_frame().unwrap(), Some(vec![0, 0, 0, 0]));
        assert_eq!(framer.take_frame().unwrap(), None);
    }

    #[test]
    fn framer_rejects_absurd_frames() {
        let mut framer = Framer::default();
        framer.buf.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(framer.take_frame().is_err());
    }

    #[test]
    fn a_fresh_status_reports_nothing_served() {
        let status = BridgeStatus::default();
        assert_eq!(status.state(), BridgeState::Connecting);
        assert_eq!(status.served_bytes(), 0);
        assert_eq!(status.blocks(), 0);
        assert_eq!(status.local_port(), None);
        assert_eq!(status.error(), None);

        status.add_served(1024);
        status.add_served(512);
        assert_eq!(status.served_bytes(), 1536);
        assert_eq!(status.blocks(), 2);
    }

    /// A bridge that spent its run waiting says so.
    ///
    /// T-037 is a run that took 274 seconds where the same command usually
    /// takes 3.2, with only 5.2 seconds of CPU behind it. Nothing in the
    /// report distinguished that from a slow mirror. The reconnect counters
    /// do: the wait is charged where it was spent and grouped by what ended
    /// the attempt before it. See `TODO/performance.md`, T-037.
    #[test]
    fn reconnects_are_counted_with_the_wait_they_cost_and_why() {
        let status = BridgeStatus::default();
        assert_eq!(status.reconnects(), (0, 0));
        assert!(status.reconnect_reasons().is_empty());

        status.record_reconnect("link", Duration::from_millis(1000));
        status.record_reconnect("link", Duration::from_millis(2000));
        status.record_reconnect("disconnected", Duration::from_millis(1000));

        let (count, waited) = status.reconnects();
        assert_eq!(count, 3);
        assert_eq!(waited, 4000);
        let reasons = status.reconnect_reasons();
        assert_eq!(reasons.get("link"), Some(&2));
        assert_eq!(reasons.get("disconnected"), Some(&1));
        assert_eq!(reasons.len(), 2, "only reasons that happened: {reasons:?}");
    }

    /// The backoff the run loop uses, checked as arithmetic rather than by
    /// waiting it out.
    ///
    /// It starts at one second, doubles, and stops at thirty. That is what
    /// sets the price of a stall: thirteen consecutive failures is 271
    /// seconds, which is the shape of the run T-037 recorded.
    #[test]
    fn the_reconnect_backoff_doubles_to_a_thirty_second_ceiling() {
        let mut delay = RECONNECT_BASE;
        let mut waited = Duration::ZERO;
        let mut steps = Vec::new();
        for _ in 0..13 {
            steps.push(delay.as_secs());
            waited += delay;
            delay = (delay * 2).min(RECONNECT_MAX);
        }
        assert_eq!(steps, vec![1, 2, 4, 8, 16, 30, 30, 30, 30, 30, 30, 30, 30]);
        assert_eq!(waited.as_secs(), 271);
    }
}
