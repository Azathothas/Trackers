//! Which source supplied which block, so a piece that fails names the source
//! that broke it.
//!
//! `bit-cli` exists to point several sources at one payload, so its normal
//! case is the ambiguous one: a piece is filled from blocks that may have come
//! from several mirrors at once, and "the piece failed" says nothing about
//! which of them was wrong. Punishing every source that contributed retires
//! healthy mirrors; punishing none is what happened before this existed.
//!
//! The shape is `torrent/smartban/smartban.go` in the corpus, 83 lines: record
//! a hash of every block against the source that supplied it, and once the
//! correct bytes for that block are known, convict **every** source whose
//! recorded hash differs. It holds no block data and re-fetches nothing, so
//! the caller owes it correct bytes from somewhere. See `TODO/webseed.md`,
//! T-179.
//!
//! Two things here are this tree's rather than the corpus's:
//!
//! - **Only a disputed block is ever resolved.** A block whose recorded hashes
//!   all agree cannot convict anyone, because a piece that verified means the
//!   bytes everybody sent for it were the right ones. So correct bytes are
//!   only ever needed for the handful of blocks two sources disagreed about,
//!   which is one 16 KiB read rather than a whole piece.
//! - **The ledger is bounded and says when it dropped something.** A run whose
//!   pieces never resolve must not grow a map for the length of the download,
//!   and a conviction that could not be made because the record was evicted is
//!   worth counting rather than hiding.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Mutex;

/// One block as the wire addresses it: piece, offset within the piece, length.
///
/// The length is part of the key rather than a field beside it. Two reads at
/// the same offset with different lengths are different byte ranges, and
/// hashes taken over different lengths are not comparable; keying on all three
/// means a mismatched length can never convict anyone.
pub type BlockKey = (u32, u32, u32);

/// How many pieces a ledger holds records for before it drops the oldest.
///
/// A piece is forgotten as soon as the session verifies it, so in a healthy
/// run the ledger holds the pieces in flight and nothing else. This is the cap
/// for the unhealthy case, where a piece never verifies and its records would
/// otherwise be kept for the length of the run.
pub const DEFAULT_PIECE_LIMIT: usize = 256;

/// A conviction: one source, one block, and the two hashes that disagree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Conviction {
    /// Index of the source in the binding set.
    pub source: usize,
    pub piece: u32,
    pub begin: u32,
    pub length: u32,
    /// What this source sent, in hex.
    pub served: String,
    /// What the verified payload holds, in hex.
    pub correct: String,
}

impl std::fmt::Display for Conviction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "served {} bytes at piece {} offset {} hashing to {}, but the verified payload hashes to {}",
            self.length, self.piece, self.begin, self.served, self.correct
        )
    }
}

/// What the ledger has done, for a report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LedgerStats {
    /// Blocks recorded across every source.
    pub recorded: u64,
    /// Pieces held right now.
    pub pieces_held: usize,
    /// Pieces dropped because the ledger was full before they resolved.
    ///
    /// Not an error on its own. It is the number of pieces that could no
    /// longer be attributed if they turned out to be wrong, which is the one
    /// thing a bounded ledger costs.
    pub evicted: u64,
    /// Pieces resolved against verified bytes.
    pub resolved: u64,
}

/// One block within a piece: its offset and its length.
type BlockWithin = (u32, u32);

/// One source and the SHA-1 of what it sent for a block.
type SourceHash = (usize, [u8; 20]);

/// One piece's records, keyed by the block within it.
#[derive(Default)]
struct PieceRecord {
    blocks: BTreeMap<BlockWithin, Vec<SourceHash>>,
}

impl PieceRecord {
    /// Blocks whose recorded hashes do not all agree.
    ///
    /// A block only one source ever sent, or that every source sent
    /// identically, cannot convict anyone once the piece verifies: the bytes
    /// that verified are the bytes that were sent.
    fn disputed(&self) -> Vec<BlockWithin> {
        self.blocks
            .iter()
            .filter(|(_, records)| records.iter().any(|(_, hash)| *hash != records[0].1))
            .map(|(key, _)| *key)
            .collect()
    }
}

