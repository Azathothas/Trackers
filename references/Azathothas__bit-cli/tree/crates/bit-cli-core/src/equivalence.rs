//! Deciding whether two torrents hold the same file, from the metadata alone.
//!
//! Two torrents that share a file are two downloads of the same bytes unless
//! something connects them. Connecting them safely means answering one
//! question first: are these two files actually the same? A caller can assert
//! it, and an assertion has to be verified before it is trusted, because a
//! wrong one corrupts a payload silently. This module answers it without an
//! assertion, where the metadata allows.
//!
//! A `.torrent` carries SHA-1 hashes of fixed-size pieces of the whole
//! payload, not of each file. So a file's bytes are covered by piece hashes
//! only where a piece lies entirely inside that file: the piece that straddles
//! the file's start also covers the end of the file before it, and its hash
//! says nothing about either one alone.
//!
//! For two files in two torrents to be compared by hash, the pieces have to
//! cover the same byte ranges of the file. That needs two things:
//!
//! - the same piece length, because a 2 MiB hash and a 1 MiB hash are hashes
//!   of different amounts of data and can never be equal for the same bytes;
//! - the same alignment, meaning the file's offset within its torrent is
//!   congruent modulo the piece length, so the first whole piece starts at the
//!   same place in the file both times.
//!
//! When both hold, the whole pieces inside the file line up one to one and
//! comparing their hashes proves the bytes equal, over that range, to the
//! strength of SHA-1. When they do not, nothing here can prove anything, and
//! [`Evidence::Length`] says exactly that: the lengths match and that is all.
//!
//! See `TODO/multi-source.md`, T-133.

use serde::{Deserialize, Serialize};

use crate::layout::Layout;

/// How strongly two files are known to be the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Evidence {
    /// Every piece that lies entirely inside both files has the same hash in
    /// both torrents, and at least one does. The bytes those pieces cover are
    /// the same. Nothing here is asserted by a caller.
    PieceHashes,
    /// The lengths match and nothing else could be checked: the piece lengths
    /// differ, the alignments differ, or the file is too small to contain a
    /// whole piece in either torrent. The files may or may not be the same,
    /// and only reading them says which.
    Length,
}

impl Evidence {
    /// The stable name used in JSON and text output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PieceHashes => "piece-hashes",
            Self::Length => "length",
        }
    }

    /// Whether this evidence is a proof rather than a candidate.
    pub const fn is_proof(self) -> bool {
        matches!(self, Self::PieceHashes)
    }
}

/// One file in one torrent matched against one file in another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    /// Index of the file in the torrent being asked about.
    pub index: usize,
    /// Index of the file in the other torrent.
    pub other_index: usize,
    /// Path of the file in the other torrent, as the torrent spells it.
    pub other_path: String,
    /// Length of the file, which is equal in both by construction.
    pub length: u64,
    pub evidence: Evidence,
    /// Whole pieces compared, zero when the evidence is `length`.
    pub pieces_compared: u32,
    /// Bytes those pieces cover, zero when the evidence is `length`.
    pub bytes_proven: u64,
}

/// Whole pieces of `layout` lying entirely inside the file at `index`, as a
/// half-open piece range, with the byte offset within the file where the first
/// one starts.
///
/// `None` when the file contains no whole piece, which is every file shorter
/// than the piece length and some that are longer but badly placed.
fn whole_pieces(layout: &Layout, index: usize) -> Option<(u32, u32, u64)> {
    let file = layout.file(index)?;
    let piece = u64::from(layout.piece_length);
    if piece == 0 {
        return None;
    }
    let start = file.offset.div_ceil(piece);
    let end = (file.offset + file.length) / piece;
    if end <= start {
        return None;
    }
    let offset_in_file = (start * piece) - file.offset;
    Some((start as u32, end as u32, offset_in_file))
}

