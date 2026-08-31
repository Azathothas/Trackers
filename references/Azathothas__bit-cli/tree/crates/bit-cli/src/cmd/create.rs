//! `bit-cli create`: build a `.torrent` from a file or a directory.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use bit_cli_core::ExitCode;
use bit_cli_core::error::{Error, Result, from_io};
use bit_cli_core::time::Timestamp;
use bit_cli_core::torrent::create::{CreateOptions, InputFile, SortBy, create};
use bit_cli_core::torrent::{Lint, Magnet, Metainfo};
use bit_cli_core::units::{Size, format_size, parse_size};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;

use crate::cli::{CreateArgs, Global, TorrentVersion};
use crate::env::Env;
use crate::output::{Renderer, field};

/// What `bit-cli create` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub info_hash: String,
    pub name: String,
    pub output: String,
    pub total: Size,
    pub file_count: usize,
    pub piece_length: Size,
    pub piece_count: u32,
    pub piece_length_reason: String,
    pub private: bool,
    pub magnet: String,
    pub written: bool,
}

impl Report {
    /// The text rendering.
    pub fn lines(&self, show: bool, link: bool) -> Vec<String> {
        if link && !show {
            return vec![self.magnet.clone()];
        }
        let mut out = vec![
            field("name", &self.name),
            field("info hash", &self.info_hash),
            field("output", &self.output),
            field("size", format_size(self.total.0)),
            field("files", self.file_count),
            field("piece length", format_size(self.piece_length.0)),
            field("pieces", self.piece_count),
            field("private", self.private),
        ];
        if show {
            out.push(field("piece choice", &self.piece_length_reason));
        }
        if link {
            out.push(field("magnet", &self.magnet));
        }
        out
    }
}

/// Files found under the input path, and whether it is a directory.
struct Walked {
    files: Vec<InputFile>,
    multi_file: bool,
    name: String,
}

/// Junk that is almost never meant to be in a torrent.
const JUNK: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    "__MACOSX",
    ".AppleDouble",
    ".Spotlight-V100",
    ".Trashes",
];

/// Walk the input path into a deterministic file list.
fn walk(args: &CreateArgs, root: &Path) -> Result<Walked> {
    let metadata = std::fs::metadata(root)
        .map_err(|e| from_io(e, format!("cannot read {}", root.display())))?;

    let name = args.name.clone().unwrap_or_else(|| {
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "torrent".to_string())
    });

    if metadata.is_file() {
        return Ok(Walked {
            files: vec![InputFile {
                source: root.to_path_buf(),
                path: name.clone(),
                length: metadata.len(),
            }],
            multi_file: false,
            name,
        });
    }

    let (include, exclude) = build_globs(&args.glob)?;
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(!args.include_hidden)
        .follow_links(args.follow_symlinks)
        .git_ignore(args.ignore)
        .git_global(args.ignore)
        .git_exclude(args.ignore)
        .ignore(args.ignore)
        .parents(args.ignore)
        // Sorting by file name makes the walk itself deterministic, before the
        // explicit sort in the creator runs.
        .sort_by_file_name(|a, b| a.cmp(b));

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry =
            entry.map_err(|e| Error::disk(format!("cannot walk {}: {e}", root.display())))?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let absolute = entry.path();
        let relative = absolute.strip_prefix(root).unwrap_or(absolute);
        // `/` separators in the metainfo on every platform. A backslash here
        // would produce a torrent that no non-Windows client can lay out.
        let path: String = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if path.is_empty() {
            continue;
        }
        if !args.include_junk
            && relative.components().any(|c| {
                let name = c.as_os_str().to_string_lossy();
                JUNK.iter().any(|junk| name.eq_ignore_ascii_case(junk))
            })
        {
            continue;
        }
        if let Some(exclude) = &exclude
            && exclude.is_match(&path)
        {
            continue;
        }
        if let Some(include) = &include
            && !include.is_match(&path)
        {
            continue;
        }
        let length = entry
            .metadata()
            .map_err(|e| Error::disk(format!("cannot stat {}: {e}", absolute.display())))?
            .len();
        files.push(InputFile {
            source: absolute.to_path_buf(),
            path,
            length,
        });
    }

    if files.is_empty() {
        return Err(
            Error::usage(format!("{} contains no files to include", root.display()))
                .with("path", root.display().to_string()),
        );
    }
    Ok(Walked {
        files,
        multi_file: true,
        name,
    })
}

