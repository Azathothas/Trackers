//! The identity `bit-cli` presents to a tracker and to a peer.
//!
//! **One place, because it was six.** Until T-236 this process could announce
//! or hand shake under any of six different peer ids, and five of them claimed
//! a two character code belonging to a client that is not this one:
//!
//! | where | prefix | what it says |
//! | --- | --- | --- |
//! | the session, when `SessionOptions::peer_id` was left `None` | `-rQ9010-` | `librqbit` 9.0.1 |
//! | `bit-cli trackers` | `-BC0100-` | BitComet 1.0.0 |
//! | `bit-cli bench probe` | `-BC0100-` | BitComet 1.0.0 |
//! | the web seed bridge | `-BCws01-` | BitComet |
//! | the swarm bench's synthetic peer | `-BCsw01-` | BitComet |
//! | the listener health check | `-BClc01-` | BitComet |
//!
//! Only the first three reach a tracker or a remote peer. The other three are
//! loopback inside this process, and they are here anyway: an identity that is
//! wrong in a log is still wrong, and the point of one module is that a
//! seventh call site cannot invent a seventh identity.
//!
//! `bit-cli`'s own code is [`CLIENT_CODE`], and why that one rather than
//! another is in `TODO/peers.md`, T-236.

/// The two character client code, BEP 20 Azureus style.
///
/// **Not free to choose.** It was checked against six registries before it was
/// used: libtorrent `v2.0.11` `src/identify_client.cpp:148-250`, which is the
/// closest thing this ecosystem has to one and carries 92 codes, and the five
/// independent implementations of the same table in the research corpus. `CL`
/// appears in none of them, and neither does its lower case form, which
/// matters because the lookup is a byte comparison: `lt` is rTorrent and `LT`
/// is libtorrent, and they have coexisted for two decades.
///
/// It reads as the command line, which is the one thing that distinguishes
/// this client from every entry in that table.
pub const CLIENT_CODE: [u8; 2] = *b"CL";

/// One version component to one character: `0` to `9`, then `A` to `Z`, then
/// `a` to `z`.
///
/// libtorrent, libtorrent-rakshasa and Transmission all encode a version this
/// way, and Transmission calls the same table `BASE62`. Encoding one component
/// per character is what keeps the prefix eight bytes when a component reaches
/// ten, which is the whole reason the alphabet is not just the digits.
const ALPHABET: [u8; 62] = *b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// A decimal `CARGO_PKG_VERSION_*` component, at compile time.
const fn decimal(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let mut value: u32 = 0;
    let mut index = 0;
    while index < bytes.len() {
        let digit = bytes[index];
        assert!(
            digit >= b'0' && digit <= b'9',
            "a version component is not decimal"
        );
        value = value * 10 + (digit - b'0') as u32;
        index += 1;
    }
    value
}

/// One component as one character, refusing what cannot be encoded.
///
/// A version component of 62 or more has no single character encoding, and the
/// two ways out of that are both worse than failing the build: a wider prefix
/// is not Azureus style any more, and a wrapped one announces a version this
/// is not. So it is a compile error, raised in the release that would have
/// needed it rather than in a tracker's statistics afterwards.
const fn version_char(component: u32) -> u8 {
    assert!(
        component < 62,
        "a version component past 61 has no single-character peer id encoding"
    );
    ALPHABET[component as usize]
}

/// The eight byte prefix a tracker and a remote peer see.
///
/// `-CL`, then three characters of `bit-cli`'s own version, then the build
/// slot, then `-`. The version is the crate's, so it moves when this crate
/// does and never when the vendored `librqbit` does. That was the other half
/// of T-236: leaving `SessionOptions::peer_id` unset announced `librqbit`'s
/// version, so bumping the vendored tree changed what a tracker was told about
/// `bit-cli`.
///
/// The build slot is `0`, and the assertion below is what keeps that honest.
/// Transmission puts `B` there for a beta and `Z` for a development build, so
/// a prerelease that shipped a `0` would be claiming to be a release. If this
/// crate ever carries a prerelease component the build stops here and somebody
/// decides what character it deserves.
pub const PREFIX: [u8; 8] = {
    assert!(
        env!("CARGO_PKG_VERSION_PRE").is_empty(),
        "this crate carries a prerelease version and the peer id build slot is still `0`"
    );
    [
        b'-',
        CLIENT_CODE[0],
        CLIENT_CODE[1],
        version_char(decimal(env!("CARGO_PKG_VERSION_MAJOR"))),
        version_char(decimal(env!("CARGO_PKG_VERSION_MINOR"))),
        version_char(decimal(env!("CARGO_PKG_VERSION_PATCH"))),
        b'0',
        b'-',
    ]
};

