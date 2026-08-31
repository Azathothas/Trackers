//! Fixtures and helpers for driving the whole binary in-process.
//!
//! Every test here runs the same code path a shell would, through
//! [`crate::run`], with no terminal attached. That is how the headless parity
//! requirement is checked rather than assumed.

#![cfg(test)]

use std::path::{Path, PathBuf};

use bit_cli_core::ExitCode;
use bit_cli_core::torrent::create::{CreateOptions, InputFile, create};
use bit_cli_core::torrent::{Lint, Metainfo};

use crate::env::Env;

/// A real `.torrent` and its payload, on disk in a temporary directory.
pub struct TorrentFixture {
    /// Kept so the directory outlives the fixture.
    _temp: tempfile::TempDir,
    /// The torrent `name`: the directory a multi-file torrent unpacks into.
    pub name: String,
    /// Whether it carries a `files` list.
    pub multi_file: bool,
    /// The directory everything lives in.
    pub root: PathBuf,
    /// The `.torrent` path.
    pub torrent: PathBuf,
    /// Its info hash, lower-case hex.
    pub info_hash: String,
    /// Payload files, as `(relative path, bytes)`.
    pub files: Vec<(String, Vec<u8>)>,
}

impl TorrentFixture {
    /// A two-file torrent: `disc 1/a.flac` (1500 bytes) and `notes.nfo` (500),
    /// with a 1024 byte piece length, so two pieces and a boundary that falls
    /// inside a file.
    pub fn multi_file() -> Self {
        Self::build(
            "album",
            true,
            &[
                ("disc 1/a.flac", 1500usize, 0xAAu8),
                ("notes.nfo", 500, 0xBB),
            ],
        )
    }

    /// A one-file torrent: `payload.bin`, 3000 bytes, 1024 byte pieces.
    pub fn single_file() -> Self {
        Self::build("payload.bin", false, &[("payload.bin", 3000usize, 0xCCu8)])
    }

    /// [`Self::multi_file`] with the same first file and a different second
    /// one, so exactly one file is shared between the two torrents.
    ///
    /// Same piece length and the shared file at offset zero in both, so its
    /// first whole piece lines up and its hash is a proof rather than a
    /// coincidence. The second file differs in length as well as in content,
    /// which keeps it out of the match entirely. See
    /// `TODO/multi-source.md`, T-133.
    pub fn multi_file_with_a_different_tail() -> Self {
        Self::build(
            "album",
            true,
            &[
                ("disc 1/a.flac", 1500usize, 0xAAu8),
                ("liner.txt", 900, 0xDD),
            ],
        )
    }

    /// Two torrents that hold the same file, provably.
    ///
    /// Different names, so they unpack into different directories and neither
    /// finds the other's copy by accident. Same piece length, the shared file
    /// first in both, and every file a whole number of pieces long, so the
    /// shared file's pieces line up one to one and every one of them lies
    /// entirely inside it. That is the case
    /// [`bit_cli_core::equivalence`] can prove, and it is what
    /// `TODO/multi-source.md` T-140 is about.
    pub fn sharing_pair() -> (Self, Self) {
        (
            Self::build(
                "donor",
                true,
                &[
                    ("shared.bin", 4096usize, 0x5Au8),
                    ("extra-a.txt", 1024, 0x11),
                ],
            ),
            Self::build(
                "receiver",
                true,
                &[
                    ("shared.bin", 4096usize, 0x5Au8),
                    ("extra-b.txt", 2048, 0x22),
                ],
            ),
        )
    }

    /// Three files at a 1024 byte piece length, chosen so that **both** file
    /// boundaries fall strictly inside a piece.
    ///
    /// - piece 0 is inside `a.bin`
    /// - piece 1 straddles `a.bin` and `b.bin`
    /// - piece 2 straddles `b.bin` and `c.bin`
    /// - piece 3 is inside `c.bin`
    ///
    /// So `--select-file 1` needs pieces 1 and 2 and nothing else, and both of
    /// them reach into a file nobody asked for. The lengths make the two
    /// outcomes differ on purpose: `a.bin` lands at 1500 bytes, its **full**
    /// length, holding 476 real ones, and `c.bin` lands at 872 of its 1500. One
    /// looks complete in a directory listing and one looks truncated. See
    /// `TODO/disk-io.md`, T-184.
    ///
    /// `create` sorts by path, so the indices are `a.bin` 0, `b.bin` 1,
    /// `c.bin` 2.
    pub fn straddling() -> Self {
        Self::build(
            "album",
            true,
            &[
                ("a.bin", 1500usize, 0xA1u8),
                ("b.bin", 700, 0xB2),
                ("c.bin", 1500, 0xC3),
            ],
        )
    }

