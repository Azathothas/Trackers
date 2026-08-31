//! Is this process's own listener still answering peers?
//!
//! A seeder that cannot be handshaked is down, and nothing a supervisor
//! normally watches says so. The process is alive, the port is open, the
//! progress line still reports a ratio, and the log is silent. `TODO/peers.md`
//! T-020 measures the failure: `librqbit` 9.0.0's accept loop advances its
//! pending handshake-check queue by one entry per accepted connection, so a
//! run of connections that close before they handshake leaves a backlog, and
//! every peer that arrives afterwards waits behind it. Twenty such connections
//! were enough, and the target then served nobody for as long as it was left
//! alone.
//!
//! The only place that state is visible from is the outside of the socket, so
//! that is where this looks from. One probe dials the run's own listen
//! address, sends a BEP 3 handshake for an info hash the run is serving, and
//! waits for the reply. A healthy listener answers in under a millisecond over
//! loopback. A listener with a backlog answers nothing at all.
//!
//! # Why the probe uses a real info hash
//!
//! An unknown info hash would be cheaper: the session drops the connection
//! without recording a peer. It is also the wrong measurement. A probe that
//! resolves to an error inside the session **adds** an entry to the same
//! backlog it is measuring, which turns a queue of one into a coin flip and
//! reports an outage on a listener that would have served a real peer. A probe
//! that completes takes an entry off the queue instead, so it measures the
//! thing a peer would experience and does not make it worse.
//!
//! What that costs is one peer row per probe, which `librqbit` keeps in a
//! terminal state and never reclaims. Measured: 24 handshakes from loopback,
//! 24 rows, `live 0` and `dead 0` throughout, so they are inert rather than
//! re-dialled. [`Probe::local_port`] is what lets a caller drop them from a
//! reported peer list, the same way a web seed bridge's port is dropped.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use librqbit_core::Id20;
use librqbit_peer_protocol::Handshake;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::webseed::bridge::Framer;

/// Peer id prefix for the listener check, from the one place the client
/// identity lives. Distinct from the bridge's and the swarm generator's, so a
/// target's log says which of the three dialled it, and distinct from the
/// session's own, which is what stops a self-connect. See `TODO/peers.md`,
/// T-236.
const PEER_ID_PREFIX: [u8; 8] = crate::peer_id::role(*b"lc", *b"01");

/// The shortest deadline a probe is given, whatever the interval.
pub const MIN_TIMEOUT: Duration = Duration::from_secs(1);

/// The longest. `librqbit` reads an incoming handshake under a 10 second
/// timeout of its own, so a listener that has not answered in ten seconds has
/// not started reading.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(10);

/// What one probe found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Probe {
    /// Whether the listener answered with a handshake for the info hash it
    /// was asked about.
    pub healthy: bool,
    /// Milliseconds from the dial to the reply handshake. `None` when none
    /// came back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<u64>,
    /// The loopback port this probe connected from, once the connection was
    /// made. The peer row the session keeps is under this port, so a caller
    /// that reports peers can tell the probe from a swarm member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
    /// A stable name for what went wrong. `None` when healthy, and it never
    /// changes once released, because a caller branches on it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<&'static str>,
}

impl Probe {
    /// A probe that never got off the ground.
    fn failed(failure: &'static str, local_port: Option<u16>) -> Self {
        Self {
            healthy: false,
            rtt_ms: None,
            local_port,
            failure: Some(failure),
        }
    }
}

/// The deadline one probe gets, derived from how often probes are made.
///
/// A probe that is still waiting when the next one is due has failed, so the
/// interval is the natural bound. It is clamped at both ends: a sub-second
/// interval would call a slow loopback dial an outage, and past ten seconds
/// the session's own handshake read has already timed out.
#[must_use]
pub fn timeout_for(interval: Duration) -> Duration {
    interval.clamp(MIN_TIMEOUT, MAX_TIMEOUT)
}

