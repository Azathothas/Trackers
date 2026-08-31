//! Bencode, encoded canonically.
//!
//! The info hash is the SHA-1 of the `info` dictionary's encoded bytes, so
//! every byte of that encoding matters. Two properties are load-bearing:
//!
//! - **Dictionary keys are sorted by raw byte value on output.** BEP 3
//!   requires it, and it is what makes `bit-cli create` byte-reproducible
//!   across platforms. A [`BTreeMap`] gives it for free, which is why this
//!   module exists rather than reusing a `HashMap`-backed decoder.
//! - **The `info` dictionary's raw bytes are kept.** `bit-cli edit` rewrites
//!   fields outside `info` and re-emits the original `info` bytes verbatim, so
//!   the info hash cannot drift even if this encoder and whatever produced the
//!   torrent disagree about canonical form.
//!
//! Decoding is strict about the things that would let two different encodings
//! mean the same thing: no leading zeros, no `i-0e`, no duplicate keys.
//!
//! It is deliberately **tolerant** about two things real torrents get wrong,
//! and records both rather than accepting them silently. Keys that arrive out
//! of order are read, because the `info` bytes are kept verbatim and never
//! re-encoded, so a torrent that violates BEP 3's sort order still hashes to
//! what every other client says it does. Trailing whitespace and NUL after the
//! top-level dictionary are read for the same reason: they are outside `info`
//! by definition and cannot move the hash. Anything else after the top-level
//! dictionary is refused. See `TODO/metainfo.md`, T-172.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

/// A bencode value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    /// A byte string. Bencode has no notion of text encoding.
    Bytes(Vec<u8>),
    /// An integer.
    Int(i64),
    /// A list.
    List(Vec<Value>),
    /// A dictionary, sorted by raw key bytes.
    Dict(BTreeMap<Vec<u8>, Value>),
}

/// Why decoding failed. Every variant names the byte offset, because a
/// truncated torrent is otherwise very hard to diagnose.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("unexpected end of input at byte {0}")]
    Eof(usize),
    #[error("unexpected byte {byte:?} at byte {at}, expected a bencode value")]
    Unexpected { at: usize, byte: char },
    #[error("integer at byte {0} is malformed")]
    BadInteger(usize),
    #[error("integer at byte {0} has a leading zero or is `-0`")]
    NonCanonicalInteger(usize),
    #[error("byte string length at byte {0} is malformed")]
    BadLength(usize),
    #[error("byte string at byte {at} claims {claimed} bytes but only {available} remain")]
    LengthOverrun {
        at: usize,
        claimed: u64,
        available: usize,
    },
    #[error("dictionary key at byte {0} is not a byte string")]
    NonStringKey(usize),
    #[error("dictionary at byte {0} has a duplicate key")]
    DuplicateKey(usize),
    #[error(
        "value at byte {at} is nested more than {limit} deep, which no torrent is;          a document this deep is built to exhaust the stack rather than to be read"
    )]
    TooDeep { at: usize, limit: u32 },
    #[error(
        "{trailing} unexpected bytes after the value at byte {at};          `bit-cli` accepts only whitespace and NUL after the top-level dictionary"
    )]
    TrailingData { at: usize, trailing: usize },
}

/// How deeply one bencode value may nest inside another.
///
/// A real torrent reaches about six: root, `info`, `files`, one file, `path`,
/// a string. `announce-list` reaches three. A hundred is far above anything
/// BEP 3 or its extensions describe and far below what the stack can take.
///
/// The bound exists because [`Parser::value`] recurses and nothing else stops
/// it. A document of ten thousand `l` bytes is twenty kilobytes on disk and
/// **overflows the stack**, which on every platform this runs on kills the
/// process outright: it is not a panic and `catch_unwind` does not see it. A
/// `.torrent` fetched from a URL and a tracker's response are both untrusted
/// input, so that is a denial of service in twenty kilobytes. Measured before
/// the bound existed: 1,000 deep parsed fine, 10,000 deep took the test
/// process down with `STATUS_STACK_OVERFLOW`.
///
/// `rustorrent/docs/DEEP_AUDIT_REPORT_2026-07-13.md` lists excessive depth in
/// its adversarial set, which is where this was looked for. See
/// `TODO/metainfo.md`, T-172.
const MAX_DEPTH: u32 = 100;