    /// A torrent whose deepest path is past the 260 character limit the
    /// classic Windows API imposes, once an output directory is in front of
    /// it.
    ///
    /// Five segments of sixty characters each, which is under the 255 byte
    /// per-component limit every filesystem has and well over the total. Five
    /// rather than four because the temporary directory is part of the total
    /// and `/tmp/.tmpXXXXXX` is thirty characters shorter than the Windows
    /// one: four segments cleared 300 characters on Windows and not on Linux.
    /// See `TODO/windows.md`, T-073.
    pub fn deep() -> Self {
        let segment = "d".repeat(60);
        let path = format!("{segment}/{segment}/{segment}/{segment}/{segment}/payload.bin");
        Self::build("deep", true, &[(path.as_str(), 2000usize, 0x77u8)])
    }
    /// Write this fixture's payload under `root`, in the layout the torrent
    /// expects: a multi-file torrent unpacks into a directory named after
    /// itself.
    ///
    /// `only` names the files to write, so a test can place everything except
    /// the one it wants fetched.
    pub fn place(&self, root: &Path, only: &[&str]) {
        let base = match self.multi_file {
            true => root.join(&self.name),
            false => root.to_path_buf(),
        };
        for (path, bytes) in &self.files {
            if !only.is_empty() && !only.contains(&path.as_str()) {
                continue;
            }
            let target = base.join(path);
            std::fs::create_dir_all(target.parent().expect("a parent")).expect("mkdir");
            std::fs::write(&target, bytes).expect("write payload");
        }
    }

    /// Three levels of directory and a BEP 47 padding file between two real
    /// ones.
    ///
    /// `disc 1/lossless/a.flac` is 1500 bytes, `.pad/548` pads it up to the
    /// 2048 byte boundary, and `disc 1/notes.nfo` starts there. So the padding
    /// does what padding is for, the second file begins on a piece, and the
    /// deepest path is three components.
    ///
    /// The bencode is written by hand because `create` writes no `attr` key,
    /// which is correct on the creating side: a torrent this tree produces has
    /// no padding in it. Reading one that has is the case here. See
    /// `TODO/metainfo.md`, T-249.
    pub fn padded() -> Self {
        use std::collections::BTreeMap;

        use bit_cli_core::torrent::bencode::{Value, encode};
        use sha1::{Digest, Sha1};

        const PIECE_LENGTH: usize = 1024;
        let spec: [(&str, usize, u8, bool); 3] = [
            ("disc 1/lossless/a.flac", 1500, 0xA1, false),
            (".pad/548", 548, 0x00, true),
            ("disc 1/notes.nfo", 500, 0xB2, false),
        ];

        let mut payload = Vec::new();
        let mut files = Vec::new();
        let mut recorded = Vec::new();
        for (path, length, fill, padding) in spec {
            let bytes = vec![fill; length];
            payload.extend_from_slice(&bytes);
            let mut entry = BTreeMap::from([
                (b"length".to_vec(), Value::Int(length as i64)),
                (
                    b"path".to_vec(),
                    Value::List(
                        path.split('/')
                            .map(|component| Value::Bytes(component.as_bytes().to_vec()))
                            .collect(),
                    ),
                ),
            ]);
            if padding {
                entry.insert(b"attr".to_vec(), Value::Bytes(b"p".to_vec()));
            }
            files.push(Value::Dict(entry));
            recorded.push((path.to_string(), bytes));
        }

        let mut pieces = Vec::new();
        for chunk in payload.chunks(PIECE_LENGTH) {
            pieces.extend_from_slice(&Sha1::digest(chunk));
        }
        let info = Value::Dict(BTreeMap::from([
            (b"files".to_vec(), Value::List(files)),
            (b"name".to_vec(), Value::Bytes(b"padded".to_vec())),
            (b"piece length".to_vec(), Value::Int(PIECE_LENGTH as i64)),
            (b"pieces".to_vec(), Value::Bytes(pieces)),
        ]));
        let bytes = encode(&Value::Dict(BTreeMap::from([(b"info".to_vec(), info)])));

        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().to_path_buf();
        let torrent = root.join("padded.torrent");
        std::fs::write(&torrent, &bytes).expect("write torrent");

        Self {
            _temp: temp,
            name: "padded".to_string(),
            multi_file: true,
            root,
            torrent,
            info_hash: Metainfo::parse(&bytes).expect("parse").info_hash().hex(),
            files: recorded,
        }
    }

    fn build(name: &str, multi_file: bool, spec: &[(&str, usize, u8)]) -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().to_path_buf();

        let mut inputs = Vec::new();
        let mut files = Vec::new();
        for (path, length, fill) in spec {
            let bytes = vec![*fill; *length];
            let on_disk = root.join("payload").join(path);
            std::fs::create_dir_all(on_disk.parent().expect("parent")).expect("mkdir");
            std::fs::write(&on_disk, &bytes).expect("write payload");
            inputs.push(InputFile {
                source: on_disk,
                path: path.to_string(),
                length: *length as u64,
            });
            files.push((path.to_string(), bytes));
        }

