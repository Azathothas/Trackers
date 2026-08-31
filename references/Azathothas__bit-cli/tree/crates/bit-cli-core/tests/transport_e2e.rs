//! Does a torrent actually move over each transport?
//!
//! `--transport tcp|utp|both` is `TODO/bep-coverage.md` T-101, and a flag that
//! selects a transport is worth nothing unless a transfer over the one it
//! selected completes. So these are two real sessions in one process: one
//! holding the payload, one starting with nothing, connected by an address and
//! by no other means. No tracker, no DHT, no local discovery, no web seed.
//!
//! **The negative case is what makes the positive one mean something.** A uTP
//! leecher against a TCP-only seeder must not complete. Without that,
//! `transport: Utp` completing proves only that a transfer happened, and the
//! first version of this flag is exactly the case that would have passed: it
//! set `ListenerOptions::mode` and left the dialer on TCP, so a `Utp` run
//! reached a `Tcp` peer over TCP and called it a pass.
//!
//! Both sides bind `127.0.0.1` and an OS-chosen port, for the same two reasons
//! `hostile_paths.rs` does: a session that can only reach loopback cannot
//! reach the network by accident, and the default nine-port range makes tests
//! that run beside each other race for it. It is **not** because uTP needs it.
//! A run over the default `[::]` dual-stack bind completes: that was believed
//! for a while during T-101 and the belief was formed before the real cause
//! was found, which is T-233.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use bit_cli_core::engine::{AddOptions, Engine, EngineOptions, Transport};
use bit_cli_core::mse::Encryption;
use bit_cli_core::torrent::bencode::{Value, encode};
use sha1::{Digest, Sha1};

const PIECE_LENGTH: usize = 16 * 1024;
const PAYLOAD_LEN: usize = 512 * 1024;

