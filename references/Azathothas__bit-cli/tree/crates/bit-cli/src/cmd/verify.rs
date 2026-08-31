//! `bit-cli verify`: hash-check data on disk against a torrent.
//!
//! Every piece is read from the payload and hashed. A piece that spans a file
//! boundary is read across it, and a file that is missing or short is treated
//! as zero bytes rather than aborting, so one absent file does not hide the
//! state of everything else.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use bit_cli_core::ExitCode;
use bit_cli_core::error::{Error, Result, from_io};
use bit_cli_core::layout::Layout;
use bit_cli_core::span::summarize_indices;
use bit_cli_core::torrent::Metainfo;
use bit_cli_core::units::{Size, format_size, percent_of};
use serde::Serialize;
use sha1::{Digest, Sha1};

use crate::cli::{Global, VerifyArgs};
use crate::env::Env;
use crate::output::{Renderer, field};
use crate::source::{Kind, resolve_source};

/// One piece's result, when `--per-piece` is given.
#[derive(Debug, Clone, Serialize)]
pub struct PieceResult {
    pub piece: u32,
    pub ok: bool,
    pub bytes: u64,
    /// Whether the selection covers this piece. Absent without a selection,
    /// where every piece is covered. See `TODO/disk-io.md`, T-184.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub not_selected: bool,
}

/// One file's result.
#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
    pub index: usize,
    pub path: String,
    pub expected: Size,
    pub found: Size,
    pub present: bool,
}

/// What `bit-cli verify` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub info_hash: String,
    pub name: String,
    pub data_dir: String,
    pub total: Size,
    /// Bytes the pieces a selection covers hold, when one was given.
    ///
    /// `have_share` is measured against this rather than against `total`: a
    /// selection that verified perfectly is complete, and reporting it as a
    /// share of the whole torrent would say 57 per cent of a run that got
    /// everything it asked for. See `TODO/disk-io.md`, T-184.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<Size>,
    pub piece_count: u32,
    pub pieces_ok: u32,
    pub pieces_bad: u32,
    pub complete: bool,
    pub have: Size,
    pub have_share: String,
    pub bad_pieces: Vec<u32>,
    /// Pieces the selection does not cover, when one was given.
    ///
    /// These are neither ok nor bad. Nothing was ever asked to fetch them, so
    /// the bytes on disk are whatever a boundary piece happened to write and a
    /// hash over them means nothing. Verifying what
    /// `download --select-file` wrote without saying so reports every one of
    /// them as a failure, which is true of the bytes and wrong about the run.
    /// See `TODO/disk-io.md`, T-184.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub not_selected: Vec<u32>,
    pub files: Vec<FileResult>,
    /// Files whose on-disk path is not the path in the torrent, and why.
    ///
    /// The same array `download --json` reports, because this command reads
    /// the files that command wrote. Absent when nothing changed, which is the
    /// common case. See `bit_cli_core::paths`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub renamed: Vec<bit_cli_core::paths::Rename>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub per_piece: Vec<PieceResult>,
}

impl Report {
    /// The text rendering.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field("torrent", &self.name),
            field("info hash", &self.info_hash),
            field("data", &self.data_dir),
            field(
                "pieces ok",
                match self.not_selected.is_empty() {
                    true => format!("{} of {}", self.pieces_ok, self.piece_count),
                    false => format!("{} of {} selected", self.pieces_ok, self.piece_count),
                },
            ),
            field(
                "have",
                format!("{} ({})", format_size(self.have.0), self.have_share),
            ),
            field("complete", self.complete),
        ];
        if !self.bad_pieces.is_empty() {
            out.push(field("failed pieces", summarize_indices(&self.bad_pieces)));
        }
        // Said separately from the failures, because they are a different
        // fact: nothing was ever asked to fetch these, so whatever is on disk
        // for them is not evidence about anything.
        if !self.not_selected.is_empty() {
            out.push(field("not selected", summarize_indices(&self.not_selected)));
        }
        // A caller that does not know a file was renamed is looking in the
        // wrong place, which reads as a missing file rather than as a rename.
        for rename in &self.renamed {
            out.push(field(
                &format!("renamed [{}]", rename.index),
                format!("{} -> {}", rename.torrent_path, rename.disk_path),
            ));
        }
        for file in &self.files {
            if !file.present {
                out.push(field("missing", &file.path));
            } else if file.found.0 != file.expected.0 {
                out.push(field(
                    "short",
                    format!(
                        "{} ({} of {})",
                        file.path,
                        format_size(file.found.0),
                        format_size(file.expected.0)
                    ),
                ));
            }
        }
        out
    }
}

