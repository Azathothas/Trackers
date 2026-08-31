//! Reading and writing `.torrent` metainfo.
//!
//! The info hash is the SHA-1 of the `info` dictionary's encoded bytes.
//! Everything about editing a torrent turns on keeping those bytes exactly as
//! they were: `announce`, `announce-list`, `url-list`, `httpseeds`, `comment`,
//! `created by`, `creation date`, and `nodes` all live outside `info` and can
//! change freely, and anything inside `info` produces a different torrent.
//!
//! [`Metainfo`] keeps the original `info` bytes and splices them back in
//! verbatim on write, so an edit cannot change the info hash even if the
//! original encoding was not canonical. [`Metainfo::write_to_vec`] proves it
//! by recomputing the hash from what it just produced.

use std::collections::BTreeMap;

use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::time::Timestamp;
use crate::torrent::bencode::{self, Value};

/// A 20-byte SHA-1 info hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InfoHash(pub [u8; 20]);

impl InfoHash {
    /// The hash of some bytes.
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Sha1::new();
        hasher.update(bytes);
        Self(hasher.finalize().into())
    }

    /// Lower-case hex.
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Parse from 40 hex characters, or from 32 base32 characters as a magnet
    /// URI may carry.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        if text.len() == 40 {
            let mut out = [0u8; 20];
            for (index, pair) in text.as_bytes().chunks(2).enumerate() {
                let hex = std::str::from_utf8(pair).map_err(|_| bad_hash(text))?;
                out[index] = u8::from_str_radix(hex, 16).map_err(|_| bad_hash(text))?;
            }
            return Ok(Self(out));
        }
        if text.len() == 32 {
            return decode_base32(text).map(Self).ok_or_else(|| bad_hash(text));
        }
        Err(bad_hash(text))
    }
}

fn bad_hash(text: &str) -> Error {
    Error::source_resolution(format!(
        "`{text}` is not an info hash (expected 40 hex characters or 32 base32 characters)"
    ))
    .with("value", text.to_string())
}

/// Decode RFC 4648 base32 without padding into 20 bytes.
fn decode_base32(text: &str) -> Option<[u8; 20]> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut bits = 0u32;
    let mut count = 0u32;
    let mut out = Vec::with_capacity(20);
    for c in text.bytes() {
        let upper = c.to_ascii_uppercase();
        let index = ALPHABET.iter().position(|a| *a == upper)? as u32;
        bits = (bits << 5) | index;
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out.try_into().ok()
}

impl std::fmt::Display for InfoHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex())
    }
}

impl serde::Serialize for InfoHash {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

/// One file inside a multi-file torrent's `info` dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoFile {
    /// Path components relative to the torrent root.
    pub path: Vec<String>,
    /// Length in bytes.
    pub length: u64,
    /// The BEP 47 `attr` string, when present. `p` marks a padding file.
    pub attr: Option<String>,
    /// Per-file MD5, when the creator wrote one.
    pub md5sum: Option<String>,
}

impl InfoFile {
    /// Whether this is a BEP 47 padding file, which carries no real data and
    /// is not shown to the user as a file.
    pub fn is_padding(&self) -> bool {
        self.attr.as_deref().is_some_and(|a| a.contains('p'))
    }
}

/// The parsed `info` dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// The torrent name: a directory name for multi-file, a file name for
    /// single-file.
    pub name: String,
    /// Length of a non-final piece.
    pub piece_length: u32,
    /// The SHA-1 of each piece, in order.
    pub pieces: Vec<[u8; 20]>,
    /// Files, always populated. A single-file torrent has exactly one entry
    /// whose path is the torrent name.
    pub files: Vec<InfoFile>,
    /// Whether the metainfo carried a `files` list.
    pub multi_file: bool,
    /// BEP 27 private flag.
    pub private: bool,
    /// The `source` key, used for cross-seeding. It is inside `info`, so
    /// changing it changes the info hash.
    pub source: Option<String>,
    /// Whether a BEP 52 `meta version` key is present, and its value.
    pub meta_version: Option<i64>,
    /// How the names above were decoded. See [`NameEncoding`].
    pub name_encoding: NameEncoding,
}

/// How a torrent's `name` and `path` keys were turned into text.
///
/// BEP 3 does not say what encoding a name is in, and real torrents carry
/// Shift-JIS, CP1251 and worse. Two rules decide it, in this order, and both
/// are the vendored `librqbit`'s so that what this reports and what a run
/// writes to disk cannot disagree. See `TODO/bep-coverage.md`, T-103.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct NameEncoding {
    /// The encoding detected over the raw `name` and `path` bytes, by WHATWG
    /// label: `UTF-8` for a torrent written the ordinary way.
    pub detected: &'static str,
    /// Whether a `.utf-8` key was present, held valid UTF-8, and was preferred
    /// over the raw key beside it.
    pub utf8_keys: bool,
}

impl Default for NameEncoding {
    fn default() -> Self {
        Self {
            detected: "UTF-8",
            utf8_keys: false,
        }
    }
}

impl NameEncoding {
    /// Whether this says anything a reader of an ordinary torrent needs.
    ///
    /// A torrent whose names are UTF-8 and which carries no `.utf-8` key had
    /// nothing decided about it, and a line saying so on every report would be
    /// noise on almost every torrent there is.
    pub fn is_plain(&self) -> bool {
        self.detected == "UTF-8" && !self.utf8_keys
    }

    /// One line for a terminal.
    pub fn describe(&self) -> String {
        match (self.utf8_keys, self.detected) {
            (true, "UTF-8") => "UTF-8, from the `.utf-8` keys".to_string(),
            (true, other) => format!("UTF-8, from the `.utf-8` keys; the raw keys are {other}"),
            (false, other) => format!("{other}, detected"),
        }
    }
}

