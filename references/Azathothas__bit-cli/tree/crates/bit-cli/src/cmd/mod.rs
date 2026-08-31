//! One module per subcommand.
//!
//! Every module has the same shape: parse what the flags mean, validate it,
//! execute, and hand the result to the renderer. Adding a subcommand touches
//! one new file here and one line of registration in [`crate::dispatch`].
//!
//! No module writes to a stream directly. They return values and let
//! [`crate::output::Renderer`] decide whether that becomes JSON, NDJSON, or
//! text, which is what keeps the machine and human surfaces from drifting
//! apart.

pub mod bench;
pub mod completions;
pub mod config;
pub mod create;
pub mod download;
pub mod edit;
pub mod files;
pub mod info;
pub mod magnet;
pub mod man;
pub mod peers;
pub mod seed;
pub mod spec;
pub mod trackers;
pub mod tree;
pub mod verify;
pub mod version;
pub mod webseed;
