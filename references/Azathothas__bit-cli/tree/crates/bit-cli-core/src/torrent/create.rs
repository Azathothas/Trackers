//! Creating `.torrent` files.
//!
//! Determinism is the property that matters most here. The same input with
//! `--no-creation-date` and a fixed sort order produces a byte-identical
//! `.torrent` on every run and on every platform. That is what lets a build
//! pipeline publish a torrent and prove it matches what it published before.
//!
//! Three things make it hold:
//!
//! - File ordering is explicit ([`SortBy`]), never filesystem order.
//! - Paths are `/`-separated and sorted by their raw bytes, so a Windows
//!   backslash never reaches the metainfo and locale collation never applies.
//! - The `info` dictionary is encoded once, hashed, and written from those
//!   same bytes. There is no second encoding that could differ.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use md5::Md5;
use sha1::{Digest, Sha1};

use crate::error::{Error, Result, from_io};
use crate::layout::Layout;
use crate::time::Timestamp;
use crate::torrent::bencode::{self, Value};
use crate::torrent::lint::{self, Lint};
use crate::torrent::metainfo::{InfoHash, Metainfo};
use crate::torrent::piece_length;

/// How input files are ordered in the torrent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// By `/`-separated path, byte order. The default, and the only key that
    /// is stable across filesystems.
    #[default]
    Path,
    /// By file size.
    Size,
}

/// Ascending or descending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Ascending,
    Descending,
}

/// A parsed `--sort-by KEY:ORDER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SortBy {
    pub key: SortKey,
    pub order: SortOrder,
}

impl SortBy {
    /// Parse `KEY` or `KEY:ORDER`.
    pub fn parse(text: &str) -> Result<Self> {
        let (key, order) = text.split_once(':').unwrap_or((text, "asc"));
        let key = match key.trim().to_ascii_lowercase().as_str() {
            "path" | "name" => SortKey::Path,
            "size" | "length" => SortKey::Size,
            other => {
                return Err(Error::usage(format!(
                    "`{other}` is not a sort key (use path or size)"
                )));
            }
        };
        let order = match order.trim().to_ascii_lowercase().as_str() {
            "asc" | "ascending" => SortOrder::Ascending,
            "desc" | "descending" => SortOrder::Descending,
            other => {
                return Err(Error::usage(format!(
                    "`{other}` is not a sort order (use asc or desc)"
                )));
            }
        };
        Ok(Self { key, order })
    }
}

/// One input file: where it is on disk, and where it goes in the torrent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFile {
    /// Where to read the bytes from.
    pub source: PathBuf,
    /// `/`-separated path within the torrent.
    pub path: String,
    /// Length in bytes.
    pub length: u64,
}

/// Everything `bit-cli create` needs.
#[derive(Debug, Clone)]
pub struct CreateOptions {
    /// The torrent name.
    pub name: String,
    /// Whether the torrent describes a directory.
    pub multi_file: bool,
    /// Piece length, or `None` to choose one.
    pub piece_length: Option<u32>,
    /// BEP 12 tracker tiers.
    pub announce_tiers: Vec<Vec<String>>,
    /// BEP 19 `url-list`.
    pub web_seeds: Vec<String>,
    /// BEP 17 `httpseeds`.
    pub http_seeds: Vec<String>,
    /// DHT bootstrap nodes as `host:port`.
    pub nodes: Vec<String>,
    pub comment: Option<String>,
    /// The `source` key, inside `info`. It changes the info hash on purpose;
    /// cross-seeding uses it.
    pub source: Option<String>,
    /// BEP 39 feed URL.
    pub update_url: Option<String>,
    /// BEP 27 private flag.
    pub private: bool,
    /// Write per-file MD5 sums. MD5 is not collision resistant and is here for
    /// compatibility with tools that expect the field, nothing more.
    pub md5: bool,
    /// The `created by` string, or `None` to omit it.
    pub created_by: Option<String>,
    /// The creation date, or `None` to omit it. Omitting it is required for
    /// byte-reproducible output.
    pub creation_date: Option<Timestamp>,
    /// Lints the caller has allowed.
    pub allowed_lints: BTreeSet<Lint>,
    /// File ordering.
    pub sort_by: SortBy,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            multi_file: false,
            piece_length: None,
            announce_tiers: Vec::new(),
            web_seeds: Vec::new(),
            http_seeds: Vec::new(),
            nodes: Vec::new(),
            comment: None,
            source: None,
            update_url: None,
            private: false,
            md5: false,
            created_by: Some(format!("bit-cli/{}", crate::VERSION)),
            creation_date: Some(Timestamp::now()),
            allowed_lints: BTreeSet::new(),
            sort_by: SortBy::default(),
        }
    }
}

