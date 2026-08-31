//! End-to-end tests for the web seed bridge.
//!
//! These run a real `librqbit` session and a stub HTTP server over loopback,
//! so they exercise the whole path: handshake, extended handshake, bitfield,
//! piece requests, ranged GETs, and the session's own hash verification.
//!
//! Nothing here reaches the network. The stub server binds `127.0.0.1:0` and
//! the session binds an OS-chosen port, so the tests never collide with each
//! other or with anything else on the machine.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bit_cli_core::engine::{AddOptions, Engine, EngineOptions};
use bit_cli_core::layout::Layout;
use bit_cli_core::webseed::binding::{BindingSet, Origin, SourceSpec};
use bit_cli_core::webseed::bridge::{self, BridgeParams, BridgeState, BridgeStatus};
use bit_cli_core::webseed::fetch::Fetcher;
use bit_cli_core::webseed::ledger::BlockLedger;
use bit_cli_core::webseed::scope::Scope;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Piece length for every fixture. Small enough to keep the payloads tiny,
/// large enough that pieces span file boundaries in the multi-file cases.
const PIECE_LENGTH: u32 = 32 * 1024;

/// How the stub server answers, so the failure paths are exercised too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServeMode {
    /// Honour `Range` properly.
    Ranges,
    /// Ignore `Range` and return the whole entity with `200 OK`.
    IgnoreRange,
    /// Answer everything with `404`.
    NotFound,
    /// Honour `Range` but return the wrong bytes.
    Corrupt,
    /// Return the wrong bytes the first time each range is asked for, and the
    /// right ones on every later request for it.
    ///
    /// This is the shape smart ban exists for. A mirror that is wrong forever
    /// is caught by the piece never verifying; one that is wrong once breaks a
    /// piece, the retry repairs it, and by the time the payload is correct
    /// nothing on the wire remembers who broke it. See `TODO/webseed.md`,
    /// T-179.
    CorruptOnce,
    /// Speak BEP 17: `?info_hash=&piece=&ranges=` instead of a `Range` header.
    Hoffman,
    /// Redirect once, then serve properly from the new location.
    Redirect,
    /// Answer 403 the first time each range is asked for and serve it on the
    /// retry. It is what a signing CDN does when a signature expires: the
    /// refusal is real, and the next request to the same URL succeeds because
    /// it is redirected to a fresh signature.
    ExpiringSignature,
}

/// Deterministic pseudorandom bytes, so fixtures have real piece hashes
/// without depending on a random source.
fn content(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

/// Bytes the stub server has sent, so a test can catch a source being asked
/// for the same range twice.
type Served = Arc<AtomicU64>;

/// Serve `root` over HTTP on loopback, returning the base URL.
///
/// Deliberately minimal: enough of HTTP/1.1 to answer the ranged GETs the
/// fetcher issues, and nothing else.
async fn serve(root: PathBuf, mode: ServeMode) -> (String, Served) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let served: Served = Served::default();
    let counter = served.clone();
    let refused: Refused = Refused::default();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let root = root.clone();
            let counter = counter.clone();
            let refused = refused.clone();
            tokio::spawn(async move {
                let _ = handle_request(stream, root, mode, counter, refused).await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}/"), served)
}

/// Ranges [`ServeMode::ExpiringSignature`] has already refused once.
///
/// Keyed by the target and the range, so the refusal follows the range rather
/// than the connection and every distinct range is refused exactly once.
type Refused = Arc<std::sync::Mutex<std::collections::HashSet<String>>>;

async fn handle_request(
    mut stream: TcpStream,
    root: PathBuf,
    mode: ServeMode,
    served: Served,
    refused: Refused,
) -> std::io::Result<()> {
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while !request.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).await? == 0 {
            return Ok(());
        }
        request.push(byte[0]);
    }
    let request = String::from_utf8_lossy(&request).to_string();
    let mut lines = request.lines();
    let target = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    // Header names are case-insensitive, and every HTTP client spells this
    // one differently.
    let range = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("range")
                .then(|| value.trim().strip_prefix("bytes="))?
        })
        .and_then(parse_range);

    // BEP 17 puts the piece and the sub-range in the query string, and there
    // is no `Range` header at all.
    if mode == ServeMode::Hoffman {
        return serve_hoffman(&mut stream, &root, &target, served).await;
    }
    if mode == ServeMode::ExpiringSignature {
        let key = format!("{target} {range:?}");
        let first_time = refused.lock().unwrap().insert(key);
        if first_time {
            return respond(&mut stream, 403, "Forbidden", None, b"").await;
        }
    }
    if mode == ServeMode::Redirect && !target.starts_with("/moved/") {
        let head = format!(
            "HTTP/1.1 302 Found\r\nLocation: /moved{target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(head.as_bytes()).await?;
        return stream.flush().await;
    }
    let target = target
        .strip_prefix("/moved")
        .map(str::to_string)
        .unwrap_or(target);

    // The query string is not part of the path. A real GetRight server ignores
    // a query it does not understand and serves the entity, which is what
    // makes BEP 17 detection a question about the **length** of the answer
    // rather than about its status. See `TODO/webseed.md`, T-004.
    let (target_path, _query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let path = root.join(percent_decode(target_path.trim_start_matches('/')));
    let Ok(body) = std::fs::read(&path) else {
        return respond(&mut stream, 404, "Not Found", None, b"missing").await;
    };
    if mode == ServeMode::NotFound {
        return respond(&mut stream, 404, "Not Found", None, b"missing").await;
    }
    if mode == ServeMode::IgnoreRange || range.is_none() {
        return respond(&mut stream, 200, "OK", None, &body).await;
    }

    let (start, end) = range.unwrap();
    let end = end.min(body.len().saturating_sub(1));
    if start > end {
        return respond(&mut stream, 416, "Range Not Satisfiable", None, b"").await;
    }
    let mut slice = body[start..=end].to_vec();
    // Corrupted the same way in both modes: flip every byte, so the data is
    // the right length and hashes wrong. `CorruptOnce` reuses the set that
    // `ExpiringSignature` keys by target and range, so the lie follows the
    // range rather than the connection and each range is lied about once.
    let corrupt = match mode {
        ServeMode::Corrupt => true,
        ServeMode::CorruptOnce => refused
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(format!("{target} {start}-{end}")),
        _ => false,
    };
    if corrupt {
        for byte in &mut slice {
            *byte = !*byte;
        }
    }
    let header = format!("bytes {start}-{end}/{}", body.len());
    served.fetch_add(slice.len() as u64, Ordering::Relaxed);
    respond(&mut stream, 206, "Partial Content", Some(&header), &slice).await
}

/// Answer one BEP 17 request.
///
/// The whole payload is served from one file on disk, so the piece index and
/// the sub-range inside it are turned back into an absolute offset. That is
/// exactly the mapping a real Hoffman seed does.
async fn serve_hoffman(
    stream: &mut TcpStream,
    root: &Path,
    target: &str,
    served: Served,
) -> std::io::Result<()> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut piece: Option<u64> = None;
    let mut range: Option<(usize, usize)> = None;
    let mut has_info_hash = false;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("piece", value)) => piece = value.parse().ok(),
            Some(("ranges", value)) => range = parse_range(value),
            Some(("info_hash", value)) => has_info_hash = !value.is_empty(),
            _ => {}
        }
    }
    let Ok(body) = std::fs::read(root.join(percent_decode(path.trim_start_matches('/')))) else {
        return respond(stream, 404, "Not Found", None, b"missing").await;
    };
    let (Some(piece), Some((begin, end)), true) = (piece, range, has_info_hash) else {
        return respond(stream, 400, "Bad Request", None, b"not a BEP 17 request").await;
    };

    let piece_length = PIECE_LENGTH as u64;
    let start = (piece * piece_length) as usize + begin;
    let stop = ((piece * piece_length) as usize + end).min(body.len().saturating_sub(1));
    if start > stop {
        return respond(stream, 416, "Range Not Satisfiable", None, b"").await;
    }
    let slice = body[start..=stop].to_vec();
    served.fetch_add(slice.len() as u64, Ordering::Relaxed);
    respond(stream, 200, "OK", None, &slice).await
}

