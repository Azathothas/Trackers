//! `bit-cli edit`: rewrite metainfo fields, writing a new file.
//!
//! This is the deliberate counterpart to attaching a web seed at runtime.
//! `edit` is how you bake web seeds into a torrent you are publishing; the
//! `--web-seed` flags on `download` are how you use web seeds on a torrent
//! someone else published.
//!
//! Fields outside the `info` dictionary can change without touching the info
//! hash, and the command reports the hash before and after so a caller can see
//! it is unchanged. Nothing edits in place.

use std::io::Write;
use std::path::Path;

use bit_cli_core::ExitCode;
use bit_cli_core::error::{Error, Result, from_io};
use bit_cli_core::torrent::Metainfo;
use bit_cli_core::torrent::bencode::Value;
use serde::Serialize;

use crate::cli::{EditArgs, Global};
use crate::env::Env;
use crate::output::{Renderer, field};

/// What `bit-cli edit` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub input: String,
    pub output: String,
    pub info_hash_before: String,
    pub info_hash_after: String,
    pub info_hash_changed: bool,
    pub changes: Vec<String>,
    pub trackers: Vec<Vec<String>>,
    pub web_seeds: Vec<String>,
    pub http_seeds: Vec<String>,
    pub written: bool,
}

impl Report {
    /// The text rendering.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field("input", &self.input),
            field("output", &self.output),
            field("info hash before", &self.info_hash_before),
            field("info hash after", &self.info_hash_after),
            field("info hash changed", self.info_hash_changed),
        ];
        for change in &self.changes {
            out.push(field("changed", change));
        }
        for seed in &self.web_seeds {
            out.push(field("web seed", seed));
        }
        out
    }
}

/// Run the command.
pub fn run(
    args: &EditArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let input = env.resolve(&args.torrent);
    let mut meta = crate::source::read_torrent_file(&input)?;
    let before = meta.info_hash();
    let mut changes = Vec::new();

    if args.no_announce {
        meta.set("announce", None)?;
        meta.set("announce-list", None)?;
        changes.push("removed every tracker".to_string());
    } else {
        let mut tiers = meta.announce_tiers();
        if let Some(primary) = &args.announce {
            match tiers.first_mut() {
                Some(first) => *first = vec![primary.clone()],
                None => tiers.push(vec![primary.clone()]),
            }
            changes.push(format!("announce = {primary}"));
        }
        if !args.announce_tier.is_empty() {
            tiers.push(args.announce_tier.clone());
            changes.push(format!("added tracker tier {:?}", args.announce_tier));
        }
        if args.announce.is_some() || !args.announce_tier.is_empty() {
            if let Some(first) = tiers.first().and_then(|t| t.first()) {
                meta.set("announce", Some(Value::text(first.clone())))?;
            }
            meta.set(
                "announce-list",
                Some(Value::List(
                    tiers
                        .iter()
                        .map(|tier| {
                            Value::List(tier.iter().map(|u| Value::text(u.clone())).collect())
                        })
                        .collect(),
                )),
            )?;
        }
    }

    if args.no_web_seed {
        meta.set("url-list", None)?;
        changes.push("removed every web seed".to_string());
    } else if !args.web_seed.is_empty() {
        let mut seeds = match args.replace_web_seeds {
            true => Vec::new(),
            false => meta.url_list(),
        };
        for url in &args.web_seed {
            if !seeds.contains(url) {
                seeds.push(url.clone());
            }
        }
        meta.set(
            "url-list",
            Some(Value::List(
                seeds.iter().map(|u| Value::text(u.clone())).collect(),
            )),
        )?;
        changes.push(format!("url-list now has {} entries", seeds.len()));
    }

    if !args.http_seed.is_empty() {
        let mut seeds = meta.http_seeds();
        for url in &args.http_seed {
            if !seeds.contains(url) {
                seeds.push(url.clone());
            }
        }
        meta.set(
            "httpseeds",
            Some(Value::List(
                seeds.iter().map(|u| Value::text(u.clone())).collect(),
            )),
        )?;
        changes.push(format!("httpseeds now has {} entries", seeds.len()));
    }

    if args.no_comment {
        meta.set("comment", None)?;
        changes.push("removed the comment".to_string());
    } else if let Some(comment) = &args.comment {
        meta.set("comment", Some(Value::text(comment.clone())))?;
        changes.push(format!("comment = {comment}"));
    }

    if let Some(created_by) = &args.created_by {
        meta.set("created by", Some(Value::text(created_by.clone())))?;
        changes.push(format!("created by = {created_by}"));
    }
    if args.no_creation_date {
        meta.set("creation date", None)?;
        changes.push("removed the creation date".to_string());
    }
    if let Some(url) = &args.update_url {
        meta.set("update-url", Some(Value::text(url.clone())))?;
        changes.push(format!("update-url = {url}"));
    }
    if !args.node.is_empty() {
        let nodes: Result<Vec<Value>> = args
            .node
            .iter()
            .map(|node| {
                let (host, port) = node
                    .rsplit_once(':')
                    .ok_or_else(|| Error::usage(format!("--node `{node}` is not host:port")))?;
                let port: u16 = port
                    .parse()
                    .map_err(|_| Error::usage(format!("--node `{node}` has a bad port")))?;
                Ok(Value::List(vec![
                    Value::text(host.to_string()),
                    Value::Int(i64::from(port)),
                ]))
            })
            .collect();
        meta.set("nodes", Some(Value::List(nodes?)))?;
        changes.push(format!("nodes now has {} entries", args.node.len()));
    }

    if changes.is_empty() {
        return Err(Error::usage(
            "nothing to change; `bit-cli edit` needs at least one field flag",
        ));
    }

    let bytes = meta.write_to_vec()?;
    let after = Metainfo::parse(&bytes)?.info_hash();

    // The whole point of this command is that the info hash survives. If it
    // did not, say so and stop unless the caller explicitly allowed it.
    if after != before && !args.allow_new_infohash {
        return Err(Error::would_change_infohash(format!(
            "this edit would change the info hash from {before} to {after}"
        ))
        .with("before", before.hex())
        .with("after", after.hex()));
    }

    let target = args.output.clone().unwrap_or_else(|| {
        let stem = input
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        input
            .with_file_name(format!("{stem}.edited.torrent"))
            .display()
            .to_string()
    });

    let mut written = false;
    if !global.dry_run {
        if target == "-" {
            env.out
                .write_all(&bytes)
                .map_err(|e| from_io(e, "cannot write the torrent to stdout"))?;
            written = true;
        } else {
            let path = env.resolve(Path::new(&target));
            if path == input {
                return Err(Error::usage(
                    "`bit-cli edit` never edits in place; --output must be a different file",
                )
                .with("path", path.display().to_string()));
            }
            if path.exists() && !args.force {
                return Err(Error::disk(format!(
                    "{} already exists; pass --force to overwrite it",
                    path.display()
                )));
            }
            std::fs::write(&path, &bytes)
                .map_err(|e| from_io(e, format!("cannot write {}", path.display())))?;
            written = true;
        }
    }

    let edited = Metainfo::parse(&bytes)?;
    let report = Report {
        input: input.display().to_string(),
        output: target.clone(),
        info_hash_before: before.hex(),
        info_hash_after: after.hex(),
        info_hash_changed: after != before,
        changes,
        trackers: edited.announce_tiers(),
        web_seeds: edited.url_list(),
        http_seeds: edited.http_seeds(),
        written,
    };

    if target == "-" && written {
        let _ = env.note(format!("info hash {} unchanged", report.info_hash_after));
    } else {
        renderer.emit(env, "edit", &report, || report.lines())?;
    }
    Ok(ExitCode::Success)
}

