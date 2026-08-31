//! Configuration, with the origin of every value.
//!
//! Precedence, highest first:
//!
//! 1. Command-line flags
//! 2. Environment variables, prefixed `BIT_CLI_`
//! 3. `--config <PATH>`
//! 4. Project config, `./bit-cli.toml`
//! 5. User config, the platform config directory
//! 6. Built-in defaults
//!
//! Every resolved value remembers where it came from. That is what makes the
//! tool debuggable in CI: `bit-cli config show --json` answers "why is this
//! set to that" without anyone having to reason about the layering.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, from_io};

/// Where a value came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// A built-in default.
    Default,
    /// The user config file.
    UserConfig { path: PathBuf },
    /// `./bit-cli.toml`.
    ProjectConfig { path: PathBuf },
    /// An explicit `--config`.
    ExplicitConfig { path: PathBuf },
    /// A `BIT_CLI_*` environment variable.
    Environment { name: String },
    /// A command-line flag.
    Flag { name: String },
}

impl Origin {
    /// Precedence, higher wins.
    pub const fn rank(&self) -> u8 {
        match self {
            Self::Default => 0,
            Self::UserConfig { .. } => 1,
            Self::ProjectConfig { .. } => 2,
            Self::ExplicitConfig { .. } => 3,
            Self::Environment { .. } => 4,
            Self::Flag { .. } => 5,
        }
    }

    /// A short label for the text rendering.
    pub fn label(&self) -> String {
        match self {
            Self::Default => "default".to_string(),
            Self::UserConfig { path } => format!("user config ({})", path.display()),
            Self::ProjectConfig { path } => format!("project config ({})", path.display()),
            Self::ExplicitConfig { path } => format!("--config ({})", path.display()),
            Self::Environment { name } => format!("env {name}"),
            Self::Flag { name } => format!("flag --{name}"),
        }
    }
}

/// One resolved setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setting {
    /// The value, as JSON so any type round-trips.
    pub value: serde_json::Value,
    /// Where it came from.
    pub origin: Origin,
}

/// The settings a config file can carry.
///
/// Every field is optional: a config file that sets one key is valid, and an
/// absent key falls through to the next layer rather than resetting it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ConfigFile {
    pub download_directory: Option<PathBuf>,
    pub listen_port: Option<String>,
    pub enable_dht: Option<bool>,
    pub enable_pex: Option<bool>,
    pub enable_lsd: Option<bool>,
    pub max_peers: Option<usize>,
    pub max_peers_total: Option<usize>,
    pub max_download_rate: Option<String>,
    pub max_upload_rate: Option<String>,
    pub max_concurrent_downloads: Option<usize>,
    pub seed_ratio: Option<f64>,
    pub seed_time: Option<String>,
    pub enable_web_seeds: Option<bool>,
    pub web_seed_concurrency: Option<usize>,
    pub web_seed_chunk_size: Option<String>,
    pub web_seed_timeout: Option<String>,
    pub web_seed_user_agent: Option<String>,
    pub file_allocation: Option<String>,
    pub piece_selector: Option<String>,
    pub log_level: Option<String>,
    pub log_format: Option<String>,
    pub color: Option<String>,
}