/// Bytes a torrent may carry after its top-level dictionary and still be read.
///
/// mkbrr's `torrent/update.go:210` `decodeTorrentRoot` is where this list comes
/// from: it accepts `ErrUnusedTrailingBytes` when the remainder is only space,
/// tab, carriage return, newline or NUL. Those are what a tool that padded or
/// line-ended a file leaves behind, and they are outside `info` by definition,
/// so they cannot move the info hash. See `TODO/metainfo.md`, T-172.
/// Written as escapes. The same five bytes spelled literally put a raw NUL
/// and a raw newline inside the literal, which made this file binary to
/// `grep` and split the constant across two lines with nothing to read.
const TOLERATED_TRAILING: &[u8] = b" \t\r\n\0";

/// What a torrent's own encoding did that a canonical encoder would not.
///
/// Recorded rather than refused, and reported rather than dropped. A caller
/// who round-trips a non-canonical torrent through a tool that **does**
/// re-encode gets a different info hash, and the only way to know that is
/// ahead of time.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Encoding {
    /// Byte offset of each dictionary whose keys were not in sorted order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unsorted_dicts: Vec<usize>,
    /// Whether any of them was the `info` dictionary or inside it.
    ///
    /// This is the one that matters: keys out of order anywhere else cost
    /// nothing, because `bit-cli` re-encodes everything outside `info`
    /// canonically on the way out anyway.
    pub unsorted_inside_info: bool,
    /// Bytes after the top-level dictionary that were tolerated.
    #[serde(skip_serializing_if = "is_zero_usize")]
    pub trailing_bytes: usize,
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

impl Encoding {
    /// Whether the torrent was encoded the way this module would encode it.
    pub fn is_canonical(&self) -> bool {
        self.unsorted_dicts.is_empty() && self.trailing_bytes == 0
    }

    /// One line per deviation, for a report or a warning.
    pub fn notes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.unsorted_dicts.is_empty() {
            out.push(format!(
                "{} dictionar{} have keys that are not in the sorted order BEP 3 requires{}",
                self.unsorted_dicts.len(),
                match self.unsorted_dicts.len() {
                    1 => "y",
                    _ => "ies",
                },
                match self.unsorted_inside_info {
                    true => ", including inside `info`",
                    false => "",
                }
            ));
        }
        if self.trailing_bytes > 0 {
            out.push(format!(
                "{} bytes of whitespace or NUL follow the top-level dictionary",
                self.trailing_bytes
            ));
        }
        out
    }
}

/// Decode one value, requiring it to consume the whole input.
pub fn decode(input: &[u8]) -> Result<Value, Error> {
    let (value, rest) = decode_prefix(input)?;
    if rest != input.len() {
        return Err(Error::TrailingData {
            at: rest,
            trailing: input.len() - rest,
        });
    }
    Ok(value)
}

/// Decode one value, returning it and the offset just past it.
pub fn decode_prefix(input: &[u8]) -> Result<(Value, usize), Error> {
    let mut parser = Parser {
        input,
        pos: 0,
        info_span: None,
        in_info: 0,
        depth: 0,
        encoding: Encoding::default(),
    };
    let value = parser.value()?;
    Ok((value, parser.pos))
}

/// Decode a torrent, also returning the byte span of the top-level `info`
/// dictionary's value.
///
/// The span is what the info hash is computed over. Recomputing it by
/// re-encoding the parsed `info` would be wrong for any torrent whose original
/// encoding was not canonical, and such torrents exist in the wild.
pub fn decode_with_info_span(input: &[u8]) -> Result<(Value, Option<Range<usize>>), Error> {
    decode_torrent(input).map(|(value, span, _)| (value, span))
}

/// [`decode_with_info_span`] plus what the encoding did that a canonical
/// encoder would not.
///
/// The deviations are returned rather than refused. See [`Encoding`] and
/// `TODO/metainfo.md`, T-172.
pub fn decode_torrent(input: &[u8]) -> Result<(Value, Option<Range<usize>>, Encoding), Error> {
    let mut parser = Parser {
        input,
        pos: 0,
        info_span: None,
        in_info: 0,
        depth: 0,
        encoding: Encoding::default(),
    };
    let value = parser.value()?;
    let trailing = &input[parser.pos..];
    if !trailing.is_empty() {
        if !trailing.iter().all(|b| TOLERATED_TRAILING.contains(b)) {
            return Err(Error::TrailingData {
                at: parser.pos,
                trailing: trailing.len(),
            });
        }
        parser.encoding.trailing_bytes = trailing.len();
    }
    Ok((value, parser.info_span, parser.encoding))
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    /// Byte span of the value under the top-level `info` key, recorded when
    /// the outermost dictionary is parsed.
    info_span: Option<Range<usize>>,
    /// How many `info` values are open above the current position.
    ///
    /// A counter rather than a flag because `info` may hold a dictionary that
    /// holds another, and a flag cleared by the inner one would say the outer
    /// keys are outside `info` when they are not.
    in_info: u32,
    /// How many values are open above the current position, so recursion is
    /// bounded rather than trusted. See [`MAX_DEPTH`].
    depth: u32,
    encoding: Encoding,
}

