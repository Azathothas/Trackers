//! Scope selectors: what part of a torrent one source is allowed to serve.
//!
//! A source that only has part of the payload is a first-class case, not an
//! error. A scope names that part. Every form below resolves against the
//! torrent's [`Layout`] into a [`SpanSet`] of byte ranges, which is what the
//! request layer clamps against and what the coverage check subtracts.
//!
//! # Grammar
//!
//! ```text
//! scope := term ("," term)*
//! term  := "!" body      exclude these bytes from the selection
//!        | body          include these bytes
//!
//! body  := "*"                       every file
//!        | N                         file index N
//!        | N "-" M                   file indices N through M inclusive
//!        | N "-"                     file index N to the last file
//!        | "piece:" N "-" M          piece indices N through M inclusive
//!        | "piece:" N                piece index N
//!        | "piece:" N "-"            piece N to the last piece
//!        | "byte:" SIZE "-" SIZE     byte range within the whole payload
//!        | "byte:" SIZE "-"          from that offset to the end
//!        | "file:" N ":byte:" RANGE  byte range within one file
//!        | "path/to/file.iso"        exact path within the torrent
//!        | "*.iso"                   glob against the file path
//! ```
//!
//! Sizes accept binary units, so `byte:1GiB-2GiB` works, per the unit rules in
//! [`crate::units`].
//!
//! # Semantics
//!
//! Includes are unioned, then excludes are subtracted. A scope made only of
//! exclusions starts from every file, so `!*.nfo` means "everything except the
//! nfo files" without having to write `*,!*.nfo`.
//!
//! An include term that matches nothing is an error, not a silent no-op. A
//! scope that resolves to no bytes at all is an error. Both exit with
//! [`crate::exit::ExitCode::Binding`], because a selector that matched nothing
//! is almost always a typo and silently downloading from peers instead is the
//! wrong thing to do to a script.

use std::fmt;
use std::ops::Range;

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::span::SpanSet;
use crate::units::parse_size;

/// One term of a scope, before it is resolved against a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    /// `*`: every file.
    All,
    /// `3`, `3-7`, `9-`: file indices, inclusive, possibly open-ended.
    FileIndices { first: usize, last: Option<usize> },
    /// `piece:0-511`, `piece:1024-`: piece indices, inclusive.
    Pieces { first: u32, last: Option<u32> },
    /// `byte:0-1MiB`: a byte range of the whole payload.
    Bytes { start: u64, end: Option<u64> },
    /// `file:3:byte:0-4MiB`: a byte range within one file.
    FileBytes {
        file: usize,
        start: u64,
        end: Option<u64>,
    },
    /// An exact path within the torrent, `/`-separated.
    Path(String),
    /// A glob matched against the `/`-separated file path.
    Glob(Box<GlobTerm>),
}

/// A compiled glob and the text it came from.
#[derive(Debug, Clone)]
pub struct GlobTerm {
    pattern: String,
    matcher: GlobMatcher,
}

impl PartialEq for GlobTerm {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for GlobTerm {}

/// A parsed scope: what to include, and what to take back out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    text: String,
    includes: Vec<Term>,
    excludes: Vec<Term>,
}

/// What a scope resolved to against one torrent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedScope {
    /// The scope as it was written.
    pub selector: String,
    /// The byte ranges the source may serve.
    pub spans: SpanSet,
    /// File indices the scope touches, in order. A file is listed when any of
    /// its bytes are in scope, even partially.
    pub files: Vec<usize>,
    /// Piece indices the scope touches, in order. A piece is listed when any
    /// of its bytes are in scope, so a partial piece appears here and the
    /// picker knows the source cannot complete it alone.
    pub pieces: Vec<u32>,
    /// Total bytes in scope.
    pub bytes: u64,
}

impl ResolvedScope {
    /// Whether the scope covers every byte of `piece`.
    ///
    /// A source can only satisfy a piece hash on its own if it holds the whole
    /// piece, so this is the test the picker uses rather than [`Self::pieces`].
    pub fn covers_piece(&self, layout: &Layout, piece: u32) -> bool {
        layout
            .piece_range(piece)
            .is_some_and(|range| self.spans.contains_range(&range))
    }

    /// Piece indices wholly inside the scope.
    pub fn whole_pieces(&self, layout: &Layout) -> Vec<u32> {
        self.pieces
            .iter()
            .copied()
            .filter(|p| self.covers_piece(layout, *p))
            .collect()
    }
}

