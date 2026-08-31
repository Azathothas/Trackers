//! `bit-cli download`: fetch to completion in the foreground, then exit.
//!
//! One invocation, one session, no daemon. Sources are fetched with peers and
//! with HTTP sources at the same time, and the accounting keeps the two apart
//! so a caller can answer "where did these bytes come from".
//!
//! Progress reaches the caller three ways, all carrying the same numbers: a
//! line on stderr for a person, a `progress` event on stdout under `--jsonl`,
//! and the final document under `--json`. Nothing is display-only.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use bit_cli_core::ExitCode;
use bit_cli_core::engine::{AddOptions, Engine, TorrentSnapshot};
use bit_cli_core::error::{Error, Result};
use bit_cli_core::layout::Layout;
use bit_cli_core::metalink::{Agreement, Checksum, Metalink};
use bit_cli_core::paths::Rename;
use bit_cli_core::torrent::Metainfo;
use bit_cli_core::units::{Size, format_rate, format_size};
use bit_cli_core::webseed::binding::SourceSpec;
use bit_cli_core::webseed::fetch::Verify;
use bit_cli_core::webseed::ledger::LedgerStats;
use serde::Serialize;
use serde_json::json;
use tokio::sync::mpsc;

use crate::cli::{DownloadArgs, Global};
use crate::env::Env;
use crate::output::{Renderer, field};
use crate::source::{Kind, ResolvedMetalink};
use crate::swarm::{
    self, AttachedSource, Progress, SessionSetup, SourceReport, StopConditions, Stopped,
};
use crate::webseed_args;