impl Parser<'_> {
    fn peek(&self) -> Result<u8, Error> {
        self.input
            .get(self.pos)
            .copied()
            .ok_or(Error::Eof(self.pos))
    }

    fn value(&mut self) -> Result<Value, Error> {
        // Counted here rather than in `list` and `dict` separately, because
        // this is the one place every nested value goes through and a bound
        // that two call sites have to remember is a bound that one of them
        // will forget.
        if self.depth >= MAX_DEPTH {
            return Err(Error::TooDeep {
                at: self.pos,
                limit: MAX_DEPTH,
            });
        }
        self.depth += 1;
        let value = match self.peek() {
            Ok(b'i') => self.integer(),
            Ok(b'l') => self.list(),
            Ok(b'd') => self.dict(),
            Ok(b'0'..=b'9') => self.bytes().map(Value::Bytes),
            Ok(byte) => Err(Error::Unexpected {
                at: self.pos,
                byte: byte as char,
            }),
            Err(e) => Err(e),
        };
        self.depth -= 1;
        value
    }

    fn integer(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        self.pos += 1;
        let end = self.find(b'e').ok_or(Error::Eof(start))?;
        let digits = &self.input[self.pos..end];
        let text = std::str::from_utf8(digits).map_err(|_| Error::BadInteger(start))?;
        let value: i64 = text.parse().map_err(|_| Error::BadInteger(start))?;
        // `i03e` and `i-0e` are refused, and the reason written here until
        // 2026-08-23 was that they would make the info hash ambiguous. They
        // cannot: [`decode_torrent`] records the byte span of `info` and
        // `Metainfo` hashes **those bytes**, so the hash is taken over what
        // was read rather than over anything re-encoded. A leading zero
        // inside `info` moves nothing, exactly as an unsorted key moves
        // nothing, which is what T-172 established.
        //
        // The rule stays, for two reasons that are about evidence rather than
        // correctness, and `TODO/metainfo.md` T-187 is where they are argued.
        // No torrent in the corpus carries one, and relaxing a rule with no
        // instance behind it grows tolerance nobody needed and gives a hostile
        // file one more shape to take. And unlike key order, which a
        // `BTreeMap` discards for free, an integer's byte form would have to
        // be recorded per value to be reportable at all, and a report saying
        // "some integer somewhere had a leading zero" is not worth the field.
        let canonical = value.to_string();
        if canonical != text {
            return Err(Error::NonCanonicalInteger(start));
        }
        self.pos = end + 1;
        Ok(Value::Int(value))
    }

    fn bytes(&mut self) -> Result<Vec<u8>, Error> {
        let start = self.pos;
        let colon = self.find(b':').ok_or(Error::Eof(start))?;
        let digits = &self.input[start..colon];
        let text = std::str::from_utf8(digits).map_err(|_| Error::BadLength(start))?;
        let length: u64 = text.parse().map_err(|_| Error::BadLength(start))?;
        if length.to_string() != text {
            return Err(Error::BadLength(start));
        }
        let from = colon + 1;
        let available = self.input.len() - from;
        let wanted = usize::try_from(length).map_err(|_| Error::LengthOverrun {
            at: start,
            claimed: length,
            available,
        })?;
        if wanted > available {
            return Err(Error::LengthOverrun {
                at: start,
                claimed: length,
                available,
            });
        }
        self.pos = from + wanted;
        Ok(self.input[from..self.pos].to_vec())
    }

    fn list(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            match self.input.get(self.pos) {
                None => return Err(Error::Eof(start)),
                Some(b'e') => {
                    self.pos += 1;
                    return Ok(Value::List(items));
                }
                Some(_) => items.push(self.value()?),
            }
        }
    }

    fn dict(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        let outermost = start == 0;
        self.pos += 1;
        let mut map = BTreeMap::new();
        let mut previous: Option<Vec<u8>> = None;
        let mut sorted = true;
        loop {
            match self.input.get(self.pos) {
                None => return Err(Error::Eof(start)),
                Some(b'e') => {
                    self.pos += 1;
                    return Ok(Value::Dict(map));
                }
                Some(b'0'..=b'9') => {
                    let key_at = self.pos;
                    let key = self.bytes()?;
                    // BEP 3 requires sorted keys and real torrents violate it.
                    // Recorded once per dictionary rather than refused: the
                    // `info` bytes are kept verbatim and never re-encoded, so
                    // the order they arrived in cannot move the info hash, and
                    // refusing would turn a torrent every other client opens
                    // into a corrupt file. See `TODO/metainfo.md`, T-172.
                    if sorted
                        && previous
                            .as_ref()
                            .is_some_and(|prev| key.as_slice() < prev.as_slice())
                    {
                        sorted = false;
                        self.encoding.unsorted_dicts.push(start);
                        // A dictionary inside `info` counts as inside it, and
                        // so does `info` itself: the span this records is the
                        // value, and `in_info` is raised before it is parsed.
                        if self.in_info > 0 {
                            self.encoding.unsorted_inside_info = true;
                        }
                    }
                    previous = Some(key.clone());

                    let inside_info = outermost && key == b"info";
                    if inside_info {
                        self.in_info += 1;
                    }
                    let value_start = self.pos;
                    let value = self.value();
                    if inside_info {
                        self.in_info -= 1;
                    }
                    let value = value?;
                    if inside_info {
                        self.info_span = Some(value_start..self.pos);
                    }
                    if map.insert(key, value).is_some() {
                        return Err(Error::DuplicateKey(key_at));
                    }
                }
                Some(_) => return Err(Error::NonStringKey(self.pos)),
            }
        }
    }

    fn find(&self, needle: u8) -> Option<usize> {
        self.input[self.pos..]
            .iter()
            .position(|b| *b == needle)
            .map(|i| i + self.pos)
    }
}