/// Reads the torrent's linear byte stream out of the files on disk.
///
/// Files are opened lazily and kept open, because a piece usually spans the
/// same one or two files as the previous piece and reopening per piece would
/// dominate the run.
struct PayloadReader<'a> {
    layout: &'a Layout,
    root: PathBuf,
    /// Where each file was actually written.
    ///
    /// A download plans every torrent path before it opens anything, so a name
    /// the filesystem refuses is written under a different one. Verifying the
    /// result means looking where the bytes went, not where the torrent said
    /// they would go: before this, a hostile torrent verified against paths
    /// that do not exist and reported every file missing. See
    /// `bit_cli_core::paths` and `TODO/windows.md`, T-076.
    plan: bit_cli_core::paths::PathPlan,
    open: Vec<Option<Option<std::fs::File>>>,
}

impl<'a> PayloadReader<'a> {
    fn new(
        layout: &'a Layout,
        root: PathBuf,
        index_out: &std::collections::BTreeMap<usize, String>,
    ) -> Self {
        let open = (0..layout.files.len()).map(|_| None).collect();
        // The same plan the download made, including whatever it was told with
        // `-O`. A file the caller renamed is somewhere only the caller knows,
        // so verifying it means being told the same thing the download was.
        // See `TODO/cli-surface.md`, T-116.
        let plan = bit_cli_core::paths::plan_with(
            &layout
                .files
                .iter()
                .map(|file| file.path.join("/"))
                .collect::<Vec<_>>(),
            index_out,
        );
        Self {
            layout,
            root,
            plan,
            open,
        }
    }

    /// The files whose on-disk path is not the path in the torrent.
    fn renames(&self) -> Vec<bit_cli_core::paths::Rename> {
        self.plan.renames.clone()
    }

    /// The on-disk path of one file.
    fn path_of(&self, index: usize) -> Option<PathBuf> {
        let relative = self.plan.disk_paths.get(index)?;
        let mut path = self.root.clone();
        // Components are pushed one at a time. Joining a whole string would
        // hand the platform's path parser something it might read as a root,
        // which the plan has already made impossible, but this is the line
        // where it would matter.
        for component in relative.split('/').filter(|c| !c.is_empty()) {
            path.push(component);
        }
        Some(path)
    }

    /// Read one byte range of the payload, zero-filling anything missing.
    fn read(&mut self, offset: u64, length: u64) -> Result<Vec<u8>> {
        let mut out = vec![0u8; length as usize];
        for slice in self.layout.split_by_file(offset..offset + length) {
            let start = (slice.file_start(self.layout, offset)) as usize;
            let handle = self.handle(slice.file)?;
            let Some(file) = handle else { continue };
            file.seek(SeekFrom::Start(slice.offset))
                .map_err(|e| from_io(e, "cannot seek in the payload"))?;
            let end = start + slice.length as usize;
            // A short read is not an error: it means the file on disk is
            // shorter than the torrent says, and the piece will simply fail
            // its hash, which is the honest answer.
            let mut filled = start;
            while filled < end {
                match file.read(&mut out[filled..end]) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(e) => return Err(from_io(e, "cannot read the payload")),
                }
            }
        }
        Ok(out)
    }

    fn handle(&mut self, index: usize) -> Result<Option<&mut std::fs::File>> {
        if self.open[index].is_none() {
            let opened = match self.path_of(index) {
                None => None,
                Some(path) => std::fs::File::open(&path).ok(),
            };
            self.open[index] = Some(opened);
        }
        Ok(self.open[index].as_mut().and_then(|slot| slot.as_mut()))
    }
}

/// Where a file slice lands in the output buffer.
trait SliceOffset {
    fn file_start(&self, layout: &Layout, request_start: u64) -> u64;
}

impl SliceOffset for bit_cli_core::layout::FileSlice {
    fn file_start(&self, layout: &Layout, request_start: u64) -> u64 {
        let absolute = layout.file(self.file).map(|f| f.offset).unwrap_or(0) + self.offset;
        absolute.saturating_sub(request_start)
    }
}

