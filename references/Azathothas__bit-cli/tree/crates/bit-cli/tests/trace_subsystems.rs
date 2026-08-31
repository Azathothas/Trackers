//! Every documented `--trace` subsystem writes on a target it raises.
//!
//! `TODO/cli-surface.md` T-219 is what this file exists for. `--trace` named
//! eleven subsystems and ten of them raised a `tracing` target nothing wrote
//! to, so a caller debugging a stalled write turned on `--trace disk`, got
//! nothing, and concluded there had been no writes. Measured before it was
//! fixed: one `download` tracing all ten wrote **0** lines of stderr against
//! 32 for `http` on the same run.
//!
//! # Why this drives the binary rather than calling `run`
//!
//! The subscriber is process-global and `bit_cli::logging::install` is
//! best-effort by design, so the first in-process test to install one decides
//! the filter for every test after it. A trace assertion made in-process would
//! be reading whichever filter won that race, and records from a concurrent
//! test's run would land in the same buffer. An integration test is its own
//! process, which is what makes "this command, with this flag, wrote this
//! record" a statement about one run.
//!
//! `--log-format json` puts the target in a field rather than in a line to be
//! parsed, so the assertion is on the target and not on the wording of a
//! message.
//!
//! # What is checked here and what is not
//!
//! Every subsystem in `SUBSYSTEMS` has a case, and
//! `every_documented_subsystem_has_a_case` fails when one is added without
//! one. Each case asserts a record on one of the targets that subsystem
//! raises.
//!
//! One target is not reachable from here: `librqbit_dht`. Every trace in the
//! vendored DHT crate is on a query, a response or a routing table change, and
//! all three need the public DHT, so a test that asserted one would be
//! asserting that a CI runner can reach the internet. It was measured
//! instead, on a run with `-vvv`: **221** records on `librqbit_dht::dht`. The
//! `dht` case below covers the subsystem through `bit_cli::dht`, which is the
//! one fact the vendored crate cannot carry: whether there is a DHT at all.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use bit_cli::logging::SUBSYSTEMS;

/// The binary under test, built by `cargo test`.
const BIN: &str = env!("CARGO_BIN_EXE_bit-cli");

/// Payload bytes. Eight pieces at the piece length below, which is enough for
/// the picker to move its window more than once and for the bridge to serve
/// more than one block.
const PAYLOAD_BYTES: usize = 2 * 1024 * 1024;

/// A payload and a `.torrent` for it, made once for the whole process.
///
/// Built by running `bit-cli create`, not by calling into the library: the
/// torrent a caller would have is the one the binary writes.
struct Fixture {
    /// The directory holding the payload, which is also the web seed root.
    payload: PathBuf,
    torrent: PathBuf,
    /// Somewhere each case can put its own output directory.
    scratch: PathBuf,
}

impl Fixture {
    fn get() -> &'static Fixture {
        static FIXTURE: OnceLock<Fixture> = OnceLock::new();
        FIXTURE.get_or_init(|| {
            // Leaked on purpose: the directory has to outlive every test in
            // this binary, and the process exiting is what cleans it up.
            let temp = Box::leak(Box::new(
                tempfile::tempdir().expect("a temporary directory"),
            ));
            let root = temp.path().to_path_buf();
            let payload = root.join("src");
            std::fs::create_dir_all(&payload).expect("the payload directory");
            let scratch = root.join("runs");
            std::fs::create_dir_all(&scratch).expect("the scratch directory");

            // Deterministic, and not a run of one byte: a payload that
            // compresses to nothing would let something along the path
            // shortcut the work these cases are watching.
            let mut bytes = vec![0u8; PAYLOAD_BYTES];
            let mut state: u64 = 987;
            for byte in bytes.iter_mut() {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                *byte = (state >> 33) as u8;
            }
            std::fs::write(payload.join("movie.bin"), &bytes).expect("the payload file");

            let torrent = root.join("fixture.torrent");
            let made = Command::new(BIN)
                .arg("create")
                .arg(&payload)
                .args(["--name", "payload"])
                .args(["--piece-length", "256KiB"])
                .arg("--no-creation-date")
                .arg("--output")
                .arg(&torrent)
                .arg("--force")
                .output()
                .expect("bit-cli create runs");
            assert!(
                made.status.success(),
                "create failed: {}",
                String::from_utf8_lossy(&made.stderr)
            );
            Fixture {
                payload,
                torrent,
                scratch,
            }
        })
    }

    /// The payload directory as a `file:` URL with a trailing slash, which is
    /// what a `prefix` composition appends each file's path to.
    ///
    /// A `file:` source takes the local branch of the fetcher and everything
    /// above it is the same code an HTTP source runs: the same window cache,
    /// concurrency limit, rate cap, retries, verification and trace record.
    /// That is what lets these cases exercise the whole path with no server.
    fn web_seed(&self) -> String {
        let text = self.payload.to_string_lossy().replace('\\', "/");
        let text = text.replace(' ', "%20");
        match text.starts_with('/') {
            true => format!("file://{text}/"),
            false => format!("file:///{text}/"),
        }
    }

    fn torrent(&self) -> String {
        self.torrent.to_string_lossy().into_owned()
    }

    fn out_dir(&self, case: &str) -> String {
        self.scratch.join(case).to_string_lossy().into_owned()
    }
}