impl ConfigFile {
    /// Read a config file.
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| from_io(e, format!("cannot read {}", path.display())))?;
        toml::from_str(&text).map_err(|e| {
            Error::config(format!("{}: {e}", path.display()))
                .with("path", path.display().to_string())
        })
    }

    /// Read a config file, treating a missing one as empty.
    pub fn read_optional(path: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map(Some).map_err(|e| {
                Error::config(format!("{}: {e}", path.display()))
                    .with("path", path.display().to_string())
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(from_io(e, format!("cannot read {}", path.display()))),
        }
    }

    /// The settings this file sets, as name and JSON value pairs.
    pub fn entries(&self) -> Vec<(&'static str, serde_json::Value)> {
        let mut out = Vec::new();
        let mut push = |name: &'static str, value: Option<serde_json::Value>| {
            if let Some(value) = value {
                out.push((name, value));
            }
        };
        push(
            "download_directory",
            self.download_directory
                .as_ref()
                .map(|p| p.display().to_string().into()),
        );
        push("listen_port", self.listen_port.clone().map(Into::into));
        push("enable_dht", self.enable_dht.map(Into::into));
        push("enable_pex", self.enable_pex.map(Into::into));
        push("enable_lsd", self.enable_lsd.map(Into::into));
        push("max_peers", self.max_peers.map(Into::into));
        push("max_peers_total", self.max_peers_total.map(Into::into));
        push(
            "max_download_rate",
            self.max_download_rate.clone().map(Into::into),
        );
        push(
            "max_upload_rate",
            self.max_upload_rate.clone().map(Into::into),
        );
        push(
            "max_concurrent_downloads",
            self.max_concurrent_downloads.map(Into::into),
        );
        push("seed_ratio", self.seed_ratio.map(Into::into));
        push("seed_time", self.seed_time.clone().map(Into::into));
        push("enable_web_seeds", self.enable_web_seeds.map(Into::into));
        push(
            "web_seed_concurrency",
            self.web_seed_concurrency.map(Into::into),
        );
        push(
            "web_seed_chunk_size",
            self.web_seed_chunk_size.clone().map(Into::into),
        );
        push(
            "web_seed_timeout",
            self.web_seed_timeout.clone().map(Into::into),
        );
        push(
            "web_seed_user_agent",
            self.web_seed_user_agent.clone().map(Into::into),
        );
        push(
            "file_allocation",
            self.file_allocation.clone().map(Into::into),
        );
        push(
            "piece_selector",
            self.piece_selector.clone().map(Into::into),
        );
        push("log_level", self.log_level.clone().map(Into::into));
        push("log_format", self.log_format.clone().map(Into::into));
        push("color", self.color.clone().map(Into::into));
        out
    }
}

/// Every setting name, with its default and a one-line description.
///
/// This is the single list the defaults, the documentation, and the
/// completeness test all read from, so a new setting cannot appear in one and
/// be missing from another.
pub const SETTINGS: &[(&str, &str, &str)] = &[
    ("download_directory", ".", "Where payloads are written"),
    (
        "listen_port",
        "6881-6889",
        "Inclusive port range for incoming peer connections",
    ),
    (
        "enable_dht",
        "true",
        "Use the DHT. Always off for a private torrent",
    ),
    (
        "enable_pex",
        "true",
        "Use peer exchange. Always off for a private torrent",
    ),
    (
        "enable_lsd",
        "true",
        "Use local service discovery. Always off for a private torrent",
    ),
    ("max_peers", "60", "Peer connections per torrent"),
    ("max_peers_total", "200", "Peer connections across the run"),
    (
        "max_download_rate",
        "unlimited",
        "Download rate cap per torrent",
    ),
    (
        "max_upload_rate",
        "unlimited",
        "Upload rate cap per torrent",
    ),
    (
        "max_concurrent_downloads",
        "1",
        "Sources fetched in parallel in one invocation",
    ),
    (
        "seed_ratio",
        "0",
        "Stop seeding at this ratio. 0 means do not seed",
    ),
    (
        "seed_time",
        "0",
        "Stop seeding after this long. 0 means do not seed",
    ),
    ("enable_web_seeds", "true", "Honour web seeds"),
    (
        "web_seed_concurrency",
        "4",
        "Concurrent ranged requests per source",
    ),
    ("web_seed_chunk_size", "4MiB", "Bytes per ranged request"),
    (
        "web_seed_timeout",
        "30s",
        "Per-request timeout for web seeds",
    ),
    (
        "web_seed_user_agent",
        "bit-cli/<version>",
        "User-Agent for web seed requests",
    ),
    ("file_allocation", "sparse", "How disk space is reserved"),
    (
        "piece_selector",
        "default",
        "Which piece to request next: default, sequential, or in-order",
    ),
    ("log_level", "warn", "Log severity"),
    ("log_format", "text", "Log rendering"),
    ("color", "auto", "When to colour output"),
];

/// The fully resolved configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resolved {
    /// Every setting, by name.
    pub settings: BTreeMap<String, Setting>,
    /// Config files that were read, in the order they were applied.
    pub files_read: Vec<PathBuf>,
    /// Config files that were looked for and not found.
    pub files_missing: Vec<PathBuf>,
}

impl Resolved {
    /// Start from the built-in defaults.
    pub fn defaults() -> Self {
        let settings = SETTINGS
            .iter()
            .map(|(name, default, _)| {
                (
                    (*name).to_string(),
                    Setting {
                        value: serde_json::Value::String((*default).to_string()),
                        origin: Origin::Default,
                    },
                )
            })
            .collect();
        Self {
            settings,
            files_read: Vec::new(),
            files_missing: Vec::new(),
        }
    }