/// Run the command.
pub fn run(
    args: &VerifyArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let kind = Kind::classify(&args.source.source, env)?;
    let meta = resolve_source(&kind, env, global, None, &args.swarm, &args.page)?;
    let layout = meta.layout();

    let index_out = crate::selection::index_out(&args.index_out, Some(layout.files.len()))?;
    let root = resolve_root(args, global, env, &meta, &index_out);
    let mut reader = PayloadReader::new(&layout, root.clone(), &index_out);

    let files: Vec<FileResult> = layout
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let found = reader
                .path_of(index)
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len());
            FileResult {
                index,
                path: file.display_path(),
                expected: Size(file.length),
                found: Size(found.unwrap_or(0)),
                present: found.is_some(),
            }
        })
        .collect();

    // A selection given here is the same selection a `--select-file` download
    // was given. The pieces it covers are the ones that run was ever asked to
    // fetch, so they are the only ones whose hashes say anything about it.
    // Unlike `download`, this command has the metainfo on disk before it
    // parses a flag, so an exclusion on its own resolves to its complement
    // here. See `TODO/cli-surface.md`, T-185.
    let selected = crate::selection::resolve(
        &args.select_file,
        &args.exclude_file,
        Some(layout.files.len()),
    )?;
    let wanted: Option<std::collections::HashSet<u32>> = selected
        .as_ref()
        .map(|files| layout.pieces_for_selection(files).into_iter().collect());

    let mut pieces_ok = 0u32;
    let mut bad_pieces = Vec::new();
    let mut not_selected = Vec::new();
    let mut per_piece = Vec::new();
    let mut have = 0u64;

    for piece in 0..layout.piece_count() {
        let Some(range) = layout.piece_range(piece) else {
            continue;
        };
        let length = range.end - range.start;
        // A piece outside the selection is not read at all. Reading it would
        // cost a hash over bytes nobody asked for, and on a large torrent
        // verified one file at a time that is the whole payload.
        if wanted.as_ref().is_some_and(|set| !set.contains(&piece)) {
            not_selected.push(piece);
            if args.per_piece {
                per_piece.push(PieceResult {
                    piece,
                    ok: false,
                    bytes: length,
                    not_selected: true,
                });
            }
            continue;
        }
        let data = reader.read(range.start, length)?;
        let expected =
            meta.info().pieces.get(piece as usize).ok_or_else(|| {
                Error::generic(format!("the torrent has no hash for piece {piece}"))
            })?;
        let mut hasher = Sha1::new();
        hasher.update(&data);
        let actual: [u8; 20] = hasher.finalize().into();
        let ok = &actual == expected;
        if ok {
            pieces_ok += 1;
            have += length;
        } else {
            bad_pieces.push(piece);
        }
        if args.per_piece {
            per_piece.push(PieceResult {
                piece,
                ok,
                bytes: length,
                not_selected: false,
            });
        }
    }

    // The denominator is what was asked for. Without this a selection that
    // verified perfectly would report `pieces ok 2 of 4` and exit non-zero,
    // which is the wrong answer twice over.
    let piece_count = layout.piece_count() - not_selected.len() as u32;
    let wanted_bytes: u64 = match &wanted {
        None => layout.total_length,
        Some(set) => set
            .iter()
            .filter_map(|piece| layout.piece_size(*piece))
            .sum(),
    };
    let report = Report {
        info_hash: meta.info_hash().hex(),
        name: layout.name.clone(),
        data_dir: root.display().to_string(),
        total: Size(layout.total_length),
        selected: wanted.is_some().then_some(Size(wanted_bytes)),
        piece_count,
        pieces_ok,
        pieces_bad: piece_count - pieces_ok,
        complete: pieces_ok == piece_count && piece_count > 0,
        have: Size(have),
        have_share: percent_of(have, wanted_bytes),
        bad_pieces,
        not_selected,
        files,
        renamed: reader.renames(),
        per_piece,
    };

    // An incomplete or corrupt payload exits non-zero, so a pipeline does not
    // have to parse the report to find out.
    //
    // On failure the report goes into the error's context rather than being
    // emitted first. Emitting both would put two JSON documents on stdout,
    // which is not something `jq` can read.
    if !report.complete {
        return Err(Error::hash_mismatch(format!(
            "{} of {} pieces failed",
            report.pieces_bad, report.piece_count
        ))
        .with("pieces_ok", report.pieces_ok)
        .with("pieces_bad", report.pieces_bad)
        .with(
            "bad_pieces",
            serde_json::to_value(&report.bad_pieces).unwrap_or_default(),
        )
        .with("report", serde_json::to_value(&report).unwrap_or_default()));
    }

    renderer.emit(env, "verify", &report, || report.lines())?;
    Ok(ExitCode::Success)
}

