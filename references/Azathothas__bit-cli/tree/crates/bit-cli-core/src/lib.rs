//! Core of `bit-cli`: the engine, the web seed addressing model, metainfo
//! handling, and everything the binary renders.
//!
//! The library is usable without the binary and does not depend on `clap`.
//! Nothing here reads a global, a terminal, or an environment variable on its
//! own; configuration is passed in explicitly, which is what makes the whole
//! surface drivable from a test.

pub mod alloc;
pub mod bench;
pub mod browser;
pub mod config;
pub mod digest;
pub mod engine;
pub mod equivalence;
pub mod error;
pub mod exit;
pub mod fast_set;
pub mod fetch;
pub mod layout;
pub mod listener;
pub mod metalink;
pub mod mse;
pub mod page;
pub mod paths;
pub mod peer_id;
pub mod piece_order;
pub mod render;
pub mod resume;
pub mod span;
pub mod storage;
pub mod sysinfo;
pub mod time;
pub mod torrent;
pub mod tracker;
pub mod units;
pub mod webseed;

pub use error::{Error, Result};
pub use exit::ExitCode;
pub use layout::Layout;

/// The version of this build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
