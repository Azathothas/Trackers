//! `bit-cli completions`: generate shell completions.

use bit_cli_core::ExitCode;
use bit_cli_core::error::{Result, from_io};
use clap::CommandFactory;
use clap_complete::Shell as ClapShell;

use crate::cli::{Cli, CompletionsArgs, Shell};
use crate::env::Env;

/// Run the command.
///
/// Completions are data, so they go to stdout and can be redirected straight
/// into the shell's completion directory.
pub fn run(args: &CompletionsArgs, env: &mut Env) -> Result<ExitCode> {
    let mut command = Cli::command();
    let name = command.get_name().to_string();
    match args.shell {
        Shell::Bash => generate(ClapShell::Bash, &mut command, &name, env),
        Shell::Zsh => generate(ClapShell::Zsh, &mut command, &name, env),
        Shell::Fish => generate(ClapShell::Fish, &mut command, &name, env),
        Shell::Powershell => generate(ClapShell::PowerShell, &mut command, &name, env),
        Shell::Elvish => generate(ClapShell::Elvish, &mut command, &name, env),
        Shell::Nushell => {
            clap_complete::generate(
                clap_complete_nushell::Nushell,
                &mut command,
                &name,
                &mut env.out,
            );
            Ok(())
        }
    }
    .map_err(|e| from_io(e, "cannot write completions"))?;
    Ok(ExitCode::Success)
}

fn generate(
    shell: ClapShell,
    command: &mut clap::Command,
    name: &str,
    env: &mut Env,
) -> std::io::Result<()> {
    clap_complete::generate(shell, command, name, &mut env.out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::test_support::run_ok;

    #[test]
    fn every_shell_produces_something() {
        for shell in ["bash", "zsh", "fish", "powershell", "elvish", "nushell"] {
            let out = run_ok(&["completions", shell], ".");
            assert!(!out.trim().is_empty(), "{shell} produced nothing");
            assert!(
                out.contains("bit-cli"),
                "{shell} output does not mention the binary"
            );
        }
    }

    #[test]
    fn completions_mention_the_subcommands() {
        let out = run_ok(&["completions", "bash"], ".");
        for sub in ["download", "webseed", "create", "verify"] {
            assert!(out.contains(sub), "bash completions do not mention `{sub}`");
        }
    }

    #[test]
    fn an_unknown_shell_is_a_usage_error() {
        let (mut env, _) = crate::env::Env::test(&["completions", "tcsh"], ".");
        assert_eq!(crate::run(&mut env), bit_cli_core::ExitCode::Usage);
    }
}