/// Where the payload lives.
///
/// A multi-file torrent lays its files under a directory named after the
/// torrent, so `--data` pointing at the parent and pointing at the directory
/// itself both have to work. Whichever contains the first file wins.
///
/// The rule itself is [`crate::payload::resolve_with`], shared with `seed`,
/// which had a different one and reported a seeder holding nothing. See
/// `TODO/cli-surface.md`, T-186.
///
/// `-O` is passed through because the file this looks for is the file that
/// flag renames. Without it a caller who points `--data` at the parent of a
/// renamed payload is answered with the parent, and every file is then looked
/// for one directory too high. See `TODO/cli-surface.md`, T-213.
fn resolve_root(
    args: &VerifyArgs,
    global: &Global,
    env: &Env,
    meta: &Metainfo,
    index_out: &std::collections::BTreeMap<usize, String>,
) -> PathBuf {
    let base = args
        .data
        .clone()
        .or_else(|| global.dir.clone())
        .map(|p| env.resolve(&p))
        .unwrap_or_else(|| env.cwd.clone());

    crate::payload::resolve_with(&base, &meta.layout(), index_out).path
}

/// The path helper is also useful to callers checking a single file.
pub fn payload_path(root: &Path, layout: &Layout, index: usize) -> Option<PathBuf> {
    let file = layout.file(index)?;
    Some(
        file.path
            .iter()
            .fold(root.to_path_buf(), |acc, c| acc.join(c)),
    )
}

#[cfg(test)]
mod tests {
    use crate::test_support::{TorrentFixture, run_err, run_json};
    use bit_cli_core::ExitCode;

