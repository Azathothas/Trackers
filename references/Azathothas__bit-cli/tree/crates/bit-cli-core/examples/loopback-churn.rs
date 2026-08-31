//! A peer that connects, handshakes, and goes away, over and over.
//!
//! It exists to answer one question: does a long-lived `bit-cli seed` leak a
//! socket every time a peer disconnects. A socket in `CLOSE_WAIT` is one whose
//! peer sent `FIN` and whose local side never called `close`, so the way to
//! produce them is a peer that connects and then closes, thousands of times.
//! See `TODO/peers.md`, T-020.
//!
//! Time is not the variable, connections are. A reporter saw twenty thousand
//! stuck sockets after two days as a service; this reaches the same connection
//! count in minutes, so the leak is either visible immediately or is not there.
//!
//! ```text
//! cargo run -p bit-cli-core --example loopback-churn -- \
//!   --peer 127.0.0.1:51413 --info-hash <40 hex> --connections 20000
//! ```
//!
//! It is a test fixture, not a product. Two behaviours, because which one
//! leaks says where the leak is:
//!
//! - `--handshake` (the default) sends a BEP 3 handshake and reads the reply,
//!   so the far side has a peer it accepted rather than a stray connection.
//! - `--no-handshake` connects and closes without a byte, which is what a port
//!   scan looks like.
//!
//! There is no mode that sends `RST` instead of `FIN`. `SO_LINGER` is not on
//! stable Rust and the control it would give, that a reset cannot leave the
//! far side in `CLOSE_WAIT`, is available for free from the socket state
//! counts the harness already takes.
//!
//! Progress goes to stderr with an ISO 8601 UTC millisecond timestamp. The
//! last line of stdout is one JSON object, so a script can read the counts
//! without parsing the log.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use bit_cli_core::time::now_iso;

/// Wire size of a BitTorrent v1 handshake.
const HANDSHAKE_LEN: usize = 68;

