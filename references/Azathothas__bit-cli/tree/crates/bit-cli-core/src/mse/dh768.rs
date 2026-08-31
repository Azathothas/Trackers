//! The 768 bit Diffie-Hellman exchange MSE runs before anything is encrypted.
//!
//! MSE fixes the group: the prime is the 768 bit MODP group from RFC 2409
//! section 6.1, the generator is 2, and the public key is exactly 96 bytes big
//! endian with leading zeros kept. The private exponent is 160 bits, which is
//! what every deployed client uses.
//!
//! There is no big integer dependency here. The modulus is one fixed 768 bit
//! number, so the arithmetic is twelve `u64` limbs wide and the reduction is
//! Montgomery's, which costs one pass per multiply rather than one pass per
//! bit of the product. One handshake costs two exponentiations, and what one
//! costs is measured by the ignored `exponentiation_cost` test at the bottom
//! of this file. See `TODO/peers.md`, T-163.

const LIMBS: usize = 12;

type U768 = [u64; LIMBS];

/// The public key on the wire, and the shared secret, are both this wide.
pub const KEY_LEN: usize = 96;

/// RFC 2409 section 6.1, the 768 bit MODP group, which BEP "MSE/PE" adopts.
///
/// Least significant limb first, so `PRIME[11]` is the top of the number.
const PRIME: U768 = [
    0x0000_0000_0009_0563,
    0xF44C_42E9_A63A_3621,
    0xE485_B576_625E_7EC6,
    0x4FE1_356D_6D51_C245,
    0x302B_0A6D_F25F_1437,
    0xEF95_19B3_CD3A_431B,
    0x514A_0879_8E34_04DD,
    0x020B_BEA6_3B13_9B22,
    0x2902_4E08_8A67_CC74,
    0xC4C6_628B_80DC_1CD1,
    0xC90F_DAA2_2168_C234,
    0xFFFF_FFFF_FFFF_FFFF,
];

/// `-PRIME^-1 mod 2^64`, the Montgomery constant, by Newton iteration.
///
/// Each round doubles how many low bits of the inverse are correct, and the
/// modulus is odd so the first is correct already: 1, 2, 4, 8, 16, 32, 64 is
/// six rounds, which is why the loop count is six and not a guess.
/// `the_montgomery_constants_are_what_they_claim` checks the result rather
/// than the reasoning.
const N0INV: u64 = {
    let mut inv: u64 = 1;
    let mut i = 0;
    while i < 6 {
        inv = inv.wrapping_mul(2u64.wrapping_sub(PRIME[0].wrapping_mul(inv)));
        i += 1;
    }
    inv.wrapping_neg()
};

/// `2^1536 mod PRIME`, which converts a number into Montgomery form.
///
/// Computed rather than pasted in. `2^768 mod PRIME` is `2^768 - PRIME`
/// because the prime's top bit is set, so the wrapping subtraction below is
/// exact; 768 modular doublings then square it.
const R2: U768 = {
    // 2^768 mod PRIME, as a wrapping 0 - PRIME.
    let mut acc = sub(&[0u64; LIMBS], &PRIME).0;
    let mut i = 0;
    while i < 768 {
        acc = double_mod(&acc);
        i += 1;
    }
    acc
};

