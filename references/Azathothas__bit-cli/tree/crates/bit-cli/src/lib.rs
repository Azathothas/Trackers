//! `bit-cli`: a non-interactive BitTorrent and HTTP download tool.
//!
//! The whole program is drivable in-process through [`run`], which takes an
//! [`env::Env`] rather than reading globals. That is what makes the headless
//! parity requirement testable: a test builds an `Env` with in-memory streams
//! and no terminal, runs the same code path a shell would, and asserts the
//! same results and the same exit code.
//!
//! Nothing here is TTY-gated. Terminal detection reaches exactly two
//! decisions, colour and progress rendering, and never decides what the
//! program does, computes, or reports.

pub mod cli;
pub mod cmd;
pub mod config_defaults;
pub mod env;
pub mod hooks;
pub mod logging;
pub mod output;
pub mod payload;
pub mod schema;
#[cfg(test)]
mod schema_gen;
pub mod selection;
pub mod source;
pub mod swarm;

#[cfg(test)]
mod test_support;
pub mod webseed_args;

use bit_cli_core::error::Error;
use bit_cli_core::exit::ExitCode;
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::env::Env;
use crate::output::Renderer;

/// The troff man page, as `man/bit-cli.1` holds it.
///
/// Exposed so `tests/man_is_current.rs` renders from this crate rather than
/// from `target/release/bit-cli`, which can be older than the source in front
/// of it. See `docs/man.md`.
pub fn man_roff() -> String {
    String::from_utf8(cmd::man::render_roff().expect("the man page renders"))
        .expect("clap_mangen writes utf-8")
}

/// The CLIspec document, as `man/bit-cli.json` holds it.
pub fn man_json() -> String {
    String::from_utf8(cmd::man::render_json().expect("the CLIspec document renders"))
        .expect("serde_json writes utf-8")
}

/// The Markdown manual, as `man/bit-cli.md` holds it.
pub fn man_markdown() -> String {
    cmd::man::render_markdown()
}

/// Run the program and return the exit code.
///
/// This never panics on a user error and never writes anything to a stream the
/// caller did not supply.
pub fn run(env: &mut Env) -> ExitCode {
    let started = std::time::Instant::now();
    let cli = match Cli::try_parse_from(&env.args) {
        Ok(cli) => cli,
        // Before the flags are parsed there is no format to emit an event in,
        // so a usage error ends the stream by ending it.
        Err(err) => return report_parse_error(env, err),
    };

    // The configuration decides what a run does, not only what `config show`
    // prints, and this is where that happens. It is resolved before the
    // renderer and before the log subscriber because it decides `--color`,
    // `--log-level` and `--log-format`, and it is resolved from the **first**
    // parse because `--config` and `--no-config` are themselves flags.
    //
    // A resolution that failed is reported the way every other failure is,
    // through a renderer and with the `--jsonl` stream closed after it. The
    // renderer is built from the first parse, so its colour and format are the
    // command line's rather than the configuration's; that is the right way
    // round for a failure that is about the configuration.
    let mut renderer = Renderer::new(&cli.global, env);
    let resolved = match cmd::config::resolve(&cli.global, env) {
        Ok(resolved) => resolved,
        Err(error) => {
            renderer.fail(env, &error);
            return end_session(&mut renderer, env, error.code(), started, Some(&error));
        }
    };
    // Nothing above the built-in defaults set anything: the first parse is the
    // answer and there is no second one.
    let defaults = config_defaults::defaults(&resolved);
    let cli = match defaults.is_empty() {
        true => cli,
        false => match reparse_with_defaults(env, &defaults) {
            Ok(cli) => cli,
            Err(err) => return report_parse_error(env, err),
        },
    };

    // Rebuilt, because the configuration may have set `--color`, `--json` or
    // `--progress` and the one above was built before it was read. Nothing has
    // been emitted through the first, so no sequence number is lost.
    let mut renderer = Renderer::new(&cli.global, env);

    if cli.global.schema_version {
        let _ = env.say(output::SCHEMA_VERSION);
        return ExitCode::Success;
    }

    if let Err(error) = logging::install(&cli.global, env) {
        renderer.fail(env, &error);
        return end_session(&mut renderer, env, error.code(), started, Some(&error));
    }
    // After `install` and not before it, because the resolution is what
    // decides the log level and so has to happen while there is nothing to
    // write to. `--trace config` therefore works on every command rather than
    // only on `config show`. See `TODO/cli-surface.md`, T-219 and T-222.
    resolved.trace();

    let (code, error) = match dispatch(&cli, &mut renderer, env) {
        Ok(code) => (code, None),
        Err(error) => {
            renderer.fail(env, &error);
            (error.code(), Some(error))
        }
    };
    end_session(&mut renderer, env, code, started, error.as_ref())
}

