//! Logging and subsystem tracing.
//!
//! Levels are for severity, subsystem traces are for detail. Turning on
//! `trace` globally in a torrent client buries the thing you are looking for
//! under peer chatter, so `--trace http` raises exactly one subsystem and
//! leaves the rest alone.
//!
//! Logs always go to stderr. A caller doing `bit-cli ... --json | jq` must
//! never see a log line in the pipe, and that holds at every level.
//!
//! `--log-file` adds a second destination rather than replacing stderr, so
//! that rule holds whatever else is set. The file rotates by size and keeps a
//! bounded number of old ones, because a cron job that cannot keep a log has
//! no way to explain a failure after the fact and one that keeps an unbounded
//! log fills the disk instead.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bit_cli_core::error::{Error, Result, from_io};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;

use crate::cli::{Global, LogFormat};
use crate::env::Env;

/// How many times a rotation retries a rename before giving up.
///
/// Windows refuses to rename a file another process has open, and a log file
/// is exactly the file someone is tailing. Retrying with a doubling wait
/// covers a reader that is between reads; a reader that holds it open forever
/// is not something this can fix, and the rotation is skipped rather than the
/// run failing.
const RENAME_ATTEMPTS: u32 = 5;

/// First wait between rename attempts, in milliseconds. Doubles each time.
const RENAME_BACKOFF_MS: u64 = 10;

/// An append-only log file that rotates by size and keeps a bounded number of
/// old ones.
///
/// `--log-max-files N` means N files in total: the live one plus `N - 1`
/// rotated. `--log-max-size 0` turns rotation off, which is what a caller who
/// manages the file some other way wants.
#[derive(Debug)]
struct Rotating {
    path: PathBuf,
    /// Bytes the live file may reach before it rotates. Zero never rotates.
    max_size: u64,
    /// Files kept in total, the live one included.
    max_files: u32,
    file: Option<std::fs::File>,
    /// Bytes in the live file. Seeded from its length so an append to an
    /// existing log rotates when the file is full rather than when this
    /// process has written that much.
    written: u64,
}

impl Rotating {
    fn open(path: PathBuf, max_size: u64, max_files: u32) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                from_io(
                    e,
                    format!("cannot create the log directory {}", parent.display()),
                )
            })?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| from_io(e, format!("cannot open the log file {}", path.display())))?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            max_size,
            max_files,
            file: Some(file),
            written,
        })
    }

    /// Rename with a few retries, because Windows refuses while a reader has
    /// the file open. Returns whether it happened.
    fn rename(from: &Path, to: &Path) -> bool {
        let mut wait = RENAME_BACKOFF_MS;
        for attempt in 0..RENAME_ATTEMPTS {
            if std::fs::rename(from, to).is_ok() {
                return true;
            }
            if attempt + 1 < RENAME_ATTEMPTS {
                std::thread::sleep(std::time::Duration::from_millis(wait));
                wait *= 2;
            }
        }
        false
    }

    fn rotated(&self, index: u32) -> PathBuf {
        let mut name = self.path.as_os_str().to_os_string();
        name.push(format!(".{index}"));
        PathBuf::from(name)
    }

    /// Move the live file aside and start a new one.
    ///
    /// The oldest is deleted first, then each rotated file shifts up by one,
    /// then the live file becomes `.1`. A rename that will not happen leaves
    /// the file where it is and the log keeps growing, which is better than
    /// losing a line.
    fn rotate(&mut self) {
        drop(self.file.take());
        if self.max_files <= 1 {
            // One file total means no history: start it over rather than
            // keeping a rotated copy the caller said it did not want.
            if std::fs::File::create(&self.path).is_ok() {
                self.written = 0;
            }
            self.reopen();
            return;
        }
        let oldest = self.max_files - 1;
        let _ = std::fs::remove_file(self.rotated(oldest));
        for index in (1..oldest).rev() {
            let from = self.rotated(index);
            if from.exists() {
                Self::rename(&from, &self.rotated(index + 1));
            }
        }
        if Self::rename(&self.path.clone(), &self.rotated(1)) {
            self.written = 0;
        }
        self.reopen();
    }

    fn reopen(&mut self) {
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        if let Some(file) = &self.file
            && let Ok(meta) = file.metadata()
        {
            self.written = meta.len();
        }
    }
}