/// Compare one file in one torrent against one file in another.
///
/// `None` when the lengths differ, which is the only thing that rules two
/// files out outright.
pub fn compare(
    left: &Layout,
    left_hashes: &[[u8; 20]],
    left_index: usize,
    right: &Layout,
    right_hashes: &[[u8; 20]],
    right_index: usize,
) -> Option<Match> {
    let a = left.file(left_index)?;
    let b = right.file(right_index)?;
    if a.length != b.length {
        return None;
    }

    let candidate = Match {
        index: left_index,
        other_index: right_index,
        other_path: b.display_path(),
        length: a.length,
        evidence: Evidence::Length,
        pieces_compared: 0,
        bytes_proven: 0,
    };

    // A hash of 2 MiB and a hash of 1 MiB are hashes of different amounts of
    // data, so a differing piece length rules the comparison out rather than
    // the files.
    if left.piece_length != right.piece_length {
        return Some(candidate);
    }
    let piece = u64::from(left.piece_length);
    // The first whole piece has to start at the same place in both files, or
    // the pieces cover different bytes of it.
    if a.offset % piece != b.offset % piece {
        return Some(candidate);
    }
    let (Some((a_start, a_end, a_offset)), Some((b_start, b_end, b_offset))) = (
        whole_pieces(left, left_index),
        whole_pieces(right, right_index),
    ) else {
        return Some(candidate);
    };
    debug_assert_eq!(
        a_offset, b_offset,
        "congruent offsets give the same first whole piece"
    );
    let count = (a_end - a_start).min(b_end - b_start);
    if count == 0 {
        return Some(candidate);
    }

    for step in 0..count {
        let left_hash = left_hashes.get((a_start + step) as usize);
        let right_hash = right_hashes.get((b_start + step) as usize);
        match (left_hash, right_hash) {
            (Some(x), Some(y)) if x == y => {}
            // A mismatch is the answer, not a missing proof: these two files
            // are not the same bytes.
            (Some(_), Some(_)) => return None,
            // Metadata that does not carry the hashes it claims to. Fall back
            // to the weaker answer rather than assert anything.
            _ => return Some(candidate),
        }
    }

    Some(Match {
        evidence: Evidence::PieceHashes,
        pieces_compared: count,
        bytes_proven: u64::from(count) * piece,
        ..candidate
    })
}