/// Parse again, with the configured settings as the flags' defaults.
///
/// The second parse is what makes a config file reach a run at all, and it is
/// a parse rather than a pass over the parsed struct so that `clap` keeps
/// deciding precedence: a value on the command line beats a default, which is
/// exactly what a flag has to beat a config file. `crate::config_defaults`
/// has the argument in full.
fn reparse_with_defaults(
    env: &Env,
    defaults: &[(&'static str, String)],
) -> Result<Cli, clap::Error> {
    use clap::{CommandFactory, FromArgMatches};
    let command = config_defaults::apply(Cli::command(), defaults);
    let matches = command.try_get_matches_from(&env.args)?;
    Cli::from_arg_matches(&matches)
}

/// Close the `--jsonl` stream with the event that says it closed.
///
/// Emitted here rather than per command, from the one place every run returns
/// through, so it cannot be forgotten by a command that is added later. An
/// agent reading NDJSON otherwise cannot tell "finished" from "the pipe
/// broke". See `TODO/cli-surface.md`, T-110.
fn end_session(
    renderer: &mut Renderer,
    env: &mut Env,
    code: ExitCode,
    started: std::time::Instant,
    error: Option<&Error>,
) -> ExitCode {
    let elapsed = started.elapsed();
    let mut payload = serde_json::json!({
        "exit_code": code.code(),
        "exit_status": code.kind(),
        "ok": code == ExitCode::Success,
        "elapsed_ms": elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
        "elapsed_human": bit_cli_core::units::format_duration(elapsed),
    });
    if let Some(error) = error
        && let Some(object) = payload.as_object_mut()
    {
        object.insert("error".into(), serde_json::Value::String(error.to_string()));
    }
    // A stream that cannot be written is not a reason to change the exit code:
    // the run already happened, and the caller's pipe closing is theirs.
    let _ = renderer.event(env, "session_end", &payload);
    code
}

/// The error for `bit-cli tre album.torrent`, where `tre` was meant to be a
/// subcommand and is read as a source instead.
///
/// The root command takes positional sources, so `bit-cli <word>` is always a
/// download of something called `<word>` and a typo comes back as a missing
/// file. Four conditions have to hold before that is called a typo, and each
/// one exists to keep a real file out of this branch:
///
/// - it is a bare word: no separator, no extension, no drive letter, so
///   `./tre` and `tre.torrent` are paths and are treated as such;
/// - nothing of that name is on disk, so a torrent actually called `tre` is
///   still downloaded;
/// - it is not a URL, a magnet, an info hash or `-`, which is what
///   `Kind::classify` answers;
/// - a subcommand is within one edit of it, so an unrelated word is left
///   alone rather than corrected to whatever happens to be nearest.
///
/// See `TODO/cli-surface.md`, T-246.
fn mistyped_subcommand(source: &str, env: &Env) -> Option<Error> {
    let word = source.trim();
    if word.is_empty()
        || word.contains(['/', '\\', '.', ':'])
        || !matches!(source::Kind::classify(word, env), Ok(source::Kind::File(_)))
        || env.resolve(std::path::Path::new(word)).exists()
    {
        return None;
    }
    let lower = word.to_ascii_lowercase();
    let nearest = subcommand_names()
        .into_iter()
        .map(|name| (edit_distance(&lower, &name), name))
        .filter(|(distance, _)| *distance <= 1)
        .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))?;
    Some(
        Error::usage(format!(
            "`{word}` is not a command, and there is no file of that name. Did you mean `bit-cli {}`?",
            nearest.1
        ))
        .with("given", word.to_string())
        .with("nearest", nearest.1)
        .with(
            "hint",
            format!("to download a file called `{word}`, write it as a path: ./{word}"),
        ),
    )
}

