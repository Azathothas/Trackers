//! `bit-cli config show`: the resolved configuration and where each value came
//! from.

use std::path::PathBuf;

use bit_cli_core::ExitCode;
use bit_cli_core::config::{ConfigFile, Origin, PROJECT_CONFIG, Resolved, user_config_path};
use bit_cli_core::error::Result;
use serde::Serialize;

use crate::cli::{ConfigCommand, Global};
use crate::env::Env;
use crate::output::{Renderer, table};

/// What `bit-cli config show` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    #[serde(flatten)]
    pub resolved: Resolved,
}

impl Report {
    /// The text rendering: one row per setting, with its origin.
    pub fn lines(&self) -> Vec<String> {
        let rows: Vec<Vec<String>> = self
            .resolved
            .settings
            .iter()
            .map(|(name, setting)| {
                let value = match &setting.value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                vec![name.clone(), value, setting.origin.label()]
            })
            .collect();
        let mut out = table(&["SETTING", "VALUE", "ORIGIN"], &rows);
        if !self.resolved.files_read.is_empty() {
            out.push(String::new());
            for path in &self.resolved.files_read {
                out.push(format!("read    {}", path.display()));
            }
        }
        for path in &self.resolved.files_missing {
            out.push(format!("absent  {}", path.display()));
        }
        out
    }
}

/// `BIT_CLI_*` variables this program sets or reads that are not settings.
///
/// The hook variables are not listed here: [`crate::hooks::VARIABLES`] is the
/// one list of those and [`reserved`] reads it, so a hook variable added there
/// is reserved here without anybody remembering to. These two have no other
/// list to read.
///
/// `BIT_CLI_TARGET` is set by the build script and is in the environment of
/// anything `cargo` runs, so before it was reserved every run under
/// `cargo test` failed. `BIT_CLI_UPDATE_FLAGS` is read by the short-flag test.
///
/// `BIT_CLI_EXTRA_CA_FILE` names a PEM bundle of certificate authorities the
/// source-document fetcher trusts **in addition to** the usual roots. It is
/// not a setting because it is an operator's trust decision about the whole
/// process rather than a per-run option, which is the same reason
/// `SSL_CERT_FILE` is an environment variable everywhere else. See
/// `bit_cli_core::fetch::EXTRA_CA_FILE_ENV` and `TODO/cli-surface.md`, T-244.
const NOT_SETTINGS: &[&str] = &[
    "BIT_CLI_TARGET",
    "BIT_CLI_UPDATE_FLAGS",
    bit_cli_core::fetch::EXTRA_CA_FILE_ENV,
];

/// Every `BIT_CLI_*` name that is not a setting, so a run does not refuse one
/// of its own variables as a misspelt setting.
pub fn reserved() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = crate::hooks::VARIABLES.iter().map(|(n, _)| *n).collect();
    out.extend_from_slice(NOT_SETTINGS);
    out
}

/// Resolve the configuration from every layer.
pub fn resolve(global: &Global, env: &Env) -> Result<Resolved> {
    let reserved = reserved();
    let mut resolved = Resolved::defaults();
    if global.no_config {
        // `--no-config` skips the files but not the environment or the flags,
        // which are what the caller just typed.
        resolved.apply_env(&env.vars, &reserved)?;
        apply_flags(&mut resolved, global);
        return Ok(resolved);
    }

    let consider = |resolved: &mut Resolved, path: PathBuf, origin: Origin| -> Result<()> {
        match ConfigFile::read_optional(&path)? {
            Some(file) => resolved.apply_file(&file, origin, &path),
            None => resolved.missed(path),
        }
        Ok(())
    };

    if let Some(path) = user_config_path(&env.vars) {
        consider(&mut resolved, path.clone(), Origin::UserConfig { path })?;
    }
    let project = env.cwd.join(PROJECT_CONFIG);
    consider(
        &mut resolved,
        project.clone(),
        Origin::ProjectConfig { path: project },
    )?;

    if let Some(explicit) = &global.config {
        let path = env.resolve(explicit);
        // An explicit --config that does not exist is an error, unlike the
        // files that are merely looked for.
        let file = ConfigFile::read(&path)?;
        resolved.apply_file(&file, Origin::ExplicitConfig { path: path.clone() }, &path);
    }

    resolved.apply_env(&env.vars, &reserved)?;
    apply_flags(&mut resolved, global);
    Ok(resolved)
}

