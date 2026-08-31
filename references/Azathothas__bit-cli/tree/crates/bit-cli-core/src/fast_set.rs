//! BEP 6's allowed-fast set, and the one place two deployed clients disagree.
//!
//! A seeder that is choking a peer may still serve a small, fixed set of
//! pieces to it, so a new peer can start without waiting to be unchoked. Which
//! pieces is not negotiated: both sides derive the same set from the peer's
//! address, the info hash, and the piece count, and the derivation has to
//! agree byte for byte or the feature is worse than not having it.
//!
//! # The divergence
//!
//! BEP 6 says to mask the peer's address to a /24 before hashing. **aria2 does
//! not.** `aria2_rust/aria2-protocol/src/bittorrent/fast_set.rs:150` mirrors
//! aria2's own C++: an address whose first octet has bit 7 or bit 6 clear,
//! which is the old class A and class B space, is masked to a /16 instead. So
//! aria2 and a conformant client derive different sets for the same peer
//! whenever the peer's address is below 192.0.0.0, which is most of the
//! routable internet.
//!
//! Both are implemented here, selected by [`Mask`], because a measurement that
//! can only say "these do not match" is less useful than one that can say
//! which of the two the other end is using. [`Mask::Bep6`] is what to send.
//!
//! # Where this is used, and where it is not
//!
//! `bench swarm` uses it to check what a target announced as its allowed-fast
//! set against what the target should have announced.
//!
//! `bit-cli`'s own peer implementation, the web seed bridge, does **not** send
//! an allowed-fast set, and cannot: its only counterparty is the `librqbit`
//! session in the same process, and `librqbit` 9.0.0 has no BEP 6 at all. See
//! `TODO/bep-coverage.md`, T-100.
//!
//! The algorithm is `vortex/bittorrent/src/peer_comm/peer_connection.rs:89`,
//! read rather than copied, and the conformance vector in the tests below is
//! from anacrolix [PR 1052](https://github.com/anacrolix/torrent/pull/1052).

use std::net::Ipv4Addr;

use sha1::{Digest, Sha1};

/// The spec caps the search so a small piece count cannot spin forever.
const MAX_ROUNDS: u32 = 300;

/// Which masking rule to derive the set under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mask {
    /// BEP 6 as written: keep the first three octets.
    Bep6,
    /// aria2's rule: class A and class B addresses keep the first two octets,
    /// class C keeps three. Not the spec, and widely deployed.
    Aria2,
}

impl Mask {
    /// The stable name used in reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bep6 => "bep6",
            Self::Aria2 => "aria2",
        }
    }

    /// Apply the rule to an address.
    #[must_use]
    pub fn apply(self, ip: Ipv4Addr) -> [u8; 4] {
        let mut octets = ip.octets();
        match self {
            Self::Bep6 => octets[3] = 0,
            Self::Aria2 => match (octets[0] & 0x80) == 0 || (octets[0] & 0x40) == 0 {
                true => {
                    octets[2] = 0;
                    octets[3] = 0;
                }
                false => octets[3] = 0,
            },
        }
        octets
    }

    /// Whether the two rules give the same answer for this address.
    ///
    /// They agree only above 192.0.0.0, so a measurement against a peer on a
    /// class C address cannot tell the two apart and has to say so rather than
    /// report a match.
    #[must_use]
    pub fn is_ambiguous(ip: Ipv4Addr) -> bool {
        Self::Bep6.apply(ip) == Self::Aria2.apply(ip)
    }
}