/// Every subcommand of the root, from `clap` rather than from a list.
///
/// Read from the parser so a subcommand that is added is suggestible with
/// nothing for anybody to remember, which is the same reason
/// `cmd/spec.rs` walks `Cli::command()` instead of enumerating.
fn subcommand_names() -> Vec<String> {
    use clap::CommandFactory;
    Cli::command()
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect()
}

/// Levenshtein distance, iterative, one row at a time.
///
/// Only ever called on two short words, and only to answer "is this within one
/// edit", so the row-at-a-time form is the whole implementation rather than a
/// step toward a faster one.
fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (i, l) in left.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, r) in right.iter().enumerate() {
            let cost = usize::from(l != *r);
            let next = (row[j] + 1).min(row[j + 1] + 1).min(diagonal + cost);
            diagonal = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[right.len()]
}

fn dispatch(cli: &Cli, renderer: &mut Renderer, env: &mut Env) -> Result<ExitCode, Error> {
    match &cli.command {
        Some(Command::Info(args)) => cmd::info::run(args, &cli.global, renderer, env),
        Some(Command::Files(args)) => cmd::files::run(args, &cli.global, renderer, env),
        Some(Command::Tree(args)) => cmd::tree::run(args, &cli.global, renderer, env),
        Some(Command::Magnet(args)) => cmd::magnet::run(args, &cli.global, renderer, env),
        Some(Command::Verify(args)) => cmd::verify::run(args, &cli.global, renderer, env),
        Some(Command::Create(args)) => cmd::create::run(args, &cli.global, renderer, env),
        Some(Command::Edit(args)) => cmd::edit::run(args, &cli.global, renderer, env),
        Some(Command::Webseed(args)) => cmd::webseed::run(args, &cli.global, renderer, env),
        Some(Command::Config(args)) => cmd::config::run(args, &cli.global, renderer, env),
        Some(Command::Completions(args)) => cmd::completions::run(args, env),
        Some(Command::Man(args)) => cmd::man::run(args, env),
        Some(Command::Version) => cmd::version::run(renderer, env),
        Some(Command::Download(args)) => cmd::download::run(args, &cli.global, renderer, env),
        Some(Command::Seed(args)) => cmd::seed::run(args, &cli.global, renderer, env),
        Some(Command::Peers(args)) => cmd::peers::run(args, &cli.global, renderer, env),
        Some(Command::Trackers(args)) => cmd::trackers::run(args, &cli.global, renderer, env),
        Some(Command::Bench(args)) => cmd::bench::run(args, &cli.global, renderer, env),
        // `bit-cli <SOURCE>` is `bit-cli download <SOURCE>`.
        None if !cli.sources.is_empty() => {
            if let Some(error) = mistyped_subcommand(&cli.sources[0], env) {
                return Err(error);
            }
            let args = cli::DownloadArgs::from_sources(cli.sources.clone());
            cmd::download::run(&args, &cli.global, renderer, env)
        }
        None => {
            let _ = env.note("no source given. Run `bit-cli --help`.");
            Ok(ExitCode::Usage)
        }
    }
}

