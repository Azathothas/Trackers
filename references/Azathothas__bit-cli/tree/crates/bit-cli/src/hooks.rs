//! Running the `--on-*` hooks, and the variables they receive.
//!
//! Three flags: `--on-complete` and `--on-error`, once per **torrent**, and
//! `--on-piece-verified`, once per verified piece. See `TODO/cli-surface.md`,
//! T-115.
//!
//! **Nothing torrent-supplied is ever interpolated into a command line.** The
//! command is run as written and every fact arrives as a `BIT_CLI_*`
//! environment variable, so a file named `; rm -rf /` is a file name and not a
//! command. [`crate::swarm::run_hook`] is the one place a process is started
//! and it takes the variables as a map for exactly that reason.
//!
//! **`--on-piece-verified` is high frequency by construction**, and that is why
//! [`PieceHook`] exists rather than a call in the watch loop. One piece is one
//! process: a 4 GiB torrent at a 1 MiB piece length is 4,096 of them. Two
//! bounds keep that from deciding how fast a download goes.
//!
//! 1. **It runs on its own thread.** The watch loop hands over a map and
//!    returns; it never waits for a process to exit. Without this a hook that
//!    takes 20 ms would cap the download at 50 pieces a second whatever the
//!    network could do.
//! 2. **The queue is bounded**, and what does not fit is **counted** rather
//!    than waited for or silently dropped. A hook slower than pieces arrive is
//!    the caller's to fix, and the report says how many invocations it cost.
//!    `--json` carries `hooks`, and a run that skipped any says so on stderr.
//!
//! `docs/hooks.md` is the documented surface and
//! `every_hook_variable_is_documented` fails when this file and that file
//! disagree, the same way `docs/flags.md` is held to the `clap` tree.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

/// How many piece hook invocations may be waiting to run.
///
/// Large enough that an ordinary hook never fills it, small enough that a hook
/// which has stopped answering cannot grow this process without bound. At the
/// 16 KiB minimum piece length this is 16 MiB of payload's worth of pieces.
const QUEUE: usize = 1024;

/// What the hooks did, for the report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct HookCounts {
    /// Invocations that ran to completion, whatever they exited with.
    pub ran: u64,
    /// Invocations that exited non-zero.
    pub failed: u64,
    /// Invocations not made because the queue was full.
    ///
    /// Never silent: a run with any is warned about, and this is the number.
    pub skipped: u64,
}

impl HookCounts {
    /// Whether anything happened at all, so a report can leave the block out.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A `--on-piece-verified` hook, running on its own thread.
pub struct PieceHook {
    tx: Option<std::sync::mpsc::SyncSender<BTreeMap<String, String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
    skipped: Arc<AtomicU64>,
    ran: Arc<AtomicU64>,
    failed: Arc<AtomicU64>,
}

impl PieceHook {
    /// Start the worker for `command`.
    pub fn start(command: String) -> Self {
        Self::with_capacity(command, QUEUE)
    }