        let options = CreateOptions {
            name: name.to_string(),
            multi_file,
            piece_length: Some(1024),
            announce_tiers: vec![vec!["udp://tracker.example.com:80".to_string()]],
            web_seeds: vec!["https://mirror.example.com/pub/".to_string()],
            created_by: None,
            creation_date: None,
            allowed_lints: Lint::ALL.iter().copied().collect(),
            ..Default::default()
        };
        let created = create(inputs, &options, |path: &Path| {
            std::fs::File::open(path)
                .map_err(|e| bit_cli_core::error::from_io(e, format!("open {}", path.display())))
        })
        .expect("create the fixture torrent");

        let torrent = root.join(format!("{name}.torrent"));
        std::fs::write(&torrent, &created.bytes).expect("write torrent");
        let info_hash = Metainfo::parse(&created.bytes)
            .expect("parse")
            .info_hash()
            .hex();

        Self {
            _temp: temp,
            name: name.to_string(),
            multi_file,
            root,
            torrent,
            info_hash,
            files,
        }
    }

    /// A torrent whose paths cannot be written as given: a drive component
    /// that escapes the output directory, a reserved Windows device name,
    /// characters NTFS refuses, a name Windows strips to another, and a pair
    /// that collides on a case-insensitive filesystem.
    ///
    /// The bencode is written by hand because `create` refuses all of this,
    /// which is correct on the creating side and exactly the input a hostile
    /// torrent carries on the reading side. No payload is written: the fixture
    /// exists to be added, not completed.
    pub fn hostile() -> Self {
        use std::collections::BTreeMap;

        use bit_cli_core::torrent::bencode::{Value, encode};
        use sha1::{Digest, Sha1};

        const PIECE_LENGTH: usize = 1024;
        let paths = [
            "C:/pwned.txt",
            "CON.txt",
            "a<b.bin",
            "x .",
            "README",
            "readme",
        ];

        let mut payload = Vec::new();
        let mut files = Vec::new();
        let mut recorded = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            let bytes = vec![index as u8 + 1; 500];
            payload.extend_from_slice(&bytes);
            files.push(Value::Dict(BTreeMap::from([
                (b"length".to_vec(), Value::Int(bytes.len() as i64)),
                (
                    b"path".to_vec(),
                    Value::List(
                        path.split('/')
                            .map(|c| Value::Bytes(c.as_bytes().to_vec()))
                            .collect(),
                    ),
                ),
            ])));
            recorded.push(((*path).to_string(), bytes));
        }

        let mut pieces = Vec::new();
        for chunk in payload.chunks(PIECE_LENGTH) {
            pieces.extend_from_slice(&Sha1::digest(chunk));
        }
        let info = Value::Dict(BTreeMap::from([
            (b"files".to_vec(), Value::List(files)),
            (b"name".to_vec(), Value::Bytes(b"hostile".to_vec())),
            (b"piece length".to_vec(), Value::Int(PIECE_LENGTH as i64)),
            (b"pieces".to_vec(), Value::Bytes(pieces)),
        ]));
        let bytes = encode(&Value::Dict(BTreeMap::from([(b"info".to_vec(), info)])));

        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().to_path_buf();
        let torrent = root.join("hostile.torrent");
        std::fs::write(&torrent, &bytes).expect("write torrent");

        Self {
            _temp: temp,
            name: "hostile".to_string(),
            multi_file: true,
            root,
            torrent,
            info_hash: Metainfo::parse(&bytes).expect("parse").info_hash().hex(),
            files: recorded,
        }
    }

    /// A torrent whose `name` and `path` are cp932, with the `.utf-8` twins a
    /// uTorrent-created torrent carries beside them.
    ///
    /// The cp932 bytes are the ones `chardetng` reads as windows-1252, so
    /// detection alone names the file `‹È.bin` and only the `.utf-8` key gets
    /// `曲.bin`. That is the shape T-103 is about, and it is written as bytes
    /// rather than encoded here because this repository has no cp932 encoder
    /// and does not need one to read a torrent that has one.
    ///
    /// The payload is placed under the **decoded** names, because that is what
    /// a mirror carrying this torrent's files has.
    pub fn names_that_are_not_utf8() -> Self {
        use std::collections::BTreeMap;

        use bit_cli_core::torrent::bencode::{Value, encode};
        use sha1::{Digest, Sha1};

        const PIECE_LENGTH: usize = 1024;
        // `音楽` and `曲.bin` in cp932.
        const NAME: &[u8] = &[0x89, 0xB9, 0x8A, 0x79];
        const PATH: &[u8] = &[0x8B, 0xC8, b'.', b'b', b'i', b'n'];

        let payload = vec![0xD1u8; 1500];
        let mut pieces = Vec::new();
        for chunk in payload.chunks(PIECE_LENGTH) {
            pieces.extend_from_slice(&Sha1::digest(chunk));
        }

        let file = Value::Dict(BTreeMap::from([
            (b"length".to_vec(), Value::Int(payload.len() as i64)),
            (
                b"path".to_vec(),
                Value::List(vec![Value::Bytes(PATH.to_vec())]),
            ),
            (
                b"path.utf-8".to_vec(),
                Value::List(vec![Value::text("曲.bin")]),
            ),
        ]));
        let info = Value::Dict(BTreeMap::from([
            (b"files".to_vec(), Value::List(vec![file])),
            (b"name".to_vec(), Value::Bytes(NAME.to_vec())),
            (b"name.utf-8".to_vec(), Value::text("音楽")),
            (b"piece length".to_vec(), Value::Int(PIECE_LENGTH as i64)),
            (b"pieces".to_vec(), Value::Bytes(pieces)),
        ]));
        let bytes = encode(&Value::Dict(BTreeMap::from([(b"info".to_vec(), info)])));

        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().to_path_buf();
        let torrent = root.join("names.torrent");
        std::fs::write(&torrent, &bytes).expect("write torrent");

        Self {
            _temp: temp,
            name: "音楽".to_string(),
            multi_file: true,
            root,
            torrent,
            info_hash: Metainfo::parse(&bytes).expect("parse").info_hash().hex(),
            files: vec![("曲.bin".to_string(), payload)],
        }
    }

    /// [`Self::single_file`] with both web seed keys rewritten as a **bare
    /// bencoded string** rather than a list.
    ///
    /// BEP 17 specifies `httpseeds` as a list and BEP 19 specifies `url-list`
    /// as a list, and torrents carrying one bare string exist for both keys.
    /// A reader that accepts the list alone yields nothing for these, with no
    /// error and no warning, which is `TODO/metainfo.md` T-171.
    ///
    /// Both keys are outside `info`, so the info hash is unchanged and the
    /// field recorded on the fixture stays true.
    pub fn web_seed_keys_as_strings() -> Self {
        use bit_cli_core::torrent::bencode::{self, Value};

        let fixture = Self::single_file();
        let bytes = std::fs::read(&fixture.torrent).expect("read the fixture torrent");
        let mut root = bencode::decode(&bytes).expect("decode the fixture torrent");
        {
            let dict = root.as_dict_mut().expect("a top-level dictionary");
            dict.insert(
                b"url-list".to_vec(),
                Value::text("https://getright.example.com/pub/"),
            );
            dict.insert(
                b"httpseeds".to_vec(),
                Value::text("https://hoffman.example.com/"),
            );
        }
        std::fs::write(&fixture.torrent, bencode::encode(&root)).expect("rewrite the torrent");
        fixture
    }

    /// The `.torrent` path, as an argument.
    pub fn path_str(&self) -> &str {
        self.torrent.to_str().expect("utf-8 path")
    }

    /// The directory to run commands from.
    pub fn dir(&self) -> PathBuf {
        self.root.clone()
    }

    /// Where the payload lives.
    pub fn payload_dir(&self) -> PathBuf {
        self.root.join("payload")
    }
}