fn parse_range(spec: &str) -> Option<(usize, usize)> {
    let (start, end) = spec.trim().split_once('-')?;
    Some((start.parse().ok()?, end.parse().unwrap_or(usize::MAX)))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn respond(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    content_range: Option<&str>,
    body: &[u8],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {code} {reason}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(range) = content_range {
        head.push_str(&format!("Content-Range: {range}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

/// A session with no discovery at all, so the only source is the bridge.
async fn engine(download_dir: &Path) -> Engine {
    Engine::start(&EngineOptions {
        download_directory: download_dir.to_path_buf(),
        // Port zero means the OS chooses, so tests never collide. Binding
        // loopback rather than the wildcard address keeps the whole test to
        // this machine, and stops a host firewall asking about every fresh
        // test binary.
        listen_ports: 0..=0,
        listen_ip: Some(Ipv4Addr::LOCALHOST.into()),
        enable_dht: false,
        enable_lsd: false,
        enable_trackers: false,
        enable_peers: false,
        ..Default::default()
    })
    .await
    .unwrap()
}

/// A torrent built from `source`, as `.torrent` bytes written to `path`.
///
/// This uses `bit-cli`'s own creator rather than `librqbit`'s. `librqbit`
/// 9.0.0's `create_torrent` appends one extra piece hash when the payload is
/// an exact multiple of the piece length, because its final flush tests
/// `remaining_piece_length > 0` after resetting that counter to a full piece.
/// Fixtures built with it are rejected by any client that checks the piece
/// count, this one included. `TODO/create-seed.md` records the upstream
/// defect.
async fn make_torrent(source: &Path, path: &Path) -> Vec<u8> {
    make_torrent_with(source, path, PIECE_LENGTH).await
}

/// [`make_torrent`] at a piece length the caller chooses.
///
/// Every other fixture here uses a power of two, which only ever exercises the
/// easy case of the last-block arithmetic. See `TODO/metainfo.md`, T-174.
async fn make_torrent_with(source: &Path, path: &Path, piece_length: u32) -> Vec<u8> {
    use bit_cli_core::torrent::create::{CreateOptions, InputFile, create};

    let mut files = Vec::new();
    let (name, multi_file) = match source.is_dir() {
        false => {
            let name = source.file_name().unwrap().to_string_lossy().into_owned();
            files.push(InputFile {
                source: source.to_path_buf(),
                path: name.clone(),
                length: std::fs::metadata(source).unwrap().len(),
            });
            (name, false)
        }
        true => {
            for entry in walk(source) {
                let relative = entry
                    .strip_prefix(source)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                files.push(InputFile {
                    length: std::fs::metadata(&entry).unwrap().len(),
                    source: entry,
                    path: relative,
                });
            }
            (
                source.file_name().unwrap().to_string_lossy().into_owned(),
                true,
            )
        }
    };

    let created = create(
        files,
        &CreateOptions {
            name,
            multi_file,
            piece_length: Some(piece_length),
            creation_date: None,
            // A piece length that is not a power of two is refused on the
            // writing side, correctly: BEP 52 requires one and the v1
            // convention is one too. This fixture is about the **reading**
            // side, where a torrent somebody else wrote turns up with an odd
            // piece length and has to be handled rather than refused. See
            // `TODO/metainfo.md`, T-174.
            allowed_lints: std::collections::BTreeSet::from([
                bit_cli_core::torrent::Lint::PieceLengthNotPowerOfTwo,
            ]),
            ..Default::default()
        },
        |path| {
            std::fs::File::open(path).map_err(|e| {
                bit_cli_core::error::from_io(e, format!("cannot open {}", path.display()))
            })
        },
    )
    .unwrap();
    std::fs::write(path, &created.bytes).unwrap();
    created.bytes
}

/// Every file under `root`, sorted, so fixtures are deterministic.
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for entry in entries {
            match entry.is_dir() {
                true => stack.push(entry),
                false => out.push(entry),
            }
        }
    }
    out.sort();
    out
}

/// Everything one attached run needs to be inspected afterwards.
struct Attached {
    engine: Engine,
    handle: bit_cli_core::engine::Handle,
    statuses: Vec<Arc<BridgeStatus>>,
    fetchers: Vec<Arc<Fetcher>>,
    layout: Arc<Layout>,
    /// Every block every source served, keyed by block. See
    /// `TODO/webseed.md`, T-179.
    ledger: Arc<BlockLedger>,
}

impl Attached {
    fn finished(&self) -> bool {
        self.handle.stats().finished
    }

    /// Attach one more source to a run that has already started.
    ///
    /// The same shape `swarm::attach_late` has, for the tests that need a
    /// source to arrive after another one has done some work. It takes the run's
    /// own ledger, so a late source is judged on the same evidence, and it
    /// numbers the binding itself: a set resolved on its own is always index
    /// zero, and the ledger is keyed on the index. See
    /// `TODO/multi-source.md`, T-143.
    async fn attach_more(&mut self, spec: SourceSpec) {
        let info_hash = self.handle.info_hash().as_string();
        let mut set = BindingSet::resolve(&self.layout, &info_hash, &[spec]).unwrap();
        bit_cli_core::webseed::probe::resolve_auto_styles(&mut set, &info_hash).await;
        let mut binding = set.bindings.into_iter().next().unwrap();
        binding.index = self.statuses.len();
        let params = BridgeParams::for_binding(
            self.engine.bridge_target().unwrap(),
            self.handle.info_hash(),
            self.handle.shared().peer_id,
            &self.layout,
            &binding,
            4,
        )
        .with_ledger(self.ledger.clone());
        let fetcher = Arc::new(
            Fetcher::new(binding.clone(), self.layout.clone(), info_hash, 4, false).unwrap(),
        );
        self.fetchers.push(fetcher.clone());
        let status = Arc::new(BridgeStatus::default());
        self.statuses.push(status.clone());
        tokio::spawn(bridge::run(params, fetcher, status));
    }

    /// Resolve the ledger against what the session has verified, reading the
    /// correct bytes back out of the payload on disk.
    ///
    /// This is what `download`'s watch loop does once a tick, written out here
    /// so the test drives the same two calls rather than a copy of them.
    fn resolve(&self, out: &Path) -> Vec<bit_cli_core::webseed::ledger::Conviction> {
        let Some(have) = self.engine.have_pieces(&self.handle) else {
            return Vec::new();
        };
        let paths: Vec<String> = self
            .layout
            .files
            .iter()
            .map(bit_cli_core::layout::LayoutFile::display_path)
            .collect();
        let planned = bit_cli_core::paths::plan(&paths);
        let root = bit_cli_core::storage::payload_root(out, &self.layout);
        let convicted = self.ledger.resolve(&have, |offset, length| {
            bit_cli_core::storage::read_range(
                &root,
                &self.layout,
                &planned.disk_paths,
                offset..offset + u64::from(length),
            )
        });
        self.ledger.forget_settled(&have);
        for conviction in &convicted {
            self.fetchers[conviction.source]
                .stats()
                .ban(conviction.to_string());
        }
        convicted
    }

    fn served(&self) -> u64 {
        self.statuses.iter().map(|s| s.served_bytes()).sum()
    }

    fn failed(&self) -> bool {
        self.statuses
            .iter()
            .any(|s| s.state() == BridgeState::Failed)
    }

    fn reasons(&self) -> Vec<String> {
        self.statuses.iter().filter_map(|s| s.error()).collect()
    }

    /// Retries across every source, and what status each was spent on.
    fn retries_by_status(&self) -> std::collections::BTreeMap<u16, u64> {
        let mut total = std::collections::BTreeMap::new();
        for fetcher in &self.fetchers {
            for (code, count) in fetcher.stats().retries_by_status() {
                *total.entry(code).or_default() += count;
            }
        }
        total
    }
}

/// Build a torrent from `source`, add it to a fresh session downloading into
/// `download_dir`, and attach one bridge per spec.
async fn attach(
    source: &Path,
    download_dir: &Path,
    torrent_dir: &Path,
    specs: Vec<SourceSpec>,
) -> Attached {
    attach_with(source, download_dir, torrent_dir, specs, PIECE_LENGTH).await
}

/// [`attach`] at a piece length the caller chooses.
async fn attach_with(
    source: &Path,
    download_dir: &Path,
    torrent_dir: &Path,
    specs: Vec<SourceSpec>,
    piece_length: u32,
) -> Attached {
    attach_selected(source, download_dir, torrent_dir, specs, piece_length, None).await
}

/// [`attach_with`] downloading only the named file indices.
async fn attach_selected(
    source: &Path,
    download_dir: &Path,
    torrent_dir: &Path,
    specs: Vec<SourceSpec>,
    piece_length: u32,
    only_files: Option<Vec<usize>>,
) -> Attached {
    let torrent_path = torrent_dir.join("fixture.torrent");
    make_torrent_with(source, &torrent_path, piece_length).await;

    let engine = engine(download_dir).await;
    let handle = engine
        .add(
            torrent_path.to_str().unwrap(),
            &AddOptions {
                overwrite: true,
                only_files,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    engine.wait_until_initialized(&handle).await.unwrap();

    let layout = Arc::new(engine.layout(&handle).unwrap());
    let info_hash = handle.info_hash().as_string();
    let set = BindingSet::resolve(&layout, &info_hash, &specs).unwrap();
    let target = engine.bridge_target().unwrap();
    let peer_id = handle.shared().peer_id;

    // The same step `swarm::attach_sources` takes: decide the wire style before
    // the first real request, so a command-line source left at `auto` is
    // addressed the way the server expects. See `TODO/webseed.md`, T-004.
    let mut set = set;
    bit_cli_core::webseed::probe::resolve_auto_styles(&mut set, &info_hash).await;
    let set = set;

    let mut statuses = Vec::new();
    let mut fetchers = Vec::new();
    let ledger = Arc::new(BlockLedger::new(layout.piece_length));
    for binding in &set.bindings {
        let params =
            BridgeParams::for_binding(target, handle.info_hash(), peer_id, &layout, binding, 4)
                .with_ledger(ledger.clone());
        let fetcher = Arc::new(
            Fetcher::new(binding.clone(), layout.clone(), info_hash.clone(), 4, false).unwrap(),
        );
        fetchers.push(fetcher.clone());
        let status = Arc::new(BridgeStatus::default());
        statuses.push(status.clone());
        tokio::spawn(bridge::run(params, fetcher, status));
    }

    Attached {
        engine,
        handle,
        statuses,
        fetchers,
        layout,
        ledger,
    }
}

/// Poll until `check` passes or the timeout expires.
async fn wait_for(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    check()
}

/// A source serving the whole torrent from `base`.
fn whole(base: &str) -> SourceSpec {
    SourceSpec::new(base, Origin::CommandLine)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_single_file_torrent_downloads_from_a_web_seed_alone() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(300 * 1024, 7);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "did not complete from HTTP alone: {:?}",
        run.reasons()
    );
    assert_eq!(std::fs::read(out.path().join("movie.bin")).unwrap(), data);
    assert!(
        run.served() > 0,
        "the source should have served the payload"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_multi_file_torrent_downloads_across_file_boundaries() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = src.path().join("album");
    std::fs::create_dir_all(root.join("disc 1")).unwrap();
    // Sizes chosen so pieces straddle both file boundaries.
    let a = content(50 * 1024, 1);
    let b = content(40 * 1024, 2);
    let c = content(60 * 1024, 3);
    std::fs::write(root.join("disc 1").join("one.bin"), &a).unwrap();
    std::fs::write(root.join("disc 1").join("two.bin"), &b).unwrap();
    std::fs::write(root.join("three.bin"), &c).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(&root, out.path(), tmp.path(), vec![whole(&base)]).await;

    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );
    let got = out.path().join("album");
    assert_eq!(
        std::fs::read(got.join("disc 1").join("one.bin")).unwrap(),
        a
    );
    assert_eq!(
        std::fs::read(got.join("disc 1").join("two.bin")).unwrap(),
        b
    );
    assert_eq!(std::fs::read(got.join("three.bin")).unwrap(), c);
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_ignores_range_fails_the_source_instead_of_serving_wrong_bytes() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    // Larger than one window, so at least one request is a real sub-range and
    // a 200 response is unambiguously wrong.
    let data = content(300 * 1024, 11);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::IgnoreRange).await;
    let mut spec = whole(&base);
    spec.limits.chunk_size = 64 * 1024;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![spec],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(30), || run.failed()).await,
        "a server that ignores Range has to fail the source"
    );
    assert!(!run.finished(), "nothing should have completed");
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_file_fails_the_source() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(100 * 1024, 13);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::NotFound).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(30), || run.failed()).await,
        "404 has to fail the source"
    );
    let reasons = run.reasons().join(" ");
    assert!(
        reasons.contains("404"),
        "the reason should name the status: {reasons}"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_payload_is_never_fetched_twice_over() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(300 * 1024, 17);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, served) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );
    let bytes = served.load(Ordering::Relaxed);
    // Some slack for a window that overlaps the tail of the file, but nothing
    // close to a second full pass.
    assert!(
        bytes < (data.len() as u64) * 3 / 2,
        "fetched {bytes} bytes for a {} byte payload",
        data.len()
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connected_source_reports_active_before_it_is_asked_for_anything() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(64 * 1024, 19);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(30), || {
            run.statuses[0].state() == BridgeState::Active
        })
        .await,
        "a connected and unchoked source is available whether or not it is being asked for anything"
    );
    assert!(
        run.statuses[0].local_port().is_some(),
        "an active bridge has a loopback port"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corrupt_data_never_completes_the_torrent() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 23);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Corrupt).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;

    // Nothing verifies, so nothing completes. The bridge does not hash-check;
    // the session does, which is exactly how a lying peer is handled.
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(
        !run.finished(),
        "a source serving wrong bytes must never complete a torrent"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_scoped_sources_cover_a_torrent_between_them() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    // Ten pieces of 32 KiB.
    let data = content(320 * 1024, 29);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let front = whole(&base).with_scope(Scope::parse("piece:0-4").unwrap());
    let back = whole(&base).with_scope(Scope::parse("piece:5-").unwrap());
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![front, back],
    )
    .await;

    assert_eq!(run.layout.piece_count(), 10);
    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "two partial sources should cover the payload between them: {:?}",
        run.reasons()
    );
    assert_eq!(std::fs::read(out.path().join("movie.bin")).unwrap(), data);
    // Both sources did work, which is what proves the pieces were split rather
    // than one source quietly serving everything.
    for status in &run.statuses {
        assert!(
            status.served_bytes() > 0,
            "both sources should have served something"
        );
    }
    run.engine.stop().await;
}

