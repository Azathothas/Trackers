//! RC4, which is the only cipher MSE actually deploys.
//!
//! MSE names two crypto methods, plaintext and RC4, and every client that
//! implements the specification implements RC4. The key is a SHA-1 digest and
//! the first 1,024 bytes of keystream are discarded, which is what
//! [`Rc4::new_mse`] does and what makes this an MSE cipher rather than a
//! general one.
//!
//! RC4 is broken for confidentiality against a serious attacker and nothing
//! here pretends otherwise. What MSE buys is that a middlebox cannot classify
//! the stream by reading `BitTorrent protocol` off the front of it, and that a
//! peer which refuses plaintext will talk to us at all. See `TODO/peers.md`,
//! T-163.

/// The MSE discard: the first 1,024 keystream bytes are thrown away, because
/// the start of an RC4 keystream leaks key material.
const DISCARD: usize = 1024;

pub struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl std::fmt::Debug for Rc4 {
    /// The state is the key. It never goes to a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Rc4")
    }
}

impl Rc4 {
    /// A cipher with no bytes discarded. Only the RFC 6229 vectors use this.
    fn new(key: &[u8]) -> Self {
        assert!(!key.is_empty() && key.len() <= 256, "rc4 key length");
        let mut s = [0u8; 256];
        for (i, v) in s.iter_mut().enumerate() {
            *v = i as u8;
        }
        let mut j = 0u8;
        for i in 0..256 {
            j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
            s.swap(i, j as usize);
        }
        Self { s, i: 0, j: 0 }
    }

    /// An MSE cipher: RC4 with the first [`DISCARD`] keystream bytes dropped.
    pub fn new_mse(key: &[u8; 20]) -> Self {
        let mut rc4 = Self::new(key);
        let mut discard = [0u8; DISCARD];
        rc4.apply(&mut discard);
        rc4
    }

    /// XOR `buf` with the next `buf.len()` bytes of keystream, in place.
    ///
    /// Encryption and decryption are the same operation. The cipher advances
    /// by exactly `buf.len()`, so a caller may split a message into chunks of
    /// any size and get the same bytes.
    pub fn apply(&mut self, buf: &mut [u8]) {
        for byte in buf.iter_mut() {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[self.i as usize]);
            self.s.swap(self.i as usize, self.j as usize);
            let k = self.s[self.i as usize].wrapping_add(self.s[self.j as usize]);
            *byte ^= self.s[k as usize];
        }
    }

    /// The next `N` keystream bytes, which is `apply` over zeros.
    ///
    /// The verification constant is eight zero bytes encrypted with the
    /// sender's key, and the receiver has to be able to produce the same eight
    /// bytes to find it in the padding.
    pub fn keystream<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        self.apply(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(text: &str) -> Vec<u8> {
        assert_eq!(text.len() % 2, 0);
        (0..text.len() / 2)
            .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).expect("hex"))
            .collect()
    }

    /// RFC 6229 section 2, three keys, at stream offsets 0, 16 and 240.
    ///
    /// These are the published answer for a cipher that has one, and nothing
    /// in this file was written from the implementation's own output. Offset
    /// 240 is in the table because the first draft's vectors were the offset
    /// 16 line mislabelled, and only a second offset catches that.
    #[test]
    fn matches_rfc_6229() {
        let cases: &[(&str, &str, &str, &str)] = &[
            (
                "0102030405",
                "b2396305f03dc027ccc3524a0a1118a8",
                "6982944f18fc82d589c403a47a0d0919",
                "28cb1132c96ce286421dcaadb8b69eae",
            ),
            (
                "0102030405060708",
                "97ab8a1bf0afb96132f2f67258da15a8",
                "8263efdb45c4a18684ef87e6b19e5b09",
                "9636ebc9841926f4f7d1f362bddf6e18",
            ),
            (
                "0102030405060708090a0b0c0d0e0f10",
                "9ac7cc9a609d1ef7b2932899cde41b97",
                "5248c4959014126a6e8a84f11d1a9e1c",
                "065902e4b620f6cc36c8589f66432f2b",
            ),
        ];
        for (key, at_0, at_16, at_240) in cases {
            let mut rc4 = Rc4::new(&decode(key));
            let mut buf = [0u8; 256];
            rc4.apply(&mut buf);
            assert_eq!(&buf[..16], decode(at_0).as_slice(), "key {key} at 0");
            assert_eq!(&buf[16..32], decode(at_16).as_slice(), "key {key} at 16");
            assert_eq!(
                &buf[240..256],
                decode(at_240).as_slice(),
                "key {key} at 240"
            );
        }
    }

    /// The discard is 1,024 bytes and not 768 or 1,536, and this says which by
    /// lining a discarded cipher up against an undiscarded one.
    #[test]
    fn the_mse_discard_is_exactly_1024_bytes() {
        let key = [7u8; 20];
        let mut plain = Rc4::new(&key);
        let mut skipped = [0u8; DISCARD];
        plain.apply(&mut skipped);
        let expected: [u8; 32] = plain.keystream();

        let mut mse = Rc4::new_mse(&key);
        assert_eq!(mse.keystream::<32>(), expected);
    }

    /// A stream cipher applied in pieces has to give the same answer as one
    /// applied whole, because the wire hands it arbitrary read sizes.
    #[test]
    fn chunking_does_not_change_the_bytes() {
        let key = [0x2au8; 20];
        let plaintext: Vec<u8> = (0..300u32).map(|i| (i * 7 % 251) as u8).collect();

        let whole = {
            let mut buf = plaintext.clone();
            Rc4::new_mse(&key).apply(&mut buf);
            buf
        };

        for chunk in [1usize, 3, 16, 64, 299, 300] {
            let mut cipher = Rc4::new_mse(&key);
            let mut buf = plaintext.clone();
            for piece in buf.chunks_mut(chunk) {
                cipher.apply(piece);
            }
            assert_eq!(buf, whole, "chunk size {chunk}");
        }
    }

    #[test]
    fn encryption_and_decryption_are_the_same_operation() {
        let key = [0x11u8; 20];
        let plaintext = b"BitTorrent protocol and then some payload".to_vec();
        let mut buf = plaintext.clone();
        Rc4::new_mse(&key).apply(&mut buf);
        assert_ne!(buf, plaintext);
        Rc4::new_mse(&key).apply(&mut buf);
        assert_eq!(buf, plaintext);
    }
}