#[derive(Default)]
struct Inner {
    pieces: HashMap<u32, PieceRecord>,
    /// Insertion order, so the oldest piece is the one evicted.
    order: VecDeque<u32>,
    recorded: u64,
    evicted: u64,
    resolved: u64,
}

/// A block-to-source map for one torrent.
///
/// Shared by every source attached to that torrent, because attribution is a
/// statement about which of them sent a block and cannot be made from inside
/// any one of them.
pub struct BlockLedger {
    /// Length of a non-final piece, so a block key becomes a payload offset.
    piece_length: u32,
    limit: usize,
    inner: Mutex<Inner>,
}

/// The counters rather than the map.
///
/// A ledger holding a thousand block hashes has no useful `Debug` output, and
/// printing one into a log is how a diagnostic turns into a page of hex.
impl std::fmt::Debug for BlockLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockLedger")
            .field("piece_length", &self.piece_length)
            .field("limit", &self.limit)
            .field("stats", &self.stats())
            .finish()
    }
}

impl BlockLedger {
    /// A ledger for a torrent with pieces of `piece_length` bytes.
    pub fn new(piece_length: u32) -> Self {
        Self::with_limit(piece_length, DEFAULT_PIECE_LIMIT)
    }

    /// [`Self::new`] holding records for at most `limit` pieces.
    pub fn with_limit(piece_length: u32, limit: usize) -> Self {
        Self {
            piece_length,
            limit: limit.max(1),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Note that `source` supplied these bytes for this block.
    ///
    /// Called once per block actually put on the wire. A block fetched and
    /// then dropped because the session cancelled it never reached the piece
    /// and must not be able to convict anyone.
    pub fn record(&self, source: usize, key: BlockKey, data: &[u8]) {
        let (piece, begin, length) = key;
        let hash = sha1_of(data);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.recorded += 1;
        if !inner.pieces.contains_key(&piece) {
            while inner.order.len() >= self.limit {
                // The order entry and the map entry go together. An order
                // entry whose piece is already gone would otherwise leak a
                // slot and shrink the ledger by one for the rest of the run.
                match inner.order.pop_front() {
                    Some(oldest) => {
                        if inner.pieces.remove(&oldest).is_some() {
                            inner.evicted += 1;
                        }
                    }
                    None => break,
                }
            }
            inner.order.push_back(piece);
        }
        let record = inner.pieces.entry(piece).or_default();
        let entries = record.blocks.entry((begin, length)).or_default();
        // The same source sending the same bytes twice is one fact, not two.
        // Without this a source re-serving a block after a reconnect would
        // grow the record without adding anything to it.
        if !entries
            .iter()
            .any(|(who, what)| *who == source && *what == hash)
        {
            entries.push((source, hash));
        }
    }

    /// Blocks in `piece` whose recorded hashes disagree, as `(begin, length)`.
    ///
    /// Empty for a piece nobody disagreed about, which is the ordinary case
    /// and the reason resolving costs nothing in a healthy run.
    pub fn disputed_blocks(&self, piece: u32) -> Vec<BlockWithin> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .pieces
            .get(&piece)
            .map(PieceRecord::disputed)
            .unwrap_or_default()
    }

    /// Every piece the ledger holds a disagreement about.
    ///
    /// Sorted, so a caller resolving them reports in piece order rather than
    /// in hash order.
    pub fn disputed_pieces(&self) -> Vec<u32> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<u32> = inner
            .pieces
            .iter()
            .filter(|(_, record)| !record.disputed().is_empty())
            .map(|(piece, _)| *piece)
            .collect();
        out.sort_unstable();
        out
    }