/// Dial this run's own listener and see whether it answers.
///
/// `info_hash` must be one the run is serving. Anything else measures the
/// session's rejection path instead, which is the wrong question and the wrong
/// side of the backlog.
pub async fn probe(target: SocketAddr, info_hash: [u8; 20], timeout: Duration) -> Probe {
    let started = Instant::now();
    let stream = match tokio::time::timeout(timeout, TcpStream::connect(target)).await {
        Err(_) => return Probe::failed("connect_timeout", None),
        Ok(Err(e)) => return Probe::failed(crate::bench::swarm::connect_class(&e), None),
        Ok(Ok(stream)) => stream,
    };
    let local_port = stream.local_addr().ok().map(|a| a.port());
    let deadline = started + timeout;

    let (mut read, mut write) = stream.into_split();
    let ours = Handshake::new(
        Id20::new(info_hash),
        Id20::new(crate::peer_id::generate(&PEER_ID_PREFIX)),
    );
    let mut buf = [0u8; 68];
    let len = ours.serialize_unchecked_len(&mut buf);
    if write.write_all(&buf[..len]).await.is_err() {
        return Probe::failed("write_handshake", local_port);
    }

    let mut frames = Framer::default();
    let theirs = loop {
        if let Ok((theirs, _)) = Handshake::deserialize(frames.buffered()) {
            break theirs.info_hash;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Probe::failed("handshake_timeout", local_port);
        }
        match tokio::time::timeout(left, frames.fill(&mut read)).await {
            Err(_) => return Probe::failed("handshake_timeout", local_port),
            Ok(Ok(0)) => return Probe::failed("closed_before_handshake", local_port),
            Ok(Ok(_)) => {}
            Ok(Err(_)) => return Probe::failed("read_before_handshake", local_port),
        }
    };

    // An answer for a different torrent is not an answer. Nothing in this
    // tree can produce one, so this catches the case where the port was
    // rebound under the run rather than a fault in the session.
    if theirs.0 != info_hash {
        return Probe::failed("wrong_info_hash", local_port);
    }
    Probe {
        healthy: true,
        rtt_ms: Some(
            started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX))
                .try_into()
                .unwrap_or(u64::MAX),
        ),
        local_port,
        failure: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    const HASH: [u8; 20] = [9u8; 20];

    /// A listener that reads a handshake and answers with one, which is what
    /// a healthy session does.
    async fn answering(hash: [u8; 20]) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let addr = listener.local_addr().expect("an address");
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut incoming = [0u8; 68];
                    if stream.read_exact(&mut incoming).await.is_err() {
                        return;
                    }
                    // A stand-in for whatever remote peer answers, so it is
                    // deliberately **not** this client's identity: a fixture
                    // that replied with our own prefix would hide a
                    // self-connect rather than exercise one.
                    let reply =
                        Handshake::new(Id20::new(hash), Id20::new(*b"-ZZ0000-fixturepeer1"));
                    let mut buf = [0u8; 68];
                    let len = reply.serialize_unchecked_len(&mut buf);
                    let _ = stream.write_all(&buf[..len]).await;
                    // Held open, so the probe's own close is what ends it.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
            }
        });
        addr
    }

    /// A listener that accepts and then says nothing, which is the poisoned
    /// accept loop T-020 measures.
    async fn silent() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let addr = listener.local_addr().expect("an address");
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_listener_that_answers_is_healthy() {
        let addr = answering(HASH).await;
        let found = probe(addr, HASH, Duration::from_secs(5)).await;
        assert!(found.healthy, "{found:?}");
        assert!(found.failure.is_none());
        assert!(found.local_port.is_some_and(|p| p != 0));
    }

    #[tokio::test]
    async fn a_listener_that_accepts_and_says_nothing_is_not() {
        let addr = silent().await;
        let found = probe(addr, HASH, Duration::from_secs(1)).await;
        assert!(!found.healthy);
        assert_eq!(found.failure, Some("handshake_timeout"));
        // The port is still reported: the connection was made, so the peer row
        // exists on the other side whether or not the handshake finished.
        assert!(found.local_port.is_some());
    }

    #[tokio::test]
    async fn nothing_listening_is_refused_rather_than_reported_slow() {
        // Bind and drop, so the port is one nothing is on.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let addr = listener.local_addr().expect("an address");
        drop(listener);
        // Windows retries the SYN before it gives up, so a dead loopback port
        // takes about two seconds to refuse rather than refusing at once. The
        // deadline is well past that on purpose: what this asserts is the
        // class, and a two second deadline would assert the platform's retry
        // schedule instead.
        let found = probe(addr, HASH, Duration::from_secs(10)).await;
        assert!(!found.healthy);
        assert_eq!(found.failure, Some("connect_refused"));
        assert!(found.local_port.is_none());
    }

    #[tokio::test]
    async fn an_answer_for_another_torrent_is_not_an_answer() {
        let addr = answering([1u8; 20]).await;
        let found = probe(addr, HASH, Duration::from_secs(5)).await;
        assert!(!found.healthy);
        assert_eq!(found.failure, Some("wrong_info_hash"));
    }

    #[test]
    fn the_probe_deadline_is_clamped_at_both_ends() {
        assert_eq!(timeout_for(Duration::from_millis(1)), MIN_TIMEOUT);
        assert_eq!(timeout_for(Duration::from_secs(3)), Duration::from_secs(3));
        assert_eq!(timeout_for(Duration::from_secs(600)), MAX_TIMEOUT);
    }
}
