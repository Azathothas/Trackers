//! The MSE handshake, both directions.
//!
//! The shape, with `A` the end that dialled and `B` the end that accepted:
//!
//! ```text
//! A -> B  Ya (96) PadA (0..512)
//! B -> A  Yb (96) PadB (0..512)
//! A -> B  HASH('req1', S) (20)
//!         HASH('req2', SKEY) xor HASH('req3', S) (20)
//!         ENCRYPT(VC (8) crypto_provide (4) len(PadC) (2) PadC len(IA) (2))
//!         ENCRYPT(IA)
//! B -> A  ENCRYPT(VC (8) crypto_select (4) len(PadD) (2) PadD)
//! ```
//!
//! Everything after that is RC4 in both directions, with the keys derived from
//! the shared secret and the info hash.
//!
//! Two things about it are worth knowing before reading the code.
//!
//! **Neither side frames the padding**, so both sides find the next field by
//! searching for a known 20 or 8 byte string in the stream. That is why
//! [`Buffered`] exists: a search reads past what it needs, and the bytes it
//! read past belong to the payload.
//!
//! **The responder does not know which torrent the connection is for.** The
//! info hash is the key to `HASH('req2', SKEY)`, so it is recovered by trying
//! every info hash this session holds. That is why [`respond`] takes a list.
//!
//! See `TODO/peers.md`, T-163.

use sha1::{Digest, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::dh768::{KEY_LEN, KeyPair};
use super::rc4::Rc4;

/// The verification constant: eight zero bytes, encrypted, which is how each
/// end finds the end of the other's padding.
const VC: [u8; 8] = [0u8; 8];

/// Neither padding field may exceed this, and a peer that says otherwise is
/// refused rather than believed.
const MAX_PAD: usize = 512;

/// `crypto_provide` and `crypto_select` bit 1.
const CRYPTO_PLAINTEXT: u32 = 0x01;
/// `crypto_provide` and `crypto_select` bit 2, and the only one used here.
const CRYPTO_RC4: u32 = 0x02;

/// The plaintext BitTorrent handshake starts with these 20 bytes, which is how
/// an accepting end tells a plaintext peer from an encrypted one without a
/// second port or a flag.
pub const BT_PROTOCOL_HEADER: &[u8; 20] = b"\x13BitTorrent protocol";

/// What a connection settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Negotiated {
    /// No MSE at all: the peer spoke the plaintext protocol.
    Plaintext,
    /// The MSE handshake completed and both directions are RC4.
    Rc4,
}

impl Negotiated {
    /// The word that goes in `--json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Negotiated::Plaintext => "plaintext",
            Negotiated::Rc4 => "rc4",
        }
    }
}

/// A completed handshake: the read half back, the two ciphers, and the bytes
/// read past the end of the handshake that belong to the payload.
pub struct Established<R> {
    /// The read half, handed back because the handshake borrowed it into a
    /// buffered reader and the caller needs it to build the stream.
    pub reader: R,
    /// Encrypts what this end sends.
    pub encrypt: Rc4,
    /// Decrypts what this end receives.
    pub decrypt: Rc4,
    /// Payload bytes already read and already decrypted. The reader must serve
    /// these before it touches the socket again.
    pub leftover: Vec<u8>,
    /// Which info hash the peer asked for. Meaningful for the accepting end,
    /// which does not otherwise know.
    pub info_hash: [u8; 20],
}

impl<R> std::fmt::Debug for Established<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Established")
            .field("leftover", &self.leftover.len())
            .finish()
    }
}

/// What an accepting end found on the wire.
pub enum Accepted<R> {
    /// An MSE peer, and the handshake completed.
    Encrypted(Box<Established<R>>),
    /// A plaintext peer. The bytes already read off the socket come back with
    /// it, because the BitTorrent handshake starts with them.
    Plaintext { reader: R, prefix: Vec<u8> },
}