/// A prefix for one of this process's own loopback roles.
///
/// The bridge, the swarm bench's synthetic peer and the listener health check
/// each open a connection to a session running in the same process. Each one
/// **has to differ from the session's own id**, because a session that sees
/// its own peer id in a handshake drops the connection as a self-connect.
///
/// So the four version characters carry the role rather than a version: `ws`
/// for the web seed bridge, `sw` for the swarm bench, `lc` for the listener
/// check, then two digits that are the role's own generation. A tracker never
/// sees any of these, because none of these connections announces.
pub const fn role(tag: [u8; 2], generation: [u8; 2]) -> [u8; 8] {
    [
        b'-',
        CLIENT_CODE[0],
        CLIENT_CODE[1],
        tag[0],
        tag[1],
        generation[0],
        generation[1],
        b'-',
    ]
}

/// The printable suffix alphabet: lower case letters and digits.
///
/// A peer id is twenty arbitrary bytes and nothing requires this. It is
/// printable because the alternative is what `librqbit`'s generator does,
/// which is twelve raw bytes, and the first thing anyone reading an announce
/// log then does is percent-escape them. Every widely deployed client picks a
/// printable suffix for the same reason.
const SUFFIX: [u8; 36] = *b"0123456789abcdefghijklmnopqrstuvwxyz";