/// A ranged HTTP server over a directory, on a thread, for the tests that
/// need a real web seed rather than a stub.
///
/// It binds port zero and reports what it got, so tests running at once never
/// race for a port. It speaks the little of HTTP/1.1 a web seed needs: `GET`,
/// one `Range: bytes=a-b` header, `206` with `Content-Range`, `404` for a path
/// that is not there. Nothing is kept alive between requests, which is slower
/// than a real mirror and is exactly why throughput assertions do not belong
/// against it.
pub struct FileServer {
    pub base: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Every request target it was asked for, in order, whether or not the
    /// file was there. What a mirror was **not** asked for is the only
    /// evidence that a selection was applied before the fetch rather than
    /// after it. See `TODO/cli-surface.md`, T-185.
    asked: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl FileServer {
    /// Serve `root` on loopback, speaking BEP 19: ranged GETs against a path.
    pub fn start(root: impl Into<PathBuf>) -> Self {
        Self::start_with(root, false, false, 0)
    }

    /// Serve `root` on loopback speaking BEP 17 instead.
    ///
    /// A Hoffman seed takes `?info_hash=&piece=&ranges=` and answers 200 with
    /// exactly the bytes named, and refuses a request that carries none of
    /// them. The piece length is the fixtures' 1024. See `TODO/webseed.md`,
    /// T-004.
    pub fn start_hoffman(root: impl Into<PathBuf>) -> Self {
        Self::start_with(root, true, false, 0)
    }

    /// Serve `root` the way a CDN in front of a bucket does.
    ///
    /// Four headers a report keeps and two it must drop, in one fixture, so
    /// the allowlist is proved in both directions by one run. `x-cache-hits`
    /// and `x-frame-options` are the two that must not appear: the first
    /// because it is not on the list and looks like one that is, the second
    /// because it is the kind of header every origin sends and none of it is
    /// diagnostic. See `TODO/webseed.md`, T-254.
    pub fn start_cdn(root: impl Into<PathBuf>) -> Self {
        Self::start_with(root, false, true, 0)
    }

    /// Serve `root` behind `hops` redirects, the way a bucket behind a signed
    /// URL or a mirror redirector does.
    ///
    /// A request that has not been redirected `hops` times yet is answered
    /// `302` with a `Location` one `via/` segment longer; the segments are
    /// stripped before the file is opened. So the client walks a real chain
    /// and every hop is a separate request with a separate status.
    ///
    /// It exists because `sources[].redirects[]` is three documented fields
    /// that no sample run had ever produced: `loopback-fileserver` issues no
    /// redirect, so the schema described them from a real S3 endpoint rather
    /// than from a run. See `TODO/cli-surface.md`, T-253.
    pub fn start_redirecting(root: impl Into<PathBuf>, hops: u8) -> Self {
        Self::start_with(root, false, false, hops)
    }

    fn start_with(root: impl Into<PathBuf>, hoffman: bool, cdn: bool, redirects: u8) -> Self {
        use std::io::{Read, Write};

        let root = root.into();
        let listener =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        listener
            .set_nonblocking(true)
            .expect("non-blocking listener");
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = asked.clone();

        std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                // The listener polls, the connection does not. An accepted
                // socket inherits the listener's non-blocking mode, and the
                // read below treats every error as the end of the connection,
                // so a request that had not arrived yet was answered by
                // hanging up. That is one flaky test in a hundred runs, and it
                // took a captured failure to see.
                let _ = stream.set_nonblocking(false);
                let root = root.clone();
                let log = log.clone();
                std::thread::spawn(move || {
                    let mut request = Vec::new();
                    let mut buf = [0u8; 4096];
                    // Headers end at the blank line. A web seed request has no
                    // body, so that is the whole request.
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => request.extend_from_slice(&buf[..n]),
                        }
                    }
                    let text = String::from_utf8_lossy(&request).to_string();
                    let mut lines = text.lines();
                    let Some(start) = lines.next() else { return };
                    let Some(path) = start.split_whitespace().nth(1) else {
                        return;
                    };
                    // Recorded before the file is opened, so a request for
                    // something that is not there is still a request that was
                    // made.
                    if let Ok(mut asked) = log.lock() {
                        asked.push(path.to_string());
                    }
                    // Header names are case insensitive, and every HTTP client
                    // this is pointed at writes them lower case, so matching
                    // `Range:` exactly matched nothing: every ranged request
                    // was answered with the whole file and a 200. Small
                    // fixtures still verified, which is what hid it.
                    let range = text
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.trim().eq_ignore_ascii_case("range").then_some(value)
                        })
                        .and_then(|value| value.trim().strip_prefix("bytes="))
                        .and_then(|spec| spec.split_once('-'))
                        .map(|(a, b)| (a.to_string(), b.to_string()));