impl Info {
    /// Total payload length.
    pub fn total_length(&self) -> u64 {
        self.files.iter().map(|f| f.length).sum()
    }

    /// The torrent's shape, for the addressing model.
    pub fn layout(&self) -> Layout {
        Layout::from_lengths(
            self.name.clone(),
            self.multi_file,
            self.piece_length,
            self.files.iter().map(|f| (f.path.join("/"), f.length)),
        )
    }
}

/// A parsed `.torrent`.
#[derive(Debug, Clone)]
pub struct Metainfo {
    /// The whole top-level dictionary, as parsed.
    root: Value,
    /// The exact bytes of the `info` dictionary's value.
    info_bytes: Vec<u8>,
    /// The info hash of those bytes.
    info_hash: InfoHash,
    /// The parsed `info` dictionary.
    info: Info,
    /// What this torrent's own encoding did that a canonical encoder would
    /// not. See `TODO/metainfo.md`, T-172.
    encoding: bencode::Encoding,
}

impl Metainfo {
    /// Parse a `.torrent` from bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let (root, span, encoding) = bencode::decode_torrent(bytes)
            .map_err(|e| Error::source_resolution(format!("not a valid torrent: {e}")))?;
        let span =
            span.ok_or_else(|| Error::source_resolution("torrent has no `info` dictionary"))?;
        let info_bytes = bytes[span].to_vec();
        let info_hash = InfoHash::of(&info_bytes);
        let info = parse_info(&root)?;
        Ok(Self {
            root,
            info_bytes,
            info_hash,
            info,
            encoding,
        })
    }

    /// Read a `.torrent` from a file.
    ///
    /// A torrent that cannot be read is a source resolution failure, not a
    /// disk failure: from the caller's side "the file is not there" and "the
    /// file is not a torrent" are the same problem, and the exit code table
    /// puts an unreadable torrent under code 4.
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| {
            Error::source_resolution(format!("cannot read {}: {e}", path.display()))
                .with("path", path.display().to_string())
                .with("io_kind", format!("{:?}", e.kind()))
        })?;
        Self::parse(&bytes)
            .map_err(|e| Error::source_resolution(format!("{}: {e}", path.display())))
    }

    /// Build a metainfo from an already-encoded `info` dictionary.
    ///
    /// This is the path `bit-cli create` takes: the `info` dictionary is
    /// encoded once, its bytes are hashed, and those same bytes are what get
    /// written. There is no second encoding that could differ.
    pub fn from_info_bytes(info_bytes: Vec<u8>) -> Result<Self> {
        let info_value = bencode::decode(&info_bytes).map_err(|e| {
            Error::generic(format!("the info dictionary is not valid bencode: {e}"))
        })?;
        let mut map = BTreeMap::new();
        map.insert(b"info".to_vec(), info_value);
        let root = Value::Dict(map);
        let info = parse_info(&root)?;
        let info_hash = InfoHash::of(&info_bytes);
        Ok(Self {
            root,
            info_bytes,
            info_hash,
            info,
            // Built here rather than read: `create` encodes the `info`
            // dictionary itself and this module is what canonical means.
            encoding: bencode::Encoding::default(),
        })
    }

    /// The info hash.
    pub fn info_hash(&self) -> InfoHash {
        self.info_hash
    }

    /// The exact bytes the info hash was computed over.
    pub fn info_bytes(&self) -> &[u8] {
        &self.info_bytes
    }

    /// The parsed `info` dictionary.
    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The torrent's shape, for the addressing model.
    pub fn layout(&self) -> Layout {
        self.info.layout()
    }

    /// The top-level dictionary, for fields this type does not name.
    pub fn root(&self) -> &Value {
        &self.root
    }

    /// What this torrent's own encoding did that a canonical encoder would
    /// not: keys out of order, and tolerated bytes after the top-level
    /// dictionary.
    ///
    /// Neither refuses the torrent. Both are worth knowing, because a tool
    /// that re-encodes the `info` dictionary rather than splicing it, as
    /// [`Self::write_to_vec`] does, would produce a different info hash from
    /// the same file. See `TODO/metainfo.md`, T-172.
    pub fn encoding(&self) -> &bencode::Encoding {
        &self.encoding
    }

    /// The primary tracker.
    pub fn announce(&self) -> Option<String> {
        self.root.get("announce").and_then(Value::as_text)
    }

    /// The BEP 12 tracker tiers.
    ///
    /// A torrent with only `announce` reads as one tier holding it, so callers
    /// never have to handle both shapes.
    pub fn announce_tiers(&self) -> Vec<Vec<String>> {
        let tiers: Vec<Vec<String>> = self
            .root
            .get("announce-list")
            .and_then(Value::as_list)
            .map(|tiers| {
                tiers
                    .iter()
                    .map(Value::as_text_list)
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if !tiers.is_empty() {
            return tiers;
        }
        self.announce().map(|a| vec![vec![a]]).unwrap_or_default()
    }

    /// Every tracker, flattened.
    pub fn trackers(&self) -> Vec<String> {
        self.announce_tiers().into_iter().flatten().collect()
    }

    /// BEP 19 `url-list` web seeds.
    ///
    /// The key is a string for a single entry and a list for several, and both
    /// appear in the wild.
    pub fn url_list(&self) -> Vec<String> {
        self.root
            .get("url-list")
            .map(Value::as_text_or_text_list)
            .unwrap_or_default()
    }

    /// BEP 17 `httpseeds`.
    ///
    /// BEP 17 specifies a list and BEP 19 specifies a list, and torrents
    /// carrying a bare string exist for both. The two keys read through the
    /// same accessor so they cannot drift apart again: reading one shape here
    /// and both next door is how a torrent that names an HTTP seed yields
    /// none, silently.
    ///
    /// The two lists stay separate after they are read. Which key a URL came
    /// from is what decides BEP 17 style from BEP 19 style, so merging them
    /// would throw away the only signal that needs no network round trip.
    pub fn http_seeds(&self) -> Vec<String> {
        self.root
            .get("httpseeds")
            .map(Value::as_text_or_text_list)
            .unwrap_or_default()
    }

    /// DHT bootstrap nodes written into the torrent, as `host:port`.
    pub fn nodes(&self) -> Vec<String> {
        self.root
            .get("nodes")
            .and_then(Value::as_list)
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|node| {
                        let pair = node.as_list()?;
                        let host = pair.first()?.as_text()?;
                        let port = pair.get(1)?.as_int()?;
                        Some(format!("{host}:{port}"))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `comment` field.
    ///
    /// `comment.utf-8` wins where a creator wrote both, on the same rule the
    /// names follow. parse-torrent
    /// [issue 177](https://github.com/webtorrent/parse-torrent/issues/177) is
    /// where that key is recorded. See `TODO/bep-coverage.md`, T-103.
    pub fn comment(&self) -> Option<String> {
        utf8_twin(self.root.get("comment.utf-8"))
            .and_then(|twin| twin.into_iter().next())
            .or_else(|| self.root.get("comment").and_then(Value::as_text))
    }

    /// The `created by` field.
    pub fn created_by(&self) -> Option<String> {
        self.root.get("created by").and_then(Value::as_text)
    }

    /// The `creation date` field.
    pub fn creation_date(&self) -> Option<Timestamp> {
        self.root
            .get("creation date")
            .and_then(Value::as_int)
            .map(Timestamp::from_epoch_secs)
    }

    /// The BEP 39 `update-url` feed.
    pub fn update_url(&self) -> Option<String> {
        self.root.get("update-url").and_then(Value::as_text)
    }

    /// Replace a top-level field, or remove it when `value` is `None`.
    ///
    /// Refuses to touch `info`, because that is the one field whose bytes the
    /// info hash depends on.
    pub fn set(&mut self, key: &str, value: Option<Value>) -> Result<()> {
        if key == "info" {
            return Err(Error::would_change_infohash(
                "the `info` dictionary cannot be edited in place: it is what the info hash is computed over",
            ));
        }
        let map = self
            .root
            .as_dict_mut()
            .ok_or_else(|| Error::generic("the torrent's root is not a dictionary"))?;
        match value {
            Some(value) => map.insert(key.as_bytes().to_vec(), value),
            None => map.remove(key.as_bytes()),
        };
        Ok(())
    }

    /// Encode the torrent.
    ///
    /// Every key other than `info` is re-encoded canonically. `info` is
    /// spliced in as the original bytes, so the info hash is preserved exactly.
    /// The result is checked before it is returned.
    pub fn write_to_vec(&self) -> Result<Vec<u8>> {
        let map = self
            .root
            .as_dict()
            .ok_or_else(|| Error::generic("the torrent's root is not a dictionary"))?;
        let mut out = Vec::with_capacity(self.info_bytes.len() + 512);
        out.push(b'd');
        // Keys are emitted in sorted byte order, with `info` taking its place
        // in that order like any other key.
        let info_key = b"info".to_vec();
        for (key, value) in map {
            bencode::encode_into(&Value::Bytes(key.clone()), &mut out);
            if *key == info_key {
                out.extend_from_slice(&self.info_bytes);
            } else {
                bencode::encode_into(value, &mut out);
            }
        }
        out.push(b'e');

        // Prove the splice worked rather than trusting it. Getting this wrong
        // would silently publish a different torrent.
        let (_, span) = bencode::decode_with_info_span(&out)
            .map_err(|e| Error::generic(format!("produced an invalid torrent: {e}")))?;
        let span =
            span.ok_or_else(|| Error::generic("produced a torrent with no info dictionary"))?;
        let written = InfoHash::of(&out[span]);
        if written != self.info_hash {
            return Err(Error::would_change_infohash(format!(
                "writing the torrent would change the info hash from {} to {written}",
                self.info_hash
            ))
            .with("before", self.info_hash.hex())
            .with("after", written.hex()));
        }
        Ok(out)
    }
}

/// The `.utf-8` twin of a name or a path, when there is one and it holds what
/// it says it holds.
///
/// uTorrent writes `name` in the creator's local encoding and `name.utf-8`
/// beside it, and the same per file. Neither key is in BEP 3 and both are
/// universal in practice, so the rule every other reader settled on is the one
/// used here: if the `.utf-8` variant exists, prefer it. A variant that is not
/// valid UTF-8 is a creator that wrote the key without meaning it, and the raw
/// key with the detected encoding is the better answer for that torrent.
fn utf8_twin(value: Option<&Value>) -> Option<Vec<String>> {
    let parts: Vec<&[u8]> = match value? {
        Value::Bytes(bytes) => vec![bytes.as_slice()],
        Value::List(items) => items.iter().map(Value::as_bytes).collect::<Option<_>>()?,
        _ => return None,
    };
    if parts.is_empty() {
        return None;
    }
    parts
        .into_iter()
        .map(|part| std::str::from_utf8(part).ok().map(str::to_string))
        .collect()
}

/// Every raw `name` and `path` byte string in an `info` dictionary, in the
/// order the detector is fed them.
///
/// The `.utf-8` twins are left out on purpose: they are UTF-8 by definition,
/// so feeding them would let a correctly written twin talk the detector out of
/// the encoding the raw keys are actually in.
fn raw_name_bytes(info: &Value) -> Vec<&[u8]> {
    let mut parts: Vec<&[u8]> = Vec::new();
    if let Some(name) = info.get("name").and_then(Value::as_bytes) {
        parts.push(name);
    }
    if let Some(files) = info.get("files").and_then(Value::as_list) {
        for file in files {
            let Some(components) = file.get("path").and_then(Value::as_list) else {
                continue;
            };
            parts.extend(components.iter().filter_map(Value::as_bytes));
        }
    }
    parts
}

/// Decode one raw byte string with an encoding, the way the run does.
fn decode_with(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> String {
    encoding.decode(bytes).0.into_owned()
}

fn parse_info(root: &Value) -> Result<Info> {
    let info = root
        .get("info")
        .ok_or_else(|| Error::source_resolution("torrent has no `info` dictionary"))?;
    let missing = |key: &str| {
        Error::source_resolution(format!("torrent `info` dictionary has no `{key}`"))
            .with("key", key.to_string())
    };

    // The encoding is settled before anything is decoded, because it is
    // detected over the whole dictionary rather than per key, and because the
    // vendored `librqbit` settles it the same way over the same bytes. Two
    // decoders that disagree are what T-103 was filed about.
    let detected = librqbit_core::torrent_metainfo::detect_encoding_of(raw_name_bytes(info));
    let mut utf8_keys = false;

    let name = match utf8_twin(info.get("name.utf-8")) {
        Some(twin) => {
            utf8_keys = true;
            twin.into_iter().next().unwrap_or_default()
        }
        None => info
            .get("name")
            .and_then(Value::as_bytes)
            .map(|bytes| decode_with(detected, bytes))
            .ok_or_else(|| missing("name"))?,
    };
    let piece_length = info
        .get("piece length")
        .and_then(Value::as_int)
        .ok_or_else(|| missing("piece length"))?;
    let piece_length = u32::try_from(piece_length).map_err(|_| {
        Error::source_resolution(format!(
            "piece length {piece_length} does not fit in 32 bits"
        ))
    })?;
    if piece_length == 0 {
        return Err(Error::source_resolution("piece length is zero"));
    }

    let raw_pieces = info
        .get("pieces")
        .and_then(Value::as_bytes)
        .ok_or_else(|| missing("pieces"))?;
    if raw_pieces.len() % 20 != 0 {
        return Err(Error::source_resolution(format!(
            "`pieces` is {} bytes, which is not a multiple of 20",
            raw_pieces.len()
        ))
        .with("pieces_bytes", raw_pieces.len()));
    }
    // The length was checked to be a multiple of 20 just above, so the
    // remainder here is always empty.
    let (chunks, _) = raw_pieces.as_chunks::<20>();
    let pieces: Vec<[u8; 20]> = chunks.to_vec();

    let (files, multi_file) = match info.get("files").and_then(Value::as_list) {
        Some(entries) => {
            let mut files = Vec::with_capacity(entries.len());
            for entry in entries {
                let length = entry
                    .get("length")
                    .and_then(Value::as_int)
                    .ok_or_else(|| Error::source_resolution("a file entry has no `length`"))?;
                let path: Vec<String> = match utf8_twin(entry.get("path.utf-8")) {
                    Some(twin) => {
                        utf8_keys = true;
                        twin
                    }
                    None => entry
                        .get("path")
                        .and_then(Value::as_list)
                        .map(|components| {
                            components
                                .iter()
                                .filter_map(Value::as_bytes)
                                .map(|bytes| decode_with(detected, bytes))
                                .collect::<Vec<String>>()
                        })
                        .filter(|p: &Vec<String>| !p.is_empty())
                        .ok_or_else(|| Error::source_resolution("a file entry has no `path`"))?,
                };
                files.push(InfoFile {
                    path,
                    length: u64::try_from(length).map_err(|_| {
                        Error::source_resolution(format!("file length {length} is negative"))
                    })?,
                    attr: entry.get("attr").and_then(Value::as_text),
                    md5sum: entry.get("md5sum").and_then(Value::as_text),
                });
            }
            (files, true)
        }
        None => {
            let length = info.get("length").and_then(Value::as_int).ok_or_else(|| {
                Error::source_resolution(
                    "torrent has neither `files` nor `length`, so it describes no data",
                )
            })?;
            let file = InfoFile {
                path: vec![name.clone()],
                length: u64::try_from(length).map_err(|_| {
                    Error::source_resolution(format!("length {length} is negative"))
                })?,
                attr: info.get("attr").and_then(Value::as_text),
                md5sum: info.get("md5sum").and_then(Value::as_text),
            };
            (vec![file], false)
        }
    };

    let total: u64 = files.iter().map(|f| f.length).sum();
    let expected_pieces = total.div_ceil(u64::from(piece_length)) as usize;
    if pieces.len() != expected_pieces {
        return Err(Error::source_resolution(format!(
            "torrent declares {} pieces but {total} bytes at {piece_length} bytes per piece needs {expected_pieces}",
            pieces.len()
        ))
        .with("declared_pieces", pieces.len())
        .with("expected_pieces", expected_pieces)
        .with("total_bytes", total));
    }

    Ok(Info {
        name,
        piece_length,
        pieces,
        files,
        multi_file,
        private: info
            .get("private")
            .and_then(Value::as_int)
            .is_some_and(|v| v != 0),
        source: info.get("source").and_then(Value::as_text),
        meta_version: info.get("meta version").and_then(Value::as_int),
        name_encoding: NameEncoding {
            detected: detected.name(),
            utf8_keys,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(pairs: Vec<(&str, Value)>) -> Value {
        Value::Dict(
            pairs
                .into_iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v))
                .collect(),
        )
    }

    /// A single-file torrent: 3000 bytes, 1024-byte pieces, so three pieces.
    fn single_file() -> Vec<u8> {
        bencode::encode(&dict(vec![
            ("announce", Value::text("udp://tracker.example.com:80")),
            ("comment", Value::text("hello")),
            ("creation date", Value::Int(1_787_140_323)),
            (
                "info",
                dict(vec![
                    ("length", Value::Int(3000)),
                    ("name", Value::text("payload.bin")),
                    ("piece length", Value::Int(1024)),
                    ("pieces", Value::Bytes(vec![0u8; 60])),
                ]),
            ),
            (
                "url-list",
                Value::List(vec![Value::text("https://e.com/pub/")]),
            ),
        ]))
    }

    /// A multi-file torrent: 1500 + 500 bytes, 1024-byte pieces.
    fn multi_file() -> Vec<u8> {
        let file = |path: &[&str], length: i64| {
            dict(vec![
                ("length", Value::Int(length)),
                (
                    "path",
                    Value::List(path.iter().map(|p| Value::text(*p)).collect()),
                ),
            ])
        };
        bencode::encode(&dict(vec![
            (
                "announce-list",
                Value::List(vec![
                    Value::List(vec![Value::text("udp://a:80"), Value::text("udp://b:80")]),
                    Value::List(vec![Value::text("udp://c:80")]),
                ]),
            ),
            (
                "info",
                dict(vec![
                    (
                        "files",
                        Value::List(vec![
                            file(&["disc 1", "a.flac"], 1500),
                            file(&["notes.nfo"], 500),
                        ]),
                    ),
                    ("name", Value::text("album")),
                    ("piece length", Value::Int(1024)),
                    ("pieces", Value::Bytes(vec![0u8; 40])),
                    ("private", Value::Int(1)),
                ]),
            ),
        ]))
    }

    #[test]
    fn a_single_file_torrent_parses_into_one_file() {
        let meta = Metainfo::parse(&single_file()).unwrap();
        assert_eq!(meta.info().name, "payload.bin");
        assert_eq!(meta.info().piece_length, 1024);
        assert_eq!(meta.info().pieces.len(), 3);
        assert!(!meta.info().multi_file);
        assert_eq!(meta.info().files.len(), 1);
        assert_eq!(meta.info().files[0].path, ["payload.bin"]);
        assert_eq!(meta.info().total_length(), 3000);
    }

    #[test]
    fn a_multi_file_torrent_parses_every_file_in_order() {
        let meta = Metainfo::parse(&multi_file()).unwrap();
        assert!(meta.info().multi_file);
        assert!(meta.info().private);
        assert_eq!(meta.info().files.len(), 2);
        assert_eq!(meta.info().files[0].path, ["disc 1", "a.flac"]);
        assert_eq!(meta.info().total_length(), 2000);
    }

    #[test]
    fn the_layout_matches_the_metainfo() {
        let layout = Metainfo::parse(&multi_file()).unwrap().layout();
        assert_eq!(layout.name, "album");
        assert!(layout.multi_file);
        assert_eq!(layout.total_length, 2000);
        assert_eq!(layout.piece_count(), 2);
        assert_eq!(layout.file(1).unwrap().offset, 1500);
    }

    #[test]
    fn trackers_read_from_either_key() {
        let single = Metainfo::parse(&single_file()).unwrap();
        assert_eq!(
            single.announce().as_deref(),
            Some("udp://tracker.example.com:80")
        );
        assert_eq!(
            single.announce_tiers(),
            vec![vec!["udp://tracker.example.com:80".to_string()]]
        );

        let multi = Metainfo::parse(&multi_file()).unwrap();
        assert_eq!(multi.announce_tiers().len(), 2);
        assert_eq!(multi.announce_tiers()[0].len(), 2);
        assert_eq!(multi.trackers().len(), 3);
    }

    #[test]
    fn a_url_list_is_read_whether_it_is_a_string_or_a_list() {
        let as_list = Metainfo::parse(&single_file()).unwrap();
        assert_eq!(as_list.url_list(), vec!["https://e.com/pub/".to_string()]);

        let mut torrent = bencode::decode(&single_file()).unwrap();
        torrent.as_dict_mut().unwrap().insert(
            b"url-list".to_vec(),
            Value::text("https://only.example.com/"),
        );
        let as_string = Metainfo::parse(&bencode::encode(&torrent)).unwrap();
        assert_eq!(
            as_string.url_list(),
            vec!["https://only.example.com/".to_string()]
        );
    }

    #[test]
    fn httpseeds_is_read_whether_it_is_a_string_or_a_list() {
        let mut torrent = bencode::decode(&single_file()).unwrap();
        torrent.as_dict_mut().unwrap().insert(
            b"httpseeds".to_vec(),
            Value::List(vec![
                Value::text("https://hoffman-a.example.com/"),
                Value::text("https://hoffman-b.example.com/"),
            ]),
        );
        let as_list = Metainfo::parse(&bencode::encode(&torrent)).unwrap();
        assert_eq!(
            as_list.http_seeds(),
            vec![
                "https://hoffman-a.example.com/".to_string(),
                "https://hoffman-b.example.com/".to_string(),
            ]
        );

        torrent.as_dict_mut().unwrap().insert(
            b"httpseeds".to_vec(),
            Value::text("https://only.hoffman.example.com/"),
        );
        let as_string = Metainfo::parse(&bencode::encode(&torrent)).unwrap();
        assert_eq!(
            as_string.http_seeds(),
            vec!["https://only.hoffman.example.com/".to_string()]
        );
    }

    /// One fixture, both keys, both written as a bare string.
    ///
    /// The defect this pair guards against is one key accepting a shape the
    /// key beside it does not, so the two accessors have to be exercised by
    /// the same torrent or the asymmetry is what the tests preserve. The
    /// lists stay separate: which key a URL came from is what tells BEP 17
    /// style from BEP 19 style.
    #[test]
    fn both_web_seed_keys_read_the_string_shape_and_stay_separate() {
        let mut torrent = bencode::decode(&single_file()).unwrap();
        {
            let root = torrent.as_dict_mut().unwrap();
            root.insert(
                b"url-list".to_vec(),
                Value::text("https://getright.example.com/pub/"),
            );
            root.insert(
                b"httpseeds".to_vec(),
                Value::text("https://hoffman.example.com/"),
            );
        }
        let meta = Metainfo::parse(&bencode::encode(&torrent)).unwrap();
        assert_eq!(
            meta.url_list(),
            vec!["https://getright.example.com/pub/".to_string()]
        );
        assert_eq!(
            meta.http_seeds(),
            vec!["https://hoffman.example.com/".to_string()]
        );
    }

    #[test]
    fn the_info_hash_is_the_sha1_of_the_info_dictionary_bytes() {
        let meta = Metainfo::parse(&single_file()).unwrap();
        assert_eq!(meta.info_hash(), InfoHash::of(meta.info_bytes()));
        assert_eq!(meta.info_hash().hex().len(), 40);
    }

    #[test]
    fn writing_an_unedited_torrent_reproduces_it_byte_for_byte() {
        let original = single_file();
        let meta = Metainfo::parse(&original).unwrap();
        assert_eq!(meta.write_to_vec().unwrap(), original);
    }

    #[test]
    fn editing_fields_outside_info_keeps_the_info_hash() {
        let mut meta = Metainfo::parse(&single_file()).unwrap();
        let before = meta.info_hash();

        meta.set(
            "url-list",
            Some(Value::List(vec![
                Value::text("https://mirror-a.example.com/pub/"),
                Value::text("https://mirror-b.example.com/pub/"),
            ])),
        )
        .unwrap();
        meta.set("comment", Some(Value::text("edited"))).unwrap();
        meta.set("creation date", None).unwrap();
        meta.set(
            "httpseeds",
            Some(Value::List(vec![Value::text("https://old.example.com/")])),
        )
        .unwrap();

        let written = meta.write_to_vec().unwrap();
        let reread = Metainfo::parse(&written).unwrap();
        assert_eq!(
            reread.info_hash(),
            before,
            "the info hash must survive an edit"
        );
        assert_eq!(reread.url_list().len(), 2);
        assert_eq!(reread.comment().as_deref(), Some("edited"));
        assert!(reread.creation_date().is_none());
        assert_eq!(reread.http_seeds().len(), 1);
    }

    #[test]
    fn the_info_dictionary_cannot_be_edited_through_set() {
        let mut meta = Metainfo::parse(&single_file()).unwrap();
        let err = meta.set("info", Some(Value::Int(1))).unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::WouldChangeInfoHash);
    }

    #[test]
    fn a_non_canonical_info_encoding_still_round_trips() {
        // Hand-built with `piece length` before `name`, which is not sorted
        // order. A re-encode would reorder it and change the hash; splicing
        // the original bytes does not.
        let torrent = b"d8:announce3:foo4:infod12:piece lengthi1024e4:name3:bin6:lengthi1024e6:pieces20:00000000000000000000ee";
        let meta = Metainfo::parse(torrent).unwrap();
        let expected = InfoHash::of(
            b"d12:piece lengthi1024e4:name3:bin6:lengthi1024e6:pieces20:00000000000000000000e",
        );
        assert_eq!(meta.info_hash(), expected);
        let written = meta.write_to_vec().unwrap();
        assert_eq!(Metainfo::parse(&written).unwrap().info_hash(), expected);
    }

    #[test]
    fn a_torrent_whose_piece_count_disagrees_with_its_length_is_refused() {
        let bad = bencode::encode(&dict(vec![(
            "info",
            dict(vec![
                ("length", Value::Int(3000)),
                ("name", Value::text("x")),
                ("piece length", Value::Int(1024)),
                // Two pieces where three are needed.
                ("pieces", Value::Bytes(vec![0u8; 40])),
            ]),
        )]));
        let err = Metainfo::parse(&bad).unwrap_err();
        assert!(err.message().contains("needs 3"), "{}", err.message());
        assert_eq!(err.context()["expected_pieces"], 3);
    }

    #[test]
    fn a_pieces_field_that_is_not_a_multiple_of_twenty_is_refused() {
        let bad = bencode::encode(&dict(vec![(
            "info",
            dict(vec![
                ("length", Value::Int(1024)),
                ("name", Value::text("x")),
                ("piece length", Value::Int(1024)),
                ("pieces", Value::Bytes(vec![0u8; 19])),
            ]),
        )]));
        assert!(
            Metainfo::parse(&bad)
                .unwrap_err()
                .message()
                .contains("multiple of 20")
        );
    }

    #[test]
    fn a_torrent_describing_no_data_is_refused() {
        let bad = bencode::encode(&dict(vec![(
            "info",
            dict(vec![
                ("name", Value::text("x")),
                ("piece length", Value::Int(1024)),
                ("pieces", Value::Bytes(Vec::new())),
            ]),
        )]));
        assert!(
            Metainfo::parse(&bad)
                .unwrap_err()
                .message()
                .contains("describes no data")
        );
    }

    #[test]
    fn garbage_is_refused_with_a_source_resolution_code() {
        let err = Metainfo::parse(b"this is not bencode").unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::SourceResolution);
    }

    #[test]
    fn info_hashes_parse_from_hex_and_base32() {
        let hex = "0102030405060708090a0b0c0d0e0f1011121314";
        let hash = InfoHash::parse(hex).unwrap();
        assert_eq!(hash.hex(), hex);
        assert_eq!(hash.0[0], 1);
        assert_eq!(hash.0[19], 0x14);

        // The same hash, base32 encoded.
        let base32 = "AEBAGBAFAYDQQCIKBMGA2DQPCAIREEYU";
        assert_eq!(InfoHash::parse(base32).unwrap(), hash);
        assert_eq!(InfoHash::parse(&base32.to_lowercase()).unwrap(), hash);
    }

    #[test]
    fn a_bad_info_hash_says_what_was_expected() {
        for bad in ["", "abc", "z".repeat(40).as_str(), "1".repeat(39).as_str()] {
            let err = InfoHash::parse(bad).unwrap_err();
            assert_eq!(err.code(), crate::exit::ExitCode::SourceResolution);
        }
    }

    #[test]
    fn padding_files_are_recognised() {
        let padding = InfoFile {
            path: vec![".pad".into()],
            length: 100,
            attr: Some("p".into()),
            md5sum: None,
        };
        assert!(padding.is_padding());
        let real = InfoFile {
            path: vec!["a.bin".into()],
            length: 100,
            attr: None,
            md5sum: None,
        };
        assert!(!real.is_padding());
    }

    #[test]
    fn nodes_render_as_host_and_port() {
        let mut torrent = bencode::decode(&single_file()).unwrap();
        torrent.as_dict_mut().unwrap().insert(
            b"nodes".to_vec(),
            Value::List(vec![Value::List(vec![
                Value::text("dht.example.com"),
                Value::Int(6881),
            ])]),
        );
        let meta = Metainfo::parse(&bencode::encode(&torrent)).unwrap();
        assert_eq!(meta.nodes(), vec!["dht.example.com:6881".to_string()]);
    }

    #[test]
    fn creation_date_reads_as_an_iso_timestamp() {
        let meta = Metainfo::parse(&single_file()).unwrap();
        assert_eq!(
            meta.creation_date().unwrap().iso(),
            "2026-08-19T11:52:03.000Z"
        );
    }

    // ---------------------------------------------------------------------
    // T-103: what a name that is not UTF-8 becomes.
    //
    // Every case below is checked against the vendored `librqbit` as well as
    // against a literal, by `the_two_decoders_in_this_tree_agree`. Two
    // decoders is what the entry was filed about: this parser used
    // `String::from_utf8_lossy` and the session named the same files through
    // `librqbit`'s detected encoding, so `info`, `files` and `webseed list`
    // described a torrent the same run then wrote under different names.
    // ---------------------------------------------------------------------

    /// The cp932 bytes for the text, which is what a Japanese creator's
    /// torrent carries where BEP 3 says nothing about encoding.
    fn cp932(text: &str) -> Vec<u8> {
        const TABLE: &[(&str, &[u8])] = &[
            ("音楽", &[0x89, 0xB9, 0x8A, 0x79]),
            ("曲.bin", &[0x8B, 0xC8, b'.', b'b', b'i', b'n']),
            (
                "フォルダ",
                &[0x83, 0x74, 0x83, 0x48, 0x83, 0x8B, 0x83, 0x5F],
            ),
            (
                "ファイル.bin",
                &[
                    0x83, 0x74, 0x83, 0x40, 0x83, 0x43, 0x83, 0x8B, b'.', b'b', b'i', b'n',
                ],
            ),
            ("あ.bin", &[0x82, 0xA0, b'.', b'b', b'i', b'n']),
            ("い.bin", &[0x82, 0xA2, b'.', b'b', b'i', b'n']),
        ];
        TABLE
            .iter()
            .find(|(key, _)| *key == text)
            .map(|(_, bytes)| bytes.to_vec())
            .unwrap_or_else(|| panic!("no cp932 bytes recorded for {text}"))
    }

    /// A multi-file torrent whose `name` and `path` are raw bytes, with the
    /// `.utf-8` twins written beside them when `twins` says so.
    fn raw_names(name: &[u8], path: &[u8], twins: Option<(&str, &str)>) -> Vec<u8> {
        let mut file = vec![
            ("length", Value::Int(1000)),
            ("path", Value::List(vec![Value::Bytes(path.to_vec())])),
        ];
        let mut info = vec![
            ("name", Value::Bytes(name.to_vec())),
            ("piece length", Value::Int(1024)),
            ("pieces", Value::Bytes(vec![0u8; 20])),
        ];
        if let Some((name_utf8, path_utf8)) = twins {
            info.push(("name.utf-8", Value::text(name_utf8)));
            file.push(("path.utf-8", Value::List(vec![Value::text(path_utf8)])));
        }
        info.push(("files", Value::List(vec![dict(file)])));
        bencode::encode(&dict(vec![("info", dict(info))]))
    }

    #[test]
    fn a_name_that_is_not_utf8_is_decoded_rather_than_replaced() {
        let bytes = raw_names(&cp932("フォルダ"), &cp932("ファイル.bin"), None);
        let meta = Metainfo::parse(&bytes).expect("parses");
        assert_eq!(meta.info().name, "フォルダ");
        assert_eq!(meta.info().files[0].path, vec!["ファイル.bin".to_string()]);
        assert_eq!(meta.info().name_encoding.detected, "Shift_JIS");
        assert!(!meta.info().name_encoding.utf8_keys);
    }

    /// The half of T-103 that is worth the most, and the reason it is a rule
    /// rather than a better detector. `音楽` in cp932 is four bytes that
    /// `chardetng` reads as windows-1252, so detection alone produces `‰¹Šy`.
    /// The creator wrote the answer down in `name.utf-8` and this prefers it.
    #[test]
    fn the_utf8_keys_win_where_detection_alone_is_wrong() {
        let name = cp932("音楽");
        let path = cp932("曲.bin");

        let detected_only = Metainfo::parse(&raw_names(&name, &path, None)).expect("parses");
        assert_eq!(detected_only.info().name_encoding.detected, "windows-1252");
        assert_eq!(detected_only.info().name, "‰¹Šy");

        let with_twins =
            Metainfo::parse(&raw_names(&name, &path, Some(("音楽", "曲.bin")))).expect("parses");
        assert_eq!(with_twins.info().name, "音楽");
        assert_eq!(with_twins.info().files[0].path, vec!["曲.bin".to_string()]);
        assert!(with_twins.info().name_encoding.utf8_keys);
    }

    /// A `.utf-8` key that does not hold UTF-8 is a creator that wrote the key
    /// without meaning it, and trusting it would be worse than the raw key.
    #[test]
    fn a_utf8_key_that_is_not_utf8_is_not_preferred() {
        let name = cp932("フォルダ");
        let path = cp932("ファイル.bin");
        let mut info = vec![
            ("name", Value::Bytes(name.clone())),
            // The same cp932 bytes, under the key that promises UTF-8.
            ("name.utf-8", Value::Bytes(name)),
            ("piece length", Value::Int(1024)),
            ("pieces", Value::Bytes(vec![0u8; 20])),
            (
                "files",
                Value::List(vec![dict(vec![
                    ("length", Value::Int(1000)),
                    ("path", Value::List(vec![Value::Bytes(path.clone())])),
                    ("path.utf-8", Value::List(vec![Value::Bytes(path)])),
                ])]),
            ),
        ];
        info.sort_by_key(|(k, _)| *k);
        let bytes = bencode::encode(&dict(vec![("info", dict(info))]));
        let meta = Metainfo::parse(&bytes).expect("parses");
        assert!(!meta.info().name_encoding.utf8_keys);
        assert_eq!(meta.info().name, "フォルダ");
    }

    /// Two files whose raw bytes differ used to decode to the same string of
    /// replacement characters, so `files` reported one path twice and nothing
    /// downstream could tell them apart.
    #[test]
    fn two_paths_that_differ_in_the_raw_bytes_stay_two_paths() {
        let bytes = bencode::encode(&dict(vec![(
            "info",
            dict(vec![
                (
                    "files",
                    Value::List(vec![
                        dict(vec![
                            ("length", Value::Int(1000)),
                            ("path", Value::List(vec![Value::Bytes(cp932("あ.bin"))])),
                        ]),
                        dict(vec![
                            ("length", Value::Int(1000)),
                            ("path", Value::List(vec![Value::Bytes(cp932("い.bin"))])),
                        ]),
                    ]),
                ),
                ("name", Value::text("collide")),
                ("piece length", Value::Int(1024)),
                ("pieces", Value::Bytes(vec![0u8; 40])),
            ]),
        )]));
        let meta = Metainfo::parse(&bytes).expect("parses");
        let paths: Vec<&String> = meta.info().files.iter().map(|f| &f.path[0]).collect();
        assert_ne!(paths[0], paths[1], "two files decoded to one path");
    }

    /// The ordinary torrent says nothing, because a line on every report about
    /// an encoding nobody chose is noise.
    #[test]
    fn a_plain_utf8_torrent_reports_no_name_encoding() {
        let meta = Metainfo::parse(&multi_file()).expect("parses");
        assert!(meta.info().name_encoding.is_plain());
    }

    #[test]
    fn comment_utf8_wins_over_comment() {
        let bytes = bencode::encode(&dict(vec![
            ("comment", Value::Bytes(vec![0xC0, 0xEE])),
            ("comment.utf-8", Value::text("сообщение")),
            (
                "info",
                dict(vec![
                    ("length", Value::Int(1000)),
                    ("name", Value::text("payload.bin")),
                    ("piece length", Value::Int(1024)),
                    ("pieces", Value::Bytes(vec![0u8; 20])),
                ]),
            ),
        ]));
        let meta = Metainfo::parse(&bytes).expect("parses");
        assert_eq!(meta.comment().as_deref(), Some("сообщение"));
    }

    /// The test the entry exists for: this parser and the one the session
    /// downloads through must name the same file the same way, on the same
    /// bytes. Anything else and `files` describes a torrent the run does not
    /// write. It is asserted over every shape above rather than one, because
    /// the two implementations are separate and only a comparison keeps them
    /// together.
    #[test]
    fn the_two_decoders_in_this_tree_agree() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("plain", multi_file()),
            (
                "shift-jis",
                raw_names(&cp932("フォルダ"), &cp932("ファイル.bin"), None),
            ),
            (
                "detection is wrong",
                raw_names(&cp932("音楽"), &cp932("曲.bin"), None),
            ),
            (
                "the utf-8 keys",
                raw_names(&cp932("音楽"), &cp932("曲.bin"), Some(("音楽", "曲.bin"))),
            ),
        ];
        for (label, bytes) in cases {
            let ours = Metainfo::parse(&bytes).expect("parses here");
            let theirs = librqbit_core::torrent_metainfo::torrent_from_bytes(&bytes)
                .expect("parses there")
                .info
                .data
                .validate()
                .expect("validates");

            let their_paths: Vec<Vec<String>> = theirs
                .iter_file_details()
                .map(|file| file.filename.to_vec())
                .collect();
            let our_paths: Vec<Vec<String>> =
                ours.info().files.iter().map(|f| f.path.clone()).collect();
            assert_eq!(our_paths, their_paths, "{label}: file paths disagree");

            if ours.info().multi_file {
                assert_eq!(
                    Some(ours.info().name.as_str()),
                    theirs.name().as_deref(),
                    "{label}: torrent names disagree"
                );
            }
        }
    }
}