impl Scope {
    /// The scope covering the whole torrent.
    pub fn all() -> Self {
        Self {
            text: "*".to_string(),
            includes: vec![Term::All],
            excludes: Vec::new(),
        }
    }

    /// Parse a scope selector.
    pub fn parse(input: &str) -> Result<Self> {
        let text = input.trim();
        if text.is_empty() {
            return Err(Error::binding("empty scope selector").with("selector", input));
        }
        let mut includes = Vec::new();
        let mut excludes = Vec::new();
        for raw in split_terms(text) {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            match raw.strip_prefix('!') {
                Some(rest) => excludes.push(parse_term(rest.trim(), input)?),
                None => includes.push(parse_term(raw, input)?),
            }
        }
        if includes.is_empty() && excludes.is_empty() {
            return Err(Error::binding("scope selector has no terms").with("selector", input));
        }
        // A scope written only as exclusions starts from everything, so
        // `!*.nfo` reads the way it looks.
        if includes.is_empty() {
            includes.push(Term::All);
        }
        Ok(Self {
            text: text.to_string(),
            includes,
            excludes,
        })
    }

    /// The selector as written.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether this scope is the unrestricted one.
    pub fn is_all(&self) -> bool {
        self.excludes.is_empty() && self.includes == [Term::All]
    }

    /// Resolve against a torrent.
    ///
    /// Every include term is checked against the layout before anything else
    /// happens, so a typo in a selector fails immediately with the term named
    /// rather than turning into a quietly smaller download.
    pub fn resolve(&self, layout: &Layout) -> Result<ResolvedScope> {
        let mut included = SpanSet::new();
        for term in &self.includes {
            let spans = resolve_term(term, layout, &self.text)?;
            if spans.is_empty() {
                return Err(Error::binding(format!(
                    "scope term `{}` matched no bytes in this torrent",
                    term
                ))
                .with("selector", self.text.clone())
                .with("term", term.to_string())
                .with("files_in_torrent", layout.files.len())
                .with("pieces_in_torrent", layout.piece_count()));
            }
            included = included.union(&spans);
        }

        let mut excluded = SpanSet::new();
        for term in &self.excludes {
            // An exclusion that matches nothing is not an error. Excluding
            // `*.nfo` from a torrent with no nfo files is a reasonable thing
            // for a generated config to do.
            excluded = excluded.union(&resolve_term(term, layout, &self.text)?);
        }

        let spans = included.difference(&excluded).clamp(layout.payload());
        if spans.is_empty() {
            return Err(Error::binding(format!(
                "scope `{}` resolves to no bytes: the exclusions remove everything the inclusions select",
                self.text
            ))
            .with("selector", self.text.clone()));
        }

        let files: Vec<usize> = (0..layout.files.len())
            .filter(|&i| {
                let file = &layout.files[i];
                file.length > 0
                    && !spans
                        .intersection(&SpanSet::from_range(file.range()))
                        .is_empty()
            })
            .collect();
        let mut pieces: Vec<u32> = Vec::new();
        for span in spans.spans() {
            let range = layout.pieces_overlapping(span);
            for piece in range {
                if pieces.last() != Some(&piece) {
                    pieces.push(piece);
                }
            }
        }
        let bytes = spans.len();
        Ok(ResolvedScope {
            selector: self.text.clone(),
            spans,
            files,
            pieces,
            bytes,
        })
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for Scope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Scope::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::All => f.write_str("*"),
            Term::FileIndices { first, last } => match last {
                Some(last) if last == first => write!(f, "{first}"),
                Some(last) => write!(f, "{first}-{last}"),
                None => write!(f, "{first}-"),
            },
            Term::Pieces { first, last } => match last {
                Some(last) if last == first => write!(f, "piece:{first}"),
                Some(last) => write!(f, "piece:{first}-{last}"),
                None => write!(f, "piece:{first}-"),
            },
            Term::Bytes { start, end } => match end {
                Some(end) => write!(f, "byte:{start}-{end}"),
                None => write!(f, "byte:{start}-"),
            },
            Term::FileBytes { file, start, end } => match end {
                Some(end) => write!(f, "file:{file}:byte:{start}-{end}"),
                None => write!(f, "file:{file}:byte:{start}-"),
            },
            Term::Path(path) => f.write_str(path),
            Term::Glob(g) => f.write_str(&g.pattern),
        }
    }
}

