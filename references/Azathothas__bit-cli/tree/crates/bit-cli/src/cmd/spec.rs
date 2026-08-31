//! The command surface as a [CLIspec](https://github.com/rvben/clispec) 0.3
//! document, which is `man/bit-cli.json`.
//!
//! **Why this exists.** A man page is for a person. An agent driving this tool
//! reads roff badly, so it guesses flag names, and a guessed flag is a run that
//! fails on exit 2 or, worse, one that silently does something else. This is
//! the same surface in a shape a program can index: every command, every flag,
//! its type, its default, its accepted values, and every exit code the tool can
//! produce.
//!
//! **It is generated, never written.** Everything here is walked out of
//! `Cli::command()` and out of [`ExitCode::ALL`], so a flag that is added,
//! renamed or removed moves this file on the next run and
//! `scripts/check-man.ps1` fails the gates until the committed copy is
//! regenerated. The one thing a person maintains is [`effects_of`], and a
//! subcommand with no entry there fails a test rather than being guessed at.

use bit_cli_core::ExitCode;
use clap::{Command, ValueHint};
use serde_json::{Map, Value, json};

/// The spec version this document claims.
const CLISPEC_VERSION: &str = "0.3";

/// What running a command does to the world.
///
/// CLIspec's vocabulary, and the reason an agent needs it: `read_only` is safe
/// to run to find something out, `idempotent` can be retried after a failure
/// without asking, and `non_idempotent` cannot.
///
/// This is the one table in this file that a person maintains, because nothing
/// in a clap definition says whether a command writes. `every_subcommand_is_classified`
/// fails when a new subcommand has no entry, so it cannot be forgotten.
fn effects_of(path: &str) -> &'static str {
    match path {
        // Read something and print it. None of these opens a file for writing.
        "bit-cli info"
        | "bit-cli files"
        | "bit-cli tree"
        | "bit-cli magnet"
        | "bit-cli version"
        | "bit-cli completions"
        | "bit-cli man"
        | "bit-cli config" => "read_only",

        // Talk to the network and report, writing nothing. `verify` reads the
        // payload and hashes it.
        "bit-cli peers" | "bit-cli trackers" | "bit-cli webseed" | "bit-cli verify" => "read_only",

        // Writes, and running it twice from the same state lands in the same
        // place: the payload either verifies or is fetched again.
        "bit-cli download" => "idempotent",

        // Serves what is already on disk and writes nothing to it.
        "bit-cli seed" => "read_only",

        // Creates or rewrites a file. Running it twice is not the same as
        // running it once: `create` refuses or overwrites depending on
        // `--force`, and `edit` writes a new file each time.
        "bit-cli create" | "bit-cli edit" => "non_idempotent",

        // Writes reports and, in the disk case, large scratch files.
        "bit-cli bench" => "non_idempotent",

        // The bare invocation with a SOURCE, which is `download`.
        "bit-cli" => "idempotent",

        // Nested subcommands. Each is classified on its own, because a parent
        // that writes does not mean every child does: `bench probe` reads and
        // `bench disk` writes gigabytes.
        "bit-cli webseed list" | "bit-cli webseed test" | "bit-cli webseed probe" => "read_only",
        // Writes what it fetched, to a path or to stdout.
        "bit-cli webseed fetch" => "idempotent",

        // Measures a target and writes a report. `probe` only reads.
        "bit-cli bench probe" => "read_only",
        "bit-cli bench leech"
        | "bit-cli bench seed"
        | "bit-cli bench webseed"
        | "bit-cli bench disk"
        | "bit-cli bench swarm" => "non_idempotent",

        "bit-cli config show" => "read_only",

        _ => "",
    }
}