fn main() {
    let mut peer: Option<SocketAddr> = None;
    let mut info_hash: Option<[u8; 20]> = None;
    let mut connections: u64 = 1000;
    let mut concurrency: usize = 8;
    let mut handshake = true;
    let mut settle_ms: u64 = 0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--peer" => {
                peer = Some(
                    next_value(&mut args, "--peer")
                        .parse()
                        .expect("--peer wants HOST:PORT"),
                )
            }
            "--info-hash" => {
                info_hash = Some(
                    decode_hash(&next_value(&mut args, "--info-hash"))
                        .expect("--info-hash wants 40 hex characters"),
                )
            }
            "--connections" => {
                connections = next_value(&mut args, "--connections")
                    .parse()
                    .expect("--connections")
            }
            "--concurrency" => {
                concurrency = next_value(&mut args, "--concurrency")
                    .parse()
                    .expect("--concurrency")
            }
            "--no-handshake" => handshake = false,
            "--settle" => {
                settle_ms = next_value(&mut args, "--settle")
                    .parse()
                    .expect("--settle wants milliseconds")
            }
            "--help" | "-h" => {
                println!(
                    "usage: loopback-churn --peer HOST:PORT [--info-hash HEX]\n\
                     \x20                     [--connections N] [--concurrency N]\n\
                     \x20                     [--no-handshake] [--settle MS]"
                );
                return;
            }
            other => {
                eprintln!("loopback-churn: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }

    let Some(peer) = peer else {
        eprintln!("loopback-churn: --peer HOST:PORT is required");
        std::process::exit(2);
    };
    if handshake && info_hash.is_none() {
        eprintln!("loopback-churn: --info-hash is required unless --no-handshake is set");
        std::process::exit(2);
    }
    let concurrency = concurrency.max(1);

    eprintln!(
        "{} churning {connections} connections at {peer}, {concurrency} at a time",
        now_iso()
    );
    let began = Instant::now();
    let counts = std::sync::Arc::new(Counts::default());
    let next = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    let mut workers = Vec::new();
    for _ in 0..concurrency {
        let counts = counts.clone();
        let next = next.clone();
        workers.push(std::thread::spawn(move || {
            loop {
                let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if index >= connections {
                    return;
                }
                match one(peer, info_hash, handshake) {
                    Ok(()) => counts.bump(&counts.completed),
                    Err(_) => counts.bump(&counts.failed),
                }
                // Every thousand, so a long run says it is alive without
                // drowning the log.
                let done = counts.total();
                if done % 1000 == 0 {
                    eprintln!("{} {done} connections", now_iso());
                }
            }
        }));
    }
    for worker in workers {
        let _ = worker.join();
    }

    // Windows holds a closed socket in TIME_WAIT for a while, and a count
    // taken the instant the last connection closes measures the churn rather
    // than what it left behind.
    if settle_ms > 0 {
        eprintln!("{} settling for {settle_ms}ms", now_iso());
        std::thread::sleep(Duration::from_millis(settle_ms));
    }

    let elapsed = began.elapsed();
    let completed = counts.completed.load(std::sync::atomic::Ordering::Relaxed);
    let failed = counts.failed.load(std::sync::atomic::Ordering::Relaxed);
    eprintln!(
        "{} done: {completed} completed, {failed} failed in {:?}",
        now_iso(),
        elapsed
    );
    println!(
        "{{\"kind\":\"loopback-churn\",\"peer\":\"{peer}\",\"completed\":{completed},\
         \"failed\":{failed},\"elapsed_ms\":{},\"handshake\":{handshake}}}",
        elapsed.as_millis()
    );
    std::io::stdout().flush().ok();
}

/// Counters, shared across the worker threads.
#[derive(Default)]
struct Counts {
    completed: std::sync::atomic::AtomicU64,
    failed: std::sync::atomic::AtomicU64,
}

impl Counts {
    fn bump(&self, counter: &std::sync::atomic::AtomicU64) {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn total(&self) -> u64 {
        self.completed.load(std::sync::atomic::Ordering::Relaxed)
            + self.failed.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// One connection, from `connect` to `close`.
fn one(peer: SocketAddr, info_hash: Option<[u8; 20]>, handshake: bool) -> std::io::Result<()> {
    let stream = TcpStream::connect_timeout(&peer, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut stream = stream;
    if handshake && let Some(hash) = info_hash {
        stream.write_all(&handshake_bytes(&hash))?;
        stream.flush()?;
        // Read the reply so the far side has completed its half too. A peer
        // that vanishes before the reply is a different case and would not
        // exercise the path this is about.
        let mut reply = [0u8; HANDSHAKE_LEN];
        stream.read_exact(&mut reply)?;
    }
    // Dropping the stream closes it. That is the FIN the far side has to
    // answer, and the point of the whole exercise.
    drop(stream);
    Ok(())
}

/// A BEP 3 handshake for one info hash, with a fresh peer id.
fn handshake_bytes(info_hash: &[u8; 20]) -> [u8; HANDSHAKE_LEN] {
    let mut out = [0u8; HANDSHAKE_LEN];
    out[0] = 19;
    out[1..20].copy_from_slice(b"BitTorrent protocol");
    // Reserved bytes stay zero: no extensions are claimed, because this peer
    // does nothing but connect and leave.
    out[28..48].copy_from_slice(info_hash);
    out[48..56].copy_from_slice(b"-BCch01-");
    // Twelve bytes of the clock, which is enough to keep two ids apart within
    // one run and does not need a random source.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tail = format!("{:012}", stamp % 1_000_000_000_000);
    out[56..68].copy_from_slice(&tail.as_bytes()[..12]);
    out
}

/// Twenty bytes from forty hex characters.
fn decode_hash(text: &str) -> Option<[u8; 20]> {
    if text.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> String {
    match args.next() {
        Some(value) => value,
        None => {
            eprintln!("loopback-churn: {flag} needs a value");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handshake_is_the_shape_bep_3_describes() {
        let hash = [0xabu8; 20];
        let bytes = handshake_bytes(&hash);
        assert_eq!(bytes.len(), HANDSHAKE_LEN);
        assert_eq!(bytes[0], 19);
        assert_eq!(&bytes[1..20], b"BitTorrent protocol");
        assert_eq!(&bytes[20..28], &[0u8; 8], "no extension is claimed");
        assert_eq!(&bytes[28..48], &hash);
        assert_eq!(&bytes[48..56], b"-BCch01-");
    }

    #[test]
    fn two_handshakes_carry_two_peer_ids() {
        let hash = [1u8; 20];
        let first = handshake_bytes(&hash);
        std::thread::sleep(Duration::from_millis(2));
        let second = handshake_bytes(&hash);
        assert_ne!(&first[56..68], &second[56..68]);
    }

    #[test]
    fn an_info_hash_is_forty_hex_characters_and_nothing_else() {
        assert_eq!(
            decode_hash("00112233445566778899aabbccddeeff00112233").unwrap()[..4],
            [0x00, 0x11, 0x22, 0x33]
        );
        assert!(decode_hash("abc").is_none(), "too short");
        assert!(decode_hash(&"a".repeat(41)).is_none(), "too long");
        assert!(decode_hash(&"z".repeat(40)).is_none(), "not hex");
    }
}