/// What a creation run produced.
#[derive(Debug, Clone)]
pub struct Created {
    /// The encoded `.torrent`.
    pub bytes: Vec<u8>,
    /// The info hash.
    pub info_hash: InfoHash,
    /// The chosen piece length.
    pub piece_length: u32,
    /// Why that piece length was chosen.
    pub piece_length_reason: String,
    /// Number of pieces.
    pub piece_count: u32,
    /// Total payload bytes.
    pub total_length: u64,
    /// Files, in torrent order.
    pub files: Vec<InputFile>,
}

/// Sort input files deterministically.
///
/// Ties fall back to path order so the result never depends on the order the
/// filesystem happened to hand files over.
pub fn sort_files(files: &mut [InputFile], sort_by: SortBy) {
    files.sort_by(|a, b| {
        let primary = match sort_by.key {
            // Compare raw bytes, not `str`, so ordering never depends on a
            // locale or a Unicode collation table.
            SortKey::Path => a.path.as_bytes().cmp(b.path.as_bytes()),
            SortKey::Size => a.length.cmp(&b.length),
        };
        let primary = match sort_by.order {
            SortOrder::Ascending => primary,
            SortOrder::Descending => primary.reverse(),
        };
        primary.then_with(|| a.path.as_bytes().cmp(b.path.as_bytes()))
    });
}

/// Create a torrent from a list of input files.
///
/// `read_file` opens each file. It is a parameter so the whole creator can be
/// driven from in-memory fixtures in a test without touching a disk.
pub fn create<R: Read>(
    mut files: Vec<InputFile>,
    options: &CreateOptions,
    mut open: impl FnMut(&Path) -> Result<R>,
) -> Result<Created> {
    sort_files(&mut files, options.sort_by);

    let total_length: u64 = files.iter().map(|f| f.length).sum();
    let piece_length = match options.piece_length {
        Some(explicit) => {
            piece_length::validate(explicit)?;
            explicit
        }
        None => piece_length::choose(total_length),
    };
    let piece_length_reason = piece_length::explain(total_length, piece_length);

    let layout = Layout::from_lengths(
        options.name.clone(),
        options.multi_file,
        piece_length,
        files.iter().map(|f| (f.path.clone(), f.length)),
    );

    let trackers: Vec<String> = options.announce_tiers.iter().flatten().cloned().collect();
    let findings = lint::check(
        &lint::Candidate {
            layout: &layout,
            private: options.private,
            trackers: &trackers,
            web_seeds: &options.web_seeds,
        },
        &options.allowed_lints,
    );
    if !findings.is_empty() {
        return Err(lint::refuse(&findings));
    }

    let (pieces, md5s) = hash_payload(&files, piece_length, options.md5, &mut open)?;
    let piece_count = layout.piece_count();

    let info = build_info(&files, &layout, piece_length, &pieces, &md5s, options);
    let info_bytes = bencode::encode(&info);
    let info_hash = InfoHash::of(&info_bytes);

    let mut meta = Metainfo::from_info_bytes(info_bytes)?;
    apply_outer_fields(&mut meta, options)?;
    let bytes = meta.write_to_vec()?;

    Ok(Created {
        bytes,
        info_hash,
        piece_length,
        piece_length_reason,
        piece_count,
        total_length,
        files,
    })
}