/// One argument, as CLIspec describes it.
fn arg_value(arg: &clap::Arg) -> Value {
    let mut out = Map::new();

    // Positionals have no long form; CLIspec still wants a name, and the
    // value name is what the help shows a caller.
    let name = match arg.get_long() {
        Some(long) => format!("--{long}"),
        None => arg.get_id().to_string(),
    };
    out.insert("name".into(), json!(name));

    if let Some(short) = arg.get_short() {
        out.insert("short".into(), json!(format!("-{short}")));
    }

    // From the action, not from `get_num_args`: that is only populated once
    // the command has been built, and a flag reported as taking no value when
    // it needs a URL is worse than saying nothing. `--web-seed` was typed
    // `boolean` while carrying `value_name: URL` until this was fixed.
    let takes_value = !matches!(
        arg.get_action(),
        clap::ArgAction::SetTrue
            | clap::ArgAction::SetFalse
            | clap::ArgAction::Count
            | clap::ArgAction::Help
            | clap::ArgAction::HelpShort
            | clap::ArgAction::HelpLong
            | clap::ArgAction::Version
    );
    let multiple = matches!(arg.get_action(), clap::ArgAction::Append)
        || arg
            .get_num_args()
            .map(|n| n.max_values() > 1)
            .unwrap_or(false);

    let ty = if matches!(arg.get_action(), clap::ArgAction::Count) {
        // Repeatable, and its value is how many times: -v, -vv, -vvv. Calling
        // that a boolean loses the only thing about it that matters.
        "integer"
    } else if !takes_value {
        "boolean"
    } else if multiple {
        "array"
    } else {
        "string"
    };
    out.insert("type".into(), json!(ty));

    if takes_value
        && matches!(
            arg.get_value_hint(),
            ValueHint::FilePath | ValueHint::DirPath | ValueHint::AnyPath
        )
    {
        out.insert("value_hint".into(), json!("path"));
    }

    if arg.is_positional() {
        out.insert("positional".into(), json!(true));
    }
    if arg.is_required_set() {
        out.insert("required".into(), json!(true));
    }

    // Accepted values, where clap knows them. This is the field that stops an
    // agent inventing a value for an enum flag.
    let possible: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|p| p.get_name().to_string())
        .collect();
    if !possible.is_empty() {
        out.insert("enum".into(), json!(possible));
    }

    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();
    if defaults.len() == 1 {
        out.insert("default".into(), json!(defaults[0]));
    } else if !defaults.is_empty() {
        out.insert("default".into(), json!(defaults));
    }

    if let Some(help) = arg.get_help().or_else(|| arg.get_long_help()) {
        out.insert("description".into(), json!(one_line(&help.to_string())));
    }

    // Only where there is a value to name. clap invents one from the argument
    // id for a flag as well, so `--json` came out carrying `value_name: JSON`,
    // which reads as though it took an argument.
    if let Some(value_name) = arg
        .get_value_names()
        .and_then(|n| n.first())
        .filter(|_| takes_value)
    {
        out.insert("value_name".into(), json!(value_name.to_string()));
    }

    Value::Object(out)
}

/// clap's own `--help` and `--version`, which every command has and which no
/// caller needs told about.
fn is_generated_help_or_version(arg: &clap::Arg) -> bool {
    matches!(
        arg.get_action(),
        clap::ArgAction::Help
            | clap::ArgAction::HelpShort
            | clap::ArgAction::HelpLong
            | clap::ArgAction::Version
    )
}

/// Help text as one line, because a description that carries newlines is
/// awkward to print and every consumer of this file re-wraps it anyway.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Walk `command` and every subcommand under it, appending to `out`.
fn walk(command: &Command, path: &str, out: &mut Vec<Value>) {
    let mut entry = Map::new();
    entry.insert("name".into(), json!(path));
    if let Some(about) = command.get_about() {
        entry.insert("description".into(), json!(one_line(&about.to_string())));
    }
    entry.insert("effects".into(), json!(effects_of(path)));

    // Global args are declared once at the top level, so a subcommand lists
    // only what is its own.
    let args: Vec<Value> = command
        .get_arguments()
        .filter(|a| !a.is_global_set())
        // By action, not by id. Filtering on the id "version" also deleted
        // `create --version`, which is the metainfo version and a real flag
        // that takes v1, v2 or hybrid.
        .filter(|a| !is_generated_help_or_version(a))
        .map(arg_value)
        .collect();
    entry.insert("args".into(), Value::Array(args));

    let children: Vec<String> = command
        .get_subcommands()
        .map(|c| format!("{path} {}", c.get_name()))
        .collect();
    if !children.is_empty() {
        entry.insert("subcommands".into(), json!(children));
    }

    out.push(Value::Object(entry));

    for sub in command.get_subcommands() {
        walk(sub, &format!("{path} {}", sub.get_name()), out);
    }
}