/// The acceptance for `TODO/webseed.md` T-179.
///
/// Two mirrors of one payload, one of which lies once about every range it is
/// asked for. A failed piece names neither of them on the wire; the ledger
/// names the one that lied, and only that one.
///
/// The healthy mirror still being usable at the end is the assertion that
/// matters most: retiring both is the failure this entry exists to prevent,
/// and "the piece failed, so blame everyone who touched it" would pass every
/// other check here.
///
/// **Both mirrors serving is arranged rather than hoped for**, and the first
/// shape of this test hoped. `librqbit`'s `piece_tracker.rs:114` assigns a
/// piece to one peer at a time unless another steals it, so which mirror gets
/// work is a scheduling outcome. Attached together against a 640 KiB payload
/// on loopback, the first bridge to connect can finish the whole thing before
/// the second bridge's task is scheduled at all, and then the liar serves
/// nothing, no piece fails, and the run waits out its timeout. That happened
/// twice on 2026-08-22 under whole-suite load, with `served [655360, 0]` and
/// no bridge error, and reran clean twenty times on an idle machine, which is
/// what a fixture that depends on a race looks like.
///
/// So the liar goes first, scoped to half the payload, and the healthy mirror
/// joins once the liar has served something. Every assertion below is then
/// structural: the liar has served by construction, the healthy mirror is the
/// only source of pieces 10 to 19 so it has too, and neither can be starved by
/// the other. See `TODO/webseed.md`, T-179.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_mirror_that_serves_wrong_bytes_is_named_and_the_healthy_one_is_not() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    // Twenty pieces of 32 KiB, so a piece is filled from several blocks and
    // the two mirrors have plenty to divide between them.
    let data = content(640 * 1024, 37);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (honest, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let (liar, _) = serve(src.path().to_path_buf(), ServeMode::CorruptOnce).await;
    // Source 0 is the liar, alone, and it holds half the payload. It cannot
    // finish the torrent by itself, which is what leaves work for the mirror
    // that joins next.
    let mut run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&liar).with_scope(Scope::parse("piece:0-9").unwrap())],
    )
    .await;
    assert_eq!(run.layout.piece_count(), 20);

    // Waiting on the condition. The liar has contributed when it has served a
    // byte, whenever that is.
    assert!(
        wait_for(Duration::from_secs(60), || run.statuses[0].served_bytes()
            > 0)
        .await,
        "the liar never served anything: {:?}",
        run.reasons()
    );
    // Source 1 is the healthy mirror, and it covers everything, so it can
    // finish the torrent once the liar is retired.
    run.attach_more(whole(&honest)).await;

    // The ledger is resolved on a tick, exactly as `download`'s watch loop
    // does it. Waiting on the conviction rather than on a duration: the run
    // is over when the liar has been named, whenever that is.
    let convicted: Arc<std::sync::Mutex<Vec<bit_cli_core::webseed::ledger::Conviction>>> =
        Arc::default();
    let named = {
        let convicted = convicted.clone();
        wait_for(Duration::from_secs(120), || {
            let mut found = convicted.lock().unwrap();
            found.extend(run.resolve(out.path()));
            !found.is_empty()
        })
        .await
    };
    let convicted = convicted.lock().unwrap().clone();
    assert!(
        named,
        "the mirror that served wrong bytes was never named: served {:?}, reasons {:?}",
        run.statuses
            .iter()
            .map(|s| s.served_bytes())
            .collect::<Vec<_>>(),
        run.reasons()
    );

    // Source 0 is the liar. Every conviction has to be against it, because a
    // conviction against source 1 is a healthy mirror retired for someone
    // else's bytes.
    let guilty: std::collections::BTreeSet<usize> = convicted.iter().map(|c| c.source).collect();
    assert_eq!(
        guilty,
        std::collections::BTreeSet::from([0]),
        "only the mirror that lied should be convicted: {convicted:?}"
    );
    for conviction in &convicted {
        assert_ne!(
            conviction.served, conviction.correct,
            "a conviction records two hashes that differ"
        );
        assert!(conviction.piece < 20, "{conviction:?}");
    }

    // Both mirrors served, which is what makes this the case the entry is
    // about: pieces filled from two sources rather than one. With one of them
    // idle, "blame whoever filled it" would give the same answer as
    // attribution does. Structural here rather than measured: the liar was
    // waited on above, and the healthy mirror is the only source of pieces 10
    // to 19.
    assert!(
        wait_for(Duration::from_secs(60), || run.statuses[1].served_bytes()
            > 0)
        .await,
        "the healthy mirror served nothing, so no piece was split: {:?}",
        run.reasons()
    );

    // The liar is retired and the honest mirror is not.
    assert!(
        wait_for(Duration::from_secs(30), || {
            run.statuses[0].state() == BridgeState::Failed
        })
        .await,
        "a convicted mirror is retired: {:?}",
        run.statuses[0].state()
    );
    assert_ne!(
        run.statuses[1].state(),
        BridgeState::Failed,
        "the healthy mirror must survive its neighbour lying: {:?}",
        run.statuses[1].error()
    );

    // And the payload still arrives, from the mirror that was telling the
    // truth all along.
    assert!(
        wait_for(Duration::from_secs(120), || {
            run.resolve(out.path());
            run.finished()
        })
        .await,
        "the honest mirror should finish the torrent on its own: {:?}",
        run.reasons()
    );
    assert_eq!(std::fs::read(out.path().join("movie.bin")).unwrap(), data);
    run.engine.stop().await;
}

/// The other half of T-179, and the one a wrong implementation passes the
/// first half of: two honest mirrors filling the same pieces convict nobody.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_honest_mirrors_filling_one_payload_convict_nobody() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(320 * 1024, 41);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (one, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let (two, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&one), whole(&two)],
    )
    .await;

    let mut convicted = Vec::new();
    let done = wait_for(Duration::from_secs(120), || {
        convicted.extend(run.resolve(out.path()));
        run.finished()
    })
    .await;
    convicted.extend(run.resolve(out.path()));
    assert!(
        done,
        "two mirrors should finish a torrent: {:?}",
        run.reasons()
    );
    assert!(
        convicted.is_empty(),
        "two mirrors serving the same correct bytes convict nobody: {convicted:?}"
    );
    for status in &run.statuses {
        assert_ne!(status.state(), BridgeState::Failed, "{:?}", status.error());
    }
    // The ledger recorded work and let it go again, rather than doing nothing
    // at all: a ledger that records nothing also convicts nobody.
    let stats = run.ledger.stats();
    assert!(stats.recorded > 0, "{stats:?}");
    assert_eq!(stats.evicted, 0, "nothing should be evicted: {stats:?}");
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_partial_source_never_completes_a_torrent_on_its_own() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(320 * 1024, 31);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, served) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let half = whole(&base).with_scope(Scope::parse("piece:0-4").unwrap());
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![half],
    )
    .await;

    // The source announces five of ten pieces, so the session can never
    // finish, and it must never ask for a piece outside that set.
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(!run.finished());
    assert!(
        !run.failed(),
        "a partial source is not a broken one: {:?}",
        run.reasons()
    );
    let bytes = served.load(Ordering::Relaxed);
    assert!(bytes > 0, "the in-scope half should still have been served");
    assert!(
        bytes <= (data.len() as u64) / 2 + PIECE_LENGTH as u64,
        "served {bytes} bytes, which is more than the scope allows"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bep_17_source_downloads_a_torrent() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 37);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, served) = serve(src.path().to_path_buf(), ServeMode::Hoffman).await;
    // BEP 17 addresses the torrent, not a file, so the URL is the base with
    // nothing appended.
    let mut spec = SourceSpec::new(format!("{base}movie.bin"), Origin::TorrentHttpSeeds);
    spec.style = bit_cli_core::webseed::Style::Hoffman;
    spec.mode = bit_cli_core::webseed::Mode::Exact;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![spec],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "a BEP 17 source should complete a torrent: {:?}",
        run.reasons()
    );
    assert_eq!(std::fs::read(out.path().join("movie.bin")).unwrap(), data);
    assert!(
        served.load(Ordering::Relaxed) > 0,
        "the BEP 17 path served nothing"
    );
    run.engine.stop().await;
}

/// `TODO/webseed.md` T-004. A BEP 17 seed named on the command line, with no
/// `--web-seed-style`, is detected and downloads.
///
/// This is the only case the entry left: a source out of `httpseeds` is keyed
/// BEP 17 by the key it came from and one out of `url-list` is keyed BEP 19,
/// which is what both BEPs specify and costs no request. A command-line URL
/// has no key to read, so it is asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_command_line_bep_17_source_is_detected_without_the_flag() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 61);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, served) = serve(src.path().to_path_buf(), ServeMode::Hoffman).await;
    let mut spec = SourceSpec::new(format!("{base}movie.bin"), Origin::CommandLine);
    spec.mode = bit_cli_core::webseed::Mode::Exact;
    assert_eq!(
        spec.style,
        bit_cli_core::webseed::Style::Auto,
        "the point of this test is that nothing declared a style"
    );

    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![spec],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "a detected BEP 17 source should complete a torrent: {:?}",
        run.reasons()
    );
    assert_eq!(std::fs::read(out.path().join("movie.bin")).unwrap(), data);
    assert!(served.load(Ordering::Relaxed) > 0);
    run.engine.stop().await;
}

/// The other half, and the one a detector biased towards BEP 17 would break:
/// an ordinary GetRight mirror left at `auto` stays GetRight.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_command_line_getright_source_is_not_mistaken_for_bep_17() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 67);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _served) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "a GetRight mirror should still complete: {:?}",
        run.reasons()
    );
    assert_eq!(std::fs::read(out.path().join("movie.bin")).unwrap(), data);
    run.engine.stop().await;
}

/// The detector on its own, against both server kinds.
///
/// The GetRight stub answers the BEP 17 probe with the whole entity, because a
/// server that does not understand a query parameter serves the resource
/// anyway. So the discriminator is the **length** of the answer and not its
/// status, which is the part a stub that 404s on any query would not have
/// tested.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_style_probe_tells_a_hoffman_seed_from_a_getright_one() {
    let src = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 71);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (hoffman_base, _) = serve(src.path().to_path_buf(), ServeMode::Hoffman).await;
    let (getright_base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;

    let layout = Arc::new(Layout::from_lengths(
        "movie.bin",
        false,
        PIECE_LENGTH,
        [("movie.bin".to_string(), data.len() as u64)],
    ));
    let hash = "0".repeat(40);

    for (base, expected) in [(hoffman_base, true), (getright_base, false)] {
        let mut spec = SourceSpec::new(format!("{base}movie.bin"), Origin::CommandLine);
        spec.mode = bit_cli_core::webseed::Mode::Exact;
        let set = BindingSet::resolve(&layout, &hash, &[spec]).unwrap();
        let answer = bit_cli_core::webseed::probe::speaks_hoffman(&set.bindings[0], &hash)
            .await
            .unwrap();
        assert_eq!(answer, expected, "{base}");
    }
}

/// The whole style pass is bounded, so one unreachable mirror cannot hold up
/// the reachable ones.
///
/// Everything waits on this pass: no bridge starts serving until every style
/// is decided. A source that does not answer keeps BEP 19, which is what
/// `auto` did before the probe existed, so the probe can never cost more than
/// the answer it replaces. The assertion is on the clock as well as the
/// answer, because the point is the delay and not the fallback.
/// See `TODO/webseed.md`, T-004.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_unreachable_mirror_does_not_hold_up_the_others() {
    let src = tempfile::tempdir().unwrap();
    let data = content(64 * 1024, 73);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();
    let (reachable, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;

    let layout = Arc::new(Layout::from_lengths(
        "movie.bin",
        false,
        PIECE_LENGTH,
        [("movie.bin".to_string(), data.len() as u64)],
    ));
    let hash = "0".repeat(40);

    // 203.0.113.0/24 is TEST-NET-3 and is not routed, so a connect to it hangs
    // rather than being refused. That is the case a refused port does not
    // reach: `127.0.0.1:9` answers instantly and would prove nothing here.
    let mut dead = SourceSpec::new("http://203.0.113.1/movie.bin", Origin::CommandLine);
    dead.mode = bit_cli_core::webseed::Mode::Exact;
    dead.limits.connect_timeout_ms = 30_000;
    dead.limits.timeout_ms = 30_000;
    let mut live = SourceSpec::new(format!("{reachable}movie.bin"), Origin::CommandLine);
    live.mode = bit_cli_core::webseed::Mode::Exact;

    let mut set = BindingSet::resolve(&layout, &hash, &[dead, live]).unwrap();
    let started = std::time::Instant::now();
    let decisions = bit_cli_core::webseed::probe::resolve_auto_styles(&mut set, &hash).await;
    let elapsed = started.elapsed();

    assert_eq!(decisions.len(), 2);
    assert!(
        elapsed < Duration::from_secs(10),
        "a 30 second connect timeout must not become a 30 second wait: {elapsed:?}"
    );
    // Both end up BEP 19: the reachable one because it answered with the whole
    // entity, the dead one because nothing answered.
    for decision in &decisions {
        assert_eq!(decision.style, bit_cli_core::webseed::Style::GetRight);
    }
    assert!(
        decisions[0].probe_error.is_some(),
        "the source that was cut off says so: {decisions:?}"
    );
    assert!(
        decisions[1].probe_error.is_none(),
        "the source that answered has nothing to report: {decisions:?}"
    );
}

