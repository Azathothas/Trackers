//! The minimum supported Rust version is stated in three places. They agree.
//!
//! `Cargo.toml` is what `cargo` enforces, `.github/workflows/ci.yml` is what
//! actually gets compiled with that toolchain, and `README.md` is what a
//! reader believes. Nothing tied them together, so the `MSRV` job pinned
//! 1.85.1 for as long as it took a dependency to need 1.88, and then failed
//! every push until somebody read the log. See `TODO/cli-surface.md`, T-144.
//!
//! This is a text check on purpose. Parsing the workflow needs a YAML
//! dependency for one line, and the line is a literal.

use std::path::{Path, PathBuf};

/// The repository root. The crate directory is the working directory for its
/// own tests, the same way `schema_gen` finds `docs/schema.md`.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = repo().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// The one value the other two are checked against.
fn declared() -> String {
    let manifest = read("Cargo.toml");
    let line = manifest
        .lines()
        .find(|line| line.trim_start().starts_with("rust-version"))
        .expect("Cargo.toml declares a rust-version");
    line.split('"')
        .nth(1)
        .expect("rust-version is a quoted string")
        .to_string()
}

#[test]
fn the_msrv_job_pins_the_version_the_manifest_declares() {
    let version = declared();
    let workflow = read(".github/workflows/ci.yml");
    let pin = format!("toolchain: \"{version}\"");
    assert!(
        workflow.contains(&pin),
        "Cargo.toml declares rust-version {version}, and .github/workflows/ci.yml \
         does not contain `{pin}`. A job that compiles with a different toolchain \
         than the manifest claims proves nothing about the claim."
    );
}

#[test]
fn the_readme_names_the_version_the_manifest_declares() {
    let version = declared();
    let readme = read("README.md");
    let sentence = format!("The minimum supported Rust version is **{version}**.");
    assert!(
        readme.contains(&sentence),
        "Cargo.toml declares rust-version {version}, and README.md does not say \
         `{sentence}`. Somebody packaging this reads the README, not the manifest."
    );
}

/// The version is a bare `major.minor`, which is what `cargo` compares against.
///
/// A patch level here would be a claim nobody checks: `cargo` ignores it for
/// the compatibility test, and `dtolnay/rust-toolchain` would install exactly
/// that patch, so the two would stop meaning the same thing.
#[test]
fn the_declared_version_has_no_patch_level() {
    let version = declared();
    assert_eq!(
        version.split('.').count(),
        2,
        "rust-version is `{version}`; it should be major.minor"
    );
    assert!(
        version.split('.').all(|part| part.parse::<u32>().is_ok()),
        "rust-version is `{version}`; both parts should be numbers"
    );
}