fn sha1_of(parts: &[&[u8]]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn xor20(a: [u8; 20], b: [u8; 20]) -> [u8; 20] {
    let mut out = [0u8; 20];
    for i in 0..20 {
        out[i] = a[i] ^ b[i];
    }
    out
}

/// The two ciphers for one end.
///
/// `keyA` encrypts what the dialling end sends and `keyB` what the accepting
/// end sends, so the two ends derive the same pair and use it in opposite
/// directions.
fn ciphers(secret: &[u8; KEY_LEN], info_hash: &[u8; 20], initiator: bool) -> (Rc4, Rc4) {
    let key_a = sha1_of(&[b"keyA", secret, info_hash]);
    let key_b = sha1_of(&[b"keyB", secret, info_hash]);
    if initiator {
        (Rc4::new_mse(&key_a), Rc4::new_mse(&key_b))
    } else {
        (Rc4::new_mse(&key_b), Rc4::new_mse(&key_a))
    }
}

/// A read side with a pushback buffer.
///
/// The handshake has to search for byte strings it cannot frame, so it reads
/// more than it consumes. Everything unconsumed at the end is the payload's
/// first bytes and is handed back rather than dropped, which is the bug this
/// type exists to make impossible.
struct Buffered<R> {
    reader: R,
    buf: Vec<u8>,
    pos: usize,
}

impl<R: AsyncRead + Unpin> Buffered<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buf: Vec::new(),
            pos: 0,
        }
    }

    fn buffered(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    /// Read at least one more byte, or fail. Never grows without bound: the
    /// caller's limit is what stops it, and every caller has one.
    async fn fill(&mut self) -> std::io::Result<()> {
        let mut chunk = [0u8; 512];
        let n = self.reader.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "peer closed during the encryption handshake",
            ));
        }
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(())
    }

    async fn take(&mut self, n: usize) -> std::io::Result<Vec<u8>> {
        while self.buffered().len() < n {
            self.fill().await?;
        }
        let out = self.buf[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(out)
    }

    async fn take_array<const N: usize>(&mut self) -> std::io::Result<[u8; N]> {
        let v = self.take(N).await?;
        let mut out = [0u8; N];
        out.copy_from_slice(&v);
        Ok(out)
    }

    /// Consume bytes until `pattern` has been read and consumed.
    ///
    /// `max_skip` is how much padding may precede it. A peer that never sends
    /// the pattern costs `max_skip + pattern.len()` bytes and then the
    /// connection, which is the bound that stops an accepting end from reading
    /// forever on a socket that is not speaking MSE.
    async fn sync_to(&mut self, pattern: &[u8], max_skip: usize) -> std::io::Result<()> {
        let limit = max_skip + pattern.len();
        loop {
            let window = self.buffered();
            if window.len() >= pattern.len() {
                let last = window.len() - pattern.len();
                if let Some(at) = (0..=last).find(|i| &window[*i..*i + pattern.len()] == pattern) {
                    self.pos += at + pattern.len();
                    return Ok(());
                }
            }
            if self.buffered().len() >= limit {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "encryption handshake marker not found within the padding limit",
                ));
            }
            self.fill().await?;
        }
    }

    /// The reader back, and everything read and not consumed.
    fn into_parts(mut self) -> (R, Vec<u8>) {
        self.buf.drain(..self.pos);
        (self.reader, self.buf)
    }
}

/// Random padding, `0..MAX_PAD` bytes of it.
fn padding() -> Vec<u8> {
    let len = usize::from(rand::random::<u16>()) % MAX_PAD;
    let mut pad = vec![0u8; len];
    rand::fill(pad.as_mut_slice());
    pad
}