/// Split `--glob` into include and exclude sets.
fn build_globs(patterns: &[String]) -> Result<(Option<GlobSet>, Option<GlobSet>)> {
    let mut include = GlobSetBuilder::new();
    let mut exclude = GlobSetBuilder::new();
    let (mut has_include, mut has_exclude) = (false, false);
    for pattern in patterns {
        let (target, text, flag) = match pattern.strip_prefix('!') {
            Some(rest) => (&mut exclude, rest, &mut has_exclude),
            None => (&mut include, pattern.as_str(), &mut has_include),
        };
        let glob = Glob::new(text).map_err(|e| {
            Error::usage(format!("--glob `{pattern}`: {e}")).with("glob", pattern.clone())
        })?;
        target.add(glob);
        *flag = true;
    }
    let build = |builder: GlobSetBuilder, present: bool| -> Result<Option<GlobSet>> {
        match present {
            false => Ok(None),
            true => builder
                .build()
                .map(Some)
                .map_err(|e| Error::usage(format!("cannot build the glob set: {e}"))),
        }
    };
    Ok((build(include, has_include)?, build(exclude, has_exclude)?))
}

/// Run the command.
pub fn run(
    args: &CreateArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    if args.version != TorrentVersion::V1 {
        return Err(Error::usage(format!(
            "`--version {}` is not implemented yet; only v1 is available. See TODO/create-seed.md",
            format!("{:?}", args.version).to_lowercase()
        ))
        .with("todo", "T-081"));
    }

    let root = env.resolve(&args.path);
    let walked = walk(args, &root)?;

    let mut tiers: Vec<Vec<String>> = Vec::new();
    if let Some(primary) = &args.announce {
        tiers.push(vec![primary.clone()]);
    }
    if !args.announce_tier.is_empty() {
        tiers.push(args.announce_tier.clone());
    }

    let allowed: BTreeSet<Lint> = args
        .allow
        .iter()
        .map(|name| Lint::parse(name))
        .collect::<Result<_>>()?;

    let options = CreateOptions {
        name: walked.name.clone(),
        multi_file: walked.multi_file,
        piece_length: match &args.piece_length {
            None => None,
            Some(text) => Some(
                u32::try_from(parse_size(text).map_err(|e| {
                    Error::usage(format!("--piece-length: {e}")).with("value", text.clone())
                })?)
                .map_err(|_| Error::usage("--piece-length does not fit in 32 bits"))?,
            ),
        },
        announce_tiers: tiers,
        web_seeds: args.web_seed.clone(),
        http_seeds: args.http_seed.clone(),
        nodes: args.node.clone(),
        comment: args.comment.clone(),
        source: args.source.clone(),
        update_url: args.update_url.clone(),
        private: args.private,
        md5: args.md5,
        created_by: (!args.no_created_by).then(|| format!("bit-cli/{}", bit_cli_core::VERSION)),
        creation_date: (!args.no_creation_date).then(Timestamp::now),
        allowed_lints: allowed,
        sort_by: SortBy::parse(&args.sort_by)?,
    };

    let created = create(walked.files, &options, |path: &Path| {
        std::fs::File::open(path).map_err(|e| from_io(e, format!("cannot read {}", path.display())))
    })?;

    let meta = Metainfo::parse(&created.bytes)?;
    let magnet = Magnet::from_metainfo(&meta).to_uri();

    let target = args
        .output
        .clone()
        .unwrap_or_else(|| default_output(&root, &walked.name).display().to_string());

    let mut written = false;
    if !global.dry_run {
        if target == "-" {
            env.out
                .write_all(&created.bytes)
                .map_err(|e| from_io(e, "cannot write the torrent to stdout"))?;
            written = true;
        } else {
            let path = env.resolve(Path::new(&target));
            if path.exists() && !args.force {
                return Err(Error::disk(format!(
                    "{} already exists; pass --force to overwrite it",
                    path.display()
                ))
                .with("path", path.display().to_string()));
            }
            std::fs::write(&path, &created.bytes)
                .map_err(|e| from_io(e, format!("cannot write {}", path.display())))?;
            written = true;
        }
    }

    let report = Report {
        info_hash: created.info_hash.hex(),
        name: walked.name,
        output: target.clone(),
        total: Size(created.total_length),
        file_count: created.files.len(),
        piece_length: Size(u64::from(created.piece_length)),
        piece_count: created.piece_count,
        piece_length_reason: created.piece_length_reason.clone(),
        private: args.private,
        magnet,
        written,
    };

    // Writing the torrent to stdout means stdout is the torrent. The summary
    // goes to stderr so the two do not interleave.
    if target == "-" && written {
        let _ = env.note(format!("created {} ({})", report.name, report.info_hash));
    } else {
        renderer.emit(env, "create", &report, || {
            report.lines(args.show, args.link)
        })?;
    }
    Ok(ExitCode::Success)
}

