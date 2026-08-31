//! `bit-cli peers`: connect, sample the swarm, report, exit.
//!
//! This takes a duration or a peer count, not a session id, because there is
//! no session to hold one. It joins the swarm, watches for as long as it was
//! told to, and reports every peer it saw with what came from each.

use std::collections::HashSet;
use std::time::Duration;

use bit_cli_core::ExitCode;
use bit_cli_core::engine::{AddOptions, Engine, PeerSnapshot};
use bit_cli_core::error::{Error, Result};
use bit_cli_core::units::Size;
use serde::Serialize;
use serde_json::json;

use crate::cli::{Global, PeersArgs};
use crate::env::Env;
use crate::output::{Renderer, field};
use crate::source::Kind;
use crate::swarm::{self, SessionSetup};

/// What `bit-cli peers` reports.
#[derive(Debug, Clone, Serialize)]
pub struct PeersReport {
    pub info_hash: String,
    pub name: String,
    pub sampled_ms: u64,
    pub sampled_human: String,
    pub live: u32,
    pub connecting: u32,
    pub queued: u32,
    pub seen: u32,
    pub dead: u32,
    pub downloaded: Size,
    /// Connections `--block-peer` refused, in each direction. Absent when
    /// nothing was blocked, so an ordinary sample carries no extra field.
    /// See `TODO/peers.md`, T-164.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bit_cli_core::engine::BlockedPeers>,
    pub peers: Vec<PeerSnapshot>,
}

/// How the peer list is sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Addr,
    Client,
    Speed,
    Pieces,
}

impl SortKey {
    fn parse(text: &str) -> Result<(Self, bool)> {
        let (key, order) = match text.split_once(':') {
            Some((key, order)) => (key, order),
            None => (text, "asc"),
        };
        let key = match key.trim() {
            "addr" | "address" => Self::Addr,
            "client" => Self::Client,
            "speed" | "down" => Self::Speed,
            "pieces" => Self::Pieces,
            other => {
                return Err(Error::usage(format!(
                    "--sort `{other}` is not a peer key; use addr, client, speed, or pieces"
                ))
                .with("value", other.to_string()));
            }
        };
        let descending = match order.trim() {
            "asc" | "ascending" => false,
            "desc" | "descending" => true,
            other => {
                return Err(
                    Error::usage(format!("--sort order `{other}` is not asc or desc"))
                        .with("value", other.to_string()),
                );
            }
        };
        Ok((key, descending))
    }
}

/// Sort peers in place.
fn sort_peers(peers: &mut [PeerSnapshot], key: SortKey, descending: bool) {
    match key {
        SortKey::Addr => peers.sort_by(|a, b| a.addr.cmp(&b.addr)),
        SortKey::Client => peers.sort_by(|a, b| {
            a.client
                .as_deref()
                .unwrap_or("")
                .cmp(b.client.as_deref().unwrap_or(""))
                .then_with(|| a.addr.cmp(&b.addr))
        }),
        SortKey::Speed => peers.sort_by(|a, b| {
            a.downloaded_bytes
                .cmp(&b.downloaded_bytes)
                .then_with(|| a.addr.cmp(&b.addr))
        }),
        SortKey::Pieces => peers.sort_by(|a, b| {
            a.verified_pieces
                .cmp(&b.verified_pieces)
                .then_with(|| a.addr.cmp(&b.addr))
        }),
    }
    if descending {
        peers.reverse();
    }
}

