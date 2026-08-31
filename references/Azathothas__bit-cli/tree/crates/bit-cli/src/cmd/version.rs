//! `bit-cli version`: what this build is and what it supports.

use bit_cli_core::ExitCode;
use bit_cli_core::error::Result;
use bit_cli_core::exit::ExitCode as Code;
use serde::Serialize;

use crate::env::Env;
use crate::output::{Renderer, field, table};

/// The triple this binary was built for.
const TARGET: &str = env!("BIT_CLI_TARGET");

/// One exit code, for the documented table.
#[derive(Debug, Clone, Serialize)]
pub struct ExitCodeRow {
    pub code: u8,
    pub kind: &'static str,
    pub description: &'static str,
}

/// One trace subsystem.
#[derive(Debug, Clone, Serialize)]
pub struct SubsystemRow {
    pub name: &'static str,
    pub description: &'static str,
}

/// What `bit-cli version` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub version: &'static str,
    pub schema_version: &'static str,
    pub target: &'static str,
    pub features: Vec<&'static str>,
    pub exit_codes: Vec<ExitCodeRow>,
    pub trace_subsystems: Vec<SubsystemRow>,
    pub composition_modes: Vec<&'static str>,
    pub lints: Vec<&'static str>,
}

impl Report {
    /// Gather everything about this build.
    pub fn gather() -> Self {
        let mut features = Vec::new();
        if cfg!(feature = "dht") {
            features.push("dht");
        }
        if cfg!(feature = "pex") {
            features.push("pex");
        }
        if cfg!(feature = "lsd") {
            features.push("lsd");
        }
        if cfg!(feature = "upnp") {
            features.push("upnp");
        }
        Self {
            version: bit_cli_core::VERSION,
            schema_version: crate::output::SCHEMA_VERSION,
            target: TARGET,
            features,
            exit_codes: Code::ALL
                .iter()
                .map(|c| ExitCodeRow {
                    code: c.code(),
                    kind: c.kind(),
                    description: c.description(),
                })
                .collect(),
            trace_subsystems: crate::logging::SUBSYSTEMS
                .iter()
                .map(|s| SubsystemRow {
                    name: s.name,
                    description: s.description,
                })
                .collect(),
            composition_modes: bit_cli_core::webseed::Mode::ALL
                .iter()
                .map(|m| m.as_str())
                .collect(),
            lints: bit_cli_core::torrent::Lint::ALL
                .iter()
                .map(|l| l.name())
                .collect(),
        }
    }

    /// The text rendering.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field("bit-cli", self.version),
            field("target", self.target),
            field("schema", self.schema_version),
            field("features", self.features.join(", ")),
            field("web seed modes", self.composition_modes.join(", ")),
            String::new(),
            "Exit codes".to_string(),
        ];
        let rows: Vec<Vec<String>> = self
            .exit_codes
            .iter()
            .map(|c| {
                vec![
                    c.code.to_string(),
                    c.kind.to_string(),
                    c.description.to_string(),
                ]
            })
            .collect();
        out.extend(table(&["CODE", "KIND", "MEANING"], &rows));
        out
    }
}

/// Run the command.
pub fn run(renderer: &mut Renderer, env: &mut Env) -> Result<ExitCode> {
    let report = Report::gather();
    renderer.emit(env, "version", &report, || report.lines())?;
    Ok(ExitCode::Success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{run_json, run_ok};

    #[test]
    fn the_exit_code_table_is_complete_and_in_order() {
        let doc = run_json(&["version"], ".");
        let codes = doc["exit_codes"].as_array().unwrap();
        assert_eq!(codes.len(), Code::ALL.len());
        for (index, entry) in codes.iter().enumerate() {
            assert_eq!(entry["code"], index);
            assert!(!entry["kind"].as_str().unwrap().is_empty());
            assert!(!entry["description"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn the_text_form_lists_every_exit_code_too() {
        let out = run_ok(&["version"], ".");
        for code in Code::ALL {
            assert!(
                out.contains(code.kind()),
                "{} missing from:\n{out}",
                code.kind()
            );
        }
    }

    #[test]
    fn the_build_target_is_recorded() {
        let doc = run_json(&["version"], ".");
        let target = doc["target"].as_str().unwrap();
        assert!(target.contains('-'), "{target} is not a target triple");
    }

    #[test]
    fn every_trace_subsystem_and_lint_is_listed() {
        let doc = run_json(&["version"], ".");
        assert_eq!(
            doc["trace_subsystems"].as_array().unwrap().len(),
            crate::logging::SUBSYSTEMS.len()
        );
        assert_eq!(
            doc["lints"].as_array().unwrap().len(),
            bit_cli_core::torrent::Lint::ALL.len()
        );
    }
}
