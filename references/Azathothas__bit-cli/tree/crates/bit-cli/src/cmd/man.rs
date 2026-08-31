//! `bit-cli man`: generate a man page, or the same surface as CLIspec JSON.
//!
//! Both are committed, at `man/bit-cli.1` and `man/bit-cli.json`, and
//! `scripts/check-man.ps1` fails the gates when either drifts from what this
//! renders. See `docs/man.md`.

use std::io::Write;

use bit_cli_core::ExitCode;
use bit_cli_core::error::{Result, from_io};
use clap::CommandFactory;

use crate::cli::{Cli, ManArgs, ManFormat};
use crate::env::Env;

/// Run the command.
pub fn run(args: &ManArgs, env: &mut Env) -> Result<ExitCode> {
    let page = match args.format {
        ManFormat::Roff => render_roff()?,
        ManFormat::Json => render_json()?,
        ManFormat::Markdown => render_markdown().into_bytes(),
    };

    match &args.output {
        Some(path) => {
            let path = env.resolve(path);
            std::fs::write(&path, &page)
                .map_err(|e| from_io(e, format!("cannot write {}", path.display())))?;
        }
        None => {
            env.out
                .write_all(&page)
                .map_err(|e| from_io(e, "cannot write to stdout"))?;
        }
    }
    Ok(ExitCode::Success)
}

/// The CLIspec document as a string, which is what the drift test compares.
pub fn spec_document() -> serde_json::Value {
    crate::cmd::spec::render(&Cli::command(), env!("CARGO_PKG_VERSION"))
}

/// The Markdown manual, rendered from the CLIspec document so the two cannot
/// disagree about a flag.
pub fn render_markdown() -> String {
    crate::cmd::spec::markdown(&spec_document())
}

/// The troff man page: the whole surface, top level then one section per
/// subcommand, so `bit-cli.1` documents everything rather than only the root.
pub fn render_roff() -> Result<Vec<u8>> {
    let mut page = Vec::new();
    clap_mangen::Man::new(Cli::command())
        .render(&mut page)
        .map_err(|e| from_io(e, "cannot render the man page"))?;

    for sub in Cli::command().get_subcommands() {
        let mut section = Vec::new();
        clap_mangen::Man::new(sub.clone().name(format!("bit-cli-{}", sub.get_name())))
            .render(&mut section)
            .map_err(|e| from_io(e, "cannot render a subcommand man page"))?;
        page.extend_from_slice(&section);
    }
    Ok(page)
}

/// The CLIspec document, for a reader that is a program.
///
/// Pretty printed and newline terminated because it is committed: a diff of a
/// one line JSON file says nothing about what changed.
pub fn render_json() -> Result<Vec<u8>> {
    let doc = spec_document();
    let mut out = serde_json::to_vec_pretty(&doc).map_err(|e| {
        from_io(
            std::io::Error::other(e),
            "cannot render the CLIspec document",
        )
    })?;
    out.push(b'\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use crate::test_support::run_ok;

    #[test]
    fn the_man_page_renders_and_names_the_tool() {
        let out = run_ok(&["man"], ".");
        assert!(out.contains("bit-cli"), "{out}");
        assert!(out.contains(".TH"), "not roff output");
    }

    #[test]
    fn every_subcommand_gets_a_section() {
        let out = run_ok(&["man"], ".");
        for sub in [
            "bit-cli-download",
            "bit-cli-webseed",
            "bit-cli-create",
            "bit-cli-verify",
        ] {
            assert!(out.contains(sub), "`{sub}` has no man section");
        }
    }

    #[test]
    fn the_page_can_be_written_to_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bit-cli.1");
        run_ok(&["man", "-o", path.to_str().unwrap()], dir.path());
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("bit-cli"));
    }
}