/// Turn a `clap` parse failure into an exit code.
///
/// `--help` and `--version` come back from `clap` as errors but are successful
/// requests, so they print to stdout and exit zero.
fn report_parse_error(env: &mut Env, err: clap::Error) -> ExitCode {
    use clap::error::ErrorKind;
    match err.kind() {
        ErrorKind::DisplayHelp
        | ErrorKind::DisplayVersion
        | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            let _ = env.say(err.render().ansi().to_string().trim_end());
            ExitCode::Success
        }
        _ => {
            let _ = env.note(err.render().ansi().to_string().trim_end());
            ExitCode::Usage
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_goes_to_stdout_and_exits_zero() {
        let (mut env, captured) = Env::test(&["--help"], "/w");
        assert_eq!(run(&mut env), ExitCode::Success);
        assert!(captured.out().contains("web seed"), "{}", captured.out());
        assert_eq!(captured.err(), "");
    }

    #[test]
    fn a_bad_flag_goes_to_stderr_and_exits_two() {
        let (mut env, captured) = Env::test(&["--nope"], "/w");
        assert_eq!(run(&mut env), ExitCode::Usage);
        assert_eq!(captured.out(), "", "stdout carries data only");
        assert!(captured.err().contains("--nope"));
    }

    #[test]
    fn no_arguments_at_all_is_a_usage_error() {
        let (mut env, captured) = Env::test(&[], "/w");
        assert_eq!(run(&mut env), ExitCode::Usage);
        assert_eq!(captured.out(), "");
    }

    #[test]
    fn schema_version_prints_and_exits() {
        let (mut env, captured) = Env::test(&["--schema-version"], "/w");
        assert_eq!(run(&mut env), ExitCode::Success);
        assert_eq!(captured.out().trim(), output::SCHEMA_VERSION);
    }

    #[test]
    fn version_reports_the_build() {
        let (mut env, captured) = Env::test(&["version", "--json"], "/w");
        assert_eq!(run(&mut env), ExitCode::Success);
        let doc = captured.json().unwrap();
        assert_eq!(doc["version"], bit_cli_core::VERSION);
        assert!(doc["exit_codes"].as_array().unwrap().len() >= 16);
    }

    #[test]
    fn every_subcommand_help_renders() {
        for sub in [
            "download",
            "info",
            "files",
            "peers",
            "trackers",
            "webseed",
            "verify",
            "create",
            "edit",
            "magnet",
            "seed",
            "bench",
            "config",
            "completions",
            "man",
            "version",
        ] {
            let (mut env, captured) = Env::test(&[sub, "--help"], "/w");
            assert_eq!(run(&mut env), ExitCode::Success, "{sub} --help failed");
            assert!(!captured.out().is_empty(), "{sub} --help printed nothing");
        }
    }

    /// Every `--jsonl` run ends with the event that says it ended.
    ///
    /// An agent reading NDJSON cannot otherwise tell "finished" from "the pipe
    /// broke". It is emitted from `run` rather than per command, so a command
    /// added later cannot forget it, and this test walks every command that
    /// can be driven with no network to prove it. See `TODO/cli-surface.md`,
    /// T-110.
    #[test]
    fn every_jsonl_run_ends_with_session_end() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let torrent = fixture.path_str().to_string();
        let cases: Vec<Vec<&str>> = vec![
            vec!["--jsonl", "info", &torrent],
            vec!["--jsonl", "files", &torrent],
            vec!["--jsonl", "magnet", &torrent],
            vec!["--jsonl", "version"],
            vec!["--jsonl", "config", "show"],
            vec![
                "--jsonl",
                "webseed",
                "list",
                &torrent,
                "--web-seed",
                "https://e.example/",
            ],
        ];
        for args in cases {
            let (mut env, captured) = Env::test(&args, fixture.dir());
            let code = run(&mut env);
            let events = captured.jsonl().unwrap_or_else(|e| {
                panic!("{args:?} did not write ndjson: {e}\n{}", captured.out())
            });
            let last = events
                .last()
                .unwrap_or_else(|| panic!("{args:?} wrote nothing"));
            assert_eq!(last["type"], "session_end", "{args:?} ended with {last}");
            assert_eq!(last["exit_code"], code.code(), "{args:?}");
            assert!(last["elapsed_ms"].is_number(), "{args:?}");
            assert!(last["at"].is_string(), "{args:?}");
        }
    }

    /// A failure ends the stream too, with the code and the reason.
    #[test]
    fn a_failed_jsonl_run_ends_with_session_end_carrying_the_error() {
        let (mut env, captured) = Env::test(&["--jsonl", "info", "nope.torrent"], "/w");
        let code = run(&mut env);
        assert_ne!(code, ExitCode::Success);
        let events = captured.jsonl().unwrap();
        let last = events.last().expect("an event");
        assert_eq!(last["type"], "session_end");
        assert_eq!(last["ok"], false);
        assert_eq!(last["exit_code"], code.code());
        assert!(
            last["error"]
                .as_str()
                .unwrap_or_default()
                .contains("nope.torrent"),
            "{last}"
        );
    }

    /// T-246's second case. The root command takes positional sources, so a
    /// mistyped subcommand used to come back as a missing file.
    #[test]
    fn a_mistyped_subcommand_is_a_usage_error_naming_the_nearest_one() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let (mut env, captured) = Env::test(&["tre", fixture.path_str()], fixture.dir());
        assert_eq!(run(&mut env), ExitCode::Usage);
        let err = captured.err();
        assert!(err.contains("is not a command"), "{err}");
        assert!(err.contains("bit-cli tree"), "{err}");
    }

    /// The other half, and it is what keeps a real file out of the branch: a
    /// path written as a path is a path, present or missing.
    #[test]
    fn a_source_written_as_a_path_is_never_read_as_a_subcommand() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        for source in ["./tre", "tre.torrent", "sub/tre"] {
            let (mut env, captured) = Env::test(&[source], fixture.dir());
            assert_eq!(
                run(&mut env),
                ExitCode::SourceResolution,
                "{source} was not read as a path: {}",
                captured.err()
            );
            assert!(captured.err().contains("cannot read"), "{source}");
        }
    }

    /// A bare word two or more edits from every subcommand is a source, not a
    /// typo. Correcting it would be guessing.
    #[test]
    fn a_word_that_is_near_nothing_is_still_a_source() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        let (mut env, captured) = Env::test(&["quuxly"], fixture.dir());
        assert_eq!(run(&mut env), ExitCode::SourceResolution);
        assert!(captured.err().contains("cannot read"), "{}", captured.err());
    }

    /// A file whose name happens to be a near-miss for a subcommand is
    /// downloaded rather than corrected, because it is on disk.
    #[test]
    fn a_file_named_like_a_typo_is_read_rather_than_corrected() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        std::fs::copy(fixture.path_str(), fixture.dir().join("tre")).expect("copy the fixture");
        let (mut env, captured) = Env::test(&["--dry-run", "tre"], fixture.dir());
        assert_eq!(run(&mut env), ExitCode::Success, "{}", captured.err());
    }

    #[test]
    fn the_edit_distance_is_the_ordinary_one() {
        assert_eq!(edit_distance("tre", "tree"), 1);
        assert_eq!(edit_distance("tree", "tree"), 0);
        assert_eq!(edit_distance("", "tree"), 4);
        assert_eq!(edit_distance("tree", ""), 4);
        assert_eq!(edit_distance("infp", "info"), 1);
        // Same length, and only the trailing `y` matches.
        assert_eq!(edit_distance("quuxly", "verify"), 5);
    }

    /// Every name the suggester can offer is a name `clap` will accept, so a
    /// suggestion cannot point at a command that does not exist.
    #[test]
    fn every_suggestible_name_is_a_real_subcommand() {
        let names = subcommand_names();
        assert!(names.contains(&"tree".to_string()), "{names:?}");
        assert!(names.contains(&"download".to_string()), "{names:?}");
        for name in names {
            let parsed = Cli::try_parse_from(["bit-cli", &name, "--help"]);
            assert!(parsed.is_err(), "`{name}` did not reach clap as a command");
        }
    }

    /// The event is a `--jsonl` surface only. `--json` carries one document
    /// and text carries lines, and neither gains a stray object at the end.
    #[test]
    fn session_end_does_not_appear_outside_jsonl() {
        let fixture = crate::test_support::TorrentFixture::multi_file();
        for flag in ["--json", "--quiet"] {
            let (mut env, captured) = Env::test(&[flag, "info", fixture.path_str()], fixture.dir());
            assert_eq!(run(&mut env), ExitCode::Success);
            assert!(
                !captured.out().contains("session_end"),
                "{flag} leaked an event: {}",
                captured.out()
            );
        }
    }
}