                    // A query string is not part of the path. A BEP 19 server
                    // that does not understand one serves the resource anyway,
                    // which is what makes the BEP 17 probe a question about
                    // the length of the answer rather than its status.
                    let (path, query) = path.split_once('?').unwrap_or((path, ""));

                    // A redirect chain, counted in `via/` segments so the
                    // server is stateless and two clients cannot interfere.
                    // The segments are stripped before the file is opened, so
                    // the last hop serves the resource that was asked for.
                    let mut walked = 0u8;
                    let mut rest = path.trim_start_matches('/');
                    while let Some(tail) = rest.strip_prefix("via/") {
                        walked = walked.saturating_add(1);
                        rest = tail;
                    }
                    if walked < redirects {
                        let location = format!("/{}{}", "via/".repeat(walked as usize + 1), rest);
                        let head = format!(
                            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.flush();
                        return;
                    }
                    let path = rest;
                    let relative = percent_decode(path.trim_start_matches('/'));
                    let mut target = root.clone();
                    for part in relative.split('/').filter(|p| !p.is_empty()) {
                        target.push(part);
                    }
                    let Ok(body) = std::fs::read(&target) else {
                        let _ = stream.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        return;
                    };

                    if hoffman {
                        let mut piece: Option<usize> = None;
                        let mut span: Option<(usize, usize)> = None;
                        let mut has_hash = false;
                        for pair in query.split('&') {
                            match pair.split_once('=') {
                                Some(("piece", value)) => piece = value.parse().ok(),
                                Some(("ranges", value)) => {
                                    span = value.split_once('-').and_then(|(a, b)| {
                                        Some((a.parse().ok()?, b.parse().ok()?))
                                    });
                                }
                                Some(("info_hash", value)) => has_hash = !value.is_empty(),
                                _ => {}
                            }
                        }
                        let (Some(piece), Some((begin, end)), true) = (piece, span, has_hash)
                        else {
                            let _ = stream.write_all(
                                b"HTTP/1.1 400 Bad Request
Content-Length: 0
Connection: close

",
                            );
                            return;
                        };
                        let start = piece * 1024 + begin;
                        let stop = (piece * 1024 + end).min(body.len().saturating_sub(1));
                        let slice = body.get(start..=stop).unwrap_or(&[]).to_vec();
                        let head = format!(
                            "HTTP/1.1 200 OK
Content-Length: {}
Connection: close

",
                            slice.len()
                        );
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.write_all(&slice);
                        let _ = stream.flush();
                        return;
                    }

                    let total = body.len();
                    // Written as escapes rather than as real line breaks in a
                    // string: a heredoc turns `\r\n` into a CR and an LF byte
                    // on the way here and the fixture then serves a malformed
                    // response that hangs the client. `TODO/RULES.md` section 5.
                    let extra = match cdn {
                        true => {
                            "Cache-Control: public, max-age=3600\r\nETag: \"d41d8cd9\"\r\nAge: 41\r\nX-Cache: HIT\r\nX-Cache-Hits: 12\r\nX-Frame-Options: DENY\r\n"
                        }
                        false => "",
                    };
                    let response = match range {
                        None => format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\n{extra}Connection: close\r\n\r\n"
                        ),
                        Some((from, to)) => {
                            let from: usize = from.parse().unwrap_or(0);
                            let to: usize = to.parse().unwrap_or(total.saturating_sub(1));
                            let to = to.min(total.saturating_sub(1));
                            let slice = body.get(from..=to).unwrap_or(&[]).to_vec();
                            let head = format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {from}-{to}/{total}\r\nAccept-Ranges: bytes\r\n{extra}Connection: close\r\n\r\n",
                                slice.len()
                            );
                            let _ = stream.write_all(head.as_bytes());
                            let _ = stream.write_all(&slice);
                            let _ = stream.flush();
                            return;
                        }
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(&body);
                    let _ = stream.flush();
                });
            }
        });

        Self {
            base: format!("http://127.0.0.1:{port}/"),
            stop,
            asked,
        }
    }

    /// Every request target served so far, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("the request log").clone()
    }
}