#[cfg(test)]
mod tests {
    use crate::test_support::{TorrentFixture, run_err, run_json};
    use bit_cli_core::ExitCode;
    use bit_cli_core::torrent::Metainfo;

    /// Editing a torrent whose own encoding is not canonical keeps its info
    /// hash, which is the property that makes reading one safe at all.
    ///
    /// `write_to_vec` re-encodes every key **outside** `info` canonically and
    /// splices the original `info` bytes back verbatim, so the top-level keys
    /// come out sorted, the `info` keys stay exactly as they were, and the
    /// hash does not move. A tool that re-encoded `info` instead would publish
    /// a different torrent from the same file. See `TODO/metainfo.md`, T-172.
    #[test]
    fn editing_a_torrent_with_unsorted_keys_keeps_its_info_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sloppy.torrent");
        // `info` before `announce`, and `info`'s own keys out of order.
        let info = {
            // Built rather than written as one literal: the piece hashes are
            // sixty bytes and a line continuation inside a byte string is one
            // more thing to get wrong in a fixture whose whole point is exact
            // bytes.
            let mut info = Vec::new();
            info.extend_from_slice(b"d12:piece lengthi1024e4:name9:movie.bin");
            info.extend_from_slice(b"6:lengthi3000e6:pieces60:");
            info.extend_from_slice(&[b'0'; 60]);
            info.push(b'e');
            info
        };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"d4:info");
        bytes.extend_from_slice(&info);
        bytes.extend_from_slice(b"8:announce28:udp://tracker.example.com:80e");
        std::fs::write(&path, &bytes).unwrap();

        let before = Metainfo::read(&path).unwrap();
        assert!(!before.encoding().is_canonical());
        assert!(before.encoding().unsorted_inside_info);

        let doc = run_json(
            &[
                "edit",
                "--web-seed",
                "https://a.example.com/pub/",
                path.to_str().unwrap(),
            ],
            dir.path().to_path_buf(),
        );
        assert_eq!(doc["info_hash_changed"], false, "{doc}");
        assert_eq!(doc["info_hash_after"], before.info_hash().hex());

        let after = Metainfo::read(&path.with_extension("edited.torrent"))
            .or_else(|_| Metainfo::read(dir.path().join("sloppy.edited.torrent").as_path()))
            .expect("the edited torrent");
        assert_eq!(after.info_hash(), before.info_hash());
        assert_eq!(
            after.info_bytes(),
            before.info_bytes(),
            "the `info` bytes are spliced back exactly, out-of-order keys and all"
        );
        // The top level came out sorted, because everything outside `info` is
        // re-encoded canonically. `info` did not, because it was spliced back
        // byte for byte. So the edited file is canonical everywhere the info
        // hash does not depend on, and untouched everywhere it does, which is
        // the whole design in one assertion.
        assert!(
            !after.encoding().unsorted_dicts.contains(&0),
            "the top level should have been re-encoded sorted: {:?}",
            after.encoding()
        );
        assert!(
            after.encoding().unsorted_inside_info,
            "`info` keeps the order it was written in, or the hash would move: {:?}",
            after.encoding()
        );
    }

    #[test]
    fn adding_web_seeds_keeps_the_info_hash() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "edit",
                "--web-seed",
                "https://a.example.com/pub/",
                "--web-seed",
                "https://b.example.com/pub/",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["info_hash_before"], fixture.info_hash);
        assert_eq!(doc["info_hash_after"], fixture.info_hash);
        assert_eq!(doc["info_hash_changed"], false);
        // The torrent already carried one, so adding two makes three.
        assert_eq!(doc["web_seeds"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn the_edited_torrent_is_written_beside_the_original_and_still_parses() {
        let fixture = TorrentFixture::multi_file();
        run_json(
            &[
                "edit",
                "--web-seed",
                "https://a.example.com/pub/",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        let edited = fixture.root.join("album.edited.torrent");
        assert!(edited.exists(), "the edited torrent was not written");
        let meta = Metainfo::read(&edited).unwrap();
        assert_eq!(meta.info_hash().hex(), fixture.info_hash);
        assert!(
            meta.url_list()
                .contains(&"https://a.example.com/pub/".to_string())
        );
    }

    #[test]
    fn the_original_is_never_touched() {
        let fixture = TorrentFixture::multi_file();
        let before = std::fs::read(&fixture.torrent).unwrap();
        run_json(
            &["edit", "--comment", "changed", fixture.path_str()],
            fixture.dir(),
        );
        assert_eq!(std::fs::read(&fixture.torrent).unwrap(), before);
    }

    #[test]
    fn writing_over_the_input_is_refused() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &[
                "edit",
                "--comment",
                "x",
                "-o",
                fixture.path_str(),
                fixture.path_str(),
            ],
            fixture.dir(),
            ExitCode::Usage,
        );
        assert!(err.contains("never edits in place"), "{err}");
    }

    #[test]
    fn replacing_web_seeds_drops_the_torrents_own() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "edit",
                "--replace-web-seeds",
                "--web-seed",
                "https://only.example.com/",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(
            doc["web_seeds"],
            serde_json::json!(["https://only.example.com/"])
        );
        assert_eq!(doc["info_hash_after"], fixture.info_hash);
    }

    #[test]
    fn removing_web_seeds_leaves_none() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &["edit", "--no-web-seed", fixture.path_str()],
            fixture.dir(),
        );
        assert!(doc["web_seeds"].as_array().unwrap().is_empty());
        assert_eq!(doc["info_hash_after"], fixture.info_hash);
    }

    #[test]
    fn tracker_edits_keep_the_info_hash_too() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "edit",
                "--announce",
                "udp://new.example.com:80",
                "--announce-tier",
                "udp://b:80,udp://c:80",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["info_hash_after"], fixture.info_hash);
        assert_eq!(doc["trackers"][0][0], "udp://new.example.com:80");
        assert_eq!(doc["trackers"][1].as_array().unwrap().len(), 2);
    }

    #[test]
    fn an_edit_with_no_flags_is_a_usage_error() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &["edit", fixture.path_str()],
            fixture.dir(),
            ExitCode::Usage,
        );
        assert!(err.contains("at least one field flag"), "{err}");
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &["edit", "--dry-run", "--comment", "x", fixture.path_str()],
            fixture.dir(),
        );
        assert_eq!(doc["written"], false);
        assert!(!fixture.root.join("album.edited.torrent").exists());
    }

    #[test]
    fn an_existing_output_needs_force() {
        let fixture = TorrentFixture::multi_file();
        run_json(
            &["edit", "--comment", "one", fixture.path_str()],
            fixture.dir(),
        );
        run_err(
            &["edit", "--comment", "two", fixture.path_str()],
            fixture.dir(),
            ExitCode::Disk,
        );
        run_json(
            &["edit", "--force", "--comment", "two", fixture.path_str()],
            fixture.dir(),
        );
    }

    #[test]
    fn the_edited_torrent_still_verifies_against_the_original_data() {
        let fixture = TorrentFixture::multi_file();
        run_json(
            &[
                "edit",
                "--web-seed",
                "https://a.example.com/pub/",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        let edited = fixture.root.join("album.edited.torrent");
        let original = Metainfo::read(&fixture.torrent).unwrap();
        let after = Metainfo::read(&edited).unwrap();
        assert_eq!(original.info_bytes(), after.info_bytes());
        assert_eq!(original.info().pieces, after.info().pieces);
    }
}