    /// [`Self::start`] with a queue of a given size.
    ///
    /// For the test that drives the bound. Filling a 1,024 entry queue means
    /// starting more than a thousand processes, which was 47.55 seconds when
    /// this was written and measures the operating system rather than this
    /// code. `docs/hooks.md` carries that number and what it was a measurement
    /// of.
    fn with_capacity(command: String, capacity: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<BTreeMap<String, String>>(capacity);
        let ran = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicU64::new(0));
        let counted_ran = ran.clone();
        let counted_failed = failed.clone();
        let thread = std::thread::spawn(move || {
            for vars in rx {
                match crate::swarm::run_hook(&command, &vars) {
                    Ok(0) => {}
                    // A hook that exits non-zero is counted and does not stop
                    // the run. The download is what the caller asked for; the
                    // hook is a notification about it.
                    Ok(_) | Err(_) => {
                        counted_failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
                counted_ran.fetch_add(1, Ordering::Relaxed);
            }
        });
        Self {
            tx: Some(tx),
            thread: Some(thread),
            skipped: Arc::new(AtomicU64::new(0)),
            ran,
            failed,
        }
    }

    /// Queue one invocation, or count it as skipped.
    ///
    /// Never blocks. The watch loop that calls this is what decides how fast
    /// the download goes, and a hook is not allowed a vote.
    pub fn fire(&self, vars: BTreeMap<String, String>) {
        let Some(tx) = &self.tx else { return };
        if tx.try_send(vars).is_err() {
            self.skipped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Stop taking work, wait for what is queued, and report what happened.
    pub fn finish(mut self) -> HookCounts {
        // Dropping the sender is what ends the worker's loop.
        self.tx = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        HookCounts {
            ran: self.ran.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
        }
    }
}

impl Drop for PieceHook {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Where `docs/hooks.md` lives, relative to the repository root.
pub const HOOKS_PATH: &str = "docs/hooks.md";

/// Every variable a hook receives, with what it holds.
///
/// The one list. `docs/hooks.md` is checked against it, and the two builders
/// below are checked against it, so a variable cannot be added in one place and
/// missed in the other two. See `TODO/cli-surface.md`, T-115.
pub const VARIABLES: &[(&str, &str)] = &[
    ("BIT_CLI_VERSION", "The version of `bit-cli` that ran."),
    (
        "BIT_CLI_HOOK",
        "Which hook this is: `on-complete`, `on-error`, or `on-piece-verified`.",
    ),
    (
        "BIT_CLI_INFO_HASH",
        "The torrent's info hash, lower-case hex.",
    ),
    ("BIT_CLI_NAME", "The torrent's name."),
    (
        "BIT_CLI_SOURCE",
        "The source as it was given on the command line.",
    ),
    (
        "BIT_CLI_DIR",
        "The directory this torrent's payload was written to.",
    ),
    (
        "BIT_CLI_TOTAL_BYTES",
        "The torrent's total length. **This torrent's**, not the run's.",
    ),
    (
        "BIT_CLI_DOWNLOADED_BYTES",
        "What arrived for this torrent, from every source.",
    ),
    (
        "BIT_CLI_FROM_PEERS_BYTES",
        "What arrived from the swarm for this torrent.",
    ),
    (
        "BIT_CLI_FROM_WEB_SEEDS_BYTES",
        "What arrived from HTTP sources for this torrent.",
    ),
    (
        "BIT_CLI_FINISHED",
        "`true` when every selected piece verified, `false` otherwise.",
    ),
    (
        "BIT_CLI_STOPPED",
        "Why this torrent stopped: `completed`, `timeout`, `stalled`, and so on.",
    ),
    (
        "BIT_CLI_ELAPSED_MS",
        "How long this torrent took, in milliseconds.",
    ),
    (
        "BIT_CLI_ERROR",
        "The failure, when there was one. Absent on success rather than empty.",
    ),
    (
        "BIT_CLI_TORRENTS",
        "How many torrents the whole run was asked for.",
    ),
    ("BIT_CLI_COMPLETED", "How many of them finished."),
    ("BIT_CLI_FAILED", "How many did not."),
    (
        "BIT_CLI_RUN_ELAPSED_MS",
        "How long the whole run took, in milliseconds.",
    ),
    (
        "BIT_CLI_PIECE",
        "`--on-piece-verified` only: the piece index that just verified.",
    ),
    (
        "BIT_CLI_PIECE_LENGTH",
        "`--on-piece-verified` only: that piece's length in bytes. The last piece of a torrent is usually shorter than the rest.",
    ),
];

/// The variables every hook receives, whichever one it is.
fn common(hook: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    vars.insert(
        "BIT_CLI_VERSION".to_string(),
        bit_cli_core::VERSION.to_string(),
    );
    vars.insert("BIT_CLI_HOOK".to_string(), hook.to_string());
    vars
}

/// The environment `--on-piece-verified` receives.
///
/// Deliberately thin: it fires once per piece, so anything expensive to
/// compute here is computed thousands of times. Everything that identifies the
/// torrent is here and nothing that describes its progress is, because a
/// progress figure read per piece is a figure that changed before the hook
/// could read it.
pub fn piece_vars(
    info_hash: &str,
    name: &str,
    directory: &str,
    piece: u32,
    piece_length: u64,
) -> BTreeMap<String, String> {
    let mut vars = common("on-piece-verified");
    vars.insert("BIT_CLI_INFO_HASH".to_string(), info_hash.to_string());
    vars.insert("BIT_CLI_NAME".to_string(), name.to_string());
    vars.insert("BIT_CLI_DIR".to_string(), directory.to_string());
    vars.insert("BIT_CLI_PIECE".to_string(), piece.to_string());
    vars.insert("BIT_CLI_PIECE_LENGTH".to_string(), piece_length.to_string());
    vars
}

/// What one torrent's `--on-complete` or `--on-error` receives.
///
/// One argument per fact rather than the report struct, so this module does not
/// depend on `cmd::download`'s shapes and the test below can call it.
pub struct Finished<'a> {
    pub info_hash: &'a str,
    pub name: &'a str,
    pub source: &'a str,
    pub directory: &'a str,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub from_peers_bytes: u64,
    pub from_web_seeds_bytes: u64,
    pub finished: bool,
    pub stopped: &'a str,
    pub elapsed_ms: u64,
    pub error: Option<&'a str>,
    /// The run this torrent was part of, so a hook firing four times can tell
    /// where it is in the run without being told by its own caller.
    pub torrents: usize,
    pub completed: usize,
    pub failed: usize,
    pub run_elapsed_ms: u64,
}

/// The environment `--on-complete` and `--on-error` receive.
///
/// Which of the two fired follows `finished`, because on a download they are
/// the same question. On a seeder they are not: a partial seed is a legitimate
/// thing to be serving, so [`hook_vars`] takes the name instead.
pub fn finished_vars(one: &Finished<'_>) -> BTreeMap<String, String> {
    hook_vars(
        match one.finished {
            true => "on-complete",
            false => "on-error",
        },
        one,
    )
}

/// The same, for a caller that knows which hook it is firing.
///
/// `seed` fires `--on-complete` when the hash check has passed and the
/// listener is up, which is the only moment a seeder has, and `finished` there
/// says whether the payload is whole rather than whether the run succeeded.
/// See `TODO/cli-surface.md`, T-214.
pub fn hook_vars(hook: &str, one: &Finished<'_>) -> BTreeMap<String, String> {
    let mut vars = common(hook);
    vars.insert("BIT_CLI_INFO_HASH".to_string(), one.info_hash.to_string());
    vars.insert("BIT_CLI_NAME".to_string(), one.name.to_string());
    vars.insert("BIT_CLI_SOURCE".to_string(), one.source.to_string());
    vars.insert("BIT_CLI_DIR".to_string(), one.directory.to_string());
    vars.insert(
        "BIT_CLI_TOTAL_BYTES".to_string(),
        one.total_bytes.to_string(),
    );
    vars.insert(
        "BIT_CLI_DOWNLOADED_BYTES".to_string(),
        one.downloaded_bytes.to_string(),
    );
    vars.insert(
        "BIT_CLI_FROM_PEERS_BYTES".to_string(),
        one.from_peers_bytes.to_string(),
    );
    vars.insert(
        "BIT_CLI_FROM_WEB_SEEDS_BYTES".to_string(),
        one.from_web_seeds_bytes.to_string(),
    );
    vars.insert("BIT_CLI_FINISHED".to_string(), one.finished.to_string());
    vars.insert("BIT_CLI_STOPPED".to_string(), one.stopped.to_string());
    vars.insert("BIT_CLI_ELAPSED_MS".to_string(), one.elapsed_ms.to_string());
    // Absent rather than empty on success. A hook testing `if [ -n
    // "$BIT_CLI_ERROR" ]` is the obvious thing to write, and an empty string
    // set on every successful run would make it work by accident rather than
    // by contract.
    if let Some(error) = one.error {
        vars.insert("BIT_CLI_ERROR".to_string(), error.to_string());
    }
    vars.insert("BIT_CLI_TORRENTS".to_string(), one.torrents.to_string());
    vars.insert("BIT_CLI_COMPLETED".to_string(), one.completed.to_string());
    vars.insert("BIT_CLI_FAILED".to_string(), one.failed.to_string());
    vars.insert(
        "BIT_CLI_RUN_ELAPSED_MS".to_string(),
        one.run_elapsed_ms.to_string(),
    );
    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished() -> Finished<'static> {
        Finished {
            info_hash: "aa",
            name: "payload",
            source: "a.torrent",
            directory: "/out",
            total_bytes: 10,
            downloaded_bytes: 10,
            from_peers_bytes: 4,
            from_web_seeds_bytes: 6,
            finished: true,
            stopped: "completed",
            elapsed_ms: 5,
            error: None,
            torrents: 2,
            completed: 2,
            failed: 0,
            run_elapsed_ms: 9,
        }
    }

    /// The one list and the two builders cannot drift apart: every variable
    /// either builder sets is in [`VARIABLES`]. T-115.
    #[test]
    fn every_variable_a_hook_sets_is_in_the_list() {
        let known: Vec<&str> = VARIABLES.iter().map(|(name, _)| *name).collect();
        let mut failing = finished();
        failing.finished = false;
        failing.error = Some("it did not work");
        let sets = [
            finished_vars(&finished()),
            finished_vars(&failing),
            piece_vars("aa", "payload", "/out", 3, 16384),
        ];
        for vars in &sets {
            for name in vars.keys() {
                assert!(known.contains(&name.as_str()), "{name} is not in VARIABLES");
            }
        }
        // And the other way, so a variable can be retired from the code
        // without leaving a row behind: every documented name is set by one of
        // the three shapes above.
        for (name, _) in VARIABLES {
            assert!(
                sets.iter().any(|vars| vars.contains_key(*name)),
                "{name} is documented and nothing sets it"
            );
        }
    }

    /// `docs/hooks.md` is the surface a caller reads, and a table nothing
    /// checks drifts within a week. Same rule as `docs/flags.md`, T-118.
    #[test]
    fn every_hook_variable_is_documented() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(HOOKS_PATH);
        let doc = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        for (name, _) in VARIABLES {
            assert!(
                doc.contains(&format!("`{name}`")),
                "{} has no row for {name}",
                HOOKS_PATH
            );
        }
    }

    #[test]
    fn a_successful_hook_carries_no_error_variable() {
        let vars = finished_vars(&finished());
        assert!(!vars.contains_key("BIT_CLI_ERROR"));
        assert_eq!(vars["BIT_CLI_HOOK"], "on-complete");
        // This torrent's total, not the run's, which is what T-115 is about.
        assert_eq!(vars["BIT_CLI_TOTAL_BYTES"], "10");
        assert_eq!(vars["BIT_CLI_TORRENTS"], "2");
    }

    #[test]
    fn a_failed_hook_names_the_error_and_says_which_hook_it_is() {
        let mut one = finished();
        one.finished = false;
        one.stopped = "timeout";
        one.error = Some("the deadline passed");
        let vars = finished_vars(&one);
        assert_eq!(vars["BIT_CLI_HOOK"], "on-error");
        assert_eq!(vars["BIT_CLI_FINISHED"], "false");
        assert_eq!(vars["BIT_CLI_STOPPED"], "timeout");
        assert_eq!(vars["BIT_CLI_ERROR"], "the deadline passed");
    }

    /// The queue is bounded and what does not fit is counted rather than
    /// waited for. Driven with a command that exists on both platforms and
    /// does nothing, so what is measured is the bound and not the command.
    #[test]
    fn a_piece_hook_counts_what_it_could_not_queue() {
        // `run_hook` already wraps the command in `cmd /C` or `sh -c`, so
        // this is the shell's own do-nothing rather than a second shell.
        let capacity = 4;
        let hook = PieceHook::with_capacity(
            match cfg!(windows) {
                true => "rem".to_string(),
                false => "true".to_string(),
            },
            capacity,
        );
        let wanted = (capacity as u64) * 8;
        for piece in 0..wanted {
            hook.fire(piece_vars("aa", "payload", "/out", piece as u32, 16384));
        }
        let counts = hook.finish();
        // Everything is accounted for: nothing vanishes between `fire` and the
        // report, whichever side of the bound it landed on.
        assert_eq!(
            counts.ran + counts.skipped,
            wanted,
            "ran {} skipped {}",
            counts.ran,
            counts.skipped
        );
        assert_eq!(counts.failed, 0);
    }

    #[test]
    fn no_counts_at_all_is_an_empty_block() {
        assert!(HookCounts::default().is_empty());
        assert!(
            !HookCounts {
                ran: 1,
                ..Default::default()
            }
            .is_empty()
        );
    }
}