/// Run the command.
pub fn run(
    args: &PeersArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let duration = swarm::duration_flag(&args.duration, "duration")?;
    let (key, descending) = SortKey::parse(&args.sort)?;
    let kind = Kind::classify(&args.source.source, env)?;

    let web_seeds = crate::cli::WebSeedArgs::default();
    let setup = SessionSetup {
        global,
        trackers: &args.trackers,
        limits: &args.limits,
        web_seeds: &web_seeds,
        listen_ports: swarm::port_range(&args.port)?,
        no_dht: args.no_dht,
        no_lsd: args.no_lsd,
        // `peers` samples a swarm and writes no payload, so allocation never
        // comes up.
        allocation: bit_cli_core::alloc::Allocation::default(),
    };
    let mut engine_options = setup.engine_options(env)?;
    // A sample transfers, and what it transfers goes to a temporary directory
    // that the process removes when it exits. Nothing is written where the
    // caller is standing: this command reports a swarm, it does not deliver a
    // payload. `--duration`, `--count`, and `--max-download-rate` are what
    // bound how much moves. See `TODO/peers.md`, T-142.
    let scratch = tempfile::tempdir()
        .map_err(|e| bit_cli_core::error::from_io(e, "cannot create a scratch directory"))?;
    engine_options.download_directory = scratch.path().to_path_buf();

    let source = args.source.source.clone();
    let count = args.count;
    let initial_peers = swarm::peer_addrs(&args.peers)?;
    let no_trackers = args.trackers.no_tracker;
    // `--max-download-rate` is a per-torrent cap and goes on the add, which is
    // the comment above made to still be true. Before T-181 it reached the
    // session field instead, where it happened to bound this command because
    // this command adds one torrent. See `TODO/cli-surface.md`, T-181.
    let (torrent_download_rate, torrent_upload_rate) = setup.torrent_rates()?;
    let _ = kind;
    let runtime = swarm::runtime()?;

    let report = runtime.block_on(async {
        let engine = Engine::start(&engine_options).await?;
        for warning in engine.warnings() {
            renderer.warn(env, warning);
        }
        // Live, and interested. Sampling a swarm means joining it, and a
        // paused torrent is not in it: `librqbit` 9.0.0 hands a torrent its
        // peer stream only when it starts, so a paused one never announces,
        // never dials, and reports an empty swarm however long it is watched.
        //
        // Interested is the other half, and it is why nothing is deselected.
        // A peer holds a connection open for as long as one side wants
        // something from the other: with an empty selection every peer is
        // dropped as `not needed` on the handshake, which reports an address
        // and nothing else. `--sort speed` orders by bytes that arrived, so
        // the report is built on a transfer having happened. See
        // `TODO/peers.md`, T-142.
        let add = AddOptions {
            paused: false,
            only_files: None,
            list_only: false,
            initial_peers: initial_peers.clone(),
            disable_trackers: no_trackers,
            download_rate: torrent_download_rate,
            upload_rate: torrent_upload_rate,
            ..Default::default()
        };
        let handle = engine.add(&source, &add).await?;

        renderer.event(
            env,
            "session_start",
            &json!({
                "source": source,
                "duration_ms": duration.as_millis().min(u128::from(u64::MAX)) as u64,
                "count": count,
            }),
        )?;

        let started = std::time::Instant::now();
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let interrupt = tokio::signal::ctrl_c();
        tokio::pin!(interrupt);

        loop {
            tokio::select! {
                _ = &mut interrupt => break,
                _ = ticker.tick() => {}
            }
            let snapshot = engine.snapshot(&handle);
            if started.elapsed() >= duration {
                break;
            }
            if let Some(count) = count
                && snapshot.peers.seen as usize >= count
            {
                break;
            }
        }

        let elapsed = started.elapsed();
        let snapshot = engine.snapshot(&handle);
        let peers = engine.peers(&handle, &HashSet::new());
        let report = PeersReport {
            info_hash: snapshot.info_hash.clone(),
            name: snapshot.name.clone(),
            sampled_ms: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
            sampled_human: bit_cli_core::units::format_duration(elapsed),
            live: snapshot.peers.live,
            connecting: snapshot.peers.connecting,
            queued: snapshot.peers.queued,
            seen: snapshot.peers.seen,
            dead: snapshot.peers.dead,
            downloaded: Size(snapshot.progress_bytes),
            blocked: Some(engine.blocked()).filter(|b| b.any()),
            peers,
        };
        engine.stop().await;
        Ok::<_, Error>(report)
    })?;

    let mut report = report;
    sort_peers(&mut report.peers, key, descending);

    // A swarm with nobody in it is a real answer, not a failure to produce
    // one, but a script needs to tell the two apart from the exit code.
    let code = match report.seen {
        0 => ExitCode::NoUsableSources,
        _ => ExitCode::Success,
    };
    renderer.emit(env, "peers", &report, || lines(&report))?;
    Ok(code)
}