/// Every file in `left` that matches a file in `right`.
///
/// A file with several matches in the other torrent keeps them all, because
/// two copies of the same bytes under two names is a real case and picking one
/// is the caller's decision.
pub fn matches(
    left: &Layout,
    left_hashes: &[[u8; 20]],
    right: &Layout,
    right_hashes: &[[u8; 20]],
) -> Vec<Match> {
    let mut out = Vec::new();
    for left_index in 0..left.files.len() {
        for right_index in 0..right.files.len() {
            // A zero-length file matches every other zero-length file and
            // says nothing, so it is left out rather than reported hundreds
            // of times.
            if left.files[left_index].length == 0 {
                continue;
            }
            if let Some(found) = compare(
                left,
                left_hashes,
                left_index,
                right,
                right_hashes,
                right_index,
            ) {
                out.push(found);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(count: usize, fill: u8) -> Vec<[u8; 20]> {
        (0..count)
            .map(|index| {
                let mut hash = [fill; 20];
                hash[0] = index as u8;
                hash
            })
            .collect()
    }

    /// Two torrents whose shared file starts at the same offset with the same
    /// piece length: the pieces line up and the hashes prove it.
    #[test]
    fn a_shared_file_at_the_same_offset_and_piece_length_is_proven() {
        let left = Layout::from_lengths(
            "a",
            true,
            1024,
            [
                ("shared.bin".to_string(), 4096u64),
                ("tail.bin".into(), 100),
            ],
        );
        let right = Layout::from_lengths(
            "b",
            true,
            1024,
            [
                ("shared.bin".to_string(), 4096u64),
                ("other.bin".into(), 500),
            ],
        );
        let shared = hashes(5, 0xAA);
        let found = compare(&left, &shared, 0, &right, &shared, 0).expect("a match");
        assert_eq!(found.evidence, Evidence::PieceHashes);
        assert_eq!(found.pieces_compared, 4);
        assert_eq!(found.bytes_proven, 4096);
    }

    /// Different piece lengths cannot be compared at all, whatever the bytes
    /// are. This is the case the three-torrent fixture is built from, and it
    /// is why a declared equivalence exists.
    #[test]
    fn different_piece_lengths_leave_only_the_length() {
        let left = Layout::from_lengths("a", true, 1024, [("shared.bin".to_string(), 4096u64)]);
        let right = Layout::from_lengths("b", true, 2048, [("shared.bin".to_string(), 4096u64)]);
        let found = compare(&left, &hashes(4, 1), 0, &right, &hashes(2, 2), 0).expect("a match");
        assert_eq!(found.evidence, Evidence::Length);
        assert_eq!(found.pieces_compared, 0);
        assert_eq!(found.bytes_proven, 0);
    }

    /// The same piece length but a different alignment: piece k of one covers
    /// different bytes of the file than piece k of the other.
    #[test]
    fn a_different_alignment_leaves_only_the_length() {
        let left = Layout::from_lengths("a", true, 1024, [("shared.bin".to_string(), 4096u64)]);
        let right = Layout::from_lengths(
            "b",
            true,
            1024,
            [("pad.bin".to_string(), 512u64), ("shared.bin".into(), 4096)],
        );
        let found = compare(&left, &hashes(4, 3), 0, &right, &hashes(5, 3), 1).expect("a match");
        assert_eq!(found.evidence, Evidence::Length);
    }

    /// A hash that differs is an answer: these are not the same bytes.
    #[test]
    fn one_differing_piece_rules_the_pair_out() {
        let left = Layout::from_lengths("a", true, 1024, [("shared.bin".to_string(), 4096u64)]);
        let right = Layout::from_lengths("b", true, 1024, [("shared.bin".to_string(), 4096u64)]);
        let mut theirs = hashes(4, 0xAA);
        theirs[2][5] = 0xFF;
        assert!(compare(&left, &hashes(4, 0xAA), 0, &right, &theirs, 0).is_none());
    }

    /// Lengths that differ rule the pair out before any hash is read.
    #[test]
    fn different_lengths_are_not_a_match_at_all() {
        let left = Layout::from_lengths("a", true, 1024, [("shared.bin".to_string(), 4096u64)]);
        let right = Layout::from_lengths("b", true, 1024, [("shared.bin".to_string(), 2048u64)]);
        assert!(compare(&left, &hashes(4, 1), 0, &right, &hashes(2, 1), 0).is_none());
    }

    /// A file shorter than one piece contains no whole piece, so there is
    /// nothing to compare even when everything else lines up.
    #[test]
    fn a_file_smaller_than_a_piece_can_only_be_a_candidate() {
        let left = Layout::from_lengths(
            "a",
            true,
            4096,
            [("small.bin".to_string(), 100u64), ("rest.bin".into(), 8000)],
        );
        let right = Layout::from_lengths(
            "b",
            true,
            4096,
            [("small.bin".to_string(), 100u64), ("rest.bin".into(), 8000)],
        );
        let found = compare(&left, &hashes(2, 7), 0, &right, &hashes(2, 7), 0).expect("a match");
        assert_eq!(found.evidence, Evidence::Length);
    }

    /// Every pair, and a zero-length file matches nothing rather than
    /// everything.
    #[test]
    fn matches_pairs_every_file_and_skips_empty_ones() {
        let left = Layout::from_lengths(
            "a",
            true,
            1024,
            [
                ("shared.bin".to_string(), 4096u64),
                ("empty.bin".into(), 0),
                ("odd.bin".into(), 7),
            ],
        );
        let right = Layout::from_lengths(
            "b",
            true,
            1024,
            [
                ("shared.bin".to_string(), 4096u64),
                ("also-empty.bin".into(), 0),
            ],
        );
        let found = matches(&left, &hashes(5, 9), &right, &hashes(5, 9));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].index, 0);
        assert_eq!(found[0].other_index, 0);
        assert_eq!(found[0].evidence, Evidence::PieceHashes);
    }
}