/// Split on commas that separate terms.
///
/// A `file:3:byte:0-4MiB` term has no commas in it and neither does any other
/// form, so a plain split is correct. It is a function rather than an inline
/// `split` so the reason is recorded next to the assumption.
fn split_terms(text: &str) -> impl Iterator<Item = &str> {
    text.split(',')
}

fn parse_term(raw: &str, whole: &str) -> Result<Term> {
    let bad = |reason: &str| {
        Error::binding(format!("cannot parse scope term `{raw}`: {reason}"))
            .with("selector", whole.to_string())
            .with("term", raw.to_string())
    };

    if raw == "*" {
        return Ok(Term::All);
    }
    if let Some(rest) = raw.strip_prefix("piece:") {
        let (first, last) = parse_index_range(rest).ok_or_else(|| bad("expected N, N-M, or N-"))?;
        let first = u32::try_from(first).map_err(|_| bad("piece index does not fit in 32 bits"))?;
        let last = match last {
            Some(l) => {
                Some(u32::try_from(l).map_err(|_| bad("piece index does not fit in 32 bits"))?)
            }
            None => None,
        };
        if let Some(last) = last
            && last < first
        {
            return Err(bad("the range ends before it starts"));
        }
        return Ok(Term::Pieces { first, last });
    }
    if let Some(rest) = raw.strip_prefix("byte:") {
        let (start, end) =
            parse_size_range(rest).ok_or_else(|| bad("expected SIZE-SIZE or SIZE-"))?;
        if let Some(end) = end
            && end <= start
        {
            return Err(bad("the range ends at or before it starts"));
        }
        return Ok(Term::Bytes { start, end });
    }
    if let Some(rest) = raw.strip_prefix("file:") {
        // `file:3` and `file:3-7` are the explicit spellings of `3` and
        // `3-7`. A generated binding table tends to prefer them because they
        // cannot be mistaken for anything else.
        let Some((index, tail)) = rest.split_once(':') else {
            let (first, last) = parse_index_range(rest)
                .ok_or_else(|| bad("expected file:N, file:N-M, or file:N:byte:RANGE"))?;
            if let Some(last) = last
                && last < first
            {
                return Err(bad("the range ends before it starts"));
            }
            return Ok(Term::FileIndices { first, last });
        };
        let file: usize = index
            .parse()
            .map_err(|_| bad("file index is not a number"))?;
        let range = tail
            .strip_prefix("byte:")
            .ok_or_else(|| bad("expected file:N:byte:RANGE"))?;
        let (start, end) =
            parse_size_range(range).ok_or_else(|| bad("expected SIZE-SIZE or SIZE-"))?;
        if let Some(end) = end
            && end <= start
        {
            return Err(bad("the range ends at or before it starts"));
        }
        return Ok(Term::FileBytes { file, start, end });
    }
    // A bare number or numeric range is file indices. This is checked before
    // the glob branch so `3-7` never reaches globset, where `-` inside a
    // bracket expression means something else entirely.
    if raw.chars().all(|c| c.is_ascii_digit() || c == '-')
        && raw.chars().any(|c| c.is_ascii_digit())
        && let Some((first, last)) = parse_index_range(raw)
    {
        if let Some(last) = last
            && last < first
        {
            return Err(bad("the range ends before it starts"));
        }
        return Ok(Term::FileIndices { first, last });
    }
    if raw.contains(['*', '?', '[', '{']) {
        let glob = Glob::new(raw).map_err(|e| bad(&format!("invalid glob: {e}")))?;
        return Ok(Term::Glob(Box::new(GlobTerm {
            pattern: raw.to_string(),
            matcher: glob.compile_matcher(),
        })));
    }
    Ok(Term::Path(raw.trim_matches('/').to_string()))
}

/// Parse `N`, `N-M`, or `N-` into an inclusive index range.
fn parse_index_range(text: &str) -> Option<(usize, Option<usize>)> {
    match text.split_once('-') {
        None => {
            let n = text.trim().parse().ok()?;
            Some((n, Some(n)))
        }
        Some((first, "")) => Some((first.trim().parse().ok()?, None)),
        Some((first, last)) => Some((first.trim().parse().ok()?, Some(last.trim().parse().ok()?))),
    }
}