/// Render the whole surface.
///
/// The command is built first. Until it is, clap leaves `num_args`, propagated
/// globals and inherited settings unpopulated, and this walks all three.
pub fn render(command: &Command, version: &str) -> Value {
    let mut command = command.clone();
    command.build();
    let command = &command;

    let global_args: Vec<Value> = command
        .get_arguments()
        .filter(|a| a.is_global_set())
        .map(arg_value)
        .collect();

    let mut commands = Vec::new();
    walk(command, "bit-cli", &mut commands);

    // Generated from ExitCode::ALL, so a new code appears here without anybody
    // remembering to add it. `retryable` is a property of the code rather than
    // of the run: a network failure or a timeout may succeed on a second
    // attempt, and a usage error never will.
    let errors: Vec<Value> = ExitCode::ALL
        .iter()
        .filter(|c| **c != ExitCode::Success)
        .map(|c| {
            json!({
                "kind": c.kind(),
                "exit_code": c.code(),
                "description": c.description(),
                "retryable": retryable(*c),
            })
        })
        .collect();

    json!({
        "clispec": CLISPEC_VERSION,
        "name": "bit-cli",
        "version": version,
        "description": one_line(&command.get_about().map(|a| a.to_string()).unwrap_or_default()),
        "output": {
            "tty": "text",
            "piped": "text",
            "structured": "pass --json for a single document or --jsonl for events, on any command that produces output"
        },
        "global_args": global_args,
        "commands": commands,
        "errors": errors,
    })
}

/// The same manual as Markdown, rendered from the CLIspec document.
///
/// From the document rather than from clap a second time, so the Markdown and
/// the JSON cannot disagree about what a flag is called or what it accepts.
/// The roff page is clap's own rendering and is the one that can differ in
/// wording; all three come from the same clap definition.
pub fn markdown(doc: &Value) -> String {
    let mut out = String::new();
    let name = doc["name"].as_str().unwrap_or("bit-cli");
    let version = doc["version"].as_str().unwrap_or("");

    out.push_str(&format!("# {name}({version})\n\n"));
    if let Some(description) = doc["description"].as_str() {
        out.push_str(&format!("{description}\n\n"));
    }
    out.push_str(
        "This file is generated from the command definition by `bit-cli man --format markdown`. \
         Do not edit it: `cargo test -p bit-cli` fails when it stops describing the binary. \
         The same surface is available as a man page in `bit-cli.1` and, for a program, as a \
         CLIspec document in `bit-cli.json`.\n\n",
    );

    out.push_str("## Global options\n\n");
    out.push_str("These are accepted by every command.\n\n");
    out.push_str(&options_table(&doc["global_args"]));

    out.push_str("\n## Commands\n\n");
    out.push_str("| command | effects | what it does |\n| --- | --- | --- |\n");
    for command in doc["commands"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            command["name"].as_str().unwrap_or(""),
            command["effects"].as_str().unwrap_or(""),
            escape_cell(command["description"].as_str().unwrap_or(""))
        ));
    }
    out.push('\n');
    out.push_str(
        "`effects` is CLIspec's word for what running the command does: `read_only` is safe to \
         run to find something out, `idempotent` can be retried after a failure, and \
         `non_idempotent` cannot.\n\n",
    );

    for command in doc["commands"].as_array().into_iter().flatten() {
        let path = command["name"].as_str().unwrap_or("");
        out.push_str(&format!("### `{path}`\n\n"));
        if let Some(description) = command["description"].as_str() {
            out.push_str(&format!("{description}\n\n"));
        }
        out.push_str(&format!(
            "Effects: `{}`.\n\n",
            command["effects"].as_str().unwrap_or("")
        ));
        let args = &command["args"];
        if args.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            out.push_str("Takes no options of its own.\n\n");
        } else {
            out.push_str(&options_table(args));
            out.push('\n');
        }
    }

    out.push_str("## Exit codes\n\n");
    out.push_str(
        "The exit code is the primary success signal, and no code is ever reused for a second \
         meaning. `retryable` says whether a second attempt could succeed without changing \
         anything.\n\n",
    );
    out.push_str("| code | kind | retryable | meaning |\n| --- | --- | --- | --- |\n");
    out.push_str("| 0 | `success` | | The command did what it was asked to do |\n");
    for error in doc["errors"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "| {} | `{}` | {} | {} |\n",
            error["exit_code"],
            error["kind"].as_str().unwrap_or(""),
            if error["retryable"] == Value::Bool(true) {
                "yes"
            } else {
                "no"
            },
            escape_cell(error["description"].as_str().unwrap_or(""))
        ));
    }

    out
}

