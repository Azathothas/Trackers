//! `file:` sources: a local path as a web seed.
//!
//! A source does not have to be remote to be useful. The bytes for a torrent
//! are often already on the disk under a different name, in a different
//! directory, or inside a completed copy of a different torrent that happens
//! to hold the same file. Naming that path as a source is how those bytes get
//! reused instead of fetched again. See `TODO/multi-source.md`, T-133, layer 1.
//!
//! Everything else about a source still applies. A `file:` source has a scope,
//! a composition, a chunk size, a rate limit, and per-piece verification, and
//! it reaches the session through the same bridge as an HTTP one. The only
//! difference is where the range comes from.
//!
//! Two things this deliberately does not do. It does not copy the file, so a
//! source pointed at the output directory of another torrent reads it in
//! place. And it does not trust it: `--web-seed-verify piece` is on by
//! default, so a local file that is not what the torrent says gets caught at
//! the source with the path named, exactly as a wrong mirror would.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Whether a URL names a local path.
pub fn is_file_url(url: &str) -> bool {
    url.len() >= 5 && url[..5].eq_ignore_ascii_case("file:")
}

/// Turn a `file:` URL into a path.
///
/// Accepts `file:///c:/dir/name.bin` and `file://localhost/c:/dir/name.bin`,
/// which are the two spellings RFC 8089 gives for a local file, plus the
/// `file:/c:/dir` shorthand every parser in practice takes. A `file://host/`
/// naming a real host is refused rather than silently read from the local
/// disk, because reading the wrong bytes is worse than failing.
///
/// Percent escapes are decoded, so a directory with a space in it works. On
/// Windows the leading `/` before a drive letter is dropped and `/` becomes
/// `\`, which is what `File::open` wants.
///
/// A `..` component is refused. `auto` and `prefix` composition append the
/// torrent's own `name` and `path` to the source URL, so the tail of it is
/// written by the `.torrent` rather than by the caller.
pub fn path_of(url: &str) -> Result<PathBuf> {
    let refuse =
        |why: &str| Err(Error::binding(format!("{url}: {why}")).with("url", url.to_string()));
    if !is_file_url(url) {
        return refuse("not a file: URL");
    }
    let rest = &url[5..];

    // `file://host/path`. An empty host and `localhost` both mean this
    // machine; anything else names a share this cannot read.
    let path = match rest.strip_prefix("//") {
        None => rest,
        Some(after) => {
            let (host, path) = match after.find('/') {
                Some(at) => after.split_at(at),
                None => (after, ""),
            };
            // `file://C:/dir` has host `C:` by the URL grammar and means drive
            // C by every intention anyone has ever had writing it. Refusing it
            // would refuse the form a Windows caller types first.
            if is_drive_letter(host) {
                return finish(url, after);
            }
            if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
                return refuse(&format!(
                    "`{host}` is a remote host; a file: source reads the local disk only"
                ));
            }
            path
        }
    };
    finish(url, path)
}

/// Whether a URL authority is really a Windows drive, as in `file://C:/dir`.
fn is_drive_letter(host: &str) -> bool {
    let bytes = host.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && (bytes[1] == b':' || bytes[1] == b'|')
}

