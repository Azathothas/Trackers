//! Synthetic peer load against one target.
//!
//! Every other `bench` subcommand measures this process. This one measures
//! somebody else's: it opens N peer connections to an address the caller named
//! and reports what happened on them. The target is never told anything it did
//! not already know, because there is no way to tell it: decision 7.4 rules
//! out a daemon and an RPC, so a torrent this run invented is a torrent the
//! target cannot be serving.
//!
//! That constraint is what splits this into two loads, and both are real
//! measurements rather than one being a fallback for the other.
//!
//! - [`Mode::Leech`] is the load the operator asks for. The target already
//!   serves the torrents; N peers handshake, declare interest, request blocks,
//!   and check every completed piece against the torrent's own SHA-1. What
//!   comes out is the target's serving path: bytes out, per-peer rate, when it
//!   chokes, how many peers it accepts, and where the aggregate stops rising
//!   with peer count.
//! - [`Mode::Connect`] is the load with no torrent in common. N peers
//!   handshake for info hashes the target does not have, which exercises the
//!   accept and handshake path and nothing else. That is not a degenerate
//!   case: it is the exact shape that killed `librqbit`'s accept loop in 79
//!   seconds under 3000 connections, and the half of `TODO/peers.md` T-020
//!   that is still open is that those connections strand a socket about half
//!   the time.
//!
//! # What it will not do
//!
//! It dials the target and nothing else, ever. No tracker announce, no DHT, no
//! PEX, and no peer address read out of a torrent or a configuration file. A
//! load generator that discovers its own targets is one command-line typo away
//! from being pointed at a stranger, so the only address it knows is the one
//! it was given. [`Outcome::dialled`] is what a report states that with.
//!
//! # Holding pieces
//!
//! A hundred peers that only ever take is not the load a seeder meets. A
//! target that superseeds, or that ranks peers by what they have uploaded,
//! behaves differently against peers holding nothing, so a verified piece is
//! **kept** rather than dropped, up to the disk budget. Past the budget a
//! verified piece is counted and dropped and the report says how many, because
//! a swarm that stopped growing is a different measurement from one that did
//! not.
//!
//! # Serving them back
//!
//! A peer announces what it holds and answers requests for it, on the same
//! connection it leeches over. It sends a bitfield, unchokes the target, sends
//! a `have` for every piece it verified and kept, and reads a requested block
//! back out of the packed hold file. That is what makes it a peer rather than
//! a downloader, and it is what a target that superseeds or that ranks peers
//! by what they uploaded is looking at.
//!
//! **What it cannot do is give the target something the target does not
//! have.** A synthetic peer has exactly one source, which is the target, so
//! everything it can announce is something the target served it. The bytes it
//! can send back are therefore only ever bytes the target already has. What
//! the serving side measures is what the target does with the announcement and
//! whether it asks at all. See `TODO/bench.md`, T-092.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use librqbit::ByteBuf;
use librqbit_core::Id20;
use librqbit_peer_protocol::{Handshake, Message, Piece, Request};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::bench::recorder::Recorder;
use crate::bench::report::Percentiles;
use crate::error::{Error, Result};
use crate::units::Size;
use crate::webseed::bridge::Framer;

/// Blocks are 16 KiB by convention and every client in circulation assumes it.
const BLOCK_LEN: u32 = 16 * 1024;

/// The largest block this peer will serve, whatever a target asks for.
///
/// A request carries a 32-bit length and nothing on the wire stops a target
/// naming 4 GiB of it. Serving is allocation, so the length is bounded before
/// the buffer is made rather than after. Eight times the conventional block is
/// past anything in circulation and still small.
const MAX_SERVED_BLOCK: u32 = 8 * BLOCK_LEN;

/// Peer id prefix for a synthetic swarm peer, from the one place the client
/// identity lives. Distinct from the bridge's so a target's logs can tell a
/// synthetic peer from a web seed bridge. See `TODO/peers.md`, T-236.
const PEER_ID_PREFIX: [u8; 8] = crate::peer_id::role(*b"sw", *b"01");

/// BEP 6's reserved bit: the third least significant bit of the last reserved
/// byte. `librqbit_peer_protocol::Handshake::new` sets bit 20 for the
/// extension protocol and nothing else, so this is added on top.
const RESERVED_FAST_EXTENSION: u64 = 1 << 2;

/// BEP 6 message ids. `librqbit_peer_protocol` 9.0.0 knows none of them, so a
/// frame carrying one fails to deserialize and used to be counted as a
/// protocol error. Reading the id first is what tells "the target speaks BEP
/// 6" from "the target is broken".
const MSGID_SUGGEST: u8 = 0x0D;
const MSGID_HAVE_ALL: u8 = 0x0E;
const MSGID_HAVE_NONE: u8 = 0x0F;
const MSGID_REJECT_REQUEST: u8 = 0x10;
const MSGID_ALLOWED_FAST: u8 = 0x11;

/// What a target did with BEP 6 on one connection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastExtension {
    /// Whether the target set the fast extension bit in its own handshake.
    /// Every synthetic peer sets it, so this is the target's answer.
    pub negotiated: bool,
    pub have_all: u64,
    pub have_none: u64,
    pub suggest: u64,
    /// Requests the target refused cleanly rather than by going silent, which
    /// is the half of BEP 6 that matters most to a partial seed.
    pub reject: u64,
    /// Piece indices the target offered, in the order they arrived.
    pub allowed_fast: Vec<u32>,
    /// Which derivation the offered set matches: `bep6`, `aria2`, `ambiguous`
    /// when the peer's address makes the two identical, or `neither`. Absent
    /// when nothing was offered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_fast_rule: Option<String>,
}

/// Which load to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The target serves these torrents and the peers leech them.
    Leech,
    /// The target does not have these torrents. Only the accept and handshake
    /// path is exercised.
    Connect,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Leech => "leech",
            Self::Connect => "connect",
        }
    }
}

/// One torrent the synthetic peers ask about.
#[derive(Debug, Clone)]
pub struct TorrentUnderTest {
    pub info_hash: [u8; 20],
    pub name: String,
    pub piece_length: u32,
    pub total_length: u64,
    /// One SHA-1 per piece. Empty in [`Mode::Connect`], where nothing is
    /// fetched and so nothing can be checked.
    pub piece_hashes: Vec<[u8; 20]>,
}

impl TorrentUnderTest {
    pub fn piece_count(&self) -> u32 {
        match self.piece_length {
            0 => 0,
            length => self.total_length.div_ceil(u64::from(length)) as u32,
        }
    }

    /// The length of one piece, which is shorter for the last one.
    fn length_of(&self, piece: u32) -> u32 {
        let start = u64::from(piece) * u64::from(self.piece_length);
        let remaining = self.total_length.saturating_sub(start);
        remaining.min(u64::from(self.piece_length)) as u32
    }
}

/// What one run is asked to do.
#[derive(Debug, Clone)]
pub struct Options {
    /// The one address this run will ever connect to.
    pub target: SocketAddr,
    pub peers: usize,
    pub duration: Duration,
    pub connect_timeout: Duration,
    /// Blocks in flight per peer.
    pub requests_in_flight: usize,
    /// A hard cap on everything this run writes.
    pub disk_budget: u64,
    /// Where verified pieces are kept. `None` writes nothing.
    pub hold_dir: Option<PathBuf>,
    pub torrents: Vec<TorrentUnderTest>,
    pub mode: Mode,
}

/// What one synthetic peer did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerOutcome {
    /// Position in the run, counting from zero.
    pub index: usize,
    /// Which torrent it asked about.
    pub torrent: usize,
    /// Whether the TCP connection was established.
    pub connected: bool,
    /// Whether a handshake came back.
    pub handshaked: bool,
    /// Whether the target echoed the info hash that was asked for. A target
    /// that does not is answering about a different torrent.
    pub info_hash_echoed: bool,
    /// Whether it was ever unchoked. In [`Mode::Connect`] it never is, and
    /// that is not a failure.
    pub unchoked: bool,
    pub bytes_received: u64,
    pub blocks_received: u64,
    pub pieces_verified: u64,
    /// Pieces that arrived complete and did not match the torrent's own hash.
    /// Anything above zero is the target serving wrong data.
    pub pieces_failed: u64,
    pub choke_events: u64,
    pub unchoke_events: u64,
    /// Pieces this peer announced with a `have`, which is what a target that
    /// ranks peers by what they hold is reading.
    pub pieces_announced: u64,
    /// Blocks the target asked this peer for. Zero against a target that
    /// already has everything, which is not a failure and is the point of
    /// reporting it: it says the serving path was never exercised.
    pub requests_received: u64,
    /// Requests refused because this peer does not hold that piece. A target
    /// asking for one it was never told about is asking out of turn.
    pub requests_refused: u64,
    pub cancels_received: u64,
    pub blocks_sent: u64,
    pub bytes_sent: u64,
    /// Whether the target ever declared interest in what this peer holds.
    pub target_interested: bool,
    /// Milliseconds to the established connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_ms: Option<u64>,
    /// Milliseconds from the connection to their handshake.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handshake_ms: Option<u64>,
    /// How long the connection lasted.
    pub open_ms: u64,
    /// What ended it: `deadline`, `closed`, or an error class.
    pub ended: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What the target did with BEP 6. See `TODO/bep-coverage.md`, T-100.
    pub fast: FastExtension,
}

