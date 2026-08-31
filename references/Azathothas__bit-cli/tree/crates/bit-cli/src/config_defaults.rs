//! Making the resolved configuration the default of every flag it names.
//!
//! `TODO/cli-surface.md` T-222 is what this module exists for. `bit-cli.toml`,
//! the user config file, `--config` and every `BIT_CLI_*` variable were read
//! by `bit-cli config show` and by nothing else, so the six layer precedence
//! chain `README.md` documents decided what one command **printed** and
//! nothing about what any command **did**.
//!
//! # Why this is a second parse rather than a pass over the parsed struct
//!
//! The obvious shape is to parse, resolve, and then overwrite each field whose
//! flag was not given on the command line, using `clap`'s
//! `ArgMatches::value_source` to tell the two apart. It works, and it needs a
//! branch per setting in whichever struct holds that flag, spread over
//! `Global`, `LimitArgs`, `WebSeedArgs` and five command structs, with nothing
//! checking that a new flag was added to it.
//!
//! Setting the flag's **default** instead moves the whole question back into
//! `clap`, which already knows that a value on the command line beats a
//! default. So a configured setting becomes `Arg::default_value` on the arg
//! whose long name matches it, the tree is parsed again, and precedence falls
//! out rather than being implemented. The mapping below is then the only thing
//! anybody has to keep true, and
//! `every_setting_names_a_flag_that_exists` is what keeps it true.
//!
//! The second parse costs one `Command` build and one match, and it happens
//! only when a config layer actually set something: a run with no config file
//! and no `BIT_CLI_*` variable parses once.
//!
//! # What a configured boolean cannot do
//!
//! Three settings are `enable_*` and the flags are `--no-*`, so the value is
//! inverted on the way in. `enable_dht = false` in a config file makes
//! `--no-dht` default to true, and there is no `--dht` to turn it back on for
//! one run. `--no-config` is the escape hatch and it is the honest one: a
//! caller who wants the file ignored says so.

use bit_cli_core::config::Resolved;
use clap::Command;

/// One setting, and the flag it is the default for.
pub struct Mapping {
    /// The setting name, as `bit_cli_core::config::SETTINGS` carries it.
    pub setting: &'static str,
    /// The **long** flag, without the leading dashes.
    ///
    /// Matched against `Arg::get_long` rather than against the `clap` id,
    /// because the id of a derived field is the field name and two structs
    /// may spell the same flag differently.
    pub flag: &'static str,
    /// Whether the flag says the opposite of the setting.
    ///
    /// `enable_dht` is `--no-dht`. The value is inverted, and only a boolean
    /// setting may set this.
    pub negated: bool,
}

const fn plain(setting: &'static str, flag: &'static str) -> Mapping {
    Mapping {
        setting,
        flag,
        negated: false,
    }
}

const fn inverted(setting: &'static str, flag: &'static str) -> Mapping {
    Mapping {
        setting,
        flag,
        negated: true,
    }
}

/// Every setting, and the flag it is the default for.
///
/// Twenty of the twenty-two settings are spelled exactly like their flag. The
/// two that are not are the two that were named before the flag was:
/// `download_directory` is `--dir` and `listen_port` is `--port`.
///
/// A setting with no flag would be a setting nothing can act on, so
/// `every_setting_is_mapped` refuses one.
pub const MAPPINGS: &[Mapping] = &[
    plain("download_directory", "dir"),
    plain("listen_port", "port"),
    inverted("enable_dht", "no-dht"),
    inverted("enable_pex", "no-pex"),
    inverted("enable_lsd", "no-lsd"),
    plain("max_peers", "max-peers"),
    plain("max_peers_total", "max-peers-total"),
    plain("max_download_rate", "max-download-rate"),
    plain("max_upload_rate", "max-upload-rate"),
    plain("max_concurrent_downloads", "max-concurrent-downloads"),
    plain("seed_ratio", "seed-ratio"),
    plain("seed_time", "seed-time"),
    inverted("enable_web_seeds", "no-web-seed"),
    plain("web_seed_concurrency", "web-seed-concurrency"),
    plain("web_seed_chunk_size", "web-seed-chunk-size"),
    plain("web_seed_timeout", "web-seed-timeout"),
    plain("web_seed_user_agent", "web-seed-user-agent"),
    plain("file_allocation", "file-allocation"),
    plain("piece_selector", "piece-selector"),
    plain("log_level", "log-level"),
    plain("log_format", "log-format"),
    plain("color", "color"),
];