/// Where the torrent goes when `--output` is not given.
fn default_output(root: &Path, name: &str) -> PathBuf {
    let parent = root.parent().unwrap_or(Path::new("."));
    parent.join(format!("{name}.torrent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{run_err, run_json, run_ok};

    struct Payload {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    /// A directory with two files and a hidden one.
    fn payload() -> Payload {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("album");
        std::fs::create_dir_all(root.join("disc 1")).unwrap();
        std::fs::write(root.join("disc 1").join("a.flac"), vec![1u8; 40_000]).unwrap();
        std::fs::write(root.join("notes.nfo"), vec![2u8; 5_000]).unwrap();
        std::fs::write(root.join(".hidden"), vec![3u8; 100]).unwrap();
        std::fs::write(root.join(".DS_Store"), vec![4u8; 10]).unwrap();
        Payload { _dir: dir, root }
    }

    fn root_str(p: &Payload) -> &str {
        p.root.to_str().unwrap()
    }

    #[test]
    fn a_directory_becomes_a_multi_file_torrent() {
        let p = payload();
        let doc = run_json(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                root_str(&p),
            ],
            p.root.parent().unwrap(),
        );
        assert_eq!(doc["name"], "album");
        assert_eq!(doc["file_count"], 2, "hidden and junk files are excluded");
        assert_eq!(doc["total"]["bytes"], 45_000);
        assert!(doc["written"].as_bool().unwrap());

        let torrent = p.root.parent().unwrap().join("album.torrent");
        let meta = Metainfo::read(&torrent).unwrap();
        assert_eq!(meta.info_hash().hex(), doc["info_hash"].as_str().unwrap());
        let paths: Vec<String> = meta.info().files.iter().map(|f| f.path.join("/")).collect();
        assert_eq!(paths, ["disc 1/a.flac", "notes.nfo"]);
    }

    #[test]
    fn paths_use_forward_slashes_on_every_platform() {
        let p = payload();
        run_ok(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                root_str(&p),
            ],
            p.root.parent().unwrap(),
        );
        let meta = Metainfo::read(&p.root.parent().unwrap().join("album.torrent")).unwrap();
        for file in &meta.info().files {
            for component in &file.path {
                assert!(!component.contains('\\'), "backslash in {component}");
                assert!(!component.contains('/'), "unsplit path in {component}");
            }
        }
    }

    #[test]
    fn the_same_input_produces_a_byte_identical_torrent() {
        let p = payload();
        let out = p.root.parent().unwrap();
        run_ok(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "-o",
                "one.torrent",
                root_str(&p),
            ],
            out,
        );
        run_ok(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "-o",
                "two.torrent",
                root_str(&p),
            ],
            out,
        );
        assert_eq!(
            std::fs::read(out.join("one.torrent")).unwrap(),
            std::fs::read(out.join("two.torrent")).unwrap()
        );
    }

    #[test]
    fn hidden_and_junk_files_are_included_when_asked_for() {
        let p = payload();
        let doc = run_json(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "--include-hidden",
                "--include-junk",
                "-o",
                "all.torrent",
                root_str(&p),
            ],
            p.root.parent().unwrap(),
        );
        assert_eq!(doc["file_count"], 4);
    }

    #[test]
    fn writing_the_torrent_to_stdout_keeps_the_summary_off_stdout() {
        let p = payload();
        let (mut env, captured) = crate::env::Env::test(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "-o",
                "-",
                root_str(&p),
            ],
            p.root.parent().unwrap(),
        );
        assert_eq!(crate::run(&mut env), ExitCode::Success);
        // stdout is the torrent and nothing else. Piece hashes are arbitrary
        // bytes, so this has to be read as bytes rather than as text.
        let bytes = captured.out_bytes();
        assert!(bytes.starts_with(b"d8:announce"), "stdout is not bencode");
        let meta = Metainfo::parse(&bytes).expect("stdout parses as a torrent");
        assert_eq!(meta.info().name, "album");
        // The summary went to stderr.
        assert!(
            captured.err().contains("created album"),
            "{}",
            captured.err()
        );
    }

    #[test]
    fn globs_include_and_exclude() {
        let p = payload();
        let out = p.root.parent().unwrap();
        let doc = run_json(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "--glob",
                "*.flac",
                "-o",
                "g.torrent",
                root_str(&p),
            ],
            out,
        );
        assert_eq!(doc["file_count"], 1);

        let doc = run_json(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "--glob",
                "!*.nfo",
                "-o",
                "h.torrent",
                root_str(&p),
            ],
            out,
        );
        assert_eq!(doc["file_count"], 1);
    }

    #[test]
    fn a_single_file_becomes_a_single_file_torrent() {
        let p = payload();
        let file = p.root.join("notes.nfo");
        let doc = run_json(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                file.to_str().unwrap(),
            ],
            &p.root,
        );
        assert_eq!(doc["name"], "notes.nfo");
        assert_eq!(doc["file_count"], 1);
        let meta = Metainfo::read(&p.root.join("notes.nfo.torrent")).unwrap();
        assert!(!meta.info().multi_file);
    }

    #[test]
    fn lints_refuse_a_private_torrent_with_no_tracker() {
        let p = payload();
        let err = run_err(
            &["create", "--no-creation-date", "--private", root_str(&p)],
            p.root.parent().unwrap(),
            ExitCode::LintRefused,
        );
        assert!(err.contains("private-no-tracker"), "{err}");
        assert!(err.contains("--allow"), "{err}");
    }

    #[test]
    fn an_allowed_lint_lets_the_build_through() {
        let p = payload();
        run_ok(
            &[
                "create",
                "--no-creation-date",
                "--private",
                "--allow",
                "private-no-tracker",
                root_str(&p),
            ],
            p.root.parent().unwrap(),
        );
    }

    #[test]
    fn an_unknown_lint_name_is_a_usage_error() {
        let p = payload();
        let err = run_err(
            &["create", "--allow", "no-such-lint", root_str(&p)],
            p.root.parent().unwrap(),
            ExitCode::Usage,
        );
        assert!(err.contains("known lints"), "{err}");
    }

    #[test]
    fn an_existing_output_is_not_overwritten_without_force() {
        let p = payload();
        let out = p.root.parent().unwrap();
        run_ok(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                root_str(&p),
            ],
            out,
        );
        let err = run_err(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                root_str(&p),
            ],
            out,
            ExitCode::Disk,
        );
        assert!(err.contains("--force"), "{err}");
        run_ok(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "--force",
                root_str(&p),
            ],
            out,
        );
    }

    #[test]
    fn dry_run_writes_nothing_but_still_reports() {
        let p = payload();
        let out = p.root.parent().unwrap();
        let doc = run_json(
            &[
                "create",
                "--dry-run",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                root_str(&p),
            ],
            out,
        );
        assert_eq!(doc["written"], false);
        assert!(doc["info_hash"].as_str().unwrap().len() == 40);
        assert!(!out.join("album.torrent").exists());
    }

    #[test]
    fn link_prints_only_the_magnet() {
        let p = payload();
        let out = run_ok(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "--link",
                "-o",
                "linked.torrent",
                root_str(&p),
            ],
            p.root.parent().unwrap(),
        );
        assert_eq!(
            out.lines().count(),
            1,
            "--link output must be pipeable: {out}"
        );
        assert!(out.starts_with("magnet:?xt=urn:btih:"), "{out}");
    }

    #[test]
    fn web_seeds_and_trackers_are_baked_into_the_torrent() {
        let p = payload();
        let out = p.root.parent().unwrap();
        run_ok(
            &[
                "create",
                "--no-creation-date",
                "--announce",
                "udp://a:80",
                "--announce-tier",
                "udp://b:80,udp://c:80",
                "--web-seed",
                "https://mirror.example.com/pub/",
                "--http-seed",
                "https://old.example.com/",
                "--node",
                "dht.example.com:6881",
                "--comment",
                "hello",
                root_str(&p),
            ],
            out,
        );
        let meta = Metainfo::read(&out.join("album.torrent")).unwrap();
        assert_eq!(meta.url_list(), ["https://mirror.example.com/pub/"]);
        assert_eq!(meta.http_seeds(), ["https://old.example.com/"]);
        assert_eq!(meta.nodes(), ["dht.example.com:6881"]);
        assert_eq!(meta.trackers().len(), 3);
        assert_eq!(meta.comment().as_deref(), Some("hello"));
    }

    #[test]
    fn v2_and_hybrid_say_they_are_not_available_rather_than_producing_a_v1() {
        let p = payload();
        for version in ["v2", "hybrid"] {
            let err = run_err(
                &["create", "--version", version, root_str(&p)],
                p.root.parent().unwrap(),
                ExitCode::Usage,
            );
            assert!(err.contains("not implemented yet"), "{err}");
        }
    }

    #[test]
    fn an_empty_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        run_err(
            &["create", empty.to_str().unwrap()],
            dir.path(),
            ExitCode::Usage,
        );
    }

    /// The same input produces the same bytes on every platform, not only on
    /// repeat runs of one.
    ///
    /// A path separator or an ordering rule that differs between Windows and
    /// Linux produces two info hashes for one payload, and two mirrors
    /// publishing the same file then publish two torrents. A repeat-run test
    /// cannot catch that: both runs are on the same machine. A constant can,
    /// because the test suite runs on both platforms in CI and both compare
    /// against this same number.
    ///
    /// The fixture is the one `ci.yml`'s determinism job builds, so the two
    /// checks are the same check. See `TODO/create-seed.md`, T-085.
    #[test]
    fn a_fixture_torrent_hashes_the_same_on_every_platform() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("fixture");
        std::fs::create_dir_all(root.join("sub")).expect("mkdir");
        std::fs::write(root.join("a.bin"), b"one").expect("write");
        std::fs::write(root.join("sub").join("b.bin"), b"two").expect("write");

        run_ok(
            &[
                "create",
                root.to_str().unwrap(),
                "--name",
                "fixture",
                "--no-creation-date",
                "--no-created-by",
                "--sort-by",
                "path:ascending",
                "--piece-length",
                "16KiB",
                "-o",
                "fixture.torrent",
            ],
            temp.path(),
        );

        let bytes = std::fs::read(temp.path().join("fixture.torrent")).expect("read the torrent");
        // SHA-1 because the crate already carries it for piece hashes. This is
        // a file identity check, not a security property.
        let digest = <sha1::Sha1 as sha1::Digest>::digest(&bytes);
        // Formatted here rather than with `{digest:x}`. `sha1` 0.11 returns
        // `hybrid_array::Array` where 0.10 returned a `GenericArray`, and only
        // the second implements `LowerHex`. Writing the twenty bytes out is
        // one line and does not depend on which array type the crate is
        // currently returning.
        let digest: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            digest, "069804535e172027dfd40388bc0b7a64d8e8770b",
            "the bytes this platform wrote differ from every other platform's"
        );
    }
}