/// What the whole run found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub mode: Mode,
    /// The one address dialled, and the count. Together these are what says
    /// the run contacted nothing it was not pointed at.
    pub dialled: String,
    pub peers_dialled: usize,
    pub peers_connected: usize,
    pub peers_handshaked: usize,
    /// Peers whose handshake came back with a different info hash.
    pub peers_wrong_info_hash: usize,
    pub peers_unchoked: usize,
    /// Peers that never got a connection, by error class.
    pub failures: Vec<FailureClass>,
    pub connect: Percentiles,
    pub handshake: Percentiles,
    pub bytes_received: Size,
    pub blocks_received: u64,
    pub pieces_verified: u64,
    pub pieces_failed: u64,
    pub choke_events: u64,
    pub unchoke_events: u64,
    /// What the peers gave back, folded. `peers_asked` is the one to read
    /// first: it is zero against a target that already holds everything, and
    /// then every other number here is zero for that reason rather than
    /// because the serving path is broken.
    pub serving: Serving,
    /// Bytes written to the scratch directory, and the cap they were held
    /// under. Both are stated so "never exceeded the budget" is a number.
    pub bytes_held: Size,
    pub disk_budget: Size,
    /// Verified pieces dropped because the budget was full.
    pub pieces_dropped_over_budget: u64,
    /// What the target did with BEP 6, folded across every peer.
    pub fast_extension: FastSummary,
    pub peers: Vec<PeerOutcome>,
}

/// What the synthetic peers gave back, across the whole run.
///
/// A synthetic peer holds only what the target served it, so it can never hand
/// the target a piece the target is missing. Everything here is therefore
/// about the announcement and about whether the target acted on it, which is
/// the half a seeder's behaviour actually turns on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Serving {
    /// Pieces announced with a `have`, summed over the peers.
    pub pieces_announced: u64,
    /// Peers the target sent at least one request to. Zero says the target
    /// never asked, which is what a full seeder does.
    pub peers_asked: usize,
    /// Peers the target declared interest in.
    pub peers_target_interested: usize,
    pub requests_received: u64,
    pub requests_refused: u64,
    pub cancels_received: u64,
    pub blocks_sent: u64,
    pub bytes_sent: Size,
}

/// BEP 6 across the whole run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastSummary {
    /// Peers whose handshake came back with the fast extension bit set. Every
    /// synthetic peer offers it, so anything less than `peers_handshaked` is
    /// the target declining.
    pub peers_negotiated: usize,
    pub have_all: u64,
    pub have_none: u64,
    pub suggest: u64,
    pub reject: u64,
    pub allowed_fast: u64,
    /// Which derivations the offered sets matched, and how many peers each.
    /// Empty when nothing offered one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_fast_rules: Vec<FailureClass>,
}

/// One class of failure and how many peers hit it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureClass {
    pub class: String,
    pub count: usize,
}

/// Run the load and return what it found.
pub async fn run(options: &Options, recorder: &Arc<Recorder>) -> Result<Outcome> {
    if options.torrents.is_empty() {
        return Err(Error::usage("bench swarm needs at least one torrent"));
    }
    if options.peers == 0 {
        return Err(Error::usage("bench swarm needs at least one peer"));
    }
    let held = Arc::new(Held::new(
        options.hold_dir.clone(),
        options.disk_budget,
        options.torrents.len(),
    ));
    let deadline = Instant::now() + options.duration;
    let shared = Arc::new(options.clone());
    // Peers handshaked and not yet finished. `Recorder::observe_peers` keeps a
    // high-water mark of whatever it is handed, so handing it 1 from each peer
    // would report a peak of one however many were live at once.
    let live = Arc::new(AtomicU64::new(0));

    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..options.peers {
        let shared = shared.clone();
        let held = held.clone();
        let recorder = recorder.clone();
        let live = live.clone();
        tasks.spawn(async move {
            let torrent = index % shared.torrents.len();
            one_peer(index, torrent, &shared, deadline, &held, &recorder, &live).await
        });
    }

    let mut peers: Vec<PeerOutcome> = Vec::with_capacity(options.peers);
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(outcome) => peers.push(outcome),
            // A panicked task is a defect in this file, not a measurement, and
            // silently reporting one fewer peer would hide it.
            Err(e) => return Err(Error::generic(format!("a synthetic peer panicked: {e}"))),
        }
    }
    peers.sort_by_key(|peer| peer.index);
    Ok(summarise(options, &peers, &held))
}

/// Fold the per-peer outcomes into the run's own.
fn summarise(options: &Options, peers: &[PeerOutcome], held: &Held) -> Outcome {
    let mut connect = Histogram::<u64>::new(3).expect("a histogram");
    let mut handshake = Histogram::<u64>::new(3).expect("a histogram");
    let mut classes: HashMap<String, usize> = HashMap::new();
    for peer in peers {
        if let Some(ms) = peer.connect_ms {
            let _ = connect.record(ms);
        }
        if let Some(ms) = peer.handshake_ms {
            let _ = handshake.record(ms);
        }
        if !peer.handshaked {
            *classes.entry(peer.ended.clone()).or_default() += 1;
        }
    }
    let mut failures: Vec<FailureClass> = classes
        .into_iter()
        .map(|(class, count)| FailureClass { class, count })
        .collect();
    // Most common first, then by name, so two runs of the same shape print the
    // same order.
    failures.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.class.cmp(&b.class)));

    let sum = |f: fn(&PeerOutcome) -> u64| peers.iter().map(f).sum::<u64>();
    Outcome {
        mode: options.mode,
        dialled: options.target.to_string(),
        peers_dialled: peers.len(),
        peers_connected: peers.iter().filter(|p| p.connected).count(),
        peers_handshaked: peers.iter().filter(|p| p.handshaked).count(),
        peers_wrong_info_hash: peers
            .iter()
            .filter(|p| p.handshaked && !p.info_hash_echoed)
            .count(),
        peers_unchoked: peers.iter().filter(|p| p.unchoked).count(),
        failures,
        connect: crate::bench::recorder::percentiles(&connect),
        handshake: crate::bench::recorder::percentiles(&handshake),
        bytes_received: Size(sum(|p| p.bytes_received)),
        blocks_received: sum(|p| p.blocks_received),
        pieces_verified: sum(|p| p.pieces_verified),
        pieces_failed: sum(|p| p.pieces_failed),
        choke_events: sum(|p| p.choke_events),
        unchoke_events: sum(|p| p.unchoke_events),
        serving: Serving {
            pieces_announced: sum(|p| p.pieces_announced),
            peers_asked: peers.iter().filter(|p| p.requests_received > 0).count(),
            peers_target_interested: peers.iter().filter(|p| p.target_interested).count(),
            requests_received: sum(|p| p.requests_received),
            requests_refused: sum(|p| p.requests_refused),
            cancels_received: sum(|p| p.cancels_received),
            blocks_sent: sum(|p| p.blocks_sent),
            bytes_sent: Size(sum(|p| p.bytes_sent)),
        },
        bytes_held: Size(held.written.load(Ordering::Relaxed)),
        disk_budget: Size(options.disk_budget),
        pieces_dropped_over_budget: held.dropped.load(Ordering::Relaxed),
        fast_extension: fast_summary(peers),
        peers: peers.to_vec(),
    }
}

/// Fold BEP 6 across the peers.
fn fast_summary(peers: &[PeerOutcome]) -> FastSummary {
    let mut rules: HashMap<String, usize> = HashMap::new();
    for peer in peers {
        if let Some(rule) = &peer.fast.allowed_fast_rule {
            *rules.entry(rule.clone()).or_default() += 1;
        }
    }
    let mut allowed_fast_rules: Vec<FailureClass> = rules
        .into_iter()
        .map(|(class, count)| FailureClass { class, count })
        .collect();
    allowed_fast_rules.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.class.cmp(&b.class)));
    FastSummary {
        peers_negotiated: peers.iter().filter(|p| p.fast.negotiated).count(),
        have_all: peers.iter().map(|p| p.fast.have_all).sum(),
        have_none: peers.iter().map(|p| p.fast.have_none).sum(),
        suggest: peers.iter().map(|p| p.fast.suggest).sum(),
        reject: peers.iter().map(|p| p.fast.reject).sum(),
        allowed_fast: peers.iter().map(|p| p.fast.allowed_fast.len() as u64).sum(),
        allowed_fast_rules,
    }
}