/// What one pass over the payload produces: the piece hashes, and one MD5 per
/// file when `--md5` asked for them.
type Hashed = (Vec<[u8; 20]>, Vec<Option<String>>);

/// Hash the payload into piece hashes, and optionally per-file MD5 sums.
///
/// Files are read back to back as one linear stream, which is what a piece
/// boundary crossing a file boundary means. The buffer is one piece long, so
/// peak memory does not depend on the payload size.
fn hash_payload<R: Read>(
    files: &[InputFile],
    piece_length: u32,
    want_md5: bool,
    open: &mut impl FnMut(&Path) -> Result<R>,
) -> Result<Hashed> {
    let mut pieces = Vec::new();
    let mut md5s = Vec::with_capacity(files.len());
    let mut piece = Vec::with_capacity(piece_length as usize);
    let mut buffer = vec![0u8; 256 * 1024];

    for file in files {
        let mut reader = open(&file.source)?;
        let mut md5 = want_md5.then(Md5::new);
        let mut read_total = 0u64;
        loop {
            let n = reader
                .read(&mut buffer)
                .map_err(|e| from_io(e, format!("cannot read {}", file.source.display())))?;
            if n == 0 {
                break;
            }
            read_total += n as u64;
            if let Some(md5) = &mut md5 {
                md5.update(&buffer[..n]);
            }
            let mut rest = &buffer[..n];
            while !rest.is_empty() {
                let want = piece_length as usize - piece.len();
                let take = want.min(rest.len());
                piece.extend_from_slice(&rest[..take]);
                rest = &rest[take..];
                if piece.len() == piece_length as usize {
                    pieces.push(sha1(&piece));
                    piece.clear();
                }
            }
        }
        // A file that changed size between the walk and the read would produce
        // a torrent that does not describe the data on disk, so refuse rather
        // than publish it.
        if read_total != file.length {
            return Err(Error::disk(format!(
                "{} was {} bytes when it was listed but {read_total} bytes when it was read",
                file.source.display(),
                file.length
            ))
            .with("path", file.path.clone())
            .with("expected_bytes", file.length)
            .with("actual_bytes", read_total));
        }
        md5s.push(md5.map(|m| m.finalize().iter().map(|b| format!("{b:02x}")).collect()));
    }

    if !piece.is_empty() {
        pieces.push(sha1(&piece));
    }
    Ok((pieces, md5s))
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn build_info(
    files: &[InputFile],
    layout: &Layout,
    piece_length: u32,
    pieces: &[[u8; 20]],
    md5s: &[Option<String>],
    options: &CreateOptions,
) -> Value {
    let mut info: BTreeMap<Vec<u8>, Value> = BTreeMap::new();
    info.insert(b"name".to_vec(), Value::text(options.name.clone()));
    info.insert(
        b"piece length".to_vec(),
        Value::Int(i64::from(piece_length)),
    );
    info.insert(
        b"pieces".to_vec(),
        Value::Bytes(pieces.iter().flat_map(|p| p.iter().copied()).collect()),
    );
    if options.private {
        info.insert(b"private".to_vec(), Value::Int(1));
    }
    if let Some(source) = &options.source {
        info.insert(b"source".to_vec(), Value::text(source.clone()));
    }

    if options.multi_file {
        let entries: Vec<Value> = layout
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let mut entry: BTreeMap<Vec<u8>, Value> = BTreeMap::new();
                entry.insert(b"length".to_vec(), Value::Int(file.length as i64));
                entry.insert(
                    b"path".to_vec(),
                    Value::List(file.path.iter().map(|c| Value::text(c.clone())).collect()),
                );
                if let Some(Some(md5)) = md5s.get(index) {
                    entry.insert(b"md5sum".to_vec(), Value::text(md5.clone()));
                }
                Value::Dict(entry)
            })
            .collect();
        info.insert(b"files".to_vec(), Value::List(entries));
    } else {
        let length = files.first().map(|f| f.length).unwrap_or(0);
        info.insert(b"length".to_vec(), Value::Int(length as i64));
        if let Some(Some(md5)) = md5s.first() {
            info.insert(b"md5sum".to_vec(), Value::text(md5.clone()));
        }
    }

    Value::Dict(info)
}