/// One options table, or nothing when there are no options.
fn options_table(args: &Value) -> String {
    let args = match args.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return String::new(),
    };
    let mut out = String::from("| option | type | accepts | default | what it does |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");
    for arg in args {
        let name = arg["name"].as_str().unwrap_or("");
        let short = arg["short"].as_str().map(|s| format!(", `{s}`"));
        let value_name = arg["value_name"]
            .as_str()
            .map(|v| format!(" <{v}>"))
            .unwrap_or_default();
        let accepts = match arg["enum"].as_array() {
            Some(values) => values
                .iter()
                .filter_map(|v| v.as_str())
                .map(|v| format!("`{v}`"))
                .collect::<Vec<_>>()
                .join(", "),
            None => String::new(),
        };
        let default = match &arg["default"] {
            Value::String(s) => format!("`{s}`"),
            Value::Array(values) => values
                .iter()
                .filter_map(|v| v.as_str())
                .map(|v| format!("`{v}`"))
                .collect::<Vec<_>>()
                .join(", "),
            _ => String::new(),
        };
        out.push_str(&format!(
            "| `{}{}`{} | {} | {} | {} | {} |\n",
            name,
            value_name,
            short.unwrap_or_default(),
            arg["type"].as_str().unwrap_or(""),
            accepts,
            default,
            escape_cell(arg["description"].as_str().unwrap_or(""))
        ));
    }
    out
}

/// A pipe inside a table cell ends the cell, and help text does carry them.
fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// Whether a second attempt could succeed without changing anything.
const fn retryable(code: ExitCode) -> bool {
    match code {
        // The world may be different in a moment.
        ExitCode::Network
        | ExitCode::NoUsableSources
        | ExitCode::Timeout
        | ExitCode::ResourceCeiling
        | ExitCode::ListenerUnhealthy
        | ExitCode::Generic => true,

        // Nothing about a retry changes these: the arguments, the config, the
        // torrent, the disk or the data are what they are.
        ExitCode::Success
        | ExitCode::Usage
        | ExitCode::Config
        | ExitCode::SourceResolution
        | ExitCode::HashMismatch
        | ExitCode::Disk
        | ExitCode::Interrupted
        | ExitCode::CoverageGap
        | ExitCode::Binding
        | ExitCode::LintRefused
        | ExitCode::ThresholdNotMet
        | ExitCode::WouldChangeInfoHash => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::CommandFactory;

    fn spec() -> Value {
        render(&Cli::command(), "0.0.0-test")
    }

    #[test]
    fn every_subcommand_is_classified() {
        // The one hand-maintained table in this file. A new subcommand fails
        // here rather than shipping with an empty `effects`, which an agent
        // would read as "no side effects".
        let doc = spec();
        let mut unclassified = Vec::new();
        for command in doc["commands"].as_array().unwrap() {
            if command["effects"].as_str().unwrap_or("").is_empty() {
                unclassified.push(command["name"].as_str().unwrap().to_string());
            }
        }
        assert!(
            unclassified.is_empty(),
            "no effects_of entry for: {unclassified:?}"
        );
    }

    #[test]
    fn every_effect_is_one_of_the_three_clispec_words() {
        let doc = spec();
        for command in doc["commands"].as_array().unwrap() {
            let effects = command["effects"].as_str().unwrap();
            assert!(
                matches!(effects, "read_only" | "idempotent" | "non_idempotent"),
                "{} has effects {effects}",
                command["name"]
            );
        }
    }

    #[test]
    fn every_subcommand_is_present_with_its_full_path() {
        let doc = spec();
        let names: Vec<&str> = doc["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        for expected in [
            "bit-cli",
            "bit-cli download",
            "bit-cli webseed",
            "bit-cli create",
            "bit-cli bench",
            "bit-cli bench disk",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
    }

    #[test]
    fn every_exit_code_is_carried_except_success() {
        let doc = spec();
        let errors = doc["errors"].as_array().unwrap();
        assert_eq!(errors.len(), ExitCode::ALL.len() - 1);
        let usage = errors
            .iter()
            .find(|e| e["kind"] == "usage")
            .expect("usage is not in the errors table");
        assert_eq!(usage["exit_code"], 2);
        assert_eq!(usage["retryable"], false);
    }

    #[test]
    fn an_enum_flag_carries_the_values_it_accepts() {
        // This is the field that stops a caller inventing a value. `create
        // --version` takes v1, v2 or hybrid and nothing else.
        let doc = spec();
        let create = doc["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "bit-cli create")
            .unwrap();
        let version = create["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == "--version")
            .expect("create --version is missing");
        let values: Vec<&str> = version["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(values, vec!["v1", "v2", "hybrid"]);
    }

    #[test]
    fn a_global_flag_is_declared_once_at_the_top_and_not_repeated() {
        let doc = spec();
        let globals: Vec<&str> = doc["global_args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert!(globals.contains(&"--json"), "{globals:?}");

        for command in doc["commands"].as_array().unwrap() {
            for arg in command["args"].as_array().unwrap() {
                assert_ne!(
                    arg["name"], "--json",
                    "{} repeats a global flag",
                    command["name"]
                );
            }
        }
    }
}