impl Drop for FileServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Decode `%XX` escapes in a request path.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A BEP 3 HTTP tracker on loopback, for the commands that announce.
///
/// It keeps no swarm. Every announce gets the same answer, built once from the
/// peers it was started with, and every field a
/// [`bit_cli_core::tracker::TrackerResult`] can carry is in it: `complete`,
/// `incomplete`, `downloaded`, `interval`, `min interval`, `warning message`,
/// and a compact `peers` list. A real tracker answers differently on the first
/// request and the hundredth, which is right for a tracker and wrong for a
/// fixture that has to produce the same document twice.
///
/// `crates/bit-cli-core/examples/loopback-tracker.rs` is the other one: it
/// tracks a real swarm and is what the interop scripts drive. This one is for
/// tests, which cannot run an example binary.
pub struct Tracker {
    /// The announce URL, ready for `--announce` or `--tracker`.
    pub announce: String,
    /// Every request line it has served, in order.
    seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Tracker {
    /// Serve an announce that hands back `peers`, on a port the OS picks.
    pub fn start(peers: &[std::net::SocketAddrV4]) -> Self {
        Self::start_serving(announce_body(peers))
    }

    /// Serve one document, whatever it is, to every announce.
    ///
    /// For the responses a well-behaved tracker does not send. A tracker list
    /// comes out of a `.torrent` and is untrusted, so what a malformed answer
    /// does here is a property worth a fixture rather than a unit test on the
    /// parser: the question is what the command reports, not what one function
    /// returns. See `TODO/trackers.md`, T-180.
    pub fn start_serving(body: Vec<u8>) -> Self {
        use std::io::{Read, Write};

        let listener =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        listener
            .set_nonblocking(true)
            .expect("non-blocking listener");
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();

        std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                // The listener polls, the connection does not. An accepted
                // socket inherits the listener's non-blocking mode, and the
                // read below treats every error as the end of the connection,
                // so a request that had not arrived yet was answered by
                // hanging up. That is one flaky test in a hundred runs, and it
                // took a captured failure to see.
                let _ = stream.set_nonblocking(false);
                let body = body.clone();
                let log = log.clone();
                std::thread::spawn(move || {
                    let mut request = Vec::new();
                    let mut buf = [0u8; 2048];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => request.extend_from_slice(&buf[..n]),
                        }
                    }
                    // The request line, which carries the whole announce: a
                    // tracker query has no body. Recorded before the reply, so
                    // a caller that reads `seen` after the command exits sees
                    // everything the command sent.
                    if let Some(line) = String::from_utf8_lossy(&request).lines().next()
                        && let Ok(mut log) = log.lock()
                    {
                        log.push(line.to_string());
                    }
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&body);
                    let _ = stream.flush();
                });
            }
        });

        Self {
            announce: format!("http://127.0.0.1:{port}/announce"),
            seen,
            stop,
        }
    }

    /// Every request line served so far.
    pub fn seen(&self) -> Vec<String> {
        self.seen.lock().map(|log| log.clone()).unwrap_or_default()
    }

    /// The value of one query parameter, once per announce that carried it.
    ///
    /// No decoding: the parameters this is asked for (`port`, `event`,
    /// `left`, `numwant`) are all plain. `info_hash` and `peer_id` are binary
    /// and percent-encoded, and reading them is not what these tests are for.
    pub fn param(&self, name: &str) -> Vec<String> {
        let prefix = format!("{name}=");
        self.seen()
            .iter()
            .filter_map(|line| {
                let query = line.split_whitespace().nth(1)?.split_once('?')?.1;
                query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix(&prefix))
                    .map(str::to_string)
            })
            .collect()
    }
}