/// Parse `SIZE-SIZE` or `SIZE-` into a half-open byte range.
///
/// The end is exclusive, so `byte:0-1MiB` is the first mebibyte and
/// `byte:1MiB-2MiB` is the second, with no overlap and no off-by-one for a
/// caller to reason about.
fn parse_size_range(text: &str) -> Option<(u64, Option<u64>)> {
    let (first, last) = text.split_once('-')?;
    let start = parse_size(first).ok()?;
    if last.trim().is_empty() {
        return Some((start, None));
    }
    Some((start, Some(parse_size(last).ok()?)))
}

fn resolve_term(term: &Term, layout: &Layout, selector: &str) -> Result<SpanSet> {
    Ok(match term {
        Term::All => SpanSet::from_range(layout.payload()),
        Term::FileIndices { first, last } => {
            let last = last.unwrap_or(layout.files.len().saturating_sub(1));
            if *first >= layout.files.len() {
                return Err(Error::binding(format!(
                    "file index {first} is out of range: this torrent has {} files (0-{})",
                    layout.files.len(),
                    layout.files.len().saturating_sub(1)
                ))
                .with("selector", selector.to_string())
                .with("term", term.to_string())
                .with("file_count", layout.files.len()));
            }
            SpanSet::from_ranges(
                layout.files[*first..=last.min(layout.files.len() - 1)]
                    .iter()
                    .map(|f| f.range()),
            )
        }
        Term::Pieces { first, last } => {
            let count = layout.piece_count();
            if *first >= count {
                return Err(Error::binding(format!(
                    "piece index {first} is out of range: this torrent has {count} pieces (0-{})",
                    count.saturating_sub(1)
                ))
                .with("selector", selector.to_string())
                .with("term", term.to_string())
                .with("piece_count", count));
            }
            let last = last
                .unwrap_or(count.saturating_sub(1))
                .min(count.saturating_sub(1));
            SpanSet::from_range(layout.pieces_range(*first, last))
        }
        Term::Bytes { start, end } => {
            SpanSet::from_range(*start..end.unwrap_or(layout.total_length)).clamp(layout.payload())
        }
        Term::FileBytes { file, start, end } => {
            let entry = layout.file(*file).ok_or_else(|| {
                Error::binding(format!(
                    "file index {file} is out of range: this torrent has {} files",
                    layout.files.len()
                ))
                .with("selector", selector.to_string())
                .with("term", term.to_string())
            })?;
            let end = end.unwrap_or(entry.length).min(entry.length);
            SpanSet::from_range(entry.offset + start..entry.offset + end).clamp(entry.range())
        }
        Term::Path(path) => {
            let wanted = path.trim_matches('/');
            SpanSet::from_ranges(
                layout
                    .files
                    .iter()
                    .filter(|f| f.display_path() == wanted)
                    .map(|f| f.range()),
            )
        }
        Term::Glob(glob) => SpanSet::from_ranges(
            layout
                .files
                .iter()
                .filter(|f| {
                    // Match the full path first, then the bare file name, so
                    // `*.iso` finds `sub/dir/x.iso` the way a person expects
                    // rather than only a top-level file.
                    glob.matcher.is_match(f.display_path()) || glob.matcher.is_match(f.file_name())
                })
                .map(|f| f.range()),
        ),
    })
}