/// What one finished torrent reports.
#[derive(Debug, Clone, Serialize)]
pub struct TorrentReport {
    pub source: String,
    pub info_hash: String,
    pub name: String,
    pub stopped: Stopped,
    pub finished: bool,
    pub total: Size,
    pub downloaded: Size,
    pub uploaded: Size,
    /// Bytes served by HTTP sources. The rest came from peers.
    pub from_web_seeds: Size,
    pub from_peers: Size,
    /// Bytes that were already on the disk when the torrent was added, found
    /// by the hash check. Charged to neither transport, because this run did
    /// not fetch them. See `TODO/multi-source.md`, T-139.
    pub from_resume: Size,
    pub elapsed_ms: u64,
    pub elapsed_human: String,
    pub mean_rate: Size,
    pub mean_rate_human: String,
    pub peers_seen: u32,
    /// Every time `--redial-after` fired, and what the run had been waiting
    /// for when it did. Empty when the flag is off or the run never stalled.
    /// See `TODO/peers.md`, T-138.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub redials: Vec<Redial>,
    pub sources: Vec<SourceReport>,
    pub output_directory: String,
    /// Files whose on-disk path is not the path in the torrent, and why.
    /// Empty for the ordinary torrent. See `bit_cli_core::paths`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub renamed: Vec<Rename>,
    /// Files read from another torrent in the same run rather than fetched.
    /// Empty unless two torrents in one invocation hold the same file.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<SharedFile>,
    /// Files `--select-file` did not choose that a boundary piece wrote into
    /// anyway. Empty without a selection, and empty for a torrent whose file
    /// boundaries fall on piece edges. See `TODO/disk-io.md`, T-184.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub partial: Vec<PartialFile>,
    /// Announces this run sent itself: `completed` when the download
    /// finished and `stopped` when it ended. Empty when the torrent has no
    /// trackers or `--no-tracker` was given.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub announced: Vec<SentAnnounce>,
    /// What the Metalink said and whether the payload agreed with it. Present
    /// only when the source was a Metalink. See `TODO/cli-surface.md`, T-113.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metalink: Option<MetalinkReport>,
    /// What the block-to-source ledger did, for a run with HTTP sources.
    ///
    /// `evicted` is the one number worth reading on a healthy run: it counts
    /// pieces whose records were dropped before they could be resolved, so it
    /// is how many pieces could no longer have been attributed if they had
    /// turned out wrong. Absent when the run attached no sources, because a
    /// ledger with nothing to record says nothing.
    /// See `TODO/webseed.md`, T-179.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<LedgerStats>,
    /// A digest per file, from `--verify-on-complete`.
    ///
    /// Absent unless the flag was given. It is redundant with the piece hashes
    /// by construction and that is the point: it is the check a caller can run
    /// without trusting the thing that wrote the bytes, and the one whose
    /// output can be compared against a digest published somewhere else. See
    /// `docs/integrity.md` and `TODO/multi-source.md`, T-136.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub verified_files: Vec<VerifiedFile>,
    /// The exit code this torrent's outcome produces.
    ///
    /// A run's code is the worst of its torrents'. Without this, a torrent
    /// that failed because a file was already there and one that failed
    /// because the tracker was unreachable would both arrive as a generic
    /// failure, which is exactly the distinction the exit code table exists to
    /// make. See `TODO/disk-io.md`, T-014.
    pub code: ExitCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Which phase gave up, when the error named one.
    ///
    /// The error carries a context map and this report carried none of it, so
    /// a run that stopped resolving a magnet and a run that stopped fetching
    /// its pieces both said `timeout` and nothing else. A reader then has to
    /// guess which flag to reach for. See `TODO/cli-surface.md`, T-196.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

/// What a Metalink said, and whether the bytes on disk agreed with it.
///
/// A Metalink and a `.torrent` are two independent descriptions of the same
/// payload, and this run has both. Two checks come out of that, and they fail
/// for different reasons.
///
/// The first costs nothing and runs before a byte is fetched: the two
/// documents each declare a length, and lengths that differ mean they describe
/// different files. The second runs on the payload the session has already
/// verified piece by piece against the torrent's own SHA-1 hashes, so a
/// checksum that then disagrees says the Metalink is the document that is
/// wrong, not the torrent. Saying which one is wrong is the whole point of
/// carrying both. See `TODO/cli-surface.md`, T-113.
#[derive(Debug, Clone, Serialize)]
pub struct MetalinkReport {
    /// `4` for RFC 5854 `.meta4`, `3` for the older `.metalink`.
    pub version: &'static str,
    /// The `<file name>` the document carried.
    pub file: String,
    /// The `<metaurl>` the `.torrent` was fetched from.
    pub torrent_url: String,
    /// Torrent URLs tried before that one, and what went wrong with each.
    /// Empty when the document's first choice answered.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub torrent_fallbacks: Vec<MirrorError>,
    /// Mirrors the document listed for the payload.
    pub mirrors_listed: usize,
    /// Mirrors that became sources in this run. Lower than `mirrors_listed`
    /// when `--no-torrent-web-seed` or `--no-web-seed` dropped them, or when
    /// the document's file could not be attributed to one file of a multi-file
    /// torrent, in which case `agreement.matched_by` says why.
    pub mirrors_registered: usize,
    /// Mirrors the document listed under a scheme this cannot fetch, `ftp:`
    /// being the one that occurs. Counted so the report can say the document
    /// had more in it than the run could use.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mirrors_unsupported: Vec<String>,
    /// What the two documents say about the file's length, compared before
    /// anything was fetched.
    pub agreement: MetalinkAgreement,
    /// The checksum the document supplied, when it supplied one this can
    /// compute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<MetalinkChecksum>,
}

/// One torrent URL that did not answer.
#[derive(Debug, Clone, Serialize)]
pub struct MirrorError {
    pub url: String,
    pub error: String,
}

/// What the Metalink and the `.torrent` each say about the same file.
#[derive(Debug, Clone, Serialize)]
pub struct MetalinkAgreement {
    /// The file index in the torrent this entry was attributed to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_index: Option<usize>,
    /// Which rule attributed it, or why none could: `only_file`, `path`,
    /// `prefixed_path`, `file_name`, `ambiguous`, `no_match`, `no_name`.
    pub matched_by: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metalink_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub torrent_size: Option<u64>,
    /// `true` when both declare a length and the two are equal, `false` when
    /// they differ, absent when either is missing. Absent is neither
    /// agreement nor disagreement and must not be read as either.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_agrees: Option<bool>,
}

/// The Metalink's own checksum, and what checking it found.
#[derive(Debug, Clone, Serialize)]
pub struct MetalinkChecksum {
    pub algorithm: String,
    pub expected: String,
    /// What the payload hashes to. Absent when the check did not run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    /// `true` when the payload matched, `false` when it did not, absent when
    /// the check did not run. Absent is not a pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<bool>,
    /// Bytes read to compute it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_hashed: Option<u64>,
    /// The file that was hashed, as it sits on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Why the check did not run, when it did not. A checksum that was not
    /// computed is not a checksum that passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_checked: Option<String>,
}

/// One announce this run sent itself, beyond the session's own.
///
/// The session announces `started` when a torrent goes live and then repeats
/// on the tracker's interval. It never says a download finished and never says
/// it stopped, so a tracker's seeder count is wrong and a dead address is
/// handed out until the record expires. `bit-cli` runs in the foreground and
/// knows both moments exactly. See `TODO/trackers.md`, T-062.
#[derive(Debug, Clone, Serialize)]
pub struct SentAnnounce {
    /// `completed` or `stopped`.
    pub event: &'static str,
    /// Trackers it was sent to.
    pub trackers: usize,
    /// Trackers that answered without a failure.
    pub accepted: usize,
    /// Milliseconds into the run.
    pub at_ms: u64,
}

/// One file this torrent read from another torrent in the same run.
///
/// The proof is in the metadata: `pieces_compared` whole pieces of this file
/// have the same SHA-1 in both torrents, so the bytes those pieces cover are
/// the same. Nothing here is asserted by the caller, and the source is checked
/// per piece on the way in like every other source. See
/// `TODO/multi-source.md`, T-140.
#[derive(Debug, Clone, Serialize)]
pub struct SharedFile {
    /// File index in this torrent.
    pub index: usize,
    /// This torrent's path for it.
    pub path: String,
    pub length: Size,
    /// The source argument of the torrent it was read from.
    pub from_source: String,
    pub from_info_hash: String,
    /// File index in that torrent.
    pub from_index: usize,
    /// Where it was read from on disk.
    pub from_path: String,
    /// Whole pieces whose hashes were compared, all of which agreed.
    pub pieces_compared: u32,
    pub bytes_proven: Size,
}

/// One file a selection did not choose, holding bytes anyway.
///
/// A piece that straddles the boundary between a selected file and an
/// unselected one carries bytes of both and cannot be verified without them,
/// so the unselected file is written into whatever the selection said. What
/// lands is a file holding those bytes and nothing else.
///
/// `bytes` is how much of it is real, `on_disk` is how long it ends up, and
/// `length` is how long the torrent says it is. `on_disk` equal to `length` is
/// the case worth reporting: the file looks complete in a directory listing
/// and is almost entirely zeroes. See `TODO/disk-io.md`, T-184.
#[derive(Debug, Clone, Serialize)]
pub struct PartialFile {
    pub index: usize,
    pub path: String,
    pub bytes: Size,
    pub on_disk: Size,
    pub length: Size,
}

/// One forced re-dial: the peer state was thrown away and the peer list
/// dialled again, because nothing had arrived for `--redial-after`.
#[derive(Debug, Clone, Serialize)]
pub struct Redial {
    /// Which re-dial this was, counting from 1.
    pub attempt: u32,
    /// Milliseconds into the run.
    pub at_ms: u64,
    /// How long the byte count had been flat when it fired.
    pub stalled_ms: u64,
    /// Live peer connections thrown away, which is what this cost.
    pub peers_dropped: u32,
    /// The reason it did not happen, when it did not. `None` on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One file re-read from disk after the download finished.
///
/// `sha256` rather than the torrent's own `sha1` piece hashes, because this
/// digest exists to be compared against one published somewhere else and
/// nobody publishes a per-file sha1 of a torrent's contents. The piece hashes
/// have already been checked twice by the time this runs.
/// See `TODO/multi-source.md`, T-136.
#[derive(Debug, Clone, Serialize)]
pub struct VerifiedFile {
    /// Index into the torrent's file list.
    pub index: usize,
    /// The path as the metainfo gives it, `/`-separated.
    pub torrent_path: String,
    /// Where it was actually read from, absolute.
    pub disk_path: String,
    /// The algorithm, always `sha256` today.
    pub algorithm: String,
    /// Lowercase hex.
    pub hex: String,
    /// Bytes read. A caller comparing this against `length` learns whether it
    /// hashed the whole file.
    pub bytes: u64,
    /// What the torrent says the file is.
    pub length: u64,
    /// Why it could not be hashed, when it could not. Absent on success rather
    /// than empty: a digest that was not computed is not one that matched, and
    /// this is the field that says which.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What the whole run reports.
#[derive(Debug, Clone, Serialize)]
pub struct DownloadReport {
    pub torrents: Vec<TorrentReport>,
    pub total: Size,
    pub downloaded: Size,
    pub from_web_seeds: Size,
    pub from_peers: Size,
    /// Bytes that were already on the disk when the torrent was added, found
    /// by the hash check. Charged to neither transport, because this run did
    /// not fetch them. See `TODO/multi-source.md`, T-139.
    pub from_resume: Size,
    pub elapsed_ms: u64,
    pub elapsed_human: String,
    pub completed: usize,
    pub failed: usize,
    /// What this run cost: peak RSS, CPU time, and open handles.
    ///
    /// Measuring a download from outside means sampling a process that has
    /// already exited, which reports zero. The process is the only thing that
    /// can report its own high-water mark, so it does.
    pub process: bit_cli_core::sysinfo::Process,
    /// What the `--on-*` hooks did, when any were given.
    ///
    /// Absent when none were, rather than a block of zeroes on every run that
    /// used no hook. `skipped` is the one that matters: a
    /// `--on-piece-verified` slower than pieces arrive is counted rather than
    /// waited for. See `docs/hooks.md` and `TODO/cli-surface.md`, T-115.
    #[serde(skip_serializing_if = "crate::hooks::HookCounts::is_empty")]
    pub hooks: crate::hooks::HookCounts,
    /// What this run's storage did.
    pub disk: DiskTotals,
}

/// Bytes written to the payload and the time those writes took.
///
/// Both were already being counted and neither was reported: `--trace disk`
/// carries the events and nothing totalled them, so the two numbers that say
/// whether a slow run was slow at the disk existed only as a log. The counters
/// are always on, at two `Instant::now()` calls per write, which is the price
/// of a run being able to say where the time went rather than guessing. See
/// `crate::storage::StorageMetrics` and `TODO/cli-surface.md`, T-252.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct DiskTotals {
    /// Bytes that reached the device, which is more than `downloaded` when a
    /// piece was written twice and less when the run resumed.
    pub bytes_written: Size,
    /// Wall time inside those writes, summed across every worker, so it can
    /// exceed the run's own elapsed time.
    pub write_time: bit_cli_core::units::Millis,
    /// Positioned writes that reached the device.
    pub write_ops: u64,
    /// Writes the session asked for, before any were combined. `write_ops`
    /// over this is the coalescing factor. See `TODO/disk-io.md`, T-018.
    pub write_calls: u64,
}

/// A message from a worker to the one thread that owns the output streams.
enum Msg {
    Event(&'static str, serde_json::Value),
    Warn(String),
    Progress(String),
    Done(Box<TorrentReport>),
}

/// Run the command.
pub fn run(
    args: &DownloadArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    // The `aria2` aliases are close to their originals and not identical, and
    // saying so is what `docs/flags.md` asks for instead of refusing them.
    // Emitted here, once per run, before anything is resolved. See
    // `TODO/performance.md`, T-033.
    for note in webseed_args::aria2_notes(&args.web_seeds) {
        renderer.warn(env, note);
    }
    let report_interval = swarm::duration_flag(&args.report_interval, "report-interval")?;
    let stop = StopConditions {
        timeout: swarm::optional_duration(&global.timeout, "timeout")?,
        stop_after: swarm::optional_duration(&global.stop_after, "stop-after")?,
        stall: swarm::optional_duration(&args.limits.stop_timeout, "stop-timeout")?,
        lowest_rate: swarm::rate_flag(&args.limits.lowest_speed_limit, "lowest-speed-limit")?,
        seed_ratio: args.limits.seed_ratio,
        seed_time: swarm::optional_duration(&args.limits.seed_time, "seed-time")?,
        exit_when_idle: None,
        max_handles: args.limits.max_handles,
        max_rss: swarm::size_flag(&args.limits.max_rss, "max-rss")?,
        // `download` does not offer `--listener-check`. See `TODO/peers.md`,
        // T-020: the probe watches one listener and a `-j` run has one
        // session behind several of these loops, so the flag lives on `seed`,
        // which is the long-lived shape the entry is about.
        listener: None,
    };

    let setup = SessionSetup {
        global,
        trackers: &args.trackers,
        limits: &args.limits,
        web_seeds: &args.web_seeds,
        listen_ports: swarm::port_range(&args.port)?,
        no_dht: args.no_dht,
        no_lsd: args.no_lsd,
        allocation: allocation_of(args.selection.file_allocation),
    };
    let engine_options = setup.engine_options(env)?;
    // Parsed here so a bad rate fails before the session starts, next to the
    // session caps that `engine_options` just read. See `TODO/cli-surface.md`,
    // T-181.
    let (torrent_download_rate, torrent_upload_rate) = setup.torrent_rates()?;
    let directory = engine_options.download_directory.clone();

    // `-o`/`--out` names one payload's destination, so a run with two sources
    // would be telling both to write to the same path. Refused here, before
    // the session starts, rather than per worker: by the time the second
    // worker noticed, the first would already have created files.
    // See `TODO/cli-surface.md`, T-226.
    if args.selection.out.is_some() && args.sources.len() > 1 {
        return Err(Error::usage(format!(
            "--out names where one payload goes and this run has {} sources; use --dir for the directory they share",
            args.sources.len()
        ))
        .with("sources", args.sources.len()));
    }
    // Relative to `--dir` where one is given, and to the working directory
    // otherwise, so neither flag is silently inert beside the other:
    // `--dir out --out album` is `out/album`. `directory` is already absolute,
    // so joining a relative `--out` onto it is the whole rule, and an absolute
    // `--out` is the caller naming a destination outright, which is what `-o`
    // means everywhere else and what `--dir` is already allowed to do.
    //
    // Normalised lexically rather than with `canonicalize`, which needs the
    // path to exist and returns an extended-length prefix on Windows. Without it a
    // `..` survives into the report, and the report is what says where the
    // payload went. See `TODO/cli-surface.md`, T-226.
    let out = args
        .selection
        .out
        .as_ref()
        .map(|path| match path.is_absolute() {
            true => normalise(path),
            false => normalise(&directory.join(path)),
        });

    if global.dry_run {
        return dry_run(args, global, &setup, renderer, env, &directory);
    }

    // Every source is classified before the session starts, so a typo in the
    // fifth argument fails before the first byte is fetched.
    let kinds: Vec<Kind> = args
        .sources
        .iter()
        .map(|source| Kind::classify(source, env))
        .collect::<Result<_>>()?;

    // A source that carries no metadata, with nothing left that could fetch
    // it. Refused here rather than waited on: the run would otherwise sit in
    // `wait_until_initialized` until a deadline it cannot meet, and report a
    // timeout, which reads like the network being slow. See `TODO/dht.md`,
    // T-051.
    if let Some(source) = metadata_that_cannot_arrive(&kinds, args, &setup) {
        return Err(Error::usage(format!(
            "{source} carries no metadata and every way of fetching it is off.              A web seed serves payload, not the torrent file: name a .torrent,              or leave one of the DHT, the trackers or local discovery on"
        )));
    }

    // One runtime for the whole command. It is built here rather than beside
    // the session because a Metalink has to be resolved over HTTP before the
    // plans can be built, and resolving it needs somewhere to run.
    let runtime = swarm::runtime()?;
    // The Metalink fetch below is bounded by `--timeout` the same way a source
    // fetch in every other command is, so one flag means one thing everywhere.
    // See `TODO/cli-surface.md`, T-245.
    let fetch_deadline = crate::source::deadline(stop.timeout);
    // The document fetch presents as a browser by default, the same as every
    // other command's does, because a Metalink URL and a page URL are read by
    // the same origins. `download` has no `--page-client` of its own, so it
    // takes the default. See `TODO/cli-surface.md`, T-244.
    let identity = crate::source::Identity {
        user_agent: args
            .web_seeds
            .web_seed_user_agent
            .clone()
            .unwrap_or_else(bit_cli_core::webseed::fetch::default_user_agent),
        user_agent_given: args.web_seeds.web_seed_user_agent.is_some(),
        profile: bit_cli_core::page::ClientProfile::default(),
    };
    let mut resolved: std::collections::HashMap<usize, ResolvedMetalink> =
        std::collections::HashMap::new();
    // A Metalink is either a path or a URL, and only where the document comes
    // from differs: `resolve_metalink` takes a parsed document either way. See
    // `TODO/cli-surface.md`, T-154.
    enum Document {
        Local(std::path::PathBuf),
        Remote(String),
    }
    let metalinks: Vec<(usize, Document)> = kinds
        .iter()
        .enumerate()
        .filter_map(|(index, kind)| match kind {
            Kind::Metalink(path) => Some((index, Document::Local(path.clone()))),
            Kind::MetalinkUrl(url) => Some((index, Document::Remote(url.clone()))),
            _ => None,
        })
        .collect();
    if !metalinks.is_empty() {
        runtime.block_on(async {
            for (index, from) in metalinks {
                let document = match &from {
                    Document::Local(path) => Metalink::read(path)?,
                    Document::Remote(url) => {
                        crate::source::fetch_metalink(url, &identity, fetch_deadline).await?
                    }
                };
                let one =
                    crate::source::resolve_metalink(&document, &identity, fetch_deadline).await?;
                renderer.event(
                    env,
                    "metalink_resolved",
                    &json!({
                        "source": args.sources[index],
                        "version": one.version,
                        "file": one.file.name,
                        "torrent_url": one.torrent_url,
                        "info_hash": one.meta.info_hash().hex(),
                        "mirrors": one.file.mirrors.len(),
                        "unsupported_mirrors": one.file.unsupported_mirrors.len(),
                        "checksums": one.file.checksums.len(),
                    }),
                )?;
                resolved.insert(index, one);
            }
            Ok::<(), Error>(())
        })?;
    }

    let mut plans = Vec::with_capacity(args.sources.len());
    let mut known_hashes: HashSet<String> = HashSet::new();
    let mut metas: Vec<Option<Metainfo>> = Vec::with_capacity(args.sources.len());
    for (index, source) in args.sources.iter().enumerate() {
        let one = resolved.remove(&index);
        let meta = match (&kinds[index], &one) {
            (Kind::File(path), _) => Some(crate::source::read_torrent_file(path)?),
            (Kind::Metalink(_) | Kind::MetalinkUrl(_), Some(one)) => Some(one.meta.clone()),
            _ => None,
        };
        if let Some(meta) = &meta {
            known_hashes.insert(meta.info_hash().hex().to_ascii_lowercase());
        }
        // `--web-seed-list-url` is fetched here, on the runtime this command
        // already built. Every caller used to pass `no_network`, including
        // this one, so the flag parsed, was read, and could only ever fail.
        // See `TODO/cli-surface.md`, T-183.
        let specs = webseed_args::collect(
            &args.web_seeds,
            meta.as_ref(),
            one.as_ref().map(|m| &m.file),
            env,
            crate::source::list_fetcher(&runtime, &identity.user_agent),
        )?;
        // `--tracker-list-url` is fetched on the runtime this command already
        // built, rather than on one of its own. See `TODO/cli-surface.md`,
        // T-181.
        let trackers = setup.tracker_list(
            meta.as_ref(),
            env,
            crate::source::list_fetcher(&runtime, &identity.user_agent),
        )?;
        let (torrent_bytes, metalink) = match one {
            None => (None, None),
            Some(one) => {
                let registered = specs
                    .iter()
                    .filter(|spec| spec.origin == bit_cli_core::webseed::binding::Origin::Metalink)
                    .count();
                // `one.meta` is the torrent this Metalink named, so the two
                // documents being compared are always both present here.
                let agreement = one.file.agreement(&one.meta.layout());
                if agreement.disagrees() {
                    renderer.warn(
                        env,
                        format!(
                            "{source}: the metalink says the file is {} bytes and the torrent says {}. One of the two is wrong; the payload is checked against both.",
                            agreement.metalink_size.unwrap_or_default(),
                            agreement.torrent_size.unwrap_or_default(),
                        ),
                    );
                }
                let best = one.file.best_checksum().cloned();
                let unusable_algorithm = match &best {
                    Some(_) => None,
                    None => one.file.checksums.first().map(|c| c.algorithm.clone()),
                };
                let plan = MetalinkPlan {
                    version: one.version,
                    file_name: one.file.name.clone(),
                    torrent_url: one.torrent_url.clone(),
                    torrent_fallbacks: one
                        .torrent_errors
                        .iter()
                        .map(|(url, error)| MirrorError {
                            url: url.clone(),
                            error: error.clone(),
                        })
                        .collect(),
                    mirrors_listed: one.file.mirrors.len(),
                    mirrors_registered: registered,
                    mirrors_unsupported: one.file.unsupported_mirrors.clone(),
                    agreement,
                    checksum: best,
                    unusable_algorithm,
                };
                (Some(one.torrent_bytes), Some(Box::new(plan)))
            }
        };
        let files = plan_selection(&args.selection, meta.as_ref())?;
        // Checked here, before the session starts, wherever the count is
        // already known. A magnet's is not, and is checked once its metadata
        // resolves. See `TODO/cli-surface.md`, T-116.
        let file_count = meta.as_ref().map(|m| m.layout().files.len());
        crate::selection::index_out(&args.selection.index_out, file_count)?;
        plans.push(Plan {
            index,
            source: source.clone(),
            torrent_bytes,
            metalink,
            specs,
            trackers,
            files,
            multi_file: meta.as_ref().map(|m| m.info().multi_file),
            file_count,
            donations: Vec::new(),
        });
        metas.push(meta);
    }

    // Two torrents in one run that hold the same file, proven by their piece
    // hashes, are one fetch and one copy rather than two fetches. The proof is
    // computed here, from metadata that is already read, and costs one pass
    // per pair of torrents. Which of them can actually donate is decided when
    // each starts, because it depends on the donor having finished. See
    // `TODO/multi-source.md`, T-140.
    if !args.no_share_files {
        for (plan, donations) in plans.iter_mut().zip(share_plan(&metas)) {
            plan.donations = donations;
        }
    }
    // What the window caches will cost, said before the run rather than
    // discovered in a resident set. Raised here rather than in the worker
    // because a run with `-j 4` would otherwise say it four times, and once is
    // the useful number of times. Named by source where there is more than one
    // torrent, because the chunk size is per source. See `TODO/memory.md`,
    // T-041.
    for plan in &plans {
        if let Some(message) = cache_budget_warning(&plan.specs) {
            match plans.len() {
                1 => renderer.warn(env, message),
                _ => renderer.warn(env, format!("{}: {message}", plan.source)),
            }
        }
    }
    let donor_files: SharedDonors = Arc::new(std::sync::Mutex::new(
        std::collections::HashMap::with_capacity(plans.len()),
    ));
    for (index, meta) in metas.iter().enumerate() {
        // A magnet has no metadata yet, so it can neither donate nor receive.
        // Recording the source and hash of the ones that do keeps the report
        // able to name the donor without carrying the metainfo around.
        if let Some(meta) = meta {
            let layout = meta.layout();
            let mut map = donor_files.lock().expect("donor registry");
            map.insert(
                index,
                DonorFiles {
                    source: args.sources[index].clone(),
                    info_hash: meta.info_hash().hex(),
                    root: bit_cli_core::storage::payload_root(&directory, &layout),
                    // Filled in when the torrent finishes. An empty list is
                    // what says it has nothing to lend yet.
                    disk_paths: Vec::new(),
                },
            );
        }
    }

    // A binding for a torrent that is not in this invocation binds nothing,
    // and `collect` drops it per torrent without knowing that. A mistyped
    // forty character hash would otherwise be a run that quietly used no
    // source at all.
    for (binding, hash) in webseed_args::qualified_torrents(&args.web_seeds) {
        if !known_hashes.contains(&hash) {
            let known: Vec<String> = known_hashes.iter().cloned().collect();
            return Err(Error::usage(format!(
                "--web-seed-for `{binding}` names info hash {hash}, which is not one of the torrents in this run"
            ))
            .with("value", binding)
            .with("torrents", known.join(", ")));
        }
    }

    let init_timeout = swarm::duration_flag(&args.limits.init_timeout, "init-timeout")?;
    // The courtesy announces at the end of a run use the same timeouts
    // `bit-cli trackers` does, because they are the same client talking to the
    // same trackers. See `TODO/trackers.md`, T-062.
    let tracker_timeout =
        swarm::optional_duration(&args.trackers.tracker_timeout, "tracker-timeout")?
            .unwrap_or(Duration::from_secs(30));
    let tracker_connect_timeout = swarm::optional_duration(
        &args.trackers.tracker_connect_timeout,
        "tracker-connect-timeout",
    )?
    .unwrap_or(Duration::from_secs(10));
    let trace_http = global.trace.iter().any(|t| t == "http");
    // A source-level check is per piece or nothing. `file` asks for a coarser
    // grain than the fetcher works at, and the per-piece check subsumes it, so
    // it gets the stronger check and is told so rather than silently ignored.
    if args.web_seeds.web_seed_verify == crate::cli::VerifyWhen::File {
        renderer.warn(
            env,
            "--web-seed-verify file is served by the per-piece check, which is stricter",
        );
    }
    let verify = match args.web_seeds.web_seed_verify {
        crate::cli::VerifyWhen::None => Verify::None,
        crate::cli::VerifyWhen::Piece | crate::cli::VerifyWhen::File => Verify::Piece,
    };
    let peers = swarm::peer_addrs(&args.peers)?;
    let redial_after = swarm::optional_duration(&args.redial_after, "redial-after")?;
    if let (Some(redial), Some(stall)) = (redial_after, stop.stall)
        && redial >= stall
    {
        renderer.warn(
            env,
            format!(
                "--redial-after {} is not shorter than --stop-timeout {}, so the run gives up before it re-dials",
                bit_cli_core::units::format_duration(redial),
                bit_cli_core::units::format_duration(stall),
            ),
        );
    }
    let concurrency = args.max_concurrent_downloads.max(1);
    let started = std::time::Instant::now();

    // One worker thread for `--on-piece-verified`, shared by every download in
    // the run, started before the session and stopped after it. See
    // `docs/hooks.md` for what it costs and why it is bounded.
    let piece_hook = args
        .on_piece_verified
        .clone()
        .map(|command| Arc::new(crate::hooks::PieceHook::start(command)));

    let outcome = runtime.block_on(async {
        let engine = Arc::new(Engine::start(&engine_options).await?);
        for warning in engine.warnings() {
            renderer.warn(env, warning);
        }

        renderer.event(
            env,
            "session_start",
            &json!({
                "sources": args.sources.len(),
                "directory": directory.display().to_string(),
                "listen_addr": engine.listen_addr().map(|a| a.to_string()),
                "max_concurrent_downloads": concurrency,
            }),
        )?;

        let (tx, mut rx) = mpsc::channel::<Msg>(256);
        // A queue of plans taken in order by a fixed pool of workers, rather
        // than one task per plan queuing on a semaphore. Two reasons. The
        // order torrents start in is then the order they were given, which is
        // what makes `-j 1` a sequence a caller can depend on: a torrent whose
        // source is a file an earlier torrent writes needs the earlier one to
        // go first. And a hundred sources no longer spawn a hundred tasks that
        // do nothing but wait.
        let queue = Arc::new(tokio::sync::Mutex::new(
            plans.into_iter().collect::<std::collections::VecDeque<_>>(),
        ));
        let workers_wanted = concurrency.min(queue.lock().await.len().max(1));
        let mut workers = tokio::task::JoinSet::new();
        for _ in 0..workers_wanted {
            let engine = engine.clone();
            let tx = tx.clone();
            let queue = queue.clone();
            let options = Options {
                // Existing data is hash-checked on add, and the check is what
                // makes resuming safe. All four of these flags mean "look at
                // what is already on disk", so they all reach the session the
                // same way.
                overwrite: args.allow_overwrite
                    || !args.no_continue
                    || args.check_integrity
                    || args.hash_check_only,
                hash_check_only: args.hash_check_only,
                init_timeout,
                select_file: args.selection.select_file.clone(),
                exclude_file: args.selection.exclude_file.clone(),
                index_out: args.selection.index_out.clone(),
                out: out.clone(),
                piece_hook: piece_hook.clone(),
                verify_on_complete: args.verify_on_complete,
                report_interval,
                stop: stop.clone(),
                require: args.web_seeds.web_seed_require,
                web_seed_only: args.web_seeds.web_seed_only,
                max_total: args.web_seeds.web_seed_max_total,
                prefer: args.web_seeds.prefer_web_seed,
                verify,
                trace_http,
                directory: directory.clone(),
                peers: peers.clone(),
                in_order: wants_in_order(args.selection.piece_selector),
                redial_after,
                max_redials: args.max_redials,
                donors: donor_files.clone(),
                tracker_timeout,
                tracker_connect_timeout,
                torrent_download_rate,
                torrent_upload_rate,
            };
            workers.spawn(async move {
                loop {
                    // The lock is held only to take the next plan, never
                    // across the download, so one slow torrent does not hold
                    // the queue.
                    let Some(plan) = queue.lock().await.pop_front() else {
                        break;
                    };
                    let report = one(&engine, plan, options.clone(), &tx).await;
                    let _ = tx.send(Msg::Done(Box::new(report))).await;
                }
            });
        }
        drop(tx);

        let mut reports = Vec::new();
        while let Some(msg) = rx.recv().await {
            match msg {
                Msg::Event(kind, value) => renderer.event(env, kind, &value)?,
                Msg::Warn(text) => renderer.warn(env, text),
                Msg::Progress(line) => {
                    if renderer.progress == crate::cli::ProgressMode::Plain {
                        let _ = env.note(line);
                    }
                }
                Msg::Done(report) => reports.push(*report),
            }
        }
        while workers.join_next().await.is_some() {}
        // Storage runs on the session's threads and the streams belong to this
        // one, so it collects what the caller should know and this is where it
        // is read: an allocation method that could not be used, and what ran
        // instead.
        for note in engine.storage_notes() {
            renderer.warn(env, note);
        }
        // Read before the engine is dropped, because the counters live on it
        // and nothing else can reach them afterwards.
        let disk = engine.storage_counts();
        Arc::try_unwrap(engine).ok().map(Engine::stop);

        Ok::<_, Error>((reports, disk))
    });

    // Before `outcome?`, so a run that failed still stops the worker and does
    // not leave a thread joined only by `Drop` after the error propagates.
    let piece_hook_counts = match piece_hook {
        None => crate::hooks::HookCounts::default(),
        Some(hook) => match Arc::try_unwrap(hook) {
            Ok(hook) => hook.finish(),
            // Every worker holds a clone and every worker has finished by
            // here, so this is unreachable in practice. Counting nothing is
            // the honest answer if it ever is not, rather than a panic in the
            // reporting path of a download that worked.
            Err(_) => crate::hooks::HookCounts::default(),
        },
    };

    let (mut reports, disk) = outcome?;
    reports.sort_by(|a, b| a.source.cmp(&b.source));

    let elapsed = started.elapsed();
    let mut report = DownloadReport {
        total: Size(reports.iter().map(|r| r.total.0).sum()),
        downloaded: Size(reports.iter().map(|r| r.downloaded.0).sum()),
        from_web_seeds: Size(reports.iter().map(|r| r.from_web_seeds.0).sum()),
        from_peers: Size(reports.iter().map(|r| r.from_peers.0).sum()),
        from_resume: Size(reports.iter().map(|r| r.from_resume.0).sum()),
        completed: reports.iter().filter(|r| r.finished).count(),
        failed: reports.iter().filter(|r| !r.finished).count(),
        elapsed_ms: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
        elapsed_human: bit_cli_core::units::format_duration(elapsed),
        torrents: reports,
        process: bit_cli_core::sysinfo::Process::sample(),
        hooks: piece_hook_counts,
        disk: DiskTotals {
            bytes_written: Size(disk.write_bytes),
            write_time: bit_cli_core::units::Millis(disk.write_nanos / 1_000_000),
            write_ops: disk.write_ops,
            write_calls: disk.write_calls,
        },
    };

    // The worst outcome decides the exit code, so a run with one failed
    // torrent never exits zero.
    let code = report
        .torrents
        .iter()
        .map(|r| r.code)
        .max_by_key(|c| c.code())
        .unwrap_or(ExitCode::Success);

    let finished_counts = run_hooks(&report, args, renderer, env);
    report.hooks.ran += finished_counts.ran;
    report.hooks.failed += finished_counts.failed;
    if report.hooks.skipped > 0 {
        renderer.warn(
            env,
            format!(
                "--on-piece-verified was skipped {} time(s): the hook is slower than pieces arrive. docs/hooks.md",
                report.hooks.skipped
            ),
        );
    }
    renderer.emit(env, "download", &report, || lines(&report))?;
    Ok(code)
}

/// The CLI's allocation names, as the core knows them.
///
/// Two enums for one concept because the core does not depend on `clap` and
/// the CLI does not define storage behaviour. The mapping is total, so a new
/// method cannot be added on one side without the other failing to compile.
pub(crate) fn allocation_of(method: crate::cli::FileAllocation) -> bit_cli_core::alloc::Allocation {
    use bit_cli_core::alloc::Allocation;
    match method {
        crate::cli::FileAllocation::None => Allocation::None,
        crate::cli::FileAllocation::Sparse => Allocation::Sparse,
        crate::cli::FileAllocation::Prealloc => Allocation::Prealloc,
        crate::cli::FileAllocation::Falloc => Allocation::Falloc,
    }
}

/// Resolve `.` and `..` in a path without touching the filesystem.
///
/// `std::fs::canonicalize` needs every component to exist, which `--out`'s
/// does not yet, and on Windows it returns an extended-length prefixed path that no
/// caller wants to read in a report. A `..` with nothing before it is kept,
/// because dropping it would change which directory the path names.
/// See `TODO/cli-surface.md`, T-226.
fn normalise(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    match out.as_os_str().is_empty() {
        true => std::path::PathBuf::from("."),
        false => out,
    }
}

/// Everything one worker needs beyond the plan.
#[derive(Clone)]
struct Options {
    overwrite: bool,
    hash_check_only: bool,
    /// How long the hash check gets before the run gives up on it.
    init_timeout: Duration,
    /// `--select-file` and `--exclude-file` as given. Carried unresolved
    /// because a magnet's `Plan` cannot resolve them until its metadata does,
    /// and every other plan already has. See `TODO/cli-surface.md`, T-185.
    select_file: Vec<String>,
    exclude_file: Vec<String>,
    /// `-O`/`--index-out` as given, for the same reason: a magnet has no file
    /// count until its metadata resolves, and the count is what makes an index
    /// past the end a usage error. See `TODO/cli-surface.md`, T-116.
    index_out: Vec<String>,
    /// `-o`/`--out`, already resolved against `--dir` and the working
    /// directory. `None` when the flag was not given, which is the ordinary
    /// case and leaves the session's own rule in force. See
    /// `TODO/cli-surface.md`, T-226.
    out: Option<std::path::PathBuf>,
    /// `--on-piece-verified`, already running, or `None` when the flag was not
    /// given. Shared by every worker: one command means one queue and one
    /// thread whatever `-j` is. See `TODO/cli-surface.md`, T-115.
    piece_hook: Option<Arc<crate::hooks::PieceHook>>,
    /// `--verify-on-complete`: re-read the finished payload and report a
    /// digest per file. See `TODO/multi-source.md`, T-136.
    verify_on_complete: bool,
    report_interval: Duration,
    stop: StopConditions,
    require: bool,
    web_seed_only: bool,
    max_total: Option<usize>,
    prefer: bool,
    verify: Verify,
    trace_http: bool,
    directory: std::path::PathBuf,
    /// How long with no progress before every peer connection is dropped and
    /// the peer list is dialled again. `None` never re-dials.
    redial_after: Option<Duration>,
    /// How many times that may happen in one run.
    max_redials: u32,
    /// Peers to dial before any are discovered, from `--peer`.
    peers: Vec<std::net::SocketAddr>,
    /// Whether to hold the session's piece priority at the front of what is
    /// missing. See `TODO/performance.md`, T-032.
    in_order: bool,
    /// Where each torrent in the run wrote its files, filled in as they
    /// finish. See `TODO/multi-source.md`, T-140.
    donors: SharedDonors,
    /// How long a courtesy announce at the end of a run waits. See
    /// `TODO/trackers.md`, T-062.
    tracker_timeout: Duration,
    tracker_connect_timeout: Duration,
    /// The per-torrent rate caps, from `--max-download-rate` and
    /// `--max-upload-rate`. The whole-run pair is on the session instead. See
    /// `TODO/cli-surface.md`, T-181.
    torrent_download_rate: Option<u64>,
    torrent_upload_rate: Option<u64>,
}

/// Whether a selector asks for pieces front to back.
///
/// `sequential` and `in-order` are the same behaviour under two names, one
/// common and one `aria2`'s, and this is the single place that says so. See
/// `TODO/performance.md`, T-032.
const fn wants_in_order(selector: crate::cli::PieceSelector) -> bool {
    matches!(
        selector,
        crate::cli::PieceSelector::Sequential | crate::cli::PieceSelector::InOrder
    )
}

/// What `--select-file` and `--exclude-file` mean for one source.
///
/// Two of their spellings need the number of files in the torrent, and every
/// source but a magnet has that before the session starts: `run` parses the
/// metainfo of a local `.torrent`, a fetched one and a Metalink's before any
/// plan is handed out. A magnet has no file list until its metadata resolves
/// over the network, so its selection is decided by the worker that adds it.
/// See `TODO/cli-surface.md`, T-185.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FileSelection {
    /// Settled before anything was added. `None` is every file.
    Decided(Option<Vec<usize>>),
    /// A magnet, and the flags cannot be resolved until it has a file count.
    AwaitingCount,
}

/// One source and what was resolved for it before the session started.
struct Plan {
    /// Position in the run, which is the order the queue hands plans out in.
    index: usize,
    source: String,
    /// The exact `.torrent` bytes, when this run resolved them itself rather
    /// than leaving it to the session. A Metalink names its torrent by URL and
    /// the URL was already fetched; handing the session the same URL would
    /// fetch it twice. See `TODO/cli-surface.md`, T-113.
    torrent_bytes: Option<Vec<u8>>,
    /// What the Metalink said, when the source was one.
    metalink: Option<Box<MetalinkPlan>>,
    specs: Vec<SourceSpec>,
    trackers: Option<Vec<String>>,
    /// Which files of this torrent to fetch, from `--select-file` and
    /// `--exclude-file`. See `TODO/cli-surface.md`, T-185.
    files: FileSelection,
    /// Whether the torrent carries a `files` list, when this run already read
    /// the metadata. It decides what `--out` names: the file, for a
    /// single-file torrent, or the directory the files go directly into, for a
    /// multi-file one. `None` for a magnet, for the same reason `file_count`
    /// is. A multi-file torrent holding one file is still multi-file, which is
    /// why this is not derived from the count. See `TODO/cli-surface.md`,
    /// T-226 and T-036.
    multi_file: Option<bool>,
    /// How many files the torrent has, when this run already read the
    /// metadata. `None` for a magnet, whose file list does not exist until it
    /// resolves. See `TODO/cli-surface.md`, T-116.
    file_count: Option<usize>,
    /// Files an earlier torrent in this run is proven to hold, computed from
    /// the metadata before anything starts. See `TODO/multi-source.md`, T-140.
    donations: Vec<Donation>,
}

/// Everything the Metalink said, carried to the end of the run so the
/// checksum can be checked against the payload it describes.
struct MetalinkPlan {
    version: &'static str,
    file_name: String,
    torrent_url: String,
    torrent_fallbacks: Vec<MirrorError>,
    mirrors_listed: usize,
    mirrors_registered: usize,
    mirrors_unsupported: Vec<String>,
    agreement: Agreement,
    /// The strongest checksum in the document that this can compute. `None`
    /// when the document had none, or only ones nothing here hashes.
    checksum: Option<Checksum>,
    /// The strongest checksum in the document, computable or not. Used only to
    /// say why nothing was checked.
    unusable_algorithm: Option<String>,
}

/// One file this torrent could read from an earlier torrent in the run.
///
/// Only the donor's position is fixed here. Whether it can actually be read
/// depends on that torrent having finished, which is known when this one
/// starts and not before.
#[derive(Debug, Clone)]
struct Donation {
    /// File index in the torrent that would read it.
    index: usize,
    /// Position of the donor in the run. Always lower than the receiver's:
    /// a torrent can only read what an earlier one has already written.
    donor: usize,
    /// File index in the donor.
    donor_index: usize,
    length: u64,
    pieces_compared: u32,
    bytes_proven: u64,
}

/// What a finished torrent can lend to the ones after it.
#[derive(Debug, Clone)]
struct DonorFiles {
    source: String,
    info_hash: String,
    /// Directory its payload landed in, subfolder included.
    root: std::path::PathBuf,
    /// One path per file index, relative to `root`, as planned. Empty until
    /// the torrent finishes, which is what says it has nothing to lend yet:
    /// a `file:` source over a half-written file serves bytes that are not
    /// there.
    disk_paths: Vec<String>,
}

/// Every torrent in the run that could donate, keyed by position.
type SharedDonors = Arc<std::sync::Mutex<std::collections::HashMap<usize, DonorFiles>>>;

/// Every file each torrent could take from an earlier one, proven from the
/// metadata.
///
/// Proof only. [`bit_cli_core::equivalence::Evidence::Length`] says two files
/// are the same size and nothing else, and reading a file on that basis is
/// exactly the silent corruption the equivalence module exists to avoid. A
/// piece-hash proof means the whole pieces inside both files have the same
/// SHA-1, which is the same evidence a torrent gives about its own bytes.
///
/// The earliest torrent that holds the file donates it. That is the one most
/// likely to have finished by the time a later one starts, and it makes the
/// choice a function of the command line rather than of the order things
/// happened to complete.
fn share_plan(metas: &[Option<Metainfo>]) -> Vec<Vec<Donation>> {
    let mut out: Vec<Vec<Donation>> = vec![Vec::new(); metas.len()];
    for (index, meta) in metas.iter().enumerate() {
        let Some(meta) = meta else { continue };
        let layout = meta.layout();
        let mut taken: HashSet<usize> = HashSet::new();
        for (donor, other) in metas[..index].iter().enumerate() {
            let Some(other) = other.as_ref() else {
                continue;
            };
            let other_layout = other.layout();
            for found in bit_cli_core::equivalence::matches(
                &layout,
                &meta.info().pieces,
                &other_layout,
                &other.info().pieces,
            ) {
                if !found.evidence.is_proof() || !taken.insert(found.index) {
                    continue;
                }
                out[index].push(Donation {
                    index: found.index,
                    donor,
                    donor_index: found.other_index,
                    length: found.length,
                    pieces_compared: found.pieces_compared,
                    bytes_proven: found.bytes_proven,
                });
            }
        }
    }
    out
}

/// Every source one torrent has, including the ones that turn up after it
/// starts.
///
/// A donated file is only a source once the torrent holding it has finished
/// writing, which above `-j 1` is partway through this run rather than before
/// it. Keeping the sources, the ledger they all record into and the report rows
/// in one place is what makes attaching one late a call rather than three
/// separate pieces of bookkeeping that can disagree. See
/// `TODO/multi-source.md`, T-143.
struct Attachments {
    sources: Vec<AttachedSource>,
    /// Where every block a source served is recorded. One per torrent, so a
    /// late source is judged on the same evidence as the rest. See
    /// `TODO/webseed.md`, T-179.
    ledger: Arc<bit_cli_core::webseed::ledger::BlockLedger>,
    /// Report rows for the files read off another torrent's disk, in the order
    /// they attached.
    shared: Vec<SharedFile>,
    /// Donations whose donor had not finished yet. Emptied as they attach.
    pending: Vec<Donation>,
    /// The next free source index. The ledger is keyed on it, so it only ever
    /// goes up: two sources sharing an index would convict each other.
    next_index: usize,
    options: swarm::AttachOptions,
    /// How many sources this torrent will have in total, which is what the
    /// request budget was divided by.
    total_sources: usize,
}

/// Announce one source on the event stream.
///
/// Late attachments say exactly what the ones present at the start said. The
/// event already carries everything a caller needs to tell them apart: a
/// donation's `origin` is `shared_file`, and the event's position in the
/// stream is when it happened.
async fn source_added(source: &AttachedSource, tx: &mpsc::Sender<Msg>) {
    let _ = tx
        .send(Msg::Event(
            "source_added",
            json!({
                "index": source.index,
                "url": source.url,
                "origin": source.origin,
                "scope": source.scope,
                "whole_pieces": source.whole_pieces,
            }),
        ))
        .await;
}

/// Attach any donation whose donor has finished since the last tick.
///
/// Called from the watch loop, so a torrent that started with no source at all
/// gets one the moment an earlier torrent in the same invocation writes the
/// file it is proven to hold. Costs one lock and one `stat` per pending
/// donation per tick, and nothing at all once the list is empty, which under
/// `-j 1` it always is. See `TODO/multi-source.md`, T-143.
async fn attach_pending(
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    layout: &Arc<Layout>,
    options: &Options,
    attachments: &mut Attachments,
    tx: &mpsc::Sender<Msg>,
) {
    if attachments.pending.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut attachments.pending);
    let (specs, shared, still_pending) = donated_sources(&pending, options, layout);
    attachments.pending = still_pending;
    if specs.is_empty() {
        return;
    }
    // Shaped by the same budget the sources at the start were shaped by. The
    // divisor counted this source when the run began, so its share was
    // reserved and nothing running has to give any of it back.
    let specs = apply_preference(
        apply_max_total(&specs, options.max_total, attachments.total_sources),
        options.prefer,
    );
    for (spec, file) in specs.iter().zip(shared) {
        let index = attachments.next_index;
        match swarm::attach_late(
            engine,
            handle,
            layout,
            spec,
            index,
            &attachments.options,
            &attachments.ledger,
        )
        .await
        {
            Ok(source) => {
                attachments.next_index += 1;
                let _ = tx
                    .send(Msg::Warn(format!(
                        "file {} ({}) is proven to be the file {} holds at index {}, reading it from {} rather than fetching it",
                        file.index, file.path, file.from_source, file.from_index, file.from_path
                    )))
                    .await;
                source_added(&source, tx).await;
                attachments.sources.push(source);
                attachments.shared.push(file);
            }
            // A donation that cannot be attached is one file this torrent
            // fetches instead of reading, not a failed run: everything it
            // covers is still reachable from the swarm and the mirrors. The
            // index is not consumed, so the ledger keeps its one to one map.
            Err(error) => {
                let _ = tx
                    .send(Msg::Warn(format!(
                        "file {} ({}) could not be read from {}, so it is fetched instead: {error}",
                        file.index, file.path, file.from_path
                    )))
                    .await;
            }
        }
    }
}

/// Fetch one source to completion.
async fn one(
    engine: &Engine,
    plan: Plan,
    options: Options,
    tx: &mpsc::Sender<Msg>,
) -> TorrentReport {
    match one_inner(engine, &plan, &options, tx).await {
        Ok(report) => report,
        Err(error) => {
            let _ = tx
                .send(Msg::Event(
                    "error",
                    serde_json::to_value(error.report()).unwrap_or_default(),
                ))
                .await;
            TorrentReport {
                source: plan.source,
                info_hash: String::new(),
                name: String::new(),
                stopped: Stopped::Failed,
                finished: false,
                total: Size(0),
                downloaded: Size(0),
                uploaded: Size(0),
                from_web_seeds: Size(0),
                from_peers: Size(0),
                from_resume: Size(0),
                elapsed_ms: 0,
                elapsed_human: "0s".into(),
                mean_rate: Size(0),
                mean_rate_human: format_rate(0),
                peers_seen: 0,
                redials: Vec::new(),
                sources: Vec::new(),
                output_directory: options.directory.display().to_string(),
                renamed: Vec::new(),
                shared: Vec::new(),
                announced: Vec::new(),
                partial: Vec::new(),
                metalink: None,
                attribution: None,
                verified_files: Vec::new(),
                code: error.code(),
                error: Some(error.to_string()),
                phase: error
                    .context()
                    .get("phase")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            }
        }
    }
}

async fn one_inner(
    engine: &Engine,
    plan: &Plan,
    options: &Options,
    tx: &mpsc::Sender<Msg>,
) -> Result<TorrentReport> {
    let mut add = AddOptions {
        overwrite: options.overwrite,
        only_files: None,
        // Resolved from the metadata this run already read where there is any,
        // so an index past the end fails before the session starts rather than
        // renaming nothing. A magnet has none yet and is checked below, once
        // its file list exists. See `TODO/cli-surface.md`, T-116.
        index_out: crate::selection::index_out(&options.index_out, plan.file_count)?,
        trackers: plan.trackers.clone(),
        disable_trackers: plan.trackers.as_ref().is_some_and(Vec::is_empty),
        initial_peers: options.peers.clone(),
        download_rate: options.torrent_download_rate,
        upload_rate: options.torrent_upload_rate,
        ..Default::default()
    };
    let mut torrent_bytes = plan.torrent_bytes.clone();
    // A magnet's shape, once its metadata resolves. `plan.multi_file` already
    // has it for every other source kind. T-226.
    let mut resolved_multi_file: Option<bool> = None;
    match &plan.files {
        FileSelection::Decided(files) => add.only_files = files.clone(),
        // A magnet whose selection needs a file count. Read the metadata
        // first, rather than adding the torrent and narrowing it afterwards:
        // the initial check creates and opens every file it was not told to
        // skip, so a selection applied after the add has already created the
        // files it excludes. Resolving keeps the `.torrent` bytes it built, so
        // the add below is the same one metadata resolution, not a second.
        // See `TODO/cli-surface.md`, T-185.
        FileSelection::AwaitingCount => {
            // Bounded by `--init-timeout`, which is the budget for getting a
            // torrent ready before anything is fetched. `engine.add` would do
            // this same resolution with no bound at all, so a magnet that
            // never resolves used to hang the run rather than report why.
            let resolved = tokio::time::timeout(
                options.init_timeout,
                engine.resolve_with(&plan.source, &add),
            )
            .await
            .map_err(|_| {
                Error::timeout(format!(
                    "{}: the metadata did not resolve in {}ms, so --exclude-file and an open-ended --select-file have no file count to work from",
                    plan.source,
                    options.init_timeout.as_millis()
                ))
                .with("phase", "resolving_metadata")
                .with(
                    "waited_ms",
                    options.init_timeout.as_millis().min(u128::from(u64::MAX)) as u64,
                )
            })??;
            let count = resolved.layout.files.len();
            add.only_files = crate::selection::resolve(
                &options.select_file,
                &options.exclude_file,
                Some(count),
            )?;
            // Re-parsed now that the count exists, so `-O 9=x` against a
            // five-file magnet is the same usage error it is against a
            // `.torrent`. T-116.
            add.index_out = crate::selection::index_out(&options.index_out, Some(count))?;
            resolved_multi_file = Some(resolved.layout.multi_file);
            torrent_bytes = Some(resolved.torrent_bytes);
        }
    }
    // `--out` is the payload's destination, and what it replaces depends on
    // the torrent's shape. A multi-file torrent's name is a directory, so the
    // path becomes that directory and `subfolder: false` in `add_inner` is
    // what stops the name being appended to it. A single-file torrent's name
    // is the file, so the path's parent becomes the directory and the leaf
    // becomes an `--index-out` rename of file 0, which is machinery that
    // already sanitises, truncates and disambiguates a caller's path. Doing it
    // any other way would let `-o ../../x` out of the output directory.
    // See `TODO/cli-surface.md`, T-226.
    // Where this torrent's payload actually lands, which is what the report
    // has to name. `options.directory` is the run's and stops being this
    // torrent's the moment `--out` is given.
    let mut payload_directory = options.directory.clone();
    if let Some(out) = &options.out {
        let multi_file = plan
            .multi_file
            .or_else(|| resolved_multi_file.as_ref().copied())
            .ok_or_else(|| {
                Error::usage(format!(
                    "{}: --out needs to know whether the torrent is single-file, and the metadata did not say",
                    plan.source
                ))
            })?;
        match multi_file {
            true => {
                payload_directory = out.clone();
                add.output_folder = Some(out.to_string_lossy().into_owned());
            }
            false => {
                let leaf = out.file_name().ok_or_else(|| {
                    Error::usage(format!("--out `{}` names no file", out.display()))
                        .with("value", out.display().to_string())
                })?;
                payload_directory = out
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .to_path_buf();
                add.output_folder = Some(payload_directory.to_string_lossy().into_owned());
                add.index_out
                    .insert(0, leaf.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    let only_files = add.only_files.clone();
    // Bounded by `--init-timeout`, the same budget and the same error as the
    // selection branch fifty lines above. `engine.add` resolves a magnet's
    // metadata itself and does it with no bound, and an ordinary invocation
    // with no file selection takes this branch, so this was the one that
    // hung: a magnet against a peer that could not serve it ran for ten
    // minutes and was killed by the harness rather than by `bit-cli`. The
    // `wait_until_initialized_within` below would have applied the bound and
    // is never reached. See `TODO/cli-surface.md`, T-196.
    let added = tokio::time::timeout(options.init_timeout, async {
        match torrent_bytes {
            // A Metalink's torrent was fetched while the plans were being
            // built, so the session gets those exact bytes rather than the URL
            // again. A magnet's were built when its metadata resolved, for the
            // same reason.
            Some(bytes) => engine.add_bytes(&plan.source, bytes, &add).await,
            None => engine.add(&plan.source, &add).await,
        }
    })
    .await
    .map_err(|_| {
        Error::timeout(format!(
            "{}: the metadata did not resolve in {}ms",
            plan.source,
            options.init_timeout.as_millis()
        ))
        .with("phase", "resolving_metadata")
        .with(
            "waited_ms",
            options.init_timeout.as_millis().min(u128::from(u64::MAX)) as u64,
        )
    })?;
    let handle = added?;
    let snapshot = engine.snapshot(&handle);
    let _ = tx
        .send(Msg::Event(
            "torrent_added",
            json!({
                "source": plan.source,
                "info_hash": snapshot.info_hash,
                "name": snapshot.name,
            }),
        ))
        .await;

    engine
        .wait_until_initialized_within(&handle, options.init_timeout)
        .await?;
    // A rename is not an error, but a caller who is not told about one cannot
    // find the file it asked for, so it goes to stderr as well as into the
    // report.
    if let Some(planned) = engine.path_plan(&handle)
        && !planned.is_clean()
    {
        let reasons: Vec<&str> = planned
            .reasons()
            .iter()
            .map(|reason| reason.description())
            .collect();
        let _ = tx
            .send(Msg::Warn(format!(
                "{} of {} paths were changed to be writable here ({}); see `renamed` in --json",
                planned.renames.len(),
                planned.disk_paths.len(),
                reasons.join(", ")
            )))
            .await;
    }
    let layout = Arc::new(engine.layout(&handle).ok_or_else(|| {
        Error::source_resolution(format!("{}: the torrent has no metadata", plan.source))
    })?);
    let _ = tx
        .send(Msg::Event(
            "metadata_resolved",
            json!({
                "info_hash": handle.info_hash().as_string(),
                "name": layout.name,
                "files": layout.files.len(),
                "piece_count": layout.piece_count(),
                "piece_length": layout.piece_length,
                "total_bytes": layout.total_length,
            }),
        ))
        .await;

    // A selection whose boundary pieces straddle into a file it did not choose
    // writes into that file, because a piece cannot be verified without every
    // byte of it. Said here, before anything is fetched, so a caller who is
    // about to see files it did not ask for knows why they are there. See
    // `TODO/disk-io.md`, T-184.
    for partial in partial_files(&layout, only_files.as_ref()) {
        let _ = tx
            .send(Msg::Warn(format!(
                "{} was not selected, and a piece it shares with a selected file writes {} into it, leaving a {} file where the torrent says {}",
                partial.path,
                format_size(partial.bytes.0),
                format_size(partial.on_disk.0),
                format_size(partial.length.0)
            )))
            .await;
    }

    // What the hash check found already on disk, read once the check has
    // finished and before anything is fetched.
    //
    // `progress_bytes` is everything the torrent has, not everything this run
    // fetched, so charging `progress_bytes - served` to peers charges them for
    // a resumed download's existing bytes as well. A run that resumed 45 MiB
    // of a 64 MiB file with no peer in the swarm reported 45 MiB from peers.
    // See `TODO/multi-source.md`, T-139.
    let resumed = engine.snapshot(&handle).progress_bytes;

    if options.hash_check_only {
        let snapshot = engine.snapshot(&handle);
        let mut report = finish(
            plan,
            &payload_directory,
            &snapshot,
            &[],
            Stopped::Completed,
            Duration::ZERO,
            Vec::new(),
            resumed,
            renames(engine, &handle),
        );
        // The same call the normal exit makes. This return used to come before
        // the block that built it, so a Metalink run with `--hash-check-only`
        // reported nothing about the document at all: not the mirror count, not
        // the torrent it resolved, and not the size comparison, which is
        // computed before this point and was then thrown away.
        //
        // A payload that is complete on disk gets its checksum checked here
        // too, which is the strongest thing this flag can report: the hash
        // check proved the bytes against the torrent, and this proves the same
        // bytes against the Metalink. `check_metalink` decides that from
        // `report.finished` and needs no branch of its own.
        // See `TODO/cli-surface.md`, T-155.
        report.verified_files =
            verify_on_complete(engine, &handle, options, &report, only_files.as_ref());
        apply_metalink(&mut report, plan, engine, &handle, options, tx).await;
        return Ok(report);
    }

    // `--piece-selector sequential` holds the session's priority window at the
    // earliest piece still missing, and it is registered **here**, before any
    // source is attached, rather than in the watch loop below.
    //
    // The reason is a race that the measurement found rather than the design
    // predicted. `librqbit`'s natural order yields the last piece of a file
    // second, so if anything can ask for a piece before the window exists, the
    // tail arrives early and the order has a descent in it. Registering before
    // the sources means nothing can: under `--web-seed-only` the bridges are
    // the only peers, and they do not exist yet. Against a real swarm it is
    // best effort, because a peer dialled during the hash check may already
    // have been handed one. See `TODO/performance.md`, T-032.
    let mut ordering = match options.in_order {
        false => None,
        true => {
            let mut driver =
                bit_cli_core::piece_order::InOrder::new(handle.clone(), layout.clone());
            if let Some(have) = engine.have_pieces(&handle) {
                // A failure here loses the ordering, not the download: the
                // window is a hint to a picker that works without it.
                if driver.advance(&have).await.is_err() {
                    let _ = tx
                        .send(Msg::Warn(
                            "the session refused a piece priority window, so pieces will arrive in its own order".to_string(),
                        ))
                        .await;
                }
            }
            Some(driver)
        }
    };

    // Files an earlier torrent in this run has already written, which this one
    // is proven to hold too. These are sources like any other: scoped to one
    // file, checked per piece on the way in, and reported with their own
    // origin. See `TODO/multi-source.md`, T-140.
    //
    // `pending` is the donations whose donor is still running, which above
    // `-j 1` is all of them at this point. They attach from the watch loop as
    // their donors finish. See `TODO/multi-source.md`, T-143.
    let (donated, shared, pending) = donated_sources(&plan.donations, options, &layout);
    for file in &shared {
        let _ = tx
            .send(Msg::Warn(format!(
                "file {} ({}) is proven to be the file {} holds at index {}, reading it from {} rather than fetching it",
                file.index, file.path, file.from_source, file.from_index, file.from_path
            )))
            .await;
    }
    let mut declared = plan.specs.clone();
    declared.extend(donated);

    // The whole-run concurrency cap is shared out across the declared sources,
    // so `--web-seed-max-total 8` with four mirrors means two requests each
    // rather than eight each. A pending donation counts against the divisor
    // without being in the list, so its share is reserved rather than taken
    // back off a running bridge when it arrives.
    let total_sources = declared.len() + pending.len();
    let specs = apply_preference(
        apply_max_total(&declared, options.max_total, total_sources),
        options.prefer,
    );
    let attach_options = swarm::AttachOptions {
        require: options.require,
        peers_available: !options.web_seed_only,
        cache_windows: cache_windows(&specs),
        trace: options.trace_http,
        verify: options.verify,
    };
    let (sources, _set, ledger) =
        swarm::attach_sources_tracked(engine, &handle, &layout, &specs, &attach_options).await?;
    for source in &sources {
        source_added(source, tx).await;
    }

    let mut attachments = Attachments {
        next_index: specs.len(),
        sources,
        ledger,
        shared,
        pending,
        options: attach_options,
        total_sources,
    };

    let mut announced: Vec<SentAnnounce> = Vec::new();
    let outcome = watch(
        engine,
        &handle,
        &layout,
        &mut attachments,
        plan,
        options,
        tx,
        &mut announced,
        ordering.take(),
    )
    .await;
    let Attachments {
        sources,
        ledger,
        shared,
        ..
    } = attachments;
    for source in &sources {
        source.stop();
    }
    let (stopped, elapsed, redials) = outcome;

    // `stopped` last, whatever ended the run. A tracker that is not told keeps
    // handing this address out until the record expires, which on a public
    // tracker is the next half hour.
    if let Some(sent) = announce_event(
        engine,
        &handle,
        plan,
        options,
        bit_cli_core::tracker::Event::Stopped,
        elapsed,
    )
    .await
    {
        announced.push(sent);
    }
    let snapshot = engine.snapshot(&handle);
    let mut report = finish(
        plan,
        &payload_directory,
        &snapshot,
        &sources,
        stopped,
        elapsed,
        redials,
        resumed,
        renames(engine, &handle),
    );
    report.shared = shared;
    report.announced = announced;
    report.partial = partial_files(&layout, only_files.as_ref());
    // Set here rather than passed into `finish`, which already takes nine
    // arguments and does not otherwise know the ledger exists.
    report.attribution = (!sources.is_empty()).then(|| ledger.stats());
    // Before the metalink check, so a Metalink run's own checksum and this
    // run's per-file digests are both taken from the same bytes on disk with
    // nothing between them.
    report.verified_files =
        verify_on_complete(engine, &handle, options, &report, only_files.as_ref());
    apply_metalink(&mut report, plan, engine, &handle, options, tx).await;
    // A finished torrent can lend its files to the ones after it. An
    // unfinished one cannot: its files are on disk but not all of their bytes
    // are.
    if report.finished {
        publish_donor(engine, &handle, plan, options);
    }

    let _ = tx
        .send(Msg::Event(
            "torrent_completed",
            json!({
                "info_hash": report.info_hash,
                "name": report.name,
                "stopped": report.stopped,
                "finished": report.finished,
                "downloaded_bytes": report.downloaded.0,
                "from_web_seeds": report.from_web_seeds.0,
                "from_peers": report.from_peers.0,
                "from_resume": report.from_resume.0,
                "elapsed_ms": report.elapsed_ms,
            }),
        ))
        .await;
    Ok(report)
}

/// Watch one torrent until a stop condition fires.
///
/// Three things wake this loop, and the report interval is only one of them.
/// The other two are the events that end a run: the torrent completing, and
/// the earliest deadline the caller set. Waking only on the tick would make
/// every run as long as the next multiple of `--report-interval`, which
/// defaults to a second, so a download that finished in 1.1 s would take 2 s
/// and a `--timeout 30s` would fire at 31. See `TODO/performance.md`, T-030.
#[allow(clippy::too_many_arguments)]
async fn watch(
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    layout: &Arc<Layout>,
    // Every source, the ledger they record into, and the donations still
    // waiting for their donor. Taken by reference rather than by value because
    // a donation attaches from inside this loop. See
    // `TODO/multi-source.md`, T-143 and `TODO/webseed.md`, T-179.
    attachments: &mut Attachments,
    plan: &Plan,
    options: &Options,
    tx: &mpsc::Sender<Msg>,
    announced: &mut Vec<SentAnnounce>,
    // The piece priority window, already registered by the caller so that
    // nothing could ask for a piece before it existed.
    mut ordering: Option<bit_cli_core::piece_order::InOrder>,
) -> (Stopped, Duration, Vec<Redial>) {
    let lengths: Vec<u64> = layout.files.iter().map(|f| f.length).collect();
    let mut progress = Progress::new(layout.piece_count(), lengths);
    let mut redials: Vec<Redial> = Vec::new();
    // Measured from the last re-dial rather than from the last byte, so a
    // stall that outlasts the interval re-dials once per interval instead of
    // once per report tick. `--stop-timeout` keeps measuring from the last
    // byte, which is what lets a run both re-dial and still give up.
    let mut last_redial = std::time::Instant::now();
    let mut ticker = tokio::time::interval(options.report_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut reported_failures = HashSet::new();
    let mut reported_cooldowns: HashSet<(usize, u64)> = HashSet::new();
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(interrupt);

    // Completion resolves once and must not be polled again, so it is guarded
    // rather than recreated: a run that goes on seeding after completing is
    // still driven by the tick.
    let completion = engine.wait_until_completed(handle);
    tokio::pin!(completion);
    let mut completed = false;

    // The soonest moment a deadline could fire. `should_stop` decides whether
    // it actually does; this only makes sure the loop is awake to ask, and it
    // measures from here because `should_stop` measures from `progress`, which
    // starts on the line above.
    //
    // With no deadline set the sleep is parked a day out rather than made
    // optional, because an optional future in a `select!` needs either a boxed
    // `Option` or a second arm. A run still going after a day wakes once more
    // than it needed to and nothing else changes.
    const NO_DEADLINE: Duration = Duration::from_secs(86_400);
    let limit = [options.stop.timeout, options.stop.stop_after]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(NO_DEADLINE);
    let deadline = tokio::time::sleep_until(tokio::time::Instant::now() + limit);
    tokio::pin!(deadline);
    let mut deadline_fired = false;

    // The priority window gets a ticker of its own rather than riding the
    // report tick, because how often a caller wants progress printed is not a
    // statement about what order pieces should arrive in: `--report-interval
    // 10s` must not mean a window that moves twice a minute. Fifty
    // milliseconds is well inside the 32 MiB of lookahead the window carries,
    // even on loopback. See `TODO/performance.md`, T-032.
    let mut order_ticker = tokio::time::interval(Duration::from_millis(50));
    order_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = &mut interrupt => return (Stopped::Interrupted, progress.elapsed(), redials),
            _ = ticker.tick() => {}
            _ = order_ticker.tick(), if ordering.is_some() => {
                // This arm does not fall through to the body below, which is a
                // progress report: it fires twenty times a second and a run
                // does not want twenty progress events a second.
                //
                // Losing the ordering loses the ordering and not the download.
                // The window is a hint to a picker that works without it, so a
                // failure here drops back to the natural order rather than
                // failing a run over a preference.
                if let Some(driver) = ordering.as_mut() {
                    let keep = match engine.have_pieces(handle) {
                        // Pointing at nothing means nothing is missing, so the
                        // window has no more work and the stream is released.
                        Some(have) => matches!(driver.advance(&have).await, Ok(Some(_))),
                        // No bitfield yet is not a reason to give up: the
                        // torrent may still be hash-checking, and a tick that
                        // lands in that window used to disable the ordering
                        // for the whole run.
                        None => true,
                    };
                    if !keep {
                        ordering = None;
                    }
                }
                continue;
            }
            _ = &mut completion, if !completed => {
                completed = true;
                // Now, not when the run ends: a run that keeps seeding after
                // completing would otherwise tell the tracker minutes late,
                // and the seeder count is what a tracker uses this for.
                if let Some(sent) = announce_event(
                    engine,
                    handle,
                    plan,
                    options,
                    bit_cli_core::tracker::Event::Completed,
                    progress.elapsed(),
                )
                .await
                {
                    announced.push(sent);
                }
            }
            _ = &mut deadline, if !deadline_fired => deadline_fired = true,
        }

        let snapshot = engine.snapshot(handle);
        let have = engine.have_pieces(handle);
        let file_progress = handle.stats().file_progress;
        let tick = progress.observe(&snapshot, have.as_deref(), &file_progress);

        for piece in tick.verified_pieces {
            let length = layout.piece_size(piece);
            // Queued and not waited for. A hook is a notification about the
            // download and is never allowed to decide how fast it goes; what
            // does not fit the queue is counted. See `docs/hooks.md` and
            // `TODO/cli-surface.md`, T-115.
            if let Some(hook) = &options.piece_hook {
                hook.fire(crate::hooks::piece_vars(
                    &snapshot.info_hash,
                    &snapshot.name,
                    &options.directory.display().to_string(),
                    piece,
                    // Zero when the layout cannot size the piece, which is a
                    // piece index it does not have. The hook still fires,
                    // because the index is the fact it was asked about.
                    length.unwrap_or(0),
                ));
            }
            let _ = tx
                .send(Msg::Event(
                    "piece_verified",
                    json!({ "piece": piece, "length": length }),
                ))
                .await;
        }
        for file in tick.completed_files {
            let _ = tx
                .send(Msg::Event(
                    "file_completed",
                    json!({
                        "file": file,
                        "path": layout.file(file).map(|f| f.display_path()),
                        "length": layout.file(file).map(|f| f.length),
                    }),
                ))
                .await;
        }

        // A donation whose donor has finished since the last tick becomes a
        // source here, before the accounting below reads the list, so the tick
        // that attaches one already counts it. See `TODO/multi-source.md`,
        // T-143.
        attach_pending(engine, handle, layout, options, attachments, tx).await;
        let Attachments {
            sources, ledger, ..
        } = &*attachments;

        // Attribution runs before the failure reporting below, so a source
        // convicted on this tick is retired and reported as failed on this
        // tick rather than the next. The correct bytes come off the disk, from
        // a piece the session has already hash-checked, so nothing is fetched
        // twice; and only a block two sources disagreed about is ever read,
        // which in a healthy run is none of them. See `TODO/webseed.md`,
        // T-179.
        if let Some(have) = have.as_deref() {
            let convicted = swarm::resolve_convictions(ledger, sources, have, |offset, length| {
                read_payload(engine, handle, options, layout, offset, length)
            });
            // Warned here and reported as an event below, by the
            // `source_failed` the retirement produces. A second event carrying
            // a subset of the same `SourceReport` would be two names for one
            // fact, and `sources[].convictions` already carries the piece, the
            // offset and both hashes.
            for conviction in convicted {
                let url = sources
                    .iter()
                    .find(|s| s.index == conviction.source)
                    .map(|s| s.url.clone())
                    .unwrap_or_default();
                let _ = tx
                    .send(Msg::Warn(format!(
                        "web seed {url} {conviction}, so it is retired"
                    )))
                    .await;
            }
        }

        for source in sources {
            if source.state() == bit_cli_core::webseed::BridgeState::Failed
                && reported_failures.insert(source.index)
            {
                let report = source.report();
                let _ = tx
                    .send(Msg::Warn(format!(
                        "web seed {} is unusable: {}",
                        report.url,
                        report.error.as_deref().unwrap_or("no reason given")
                    )))
                    .await;
                let _ = tx
                    .send(Msg::Event(
                        "source_failed",
                        serde_json::to_value(&report).unwrap_or_default(),
                    ))
                    .await;
            }
            // Keyed by how many times the source has cooled down, not by its
            // index, so a mirror that goes out, comes back, and goes out again
            // is reported each time. A run waiting on a sleeping source has to
            // be told, or the wait looks like a hang. See
            // `TODO/multi-source.md`, T-137.
            if source.state() == bit_cli_core::webseed::BridgeState::Cooling {
                let report = source.report();
                if reported_cooldowns.insert((source.index, report.cooldowns)) {
                    let _ = tx
                        .send(Msg::Warn(format!(
                            "web seed {} is cooling down for {}: {}",
                            report.url,
                            bit_cli_core::units::format_duration(Duration::from_millis(
                                report.cooldown_remaining_ms.unwrap_or(0)
                            )),
                            report.error.as_deref().unwrap_or("no reason given")
                        )))
                        .await;
                    let _ = tx
                        .send(Msg::Event(
                            "source_cooling",
                            serde_json::to_value(&report).unwrap_or_default(),
                        ))
                        .await;
                }
            }
        }

        let served: u64 = sources.iter().map(AttachedSource::served_bytes).sum();
        let _ = tx
            .send(Msg::Event(
                "progress",
                json!({
                    "info_hash": snapshot.info_hash,
                    "progress_bytes": snapshot.progress_bytes,
                    "total_bytes": snapshot.total_bytes,
                    "percent": format!("{:.2}", snapshot.fraction() * 100.0),
                    "download_rate": snapshot.download_rate,
                    "upload_rate": snapshot.upload_rate,
                    "peers": snapshot.peers,
                    "from_web_seeds": served,
                    "eta_ms": snapshot.eta_ms,
                    "eta_confidence": snapshot.eta_confidence,
                    // What the process costs right now, so a long run reads a slope out
                    // of the event stream rather than sampling the process from outside.
                    // See `TODO/memory.md`, T-040.
                    "process": bit_cli_core::sysinfo::Process::sample(),
                }),
            ))
            .await;
        let _ = tx
            .send(Msg::Progress(swarm::progress_line(&snapshot, sources)))
            .await;

        // A run with no peers and every HTTP source dead cannot finish, and
        // waiting out the deadline to say so wastes the caller's time.
        if !sources.is_empty()
            && options.web_seed_only
            && sources
                .iter()
                .all(|s| s.state() == bit_cli_core::webseed::BridgeState::Failed)
        {
            return (Stopped::Failed, progress.elapsed(), redials);
        }

        let seeding = options.stop.seed_ratio.is_some() || options.stop.seed_time.is_some();
        if let Some(reason) = progress.should_stop(&snapshot, &options.stop, seeding) {
            return (reason, progress.elapsed(), redials);
        }

        // Checked after the stop conditions, so a run that was going to give
        // up this tick gives up rather than re-dialling on its way out.
        if let Some(interval) = options.redial_after
            && !snapshot.finished
            && (redials.len() as u32) < options.max_redials
            && progress.stalled_for() >= interval
            && last_redial.elapsed() >= interval
        {
            let attempt = redials.len() as u32 + 1;
            let stalled = progress.stalled_for();
            let error = engine.redial(handle).await.err().map(|e| e.to_string());
            last_redial = std::time::Instant::now();
            let redial = Redial {
                attempt,
                at_ms: progress.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                stalled_ms: stalled.as_millis().min(u128::from(u64::MAX)) as u64,
                peers_dropped: snapshot.peers.live,
                error: error.clone(),
            };
            let _ = tx
                .send(Msg::Event(
                    "peer_redial",
                    serde_json::to_value(&redial).unwrap_or_default(),
                ))
                .await;
            if let Some(reason) = &error {
                let _ = tx
                    .send(Msg::Warn(format!("re-dial {attempt} failed: {reason}")))
                    .await;
            }
            redials.push(redial);
        }
    }
}

/// Check the payload against the Metalink's own checksum, and say which of the
/// two documents is wrong when they disagree.
///
/// The order of the guards is the point. The check runs only on a payload the
/// session has finished and hash-checked against the torrent's own piece
/// hashes, so a digest that then disagrees is evidence about the Metalink and
/// not about the bytes. Every guard that stops the check writes a
/// `not_checked` reason, because a checksum that was not computed is not a
/// checksum that passed. See `TODO/cli-surface.md`, T-113.
/// Re-read the finished payload and hash every file, for `--verify-on-complete`.
///
/// **Redundant by construction, which is the point.** Every byte has already
/// been checked against the torrent's own piece hashes twice: once at the
/// source under `--web-seed-verify piece`, and once by the session before it
/// counted the piece. This reads the files back off the disk afterwards and
/// reports a digest a caller can compare against one published somewhere else.
/// It is the check that does not trust the thing that wrote the bytes.
///
/// Nothing here changes the exit code. The digests are facts about the payload
/// and there is nothing to compare them against inside this run; a caller that
/// has something to compare them against is the one that can decide.
/// A file that cannot be read carries its error rather than being left out, so
/// a caller counting rows against the torrent's file list is never short one.
///
/// Only a **finished** torrent is hashed. Hashing a partial payload produces
/// digests of files that are not the files, which is a wrong answer rather than
/// a missing one. See `docs/integrity.md` and `TODO/multi-source.md`, T-136.
fn verify_on_complete(
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    options: &Options,
    report: &TorrentReport,
    only_files: Option<&Vec<usize>>,
) -> Vec<VerifiedFile> {
    if !options.verify_on_complete || !report.finished {
        return Vec::new();
    }
    let Some(layout) = engine.layout(handle) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (index, file) in layout.files.iter().enumerate() {
        // A file the run was told not to fetch is not a file this run wrote,
        // so hashing it would report a digest of whatever was there before.
        if only_files.is_some_and(|only| !only.contains(&index)) {
            continue;
        }
        let Some(path) = payload_path(engine, handle, options, index) else {
            continue;
        };
        let mut row = VerifiedFile {
            index,
            torrent_path: file.display_path(),
            disk_path: path.display().to_string(),
            algorithm: "sha256".to_string(),
            hex: String::new(),
            bytes: 0,
            length: file.length,
            error: None,
        };
        match bit_cli_core::digest::hash_file(&path, "sha256") {
            Ok(digest) => {
                row.hex = digest.hex;
                row.bytes = digest.bytes;
            }
            Err(error) => row.error = Some(error.to_string()),
        }
        out.push(row);
    }
    out
}

/// Fold the Metalink's own findings into a torrent's report, and say so on the
/// event stream.
///
/// Called at both of `one_inner`'s exits. It was inline at the normal one, and
/// `--hash-check-only` returns before it, which is [T-155]. Everything it needs
/// is on the report it is handed: `check_metalink` reads `finished` to decide
/// whether there is a complete payload to hash, so a run that checked what was
/// on disk and a run that fetched it take the same path through this.
///
/// [T-155]: `TODO/cli-surface.md`
async fn apply_metalink(
    report: &mut TorrentReport,
    plan: &Plan,
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    options: &Options,
    tx: &mpsc::Sender<Msg>,
) {
    let Some(metalink) = &plan.metalink else {
        return;
    };
    let (metalink_report, code) = check_metalink(metalink, engine, handle, options, report);
    if let Some(checksum) = &metalink_report.checksum {
        // Serialised from the report's own struct rather than rebuilt
        // here, so a field the report omits is omitted from the event too.
        // Rebuilding it with `json!` put `"not_checked": null` in every
        // successful run, which documents a field as always-null and tells
        // a reader nothing.
        let mut payload = serde_json::to_value(checksum).unwrap_or_default();
        if let Some(fields) = payload.as_object_mut() {
            fields.insert("info_hash".to_string(), json!(report.info_hash));
        }
        let _ = tx.send(Msg::Event("metalink_checked", payload)).await;
        if checksum.matched == Some(false) {
            let _ = tx
                .send(Msg::Warn(format!(
                    "the metalink's {} checksum does not match the payload: it says {}, the bytes hash to {}. The payload passed the torrent's own piece hashes, so the metalink is the document that disagrees.",
                    checksum.algorithm,
                    checksum.expected,
                    checksum.actual.as_deref().unwrap_or("nothing"),
                )))
                .await;
        }
    }
    report.metalink = Some(metalink_report);
    // A checksum that disagrees is the failure this feature exists to
    // find, so it decides the torrent's code unless something worse
    // already had.
    if let Some(code) = code
        && report.code == ExitCode::Success
    {
        report.code = code;
    }
}

fn check_metalink(
    metalink: &MetalinkPlan,
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    options: &Options,
    report: &TorrentReport,
) -> (MetalinkReport, Option<ExitCode>) {
    let agreement = MetalinkAgreement {
        file_index: metalink.agreement.file_index,
        matched_by: metalink.agreement.matched_by,
        metalink_size: metalink.agreement.metalink_size,
        torrent_size: metalink.agreement.torrent_size,
        size_agrees: metalink.agreement.size_agrees,
    };
    let mut out = MetalinkReport {
        version: metalink.version,
        file: metalink.file_name.clone(),
        torrent_url: metalink.torrent_url.clone(),
        torrent_fallbacks: metalink.torrent_fallbacks.clone(),
        mirrors_listed: metalink.mirrors_listed,
        mirrors_registered: metalink.mirrors_registered,
        mirrors_unsupported: metalink.mirrors_unsupported.clone(),
        agreement,
        checksum: None,
    };
    // Two documents that declare different lengths describe different files,
    // and that is decided before anything is hashed.
    let code = metalink
        .agreement
        .disagrees()
        .then_some(ExitCode::HashMismatch);

    let Some(checksum) = &metalink.checksum else {
        if let Some(algorithm) = &metalink.unusable_algorithm {
            out.checksum = Some(MetalinkChecksum {
                algorithm: algorithm.clone(),
                expected: String::new(),
                actual: None,
                matched: None,
                bytes_hashed: None,
                path: None,
                not_checked: Some(format!("this cannot compute {algorithm}")),
            });
        }
        return (out, code);
    };
    let mut result = MetalinkChecksum {
        algorithm: checksum.algorithm.clone(),
        expected: checksum.value.clone(),
        actual: None,
        matched: None,
        bytes_hashed: None,
        path: None,
        not_checked: None,
    };

    let stop = |result: &mut MetalinkChecksum, why: String| {
        result.not_checked = Some(why);
    };
    if !report.finished {
        stop(
            &mut result,
            "the download did not finish, so there is nothing complete to hash".to_string(),
        );
    } else if let Some(index) = metalink.agreement.file_index {
        match payload_path(engine, handle, options, index) {
            None => stop(
                &mut result,
                "the torrent's paths were not planned, so the file on disk cannot be named"
                    .to_string(),
            ),
            Some(path) => {
                result.path = Some(path.display().to_string());
                match checksum.verify_file(&path) {
                    Ok(verified) => {
                        result.actual = Some(verified.actual);
                        result.matched = Some(verified.matched);
                        result.bytes_hashed = Some(verified.bytes_hashed);
                    }
                    Err(error) => stop(&mut result, error.to_string()),
                }
            }
        }
    } else {
        stop(
            &mut result,
            format!(
                "the metalink's checksum could not be attributed to a file in the torrent ({})",
                metalink.agreement.matched_by
            ),
        );
    }

    let mismatch = (result.matched == Some(false)).then_some(ExitCode::HashMismatch);
    out.checksum = Some(result);
    (out, code.or(mismatch))
}

/// Where one file of a finished torrent actually sits on disk.
///
/// The torrent's own path is not necessarily the path on disk: a name the
/// filesystem refuses, or one that would leave the output directory, is
/// rewritten before anything is opened. Hashing the torrent's path rather than
/// the planned one would hash a file that is not there.
fn payload_path(
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    options: &Options,
    index: usize,
) -> Option<std::path::PathBuf> {
    let layout = engine.layout(handle)?;
    let planned = engine.path_plan(handle)?;
    let relative = planned.disk_paths.get(index)?;
    let root = bit_cli_core::storage::payload_root(&options.directory, &layout);
    Some(root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

/// Files the selection did not choose that a boundary piece writes into.
///
/// Reported rather than prevented, and the reason is that preventing it is not
/// possible: a piece is verified against its whole hash, so the bytes of an
/// unselected file that share a piece with a selected one have to be fetched
/// and have to be written somewhere for the piece to be provable. Writing them
/// into the file they belong to is the cheapest place, and it is what
/// `TODO/disk-io.md` T-013's closing already predicted would happen.
///
/// What was missing is saying so. See `TODO/disk-io.md`, T-184.
fn partial_files(layout: &Layout, only_files: Option<&Vec<usize>>) -> Vec<PartialFile> {
    let Some(selected) = only_files else {
        return Vec::new();
    };
    layout
        .selection_spill(selected)
        .into_iter()
        .filter_map(|spill| {
            let file = layout.file(spill.file)?;
            Some(PartialFile {
                index: spill.file,
                path: file.display_path(),
                bytes: Size(spill.bytes),
                on_disk: Size(spill.written_to),
                length: Size(spill.length),
            })
        })
        .collect()
}

/// Read verified bytes back out of the payload the session is writing.
///
/// This is where the correct bytes for a disputed block come from. Reading
/// them off the disk rather than fetching them again is the whole reason smart
/// ban costs nothing: the session has already hash-checked the piece holding
/// them, so the bytes on disk are the truth by definition, and a source whose
/// recorded hash disagrees with them is proved wrong rather than suspected.
///
/// `None` when the range could not be read, which leaves the piece for the
/// next pass. See `TODO/webseed.md`, T-179.
fn read_payload(
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    options: &Options,
    layout: &Layout,
    offset: u64,
    length: u32,
) -> Option<Vec<u8>> {
    let planned = engine.path_plan(handle)?;
    let root = bit_cli_core::storage::payload_root(&options.directory, layout);
    bit_cli_core::storage::read_range(
        &root,
        layout,
        &planned.disk_paths,
        offset..offset + u64::from(length),
    )
}

/// Where this torrent's files were actually written, when that is not where
/// the torrent said.
///
/// A torrent path that cannot exist on the filesystem, or that would leave the
/// output directory, is rewritten before anything is opened. The caller has to
/// be told, or it cannot find what it downloaded.
fn renames(engine: &Engine, handle: &bit_cli_core::engine::Handle) -> Vec<Rename> {
    engine
        .path_plan(handle)
        .map(|plan| plan.renames)
        .unwrap_or_default()
}

/// A `file:` source per donation whose donor has finished, the report rows that
/// say where each came from, and the donations whose donor has not finished.
///
/// Under `-j 1` the donor ran first, every donation is decided on the first
/// call, and nothing is ever pending. Above that the two are in flight
/// together, so the answer at the moment this torrent starts is "not yet" and
/// the third return is what makes the question askable again. `attach_pending`
/// asks it on every report tick until the list is empty. See
/// `TODO/multi-source.md`, T-140 and T-143.
fn donated_sources(
    donations: &[Donation],
    options: &Options,
    layout: &Layout,
) -> (Vec<SourceSpec>, Vec<SharedFile>, Vec<Donation>) {
    use bit_cli_core::webseed::binding::Origin;
    use bit_cli_core::webseed::composition::Mode;
    use bit_cli_core::webseed::scope::Scope;

    let mut specs = Vec::new();
    let mut shared = Vec::new();
    let mut pending = Vec::new();
    if donations.is_empty() {
        return (specs, shared, pending);
    }
    let Ok(donors) = options.donors.lock() else {
        // A poisoned registry is not a donor that has not finished. Nothing
        // will ever be readable through it, so nothing is left pending.
        return (specs, shared, pending);
    };
    for donation in donations {
        let Some(donor) = donors.get(&donation.donor) else {
            continue;
        };
        let Some(relative) = donor.disk_paths.get(donation.donor_index) else {
            // The donor has not published where it wrote, which is what
            // finishing does. Above `-j 1` that is the common case at the
            // moment this torrent starts, and it is what makes the donation
            // pending rather than absent. See `TODO/multi-source.md`, T-143.
            pending.push(donation.clone());
            continue;
        };
        let mut path = donor.root.clone();
        for component in relative.split('/').filter(|part| !part.is_empty()) {
            path.push(component);
        }
        // A donor that finished has the file. Checking anyway costs one stat
        // and turns a source that would fail every request into no source.
        if !path.is_file() {
            pending.push(donation.clone());
            continue;
        }
        let url = bit_cli_core::webseed::local::url_of(&path);
        let Ok(scope) = Scope::parse(&format!("file:{}", donation.index)) else {
            continue;
        };
        specs.push(
            SourceSpec::new(url.clone(), Origin::SharedFile)
                .with_scope(scope)
                .with_mode(Mode::Exact),
        );
        shared.push(SharedFile {
            index: donation.index,
            path: layout
                .file(donation.index)
                .map(|file| file.display_path())
                .unwrap_or_default(),
            length: Size(donation.length),
            from_source: donor.source.clone(),
            from_info_hash: donor.info_hash.clone(),
            from_index: donation.donor_index,
            from_path: path.display().to_string(),
            pieces_compared: donation.pieces_compared,
            bytes_proven: Size(donation.bytes_proven),
        });
    }
    (specs, shared, pending)
}

/// The first source whose metadata nothing left in this run could fetch.
///
/// A magnet and a bare info hash name a torrent without carrying it. The
/// metadata comes from a peer, over BEP 9, and a peer is found through the
/// DHT, a tracker or local discovery. Turn all three off and there is no
/// second way: a web seed answers ranged GETs for payload and knows nothing
/// about the torrent file, which is exactly why `--web-seed-only` is the flag
/// that produces this.
///
/// `None` when nothing is wrong, which includes every `.torrent` source: a
/// file, a URL and a Metalink all carry their own metadata, so `--web-seed-only`
/// with one of those is the arrangement this tool exists for.
fn metadata_that_cannot_arrive(
    kinds: &[Kind],
    args: &DownloadArgs,
    setup: &swarm::SessionSetup<'_>,
) -> Option<String> {
    // A peer named with `--peer` is dialled whether or not anything was
    // discovered, and BEP 9 is how metadata arrives from a peer, so one is a
    // way for this to work. `--web-seed-only` turns peers off entirely, which
    // takes that way with it.
    let discovery_off = setup.no_dht && setup.no_lsd && setup.trackers.no_tracker;
    let peers_off = setup.web_seeds.web_seed_only || (discovery_off && args.peers.is_empty());
    if !peers_off {
        return None;
    }
    kinds
        .iter()
        .zip(args.sources.iter())
        .find(|(kind, _)| matches!(kind, Kind::Magnet(_) | Kind::InfoHash(_)))
        .map(|(_, source)| source.clone())
}

/// Tell every tracker this torrent uses that something happened.
///
/// The announce carries the session's own peer id and listening port, so a
/// tracker updates the record the session created rather than registering a
/// second peer that then has to be cleaned up. A tracker that fails is counted
/// and nothing else: this is a courtesy announce, and a run does not fail
/// because a tracker was down when it ended.
///
/// See `TODO/trackers.md`, T-062.
async fn announce_event(
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    plan: &Plan,
    options: &Options,
    event: bit_cli_core::tracker::Event,
    at: Duration,
) -> Option<SentAnnounce> {
    use bit_cli_core::tracker::{Announce, Client};

    let urls = plan.trackers.clone().unwrap_or_default();
    if urls.is_empty() {
        return None;
    }
    let port = engine.listen_addr().map(|addr| addr.port()).unwrap_or(0);
    let snapshot = engine.snapshot(handle);
    // A total of zero is a torrent whose metadata has not arrived, which is
    // every magnet until it does. Subtracting from it gives `left = 0`, and
    // zero is the one answer that means something specific: this client is a
    // seed. Announcing that of a torrent whose length is not even known hands
    // this address to every peer looking for one, and none of them can be
    // served. `None` is "not known" and goes out as `UNKNOWN_LEFT`. See
    // `TODO/trackers.md`, T-180.
    //
    // A torrent that really is zero bytes is reported as unknown by the same
    // test. Nothing wants its bytes, so the two answers cost the same.
    let left = (snapshot.total_bytes > 0)
        .then(|| snapshot.total_bytes.saturating_sub(snapshot.progress_bytes));
    let request = Announce {
        event,
        uploaded: snapshot.uploaded_bytes,
        downloaded: snapshot.progress_bytes,
        left,
        // A client that is leaving or has finished is not asking for peers.
        numwant: 0,
        ..Announce::new(handle.info_hash().0, handle.shared().peer_id.0, port, left)
    };

    let client = Client::new(
        &format!("bit-cli/{}", bit_cli_core::VERSION),
        options.tracker_timeout,
        options.tracker_connect_timeout,
    )
    .ok()?;
    let client = std::sync::Arc::new(client);
    let mut work = tokio::task::JoinSet::new();
    for url in &urls {
        let client = client.clone();
        let url = url.clone();
        let request = request.clone();
        work.spawn(async move { client.announce(&url, 0, &request).await });
    }
    let mut accepted = 0usize;
    while let Some(finished) = work.join_next().await {
        if let Ok(result) = finished
            && result.ok
        {
            accepted += 1;
        }
    }
    Some(SentAnnounce {
        event: event.as_str().unwrap_or("none"),
        trackers: urls.len(),
        accepted,
        at_ms: at.as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

/// Record where a finished torrent put its files, so the ones after it can
/// read them.
fn publish_donor(
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
    plan: &Plan,
    options: &Options,
) {
    let Some(planned) = engine.path_plan(handle) else {
        return;
    };
    let Ok(mut donors) = options.donors.lock() else {
        return;
    };
    if let Some(entry) = donors.get_mut(&plan.index) {
        entry.disk_paths = planned.disk_paths;
    }
}

#[allow(clippy::too_many_arguments)]
fn finish(
    plan: &Plan,
    // Where this torrent's payload landed, which is the run's download
    // directory unless `--out` moved it. It replaced an `&Options` this
    // function read one field of. See `TODO/cli-surface.md`, T-226.
    payload_directory: &std::path::Path,
    snapshot: &TorrentSnapshot,
    sources: &[AttachedSource],
    stopped: Stopped,
    elapsed: Duration,
    redials: Vec<Redial>,
    resumed: u64,
    renamed: Vec<Rename>,
) -> TorrentReport {
    let served: u64 = sources.iter().map(AttachedSource::served_bytes).sum();
    let elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
    let mean = match elapsed_ms {
        0 => 0,
        ms => snapshot.progress_bytes.saturating_mul(1000) / ms,
    };
    TorrentReport {
        source: plan.source.clone(),
        info_hash: snapshot.info_hash.clone(),
        name: snapshot.name.clone(),
        stopped,
        finished: snapshot.finished,
        total: Size(snapshot.total_bytes),
        downloaded: Size(snapshot.progress_bytes),
        uploaded: Size(snapshot.uploaded_bytes),
        from_web_seeds: Size(served),
        from_peers: Size(
            snapshot
                .progress_bytes
                .saturating_sub(served)
                .saturating_sub(resumed),
        ),
        from_resume: Size(resumed),
        elapsed_ms,
        elapsed_human: bit_cli_core::units::format_duration(elapsed),
        mean_rate: Size(mean),
        mean_rate_human: format_rate(mean),
        peers_seen: snapshot.peers.seen,
        redials,
        sources: sources.iter().map(AttachedSource::report).collect(),
        output_directory: payload_directory.display().to_string(),
        renamed,
        shared: Vec::new(),
        announced: Vec::new(),
        partial: Vec::new(),
        metalink: None,
        attribution: None,
        verified_files: Vec::new(),
        code: stopped.code(),
        // A run that reached here got past resolving and past initialising,
        // so there is no phase to name: the code and `stopped` say it.
        phase: None,
        error: snapshot.error.clone(),
    }
}

/// Apply `--prefer-web-seed` to the declared sources.
///
/// `bit-cli` cannot reach `librqbit`'s piece picker, so it cannot tell the
/// picker "take this piece from HTTP rather than from that peer". What it can
/// do is give the HTTP source more of what decides which answer arrives first,
/// because the session takes whichever peer answers a block soonest.
///
/// What decides that is receive paths, not requests. The flag used to double
/// the per-source request budget, and `TODO/webseed.md` T-009 measured that at
/// 0.81x: eight times the requests in flight on one connection is slightly
/// slower, not faster. Doubling the connections is 1.92x on the same
/// measurement. So the preference is a doubled connection count, bounded, and
/// the request budget is left alone.
///
/// This is still not the picker. `TODO/webseed.md` T-003 records the gap and
/// what closing it would take.
fn apply_preference(specs: Vec<SourceSpec>, prefer: bool) -> Vec<SourceSpec> {
    if !prefer {
        return specs;
    }
    specs
        .into_iter()
        .map(|mut spec| {
            spec.limits.connections = (spec.limits.connections().saturating_mul(2)).clamp(2, 8);
            spec
        })
        .collect()
}

/// Divide the whole-run request budget across the declared sources.
///
/// `sources` is how many the run will have, which is not always how many are
/// in `specs`: a donation whose donor has not finished yet is a source this
/// torrent will attach and cannot shape yet, because its URL is the path the
/// donor has not written. Counting it in the divisor reserves its share up
/// front, so attaching it later never re-scopes a bridge that is already
/// running. See `TODO/multi-source.md`, T-143.
fn apply_max_total(
    specs: &[SourceSpec],
    max_total: Option<usize>,
    sources: usize,
) -> Vec<SourceSpec> {
    let Some(total) = max_total.filter(|t| *t > 0) else {
        return specs.to_vec();
    };
    if specs.is_empty() {
        return Vec::new();
    }
    let share = (total / sources.max(specs.len())).max(1);
    specs
        .iter()
        .map(|spec| {
            let mut spec = spec.clone();
            spec.limits.concurrency = spec.limits.concurrency.min(share).max(1);
            spec
        })
        .collect()
}

/// What a mirror's window cache is allowed before eviction starts.
///
/// Per source, not per run, which is the whole of what makes the total a
/// number worth reporting. See `TODO/memory.md`, T-041.
pub(crate) const CACHE_BUDGET_PER_SOURCE: u64 = 16 * bit_cli_core::units::MIB;

/// Total window cache above which a run says so before it starts.
///
/// Sixteen mirrors at the default chunk size is 256 MiB, which is the largest
/// total the ordinary case reaches. Anything above it comes from a chunk size
/// the caller chose, and the caller is the one who can undo it.
/// See `TODO/memory.md`, T-041.
pub(crate) const CACHE_TOTAL_WARN: u64 = 256 * bit_cli_core::units::MIB;

/// How many windows each source caches.
///
/// Memory is `windows * chunk_size` per source, so the window count comes down
/// as the chunk size goes up. Four windows of the default 4 MiB is 16 MiB per
/// source, which is the budget a mirror gets before eviction starts.
///
/// **The floor of two is what puts a run over that budget**, and it is
/// deliberate: a cache of one window cannot hold the window a read is being
/// served from and the next one at the same time, so the source re-fetches
/// every window it just evicted. So a 64 MiB chunk size costs 128 MiB per
/// source rather than 16, by design, and [`cache_budget`] is what says so
/// before the run rather than after it.
pub(crate) fn cache_windows(specs: &[SourceSpec]) -> usize {
    let largest = specs
        .iter()
        .map(|s| s.limits.chunk_size)
        .max()
        .unwrap_or(bit_cli_core::units::MIB);
    (CACHE_BUDGET_PER_SOURCE / largest.max(1)).clamp(2, 16) as usize
}

/// What the window caches will cost this run, per source and in total.
///
/// One window count is used for every source, computed from the largest chunk
/// size any of them asked for, so a source with a smaller chunk gets the same
/// number of smaller windows. Both numbers are reported by
/// `bit-cli webseed list --json` and are what the warning below is raised on.
/// See `TODO/memory.md`, T-041.
pub(crate) fn cache_budget(specs: &[SourceSpec]) -> (usize, Vec<u64>, u64) {
    let windows = cache_windows(specs);
    let per_source: Vec<u64> = specs
        .iter()
        .map(|s| s.limits.chunk_size.saturating_mul(windows as u64))
        .collect();
    let total = per_source.iter().copied().fold(0u64, u64::saturating_add);
    (windows, per_source, total)
}

/// The line a run prints when the window caches will cost more than
/// [`CACHE_TOTAL_WARN`], or `None` when they will not.
///
/// It names the flag that produced the number, because the only way a caller
/// gets here is by choosing a chunk size or by attaching a lot of mirrors, and
/// both are theirs to change. See `TODO/memory.md`, T-041.
pub(crate) fn cache_budget_warning(specs: &[SourceSpec]) -> Option<String> {
    let (windows, _, total) = cache_budget(specs);
    if total <= CACHE_TOTAL_WARN || specs.is_empty() {
        return None;
    }
    let largest = specs.iter().map(|s| s.limits.chunk_size).max().unwrap_or(0);
    Some(format!(
        "the window caches will hold up to {} across {} source(s): {windows} window(s) of {} each. Lower --web-seed-chunk-size or attach fewer sources.",
        bit_cli_core::units::format_size(total),
        specs.len(),
        bit_cli_core::units::format_size(largest),
    ))
}

/// Resolve `--select-file` and `--exclude-file` into explicit indices.
///
/// `file_count` is `None` only where the flags do not need one, which
/// `crate::selection::needs_file_count` is how a caller checks. Every source in
/// a run is resolved separately because the count is the torrent's, not the
/// run's. See `TODO/cli-surface.md`, T-185.
fn selection(
    args: &crate::cli::SelectionArgs,
    file_count: Option<usize>,
) -> Result<Option<Vec<usize>>> {
    crate::selection::resolve(&args.select_file, &args.exclude_file, file_count)
}

/// What one source's selection is, given the metadata this run has for it.
///
/// The file count comes from metadata `run` has already parsed, so an
/// exclusion with no selection beside it resolves to its complement before the
/// torrent is added and before anything is fetched. A magnet has no metadata
/// yet: it defers only when the flags actually need a count, which keeps the
/// common case a magnet with no selection at all costing nothing. A usage
/// error surfaces here, before the session starts, rather than per worker.
/// See `TODO/cli-surface.md`, T-185.
fn plan_selection(
    args: &crate::cli::SelectionArgs,
    meta: Option<&Metainfo>,
) -> Result<FileSelection> {
    match meta {
        Some(meta) => Ok(FileSelection::Decided(selection(
            args,
            Some(meta.layout().files.len()),
        )?)),
        // `-O` needs the count for the same reason `--exclude-file` does: an
        // index past the end is a usage error, and the only way to know is to
        // have the file list. Resolving first costs a magnet one metadata
        // round trip it was going to make anyway.
        // See `TODO/cli-surface.md`, T-116 and T-185.
        // `--out` needs the metadata for a different fact than the count:
        // whether the torrent is single-file, which is what says if the path
        // names a file or a directory. Same round trip, so it is the same
        // branch. See `TODO/cli-surface.md`, T-226.
        None if crate::selection::needs_file_count(&args.select_file, &args.exclude_file)
            || !args.index_out.is_empty()
            || args.out.is_some() =>
        {
            Ok(FileSelection::AwaitingCount)
        }
        None => Ok(FileSelection::Decided(selection(args, None)?)),
    }
}

/// Resolve and report without fetching anything.
fn dry_run(
    args: &DownloadArgs,
    global: &Global,
    setup: &SessionSetup<'_>,
    renderer: &mut Renderer,
    env: &mut Env,
    directory: &std::path::Path,
) -> Result<ExitCode> {
    let mut planned = Vec::new();
    for source in &args.sources {
        let kind = Kind::classify(source, env)?;
        let meta = match &kind {
            Kind::File(path) => Some(crate::source::read_torrent_file(path)?),
            _ => None,
        };
        // A dry run reads the Metalink and does not fetch the torrent it
        // names. Everything the document itself claims is reportable without
        // the network: the mirrors, the torrent URL, the size, the checksum.
        // What needs the network is the `.torrent`, and `needs_network` on
        // this row is what says so. This is the cheapest way to check that a
        // `.meta4` says what its author meant.
        //
        // A Metalink named by **URL** is the case where that stops being free:
        // the document itself is the thing to fetch. It is not fetched here,
        // for the same reason `--web-seed-list-url` is not on this path, and
        // `document_needs_network` on the row is what says the block is absent
        // because nothing was contacted rather than because the document had
        // nothing in it. See `TODO/cli-surface.md`, T-154.
        let metalink = match &kind {
            Kind::Metalink(path) => {
                let document = Metalink::read(path)?;
                let file = document.single_file()?.clone();
                Some((document.version.as_str(), file))
            }
            _ => None,
        };
        let specs = webseed_args::collect(
            &args.web_seeds,
            meta.as_ref(),
            metalink.as_ref().map(|(_, file)| file),
            env,
            webseed_args::no_network,
        )?;
        // A dry run reports without doing, so a list URL is refused rather
        // than fetched. That is the decision `--web-seed-list-url` already
        // takes on this same command.
        let trackers = setup
            .tracker_list(meta.as_ref(), env, webseed_args::no_network)?
            .unwrap_or_default();
        let coverage = match (&meta, specs.is_empty()) {
            (Some(meta), false) => {
                let layout = meta.layout();
                let set = bit_cli_core::webseed::binding::BindingSet::resolve(
                    &layout,
                    &meta.info_hash().hex(),
                    &specs,
                )?;
                if args.web_seeds.web_seed_require {
                    set.require_coverage(!args.web_seeds.web_seed_only)?;
                }
                Some(json!({
                    "covered_bytes": set.covered.len(),
                    "uncovered_bytes": set.uncovered.len(),
                    "uncovered_pieces": set.uncovered_pieces,
                    "complete": set.is_complete(),
                }))
            }
            _ => None,
        };
        planned.push(json!({
            "source": source,
            "kind": kind.name(),
            "needs_network": kind.needs_network(),
            "document_needs_network": kind.document_needs_network(),
            "name": meta.as_ref().map(|m| m.layout().name),
            "info_hash": meta.as_ref().map(|m| m.info_hash().hex()),
            "total_bytes": meta.as_ref().map(|m| m.layout().total_length),
            "web_seeds": specs.iter().map(|s| json!({
                "url": s.url,
                "origin": s.origin.as_str(),
                "scope": s.scope.text(),
                "mode": s.mode.as_str(),
            })).collect::<Vec<_>>(),
            "trackers": trackers,
            "coverage": coverage,
            "metalink": metalink.as_ref().map(|(version, file)| json!({
                "version": version,
                "file": file.name,
                "size": file.size,
                "torrents": file.torrents_by_priority().iter().map(|m| &m.url).collect::<Vec<_>>(),
                "mirrors_listed": file.mirrors.len(),
                "mirrors_unsupported": file.unsupported_mirrors,
                "checksum": file.best_checksum().map(|c| json!({
                    "algorithm": c.algorithm,
                    "expected": c.value,
                })),
            })),
        }));
    }

    let report = json!({
        "dry_run": true,
        "directory": directory.display().to_string(),
        "torrents": planned,
    });
    let _ = global;
    // `download_dry_run` rather than `download`. The two documents share almost
    // no fields, and a consumer selecting by `kind`, which is the documented way
    // to select, was getting two shapes under one name. `dry_run: true` stays,
    // so a reader holding the document does not have to know the kind changed.
    // See `TODO/cli-surface.md`, T-156.
    renderer.emit(env, "download_dry_run", &report, || {
        let mut out = vec![
            field("dry run", "nothing will be written"),
            field("directory", directory.display()),
        ];
        for torrent in report["torrents"].as_array().into_iter().flatten() {
            out.push(String::new());
            out.push(field(
                "source",
                torrent["source"].as_str().unwrap_or_default(),
            ));
            // `name` is present exactly when the metainfo was read, which is
            // the one fact both counts below depend on. A dry run does not
            // fetch, so a URL, a magnet and a metalink all reach here with the
            // torrent's own web seeds and trackers unknown, and printing `0`
            // for them said the torrent had none. The `--json` form always
            // said so, through `name`, `info_hash` and `total_bytes` being
            // null; the text form is the one a person reads. See
            // `TODO/cli-surface.md`, T-247.
            let read = torrent["name"].as_str();
            if let Some(name) = read {
                out.push(field("name", name));
            } else {
                out.push(field(
                    "not fetched",
                    "a dry run does not fetch the torrent, so its own web seeds and trackers are not counted",
                ));
            }
            let counted = |key: &str, value: &serde_json::Value| {
                let count = value.as_array().map_or(0, Vec::len);
                match read {
                    Some(_) => field(key, count),
                    // Whatever was named without the torrent: the command
                    // line, a `--web-seed-file`, a metalink's mirrors.
                    None => field(key, format!("{count} so far")),
                }
            };
            out.push(counted("web seeds", &torrent["web_seeds"]));
            out.push(counted("trackers", &torrent["trackers"]));
        }
        out
    })?;
    Ok(ExitCode::Success)
}

/// Run `--on-complete` or `--on-error`, **once per torrent**.
///
/// It ran once for the whole run until T-115, with the first torrent's identity
/// and the run's totals, which describes neither. A `-j 4` invocation is four
/// downloads and a caller notifying something about each of them needs four
/// notifications with four info hashes. A run where one torrent finished and
/// another did not fires both hooks, which the old shape could not express at
/// all: it picked one by `report.failed`.
///
/// See `TODO/cli-surface.md`, T-115, and `docs/hooks.md` for the variables.
fn run_hooks(
    report: &DownloadReport,
    args: &DownloadArgs,
    renderer: &Renderer,
    env: &mut Env,
) -> crate::hooks::HookCounts {
    let mut counts = crate::hooks::HookCounts::default();
    if args.hooks.on_complete.is_none() && args.hooks.on_error.is_none() {
        return counts;
    }
    for torrent in &report.torrents {
        let hook = match torrent.finished {
            true => args.hooks.on_complete.as_deref(),
            false => args.hooks.on_error.as_deref(),
        };
        let Some(command) = hook else { continue };
        let vars = crate::hooks::finished_vars(&crate::hooks::Finished {
            info_hash: &torrent.info_hash,
            name: &torrent.name,
            source: &torrent.source,
            directory: &torrent.output_directory,
            total_bytes: torrent.total.0,
            downloaded_bytes: torrent.downloaded.0,
            from_peers_bytes: torrent.from_peers.0,
            from_web_seeds_bytes: torrent.from_web_seeds.0,
            finished: torrent.finished,
            stopped: torrent.stopped.as_str(),
            elapsed_ms: torrent.elapsed_ms,
            error: torrent.error.as_deref(),
            torrents: report.torrents.len(),
            completed: report.completed,
            failed: report.failed,
            run_elapsed_ms: report.elapsed_ms,
        });
        counts.ran += 1;
        match swarm::run_hook(command, &vars) {
            Ok(0) => {}
            Ok(code) => {
                counts.failed += 1;
                renderer.warn(
                    env,
                    format!("hook `{command}` exited {code} for {}", torrent.name),
                );
            }
            Err(error) => {
                counts.failed += 1;
                renderer.warn(
                    env,
                    format!("hook `{command}` failed for {}: {error}", torrent.name),
                );
            }
        }
    }
    counts
}

fn lines(report: &DownloadReport) -> Vec<String> {
    let mut out = Vec::new();
    for torrent in &report.torrents {
        out.push(field("name", &torrent.name));
        out.push(field("info hash", &torrent.info_hash));
        out.push(field("stopped", torrent.stopped.as_str()));
        out.push(field(
            "downloaded",
            format!(
                "{} of {}",
                format_size(torrent.downloaded.0),
                format_size(torrent.total.0)
            ),
        ));
        out.push(field("from peers", format_size(torrent.from_peers.0)));
        out.push(field(
            "from web seeds",
            format_size(torrent.from_web_seeds.0),
        ));
        // Only when there were any, because a fresh download resumes nothing
        // and a line reading zero on every one of them is noise.
        if torrent.from_resume.0 > 0 {
            out.push(field("already on disk", format_size(torrent.from_resume.0)));
        }
        out.push(field("uploaded", format_size(torrent.uploaded.0)));
        out.push(field("elapsed", &torrent.elapsed_human));
        out.push(field("mean rate", &torrent.mean_rate_human));
        out.push(field("peers seen", torrent.peers_seen));
        // A run that finished only because it threw its peer state away three
        // times is not the same result as one that never stalled, and the
        // totals alone cannot tell them apart.
        if let Some(last) = torrent.redials.last() {
            out.push(field(
                "re-dialled",
                format!(
                    "{} time(s), last after {} of no progress",
                    torrent.redials.len(),
                    bit_cli_core::units::format_duration(Duration::from_millis(last.stalled_ms)),
                ),
            ));
        }
        out.push(field("written to", &torrent.output_directory));
        // A caller that does not know a file was renamed cannot find it, so
        // every rename is listed rather than counted.
        for rename in &torrent.renamed {
            out.push(field(
                &format!("renamed [{}]", rename.index),
                format!("{} -> {}", rename.torrent_path, rename.disk_path),
            ));
        }
        for file in &torrent.shared {
            out.push(field(
                &format!("shared [{}]", file.index),
                format!(
                    "{} read from {} ({} proven over {} piece(s))",
                    file.path,
                    file.from_path,
                    format_size(file.bytes_proven.0),
                    file.pieces_compared,
                ),
            ));
        }
        if let Some(error) = &torrent.error {
            out.push(field("error", error));
        }
        for source in &torrent.sources {
            out.push(String::new());
            out.push(field("source", &source.url));
            out.push(field("  scope", &source.scope));
            out.push(field(
                "  state",
                format!("{:?}", source.state).to_lowercase(),
            ));
            out.push(field("  served", &source.served_human));
            // Only when there were any. A retry line reading zero on every
            // healthy source is noise, and the absence of the line is the
            // same information.
            if source.retries > 0 {
                let by_status: Vec<String> = source
                    .retries_by_status
                    .iter()
                    .map(|(code, count)| format!("{count} on {code}"))
                    .collect();
                let detail = match by_status.is_empty() {
                    true => String::new(),
                    false => format!(" ({})", by_status.join(", ")),
                };
                out.push(field("  retries", format!("{}{detail}", source.retries)));
            }
            // Same rule: absent when the source never lost its connection,
            // which is the healthy case. When it is there it is the line that
            // says a run was waiting rather than working.
            if source.reconnects > 0 {
                let by_reason: Vec<String> = source
                    .reconnect_reasons
                    .iter()
                    .map(|(reason, count)| format!("{count} {reason}"))
                    .collect();
                out.push(field(
                    "  reconnects",
                    format!(
                        "{} in {} ({})",
                        source.reconnects,
                        bit_cli_core::units::format_duration(Duration::from_millis(
                            source.reconnect_wait_ms
                        )),
                        by_reason.join(", ")
                    ),
                ));
            }
            if source.cooldowns > 0 {
                let left = match source.cooldown_remaining_ms {
                    Some(ms) => format!(
                        ", {} left",
                        bit_cli_core::units::format_duration(Duration::from_millis(ms))
                    ),
                    None => String::new(),
                };
                out.push(field("  cooldowns", format!("{}{left}", source.cooldowns)));
            }
            if let Some(error) = &source.error {
                out.push(field("  error", error));
            }
        }
        out.push(String::new());
    }
    if report.torrents.len() > 1 {
        out.push(field("torrents", report.torrents.len()));
        out.push(field("completed", report.completed));
        out.push(field("failed", report.failed));
        out.push(field("downloaded", format_size(report.downloaded.0)));
        out.push(field("elapsed", &report.elapsed_human));
    }
    out.push(field("cost", report.process.summary()));
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    /// A magnet with every way of fetching metadata turned off is refused.
    ///
    /// It used to wait out `--init-timeout` and report a timeout, which reads
    /// like a slow network rather than an arrangement that cannot work. A web
    /// seed answers ranged GETs for payload and knows nothing about the
    /// torrent file. See `TODO/dht.md`, T-051.
    #[test]
    fn a_magnet_with_no_way_to_fetch_metadata_is_refused_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let (mut env, captured) = Env::test(
            &[
                "download",
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
                "--web-seed-only",
                "--web-seed",
                "http://127.0.0.1:1/payload.bin",
                "--dir",
                dir.path().to_str().unwrap(),
            ],
            dir.path(),
        );
        let code = crate::run(&mut env);

        let said = format!("{}{}", captured.out(), captured.err());
        assert_eq!(code, ExitCode::Usage, "{said}");
        assert!(said.contains("carries no metadata"), "{said}");
        // Refused before anything was added, which is what makes the exit
        // code a usage error rather than a source failure. The old path got
        // as far as the session and came back with librqbit's own words, "no
        // known way to resolve peers (no DHT, no trackers, no initial_peers)",
        // and exit 6, which invites the retry that cannot work. See
        // `TODO/dht.md`, T-051.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "the run waited rather than refusing: {:?}",
            started.elapsed()
        );
        assert!(
            !said.contains("initial_peers"),
            "the message still names a librqbit field: {said}"
        );
    }

    /// The same three off, without `--web-seed-only`, is the same problem.
    #[test]
    fn a_magnet_with_dht_lsd_and_trackers_all_off_is_refused_too() {
        let dir = tempfile::tempdir().unwrap();
        let (mut env, captured) = Env::test(
            &[
                "download",
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--dir",
                dir.path().to_str().unwrap(),
            ],
            dir.path(),
        );
        let code = crate::run(&mut env);
        let said = format!("{}{}", captured.out(), captured.err());
        assert_eq!(code, ExitCode::Usage, "{said}");
        assert!(
            said.contains("carries no metadata"),
            "refused, and not for this reason: {said}"
        );
    }

    /// A named peer is a way for metadata to arrive, so the run is not refused.
    ///
    /// BEP 9 carries metadata from a peer, and `--peer` is dialled whether or
    /// not discovery ever answers. A check that refused this would be refusing
    /// the one arrangement a private swarm has. See `TODO/dht.md`, T-051.
    #[test]
    fn a_magnet_with_no_discovery_but_a_named_peer_is_not_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (mut env, captured) = Env::test(
            &[
                "download",
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--peer",
                "127.0.0.1:1",
                "--init-timeout",
                "1s",
                "--dir",
                dir.path().to_str().unwrap(),
            ],
            dir.path(),
        );
        let code = crate::run(&mut env);
        let said = format!("{}{}", captured.out(), captured.err());
        assert_ne!(code, ExitCode::Usage, "{said}");
        assert!(!said.contains("carries no metadata"), "{said}");
    }

    /// A `.torrent` carries its own metadata, so nothing about it is refused.
    ///
    /// This is the arrangement the tool exists for: a torrent file, its
    /// payload from HTTP, and no swarm at all.
    #[test]
    fn a_torrent_file_with_web_seed_only_is_not_refused() {
        let fixture = crate::test_support::TorrentFixture::single_file();
        let server = crate::test_support::FileServer::start(fixture.payload_dir());
        let dir = tempfile::tempdir().unwrap();
        let (mut env, captured) = Env::test(
            &[
                "download",
                fixture.path_str(),
                "--web-seed-only",
                "--web-seed",
                &format!("{}/{}", server.base, fixture.files[0].0),
                "--dir",
                dir.path().to_str().unwrap(),
            ],
            dir.path(),
        );
        let code = crate::run(&mut env);
        assert_ne!(code, ExitCode::Usage, "{}", captured.err());
    }

    use super::*;
    use crate::cli::SelectionArgs;
    use crate::test_support::{
        FileServer, TorrentFixture, run_err, run_json, run_json_code, run_ok,
    };

    /// Every selector value maps to one of two behaviours, and which is which
    /// is stated once.
    ///
    /// `sequential` and `in-order` are synonyms on purpose: one is the common
    /// name and the other is `aria2`'s. `default` is not a synonym for either,
    /// and the enum used to carry two more values that named behaviour nothing
    /// implemented. See `TODO/performance.md`, T-032.
    #[test]
    fn sequential_and_in_order_are_the_same_selector() {
        use crate::cli::PieceSelector;
        assert!(wants_in_order(PieceSelector::Sequential));
        assert!(wants_in_order(PieceSelector::InOrder));
        assert!(!wants_in_order(PieceSelector::Default));
        assert_eq!(PieceSelector::default(), PieceSelector::Default);
    }

    /// `--verify-on-complete` re-reads the payload and reports a digest per
    /// file, and the run still exits 0.
    ///
    /// The digests are checked against the bytes the fixture wrote, hashed in
    /// this test rather than copied from a previous run's output, so what is
    /// asserted is that the flag reports the payload's real digest and not that
    /// it reports the same thing it did last time.
    ///
    /// See `docs/integrity.md` and `TODO/multi-source.md`, T-136.
    #[test]
    fn verify_on_complete_reports_a_digest_per_file() {
        let fixture = TorrentFixture::multi_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let out = dir.join("out");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
                "--verify-on-complete",
            ],
            dir.clone(),
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");

        let rows = report["torrents"][0]["verified_files"]
            .as_array()
            .unwrap_or_else(|| panic!("no verified_files: {report}"));
        assert_eq!(rows.len(), fixture.files.len(), "{report}");
        for (index, (path, bytes)) in fixture.files.iter().enumerate() {
            let row = &rows[index];
            assert_eq!(row["index"], index, "{row}");
            assert_eq!(row["torrent_path"], *path, "{row}");
            assert_eq!(row["algorithm"], "sha256", "{row}");
            assert_eq!(row["bytes"], bytes.len(), "{row}");
            assert_eq!(row["length"], bytes.len(), "{row}");
            assert_eq!(row["error"], serde_json::Value::Null, "{row}");
            // Hashed here from the bytes the fixture wrote, so this compares
            // the report against the payload rather than against itself.
            let expected = bit_cli_core::digest::hash_file(
                std::path::Path::new(row["disk_path"].as_str().expect("a path")),
                "sha256",
            )
            .expect("hash the file on disk");
            assert_eq!(row["hex"], expected.hex, "{row}");
            let mut digest = bit_cli_core::digest::Digest::new("sha256").expect("a digest");
            digest.update(bytes);
            assert_eq!(
                row["hex"],
                digest.finish(),
                "the digest is not the payload's: {row}"
            );
        }

        // Off by default: the block is absent rather than empty, so a caller
        // reading `verified_files` knows the difference between "nothing was
        // checked" and "nothing was there".
        let without = run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
            ],
            dir.clone(),
        );
        assert_eq!(
            without["torrents"][0]["verified_files"],
            serde_json::Value::Null,
            "{without}"
        );
    }

    /// A run that did not finish is not hashed. Digests of files that are not
    /// yet the files are a wrong answer rather than a missing one. T-136.
    #[test]
    fn verify_on_complete_hashes_nothing_when_the_run_did_not_finish() {
        let fixture = TorrentFixture::multi_file();
        let out = fixture.dir().join("out");
        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().expect("utf-8 path"),
                "--web-seed-only",
                "--web-seed",
                "http://127.0.0.1:9/",
                "--no-torrent-web-seed",
                "--no-tracker",
                "--port",
                "0",
                "--stop-after",
                "2s",
                "--verify-on-complete",
            ],
            fixture.dir(),
            ExitCode::Timeout,
        );
        assert_eq!(report["torrents"][0]["finished"], false, "{report}");
        assert_eq!(
            report["torrents"][0]["verified_files"],
            serde_json::Value::Null,
            "{report}"
        );
    }

    /// `--on-complete` fires **once per torrent**, with a different info hash
    /// each time.
    ///
    /// This is T-115's acceptance, run. It fired once for the whole run before,
    /// with the first torrent's identity and the run's totals, which describes
    /// neither: a `-j 2` invocation is two downloads and a caller notifying
    /// something about each of them got one notification.
    ///
    /// The hook appends `BIT_CLI_INFO_HASH` to a file. Reading a file the
    /// hooks wrote is what makes this a measurement of the hooks rather than
    /// of the report: the report is what the run says it did, and the file is
    /// what actually ran.
    #[test]
    fn on_complete_fires_once_per_torrent_with_its_own_info_hash() {
        let one = TorrentFixture::multi_file();
        let two = TorrentFixture::single_file();
        let dir = one.dir();
        let server_one = crate::test_support::FileServer::start(dir.clone());
        let server_two = crate::test_support::FileServer::start(two.dir());
        // The hook creates a directory named after the variables it was
        // given. `mkdir` rather than a redirected `echo`: a redirect goes
        // through `cmd`'s own parser after Rust has already quoted the
        // argument, and the two disagree about the quoting of a Windows path.
        // What is being tested is the hook, not the shell.
        let marks = dir.join("marks");
        std::fs::create_dir_all(&marks).expect("make the marker directory");
        let marks_arg = marks.to_str().expect("utf-8 path").to_string();
        let command = match cfg!(windows) {
            true => format!(r#"mkdir "{marks_arg}\%BIT_CLI_HOOK%-%BIT_CLI_INFO_HASH%""#),
            false => format!(r#"mkdir -p "{marks_arg}/$BIT_CLI_HOOK-$BIT_CLI_INFO_HASH""#),
        };
        let out = dir.join("out");
        let report = run_json(
            &[
                "download",
                one.path_str(),
                two.path_str(),
                "-j",
                "2",
                "--dir",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server_one.base),
                "--web-seed",
                &format!("{}payload/", server_two.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
                "--on-complete",
                &command,
            ],
            dir.clone(),
        );
        assert_eq!(report["completed"], 2, "{report}");
        assert_eq!(report["hooks"]["ran"], 2, "{report}");
        assert_eq!(report["hooks"]["failed"], 0, "{report}");

        let mut left: Vec<String> = std::fs::read_dir(&marks)
            .expect("read the marker directory")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        left.sort();
        // Two invocations, two info hashes, and `BIT_CLI_HOOK` naming which
        // hook it was on both.
        assert_eq!(
            left.len(),
            2,
            "the hook ran {} time(s): {left:?}",
            left.len()
        );
        assert!(
            left.contains(&format!("on-complete-{}", one.info_hash)),
            "{left:?}"
        );
        assert!(
            left.contains(&format!("on-complete-{}", two.info_hash)),
            "{left:?}"
        );
    }

    /// `--on-piece-verified` fires, once per piece, and the run counts it.
    ///
    /// It reached no code at all before T-115: the field was on `cli.rs`'s own
    /// list of things nothing outside that file reads. See
    /// `TODO/cli-surface.md`, T-115, and `docs/hooks.md` for what it costs.
    #[test]
    fn on_piece_verified_fires_once_per_piece() {
        let fixture = TorrentFixture::multi_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let marks = dir.join("marks");
        std::fs::create_dir_all(&marks).expect("make the marker directory");
        let marks_arg = marks.to_str().expect("utf-8 path").to_string();
        // One directory per piece index, so a hook that fired twice for one
        // piece cannot look like two pieces.
        let command = match cfg!(windows) {
            true => format!(r#"mkdir "{marks_arg}\piece-%BIT_CLI_PIECE%""#),
            false => format!(r#"mkdir -p "{marks_arg}/piece-$BIT_CLI_PIECE""#),
        };
        let out = dir.join("out");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
                "--on-piece-verified",
                &command,
            ],
            dir.clone(),
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");
        let pieces = report["torrents"][0]["total"]["bytes"]
            .as_u64()
            .unwrap_or(0);
        assert!(pieces > 0, "{report}");

        let mut left: Vec<String> = std::fs::read_dir(&marks)
            .expect("read the marker directory")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        left.sort();
        // Every piece of the fixture, and nothing else. The count comes from
        // the report rather than from a number written here, so a fixture
        // whose piece length changes does not quietly stop testing anything.
        let expected = report["torrents"][0]["downloaded"]["bytes"]
            .as_u64()
            .expect("a byte count");
        assert!(
            !left.is_empty(),
            "the hook never fired for {expected} bytes"
        );
        assert!(
            left.iter().all(|name| name.starts_with("piece-")),
            "{left:?}"
        );
        // Accounted for either way: what ran plus what was skipped is what the
        // markers show, and nothing vanished.
        let ran = report["hooks"]["ran"].as_u64().unwrap_or(0);
        let skipped = report["hooks"]["skipped"].as_u64().unwrap_or(0);
        assert_eq!(ran, left.len() as u64, "{report}");
        assert_eq!(skipped, 0, "a fixture this small cannot fill the queue");
        assert_eq!(report["hooks"]["failed"], 0, "{report}");
    }

    /// A run where one torrent finished and another did not fires **both**
    /// hooks. The old shape could not express that at all: it picked one for
    /// the whole run by counting failures. T-115.
    #[test]
    fn a_mixed_run_fires_on_complete_and_on_error() {
        let good = TorrentFixture::multi_file();
        let bad = TorrentFixture::single_file();
        let dir = good.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let marks = dir.join("marks");
        std::fs::create_dir_all(&marks).expect("make the marker directory");
        let marks_arg = marks.to_str().expect("utf-8 path").to_string();
        let command = match cfg!(windows) {
            true => format!(r#"mkdir "{marks_arg}\%BIT_CLI_HOOK%""#),
            false => format!(r#"mkdir -p "{marks_arg}/$BIT_CLI_HOOK""#),
        };
        let out = dir.join("out");
        // The second torrent has no source it can reach: its own payload is
        // not under this server's root, so it runs out its deadline.
        let (mut env, captured) = crate::env::Env::test(
            &[
                "--json",
                "download",
                good.path_str(),
                bad.path_str(),
                "-j",
                "2",
                "--dir",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "3s",
                "--on-complete",
                &command,
                "--on-error",
                &command,
            ],
            dir.clone(),
        );
        crate::run(&mut env);
        let report: serde_json::Value = captured.json().expect("a JSON report");
        assert_eq!(report["completed"], 1, "{report}");
        assert_eq!(report["failed"], 1, "{report}");

        assert!(
            marks.join("on-complete").is_dir(),
            "--on-complete did not fire"
        );
        assert!(marks.join("on-error").is_dir(), "--on-error did not fire");
    }

    /// `-o`/`--out` writes a **multi-file** torrent's payload directly into
    /// the named directory, without the torrent's own name under it.
    ///
    /// The flag parsed and reached no code at all until T-226: renaming the
    /// field broke no build, and a run passing it wrote where it would have
    /// written anyway. See `TODO/cli-surface.md`, T-226.
    #[test]
    fn out_writes_a_multi_file_payload_into_the_named_directory() {
        let fixture = TorrentFixture::multi_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let out = dir.join("elsewhere");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--out",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
            ],
            dir.clone(),
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");

        for (path, bytes) in &fixture.files {
            let landed = out.join(path);
            assert!(landed.is_file(), "nothing at {}", landed.display());
            assert_eq!(
                &std::fs::read(&landed).expect("read the file"),
                bytes,
                "{} does not hold the torrent's bytes",
                landed.display()
            );
        }
        // The torrent's own name is what `--out` replaced, so it must not be
        // a directory under it. Without this the test passes on a run that
        // ignored the flag and wrote to `<cwd>/album/` instead.
        assert!(
            !out.join(&fixture.name).exists(),
            "the torrent's name is still a directory under --out"
        );
        assert!(
            !dir.join(&fixture.name).exists(),
            "the payload also landed where --out was supposed to move it from"
        );
        // And the report says where it went, which is the half a script reads.
        assert_eq!(
            report["torrents"][0]["output_directory"],
            out.display().to_string(),
            "{report}"
        );
    }

    /// For a **single-file** torrent the payload is one file, so `--out` names
    /// that file. T-226.
    #[test]
    fn out_names_the_file_itself_for_a_single_file_torrent() {
        let fixture = TorrentFixture::single_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let out = dir.join("renamed").join("payload.dat");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--out",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
            ],
            dir.clone(),
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");
        assert!(out.is_file(), "nothing at {}", out.display());
        assert_eq!(
            std::fs::read(&out).expect("read the renamed payload"),
            fixture.files[0].1
        );
        assert!(
            !dir.join(&fixture.name).exists(),
            "the payload also landed under the torrent's own name"
        );
    }

    /// A relative `--out` is relative to `--dir`, so neither flag is inert
    /// beside the other. T-226.
    #[test]
    fn a_relative_out_resolves_against_dir() {
        let fixture = TorrentFixture::multi_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let base = dir.join("base");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                base.to_str().expect("utf-8 path"),
                "--out",
                "under",
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
            ],
            dir.clone(),
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");
        let landed = base.join("under").join(&fixture.files[0].0);
        assert!(landed.is_file(), "nothing at {}", landed.display());
        assert_eq!(
            report["torrents"][0]["output_directory"],
            base.join("under").display().to_string(),
            "{report}"
        );
    }

    /// `--out` may name a path outside the output directory, and that is the
    /// decision rather than an oversight.
    ///
    /// The operator ruled on it on 2026-08-24: `--out` is the caller's own
    /// path, typed on their own command line, and `--dir` is already allowed
    /// anywhere. The neighbour it reads inconsistently against is
    /// `-O`/`--index-out`, which is sanitised, and the difference is that
    /// `-O` names a file **inside** the output directory while `--out` names
    /// the destination itself.
    ///
    /// Pinned so that tightening it later is a decision somebody makes against
    /// a passing test rather than a change nobody notices. T-226.
    #[test]
    fn out_may_leave_the_output_directory_because_it_is_the_callers_path() {
        let fixture = TorrentFixture::multi_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let base = dir.join("base");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                base.to_str().expect("utf-8 path"),
                "--out",
                "../beside",
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
            ],
            dir.clone(),
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");
        let landed = dir.join("beside").join(&fixture.files[0].0);
        assert!(
            landed.is_file(),
            "nothing at {}, so --out no longer leaves --dir",
            landed.display()
        );
        assert!(
            !base.join("beside").exists(),
            "the `..` was swallowed and the payload stayed inside --dir"
        );
        // The report says where it went, with the `..` resolved. A report that
        // still carried one would be naming a path the reader has to resolve
        // themselves.
        assert_eq!(
            report["torrents"][0]["output_directory"],
            dir.join("beside").display().to_string(),
            "{report}"
        );
    }

    /// `--out` names where one payload goes, so a run with two sources is a
    /// usage error before the session starts rather than two torrents writing
    /// over each other. T-226.
    #[test]
    fn out_with_more_than_one_source_is_a_usage_error() {
        let first = TorrentFixture::multi_file();
        let second = TorrentFixture::single_file();
        let dir = first.dir();
        let said = run_err(
            &[
                "download",
                first.path_str(),
                second.path_str(),
                "--out",
                "somewhere",
                "--port",
                "0",
            ],
            dir,
            ExitCode::Usage,
        );
        assert!(said.contains("--out"), "{said}");
        assert!(said.contains("2 sources"), "{said}");
    }

    /// `-O`/`--index-out` writes a file to the path the caller named, and
    /// `--json` reports the mapping.
    ///
    /// The flag parsed and reached no code at all until T-116: it was on
    /// `cli.rs`'s own list of fields nothing outside that file reads. See
    /// `TODO/cli-surface.md`, T-116.
    ///
    /// The payload comes from a web seed rather than a swarm, so the run
    /// finishes and the bytes are really written to the renamed path rather
    /// than the path merely being planned.
    #[test]
    fn index_out_writes_the_file_where_the_caller_asked() {
        let fixture = TorrentFixture::multi_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let out = dir.join("out");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
                "-O",
                "0=renamed/first.bin",
            ],
            dir.clone(),
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");

        // The mapping, which is the half of the acceptance a script reads.
        let renamed = &report["torrents"][0]["renamed"];
        assert_eq!(renamed.as_array().map(Vec::len), Some(1), "{report}");
        assert_eq!(renamed[0]["index"], 0);
        assert_eq!(renamed[0]["disk_path"], "renamed/first.bin");
        assert_eq!(renamed[0]["reasons"][0], "requested");
        let torrent_path = renamed[0]["torrent_path"].as_str().expect("a path");

        // And the bytes, which is the half a user cares about. The file is
        // where it was asked for, it is not where the torrent said, and it
        // holds what the torrent says it should.
        let landed = out.join(&fixture.name).join("renamed/first.bin");
        assert!(landed.is_file(), "nothing at {}", landed.display());
        let original = out.join(&fixture.name).join(torrent_path);
        assert!(
            !original.exists(),
            "{} is still there too",
            original.display()
        );
        assert_eq!(
            std::fs::read(&landed).expect("read the renamed file"),
            fixture.files[0].1,
            "the renamed file does not hold the torrent's first file"
        );
    }

    /// `verify` finds a file `download -O` renamed, and only when it is told.
    ///
    /// `verify` looks where the bytes went rather than where the torrent said
    /// they would go, which is [T-076], but the plan it builds knows nothing
    /// about `-O` unless it is given the same argument. Without this the tree
    /// could rename a file its own verifier then reported as missing, which is
    /// half a feature. See `TODO/cli-surface.md`, T-116.
    ///
    /// Both directions, because the second is what makes the first mean
    /// something: without `-O`, `verify` looks at the torrent's path and finds
    /// nothing there.
    ///
    /// [T-076]: `TODO/windows.md`
    #[test]
    fn verify_finds_a_file_renamed_by_index_out_when_it_is_told() {
        let fixture = TorrentFixture::multi_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let out = dir.join("out");
        let report = run_json(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().expect("utf-8 path"),
                "--web-seed",
                &format!("{}payload/", server.base),
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--no-torrent-web-seed",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
                "-O",
                "0=renamed.bin",
            ],
            dir.clone(),
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");
        let data = out.join(&fixture.name);
        let data = data.to_str().expect("utf-8 path");

        let told = run_json(
            &[
                "verify",
                fixture.path_str(),
                "--data",
                data,
                "-O",
                "0=renamed.bin",
            ],
            dir.clone(),
        );
        assert_eq!(told["kind"], "verify", "{told}");
        assert_eq!(told["files"][0]["present"], true, "{told}");
        assert_eq!(told["complete"], true, "{told}");
        assert_eq!(told["pieces_bad"], 0, "{told}");
        // The mapping is in the report, with the reason, exactly as the
        // download's was.
        assert_eq!(told["renamed"][0]["disk_path"], "renamed.bin", "{told}");
        assert_eq!(told["renamed"][0]["reasons"][0], "requested", "{told}");

        // The same thing said from one directory up. `--data` may name the
        // parent or the torrent's own directory, and the renamed file is what
        // the resolver looks for to tell them apart, so this is the spelling
        // that used to be answered with the parent and then find nothing.
        // See `TODO/cli-surface.md`, T-213.
        let from_parent = run_json(
            &[
                "verify",
                fixture.path_str(),
                "--data",
                out.to_str().expect("utf-8 path"),
                "-O",
                "0=renamed.bin",
            ],
            dir.clone(),
        );
        assert_eq!(from_parent["complete"], true, "{from_parent}");
        assert_eq!(from_parent["pieces_bad"], 0, "{from_parent}");

        // Not told, and the file is not where the torrent says it is. The run
        // fails, so the document is `hash_mismatch` and the report is nested
        // under it, which is the shape `verify` has always written for a
        // failure.
        let (mut env, captured) = crate::env::Env::test(
            &["--json", "verify", fixture.path_str(), "--data", data],
            dir.clone(),
        );
        assert_eq!(crate::run(&mut env), ExitCode::HashMismatch);
        let untold: serde_json::Value = captured
            .json()
            .expect("the report is JSON whatever it says");
        assert_eq!(untold["kind"], "hash_mismatch", "{untold}");
        let nested = &untold["context"]["report"];
        assert_eq!(nested["files"][0]["present"], false, "{untold}");
        assert_eq!(nested["complete"], false, "{untold}");
    }

    /// An index the torrent does not have is a usage error, not a rename that
    /// quietly does nothing. T-116.
    #[test]
    fn index_out_past_the_last_file_is_a_usage_error() {
        let fixture = TorrentFixture::multi_file();
        let (mut env, captured) = crate::env::Env::test(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                fixture.dir().join("out").to_str().expect("utf-8 path"),
                "-O",
                "9=x.bin",
            ],
            fixture.dir(),
        );
        assert_eq!(crate::run(&mut env), ExitCode::Usage, "{}", captured.err());
        assert!(captured.err().contains("no file 9"), "{}", captured.err());
    }

