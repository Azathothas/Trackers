//! MSE, the BitTorrent message stream encryption, and the policy around it.
//!
//! Why it exists here: a peer configured to require encryption will not
//! exchange a byte with a plaintext-only client, so the swarm a plaintext
//! client can reach is smaller than the swarm that exists. That is an
//! interoperability cost before it is a privacy feature, and it is what
//! `TODO/peers.md` T-163 measures.
//!
//! What it is not: RC4 with a 768 bit Diffie-Hellman exchange is not
//! confidentiality against anybody serious, and this module does not claim it.
//! What it buys is that a middlebox cannot classify the stream by reading
//! `BitTorrent protocol` off the front of it, and that peers which refuse
//! plaintext will talk to us.
//!
//! # Where it plugs in
//!
//! `librqbit` calls one trait, `StreamTransform`, on both sides of a
//! connection: once after dialling, before the BitTorrent handshake goes out,
//! and once after accepting, before it is read. [`MseTransform`] implements
//! it. The implementation is here rather than in the vendored tree because
//! only the seam has to be there, and a seam is what the next upstream release
//! has to be reconciled against. See `patches/UPSTREAM.md`.
//!
//! # One listening port
//!
//! There is no second port and no mode flag on the wire. The accepting end
//! reads the first 20 bytes and compares them with the plaintext protocol
//! header: an MSE connection opens with 96 bytes of public key, so the two are
//! told apart by looking, and the bytes are pushed back either way.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Mutex;

use librqbit::stream_transform::{
    BoxAsyncReadVectored, BoxAsyncWrite, OutgoingTransform, StreamTransform, TransformFuture,
};
use librqbit_core::hash_id::Id20;

pub mod dh768;
pub mod handshake;
pub mod rc4;
pub mod stream;

pub use handshake::Negotiated;

use handshake::Accepted;
use stream::{EncryptedWrite, Prefixed};

/// What this session does about encryption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encryption {
    /// Never encrypt, and refuse a peer that opens with MSE.
    Off,
    /// Try MSE when dialling, and fall back to plaintext on the one retry that
    /// a failed MSE handshake leaves possible. Accept both.
    #[default]
    Prefer,
    /// MSE or nothing, in both directions.
    Require,
}

impl Encryption {
    /// The flag values, for a parser and for an error message.
    pub const VALUES: [&'static str; 3] = ["off", "prefer", "require"];

    pub fn as_str(self) -> &'static str {
        match self {
            Encryption::Off => "off",
            Encryption::Prefer => "prefer",
            Encryption::Require => "require",
        }
    }
}

impl std::str::FromStr for Encryption {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "off" => Ok(Encryption::Off),
            "prefer" => Ok(Encryption::Prefer),
            "require" => Ok(Encryption::Require),
            other => Err(format!(
                "`{other}` is not an encryption mode; use {}",
                Encryption::VALUES.join(", ")
            )),
        }
    }
}

impl std::fmt::Display for Encryption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How many peers' outcomes are remembered.
///
/// The same bound and the same reason as `librqbit`'s peer records: a long
/// running seeder sees more addresses than it has peers, and a map that only
/// grows is the defect `TODO/memory.md` T-040 measured. Oldest out first.
const MAX_OUTCOMES: usize = 1024;

/// What each peer settled on, bounded, for `--json`.
#[derive(Debug, Default)]
struct Outcomes {
    by_addr: HashMap<SocketAddr, Negotiated>,
    order: VecDeque<SocketAddr>,
}

impl Outcomes {
    fn record(&mut self, addr: SocketAddr, mode: Negotiated) {
        if self.by_addr.insert(addr, mode).is_none() {
            self.order.push_back(addr);
            while self.order.len() > MAX_OUTCOMES {
                if let Some(old) = self.order.pop_front() {
                    self.by_addr.remove(&old);
                }
            }
        }
    }
}

/// The session's encryption policy, as `librqbit` sees it.
pub struct MseTransform {
    policy: Encryption,
    outcomes: Mutex<Outcomes>,
}

impl std::fmt::Debug for MseTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MseTransform")
            .field("policy", &self.policy)
            .finish()
    }
}

impl MseTransform {
    pub fn new(policy: Encryption) -> Self {
        Self {
            policy,
            outcomes: Mutex::new(Outcomes::default()),
        }
    }

    pub fn policy(&self) -> Encryption {
        self.policy
    }

    /// What a peer settled on, or `None` for one this session never completed
    /// a connection with.
    pub fn negotiated(&self, addr: &SocketAddr) -> Option<Negotiated> {
        self.outcomes
            .lock()
            .ok()
            .and_then(|o| o.by_addr.get(addr).copied())
    }

    /// Every recorded outcome, as strings, for a caller that joins on address.
    pub fn negotiated_all(&self) -> HashMap<String, &'static str> {
        match self.outcomes.lock() {
            Ok(o) => o
                .by_addr
                .iter()
                .map(|(addr, mode)| (addr.to_string(), mode.as_str()))
                .collect(),
            Err(_) => HashMap::new(),
        }
    }

    fn record(&self, addr: SocketAddr, mode: Negotiated) {
        if let Ok(mut o) = self.outcomes.lock() {
            o.record(addr, mode);
        }
    }
}