/// Whether a resolved scope names exactly one file, which is what composition
/// mode `exact` requires on a multi-file torrent.
pub fn single_file(resolved: &ResolvedScope) -> Option<usize> {
    match resolved.files.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

/// The byte range shared by a scope and a piece, clamped.
pub fn piece_overlap(resolved: &ResolvedScope, layout: &Layout, piece: u32) -> Option<Range<u64>> {
    let range = layout.piece_range(piece)?;
    let overlap = resolved.spans.intersection(&SpanSet::from_range(range));
    overlap.bounds()
}

#[cfg(test)]
mod tests {
    // Same as in `span.rs`: these compare against a one-element array of
    // ranges, which is the shape the API returns.
    #![allow(clippy::single_range_in_vec_init)]

    use super::*;

    /// Three files: 0..1500, 1500..2000, 2000..2100. Piece length 1024, so
    /// three pieces with a short last one.
    fn layout() -> Layout {
        Layout::from_lengths(
            "album",
            true,
            1024,
            [
                ("disc 1/a.flac".to_string(), 1500u64),
                ("disc 1/b.flac".to_string(), 500),
                ("notes.nfo".to_string(), 100),
            ],
        )
    }

    fn resolve(selector: &str) -> Result<ResolvedScope> {
        Scope::parse(selector)?.resolve(&layout())
    }

    #[test]
    fn star_selects_the_whole_payload() {
        let r = resolve("*").unwrap();
        assert_eq!(r.spans.spans(), &[0..2100]);
        assert_eq!(r.files, vec![0, 1, 2]);
        assert_eq!(r.pieces, vec![0, 1, 2]);
        assert_eq!(r.bytes, 2100);
        assert!(Scope::parse("*").unwrap().is_all());
    }

    #[test]
    fn a_bare_number_selects_one_file() {
        let r = resolve("1").unwrap();
        assert_eq!(r.spans.spans(), &[1500..2000]);
        assert_eq!(r.files, vec![1]);
        assert_eq!(single_file(&r), Some(1));
    }

    #[test]
    fn an_index_range_is_inclusive_at_both_ends() {
        let r = resolve("0-1").unwrap();
        assert_eq!(r.spans.spans(), &[0..2000]);
        assert_eq!(r.files, vec![0, 1]);
    }

    #[test]
    fn an_open_ended_index_range_runs_to_the_last_file() {
        let r = resolve("1-").unwrap();
        assert_eq!(r.spans.spans(), &[1500..2100]);
        assert_eq!(r.files, vec![1, 2]);
    }

    #[test]
    fn an_index_list_unions_its_terms() {
        let r = resolve("0,2").unwrap();
        assert_eq!(r.spans.spans(), &[0..1500, 2000..2100]);
        assert_eq!(r.files, vec![0, 2]);
    }

    #[test]
    fn piece_ranges_select_by_piece() {
        let r = resolve("piece:0-1").unwrap();
        assert_eq!(r.spans.spans(), &[0..2048]);
        assert_eq!(r.pieces, vec![0, 1]);
        let open = resolve("piece:1-").unwrap();
        assert_eq!(open.spans.spans(), &[1024..2100]);
        let single = resolve("piece:2").unwrap();
        assert_eq!(single.spans.spans(), &[2048..2100]);
    }

    #[test]
    fn byte_ranges_are_half_open_and_take_binary_units() {
        let r = resolve("byte:0-1024").unwrap();
        assert_eq!(r.spans.spans(), &[0..1024]);
        let second = resolve("byte:1024-2048").unwrap();
        assert_eq!(second.spans.spans(), &[1024..2048]);
        assert!(
            r.spans.intersection(&second.spans).is_empty(),
            "adjacent ranges must not overlap"
        );
        let kib = resolve("byte:0-1KiB").unwrap();
        assert_eq!(kib.spans.spans(), &[0..1024]);
    }

    #[test]
    fn an_open_ended_byte_range_runs_to_the_end() {
        let r = resolve("byte:2000-").unwrap();
        assert_eq!(r.spans.spans(), &[2000..2100]);
    }

    #[test]
    fn file_n_is_the_explicit_spelling_of_a_bare_index() {
        assert_eq!(resolve("file:1").unwrap().files, vec![1]);
        assert_eq!(resolve("file:1").unwrap().spans.spans(), &[1500..2000]);
        assert_eq!(resolve("file:0-1").unwrap().files, vec![0, 1]);
        assert_eq!(resolve("file:1-").unwrap().files, vec![1, 2]);
        assert!(Scope::parse("file:7-3").is_err());
        assert!(Scope::parse("file:x").is_err());
    }

    #[test]
    fn a_byte_range_within_a_file_is_relative_to_that_file() {
        let r = resolve("file:1:byte:0-100").unwrap();
        assert_eq!(r.spans.spans(), &[1500..1600]);
        assert_eq!(r.files, vec![1]);
    }

    #[test]
    fn a_file_byte_range_is_clamped_to_the_file() {
        let r = resolve("file:2:byte:0-99999").unwrap();
        assert_eq!(r.spans.spans(), &[2000..2100]);
    }

    #[test]
    fn an_exact_path_selects_that_file() {
        let r = resolve("disc 1/b.flac").unwrap();
        assert_eq!(r.files, vec![1]);
    }

    #[test]
    fn a_glob_matches_the_path_or_the_file_name() {
        assert_eq!(resolve("*.flac").unwrap().files, vec![0, 1]);
        assert_eq!(resolve("disc 1/*").unwrap().files, vec![0, 1]);
        assert_eq!(resolve("*.nfo").unwrap().files, vec![2]);
    }

    #[test]
    fn a_negated_glob_alone_starts_from_everything() {
        let r = resolve("!*.nfo").unwrap();
        assert_eq!(r.files, vec![0, 1]);
        assert_eq!(r.spans.spans(), &[0..2000]);
    }

    #[test]
    fn exclusions_are_subtracted_from_inclusions() {
        let r = resolve("*,!*.nfo").unwrap();
        assert_eq!(r.spans.spans(), &[0..2000]);
        let narrower = resolve("0-2,!1").unwrap();
        assert_eq!(narrower.spans.spans(), &[0..1500, 2000..2100]);
    }

    #[test]
    fn an_exclusion_that_matches_nothing_is_not_an_error() {
        let r = resolve("*,!*.txt").unwrap();
        assert_eq!(r.spans.spans(), &[0..2100]);
    }

    #[test]
    fn an_inclusion_that_matches_nothing_is_an_error() {
        let err = resolve("*.txt").unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Binding);
        assert!(
            err.message().contains("matched no bytes"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn an_out_of_range_index_names_the_real_range() {
        let err = resolve("9").unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Binding);
        assert!(
            err.message().contains("this torrent has 3 files"),
            "{}",
            err.message()
        );
        assert_eq!(err.context()["file_count"], 3);
    }

    #[test]
    fn an_out_of_range_piece_names_the_piece_count() {
        let err = resolve("piece:99").unwrap_err();
        assert!(
            err.message().contains("this torrent has 3 pieces"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn excluding_everything_is_an_error_rather_than_an_empty_download() {
        let err = resolve("0,!0").unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Binding);
        assert!(
            err.message().contains("resolves to no bytes"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn reversed_ranges_are_rejected_at_parse_time() {
        assert!(Scope::parse("7-3").is_err());
        assert!(Scope::parse("piece:9-2").is_err());
        assert!(Scope::parse("byte:100-50").is_err());
    }

    #[test]
    fn an_empty_selector_is_an_error() {
        assert!(Scope::parse("").is_err());
        assert!(Scope::parse("   ").is_err());
    }

    #[test]
    fn whole_pieces_excludes_partially_covered_ones() {
        // Pieces are 1024 bytes. File 1 is 1500..2000, which covers no whole
        // piece: piece 1 is 1024..2048 and extends past both ends of the file.
        let r = resolve("1").unwrap();
        assert_eq!(r.pieces, vec![1]);
        assert!(r.whole_pieces(&layout()).is_empty());
        assert!(!r.covers_piece(&layout(), 1));

        let all = resolve("*").unwrap();
        assert_eq!(all.whole_pieces(&layout()), vec![0, 1, 2]);
    }

    #[test]
    fn a_scope_round_trips_through_serde() {
        let scope = Scope::parse("0-1,!*.nfo").unwrap();
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(json, r#""0-1,!*.nfo""#);
        let back: Scope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, scope);
    }

    #[test]
    fn terms_render_back_to_their_source_form() {
        for text in [
            "*",
            "3",
            "3-7",
            "3-",
            "piece:0-511",
            "piece:1024-",
            "byte:0-1024",
            "file:3:byte:0-100",
        ] {
            let scope = Scope::parse(text).unwrap();
            assert_eq!(
                scope.includes[0].to_string(),
                text,
                "round trip failed for {text}"
            );
        }
    }

    #[test]
    fn a_single_file_torrent_resolves_the_same_way() {
        let single = Layout::from_lengths(
            "movie.mkv",
            false,
            1024,
            [("movie.mkv".to_string(), 3000u64)],
        );
        let r = Scope::parse("*").unwrap().resolve(&single).unwrap();
        assert_eq!(r.files, vec![0]);
        assert_eq!(r.bytes, 3000);
        let head = Scope::parse("byte:0-1KiB")
            .unwrap()
            .resolve(&single)
            .unwrap();
        assert_eq!(head.spans.spans(), &[0..1024]);
    }

    #[test]
    fn piece_overlap_clips_to_the_scope() {
        let l = layout();
        let r = resolve("byte:0-500").unwrap();
        assert_eq!(piece_overlap(&r, &l, 0), Some(0..500));
        assert_eq!(piece_overlap(&r, &l, 1), None);
    }
}
