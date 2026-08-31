//! Composition: how a request URL is built from a source URL and the torrent.
//!
//! This is the "with or without the exact path" control. A mirror rarely lays
//! its files out the way the torrent does, and every other client assumes it
//! does. Four modes cover what mirrors actually do:
//!
//! | Mode | What it appends | Use it when |
//! | --- | --- | --- |
//! | `auto` | BEP 19 rules | The mirror mirrors the torrent. Matches `aria2`. |
//! | `exact` | nothing | The file is renamed or flattened on the server. |
//! | `prefix` | `path` only | The mirror hosts the contents at the root, not inside a directory named after the torrent. |
//! | `template` | whatever you write | Object stores, CDN rewrites, piece-indexed layouts. |
//!
//! Composition is orthogonal to scope: any source can serve any scope under
//! any composition.

use std::fmt;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::Layout;

/// How a request URL is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// BEP 19 default. A single-file torrent whose URL ends in `/` gets `name`
    /// appended, otherwise the URL is the complete resource. A multi-file
    /// torrent gets `name` and then the file's `path` appended.
    #[default]
    Auto,
    /// The URL is the complete resource. Nothing is appended. On a multi-file
    /// torrent this is only valid with a scope that resolves to exactly one
    /// file, and it is a binding error otherwise.
    Exact,
    /// Append the file's `path` but not the torrent `name`.
    Prefix,
    /// The URL carries placeholders that are expanded per request.
    Template,
}

impl Mode {
    /// Parse a mode name.
    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "exact" => Self::Exact,
            "prefix" => Self::Prefix,
            "template" => Self::Template,
            other => {
                return Err(Error::binding(format!(
                    "`{other}` is not a composition mode (use auto, exact, prefix, or template)"
                ))
                .with("mode", other.to_string()));
            }
        })
    }

    /// The mode name as written on the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Exact => "exact",
            Self::Prefix => "prefix",
            Self::Template => "template",
        }
    }

    /// Whether the mode expands placeholders that depend on the byte range of
    /// an individual request rather than only on the file.
    ///
    /// A source whose URL varies per request cannot be served with one ranged
    /// GET per file, so this decides how the fetch layer batches.
    pub fn is_per_request(self) -> bool {
        matches!(self, Self::Template)
    }

    /// Every mode, for documentation and for tests that must stay exhaustive.
    pub const ALL: &'static [Mode] = &[Self::Auto, Self::Exact, Self::Prefix, Self::Template];
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything a template can refer to when a URL is built.
#[derive(Debug, Clone)]
pub struct RequestContext<'a> {
    /// The torrent's shape.
    pub layout: &'a Layout,
    /// Hex info hash, lower case.
    pub info_hash: &'a str,
    /// Index of the file the request falls in, when it falls in exactly one.
    pub file: Option<usize>,
    /// Piece index the request falls in, when it falls in exactly one.
    pub piece: Option<u32>,
    /// Absolute byte offset within the torrent's linear payload.
    pub offset: u64,
    /// Length of the request in bytes.
    pub length: u64,
}

impl<'a> RequestContext<'a> {
    /// A context that names a whole file, which is what `webseed list` reports
    /// and what a GetRight-style ranged GET is built from.
    pub fn for_file(layout: &'a Layout, info_hash: &'a str, file: usize) -> Option<Self> {
        let entry = layout.file(file)?;
        Some(Self {
            layout,
            info_hash,
            file: Some(file),
            piece: layout.piece_at(entry.offset),
            offset: entry.offset,
            length: entry.length,
        })
    }

    /// A context for one byte range of the payload.
    pub fn for_range(layout: &'a Layout, info_hash: &'a str, offset: u64, length: u64) -> Self {
        // A request confined to one file and one piece can name them; one that
        // straddles a boundary cannot, and the placeholders are then an error
        // rather than a silently wrong number.
        let slices = layout.split_by_file(offset..offset + length);
        let file = match slices.as_slice() {
            [only] => Some(only.file),
            _ => None,
        };
        let pieces = layout.pieces_overlapping(&(offset..offset + length));
        let piece = (pieces.end == pieces.start + 1).then_some(pieces.start);
        Self {
            layout,
            info_hash,
            file,
            piece,
            offset,
            length,
        }
    }
}