fn apply_outer_fields(meta: &mut Metainfo, options: &CreateOptions) -> Result<()> {
    let tiers: Vec<Vec<String>> = options
        .announce_tiers
        .iter()
        .filter(|t| !t.is_empty())
        .cloned()
        .collect();
    if let Some(first) = tiers.first().and_then(|t| t.first()) {
        meta.set("announce", Some(Value::text(first.clone())))?;
    }
    // A single tracker needs no `announce-list`; writing one anyway is noise
    // that some older clients handle badly.
    if tiers.len() > 1 || tiers.first().is_some_and(|t| t.len() > 1) {
        meta.set(
            "announce-list",
            Some(Value::List(
                tiers
                    .iter()
                    .map(|tier| Value::List(tier.iter().map(|u| Value::text(u.clone())).collect()))
                    .collect(),
            )),
        )?;
    }
    if !options.web_seeds.is_empty() {
        meta.set(
            "url-list",
            Some(Value::List(
                options
                    .web_seeds
                    .iter()
                    .map(|u| Value::text(u.clone()))
                    .collect(),
            )),
        )?;
    }
    if !options.http_seeds.is_empty() {
        meta.set(
            "httpseeds",
            Some(Value::List(
                options
                    .http_seeds
                    .iter()
                    .map(|u| Value::text(u.clone()))
                    .collect(),
            )),
        )?;
    }
    if !options.nodes.is_empty() {
        let nodes: Result<Vec<Value>> = options
            .nodes
            .iter()
            .map(|node| {
                let (host, port) = node.rsplit_once(':').ok_or_else(|| {
                    Error::usage(format!("`{node}` is not a DHT node (expected host:port)"))
                })?;
                let port: u16 = port.parse().map_err(|_| {
                    Error::usage(format!("`{node}` has a port that is not a number"))
                })?;
                Ok(Value::List(vec![
                    Value::text(host.to_string()),
                    Value::Int(i64::from(port)),
                ]))
            })
            .collect();
        meta.set("nodes", Some(Value::List(nodes?)))?;
    }
    if let Some(comment) = &options.comment {
        meta.set("comment", Some(Value::text(comment.clone())))?;
    }
    if let Some(created_by) = &options.created_by {
        meta.set("created by", Some(Value::text(created_by.clone())))?;
    }
    if let Some(when) = options.creation_date {
        meta.set("creation date", Some(Value::Int(when.epoch_secs())))?;
    }
    if let Some(url) = &options.update_url {
        meta.set("update-url", Some(Value::text(url.clone())))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// An in-memory payload: path in the torrent, and its bytes.
    fn inputs(files: &[(&str, &[u8])]) -> (Vec<InputFile>, BTreeMap<PathBuf, Vec<u8>>) {
        let mut list = Vec::new();
        let mut data = BTreeMap::new();
        for (path, bytes) in files {
            let source = PathBuf::from(path);
            list.push(InputFile {
                source: source.clone(),
                path: path.to_string(),
                length: bytes.len() as u64,
            });
            data.insert(source, bytes.to_vec());
        }
        (list, data)
    }

    fn opener(data: BTreeMap<PathBuf, Vec<u8>>) -> impl FnMut(&Path) -> Result<Cursor<Vec<u8>>> {
        move |path: &Path| {
            data.get(path)
                .cloned()
                .map(Cursor::new)
                .ok_or_else(|| Error::disk(format!("no such fixture: {}", path.display())))
        }
    }

    fn options(name: &str, multi_file: bool) -> CreateOptions {
        CreateOptions {
            name: name.to_string(),
            multi_file,
            piece_length: Some(16 * 1024),
            announce_tiers: vec![vec!["udp://tracker.example.com:80".to_string()]],
            created_by: None,
            creation_date: None,
            allowed_lints: Lint::ALL.iter().copied().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_single_file_torrent_is_created_and_parses_back() {
        let (files, data) = inputs(&[("payload.bin", &vec![7u8; 40_000])]);
        let created = create(files, &options("payload.bin", false), opener(data)).unwrap();

        let meta = Metainfo::parse(&created.bytes).unwrap();
        assert_eq!(meta.info_hash(), created.info_hash);
        assert_eq!(meta.info().name, "payload.bin");
        assert_eq!(meta.info().piece_length, 16 * 1024);
        assert_eq!(meta.info().total_length(), 40_000);
        assert!(!meta.info().multi_file);
        assert_eq!(meta.info().pieces.len(), 3);
        assert_eq!(created.piece_count, 3);
        assert_eq!(
            meta.announce().as_deref(),
            Some("udp://tracker.example.com:80")
        );
    }

    #[test]
    fn a_multi_file_torrent_records_every_path() {
        let (files, data) = inputs(&[
            ("disc 1/a.flac", &vec![1u8; 20_000]),
            ("notes.nfo", &vec![2u8; 5_000]),
        ]);
        let created = create(files, &options("album", true), opener(data)).unwrap();

        let meta = Metainfo::parse(&created.bytes).unwrap();
        assert!(meta.info().multi_file);
        assert_eq!(meta.info().files.len(), 2);
        assert_eq!(meta.info().files[0].path, ["disc 1", "a.flac"]);
        assert_eq!(meta.info().total_length(), 25_000);
    }

    #[test]
    fn piece_hashes_span_file_boundaries() {
        // Two files of 10_000 bytes with a 16 KiB piece length: piece 0 covers
        // all of file 0 and the first 6_384 bytes of file 1.
        let (files, data) = inputs(&[
            ("a.bin", &vec![0xAAu8; 10_000]),
            ("b.bin", &vec![0xBBu8; 10_000]),
        ]);
        let created = create(files, &options("t", true), opener(data)).unwrap();
        assert_eq!(created.piece_count, 2);

        let mut expected = vec![0xAAu8; 10_000];
        expected.extend(vec![0xBBu8; 6_384]);
        let meta = Metainfo::parse(&created.bytes).unwrap();
        assert_eq!(meta.info().pieces[0], sha1(&expected));
    }

    #[test]
    fn the_same_input_always_produces_the_same_bytes() {
        let build = || {
            let (files, data) =
                inputs(&[("b.bin", &vec![2u8; 20_000]), ("a.bin", &vec![1u8; 20_000])]);
            create(files, &options("t", true), opener(data))
                .unwrap()
                .bytes
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn input_order_does_not_change_the_result() {
        let forwards = {
            let (files, data) =
                inputs(&[("a.bin", &vec![1u8; 20_000]), ("b.bin", &vec![2u8; 20_000])]);
            create(files, &options("t", true), opener(data)).unwrap()
        };
        let backwards = {
            let (files, data) =
                inputs(&[("b.bin", &vec![2u8; 20_000]), ("a.bin", &vec![1u8; 20_000])]);
            create(files, &options("t", true), opener(data)).unwrap()
        };
        assert_eq!(forwards.info_hash, backwards.info_hash);
        assert_eq!(forwards.bytes, backwards.bytes);
    }

    #[test]
    fn a_creation_date_is_the_only_thing_that_makes_two_runs_differ() {
        let (files, data) = inputs(&[("a.bin", &vec![1u8; 20_000])]);
        let mut with_date = options("a.bin", false);
        with_date.creation_date = Some(Timestamp::from_epoch_secs(1_787_140_323));
        let dated = create(files.clone(), &with_date, opener(data.clone())).unwrap();
        let undated = create(files, &options("a.bin", false), opener(data)).unwrap();

        assert_ne!(dated.bytes, undated.bytes);
        assert_eq!(
            dated.info_hash, undated.info_hash,
            "the date lives outside info"
        );
    }

    #[test]
    fn sorting_by_size_changes_the_order_and_the_hash() {
        let by_path = {
            let (files, data) =
                inputs(&[("a.bin", &vec![1u8; 30_000]), ("b.bin", &vec![2u8; 10_000])]);
            create(files, &options("t", true), opener(data)).unwrap()
        };
        let by_size = {
            let (files, data) =
                inputs(&[("a.bin", &vec![1u8; 30_000]), ("b.bin", &vec![2u8; 10_000])]);
            let mut opts = options("t", true);
            opts.sort_by = SortBy {
                key: SortKey::Size,
                order: SortOrder::Ascending,
            };
            create(files, &opts, opener(data)).unwrap()
        };
        let path_first = Metainfo::parse(&by_path.bytes).unwrap();
        let size_first = Metainfo::parse(&by_size.bytes).unwrap();
        assert_eq!(path_first.info().files[0].path, ["a.bin"]);
        assert_eq!(size_first.info().files[0].path, ["b.bin"]);
        assert_ne!(by_path.info_hash, by_size.info_hash);
    }

    #[test]
    fn sort_specs_parse_in_every_documented_form() {
        assert_eq!(SortBy::parse("path").unwrap(), SortBy::default());
        assert_eq!(
            SortBy::parse("size:desc").unwrap(),
            SortBy {
                key: SortKey::Size,
                order: SortOrder::Descending
            }
        );
        assert_eq!(SortBy::parse("PATH:ASC").unwrap(), SortBy::default());
        assert!(SortBy::parse("mtime").is_err());
        assert!(SortBy::parse("path:sideways").is_err());
    }

    #[test]
    fn lints_refuse_a_bad_torrent_and_allow_lets_it_through() {
        let (files, data) = inputs(&[("CON", &vec![1u8; 20_000])]);
        let mut opts = options("t", true);
        opts.allowed_lints.clear();
        let err = create(files.clone(), &opts, opener(data.clone())).unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::LintRefused);

        opts.allowed_lints = [Lint::WindowsPath, Lint::PieceCount].into_iter().collect();
        assert!(create(files, &opts, opener(data)).is_ok());
    }

    #[test]
    fn a_file_that_changes_size_under_us_is_refused() {
        let (mut files, data) = inputs(&[("a.bin", &vec![1u8; 20_000])]);
        files[0].length = 30_000;
        let err = create(files, &options("t", false), opener(data)).unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Disk);
        assert_eq!(err.context()["expected_bytes"], 30_000);
        assert_eq!(err.context()["actual_bytes"], 20_000);
    }

    #[test]
    fn web_seeds_trackers_and_nodes_are_written_and_read_back() {
        let (files, data) = inputs(&[("a.bin", &vec![1u8; 20_000])]);
        let mut opts = options("a.bin", false);
        opts.announce_tiers = vec![
            vec!["udp://a:80".to_string(), "udp://b:80".to_string()],
            vec!["udp://c:80".to_string()],
        ];
        opts.web_seeds = vec!["https://mirror.example.com/pub/".to_string()];
        opts.http_seeds = vec!["https://old.example.com/".to_string()];
        opts.nodes = vec!["dht.example.com:6881".to_string()];
        opts.comment = Some("hello".to_string());
        opts.update_url = Some("https://e.com/feed".to_string());

        let created = create(files, &opts, opener(data)).unwrap();
        let meta = Metainfo::parse(&created.bytes).unwrap();
        assert_eq!(meta.announce_tiers().len(), 2);
        assert_eq!(meta.trackers().len(), 3);
        assert_eq!(meta.url_list(), ["https://mirror.example.com/pub/"]);
        assert_eq!(meta.http_seeds(), ["https://old.example.com/"]);
        assert_eq!(meta.nodes(), ["dht.example.com:6881"]);
        assert_eq!(meta.comment().as_deref(), Some("hello"));
        assert_eq!(meta.update_url().as_deref(), Some("https://e.com/feed"));
    }

    #[test]
    fn one_tracker_writes_announce_without_an_announce_list() {
        let (files, data) = inputs(&[("a.bin", &vec![1u8; 20_000])]);
        let created = create(files, &options("a.bin", false), opener(data)).unwrap();
        let meta = Metainfo::parse(&created.bytes).unwrap();
        assert!(meta.root().get("announce").is_some());
        assert!(meta.root().get("announce-list").is_none());
    }

    #[test]
    fn private_and_source_live_inside_info_and_change_the_hash() {
        let plain = {
            let (files, data) = inputs(&[("a.bin", &vec![1u8; 20_000])]);
            create(files, &options("a.bin", false), opener(data)).unwrap()
        };
        let private = {
            let (files, data) = inputs(&[("a.bin", &vec![1u8; 20_000])]);
            let mut opts = options("a.bin", false);
            opts.private = true;
            create(files, &opts, opener(data)).unwrap()
        };
        let sourced = {
            let (files, data) = inputs(&[("a.bin", &vec![1u8; 20_000])]);
            let mut opts = options("a.bin", false);
            opts.source = Some("TRACKER".to_string());
            create(files, &opts, opener(data)).unwrap()
        };
        assert_ne!(plain.info_hash, private.info_hash);
        assert_ne!(plain.info_hash, sourced.info_hash);
        assert!(Metainfo::parse(&private.bytes).unwrap().info().private);
        assert_eq!(
            Metainfo::parse(&sourced.bytes)
                .unwrap()
                .info()
                .source
                .as_deref(),
            Some("TRACKER")
        );
    }

    #[test]
    fn md5_sums_are_written_per_file_when_asked_for() {
        let (files, data) = inputs(&[("a.bin", b"hello"), ("b.bin", b"world")]);
        let mut opts = options("t", true);
        opts.md5 = true;
        opts.piece_length = Some(16 * 1024);
        let created = create(files, &opts, opener(data)).unwrap();
        let meta = Metainfo::parse(&created.bytes).unwrap();
        // md5("hello")
        assert_eq!(
            meta.info().files[0].md5sum.as_deref(),
            Some("5d41402abc4b2a76b9719d911017c592")
        );
        assert!(meta.info().files[1].md5sum.is_some());
    }

    #[test]
    fn the_piece_length_is_chosen_when_it_is_not_given() {
        let (files, data) = inputs(&[("a.bin", &vec![1u8; 100_000])]);
        let mut opts = options("a.bin", false);
        opts.piece_length = None;
        let created = create(files, &opts, opener(data)).unwrap();
        assert!(created.piece_length.is_power_of_two());
        assert!(
            created.piece_length_reason.contains("pieces"),
            "{}",
            created.piece_length_reason
        );
    }

    #[test]
    fn paths_are_sorted_by_raw_bytes_not_by_locale() {
        // Uppercase sorts before lowercase in byte order. A locale-aware sort
        // would interleave them and break cross-platform reproducibility.
        let (files, data) = inputs(&[
            ("b.bin", b"1"),
            ("A.bin", b"2"),
            ("a.bin", b"3"),
            ("B.bin", b"4"),
        ]);
        let mut opts = options("t", true);
        opts.allowed_lints = Lint::ALL.iter().copied().collect();
        let created = create(files, &opts, opener(data)).unwrap();
        let meta = Metainfo::parse(&created.bytes).unwrap();
        let paths: Vec<String> = meta.info().files.iter().map(|f| f.path.join("/")).collect();
        assert_eq!(paths, ["A.bin", "B.bin", "a.bin", "b.bin"]);
    }
}