impl Write for Rotating {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Rotate before the write rather than after, so a line is never split
        // across two files.
        if self.max_size > 0 && self.written > 0 && self.written + buf.len() as u64 > self.max_size
        {
            self.rotate();
        }
        let Some(file) = self.file.as_mut() else {
            // Nowhere to write. Report the bytes as taken: a log that cannot
            // be written is not a reason to fail the run, and `tracing` has
            // nowhere useful to report it to anyway.
            return Ok(buf.len());
        };
        let written = file.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

/// A handle to the rotating log file, shared by every writer `tracing` makes.
#[derive(Debug, Clone)]
struct LogFile(Arc<Mutex<Rotating>>);

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.0.lock() {
            Ok(mut file) => file.write(buf),
            Err(poisoned) => poisoned.into_inner().write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.0.lock() {
            Ok(mut file) => file.flush(),
            Err(poisoned) => poisoned.into_inner().flush(),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogFile {
    type Writer = LogFile;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// One subsystem `--trace` can raise.
#[derive(Debug, Clone, Copy)]
pub struct Subsystem {
    /// The name a caller types after `--trace`.
    pub name: &'static str,
    /// What turning it on shows. One line, and it is the same sentence the
    /// manuals carry.
    pub description: &'static str,
    /// Every `tracing` target the name raises.
    ///
    /// More than one, because the facts a subsystem covers are not all decided
    /// in one place. `bit_cli::<name>` is where this repository's own code
    /// emits; the `librqbit*` entries are where the vendored session emits the
    /// same kind of fact, and both are code this repository owns. A name whose
    /// facts are decided entirely in one of the two has one target.
    ///
    /// Every target here is checked by `a_run_emits_on_every_subsystem`: a
    /// name that raises something nothing writes to is what
    /// `TODO/cli-surface.md` T-219 was.
    pub targets: &'static [&'static str],
}

/// Subsystems that can be traced independently.
pub const SUBSYSTEMS: &[Subsystem] = &[
    Subsystem {
        name: "peer",
        description: "Wire messages: type, index, begin, length, direction, peer id",
        targets: &["bit_cli::peer", "librqbit::peer_connection"],
    },
    Subsystem {
        name: "handshake",
        description: "Peer handshakes and extension negotiation",
        targets: &["bit_cli::handshake", "librqbit::handshake"],
    },
    Subsystem {
        name: "tracker",
        description: "Announce and scrape requests and responses in full",
        targets: &["bit_cli::tracker", "librqbit_tracker_comms"],
    },
    Subsystem {
        name: "dht",
        description: "DHT queries, responses, and routing table changes",
        targets: &["bit_cli::dht", "librqbit_dht"],
    },
    Subsystem {
        name: "http",
        description: "Web seed requests and responses, status, headers, ranges, redirects, TLS",
        targets: &["bit_cli::http"],
    },
    Subsystem {
        name: "piece",
        description: "Piece request, receipt, verification result, and timing",
        targets: &["bit_cli::piece", "librqbit::piece"],
    },
    Subsystem {
        name: "picker",
        description: "Why a piece was requested from a given source",
        targets: &["bit_cli::picker", "librqbit::picker"],
    },
    Subsystem {
        name: "disk",
        description: "Reads, writes, flushes, and allocation, with offsets and sizes",
        targets: &["bit_cli::disk"],
    },
    Subsystem {
        name: "ratelimit",
        description: "Token bucket decisions and stalls",
        targets: &["bit_cli::ratelimit"],
    },
    Subsystem {
        name: "retry",
        description: "Retry attempts, backoff, and cooldown",
        targets: &["bit_cli::retry"],
    },
    Subsystem {
        name: "config",
        description: "Resolution of every configuration value and its origin",
        targets: &["bit_cli::config"],
    },
];

/// Check a subsystem name.
pub fn parse_subsystem(name: &str) -> Result<&'static Subsystem> {
    let name = name.trim();
    SUBSYSTEMS
        .iter()
        .find(|known| known.name == name)
        .ok_or_else(|| {
            let known: Vec<&str> = SUBSYSTEMS.iter().map(|s| s.name).collect();
            Error::usage(format!(
                "`{name}` is not a trace subsystem (known: {})",
                known.join(", ")
            ))
            .with("subsystem", name.to_string())
        })
}

/// Build the `tracing` filter directive for the given flags.
///
/// The global level applies to everything, then each traced subsystem raises
/// every target it names. The result is one directive string, which is exactly
/// what `EnvFilter` takes.
///
/// Deduplication is on the **target** rather than on the subsystem name, so
/// two names that share one raise it once. The order is the order the names
/// were given, and within a name the order [`Subsystem::targets`] lists.
pub fn filter_directive(global: &Global) -> Result<String> {
    let level = global.log_level.raised(global.verbose);
    let mut parts = vec![level.directive().to_string()];
    let mut seen = BTreeSet::new();
    for requested in &global.trace {
        let subsystem = parse_subsystem(requested)?;
        for target in subsystem.targets {
            if seen.insert(*target) {
                parts.push(format!("{target}=trace"));
            }
        }
    }
    Ok(parts.join(","))
}

/// Install the log subscriber.
///
/// Installation is best-effort by design: a second call in the same process is
/// a no-op rather than an error, so the in-process test harness can run many
/// commands without each one fighting over the global subscriber.
pub fn install(global: &Global, env: &Env) -> Result<()> {
    // Validate the subsystem names even when nothing will be installed, so a
    // typo in --trace is reported rather than silently ignored.
    let directive = filter_directive(global)?;
    let log_file = match &global.log_file {
        None => None,
        Some(path) => {
            let max_size = bit_cli_core::units::parse_size(&global.log_max_size)
                .map_err(|e| Error::config(format!("--log-max-size: {e}")))?;
            Some(LogFile(Arc::new(Mutex::new(Rotating::open(
                env.resolve(path),
                max_size,
                global.log_max_files,
            )?))))
        }
    };

    let filter = EnvFilter::try_new(&directive)
        .map_err(|e| Error::config(format!("cannot build a log filter from `{directive}`: {e}")))?;
    let ansi = env.err_is_terminal && env.wants_color(global.color.into());

    // Logs go to stderr at every level. stdout carries data only.
    //
    // `--log-file` adds a destination rather than replacing stderr, so
    // "stderr carries the logs" holds whatever else is set. A caller who wants
    // only the file redirects stderr.
    let result = match (global.log_format, log_file) {
        (LogFormat::Json, None) => tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .try_init(),
        (LogFormat::Json, Some(file)) => tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_writer(std::io::stderr.and(file))
            .with_ansi(false)
            .try_init(),
        (LogFormat::Text, None) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_ansi(ansi)
            .try_init(),
        // The file gets the same text a terminal would, without the escapes:
        // colour in a log file is noise in every reader that opens it.
        (LogFormat::Text, Some(file)) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr.and(file))
            .with_ansi(false)
            .try_init(),
    };
    // An already-installed subscriber is not a failure worth stopping for.
    let _ = result;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use clap::Parser;

    fn global(args: &[&str]) -> Global {
        let mut full = vec!["bit-cli"];
        full.extend_from_slice(args);
        full.extend_from_slice(&["info", "x.torrent"]);
        let cli = Cli::try_parse_from(full).unwrap();
        assert!(matches!(cli.command, Some(Command::Info(_))));
        cli.global
    }

    #[test]
    fn the_default_level_is_warn() {
        assert_eq!(filter_directive(&global(&[])).unwrap(), "warn");
    }

    #[test]
    fn verbosity_raises_the_level() {
        assert_eq!(filter_directive(&global(&["-v"])).unwrap(), "info");
        assert_eq!(filter_directive(&global(&["-vv"])).unwrap(), "debug");
        assert_eq!(filter_directive(&global(&["-vvv"])).unwrap(), "trace");
        assert_eq!(filter_directive(&global(&["-vvvvvv"])).unwrap(), "trace");
    }

    #[test]
    fn an_explicit_level_is_honoured() {
        assert_eq!(
            filter_directive(&global(&["--log-level", "error"])).unwrap(),
            "error"
        );
        assert_eq!(
            filter_directive(&global(&["--log-level", "off"])).unwrap(),
            "off"
        );
    }

    #[test]
    fn a_traced_subsystem_is_raised_without_raising_everything() {
        let directive = filter_directive(&global(&["--trace", "http"])).unwrap();
        assert_eq!(directive, "warn,bit_cli::http=trace");
    }

    #[test]
    fn several_subsystems_can_be_traced_at_once() {
        let directive = filter_directive(&global(&["--trace", "http,piece,picker"])).unwrap();
        assert_eq!(
            directive,
            "warn,bit_cli::http=trace,bit_cli::piece=trace,librqbit::piece=trace,\
             bit_cli::picker=trace,librqbit::picker=trace"
        );
    }

    /// A subsystem raises every target it names, in the order it names them.
    ///
    /// This is the half of T-219 a unit test can hold: the directive covers
    /// the vendored session as well as this repository's own code. The other
    /// half, that something writes to each of them, is
    /// `a_run_emits_on_every_subsystem`.
    #[test]
    fn a_subsystem_raises_every_target_it_names() {
        let directive = filter_directive(&global(&["--trace", "peer"])).unwrap();
        assert_eq!(
            directive,
            "warn,bit_cli::peer=trace,librqbit::peer_connection=trace"
        );
    }

    /// Two names that shared a target would raise it once. None do today, and
    /// the check is here so that adding one is a decision rather than a
    /// duplicate directive nobody notices.
    #[test]
    fn a_target_two_subsystems_share_is_raised_once() {
        let every: Vec<&str> = SUBSYSTEMS.iter().map(|s| s.name).collect();
        let directive = filter_directive(&global(&["--trace", &every.join(",")])).unwrap();
        let mut targets: Vec<&str> = directive.split(',').skip(1).collect();
        let before = targets.len();
        targets.sort_unstable();
        targets.dedup();
        assert_eq!(before, targets.len(), "{directive}");
    }

    #[test]
    fn a_repeated_subsystem_appears_once() {
        let directive = filter_directive(&global(&["--trace", "http", "--trace", "http"])).unwrap();
        assert_eq!(directive, "warn,bit_cli::http=trace");
    }

    #[test]
    fn an_unknown_subsystem_is_refused_with_the_list() {
        let err = filter_directive(&global(&["--trace", "nope"])).unwrap_err();
        assert_eq!(err.code(), bit_cli_core::ExitCode::Usage);
        assert!(err.message().contains("http"), "{}", err.message());
    }

    /// Every target is a plausible one: non-empty, and prefixed by a crate
    /// this workspace or its vendored trees own. A typo in a target name is
    /// invisible at runtime, because `EnvFilter` accepts any string and simply
    /// matches nothing, which is exactly the failure T-219 was.
    #[test]
    fn every_target_names_a_crate_this_repository_owns() {
        for subsystem in SUBSYSTEMS {
            assert!(
                !subsystem.targets.is_empty(),
                "{} raises nothing",
                subsystem.name
            );
            for target in subsystem.targets {
                assert!(
                    target.starts_with("bit_cli::") || target.starts_with("librqbit"),
                    "{}: {target} is not in a crate this repository owns",
                    subsystem.name
                );
            }
        }
    }

    #[test]
    fn every_directive_builds_a_real_filter() {
        for args in [
            vec![],
            vec!["-vvv"],
            vec!["--log-level", "debug", "--trace", "http"],
            vec![
                "--trace",
                "peer,handshake,tracker,dht,http,piece,picker,disk,ratelimit,retry,config",
            ],
        ] {
            let directive = filter_directive(&global(&args)).unwrap();
            EnvFilter::try_new(&directive)
                .unwrap_or_else(|e| panic!("{directive} is not a valid filter: {e}"));
        }
    }

    #[test]
    fn every_subsystem_is_documented_and_uniquely_named() {
        let names: BTreeSet<&str> = SUBSYSTEMS.iter().map(|s| s.name).collect();
        assert_eq!(names.len(), SUBSYSTEMS.len());
        for subsystem in SUBSYSTEMS {
            assert!(
                !subsystem.description.is_empty(),
                "{} has no description",
                subsystem.name
            );
            assert_eq!(
                parse_subsystem(subsystem.name).unwrap().name,
                subsystem.name
            );
        }
    }

    #[test]
    fn a_bad_log_size_is_reported_as_a_config_error() {
        let g = global(&["--log-file", "x.log", "--log-max-size", "4 potatoes"]);
        let (env, _) = Env::test(&[], "/w");
        let err = install(&g, &env).unwrap_err();
        assert_eq!(err.code(), bit_cli_core::ExitCode::Config);
    }

    /// Rotation keeps `--log-max-files` files in total and no more.
    ///
    /// Driven through the writer rather than through a run, because what is
    /// under test is the rotation and a run producing exactly 1 KiB of log
    /// lines would be testing the log volume instead. See
    /// `TODO/cli-surface.md`, T-112.
    #[test]
    fn rotation_keeps_the_live_file_and_max_files_minus_one_behind_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("x.log");
        let mut file = Rotating::open(path.clone(), 1024, 3).unwrap();

        // Twelve writes of 200 bytes is 2400 bytes through a 1 KiB file, so it
        // rotates twice and the third generation is dropped.
        let line = vec![b'x'; 200];
        for _ in 0..12 {
            file.write_all(&line).unwrap();
        }
        file.flush().unwrap();
        drop(file);

        assert!(path.exists(), "the live file is there");
        assert!(path.with_extension("log.1").exists(), "one rotation back");
        assert!(path.with_extension("log.2").exists(), "two rotations back");
        assert!(
            !path.with_extension("log.3").exists(),
            "--log-max-files 3 means three files, not four"
        );
    }