/// A source that cannot be reached at all keeps the answer `auto` gave before
/// the probe existed, rather than being refused over a failed probe.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_that_cannot_be_probed_falls_back_to_getright() {
    let layout = Arc::new(Layout::from_lengths(
        "movie.bin",
        false,
        PIECE_LENGTH,
        [("movie.bin".to_string(), 200 * 1024)],
    ));
    let hash = "0".repeat(40);
    // Port 9 is discard, and nothing listens on it here.
    let mut spec = SourceSpec::new("http://127.0.0.1:9/movie.bin", Origin::CommandLine);
    spec.mode = bit_cli_core::webseed::Mode::Exact;
    spec.limits.connect_timeout_ms = 500;
    spec.limits.timeout_ms = 1000;
    let mut set = BindingSet::resolve(&layout, &hash, &[spec]).unwrap();

    let decisions = bit_cli_core::webseed::probe::resolve_auto_styles(&mut set, &hash).await;
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].style, bit_cli_core::webseed::Style::GetRight);
    assert!(
        decisions[0].probe_error.is_some(),
        "a probe that could not be made says so: {:?}",
        decisions[0]
    );
    assert_eq!(
        set.bindings[0].spec.style,
        bit_cli_core::webseed::Style::GetRight
    );
}

/// A source whose style the caller declared is never probed, and a source from
/// a metainfo key is decided by the key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_style_and_a_metainfo_key_cost_no_request() {
    use bit_cli_core::webseed::probe::StyleSource;

    let layout = Arc::new(Layout::from_lengths(
        "movie.bin",
        false,
        PIECE_LENGTH,
        [("movie.bin".to_string(), 200 * 1024)],
    ));
    let hash = "0".repeat(40);

    // Nothing listens on port 9, so any probe would take a timeout and fail.
    // These resolve instantly and correctly, which is the assertion.
    let mut declared = SourceSpec::new("http://127.0.0.1:9/movie.bin", Origin::CommandLine);
    declared.style = bit_cli_core::webseed::Style::Hoffman;
    let mut from_key = SourceSpec::new("http://127.0.0.1:9/movie.bin", Origin::TorrentUrlList);
    from_key.mode = bit_cli_core::webseed::Mode::Exact;

    let mut set = BindingSet::resolve(&layout, &hash, &[declared, from_key]).unwrap();
    let started = std::time::Instant::now();
    let decisions = bit_cli_core::webseed::probe::resolve_auto_styles(&mut set, &hash).await;
    assert_eq!(decisions.len(), 2);
    assert_eq!(decisions[0].style, bit_cli_core::webseed::Style::Hoffman);
    assert_eq!(decisions[0].decided_by, StyleSource::Declared);
    assert_eq!(decisions[1].style, bit_cli_core::webseed::Style::GetRight);
    assert_eq!(decisions[1].decided_by, StyleSource::MetainfoKey);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "neither source should have been asked anything"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn source_side_verification_names_the_mirror_that_served_a_wrong_piece() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 41);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = Arc::new(meta.layout());
    let hashes = Arc::new(meta.info().pieces.clone());

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Corrupt).await;
    let spec = whole(&base);
    let set = BindingSet::resolve(&layout, &meta.info_hash().hex(), &[spec]).unwrap();
    let fetcher = Fetcher::new(
        set.bindings[0].clone(),
        layout.clone(),
        meta.info_hash().hex(),
        4,
        false,
    )
    .unwrap()
    .with_verification(bit_cli_core::webseed::fetch::Verify::Piece, Some(hashes));

    // The window covers whole pieces, so the mismatch is caught at the source
    // rather than several hops later inside the session.
    let err = fetcher.read(0, 16 * 1024).await.unwrap_err();
    assert_eq!(err.class(), "hash_mismatch", "{err}");
    let text = err.to_string();
    assert!(text.contains(&base), "the mirror has to be named: {text}");
    assert!(
        text.contains("piece 0"),
        "the piece has to be named: {text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn webseed_test_reports_range_support_size_and_the_redirect_chain() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 43);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = meta.layout();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let set = BindingSet::resolve(&layout, &meta.info_hash().hex(), &[whole(&base)]).unwrap();
    let report = bit_cli_core::webseed::probe::test_source(
        &set.bindings[0],
        &layout,
        &meta.info_hash().hex(),
        false,
        &[],
        true,
    )
    .await;

    assert!(report.ok, "{:?}", report.error);
    assert_eq!(report.status, Some(206));
    assert_eq!(
        report.range_support,
        bit_cli_core::webseed::probe::RangeSupport::Yes
    );
    assert_eq!(report.content_length, Some(data.len() as u64));
    assert_eq!(report.length_matches, Some(true));
    assert!(report.redirects.is_empty());
    assert!(report.tls.is_none(), "plain HTTP has no TLS to report");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn webseed_test_follows_and_reports_every_redirect_hop() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(64 * 1024, 47);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = meta.layout();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Redirect).await;
    let set = BindingSet::resolve(&layout, &meta.info_hash().hex(), &[whole(&base)]).unwrap();
    let report = bit_cli_core::webseed::probe::test_source(
        &set.bindings[0],
        &layout,
        &meta.info_hash().hex(),
        false,
        &[],
        true,
    )
    .await;

    assert!(report.ok, "{:?}", report.error);
    assert_eq!(
        report.redirects.len(),
        1,
        "the chain has to be reported hop by hop"
    );
    assert_eq!(report.redirects[0].status, 302);
    assert!(
        report.redirects[0].to.contains("/moved/"),
        "{:?}",
        report.redirects[0]
    );
    assert!(
        report.resolved_url.is_some(),
        "the resolved URL is what to request next"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn webseed_test_says_no_when_the_server_ignores_range() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(64 * 1024, 53);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = meta.layout();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::IgnoreRange).await;
    let set = BindingSet::resolve(&layout, &meta.info_hash().hex(), &[whole(&base)]).unwrap();
    let report = bit_cli_core::webseed::probe::test_source(
        &set.bindings[0],
        &layout,
        &meta.info_hash().hex(),
        false,
        &[],
        true,
    )
    .await;

    assert!(!report.ok);
    assert_eq!(
        report.range_support,
        bit_cli_core::webseed::probe::RangeSupport::No
    );
    assert!(report.error.unwrap().contains("Range"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn webseed_probe_produces_a_concurrency_curve() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(320 * 1024, 59);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = meta.layout();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let mut spec = whole(&base);
    spec.limits.chunk_size = 32 * 1024;
    let set = BindingSet::resolve(&layout, &meta.info_hash().hex(), &[spec]).unwrap();
    let report = bit_cli_core::webseed::probe::probe_source(
        &set.bindings[0],
        &layout,
        &meta.info_hash().hex(),
        &[1, 2],
        Duration::from_millis(600),
    )
    .await;

    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.steps.len(), 2, "one step per concurrency");
    for step in &report.steps {
        assert!(
            step.requests > 0,
            "no requests at concurrency {}",
            step.concurrency
        );
        assert_eq!(step.errors, 0, "step {} had errors", step.concurrency);
        assert!(step.bytes > 0);
        assert!(step.p99_ms >= step.p50_ms, "percentiles have to be ordered");
    }
    assert!(report.best_concurrency.is_some());
    assert!(report.best_throughput > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scope_that_leaves_a_gap_names_the_uncovered_pieces() {
    // No session needed: coverage is decided before any request goes out,
    // which is the point of checking it there.
    let layout = Layout::from_lengths(
        "movie.bin",
        false,
        PIECE_LENGTH,
        [("movie.bin".to_string(), 320 * 1024u64)],
    );
    let spec = whole("https://mirror.example.com/").with_scope(Scope::parse("piece:0-3").unwrap());
    let set = BindingSet::resolve(&layout, &"0".repeat(40), &[spec]).unwrap();

    assert!(!set.is_complete());
    assert_eq!(set.uncovered_pieces, vec![4, 5, 6, 7, 8, 9]);

    // With peers available a gap is fine; without them it is a hard error.
    assert!(set.require_coverage(true).is_ok());
    let err = set.require_coverage(false).unwrap_err();
    assert_eq!(err.code(), bit_cli_core::ExitCode::CoverageGap);
    assert_eq!(
        err.context()["uncovered_pieces"],
        serde_json::json!([4, 5, 6, 7, 8, 9])
    );
}

// -- bench webseed ---------------------------------------------------------
//
// `bench webseed` reads real payload off a real socket and throws it away.
// These drive the whole path against the same loopback server the download
// tests use, so the numbers in a report come from bytes that actually moved.

/// Options for a short bench run: no warmup, a fine sampling interval, and a
/// chunk size small enough that a fraction of a second still issues many
/// requests.
fn bench_options(duration_ms: u64) -> bit_cli_core::bench::webseed::Options {
    bit_cli_core::bench::webseed::Options {
        duration: Duration::from_millis(duration_ms),
        warmup: Duration::ZERO,
        metrics_interval: Duration::from_millis(100),
        concurrency: 4,
        concurrency_sweep: Vec::new(),
        target_rate: None,
        chunk_size: Some(16 * 1024),
    }
}

/// A torrent, a server, and the bindings that join them.
async fn bench_fixture(
    mode: ServeMode,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Layout,
    String,
    BindingSet,
) {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("movie.bin"), content(512 * 1024, 71)).unwrap();

    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = meta.layout();
    let info_hash = meta.info_hash().hex();

    let (base, _) = serve(src.path().to_path_buf(), mode).await;
    let mut spec = whole(&base);
    spec.limits.chunk_size = 16 * 1024;
    let set = BindingSet::resolve(&layout, &info_hash, &[spec]).unwrap();
    (src, tmp, layout, info_hash, set)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_webseed_moves_real_bytes_and_reports_them() {
    let (_src, _tmp, layout, info_hash, set) = bench_fixture(ServeMode::Ranges).await;
    let mut samples = 0usize;
    let outcome =
        bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &bench_options(700), |_| {
            samples += 1
        })
        .await
        .unwrap();

    assert!(
        outcome.summary.bytes.0 > 0,
        "no bytes moved: {:?}",
        outcome.summary
    );
    assert!(outcome.summary.requests > 0);
    // The same invariant, and for the same reason, as the scope test at the
    // bottom of this file: a 700 ms bench against a loopback server on a
    // loaded runner can lose a connection, and what this test is about is that
    // bytes moved and were reported. Taken here **before** it turned a job
    // red, because the one below it did and the two are the same assumption.
    // See `TODO/webseed.md`, T-215.
    assert_eq!(
        outcome.summary.errors.by_class.values().sum::<u64>(),
        outcome.summary.errors.total,
        "an error with no class is an error nobody can act on: {:?}",
        outcome.summary.errors
    );
    assert!(outcome.summary.sustained_rate.0 > 0);
    assert!(outcome.summary.peak_rate.0 > 0);
    assert!(samples > 0, "the time series was never sampled");
    assert_eq!(outcome.series.len(), samples);

    let complete = &outcome.summary.latency.complete;
    assert!(complete.count > 0);
    assert!(complete.p50_ms <= complete.p90_ms);
    assert!(complete.p90_ms <= complete.p99_ms);
    assert!(complete.p99_ms <= complete.max_ms);
    assert!(
        outcome.summary.latency.first_byte.count > 0,
        "first byte latency is not recorded"
    );
    assert!(
        outcome.summary.latency.connect.count > 0,
        "connection establishment is measured on its own cadence"
    );

    assert_eq!(outcome.sources.len(), 1);
    let source = &outcome.sources[0];
    assert_eq!(
        source.range_support,
        bit_cli_core::webseed::probe::RangeSupport::Yes
    );
    assert_eq!(source.summary.bytes, outcome.summary.bytes);
    assert!(source.failure.is_none());
    assert_eq!(outcome.endpoints.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_webseed_series_totals_agree_with_the_summary() {
    let (_src, _tmp, layout, info_hash, set) = bench_fixture(ServeMode::Ranges).await;
    let outcome =
        bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &bench_options(700), |_| {})
            .await
            .unwrap();

    let from_series: u64 = outcome.series.iter().map(|s| s.bytes.0).sum();
    // The series is sampled on the interval and the run stops between ticks,
    // so the last partial interval is in the summary and not yet in a sample.
    assert!(
        from_series <= outcome.summary.bytes.0,
        "the series claims {from_series} bytes but the summary claims {}",
        outcome.summary.bytes.0
    );
    assert!(
        from_series > 0,
        "the series recorded no bytes at all: {:?}",
        outcome.series
    );
    let last = outcome.series.last().unwrap();
    assert_eq!(
        last.cumulative_bytes.0, from_series,
        "the cumulative column is the running total of the interval column"
    );
    for sample in &outcome.series {
        assert!(sample.process.peak_rss_bytes > 0, "no cost was sampled");
        assert!(!sample.warmup, "this run had no warmup window");
    }
}

/// A sweep pays its warmup **before** the curve rather than out of its first
/// steps.
///
/// The recorder excludes warmup samples from a step's byte count, and
/// `end_step` divides by the step's own wall time, so a step that fell inside
/// the warmup reported its real seconds against no bytes. Measured on a 64 MiB
/// loopback payload before the fix: `--duration 6s --concurrency-sweep
/// 1,2,4,8,16` gave 1.2 seconds a step and the first two came out at 0 B/s,
/// and `--concurrency-sweep 16,1` reported `best concurrency 1` because
/// whichever step went first was the one that was crippled.
///
/// **`bench_options` sets `warmup: Duration::ZERO`**, which is why
/// `bench_webseed_reports_a_concurrency_curve_with_its_own_latency` asserts
/// exactly this and has always passed: every other test of the sweep turns off
/// the thing that breaks it. This one turns it on.
///
/// The two steps are the same concurrency on purpose. It is the control: with
/// the warmup paid out of the first step the two disagreed by a factor of 340,
/// and there is nothing about the run that should tell them apart.
/// See `TODO/bench.md`, T-229.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sweep_pays_its_warmup_before_the_curve_rather_than_out_of_it() {
    let (_src, _tmp, layout, info_hash, set) = bench_fixture(ServeMode::Ranges).await;
    let mut options = bench_options(600);
    // Two steps of 300ms against a 500ms warmup. The numbers are chosen so the
    // first step falls **entirely** inside the warmup window: both boundaries
    // are measured from the same `Instant`, so that is arithmetic rather than
    // a race, and it is the shape a default 3 second warmup has against the 8
    // second sweeps this defect was found in.
    options.warmup = Duration::from_millis(500);
    options.concurrency_sweep = vec![4, 4];
    let outcome = bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &options, |_| {})
        .await
        .unwrap();

    assert_eq!(outcome.concurrency_curve.len(), 2);
    for (index, step) in outcome.concurrency_curve.iter().enumerate() {
        assert!(
            step.requests > 0,
            "step {index} at concurrency {} issued no request, so the warmup was charged to it",
            step.concurrency
        );
        assert!(step.bytes.0 > 0, "step {index} moved no bytes");
    }

    // The steps claim no more than the run measured. That much is guaranteed
    // by construction: a step's bytes are a subset of the window's.
    let total: u64 = outcome.concurrency_curve.iter().map(|s| s.bytes.0).sum();
    assert!(
        total <= outcome.summary.bytes.0,
        "the steps claim more than the run measured"
    );

    // What is deliberately **not** asserted here, and why it was.
    //
    // This bounded the bytes that fall outside every step at
    // `concurrency * chunk_size`, on the reasoning that at most `concurrency`
    // requests of one chunk each can be in flight when the warmup closes.
    // That reasoning is wrong about the code it describes. The warmup is
    // driven by `while recorder.in_warmup() { drive(..) }`, **every iteration
    // spawns `concurrency` fresh workers**, and a worker that passes its
    // deadline check starts one more request and finishes it after the
    // deadline. So the tail is `concurrency` per iteration and the iteration
    // count is whatever the clock and the machine make it.
    //
    // It went red on `macos-latest` at seven chunks against a bound of four,
    // on a loaded shared runner and not on a defect. That is the fourth entry
    // on `TODO/RULES.md` section 5's line about a test asserting that the
    // machine cannot fail some other way, and the rule is to fix the file
    // rather than the line. See `TODO/bench.md`, T-229.
    //
    // The claim the entry is actually about is asserted instead. Against the
    // original defect it is `step.requests > 0` above that fires first, and
    // the control below is what covers a partial version, where a step falls
    // inside part of the warmup and is charged some of it rather than all.

    // The control. Two steps at the same concurrency against the same server,
    // so nothing about the run should tell them apart; with the warmup paid
    // out of the first step they differed by a factor of 340. An order of
    // magnitude is a wide bound on purpose: this runs on a shared runner and
    // the claim is "neither step was crippled", not "the two are identical".
    let first = outcome.concurrency_curve[0].bytes.0;
    let second = outcome.concurrency_curve[1].bytes.0;
    let (low, high) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    assert!(
        low > 0 && high <= low.saturating_mul(10),
        "two steps at the same concurrency moved {first} and {second} bytes,          which is over an order of magnitude apart: the warmup is being charged to one of them"
    );

    // The warmup was still recorded rather than deleted, which is the rule the
    // recorder's own module comment gives.
    assert!(
        outcome.series.iter().any(|sample| sample.warmup),
        "a run with a warmup window records the samples inside it"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_webseed_reports_a_concurrency_curve_with_its_own_latency() {
    let (_src, _tmp, layout, info_hash, set) = bench_fixture(ServeMode::Ranges).await;
    let mut options = bench_options(900);
    options.concurrency_sweep = vec![1, 4];
    let outcome = bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &options, |_| {})
        .await
        .unwrap();

    assert_eq!(outcome.concurrency_curve.len(), 2);
    for step in &outcome.concurrency_curve {
        assert!(
            step.requests > 0,
            "concurrency {} issued no request",
            step.concurrency
        );
        assert!(step.bytes.0 > 0);
        assert!(
            step.latency.complete.count > 0,
            "a step carries its own latency, which is what makes a knee visible"
        );
        assert!(step.latency.complete.p99_ms >= step.latency.complete.p50_ms);
    }
    assert_eq!(outcome.concurrency_curve[0].concurrency, 1);
    assert_eq!(outcome.concurrency_curve[1].concurrency, 4);
    assert!(outcome.summary.best_concurrency.is_some());
    let total: u64 = outcome.concurrency_curve.iter().map(|s| s.bytes.0).sum();
    assert_eq!(
        total, outcome.summary.bytes.0,
        "the steps add up to the run"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_webseed_names_a_server_that_ignores_range() {
    let (_src, _tmp, layout, info_hash, set) = bench_fixture(ServeMode::IgnoreRange).await;
    let outcome =
        bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &bench_options(500), |_| {})
            .await
            .unwrap();

    assert_eq!(
        outcome.summary.bytes.0, 0,
        "a server that ignores Range serves no usable byte"
    );
    assert!(outcome.summary.errors.total > 0);
    // Every response that came back is a range-ignored one, and every error
    // carries a class. Not "range_ignored is the only class there is": under a
    // 500 ms burst at concurrency 4 a loaded runner refuses or resets some
    // connections before they reach the range check, and those are transport
    // errors rather than this server's answer. Asserting the stricter thing
    // turned `Test (macos-latest)` red on a documentation-only commit with
    // 1,828 of 7,557. See TODO/webseed.md, T-162.
    let ignored = outcome
        .summary
        .errors
        .by_class
        .get("range_ignored")
        .copied()
        .unwrap_or(0);
    assert!(ignored > 0, "{:?}", outcome.summary.errors.by_class);
    assert_eq!(
        outcome.summary.errors.by_status.get("200").copied(),
        Some(ignored),
        "every 200 that arrived is what range_ignored counts: {:?}",
        outcome.summary.errors.by_status
    );
    assert_eq!(
        outcome.summary.errors.by_class.values().sum::<u64>(),
        outcome.summary.errors.total,
        "an error with no class is an error nobody can act on: {:?}",
        outcome.summary.errors.by_class
    );
    assert_eq!(
        outcome.sources[0].range_support,
        bit_cli_core::webseed::probe::RangeSupport::No
    );
    assert!(
        outcome
            .notes
            .iter()
            .any(|note| note.contains("does not honour Range")),
        "{:?}",
        outcome.notes
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_webseed_counts_a_404_by_class_and_by_status() {
    let (_src, _tmp, layout, info_hash, set) = bench_fixture(ServeMode::NotFound).await;
    let outcome =
        bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &bench_options(500), |_| {})
            .await
            .unwrap();

    assert_eq!(outcome.summary.bytes.0, 0);
    assert!(outcome.summary.errors.total > 0);
    // Same shape as the range test above and the same reason: what is asserted
    // is that a 404 is classified and counted, not that a loaded runner is
    // incapable of also resetting a connection. See TODO/webseed.md, T-162.
    let missing = outcome
        .summary
        .errors
        .by_class
        .get("not_found")
        .copied()
        .unwrap_or(0);
    assert!(missing > 0, "{:?}", outcome.summary.errors.by_class);
    assert_eq!(
        outcome.summary.errors.by_status.get("404").copied(),
        Some(missing),
        "every 404 that arrived is what not_found counts: {:?}",
        outcome.summary.errors.by_status
    );
    assert_eq!(
        outcome.summary.errors.by_class.values().sum::<u64>(),
        outcome.summary.errors.total,
        "an error with no class is an error nobody can act on: {:?}",
        outcome.summary.errors.by_class
    );
    assert!(
        outcome.summary.latency.complete.count > 0,
        "the timing of a failing request is still a measurement"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_webseed_honours_a_target_rate() {
    let (_src, _tmp, layout, info_hash, set) = bench_fixture(ServeMode::Ranges).await;
    let mut options = bench_options(1500);
    // Loopback serves far faster than this, so the pacer has to hold it down
    // or the flag does nothing.
    options.target_rate = Some(64 * 1024);
    let outcome = bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &options, |_| {})
        .await
        .unwrap();

    assert!(outcome.summary.bytes.0 > 0);
    assert!(
        outcome.summary.sustained_rate.0 <= 4 * 64 * 1024,
        "asked for 64 KiB/s and got {} B/s",
        outcome.summary.sustained_rate.0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_webseed_measures_only_what_a_scope_covers() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(src.path().join("album")).unwrap();
    for (name, seed) in [("a.bin", 11u64), ("b.bin", 12)] {
        std::fs::write(
            src.path().join("album").join(name),
            content(256 * 1024, seed),
        )
        .unwrap();
    }
    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("album"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = meta.layout();
    let info_hash = meta.info_hash().hex();

    let (base, served) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let mut spec = whole(&base).with_scope(Scope::parse("0").unwrap());
    spec.limits.chunk_size = 16 * 1024;
    let set = BindingSet::resolve(&layout, &info_hash, &[spec]).unwrap();

    let outcome =
        bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &bench_options(600), |_| {})
            .await
            .unwrap();

    assert!(outcome.summary.bytes.0 > 0);
    assert!(served.load(Ordering::Relaxed) > 0);

    // **What this test is about is the scope**, so that is what it asserts:
    // every endpoint the bench read is file 0, and none of them is file 1.
    // Both directions, because a run that read nothing would satisfy the first
    // alone.
    assert!(!outcome.endpoints.is_empty(), "nothing was read at all");
    for endpoint in &outcome.endpoints {
        assert!(
            endpoint.ends_with("a.bin"),
            "a scope of file 0 reads file 0: {endpoint}"
        );
        assert!(
            !endpoint.contains("b.bin"),
            "a scope of file 0 read file 1: {endpoint}"
        );
    }

    // It used to assert `errors.total == 0` here, which is a claim about the
    // machine rather than about the scope: a 600 ms bench against a loopback
    // server on a loaded runner can lose a connection, and one did, on CI run
    // 32626337016, `Test (windows-latest)`. What holds whatever the runner
    // does is that an error which happened is one a reader can act on. That is
    // the shape [T-162] settled for the two tests above this one and this is
    // the third that needed it. See `TODO/webseed.md`, T-162 and T-215.
    //
    // [T-162]: `TODO/webseed.md`
    assert_eq!(
        outcome.summary.errors.by_class.values().sum::<u64>(),
        outcome.summary.errors.total,
        "an error with no class is an error nobody can act on: {:?}",
        outcome.summary.errors
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_webseed_keeps_a_broken_mirror_apart_from_a_healthy_one() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("movie.bin"), content(256 * 1024, 83)).unwrap();
    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = meta.layout();
    let info_hash = meta.info_hash().hex();

    let (good, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let (bad, _) = serve(src.path().to_path_buf(), ServeMode::NotFound).await;
    let specs: Vec<SourceSpec> = [good, bad]
        .iter()
        .map(|base| {
            let mut spec = whole(base);
            spec.limits.chunk_size = 16 * 1024;
            spec
        })
        .collect();
    let set = BindingSet::resolve(&layout, &info_hash, &specs).unwrap();

    let mut options = bench_options(700);
    options.concurrency = 2;
    let outcome = bit_cli_core::bench::webseed::run(&set, &layout, &info_hash, &options, |_| {})
        .await
        .unwrap();

    assert_eq!(outcome.sources.len(), 2, "one row per source");
    let healthy = &outcome.sources[0];
    let broken = &outcome.sources[1];
    assert!(healthy.summary.bytes.0 > 0);
    assert_eq!(healthy.summary.errors, 0);
    assert_eq!(broken.summary.bytes.0, 0);
    assert!(broken.summary.errors > 0);
    assert_eq!(
        broken
            .summary
            .error_detail
            .as_ref()
            .unwrap()
            .by_status
            .get("404")
            .copied(),
        Some(broken.summary.errors),
        "the failing mirror is visible rather than averaged away"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_sources_are_probed_in_parallel_and_every_one_is_reported() {
    // A real torrent carries hundreds of web seeds: the Arch Linux ISO torrent
    // carries 468. Probing them one at a time takes minutes, so they are
    // probed in parallel. What has to hold is that every declared source comes
    // back, in the order it was declared, with its own result.
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("movie.bin"), content(64 * 1024, 91)).unwrap();
    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = meta.layout();
    let info_hash = meta.info_hash().hex();

    // Half the sources answer and half return 404, so the results cannot be
    // told apart by anything except which source produced them.
    let (good, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let (bad, _) = serve(src.path().to_path_buf(), ServeMode::NotFound).await;
    let specs: Vec<SourceSpec> = (0..32)
        .map(|index| match index % 2 {
            0 => whole(&good),
            _ => whole(&bad),
        })
        .collect();
    let set = BindingSet::resolve(&layout, &info_hash, &specs).unwrap();

    let mut workers = tokio::task::JoinSet::new();
    for (index, binding) in set.bindings.iter().enumerate() {
        let binding = binding.clone();
        let layout = layout.clone();
        let info_hash = info_hash.clone();
        workers.spawn(async move {
            (
                index,
                bit_cli_core::webseed::probe::test_source(
                    &binding,
                    &layout,
                    &info_hash,
                    false,
                    &[],
                    true,
                )
                .await,
            )
        });
    }
    let mut results: Vec<Option<bit_cli_core::webseed::probe::SourceTest>> = vec![None; 32];
    while let Some(Ok((index, result))) = workers.join_next().await {
        results[index] = Some(result);
    }

    for (index, result) in results.iter().enumerate() {
        let result = result.as_ref().unwrap_or_else(|| panic!("source {index}"));
        assert_eq!(
            result.index, index,
            "a result landed under the wrong source"
        );
        match index % 2 {
            0 => {
                assert!(result.ok, "source {index} should be usable: {result:?}");
                assert_eq!(result.status, Some(206));
            }
            _ => {
                assert!(!result.ok, "source {index} should be unusable");
                assert_eq!(result.status, Some(404));
            }
        }
    }
}

/// A hash check that has not finished reports the phase rather than a bare
/// deadline.
///
/// Upstream reports roughly one add in twenty of a torrent with existing files
/// sticking at "checking files" and never leaving. A run bounded only by
/// `--timeout` survives that but reports a deadline with no reason attached,
/// so `--init-timeout` fires first and the error names the phase, how far the
/// check had got, and how long it waited.
///
/// The hang is simulated by a deadline shorter than the check rather than by a
/// stuck volume: what is under test is that the wait is bounded and that the
/// error says what was happening, and both are the same either way. See
/// `TODO/disk-io.md`, T-015.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hash_check_that_has_not_finished_names_the_phase_it_is_in() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    // Large enough that hashing it cannot finish inside one poll.
    std::fs::write(src.path().join("movie.bin"), content(64 * 1024 * 1024, 17)).unwrap();
    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;

    let engine = engine(src.path()).await;
    let handle = engine
        .add(
            torrent_path.to_str().unwrap(),
            &AddOptions {
                // The payload is already there, so adding it means hashing it.
                overwrite: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let began = std::time::Instant::now();
    let error = engine
        .wait_until_initialized_within(&handle, Duration::from_millis(1))
        .await
        .expect_err("a 64 MiB hash check does not finish in a millisecond");

    assert_eq!(error.code(), bit_cli_core::ExitCode::Timeout);
    assert_eq!(error.context()["phase"], "initializing");
    assert_eq!(error.context()["waited_ms"], 1);
    assert!(error.context().contains_key("checked_percent"));
    assert!(error.context().contains_key("total_bytes"));
    assert!(error.message().contains("still initializing"), "{error}");
    assert!(error.message().contains("hash-checked"), "{error}");
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "the deadline did not bound the wait: {:?}",
        began.elapsed()
    );

    // Without a deadline the same wait finishes, so the timeout is the only
    // thing that ended it.
    engine.wait_until_initialized(&handle).await.unwrap();
    engine.stop().await;
}

/// Storage counts what a download actually did on disk.
///
/// The three numbers `bench leech` separates cost from throughput all come
/// from here: the piece checks, the writes underneath them, and the reads the
/// checks perform. Nothing else in the process can report them, because the
/// session does the hashing and only storage sees the I/O it takes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_reports_its_reads_writes_and_piece_checks() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(512 * 1024, 31);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;
    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );

    let counts = run.engine.storage_counts();
    let pieces = run.layout.piece_count() as u64;

    assert_eq!(
        counts.verify_pieces, pieces,
        "every piece is read back and hashed once"
    );
    assert_eq!(
        counts.verify_bytes,
        data.len() as u64,
        "a check reads exactly the piece it is checking"
    );
    assert!(
        counts.verify_nanos > 0,
        "a check that read {} bytes took no time",
        counts.verify_bytes
    );
    assert_eq!(
        counts.write_bytes,
        data.len() as u64,
        "the payload is written once"
    );
    assert!(counts.write_ops >= pieces, "{} writes", counts.write_ops);
    assert!(
        counts.read_bytes >= counts.verify_bytes,
        "the checks' reads are part of the reads"
    );
    run.engine.stop().await;
}

/// The bridge reports the session's request window from the other end.
///
/// A peer answers a bounded number of block requests at a time, and that
/// bound is what caps throughput when the link is faster than the pipeline.
/// The bridge is the only place `bit-cli` can see it, because it is the thing
/// being asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_bridge_reports_how_many_blocks_the_session_keeps_outstanding() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(2 * 1024 * 1024, 11);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;
    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );

    let pipeline = run.statuses[0].pipeline();
    assert!(pipeline.requests > 0, "the session asked for nothing");
    assert_eq!(
        pipeline.blocks, pipeline.requests,
        "every request was answered"
    );
    assert!(
        pipeline.peak_in_flight > 1,
        "the session pipelined nothing: peak depth {}",
        pipeline.peak_in_flight
    );
    assert_eq!(
        pipeline.in_flight, 0,
        "nothing is outstanding once the transfer is done"
    );
    assert!(
        pipeline.mean_service_us().is_some_and(|us| us > 0),
        "blocks were answered in no measurable time"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_403_retires_a_source_when_the_caller_has_said_nothing() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 41);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::ExpiringSignature).await;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![whole(&base)],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(30), || run.failed()).await,
        "403 is permanent by default, so the source has to give up"
    );
    assert!(!run.finished(), "nothing should have completed");
    let reasons = run.reasons().join(" ");
    assert!(
        reasons.contains("403"),
        "the reason names the status: {reasons}"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_403_the_caller_calls_retryable_completes_the_torrent() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 41);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    let (base, _) = serve(src.path().to_path_buf(), ServeMode::ExpiringSignature).await;
    let mut spec = whole(&base);
    spec.limits.retry_status = bit_cli_core::webseed::binding::StatusSet::parse("403").unwrap();
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![spec],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(120), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );
    assert_eq!(
        std::fs::read(out.path().join("movie.bin")).unwrap(),
        data,
        "the payload has to be byte for byte the source"
    );
    // Every distinct range was refused once, so the retries are the proof the
    // 403s happened rather than the server having quietly served them.
    let by_status = run.retries_by_status();
    assert!(
        by_status.get(&403).copied().unwrap_or(0) > 0,
        "no retry was charged to 403: {by_status:?}"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_404_the_caller_calls_retryable_is_still_bounded_by_the_retry_count() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = content(100 * 1024, 43);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    // A mirror that answers 404 forever. Calling it retryable does not make
    // it recover, and the run has to end rather than loop.
    let (base, _) = serve(src.path().to_path_buf(), ServeMode::NotFound).await;
    let mut spec = whole(&base);
    spec.limits.retry_status = bit_cli_core::webseed::binding::StatusSet::parse("404").unwrap();
    spec.limits.retries = 1;
    spec.limits.max_errors = 1;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![spec],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || run.failed()).await,
        "a source that never recovers has to retire even when its status is retryable"
    );
    assert!(!run.finished());
    assert!(
        run.retries_by_status().get(&404).copied().unwrap_or(0) > 0,
        "the retry should have been charged to 404"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_source_completes_a_torrent_with_no_server_at_all() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let data = content(200 * 1024, 61);
    std::fs::write(src.path().join("movie.bin"), &data).unwrap();

    // The same bytes under a name and a directory the torrent knows nothing
    // about, which is the case this exists for.
    let copy = elsewhere.path().join("a3f1-blob.dat");
    std::fs::write(&copy, &data).unwrap();

    let mut spec = SourceSpec::new(
        bit_cli_core::webseed::local::url_of(&copy),
        Origin::CommandLine,
    );
    spec.mode = bit_cli_core::webseed::Mode::Exact;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![spec],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );
    assert_eq!(
        std::fs::read(out.path().join("movie.bin")).unwrap(),
        data,
        "the payload has to be byte for byte the source"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_source_holding_the_wrong_bytes_is_caught_at_the_source() {
    let src = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("movie.bin"), content(200 * 1024, 63)).unwrap();

    // The right length and the wrong bytes: the case a length check misses
    // and the per-piece check is for.
    let wrong = elsewhere.path().join("not-it.dat");
    std::fs::write(&wrong, content(200 * 1024, 64)).unwrap();

    let torrent_path = tmp.path().join("fixture.torrent");
    make_torrent(&src.path().join("movie.bin"), &torrent_path).await;
    let meta = bit_cli_core::torrent::Metainfo::read(&torrent_path).unwrap();
    let layout = Arc::new(meta.layout());
    let hashes = Arc::new(meta.info().pieces.clone());

    let mut spec = SourceSpec::new(
        bit_cli_core::webseed::local::url_of(&wrong),
        Origin::CommandLine,
    );
    spec.mode = bit_cli_core::webseed::Mode::Exact;
    let set = BindingSet::resolve(&layout, &meta.info_hash().hex(), &[spec]).unwrap();
    let fetcher = Fetcher::new(
        set.bindings[0].clone(),
        layout.clone(),
        meta.info_hash().hex(),
        4,
        false,
    )
    .unwrap()
    .with_verification(bit_cli_core::webseed::fetch::Verify::Piece, Some(hashes));

    let err = fetcher.read(0, 16 * 1024).await.unwrap_err();
    assert_eq!(err.class(), "hash_mismatch", "{err}");
    let text = err.to_string();
    assert!(
        text.contains("not-it.dat"),
        "the path has to be named: {text}"
    );
    assert!(
        text.contains("piece 0"),
        "the piece has to be named: {text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_source_that_is_not_there_fails_the_source_by_name() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("movie.bin"), content(100 * 1024, 65)).unwrap();

    let mut spec = SourceSpec::new(
        bit_cli_core::webseed::local::url_of(&elsewhere.path().join("gone.dat")),
        Origin::CommandLine,
    );
    spec.mode = bit_cli_core::webseed::Mode::Exact;
    let run = attach(
        &src.path().join("movie.bin"),
        out.path(),
        tmp.path(),
        vec![spec],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(30), || run.failed()).await,
        "a path that is not there has to fail the source"
    );
    let reasons = run.reasons().join(" ");
    assert!(
        reasons.contains("gone.dat") && reasons.contains("no such file"),
        "the reason should name the path: {reasons}"
    );
    run.engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_source_composes_a_directory_the_way_an_http_one_does() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    // A multi-file torrent, so the composition has both a name and a path to
    // append. `auto` against a directory is what "I already have this tree"
    // looks like.
    let tree = src.path().join("album");
    std::fs::create_dir_all(tree.join("disc 1")).unwrap();
    let first = content(90 * 1024, 67);
    let second = content(40 * 1024, 68);
    std::fs::write(tree.join("disc 1/a.flac"), &first).unwrap();
    std::fs::write(tree.join("notes.nfo"), &second).unwrap();

    // The base is the directory holding `album`, so `auto` composes
    // `<base>/album/disc 1/a.flac`, space and all.
    let spec = SourceSpec::new(
        bit_cli_core::webseed::local::url_of(src.path()),
        Origin::CommandLine,
    );
    let run = attach(&tree, out.path(), tmp.path(), vec![spec]).await;

    assert!(
        wait_for(Duration::from_secs(60), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );
    assert_eq!(
        std::fs::read(out.path().join("album/disc 1/a.flac")).unwrap(),
        first
    );
    assert_eq!(
        std::fs::read(out.path().join("album/notes.nfo")).unwrap(),
        second
    );
    run.engine.stop().await;
}

// ---------------------------------------------------------------------------
// Piece alignment: `TODO/disk-io.md` T-177 and `TODO/metainfo.md` T-174.
//
// Every other fixture in this repository uses a power-of-two piece length, so
// the arithmetic on the last block of a piece is only ever exercised on the
// easy case, and no fixture is built so that pieces straddle file boundaries
// on purpose. The two entries are one fixture: a piece length that is not a
// multiple of 16 KiB, over files chosen so that every boundary falls inside a
// piece.
// ---------------------------------------------------------------------------

/// 121 * 16384 + 4096. Not a power of two, not a multiple of 16 KiB.
///
/// This is vortex [PR 124](https://github.com/Nehliin/vortex/pull/124)'s
/// number scaled to a fixture: with a piece length like this the **last
/// subpiece of every non-final piece is short**, 4096 bytes rather than
/// 16384. That tree computed `end_idx = offset + 16384`, ran past the buffer,
/// panicked, and then double-panicked in the destructor.
const ODD_PIECE_LENGTH: u32 = 121 * 16384 + 4096;

/// The three files of the alignment fixture, as `(name, length)`.
///
/// Chosen so that:
///
/// - `a.bin` is **shorter than one piece**, so piece 0 cannot be contained in
///   the file it starts in.
/// - the `a.bin`/`b.bin` boundary at 1,500,000 falls inside piece 0.
/// - the `b.bin`/`c.bin` boundary at 4,000,000 falls inside piece 2.
/// - the final piece is short.
///
/// So both boundaries straddle, which is the case fx-torrent
/// [issue 98](https://github.com/yoep/fx-torrent/issues/98) reported as "only
/// the first file is playable" on a multi-file album.
const ALIGNMENT_FILES: &[(&str, usize)] = &[
    ("a.bin", 1_500_000),
    ("b.bin", 2_500_000),
    ("c.bin", 900_000),
];

/// Write the alignment fixture under `root`, returning each file's bytes.
fn alignment_payload(root: &Path) -> Vec<(String, Vec<u8>)> {
    std::fs::create_dir_all(root).unwrap();
    let mut out = Vec::new();
    for (index, (name, length)) in ALIGNMENT_FILES.iter().enumerate() {
        // A different seed per file, so a byte written into the wrong file is
        // a mismatch rather than a coincidence.
        let bytes = content(*length, 101 + index as u64);
        std::fs::write(root.join(name), &bytes).unwrap();
        out.push(((*name).to_string(), bytes));
    }
    out
}

fn alignment_layout() -> Layout {
    Layout::from_lengths(
        "album",
        true,
        ODD_PIECE_LENGTH,
        ALIGNMENT_FILES
            .iter()
            .map(|(name, length)| ((*name).to_string(), *length as u64)),
    )
}

/// The arithmetic itself, with no session and no server in the way.
///
/// This is the part that has to be right before any of the rest means
/// anything: if `split_by_file` does not split at the boundary, a piece is
/// written entirely into the file it starts in and every byte after the first
/// file is wrong.
#[test]
fn a_piece_that_straddles_a_boundary_splits_into_one_slice_per_file() {
    let layout = alignment_layout();
    let piece = u64::from(ODD_PIECE_LENGTH);

    assert_eq!(layout.total_length, 4_900_000);
    assert_eq!(layout.piece_count(), 3, "three pieces at this piece length");
    assert!(
        (ALIGNMENT_FILES[0].1 as u64) < piece,
        "a.bin has to be shorter than one piece, or piece 0 does not straddle"
    );

    // Piece 0 covers 0..1_986_560 and the a/b boundary is at 1_500_000.
    let first = layout.split_by_file(0..piece);
    assert_eq!(first.len(), 2, "piece 0 spans two files: {first:?}");
    assert_eq!(first[0].file, 0);
    assert_eq!(first[0].offset, 0);
    assert_eq!(first[0].length, 1_500_000);
    assert_eq!(first[1].file, 1);
    assert_eq!(first[1].offset, 0);
    assert_eq!(first[1].length, piece - 1_500_000);
    assert_eq!(
        first.iter().map(|s| s.length).sum::<u64>(),
        piece,
        "the split has to account for every byte of the piece"
    );

    // Piece 1 is entirely inside b.bin, so it is the control case.
    let middle = layout.split_by_file(piece..piece * 2);
    assert_eq!(middle.len(), 1, "piece 1 is inside one file: {middle:?}");
    assert_eq!(middle[0].file, 1);

    // Piece 2 is short and covers the b/c boundary at 4_000_000.
    let last = layout.split_by_file(piece * 2..layout.total_length);
    assert_eq!(last.len(), 2, "piece 2 spans two files: {last:?}");
    assert_eq!(last[0].file, 1);
    assert_eq!(last[1].file, 2);
    assert_eq!(
        last.iter().map(|s| s.length).sum::<u64>(),
        layout.total_length - piece * 2,
        "the short last piece has to account for every remaining byte"
    );

    // Every boundary in the torrent falls strictly inside a piece, which is
    // what makes this fixture adversarial rather than incidental.
    let mut offset = 0u64;
    for (name, length) in &ALIGNMENT_FILES[..ALIGNMENT_FILES.len() - 1] {
        offset += *length as u64;
        assert_ne!(
            offset % piece,
            0,
            "the boundary after {name} at {offset} lands on a piece edge, so it is not straddled"
        );
    }
}

/// `TODO/metainfo.md` T-174. The last block of a non-final piece is short.
///
/// A reader that assumes every block is 16 KiB reads 16384 bytes from an
/// offset 4096 bytes before the end of the piece, which is the overrun vortex
/// PR 124 fixed with one `min`. The assertion here is on the numbers rather
/// than on a panic, because a fixture that only fails by panicking tells you
/// nothing when it passes.
#[test]
fn the_last_block_of_a_non_final_piece_is_four_kibibytes() {
    const BLOCK: u32 = 16 * 1024;
    let layout = alignment_layout();

    assert_ne!(
        ODD_PIECE_LENGTH % BLOCK,
        0,
        "the whole point of this piece length is that it is not a multiple of 16 KiB"
    );
    assert_eq!(ODD_PIECE_LENGTH % BLOCK, 4096);
    assert_eq!(ODD_PIECE_LENGTH / BLOCK, 121, "121 whole blocks and a tail");

    // The tail block of piece 0, addressed the way a `request` message does.
    let begin = 121 * BLOCK;
    let tail = u64::from(ODD_PIECE_LENGTH - begin);
    assert_eq!(tail, 4096);

    // It maps into b.bin, because piece 0 has already crossed out of a.bin by
    // then. A reader that clamped the block to the file it started in would
    // put these bytes at the wrong place.
    let start = u64::from(begin);
    let slices = layout.split_by_file(start..start + tail);
    assert_eq!(slices.len(), 1, "{slices:?}");
    assert_eq!(slices[0].file, 1, "the tail of piece 0 is inside b.bin");
    assert_eq!(slices[0].offset, start - 1_500_000);
    assert_eq!(slices[0].length, 4096);

    // And the final piece is shorter still, so the two short cases are not the
    // same case.
    let last_piece_length = layout.total_length - u64::from(ODD_PIECE_LENGTH) * 2;
    assert_eq!(last_piece_length, 926_880);
    assert!(last_piece_length < u64::from(ODD_PIECE_LENGTH));
}

/// The whole path, end to end: a real session, a real HTTP mirror, and this
/// fixture.
///
/// The assertion is **per file** rather than on the torrent as a whole. That
/// is the point of the entry: fx-torrent issue 98 is a payload where every
/// piece hashed against something and only the first file was playable, so a
/// check that reads the torrent-level result would have passed it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_whose_pieces_straddle_every_boundary_downloads_byte_for_byte() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = src.path().join("album");
    let files = alignment_payload(&root);

    let (base, _served) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach_with(
        &root,
        out.path(),
        tmp.path(),
        vec![whole(&base)],
        ODD_PIECE_LENGTH,
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(120), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );

    for (name, bytes) in &files {
        let landed = std::fs::read(out.path().join("album").join(name))
            .unwrap_or_else(|e| panic!("{name} was not written: {e}"));
        assert_eq!(
            landed.len(),
            bytes.len(),
            "{name} landed at the wrong length"
        );
        assert!(
            landed == *bytes,
            "{name} landed with the wrong bytes, which is the fx-torrent issue 98 shape: every piece hashed and one file is wrong"
        );
    }

    // The write fan-out, counted exactly rather than bounded.
    //
    // The storage layer is addressed by file index, so it never sees a
    // cross-file write: something above it has to split at the boundary. What
    // reaches it is one write per block, plus one extra for each block that a
    // file boundary falls inside. This fixture has two boundaries and both
    // fall strictly inside a block, so the count is `blocks + 2` and nothing
    // else. `blocks` alone would mean a straddling block went into one file,
    // which is the fx-torrent issue 98 shape asserted from the disk side.
    const BLOCK: u64 = 16 * 1024;
    let layout = alignment_layout();
    let piece = u64::from(ODD_PIECE_LENGTH);
    let blocks: u64 = (0..u64::from(layout.piece_count()))
        .map(|index| {
            let length = (layout.total_length - index * piece).min(piece);
            length.div_ceil(BLOCK)
        })
        .sum();
    assert_eq!(blocks, 301, "122 + 122 + 57 blocks at this piece length");

    let straddling_blocks = {
        let mut offset = 0u64;
        let mut count = 0u64;
        for (_, length) in &ALIGNMENT_FILES[..ALIGNMENT_FILES.len() - 1] {
            offset += *length as u64;
            // Where the boundary sits inside its own piece decides whether it
            // sits inside a block: blocks restart at every piece.
            let within_piece = offset % piece;
            if !within_piece.is_multiple_of(BLOCK) {
                count += 1;
            }
        }
        count
    };
    assert_eq!(straddling_blocks, 2, "both boundaries fall inside a block");

    let counts = run.engine.storage_counts();
    // `write_calls`, not `write_ops`. The storage layer combines a run of
    // sequential writes into one device operation, so `write_ops` counts
    // operations and no longer counts what the session asked for. The fan-out
    // this test exists for is a property of what the session asked for, and
    // that is what `write_calls` holds. See `TODO/disk-io.md`, T-018.
    assert_eq!(
        counts.write_calls,
        blocks + straddling_blocks,
        "{} writes for {blocks} blocks and {straddling_blocks} straddling ones: a block that spans a boundary has to issue one write per file",
        counts.write_calls
    );
    assert!(
        counts.write_ops <= counts.write_calls,
        "{} operations for {} writes: combining can only ever reduce them",
        counts.write_ops,
        counts.write_calls
    );
    assert_eq!(
        counts.write_bytes, layout.total_length,
        "every byte of the payload is written exactly once"
    );
    run.engine.stop().await;
}

