//! `bit-cli magnet`: convert a torrent to a magnet URI, or read one back.

use std::io::Write;

use bit_cli_core::ExitCode;
use bit_cli_core::error::{Error, Result};
use bit_cli_core::torrent::{Magnet, Metainfo};
use bit_cli_core::units::Size;
use serde::Serialize;

use crate::cli::{Global, MagnetArgs};
use crate::env::Env;
use crate::output::{Renderer, field};
use crate::source::{Kind, resolve_source};

/// What `bit-cli magnet` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub magnet: String,
    pub info_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<Size>,
    pub trackers: Vec<String>,
    pub web_seeds: Vec<String>,
    pub peers: Vec<String>,
    pub selected_files: Vec<u32>,
}

impl Report {
    fn from_magnet(magnet: &Magnet) -> Self {
        Self {
            magnet: magnet.to_uri(),
            info_hash: magnet.info_hash.map(|h| h.hex()).unwrap_or_default(),
            name: magnet.name.clone(),
            length: magnet.length.map(Size),
            trackers: magnet.trackers.clone(),
            web_seeds: magnet.web_seeds.clone(),
            peers: magnet.peers.clone(),
            selected_files: magnet.selected_files(),
        }
    }

    /// The text rendering.
    ///
    /// Converting a torrent prints only the URI, so `bit-cli magnet x.torrent`
    /// drops straight into another command with nothing to strip. Reading a
    /// magnet back prints the fields, because that is the question being asked.
    pub fn lines(&self, uri_only: bool) -> Vec<String> {
        if uri_only {
            return vec![self.magnet.clone()];
        }
        let mut out = vec![field("info hash", &self.info_hash)];
        if let Some(name) = &self.name {
            out.push(field("name", name));
        }
        if let Some(length) = self.length {
            out.push(field("size", length));
        }
        for tracker in &self.trackers {
            out.push(field("tracker", tracker));
        }
        for seed in &self.web_seeds {
            out.push(field("web seed", seed));
        }
        for peer in &self.peers {
            out.push(field("peer", peer));
        }
        if !self.selected_files.is_empty() {
            out.push(field(
                "selected files",
                format!("{:?}", self.selected_files),
            ));
        }
        out.push(field("magnet", &self.magnet));
        out
    }
}

/// Run the command.
pub fn run(
    args: &MagnetArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let kind = Kind::classify(&args.source.source, env)?;

    // A magnet source prints what the URI itself carries, and that costs
    // nothing: no swarm, no tracker, no DHT. `--output` is the one thing that
    // needs the metadata behind the URI, so it is the one thing that joins a
    // swarm, and the report is the same either way. Anything that is not a
    // magnet is read the way every other command reads it.
    let mut resolved: Option<Metainfo> = None;
    let (report, uri_only) = match &kind {
        Kind::Magnet(magnet) => (Report::from_magnet(magnet), false),
        _ => {
            let meta = resolve_source(&kind, env, global, None, &args.swarm, &args.page)?;
            let report = Report::from_magnet(&Magnet::from_metainfo(&meta));
            resolved = Some(meta);
            (report, true)
        }
    };

    if let Some(target) = &args.output {
        let meta = match resolved {
            Some(meta) => meta,
            None => resolve_source(&kind, env, global, None, &args.swarm, &args.page)?,
        };
        let magnet = match &kind {
            Kind::Magnet(magnet) => Some(magnet.as_ref()),
            _ => None,
        };
        write_torrent(target, meta, magnet, args.force, global, env)?;
    }

    renderer.emit(env, "magnet", &report, || report.lines(uri_only))?;
    Ok(ExitCode::Success)
}

/// Write resolved metainfo out as a `.torrent`.
///
/// `write_to_vec` splices the `info` dictionary in as the bytes it arrived as
/// and then decodes what it produced to check the hash did not move, so this
/// cannot publish a different torrent than the one that was asked for.
///
/// **A magnet carries things the info dictionary does not**, and they belong
/// at the top level of the written file rather than inside `info`, where they
/// would change the hash. Trackers arrive already: the session is given the
/// magnet's `tr=` list and puts it in the file it assembles. `ws=` does not,
/// so it is set here when the magnet had one and the assembled file did not.
fn write_torrent(
    target: &str,
    mut meta: Metainfo,
    magnet: Option<&Magnet>,
    force: bool,
    global: &Global,
    env: &mut Env,
) -> Result<()> {
    if let Some(magnet) = magnet
        && !magnet.web_seeds.is_empty()
        && meta.url_list().is_empty()
    {
        let seeds = magnet
            .web_seeds
            .iter()
            .map(|seed| bit_cli_core::torrent::bencode::Value::text(seed.clone()))
            .collect();
        meta.set(
            "url-list",
            Some(bit_cli_core::torrent::bencode::Value::List(seeds)),
        )?;
    }
    // What the session assembles when the magnet named no tracker: an
    // `announce` of the empty string and an `announce-list` holding one empty
    // tier. Measured against a magnet carrying only `xt`, `dn` and `xl`, the
    // file began `d8:announce0:13:announce-listllee`. Neither key means
    // anything and the first is a value no client should dial, so both come
    // out when they are empty. `write_to_vec` re-encodes everything outside
    // `info`, so the hash is untouched and it proves that before returning.
    if meta.announce().is_none_or(|url| url.is_empty()) {
        meta.set("announce", None)?;
    }
    if meta.announce_tiers().iter().all(|tier| tier.is_empty()) {
        meta.set("announce-list", None)?;
    }

    let bytes = meta.write_to_vec()?;
    if global.dry_run {
        return Ok(());
    }
    if target == "-" {
        return env
            .out
            .write_all(&bytes)
            .map_err(|e| bit_cli_core::error::from_io(e, "cannot write the torrent to stdout"));
    }
    let path = env.resolve(std::path::Path::new(target));
    if path.exists() && !force {
        return Err(Error::disk(format!(
            "{} already exists; pass --force to overwrite it",
            path.display()
        ))
        .with("path", path.display().to_string()));
    }
    std::fs::write(&path, &bytes)
        .map_err(|e| bit_cli_core::error::from_io(e, format!("cannot write {}", path.display())))
}

