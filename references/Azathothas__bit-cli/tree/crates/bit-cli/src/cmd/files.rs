//! `bit-cli files`: list the files in a torrent.

use bit_cli_core::ExitCode;
use bit_cli_core::error::{Error, Result};
use bit_cli_core::units::{Size, format_size, percent_of};
use serde::Serialize;

use crate::cli::{FilesArgs, Global};
use crate::env::Env;
use crate::output::{Renderer, table};
use crate::source::{Kind, resolve_source};

/// One row of the listing.
#[derive(Debug, Clone, Serialize)]
pub struct FileRow {
    pub index: usize,
    pub path: String,
    pub size: Size,
    /// Byte offset within the torrent's linear payload.
    pub offset: u64,
    /// Piece indices this file touches, as a half-open range.
    pub first_piece: u32,
    pub last_piece: u32,
    /// Share of the whole payload, to two decimal places.
    pub share: String,
    /// Whether this is a BEP 47 padding file, which carries no real data.
    pub padding: bool,
    /// The same file in a torrent named by `--against`, and what says so.
    /// Empty unless `--against` was given. See `TODO/multi-source.md`, T-133.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<Shared>,
}

/// One file of another torrent that holds the same bytes as this one.
#[derive(Debug, Clone, Serialize)]
pub struct Shared {
    /// The other torrent, as it was named on the command line.
    pub torrent: String,
    pub info_hash: String,
    pub index: usize,
    pub path: String,
    /// `piece-hashes` when the pieces line up and agree, `length` when the
    /// size is all that could be compared.
    pub evidence: &'static str,
    /// Whether the evidence is a proof rather than a candidate.
    pub proven: bool,
    pub pieces_compared: u32,
    pub bytes_proven: Size,
}

/// What `bit-cli files` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub info_hash: String,
    pub name: String,
    pub total: Size,
    pub file_count: usize,
    /// How this torrent's `name` and `path` keys were turned into text.
    ///
    /// Absent for the ordinary torrent, whose names are UTF-8 and where there
    /// was nothing to choose. Present when the names were decoded through a
    /// detected encoding, or when a `.utf-8` key was preferred over the raw
    /// key beside it. Either way the same rule named the files this run would
    /// write, so this is what the paths above are in rather than a guess about
    /// them. See `TODO/bep-coverage.md`, T-103.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_encoding: Option<bit_cli_core::torrent::NameEncoding>,
    pub files: Vec<FileRow>,
}

impl Report {
    /// The text rendering.
    pub fn lines(&self) -> Vec<String> {
        let rows: Vec<Vec<String>> = self
            .files
            .iter()
            .map(|f| {
                vec![
                    f.index.to_string(),
                    format_size(f.size.0),
                    f.share.clone(),
                    format!("{}-{}", f.first_piece, f.last_piece),
                    f.path.clone(),
                ]
            })
            .collect();
        let mut out = table(&["INDEX", "SIZE", "SHARE", "PIECES", "PATH"], &rows);
        if let Some(encoding) = &self.name_encoding {
            out.push(String::new());
            out.push(format!("names decoded as {}", encoding.describe()));
        }

        // A second section rather than a sixth column: a file can match
        // several files in several torrents, and one row per match reads
        // better than a cell holding a list.
        let shared: Vec<Vec<String>> = self
            .files
            .iter()
            .flat_map(|f| {
                f.shared.iter().map(move |s| {
                    vec![
                        f.index.to_string(),
                        s.evidence.to_string(),
                        match s.proven {
                            true => format_size(s.bytes_proven.0),
                            false => "-".to_string(),
                        },
                        format!("{}:{}", &s.info_hash[..8.min(s.info_hash.len())], s.index),
                        s.path.clone(),
                    ]
                })
            })
            .collect();
        if !shared.is_empty() {
            out.push(String::new());
            out.extend(table(
                &["INDEX", "EVIDENCE", "PROVEN", "OTHER", "OTHER PATH"],
                &shared,
            ));
        }
        out
    }
}