/// Everything one synthetic peer does, from the dial to the deadline.
async fn one_peer(
    index: usize,
    torrent_index: usize,
    options: &Options,
    deadline: Instant,
    held: &Held,
    recorder: &Arc<Recorder>,
    live: &Arc<AtomicU64>,
) -> PeerOutcome {
    let torrent = &options.torrents[torrent_index];
    let started = Instant::now();
    let mut outcome = PeerOutcome {
        index,
        torrent: torrent_index,
        connected: false,
        handshaked: false,
        info_hash_echoed: false,
        unchoked: false,
        bytes_received: 0,
        blocks_received: 0,
        pieces_verified: 0,
        pieces_failed: 0,
        choke_events: 0,
        unchoke_events: 0,
        pieces_announced: 0,
        requests_received: 0,
        requests_refused: 0,
        cancels_received: 0,
        blocks_sent: 0,
        bytes_sent: 0,
        target_interested: false,
        connect_ms: None,
        handshake_ms: None,
        open_ms: 0,
        ended: "deadline".to_string(),
        error: None,
        fast: FastExtension::default(),
    };

    let connect_within = options.connect_timeout.min(remaining(deadline));
    let stream =
        match tokio::time::timeout(connect_within, TcpStream::connect(options.target)).await {
            Err(_) => {
                outcome.ended = "connect_timeout".into();
                outcome.open_ms = ms(started.elapsed());
                return outcome;
            }
            Ok(Err(e)) => {
                outcome.ended = connect_class(&e).into();
                outcome.error = Some(e.to_string());
                outcome.open_ms = ms(started.elapsed());
                return outcome;
            }
            Ok(Ok(stream)) => stream,
        };
    outcome.connected = true;
    outcome.connect_ms = Some(ms(started.elapsed()));
    let connected_at = Instant::now();
    // The address the target sees, which is what it derives an allowed-fast
    // set from. Read here rather than assumed: a target reached over loopback
    // and one reached over a LAN derive different sets for the same peer.
    let local = stream.local_addr().ok().map(|a| a.ip());

    let (mut read, mut write) = stream.into_split();
    let peer_id = generate_peer_id();
    let mut ours = Handshake::new(Id20::new(torrent.info_hash), Id20::new(peer_id));
    // Advertised on every connection. A peer that does not know the bit
    // ignores it, and nothing here sends a BEP 6 message, so advertising it
    // costs nothing and is the only way to see what the target would do.
    ours.reserved |= RESERVED_FAST_EXTENSION;
    let mut buf = [0u8; 68];
    let len = ours.serialize_unchecked_len(&mut buf);
    if let Err(e) = write.write_all(&buf[..len]).await {
        outcome.ended = "write_handshake".into();
        outcome.error = Some(e.to_string());
        outcome.open_ms = ms(started.elapsed());
        return outcome;
    }

    let mut frames = Framer::default();
    // See `read_handshake`: connect mode holds to the run deadline because
    // holding is the measurement, leech mode gives up at `--connect-timeout`
    // because a target that will not answer is not going to serve either.
    let handshake_deadline = match options.mode {
        Mode::Connect => deadline,
        Mode::Leech => (Instant::now() + options.connect_timeout).min(deadline),
    };
    let theirs = match read_handshake(&mut read, &mut frames, handshake_deadline).await {
        Ok(theirs) => theirs,
        Err(reason) => {
            outcome.ended = reason;
            outcome.open_ms = ms(started.elapsed());
            return outcome;
        }
    };
    outcome.handshaked = true;
    outcome.handshake_ms = Some(ms(connected_at.elapsed()));
    outcome.info_hash_echoed = theirs.info_hash.0 == torrent.info_hash;
    outcome.fast.negotiated = theirs.reserved & RESERVED_FAST_EXTENSION != 0;
    let now_live = live.fetch_add(1, Ordering::Relaxed) + 1;
    recorder.observe_peers(now_live.min(u64::from(u32::MAX)) as u32);

    match options.mode {
        // Nothing is fetched, so the connection is held open to the deadline
        // and what the target says on it is counted. Holding it is the point:
        // a target that accepts a connection and then leaks it is what T-020
        // is about, and a peer that hangs up immediately would not show it.
        Mode::Connect => {
            drain(&mut read, &mut frames, deadline, &mut outcome).await;
        }
        Mode::Leech => {
            leech(
                &mut read,
                &mut write,
                &mut frames,
                torrent,
                torrent_index,
                options,
                deadline,
                held,
                recorder,
                &mut outcome,
            )
            .await;
        }
    }
    live.fetch_sub(1, Ordering::Relaxed);
    outcome.fast.allowed_fast_rule =
        classify_allowed_fast(&outcome.fast.allowed_fast, local, torrent);
    outcome.open_ms = ms(started.elapsed());
    outcome
}

/// Read frames until the deadline, counting what arrives and nothing else.
async fn drain(
    read: &mut (impl tokio::io::AsyncRead + Unpin),
    frames: &mut Framer,
    deadline: Instant,
    outcome: &mut PeerOutcome,
) {
    loop {
        while let Some(frame) = take_frame(frames, outcome) {
            note_message(&frame, outcome);
        }
        let left = remaining(deadline);
        if left.is_zero() {
            return;
        }
        match tokio::time::timeout(left, frames.fill(read)).await {
            Err(_) => return,
            Ok(Ok(0)) => {
                outcome.ended = "closed".into();
                return;
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                outcome.ended = "read".into();
                outcome.error = Some(e.to_string());
                return;
            }
        }
    }
}

/// Ask for blocks until the deadline, checking every completed piece.
#[allow(clippy::too_many_arguments)]
async fn leech(
    read: &mut (impl tokio::io::AsyncRead + Unpin),
    write: &mut (impl tokio::io::AsyncWrite + Unpin),
    frames: &mut Framer,
    torrent: &TorrentUnderTest,
    torrent_index: usize,
    options: &Options,
    deadline: Instant,
    held: &Held,
    recorder: &Arc<Recorder>,
    outcome: &mut PeerOutcome,
) {
    let mut state = Leecher::new(torrent, options.requests_in_flight, outcome.index);
    // The greeting, in wire order. A bitfield has to be the first message
    // after the handshake if it is sent at all, so it goes out before
    // anything else even though it is all zeros: this peer starts with
    // nothing and says so, and every piece it later keeps is announced with
    // a `have`. The unchoke is what makes the announcement worth anything,
    // because a target will not request from a peer that has it choked.
    let bits = vec![0u8; torrent.piece_count().div_ceil(8) as usize];
    if send(
        write,
        &Message::Bitfield(ByteBuf(&bits)),
        bits.len(),
        outcome,
    )
    .await
    .is_err()
    {
        return;
    }
    if send(write, &Message::Unchoke, 0, outcome).await.is_err() {
        return;
    }
    if send(write, &Message::Interested, 0, outcome).await.is_err() {
        return;
    }

    loop {
        while let Some(frame) = take_frame(frames, outcome) {
            // BEP 6 first, because these frames do not deserialize at all and
            // the three that change what a leecher does are here rather than
            // in the match below. See `TODO/bep-coverage.md`, T-100.
            if let Some(kind) = fast_message(&frame) {
                note_fast(kind, outcome);
                match kind {
                    // A bitfield in two bytes. Without this a peer that
                    // negotiated the fast extension sees no bitfield at all
                    // and requests nothing.
                    FastMsg::HaveAll => state.set_all(true),
                    FastMsg::HaveNone => state.set_all(false),
                    // A refused request is refused. Left in flight it holds a
                    // slot in the window until the deadline, which is the
                    // stall BEP 6 exists to prevent.
                    FastMsg::Reject(index, begin, _) => {
                        state.in_flight.remove(&(index, begin));
                    }
                    FastMsg::Suggest | FastMsg::AllowedFast(_) => {}
                }
                continue;
            }
            let Ok((message, _)) = Message::deserialize(&frame, &[]) else {
                outcome.ended = "protocol".into();
                return;
            };
            match message {
                Message::Unchoke => {
                    outcome.unchoked = true;
                    outcome.unchoke_events += 1;
                    state.choked = false;
                }
                Message::Choke => {
                    outcome.choke_events += 1;
                    state.choked = true;
                    // A choked peer's outstanding requests are dropped by the
                    // other end, so they have to be forgotten here or the
                    // window never refills.
                    state.in_flight.clear();
                }
                Message::Bitfield(bits) => state.set_bitfield(bits.as_ref()),
                Message::Have(piece) => state.set_have(piece),
                Message::Piece(piece) => {
                    let index = piece.index;
                    let begin = piece.begin;
                    // A `Piece` can arrive as two slices, because the frame
                    // may straddle two reads in the framer's buffer.
                    let (first, second) = piece.data();
                    let mut block = Vec::with_capacity(first.len() + second.len());
                    block.extend_from_slice(first);
                    block.extend_from_slice(second);
                    let length = block.len() as u32;
                    // Removed by `(piece, begin)` and not by the triple. A
                    // target that answers a 16 KiB request with a shorter
                    // block would otherwise leave the original tuple in
                    // flight forever, holding a window slot until the
                    // deadline and keeping `finished` from ever being true.
                    state.in_flight.remove(&(index, begin));
                    outcome.bytes_received += u64::from(length);
                    outcome.blocks_received += 1;
                    recorder.observe_bulk(torrent_index, u64::from(length), 1);
                    if let Some(complete) = state.place(index, begin, &block) {
                        let servable = finish_piece(
                            torrent,
                            torrent_index,
                            index,
                            &complete,
                            held,
                            recorder,
                            outcome,
                        );
                        state.done.insert(index);
                        state.buffers.remove(&index);
                        // Announced only once it can be served. A `have` for
                        // a piece the budget refused would spend the
                        // target's requests on a refusal.
                        if servable {
                            if send(write, &Message::Have(index), 0, outcome)
                                .await
                                .is_err()
                            {
                                return;
                            }
                            outcome.pieces_announced += 1;
                            state.announced.insert(index);
                        }
                    }
                }
                // The serving half. A synthetic peer answers out of the
                // packed hold file, which is the whole reason the pieces are
                // kept rather than dropped.
                Message::Request(request) => {
                    outcome.requests_received += 1;
                    if !serve(
                        write,
                        torrent,
                        torrent_index,
                        &state,
                        request,
                        held,
                        outcome,
                    )
                    .await
                    {
                        return;
                    }
                }
                // Answered synchronously, so by the time a cancel arrives the
                // block has already gone out. Counted rather than acted on:
                // what it says is that the target changed its mind, which is
                // a fact about the target.
                Message::Cancel(_) => outcome.cancels_received += 1,
                Message::Interested => outcome.target_interested = true,
                _ => {}
            }
        }

        if remaining(deadline).is_zero() {
            return;
        }
        // A peer holding everything the target advertised has nothing left to
        // take, so it stops rather than sitting on the connection until the
        // deadline. `--duration` is the bound on the run, not its length: a
        // load that finishes in a second and is reported over ten reads as a
        // tenth of the rate it reached.
        //
        // Unless the target wants what this peer now holds. Then hanging up
        // is what would end the measurement, and the peer stays to the
        // deadline serving. A target that already has the payload is never
        // interested, which is why the leech cases still finish the moment
        // they complete.
        if state.finished() && !outcome.target_interested {
            outcome.ended = "complete".into();
            return;
        }
        // Refill the window before waiting, so a reply and its follow-up
        // request are not a round trip apart.
        while let Some(request) = state.next_request() {
            if send(write, &Message::Request(request), 0, outcome)
                .await
                .is_err()
            {
                return;
            }
        }

        let left = remaining(deadline);
        if left.is_zero() {
            return;
        }
        match tokio::time::timeout(left, frames.fill(read)).await {
            Err(_) => return,
            Ok(Ok(0)) => {
                outcome.ended = "closed".into();
                return;
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                outcome.ended = "read".into();
                outcome.error = Some(e.to_string());
                return;
            }
        }
    }
}