const fn ge(a: &U768, b: &U768) -> bool {
    let mut i = LIMBS;
    while i > 0 {
        i -= 1;
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

/// `a - b`, and whether it borrowed.
const fn sub(a: &U768, b: &U768) -> (U768, bool) {
    let mut out = [0u64; LIMBS];
    let mut borrow = false;
    let mut i = 0;
    while i < LIMBS {
        let (lo, b1) = a[i].overflowing_sub(b[i]);
        let (lo, b2) = lo.overflowing_sub(borrow as u64);
        out[i] = lo;
        borrow = b1 || b2;
        i += 1;
    }
    (out, borrow)
}

/// `2a mod PRIME`, for a value already below the prime.
///
/// The doubling can carry out of the top limb, and that carry is worth more
/// than the prime, so a subtraction is owed whether or not the low 768 bits
/// compare high. That is the case a plain `if ge(...)` gets wrong.
const fn double_mod(a: &U768) -> U768 {
    let mut out = [0u64; LIMBS];
    let mut carry = 0u64;
    let mut i = 0;
    while i < LIMBS {
        out[i] = (a[i] << 1) | carry;
        carry = a[i] >> 63;
        i += 1;
    }
    if carry != 0 || ge(&out, &PRIME) {
        out = sub(&out, &PRIME).0;
    }
    out
}

/// Montgomery product: `a * b * 2^-768 mod PRIME`, by CIOS.
///
/// Both inputs are in Montgomery form and so is the result. `t` is two limbs
/// wider than the modulus, which is what the algorithm requires: the inner
/// loop can carry one limb past the top and the reduction step can carry one
/// more.
fn mont_mul(a: &U768, b: &U768) -> U768 {
    let mut t = [0u64; LIMBS + 2];

    for &ai in a.iter() {
        let mut carry: u128 = 0;
        for j in 0..LIMBS {
            let v = t[j] as u128 + ai as u128 * b[j] as u128 + carry;
            t[j] = v as u64;
            carry = v >> 64;
        }
        let v = t[LIMBS] as u128 + carry;
        t[LIMBS] = v as u64;
        t[LIMBS + 1] = (v >> 64) as u64;

        let m = t[0].wrapping_mul(N0INV);
        let v = t[0] as u128 + m as u128 * PRIME[0] as u128;
        let mut carry = v >> 64;
        for j in 1..LIMBS {
            let v = t[j] as u128 + m as u128 * PRIME[j] as u128 + carry;
            t[j - 1] = v as u64;
            carry = v >> 64;
        }
        let v = t[LIMBS] as u128 + carry;
        t[LIMBS - 1] = v as u64;
        t[LIMBS] = t[LIMBS + 1] + (v >> 64) as u64;
        t[LIMBS + 1] = 0;
    }

    let mut out = [0u64; LIMBS];
    out.copy_from_slice(&t[..LIMBS]);
    if t[LIMBS] != 0 || ge(&out, &PRIME) {
        out = sub(&out, &PRIME).0;
    }
    out
}

/// `base^exponent mod PRIME`, square and multiply over a 160 bit exponent.
///
/// The loop runs for every bit of the exponent whether or not it is set, so
/// the time this takes does not describe the private key.
fn powm(base: &U768, exponent: &[u8; 20]) -> U768 {
    let base = mont_mul(base, &R2);
    // 1 in Montgomery form is 2^768 mod PRIME, which is the same wrapping
    // subtraction R2's comment describes.
    let mut acc = sub(&[0u64; LIMBS], &PRIME).0;
    for byte in exponent {
        for bit in (0..8).rev() {
            acc = mont_mul(&acc, &acc);
            let product = mont_mul(&acc, &base);
            let set = (byte >> bit) & 1;
            // Both products are computed either way, and one is chosen without
            // a branch, so the sequence of operations does not depend on the
            // key. `mont_mul` itself has one data-dependent subtraction left,
            // which is the standard residual for CIOS.
            let mask = (set as u64).wrapping_neg();
            for i in 0..LIMBS {
                acc[i] = (acc[i] & !mask) | (product[i] & mask);
            }
        }
    }
    let one = {
        let mut v = [0u64; LIMBS];
        v[0] = 1;
        v
    };
    mont_mul(&acc, &one)
}

fn from_be_bytes(bytes: &[u8; KEY_LEN]) -> U768 {
    let mut limbs = [0u64; LIMBS];
    for (i, limb) in limbs.iter_mut().enumerate() {
        let start = KEY_LEN - 8 * (i + 1);
        let mut be = [0u8; 8];
        be.copy_from_slice(&bytes[start..start + 8]);
        *limb = u64::from_be_bytes(be);
    }
    limbs
}

fn to_be_bytes(limbs: &U768) -> [u8; KEY_LEN] {
    let mut bytes = [0u8; KEY_LEN];
    for (i, limb) in limbs.iter().enumerate() {
        let start = KEY_LEN - 8 * (i + 1);
        bytes[start..start + 8].copy_from_slice(&limb.to_be_bytes());
    }
    bytes
}

/// One end of the exchange: a private exponent and the public key for it.
pub struct KeyPair {
    private: [u8; 20],
    public: U768,
}

impl std::fmt::Debug for KeyPair {
    /// Never the private exponent, and never the public key either: at trace
    /// level a peer's public key plus ours is enough to recover the stream.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyPair")
    }
}

impl KeyPair {
    /// A fresh key pair. One per connection, which is what MSE requires.
    pub fn generate() -> Self {
        let mut private = [0u8; 20];
        // A zero exponent would make the public key 1 and the shared secret 1,
        // which is the one draw that has to be rejected rather than used.
        while private.iter().all(|b| *b == 0) {
            rand::fill(&mut private);
        }
        Self::from_private(private)
    }

    fn from_private(private: [u8; 20]) -> Self {
        let two = {
            let mut v = [0u64; LIMBS];
            v[0] = 2;
            v
        };
        let public = powm(&two, &private);
        Self { private, public }
    }

    /// The 96 bytes this end sends first, big endian with leading zeros kept.
    pub fn public_key(&self) -> [u8; KEY_LEN] {
        to_be_bytes(&self.public)
    }

    /// `remote^private mod PRIME`, or `None` when the peer's key is one this
    /// end must not use.
    ///
    /// 0, 1 and `PRIME - 1` generate a subgroup small enough to make the
    /// shared secret guessable, and `PRIME` and above are not group members at
    /// all. A peer that sends one of them is either broken or trying, and
    /// either way the connection ends rather than continuing with a secret
    /// somebody else can compute.
    pub fn shared_secret(&self, remote: &[u8; KEY_LEN]) -> Option<[u8; KEY_LEN]> {
        let remote = from_be_bytes(remote);
        let two = {
            let mut v = [0u64; LIMBS];
            v[0] = 2;
            v
        };
        if !ge(&remote, &two) {
            return None;
        }
        let prime_minus_one = sub(&PRIME, &{
            let mut v = [0u64; LIMBS];
            v[0] = 1;
            v
        })
        .0;
        if ge(&remote, &prime_minus_one) {
            return None;
        }
        Some(to_be_bytes(&powm(&remote, &self.private)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode<const N: usize>(text: &str) -> [u8; N] {
        assert_eq!(text.len(), N * 2, "test vector is the wrong length");
        let mut out = [0u8; N];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).expect("test vector is hex");
        }
        out
    }

    /// The vectors are `pow(2, x, P)` and `pow(y, x, P)` from an arbitrary
    /// precision implementation that shares no code with this one, so a wrong
    /// reduction here cannot agree with them by construction.
    #[test]
    fn matches_arbitrary_precision_vectors() {
        let a = KeyPair::from_private(decode::<20>("000102030405060708090a0b0c0d0e0f10111213"));
        let b = KeyPair::from_private(decode::<20>("f0e0d0c0b0a09080706050403020100011223344"));
        let public_a = decode::<96>(concat!(
            "7fba71c678158bd55ef1cc04a919d1b05f79f9da403c67e82bb1a99a7b4bc4ec",
            "221cca6c3a78171a40f2cc12e3d9d4454338f7e4b9b33de5e82ab04e86f5cd43",
            "aaf9dad923988501c371d3159935de5499e5d726e740b1eabbf4a3dd03c68071",
        ));
        let public_b = decode::<96>(concat!(
            "f9fe7e1c27aee331ab8ff8a6183cfcc7bd08dc593fc4d52bc9a2694b7b787daa",
            "12e3b2695e3e9febf994447cefa427f9f5da34a4d3cd6c231a8d6517e7130de0",
            "0a8a09e753ca12648ec18da389e68eeb66f8308b19cc60dfeaadb2540a821f53",
        ));
        let shared = decode::<96>(concat!(
            "909ea4557d5b9f43dafdc5b598850045b8689e4d652af58a63730b00c574bbe4",
            "962ab9c78b2f295e3ddb3b456f20a4c65761751bf5d79ec4dba8470fe66ed22b",
            "4a25f13528a9575607c77586785a36d560f8556b66e9c16deb87fed185ee07a7",
        ));

        assert_eq!(a.public_key(), public_a);
        assert_eq!(b.public_key(), public_b);
        assert_eq!(a.shared_secret(&public_b), Some(shared));
        assert_eq!(b.shared_secret(&public_a), Some(shared));
    }

    /// An exponent of 1 is the one case where the public key is the generator
    /// itself, so it says the leading zeros are kept rather than trimmed.
    #[test]
    fn the_public_key_keeps_its_leading_zeros() {
        let mut private = [0u8; 20];
        private[19] = 1;
        let mut expected = [0u8; KEY_LEN];
        expected[KEY_LEN - 1] = 2;
        assert_eq!(KeyPair::from_private(private).public_key(), expected);
    }

    /// The largest 160 bit exponent, which is the one most likely to expose a
    /// missing final reduction.
    #[test]
    fn the_largest_exponent_agrees_too() {
        let pair = KeyPair::from_private([0xffu8; 20]);
        assert_eq!(
            pair.public_key(),
            decode::<96>(concat!(
                "fe9aac142f64ad4d5bca7e30bea17d62c709de9e4c5694e2b27e8c6ed82510bb",
                "57952fcce38f6a5084b9699ec34b033ed1a0dbd51d70274bff40ae493f0ce2cd",
                "bd3cbd6055c9677a7eb70d29f4d8ecfbc2625316a7ae1a0d3d6605f0406bad79",
            ))
        );
    }

    #[test]
    fn degenerate_remote_keys_are_refused() {
        let pair = KeyPair::from_private([1u8; 20]);
        let mut one = [0u8; KEY_LEN];
        one[KEY_LEN - 1] = 1;
        assert_eq!(pair.shared_secret(&[0u8; KEY_LEN]), None);
        assert_eq!(pair.shared_secret(&one), None);
        assert_eq!(pair.shared_secret(&[0xffu8; KEY_LEN]), None);
        assert_eq!(pair.shared_secret(&to_be_bytes(&PRIME)), None);
        assert_eq!(
            pair.shared_secret(&to_be_bytes(
                &sub(&PRIME, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).0
            )),
            None
        );
    }

    /// Two freshly generated pairs agree, which is the only property the
    /// handshake actually depends on.
    #[test]
    fn two_generated_pairs_agree() {
        let a = KeyPair::generate();
        let b = KeyPair::generate();
        let from_a = a.shared_secret(&b.public_key());
        let from_b = b.shared_secret(&a.public_key());
        assert!(from_a.is_some());
        assert_eq!(from_a, from_b);
    }

    /// What one exponentiation costs, which is half of what one MSE handshake
    /// adds to a peer connection. Ignored because it measures rather than
    /// asserts: a threshold here would fail on a loaded runner and say nothing
    /// about the code.
    ///
    /// ```text
    /// cargo test -p bit-cli-core --release mse::dh768 -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "measures, does not assert"]
    fn exponentiation_cost() {
        let rounds = 200;
        let pair = KeyPair::from_private([0xa5u8; 20]);
        let remote = pair.public_key();
        let start = std::time::Instant::now();
        for _ in 0..rounds {
            std::hint::black_box(pair.shared_secret(std::hint::black_box(&remote)));
        }
        let each = start.elapsed() / rounds;
        println!("one 768 bit exponentiation: {:?}", each);
    }

    /// `R2` and `N0INV` are computed at compile time, so a test that checks
    /// them is checking the const evaluator rather than a pasted constant.
    #[test]
    fn the_montgomery_constants_are_what_they_claim() {
        assert_eq!(PRIME[0].wrapping_mul(N0INV), u64::MAX);
        // R2 is 2^1536 mod PRIME, from the same arbitrary precision source.
        assert_eq!(
            to_be_bytes(&R2),
            decode::<96>(concat!(
                "a6130c9d54da3d538d001cb98c6f28c7a95d0ecbc5679266afbc083554e85ee3",
                "3361bff33a35e04b112ce56282e51749d515b23cd4ace6dd3423ecee2ddc6158",
                "d4c85168a99a751f6d9f175993af6d83ff0ca2a3d838c98595f0194046281a8e",
            ))
        );
    }
}
