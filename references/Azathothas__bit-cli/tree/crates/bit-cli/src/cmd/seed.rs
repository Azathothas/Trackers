//! `bit-cli seed`: serve existing data in the foreground.
//!
//! Seeding is a peer of downloading, not a mode of it. The question this
//! command exists to answer is "is my server actually serving", so the report
//! is per-peer: who connected, how they connected, what they took, and how
//! fast. Aggregate totals alone cannot answer it.
//!
//! It runs until a stop condition is met and then exits with a code naming
//! which one. There is no daemon.

use std::collections::HashSet;
use std::time::Duration;

use bit_cli_core::ExitCode;
use bit_cli_core::engine::{AddOptions, Engine, PeerSnapshot, TorrentSnapshot};
use bit_cli_core::error::{Error, Result};
use bit_cli_core::units::{Size, format_rate, format_size};
use serde::Serialize;
use serde_json::json;

use crate::cli::{Global, SeedArgs, SeedVerify};
use crate::env::Env;
use crate::output::{Renderer, field};
use crate::source::Kind;
use crate::swarm::{self, Progress, SessionSetup, StopConditions, Stopped};

/// What `bit-cli seed` reports.
#[derive(Debug, Clone, Serialize)]
pub struct SeedReport {
    pub info_hash: String,
    pub name: String,
    pub stopped: Stopped,
    pub complete: bool,
    pub total: Size,
    pub have: Size,
    pub uploaded: Size,
    pub uploaded_human: String,
    pub ratio: String,
    pub elapsed_ms: u64,
    pub elapsed_human: String,
    pub mean_upload_rate: Size,
    pub mean_upload_rate_human: String,
    pub peers_seen: u32,
    pub peers_served: usize,
    pub data_directory: String,
    pub listen_addr: Option<String>,
    pub trackers: Vec<String>,
    pub peers: Vec<PeerSnapshot>,
    /// What the `--on-*` commands did: how many ran and how many failed.
    ///
    /// Absent when no hook was given. A hook that fails is warned about and
    /// does not change the exit code, which is `download`'s rule and is right
    /// for the same reason: the seeding happened either way. See
    /// `TODO/cli-surface.md`, T-214.
    #[serde(skip_serializing_if = "crate::hooks::HookCounts::is_empty")]
    pub hooks: crate::hooks::HookCounts,
    /// Files whose on-disk path is not the path in the torrent, and why.
    ///
    /// The same array `download --json` reports, because a seeder serves the
    /// files that command wrote. A caller seeding a payload whose paths were
    /// rewritten cannot otherwise tell which file on disk is which file in the
    /// torrent. See `bit_cli_core::paths` and `TODO/windows.md`, T-076.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub renamed: Vec<bit_cli_core::paths::Rename>,
    /// What this run cost: peak RSS, CPU time, and open handles.
    ///
    /// A seeder is the long-lived process, so its own high-water marks are
    /// what a soak test reads. Sampling from outside means sampling a process
    /// that has already exited, which reports zero.
    pub process: bit_cli_core::sysinfo::Process,
    /// What `--listener-check` found. Absent unless it was asked for.
    ///
    /// A seeder whose listener has stopped answering is down, and the rest of
    /// this report cannot say so: the ratio, the uploaded total, and the peer
    /// rows are all history. See `TODO/peers.md`, T-020.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listener: Option<swarm::ListenerReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Where this torrent's files actually live, when that is not where the
/// torrent said.
///
/// A seeder serves the files a download wrote, and a download rewrites a path
/// the filesystem refuses. Without this the report names files the caller
/// cannot find. See `TODO/windows.md`, T-076.
fn renames(
    engine: &Engine,
    handle: &bit_cli_core::engine::Handle,
) -> Vec<bit_cli_core::paths::Rename> {
    engine
        .path_plan(handle)
        .map(|plan| plan.renames)
        .unwrap_or_default()
}

/// Run the command.
pub fn run(
    args: &SeedArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let report_interval = swarm::duration_flag(&args.report_interval, "report-interval")?;
    let listener_check = swarm::optional_duration(&args.listener_check, "listener-check")?;
    let mut stop = StopConditions {
        timeout: swarm::optional_duration(&global.timeout, "timeout")?,
        stop_after: swarm::optional_duration(&global.stop_after, "stop-after")?,
        stall: None,
        lowest_rate: None,
        seed_ratio: args.limits.seed_ratio,
        seed_time: swarm::optional_duration(&args.limits.seed_time, "seed-time")?,
        exit_when_idle: swarm::optional_duration(&args.exit_when_idle, "exit-when-idle")?,
        max_handles: args.limits.max_handles,
        max_rss: swarm::size_flag(&args.limits.max_rss, "max-rss")?,
        // Filled in once the session is live, because the probe needs the
        // port it bound and the info hash it settled on.
        listener: None,
    };
    if args.superseed {
        renderer.warn(
            env,
            "--superseed is accepted but BEP 16 superseeding is not implemented yet; see TODO/create-seed.md",
        );
    }
    // `librqbit` 9.0.0 has no switch for peer exchange: `SessionOptions`
    // carries `dht` and `disable_local_service_discovery` and nothing beside
    // them for PEX. A caller passing this believes their address has stopped
    // being gossiped to the swarm, so silence here is a privacy expectation
    // quietly unmet rather than a performance knob quietly ignored. See
    // `TODO/cli-surface.md`, T-181.
    if args.no_pex {
        renderer.warn(
            env,
            "--no-pex is accepted but peer exchange stays on: librqbit 9.0.0 has no switch for it, so your address is still gossiped to the swarm; see TODO/cli-surface.md T-181",
        );
    }

    let web_seeds = crate::cli::WebSeedArgs::default();
    let setup = SessionSetup {
        global,
        trackers: &args.trackers,
        limits: &args.limits,
        web_seeds: &web_seeds,
        listen_ports: swarm::port_range(&args.port)?,
        no_dht: args.no_dht,
        no_lsd: args.no_lsd,
        // Seeding reads what is already on disk and creates nothing.
        allocation: bit_cli_core::alloc::Allocation::default(),
    };
    let mut engine_options = setup.engine_options(env)?;
    // Seeding reads the payload from where it already lives, which is not
    // necessarily where a download would have written it.
    if let Some(data) = &args.data {
        engine_options.download_directory = env.resolve(data);
    }
    let base = engine_options.download_directory.clone();

    let kind = Kind::classify(&args.source.source, env)?;
    let meta = match &kind {
        Kind::File(path) => Some(crate::source::read_torrent_file(path)?),
        _ => None,
    };

    // A multi-file torrent lays its files under a directory named after
    // itself, so `--data` can name the parent or the torrent directory and
    // mean the same payload. `verify` accepted either and this accepted only
    // the parent, which made pointing at the torrent directory a seeder
    // holding nothing and a warning that said "partial seed". Both commands
    // now ask the same function. See `TODO/cli-surface.md`, T-186.
    //
    // A magnet has no layout until its metadata resolves, and by then the
    // session has already decided where to look, so it keeps `--data` as
    // given. Nothing is lost: a magnet has nothing on disk to be pointed at
    // two ways.
    let index_out = crate::selection::index_out(
        &args.index_out,
        meta.as_ref().map(|meta| meta.layout().files.len()),
    )?;
    let root = meta
        .as_ref()
        .map(|meta| crate::payload::resolve_with(&base, &meta.layout(), &index_out));
    let directory = root
        .as_ref()
        .map_or_else(|| base.clone(), |r| r.path.clone());
    let payload_root = root.as_ref().map(|r| r.path.display().to_string());
    // The resume cache, and where it lives.
    //
    // Beside the payload by default, so moving or deleting the data takes the
    // cache with it and nothing is left behind in a shared directory keyed by
    // a hash nobody can trace back. `--fastresume-dir` overrides it for a
    // caller who wants one place for many torrents. See `TODO/disk-io.md`,
    // T-016.
    if args.fastresume {
        engine_options.resume_cache = Some(match &args.fastresume_dir {
            Some(dir) => env.resolve(dir),
            None => directory.join(bit_cli_core::resume::DEFAULT_DIR_NAME),
        });
    }
    // The place this payload could have been and was not, kept for the warning
    // a seeder holding nothing gets. See `TODO/cli-surface.md`, T-186.
    let other_root = root.as_ref().and_then(|r| r.other.clone());
    if global.dry_run {
        // A dry run reports without doing, so a `--tracker-list-url` is
        // refused rather than fetched. That is the decision
        // `--web-seed-list-url` already takes on `download --dry-run`.
        let trackers = crate::swarm::SessionSetup::tracker_list(
            &setup,
            meta.as_ref(),
            env,
            crate::webseed_args::no_network,
        )?;
        let report = json!({
            "dry_run": true,
            "source": args.source.source,
            "data_directory": directory.display().to_string(),
            "trackers": trackers.clone().unwrap_or_default(),
            "verify": format!("{:?}", args.verify).to_lowercase(),
            "info_hash": meta.as_ref().map(|m| m.info_hash().hex()),
        });
        renderer.emit(env, "seed", &report, || {
            vec![
                field("dry run", "nothing will be served"),
                field("data", directory.display()),
                field("trackers", trackers.clone().unwrap_or_default().len()),
            ]
        })?;
        return Ok(ExitCode::Success);
    }

    // All three values behave the same today, and saying so is better than
    // letting a caller believe `--verify none` skipped anything. `librqbit`
    // 9.0.0 hash-checks on add and `AddTorrentOptions` carries no way to ask
    // it not to, so there is nothing for the other two values to reach.
    // Measured on a 512 MiB payload: 6087 ms, 6372 ms, and 6398 ms. See
    // `TODO/disk-io.md`, T-016.
    if args.verify != SeedVerify::Full {
        renderer.warn(
            env,
            format!(
                "--verify {} still hash-checks the whole payload on start: the session cannot serve unverified data and has no way to skip the check",
                match args.verify {
                    SeedVerify::Quick => "quick",
                    _ => "none",
                }
            ),
        );
    }

    let init_timeout = swarm::duration_flag(&args.limits.init_timeout, "init-timeout")?;
    let source = args.source.source.clone();
    let announce_only = args.announce_only;
    let on_complete = args.hooks.on_complete.clone();
    let on_error = args.hooks.on_error.clone();
    // What identifies the torrent when the run failed before there was a
    // snapshot. A `.torrent` names both; a magnet that never resolved has an
    // info hash and no name, which is exactly what a hook is being told about.
    let failed_info_hash = meta
        .as_ref()
        .map(|meta| meta.info_hash().hex())
        .unwrap_or_default();
    let failed_name = meta
        .as_ref()
        .map(|meta| meta.layout().name)
        .unwrap_or_default();
    let (torrent_download_rate, torrent_upload_rate) = setup.torrent_rates()?;
    let runtime = swarm::runtime()?;
    // `--tracker-list-url` is fetched on the runtime this command already
    // built. See `TODO/cli-surface.md`, T-181.
    let user_agent = bit_cli_core::webseed::fetch::default_user_agent();
    let trackers = setup.tracker_list(
        meta.as_ref(),
        env,
        crate::source::list_fetcher(&runtime, &user_agent),
    )?;

    let report = runtime.block_on(async {
        let engine = Engine::start(&engine_options).await?;
        for warning in engine.warnings() {
            renderer.warn(env, warning);
        }

        let add = AddOptions {
            // Seeding needs the existing payload read and hash-checked, which
            // is what `overwrite` allows. Without it the add fails on the
            // files that are the whole point of the command.
            overwrite: true,
            // The resolved payload root, which the files hang directly off.
            // Naming it rather than letting the session append the torrent's
            // own name is what makes `--data <parent>` and
            // `--data <parent>/<name>` the same payload, and it is right even
            // when the directory on disk was renamed. `None` for a magnet,
            // which has no layout to resolve against yet. See
            // `TODO/cli-surface.md`, T-186.
            output_folder: payload_root.clone(),
            // Where the bytes actually are, when the caller renamed them on
            // the way in. Empty for an ordinary payload, and then the plan is
            // the one the torrent describes. See `TODO/cli-surface.md`,
            // T-213.
            index_out: index_out.clone(),
            trackers: trackers.clone(),
            disable_trackers: trackers.as_ref().is_some_and(Vec::is_empty),
            tracker_interval: swarm::optional_duration(
                &args.trackers.tracker_interval,
                "tracker-interval",
            )?,
            download_rate: torrent_download_rate,
            upload_rate: torrent_upload_rate,
            ..Default::default()
        };
        // What the payload should look like, recorded before the add,
        // because the session loads the cached bitfield during the add. A
        // torrent with no metadata yet, which is a magnet, has nothing to
        // describe and is never served from the cache.
        if let (Some(meta), Some(dir)) = (meta.as_ref(), root.as_ref()) {
            let layout = meta.layout();
            let files: Vec<(String, u64)> = layout
                .files
                .iter()
                .map(|f| (f.path.join("/"), f.length))
                .collect();
            let pieces = layout.total_length.div_ceil(u64::from(layout.piece_length));
            engine.expect_resume(
                &meta.info_hash().hex(),
                bit_cli_core::resume::Fingerprint::of(
                    &dir.path,
                    &files,
                    layout.total_length,
                    pieces.try_into().unwrap_or(u32::MAX),
                ),
            );
        }
        let handle = engine.add(&source, &add).await?;
        renderer.event(
            env,
            "session_start",
            &json!({
                "source": source,
                "data_directory": directory.display().to_string(),
                "listen_addr": engine.listen_addr().map(|a| a.to_string()),
            }),
        )?;

        engine
            .wait_until_initialized_within(&handle, init_timeout)
            .await?;
        let layout = engine.layout(&handle).ok_or_else(|| {
            Error::source_resolution(format!("{source}: the torrent has no metadata"))
        })?;
        let snapshot = engine.snapshot(&handle);

        // Seeding data that is not all there is a partial seed, which is
        // legitimate, but the caller should be told rather than discover it
        // from a ratio that never moves.
        if !snapshot.finished {
            renderer.warn(
                env,
                format!(
                    "only {} of {} is present, so this is a partial seed",
                    format_size(snapshot.progress_bytes),
                    format_size(snapshot.total_bytes)
                ),
            );
        }
        // Holding **none** of it is the case a partial seed's warning cannot
        // describe, because a partial seed is legitimate and a wrong `--data`
        // is not. Saying which directory was searched, and which other one a
        // multi-file torrent's files also sit under, is the difference.
        //
        // Keyed on bytes rather than on whether the files exist, and that is
        // deliberate: a seeder creates the tree it was looking for, so by the
        // second run the directory holds full-length files with nothing in
        // them and "the payload is not here" would be false. See
        // `TODO/cli-surface.md`, T-186.
        if snapshot.progress_bytes == 0 {
            let elsewhere = match &other_root {
                Some(other) => format!(
                    "; a multi-file torrent's files also sit under {}",
                    other.display()
                ),
                None => String::new(),
            };
            renderer.warn(
                env,
                format!(
                    "none of {} is in {}, which is where --data resolved to{elsewhere}",
                    layout.name,
                    directory.display()
                ),
            );
        }

        let tracker_list = engine.trackers(&handle);
        if announce_only {
            // The announce already happened when the torrent went live, so
            // this reports it and stops rather than serving.
            tokio::time::sleep(Duration::from_secs(2)).await;
            let snapshot = engine.snapshot(&handle);
            let peers = engine.peers(&handle, &HashSet::new());
            return Ok(build(
                &snapshot,
                Stopped::Completed,
                Duration::from_secs(2),
                &directory,
                engine.listen_addr().map(|a| a.to_string()),
                tracker_list,
                peers,
                renames(&engine, &handle),
                // Announce-only never serves, so there is no listener to
                // have watched.
                None,
            ));
        }

        // The probe needs the port the session actually bound and an info
        // hash it is actually serving, so this is the first point where both
        // are known. Announce-only returns above it, because a run that never
        // serves has no listener worth watching.
        let listener = match (listener_check, engine.bridge_target()) {
            (Some(interval), Some(target)) => {
                let state =
                    swarm::spawn_listener_probe(target, handle.info_hash().0, interval);
                stop.listener = Some(swarm::ListenerCheck {
                    state: std::sync::Arc::clone(&state),
                    allowed: swarm::LISTENER_FAILURES_ALLOWED,
                });
                Some(state)
            }
            (Some(_), None) => {
                renderer.warn(
                    env,
                    "--listener-check does nothing here: this run bound no listen port, so there is no listener to probe",
                );
                None
            }
            (None, _) => None,
        };

        // The moment a seeder starts being useful, and the only one it has:
        // the payload has passed its hash check and the listener is up. A
        // seeder does not complete, so `--on-complete` means "ready to serve"
        // here rather than "finished", which `docs/hooks.md` states and this
        // is the only place it fires. See `TODO/cli-surface.md`, T-214.
        let mut hook_counts = crate::hooks::HookCounts::default();
        if let Some(command) = on_complete.as_deref() {
            let vars = crate::hooks::hook_vars(
                "on-complete",
                &hook_facts(
                    &snapshot,
                    &source,
                    &directory.display().to_string(),
                    "serving",
                    0,
                    None,
                ),
            );
            hook_counts.ran += 1;
            match swarm::run_hook(command, &vars) {
                Ok(0) => {}
                Ok(code) => {
                    hook_counts.failed += 1;
                    renderer.warn(env, format!("hook `{command}` exited {code}"));
                }
                Err(error) => {
                    hook_counts.failed += 1;
                    renderer.warn(env, format!("hook `{command}` failed: {error}"));
                }
            }
        }

        let lengths: Vec<u64> = layout.files.iter().map(|f| f.length).collect();
        let mut progress = Progress::new(layout.piece_count(), lengths);
        let mut ticker = tokio::time::interval(report_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let interrupt = tokio::signal::ctrl_c();
        tokio::pin!(interrupt);

        let (stopped, elapsed) = loop {
            tokio::select! {
                _ = &mut interrupt => break (Stopped::Interrupted, progress.elapsed()),
                _ = ticker.tick() => {}
            }

            let mut snapshot = engine.snapshot(&handle);
            let probe_ports = listener.as_ref().map(|s| s.ports()).unwrap_or_default();
            let view =
                swarm::without_probe_rows(engine.peers(&handle, &HashSet::new()), &probe_ports);
            // Before `observe` and before the stop conditions, both of which
            // read the peer counts. See `swarm::discount_probe_peers`.
            swarm::discount_probe_peers(&mut snapshot, &view);
            let peers = view.rows;
            progress.observe(&snapshot, None, &handle.stats().file_progress);

            let mut event = json!({
                    "info_hash": snapshot.info_hash,
                    "uploaded_bytes": snapshot.uploaded_bytes,
                    "upload_rate": snapshot.upload_rate,
                    "download_rate": snapshot.download_rate,
                    "ratio": format!("{:.3}", snapshot.ratio()),
                    "peers": snapshot.peers,
                    // The peers this session holds right now, not every peer
                    // it has ever held. See `swarm::currently_held` for what
                    // the old array cost and why the count here is one the
                    // same event already carries.
                    "peer_detail": swarm::currently_held(&peers),
                    // What the process costs right now, so a soak reads a slope out of
                    // the event stream rather than sampling the process from outside.
                    // See `TODO/memory.md`, T-040.
                    "process": bit_cli_core::sysinfo::Process::sample(),
            });
            // The key is absent unless the check was asked for, so a consumer
            // tells "watched and fine" from "not watched" without a flag of
            // its own. Inserted rather than written into the literal above,
            // because `json!` has no way to leave a key out.
            if let Some(state) = &listener
                && let Some(fields) = event.as_object_mut()
            {
                fields.insert(
                    "listener".into(),
                    serde_json::to_value(state.report()).unwrap_or_default(),
                );
            }
            renderer.event(env, "progress", &event)?;
            if renderer.progress == crate::cli::ProgressMode::Plain {
                let _ = env.note(format!(
                    "up {}  uploaded {}  ratio {:.3}  peers {}",
                    format_rate(snapshot.upload_rate),
                    format_size(snapshot.uploaded_bytes),
                    snapshot.ratio(),
                    snapshot.peers.live,
                ));
            }

            if let Some(reason) = progress.should_stop(&snapshot, &stop, true) {
                break (reason, progress.elapsed());
            }
        };

        let mut snapshot = engine.snapshot(&handle);
        let probe_ports = listener.as_ref().map(|s| s.ports()).unwrap_or_default();
        let view = swarm::without_probe_rows(engine.peers(&handle, &HashSet::new()), &probe_ports);
        swarm::discount_probe_peers(&mut snapshot, &view);
        let peers = view.rows;
        let mut report = build(
            &snapshot,
            stopped,
            elapsed,
            &directory,
            engine.listen_addr().map(|a| a.to_string()),
            tracker_list,
            peers,
            renames(&engine, &handle),
            listener.as_ref().map(|s| s.report()),
        );
        report.hooks = hook_counts;
        engine.stop().await;
        Ok::<_, Error>(report)
    });

    // A seeding run that failed is the other well defined moment, and it is
    // the one an operator watching a long-lived seeder actually wants: the
    // process is gone and something has to be told. The snapshot the hook
    // would describe does not exist here, because the failure is the reason
    // there is none, so the variables that identify the torrent come from the
    // source and the rest are what a failed run has: nothing served, nothing
    // held. See `TODO/cli-surface.md`, T-214.
    let report = match report {
        Ok(report) => report,
        Err(error) => {
            if let Some(command) = on_error.as_deref() {
                let text = error.to_string();
                let vars = crate::hooks::hook_vars(
                    "on-error",
                    &crate::hooks::Finished {
                        info_hash: &failed_info_hash,
                        name: &failed_name,
                        source: &source,
                        directory: &directory.display().to_string(),
                        total_bytes: 0,
                        downloaded_bytes: 0,
                        from_peers_bytes: 0,
                        from_web_seeds_bytes: 0,
                        finished: false,
                        stopped: "error",
                        elapsed_ms: 0,
                        error: Some(&text),
                        torrents: 1,
                        completed: 0,
                        failed: 1,
                        run_elapsed_ms: 0,
                    },
                );
                match swarm::run_hook(command, &vars) {
                    Ok(0) => {}
                    Ok(code) => renderer.warn(env, format!("hook `{command}` exited {code}")),
                    Err(hook) => renderer.warn(env, format!("hook `{command}` failed: {hook}")),
                }
            }
            return Err(error);
        }
    };

    // Seeding to nobody is the failure a seeding operator most needs to catch,
    // and it is indistinguishable from success in the totals alone.
    let code = match (report.stopped, report.peers_seen) {
        (Stopped::Idle, 0) => ExitCode::ThresholdNotMet,
        (reason, _) => reason.code(),
    };
    renderer.emit(env, "seed", &report, || lines(&report))?;
    Ok(code)
}

/// What a seeding run can tell a hook.
///
/// The same struct `download` fills, read for a seeder: `downloaded_bytes` is
/// what is on disk and verified rather than what this run fetched, because a
/// seeder fetches nothing, and `stopped` is why the run ended or `serving`
/// when it has not. `finished` says the payload is whole, which on a seeder is
/// a fact about the data rather than about the run: a partial seed is a
/// legitimate thing to be doing, so it does not make the hook an error.
/// See `TODO/cli-surface.md`, T-214.
fn hook_facts<'a>(
    snapshot: &'a TorrentSnapshot,
    source: &'a str,
    directory: &'a str,
    stopped: &'a str,
    elapsed_ms: u64,
    error: Option<&'a str>,
) -> crate::hooks::Finished<'a> {
    crate::hooks::Finished {
        info_hash: &snapshot.info_hash,
        name: &snapshot.name,
        source,
        directory,
        total_bytes: snapshot.total_bytes,
        downloaded_bytes: snapshot.progress_bytes,
        // A seeder is the other end of these two. Zero rather than absent,
        // because the variables are documented as always set and a hook
        // testing one of them should not have to know which command it is
        // under.
        from_peers_bytes: 0,
        from_web_seeds_bytes: 0,
        finished: snapshot.finished,
        stopped,
        elapsed_ms,
        error,
        torrents: 1,
        completed: usize::from(error.is_none()),
        failed: usize::from(error.is_some()),
        run_elapsed_ms: elapsed_ms,
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    snapshot: &TorrentSnapshot,
    stopped: Stopped,
    elapsed: Duration,
    directory: &std::path::Path,
    listen_addr: Option<String>,
    trackers: Vec<String>,
    peers: Vec<PeerSnapshot>,
    renamed: Vec<bit_cli_core::paths::Rename>,
    listener: Option<swarm::ListenerReport>,
) -> SeedReport {
    let elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
    let mean = match elapsed_ms {
        0 => 0,
        ms => snapshot.uploaded_bytes.saturating_mul(1000) / ms,
    };
    SeedReport {
        hooks: crate::hooks::HookCounts::default(),
        info_hash: snapshot.info_hash.clone(),
        name: snapshot.name.clone(),
        stopped,
        complete: snapshot.finished,
        total: Size(snapshot.total_bytes),
        have: Size(snapshot.progress_bytes),
        uploaded: Size(snapshot.uploaded_bytes),
        uploaded_human: format_size(snapshot.uploaded_bytes),
        ratio: bit_cli_core::units::format_ratio(snapshot.ratio()),
        elapsed_ms,
        elapsed_human: bit_cli_core::units::format_duration(elapsed),
        mean_upload_rate: Size(mean),
        mean_upload_rate_human: format_rate(mean),
        peers_seen: snapshot.peers.seen,
        peers_served: peers.iter().filter(|p| p.uploaded_bytes > 0).count(),
        data_directory: directory.display().to_string(),
        listen_addr,
        trackers,
        peers,
        renamed,
        process: bit_cli_core::sysinfo::Process::sample(),
        listener,
        error: snapshot.error.clone(),
    }
}

fn lines(report: &SeedReport) -> Vec<String> {
    let mut out = vec![
        field("name", &report.name),
        field("info hash", &report.info_hash),
        field("stopped", report.stopped.as_str()),
        field("complete", report.complete),
        field(
            "have",
            format!(
                "{} of {}",
                format_size(report.have.0),
                format_size(report.total.0)
            ),
        ),
        field("uploaded", &report.uploaded_human),
        field("ratio", &report.ratio),
        field("mean up", &report.mean_upload_rate_human),
        field("elapsed", &report.elapsed_human),
        field("peers seen", report.peers_seen),
        field("peers served", report.peers_served),
        field("data", &report.data_directory),
        field("cost", report.process.summary()),
    ];
    if let Some(addr) = &report.listen_addr {
        out.push(field("listening on", addr));
    }
    for tracker in &report.trackers {
        out.push(field("tracker", tracker));
    }
    if let Some(error) = &report.error {
        out.push(field("error", error));
    }
    if !report.peers.is_empty() {
        out.push(String::new());
        out.extend(swarm::peer_table(&report.peers));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bit_cli_core::engine::{PeerCounts, State};

    fn snapshot(uploaded: u64) -> TorrentSnapshot {
        TorrentSnapshot {
            id: 0,
            info_hash: "a".repeat(40),
            name: "payload".into(),
            state: State::Live,
            total_bytes: 1000,
            progress_bytes: 1000,
            uploaded_bytes: uploaded,
            finished: true,
            download_rate: 0,
            upload_rate: 0,
            eta_ms: None,
            eta_confidence: "none",
            peers: PeerCounts {
                seen: 3,
                ..Default::default()
            },
            error: None,
        }
    }

    fn peer(uploaded: u64) -> PeerSnapshot {
        PeerSnapshot {
            addr: "203.0.113.5:6881".into(),
            state: "live".into(),
            client: Some("rqbit".into()),
            connection: Some("tcp".into()),
            encryption: Some("rc4".into()),
            choked: 0,
            unchoked: 0,
            disconnects: Vec::new(),
            direction: "incoming",
            downloaded_bytes: 0,
            uploaded_bytes: uploaded,
            verified_pieces: 0,
            chunks: 0,
            errors: 0,
            connect_ms: 12,
            mean_piece_ms: None,
            web_seed: false,
        }
    }

    #[test]
    fn the_report_counts_only_peers_that_actually_took_bytes() {
        let report = build(
            &snapshot(2000),
            Stopped::SeedRatio,
            Duration::from_secs(4),
            std::path::Path::new("/data"),
            Some("0.0.0.0:6881".into()),
            vec!["udp://t.example:451".into()],
            vec![peer(2000), peer(0)],
            Vec::new(),
            None,
        );
        assert_eq!(
            report.peers_served, 1,
            "a connected peer that took nothing was not served"
        );
        assert_eq!(report.peers.len(), 2, "but both are still reported");
        assert_eq!(report.peers_seen, 3);
    }

    #[test]
    fn the_ratio_is_rendered_to_three_places() {
        let report = build(
            &snapshot(2500),
            Stopped::SeedTime,
            Duration::from_secs(1),
            std::path::Path::new("/data"),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        );
        assert_eq!(report.ratio, "2.500");
    }

    #[test]
    fn the_mean_upload_rate_is_bytes_over_elapsed_seconds() {
        let report = build(
            &snapshot(4000),
            Stopped::SeedTime,
            Duration::from_secs(4),
            std::path::Path::new("/data"),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        );
        assert_eq!(report.mean_upload_rate.0, 1000);
        assert_eq!(report.elapsed_ms, 4000);
    }

    #[test]
    fn a_zero_length_run_does_not_divide_by_zero() {
        let report = build(
            &snapshot(4000),
            Stopped::Interrupted,
            Duration::ZERO,
            std::path::Path::new("/data"),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        );
        assert_eq!(report.mean_upload_rate.0, 0);
    }

    #[test]
    fn the_text_rendering_carries_every_number_the_json_does() {
        let report = build(
            &snapshot(2000),
            Stopped::SeedRatio,
            Duration::from_secs(4),
            std::path::Path::new("/data"),
            Some("0.0.0.0:6881".into()),
            vec!["udp://t.example:451".into()],
            vec![peer(2000)],
            Vec::new(),
            None,
        );
        let text = lines(&report).join("\n");
        assert!(text.contains("2.000"), "{text}");
        assert!(text.contains("udp://t.example:451"), "{text}");
        assert!(text.contains("203.0.113.5:6881"), "{text}");
        assert!(text.contains("0.0.0.0:6881"), "{text}");
        assert!(text.contains("peak RSS"), "the cost is not display-only");
    }

    /// `TODO/cli-surface.md` T-181. `--no-pex` cannot be built against
    /// `librqbit` 9.0.0, so it says so instead of pretending.
    ///
    /// A caller passing this believes peer exchange is off. It is not, and
    /// their address keeps being gossiped to the swarm, which is a privacy
    /// expectation quietly unmet rather than a knob quietly ignored. The
    /// warning names the entry so a reader can find out when it will change.
    #[test]
    fn no_pex_warns_that_peer_exchange_stays_on() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let (mut env, captured) = crate::env::Env::test(
            &[
                "seed",
                "--dry-run",
                "--no-pex",
                "--data",
                fixture.dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(crate::run(&mut env), ExitCode::Success);
        let err = captured.err();
        assert!(err.contains("--no-pex"), "{err}");
        assert!(
            err.contains("peer exchange stays on"),
            "the warning has to say what is still happening: {err}"
        );
        assert!(
            err.contains("T-181"),
            "the warning has to name the entry that owns it: {err}"
        );
        assert!(!captured.out().is_empty(), "the report still prints");
    }

    /// Without the flag there is no warning, so the message is about the flag
    /// rather than something every seed prints.
    #[test]
    fn a_seed_without_no_pex_says_nothing_about_peer_exchange() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let (mut env, captured) = crate::env::Env::test(
            &[
                "seed",
                "--dry-run",
                "--data",
                fixture.dir().to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(crate::run(&mut env), ExitCode::Success);
        assert!(
            !captured.err().contains("peer exchange"),
            "{}",
            captured.err()
        );
    }

    /// A seeder is the long-lived process, so its own high-water marks are
    /// what a soak test reads. See `TODO/memory.md`, T-040.
    /// Seeding what `download --select-file` left behind.
    ///
    /// `TODO/disk-io.md` T-184 expected a seeder to announce pieces it could
    /// not prove. It does not, and the reason is that there are none: the
    /// unselected half of a boundary piece is written into the unselected file
    /// for the piece's sake, so the piece verifies and the hash check finds
    /// it. What the seeder holds is exactly pieces 1 and 2 of four, 2048 bytes
    /// of 3700, and it says so.
    #[test]
    fn a_seeder_of_a_selection_holds_the_boundary_pieces_and_says_so() {
        let fixture = crate::test_support::TorrentFixture::straddling();
        let server = crate::test_support::FileServer::start(fixture.dir());
        let source = format!("{}payload/", server.base);
        let out = fixture.dir().join("out");
        crate::test_support::run_json(
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

        let report = crate::test_support::run_json_code(
            &[
                "seed",
                fixture.path_str(),
                // The parent. Since `TODO/cli-surface.md` T-186 the torrent
                // directory works too, and
                // `either_spelling_of_data_seeds_the_same_payload` is what
                // pins that.
                "--data",
                out.to_str().unwrap(),
                "--port",
                "0",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--stop-after",
                "3s",
            ],
            fixture.dir(),
            // Nothing connects, so the run stops on its deadline. What it
            // holds is decided by the hash check and reported either way.
            ExitCode::Timeout,
        );
        assert_eq!(
            report["have"]["bytes"], 2048,
            "a seeder holds both boundary pieces, which is 2048 of 3700 bytes: {report}"
        );
        assert_eq!(report["total"]["bytes"], 3700);
        assert_eq!(
            report["complete"], false,
            "and it does not claim to hold the rest"
        );
    }

    /// `TODO/cli-surface.md` T-186's acceptance.
    ///
    /// A multi-file torrent lays its files under a directory named after
    /// itself, so `--data` can name the parent or the torrent directory. Both
    /// spellings are the same payload, and before this only one of them was:
    /// the other reported `have: 0` with "this is a partial seed", which is
    /// the right observation with the wrong reason, and created an empty tree
    /// one level deeper on its way to saying it.
    /// A payload renamed on the way in is served from where it landed.
    ///
    /// `download -O 0=renamed.bin` writes the first file under a name only the
    /// caller knows, and a seeder that looks where the torrent said finds
    /// nothing. Both halves are here because the second is what makes the
    /// first mean anything: told, every byte is present; not told, the same
    /// command holds less than the payload. See `TODO/cli-surface.md`, T-213.
    #[test]
    fn a_file_renamed_by_index_out_is_seeded_from_where_it_landed() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let data = fixture.dir().join("data");
        fixture.place(&data, &[]);

        // Rename on disk what `-O 0=renamed.bin` would have written, which is
        // the state a download with that flag leaves behind.
        let root = data.join(&fixture.name);
        let first = root.join(&fixture.files[0].0);
        let renamed = root.join("renamed.bin");
        std::fs::create_dir_all(renamed.parent().unwrap()).unwrap();
        std::fs::rename(&first, &renamed).expect("rename the payload file");

        let seed = |extra: &[&str]| {
            let mut argv = vec![
                "seed",
                fixture.path_str(),
                "--data",
                data.to_str().unwrap(),
                "--port",
                "0",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--stop-after",
                "1s",
            ];
            argv.extend_from_slice(extra);
            crate::test_support::run_json_code(
                &argv,
                fixture.dir(),
                // Nothing connects, so the run stops on its deadline.
                ExitCode::Timeout,
            )
        };

        let told = seed(&["-O", "0=renamed.bin"]);
        assert_eq!(told["complete"], true, "{told}");
        assert_eq!(told["have"]["bytes"], 2000, "{told}");

        let untold = seed(&[]);
        assert_eq!(untold["complete"], false, "{untold}");
        assert!(
            untold["have"]["bytes"].as_u64().unwrap_or(2000) < 2000,
            "a seeder told nothing about the rename claimed the whole payload: {untold}"
        );
    }

    /// `--on-complete` fires once, when the seeder is ready to serve.
    ///
    /// A seeder has no completion of its own, so the trigger is the payload
    /// passing its hash check with the listener up. The flag parsed before
    /// this and ran nothing: `SeedArgs` flattens `LimitArgs`, which carried
    /// all three hook flags, so `seed`, `peers`, `bench leech` and
    /// `bench seed` each accepted three commands and ran none. See
    /// `TODO/cli-surface.md`, T-214.
    #[test]
    fn on_complete_fires_once_when_the_seeder_is_ready_to_serve() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let data = fixture.dir().join("data");
        fixture.place(&data, &[]);

        // A directory named after the variables the hook was handed. `mkdir`
        // rather than a redirect, for the reason `download`'s own hook test
        // records: a redirect is parsed by `cmd` after Rust has quoted the
        // argument and the two disagree about a Windows path.
        let marks = fixture.dir().join("marks");
        std::fs::create_dir_all(&marks).expect("make the marker directory");
        let marks_arg = marks.to_str().expect("utf-8 path").to_string();
        let command = match cfg!(windows) {
            true => format!(r#"mkdir "{marks_arg}\%BIT_CLI_HOOK%-%BIT_CLI_INFO_HASH%""#),
            false => format!(r#"mkdir -p "{marks_arg}/$BIT_CLI_HOOK-$BIT_CLI_INFO_HASH""#),
        };

        let report = crate::test_support::run_json_code(
            &[
                "seed",
                fixture.path_str(),
                "--data",
                data.to_str().unwrap(),
                "--port",
                "0",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--stop-after",
                "1s",
                "--on-complete",
                &command,
            ],
            fixture.dir(),
            // Nothing connects, so the run stops on its deadline. The hook
            // fired before the serve loop, which is the point: it does not
            // wait for the run to end.
            ExitCode::Timeout,
        );

        assert_eq!(report["hooks"]["ran"], 1, "{report}");
        assert_eq!(report["hooks"]["failed"], 0, "{report}");

        let left: Vec<String> = std::fs::read_dir(&marks)
            .expect("read the marker directory")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        assert_eq!(
            left.len(),
            1,
            "the hook ran {} time(s): {left:?}",
            left.len()
        );
        assert_eq!(
            left[0],
            format!("on-complete-{}", fixture.info_hash),
            "the hook was told which hook it was and which torrent: {left:?}"
        );
    }

    /// A hook that fails is counted and warned about, and the seeding stands.
    #[test]
    fn a_failing_hook_does_not_fail_the_seeding() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let data = fixture.dir().join("data");
        fixture.place(&data, &[]);

        let report = crate::test_support::run_json_code(
            &[
                "seed",
                fixture.path_str(),
                "--data",
                data.to_str().unwrap(),
                "--port",
                "0",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--stop-after",
                "1s",
                "--on-complete",
                "exit 3",
            ],
            fixture.dir(),
            // The deadline, not the hook: a hook that fails is a warning.
            ExitCode::Timeout,
        );

        assert_eq!(report["hooks"]["ran"], 1, "{report}");
        assert_eq!(report["hooks"]["failed"], 1, "{report}");
        assert_eq!(report["complete"], true, "{report}");
    }

    #[test]
    fn either_spelling_of_data_seeds_the_same_payload() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let data = fixture.dir().join("data");
        fixture.place(&data, &[]);

        let seed = |dir: &std::path::Path| {
            crate::test_support::run_json_code(
                &[
                    "seed",
                    fixture.path_str(),
                    "--data",
                    dir.to_str().unwrap(),
                    "--port",
                    "0",
                    "--no-dht",
                    "--no-lsd",
                    "--no-tracker",
                    "--stop-after",
                    "1s",
                ],
                fixture.dir(),
                // Nothing connects, so the run stops on its deadline.
                ExitCode::Timeout,
            )
        };

        let parent = seed(&data);
        let torrent_dir = seed(&data.join("album"));
        assert_eq!(parent["have"]["bytes"], 2000, "{parent}");
        assert_eq!(
            torrent_dir["have"]["bytes"], parent["have"]["bytes"],
            "the torrent directory is the same payload as its parent: {torrent_dir}"
        );
        assert_eq!(torrent_dir["complete"], true, "{torrent_dir}");
        // Both resolve to the directory the files hang off, so the report says
        // where the payload is rather than what was typed.
        assert_eq!(
            torrent_dir["data_directory"], parent["data_directory"],
            "both spellings name the same directory: {torrent_dir}"
        );
        // And nothing was created a level deeper on the way.
        assert!(
            !data.join("album").join("album").exists(),
            "a seeder pointed at the torrent directory built one inside it"
        );
    }

    /// A seeder holding nothing says which directory it searched and which
    /// other one a multi-file torrent's files sit under. A partial-seed
    /// warning on its own cannot: a partial seed is legitimate and a `--data`
    /// naming the wrong place is not, and "0 B of 1.95 KiB" is what both look
    /// like. See `TODO/cli-surface.md`, T-186.
    ///
    /// Run twice on purpose. The first run creates the tree it was looking
    /// for, at full length and empty, so a message keyed on whether the files
    /// exist would be true once and false afterwards. This one is keyed on
    /// bytes and says the same thing both times.
    #[test]
    fn a_seed_that_holds_nothing_names_the_directories_it_searched() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let empty = fixture.dir().join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        for run in 1..=2 {
            let (mut env, captured) = crate::env::Env::test(
                &[
                    "seed",
                    fixture.path_str(),
                    "--data",
                    empty.to_str().unwrap(),
                    "--port",
                    "0",
                    "--no-dht",
                    "--no-lsd",
                    "--no-tracker",
                    "--stop-after",
                    "1s",
                ],
                fixture.dir(),
            );
            assert_eq!(crate::run(&mut env), ExitCode::Timeout);
            let err = captured.err();
            assert!(err.contains("none of album is in"), "run {run}: {err}");
            assert!(err.contains(empty.to_str().unwrap()), "run {run}: {err}");
            assert!(
                err.contains(empty.join("album").to_str().unwrap()),
                "run {run}: the other candidate is named too: {err}"
            );
        }
    }

    /// And a seeder that holds the payload says none of that.
    #[test]
    fn a_complete_seed_says_nothing_about_where_it_looked() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let data = fixture.dir().join("data");
        fixture.place(&data, &[]);

        let (mut env, captured) = crate::env::Env::test(
            &[
                "seed",
                fixture.path_str(),
                "--data",
                data.to_str().unwrap(),
                "--port",
                "0",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--stop-after",
                "1s",
            ],
            fixture.dir(),
        );
        assert_eq!(crate::run(&mut env), ExitCode::Timeout);
        let err = captured.err();
        assert!(!err.contains("none of album"), "{err}");
        assert!(!err.contains("partial seed"), "{err}");
    }

    #[test]
    fn a_seed_report_carries_what_the_process_cost() {
        let report = build(
            &snapshot(0),
            Stopped::Deadline,
            Duration::from_secs(1),
            std::path::Path::new("/data"),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        );
        assert!(report.process.peak_rss_bytes > 1024 * 1024);
        assert!(report.process.open_handles > 0);
        assert!(report.process.unavailable.is_empty());
    }
    /// A seeder reports where the payload actually lives.
    ///
    /// It serves the files a download wrote, and a download rewrites a path
    /// the filesystem refuses. Without the mapping a caller cannot tell which
    /// file on disk is which file in the torrent. The same array
    /// `download --json` and `verify --json` carry. See `TODO/windows.md`,
    /// T-076.
    #[test]
    fn a_seed_of_a_hostile_torrent_reports_every_renamed_path() {
        let fixture = crate::test_support::TorrentFixture::hostile();
        let data = fixture.dir().join("data");
        std::fs::create_dir_all(&data).unwrap();

        let report = crate::test_support::run_json_code(
            &[
                "seed",
                fixture.path_str(),
                "--data",
                data.to_str().unwrap(),
                "--port",
                "0",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--stop-after",
                "3s",
            ],
            fixture.dir(),
            // Nothing is on disk and nothing connects, so the run stops on its
            // deadline. The mapping is reported either way, which is the point.
            ExitCode::Timeout,
        );
        let renamed = report["renamed"].as_array().expect("a renamed array");
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
    }

    /// A soak reads its series out of the event stream.
    ///
    /// `bit-cli seed` is the long-lived process, and the question T-040 asks
    /// is whether its memory and handles grow over hours. Sampling it from
    /// outside needs a second tool per platform, so every `progress` event
    /// carries what the process costs at that moment. See `TODO/memory.md`,
    /// T-040.
    #[test]
    fn every_seed_progress_event_carries_what_the_process_costs() {
        let fixture = crate::test_support::TorrentFixture::hostile();
        let data = fixture.dir().join("data");
        std::fs::create_dir_all(&data).unwrap();

        let (mut env, captured) = crate::env::Env::test(
            &[
                "--jsonl",
                "seed",
                fixture.path_str(),
                "--data",
                data.to_str().unwrap(),
                "--port",
                "0",
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--report-interval",
                "200ms",
                "--stop-after",
                "2s",
            ],
            fixture.dir(),
        );
        let _ = crate::run(&mut env);
        let events = captured.jsonl().expect("stdout was not ndjson");
        let progress: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "progress")
            .collect();
        assert!(
            progress.len() >= 2,
            "a 2s run at a 200ms interval should tick more than once, got {}",
            progress.len()
        );
        for event in &progress {
            let process = &event["process"];
            assert!(
                process["open_handles"].as_u64().unwrap_or(0) > 0,
                "no handle count in {event}"
            );
            assert!(
                process["rss_bytes"].as_u64().unwrap_or(0) > 1024 * 1024,
                "no resident memory in {event}"
            );
            assert!(
                process["peak_rss_bytes"].as_u64().unwrap_or(0)
                    >= process["rss_bytes"].as_u64().unwrap_or(0),
                "peak below current in {event}"
            );
        }
    }

