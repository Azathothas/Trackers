//! Choosing a piece length.
//!
//! The trade is between metadata size and transfer granularity. Small pieces
//! mean a large `pieces` field (20 bytes each), which makes the `.torrent`
//! itself big and slow to exchange over BEP 9. Large pieces mean a peer has to
//! finish more bytes before any of it verifies, which hurts on a lossy link
//! and makes partial-piece web seed scopes coarser.
//!
//! The rule here targets roughly 1000 to 2000 pieces, clamped to a power of
//! two between 16 KiB and 16 MiB. That keeps the piece count in the range
//! every client handles well and the metadata under about 40 KiB.
//!
//! The chosen value and the reasoning are printed under `bit-cli create
//! --show`, and `--piece-length` overrides it.

use crate::error::{Error, Result};
use crate::units::{KIB, MIB};

/// Smallest piece length that will be chosen automatically.
pub const MIN: u32 = 16 * KIB as u32;

/// Largest piece length that will be chosen automatically.
pub const MAX: u32 = 16 * MIB as u32;

/// Pieces the heuristic aims for.
const TARGET_PIECES: u64 = 1500;

/// Choose a piece length for a payload of `total_bytes`.
pub fn choose(total_bytes: u64) -> u32 {
    if total_bytes == 0 {
        return MIN;
    }
    let ideal = total_bytes.div_ceil(TARGET_PIECES).max(1);
    // Round up to a power of two: every client in existence assumes one, and
    // BEP 3 recommends it.
    let rounded = ideal.next_power_of_two();
    rounded.clamp(u64::from(MIN), u64::from(MAX)) as u32
}

/// Why a piece length was chosen, for `--show`.
pub fn explain(total_bytes: u64, piece_length: u32) -> String {
    let pieces = total_bytes.div_ceil(u64::from(piece_length)).max(1);
    format!(
        "{} for {} of payload gives {pieces} pieces and {} of piece hashes",
        crate::units::format_size(u64::from(piece_length)),
        crate::units::format_size(total_bytes),
        crate::units::format_size(pieces * 20)
    )
}

/// Check a caller-supplied piece length.
///
/// Only zero is refused outright, because only zero is impossible. BEP 3
/// recommends at least 16 KiB, but a small payload legitimately wants smaller
/// pieces, and refusing would make it impossible to build a usable torrent for
/// one. The cases that actually hurt, an absurd piece count in either
/// direction and a length that is not a power of two, are lints, so they are
/// reported by name and cleared with `--allow`.
pub fn validate(piece_length: u32) -> Result<()> {
    if piece_length == 0 {
        return Err(Error::usage("piece length cannot be zero"));
    }
    Ok(())
}

/// Whether a piece length is a power of two.
pub fn is_power_of_two(piece_length: u32) -> bool {
    piece_length.is_power_of_two()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::GIB;

    #[test]
    fn every_choice_is_a_power_of_two_within_the_bounds() {
        let sizes = [
            0,
            1,
            1024,
            MIB,
            100 * MIB,
            GIB,
            20 * GIB,
            500 * GIB,
            10_000 * GIB,
        ];
        for size in sizes {
            let chosen = choose(size);
            assert!(chosen.is_power_of_two(), "{size} gave {chosen}");
            assert!((MIN..=MAX).contains(&chosen), "{size} gave {chosen}");
        }
    }

    #[test]
    fn small_payloads_get_the_minimum() {
        assert_eq!(choose(0), MIN);
        assert_eq!(choose(1), MIN);
        assert_eq!(choose(MIB), MIN);
    }

    #[test]
    fn large_payloads_get_the_maximum() {
        assert_eq!(choose(100_000 * GIB), MAX);
    }

    #[test]
    fn the_piece_count_stays_in_a_sensible_band() {
        // Anything from 32 MiB up should land between a few hundred and a few
        // thousand pieces, which is where clients and BEP 9 are happiest.
        for size in [32 * MIB, 700 * MIB, 4 * GIB, 50 * GIB, 200 * GIB] {
            let chosen = u64::from(choose(size));
            let pieces = size.div_ceil(chosen);
            assert!(
                (256..=4096).contains(&pieces) || chosen == u64::from(MAX),
                "{size} bytes at {chosen} gives {pieces} pieces"
            );
        }
    }

    #[test]
    fn the_choice_is_monotonic() {
        let mut previous = 0;
        for exponent in 10..46u32 {
            let chosen = choose(1u64 << exponent);
            assert!(
                chosen >= previous,
                "2^{exponent} gave {chosen} after {previous}"
            );
            previous = chosen;
        }
    }

    #[test]
    fn metadata_stays_small() {
        // The hashes for a 50 GiB payload should stay well under 100 KiB.
        let chosen = u64::from(choose(50 * GIB));
        let hash_bytes = (50 * GIB).div_ceil(chosen) * 20;
        assert!(hash_bytes < 100 * KIB, "{hash_bytes} bytes of hashes");
    }

    #[test]
    fn the_explanation_names_the_numbers() {
        let text = explain(4 * GIB, 4 * MIB as u32);
        assert!(text.contains("4.00 MiB"), "{text}");
        assert!(text.contains("1024 pieces"), "{text}");
    }

    #[test]
    fn only_a_zero_piece_length_is_refused_outright() {
        assert!(validate(0).is_err());
        // Small pieces are legal and are what a small payload needs. The
        // piece-count lint catches the cases that actually hurt.
        assert!(validate(1024).is_ok());
        assert!(validate(16 * KIB as u32).is_ok());
        assert!(validate(MAX).is_ok());
        assert!(validate(u32::MAX).is_ok());
    }

    #[test]
    fn power_of_two_detection_is_exact() {
        assert!(is_power_of_two(16384));
        assert!(!is_power_of_two(16385));
        assert!(!is_power_of_two(0));
    }
}