/// Decode a URL path and turn it into a filesystem path.
fn finish(url: &str, path: &str) -> Result<PathBuf> {
    let refuse =
        |why: &str| Err(Error::binding(format!("{url}: {why}")).with("url", url.to_string()));
    // A fragment is not part of a path, and a query has no meaning for a file.
    let path = path.split(['?', '#']).next().unwrap_or(path);
    if path.is_empty() {
        return refuse("names no path");
    }

    let decoded = match percent_decode(path) {
        Some(decoded) => decoded,
        None => return refuse("has a percent escape that is not valid UTF-8"),
    };

    // `/C:/dir` is the URL spelling of `C:\dir`. The leading slash is part of
    // the URL grammar, not of the path.
    let trimmed = match starts_with_drive(decoded.strip_prefix('/').unwrap_or("")) {
        true => decoded[1..].to_string(),
        false => decoded,
    };
    if trimmed.is_empty() {
        return refuse("names no path");
    }
    // `C|` is the pre-RFC-8089 spelling of `C:`, still emitted by some tools.
    let drive_fixed = match starts_with_drive(&trimmed) {
        true => format!("{}:{}", &trimmed[..1], &trimmed[2..]),
        false => trimmed,
    };

    // A `..` never survives to a read. Most of a source URL is written by the
    // caller, who could as easily write the resolved path, but the tail of it
    // is not: `auto` and `prefix` composition append the torrent's own `name`
    // and `path`, and a hostile `.torrent` naming `../../../Windows/win.ini`
    // would otherwise make a source rooted at one directory read out of
    // another. The bytes would fail their piece hash and be discarded, but
    // reading them at all is not this tool's business.
    if drive_fixed.split('/').any(|segment| segment == "..") {
        return refuse(
            "has a `..` component; a file: source reads the path it names, so write the resolved one",
        );
    }

    Ok(PathBuf::from(
        drive_fixed.replace('/', std::path::MAIN_SEPARATOR_STR),
    ))
}

/// Whether a path starts with a Windows drive, as in `C:` or the older `C|`.
fn starts_with_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && (bytes[1] == b':' || bytes[1] == b'|')
}

/// Decode percent escapes, refusing a sequence that is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_string());
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = text.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Read one byte range out of the file a `file:` URL names.
///
/// Open, seek, and read on a blocking thread, once per call, rather than
/// through `tokio::fs`, which is three hops for the same three calls. The
/// handle is not pooled: at the default four megabyte window a one gigabyte
/// file is 256 opens, which is not what bounds this path.
///
/// The path is returned beside the outcome so a caller can name the file in an
/// error without parsing the URL a second time.
pub async fn read_range(url: &str, start: u64, length: u64) -> (PathBuf, std::io::Result<Vec<u8>>) {
    let path = match path_of(url) {
        Ok(path) => path,
        Err(e) => {
            return (
                PathBuf::from(url),
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    e.to_string(),
                )),
            );
        }
    };
    let opened = path.clone();
    let read = tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&opened)?;
        file.seek(SeekFrom::Start(start))?;
        let mut buffer = vec![0u8; length as usize];
        file.read_exact(&mut buffer)?;
        Ok(buffer)
    })
    .await;
    match read {
        Ok(outcome) => (path, outcome),
        // The blocking pool only cancels a task when the runtime is shutting
        // down, so this is the process ending rather than the file failing.
        Err(err) => (
            path,
            Err(std::io::Error::other(format!("read cancelled: {err}"))),
        ),
    }
}