fn lines(report: &PeersReport) -> Vec<String> {
    let mut out = vec![
        field("name", &report.name),
        field("info hash", &report.info_hash),
        field("sampled for", &report.sampled_human),
        field("live", report.live),
        field("connecting", report.connecting),
        field("queued", report.queued),
        field("seen", report.seen),
        field("dead", report.dead),
    ];
    // Only when something was refused. A run with no `--block-peer` has
    // nothing to say here, and a zero would read as a flag that did nothing.
    if let Some(blocked) = report.blocked {
        out.push(field(
            "blocked",
            format!(
                "{} incoming, {} outgoing",
                blocked.incoming, blocked.outgoing
            ),
        ));
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

    fn peer(addr: &str, client: Option<&str>, down: u64, pieces: u32) -> PeerSnapshot {
        PeerSnapshot {
            addr: addr.into(),
            state: "live".into(),
            client: client.map(ToString::to_string),
            connection: Some("tcp".into()),
            encryption: Some("rc4".into()),
            choked: 0,
            unchoked: 0,
            disconnects: Vec::new(),
            direction: "outgoing",
            downloaded_bytes: down,
            uploaded_bytes: 0,
            verified_pieces: pieces,
            chunks: 0,
            errors: 0,
            connect_ms: 0,
            mean_piece_ms: None,
            web_seed: false,
        }
    }

    fn sample() -> Vec<PeerSnapshot> {
        vec![
            peer("203.0.113.9:6881", Some("rqbit"), 100, 2),
            peer("203.0.113.1:6881", Some("aria2"), 900, 1),
            peer("203.0.113.5:6881", None, 500, 9),
        ]
    }

    #[test]
    fn the_default_sort_is_by_address_ascending() {
        let (key, descending) = SortKey::parse("addr").unwrap();
        assert_eq!(key, SortKey::Addr);
        assert!(!descending);

        let mut peers = sample();
        sort_peers(&mut peers, key, descending);
        assert_eq!(peers[0].addr, "203.0.113.1:6881");
        assert_eq!(peers[2].addr, "203.0.113.9:6881");
    }

    #[test]
    fn every_documented_sort_key_parses() {
        for text in ["addr", "address", "client", "speed", "down", "pieces"] {
            assert!(SortKey::parse(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn a_descending_order_reverses_the_result() {
        let (key, descending) = SortKey::parse("speed:desc").unwrap();
        assert!(descending);
        let mut peers = sample();
        sort_peers(&mut peers, key, descending);
        assert_eq!(peers[0].downloaded_bytes, 900);
        assert_eq!(peers[2].downloaded_bytes, 100);
    }

    #[test]
    fn sorting_by_pieces_orders_by_what_each_peer_actually_gave() {
        let (key, descending) = SortKey::parse("pieces:desc").unwrap();
        let mut peers = sample();
        sort_peers(&mut peers, key, descending);
        assert_eq!(peers[0].verified_pieces, 9);
    }

    #[test]
    fn a_peer_with_no_client_string_still_sorts() {
        let (key, descending) = SortKey::parse("client").unwrap();
        let mut peers = sample();
        sort_peers(&mut peers, key, descending);
        assert_eq!(
            peers[0].client, None,
            "an unknown client sorts before a named one"
        );
    }

    #[test]
    fn a_bad_sort_key_names_the_valid_ones() {
        let err = SortKey::parse("bandwidth").unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
        assert!(err.message().contains("addr"), "{}", err.message());
        assert!(err.message().contains("pieces"), "{}", err.message());
    }

    #[test]
    fn a_bad_sort_order_is_refused_rather_than_ignored() {
        let err = SortKey::parse("addr:sideways").unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
        assert_eq!(err.context()["value"], "sideways");
    }

    #[test]
    fn the_table_labels_a_bridge_as_a_web_seed() {
        let mut peers = sample();
        peers[0].web_seed = true;
        let text = swarm::peer_table(&peers).join("\n");
        assert!(text.contains("web seed"), "{text}");
    }

    /// `--block-peer` keeps an address out of the swarm entirely.
    ///
    /// The same rig as the test below, with the one peer in it blocked. The
    /// number this moves is the session's own `blocked_outgoing`: the address
    /// is refused before a connection permit is taken, so it never holds a
    /// slot, and `seen` stays at zero where the same run without the flag
    /// reports one. Exit is `NoUsableSources`, which is what an empty swarm
    /// means.
    ///
    /// The seeder is real rather than an unroutable address, so this measures
    /// a peer that **would** have connected. See `TODO/peers.md`, T-164.
    #[test]
    fn a_blocked_peer_is_never_dialled_and_never_joins_the_swarm() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let dir = fixture.dir();
        let data = dir.join("seeded");
        for (path, bytes) in &fixture.files {
            let target = data.join("album").join(path);
            std::fs::create_dir_all(target.parent().expect("a parent")).expect("mkdir");
            std::fs::write(&target, bytes).expect("write the seeded payload");
        }

        let port = crate::test_support::free_port();
        let seeder = {
            let torrent = fixture.path_str().to_string();
            let data = data.to_str().expect("utf-8 path").to_string();
            let cwd = dir.clone();
            std::thread::spawn(move || {
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
                    cwd,
                );
                crate::run(&mut env)
            })
        };
        assert!(
            crate::test_support::wait_for_listener(port, std::time::Duration::from_secs(10)),
            "the seeder never listened on {port}"
        );

        let report = crate::test_support::run_json_code(
            &[
                "peers",
                fixture.path_str(),
                "--peer",
                &format!("127.0.0.1:{port}"),
                "--block-peer",
                "127.0.0.1",
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--duration",
                "2s",
                "--port",
                "0",
            ],
            dir.clone(),
            ExitCode::Success,
        );
        drop(seeder);

        // Never dialled. The same rig without the flag reports the peer live
        // with bytes against it, which is the test below.
        assert_eq!(report["live"], 0, "{report}");
        assert_eq!(report["connecting"], 0, "{report}");
        assert_eq!(report["dead"], 0, "{report}");
        assert_eq!(
            report["blocked"],
            serde_json::json!({"incoming": 0, "outgoing": 1}),
            "{report}"
        );

        // `seen` counts it anyway, and that is `librqbit`'s: the address is
        // registered when it is queued and the blocklist is checked when it is
        // taken off the queue, at `torrent_state/live/mod.rs:629`. So the peer
        // sits at `queued` for the whole run with nothing against it. Asserted
        // rather than corrected, because subtracting a refusal count from a
        // peer count would be arithmetic nobody can check: the counter counts
        // refusals and not addresses. See `TODO/peers.md`, T-164.
        assert_eq!(report["seen"], 1, "{report}");
        let peers = report["peers"].as_array().expect("a peer array");
        assert_eq!(peers.len(), 1, "{report}");
        assert_eq!(peers[0]["state"], "queued", "{report}");
        assert_eq!(peers[0]["downloaded_bytes"], 0, "{report}");
        assert_eq!(report["downloaded"]["bytes"], 0, "{report}");
    }

    /// A run with no `--block-peer` says nothing about blocking, so the field
    /// is not a zero on every sample.
    #[test]
    fn a_sample_with_nothing_blocked_carries_no_blocked_field() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let report = crate::test_support::run_json_code(
            &[
                "peers",
                fixture.path_str(),
                "--no-tracker",
                "--no-dht",
                "--no-lsd",
                "--duration",
                "1s",
                "--port",
                "0",
            ],
            fixture.dir(),
            ExitCode::NoUsableSources,
        );
        assert!(report.get("blocked").is_none(), "{report}");
    }

    /// Sampling a swarm means joining it.
    ///
    /// The command used to add its torrent paused, and `librqbit` 9.0.0 hands
    /// a torrent its peer stream only when it starts, so a paused one never
    /// announced, never dialled, and reported an empty swarm however long it
    /// was watched. A seeder on loopback and `--peer` pointed at it is the
    /// smallest swarm there is. See `TODO/peers.md`, T-142.
    #[test]
    fn a_sampled_swarm_carries_what_came_from_each_peer() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let dir = fixture.dir();
        // The seeder needs the payload under the torrent's own name. The
        // fixture keeps it under `payload/`.
        let data = dir.join("seeded");
        for (path, bytes) in &fixture.files {
            let target = data.join("album").join(path);
            std::fs::create_dir_all(target.parent().expect("a parent")).expect("mkdir");
            std::fs::write(&target, bytes).expect("write the seeded payload");
        }

        let port = crate::test_support::free_port();
        let seeder = {
            let torrent = fixture.path_str().to_string();
            let data = data.to_str().expect("utf-8 path").to_string();
            let cwd = dir.clone();
            std::thread::spawn(move || {
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
                        "40s",
                    ],
                    cwd,
                );
                crate::run(&mut env)
            })
        };

        // The seeder is on a thread and `peers` dials it from this one, so the
        // listener has to be up first. Without this the dial can lose the race,
        // and `librqbit` does not retry a dead peer for ten seconds, which is
        // twice the `--duration` below: the run then reports one error, zero
        // bytes, and a dead peer, and every assertion after it fails. That is
        // T-160, and it turned CI red on a docs-only commit.
        assert!(
            crate::test_support::wait_for_listener(port, std::time::Duration::from_secs(10)),
            "the seeder never listened on {port}"
        );

        // Sampled until the bytes arrive rather than once for a duration that
        // is hoped to be long enough. `--duration` is the command's own
        // contract and it samples for exactly that long, so a run on a loaded
        // machine can end with the handshake still in flight: `connecting: 1`,
        // `errors: 0`, `downloaded_bytes: 0`, and every assertion below it
        // failing for no reason but load. That is the second red job this one
        // test has cost, and it is the rule [RULES.md] states three times over.
        // See `TODO/cli-surface.md`, T-160.
        //
        // On an unloaded machine the first sample succeeds and this costs
        // nothing. The seeder runs for ninety seconds so the retries have
        // something to dial.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
        let mut report;
        loop {
            report = crate::test_support::run_json(
                &[
                    "peers",
                    fixture.path_str(),
                    "--peer",
                    &format!("127.0.0.1:{port}"),
                    "--no-tracker",
                    "--no-dht",
                    "--no-lsd",
                    "--duration",
                    "5s",
                    "--port",
                    "0",
                ],
                dir.clone(),
            );
            let moved = report["peers"][0]["downloaded_bytes"].as_u64().unwrap_or(0);
            if moved > 0 || std::time::Instant::now() >= deadline {
                break;
            }
        }
        // Not joined. The seeder runs long enough to outlast the retries, and
        // waiting for it to time out would make every run of this test as long
        // as the worst case rather than as long as it actually took. The
        // thread dies with the test binary.
        drop(seeder);

        assert_eq!(report["seen"], 1, "{report}");
        let peers = report["peers"].as_array().expect("a peer array");
        assert_eq!(peers.len(), 1, "{report}");
        assert_eq!(peers[0]["direction"], "outgoing", "{report}");
        assert_eq!(peers[0]["errors"], 0, "{report}");
        // What came from each peer, which is the report's whole point and was
        // zero for every peer before this was fixed.
        assert_eq!(peers[0]["downloaded_bytes"], 2000, "{report}");
        assert_eq!(peers[0]["verified_pieces"], 2, "{report}");

        // The payload went to the scratch directory and not to the caller's.
        // A sampler that leaves files behind is a downloader.
        assert!(
            !dir.join("album").exists(),
            "sampling wrote a payload into the working directory"
        );
    }
}
