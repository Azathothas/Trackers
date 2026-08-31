//! The committed manuals have to describe the binary that is built now.
//!
//! `man/bit-cli.1` and `man/bit-cli.json` are generated from the clap
//! definition, and both are committed so that a reader, human or agent, can
//! open them without building anything. A committed generated file is only
//! worth having if something fails when it goes stale.
//!
//! **This is the gate rather than `scripts/check-man.ps1`**, and the difference
//! matters. The script compares against `target/release/bit-cli`, which can be
//! older than the source in front of it; this renders from the crate being
//! compiled, so it cannot compare against a stale binary, and it runs on every
//! platform CI builds rather than only where pwsh is. The script is how the
//! files are regenerated: `pwsh -NoProfile -File scripts/check-man.ps1 -Fix`.

use std::path::PathBuf;

/// The repository root, two levels up from this crate.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/bit-cli is two levels below the repository root")
        .to_path_buf()
}

fn committed(name: &str) -> String {
    let path = repo_root().join("man").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}. Generate it with: pwsh -NoProfile -File scripts/check-man.ps1 -Fix",
            path.display()
        )
    })
}

/// Compare, and say where rather than dumping two files into the failure.
fn assert_same(name: &str, generated: &str) {
    let committed = committed(name);
    if committed == generated {
        return;
    }

    let mut line = 0usize;
    let mut detail = String::new();
    for (a, b) in committed.lines().zip(generated.lines()) {
        line += 1;
        if a != b {
            detail = format!("\n  line {line}\n  committed: {a}\n  generated: {b}");
            break;
        }
    }
    if detail.is_empty() {
        detail = format!(
            "\n  committed has {} lines, generated has {}",
            committed.lines().count(),
            generated.lines().count()
        );
    }

    panic!(
        "man/{name} no longer describes the binary.{detail}\n\n\
         Regenerate it: pwsh -NoProfile -File scripts/check-man.ps1 -Fix"
    );
}

#[test]
fn the_committed_clispec_document_is_current() {
    let generated = bit_cli::man_json();
    assert_same("bit-cli.json", &generated);
}

#[test]
fn the_committed_man_page_is_current() {
    let generated = bit_cli::man_roff();
    assert_same("bit-cli.1", &generated);
}

#[test]
fn the_committed_markdown_manual_is_current() {
    let generated = bit_cli::man_markdown();
    assert_same("bit-cli.md", &generated);
}

#[test]
fn the_clispec_document_carries_the_crate_version() {
    // The version is in the generated file, so a release that moves the
    // version and forgets to regenerate fails here rather than shipping a
    // document that names the previous one.
    let generated = bit_cli::man_json();
    let expected = format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(
        generated.contains(&expected),
        "the generated document does not carry {expected}"
    );
    assert!(
        committed("bit-cli.json").contains(&expected),
        "man/bit-cli.json names a different version than the crate"
    );
}