/// A full twenty byte peer id under `prefix`.
///
/// Twelve random characters after the prefix, from the operating system's
/// generator rather than from the clock. Two of the six call sites this
/// replaces seeded themselves from `SystemTime::now()`, and one of those
/// derived every one of its twelve characters from the same nanosecond
/// reading, so two runs starting in the same nanosecond produced the same id.
pub fn generate(prefix: &[u8; 8]) -> [u8; 20] {
    use rand::RngExt;

    let mut id = [0u8; 20];
    id[..8].copy_from_slice(prefix);
    let mut rng = rand::rng();
    for slot in id[8..].iter_mut() {
        *slot = SUFFIX[rng.random_range(0..SUFFIX.len())];
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape BEP 20 describes, asserted on the constant rather than on a
    /// generated id, because the constant is what a tracker files the announce
    /// under.
    #[test]
    fn the_prefix_is_azureus_style_and_carries_this_crates_version() {
        assert_eq!(PREFIX.len(), 8);
        assert_eq!(PREFIX[0], b'-');
        assert_eq!(PREFIX[7], b'-');
        assert_eq!(&PREFIX[1..3], b"CL");
        assert_eq!(
            PREFIX,
            *b"-CL0200-",
            "the crate is {} and the prefix is {}",
            env!("CARGO_PKG_VERSION"),
            String::from_utf8_lossy(&PREFIX)
        );
        for character in PREFIX[3..7].iter() {
            assert!(
                ALPHABET.contains(character),
                "{} is not in the version alphabet",
                *character as char
            );
        }
    }

    /// The code belongs to nobody else. This is the assertion that would fail
    /// if somebody changed `CLIENT_CODE` to something the registries carry,
    /// which is the mistake T-236 exists to undo rather than repeat.
    #[test]
    fn the_client_code_is_not_one_a_registry_already_names() {
        // libtorrent v2.0.11 `src/identify_client.cpp:148-250`, all 92
        // Azureus-style codes in its table. Copied rather than fetched,
        // because a test that needs the network is a test that fails when the
        // network is down.
        const TAKEN: &[&[u8; 2]] = &[
            b"7T", b"AB", b"AG", b"AR", b"AT", b"AV", b"AX", b"AZ", b"A~", b"BB", b"BC", b"BE",
            b"BF", b"BG", b"BI", b"BL", b"BP", b"BR", b"BS", b"BT", b"BU", b"BW", b"BX", b"CD",
            b"CT", b"DE", b"DP", b"EB", b"ES", b"FC", b"FT", b"FW", b"FX", b"GS", b"HK", b"HL",
            b"HN", b"IL", b"KC", b"KG", b"KT", b"LC", b"LH", b"LK", b"LP", b"LR", b"LT", b"LW",
            b"ML", b"MO", b"MP", b"MR", b"MT", b"NX", b"OS", b"OT", b"PD", b"QD", b"QT", b"RT",
            b"RZ", b"SB", b"SD", b"SK", b"SN", b"SS", b"ST", b"SZ", b"S~", b"TB", b"TL", b"TN",
            b"TR", b"TS", b"TT", b"UL", b"UM", b"UT", b"VG", b"WT", b"WY", b"XF", b"XL", b"XS",
            b"XT", b"XX", b"ZO", b"ZT", b"lt", b"pX", b"qB", b"st",
            // Not in libtorrent's table and in the corpus's four: rQ is the
            // vendored tree's own, and this must not answer to it either.
            b"rQ", b"UE", b"WD", b"WW", b"UW", b"sc", b"SC", b"MK", b"PT", b"NB", b"JS", b"JT",
            b"HM", b"GD", b"FD", b"TE", b"SM", b"SP", b"PB",
        ];
        for taken in TAKEN {
            assert_ne!(
                &CLIENT_CODE,
                *taken,
                "{} is already a registered client code",
                String::from_utf8_lossy(*taken)
            );
        }
        // And the lower case form, because the lookup is a byte comparison and
        // a human reading a statistics page is not.
        let lower = [
            CLIENT_CODE[0].to_ascii_lowercase(),
            CLIENT_CODE[1].to_ascii_lowercase(),
        ];
        let upper = [
            CLIENT_CODE[0].to_ascii_uppercase(),
            CLIENT_CODE[1].to_ascii_uppercase(),
        ];
        for taken in TAKEN {
            assert_ne!(&lower, *taken, "the lower case form is registered");
            assert_ne!(&upper, *taken, "the upper case form is registered");
        }
    }

    /// A role prefix is the same eight byte shape and is never the announcing
    /// one, because a session that meets its own id hangs up.
    #[test]
    fn a_role_prefix_is_never_the_prefix_that_announces() {
        for tag in [*b"ws", *b"sw", *b"lc"] {
            let prefix = role(tag, *b"01");
            assert_eq!(prefix.len(), 8);
            assert_eq!(prefix[0], b'-');
            assert_eq!(prefix[7], b'-');
            assert_eq!(&prefix[1..3], &CLIENT_CODE);
            assert_ne!(prefix, PREFIX, "a role would self-connect");
        }
        // And distinct from each other, so a log naming one names one.
        let all = [
            role(*b"ws", *b"01"),
            role(*b"sw", *b"01"),
            role(*b"lc", *b"01"),
        ];
        for (index, one) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(one, other);
            }
        }
    }

    /// Twenty bytes, the prefix intact, and a printable suffix.
    #[test]
    fn a_generated_id_is_the_prefix_and_twelve_printable_characters() {
        let id = generate(&PREFIX);
        assert_eq!(id.len(), 20);
        assert_eq!(&id[..8], &PREFIX);
        for byte in &id[8..] {
            assert!(
                SUFFIX.contains(byte),
                "{} is not in the suffix alphabet",
                *byte as char
            );
        }
    }

    /// Two ids differ. Two of the six generators this replaces derived every
    /// character from one `SystemTime::now()` reading, so two runs starting
    /// close enough together produced the same suffix.
    #[test]
    fn two_ids_generated_back_to_back_differ() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..64 {
            assert!(seen.insert(generate(&PREFIX)), "a peer id repeated");
        }
    }
}