/// Fold the global flags into the resolved configuration.
fn apply_flags(resolved: &mut Resolved, global: &Global) {
    if let Some(dir) = &global.dir {
        resolved.apply(
            vec![("download_directory", dir.display().to_string().into())],
            Origin::Flag { name: "dir".into() },
        );
    }
    resolved.apply(
        vec![(
            "log_level",
            format!("{:?}", global.log_level).to_lowercase().into(),
        )],
        match global.verbose > 0 {
            true => Origin::Flag {
                name: "verbose".into(),
            },
            false => Origin::Default,
        },
    );
}

/// Run the command.
pub fn run(
    command: &ConfigCommand,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    match command {
        ConfigCommand::Show => {
            let report = Report {
                resolved: resolve(global, env)?,
            };
            renderer.emit(env, "config", &report, || report.lines())?;
            Ok(ExitCode::Success)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{run_err, run_json, run_ok};

    fn workspace() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn every_setting_is_reported_with_its_origin() {
        let dir = workspace();
        let doc = run_json(&["config", "show", "--no-config"], dir.path());
        let settings = doc["settings"].as_object().unwrap();
        assert_eq!(settings.len(), bit_cli_core::config::SETTINGS.len());
        for (name, _, _) in bit_cli_core::config::SETTINGS {
            let setting = &settings[*name];
            assert!(
                setting["origin"]["kind"].is_string(),
                "{name} has no origin"
            );
        }
    }

    #[test]
    fn the_text_form_shows_the_same_settings() {
        let dir = workspace();
        let out = run_ok(&["config", "show", "--no-config"], dir.path());
        assert!(out.starts_with("SETTING"), "{out}");
        for (name, _, _) in bit_cli_core::config::SETTINGS {
            assert!(out.contains(name), "{name} missing from:\n{out}");
        }
    }

    #[test]
    fn a_project_config_overrides_the_defaults() {
        let dir = workspace();
        std::fs::write(dir.path().join(PROJECT_CONFIG), "max_peers = 42\n").unwrap();
        let doc = run_json(&["config", "show"], dir.path());
        assert_eq!(doc["settings"]["max_peers"]["value"], 42);
        assert_eq!(
            doc["settings"]["max_peers"]["origin"]["kind"],
            "project_config"
        );
    }

    #[test]
    fn no_config_ignores_the_files() {
        let dir = workspace();
        std::fs::write(dir.path().join(PROJECT_CONFIG), "max_peers = 42\n").unwrap();
        let doc = run_json(&["config", "show", "--no-config"], dir.path());
        assert_eq!(doc["settings"]["max_peers"]["origin"]["kind"], "default");
    }

    #[test]
    fn an_explicit_config_beats_the_project_one() {
        let dir = workspace();
        std::fs::write(dir.path().join(PROJECT_CONFIG), "max_peers = 42\n").unwrap();
        let explicit = dir.path().join("other.toml");
        std::fs::write(&explicit, "max_peers = 99\n").unwrap();
        let doc = run_json(
            &["config", "show", "--config", explicit.to_str().unwrap()],
            dir.path(),
        );
        assert_eq!(doc["settings"]["max_peers"]["value"], 99);
        assert_eq!(
            doc["settings"]["max_peers"]["origin"]["kind"],
            "explicit_config"
        );
    }

    #[test]
    fn a_flag_beats_every_file() {
        let dir = workspace();
        std::fs::write(
            dir.path().join(PROJECT_CONFIG),
            "download_directory = \"/from-file\"\n",
        )
        .unwrap();
        let doc = run_json(&["config", "show", "-d", "/from-flag"], dir.path());
        assert_eq!(doc["settings"]["download_directory"]["value"], "/from-flag");
        assert_eq!(
            doc["settings"]["download_directory"]["origin"]["kind"],
            "flag"
        );
    }

    #[test]
    fn a_missing_explicit_config_is_an_error() {
        let dir = workspace();
        run_err(
            &["config", "show", "--config", "nope.toml"],
            dir.path(),
            ExitCode::Disk,
        );
    }

    #[test]
    fn an_invalid_config_file_is_a_config_error() {
        let dir = workspace();
        std::fs::write(dir.path().join(PROJECT_CONFIG), "max_peerz = 1\n").unwrap();
        let err = run_err(&["config", "show"], dir.path(), ExitCode::Config);
        assert!(err.contains("max_peerz"), "{err}");
    }

    #[test]
    fn files_that_were_looked_for_are_reported() {
        let dir = workspace();
        let doc = run_json(&["config", "show"], dir.path());
        let missing = doc["files_missing"].as_array().unwrap();
        assert!(
            missing
                .iter()
                .any(|p| p.as_str().unwrap().ends_with(PROJECT_CONFIG)),
            "the project config should be listed as absent: {missing:?}"
        );
    }

    // ----------------------------------------------------------------------
    // T-222: the configuration reaches a run, not only `config show`.
    //
    // Every case below drives a command that is **not** `config show`, which
    // is the whole point: until 2026-08-23 `--config`, `--no-config`,
    // `bit-cli.toml`, the user config file and every `BIT_CLI_*` variable
    // changed what one command printed and nothing about what any command
    // did.
    //
    // `download_directory` is the setting under test in most of them because
    // it is the one whose effect is a file on the disk rather than a number in
    // a report, so a case that passes cannot be passing on the report and the
    // run disagreeing.
    // ----------------------------------------------------------------------

    /// A download that fetches its payload over HTTP from loopback, with the
    /// arguments a case wants and the environment a case sets.
    ///
    /// Returns where the payload landed, or `None`, plus the exit code.
    fn download_with(
        cwd: &std::path::Path,
        extra: &[&str],
        vars: &[(&str, &str)],
    ) -> (ExitCode, Option<PathBuf>, String) {
        let fixture = crate::test_support::TorrentFixture::single_file();
        let server = crate::test_support::FileServer::start(fixture.payload_dir());
        let mut args = vec![
            "download".to_string(),
            fixture.path_str().to_string(),
            "--web-seed-only".to_string(),
            "--web-seed".to_string(),
            format!("{}/{}", server.base, fixture.files[0].0),
            "--port".to_string(),
            "0".to_string(),
        ];
        args.extend(extra.iter().map(|a| (*a).to_string()));
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let (mut env, captured) = Env::test(&borrowed, cwd);
        for (name, value) in vars {
            env.vars.insert((*name).to_string(), (*value).to_string());
        }
        let code = crate::run(&mut env);
        let said = format!("{}{}", captured.out(), captured.err());
        (code, None, said)
    }

    /// Where `payload.bin` landed under `root`, if it did.
    fn landed(root: &std::path::Path) -> Option<u64> {
        std::fs::metadata(root.join("payload.bin"))
            .ok()
            .map(|m| m.len())
    }

    /// The payload is 3,000 bytes, from `TorrentFixture::single_file`.
    const PAYLOAD: u64 = 3000;

    #[test]
    fn a_project_config_decides_where_a_download_lands() {
        let work = workspace();
        let wanted = work.path().join("from-config");
        std::fs::write(
            work.path().join(PROJECT_CONFIG),
            format!(
                "download_directory = {:?}\n",
                wanted.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        let (code, _, said) = download_with(work.path(), &[], &[]);
        assert_eq!(code, ExitCode::Success, "{said}");
        assert_eq!(landed(&wanted), Some(PAYLOAD), "{said}");
        assert_eq!(landed(work.path()), None, "it landed in the cwd instead");
    }

    #[test]
    fn an_environment_variable_decides_it_too() {
        let work = workspace();
        let wanted = work.path().join("from-env");
        let (code, _, said) = download_with(
            work.path(),
            &[],
            &[(
                "BIT_CLI_DOWNLOAD_DIRECTORY",
                &wanted.to_string_lossy().replace('\\', "/"),
            )],
        );
        assert_eq!(code, ExitCode::Success, "{said}");
        assert_eq!(landed(&wanted), Some(PAYLOAD), "{said}");
    }

    #[test]
    fn an_explicit_config_beats_the_project_one_in_a_run() {
        let work = workspace();
        let losing = work.path().join("from-project");
        let winning = work.path().join("from-explicit");
        std::fs::write(
            work.path().join(PROJECT_CONFIG),
            format!(
                "download_directory = {:?}\n",
                losing.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        let explicit = work.path().join("other.toml");
        std::fs::write(
            &explicit,
            format!(
                "download_directory = {:?}\n",
                winning.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        let (code, _, said) =
            download_with(work.path(), &["--config", explicit.to_str().unwrap()], &[]);
        assert_eq!(code, ExitCode::Success, "{said}");
        assert_eq!(landed(&winning), Some(PAYLOAD), "{said}");
        assert_eq!(landed(&losing), None, "the project config won");
    }

    #[test]
    fn a_flag_beats_every_layer_in_a_run() {
        let work = workspace();
        let losing = work.path().join("from-config");
        let winning = work.path().join("from-flag");
        std::fs::write(
            work.path().join(PROJECT_CONFIG),
            format!(
                "download_directory = {:?}\n",
                losing.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        let (code, _, said) = download_with(
            work.path(),
            &["--dir", winning.to_str().unwrap()],
            &[(
                "BIT_CLI_DOWNLOAD_DIRECTORY",
                &losing.to_string_lossy().replace('\\', "/"),
            )],
        );
        assert_eq!(code, ExitCode::Success, "{said}");
        assert_eq!(landed(&winning), Some(PAYLOAD), "{said}");
        assert_eq!(landed(&losing), None, "a file beat the command line");
    }

    #[test]
    fn no_config_turns_the_files_off_for_a_run() {
        let work = workspace();
        let ignored = work.path().join("from-config");
        std::fs::write(
            work.path().join(PROJECT_CONFIG),
            format!(
                "download_directory = {:?}\n",
                ignored.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        let (code, _, said) = download_with(work.path(), &["--no-config"], &[]);
        assert_eq!(code, ExitCode::Success, "{said}");
        assert_eq!(landed(&ignored), None, "--no-config did not ignore it");
        assert_eq!(landed(work.path()), Some(PAYLOAD), "{said}");
    }

    /// `--config` naming a file that is not there is the same failure on every
    /// command. It used to be exit 8 on `config show` and exit 0, in silence,
    /// everywhere else: the same flag with the same value, two behaviours.
    #[test]
    fn a_missing_explicit_config_fails_the_same_way_on_every_command() {
        let work = workspace();
        let missing = work.path().join("nope.toml");
        for args in [
            vec!["config", "show"],
            vec!["version"],
            vec!["info", "x.torrent"],
        ] {
            let mut full = vec!["--config", missing.to_str().unwrap()];
            full.extend(args.iter().copied());
            let (mut env, captured) = Env::test(&full, work.path());
            let code = crate::run(&mut env);
            assert_eq!(
                code,
                ExitCode::Disk,
                "{:?} did not refuse a missing --config: {}{}",
                args,
                captured.out(),
                captured.err()
            );
        }
    }

    /// A `BIT_CLI_*` variable this program sets itself is not a misspelt
    /// setting. `BIT_CLI_TARGET` is in the environment of anything `cargo`
    /// runs and `BIT_CLI_HOOK` is set for a hook, so refusing them made every
    /// run under `cargo test` fail and would have broken a hook that runs
    /// `bit-cli`.
    #[test]
    fn a_variable_this_program_sets_itself_does_not_fail_a_run() {
        let work = workspace();
        for (name, value) in [
            ("BIT_CLI_HOOK", "on-complete"),
            ("BIT_CLI_TARGET", "x86_64-pc-windows-msvc"),
            ("BIT_CLI_INFO_HASH", "abc"),
        ] {
            let (mut env, captured) = Env::test(&["version"], work.path());
            env.vars.insert(name.to_string(), value.to_string());
            let code = crate::run(&mut env);
            assert_eq!(
                code,
                ExitCode::Success,
                "{name} was refused: {}{}",
                captured.out(),
                captured.err()
            );
        }
        // And an actual typo is still caught.
        let (mut env, _captured) = Env::test(&["version"], work.path());
        env.vars
            .insert("BIT_CLI_MAX_PEERZ".to_string(), "1".to_string());
        assert_eq!(crate::run(&mut env), ExitCode::Config);
    }
}