    /// What `download --select-file` leaves on disk, verified two ways.
    ///
    /// Without the selection every piece outside it is a failure, which is
    /// true of the bytes and wrong about the run: nothing was ever asked to
    /// fetch them. With the selection they are named separately and the run
    /// is complete. See `TODO/disk-io.md`, T-184.
    #[test]
    fn a_selection_separates_pieces_nobody_asked_for_from_pieces_that_are_wrong() {
        let fixture = TorrentFixture::straddling();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let source = format!("{}payload/", server.base);
        let out = fixture.dir().join("out");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--web-seed-only",
                "--web-seed",
                &source,
                "--web-seed-mode",
                "prefix",
                "--no-torrent-web-seed",
                "--no-tracker",
                "--port",
                "0",
                "--select-file",
                "1",
                "--stop-after",
                "30s",
            ],
            fixture.dir(),
        );
        assert_eq!(report["torrents"][0]["stopped"], "completed");
        let data = out.join("album");

        // Told nothing, verify reports the two pieces outside the selection as
        // failures and exits non-zero.
        let blind = run_err(
            &[
                "verify",
                "--data",
                data.to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
            ExitCode::HashMismatch,
        );
        assert!(blind.contains("2 of 4 pieces failed"), "{blind}");

        // Told the selection, the same bytes are complete.
        let doc = run_json(
            &[
                "verify",
                "--data",
                data.to_str().unwrap(),
                "--select-file",
                "1",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["complete"], true, "{doc}");
        assert_eq!(doc["pieces_ok"], 2);
        assert_eq!(doc["pieces_bad"], 0);
        assert_eq!(
            doc["piece_count"], 2,
            "the denominator is what was asked for"
        );
        assert_eq!(
            doc["not_selected"],
            serde_json::json!([0, 3]),
            "the pieces outside the selection are named rather than counted as failures"
        );
        // `have_share` is measured against the selection, not the torrent: a
        // run that got everything it asked for is not 55 per cent complete.
        assert_eq!(doc["have"]["bytes"], 2048);
        assert_eq!(doc["selected"]["bytes"], 2048);
        assert_eq!(doc["total"]["bytes"], 3700);
        assert_eq!(doc["have_share"], "100.00%");
    }

    /// The boundary pieces themselves verify, which is the part of T-184's
    /// premise the measurement disproved.
    ///
    /// Pieces 1 and 2 each hold bytes of a file the selection did not choose.
    /// The entry expected them to be unprovable; they are not, because those
    /// bytes are fetched and written for the piece's sake whatever the
    /// selection said. A seeder announcing them is telling the truth.
    #[test]
    fn a_boundary_piece_under_a_selection_verifies() {
        let fixture = TorrentFixture::straddling();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let source = format!("{}payload/", server.base);
        let out = fixture.dir().join("out");
        run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--web-seed-only",
                "--web-seed",
                &source,
                "--web-seed-mode",
                "prefix",
                "--no-torrent-web-seed",
                "--no-tracker",
                "--port",
                "0",
                "--select-file",
                "1",
                "--stop-after",
                "30s",
            ],
            fixture.dir(),
        );
        let doc = run_json(
            &[
                "verify",
                "--data",
                out.join("album").to_str().unwrap(),
                "--select-file",
                "1",
                "--per-piece",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        let per_piece = doc["per_piece"].as_array().unwrap();
        let states: Vec<(u64, bool, bool)> = per_piece
            .iter()
            .map(|row| {
                (
                    row["piece"].as_u64().unwrap(),
                    row["ok"].as_bool().unwrap(),
                    row["not_selected"].as_bool().unwrap_or(false),
                )
            })
            .collect();
        assert_eq!(
            states,
            vec![
                (0, false, true),
                (1, true, false),
                (2, true, false),
                (3, false, true)
            ],
            "pieces 1 and 2 straddle into files nobody selected and still verify"
        );
    }

    /// An exclusion on its own works here, because this command has the file
    /// list before it parses a flag. `download` does not, which is
    /// `TODO/cli-surface.md` T-185.
    #[test]
    fn an_exclusion_alone_selects_the_complement() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "verify",
                "--data",
                fixture.payload_dir().to_str().unwrap(),
                "--exclude-file",
                "1",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        // `album` is `disc 1/a.flac` (1500) and `notes.nfo` (500) at a 1024
        // byte piece length, so excluding the second file still needs both
        // pieces: the boundary at 1500 is inside piece 1.
        assert_eq!(doc["complete"], true, "{doc}");
        assert!(
            doc.get("not_selected").is_none(),
            "every piece is still needed: {doc}"
        );
    }

    #[test]
    fn a_complete_payload_verifies_and_exits_zero() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "verify",
                "--data",
                fixture.payload_dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["complete"], true);
        assert_eq!(doc["pieces_ok"], 2);
        assert_eq!(doc["pieces_bad"], 0);
        assert_eq!(doc["have"]["bytes"], 2000);
        assert_eq!(doc["have_share"], "100.00%");
    }

    #[test]
    fn a_corrupt_byte_fails_exactly_one_piece() {
        let fixture = TorrentFixture::multi_file();
        let target = fixture.payload_dir().join("notes.nfo");
        let mut bytes = std::fs::read(&target).unwrap();
        bytes[0] ^= 0xFF;
        std::fs::write(&target, &bytes).unwrap();

        let (mut env, captured) = crate::env::Env::test(
            &[
                "verify",
                "--json",
                "--data",
                fixture.payload_dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(crate::run(&mut env), ExitCode::HashMismatch);

        // Exactly one JSON document on stdout, carrying which piece failed.
        let doc: serde_json::Value = captured.json().expect("stdout must be one JSON document");
        assert_eq!(doc["code"], 7);
        assert_eq!(doc["kind"], "hash_mismatch");
        // notes.nfo is 1500..2000, which lies inside piece 1.
        assert_eq!(doc["context"]["bad_pieces"], serde_json::json!([1]));
        assert_eq!(doc["context"]["pieces_ok"], 1);
        assert_eq!(doc["context"]["report"]["complete"], false);
        assert!(captured.err().contains("failed"), "{}", captured.err());
    }

    #[test]
    fn a_missing_file_is_reported_rather_than_aborting() {
        let fixture = TorrentFixture::multi_file();
        std::fs::remove_file(fixture.payload_dir().join("notes.nfo")).unwrap();
        let err = run_err(
            &[
                "verify",
                "--data",
                fixture.payload_dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
            ExitCode::HashMismatch,
        );
        assert!(err.contains("failed"), "{err}");
    }

    #[test]
    fn per_piece_reports_every_piece() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "verify",
                "--per-piece",
                "--data",
                fixture.payload_dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        let pieces = doc["per_piece"].as_array().unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0]["piece"], 0);
        assert_eq!(pieces[0]["ok"], true);
        assert_eq!(pieces[1]["bytes"], 976, "the last piece is short");
    }

    #[test]
    fn a_single_file_payload_verifies() {
        let fixture = TorrentFixture::single_file();
        let doc = run_json(
            &[
                "verify",
                "--data",
                fixture.payload_dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["complete"], true);
        assert_eq!(doc["pieces_ok"], 3);
    }

    #[test]
    fn the_data_directory_can_be_the_parent_of_the_torrent_directory() {
        let fixture = TorrentFixture::multi_file();
        // Move the payload under a directory named after the torrent, which is
        // how a real download lays it out.
        let nested = fixture.root.join("downloads").join("album");
        std::fs::create_dir_all(nested.join("disc 1")).unwrap();
        for (path, bytes) in &fixture.files {
            std::fs::write(nested.join(path), bytes).unwrap();
        }
        let doc = run_json(
            &[
                "verify",
                "--data",
                fixture.root.join("downloads").to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["complete"], true);
    }

    #[test]
    fn file_results_report_what_is_present_and_what_is_short() {
        let fixture = TorrentFixture::multi_file();
        std::fs::write(fixture.payload_dir().join("notes.nfo"), vec![0u8; 100]).unwrap();
        let (mut env, captured) = crate::env::Env::test(
            &[
                "verify",
                "--json",
                "--data",
                fixture.payload_dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(crate::run(&mut env), ExitCode::HashMismatch);
        let doc: serde_json::Value = captured.json().expect("one JSON document");
        let files = doc["context"]["report"]["files"].as_array().unwrap();
        assert_eq!(files[1]["expected"]["bytes"], 500);
        assert_eq!(files[1]["found"]["bytes"], 100);
        assert_eq!(files[1]["present"], true);
    }
    /// `verify` reads where the bytes went, not where the torrent said.
    ///
    /// A download plans every torrent path before it opens anything, so a name
    /// the filesystem refuses is written under a different one. Verifying
    /// against the torrent's own path meant looking at a file that does not
    /// exist and reporting every one of them missing. The mapping is now
    /// applied and reported, the same array `download --json` carries. See
    /// `TODO/windows.md`, T-076.
    #[test]
    fn verify_reads_the_planned_paths_and_reports_the_mapping() {
        let fixture = TorrentFixture::hostile();
        let data = fixture.dir().join("data");
        std::fs::create_dir_all(&data).unwrap();

        let (mut env, captured) = crate::env::Env::test(
            &[
                "verify",
                "--json",
                "--data",
                data.to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        // Nothing is on disk, so the payload does not verify. What is under
        // test is the mapping, which is reported either way.
        assert_eq!(crate::run(&mut env), ExitCode::HashMismatch);
        let doc: serde_json::Value = captured.json().expect("one JSON document");
        let renamed = doc["context"]["report"]["renamed"]
            .as_array()
            .expect("a renamed array");

        let pairs: Vec<(String, String)> = renamed
            .iter()
            .map(|entry| {
                (
                    entry["torrent_path"].as_str().unwrap().to_string(),
                    entry["disk_path"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(
            pairs,
            [
                ("C:/pwned.txt".to_string(), "C_/pwned.txt".to_string()),
                ("CON.txt".to_string(), "CON_.txt".to_string()),
                ("a<b.bin".to_string(), "a_b.bin".to_string()),
                ("x .".to_string(), "x".to_string()),
                ("readme".to_string(), "readme-1".to_string()),
            ],
            "the same mapping `download --json` reports"
        );
        assert_eq!(renamed[0]["reasons"][0], "escape");
    }

    /// The ordinary torrent carries no mapping at all, so a caller can test
    /// for an empty array rather than comparing every path.
    #[test]
    fn an_ordinary_torrent_verifies_with_no_renames() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "verify",
                "--data",
                fixture.payload_dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["complete"], true);
        assert!(doc.get("renamed").is_none());
    }
}