    /// Every source whose recorded hash for this block differs from `correct`.
    ///
    /// `correct` is the verified payload's own bytes for that block. A source
    /// that agrees with them is not returned however many other blocks it got
    /// wrong: this convicts per block, and the caller aggregates.
    pub fn check(&self, key: BlockKey, correct: &[u8]) -> Vec<Conviction> {
        let (piece, begin, length) = key;
        let truth = sha1_of(correct);
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(record) = inner.pieces.get(&piece) else {
            return Vec::new();
        };
        record
            .blocks
            .get(&(begin, length))
            .map(|entries| {
                entries
                    .iter()
                    .filter(|(_, hash)| *hash != truth)
                    .map(|(source, hash)| Conviction {
                        source: *source,
                        piece,
                        begin,
                        length,
                        served: hex(hash),
                        correct: hex(&truth),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Drop every record for `piece`.
    ///
    /// Called once the piece is resolved, which is what keeps the ledger the
    /// size of the pieces in flight rather than the size of the torrent.
    pub fn forget_piece(&self, piece: u32) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.pieces.remove(&piece).is_some() {
            inner.order.retain(|held| *held != piece);
        }
    }

    /// Payload offset of a block, for a caller that has to read it back.
    pub fn offset_of(&self, piece: u32, begin: u32) -> u64 {
        u64::from(piece) * u64::from(self.piece_length) + u64::from(begin)
    }

    /// Resolve every disputed piece the session has since verified.
    ///
    /// `have` is one bool per piece, and only a piece the session says it
    /// holds is resolved: a piece that has not verified has no correct bytes
    /// anywhere, and guessing at them is how a healthy mirror gets retired.
    /// `read` is handed a payload offset and a length and returns the verified
    /// bytes, or `None` if it could not read them, in which case the piece is
    /// left for the next pass rather than resolved wrongly.
    ///
    /// A resolved piece is forgotten whether or not it convicted anyone.
    pub fn resolve(
        &self,
        have: &[bool],
        mut read: impl FnMut(u64, u32) -> Option<Vec<u8>>,
    ) -> Vec<Conviction> {
        let mut out = Vec::new();
        for piece in self.disputed_pieces() {
            if !have.get(piece as usize).copied().unwrap_or(false) {
                continue;
            }
            let mut unread = false;
            for (begin, length) in self.disputed_blocks(piece) {
                match read(self.offset_of(piece, begin), length) {
                    Some(correct) => out.extend(self.check((piece, begin, length), &correct)),
                    None => unread = true,
                }
            }
            if unread {
                continue;
            }
            self.forget_piece(piece);
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.resolved += 1;
        }
        out
    }

    /// Forget every piece the session has verified and nobody disagreed about.
    ///
    /// This is the housekeeping half of [`Self::resolve`]: a piece that
    /// verified with no dispute can convict nobody, so its records are dead
    /// weight. Without it the ledger fills with settled pieces and the bound
    /// starts evicting records that still matter.
    pub fn forget_settled(&self, have: &[bool]) -> usize {
        let settled: Vec<u32> = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner
                .pieces
                .iter()
                .filter(|(piece, record)| {
                    have.get(**piece as usize).copied().unwrap_or(false)
                        && record.disputed().is_empty()
                })
                .map(|(piece, _)| *piece)
                .collect()
        };
        for piece in &settled {
            self.forget_piece(*piece);
        }
        settled.len()
    }

    /// What the ledger has done so far.
    pub fn stats(&self) -> LedgerStats {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        LedgerStats {
            recorded: inner.recorded,
            pieces_held: inner.pieces.len(),
            evicted: inner.evicted,
            resolved: inner.resolved,
        }
    }
}

fn sha1_of(data: &[u8]) -> [u8; 20] {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIECE: u32 = 64 * 1024;

    fn ledger() -> BlockLedger {
        BlockLedger::new(PIECE)
    }

    #[test]
    fn a_block_only_one_source_sent_is_never_disputed() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        assert!(ledger.disputed_blocks(3).is_empty());
        assert!(ledger.disputed_pieces().is_empty());
    }

    #[test]
    fn two_sources_agreeing_is_not_a_dispute() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        ledger.record(1, (3, 0, 16), b"the right bytes.");
        assert!(ledger.disputed_blocks(3).is_empty());
    }

    #[test]
    fn two_sources_disagreeing_is_a_dispute_on_that_block_alone() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        ledger.record(1, (3, 0, 16), b"the WRONG bytes.");
        ledger.record(0, (3, 16, 16), b"another block!!!");
        assert_eq!(ledger.disputed_blocks(3), vec![(0, 16)]);
        assert_eq!(ledger.disputed_pieces(), vec![3]);
    }