/// Answer one request out of what this peer holds.
///
/// Returns whether the connection is still usable. A refusal is not a failure
/// of the connection: under BEP 6 it is a `reject request`, which says so on
/// the wire, and without it the only honest answer is silence, because BEP 3
/// has no way to decline.
#[allow(clippy::too_many_arguments)]
async fn serve(
    write: &mut (impl tokio::io::AsyncWrite + Unpin),
    torrent: &TorrentUnderTest,
    torrent_index: usize,
    state: &Leecher<'_>,
    request: Request,
    held: &Held,
    outcome: &mut PeerOutcome,
) -> bool {
    let piece_len = torrent.length_of(request.index);
    // Only what this peer announced. Another peer in the same run may have
    // held a piece this one never verified, and serving it would answer for
    // somebody else.
    let block = state
        .announced
        .contains(&request.index)
        .then(|| {
            held.read_block(
                torrent_index,
                request.index,
                request.begin,
                request.length,
                piece_len,
            )
        })
        .flatten();
    let Some(block) = block else {
        outcome.requests_refused += 1;
        if outcome.fast.negotiated {
            let frame = reject_frame(request);
            if write.write_all(&frame).await.is_err() {
                outcome.ended = "write".into();
                return false;
            }
        }
        return true;
    };
    let length = block.len();
    let message = Message::Piece(Piece::from_data(request.index, request.begin, &block));
    if send(write, &message, length, outcome).await.is_err() {
        return false;
    }
    outcome.blocks_sent += 1;
    outcome.bytes_sent += length as u64;
    true
}

/// BEP 6 `reject request`, built by hand.
///
/// `librqbit_peer_protocol` 9.0.0 knows none of the five fast-extension ids,
/// so it cannot serialize one either. The frame is a 4-byte length, the id,
/// and the request triple.
fn reject_frame(request: Request) -> [u8; 17] {
    let mut frame = [0u8; 17];
    frame[0..4].copy_from_slice(&13u32.to_be_bytes());
    frame[4] = MSGID_REJECT_REQUEST;
    frame[5..9].copy_from_slice(&request.index.to_be_bytes());
    frame[9..13].copy_from_slice(&request.begin.to_be_bytes());
    frame[13..17].copy_from_slice(&request.length.to_be_bytes());
    frame
}

/// Check a completed piece and keep it if the budget allows.
///
/// Returns whether the piece can now be served, which is what decides if it is
/// announced. A piece the budget refused is not announced, because a peer that
/// announces what it cannot serve is a peer that wastes the target's requests.
fn finish_piece(
    torrent: &TorrentUnderTest,
    torrent_index: usize,
    piece: u32,
    bytes: &[u8],
    held: &Held,
    recorder: &Arc<Recorder>,
    outcome: &mut PeerOutcome,
) -> bool {
    let started = Instant::now();
    let digest: [u8; 20] = <sha1::Sha1 as sha1::Digest>::digest(bytes).into();
    let expected = torrent.piece_hashes.get(piece as usize);
    let elapsed = started.elapsed();
    match expected {
        Some(want) if *want == digest => {
            outcome.pieces_verified += 1;
            recorder.observe_hashing(1, bytes.len() as u64, elapsed);
            held.keep(torrent_index, piece, bytes)
        }
        // No hash to check against cannot be a pass. It is a torrent this run
        // does not fully know, which is a caller error rather than a target
        // fault, and counting it as verified would report a check that did not
        // happen.
        Some(_) | None => {
            outcome.pieces_failed += 1;
            false
        }
    }
}

/// One peer's view of what it has and what it has asked for.
struct Leecher<'a> {
    torrent: &'a TorrentUnderTest,
    /// Pieces the target says it has.
    available: Vec<bool>,
    /// Pieces this peer has completed.
    done: HashSet<u32>,
    /// Pieces this peer told the target it has, which is exactly the set it
    /// will serve from.
    announced: HashSet<u32>,
    /// Partly received pieces, by index.
    buffers: HashMap<u32, PieceBuffer>,
    /// Blocks asked for and not yet answered, keyed by `(piece, begin)`.
    ///
    /// Not by the length as well. A target answering a 16 KiB request with a
    /// shorter block would leave the original triple in flight forever, and
    /// there is only ever one outstanding request per offset anyway.
    in_flight: HashSet<(u32, u32)>,
    window: usize,
    choked: bool,
    /// Where in the piece list this peer starts looking. A hundred peers all
    /// starting at zero would ask one seeder for the same piece a hundred
    /// times and measure a cache rather than a swarm.
    offset: u32,
}

impl<'a> Leecher<'a> {
    fn new(torrent: &'a TorrentUnderTest, window: usize, index: usize) -> Self {
        let count = torrent.piece_count();
        Self {
            torrent,
            available: vec![false; count as usize],
            done: HashSet::new(),
            announced: HashSet::new(),
            buffers: HashMap::new(),
            in_flight: HashSet::new(),
            window: window.max(1),
            choked: true,
            offset: match count {
                0 => 0,
                n => (index as u32).wrapping_mul(7919) % n,
            },
        }
    }

    fn set_bitfield(&mut self, bits: &[u8]) {
        for (index, slot) in self.available.iter_mut().enumerate() {
            let byte = index / 8;
            let bit = 7 - (index % 8);
            *slot = bits.get(byte).is_some_and(|b| (b >> bit) & 1 == 1);
        }
    }

    /// BEP 6's `have all` and `have none`, which stand in for a bitfield.
    fn set_all(&mut self, present: bool) {
        for slot in self.available.iter_mut() {
            *slot = present;
        }
    }

    fn set_have(&mut self, piece: u32) {
        if let Some(slot) = self.available.get_mut(piece as usize) {
            *slot = true;
        }
    }

    /// Whether every piece the target advertised has been fetched and checked.
    ///
    /// A target that advertised nothing is not finished, it is silent, and the
    /// two have to stay apart: reporting a peer that never saw a bitfield as
    /// complete would report an empty run as a successful one.
    fn finished(&self) -> bool {
        let offered = self.available.iter().filter(|has| **has).count();
        offered > 0 && self.done.len() >= offered && self.in_flight.is_empty()
    }

    /// The next block worth asking for, or `None` when the window is full or
    /// there is nothing left to want.
    fn next_request(&mut self) -> Option<Request> {
        if self.choked || self.in_flight.len() >= self.window {
            return None;
        }
        let count = self.available.len() as u32;
        for step in 0..count {
            let piece = (self.offset + step) % count;
            if self.done.contains(&piece) || !self.available[piece as usize] {
                continue;
            }
            let piece_len = self.torrent.length_of(piece);
            let buffer = self
                .buffers
                .entry(piece)
                .or_insert_with(|| PieceBuffer::new(piece_len));
            if let Some(begin) = buffer.next_gap(&self.in_flight, piece) {
                let length = BLOCK_LEN.min(piece_len - begin);
                self.in_flight.insert((piece, begin));
                return Some(Request::new(piece, begin, length));
            }
        }
        None
    }

    /// Store a block. Returns the whole piece once it is complete.
    fn place(&mut self, piece: u32, begin: u32, block: &[u8]) -> Option<Vec<u8>> {
        let piece_len = self.torrent.length_of(piece);
        if piece_len == 0 {
            return None;
        }
        let buffer = self
            .buffers
            .entry(piece)
            .or_insert_with(|| PieceBuffer::new(piece_len));
        buffer.place(begin, block);
        buffer.complete().then(|| buffer.bytes.clone())
    }
}

/// One piece being assembled out of blocks.
struct PieceBuffer {
    bytes: Vec<u8>,
    /// One flag per block, so a block that arrives twice is not counted twice
    /// and a gap is found without scanning the payload.
    received: Vec<bool>,
}

impl PieceBuffer {
    fn new(length: u32) -> Self {
        Self {
            bytes: vec![0u8; length as usize],
            received: vec![false; length.div_ceil(BLOCK_LEN) as usize],
        }
    }

    fn place(&mut self, begin: u32, block: &[u8]) {
        let start = begin as usize;
        let end = (start + block.len()).min(self.bytes.len());
        if start >= end {
            return;
        }
        self.bytes[start..end].copy_from_slice(&block[..end - start]);
        if let Some(slot) = self.received.get_mut(start / BLOCK_LEN as usize) {
            *slot = true;
        }
    }

    /// The offset of the first block neither received nor already asked for.
    fn next_gap(&self, in_flight: &HashSet<(u32, u32)>, piece: u32) -> Option<u32> {
        for (slot, got) in self.received.iter().enumerate() {
            if *got {
                continue;
            }
            let begin = slot as u32 * BLOCK_LEN;
            if !in_flight.contains(&(piece, begin)) {
                return Some(begin);
            }
        }
        None
    }

    fn complete(&self) -> bool {
        self.received.iter().all(|got| *got)
    }
}