impl Drop for Tracker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The bencoded announce response, with the keys in sorted order.
fn announce_body(peers: &[std::net::SocketAddrV4]) -> Vec<u8> {
    let mut packed = Vec::with_capacity(peers.len() * 6);
    for peer in peers {
        packed.extend_from_slice(&peer.ip().octets());
        packed.extend_from_slice(&peer.port().to_be_bytes());
    }
    let warning = "this tracker is a test fixture";

    let mut body = Vec::new();
    body.push(b'd');
    body.extend_from_slice(b"8:completei1e");
    body.extend_from_slice(b"10:downloadedi7e");
    body.extend_from_slice(b"10:incompletei2e");
    body.extend_from_slice(b"8:intervali1800e");
    body.extend_from_slice(b"12:min intervali900e");
    body.extend_from_slice(format!("5:peers{}:", packed.len()).as_bytes());
    body.extend_from_slice(&packed);
    body.extend_from_slice(format!("15:warning message{}:{warning}", warning.len()).as_bytes());
    body.push(b'e');
    body
}

/// Reserve a port by binding it and letting it go.
///
/// A seeder has to be announced before it starts listening, so its port has to
/// be known first. Binding zero and closing is the same pattern
/// `scripts/check-peer-recovery.ps1` uses to restart a peer on the port it had.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("bind loopback")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Seed one fixture on a thread, and wait until its listener answers.
///
/// The swarm a magnet resolves against has to be a real one, and it has to be
/// exactly this process: `--no-dht --no-lsd --no-tracker` on both sides leaves
/// a swarm of the addresses on the command line and nothing else, so nothing
/// here reaches the network.
///
/// The wait is not optional and it is not politeness. The seeder is on a
/// thread and the resolver dials it from another, and `librqbit` does not
/// retry a dead peer for ten seconds, which is longer than any deadline a test
/// sets. That is [`TODO/cli-surface.md`], T-160, and it turned CI red on a
/// documentation-only commit.
///
/// The payload is placed under the torrent's own name, which is what a seeder
/// resolves `--data` against.
///
/// **`--stop-after` bounds the fixture and the caller joins the handle.**
/// Twenty seconds is the seeder's whole life: it is not a deadline anything in
/// a test waits on, because the wait above is on the listener and the work
/// after it is a loopback resolution that takes about a second. Joining is
/// what keeps the thread from reading out of a `TempDir` the fixture is
/// dropping, which on Windows is a directory that will not delete.
pub fn seed_fixture(fixture: &TorrentFixture, port: u16) -> std::thread::JoinHandle<ExitCode> {
    let dir = fixture.dir();
    let data = dir.join("seeded");
    fixture.place(&data, &[]);
    let torrent = fixture.path_str().to_string();
    let data = data.to_str().expect("utf-8 path").to_string();
    let handle = std::thread::spawn(move || {
        let (mut env, _) = Env::test(
            &[
                "seed",
                &torrent,
                "--data",
                &data,
                "--port",
                &port.to_string(),
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--stop-after",
                "20s",
            ],
            dir,
        );
        crate::run(&mut env)
    });
    assert!(
        wait_for_listener(port, std::time::Duration::from_secs(15)),
        "the seeder never listened on {port}"
    );
    handle
}

/// Block until something accepts on `port`, or the timeout runs out.
///
/// `free_port` binds a port to learn its number and then drops the listener, so
/// there is a window where the number is known and nothing is listening. A test
/// that starts a seeder on a thread and dials it immediately can land in that
/// window: the dial fails, the peer is marked dead with one error, and
/// `librqbit` does not retry for ten seconds, which is longer than any of these
/// tests run. That is not a slow machine showing a real defect, it is the test
/// racing its own fixture, and it is what turned `Test (ubuntu-latest)` red on
/// CI run 32458314378. See `TODO/cli-surface.md`, T-160.
///
/// Waiting on the condition rather than sleeping a guessed amount is the rule
/// T-148 wrote down. Returns whether the port came up, so a caller can say so
/// rather than failing on the assertion three lines later.
pub fn wait_for_listener(port: u16, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        // A connect that succeeds is dropped immediately. One connection that
        // closes before it handshakes is the shape T-020 measures, and it takes
        // thousands of them to matter: `scripts/check-close-wait.ps1` puts 2000
        // through a seeder and the listener survives.
        if std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port)),
            std::time::Duration::from_millis(200),
        )
        .is_ok()
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