    /// Apply one layer.
    ///
    /// A layer only overrides a value that came from a lower-ranked origin, so
    /// applying layers out of order cannot produce the wrong answer.
    pub fn apply(&mut self, entries: Vec<(&str, serde_json::Value)>, origin: Origin) {
        for (name, value) in entries {
            let replace = self
                .settings
                .get(name)
                .is_none_or(|existing| origin.rank() >= existing.origin.rank());
            if replace {
                self.settings.insert(
                    name.to_string(),
                    Setting {
                        value,
                        origin: origin.clone(),
                    },
                );
            }
        }
    }

    /// Record a config file that was looked for and is not there.
    ///
    /// A layer, like the ones that were found: a caller debugging why a
    /// setting is at its default needs to see the file that would have changed
    /// it being absent, and that is the one step of the resolution with
    /// nothing else to show for it.
    pub fn missed(&mut self, path: PathBuf) {
        self.files_missing.push(path);
    }

    /// Apply a config file layer, recording that the file was read.
    pub fn apply_file(&mut self, file: &ConfigFile, origin: Origin, path: &Path) {
        self.files_read.push(path.to_path_buf());
        self.apply(file.entries(), origin);
    }

    /// Apply environment variables, which are `BIT_CLI_` plus the upper-cased
    /// setting name.
    ///
    /// `reserved` names `BIT_CLI_*` variables that are **not** settings and
    /// must not be refused as typos. There are three kinds and all three are
    /// this program's own: the twenty variables a hook receives, which
    /// `bit_cli::hooks::VARIABLES` lists; `BIT_CLI_TARGET`, which the build
    /// script sets; and `BIT_CLI_UPDATE_FLAGS`, which a test reads. The caller
    /// assembles the list, because the hook table lives in the binary crate
    /// and this one is below it.
    ///
    /// Until T-222 this ran on `bit-cli config show` alone, so the collision
    /// was invisible. Making the configuration reach every command made every
    /// run under `cargo test` fail on `BIT_CLI_TARGET`, and would have made a
    /// hook that runs `bit-cli` fail on `BIT_CLI_HOOK`: the hook sets it, the
    /// child reads it, and the child refused it as a misspelt setting. See
    /// `TODO/cli-surface.md`, T-222.
    pub fn apply_env(&mut self, vars: &BTreeMap<String, String>, reserved: &[&str]) -> Result<()> {
        for (name, _, _) in SETTINGS {
            let key = format!("BIT_CLI_{}", name.to_uppercase());
            if let Some(value) = vars.get(&key) {
                self.apply(
                    vec![(*name, serde_json::Value::String(value.clone()))],
                    Origin::Environment { name: key },
                );
            }
        }
        // A `BIT_CLI_` variable that matches no setting is almost always a
        // typo in a deployment script, and silently ignoring it is how a
        // production setting goes missing.
        for key in vars.keys() {
            if let Some(rest) = key.strip_prefix("BIT_CLI_") {
                if reserved.contains(&key.as_str()) {
                    continue;
                }
                let name = rest.to_lowercase();
                if !SETTINGS.iter().any(|(known, _, _)| *known == name) {
                    return Err(Error::config(format!(
                        "`{key}` is not a setting; run `bit-cli config show` for the list"
                    ))
                    .with("variable", key.clone()));
                }
            }
        }
        Ok(())
    }

    /// Write the whole resolution to `bit_cli::config`, for `--trace config`.
    ///
    /// Called once, from `run`, **after** the log subscriber is installed.
    /// That ordering is forced rather than chosen: the configuration decides
    /// the log level, so it has to be resolved before there is anything to
    /// write records to, and a resolver that traced as it went would emit into
    /// a subscriber that did not exist yet. See `TODO/cli-surface.md`, T-219
    /// and T-222.
    ///
    /// One record per file considered and one per setting. The absent files
    /// are in it because a caller asking why a setting is at its default needs
    /// to see the file that would have changed it not being there, and that is
    /// the one step of the resolution with nothing else to show for it.
    pub fn trace(&self) {
        for path in &self.files_read {
            tracing::trace!(
                target: "bit_cli::config",
                path = %path.display(),
                read = true,
                "config file"
            );
        }
        for path in &self.files_missing {
            tracing::trace!(
                target: "bit_cli::config",
                path = %path.display(),
                read = false,
                "config file"
            );
        }
        for (name, setting) in &self.settings {
            tracing::trace!(
                target: "bit_cli::config",
                setting = %name,
                value = %setting.value,
                origin = %setting.origin.label(),
                rank = setting.origin.rank(),
                "resolved"
            );
        }
    }