/// The scratch directory, the budget over it, and what has been written.
struct Held {
    dir: Option<PathBuf>,
    budget: u64,
    written: AtomicU64,
    dropped: AtomicU64,
    /// Everything the budget depends on, behind one lock.
    ///
    /// Three questions have to be answered together for one piece: has it
    /// already been kept, does it fit, and where does it go. Answered under
    /// three separate locks they could interleave, and the offset a piece
    /// went to was the offset it has in the torrent, which is what made a file
    /// larger than the budget.
    store: std::sync::Mutex<Store>,
}

/// Where the held pieces went.
#[derive(Default)]
struct Store {
    /// One file per torrent, opened on first use.
    files: HashMap<usize, std::fs::File>,
    /// Pieces already on disk and where each one landed, so two peers holding
    /// the same piece write it once, the budget counts it once, and either of
    /// them can read it back to serve it.
    ///
    /// Packing is why the offset has to be recorded: a piece is at the byte it
    /// was written to, not at the byte it occupies in the torrent.
    have: HashMap<(usize, u32), u64>,
    /// Bytes used in each torrent's file so far, which is where the next piece
    /// goes.
    used: HashMap<usize, u64>,
}

impl Held {
    fn new(dir: Option<PathBuf>, budget: u64, torrents: usize) -> Self {
        Self {
            dir,
            budget,
            written: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            store: std::sync::Mutex::new(Store {
                files: HashMap::with_capacity(torrents),
                ..Default::default()
            }),
        }
    }

    /// Write a verified piece, unless the budget is full or another peer got
    /// there first.
    ///
    /// **Pieces are packed rather than placed.** A piece goes at the next free
    /// byte of its torrent's file, not at the offset it has in the torrent, so
    /// the file on disk is exactly as long as the bytes kept. Written at its
    /// torrent offset the file is as long as the *highest* piece kept, which
    /// on a run that keeps a quarter of a payload out of order is several
    /// times the budget: measured at 4,980,736 bytes against a 2,097,152 byte
    /// budget before this changed. Nothing reads the held bytes back, so the
    /// offset bought nothing; it was written that way because it is what a
    /// real client does. See `TODO/bench.md`, T-092.
    ///
    /// Returns whether the piece is on disk when the call returns, which is
    /// what says a peer may announce it. A piece another peer already wrote
    /// counts: this peer verified it too, and the bytes are there to serve.
    fn keep(&self, index: usize, piece: u32, bytes: &[u8]) -> bool {
        let Some(dir) = &self.dir else { return false };
        let length = bytes.len() as u64;
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());

        if store.have.contains_key(&(index, piece)) {
            return true;
        }
        // Refused whole rather than trimmed: half a piece on disk is worse
        // than none, and the budget is what this command promises.
        if self.written.load(Ordering::SeqCst) + length > self.budget {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        // Read before the file map is borrowed, because both live in `store`.
        let offset = store.used.get(&index).copied().unwrap_or(0);
        let file = match store.files.entry(index) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let path = dir.join(format!("torrent-{index}.hold"));
                // Truncated, because the file is packed from zero and a
                // leftover from an earlier run into the same `--dir` would be
                // counted as this run's bytes on disk. Opened for reading as
                // well, because serving a held piece reads it back out.
                match std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .read(true)
                    .write(true)
                    .open(&path)
                {
                    Ok(file) => entry.insert(file),
                    Err(_) => {
                        self.dropped.fetch_add(1, Ordering::Relaxed);
                        return false;
                    }
                }
            }
        };

        if crate::storage::pwrite_all(file, offset, bytes).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        store.used.insert(index, offset + length);
        store.have.insert((index, piece), offset);
        self.written.fetch_add(length, Ordering::SeqCst);
        true
    }

    /// Read one block back out of a held piece, for serving it.
    ///
    /// `None` when the piece was never kept, when the request runs off the end
    /// of it, or when the read fails. All three are the same answer to the
    /// target, which is that this peer will not serve that block.
    ///
    /// The read is blocking and under the store's lock. Both are deliberate:
    /// the block is 16 KiB from a local file that was written moments ago, the
    /// write side is already blocking on the same task, and the alternative is
    /// a second file handle per peer for a path that a full seeder never even
    /// takes.
    fn read_block(
        &self,
        index: usize,
        piece: u32,
        begin: u32,
        length: u32,
        piece_len: u32,
    ) -> Option<Vec<u8>> {
        if length == 0 || length > MAX_SERVED_BLOCK {
            return None;
        }
        if u64::from(begin) + u64::from(length) > u64::from(piece_len) {
            return None;
        }
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let offset = *store.have.get(&(index, piece))?;
        let file = store.files.get_mut(&index)?;
        let mut block = vec![0u8; length as usize];
        crate::storage::pread_exact(file, offset + u64::from(begin), &mut block).ok()?;
        Some(block)
    }
}

/// Read one handshake, or the class of what went wrong instead.
///
/// `deadline` is whichever of the run deadline and this peer's handshake
/// budget comes first, and the two modes want different budgets. In
/// [`Mode::Connect`] holding the connection **is** the measurement, so a
/// target that accepts and never answers should be held to the end of the run
/// and reported as `handshake_timeout` then: that is the shape of T-020. In
/// [`Mode::Leech`] the same target costs the whole `--duration` before the
/// peer says anything, and the answer was known at `--connect-timeout`. The
/// caller picks; this reads until whichever it was told.
async fn read_handshake(
    read: &mut (impl tokio::io::AsyncRead + Unpin),
    frames: &mut Framer,
    deadline: Instant,
) -> std::result::Result<Handshake, String> {
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
        let left = remaining(deadline);
        if left.is_zero() {
            return Err("handshake_timeout".into());
        }
        match tokio::time::timeout(left, frames.fill(read)).await {
            Err(_) => return Err("handshake_timeout".into()),
            Ok(Ok(0)) => return Err("closed_before_handshake".into()),
            Ok(Ok(_)) => {}
            Ok(Err(_)) => return Err("read_before_handshake".into()),
        }
    }
}

/// Serialize and send one message.
async fn send(
    write: &mut (impl tokio::io::AsyncWrite + Unpin),
    message: &Message<'_>,
    payload: usize,
    outcome: &mut PeerOutcome,
) -> std::result::Result<(), ()> {
    let mut buf = vec![0u8; 64 + payload];
    let Ok(len) = message.serialize(&mut buf, &Default::default) else {
        outcome.ended = "serialize".into();
        return Err(());
    };
    match write.write_all(&buf[..len]).await {
        Ok(()) => Ok(()),
        Err(e) => {
            outcome.ended = "write".into();
            outcome.error = Some(e.to_string());
            Err(())
        }
    }
}

/// Take one frame, marking a protocol error on the outcome rather than
/// returning it, because a bad frame ends the connection either way.
fn take_frame(frames: &mut Framer, outcome: &mut PeerOutcome) -> Option<Vec<u8>> {
    match frames.take_frame() {
        Ok(frame) => frame,
        Err(reason) => {
            outcome.ended = "protocol".into();
            outcome.error = Some(reason);
            None
        }
    }
}

/// One BEP 6 message, as far as this tool cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FastMsg {
    HaveAll,
    HaveNone,
    Suggest,
    AllowedFast(u32),
    /// A request the target refused. The triple is what identifies which one.
    Reject(u32, u32, u32),
}

/// Read a frame's message id and say whether it is BEP 6.
///
/// `librqbit_peer_protocol` 9.0.0 has no variant for any of these, so a frame
/// carrying one comes back from `Message::deserialize` as an error and used to
/// end the connection as `protocol`. A target that speaks the fast extension
/// is not a broken target, and the two have to be told apart before either can
/// be reported.
///
/// A frame from [`Framer::take_frame`] carries its own four byte length
/// prefix, so the id is at index 4 and the payload begins at 5.
fn fast_message(frame: &[u8]) -> Option<FastMsg> {
    let id = *frame.get(4)?;
    let payload = frame.get(5..).unwrap_or_default();
    let word = |at: usize| -> Option<u32> {
        payload
            .get(at..at + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_be_bytes)
    };
    match id {
        MSGID_HAVE_ALL => Some(FastMsg::HaveAll),
        MSGID_HAVE_NONE => Some(FastMsg::HaveNone),
        MSGID_SUGGEST => Some(FastMsg::Suggest),
        MSGID_ALLOWED_FAST => Some(FastMsg::AllowedFast(word(0)?)),
        MSGID_REJECT_REQUEST => Some(FastMsg::Reject(word(0)?, word(4)?, word(8)?)),
        _ => None,
    }
}

/// Fold one BEP 6 message into what the connection reports.
fn note_fast(kind: FastMsg, outcome: &mut PeerOutcome) {
    match kind {
        FastMsg::HaveAll => outcome.fast.have_all += 1,
        FastMsg::HaveNone => outcome.fast.have_none += 1,
        FastMsg::Suggest => outcome.fast.suggest += 1,
        FastMsg::Reject(..) => outcome.fast.reject += 1,
        FastMsg::AllowedFast(index) => outcome.fast.allowed_fast.push(index),
    }
}