    #[test]
    fn a_check_convicts_only_the_source_that_disagrees_with_the_truth() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        ledger.record(1, (3, 0, 16), b"the WRONG bytes.");
        ledger.record(2, (3, 0, 16), b"the right bytes.");
        let convicted = ledger.check((3, 0, 16), b"the right bytes.");
        assert_eq!(
            convicted.iter().map(|c| c.source).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(convicted[0].piece, 3);
        assert_eq!(convicted[0].begin, 0);
        assert_eq!(convicted[0].length, 16);
        assert_ne!(convicted[0].served, convicted[0].correct);
    }

    /// The whole point of the corpus reference: it returns *every* peer whose
    /// hash differs, not the last one to answer.
    #[test]
    fn a_check_convicts_every_source_that_disagrees_at_once() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        ledger.record(1, (3, 0, 16), b"the WRONG bytes.");
        ledger.record(2, (3, 0, 16), b"also wrong bytes");
        let mut convicted: Vec<usize> = ledger
            .check((3, 0, 16), b"the right bytes.")
            .into_iter()
            .map(|c| c.source)
            .collect();
        convicted.sort_unstable();
        assert_eq!(convicted, vec![1, 2]);
    }

    /// The failure this entry exists to prevent: a piece that verified with
    /// everybody agreeing convicts nobody, however many sources filled it.
    #[test]
    fn a_piece_every_source_got_right_convicts_nobody() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        ledger.record(1, (3, 16, 16), b"another block!!!");
        let convicted = ledger.resolve(&[false, false, false, true], |_, length| {
            Some(vec![0u8; length as usize])
        });
        assert!(convicted.is_empty(), "{convicted:?}");
    }

    #[test]
    fn the_same_source_sending_the_same_bytes_twice_is_recorded_once() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        ledger.record(1, (3, 0, 16), b"the WRONG bytes.");
        assert_eq!(ledger.check((3, 0, 16), b"the right bytes.").len(), 1);
    }

    /// A source that changed its mind is two facts, and the ledger keeps both:
    /// it served wrong bytes once, which is what convicts it.
    #[test]
    fn one_source_sending_two_different_answers_keeps_both() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the WRONG bytes.");
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        assert_eq!(ledger.disputed_blocks(3), vec![(0, 16)]);
        let convicted = ledger.check((3, 0, 16), b"the right bytes.");
        assert_eq!(convicted.len(), 1);
        assert_eq!(convicted[0].source, 0);
    }

    /// Hashes taken over different lengths are not comparable, so a read of a
    /// different size at the same offset is a different block and cannot
    /// convict the source that served the first one.
    #[test]
    fn the_same_offset_at_a_different_length_is_a_different_block() {
        let ledger = ledger();
        ledger.record(0, (3, 0, 16), b"the right bytes.");
        ledger.record(1, (3, 0, 8), b"the righ");
        assert!(ledger.disputed_blocks(3).is_empty());
        assert!(ledger.check((3, 0, 16), b"the right bytes.").is_empty());
    }

    #[test]
    fn resolve_reads_only_the_disputed_blocks() {
        let ledger = ledger();
        ledger.record(0, (1, 0, 16), b"the right bytes.");
        ledger.record(1, (1, 0, 16), b"the WRONG bytes.");
        for begin in 1..8u32 {
            ledger.record(0, (1, begin * 16, 16), b"a settled block!");
        }
        let mut reads = Vec::new();
        let convicted = ledger.resolve(&[false, true], |offset, length| {
            reads.push((offset, length));
            Some(b"the right bytes.".to_vec())
        });
        assert_eq!(reads, vec![(u64::from(PIECE), 16)]);
        assert_eq!(convicted.len(), 1);
        assert_eq!(convicted[0].source, 1);
    }

    #[test]
    fn a_piece_the_session_does_not_hold_is_left_alone() {
        let ledger = ledger();
        ledger.record(0, (1, 0, 16), b"the right bytes.");
        ledger.record(1, (1, 0, 16), b"the WRONG bytes.");
        let convicted = ledger.resolve(&[false, false], |_, _| {
            panic!("a piece the session has not verified has no correct bytes to read")
        });
        assert!(convicted.is_empty());
        // Still held, so the next pass can resolve it once the piece lands.
        assert_eq!(ledger.disputed_pieces(), vec![1]);
    }

    #[test]
    fn a_block_that_cannot_be_read_back_leaves_the_piece_for_the_next_pass() {
        let ledger = ledger();
        ledger.record(0, (1, 0, 16), b"the right bytes.");
        ledger.record(1, (1, 0, 16), b"the WRONG bytes.");
        assert!(ledger.resolve(&[false, true], |_, _| None).is_empty());
        assert_eq!(ledger.disputed_pieces(), vec![1]);
        assert_eq!(ledger.stats().resolved, 0);

        let convicted = ledger.resolve(&[false, true], |_, _| Some(b"the right bytes.".to_vec()));
        assert_eq!(convicted.len(), 1);
        assert_eq!(ledger.stats().resolved, 1);
    }

    #[test]
    fn a_resolved_piece_is_forgotten() {
        let ledger = ledger();
        ledger.record(0, (1, 0, 16), b"the right bytes.");
        ledger.record(1, (1, 0, 16), b"the WRONG bytes.");
        ledger.resolve(&[false, true], |_, _| Some(b"the right bytes.".to_vec()));
        assert!(ledger.disputed_pieces().is_empty());
        assert_eq!(ledger.stats().pieces_held, 0);
    }

    #[test]
    fn a_settled_piece_the_session_holds_is_forgotten_without_a_read() {
        let ledger = ledger();
        ledger.record(0, (0, 0, 16), b"the right bytes.");
        ledger.record(0, (1, 0, 16), b"another block!!!");
        assert_eq!(ledger.stats().pieces_held, 2);
        assert_eq!(ledger.forget_settled(&[true, false]), 1);
        assert_eq!(ledger.stats().pieces_held, 1);
    }

    #[test]
    fn a_disputed_piece_is_never_forgotten_as_settled() {
        let ledger = ledger();
        ledger.record(0, (0, 0, 16), b"the right bytes.");
        ledger.record(1, (0, 0, 16), b"the WRONG bytes.");
        assert_eq!(ledger.forget_settled(&[true]), 0);
        assert_eq!(ledger.stats().pieces_held, 1);
    }

    #[test]
    fn the_ledger_evicts_the_oldest_piece_and_counts_it() {
        let ledger = BlockLedger::with_limit(PIECE, 2);
        for piece in 0..4u32 {
            ledger.record(0, (piece, 0, 16), b"the right bytes.");
        }
        let stats = ledger.stats();
        assert_eq!(stats.pieces_held, 2);
        assert_eq!(stats.evicted, 2);
        assert_eq!(stats.recorded, 4);
        assert!(ledger.disputed_blocks(0).is_empty());
        assert!(ledger.disputed_blocks(1).is_empty());
    }

    /// Forgetting a piece has to take its order entry with it. Without that,
    /// every forget would leave a stale slot and the ledger would evict a live
    /// piece one record sooner for the rest of the run.
    #[test]
    fn forgetting_a_piece_gives_its_slot_back() {
        let ledger = BlockLedger::with_limit(PIECE, 2);
        ledger.record(0, (0, 0, 16), b"the right bytes.");
        ledger.record(0, (1, 0, 16), b"the right bytes.");
        ledger.forget_piece(0);
        ledger.record(0, (2, 0, 16), b"the right bytes.");
        let stats = ledger.stats();
        assert_eq!(stats.pieces_held, 2);
        assert_eq!(
            stats.evicted, 0,
            "piece 1 was evicted for a slot that was free"
        );
    }

    #[test]
    fn a_block_key_becomes_a_payload_offset() {
        let ledger = ledger();
        assert_eq!(ledger.offset_of(0, 0), 0);
        assert_eq!(ledger.offset_of(1, 0), u64::from(PIECE));
        assert_eq!(ledger.offset_of(2, 16_384), u64::from(PIECE) * 2 + 16_384);
    }

    #[test]
    fn a_conviction_reads_as_a_sentence() {
        let ledger = ledger();
        ledger.record(1, (3, 0, 16), b"the WRONG bytes.");
        let convicted = ledger.check((3, 0, 16), b"the right bytes.");
        let text = convicted[0].to_string();
        assert!(text.contains("piece 3"), "{text}");
        assert!(text.contains("offset 0"), "{text}");
        assert!(text.contains("16 bytes"), "{text}");
    }
}
