//! Web seeds: HTTP sources attached to a torrent at runtime.
//!
//! The `.torrent` is never rewritten, never re-hashed, never touched. Sources
//! attach for the length of one invocation and the info hash does not change.
//! That is the whole premise of the tool.
//!
//! A source is a `(source, scope, composition)` triple:
//!
//! - [`binding::SourceSpec`] is the source: a URL plus its headers, auth,
//!   timeouts, concurrency, and rate limit.
//! - [`scope::Scope`] is what part of the torrent it may serve. A mirror
//!   holding only part of the payload is a first-class case.
//! - [`composition::Mode`] is how the request URL is built from the source URL
//!   and the torrent's `name` and `path`.
//!
//! The three are orthogonal. [`binding::BindingSet::resolve`] crosses them
//! against a torrent, reports the exact URL every file resolves to, and names
//! the pieces nothing can serve.

pub mod binding;
pub mod bridge;
pub mod composition;
pub mod fetch;
pub mod ledger;
pub mod local;
pub mod probe;
pub mod scope;
pub mod table;

pub use binding::{Binding, BindingSet, Origin, SourceSpec, Style};
pub use bridge::{BridgeParams, BridgeState, BridgeStatus};
pub use composition::Mode;
pub use ledger::{BlockLedger, Conviction, LedgerStats};
pub use scope::Scope;