/// The flag defaults a resolution implies, as `(long flag, value)`.
///
/// Empty when nothing above the built-in defaults set anything, which is the
/// signal to skip the second parse entirely.
pub fn defaults(resolved: &Resolved) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for (setting, value) in resolved.configured() {
        let Some(mapping) = MAPPINGS.iter().find(|m| m.setting == setting) else {
            continue;
        };
        let value = match mapping.negated {
            // A boolean that arrived as a string, which is what a
            // `BIT_CLI_ENABLE_DHT=false` variable is, has to invert the same
            // way the TOML boolean does. Anything that is not a boolean is
            // left alone and `clap` reports it against the flag.
            true => match value.as_str() {
                "true" => "false".to_string(),
                "false" => "true".to_string(),
                other => other.to_string(),
            },
            false => value,
        };
        out.push((mapping.flag, value));
    }
    out
}

/// Give every arg the configuration named its configured value as a default.
///
/// Recursive, because the flags live on the subcommands: `--max-peers` is on
/// five of them and `clap` holds one `Arg` per command that carries it. The
/// walk is by name through `mut_subcommand`, which is the only way to replace
/// a subcommand in place.
pub fn apply(command: Command, defaults: &[(&'static str, String)]) -> Command {
    if defaults.is_empty() {
        return command;
    }
    apply_to(command, defaults)
}

fn apply_to(mut command: Command, defaults: &[(&'static str, String)]) -> Command {
    command = command.mut_args(|arg| {
        match arg
            .get_long()
            .and_then(|long| defaults.iter().find(|(flag, _)| *flag == long))
        {
            Some((_, value)) => arg.default_value(value.clone()),
            None => arg,
        }
    });
    let names: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for name in names {
        command = command.mut_subcommand(&name, |sub| apply_to(sub, defaults));
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use bit_cli_core::config::{Origin, SETTINGS};
    use clap::CommandFactory;
    use std::collections::BTreeSet;

    /// Every setting has a flag, because a setting nothing can act on is the
    /// defect this module was written to remove rather than to spread.
    #[test]
    fn every_setting_is_mapped() {
        let mapped: BTreeSet<&str> = MAPPINGS.iter().map(|m| m.setting).collect();
        for (name, _, _) in SETTINGS {
            assert!(mapped.contains(name), "{name} is a setting with no flag");
        }
        assert_eq!(mapped.len(), SETTINGS.len(), "a mapping names no setting");
    }

    /// Every flag named above exists somewhere in the command tree. A typo
    /// here is silent: `mut_args` simply matches nothing.
    #[test]
    fn every_setting_names_a_flag_that_exists() {
        fn longs(command: &Command, into: &mut BTreeSet<String>) {
            for arg in command.get_arguments() {
                if let Some(long) = arg.get_long() {
                    into.insert(long.to_string());
                }
            }
            for sub in command.get_subcommands() {
                longs(sub, into);
            }
        }
        let mut every = BTreeSet::new();
        longs(&Cli::command(), &mut every);
        for mapping in MAPPINGS {
            assert!(
                every.contains(mapping.flag),
                "{} maps to --{}, which no command has",
                mapping.setting,
                mapping.flag
            );
        }
    }

    fn resolved_with(entries: &[(&str, serde_json::Value)], origin: Origin) -> Resolved {
        let mut resolved = Resolved::defaults();
        resolved.apply(
            entries.iter().map(|(n, v)| (*n, v.clone())).collect(),
            origin,
        );
        resolved
    }

    /// A tree with nothing configured is the tree, untouched, and the caller
    /// skips the second parse.
    #[test]
    fn a_resolution_of_only_defaults_names_no_flag() {
        assert!(defaults(&Resolved::defaults()).is_empty());
    }

    /// A value that came from the command line is not handed back as a
    /// default: it is already on the command line.
    #[test]
    fn a_flag_origin_is_not_turned_into_a_default() {
        let resolved = resolved_with(
            &[("max_peers", 9.into())],
            Origin::Flag {
                name: "max-peers".into(),
            },
        );
        assert!(defaults(&resolved).is_empty());
    }

    #[test]
    fn a_configured_value_becomes_the_flags_default() {
        let resolved = resolved_with(
            &[("max_peers", 9.into())],
            Origin::Environment {
                name: "BIT_CLI_MAX_PEERS".into(),
            },
        );
        assert_eq!(defaults(&resolved), vec![("max-peers", "9".to_string())]);
    }

    /// `enable_dht = false` is `--no-dht` true, and the inversion is the whole
    /// point of the `negated` column.
    #[test]
    fn an_enable_setting_inverts_into_its_no_flag() {
        let resolved = resolved_with(
            &[("enable_dht", false.into())],
            Origin::ProjectConfig {
                path: "bit-cli.toml".into(),
            },
        );
        assert_eq!(defaults(&resolved), vec![("no-dht", "true".to_string())]);

        let resolved = resolved_with(
            &[("enable_dht", true.into())],
            Origin::ProjectConfig {
                path: "bit-cli.toml".into(),
            },
        );
        assert_eq!(defaults(&resolved), vec![("no-dht", "false".to_string())]);
    }

    /// The default lands on the arg, on a subcommand, which is where every
    /// flag but the four global ones lives.
    #[test]
    fn the_default_reaches_a_subcommands_arg() {
        let applied = apply(Cli::command(), &[("max-peers", "9".to_string())]);
        let download = applied
            .get_subcommands()
            .find(|s| s.get_name() == "download")
            .expect("download is a subcommand");
        let arg = download
            .get_arguments()
            .find(|a| a.get_long() == Some("max-peers"))
            .expect("download takes --max-peers");
        assert_eq!(
            arg.get_default_values(),
            [std::ffi::OsStr::new("9")],
            "the default did not reach the arg"
        );
    }

    /// And a run parsed through the mutated tree sees it, which is the whole
    /// mechanism in one assertion.
    #[test]
    fn a_parsed_run_takes_the_configured_default() {
        use crate::cli::Command as Sub;
        let applied = apply(Cli::command(), &[("max-peers", "9".to_string())]);
        let matches = applied
            .try_get_matches_from(["bit-cli", "download", "x.torrent"])
            .expect("it parses");
        let cli = <Cli as clap::FromArgMatches>::from_arg_matches(&matches).expect("it maps");
        let Some(Sub::Download(args)) = cli.command else {
            panic!("not a download");
        };
        assert_eq!(args.limits.max_peers, Some(9));
    }

    /// The command line still wins, which is what setting a **default** buys
    /// rather than something that has to be implemented.
    #[test]
    fn the_command_line_beats_a_configured_default() {
        use crate::cli::Command as Sub;
        let applied = apply(Cli::command(), &[("max-peers", "9".to_string())]);
        let matches = applied
            .try_get_matches_from(["bit-cli", "download", "x.torrent", "--max-peers", "3"])
            .expect("it parses");
        let cli = <Cli as clap::FromArgMatches>::from_arg_matches(&matches).expect("it maps");
        let Some(Sub::Download(args)) = cli.command else {
            panic!("not a download");
        };
        assert_eq!(args.limits.max_peers, Some(3));
    }
}