/// The same fixture through the bridge alone, which is the other place this
/// arithmetic lives.
///
/// `webseed/fetch.rs` turns a byte range into one HTTP request per file, and a
/// block that straddles a boundary is where that fans out. A block wholly
/// inside one file is the case every other test covers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_block_that_straddles_a_boundary_is_fetched_as_one_request_per_file() {
    let src = tempfile::tempdir().unwrap();
    let root = src.path().join("album");
    let files = alignment_payload(&root);
    let (base, _served) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;

    let layout = Arc::new(alignment_layout());
    let hash = "0".repeat(40);
    let set = BindingSet::resolve(&layout, &hash, &[whole(&base)]).unwrap();
    let binding = &set.bindings[0];

    // A 16 KiB block positioned so the a/b boundary at 1,500,000 falls inside
    // it: eight kibibytes on each side.
    let start = 1_500_000 - 8192;
    let requests = binding
        .request_urls(&layout, &hash, start..start + 16384)
        .unwrap();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].file, 0);
    assert_eq!(requests[0].length, 8192);
    assert_eq!(requests[1].file, 1);
    assert_eq!(requests[1].file_offset, 0);
    assert_eq!(requests[1].length, 8192);

    let fetcher = Arc::new(Fetcher::new(binding.clone(), layout.clone(), hash, 4, false).unwrap());
    let got = fetcher.read(start, 16384).await.unwrap();
    let mut want = files[0].1[files[0].1.len() - 8192..].to_vec();
    want.extend_from_slice(&files[1].1[..8192]);
    assert_eq!(
        got, want,
        "a block spanning two files has to come back as the two files' bytes in order"
    );
}