/// Run the binary in-process and require success, returning stdout.
pub fn run_ok(args: &[&str], cwd: impl Into<PathBuf>) -> String {
    let (mut env, captured) = Env::test(args, cwd);
    let code = crate::run(&mut env);
    assert_eq!(
        code,
        ExitCode::Success,
        "`bit-cli {}` exited {code}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        captured.out(),
        captured.err()
    );
    captured.out()
}

/// Run the binary in-process and require a specific failure code.
pub fn run_err(args: &[&str], cwd: impl Into<PathBuf>, expected: ExitCode) -> String {
    let (mut env, captured) = Env::test(args, cwd);
    let code = crate::run(&mut env);
    assert_eq!(
        code,
        expected,
        "`bit-cli {}` exited {code}, expected {expected}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        captured.out(),
        captured.err()
    );
    captured.err()
}

/// Run the binary and return stdout parsed as JSON, requiring a given exit
/// code.
///
/// For the commands whose JSON report is the point even though the run did not
/// succeed: a download that hit its deadline still reports where it wrote.
pub fn run_json_code(
    args: &[&str],
    cwd: impl Into<PathBuf>,
    expected: ExitCode,
) -> serde_json::Value {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    let (mut env, captured) = Env::test(&full, cwd);
    let code = crate::run(&mut env);
    assert_eq!(
        code,
        expected,
        "`bit-cli {}` exited {code}, expected {expected}\nstdout:\n{}\nstderr:\n{}",
        full.join(" "),
        captured.out(),
        captured.err()
    );
    captured
        .json()
        .unwrap_or_else(|e| panic!("stdout was not JSON: {e}\n{}", captured.out()))
}

/// Run the binary and return stdout parsed as JSON.
pub fn run_json(args: &[&str], cwd: impl Into<PathBuf>) -> serde_json::Value {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    let (mut env, captured) = Env::test(&full, cwd);
    let code = crate::run(&mut env);
    assert_eq!(
        code,
        ExitCode::Success,
        "`bit-cli {}` exited {code}\nstderr:\n{}",
        full.join(" "),
        captured.err()
    );
    captured
        .json()
        .unwrap_or_else(|e| panic!("stdout was not JSON: {e}\n{}", captured.out()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_multi_file_fixture_is_what_it_claims_to_be() {
        let fixture = TorrentFixture::multi_file();
        let meta = Metainfo::read(&fixture.torrent).unwrap();
        assert_eq!(meta.info().name, "album");
        assert!(meta.info().multi_file);
        assert_eq!(meta.info().total_length(), 2000);
        assert_eq!(meta.info().piece_length, 1024);
        assert_eq!(meta.info().pieces.len(), 2);
        assert_eq!(meta.info_hash().hex(), fixture.info_hash);
    }

    #[test]
    fn the_single_file_fixture_is_what_it_claims_to_be() {
        let fixture = TorrentFixture::single_file();
        let meta = Metainfo::read(&fixture.torrent).unwrap();
        assert!(!meta.info().multi_file);
        assert_eq!(meta.info().total_length(), 3000);
        assert_eq!(meta.info().pieces.len(), 3);
    }

    #[test]
    fn the_fixture_payload_is_on_disk_and_matches_the_torrent() {
        let fixture = TorrentFixture::multi_file();
        for (path, bytes) in &fixture.files {
            let on_disk = std::fs::read(fixture.payload_dir().join(path)).unwrap();
            assert_eq!(&on_disk, bytes, "{path} does not match");
        }
    }

    #[test]
    fn the_fixture_is_deterministic() {
        let one = TorrentFixture::multi_file();
        let other = TorrentFixture::multi_file();
        assert_eq!(one.info_hash, other.info_hash);
        assert_eq!(
            std::fs::read(&one.torrent).unwrap(),
            std::fs::read(&other.torrent).unwrap()
        );
    }

    /// A wait that always says yes is not a wait, and one that always says no
    /// would fail every test that uses it for a reason nothing names. Both
    /// directions, so `wait_for_listener` cannot become vacuous without this
    /// failing. See T-160.
    #[test]
    fn waiting_for_a_listener_answers_both_ways() {
        let listener =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(
            wait_for_listener(port, std::time::Duration::from_secs(2)),
            "a bound port was reported as not listening"
        );

        // Dropping the listener frees the port, which is exactly the window
        // `free_port` leaves open and the one this helper exists to close.
        drop(listener);
        let started = std::time::Instant::now();
        assert!(
            !wait_for_listener(port, std::time::Duration::from_millis(300)),
            "an unbound port was reported as listening"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(250),
            "it gave up before its timeout, so it is not waiting at all"
        );
    }
}