/// A loopback server that answers every request `503` and closes.
///
/// Two cases need a failure rather than a success: `retry` needs a status the
/// classifier calls transient, so the ladder runs, and `tracker` needs an
/// exchange to report on. `503` is transient by
/// `webseed::fetch::classify_status`, which is what makes the retry
/// deterministic rather than dependent on how a connection error is reported
/// on one platform.
struct Stub {
    addr: SocketAddr,
}

impl Stub {
    fn start() -> Self {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback listener binds");
        let addr = listener.local_addr().expect("the bound address");
        // Detached: the process exiting is what stops it, and a test that
        // joined it would be waiting on a loop with no end condition.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 2048];
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                let _ = stream.flush();
            }
        });
        Self { addr }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

/// Run the binary and return every target it wrote a record on.
///
/// The exit code is not asserted. Two of the cases are failures on purpose,
/// and what is under test is that the run said what it was doing rather than
/// that it succeeded. `the_fixture_downloads` is what says the happy path
/// still works, so a case finding no records is failing on the trace and not
/// on a torrent that never ran.
fn targets_written(args: &[impl AsRef<OsStr>]) -> BTreeSet<String> {
    let output = Command::new(BIN)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("bit-cli runs: {e}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut targets = BTreeSet::new();
    for line in stderr.lines() {
        if !line.starts_with('{') {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(target) = record.get("target").and_then(|t| t.as_str()) {
            targets.insert(target.to_string());
        }
    }
    targets
}

/// The flags that turn a run into a traced one, in JSON so the target is a
/// field rather than something to parse out of a message.
fn traced(subsystem: &str, args: Vec<String>) -> BTreeSet<String> {
    let mut full = vec![
        "--trace".to_string(),
        subsystem.to_string(),
        "--log-format".to_string(),
        "json".to_string(),
    ];
    full.extend(args);
    targets_written(&full)
}

/// Assert one of the subsystem's own targets was written to.
///
/// A record's target may be longer than the directive, because `EnvFilter`
/// matches a directive against the **prefix** of a target: `librqbit_dht`
/// admits `librqbit_dht::dht`. The check here is that same rule, so a target
/// in the table cannot pass by matching something it does not name.
fn assert_traced(subsystem: &str, seen: &BTreeSet<String>) {
    let entry = SUBSYSTEMS
        .iter()
        .find(|s| s.name == subsystem)
        .unwrap_or_else(|| panic!("{subsystem} is not a documented subsystem"));
    let hit = seen.iter().any(|target| {
        entry
            .targets
            .iter()
            .any(|want| target == want || target.starts_with(&format!("{want}::")))
    });
    assert!(
        hit,
        "--trace {subsystem} raises {:?} and nothing wrote to any of them. \
         Targets seen on this run: {seen:?}",
        entry.targets
    );
}

/// The download every case that needs a running session drives, over the
/// `file:` source.
fn download(case: &str, extra: &[&str]) -> Vec<String> {
    let fixture = Fixture::get();
    let mut args = vec![
        "download".to_string(),
        fixture.torrent(),
        "--web-seed".to_string(),
        fixture.web_seed(),
        "--web-seed-mode".to_string(),
        "prefix".to_string(),
        "--no-torrent-web-seed".to_string(),
        "--web-seed-only".to_string(),
        "--port".to_string(),
        "0".to_string(),
        "--dir".to_string(),
        fixture.out_dir(case),
        "--json".to_string(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_string()));
    args
}

#[test]
fn every_documented_subsystem_has_a_case() {
    // Every name below has a `#[test]` in this file. A subsystem added to
    // `SUBSYSTEMS` without one fails here rather than shipping as a name that
    // raises a target nobody checked, which is what T-219 was.
    const COVERED: &[&str] = &[
        "peer",
        "handshake",
        "tracker",
        "dht",
        "http",
        "piece",
        "picker",
        "disk",
        "ratelimit",
        "retry",
        "config",
    ];
    for subsystem in SUBSYSTEMS {
        assert!(
            COVERED.contains(&subsystem.name),
            "--trace {} is documented and has no case in this file",
            subsystem.name
        );
    }
    assert_eq!(
        COVERED.len(),
        SUBSYSTEMS.len(),
        "a name here is not in SUBSYSTEMS"
    );
}

/// Driven through `version` rather than `config show`, deliberately.
///
/// The configuration is resolved once per run now, by T-222, so `--trace
/// config` works on every command. Asserting it on `config show` would pass
/// whether that were true or not, because that command resolves the
/// configuration itself.
#[test]
fn config_traces_the_resolution_and_its_origin() {
    let seen = traced("config", vec!["version".to_string(), "--json".to_string()]);
    assert_traced("config", &seen);
}

#[test]
fn dht_traces_whether_there_is_one() {
    // `--web-seed-only` turns the DHT off, and that is the case worth
    // covering: with the DHT off the vendored crate writes nothing at all, so
    // without this record "no DHT records" and "the flag does nothing" look
    // the same from outside.
    let seen = traced("dht", download("dht", &[]));
    assert_traced("dht", &seen);
}

#[test]
fn disk_traces_reads_writes_and_allocation() {
    let seen = traced("disk", download("disk", &[]));
    assert_traced("disk", &seen);
}

#[test]
fn http_traces_the_request() {
    let seen = traced("http", download("http", &[]));
    assert_traced("http", &seen);
}

#[test]
fn peer_traces_wire_messages() {
    let seen = traced("peer", download("peer", &[]));
    assert_traced("peer", &seen);
}

#[test]
fn handshake_traces_the_negotiation() {
    let seen = traced("handshake", download("handshake", &[]));
    assert_traced("handshake", &seen);
    assert!(
        seen.contains("librqbit::handshake"),
        "the session's side of the handshake is on its own target: {seen:?}"
    );
}

#[test]
fn piece_traces_what_was_served_and_verified() {
    let seen = traced("piece", download("piece", &[]));
    assert_traced("piece", &seen);
}

#[test]
fn picker_traces_the_order_it_asks_in() {
    // `--piece-selector in-order` is what puts this repository's own picker in
    // the path. Without it the order is the session's and only
    // `librqbit::picker` writes, so the flag is here to cover both targets
    // rather than to change what is asserted.
    let seen = traced(
        "picker",
        download("picker", &["--piece-selector", "in-order"]),
    );
    assert_traced("picker", &seen);
    assert!(
        seen.contains("bit_cli::picker"),
        "in-order puts this repository's own picker in the path: {seen:?}"
    );
}

#[test]
fn ratelimit_traces_the_bucket_decision() {
    let seen = traced(
        "ratelimit",
        download("ratelimit", &["--web-seed-speed-limit", "2MiB"]),
    );
    assert_traced("ratelimit", &seen);
}

#[test]
fn retry_traces_the_ladder_and_the_error_budget() {
    let fixture = Fixture::get();
    let stub = Stub::start();
    let seen = traced(
        "retry",
        vec![
            "download".to_string(),
            fixture.torrent(),
            "--web-seed".to_string(),
            stub.url("/payload/"),
            "--web-seed-mode".to_string(),
            "prefix".to_string(),
            "--no-torrent-web-seed".to_string(),
            "--web-seed-only".to_string(),
            "--web-seed-retries".to_string(),
            "1".to_string(),
            "--port".to_string(),
            "0".to_string(),
            // The run cannot finish: every request is refused. It is bounded
            // by a deadline the run itself enforces rather than by this test
            // waiting on one.
            "--stop-after".to_string(),
            "8s".to_string(),
            "--dir".to_string(),
            fixture.out_dir("retry"),
            "--json".to_string(),
        ],
    );
    assert_traced("retry", &seen);
}

#[test]
fn tracker_traces_the_announce_and_the_response() {
    let fixture = Fixture::get();
    let stub = Stub::start();
    let seen = traced(
        "tracker",
        vec![
            "trackers".to_string(),
            fixture.torrent(),
            "--tracker".to_string(),
            stub.url("/announce"),
            "--replace-trackers".to_string(),
            "--json".to_string(),
        ],
    );
    assert_traced("tracker", &seen);
    assert!(
        seen.contains("bit_cli::tracker"),
        "`trackers` is this repository's own client: {seen:?}"
    );
}

/// The other half of `tracker`: the vendored session's own announce.
///
/// `bit-cli trackers` uses this repository's tracker client, so it can never
/// write on `librqbit_tracker_comms`. A `download` with trackers enabled does,
/// and the tracker is on loopback so nothing leaves the machine. The DHT and
/// local service discovery are off for the same reason.
#[test]
fn tracker_covers_the_session_announce_as_well() {
    let fixture = Fixture::get();
    let stub = Stub::start();
    let seen = traced(
        "tracker",
        vec![
            "download".to_string(),
            fixture.torrent(),
            "--web-seed".to_string(),
            fixture.web_seed(),
            "--web-seed-mode".to_string(),
            "prefix".to_string(),
            "--no-torrent-web-seed".to_string(),
            "--tracker".to_string(),
            stub.url("/announce"),
            "--replace-trackers".to_string(),
            "--no-dht".to_string(),
            "--no-lsd".to_string(),
            "--port".to_string(),
            "0".to_string(),
            "--dir".to_string(),
            fixture.out_dir("tracker-session"),
            "--json".to_string(),
        ],
    );
    assert!(
        seen.contains("librqbit_tracker_comms::tracker_comms"),
        "the session's own announce writes on the vendored target: {seen:?}"
    );
}

/// A run that traces nothing writes nothing, which is the other half of the
/// promise: `--trace` raises one subsystem and leaves the rest alone.
#[test]
fn an_untraced_run_writes_no_records() {
    let mut args = vec!["--log-format".to_string(), "json".to_string()];
    args.extend(download("untraced", &[]));
    let output = Command::new(BIN)
        .args(&args)
        .output()
        .expect("bit-cli runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let records: Vec<&str> = stderr.lines().filter(|l| l.starts_with('{')).collect();
    assert!(
        records.is_empty(),
        "an untraced run wrote {} records: {:?}",
        records.len(),
        &records[..records.len().min(3)]
    );
}

/// The fixture downloads, so a case that finds no records is failing on the
/// trace and not on a torrent that never ran.
#[test]
fn the_fixture_downloads() {
    let case = "baseline";
    let args = download(case, &[]);
    let output = Command::new(BIN)
        .args(&args)
        .output()
        .expect("bit-cli runs");
    assert!(
        output.status.success(),
        "the fixture download failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let landed = PathBuf::from(Fixture::get().out_dir(case))
        .join("payload")
        .join("movie.bin");
    assert_eq!(
        std::fs::metadata(&landed).map(|m| m.len()).unwrap_or(0),
        PAYLOAD_BYTES as u64,
        "{} is not the payload",
        landed.display()
    );
}