// ---------------------------------------------------------------------------
// Per-file narrowing: `TODO/webseed.md` T-005.
//
// A permanent status on one file used to retire the whole source, including
// the files it was serving correctly a moment earlier. That contradicts the
// scope model this project exists for, where a mirror holding part of a
// payload is a first-class case and not an error.
// ---------------------------------------------------------------------------

/// Two files, sized so each one covers whole pieces of its own.
///
/// `a.bin` is four whole pieces and `b.bin` is four more, so losing `b.bin`
/// costs exactly the pieces `b.bin` covers and nothing that `a.bin` covers.
/// A boundary that straddles is [T-177](../../../TODO/disk-io.md)'s fixture
/// and is a different question from this one.
const PARTIAL_MIRROR_FILES: &[(&str, usize)] = &[
    ("a.bin", (PIECE_LENGTH * 4) as usize),
    ("b.bin", (PIECE_LENGTH * 4) as usize),
];

/// Write the payload under `root/album`, and a mirror root holding only the
/// files named in `mirrored`.
fn partial_mirror(src: &Path, mirror: &Path, mirrored: &[&str]) -> Vec<(String, Vec<u8>)> {
    let album = src.join("album");
    std::fs::create_dir_all(&album).unwrap();
    std::fs::create_dir_all(mirror.join("album")).unwrap();
    let mut out = Vec::new();
    for (index, (name, length)) in PARTIAL_MIRROR_FILES.iter().enumerate() {
        let bytes = content(*length, 71 + index as u64);
        std::fs::write(album.join(name), &bytes).unwrap();
        if mirrored.contains(name) {
            std::fs::write(mirror.join("album").join(name), &bytes).unwrap();
        }
        out.push(((*name).to_string(), bytes));
    }
    out
}