    /// The settings a config file or the environment set, as the setting name
    /// and its value.
    ///
    /// What the caller does with them is turn each into the default of the
    /// flag it names, which is `bit_cli::config_defaults`.
    ///
    /// A value whose origin is a **flag** is left out: it is already on the
    /// command line, and handing it back as a default would be a second copy
    /// of the same decision. A value at its built-in default is left out for
    /// the same reason, because that is what the flag already has.
    pub fn configured(&self) -> Vec<(&str, String)> {
        self.settings
            .iter()
            .filter(|(_, setting)| (1..=4).contains(&setting.origin.rank()))
            .map(|(name, setting)| {
                let text = match &setting.value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                (name.as_str(), text)
            })
            .collect()
    }

    /// The value of one setting, if it is set.
    pub fn get(&self, name: &str) -> Option<&Setting> {
        self.settings.get(name)
    }

    /// The value of one setting as a string.
    pub fn get_str(&self, name: &str) -> Option<String> {
        self.settings.get(name).map(|s| match &s.value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        })
    }
}

/// The default path of the user config file.
///
/// The variables are passed in rather than read from the process, and that is
/// not a style choice. Configuration decides what a run does now, by T-222, so
/// a test that resolved it against the real process environment would be
/// reading whatever config file the machine it runs on happens to have. `Env`
/// already carries the variables and `Env::test` carries none, so a test sees
/// no user config unless it puts one there.
pub fn user_config_path(vars: &BTreeMap<String, String>) -> Option<PathBuf> {
    // The platform config directory, resolved without pulling in a crate for
    // three environment variables.
    #[cfg(windows)]
    let base = vars.get("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = vars
        .get("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| vars.get("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.map(|dir| dir.join("bit-cli").join("config.toml"))
}

/// The project config file name, looked for in the working directory.
pub const PROJECT_CONFIG: &str = "bit-cli.toml";

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> serde_json::Value {
        serde_json::Value::String(text.to_string())
    }

    #[test]
    fn defaults_cover_every_documented_setting() {
        let resolved = Resolved::defaults();
        assert_eq!(resolved.settings.len(), SETTINGS.len());
        for (name, default, description) in SETTINGS {
            let setting = resolved
                .get(name)
                .unwrap_or_else(|| panic!("{name} has no default"));
            assert_eq!(setting.origin, Origin::Default);
            assert_eq!(setting.value, value(default));
            assert!(!description.is_empty(), "{name} has no description");
        }
    }

    #[test]
    fn setting_names_are_unique_and_snake_case() {
        let mut names: Vec<&str> = SETTINGS.iter().map(|(n, _, _)| *n).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
        for name in names {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{name}"
            );
        }
    }

    #[test]
    fn a_higher_layer_wins() {
        let mut resolved = Resolved::defaults();
        resolved.apply(
            vec![("max_peers", 10.into())],
            Origin::UserConfig { path: "u".into() },
        );
        assert_eq!(resolved.get("max_peers").unwrap().value, 10);

        resolved.apply(
            vec![("max_peers", 20.into())],
            Origin::Flag {
                name: "max-peers".into(),
            },
        );
        assert_eq!(resolved.get("max_peers").unwrap().value, 20);
        assert!(matches!(
            resolved.get("max_peers").unwrap().origin,
            Origin::Flag { .. }
        ));
    }

    #[test]
    fn a_lower_layer_cannot_overwrite_a_higher_one() {
        let mut resolved = Resolved::defaults();
        resolved.apply(
            vec![("max_peers", 20.into())],
            Origin::Flag {
                name: "max-peers".into(),
            },
        );
        resolved.apply(
            vec![("max_peers", 10.into())],
            Origin::UserConfig { path: "u".into() },
        );
        assert_eq!(
            resolved.get("max_peers").unwrap().value,
            20,
            "the flag still wins"
        );
    }

    #[test]
    fn the_precedence_order_is_the_documented_one() {
        let ranks = [
            Origin::Default,
            Origin::UserConfig { path: "u".into() },
            Origin::ProjectConfig { path: "p".into() },
            Origin::ExplicitConfig { path: "e".into() },
            Origin::Environment { name: "E".into() },
            Origin::Flag { name: "f".into() },
        ];
        for pair in ranks.windows(2) {
            assert!(
                pair[0].rank() < pair[1].rank(),
                "{:?} should rank below {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn a_config_file_only_sets_what_it_names() {
        let file: ConfigFile = toml::from_str("max_peers = 42\n").unwrap();
        let entries = file.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "max_peers");

        let mut resolved = Resolved::defaults();
        resolved.apply_file(
            &file,
            Origin::UserConfig { path: "u".into() },
            Path::new("u"),
        );
        assert_eq!(resolved.get("max_peers").unwrap().value, 42);
        assert_eq!(resolved.get("enable_dht").unwrap().origin, Origin::Default);
        assert_eq!(resolved.files_read, [PathBuf::from("u")]);
    }

    #[test]
    fn an_unknown_config_key_is_reported() {
        let err = toml::from_str::<ConfigFile>("max_peerz = 42\n").unwrap_err();
        assert!(err.to_string().contains("max_peerz"), "{err}");
    }

    #[test]
    fn environment_variables_map_to_settings() {
        let mut vars = BTreeMap::new();
        vars.insert("BIT_CLI_MAX_PEERS".to_string(), "77".to_string());
        let mut resolved = Resolved::defaults();
        resolved.apply_env(&vars, &[]).unwrap();
        assert_eq!(resolved.get("max_peers").unwrap().value, value("77"));
        assert!(matches!(
            resolved.get("max_peers").unwrap().origin,
            Origin::Environment { .. }
        ));
    }

    #[test]
    fn a_misspelled_environment_variable_is_refused_rather_than_ignored() {
        let mut vars = BTreeMap::new();
        vars.insert("BIT_CLI_MAX_PEERZ".to_string(), "77".to_string());
        let err = Resolved::defaults().apply_env(&vars, &[]).unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Config);
        assert!(
            err.message().contains("BIT_CLI_MAX_PEERZ"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn unrelated_environment_variables_are_left_alone() {
        let mut vars = BTreeMap::new();
        vars.insert("PATH".to_string(), "/usr/bin".to_string());
        vars.insert("HOME".to_string(), "/home/x".to_string());
        assert!(Resolved::defaults().apply_env(&vars, &[]).is_ok());
    }

    /// A `BIT_CLI_*` variable this program sets itself is not a misspelt
    /// setting, and refusing one made every run under `cargo test` fail on
    /// `BIT_CLI_TARGET`. See `TODO/cli-surface.md`, T-222.
    #[test]
    fn a_reserved_variable_is_not_refused_as_a_typo() {
        let mut vars = BTreeMap::new();
        vars.insert("BIT_CLI_HOOK".to_string(), "on-complete".to_string());
        vars.insert("BIT_CLI_TARGET".to_string(), "x86_64".to_string());
        assert!(
            Resolved::defaults()
                .apply_env(&vars, &["BIT_CLI_HOOK", "BIT_CLI_TARGET"])
                .is_ok()
        );
        // And reserving one does not stop an actual typo being caught.
        vars.insert("BIT_CLI_MAX_PEERZ".to_string(), "1".to_string());
        assert!(
            Resolved::defaults()
                .apply_env(&vars, &["BIT_CLI_HOOK", "BIT_CLI_TARGET"])
                .is_err()
        );
    }

    #[test]
    fn a_missing_config_file_is_not_an_error() {
        let missing = Path::new("definitely-not-here.toml");
        assert!(ConfigFile::read_optional(missing).unwrap().is_none());
        assert!(ConfigFile::read(missing).is_err());
    }

    #[test]
    fn origins_have_readable_labels() {
        assert_eq!(Origin::Default.label(), "default");
        assert_eq!(Origin::Flag { name: "dir".into() }.label(), "flag --dir");
        assert_eq!(
            Origin::Environment {
                name: "BIT_CLI_DIR".into()
            }
            .label(),
            "env BIT_CLI_DIR"
        );
    }
}