    /// A peer that connects and leaves is reported with **why** and **when**.
    ///
    /// `TODO/peers.md` T-024: the report said a peer was `dead` and nothing
    /// else, because `librqbit`'s peer snapshot carried no disconnect cause and
    /// `on_peer_died` threw the one it had away. "Why did this peer stop taking
    /// bytes" had no answer.
    ///
    /// The peer here is a raw socket that completes a BEP 3 handshake and
    /// closes, which is the cheapest thing that produces a real reason: the
    /// seeder's next read fails and that failure is what gets recorded. What is
    /// asserted is that the reason is a real one rather than a stand-in, which
    /// is the acceptance's own wording.
    #[test]
    fn a_peer_that_leaves_is_reported_with_a_reason_and_a_time() {
        use std::io::{Read, Write};

        let fixture = crate::test_support::TorrentFixture::single_file();
        let data = fixture.dir().join("served");
        fixture.place(&data, &[]);

        // The info hash comes from the tool rather than from a literal, so a
        // fixture change cannot leave this handshaking for the wrong torrent.
        let info =
            crate::test_support::run_json(&["info", fixture.path_str(), "--json"], fixture.dir());
        let info_hash: Vec<u8> = {
            let hex = info["info_hash"].as_str().expect("info_hash").to_string();
            (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex"))
                .collect()
        };

        // The peer's patience and the seeder's deadline are two numbers that
        // have to be ordered, and they were not: the peer waited up to 20
        // seconds for a listener that `--stop-after 15s` had already taken
        // away, so a slow start failed as "the peer never completed a
        // handshake" whatever had actually gone wrong. The peer now gives up
        // well inside the run. See `TODO/windows.md`, T-216.
        const PEER_PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);
        let port = crate::test_support::free_port();
        let peer = std::thread::spawn(move || -> Result<(), String> {
            let deadline = std::time::Instant::now() + PEER_PATIENCE;
            if !crate::test_support::wait_for_listener(port, PEER_PATIENCE) {
                return Err(format!(
                    "no listener on port {port} within {PEER_PATIENCE:?}: the seeder never bound it"
                ));
            }

            // One attempt: connect, send a BEP 3 handshake, read theirs back.
            // Reading theirs is what makes the connection established from
            // both ends before it is dropped; without it the seeder may never
            // reach the live state and there is nothing to disconnect from.
            let once = |info_hash: &[u8]| -> Result<(), String> {
                let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
                    .map_err(|e| format!("cannot connect to port {port}: {e}"))?;
                let mut handshake = Vec::with_capacity(68);
                handshake.push(19u8);
                handshake.extend_from_slice(b"BitTorrent protocol");
                handshake.extend_from_slice(&[0u8; 8]);
                handshake.extend_from_slice(info_hash);
                handshake.extend_from_slice(b"-bitCLItest000000001");
                stream
                    .write_all(&handshake)
                    .map_err(|e| format!("cannot send the handshake: {e}"))?;
                let mut theirs = [0u8; 68];
                let read = stream
                    .read_exact(&mut theirs)
                    .map_err(|e| format!("cannot read the seeder's handshake back: {e}"));
                drop(stream);
                read
            };

            // Bound, and on the condition rather than on one attempt. A bound
            // listener is not a session ready to answer for this info hash:
            // the seeder binds before the torrent is live, so an early connect
            // is accepted and dropped, and `read_exact` sees the close as
            // "failed to fill whole buffer". That is what turned
            // `Test (ubuntu-latest)` red at run 32637997195. Retrying inside
            // the same patience waits for the thing the test is about, which
            // is a handshake that completes. See `TODO/windows.md`, T-221.
            let mut last = String::from("nothing was attempted");
            while std::time::Instant::now() < deadline {
                match once(&info_hash) {
                    Ok(()) => return Ok(()),
                    Err(why) => {
                        last = why;
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
            Err(format!(
                "no handshake completed within {PEER_PATIENCE:?}, last attempt: {last}"
            ))
        });

        // The run length rather than a wait on the peer: the peer connects the
        // moment the listener is up, which is a loopback round trip, and the
        // assertion below is on the report rather than on the timing.
        // Exit 9 is what `--stop-after` produces: the run was cut short rather
        // than finished, and that is the expected outcome for a seeder with a
        // deadline. The report is still written.
        let report = crate::test_support::run_json_code(
            &[
                "seed",
                fixture.path_str(),
                "--data",
                data.to_str().unwrap(),
                "--port",
                &port.to_string(),
                "--no-dht",
                "--no-lsd",
                "--no-tracker",
                "--stop-after",
                "20s",
            ],
            fixture.dir(),
            bit_cli_core::exit::ExitCode::Timeout,
        );
        // Twice the peer's patience, so a slow start cannot take the listener
        // away before the peer has reached it.
        //
        // The failure is named rather than reduced to a boolean. "The peer
        // never completed a handshake" was true of a port that was never
        // bound, a connect that was refused, and a read that was cut short,
        // and a reader of a red job could not tell which.
        match peer.join() {
            Ok(Ok(())) => {}
            Ok(Err(why)) => panic!("the peer never completed a handshake: {why}"),
            Err(_) => panic!("the peer thread panicked"),
        }

        let peers = report["peers"].as_array().expect("peers");
        let with_history: Vec<_> = peers
            .iter()
            .filter(|p| p["disconnects"].as_array().is_some_and(|d| !d.is_empty()))
            .collect();
        assert!(
            !with_history.is_empty(),
            "no peer row carries a disconnect: {peers:?}"
        );

        for row in &with_history {
            for event in row["disconnects"].as_array().expect("disconnects") {
                let at = event["at"].as_str().expect("at");
                assert!(
                    at.ends_with('Z') && at.len() >= 20,
                    "the time is not ISO 8601 UTC: {at}"
                );
                let reason = event["reason"].as_str().expect("reason");
                assert!(!reason.is_empty(), "an empty reason is not a reason");
                assert_ne!(
                    reason, "gone",
                    "the acceptance asks for a real reason rather than a stand-in"
                );
            }
        }
    }

    /// The session announces the port it is listening on, and that port is
    /// dialable while the run lasts.
    ///
    /// The upstream report this comes from is a session announcing 0 on one
    /// version and a fixed 4240 on another, either of which registers a peer
    /// nobody can reach while the download itself looks fine. `bit-cli` leaves
    /// `ListenerOptions::announce_port` unset so the session announces what it
    /// bound, and this is what says so rather than assuming it. See
    /// `TODO/trackers.md`, T-060.
    #[test]
    fn the_session_announces_the_port_it_listens_on() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let tracker = crate::test_support::Tracker::start(&[]);
        let port = crate::test_support::free_port();
        let data = fixture.dir().join("served");
        fixture.place(&data, &[]);

        let seeder = {
            let torrent = fixture.path_str().to_string();
            let data = data.to_str().expect("utf-8 path").to_string();
            let announce = tracker.announce.clone();
            let cwd = fixture.dir();
            std::thread::spawn(move || {
                let (mut env, _) = crate::env::Env::test(
                    &[
                        "seed",
                        &torrent,
                        "--data",
                        &data,
                        "--port",
                        &port.to_string(),
                        "--replace-trackers",
                        "--tracker",
                        &announce,
                        "--no-dht",
                        "--no-lsd",
                        "--stop-after",
                        "4s",
                    ],
                    cwd,
                );
                crate::run(&mut env)
            })
        };

        // Wait for the first announce rather than sleeping a fixed time: the
        // session announces as soon as the torrent is live, which is after a
        // hash check whose length is the machine's business.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut announced = Vec::new();
        while announced.is_empty() && std::time::Instant::now() < deadline {
            announced = tracker.param("port");
            if announced.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        assert_eq!(
            announced.first().map(String::as_str),
            Some(port.to_string().as_str()),
            "announced {announced:?}, listening on {port}: {:?}",
            tracker.seen()
        );

        // The announced address is one a peer could dial, which is the half of
        // this that a recorded port number does not prove.
        std::net::TcpStream::connect(("127.0.0.1", port))
            .expect("the announced port is not accepting connections");

        let _ = seeder.join();
    }
}