/// Characters left alone when a path segment is percent-encoded.
///
/// RFC 3986 unreserved only. Encoding more than strictly necessary is safe,
/// under-encoding is not: a `#` or `?` left raw in a file name silently turns
/// the rest of the path into a fragment or a query.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Percent-encode one path segment.
pub fn encode_segment(segment: &str) -> String {
    utf8_percent_encode(segment, SEGMENT).to_string()
}

/// Percent-encode a `/`-separated path, leaving the separators alone.
pub fn encode_path(path: &[String]) -> String {
    path.iter()
        .map(|s| encode_segment(s))
        .collect::<Vec<_>>()
        .join("/")
}

/// Build the URL for one file under `mode`.
///
/// `template` is required when `mode` is [`Mode::Template`] and ignored
/// otherwise.
pub fn compose(
    base: &str,
    mode: Mode,
    template: Option<&str>,
    ctx: &RequestContext<'_>,
) -> Result<String> {
    match mode {
        Mode::Template => {
            let template = template.ok_or_else(|| {
                Error::binding("composition mode is `template` but no template was given")
                    .with("url", base.to_string())
            })?;
            expand(template, ctx)
        }
        Mode::Exact => Ok(base.to_string()),
        Mode::Auto | Mode::Prefix => {
            let file = ctx.file.and_then(|i| ctx.layout.file(i)).ok_or_else(|| {
                Error::binding(format!(
                    "composition mode `{mode}` needs a single file, but this request spans more than one"
                ))
                .with("url", base.to_string())
                .with("mode", mode.as_str())
                .with("offset", ctx.offset)
                .with("length", ctx.length)
            })?;
            let mut url = base.to_string();
            if mode == Mode::Auto && !ctx.layout.multi_file {
                // BEP 19: for a single-file torrent, a URL ending in `/` is a
                // directory and gets the name appended. Anything else is the
                // complete resource.
                if url.ends_with('/') {
                    url.push_str(&encode_segment(&ctx.layout.name));
                }
                return Ok(url);
            }
            if !url.ends_with('/') {
                url.push('/');
            }
            if mode == Mode::Auto {
                url.push_str(&encode_segment(&ctx.layout.name));
                url.push('/');
            }
            url.push_str(&encode_path(&file.path));
            Ok(url)
        }
    }
}