/// Which derivation the offered set matches.
///
/// Order matters as well as membership: BEP 6 produces a sequence and a
/// receiver comparing what arrived against what should have arrived compares
/// sequences. `ambiguous` is a real answer rather than a hedge, because the
/// two rules give the same set for every address at or above 192.0.0.0, and
/// loopback is not one of them: 127.x is class A under aria2's rule and the
/// two agree there too.
fn classify_allowed_fast(
    offered: &[u32],
    local: Option<std::net::IpAddr>,
    torrent: &TorrentUnderTest,
) -> Option<String> {
    if offered.is_empty() {
        return None;
    }
    let Some(std::net::IpAddr::V4(ip)) = local else {
        // BEP 6 derives the set from a four byte address and says nothing
        // about IPv6, so there is nothing to compare against.
        return Some("unknown".into());
    };
    let size = offered.len() as u32;
    let pieces = torrent.piece_count();
    let bep6 = crate::fast_set::allowed_fast(
        crate::fast_set::Mask::Bep6,
        size,
        pieces,
        &torrent.info_hash,
        ip,
    );
    let aria2 = crate::fast_set::allowed_fast(
        crate::fast_set::Mask::Aria2,
        size,
        pieces,
        &torrent.info_hash,
        ip,
    );
    let matches_bep6 = offered == bep6.as_slice();
    match (matches_bep6, crate::fast_set::Mask::is_ambiguous(ip)) {
        (true, true) => Some("ambiguous".into()),
        (true, false) => Some(crate::fast_set::Mask::Bep6.as_str().into()),
        (false, _) if offered == aria2.as_slice() => {
            Some(crate::fast_set::Mask::Aria2.as_str().into())
        }
        (false, _) => Some("neither".into()),
    }
}

/// Count a message in connect mode, where nothing is requested.
fn note_message(frame: &[u8], outcome: &mut PeerOutcome) {
    if let Some(kind) = fast_message(frame) {
        note_fast(kind, outcome);
        return;
    }
    let Ok((message, _)) = Message::deserialize(frame, &[]) else {
        outcome.ended = "protocol".into();
        return;
    };
    match message {
        Message::Choke => outcome.choke_events += 1,
        Message::Unchoke => {
            outcome.unchoke_events += 1;
            outcome.unchoked = true;
        }
        _ => {}
    }
}

/// How long is left before the deadline, saturating at zero.
fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn ms(elapsed: Duration) -> u64 {
    elapsed.as_millis().min(u128::from(u64::MAX)) as u64
}

/// Which kind of refusal a connect error was.
///
/// The classes are what a report groups by, so they have to separate the three
/// answers that mean different things: nothing is listening, something is and
/// it said no, and the route is gone.
///
/// [`crate::listener`] classifies its own dial with this, so the load
/// generator and the health probe name the same failure the same way.
pub(crate) fn connect_class(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => "connect_refused",
        std::io::ErrorKind::TimedOut => "connect_timeout",
        std::io::ErrorKind::HostUnreachable | std::io::ErrorKind::NetworkUnreachable => {
            "connect_unreachable"
        }
        _ => "connect_failed",
    }
}