/// The allowed-fast set for one peer, in the order the algorithm produces it.
///
/// Order is part of the answer rather than incidental: the set is sent as a
/// sequence of `allowed fast` messages, and a receiver comparing what arrived
/// against what should have arrived compares sequences.
///
/// The sequence is **prefix stable**: the first `n` of a set of `m` are the
/// set of `n`, because indices are appended in the order the digest produces
/// them and nothing is reordered afterwards. That is what lets a receiver that
/// caught only part of a set still check the part it caught, which is
/// `classify_allowed_fast` in `bench swarm`.
///
/// A torrent with no more pieces than `set_size` gets every piece, which is
/// what the corpus implementations do and what the algorithm would converge to
/// anyway, slowly.
#[must_use]
pub fn allowed_fast(
    mask: Mask,
    set_size: u32,
    num_pieces: u32,
    info_hash: &[u8; 20],
    ip: Ipv4Addr,
) -> Vec<u32> {
    if num_pieces == 0 {
        return Vec::new();
    }
    if num_pieces <= set_size {
        return (0..num_pieces).collect();
    }

    let mut seed = Vec::with_capacity(24);
    seed.extend_from_slice(&mask.apply(ip));
    seed.extend_from_slice(info_hash);

    let mut out: Vec<u32> = Vec::with_capacity(set_size as usize);
    let mut rounds = 0;
    while (out.len() as u32) < set_size && rounds < MAX_ROUNDS {
        rounds += 1;
        let digest = Sha1::digest(&seed);
        // Five big-endian u32s per round, which is the twenty byte digest.
        // Indexed rather than chunked: the length is fixed and known, and
        // `chunks_exact` with a constant is `clippy::chunks_exact_to_as_chunks`
        // from Rust 1.98.
        for word in 0..5 {
            if (out.len() as u32) >= set_size {
                break;
            }
            let at = word * 4;
            let value =
                u32::from_be_bytes([digest[at], digest[at + 1], digest[at + 2], digest[at + 3]]);
            let index = value % num_pieces;
            if !out.contains(&index) {
                out.push(index);
            }
        }
        seed = digest.to_vec();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The conformance vector from anacrolix PR 1052, which vortex and
    /// anacrolix both reproduce. This is the whole reason to write the
    /// algorithm rather than describe it.
    #[test]
    fn the_canonical_vector_reproduces() {
        let found = allowed_fast(
            Mask::Bep6,
            7,
            1313,
            &[0xAA; 20],
            "80.4.4.200".parse().unwrap(),
        );
        assert_eq!(found, vec![1059, 431, 808, 1217, 287, 376, 1188]);
    }

    #[test]
    fn the_set_is_stable_across_the_host_part_of_the_address() {
        // The whole point of masking: two peers on one /24 get one set, so a
        // peer that reconnects from a new port or a neighbour on the same
        // subnet is not handed a different set.
        let one = allowed_fast(
            Mask::Bep6,
            6,
            1313,
            &[0xAA; 20],
            "80.4.4.1".parse().unwrap(),
        );
        let two = allowed_fast(
            Mask::Bep6,
            6,
            1313,
            &[0xAA; 20],
            "80.4.4.254".parse().unwrap(),
        );
        assert_eq!(one, two);

        let elsewhere = allowed_fast(
            Mask::Bep6,
            6,
            1313,
            &[0xAA; 20],
            "80.4.5.1".parse().unwrap(),
        );
        assert_ne!(one, elsewhere, "a different /24 is a different set");
    }

    #[test]
    fn aria2_derives_a_different_set_below_192() {
        // 80.x is class A under aria2's rule, so it masks to /16 and the two
        // disagree. This is the divergence, asserted rather than described:
        // if it ever stops being true, this test says so before somebody
        // debugs a mismatch against a live aria2.
        let ip: Ipv4Addr = "80.4.4.200".parse().unwrap();
        assert!(!Mask::is_ambiguous(ip));
        assert_eq!(Mask::Bep6.apply(ip), [80, 4, 4, 0]);
        assert_eq!(Mask::Aria2.apply(ip), [80, 4, 0, 0]);
        assert_ne!(
            allowed_fast(Mask::Bep6, 7, 1313, &[0xAA; 20], ip),
            allowed_fast(Mask::Aria2, 7, 1313, &[0xAA; 20], ip)
        );
    }

    #[test]
    fn the_two_rules_agree_on_a_class_c_address() {
        let ip: Ipv4Addr = "203.0.113.7".parse().unwrap();
        assert!(Mask::is_ambiguous(ip));
        assert_eq!(Mask::Bep6.apply(ip), Mask::Aria2.apply(ip));
        assert_eq!(
            allowed_fast(Mask::Bep6, 6, 1313, &[0xAA; 20], ip),
            allowed_fast(Mask::Aria2, 6, 1313, &[0xAA; 20], ip)
        );
    }

    #[test]
    fn the_boundary_between_the_two_rules_is_192() {
        // 191.x has bit 7 set and bit 6 clear, so aria2 treats it as class B.
        assert_eq!(
            Mask::Aria2.apply("191.255.255.255".parse().unwrap()),
            [191, 255, 0, 0]
        );
        assert_eq!(
            Mask::Aria2.apply("192.0.0.1".parse().unwrap()),
            [192, 0, 0, 0]
        );
    }

    #[test]
    fn a_shorter_set_is_a_prefix_of_a_longer_one() {
        // Claimed in the doc comment and relied on by the receiver, which may
        // catch only part of a set before the connection ends.
        let ip: Ipv4Addr = "80.4.4.200".parse().unwrap();
        let long = allowed_fast(Mask::Bep6, 7, 1313, &[0xAA; 20], ip);
        for n in 1..=7 {
            let short = allowed_fast(Mask::Bep6, n, 1313, &[0xAA; 20], ip);
            assert_eq!(short, long[..n as usize], "at {n}");
        }
    }

    #[test]
    fn a_torrent_smaller_than_the_set_gets_every_piece() {
        let found = allowed_fast(Mask::Bep6, 6, 4, &[0xAA; 20], "80.4.4.200".parse().unwrap());
        assert_eq!(found, vec![0, 1, 2, 3]);
    }

    #[test]
    fn a_torrent_of_exactly_the_set_size_gets_every_piece() {
        let found = allowed_fast(Mask::Bep6, 6, 6, &[0xAA; 20], "80.4.4.200".parse().unwrap());
        assert_eq!(found, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_piece_count_that_cannot_fill_the_set_terminates() {
        // Seven pieces and a set of six is filled by the algorithm rather than
        // by the shortcut above, and every index has to be distinct, so this
        // is the case that leans hardest on the round cap.
        let found = allowed_fast(Mask::Bep6, 6, 7, &[0xAA; 20], "80.4.4.200".parse().unwrap());
        assert_eq!(found.len(), 6);
        let mut sorted = found.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 6, "the set has a repeat: {found:?}");
        assert!(found.iter().all(|&i| i < 7));
    }

    #[test]
    fn no_pieces_is_an_empty_set_rather_than_a_division_by_zero() {
        let found = allowed_fast(Mask::Bep6, 6, 0, &[0xAA; 20], "80.4.4.200".parse().unwrap());
        assert!(found.is_empty());
    }

    #[test]
    fn the_info_hash_changes_the_set() {
        let ip: Ipv4Addr = "80.4.4.200".parse().unwrap();
        assert_ne!(
            allowed_fast(Mask::Bep6, 6, 1313, &[0xAA; 20], ip),
            allowed_fast(Mask::Bep6, 6, 1313, &[0xBB; 20], ip)
        );
    }

    #[test]
    fn every_mask_has_a_stable_name() {
        assert_eq!(Mask::Bep6.as_str(), "bep6");
        assert_eq!(Mask::Aria2.as_str(), "aria2");
    }
}