/// Expand a template into a URL.
///
/// Placeholders are `{name}`, `{path}`, `{filename}`, `{index}`, `{piece}`,
/// `{offset}`, `{length}`, `{end}`, `{piece_offset}`, `{piece_length}`, and
/// `{infohash}`. Everything is percent-encoded on expansion; write
/// `{raw:path}` to insert a value without encoding, which is how you keep the
/// `/` separators in a path.
///
/// `{{` and `}}` are literal braces.
pub fn expand(template: &str, ctx: &RequestContext<'_>) -> Result<String> {
    let mut out = String::with_capacity(template.len() + 32);
    let mut chars = template.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '{' if chars.peek().map(|(_, c)| *c) == Some('{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek().map(|(_, c)| *c) == Some('}') => {
                chars.next();
                out.push('}');
            }
            '}' => {
                return Err(Error::binding(format!(
                    "unmatched `}}` at position {index} in template `{template}`"
                ))
                .with("template", template.to_string()));
            }
            '{' => {
                let start = index + 1;
                let mut end = None;
                for (i, c) in chars.by_ref() {
                    if c == '}' {
                        end = Some(i);
                        break;
                    }
                }
                let end = end.ok_or_else(|| {
                    Error::binding(format!(
                        "unclosed `{{` at position {index} in template `{template}`"
                    ))
                    .with("template", template.to_string())
                })?;
                let placeholder = &template[start..end];
                let (raw, name) = match placeholder.strip_prefix("raw:") {
                    Some(rest) => (true, rest),
                    None => (false, placeholder),
                };
                let value = placeholder_value(name, ctx, template)?;
                if raw {
                    out.push_str(&value);
                } else {
                    out.push_str(&encode_segment(&value));
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

fn placeholder_value(name: &str, ctx: &RequestContext<'_>, template: &str) -> Result<String> {
    let missing = |what: &str| {
        Error::binding(format!(
            "template `{template}` uses {{{name}}}, but this request {what}"
        ))
        .with("template", template.to_string())
        .with("placeholder", name.to_string())
        .with("offset", ctx.offset)
        .with("length", ctx.length)
    };
    let file = || {
        ctx.file
            .and_then(|i| ctx.layout.file(i))
            .ok_or_else(|| missing("spans more than one file"))
    };
    Ok(match name {
        "name" => ctx.layout.name.clone(),
        "path" => file()?.display_path(),
        "filename" => file()?.file_name().to_string(),
        "index" => ctx
            .file
            .ok_or_else(|| missing("spans more than one file"))?
            .to_string(),
        "piece" => ctx
            .piece
            .ok_or_else(|| missing("spans more than one piece"))?
            .to_string(),
        "offset" => ctx.offset.to_string(),
        "length" => ctx.length.to_string(),
        // Inclusive, so it drops straight into a `Range: bytes=` header.
        "end" => ctx
            .offset
            .saturating_add(ctx.length)
            .saturating_sub(1)
            .to_string(),
        "piece_offset" => {
            let piece = ctx
                .piece
                .ok_or_else(|| missing("spans more than one piece"))?;
            let start = ctx
                .layout
                .piece_range(piece)
                .ok_or_else(|| missing("names no piece"))?
                .start;
            ctx.offset.saturating_sub(start).to_string()
        }
        "piece_length" => ctx.layout.piece_length.to_string(),
        "infohash" => ctx.info_hash.to_string(),
        other => {
            return Err(Error::binding(format!(
                "`{{{other}}}` is not a template placeholder (use name, path, filename, index, piece, offset, length, end, piece_offset, piece_length, or infohash)"
            ))
            .with("template", template.to_string())
            .with("placeholder", other.to_string()));
        }
    })
}

/// Check that `mode` can be used with a scope resolving to `file_count` files.
///
/// `exact` sends every request to one URL, so it can only serve one file. On a
/// single-file torrent that is automatic; on a multi-file torrent the scope
/// has to narrow it down, and saying so up front is far better than every
/// piece past the first file failing its hash.
pub fn check_mode(
    mode: Mode,
    layout: &Layout,
    file_count: usize,
    selector: &str,
    url: &str,
) -> Result<()> {
    if mode == Mode::Exact && layout.multi_file && file_count != 1 {
        return Err(Error::binding(format!(
            "composition mode `exact` sends every request to one URL, but scope `{selector}` selects {file_count} files of a multi-file torrent"
        ))
        .with("mode", "exact")
        .with("selector", selector.to_string())
        .with("url", url.to_string())
        .with("file_count", file_count));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn multi() -> Layout {
        Layout::from_lengths(
            "album",
            true,
            1024,
            [
                ("disc 1/a.flac".to_string(), 1500u64),
                ("notes.nfo".to_string(), 100),
            ],
        )
    }

    fn single() -> Layout {
        Layout::from_lengths(
            "movie.mkv",
            false,
            1024,
            [("movie.mkv".to_string(), 3000u64)],
        )
    }

    const HASH: &str = "0102030405060708090a0b0c0d0e0f1011121314";

    fn url(base: &str, mode: Mode, layout: &Layout, file: usize) -> String {
        let ctx = RequestContext::for_file(layout, HASH, file).unwrap();
        compose(base, mode, None, &ctx).unwrap()
    }

    #[test]
    fn auto_appends_name_and_path_for_multi_file() {
        assert_eq!(
            url("https://e.com/pub/", Mode::Auto, &multi(), 0),
            "https://e.com/pub/album/disc%201/a.flac"
        );
        assert_eq!(
            url("https://e.com/pub", Mode::Auto, &multi(), 1),
            "https://e.com/pub/album/notes.nfo"
        );
    }

    #[test]
    fn auto_appends_the_name_only_for_a_directory_url_on_a_single_file_torrent() {
        assert_eq!(
            url("https://e.com/files/", Mode::Auto, &single(), 0),
            "https://e.com/files/movie.mkv"
        );
        assert_eq!(
            url("https://e.com/movie.mkv", Mode::Auto, &single(), 0),
            "https://e.com/movie.mkv"
        );
        assert_eq!(
            url("https://e.com/renamed.bin", Mode::Auto, &single(), 0),
            "https://e.com/renamed.bin"
        );
    }

    #[test]
    fn exact_appends_nothing() {
        assert_eq!(
            url("https://cdn.e.com/blob/a3f1", Mode::Exact, &multi(), 0),
            "https://cdn.e.com/blob/a3f1"
        );
        assert_eq!(
            url("https://cdn.e.com/blob/a3f1", Mode::Exact, &single(), 0),
            "https://cdn.e.com/blob/a3f1"
        );
    }

    #[test]
    fn prefix_appends_the_path_but_not_the_name() {
        assert_eq!(
            url("https://e.com/pub/", Mode::Prefix, &multi(), 0),
            "https://e.com/pub/disc%201/a.flac"
        );
        assert_eq!(
            url("https://e.com/pub", Mode::Prefix, &multi(), 1),
            "https://e.com/pub/notes.nfo"
        );
        assert_eq!(
            url("https://e.com/pub/", Mode::Prefix, &single(), 0),
            "https://e.com/pub/movie.mkv"
        );
    }

    #[test]
    fn path_segments_are_percent_encoded_but_separators_survive() {
        let layout = Layout::from_lengths("sé t", true, 16, [("a b/é.bin".to_string(), 10u64)]);
        assert_eq!(
            url("https://e.com/", Mode::Auto, &layout, 0),
            "https://e.com/s%C3%A9%20t/a%20b/%C3%A9.bin"
        );
    }

    #[test]
    fn characters_that_would_change_the_url_structure_are_encoded() {
        let layout = Layout::from_lengths("t", true, 16, [("a?b#c.bin".to_string(), 10u64)]);
        let composed = url("https://e.com/", Mode::Auto, &layout, 0);
        assert_eq!(composed, "https://e.com/t/a%3Fb%23c.bin");
        assert!(
            !composed.contains('?'),
            "a raw ? would start a query string"
        );
        assert!(!composed.contains('#'), "a raw # would start a fragment");
    }

    #[test]
    fn templates_expand_file_level_placeholders() {
        let layout = multi();
        let ctx = RequestContext::for_file(&layout, HASH, 0).unwrap();
        assert_eq!(
            expand("https://e.com/{raw:path}", &ctx).unwrap(),
            "https://e.com/disc 1/a.flac"
        );
        assert_eq!(
            expand("https://e.com/{filename}", &ctx).unwrap(),
            "https://e.com/a.flac"
        );
        assert_eq!(
            expand("https://e.com/{index}.bin", &ctx).unwrap(),
            "https://e.com/0.bin"
        );
        assert_eq!(
            expand("https://e.com/{name}/x", &ctx).unwrap(),
            "https://e.com/album/x"
        );
        assert_eq!(
            expand("https://e.com/{infohash}", &ctx).unwrap(),
            format!("https://e.com/{HASH}")
        );
    }

    #[test]
    fn templates_expand_request_level_placeholders() {
        let layout = multi();
        let ctx = RequestContext::for_range(&layout, HASH, 1024, 512);
        assert_eq!(
            expand("https://e.com/{piece}.bin", &ctx).unwrap(),
            "https://e.com/1.bin"
        );
        assert_eq!(
            expand("https://e.com/{offset}-{end}", &ctx).unwrap(),
            "https://e.com/1024-1535"
        );
        assert_eq!(
            expand("https://e.com/?len={length}", &ctx).unwrap(),
            "https://e.com/?len=512"
        );
        assert_eq!(
            expand("https://e.com/{piece_offset}", &ctx).unwrap(),
            "https://e.com/0"
        );
        assert_eq!(
            expand("https://e.com/{piece_length}", &ctx).unwrap(),
            "https://e.com/1024"
        );
    }

    #[test]
    fn encoding_is_the_default_and_raw_is_opt_in() {
        let layout = multi();
        let ctx = RequestContext::for_file(&layout, HASH, 0).unwrap();
        assert_eq!(expand("{path}", &ctx).unwrap(), "disc%201%2Fa.flac");
        assert_eq!(expand("{raw:path}", &ctx).unwrap(), "disc 1/a.flac");
    }

    #[test]
    fn doubled_braces_are_literal() {
        let layout = multi();
        let ctx = RequestContext::for_file(&layout, HASH, 0).unwrap();
        assert_eq!(
            expand("https://e.com/{{literal}}", &ctx).unwrap(),
            "https://e.com/{literal}"
        );
    }

    #[test]
    fn malformed_templates_are_rejected() {
        let layout = multi();
        let ctx = RequestContext::for_file(&layout, HASH, 0).unwrap();
        assert!(expand("https://e.com/{path", &ctx).is_err());
        assert!(expand("https://e.com/path}", &ctx).is_err());
        let err = expand("https://e.com/{nope}", &ctx).unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Binding);
        assert!(
            err.message().contains("is not a template placeholder"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn a_placeholder_that_cannot_be_answered_is_an_error_not_a_wrong_number() {
        let layout = multi();
        // 1400..1600 straddles the boundary between file 0 and file 1.
        let ctx = RequestContext::for_range(&layout, HASH, 1400, 200);
        assert!(ctx.file.is_none());
        let err = expand("https://e.com/{path}", &ctx).unwrap_err();
        assert!(
            err.message().contains("spans more than one file"),
            "{}",
            err.message()
        );
        // Offsets are still answerable, since they do not depend on a file.
        assert_eq!(
            expand("https://e.com/{offset}", &ctx).unwrap(),
            "https://e.com/1400"
        );
    }

    #[test]
    fn exact_is_refused_when_the_scope_holds_more_than_one_file() {
        let layout = multi();
        let err = check_mode(Mode::Exact, &layout, 2, "*", "https://e.com/x").unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Binding);
        assert_eq!(err.context()["file_count"], 2);
        assert!(check_mode(Mode::Exact, &layout, 1, "0", "https://e.com/x").is_ok());
        assert!(check_mode(Mode::Exact, &single(), 1, "*", "https://e.com/x").is_ok());
    }

    #[test]
    fn every_other_mode_accepts_any_file_count() {
        let layout = multi();
        for mode in [Mode::Auto, Mode::Prefix, Mode::Template] {
            assert!(check_mode(mode, &layout, 2, "*", "https://e.com/x").is_ok());
        }
    }

    #[test]
    fn mode_names_round_trip() {
        for mode in Mode::ALL {
            assert_eq!(Mode::parse(mode.as_str()).unwrap(), *mode);
            assert_eq!(Mode::parse(&mode.as_str().to_uppercase()).unwrap(), *mode);
        }
        assert!(Mode::parse("sideways").is_err());
        assert_eq!(Mode::default(), Mode::Auto);
    }

    #[test]
    fn only_template_varies_per_request() {
        assert!(Mode::Template.is_per_request());
        for mode in [Mode::Auto, Mode::Exact, Mode::Prefix] {
            assert!(!mode.is_per_request());
        }
    }

    #[test]
    fn template_mode_without_a_template_is_an_error() {
        let layout = multi();
        let ctx = RequestContext::for_file(&layout, HASH, 0).unwrap();
        let err = compose("https://e.com/", Mode::Template, None, &ctx).unwrap_err();
        assert!(
            err.message().contains("no template was given"),
            "{}",
            err.message()
        );
    }
}