/// A peer id with this tool's prefix and random bytes after it.
///
/// `librqbit_core`'s generator, so a synthetic peer's id is built the same way
/// every other peer this repository creates is.
fn generate_peer_id() -> [u8; 20] {
    crate::peer_id::generate(&PEER_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(total: u64, piece: u32) -> TorrentUnderTest {
        TorrentUnderTest {
            info_hash: [7u8; 20],
            name: "payload".into(),
            piece_length: piece,
            total_length: total,
            piece_hashes: Vec::new(),
        }
    }

    /// A length-prefixed frame the way `Framer::take_frame` hands one over:
    /// four bytes of length, the message id, then the payload.
    fn frame(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 1) as u32).to_be_bytes().to_vec();
        out.push(id);
        out.extend_from_slice(payload);
        out
    }

    fn blank_outcome() -> PeerOutcome {
        PeerOutcome {
            index: 0,
            torrent: 0,
            connected: true,
            handshaked: true,
            info_hash_echoed: true,
            unchoked: false,
            bytes_received: 0,
            blocks_received: 0,
            pieces_verified: 0,
            pieces_failed: 0,
            choke_events: 0,
            unchoke_events: 0,
            pieces_announced: 0,
            requests_received: 0,
            requests_refused: 0,
            cancels_received: 0,
            blocks_sent: 0,
            bytes_sent: 0,
            target_interested: false,
            connect_ms: None,
            handshake_ms: None,
            open_ms: 0,
            ended: "deadline".into(),
            error: None,
            fast: FastExtension::default(),
        }
    }

    #[test]
    fn every_bep6_message_is_recognised_rather_than_called_a_protocol_error() {
        // The regression this fixes: `librqbit_peer_protocol` knows none of
        // these ids, so every one of them used to end the connection as
        // `protocol` and a target that speaks the fast extension read as a
        // broken one.
        let mut outcome = blank_outcome();
        note_message(&frame(MSGID_HAVE_ALL, &[]), &mut outcome);
        note_message(&frame(MSGID_HAVE_NONE, &[]), &mut outcome);
        note_message(&frame(MSGID_SUGGEST, &7u32.to_be_bytes()), &mut outcome);
        note_message(
            &frame(MSGID_ALLOWED_FAST, &3u32.to_be_bytes()),
            &mut outcome,
        );
        note_message(
            &frame(MSGID_ALLOWED_FAST, &9u32.to_be_bytes()),
            &mut outcome,
        );
        let mut reject = Vec::new();
        reject.extend_from_slice(&1u32.to_be_bytes());
        reject.extend_from_slice(&0u32.to_be_bytes());
        reject.extend_from_slice(&16384u32.to_be_bytes());
        note_message(&frame(MSGID_REJECT_REQUEST, &reject), &mut outcome);

        assert_eq!(outcome.ended, "deadline", "nothing was a protocol error");
        assert_eq!(outcome.fast.have_all, 1);
        assert_eq!(outcome.fast.have_none, 1);
        assert_eq!(outcome.fast.suggest, 1);
        assert_eq!(outcome.fast.reject, 1);
        assert_eq!(outcome.fast.allowed_fast, vec![3, 9]);
    }

    #[test]
    fn a_reject_names_the_request_it_refused() {
        let mut reject = Vec::new();
        reject.extend_from_slice(&5u32.to_be_bytes());
        reject.extend_from_slice(&32768u32.to_be_bytes());
        reject.extend_from_slice(&16384u32.to_be_bytes());
        assert_eq!(
            fast_message(&frame(MSGID_REJECT_REQUEST, &reject)),
            Some(FastMsg::Reject(5, 32768, 16384))
        );
    }

    #[test]
    fn a_truncated_bep6_frame_is_not_read_as_one() {
        // Two bytes where four are needed. Reading it as a message would put a
        // number nobody sent into the report.
        assert_eq!(fast_message(&frame(MSGID_ALLOWED_FAST, &[0, 1])), None);
        assert_eq!(
            fast_message(&frame(MSGID_REJECT_REQUEST, &[0, 0, 0, 1])),
            None
        );
    }

    #[test]
    fn an_ordinary_message_is_left_to_the_real_parser() {
        // Choke is id 0 and has no BEP 6 meaning, so `fast_message` must not
        // claim it.
        assert_eq!(fast_message(&frame(0, &[])), None);
        let mut outcome = blank_outcome();
        note_message(&frame(1, &[]), &mut outcome);
        assert_eq!(outcome.unchoke_events, 1);
        assert!(outcome.unchoked);
    }

    #[test]
    fn have_all_and_have_none_stand_in_for_a_bitfield() {
        let t = torrent(4096, 1024);
        let mut state = Leecher::new(&t, 4, 0);
        assert!(!state.available.iter().any(|has| *has));
        state.set_all(true);
        assert!(state.available.iter().all(|has| *has));
        state.set_all(false);
        assert!(!state.available.iter().any(|has| *has));
    }

    #[test]
    fn the_offered_set_is_checked_against_the_derivation_it_should_have_used() {
        let mut t = torrent(1313 * 1024, 1024);
        t.info_hash = [0xAA; 20];
        let ip = Some("80.4.4.200".parse().expect("an address"));
        let conformant = crate::fast_set::allowed_fast(
            crate::fast_set::Mask::Bep6,
            7,
            1313,
            &t.info_hash,
            "80.4.4.200".parse().expect("an address"),
        );
        assert_eq!(
            classify_allowed_fast(&conformant, ip, &t),
            Some("bep6".into())
        );

        let aria2 = crate::fast_set::allowed_fast(
            crate::fast_set::Mask::Aria2,
            7,
            1313,
            &t.info_hash,
            "80.4.4.200".parse().expect("an address"),
        );
        assert_eq!(classify_allowed_fast(&aria2, ip, &t), Some("aria2".into()));
        assert_eq!(
            classify_allowed_fast(&[0, 1, 2, 3, 4, 5, 6], ip, &t),
            Some("neither".into())
        );
        assert_eq!(classify_allowed_fast(&[], ip, &t), None);
    }

    #[test]
    fn a_set_offered_over_loopback_cannot_tell_the_two_rules_apart() {
        // 127.x is class A under aria2's rule, so both derivations mask to
        // 127.0.0.0 and agree. Reporting that as a conformance pass would be
        // a claim the measurement cannot make.
        let mut t = torrent(1313 * 1024, 1024);
        t.info_hash = [0xAA; 20];
        let ip: std::net::Ipv4Addr = "127.0.0.1".parse().expect("an address");
        let offered =
            crate::fast_set::allowed_fast(crate::fast_set::Mask::Bep6, 6, 1313, &t.info_hash, ip);
        assert_eq!(
            classify_allowed_fast(&offered, Some(ip.into()), &t),
            Some("ambiguous".into())
        );
    }

    #[test]
    fn an_ipv6_peer_has_no_derivation_to_be_checked_against() {
        let t = torrent(1313 * 1024, 1024);
        let ip: std::net::IpAddr = "::1".parse().expect("an address");
        assert_eq!(
            classify_allowed_fast(&[1, 2, 3], Some(ip), &t),
            Some("unknown".into())
        );
    }

    #[test]
    fn the_last_piece_is_short() {
        let t = torrent(2500, 1024);
        assert_eq!(t.piece_count(), 3);
        assert_eq!(t.length_of(0), 1024);
        assert_eq!(t.length_of(2), 452);
    }

    #[test]
    fn a_piece_is_complete_only_when_every_block_arrived() {
        let mut buffer = PieceBuffer::new(BLOCK_LEN * 2 + 10);
        assert!(!buffer.complete());
        buffer.place(0, &vec![1u8; BLOCK_LEN as usize]);
        assert!(!buffer.complete());
        buffer.place(BLOCK_LEN, &vec![2u8; BLOCK_LEN as usize]);
        assert!(!buffer.complete());
        buffer.place(BLOCK_LEN * 2, &[3u8; 10]);
        assert!(buffer.complete());
        assert_eq!(buffer.bytes[0], 1);
        assert_eq!(buffer.bytes[BLOCK_LEN as usize], 2);
        assert_eq!(buffer.bytes[BLOCK_LEN as usize * 2], 3);
    }

    /// A block already asked for is not asked for again, or the window fills
    /// with duplicates of one block and the piece never completes.
    #[test]
    fn a_block_in_flight_is_not_requested_twice() {
        let buffer = PieceBuffer::new(BLOCK_LEN * 3);
        let mut in_flight = HashSet::new();
        assert_eq!(buffer.next_gap(&in_flight, 0), Some(0));
        in_flight.insert((0, 0));
        assert_eq!(buffer.next_gap(&in_flight, 0), Some(BLOCK_LEN));
    }

    /// A block that comes back shorter than it was asked for still clears the
    /// window slot.
    ///
    /// This is why `in_flight` is keyed by `(piece, begin)` rather than by the
    /// whole request triple. Keyed by the triple, a target answering a 16 KiB
    /// request with 8 KiB leaves the original entry in flight for good: the
    /// slot never comes back, `finished` never sees an empty set, and the peer
    /// runs to `--duration` instead of stopping at `complete`.
    #[test]
    fn a_short_block_still_clears_the_slot_it_was_asked_for() {
        let t = torrent(BLOCK_LEN as u64 * 4, BLOCK_LEN * 2);
        let mut leecher = Leecher::new(&t, 4, 0);
        leecher.set_all(true);
        leecher.choked = false;
        let request = leecher.next_request().expect("a first request");
        assert_eq!((request.index, request.begin), (0, 0));
        assert_eq!(leecher.in_flight.len(), 1);

        // Half of what was asked for.
        leecher.in_flight.remove(&(request.index, request.begin));
        assert!(leecher.in_flight.is_empty());
    }

    #[test]
    fn a_bitfield_says_which_pieces_the_target_has() {
        let t = torrent(1024 * 10, 1024);
        let mut leecher = Leecher::new(&t, 4, 0);
        // 0b1010_0000, 0b1100_0000: pieces 0, 2, 8, 9.
        leecher.set_bitfield(&[0b1010_0000, 0b1100_0000]);
        assert!(leecher.available[0]);
        assert!(!leecher.available[1]);
        assert!(leecher.available[2]);
        assert!(leecher.available[8]);
        assert!(leecher.available[9]);
    }

    #[test]
    fn a_choked_peer_asks_for_nothing() {
        let t = torrent(1024 * 4, 1024);
        let mut leecher = Leecher::new(&t, 4, 0);
        leecher.set_bitfield(&[0b1111_0000]);
        assert!(leecher.next_request().is_none(), "choked by default");
        leecher.choked = false;
        assert!(leecher.next_request().is_some());
    }

    #[test]
    fn the_window_bounds_what_is_outstanding() {
        let t = torrent(1024 * 64, 1024);
        let mut leecher = Leecher::new(&t, 3, 0);
        leecher.set_bitfield(&[0xFF; 8]);
        leecher.choked = false;
        for _ in 0..3 {
            assert!(leecher.next_request().is_some());
        }
        assert!(leecher.next_request().is_none(), "the window is full");
    }

    /// A hundred peers all starting at piece zero would ask a seeder for the
    /// same piece a hundred times, which measures its cache rather than its
    /// swarm.
    #[test]
    fn peers_do_not_all_start_at_the_same_piece() {
        let t = torrent(1024 * 100, 1024);
        let starts: HashSet<u32> = (0..20)
            .map(|index| Leecher::new(&t, 4, index).offset)
            .collect();
        assert!(starts.len() > 1, "every peer started at {starts:?}");
    }

    #[test]
    fn the_budget_is_never_crossed() {
        let dir = tempfile::tempdir().unwrap();
        let held = Held::new(Some(dir.path().to_path_buf()), 4096, 1);
        for piece in 0..10 {
            held.keep(0, piece, &[9u8; 1024]);
        }
        assert_eq!(held.written.load(Ordering::Relaxed), 4096);
        assert_eq!(held.dropped.load(Ordering::Relaxed), 6);
        let on_disk = std::fs::metadata(dir.path().join("torrent-0.hold"))
            .unwrap()
            .len();
        assert_eq!(on_disk, 4096, "{on_disk} bytes on disk");
    }

    /// The regression T-092 recorded: the budget bounded the bytes written
    /// and not the bytes on disk.
    ///
    /// Pieces arriving in order hid it, because the last one kept was also the
    /// highest and the file ended where the budget did. Real peers do not
    /// arrive in order, and `peers_do_not_all_start_at_the_same_piece` is this
    /// tool making sure of it. Measured before the fix: a 2,097,152 byte
    /// budget left a 4,980,736 byte file.
    #[test]
    fn pieces_kept_out_of_order_do_not_make_the_file_longer_than_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let held = Held::new(Some(dir.path().to_path_buf()), 4096, 1);
        // Four pieces fit. Taking the highest ones is what a peer starting at
        // a random offset does.
        for piece in [60, 12, 47, 3, 55, 9] {
            held.keep(0, piece, &[9u8; 1024]);
        }
        assert_eq!(held.written.load(Ordering::Relaxed), 4096);
        assert_eq!(held.dropped.load(Ordering::Relaxed), 2);
        let on_disk = std::fs::metadata(dir.path().join("torrent-0.hold"))
            .unwrap()
            .len();
        assert_eq!(
            on_disk, 4096,
            "{on_disk} bytes on disk for 4096 bytes of budget"
        );
    }

    #[test]
    fn a_hold_directory_used_twice_does_not_carry_the_first_run_forward() {
        let dir = tempfile::tempdir().unwrap();
        let first = Held::new(Some(dir.path().to_path_buf()), 8192, 1);
        for piece in 0..8 {
            first.keep(0, piece, &[1u8; 1024]);
        }
        let second = Held::new(Some(dir.path().to_path_buf()), 2048, 1);
        second.keep(0, 30, &[2u8; 1024]);
        let on_disk = std::fs::metadata(dir.path().join("torrent-0.hold"))
            .unwrap()
            .len();
        assert_eq!(on_disk, 1024, "{on_disk} bytes, so the old file survived");
    }

    /// Two peers holding the same piece write it once, or the budget counts
    /// the same bytes as many times as there are peers.
    #[test]
    fn the_same_piece_is_held_once() {
        let dir = tempfile::tempdir().unwrap();
        let held = Held::new(Some(dir.path().to_path_buf()), 1 << 20, 1);
        held.keep(0, 0, &[1u8; 1024]);
        held.keep(0, 0, &[1u8; 1024]);
        assert_eq!(held.written.load(Ordering::Relaxed), 1024);
        assert_eq!(held.dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn holding_nowhere_writes_nothing() {
        let held = Held::new(None, 1 << 20, 1);
        held.keep(0, 0, &[1u8; 1024]);
        assert_eq!(held.written.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_peer_id_carries_the_prefix_and_is_not_constant() {
        let a = generate_peer_id();
        let b = generate_peer_id();
        assert_eq!(&a[..PEER_ID_PREFIX.len()], &PEER_ID_PREFIX);
        assert_ne!(a, b, "two peers with one id is one peer to the target");
    }

    // -----------------------------------------------------------------
    // Serving what was held
    // -----------------------------------------------------------------

    /// The packing has to be reversible or nothing can be served.
    ///
    /// A piece is written at the next free byte of its file rather than at its
    /// torrent offset, so the only thing that knows where piece 7 went is the
    /// map the write built. Kept out of order on purpose: in order, every
    /// offset would happen to be the torrent offset and the test would pass
    /// against the bug it exists to catch.
    #[test]
    fn a_held_piece_reads_back_out_of_the_packed_file() {
        let dir = tempfile::tempdir().unwrap();
        let held = Held::new(Some(dir.path().to_path_buf()), 1 << 20, 1);
        for piece in [7u32, 2, 5] {
            assert!(held.keep(0, piece, &[piece as u8; 1024]));
        }
        for piece in [7u32, 2, 5] {
            let block = held.read_block(0, piece, 0, 1024, 1024).expect("held");
            assert_eq!(block, vec![piece as u8; 1024], "piece {piece}");
        }
        // Halfway into a piece, which is what a 16 KiB request into a larger
        // piece is.
        let block = held.read_block(0, 5, 512, 512, 1024).expect("held");
        assert_eq!(block, vec![5u8; 512]);
    }

    /// Three ways a request is refused, and none of them is an error.
    #[test]
    fn a_request_this_peer_cannot_answer_is_refused_rather_than_guessed_at() {
        let dir = tempfile::tempdir().unwrap();
        let held = Held::new(Some(dir.path().to_path_buf()), 1 << 20, 1);
        assert!(held.keep(0, 3, &[3u8; 1024]));

        assert!(
            held.read_block(0, 4, 0, 1024, 1024).is_none(),
            "a piece that was never kept"
        );
        assert!(
            held.read_block(0, 3, 512, 1024, 1024).is_none(),
            "a block running off the end of the piece"
        );
        assert!(
            held.read_block(0, 3, 0, MAX_SERVED_BLOCK + 1, MAX_SERVED_BLOCK + 1)
                .is_none(),
            "a length past what this peer will ever allocate"
        );
        assert!(
            held.read_block(0, 3, 0, 0, 1024).is_none(),
            "an empty block"
        );
    }

    /// A piece the budget refused is a piece this peer must not announce.
    ///
    /// This is why `keep` reports back rather than returning nothing: the
    /// announcement is decided by whether the bytes are there to serve, not by
    /// whether the piece verified.
    #[test]
    fn a_piece_the_budget_refused_is_not_servable() {
        let dir = tempfile::tempdir().unwrap();
        let held = Held::new(Some(dir.path().to_path_buf()), 2048, 1);
        assert!(held.keep(0, 0, &[0u8; 1024]));
        assert!(held.keep(0, 1, &[1u8; 1024]));
        assert!(!held.keep(0, 2, &[2u8; 1024]), "the budget is full");
        assert!(held.read_block(0, 2, 0, 1024, 1024).is_none());
        // A second peer verifying a piece another one already wrote can serve
        // it: it verified the same bytes and they are on disk.
        assert!(held.keep(0, 1, &[1u8; 1024]));
        assert_eq!(held.written.load(Ordering::Relaxed), 2048);
    }

    /// The reject this file writes is one this file can read.
    ///
    /// `librqbit_peer_protocol` 9.0.0 serializes none of the five fast ids, so
    /// the frame is built by hand and there is no library to check it against.
    /// The parser on the other side of the same file is the check.
    #[test]
    fn a_reject_written_here_parses_as_the_request_it_refused() {
        let frame = reject_frame(Request::new(11, 32768, 16384));
        assert_eq!(
            fast_message(&frame),
            Some(FastMsg::Reject(11, 32768, 16384))
        );
    }

    // A scripted target on loopback: it serves one piece, then asks for it
    // back. Nothing else in this file exercises the serving path end to end,
    // because the only real target available is a seeder and a seeder never
    // asks.
    //
    // `interested` is the whole difference between the two tests below: a
    // target that says it wants something keeps the peer on the connection
    // past `complete`, and a target that does not lets it go.
    struct Script {
        /// What came back when the target asked for piece 0.
        served: Option<Vec<u8>>,
        /// Whether the peer announced the piece before being asked.
        announced: bool,
    }

    async fn scripted_target(
        listener: tokio::net::TcpListener,
        info_hash: [u8; 20],
        payload: Vec<u8>,
        interested: bool,
    ) -> Script {
        let (stream, _) = listener.accept().await.expect("an accepted peer");
        let (mut read, mut write) = stream.into_split();

        let mut frames = Framer::default();
        while Handshake::deserialize(frames.buffered()).is_err() {
            let n = frames.fill(&mut read).await.expect("a handshake");
            assert_ne!(n, 0, "the peer hung up before handshaking");
        }
        let size = Handshake::deserialize(frames.buffered()).unwrap().1;
        frames.consume(size);

        let theirs = Handshake::new(Id20::new(info_hash), Id20::new([9u8; 20]));
        let mut buf = [0u8; 68];
        let len = theirs.serialize_unchecked_len(&mut buf);
        write.write_all(&buf[..len]).await.unwrap();

        let mut greeting = Vec::new();
        for message in [Message::Bitfield(ByteBuf(&[0b1000_0000])), Message::Unchoke] {
            let mut out = vec![0u8; 64];
            let n = message.serialize(&mut out, &Default::default).unwrap();
            greeting.extend_from_slice(&out[..n]);
        }
        if interested {
            let mut out = vec![0u8; 64];
            let n = Message::Interested
                .serialize(&mut out, &Default::default)
                .unwrap();
            greeting.extend_from_slice(&out[..n]);
        }
        write.write_all(&greeting).await.unwrap();

        let mut script = Script {
            served: None,
            announced: false,
        };
        let mut asked = false;
        loop {
            let frame = match frames.take_frame() {
                Ok(Some(frame)) => frame,
                Ok(None) => match frames.fill(&mut read).await {
                    Ok(0) | Err(_) => return script,
                    Ok(_) => continue,
                },
                Err(_) => return script,
            };
            let Ok((message, _)) = Message::deserialize(&frame, &[]) else {
                continue;
            };
            match message {
                // Answer the peer out of the payload, which is what makes it
                // hold a piece it can then be asked for.
                Message::Request(request) => {
                    let start = request.begin as usize;
                    let end = start + request.length as usize;
                    let block = &payload[start..end];
                    let mut out = vec![0u8; 64 + block.len()];
                    let n = Message::Piece(Piece::from_data(request.index, request.begin, block))
                        .serialize(&mut out, &Default::default)
                        .unwrap();
                    write.write_all(&out[..n]).await.unwrap();
                }
                // The peer announcing the piece is the cue to ask for it.
                Message::Have(0) => {
                    script.announced = true;
                    if !asked {
                        asked = true;
                        let mut out = vec![0u8; 64];
                        let n = Message::Request(Request::new(0, 0, BLOCK_LEN))
                            .serialize(&mut out, &Default::default)
                            .unwrap();
                        write.write_all(&out[..n]).await.unwrap();
                    }
                }
                Message::Piece(piece) => {
                    let (first, second) = piece.data();
                    let mut block = Vec::with_capacity(first.len() + second.len());
                    block.extend_from_slice(first);
                    block.extend_from_slice(second);
                    script.served = Some(block);
                    return script;
                }
                _ => {}
            }
        }
    }

    /// One piece of `BLOCK_LEN * 2`, with the hash that makes it real.
    fn served_torrent(payload: &[u8]) -> TorrentUnderTest {
        let digest: [u8; 20] = <sha1::Sha1 as sha1::Digest>::digest(payload).into();
        TorrentUnderTest {
            info_hash: [4u8; 20],
            name: "payload".into(),
            piece_length: payload.len() as u32,
            total_length: payload.len() as u64,
            piece_hashes: vec![digest],
        }
    }

    fn serving_options(
        target: SocketAddr,
        dir: &std::path::Path,
        torrent: TorrentUnderTest,
    ) -> Options {
        Options {
            target,
            peers: 1,
            duration: Duration::from_secs(20),
            connect_timeout: Duration::from_secs(5),
            requests_in_flight: 4,
            disk_budget: 1 << 20,
            hold_dir: Some(dir.to_path_buf()),
            torrents: vec![torrent],
            mode: Mode::Leech,
        }
    }

    /// The serving path, end to end: the peer takes a piece, announces it,
    /// and hands it back when asked.
    ///
    /// The bytes are compared rather than counted. A peer that answered with
    /// the right length and the wrong offset would satisfy every counter in
    /// the report and be serving garbage.
    #[tokio::test]
    async fn a_synthetic_peer_serves_back_the_piece_it_was_given() {
        let payload: Vec<u8> = (0..BLOCK_LEN * 2).map(|i| (i % 251) as u8).collect();
        let torrent = served_torrent(&payload);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = listener.local_addr().unwrap();
        let script = tokio::spawn(scripted_target(
            listener,
            torrent.info_hash,
            payload.clone(),
            true,
        ));

        let dir = tempfile::tempdir().unwrap();
        let options = serving_options(target, dir.path(), torrent);
        let recorder = Arc::new(Recorder::new(Duration::ZERO, Duration::from_millis(50), 1));
        let outcome = run(&options, &recorder).await.expect("a run");
        let script = script.await.expect("the target");

        assert!(script.announced, "the peer never announced the piece");
        assert_eq!(
            script.served.as_deref(),
            Some(&payload[..BLOCK_LEN as usize]),
            "the target got back something other than the block it asked for"
        );
        assert_eq!(outcome.serving.pieces_announced, 1);
        assert_eq!(outcome.serving.peers_asked, 1);
        assert_eq!(outcome.serving.peers_target_interested, 1);
        assert_eq!(outcome.serving.blocks_sent, 1);
        assert_eq!(outcome.serving.bytes_sent.0, u64::from(BLOCK_LEN));
        assert_eq!(outcome.serving.requests_refused, 0);
        assert_eq!(outcome.pieces_verified, 1);
    }

    /// A target that wants nothing does not keep the peer on the connection.
    ///
    /// This is the rule the three leech cases in `check-swarm.ps1` depend on:
    /// against a seeder, which is never interested, a peer that has everything
    /// stops at `complete` rather than sitting out `--duration`. Without it
    /// every one of those cases would take the full sixty seconds.
    #[tokio::test]
    async fn a_peer_stops_at_complete_when_the_target_wants_nothing() {
        let payload: Vec<u8> = (0..BLOCK_LEN * 2).map(|i| (i % 251) as u8).collect();
        let torrent = served_torrent(&payload);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = listener.local_addr().unwrap();
        let script = tokio::spawn(scripted_target(
            listener,
            torrent.info_hash,
            payload.clone(),
            false,
        ));

        let dir = tempfile::tempdir().unwrap();
        let options = serving_options(target, dir.path(), torrent);
        let recorder = Arc::new(Recorder::new(Duration::ZERO, Duration::from_millis(50), 1));
        let started = Instant::now();
        let outcome = run(&options, &recorder).await.expect("a run");
        let elapsed = started.elapsed();
        script.abort();

        assert_eq!(outcome.peers[0].ended, "complete");
        assert_eq!(outcome.serving.peers_target_interested, 0);
        assert!(
            elapsed < options.duration,
            "the peer sat out the whole duration at {elapsed:?}"
        );
        // It still announced, because the announcement is what a target reads
        // before it decides whether it wants anything.
        assert_eq!(outcome.serving.pieces_announced, 1);
    }
}