    /// A Metalink named by URL downloads, and reports what the saved copy does.
    ///
    /// `Kind::classify` checked the `http://` prefix before the `.meta4`
    /// extension, so this was a `Kind::Url`, was handed to the session as a
    /// `.torrent`, and failed on the bencode parse with a message about the
    /// torrent rather than about the metalink. Every real Metalink is served
    /// over HTTP. See `TODO/cli-surface.md`, T-154.
    ///
    /// The same document is run twice, once by path and once by URL, and the
    /// two `metalink` blocks are compared. That is what the entry's "behaves
    /// exactly as the same document saved to disk does" means, and it is
    /// stronger than asserting fields one at a time: a field added later is
    /// compared without this test being edited.
    #[test]
    fn a_metalink_named_by_url_downloads_the_same_as_one_on_disk() {
        let fixture = TorrentFixture::single_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let payload = &fixture.files[0].1;
        let sha1: String = <sha1::Sha1 as sha1::Digest>::digest(payload)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        // Written where the server can serve it, so the same bytes are
        // reachable both ways.
        let meta4 = dir.join("release.meta4");
        std::fs::write(
            &meta4,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="payload.bin">
    <size>{size}</size>
    <hash type="sha-1">{sha1}</hash>
    <url priority="1">{base}payload/payload.bin</url>
    <metaurl mediatype="torrent">{base}payload.bin.torrent</metaurl>
  </file>
</metalink>
"#,
                size = payload.len(),
                base = server.base,
            ),
        )
        .expect("write the metalink");

