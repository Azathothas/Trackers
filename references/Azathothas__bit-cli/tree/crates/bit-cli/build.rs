//! Records the target triple so `bit-cli version` can report what this binary
//! was built for. `TARGET` is only set for build scripts, not for the crate
//! itself, so it has to be forwarded.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=BIT_CLI_TARGET={target}");
}