impl StreamTransform for MseTransform {
    fn outgoing<'a>(
        &'a self,
        addr: SocketAddr,
        info_hash: Id20,
        read: BoxAsyncReadVectored,
        write: BoxAsyncWrite,
    ) -> TransformFuture<'a, OutgoingTransform> {
        Box::pin(async move {
            if self.policy == Encryption::Off {
                self.record(addr, Negotiated::Plaintext);
                return Ok(OutgoingTransform::Stream(read, write));
            }
            let mut write = write;
            match handshake::initiate(read, &mut write, &info_hash.0).await {
                Ok(established) => {
                    self.record(addr, Negotiated::Rc4);
                    Ok(OutgoingTransform::Stream(
                        Box::new(Prefixed::new(
                            established.reader,
                            established.leftover,
                            Some(established.decrypt),
                        )),
                        Box::new(EncryptedWrite::new(write, established.encrypt)),
                    ))
                }
                Err(e) if self.policy == Encryption::Prefer => {
                    tracing::debug!(
                        %addr,
                        "encrypted handshake failed, dialling again in plaintext: {e:#}"
                    );
                    // Recorded here rather than after the redial, because the
                    // transform is not called on the second attempt: that is
                    // what asking for a plaintext retry means. So this is the
                    // last point at which anything knows what this peer
                    // settled on, and without it a peer reached by the
                    // fallback reports no mode at all.
                    self.record(addr, Negotiated::Plaintext);
                    Ok(OutgoingTransform::RetryPlaintext)
                }
                Err(e) => Err(anyhow::anyhow!(
                    "encryption is required and the handshake with {addr} failed: {e}"
                )),
            }
        })
    }

    fn incoming<'a>(
        &'a self,
        addr: SocketAddr,
        info_hashes: Vec<Id20>,
        read: BoxAsyncReadVectored,
        write: BoxAsyncWrite,
    ) -> TransformFuture<'a, (BoxAsyncReadVectored, BoxAsyncWrite)> {
        Box::pin(async move {
            let hashes: Vec<[u8; 20]> = info_hashes.iter().map(|h| h.0).collect();
            let mut write = write;
            let allow = self.policy != Encryption::Off;
            match handshake::respond(read, &mut write, &hashes, allow).await? {
                Accepted::Plaintext { prefix, reader } => {
                    if self.policy == Encryption::Require {
                        anyhow::bail!("encryption is required and {addr} opened in plaintext");
                    }
                    self.record(addr, Negotiated::Plaintext);
                    Ok((
                        Box::new(Prefixed::new(reader, prefix, None)) as BoxAsyncReadVectored,
                        write,
                    ))
                }
                Accepted::Encrypted(established) => {
                    self.record(addr, Negotiated::Rc4);
                    Ok((
                        Box::new(Prefixed::new(
                            established.reader,
                            established.leftover,
                            Some(established.decrypt),
                        )) as BoxAsyncReadVectored,
                        Box::new(EncryptedWrite::new(write, established.encrypt)) as BoxAsyncWrite,
                    ))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_parses_and_prints_back() {
        for value in Encryption::VALUES {
            let parsed: Encryption = value.parse().expect("mode parses");
            assert_eq!(parsed.as_str(), value);
        }
    }

    #[test]
    fn an_unknown_mode_names_the_ones_that_work() {
        let err = "sometimes".parse::<Encryption>().unwrap_err();
        assert!(err.contains("off"), "{err}");
        assert!(err.contains("prefer"), "{err}");
        assert!(err.contains("require"), "{err}");
    }

    #[test]
    fn the_default_is_prefer() {
        assert_eq!(Encryption::default(), Encryption::Prefer);
    }

    /// The outcome map is what a long run accumulates, so the bound is the
    /// property worth asserting rather than the lookup.
    #[test]
    fn the_outcome_map_stops_at_its_bound() {
        let mut outcomes = Outcomes::default();
        let over = MAX_OUTCOMES as u32 + 500;
        for i in 0..over {
            let ip = std::net::Ipv4Addr::from(i);
            outcomes.record(SocketAddr::from((ip, 6881)), Negotiated::Rc4);
        }
        assert_eq!(outcomes.by_addr.len(), MAX_OUTCOMES);
        assert_eq!(outcomes.order.len(), MAX_OUTCOMES);
        // The oldest went first, so the newest is still there and the first is
        // not.
        let newest = SocketAddr::from((std::net::Ipv4Addr::from(over - 1), 6881));
        let oldest = SocketAddr::from((std::net::Ipv4Addr::from(0u32), 6881));
        assert!(outcomes.by_addr.contains_key(&newest));
        assert!(!outcomes.by_addr.contains_key(&oldest));
    }

    /// Re-recording an address must not grow the queue, or the bound counts
    /// connections rather than addresses.
    #[test]
    fn recording_the_same_address_twice_does_not_grow_the_queue() {
        let mut outcomes = Outcomes::default();
        let addr = SocketAddr::from(([203, 0, 113, 7], 6881));
        outcomes.record(addr, Negotiated::Plaintext);
        outcomes.record(addr, Negotiated::Rc4);
        assert_eq!(outcomes.order.len(), 1);
        assert_eq!(outcomes.by_addr.get(&addr), Some(&Negotiated::Rc4));
    }
}