/// The headline case. A mirror that 404s one file keeps serving the other.
///
/// The partial mirror is the **only** source, deliberately. With a second
/// source present, whether the partial one is ever asked for the missing file
/// is a race the session decides, and a test that depends on losing that race
/// is the mistake [RULES.md](RULES.md) records three times over: a test waits
/// on the condition it is about, never on a guess. Alone, the partial mirror
/// is asked for everything, so the 404 is certain.
///
/// The torrent cannot complete from one partial mirror, and that is not what
/// this asserts. What it asserts is that the source survives the 404, gives up
/// exactly the pieces the missing file touches, and goes on serving the file
/// it does hold. Before T-005 the first 404 on `b.bin` retired it whole and it
/// stopped serving `a.bin` too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_mirror_that_404s_one_file_keeps_serving_the_other() {
    let src = tempfile::tempdir().unwrap();
    let partial_root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let files = partial_mirror(src.path(), partial_root.path(), &["a.bin"]);

    let (partial, _p) = serve(partial_root.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("album"),
        out.path(),
        tmp.path(),
        vec![whole(&partial)],
    )
    .await;

    let narrowed = run.statuses[0].clone();
    assert!(
        wait_for(Duration::from_secs(60), || !narrowed
            .gone_files()
            .is_empty())
        .await,
        "the mirror never reported the file it does not hold: {:?}",
        run.reasons()
    );

    let gone = narrowed.gone_files();
    assert_eq!(
        gone.len(),
        1,
        "the partial mirror has to report exactly the one file it does not hold: {gone:?}"
    );
    assert_eq!(gone[0].file, 1, "b.bin is file index 1");
    assert!(
        gone[0].reason.contains("404"),
        "the reason has to name what the mirror said: {}",
        gone[0].reason
    );
    assert_eq!(
        gone[0].pieces_dropped, 4,
        "b.bin covers four whole pieces, so four are given up and no more"
    );
    assert_eq!(narrowed.pieces_dropped(), 4);

    // It keeps serving what it does hold: a.bin is four whole pieces, and the
    // torrent stops there because nothing else has the other four.
    assert!(
        wait_for(Duration::from_secs(60), || {
            narrowed.served_bytes() >= files[0].1.len() as u64
        })
        .await,
        "the narrowed source served {} of a.bin's {} bytes, so it did not go on serving the file it holds: {:?}",
        narrowed.served_bytes(),
        files[0].1.len(),
        run.reasons()
    );
    assert_ne!(
        narrowed.state(),
        BridgeState::Failed,
        "a mirror missing one file of two is still a mirror"
    );
    assert!(
        !run.finished(),
        "nothing held b.bin, so the torrent cannot have completed"
    );
    run.engine.stop().await;
}