/// The dialling end of the handshake.
///
/// `crypto_provide` names RC4 and nothing else. A peer that will not do RC4 is
/// refused here rather than continued in the clear: every client that
/// implements MSE implements RC4, so offering plaintext as well buys no peer
/// and costs a second state that would have to be reported and tested.
pub async fn initiate<R, W>(
    reader: R,
    writer: &mut W,
    info_hash: &[u8; 20],
) -> std::io::Result<Established<R>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let keys = KeyPair::generate();
    let mut buffered = Buffered::new(reader);

    let mut first = Vec::with_capacity(KEY_LEN + MAX_PAD);
    first.extend_from_slice(&keys.public_key());
    first.extend_from_slice(&padding());
    writer.write_all(&first).await?;
    writer.flush().await?;

    let remote: [u8; KEY_LEN] = buffered.take_array().await?;
    let secret = keys.shared_secret(&remote).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "peer sent a Diffie-Hellman key that is not usable",
        )
    })?;

    let (mut encrypt, mut decrypt) = ciphers(&secret, info_hash, true);

    let pad_c = padding();
    let mut plain = Vec::with_capacity(VC.len() + 4 + 2 + pad_c.len() + 2);
    plain.extend_from_slice(&VC);
    plain.extend_from_slice(&CRYPTO_RC4.to_be_bytes());
    plain.extend_from_slice(&(pad_c.len() as u16).to_be_bytes());
    plain.extend_from_slice(&pad_c);
    // len(IA). Nothing is sent inside the handshake: the BitTorrent handshake
    // goes over the encrypted stream a moment later and is the same bytes
    // either way.
    plain.extend_from_slice(&0u16.to_be_bytes());
    encrypt.apply(&mut plain);

    let mut third = Vec::with_capacity(40 + plain.len());
    third.extend_from_slice(&sha1_of(&[b"req1", &secret]));
    third.extend_from_slice(&xor20(
        sha1_of(&[b"req2", info_hash]),
        sha1_of(&[b"req3", &secret]),
    ));
    third.extend_from_slice(&plain);
    writer.write_all(&third).await?;
    writer.flush().await?;

    // The other end's VC is eight zero bytes under its key, which is the first
    // eight bytes of its keystream. Producing it here advances `decrypt` by
    // exactly those eight bytes, which is where the next field starts.
    let expect_vc = decrypt.keystream::<8>();
    buffered.sync_to(&expect_vc, MAX_PAD).await?;

    let mut tail = buffered.take(6).await?;
    decrypt.apply(&mut tail);
    let selected = u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]);
    let pad_d_len = usize::from(u16::from_be_bytes([tail[4], tail[5]]));
    if selected & CRYPTO_RC4 == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("peer selected crypto {selected:#x}, which is not RC4"),
        ));
    }
    if pad_d_len > MAX_PAD {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("peer's padding is {pad_d_len} bytes, over the {MAX_PAD} limit"),
        ));
    }
    let mut pad_d = buffered.take(pad_d_len).await?;
    decrypt.apply(&mut pad_d);

    let (reader, mut leftover) = buffered.into_parts();
    decrypt.apply(&mut leftover);
    Ok(Established {
        reader,
        encrypt,
        decrypt,
        leftover,
        info_hash: *info_hash,
    })
}