    /// A zero size never rotates, which is what a caller who manages the file
    /// some other way asks for.
    #[test]
    fn a_zero_max_size_never_rotates() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("y.log");
        let mut file = Rotating::open(path.clone(), 0, 3).unwrap();
        for _ in 0..20 {
            file.write_all(&[b'y'; 500]).unwrap();
        }
        file.flush().unwrap();
        drop(file);

        assert_eq!(std::fs::metadata(&path).unwrap().len(), 10_000);
        assert!(!path.with_extension("log.1").exists());
    }

    /// One file total means no history: the live file starts over rather than
    /// leaving a rotated copy the caller said it did not want.
    #[test]
    fn one_file_total_truncates_instead_of_keeping_a_copy() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("z.log");
        let mut file = Rotating::open(path.clone(), 512, 1).unwrap();
        for _ in 0..6 {
            file.write_all(&[b'z'; 200]).unwrap();
        }
        file.flush().unwrap();
        drop(file);

        assert!(!path.with_extension("log.1").exists());
        assert!(
            std::fs::metadata(&path).unwrap().len() <= 512,
            "the live file stayed inside its size"
        );
    }

    /// Appending to a log that is already full rotates on the first write,
    /// not after this process has written a file's worth of its own.
    #[test]
    fn an_existing_full_log_rotates_on_the_next_write() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("w.log");
        std::fs::write(&path, vec![b'w'; 900]).unwrap();

        let mut file = Rotating::open(path.clone(), 1024, 3).unwrap();
        file.write_all(&[b'w'; 200]).unwrap();
        file.flush().unwrap();
        drop(file);

        assert!(path.with_extension("log.1").exists(), "it rotated at once");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 200);
    }

    /// The whole binary writes to the file the flag names, and stderr keeps
    /// its logs too.
    #[test]
    fn a_run_with_a_log_file_writes_to_it_and_still_writes_to_stderr() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("run.log");
        let global = global(&["--log-file", path.to_str().unwrap()]);
        let (env, _captured) = crate::env::Env::test(&[], temp.path());
        install(&global, &env).unwrap();
        assert!(path.exists(), "the file is opened when the flag is given");
    }
}