/// Encode a value canonically.
pub fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(value, &mut out);
    out
}

/// Encode a value canonically, appending to `out`.
pub fn encode_into(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Bytes(bytes) => {
            out.extend_from_slice(bytes.len().to_string().as_bytes());
            out.push(b':');
            out.extend_from_slice(bytes);
        }
        Value::Int(n) => {
            out.push(b'i');
            out.extend_from_slice(n.to_string().as_bytes());
            out.push(b'e');
        }
        Value::List(items) => {
            out.push(b'l');
            for item in items {
                encode_into(item, out);
            }
            out.push(b'e');
        }
        Value::Dict(map) => {
            out.push(b'd');
            // BTreeMap iterates in sorted key order, which is exactly what
            // BEP 3 requires.
            for (key, item) in map {
                encode_into(&Value::Bytes(key.clone()), out);
                encode_into(item, out);
            }
            out.push(b'e');
        }
    }
}

impl Value {
    /// A byte string from anything string-like.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Bytes(value.into().into_bytes())
    }

    /// The dictionary entry at `key`, when this is a dictionary.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Dict(map) => map.get(key.as_bytes()),
            _ => None,
        }
    }

    /// This value as a dictionary.
    pub fn as_dict(&self) -> Option<&BTreeMap<Vec<u8>, Value>> {
        match self {
            Self::Dict(map) => Some(map),
            _ => None,
        }
    }

    /// This value as a mutable dictionary.
    pub fn as_dict_mut(&mut self) -> Option<&mut BTreeMap<Vec<u8>, Value>> {
        match self {
            Self::Dict(map) => Some(map),
            _ => None,
        }
    }

    /// This value as a list.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    /// This value as raw bytes.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// This value as an integer.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// This value as UTF-8 text, lossily.
    ///
    /// Torrent metadata is byte strings, and plenty of real torrents carry
    /// names that are not valid UTF-8. Refusing to display them would be worse
    /// than showing a replacement character, so this never fails.
    pub fn as_text(&self) -> Option<String> {
        self.as_bytes()
            .map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// A list of byte strings as text, skipping anything that is not a string.
    pub fn as_text_list(&self) -> Vec<String> {
        self.as_list()
            .map(|items| items.iter().filter_map(Value::as_text).collect())
            .unwrap_or_default()
    }

    /// One byte string, or a list of them, as text.
    ///
    /// Several metainfo keys are specified as a list and are written in the
    /// wild as a bare string when there is only one entry. A reader that
    /// accepts the list alone returns nothing for those, with no error, so
    /// every key with that history reads through here rather than through
    /// [`Value::as_text_list`] directly.
    pub fn as_text_or_text_list(&self) -> Vec<String> {
        match self {
            Self::Bytes(_) => self.as_text().into_iter().collect(),
            _ => self.as_text_list(),
        }
    }
}

