//! Torrent metainfo: reading, creating, editing, and verifying `.torrent`
//! files.
//!
//! The info hash is the SHA-1 of the `info` dictionary's encoded bytes, so the
//! bencode codec in [`bencode`] emits canonical form and [`metainfo::Metainfo`]
//! keeps the original `info` bytes verbatim. Everything else in this module is
//! built on those two facts.

pub mod bencode;
pub mod create;
pub mod lint;
pub mod magnet;
pub mod metainfo;
pub mod piece_length;

pub use create::{CreateOptions, Created, InputFile};
pub use lint::Lint;
pub use magnet::Magnet;
pub use metainfo::{Info, InfoFile, InfoHash, Metainfo, NameEncoding};