/// The accepting end of the handshake.
///
/// Reads the first 20 bytes and decides what the peer is: a plaintext
/// BitTorrent handshake starts with a byte string that no MSE public key can
/// be mistaken for, because the first field of an MSE connection is 96 bytes
/// of key and only 1 in 2^160 of them starts `\x13BitTorrent protocol`.
///
/// `info_hashes` is every torrent this session holds. The peer names one of
/// them, encrypted, and this is the only way to learn which.
///
/// `encrypted_allowed` is `false` for a session with encryption off, and it is
/// checked **before** the Diffie-Hellman exchange rather than after it. That
/// ordering is the whole point of the flag: a responder that completes the
/// exchange and then refuses has told the dialling end that its handshake
/// worked, and a dialling end that believes it will go on offering encryption
/// to this peer instead of falling back. Refusing at the first twenty bytes is
/// what lets the other end's fallback fire. See `TODO/peers.md`, T-163.
pub async fn respond<R, W>(
    reader: R,
    writer: &mut W,
    info_hashes: &[[u8; 20]],
    encrypted_allowed: bool,
) -> std::io::Result<Accepted<R>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffered = Buffered::new(reader);
    let head: [u8; 20] = buffered.take_array().await?;
    if &head == BT_PROTOCOL_HEADER {
        let (reader, rest) = buffered.into_parts();
        let mut prefix = head.to_vec();
        prefix.extend_from_slice(&rest);
        return Ok(Accepted::Plaintext { reader, prefix });
    }

    if !encrypted_allowed {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "peer opened with encryption and this session has it off",
        ));
    }

    let mut remote = [0u8; KEY_LEN];
    remote[..20].copy_from_slice(&head);
    let rest = buffered.take(KEY_LEN - 20).await?;
    remote[20..].copy_from_slice(&rest);

    let keys = KeyPair::generate();
    let secret = keys.shared_secret(&remote).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "peer sent a Diffie-Hellman key that is not usable",
        )
    })?;

    let mut second = Vec::with_capacity(KEY_LEN + MAX_PAD);
    second.extend_from_slice(&keys.public_key());
    second.extend_from_slice(&padding());
    writer.write_all(&second).await?;
    writer.flush().await?;

    buffered
        .sync_to(&sha1_of(&[b"req1", &secret]), MAX_PAD)
        .await?;

    let claimed: [u8; 20] = buffered.take_array().await?;
    let req3 = sha1_of(&[b"req3", &secret]);
    let info_hash = info_hashes
        .iter()
        .find(|h| xor20(sha1_of(&[b"req2", h.as_slice()]), req3) == claimed)
        .copied()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "encrypted peer asked for a torrent this session does not have",
            )
        })?;

    let (mut encrypt, mut decrypt) = ciphers(&secret, &info_hash, false);

    let mut head = buffered.take(VC.len() + 4 + 2).await?;
    decrypt.apply(&mut head);
    if head[..VC.len()] != VC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "peer's verification constant did not decrypt to zero",
        ));
    }
    let provided = u32::from_be_bytes([head[8], head[9], head[10], head[11]]);
    let pad_c_len = usize::from(u16::from_be_bytes([head[12], head[13]]));
    if pad_c_len > MAX_PAD {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("peer's padding is {pad_c_len} bytes, over the {MAX_PAD} limit"),
        ));
    }
    if provided & CRYPTO_RC4 == 0 {
        let what = match provided & CRYPTO_PLAINTEXT {
            0 => "no method this end knows",
            _ => "plaintext only",
        };
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("peer offers {what} ({provided:#x}), and this end only does RC4"),
        ));
    }

    let pad_d = padding();
    let mut fourth = Vec::with_capacity(VC.len() + 4 + 2 + pad_d.len());
    fourth.extend_from_slice(&VC);
    fourth.extend_from_slice(&CRYPTO_RC4.to_be_bytes());
    fourth.extend_from_slice(&(pad_d.len() as u16).to_be_bytes());
    fourth.extend_from_slice(&pad_d);
    encrypt.apply(&mut fourth);
    writer.write_all(&fourth).await?;
    writer.flush().await?;

    let mut pad_c = buffered.take(pad_c_len).await?;
    decrypt.apply(&mut pad_c);
    let mut ia_len = buffered.take(2).await?;
    decrypt.apply(&mut ia_len);
    let ia_len = usize::from(u16::from_be_bytes([ia_len[0], ia_len[1]]));

    // IA is the peer's first payload bytes, sent inside the handshake to save
    // a round trip. Whether they arrive here or a moment later, they are the
    // start of the same stream, so they join the leftover.
    let mut leftover = buffered.take(ia_len).await?;
    let (reader, mut rest) = buffered.into_parts();
    leftover.append(&mut rest);
    decrypt.apply(&mut leftover);

    Ok(Accepted::Encrypted(Box::new(Established {
        reader,
        encrypt,
        decrypt,
        leftover,
        info_hash,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    type Half = tokio::io::ReadHalf<tokio::io::DuplexStream>;

    /// Both ends over one in-memory duplex, which is the whole handshake
    /// without a socket.
    async fn pair(
        info_hash: [u8; 20],
        known: Vec<[u8; 20]>,
    ) -> (
        std::io::Result<Established<Half>>,
        std::io::Result<Accepted<Half>>,
    ) {
        let (a, b) = tokio::io::duplex(4096);
        let (a_read, mut a_write) = tokio::io::split(a);
        let (b_read, mut b_write) = tokio::io::split(b);
        tokio::join!(
            async move { initiate(a_read, &mut a_write, &info_hash).await },
            async move { respond(b_read, &mut b_write, &known, true).await },
        )
    }

    #[tokio::test]
    async fn both_ends_agree_on_the_keys() {
        let info_hash = [0x42u8; 20];
        let (out, inc) = pair(info_hash, vec![[9u8; 20], info_hash]).await;
        let mut out = out.expect("initiator");
        let inc = inc.expect("responder");
        let Accepted::Encrypted(mut inc) = inc else {
            panic!("responder took the plaintext path");
        };
        assert_eq!(inc.info_hash, info_hash);
        assert!(out.leftover.is_empty());
        assert!(inc.leftover.is_empty());

        let mut message = b"\x13BitTorrent protocol and the rest".to_vec();
        let plain = message.clone();
        out.encrypt.apply(&mut message);
        assert_ne!(message, plain);
        inc.decrypt.apply(&mut message);
        assert_eq!(message, plain);

        let mut reply = b"a reply from the accepting end".to_vec();
        let plain = reply.clone();
        inc.encrypt.apply(&mut reply);
        out.decrypt.apply(&mut reply);
        assert_eq!(reply, plain);
    }

    /// The padding is random, so one run proves nothing about the search that
    /// skips it. Twenty runs cover every length class the generator produces.
    #[tokio::test]
    async fn the_padding_search_holds_across_runs() {
        for _ in 0..20 {
            let info_hash = [7u8; 20];
            let (out, inc) = pair(info_hash, vec![info_hash]).await;
            assert!(out.is_ok(), "initiator: {:?}", out.err());
            assert!(matches!(inc, Ok(Accepted::Encrypted(_))));
        }
    }

    #[tokio::test]
    async fn a_torrent_this_session_does_not_have_is_refused() {
        let (out, inc) = pair([1u8; 20], vec![[2u8; 20]]).await;
        let err = inc.err().expect("responder should refuse");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(out.is_err(), "initiator should not complete either");
    }

    /// The one case that must not be treated as encrypted: a plaintext peer.
    #[tokio::test]
    async fn a_plaintext_peer_is_recognised_and_its_bytes_are_kept() {
        let (a, b) = tokio::io::duplex(4096);
        let (_a_read, mut a_write) = tokio::io::split(a);
        let (b_read, mut b_write) = tokio::io::split(b);
        let mut sent = BT_PROTOCOL_HEADER.to_vec();
        sent.extend_from_slice(&[0u8; 8]);
        sent.extend_from_slice(&[0xabu8; 20]);
        sent.extend_from_slice(&[0xcdu8; 20]);
        let expected = sent.clone();
        let (_, accepted) = tokio::join!(
            async move {
                a_write.write_all(&sent).await.unwrap();
                a_write.flush().await.unwrap();
            },
            async move { respond(b_read, &mut b_write, &[[0u8; 20]], true).await },
        );
        match accepted.expect("responder") {
            Accepted::Plaintext { prefix, .. } => {
                assert_eq!(&prefix[..20], BT_PROTOCOL_HEADER.as_slice());
                assert!(
                    expected.starts_with(&prefix),
                    "the prefix must be a prefix of what was sent"
                );
            }
            Accepted::Encrypted(_) => panic!("a plaintext handshake was taken for MSE"),
        }
    }

    /// The bytes the initiator sends straight after the handshake are read
    /// past by the responder's padding search, so they have to come back as
    /// leftover rather than be lost.
    #[tokio::test]
    async fn payload_sent_with_the_handshake_survives_it() {
        let info_hash = [0x5au8; 20];
        let (a, b) = tokio::io::duplex(8192);
        let (a_read, mut a_write) = tokio::io::split(a);
        let (b_read, mut b_write) = tokio::io::split(b);
        let payload = b"\x13BitTorrent protocol0000000000000000".to_vec();
        let expect = payload.clone();
        let (out, inc) = tokio::join!(
            async move {
                let mut est = initiate(a_read, &mut a_write, &info_hash).await?;
                let mut buf = payload.clone();
                est.encrypt.apply(&mut buf);
                a_write.write_all(&buf).await?;
                a_write.flush().await?;
                Ok::<_, std::io::Error>(est)
            },
            async move { respond(b_read, &mut b_write, &[info_hash], true).await },
        );
        out.expect("initiator");
        let Accepted::Encrypted(est) = inc.expect("responder") else {
            panic!("plaintext");
        };
        // The leftover may be empty when the search stopped exactly at the end
        // of the handshake; what must never happen is a partial or wrong one.
        assert!(
            expect.starts_with(&est.leftover),
            "leftover of {} bytes is not the head of the payload",
            est.leftover.len()
        );
    }

    /// A responder with encryption off refuses at the first twenty bytes, and
    /// the ordering is what matters: the dialling end must be told before it
    /// has a shared secret, or its fallback never fires.
    #[tokio::test]
    async fn encryption_off_refuses_before_the_key_exchange() {
        let info_hash = [0x11u8; 20];
        let (a, b) = tokio::io::duplex(4096);
        let (a_read, mut a_write) = tokio::io::split(a);
        let (b_read, mut b_write) = tokio::io::split(b);
        let (out, inc) = tokio::join!(
            async move { initiate(a_read, &mut a_write, &info_hash).await },
            async move { respond(b_read, &mut b_write, &[info_hash], false).await },
        );
        let err = inc.err().expect("the responder should refuse");
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        // Nothing was written back, so the initiator got no public key and
        // fails rather than believing it negotiated anything.
        assert!(out.is_err(), "the initiator must not complete");
    }

    #[test]
    fn the_two_ends_derive_the_same_pair_in_opposite_directions() {
        let secret = [0x33u8; KEY_LEN];
        let info_hash = [0x44u8; 20];
        let (mut a_enc, mut a_dec) = ciphers(&secret, &info_hash, true);
        let (mut b_enc, mut b_dec) = ciphers(&secret, &info_hash, false);
        let mut buf = *b"twelve bytes";
        a_enc.apply(&mut buf);
        b_dec.apply(&mut buf);
        assert_eq!(&buf, b"twelve bytes");
        let mut buf = *b"other message";
        b_enc.apply(&mut buf);
        a_dec.apply(&mut buf);
        assert_eq!(&buf, b"other message");
    }
}