/// BEP 54. A mirror that loses a file retracts the pieces it can no longer
/// serve **on the connection it is already on**, rather than dropping it and
/// announcing a smaller bitfield.
///
/// This is `TODO/bep-coverage.md` T-167's acceptance and it asserts both
/// halves: one `lt_donthave` per piece given up, and the connection surviving.
/// The second is what the message is for. Before it, the only way to retract a
/// bit was to reconnect, and the test above measures that path from the other
/// side.
///
/// The two counters are deliberately different: `pieces_dropped` counts pieces
/// given up however it happened, and `pieces_retracted` counts the ones a
/// message carried. Equal means the wire did all of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_mirror_that_loses_a_file_retracts_its_pieces_without_reconnecting() {
    let src = tempfile::tempdir().unwrap();
    let partial_root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    partial_mirror(src.path(), partial_root.path(), &["a.bin"]);

    let (partial, _p) = serve(partial_root.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("album"),
        out.path(),
        tmp.path(),
        vec![whole(&partial)],
    )
    .await;

    let narrowed = run.statuses[0].clone();
    assert!(
        wait_for(Duration::from_secs(60), || narrowed.pieces_retracted() > 0).await,
        "the mirror never retracted a piece: {:?}",
        run.reasons()
    );

    assert_eq!(
        narrowed.pieces_retracted(),
        4,
        "b.bin covers four whole pieces and every one of them has to be retracted"
    );
    assert_eq!(
        narrowed.pieces_retracted(),
        narrowed.pieces_dropped(),
        "every piece given up went out as a message, or something reconnected"
    );

    // The half that is the point of the extension. `file_gone` is the reason
    // `run` charges a reconnect to when it has to drop the connection to
    // narrow, so its absence is the connection surviving.
    assert_eq!(
        narrowed.reconnect_reasons().get("file_gone").copied(),
        None,
        "the connection was dropped to narrow, which is what lt_donthave replaces: {:?}",
        narrowed.reconnect_reasons()
    );
    // A bridge takes a new loopback port every time it dials, and the status
    // keeps the history rather than the current one, so the length of that
    // history is the number of connections this source has made. One, and no
    // reading of it can race: it is the history rather than a snapshot.
    assert_eq!(
        narrowed.local_ports().len(),
        1,
        "the source dialled more than once, so it did not narrow in place: {:?}",
        narrowed.local_ports()
    );
    assert_ne!(
        narrowed.state(),
        BridgeState::Failed,
        "a mirror missing one file of two is still a mirror"
    );
    run.engine.stop().await;
}

/// The acceptance's own shape: the narrowed mirror plus one that has the rest,
/// and the run completes.
///
/// Which source serves which piece is the session's business and this does not
/// assert it. What it asserts is that a mirror narrowing itself mid-run does
/// not cost the run: the payload lands byte for byte and nothing is retired
/// that still had something to give.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_narrowed_mirror_and_a_complete_one_finish_the_torrent() {
    let src = tempfile::tempdir().unwrap();
    let partial_root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let files = partial_mirror(src.path(), partial_root.path(), &["a.bin"]);

    let (partial, _p) = serve(partial_root.path().to_path_buf(), ServeMode::Ranges).await;
    let (complete, _c) = serve(src.path().to_path_buf(), ServeMode::Ranges).await;

    let run = attach(
        &src.path().join("album"),
        out.path(),
        tmp.path(),
        vec![whole(&partial), whole(&complete)],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(120), || run.finished()).await,
        "did not complete: {:?}",
        run.reasons()
    );
    for (name, bytes) in &files {
        assert_eq!(
            std::fs::read(out.path().join("album").join(name)).unwrap(),
            *bytes,
            "{name}"
        );
    }
    assert!(
        !run.failed(),
        "no source should have been retired: {:?}",
        run.reasons()
    );
    run.engine.stop().await;
}

/// The other end of the same rule: a source with nothing left is retired.
///
/// Narrowing is not a way to keep a dead mirror alive. When every file a
/// source claimed turns out to be gone it has no pieces to announce, and a
/// bridge with an empty bitfield is worse than no bridge: the session would
/// hold a peer slot open for a peer that can never answer anything.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_mirror_that_404s_every_file_is_still_retired() {
    let src = tempfile::tempdir().unwrap();
    let empty_root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    partial_mirror(src.path(), empty_root.path(), &[]);

    let (nothing, _n) = serve(empty_root.path().to_path_buf(), ServeMode::Ranges).await;
    let run = attach(
        &src.path().join("album"),
        out.path(),
        tmp.path(),
        vec![whole(&nothing)],
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || run.failed()).await,
        "a source holding nothing has to be retired, not narrowed forever: {:?}",
        run.reasons()
    );
    let reasons = run.reasons();
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("every piece this source covered is gone")),
        "the last error has to say the source ran out rather than name one file: {reasons:?}"
    );
    assert!(!run.finished(), "nothing could have completed the torrent");
    run.engine.stop().await;
}

/// Narrowing does not spend the source's error budget.
///
/// `--web-seed-max-errors` counts consecutive failures and trips the cooldown
/// that [T-137](../../../TODO/multi-source.md) built. A file that is
/// permanently gone is not a run of errors, it is one fact learned once, and
/// counting it would retire the source through the back door after enough
/// files went missing to reach the budget.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_that_is_gone_does_not_spend_the_error_budget() {
    let src = tempfile::tempdir().unwrap();
    let partial_root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    partial_mirror(src.path(), partial_root.path(), &["a.bin"]);

    let (partial, _p) = serve(partial_root.path().to_path_buf(), ServeMode::Ranges).await;

    // One error is the whole budget, so if the 404 were counted the source
    // would cool down on it and stop serving `a.bin` as well. The mirror is
    // alone for the same reason as the test above: with a second source the
    // 404 might never happen.
    let mut narrow = whole(&partial);
    narrow.limits.max_errors = 1;
    let run = attach(
        &src.path().join("album"),
        out.path(),
        tmp.path(),
        vec![narrow],
    )
    .await;

    let narrowed = run.statuses[0].clone();
    assert!(
        wait_for(Duration::from_secs(60), || !narrowed
            .gone_files()
            .is_empty())
        .await,
        "the mirror never reported the file it does not hold: {:?}",
        run.reasons()
    );
    assert_eq!(narrowed.gone_files().len(), 1);
    assert_eq!(
        run.fetchers[0].stats().cooldowns(),
        0,
        "a file that is gone is one fact, not a run of errors"
    );
    assert_ne!(
        narrowed.state(),
        BridgeState::Cooling,
        "the source has to still be usable for the file it does hold"
    );
    run.engine.stop().await;
}