/// Build a `file:` URL for a path, for the caller that has a path and needs a
/// source. The inverse of [`path_of`] for the cases either can express.
///
/// Every byte outside the unreserved set is percent-encoded, so a directory
/// with a space, a `#`, or a `%` in it round trips. Separators do not, or the
/// URL would name one long filename.
pub fn url_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    // An absolute Unix path already starts with `/`; a Windows one starts with
    // a drive letter and needs the URL's own root slash.
    if !text.starts_with('/') {
        out.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path separator this platform produces, so one assertion covers
    /// both.
    fn sep(text: &str) -> String {
        text.replace('/', std::path::MAIN_SEPARATOR_STR)
    }

    #[test]
    fn a_url_with_three_slashes_names_a_local_path() {
        assert_eq!(
            path_of("file:///tmp/payload.bin").unwrap(),
            PathBuf::from(sep("/tmp/payload.bin"))
        );
    }

    #[test]
    fn a_windows_drive_loses_the_url_root_slash() {
        assert_eq!(
            path_of("file:///C:/data/payload.bin").unwrap(),
            PathBuf::from(sep("C:/data/payload.bin"))
        );
        assert_eq!(
            path_of("file:/C:/data/payload.bin").unwrap(),
            PathBuf::from(sep("C:/data/payload.bin"))
        );
    }

    #[test]
    fn localhost_is_this_machine_and_any_other_host_is_refused() {
        assert_eq!(
            path_of("file://localhost/tmp/x.bin").unwrap(),
            PathBuf::from(sep("/tmp/x.bin"))
        );
        let err = path_of("file://fileserver/share/x.bin")
            .unwrap_err()
            .to_string();
        assert!(err.contains("remote host"), "{err}");
    }

    #[test]
    fn a_percent_escape_is_decoded() {
        assert_eq!(
            path_of("file:///tmp/disc%201/a.flac").unwrap(),
            PathBuf::from(sep("/tmp/disc 1/a.flac"))
        );
    }

    #[test]
    fn a_percent_escape_that_is_not_utf8_is_refused() {
        let err = path_of("file:///tmp/%FF%FE").unwrap_err().to_string();
        assert!(err.contains("valid UTF-8"), "{err}");
    }

    #[test]
    fn a_url_with_no_path_is_refused() {
        assert!(path_of("file://").is_err());
        assert!(path_of("file:").is_err());
    }

    #[test]
    fn the_root_is_a_path_because_a_composition_appends_to_it() {
        // `file:///` with `auto` composition is `<root>/<name>/<path>`, which
        // is a legitimate source. Refusing it here would refuse that.
        assert_eq!(
            path_of("file:///").unwrap(),
            PathBuf::from(std::path::MAIN_SEPARATOR_STR)
        );
    }

    #[test]
    fn a_scheme_that_is_not_file_is_refused() {
        assert!(path_of("https://example.com/x").is_err());
    }

    #[test]
    fn the_scheme_is_matched_without_regard_to_case() {
        assert!(is_file_url("FILE:///tmp/x"));
        assert!(is_file_url("File:///tmp/x"));
        assert!(!is_file_url("files://host/x"));
        assert!(!is_file_url("http://x/"));
    }

    #[test]
    fn a_query_or_fragment_is_not_part_of_the_path() {
        assert_eq!(
            path_of("file:///tmp/x.bin?v=2").unwrap(),
            PathBuf::from(sep("/tmp/x.bin"))
        );
        assert_eq!(
            path_of("file:///tmp/x.bin#top").unwrap(),
            PathBuf::from(sep("/tmp/x.bin"))
        );
    }

    #[test]
    fn a_path_round_trips_through_a_url() {
        for original in ["/tmp/payload.bin", "/tmp/disc 1/a.flac", "/tmp/100%/x.bin"] {
            let path = PathBuf::from(sep(original));
            let url = url_of(&path);
            assert_eq!(path_of(&url).unwrap(), path, "{url}");
        }
    }

    #[test]
    fn a_url_keeps_its_separators_and_encodes_everything_else() {
        let url = url_of(Path::new(&sep("/tmp/disc 1/a.flac")));
        assert_eq!(url, "file:///tmp/disc%201/a.flac");
    }
    #[test]
    fn a_drive_letter_in_the_authority_is_a_drive_and_not_a_host() {
        // `file://C:/dir` is what a Windows caller writes first, and there is
        // no other thing it could mean.
        assert_eq!(
            path_of("file://C:/data/payload.bin").unwrap(),
            PathBuf::from(sep("C:/data/payload.bin"))
        );
        assert_eq!(
            path_of("file://c|/data/payload.bin").unwrap(),
            PathBuf::from(sep("c:/data/payload.bin"))
        );
        assert_eq!(
            path_of("file:///c|/data/payload.bin").unwrap(),
            PathBuf::from(sep("c:/data/payload.bin"))
        );
    }

    #[test]
    fn a_dot_dot_component_is_refused_wherever_it_appears() {
        // Composition appends the torrent's own name and path to the base, so
        // a hostile `.torrent` decides the tail of this URL.
        for url in [
            "file:///srv/mirror/../../Windows/win.ini",
            "file:///srv/../etc/passwd",
            "file://C:/srv/../secrets.txt",
            "file:///srv/%2E%2E/secrets.txt",
        ] {
            let err = path_of(url).unwrap_err().to_string();
            assert!(err.contains("`..` component"), "{url}: {err}");
        }
    }

    #[test]
    fn a_name_that_merely_starts_with_dots_is_not_a_traversal() {
        assert_eq!(
            path_of("file:///srv/...hidden/..data.bin").unwrap(),
            PathBuf::from(sep("/srv/...hidden/..data.bin"))
        );
    }
}