#[cfg(test)]
mod tests {
    use crate::test_support::{TorrentFixture, run_json, run_ok};

    /// A resolved magnet is written back out as a `.torrent`, hash intact.
    ///
    /// T-241's own subject. The bytes carry the `info` dictionary as it arrived
    /// over BEP 9, spliced into a root the rest of which is re-encoded, and
    /// `Metainfo::write_to_vec` decodes what it produced and refuses to return
    /// it if the hash moved. Resolving the same magnet a second time is then
    /// a file read.
    #[test]
    fn a_resolved_magnet_is_written_back_out_with_the_same_info_hash() {
        use crate::test_support::{free_port, seed_fixture};

        let fixture = TorrentFixture::multi_file();
        let dir = fixture.dir();
        let port = free_port();
        let seeder = seed_fixture(&fixture, port);

        let magnet = run_ok(&["magnet", fixture.path_str()], dir.clone());
        let magnet = magnet.trim().to_string();
        let peer = format!("127.0.0.1:{port}");
        let written = dir.join("from-magnet.torrent");
        let written_str = written.to_str().expect("utf-8 path").to_string();

        run_ok(
            &[
                "magnet",
                &magnet,
                "--peer",
                &peer,
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--output",
                &written_str,
            ],
            dir.clone(),
        );

        assert!(written.exists(), "nothing was written to {written_str}");
        let round_trip = run_json(&["info", &written_str], dir.clone());
        let original = run_json(&["info", fixture.path_str()], dir);
        assert_eq!(
            round_trip["info_hash"], original["info_hash"],
            "the written torrent is a different torrent"
        );
        assert_eq!(round_trip["name"], original["name"]);
        assert_eq!(round_trip["file_count"], original["file_count"]);

        // And nothing meaningless at the top level. The session assembles a
        // torrent with `announce: ""` and one empty `announce-list` tier when
        // the magnet named no tracker, and an empty announce URL is a value a
        // client would try to dial.
        let written_bytes = std::fs::read(&written).expect("read what was written");
        let head = String::from_utf8_lossy(&written_bytes[..40.min(written_bytes.len())]);
        assert!(
            !head.contains("8:announce0:"),
            "an empty announce survived: {head}"
        );
        assert!(
            !head.contains("13:announce-listllee"),
            "an empty announce-list survived: {head}"
        );

        let _ = seeder.join();
    }

    #[test]
    fn a_torrent_converts_to_a_magnet_and_nothing_else() {
        let fixture = TorrentFixture::multi_file();
        let out = run_ok(&["magnet", fixture.path_str()], fixture.dir());
        assert_eq!(out.lines().count(), 1, "output must be pipeable: {out}");
        assert!(out.starts_with("magnet:?xt=urn:btih:"), "{out}");
        assert!(out.contains(&fixture.info_hash), "{out}");
    }

    #[test]
    fn the_generated_magnet_carries_the_trackers_and_web_seeds() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(&["magnet", fixture.path_str()], fixture.dir());
        assert_eq!(doc["info_hash"], fixture.info_hash);
        assert_eq!(doc["name"], "album");
        assert_eq!(doc["length"]["bytes"], 2000);
        assert_eq!(doc["trackers"][0], "udp://tracker.example.com:80");
        assert_eq!(doc["web_seeds"][0], "https://mirror.example.com/pub/");
    }

    #[test]
    fn a_magnet_reads_back_without_touching_the_network() {
        let fixture = TorrentFixture::multi_file();
        let uri = run_ok(&["magnet", fixture.path_str()], fixture.dir());
        let doc = run_json(&["magnet", uri.trim()], fixture.dir());
        assert_eq!(doc["info_hash"], fixture.info_hash);
        assert_eq!(doc["name"], "album");
    }

    #[test]
    fn the_round_trip_is_stable() {
        let fixture = TorrentFixture::multi_file();
        let first = run_ok(&["magnet", fixture.path_str()], fixture.dir());
        let doc = run_json(&["magnet", first.trim()], fixture.dir());
        assert_eq!(doc["magnet"].as_str().unwrap(), first.trim());
    }
}