impl fmt::Display for Value {
    /// A compact, readable rendering for diagnostics. Not bencode.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) if text.chars().all(|c| !c.is_control()) => write!(f, "{text:?}"),
                _ => write!(f, "<{} bytes>", bytes.len()),
            },
            Self::Int(n) => write!(f, "{n}"),
            Self::List(items) => {
                let rendered: Vec<String> = items.iter().map(ToString::to_string).collect();
                write!(f, "[{}]", rendered.join(", "))
            }
            Self::Dict(map) => {
                let rendered: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{}: {v}", String::from_utf8_lossy(k)))
                    .collect();
                write!(f, "{{{}}}", rendered.join(", "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(pairs: &[(&str, Value)]) -> Value {
        Value::Dict(
            pairs
                .iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn integers_round_trip() {
        for (encoded, value) in [("i0e", 0i64), ("i42e", 42), ("i-7e", -7)] {
            assert_eq!(decode(encoded.as_bytes()).unwrap(), Value::Int(value));
            assert_eq!(encode(&Value::Int(value)), encoded.as_bytes());
        }
    }

    #[test]
    fn non_canonical_integers_are_refused() {
        for bad in ["i03e", "i-0e", "i0042e"] {
            assert!(
                matches!(decode(bad.as_bytes()), Err(Error::NonCanonicalInteger(_))),
                "{bad} should be refused"
            );
        }
    }

    /// And inside `info` too, which is the case worth pinning because the
    /// obvious argument for the rule does not hold there.
    ///
    /// An unsorted key inside `info` is tolerated, by T-172, because the
    /// `info` bytes are hashed from their recorded span and never re-encoded.
    /// That is equally true of a leading zero, so "it would move the hash" is
    /// not why this is refused. It is refused because no torrent in the corpus
    /// carries one and a rule relaxed without an instance is tolerance nobody
    /// asked for. See `TODO/metainfo.md`, T-187, and the comment on
    /// `Parser::integer`.
    #[test]
    fn a_non_canonical_integer_inside_info_is_refused_too() {
        // A leading zero on `info.length`, inside the dictionary the hash
        // is taken over.
        let torrent = b"d4:infod6:lengthi03eee";
        assert!(
            matches!(decode_torrent(torrent), Err(Error::NonCanonicalInteger(_))),
            "a leading zero inside info is refused"
        );
        // The same torrent with the integer written canonically is read, and
        // the span that would have been hashed is recorded, which is what
        // makes the paragraph above true rather than asserted.
        let good = b"d4:infod6:lengthi3eee";
        let (_, span, _) = decode_torrent(good).expect("the canonical form reads");
        let span = span.expect("info has a span");
        assert_eq!(&good[span], b"d6:lengthi3ee");
    }

    #[test]
    fn byte_strings_round_trip_including_binary() {
        assert_eq!(decode(b"4:spam").unwrap(), Value::Bytes(b"spam".to_vec()));
        assert_eq!(decode(b"0:").unwrap(), Value::Bytes(Vec::new()));
        let binary = Value::Bytes(vec![0, 255, 128, b':']);
        assert_eq!(decode(&encode(&binary)).unwrap(), binary);
    }

    #[test]
    fn a_length_longer_than_the_input_is_refused() {
        assert!(matches!(
            decode(b"10:short"),
            Err(Error::LengthOverrun { .. })
        ));
        assert!(matches!(decode(b"04:spam"), Err(Error::BadLength(_))));
    }

    #[test]
    fn lists_round_trip_and_nest() {
        let value = Value::List(vec![Value::Int(1), Value::text("a"), Value::List(vec![])]);
        assert_eq!(encode(&value), b"li1e1:alee");
        assert_eq!(decode(&encode(&value)).unwrap(), value);
    }

    // -----------------------------------------------------------------------
    // `TODO/metainfo.md`, T-172: what a torrent's own encoding is allowed to
    // do. A torrent with keys out of order or a trailing newline is read, and
    // both are recorded rather than accepted silently.
    // -----------------------------------------------------------------------

    /// `d4:infod4:name1:a12:piece lengthi1024ee8:announce3:xyze`
    /// has `info` before `announce`, which BEP 3 forbids.
    const UNSORTED_TOP: &[u8] = b"d4:infod4:name1:a12:piece lengthi1024ee8:announce3:xyze";

    /// The same torrent with the top level sorted and `info`'s own keys out of
    /// order instead.
    const UNSORTED_INFO: &[u8] = b"d8:announce3:xyz4:infod12:piece lengthi1024e4:name1:aee";

    /// Both lists sorted.
    const SORTED: &[u8] = b"d8:announce3:xyz4:infod4:name1:a12:piece lengthi1024eee";

    #[test]
    fn a_torrent_with_sorted_keys_reports_a_canonical_encoding() {
        let (_, span, encoding) = decode_torrent(SORTED).unwrap();
        assert!(span.is_some());
        assert!(encoding.is_canonical(), "{encoding:?}");
        assert!(encoding.notes().is_empty());
    }

    /// Read, not refused. Every other client opens these, and the `info` bytes
    /// are kept verbatim so the hash is what those clients say it is.
    #[test]
    fn keys_out_of_order_are_read_and_recorded() {
        let (value, _, encoding) = decode_torrent(UNSORTED_TOP).unwrap();
        assert!(value.get("announce").is_some(), "the data is still there");
        assert_eq!(encoding.unsorted_dicts, vec![0]);
        assert!(
            !encoding.unsorted_inside_info,
            "the top-level dictionary is not inside `info`"
        );
        assert!(!encoding.is_canonical());
    }

    /// The one that matters, kept apart from the one that does not.
    #[test]
    fn keys_out_of_order_inside_info_are_recorded_as_such() {
        let (_, span, encoding) = decode_torrent(UNSORTED_INFO).unwrap();
        assert_eq!(encoding.unsorted_dicts.len(), 1);
        assert!(encoding.unsorted_inside_info, "{encoding:?}");
        // And the span still points at the original bytes, so the hash is
        // taken over what the torrent actually said.
        let span = span.unwrap();
        assert_eq!(
            &UNSORTED_INFO[span],
            b"d12:piece lengthi1024e4:name1:ae".as_slice()
        );
    }

    /// The recorded offset names the dictionary that is out of order, not the
    /// key, so a caller can point at it.
    #[test]
    fn the_recorded_offset_is_the_start_of_the_unsorted_dictionary() {
        let (_, _, encoding) = decode_torrent(UNSORTED_INFO).unwrap();
        let at = encoding.unsorted_dicts[0];
        assert_eq!(UNSORTED_INFO[at], b'd');
        assert!(
            UNSORTED_INFO[at..].starts_with(b"d12:piece length"),
            "the offset names the `info` value's own dictionary, not the top level"
        );
        assert_ne!(at, 0, "the top level is sorted in this fixture");
    }

    #[test]
    fn a_note_says_which_rule_was_bent() {
        let (_, _, encoding) = decode_torrent(UNSORTED_INFO).unwrap();
        let notes = encoding.notes();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("BEP 3"), "{notes:?}");
        assert!(notes[0].contains("inside `info`"), "{notes:?}");
    }

    /// mkbrr's rule: space, tab, CR, LF and NUL after the top-level dictionary
    /// are read. They are outside `info` by definition, so they cannot move
    /// the hash.
    #[test]
    fn trailing_whitespace_and_nul_are_read_and_counted() {
        for tail in [
            // Escapes rather than the bytes themselves, for the reason on
            // TOLERATED_TRAILING: a raw NUL here made the whole file binary
            // to every text tool that looked at it.
            b" ".as_slice(),
            b"\t",
            b"\r\n",
            b"\0",
            b" \r\n\0\t",
        ] {
            let mut input = SORTED.to_vec();
            input.extend_from_slice(tail);
            let (_, span, encoding) = decode_torrent(&input)
                .unwrap_or_else(|e| panic!("{tail:?} should be tolerated: {e}"));
            assert!(span.is_some());
            assert_eq!(encoding.trailing_bytes, tail.len());
            assert!(!encoding.is_canonical());
            assert!(encoding.notes()[0].contains("whitespace or NUL"));
        }
    }

    /// Anything else after the dictionary is still refused, and the error says
    /// what the rule is rather than only that something was there.
    #[test]
    fn other_trailing_bytes_are_still_refused_and_the_error_names_the_rule() {
        let mut input = SORTED.to_vec();
        input.extend_from_slice(b"XYZ");
        let err = decode_torrent(&input).unwrap_err();
        assert!(matches!(err, Error::TrailingData { trailing: 3, .. }));
        let text = err.to_string();
        assert!(text.contains("whitespace and NUL"), "{text}");
        assert!(text.contains("top-level dictionary"), "{text}");
    }

    /// One tolerated byte followed by one that is not is refused whole, rather
    /// than the tolerated prefix being consumed and the rest reported.
    #[test]
    fn a_tolerated_byte_does_not_excuse_the_one_after_it() {
        let mut input = SORTED.to_vec();
        input.extend_from_slice(
            b"
X",
        );
        assert!(matches!(
            decode_torrent(&input).unwrap_err(),
            Error::TrailingData { trailing: 2, .. }
        ));
    }

    /// `decode` is the general entry point, used for an `info` dictionary on
    /// its own and for tracker responses, and it stays strict. Trailing bytes
    /// inside something that gets hashed are not the same question as trailing
    /// bytes after a file.
    #[test]
    fn the_general_decoder_tolerates_nothing_after_the_value() {
        let mut input = SORTED.to_vec();
        input.push(b'\n');
        assert!(decode(&input).is_err());
        assert!(decode_torrent(&input).is_ok());
    }

    /// Duplicate keys are still refused, inside `info` and out. Two encodings
    /// of one dictionary is the ambiguity that key **order** is not: a reader
    /// taking the first and a reader taking the last disagree about what the
    /// torrent says while agreeing on its hash.
    #[test]
    fn a_duplicate_key_is_still_refused_after_the_sort_rule_relaxed() {
        let input = b"d3:fooi1e3:fooi2ee";
        assert!(matches!(
            decode_torrent(input).unwrap_err(),
            Error::DuplicateKey(_)
        ));
    }

    /// A document built to exhaust the stack is refused, in bytes rather than
    /// in a crash.
    ///
    /// Before the bound existed, 1,000 deep parsed and 10,000 deep took the
    /// test process down with `STATUS_STACK_OVERFLOW`, which `catch_unwind`
    /// cannot see. The assertion here is on the error, because a test that
    /// only fails by killing the runner tells you nothing when it passes.
    #[test]
    fn a_document_nested_past_the_bound_is_refused_rather_than_crashing() {
        let deep = 10_000usize;
        let mut input = vec![b'l'; deep];
        input.extend(std::iter::repeat_n(b'e', deep));
        assert!(matches!(
            decode(&input).unwrap_err(),
            Error::TooDeep { limit: 100, .. }
        ));

        let text = decode(&input).unwrap_err().to_string();
        assert!(text.contains("nested more than 100 deep"), "{text}");
    }

    /// And the bound is far above anything a real torrent reaches, so nothing
    /// legitimate is caught by it.
    #[test]
    fn nesting_a_real_torrent_reaches_is_well_inside_the_bound() {
        // root, `info`, `files`, one file, `path`, a string: six.
        let torrent = dict(&[(
            "info",
            dict(&[(
                "files",
                Value::List(vec![dict(&[(
                    "path",
                    Value::List(vec![Value::Bytes(b"a.bin".to_vec())]),
                )])]),
            )]),
        )]);
        let encoded = encode(&torrent);
        assert_eq!(decode(&encoded).unwrap(), torrent);

        // Ten times deeper than that still reads.
        let mut nested = Value::Int(1);
        for _ in 0..60 {
            nested = Value::List(vec![nested]);
        }
        assert!(decode(&encode(&nested)).is_ok());
    }

    /// The bound is on nesting, not on length: a list of a hundred thousand
    /// integers is flat and is read.
    #[test]
    fn a_long_flat_list_is_not_deep() {
        let flat = Value::List((0..100_000).map(Value::Int).collect());
        let encoded = encode(&flat);
        assert_eq!(decode(&encoded).unwrap(), flat);
    }

    #[test]
    fn dictionary_keys_are_emitted_in_sorted_order() {
        // Inserted out of order, emitted in byte order.
        let value = dict(&[("zebra", Value::Int(1)), ("apple", Value::Int(2))]);
        assert_eq!(encode(&value), b"d5:applei2e5:zebrai1ee");
    }

    #[test]
    fn key_sorting_is_by_raw_bytes_not_by_length() {
        // "b" sorts after "ab" by byte value even though it is shorter.
        let value = dict(&[("b", Value::Int(1)), ("ab", Value::Int(2))]);
        assert_eq!(encode(&value), b"d2:abi2e1:bi1ee");
    }

    #[test]
    fn encoding_is_stable_regardless_of_insertion_order() {
        let one = dict(&[
            ("a", Value::Int(1)),
            ("b", Value::Int(2)),
            ("c", Value::Int(3)),
        ]);
        let other = dict(&[
            ("c", Value::Int(3)),
            ("a", Value::Int(1)),
            ("b", Value::Int(2)),
        ]);
        assert_eq!(encode(&one), encode(&other));
    }

    #[test]
    fn a_duplicate_key_is_refused() {
        assert!(matches!(
            decode(b"d1:ai1e1:ai2ee"),
            Err(Error::DuplicateKey(_))
        ));
    }

    #[test]
    fn a_non_string_key_is_refused() {
        assert!(matches!(decode(b"di1ei2ee"), Err(Error::NonStringKey(_))));
    }

    #[test]
    fn trailing_data_is_refused() {
        assert!(matches!(
            decode(b"i1eXX"),
            Err(Error::TrailingData { trailing: 2, .. })
        ));
        // A prefix decode accepts it and reports where it stopped.
        assert_eq!(decode_prefix(b"i1eXX").unwrap(), (Value::Int(1), 3));
    }

    #[test]
    fn truncated_input_is_refused_rather_than_silently_accepted() {
        let truncated: [&[u8]; 5] = [b"d1:a", b"li1e", b"i42", b"3:ab", b"d"];
        for input in truncated {
            assert!(decode(input).is_err(), "{input:?} should be refused");
        }
    }

    #[test]
    fn the_info_span_is_the_exact_bytes_of_the_info_value() {
        let torrent = b"d8:announce3:foo4:infod4:name3:bar12:piece lengthi16eee";
        let (_, span) = decode_with_info_span(torrent).unwrap();
        let span = span.expect("info key exists");
        assert_eq!(&torrent[span.clone()], b"d4:name3:bar12:piece lengthi16ee");
        // The span decodes on its own, which is what SHA-1 is taken over.
        assert!(decode(&torrent[span]).is_ok());
    }

    #[test]
    fn a_nested_info_key_is_not_mistaken_for_the_top_level_one() {
        // The inner dict under key "a" also has an "info" key. Only the outer
        // one counts, because that is the one the info hash is taken over.
        let torrent = b"d1:ad4:info3:xxxe4:infod4:name3:baree";
        let (_, span) = decode_with_info_span(torrent).unwrap();
        let span = span.expect("top level info exists");
        assert_eq!(&torrent[span], b"d4:name3:bare");
    }

    #[test]
    fn a_torrent_without_an_info_key_reports_no_span() {
        let (_, span) = decode_with_info_span(b"d8:announce3:fooe").unwrap();
        assert!(span.is_none());
    }

    #[test]
    fn accessors_read_what_is_there_and_nothing_else() {
        let value = dict(&[
            ("n", Value::Int(7)),
            ("s", Value::text("hi")),
            ("l", Value::List(vec![Value::text("a"), Value::Int(1)])),
        ]);
        assert_eq!(value.get("n").and_then(Value::as_int), Some(7));
        assert_eq!(
            value.get("s").and_then(Value::as_text).as_deref(),
            Some("hi")
        );
        assert_eq!(
            value.get("l").map(Value::as_text_list),
            Some(vec!["a".to_string()])
        );
        assert!(value.get("missing").is_none());
        assert!(value.get("n").and_then(Value::as_bytes).is_none());
    }

    #[test]
    fn one_string_and_a_list_of_them_read_the_same_way() {
        let value = dict(&[
            ("one", Value::text("https://a.example.com/")),
            (
                "many",
                Value::List(vec![
                    Value::text("https://a.example.com/"),
                    Value::text("https://b.example.com/"),
                    Value::Int(1),
                ]),
            ),
            ("neither", Value::Int(7)),
        ]);
        assert_eq!(
            value.get("one").map(Value::as_text_or_text_list),
            Some(vec!["https://a.example.com/".to_string()])
        );
        assert_eq!(
            value.get("many").map(Value::as_text_or_text_list),
            Some(vec![
                "https://a.example.com/".to_string(),
                "https://b.example.com/".to_string(),
            ])
        );
        // A shape that is neither still yields nothing rather than panicking,
        // and the plain list accessor keeps refusing the string form, which is
        // why the two are separate methods.
        assert_eq!(
            value.get("neither").map(Value::as_text_or_text_list),
            Some(Vec::new())
        );
        assert_eq!(value.get("one").map(Value::as_text_list), Some(Vec::new()));
    }

    #[test]
    fn invalid_utf8_names_are_shown_rather_than_refused() {
        let value = Value::Bytes(vec![0xff, 0xfe]);
        assert!(value.as_text().is_some());
    }

    #[test]
    fn a_realistic_torrent_round_trips_byte_for_byte() {
        let original = dict(&[
            ("announce", Value::text("udp://tracker.example.com:80")),
            (
                "announce-list",
                Value::List(vec![Value::List(vec![Value::text("udp://a:80")])]),
            ),
            ("comment", Value::text("hello")),
            ("creation date", Value::Int(1_787_140_323)),
            (
                "info",
                dict(&[
                    ("length", Value::Int(1024)),
                    ("name", Value::text("payload.bin")),
                    ("piece length", Value::Int(16384)),
                    ("pieces", Value::Bytes(vec![0u8; 20])),
                ]),
            ),
            (
                "url-list",
                Value::List(vec![Value::text("https://e.com/pub/")]),
            ),
        ]);
        let encoded = encode(&original);
        assert_eq!(decode(&encoded).unwrap(), original);
        assert_eq!(encode(&decode(&encoded).unwrap()), encoded);
    }
}