        let run = |source: &str, out: &str| {
            run_json(
                &[
                    "download",
                    source,
                    "--dir",
                    out,
                    "--web-seed-only",
                    "--allow-overwrite",
                    "--port",
                    "0",
                    "--stop-after",
                    "30s",
                ],
                dir.clone(),
            )
        };
        let from_disk_out = dir.join("out-disk");
        let from_disk = run(
            meta4.to_str().expect("utf-8 path"),
            from_disk_out.to_str().expect("utf-8 path"),
        );
        let from_url_out = dir.join("out-url");
        let url = format!("{}release.meta4", server.base);
        let from_url = run(&url, from_url_out.to_str().expect("utf-8 path"));

        assert_eq!(from_url["torrents"][0]["finished"], true, "{from_url}");
        assert_eq!(from_url["torrents"][0]["source"], url, "{from_url}");
        assert_eq!(
            from_url["torrents"][0]["from_web_seeds"]["bytes"],
            payload.len(),
            "{from_url}"
        );

        // `checksum.path` is the one field that must differ: each run wrote
        // into its own directory. Blanked for the comparison and asserted on
        // its own, so a payload that landed somewhere else fails rather than
        // passes.
        let mut a = from_disk["torrents"][0]["metalink"].clone();
        let mut b = from_url["torrents"][0]["metalink"].clone();
        assert!(b.is_object(), "no metalink block for the URL: {from_url}");
        let checked = b["checksum"]["path"].as_str().unwrap_or("").to_string();
        for block in [&mut a, &mut b] {
            block["checksum"]["path"] = serde_json::Value::Null;
        }
        assert_eq!(a, b, "the URL's metalink block differs from the file's");
        assert!(
            std::path::Path::new(&checked).starts_with(&from_url_out),
            "the checksum was computed over {checked}, expected a file under {}",
            from_url_out.display()
        );
    }

    /// `--hash-check-only` over a Metalink reports the document, not silence.
    ///
    /// `one_inner` returned for that flag above the block that built the
    /// `metalink` report, so the run said nothing about the document at all:
    /// not the mirror count, not the torrent it resolved, and not the size
    /// comparison, which is computed before that return and was then thrown
    /// away. See `TODO/cli-surface.md`, T-155.
    ///
    /// The payload is complete on disk before the checked run starts, which is
    /// the case worth holding: the hash check proves the bytes against the
    /// torrent and the checksum then proves the same bytes against the
    /// Metalink, which is the strongest thing this flag can report.
    ///
    /// `scripts/check-metalink.ps1` case `hash_check_only` is the acceptance.
    /// This is the same case in `cargo test`, because CI does not run that
    /// script and a return moved back above the call would otherwise be caught
    /// only by somebody running it locally.
    #[test]
    fn hash_check_only_over_a_metalink_still_reports_the_document() {
        let fixture = TorrentFixture::single_file();
        let dir = fixture.dir();
        let server = crate::test_support::FileServer::start(dir.clone());
        let payload = &fixture.files[0].1;
        let sha1: String = <sha1::Sha1 as sha1::Digest>::digest(payload)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let meta4 = dir.join("release.meta4");
        std::fs::write(
            &meta4,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="payload.bin">
    <size>{size}</size>
    <hash type="sha-1">{sha1}</hash>
    <url priority="1">{base}payload/payload.bin</url>
    <metaurl mediatype="torrent">{base}payload.bin.torrent</metaurl>
  </file>
</metalink>
"#,
                size = payload.len(),
                base = server.base,
            ),
        )
        .expect("write the metalink");
        let meta4 = meta4.to_str().expect("utf-8 path").to_string();
        let out = dir.join("out");
        let out_arg = out.to_str().expect("utf-8 path").to_string();

        // Fetch it once, so the payload on disk is complete and verified.
        let filled = run_json(
            &[
                "download",
                &meta4,
                "--dir",
                &out_arg,
                "--web-seed-only",
                "--allow-overwrite",
                "--port",
                "0",
                "--stop-after",
                "30s",
            ],
            dir.clone(),
        );
        assert_eq!(filled["torrents"][0]["finished"], true, "{filled}");

        // Then check what is there, and read what the document says about it.
        //
        // `--port 0` and `--no-dht` because this run needs no swarm at all:
        // without them it opened a DHT alongside every other test in the
        // module and failed with "error initializing persistent DHT" once the
        // module grew enough parallel tests to contend. A hash check reads the
        // disk; asserting that a DHT can be started is asserting something
        // else.
        let checked = run_json(
            &[
                "download",
                &meta4,
                "--dir",
                &out_arg,
                "--hash-check-only",
                "--port",
                "0",
                "--no-dht",
            ],
            dir.clone(),
        );
        let metalink = &checked["torrents"][0]["metalink"];
        assert!(
            metalink.is_object(),
            "no metalink block, which is the whole of T-155: {checked}"
        );
        assert_eq!(metalink["agreement"]["size_agrees"], true, "{metalink}");
        assert_eq!(
            metalink["agreement"]["metalink_size"],
            payload.len(),
            "{metalink}"
        );
        assert_eq!(metalink["mirrors_listed"], 1, "{metalink}");
        // A payload that is complete on disk gets the strongest answer
        // available: the digest computed and compared, rather than a reason it
        // was not.
        assert_eq!(metalink["checksum"]["matched"], true, "{metalink}");
        assert_eq!(metalink["checksum"]["expected"], sha1, "{metalink}");
        assert_eq!(
            metalink["checksum"]["bytes_hashed"],
            payload.len(),
            "{metalink}"
        );
    }

    /// T-247. The text form printed `trackers 0` for a torrent it had not
    /// read, so the two renderings of one document disagreed and the one a
    /// person reads was the wrong one.
    #[test]
    fn a_dry_run_over_a_url_prints_no_count_it_did_not_take() {
        let fixture = TorrentFixture::multi_file();
        let server = FileServer::start(fixture.dir());
        let url = format!("{}album.torrent", server.base);

        let out = run_ok(&["download", &url, "--dry-run"], fixture.dir());
        assert!(out.contains("not fetched"), "{out}");
        assert!(out.contains("web seeds            0 so far"), "{out}");
        assert!(out.contains("trackers             0 so far"), "{out}");
        assert!(!out.contains("name  "), "the name cannot be known: {out}");

        // What is known without the torrent is still counted, and still says
        // it is only what is known so far.
        let out = run_ok(
            &[
                "download",
                &url,
                "--dry-run",
                "--web-seed",
                "https://mirror.example.com/pub/",
                "--tracker",
                "udp://t.example.com:80",
            ],
            fixture.dir(),
        );
        assert!(out.contains("web seeds            1 so far"), "{out}");
        assert!(out.contains("trackers             1 so far"), "{out}");
    }

    /// The other half: a torrent that **was** read still reports what it
    /// carries, with no qualifier on it.
    #[test]
    fn a_dry_run_over_a_local_torrent_counts_what_it_read() {
        let fixture = TorrentFixture::multi_file();
        let out = run_ok(
            &["download", fixture.path_str(), "--dry-run"],
            fixture.dir(),
        );
        assert!(out.contains("name                 album"), "{out}");
        // The fixture carries one tier of one tracker and one web seed.
        assert!(out.contains("web seeds            1"), "{out}");
        assert!(out.contains("trackers             1"), "{out}");
        assert!(!out.contains("so far"), "{out}");
        assert!(!out.contains("not fetched"), "{out}");
    }

    /// And the JSON shape is untouched, which is what the acceptance asks.
    /// The document already said the torrent was not read, through three
    /// nulls and `needs_network`.
    #[test]
    fn the_dry_run_json_still_carries_the_nulls_that_said_so() {
        let fixture = TorrentFixture::multi_file();
        let server = FileServer::start(fixture.dir());
        let url = format!("{}album.torrent", server.base);
        let doc = run_json(&["download", &url, "--dry-run"], fixture.dir());
        let torrent = &doc["torrents"][0];
        assert_eq!(torrent["name"], serde_json::Value::Null);
        assert_eq!(torrent["info_hash"], serde_json::Value::Null);
        assert_eq!(torrent["total_bytes"], serde_json::Value::Null);
        assert_eq!(torrent["needs_network"], true);
        assert_eq!(torrent["web_seeds"].as_array().map(Vec::len), Some(0));
        assert_eq!(torrent["trackers"].as_array().map(Vec::len), Some(0));
    }

    /// A dry run and a real run are two documents, so they carry two kinds.
    ///
    /// They share `dry_run` and `directory` and nothing else that matters: a
    /// real run has `stopped`, `finished`, `sources[]` and `total`, and this
    /// has `torrents[].kind`, `needs_network`, `coverage` and `total_bytes`. A
    /// consumer selecting by `kind`, which is the documented way to select,
    /// was getting both under `download`. See `TODO/cli-surface.md`, T-156.
    #[test]
    fn a_dry_run_writes_its_own_document_kind() {
        let fixture = TorrentFixture::multi_file();
        let report = run_json(
            &["download", fixture.path_str(), "--dry-run"],
            fixture.dir(),
        );
        assert_eq!(report["kind"], "download_dry_run");
        // Kept, so a reader holding the document does not have to know the
        // kind changed to know what it is.
        assert_eq!(report["dry_run"], true);
        assert_eq!(report["torrents"][0]["kind"], "torrent_file");
        assert_eq!(report["torrents"][0]["needs_network"], false);

        // The other half: a real run is still `download`. Without this the
        // case above passes if the kind is renamed everywhere.
        let out = fixture.dir().join("out");
        let real = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-tracker",
                "--port",
                "0",
                "--stop-after",
                "1s",
            ],
            fixture.dir(),
            ExitCode::Timeout,
        );
        assert_eq!(real["kind"], "download");
        assert_eq!(real["dry_run"], serde_json::Value::Null);
    }

    /// A torrent whose paths cannot be written as given still reports where
    /// its files went. Without this a caller cannot find what it downloaded.
    ///
    /// The run has no peers and no web seeds, so it stops on its deadline. The
    /// storage is created when the torrent is added, which is before any of
    /// that matters, so the mapping is there either way. See
    /// `TODO/windows.md` T-071 and T-072.
    #[test]
    fn a_hostile_torrent_reports_every_renamed_path_in_json() {
        let fixture = TorrentFixture::hostile();
        let out = fixture.dir().join("out");
        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--web-seed-only",
                "--web-seed",
                "http://127.0.0.1:9/",
                "--no-tracker",
                // An OS-chosen port, so two tests running at once cannot
                // race for the same one.
                "--port",
                "0",
                "--stop-after",
                "2s",
            ],
            fixture.dir(),
            ExitCode::Timeout,
        );

        let renamed = report["torrents"][0]["renamed"]
            .as_array()
            .expect("a renamed array")
            .clone();
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
            ]
        );
        // The index ties each entry back to the torrent's own file list, and
        // the reason says which rule applied.
        assert_eq!(renamed[0]["index"], 0);
        assert_eq!(renamed[0]["reasons"][0], "escape");
        assert_eq!(renamed[1]["reasons"][0], "reserved-name");
        assert_eq!(renamed[4]["reasons"][0], "case-collision");

        // Every file landed, including the two that collide only on a
        // case-insensitive filesystem.
        let mut landed: Vec<String> = walk(&out.join("hostile"));
        landed.sort();
        assert_eq!(
            landed,
            [
                "CON_.txt",
                "C_/pwned.txt",
                "README",
                "a_b.bin",
                "readme-1",
                "x"
            ]
        );
    }

    /// An ordinary torrent reports no renames at all, so a caller can test for
    /// an empty list rather than comparing every path.
    #[test]
    fn an_ordinary_torrent_reports_no_renames() {
        let fixture = TorrentFixture::multi_file();
        let out = fixture.dir().join("out");
        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--web-seed-only",
                "--web-seed",
                "http://127.0.0.1:9/",
                "--no-tracker",
                // An OS-chosen port, so two tests running at once cannot
                // race for the same one.
                "--port",
                "0",
                "--stop-after",
                "2s",
            ],
            fixture.dir(),
            ExitCode::Timeout,
        );
        assert!(report["torrents"][0].get("renamed").is_none());
    }

    /// `TODO/disk-io.md` T-190's acceptance: the landing path, written down.
    ///
    /// `--dir` is the run's output directory, which is the session default,
    /// and it is **not** `AddOptions::output_folder`. So the session's rule
    /// applies to it: a multi-file torrent unpacks into a directory named
    /// after itself and a single-file one lands directly in the directory that
    /// was named. The per-add override belongs to `seed` alone, and
    /// `cmd/seed.rs`'s `either_spelling_of_data_seeds_the_same_payload` pins
    /// that half. Reading a comment is what got this wrong twice, so this
    /// asserts on the directory itself.
    #[test]
    fn dir_lands_a_multi_file_torrent_under_its_own_name_and_a_single_file_one_directly() {
        let fetch = |fixture: &TorrentFixture, out: &std::path::Path| {
            let server = crate::test_support::FileServer::start(fixture.dir());
            let source = format!("{}payload/", server.base);
            let report = crate::test_support::run_json(
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
                    "--stop-after",
                    "30s",
                ],
                fixture.dir(),
            );
            assert_eq!(
                report["torrents"][0]["stopped"], "completed",
                "the payload has to arrive before where it arrived means anything: {report}"
            );
        };

        // The torrent is named `album`, and `--dir` names `out`.
        let multi = TorrentFixture::straddling();
        let multi_out = multi.dir().join("out");
        fetch(&multi, &multi_out);
        assert!(
            multi_out.join("album").join("a.bin").is_file(),
            "a multi-file torrent lands under a directory named after itself"
        );
        assert!(
            !multi_out.join("a.bin").exists(),
            "and never directly in the directory --dir named"
        );

        // The same flag, a torrent with no directory of its own.
        let single = TorrentFixture::single_file();
        let single_out = single.dir().join("out");
        fetch(&single, &single_out);
        assert!(
            single_out.join("payload.bin").is_file(),
            "a single-file torrent lands directly in the directory --dir named"
        );
        assert!(
            !single_out.join("payload.bin").join("payload.bin").exists(),
            "and nothing builds a directory out of its name"
        );
    }

    /// Writing over an existing payload without permission is a disk failure,
    /// not a generic one.
    ///
    /// A caller branches on the exit code, and the fix here is a flag, so the
    /// code has to say "disk" and the message has to name the flag. See
    /// `TODO/disk-io.md`, T-014.
    #[test]
    fn a_download_over_an_existing_file_exits_eight_and_names_the_flag() {
        let fixture = TorrentFixture::multi_file();
        let out = fixture.dir().join("out");
        // The payload is already there, written by something else.
        let existing = out.join("album").join("notes.nfo");
        std::fs::create_dir_all(existing.parent().unwrap()).unwrap();
        std::fs::write(&existing, b"not the payload").unwrap();

        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--web-seed-only",
                "--web-seed",
                "http://127.0.0.1:9/",
                "--no-tracker",
                "--port",
                "0",
                // `--continue` defaults on and means "resume into what is
                // there", so it has to be off for this to be the refusal case.
                "--no-continue",
                "--stop-after",
                "5s",
            ],
            fixture.dir(),
            ExitCode::Disk,
        );
        let torrent = &report["torrents"][0];
        assert_eq!(torrent["code"], "disk", "the per-torrent code says why");
        let message = torrent["error"].as_str().unwrap_or_default();
        assert!(message.contains("already exists"), "{message}");
        assert!(message.contains("--allow-overwrite"), "{message}");
        // Nothing was written over.
        assert_eq!(std::fs::read(&existing).unwrap(), b"not the payload");
    }

    /// `--init-timeout` is a real value that reaches the wait.
    ///
    /// What the deadline does when it fires is asserted in
    /// `webseed_e2e::a_hash_check_that_has_not_finished_names_the_phase_it_is_in`,
    /// which needs a payload large enough that hashing it takes measurable
    /// time. This is the flag half: that it parses, that it defaults, and that
    /// a bad value is refused rather than ignored. See `TODO/disk-io.md`,
    /// T-015.
    #[test]
    fn the_initialisation_deadline_parses_and_a_bad_one_is_refused() {
        use crate::cli::{Cli, Command};
        use clap::Parser;

        let parse = |extra: &[&str]| {
            let mut args = vec!["bit-cli", "download", "a.torrent"];
            args.extend_from_slice(extra);
            let cli = Cli::try_parse_from(args).unwrap();
            let Some(Command::Download(args)) = cli.command else {
                panic!("expected download")
            };
            args.limits.init_timeout
        };
        assert_eq!(parse(&[]), "10m");
        assert_eq!(parse(&["--init-timeout", "45s"]), "45s");

        let fixture = TorrentFixture::multi_file();
        let error = crate::test_support::run_err(
            &[
                "download",
                fixture.path_str(),
                "--init-timeout",
                "not-a-duration",
            ],
            fixture.dir(),
            ExitCode::Usage,
        );
        assert!(error.contains("--init-timeout"), "{error}");
    }

    /// `--continue` is on by default and `--no-continue` turns it off.
    ///
    /// Before this, `--continue` defaulted to true with nothing to set it
    /// false, so the refusal above was unreachable from the command line and
    /// the flag could not do anything.
    #[test]
    fn continue_is_on_by_default_and_no_continue_turns_it_off() {
        use crate::cli::{Cli, Command};
        use clap::Parser;

        let parse = |extra: &[&str]| {
            let mut args = vec!["bit-cli", "download", "a.torrent"];
            args.extend_from_slice(extra);
            let cli = Cli::try_parse_from(args).unwrap();
            let Some(Command::Download(args)) = cli.command else {
                panic!("expected download")
            };
            args.no_continue
        };
        assert!(!parse(&[]), "resuming is the default");
        assert!(parse(&["--no-continue"]));
        assert!(!parse(&["--continue"]));
        // The later flag wins, so a script can append an override.
        assert!(!parse(&["--no-continue", "--continue"]));
        assert!(parse(&["--continue", "--no-continue"]));
    }

    /// A download reports what it cost.
    ///
    /// Measuring a process from outside means sampling one that has already
    /// exited, which reports zero, so the process is the only thing that can
    /// report its own high-water mark. `scripts/bench-webseed.ps1` reads these
    /// three fields.
    #[test]
    fn a_download_reports_its_own_peak_rss_cpu_and_handles() {
        let fixture = TorrentFixture::multi_file();
        let out = fixture.dir().join("out");
        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--web-seed-only",
                "--web-seed",
                "http://127.0.0.1:9/",
                "--no-tracker",
                // An OS-chosen port, so two tests running at once cannot
                // race for the same one.
                "--port",
                "0",
                "--stop-after",
                "1s",
            ],
            fixture.dir(),
            ExitCode::Timeout,
        );
        let process = &report["process"];
        assert!(
            process["peak_rss_bytes"].as_u64().unwrap() > 1024 * 1024,
            "peak RSS of {} is not a running process",
            process["peak_rss_bytes"]
        );
        assert!(process["open_handles"].as_u64().unwrap() > 0);
        assert_eq!(
            process["cpu_ms"].as_u64().unwrap(),
            process["cpu_user_ms"].as_u64().unwrap() + process["cpu_system_ms"].as_u64().unwrap()
        );
        assert!(
            process.get("unavailable").is_none(),
            "some field could not be read: {process}"
        );
    }

    fn walk(root: &std::path::Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Ok(relative) = path.strip_prefix(root) {
                    out.push(
                        relative
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join("/"),
                    );
                }
            }
        }
        out
    }

    /// `TODO/disk-io.md` T-184, and it pins the corrected premise as much as
    /// the fix.
    ///
    /// The entry expected a selection to leave pieces that "can never be
    /// proved". It does not: the unselected half of a boundary piece is
    /// written into the unselected file, so the piece verifies and the session
    /// holds it honestly. What actually happens is that files nobody selected
    /// land on disk, one of them at its **full** length, and before this
    /// nothing said so.
    #[test]
    fn a_selection_reports_the_files_its_boundary_pieces_write_into() {
        let fixture = TorrentFixture::straddling();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let source = format!("{}payload/", server.base);
        let out = fixture.dir().join("out");
        let report = crate::test_support::run_json(
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
        let torrent = &report["torrents"][0];
        assert_eq!(torrent["stopped"], "completed", "{torrent}");

        // Only the two pieces the selection covers were fetched: 1024 + 1024.
        assert_eq!(
            torrent["downloaded"]["bytes"].as_u64().unwrap(),
            2048,
            "a selection fetches the pieces its files touch and no others"
        );

        let partial = torrent["partial"].as_array().expect("a partial array");
        let rows: Vec<(u64, u64, u64, String)> = partial
            .iter()
            .map(|row| {
                (
                    row["bytes"]["bytes"].as_u64().unwrap(),
                    row["on_disk"]["bytes"].as_u64().unwrap(),
                    row["length"]["bytes"].as_u64().unwrap(),
                    row["path"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                (476, 1500, 1500, "a.bin".to_string()),
                (872, 872, 1500, "c.bin".to_string()),
            ],
            "the report has to name both unselected files and say how much of each is real"
        );

        // And the disk agrees with the report, which is the half that would
        // otherwise be a claim about arithmetic rather than about behaviour.
        let base = out.join("album");
        assert_eq!(
            std::fs::metadata(base.join("a.bin")).unwrap().len(),
            1500,
            "a.bin lands at its full length while holding 476 real bytes, which is why this is reported at all"
        );
        assert_eq!(std::fs::metadata(base.join("c.bin")).unwrap().len(), 872);
        let selected = std::fs::read(base.join("b.bin")).unwrap();
        assert_eq!(selected, vec![0xB2u8; 700], "the selected file is whole");
    }

    /// `TODO/cli-surface.md` T-185's acceptance.
    ///
    /// `--exclude-file` with no `--select-file` used to resolve to `None`,
    /// every file, so the flag skipped nothing and the run fetched the file it
    /// had been told to skip. The donor fixture is `extra-a.txt` (1024 bytes)
    /// and `shared.bin` (4096) at a 1024 byte piece length, so every file is a
    /// whole number of pieces and nothing straddles: what lands on disk is
    /// exactly what was selected. `create` sorts by path, so index 0 is
    /// `extra-a.txt` and index 1 is `shared.bin`.
    ///
    /// The mirror's request log is the half that says the exclusion was
    /// applied **before** the fetch rather than after it.
    #[test]
    fn an_exclusion_with_no_selection_skips_the_file_and_never_asks_for_it() {
        let (fixture, _receiver) = TorrentFixture::sharing_pair();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let source = format!("{}payload/", server.base);
        let out = fixture.dir().join("out");
        let report = crate::test_support::run_json(
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
                "--exclude-file",
                "1",
                "--stop-after",
                "30s",
            ],
            fixture.dir(),
        );
        let torrent = &report["torrents"][0];
        assert_eq!(torrent["stopped"], "completed", "{torrent}");
        assert_eq!(
            torrent["downloaded"]["bytes"].as_u64().unwrap(),
            1024,
            "only the excluded file's complement is fetched, which is one piece"
        );

        let base = out.join("donor");
        assert_eq!(
            std::fs::read(base.join("extra-a.txt")).unwrap(),
            vec![0x11u8; 1024],
            "the file that was not excluded is whole"
        );
        assert!(
            !base.join("shared.bin").exists(),
            "the excluded file is not created: {:?}",
            walk(&out)
        );

        let asked = server.asked();
        assert!(
            asked.iter().any(|path| path.contains("extra-a.txt")),
            "the mirror served the file that was kept: {asked:?}"
        );
        assert!(
            !asked.iter().any(|path| path.contains("shared.bin")),
            "the mirror was asked for the excluded file: {asked:?}"
        );
    }

    /// The magnet half of `TODO/cli-surface.md` T-185.
    ///
    /// A magnet has no file list until its metadata resolves, which is why the
    /// exclusion was left unapplied in the first place. The answer is to read
    /// the metadata before the add rather than narrow the selection after it:
    /// `librqbit`'s initial check creates and opens every file it was not told
    /// to skip, so a selection applied afterwards has already created what it
    /// excludes. The `.torrent` bytes the resolution builds are what the add
    /// then uses, so this is one metadata resolution and not two.
    ///
    /// A seeder on loopback and `--peer` pointed at it is the smallest swarm
    /// there is, and it is the same shape `cmd::peers`'s test uses. See
    /// `TODO/peers.md`, T-142 for why the listener is waited on.
    #[test]
    fn a_magnet_resolves_its_metadata_before_it_applies_an_exclusion() {
        let (fixture, _receiver) = TorrentFixture::sharing_pair();
        let dir = fixture.dir();
        // The seeder needs the payload under the torrent's own name. The
        // fixture keeps it under `payload/`.
        let data = dir.join("seeded");
        fixture.place(&data, &[]);

        let port = crate::test_support::free_port();
        let seeder = {
            let torrent = fixture.path_str().to_string();
            let data = data.to_str().expect("utf-8 path").to_string();
            let cwd = dir.clone();
            std::thread::spawn(move || {
                let (mut env, _) = crate::env::Env::test(
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
                        "90s",
                    ],
                    cwd,
                );
                crate::run(&mut env)
            })
        };
        assert!(
            crate::test_support::wait_for_listener(port, std::time::Duration::from_secs(10)),
            "the seeder never listened on {port}"
        );

        let magnet = format!("magnet:?xt=urn:btih:{}", fixture.info_hash);
        let out = dir.join("out");
        // `--stop-after` ends the run the moment it completes, so this is an
        // upper bound on failure rather than a duration waited out.
        // `--init-timeout` bounds the metadata resolution, which is otherwise
        // unbounded for a magnet, so a swarm that never answers reports why
        // instead of hanging the test binary.
        let report = crate::test_support::run_json(
            &[
                "download",
                &magnet,
                "--dir",
                out.to_str().unwrap(),
                "--peer",
                &format!("127.0.0.1:{port}"),
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "--exclude-file",
                "1",
                "--init-timeout",
                "60s",
                "--stop-after",
                "60s",
            ],
            dir.clone(),
        );
        drop(seeder);

        let torrent = &report["torrents"][0];
        assert_eq!(torrent["stopped"], "completed", "{torrent}");
        let base = out.join("donor");
        assert_eq!(
            std::fs::read(base.join("extra-a.txt")).unwrap(),
            vec![0x11u8; 1024],
            "the file that was not excluded is whole"
        );
        assert!(
            !base.join("shared.bin").exists(),
            "the excluded file is neither fetched nor created: {:?}",
            walk(&out)
        );
    }

    /// `TODO/disk-io.md` T-188's acceptance.
    ///
    /// A selection that starts after file 0, on a torrent whose file 0 ends
    /// exactly on a piece boundary, left file 0 on disk at zero bytes.
    /// `librqbit` issues a zero length write to the file before a chunk that
    /// begins on a boundary, and a write is what creates a file. Nothing
    /// spilled, so [T-184](disk-io.md)'s `partial` array is empty and this is
    /// not that: it is a file with no bytes in it and no bytes owed to it.
    ///
    /// The donor fixture is `extra-a.txt` at 1,024 bytes and `shared.bin` at
    /// 4,096, at a 1,024 byte piece length, so file 0 is exactly piece 0 and
    /// piece 1 starts on the boundary.
    #[test]
    fn a_selection_that_starts_after_file_zero_leaves_it_off_the_disk() {
        let (fixture, _receiver) = TorrentFixture::sharing_pair();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let source = format!("{}payload/", server.base);
        let out = fixture.dir().join("out");
        let report = crate::test_support::run_json(
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
        let torrent = &report["torrents"][0];
        assert_eq!(torrent["stopped"], "completed", "{torrent}");
        assert_eq!(torrent["downloaded"]["bytes"].as_u64().unwrap(), 4096);
        assert!(
            torrent.get("partial").is_none(),
            "nothing spilled, so this is not T-184: {torrent}"
        );

        let base = out.join("donor");
        assert_eq!(
            std::fs::read(base.join("shared.bin")).unwrap(),
            vec![0x5Au8; 4096],
            "the selected file is whole"
        );
        assert!(
            !base.join("extra-a.txt").exists(),
            "the unselected file before the selection was created anyway: {:?}",
            walk(&out)
        );
    }

    /// The same run without a selection reports nothing, so the field is not
    /// noise on every download.
    #[test]
    fn a_download_with_no_selection_reports_no_partial_files() {
        let fixture = TorrentFixture::straddling();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let source = format!("{}payload/", server.base);
        let out = fixture.dir().join("out");
        let report = crate::test_support::run_json(
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
                "--stop-after",
                "30s",
            ],
            fixture.dir(),
        );
        let torrent = &report["torrents"][0];
        assert_eq!(torrent["stopped"], "completed", "{torrent}");
        assert!(
            torrent.get("partial").is_none(),
            "an unselected run has no partial files: {torrent}"
        );
    }

    fn selection_args(select: &[&str], exclude: &[&str]) -> SelectionArgs {
        SelectionArgs {
            select_file: select.iter().map(ToString::to_string).collect(),
            exclude_file: exclude.iter().map(ToString::to_string).collect(),
            ..Default::default()
        }
    }

    /// The parsing itself is `crate::selection`'s and tested there. What this
    /// pins is the one thing that belongs to `download`: it resolves against
    /// the count of the torrent in hand, and an exclusion with no selection
    /// beside it is that torrent's complement rather than every file. See
    /// `TODO/cli-surface.md`, T-185.
    #[test]
    fn download_resolves_a_selection_against_the_file_count() {
        assert_eq!(selection(&selection_args(&[], &[]), Some(4)).unwrap(), None);
        assert_eq!(
            selection(&selection_args(&["1-3"], &["2"]), Some(4)).unwrap(),
            Some(vec![1, 3])
        );
        assert_eq!(
            selection(&selection_args(&[], &["1"]), Some(4)).unwrap(),
            Some(vec![0, 2, 3])
        );
        assert_eq!(
            selection(&selection_args(&["1-"], &[]), Some(4)).unwrap(),
            Some(vec![1, 2, 3])
        );
    }

    /// A source whose metadata this run parsed settles its selection before
    /// the session starts, and the exclusion is applied there rather than
    /// nowhere. The two-file fixture makes the answer readable: index 0 is
    /// `disc 1/a.flac` and index 1 is `notes.nfo`.
    #[test]
    fn a_torrent_on_disk_settles_its_selection_before_anything_is_added() {
        let fixture = TorrentFixture::multi_file();
        let meta = Metainfo::read(std::path::Path::new(fixture.path_str())).expect("the fixture");
        let decided = |select: &[&str], exclude: &[&str]| {
            plan_selection(&selection_args(select, exclude), Some(&meta)).unwrap()
        };
        assert_eq!(decided(&[], &[]), FileSelection::Decided(None));
        assert_eq!(
            decided(&[], &["1"]),
            FileSelection::Decided(Some(vec![0])),
            "an exclusion alone is the complement, which is T-185"
        );
        assert_eq!(decided(&["1-"], &[]), FileSelection::Decided(Some(vec![1])));
        assert_eq!(decided(&["0"], &[]), FileSelection::Decided(Some(vec![0])));
        // Excluding every file it has is a usage error, and it is raised here
        // rather than after the session is up.
        let err = plan_selection(&selection_args(&[], &["0-1"]), Some(&meta)).unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
    }

    /// A magnet waits for its file count only when the flags need one, so the
    /// common case still adds without a round trip for metadata it does not
    /// have to read twice.
    #[test]
    fn a_magnet_waits_for_its_file_count_only_when_the_flags_need_one() {
        let decided = |select: &[&str], exclude: &[&str]| {
            plan_selection(&selection_args(select, exclude), None).unwrap()
        };
        assert_eq!(decided(&[], &[]), FileSelection::Decided(None));
        assert_eq!(
            decided(&["0", "2"], &[]),
            FileSelection::Decided(Some(vec![0, 2]))
        );
        assert_eq!(
            decided(&["1-3"], &["2"]),
            FileSelection::Decided(Some(vec![1, 3]))
        );
        assert_eq!(decided(&[], &["1"]), FileSelection::AwaitingCount);
        assert_eq!(decided(&["3-"], &[]), FileSelection::AwaitingCount);
    }

    #[test]
    fn the_whole_run_request_budget_is_shared_across_sources() {
        let specs: Vec<SourceSpec> = (0..4)
            .map(|i| {
                SourceSpec::new(
                    format!("https://m{i}.example.com/"),
                    bit_cli_core::webseed::Origin::CommandLine,
                )
            })
            .collect();
        // Default per-source concurrency is 4, so four sources want 16.
        let limited = apply_max_total(&specs, Some(8), specs.len());
        assert_eq!(limited.len(), 4);
        for spec in &limited {
            assert_eq!(spec.limits.concurrency, 2);
        }
    }

    #[test]
    fn a_budget_smaller_than_the_source_count_still_leaves_one_request_each() {
        let specs: Vec<SourceSpec> = (0..4)
            .map(|i| {
                SourceSpec::new(
                    format!("https://m{i}.example.com/"),
                    bit_cli_core::webseed::Origin::CommandLine,
                )
            })
            .collect();
        for spec in apply_max_total(&specs, Some(2), specs.len()) {
            assert_eq!(
                spec.limits.concurrency, 1,
                "a source with no budget cannot serve"
            );
        }
    }

    #[test]
    fn no_budget_leaves_the_per_source_setting_alone() {
        let specs = vec![SourceSpec::new(
            "https://m.example.com/",
            bit_cli_core::webseed::Origin::CommandLine,
        )];
        assert_eq!(apply_max_total(&specs, None, 1)[0].limits.concurrency, 4);
        assert_eq!(apply_max_total(&specs, Some(0), 1)[0].limits.concurrency, 4);
    }

    /// A source that will attach later counts against the budget now, so the
    /// share a running bridge holds is the share it keeps. Attaching one late
    /// must never mean taking requests back off a mirror already serving. See
    /// `TODO/multi-source.md`, T-143.
    #[test]
    fn a_source_that_has_not_attached_yet_still_takes_its_share_of_the_budget() {
        let specs: Vec<SourceSpec> = (0..2)
            .map(|i| {
                SourceSpec::new(
                    format!("https://m{i}.example.com/"),
                    bit_cli_core::webseed::Origin::CommandLine,
                )
            })
            .collect();
        // Two attached now and two donations still waiting: the budget of
        // eight is divided by four, not by two.
        for spec in apply_max_total(&specs, Some(8), 4) {
            assert_eq!(spec.limits.concurrency, 2);
        }
        // And the one that arrives later, shaped on its own against the same
        // divisor, gets the same share rather than the whole budget.
        let late = apply_max_total(&specs[..1], Some(8), 4);
        assert_eq!(late[0].limits.concurrency, 2);
    }

    /// The window count against the chunk size, and what it costs.
    ///
    /// This test was called `the_window_cache_stays_inside_its_memory_budget`
    /// and its middle case asserted the run where it does not: two windows of
    /// 64 MiB is 128 MiB against a per-source budget of 16. The floor of two
    /// is right and the name was wrong, so the name went and the cost the
    /// floor produces is asserted beside the count. See `TODO/memory.md`,
    /// T-041.
    #[test]
    fn the_window_count_falls_as_the_chunk_size_rises_until_the_floor() {
        let spec_at = |chunk: u64| {
            let mut spec = SourceSpec::new(
                "https://m.example.com/",
                bit_cli_core::webseed::Origin::CommandLine,
            );
            spec.limits.chunk_size = chunk;
            spec
        };

        let default = spec_at(4 * bit_cli_core::units::MIB);
        assert_eq!(cache_windows(std::slice::from_ref(&default)), 4);
        assert_eq!(
            cache_budget(std::slice::from_ref(&default)).2,
            CACHE_BUDGET_PER_SOURCE,
            "the ordinary chunk size spends exactly the budget"
        );

        let huge = spec_at(64 * bit_cli_core::units::MIB);
        assert_eq!(
            cache_windows(std::slice::from_ref(&huge)),
            2,
            "never below two windows"
        );
        assert_eq!(
            cache_budget(std::slice::from_ref(&huge)).2,
            8 * CACHE_BUDGET_PER_SOURCE,
            "and the floor is what puts one source eight times over it"
        );

        let tiny = spec_at(64 * bit_cli_core::units::KIB);
        assert_eq!(
            cache_windows(std::slice::from_ref(&tiny)),
            16,
            "and never above sixteen"
        );
        assert!(
            cache_budget(std::slice::from_ref(&tiny)).2 < CACHE_BUDGET_PER_SOURCE,
            "a small chunk size runs under the budget rather than over it"
        );
    }

    /// The total is what a caller cannot work out from one source, and it is
    /// where the warning comes from. T-041.
    #[test]
    fn the_total_budget_is_the_sum_and_the_warning_names_the_flag() {
        let specs = |count: usize, chunk: u64| -> Vec<SourceSpec> {
            (0..count)
                .map(|index| {
                    let mut spec = SourceSpec::new(
                        format!("https://m{index}.example.com/"),
                        bit_cli_core::webseed::Origin::CommandLine,
                    );
                    spec.limits.chunk_size = chunk;
                    spec
                })
                .collect()
        };

        // Sixteen mirrors at the default chunk size is the largest total the
        // ordinary case reaches, and it is the ceiling, so it does not warn.
        let ordinary = specs(16, 4 * bit_cli_core::units::MIB);
        assert_eq!(cache_budget(&ordinary).2, CACHE_TOTAL_WARN);
        assert!(cache_budget_warning(&ordinary).is_none());

        // The entry was filed on ten sources at 64 MiB and put the figure at
        // 640 MiB, which assumes one window each. The floor is two, so it is
        // 1.25 GiB, and that correction is why this asserts the number rather
        // than only that something warned.
        let chosen = specs(10, 64 * bit_cli_core::units::MIB);
        assert_eq!(cache_budget(&chosen).0, 2);
        assert_eq!(cache_budget(&chosen).2, 20 * 64 * bit_cli_core::units::MIB);
        let said = cache_budget_warning(&chosen).expect("a warning");
        assert!(said.contains("1.25 GiB"), "{said}");
        assert!(said.contains("--web-seed-chunk-size"), "{said}");
        assert!(said.contains("10 source(s)"), "{said}");

        // And nothing warns about nothing.
        assert!(cache_budget_warning(&[]).is_none());
    }

    /// A run that stalls with `--redial-after` set throws its peer state away
    /// and says so, rather than waiting out a backoff that grows by six.
    ///
    /// Nothing answers here, so every re-dial fires and none of them helps.
    /// That is the point: what is under test is that the flag reaches the
    /// watch loop, that the cap holds, and that the report carries both
    /// numbers T-138's acceptance asks for. Whether a re-dial recovers a real
    /// outage is `scripts/check-peer-recovery.ps1`, which measures it against
    /// a seeder that comes back. See `TODO/peers.md`, T-138.
    #[test]
    fn a_stalled_run_redials_up_to_the_cap_and_reports_each_one() {
        let fixture = TorrentFixture::single_file();
        let out = fixture.dir().join("out");
        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "--report-interval",
                "200ms",
                "--redial-after",
                "500ms",
                "--max-redials",
                "2",
                "--stop-after",
                "4s",
            ],
            fixture.dir(),
            // Nothing serves the payload, so the run ends on its deadline.
            ExitCode::Timeout,
        );
        let redials = report["torrents"][0]["redials"]
            .as_array()
            .expect("a redials array");
        assert_eq!(redials.len(), 2, "--max-redials 2 is a cap: {redials:?}");
        assert_eq!(redials[0]["attempt"], 1);
        assert_eq!(redials[1]["attempt"], 2);
        for redial in redials {
            assert!(
                redial["stalled_ms"].as_u64().unwrap_or(0) >= 500,
                "a re-dial fired before --redial-after elapsed: {redial}"
            );
            assert!(redial["error"].is_null(), "a re-dial failed: {redial}");
        }
        // The second waits out the interval again rather than firing on the
        // next report tick.
        let first = redials[0]["at_ms"].as_u64().unwrap();
        let second = redials[1]["at_ms"].as_u64().unwrap();
        assert!(
            second >= first + 400,
            "re-dials {first}ms and {second}ms apart, under --redial-after"
        );
    }

    /// With the flag off, nothing re-dials and the report says nothing.
    #[test]
    fn a_stalled_run_without_the_flag_never_redials() {
        let fixture = TorrentFixture::single_file();
        let out = fixture.dir().join("out");
        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "--report-interval",
                "200ms",
                "--stop-after",
                "2s",
            ],
            fixture.dir(),
            ExitCode::Timeout,
        );
        assert!(
            report["torrents"][0]["redials"].is_null(),
            "an empty array is not serialised: {}",
            report["torrents"][0]
        );
    }

    /// `-j 1` runs the sources in the order they were given.
    ///
    /// A torrent whose source is a file an earlier torrent writes needs the
    /// earlier one to have finished, which only holds if the order is the
    /// caller's rather than the scheduler's. Before the plans became a queue
    /// taken by a fixed pool, every plan was its own task queuing on a
    /// semaphore, and which task reached the semaphore first was up to the
    /// runtime. See `TODO/multi-source.md`, T-133.
    #[test]
    fn sources_start_in_the_order_they_were_given() {
        let first = TorrentFixture::single_file();
        let second = TorrentFixture::multi_file();
        let out = first.dir().join("out");

        let (mut env, captured) = crate::env::Env::test(
            &[
                "--jsonl",
                "download",
                first.path_str(),
                second.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "-j",
                "1",
                "--report-interval",
                "200ms",
                "--stop-after",
                "1s",
            ],
            first.dir(),
        );
        let _ = crate::run(&mut env);
        let events = captured.jsonl().expect("stdout was not ndjson");
        let added: Vec<String> = events
            .iter()
            .filter(|event| event["type"] == "torrent_added")
            .filter_map(|event| event["info_hash"].as_str().map(str::to_string))
            .collect();
        assert_eq!(
            added,
            [first.info_hash.clone(), second.info_hash.clone()],
            "torrents started out of order: {added:?}"
        );
    }

    /// One torrent's report, by its name.
    ///
    /// The list is sorted by source path and two fixtures live in two
    /// temporary directories, so position says nothing about order.
    fn by_name<'a>(report: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        report["torrents"]
            .as_array()
            .expect("a torrent array")
            .iter()
            .find(|torrent| torrent["name"] == name)
            .unwrap_or_else(|| panic!("no torrent named {name} in {report}"))
    }

    /// A file one torrent holds is read from it by the next, with no flag.
    ///
    /// The donor is complete on disk, so it finishes on its hash check with no
    /// source at all. The receiver has everything except the shared file and
    /// no source at all either, so the only way it can finish is by reading
    /// the donor's copy. See `TODO/multi-source.md`, T-140.
    #[test]
    fn a_proven_shared_file_is_read_from_the_torrent_that_holds_it() {
        let (donor, receiver) = TorrentFixture::sharing_pair();
        let out = donor.dir().join("out");
        donor.place(&out, &[]);
        receiver.place(&out, &["extra-b.txt"]);

        let report = run_json_code(
            &[
                "download",
                donor.path_str(),
                receiver.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-torrent-web-seed",
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "-j",
                "1",
                "--report-interval",
                "100ms",
                "--stop-after",
                "20s",
            ],
            donor.dir(),
            ExitCode::Success,
        );

        // Sorted by source path, and the two fixtures are in different temp
        // directories, so the order of the two reports is not the order they
        // ran in. Find it by name.
        let taken = by_name(&report, "receiver");
        assert_eq!(taken["finished"], true, "{taken}");
        let shared = taken["shared"].as_array().expect("a shared array");
        assert_eq!(shared.len(), 1, "{taken}");
        assert_eq!(shared[0]["path"], "shared.bin", "{taken}");
        assert_eq!(shared[0]["from_info_hash"], donor.info_hash, "{taken}");
        assert_eq!(shared[0]["from_index"], 1, "{taken}");
        // Four whole 1 KiB pieces lie entirely inside the 4 KiB file, and all
        // four hashes agree. Nothing is asserted by the caller.
        assert_eq!(shared[0]["pieces_compared"], 4, "{taken}");
        assert_eq!(shared[0]["bytes_proven"]["bytes"], 4096, "{taken}");

        // The whole shared file came from the donor's copy, and the rest was
        // already on disk. No peer served anything: there was no swarm.
        assert_eq!(taken["from_web_seeds"]["bytes"], 4096, "{taken}");
        assert_eq!(taken["from_resume"]["bytes"], 2048, "{taken}");
        assert_eq!(taken["from_peers"]["bytes"], 0, "{taken}");
        assert_eq!(taken["sources"][0]["origin"], "shared_file", "{taken}");
        assert_eq!(taken["sources"][0]["scope"], "file:1", "{taken}");

        // Same bytes in both output directories.
        let from_donor = std::fs::read(out.join("donor").join("shared.bin")).expect("donor file");
        let landed = std::fs::read(out.join("receiver").join("shared.bin")).expect("receiver file");
        assert_eq!(from_donor, landed);
    }

    /// The same thing above `-j 1`, where the donor finishes while the
    /// receiver is already running.
    ///
    /// `TODO/multi-source.md` T-143. The donor's payload is **not** on disk:
    /// it is fetched from a mirror bound to the donor's info hash alone, so
    /// the donor cannot have finished at the moment the receiver resolves its
    /// donations, and the receiver starts with no source at all. Before this,
    /// that was the end of it: the receiver had nothing to fetch from and ran
    /// to `--stop-after` unfinished, which is what
    /// `scripts/check-shared-files.ps1 -Jobs 3` measured.
    ///
    /// The receiver taking exactly the shared file's 4,096 bytes from a source
    /// while its own HTTP traffic stays zero is the whole assertion: the
    /// mirror is not its to reach, so those bytes can only have come off the
    /// donor's disk, and the source that served them did not exist when the
    /// receiver started.
    #[test]
    fn a_donated_file_attaches_to_a_torrent_that_has_already_started() {
        let (donor, receiver) = TorrentFixture::sharing_pair();
        let server = crate::test_support::FileServer::start(donor.dir());
        let mirror = format!("{}payload/", server.base);
        let out = donor.dir().join("out");
        receiver.place(&out, &["extra-b.txt"]);

        let report = run_json_code(
            &[
                "download",
                donor.path_str(),
                receiver.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--web-seed-for",
                &format!("{}:*={mirror}", donor.info_hash),
                "--web-seed-mode",
                "prefix",
                "--no-torrent-web-seed",
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "-j",
                "2",
                "--report-interval",
                "100ms",
                "--stop-after",
                "30s",
            ],
            donor.dir(),
            ExitCode::Success,
        );

        let gave = by_name(&report, "donor");
        assert_eq!(gave["finished"], true, "{gave}");
        assert_eq!(
            gave["from_web_seeds"]["bytes"], 5120,
            "the donor fetched its whole payload from the mirror: {gave}"
        );

        let taken = by_name(&report, "receiver");
        assert_eq!(taken["finished"], true, "{taken}");
        let shared = taken["shared"].as_array().expect("a shared array");
        assert_eq!(shared.len(), 1, "{taken}");
        assert_eq!(shared[0]["path"], "shared.bin", "{taken}");
        assert_eq!(shared[0]["from_info_hash"], donor.info_hash, "{taken}");
        assert_eq!(shared[0]["bytes_proven"]["bytes"], 4096, "{taken}");
        assert_eq!(taken["from_web_seeds"]["bytes"], 4096, "{taken}");
        assert_eq!(taken["from_resume"]["bytes"], 2048, "{taken}");
        assert_eq!(taken["from_peers"]["bytes"], 0, "{taken}");
        assert_eq!(taken["sources"][0]["origin"], "shared_file", "{taken}");

        // The mirror is the donor's alone, so what the receiver holds cannot
        // have come from it. Two requests for the donor's two files and
        // nothing for the receiver's directory.
        let asked = server.asked();
        assert!(
            !asked.iter().any(|path| path.contains("extra-b")),
            "the receiver reached a mirror it was not given: {asked:?}"
        );

        let from_donor = std::fs::read(out.join("donor").join("shared.bin")).expect("donor file");
        let landed = std::fs::read(out.join("receiver").join("shared.bin")).expect("receiver file");
        assert_eq!(from_donor, landed);
    }

    /// `--no-share-files` turns it off, and then the same run cannot finish.
    ///
    /// A flag that does not move a number does not ship. The number here is
    /// the receiver's completion: with sharing on it finishes from the donor's
    /// copy, and with it off there is no source for the shared file at all.
    #[test]
    fn no_share_files_leaves_the_receiver_with_nothing_to_fetch_from() {
        let (donor, receiver) = TorrentFixture::sharing_pair();
        let out = donor.dir().join("out");
        donor.place(&out, &[]);
        receiver.place(&out, &["extra-b.txt"]);

        let report = run_json_code(
            &[
                "download",
                donor.path_str(),
                receiver.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-torrent-web-seed",
                "--no-share-files",
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "-j",
                "1",
                "--report-interval",
                "100ms",
                "--stop-after",
                "2s",
            ],
            donor.dir(),
            ExitCode::Timeout,
        );
        let taken = by_name(&report, "receiver");
        assert_eq!(taken["finished"], false, "{taken}");
        assert!(taken["shared"].is_null(), "{taken}");
        assert_eq!(taken["from_web_seeds"]["bytes"], 0, "{taken}");
    }

    /// Bytes that were already on the disk are not charged to peers.
    ///
    /// `progress_bytes` is everything the torrent has, not everything this run
    /// fetched, so `progress_bytes - served` charges a resumed download's
    /// existing bytes to the swarm. This run has no peers and no sources at
    /// all, and the payload is already complete, so anything non-zero in
    /// `from_peers` is that arithmetic and nothing else. See
    /// `TODO/multi-source.md`, T-139.
    #[test]
    fn bytes_already_on_disk_are_reported_as_resumed_rather_than_from_peers() {
        let fixture = TorrentFixture::single_file();
        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                fixture.payload_dir().to_str().unwrap(),
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "--allow-overwrite",
                "--stop-after",
                "20s",
            ],
            fixture.dir(),
            // The payload is already there, so the hash check finds it
            // complete and the run finishes at once.
            ExitCode::Success,
        );
        let torrent = &report["torrents"][0];
        assert_eq!(torrent["downloaded"]["bytes"], 3000);
        assert_eq!(torrent["from_resume"]["bytes"], 3000, "{torrent}");
        assert_eq!(torrent["from_peers"]["bytes"], 0, "{torrent}");
        assert_eq!(torrent["from_web_seeds"]["bytes"], 0, "{torrent}");
        assert_eq!(report["from_resume"]["bytes"], 3000);
    }

    /// A whole run tells its trackers when it started, when it finished, and
    /// when it stopped.
    ///
    /// The session sends `started` and then repeats on the interval. It never
    /// says a download completed, so a tracker's seeder count is wrong, and it
    /// never says stopped, so a dead address is handed out until the record
    /// expires. Both are sent by `bit-cli` itself, from the session's own peer
    /// id and port, so the tracker updates one record rather than seeing two
    /// peers. See `TODO/trackers.md`, T-062.
    ///
    /// The payload is fetched rather than already on disk. A torrent that is
    /// complete on its hash check finishes before the session's own `started`
    /// announce has left, and the order the tracker sees is then a race rather
    /// than a sequence.
    /// `TODO/cli-surface.md` T-183. `--web-seed-list-url` is fetched over
    /// loopback HTTP and the sources it names are used.
    ///
    /// The flag parsed and was read, and every call site handed the reader a
    /// function that refuses, so it could only ever fail. That is why the flag
    /// audit that found T-181 missed it: it looked for a field nothing reads,
    /// and this one is read.
    #[test]
    fn a_web_seed_list_url_is_fetched_and_its_sources_are_used() {
        let fixture = TorrentFixture::multi_file();
        let server = crate::test_support::FileServer::start(fixture.dir());
        std::fs::write(
            fixture.dir().join("mirrors.txt"),
            format!(
                "# the mirror list
{}payload/
",
                server.base
            ),
        )
        .unwrap();

        let out = fixture.dir().join("out");
        let list_url = format!("{}mirrors.txt", server.base);

        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-torrent-web-seed",
                "--web-seed-list-url",
                &list_url,
                "--web-seed-mode",
                "prefix",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--port",
                "0",
                "--report-interval",
                "100ms",
                "--stop-after",
                "20s",
            ],
            fixture.dir(),
            ExitCode::Success,
        );

        let torrent = &report["torrents"][0];
        assert_eq!(torrent["finished"], true, "{report}");
        let sources = torrent["sources"].as_array().expect("a sources array");
        assert_eq!(sources.len(), 1, "{report}");
        assert_eq!(sources[0]["origin"], "list_url", "{report}");
        assert_eq!(
            sources[0]["served_bytes"], 2000,
            "the fetched source has to have served the whole payload: {report}"
        );
    }

    /// `TODO/cli-surface.md` T-181. `--tracker-list-url` is fetched over
    /// loopback HTTP and every tracker it names is announced to.
    ///
    /// Three trackers rather than one, because the failure this guards against
    /// is a list that is read and then partly dropped, and one tracker cannot
    /// tell a whole list from the first line of one. Each tracker records what
    /// it was asked, so the proof is on the tracker's side rather than in a
    /// count the run reports about itself.
    #[test]
    fn a_tracker_list_url_is_fetched_and_every_tracker_in_it_is_announced_to() {
        let fixture = TorrentFixture::multi_file();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let trackers = [
            crate::test_support::Tracker::start(&[]),
            crate::test_support::Tracker::start(&[]),
            crate::test_support::Tracker::start(&[]),
        ];
        std::fs::write(
            fixture.dir().join("trackers.txt"),
            format!(
                "# the mirror list
{}

{}
{}
",
                trackers[0].announce, trackers[1].announce, trackers[2].announce
            ),
        )
        .unwrap();

        let out = fixture.dir().join("out");
        let source = format!("{}payload/", server.base);
        let list_url = format!("{}trackers.txt", server.base);

        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-torrent-web-seed",
                "--web-seed",
                &source,
                "--web-seed-mode",
                "prefix",
                "--replace-trackers",
                "--tracker-list-url",
                &list_url,
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "--report-interval",
                "100ms",
                "--stop-after",
                "20s",
            ],
            fixture.dir(),
            ExitCode::Success,
        );

        for (index, tracker) in trackers.iter().enumerate() {
            assert!(
                !tracker.seen().is_empty(),
                "tracker {index} was never announced to, so the fetched list did not reach the session: {report}"
            );
        }

        let announced = report["torrents"][0]["announced"]
            .as_array()
            .expect("an announced array");
        assert!(
            announced.iter().any(|sent| sent["trackers"] == 3),
            "the report has to say three trackers were announced to: {report}"
        );
    }

    #[test]
    fn a_run_announces_started_then_completed_then_stopped() {
        let fixture = TorrentFixture::multi_file();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let tracker = crate::test_support::Tracker::start(&[]);
        let out = fixture.dir().join("out");
        let source = format!("{}payload/", server.base);

        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-torrent-web-seed",
                "--web-seed",
                &source,
                "--web-seed-mode",
                "prefix",
                "--replace-trackers",
                "--tracker",
                &tracker.announce,
                "--no-dht",
                "--no-lsd",
                "--port",
                "0",
                "--report-interval",
                "100ms",
                "--stop-after",
                "20s",
            ],
            fixture.dir(),
            ExitCode::Success,
        );

        let announced = report["torrents"][0]["announced"]
            .as_array()
            .expect("an announced array");
        let events: Vec<&str> = announced
            .iter()
            .filter_map(|sent| sent["event"].as_str())
            .collect();
        assert_eq!(events, ["completed", "stopped"], "{report}");
        for sent in announced {
            assert_eq!(sent["trackers"], 1, "{sent}");
            assert_eq!(sent["accepted"], 1, "{sent}");
        }

        // What the tracker actually saw, in order. `started` is the session's
        // own; the other two are this run's.
        assert_eq!(
            tracker.param("event"),
            ["started", "completed", "stopped"],
            "{:?}",
            tracker.seen()
        );

        // One peer id and one port throughout, which is what makes these
        // updates to the session's record rather than a second peer.
        let ids: std::collections::HashSet<String> = tracker.param("peer_id").into_iter().collect();
        assert_eq!(ids.len(), 1, "{:?}", tracker.seen());
        let ports: std::collections::HashSet<String> = tracker.param("port").into_iter().collect();
        assert_eq!(ports.len(), 1, "{:?}", tracker.seen());
    }

    /// A payload path past the classic Windows limit lands and verifies.
    ///
    /// The download directory plus this torrent's deepest path is over 300
    /// characters, which is past the 260 the `MAX_PATH` era allows. Nothing
    /// here adds an extended-length prefix: it is a test of whether the tool
    /// needs one. See `TODO/windows.md`, T-073.
    #[test]
    fn a_path_past_the_classic_windows_limit_lands_and_verifies() {
        let fixture = TorrentFixture::deep();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let out = fixture.dir().join("out");
        let source = format!("{}payload/", server.base);

        let landed = out.join("deep").join(&fixture.files[0].0);
        assert!(
            landed.to_string_lossy().chars().count() > 300,
            "the fixture is not long enough to test anything: {}",
            landed.display()
        );

        let report = run_json_code(
            &[
                "download",
                fixture.path_str(),
                "--dir",
                out.to_str().unwrap(),
                "--no-torrent-web-seed",
                "--web-seed",
                &source,
                "--web-seed-mode",
                "prefix",
                "--web-seed-only",
                "--port",
                "0",
                "--report-interval",
                "100ms",
                "--stop-after",
                "20s",
            ],
            fixture.dir(),
            ExitCode::Success,
        );
        assert_eq!(report["torrents"][0]["finished"], true, "{report}");
        assert!(
            report["torrents"][0]["renamed"].is_null(),
            "a long path was rewritten rather than written: {report}"
        );
        assert_eq!(
            std::fs::read(&landed).expect("the payload is not where it was planned"),
            fixture.files[0].1
        );

        // And the hash check reads it back from the same path.
        let verified = run_json_code(
            &["verify", fixture.path_str(), "--dir", out.to_str().unwrap()],
            fixture.dir(),
            ExitCode::Success,
        );
        assert_eq!(verified["complete"], true, "{verified}");
    }
}