/// Deterministic bytes, so both sides agree on what the payload is.
fn content(len: usize) -> Vec<u8> {
    let mut state = 20_260_824u64;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

/// A single-file torrent over `payload`, with real piece hashes.
fn torrent_for(name: &str, payload: &[u8]) -> Vec<u8> {
    let mut pieces = Vec::new();
    for chunk in payload.chunks(PIECE_LENGTH) {
        pieces.extend_from_slice(&Sha1::digest(chunk));
    }
    let info = Value::Dict(BTreeMap::from([
        (b"length".to_vec(), Value::Int(payload.len() as i64)),
        (b"name".to_vec(), Value::Bytes(name.as_bytes().to_vec())),
        (b"piece length".to_vec(), Value::Int(PIECE_LENGTH as i64)),
        (b"pieces".to_vec(), Value::Bytes(pieces)),
    ]));
    encode(&Value::Dict(BTreeMap::from([(b"info".to_vec(), info)])))
}

/// A session that can reach nothing it is not told about.
async fn engine_at(
    directory: &std::path::Path,
    transport: Transport,
    encryption: Encryption,
) -> Engine {
    Engine::start(&EngineOptions {
        download_directory: directory.to_path_buf(),
        // An OS-chosen port on loopback. The default range is nine ports wide
        // and these tests run beside each other, so a fixed range makes them
        // race for it.
        listen_ports: 0..=0,
        listen_ip: Some(Ipv4Addr::LOCALHOST.into()),
        enable_dht: false,
        enable_lsd: false,
        enable_trackers: false,
        transport,
        encryption,
        ..Default::default()
    })
    .await
    .unwrap()
}

/// Seed `payload` on `seed_transport`, fetch it on `leech_transport`, and say
/// whether the bytes arrived.
///
/// Waits on the condition rather than on a duration: the loop polls for the
/// finished file and gives up at a deadline, and the deadline is a failure
/// rather than the measurement. See `TODO/RULES.md` section 5.
async fn transfer(
    seed_transport: Transport,
    leech_transport: Transport,
    encryption: Encryption,
    deadline: Duration,
) -> bool {
    let payload = content(PAYLOAD_LEN);
    let torrent = torrent_for("payload.bin", &payload);

    let seed_dir = tempfile::tempdir().unwrap();
    let leech_dir = tempfile::tempdir().unwrap();
    let meta_dir = tempfile::tempdir().unwrap();
    let meta = meta_dir.path().join("transport.torrent");
    std::fs::write(&meta, &torrent).unwrap();
    std::fs::write(seed_dir.path().join("payload.bin"), &payload).unwrap();

    let seeder = engine_at(seed_dir.path(), seed_transport, encryption).await;
    let seed_handle = seeder
        .add(
            &meta.display().to_string(),
            &AddOptions {
                overwrite: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let _ = seeder.wait_until_initialized(&seed_handle).await;

    let listen = seeder.listen_addr().expect("the seeder bound a port");
    let peer = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), listen.port());

    let leecher = engine_at(leech_dir.path(), leech_transport, encryption).await;
    let leech_handle = leecher
        .add(
            &meta.display().to_string(),
            &AddOptions {
                overwrite: true,
                initial_peers: vec![peer],
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let landed = leech_dir.path().join("payload.bin");
    let started = std::time::Instant::now();
    let mut arrived = false;
    while started.elapsed() < deadline {
        if std::fs::read(&landed)
            .map(|got| got == payload)
            .unwrap_or(false)
        {
            arrived = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let _ = leech_handle;
    leecher.stop().await;
    seeder.stop().await;
    arrived
}

/// TCP moves a torrent. The control for everything below.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_moves_over_tcp() {
    assert!(
        transfer(
            Transport::Tcp,
            Transport::Tcp,
            Encryption::Prefer,
            Duration::from_secs(60)
        )
        .await,
        "the payload did not arrive over TCP, so nothing else in this file means anything"
    );
}

/// A uTP leecher does not reach a TCP-only seeder.
///
/// This is the negative control and it is the assertion that gives
/// [`a_torrent_moves_over_utp`] its meaning. `Transport::Utp` has to turn TCP
/// off in the **dialer** as well as in the listener, and the first version of
/// the flag did not: it set `ListenerOptions::mode` alone, the dialer tried TCP
/// first as it always does, and a run asking for uTP got TCP and reported
/// success.
///
/// The deadline is short on purpose. Nothing is expected to connect, so this
/// case costs whatever it is given.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_utp_leecher_does_not_reach_a_tcp_seeder() {
    assert!(
        !transfer(
            Transport::Utp,
            Transport::Tcp,
            Encryption::Off,
            Duration::from_secs(10)
        )
        .await,
        "a uTP-only run reached a TCP-only peer, so --transport is not reaching the dialer"
    );
}

/// A torrent moves over uTP, with TCP off on both ends.
///
/// Neither side has a TCP listener and neither dials TCP, so every byte here
/// crossed BEP 29. See `TODO/bep-coverage.md`, T-101.
///
/// `Encryption::Off` is not incidental and is not tidiness. uTP carries a
/// torrent in plaintext and does not carry one under MSE, which is
/// `TODO/peers.md` T-233 and is pinned below.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_moves_over_utp() {
    assert!(
        transfer(
            Transport::Utp,
            Transport::Utp,
            Encryption::Off,
            Duration::from_secs(60)
        )
        .await,
        "the payload did not arrive over uTP"
    );
}

/// `both` moves a torrent, and says nothing about which transport carried it.
///
/// It is here because `both` is a value the flag takes and an untested value
/// is a value that does not work. What it cannot assert is the transport: the
/// dialer tries TCP first and reaches uTP only a second later, so `both`
/// against `both` over loopback is TCP every time.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_moves_when_both_transports_are_enabled() {
    assert!(
        transfer(
            Transport::Both,
            Transport::Both,
            Encryption::Prefer,
            Duration::from_secs(60)
        )
        .await,
        "the payload did not arrive with both transports enabled"
    );
}

/// MSE over uTP does not carry a torrent, and this pins that it does not.
///
/// `TODO/peers.md` T-233. The handshake completes, the extended handshake,
/// `HaveAll` and `Unchoke` all arrive, the leecher sends `Interested` and its
/// first block requests, and the seeder never reads them. It is not the
/// transport and it is not the encryption on its own: every other combination
/// of the two works.
///
/// Pinned rather than asserted as correct, the same way `TODO/metainfo.md`
/// T-173's drop was. A change that makes this transfer complete fails here and
/// is read as progress.
///
/// The deadline is short because nothing is expected to arrive.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mse_over_utp_does_not_carry_a_torrent() {
    assert!(
        !transfer(
            Transport::Utp,
            Transport::Utp,
            Encryption::Require,
            Duration::from_secs(15)
        )
        .await,
        "MSE over uTP completed, which closes T-233"
    );
}

/// MSE over TCP does carry one, which is what makes the pin above a statement
/// about the pair rather than about encryption.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mse_over_tcp_carries_a_torrent() {
    assert!(
        transfer(
            Transport::Tcp,
            Transport::Tcp,
            Encryption::Require,
            Duration::from_secs(60)
        )
        .await,
        "MSE over TCP did not complete, so T-233 is not about uTP"
    );
}