/// A sort key for the listing.
fn sort_rows(rows: &mut [FileRow], spec: &str) -> Result<()> {
    let (key, order) = spec.split_once(':').unwrap_or((spec, "asc"));
    let descending = match order.trim().to_ascii_lowercase().as_str() {
        "asc" | "ascending" => false,
        "desc" | "descending" => true,
        other => {
            return Err(Error::usage(format!(
                "`{other}` is not a sort order (use asc or desc)"
            )));
        }
    };
    match key.trim().to_ascii_lowercase().as_str() {
        "index" => rows.sort_by_key(|r| r.index),
        "path" | "name" => rows.sort_by(|a, b| a.path.cmp(&b.path)),
        "size" | "length" => {
            rows.sort_by(|a, b| a.size.0.cmp(&b.size.0).then(a.index.cmp(&b.index)))
        }
        other => {
            return Err(Error::usage(format!(
                "`{other}` is not a sort key for `files` (use index, path, or size)"
            )));
        }
    }
    if descending {
        rows.reverse();
    }
    Ok(())
}

/// Run the command.
pub fn run(
    args: &FilesArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let kind = Kind::classify(&args.source.source, env)?;
    let meta = resolve_source(&kind, env, global, None, &args.swarm, &args.page)?;
    let layout = meta.layout();
    let total = layout.total_length;

    // Every comparison torrent is read before anything is printed, so a
    // mistyped path fails rather than producing a listing with a gap in it.
    let mut others = Vec::with_capacity(args.against.len());
    for source in &args.against {
        let kind = Kind::classify(source, env)?;
        let other = resolve_source(&kind, env, global, None, &args.swarm, &args.page)?;
        others.push((source.clone(), other));
    }

    let mut files: Vec<FileRow> = layout
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let pieces = layout.pieces_overlapping(&file.range());
            FileRow {
                index,
                path: file.display_path(),
                size: Size(file.length),
                offset: file.offset,
                first_piece: pieces.start,
                last_piece: pieces.end.saturating_sub(1),
                share: percent_of(file.length, total),
                padding: meta.info().files.get(index).is_some_and(|f| f.is_padding()),
                shared: Vec::new(),
            }
        })
        .collect();

    for (source, other) in &others {
        let other_layout = other.layout();
        let found = bit_cli_core::equivalence::matches(
            &layout,
            &meta.info().pieces,
            &other_layout,
            &other.info().pieces,
        );
        for one in found {
            let Some(row) = files.get_mut(one.index) else {
                continue;
            };
            row.shared.push(Shared {
                torrent: source.clone(),
                info_hash: other.info_hash().hex(),
                index: one.other_index,
                path: one.other_path,
                evidence: one.evidence.as_str(),
                proven: one.evidence.is_proof(),
                pieces_compared: one.pieces_compared,
                bytes_proven: Size(one.bytes_proven),
            });
        }
    }

    sort_rows(&mut files, &args.sort)?;

    let report = Report {
        info_hash: meta.info_hash().hex(),
        name: meta.info().name.clone(),
        total: Size(total),
        file_count: files.len(),
        name_encoding: crate::cmd::info::reportable_name_encoding(meta.info().name_encoding),
        files,
    };
    renderer.emit(env, "files", &report, || report.lines())?;
    Ok(ExitCode::Success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TorrentFixture, run_err, run_json, run_ok};

    /// `TODO/bep-coverage.md`, T-103, on the command whose whole output is
    /// paths.
    #[test]
    fn a_path_that_is_not_utf8_is_listed_decoded() {
        let fixture = TorrentFixture::names_that_are_not_utf8();
        let doc = run_json(&["files", fixture.path_str()], fixture.dir());
        assert_eq!(doc["files"][0]["path"], "曲.bin");
        assert_eq!(doc["name_encoding"]["utf8_keys"], true);

        let text = run_ok(&["files", fixture.path_str()], fixture.dir());
        assert!(text.contains("曲.bin"), "{text}");
        assert!(text.contains("names decoded as"), "{text}");
    }

    #[test]
    fn files_lists_every_file_with_its_index() {
        let fixture = TorrentFixture::multi_file();
        let out = run_ok(&["files", fixture.path_str()], fixture.dir());
        assert!(out.contains("disc 1/a.flac"), "{out}");
        assert!(out.contains("notes.nfo"), "{out}");
        assert!(out.starts_with("INDEX"), "{out}");
    }

    #[test]
    fn the_json_form_carries_raw_bytes_alongside_the_human_string() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(&["files", fixture.path_str()], fixture.dir());
        let files = doc["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["index"], 0);
        assert_eq!(files[0]["path"], "disc 1/a.flac");
        assert_eq!(files[0]["size"]["bytes"], 1500);
        assert_eq!(files[0]["size"]["human"], "1.46 KiB");
        assert_eq!(files[0]["offset"], 0);
        assert_eq!(files[1]["offset"], 1500);
    }

    #[test]
    fn piece_ranges_show_which_pieces_a_file_touches() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(&["files", fixture.path_str()], fixture.dir());
        let files = doc["files"].as_array().unwrap();
        // 0..1500 with 1024 byte pieces touches pieces 0 and 1.
        assert_eq!(files[0]["first_piece"], 0);
        assert_eq!(files[0]["last_piece"], 1);
        // 1500..2000 lies entirely inside piece 1.
        assert_eq!(files[1]["first_piece"], 1);
        assert_eq!(files[1]["last_piece"], 1);
    }

    #[test]
    fn shares_add_up_to_the_whole_payload() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(&["files", fixture.path_str()], fixture.dir());
        let total: f64 = doc["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                f["share"]
                    .as_str()
                    .unwrap()
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .unwrap()
            })
            .sum();
        assert!((total - 100.0).abs() < 0.01, "shares summed to {total}");
    }

    #[test]
    fn sorting_by_size_reorders_the_listing() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &["files", "--sort", "size", fixture.path_str()],
            fixture.dir(),
        );
        let files = doc["files"].as_array().unwrap();
        assert_eq!(files[0]["path"], "notes.nfo", "smallest first");

        let doc = run_json(
            &["files", "--sort", "size:desc", fixture.path_str()],
            fixture.dir(),
        );
        let files = doc["files"].as_array().unwrap();
        assert_eq!(files[0]["path"], "disc 1/a.flac", "largest first");
    }

    #[test]
    fn a_bad_sort_key_is_a_usage_error_that_names_the_valid_keys() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &["files", "--sort", "mtime", fixture.path_str()],
            fixture.dir(),
            ExitCode::Usage,
        );
        assert!(err.contains("index, path, or size"), "{err}");
    }

    #[test]
    fn a_single_file_torrent_lists_one_file() {
        let fixture = TorrentFixture::single_file();
        let doc = run_json(&["files", fixture.path_str()], fixture.dir());
        assert_eq!(doc["file_count"], 1);
        assert_eq!(doc["files"][0]["path"], "payload.bin");
        assert_eq!(doc["files"][0]["share"], "100.00%");
    }

    /// A file that another torrent holds identically is reported with the
    /// evidence behind it.
    ///
    /// Both fixtures start with the same 1500 byte file at offset 0 under the
    /// same 1024 byte piece length, so the first piece lines up and its hash
    /// is the proof. The second file differs in both, so it is not a match at
    /// all. See `TODO/multi-source.md`, T-133.
    #[test]
    fn against_reports_a_shared_file_and_what_proves_it() {
        let mine = TorrentFixture::multi_file();
        let theirs = TorrentFixture::multi_file_with_a_different_tail();
        let doc = run_json(
            &["files", mine.path_str(), "--against", theirs.path_str()],
            mine.dir(),
        );
        let shared = doc["files"][0]["shared"].as_array().expect("an array");
        assert_eq!(shared.len(), 1, "{}", doc["files"][0]);
        assert_eq!(shared[0]["evidence"], "piece-hashes");
        assert_eq!(shared[0]["proven"], true);
        assert_eq!(shared[0]["index"], 0);
        assert_eq!(shared[0]["path"], "disc 1/a.flac");
        assert_eq!(shared[0]["pieces_compared"], 1);
        assert_eq!(shared[0]["bytes_proven"]["bytes"], 1024);
        assert_eq!(shared[0]["info_hash"], theirs.info_hash);

        // The second file is 500 bytes here and 900 there, so nothing matches.
        assert!(doc["files"][1]["shared"].is_null(), "{}", doc["files"][1]);
    }

    /// Without the flag the field is absent, so an ordinary listing is
    /// unchanged.
    #[test]
    fn a_listing_without_against_carries_no_shared_field() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(&["files", fixture.path_str()], fixture.dir());
        assert!(doc["files"][0]["shared"].is_null(), "{doc}");
    }

    /// A comparison torrent that cannot be read fails the command rather than
    /// producing a listing with a gap in it.
    #[test]
    fn an_unreadable_comparison_torrent_fails_the_command() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &["files", fixture.path_str(), "--against", "nope.torrent"],
            fixture.dir(),
            ExitCode::SourceResolution,
        );
        assert!(err.contains("nope.torrent"), "{err}");
    }
}
