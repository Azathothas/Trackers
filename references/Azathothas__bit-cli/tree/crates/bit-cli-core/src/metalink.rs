//! Reading a Metalink: a mirror list, a torrent, and checksums in one file.
//!
//! Metalink is in scope because it is a torrent format. One document carries a
//! `.torrent`, a list of HTTP mirrors for the same bytes, and checksums over
//! the whole file, which is exactly the hybrid this tool exists for.
//! Everything a caller would otherwise assemble with `--web-seed` repeated
//! twelve times, a Metalink gives in one file.
//!
//! Two versions are read, because both are in circulation and a caller with a
//! file does not care which they were handed.
//!
//! **Metalink 4, RFC 5854**, extension `.meta4`:
//!
//! ```xml
//! <metalink xmlns="urn:ietf:params:xml:ns:metalink">
//!   <file name="example.iso">
//!     <size>14471447</size>
//!     <hash type="sha-256">3d6fece8...</hash>
//!     <url priority="1">https://mirror-a.example.com/example.iso</url>
//!     <metaurl mediatype="torrent">https://example.com/example.iso.torrent</metaurl>
//!   </file>
//! </metalink>
//! ```
//!
//! **Metalink 3**, extension `.metalink`: the same facts one level deeper,
//! under `<files>`, with the hashes under `<verification>`, the URLs under
//! `<resources>`, and the torrent as a `<url type="bittorrent">` rather than a
//! `<metaurl>`. Priority is spelled `preference` and runs the other way: in
//! version 3 a **higher** number is preferred, in version 4 a **lower** one is.
//! Both are normalised here to version 4's rule, so a caller sorting by
//! `priority` gets the document's intent either way.
//!
//! # What is read and what is not
//!
//! Read: file name, size, hashes, HTTP and HTTPS mirrors, and the torrent.
//! Not read, and deliberately: `<pieces>`, because the torrent carries piece
//! hashes and two sets of them that disagree is a problem to detect rather
//! than a feature to support; `<languages>`, `<os>`, and `<countries>`, because
//! filtering by them is a mirror-choosing policy and this tool takes its
//! mirrors from the caller; and `<signature>`, because verifying an OpenPGP
//! signature needs a keyring and a trust model that nothing here has.
//!
//! `ftp:` and `ftps:` mirrors are parsed and kept out of the source list, with
//! the count reported, because `bit-cli` speaks HTTP and a source it cannot
//! fetch from is worse than one it never had.
//!
//! # Why parse rather than deserialize
//!
//! `quick-xml`'s `serialize` feature would map this onto structs, and the two
//! versions would need two sets of them plus a probe to decide which. Pulling
//! events is one pass that handles both, ignores namespace prefixes, and does
//! not fail on an element it has never heard of, which matters for a format
//! whose files are written by many tools.
//!
//! See `TODO/cli-surface.md`, T-113.

use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;

use crate::error::{Error, Result, from_io};
use crate::layout::{Layout, LayoutFile};

/// One checksum over a whole file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    /// The algorithm, lowercased and with the hyphen removed: `sha256`,
    /// `sha1`, `md5`. Version 4 writes `sha-256` and version 3 writes
    /// `sha256`, and they mean the same thing.
    pub algorithm: String,
    /// The digest, lowercase hex.
    pub value: String,
}

impl Checksum {
    /// Whether this is an algorithm that can actually be checked here.
    pub fn is_supported(&self) -> bool {
        matches!(self.algorithm.as_str(), "sha256" | "sha1" | "md5")
    }

    /// Hash a file with this checksum's algorithm and compare.
    ///
    /// Streamed in fixed-size reads, so peak memory does not depend on the
    /// payload size: the file this checks is the file that was just
    /// downloaded, and it can be an ISO.
    ///
    /// An unsupported algorithm is an error rather than a pass. A checksum
    /// that was not computed is not a checksum that matched, and returning
    /// "fine" for one is the quiet wrong answer this repository keeps finding.
    pub fn verify_file(&self, path: &Path) -> Result<Verified> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| from_io(e, format!("cannot read {}", path.display())))?;
        let mut digest = crate::digest::Digest::new(&self.algorithm).map_err(|_| {
            Error::usage(format!(
                "the metalink's checksum uses {}, which this cannot compute",
                self.algorithm
            ))
            .with("algorithm", self.algorithm.clone())
        })?;
        let mut buffer = vec![0u8; 256 * 1024];
        let mut bytes_hashed = 0u64;
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|e| from_io(e, format!("cannot read {}", path.display())))?;
            if n == 0 {
                break;
            }
            bytes_hashed += n as u64;
            digest.update(&buffer[..n]);
        }
        let actual = digest.finish();
        Ok(Verified {
            algorithm: self.algorithm.clone(),
            expected: self.value.clone(),
            matched: actual == self.value,
            actual,
            bytes_hashed,
        })
    }
}

/// What hashing a file with one of a Metalink's checksums found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The algorithm, in this module's normalised spelling.
    pub algorithm: String,
    /// What the document said, lowercase hex.
    pub expected: String,
    /// What the bytes on disk hash to, lowercase hex.
    pub actual: String,
    pub matched: bool,
    /// Bytes read. A caller comparing this against the size the torrent
    /// declares learns whether the two documents describe the same file at
    /// all, which is a different failure from a digest that disagrees.
    pub bytes_hashed: u64,
}

/// One place the bytes can be fetched from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    pub url: String,
    /// Lower is preferred, as in RFC 5854. A version 3 `preference` is
    /// converted, so this always reads the same way.
    pub priority: Option<u32>,
    /// The `location` attribute, a two letter country code, when present.
    /// Reported and not acted on.
    pub location: Option<String>,
}

/// One `<file>` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetalinkFile {
    /// The `name` attribute. Untrusted: it is a path from a downloaded
    /// document and goes through [`crate::paths::plan_one`] before it is used.
    pub name: String,
    pub size: Option<u64>,
    pub checksums: Vec<Checksum>,
    /// HTTP and HTTPS mirrors, in document order.
    pub mirrors: Vec<Mirror>,
    /// Torrents for this file: `<metaurl mediatype="torrent">` in version 4,
    /// `<url type="bittorrent">` in version 3.
    pub torrents: Vec<Mirror>,
    /// Mirrors that were listed and dropped because their scheme is not one
    /// this tool speaks. Counted rather than kept, so the report can say the
    /// document had more in it than the run used.
    pub unsupported_mirrors: Vec<String>,
}

impl MetalinkFile {
    /// The checksum a caller should verify against, strongest first.
    ///
    /// A document may carry several. SHA-256 beats SHA-1 beats MD5, and an
    /// algorithm this cannot compute is not returned at all: reporting a
    /// checksum nothing will check is a claim of verification that did not
    /// happen.
    pub fn best_checksum(&self) -> Option<&Checksum> {
        for want in ["sha256", "sha1", "md5"] {
            if let Some(found) = self
                .checksums
                .iter()
                .find(|c| c.algorithm == want && c.is_supported())
            {
                return Some(found);
            }
        }
        None
    }

    /// Mirrors in the order the document prefers them.
    ///
    /// Stable within a priority, so two mirrors at the same priority stay in
    /// document order and the result is the same on every run.
    pub fn mirrors_by_priority(&self) -> Vec<&Mirror> {
        by_priority(&self.mirrors)
    }

    /// Torrents in the order the document prefers them.
    ///
    /// A document may list several, which is a mirror list for the `.torrent`
    /// itself. Sorted the same way the payload mirrors are, so a caller that
    /// falls through to the second one on a failure is following the
    /// document's own preference rather than its element order.
    pub fn torrents_by_priority(&self) -> Vec<&Mirror> {
        by_priority(&self.torrents)
    }

    /// Which file in a torrent this entry describes, and whether the two
    /// documents agree about it.
    ///
    /// This runs before anything is fetched and costs nothing: it compares
    /// what the Metalink claims against what the `.torrent` claims. A size
    /// that disagrees means the two describe different bytes, and learning
    /// that before the download beats learning it after.
    pub fn agreement(&self, layout: &Layout) -> Agreement {
        let (file_index, matched_by) = self.match_file(layout);
        let torrent_size = file_index.map(|i| layout.files[i].length);
        let size_agrees = match (self.size, torrent_size) {
            (Some(a), Some(b)) => Some(a == b),
            _ => None,
        };
        Agreement {
            file_index,
            matched_by,
            metalink_size: self.size,
            torrent_size,
            size_agrees,
        }
    }

    /// The file index this entry names, by the first rule that matches exactly
    /// one file.
    ///
    /// Three rules, weakest last. A single-file torrent is the whole of the
    /// common case and needs no name at all. Otherwise the `name` attribute is
    /// matched against the torrent's own path for a file, with and without the
    /// torrent name in front of it, and then against the last component alone.
    /// A rule that matches more than one file stops the search rather than
    /// picking one: attributing a checksum to the wrong file would report a
    /// mismatch against a file the document never mentioned.
    fn match_file(&self, layout: &Layout) -> (Option<usize>, &'static str) {
        if layout.files.len() == 1 {
            return (Some(0), "only_file");
        }
        let wanted = self.name.replace('\\', "/");
        let wanted = wanted.trim_start_matches('/');
        if wanted.is_empty() {
            return (None, "no_name");
        }
        let rules: [MatchRule<'_>; 3] = [
            ("path", &|f: &LayoutFile| f.display_path() == wanted),
            // A document may spell the path with the torrent's own name in
            // front of it, because that is the path the file lands at on disk.
            ("prefixed_path", &|f: &LayoutFile| {
                format!("{}/{}", layout.name, f.display_path()) == wanted
            }),
            ("file_name", &|f: &LayoutFile| {
                Some(f.file_name()) == wanted.rsplit('/').next()
            }),
        ];
        for (how, matches) in rules {
            let mut found = layout.files.iter().enumerate().filter(|(_, f)| matches(f));
            if let Some((index, _)) = found.next() {
                return match found.next() {
                    None => (Some(index), how),
                    Some(_) => (None, "ambiguous"),
                };
            }
        }
        (None, "no_match")
    }
}

/// One way of deciding whether a file in the torrent is the one a Metalink
/// entry names: a label for the report, and the test itself.
type MatchRule<'a> = (&'static str, &'a dyn Fn(&LayoutFile) -> bool);

/// Mirrors sorted by the document's preference, stable within a priority.
fn by_priority(mirrors: &[Mirror]) -> Vec<&Mirror> {
    let mut out: Vec<&Mirror> = mirrors.iter().collect();
    // `u32::MAX` for an absent priority, so a document that gives some mirrors
    // a priority and not others puts the unrated ones last rather than first.
    out.sort_by_key(|m| m.priority.unwrap_or(u32::MAX));
    out
}

/// What a Metalink and a `.torrent` say about the same file, compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreement {
    /// File index in the torrent, when the entry could be attributed to one.
    pub file_index: Option<usize>,
    /// Which rule matched, or why none did: `only_file`, `path`,
    /// `prefixed_path`, `file_name`, `ambiguous`, `no_match`, `no_name`.
    pub matched_by: &'static str,
    /// The size the Metalink declares, when it declares one.
    pub metalink_size: Option<u64>,
    /// The length the torrent declares for the matched file.
    pub torrent_size: Option<u64>,
    /// Whether the two sizes are equal. `None` when either is missing, which
    /// is not agreement and is not disagreement.
    pub size_agrees: Option<bool>,
}

impl Agreement {
    /// Whether the two documents state something that cannot both be true.
    pub fn disagrees(&self) -> bool {
        self.size_agrees == Some(false)
    }
}

/// Which spelling of the format a document turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// RFC 5854, `.meta4`.
    V4,
    /// Metalink 3, `.metalink`.
    V3,
}

impl Version {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V4 => "4",
            Self::V3 => "3",
        }
    }
}

/// A parsed Metalink document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metalink {
    pub version: Version,
    pub files: Vec<MetalinkFile>,
}

impl Metalink {
    /// Read a Metalink from disk.
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| crate::error::from_io(e, format!("cannot read {}", path.display())))?;
        Self::parse(&bytes).map_err(|e| e.with("path", path.display().to_string()))
    }

    /// Parse a Metalink from bytes.
    ///
    /// One pass over the events, so both versions are handled by the same
    /// code: the elements that matter have different depths and different
    /// parents in the two, and none of the names collide.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut reader = quick_xml::Reader::from_reader(bytes);
        reader.config_mut().trim_text(true);
        // A truncated metalink is a mirror list that may be missing mirrors,
        // and accepting one silently is the "wrong answer rather than no
        // answer" failure. `check_end_names` catches a closing tag that does
        // not match; it does not catch a document that simply stops, because
        // that is an EOF and not a mismatch. The depth counter below is what
        // catches that one.
        reader.config_mut().check_end_names = true;

        let mut version = None;
        let mut files: Vec<MetalinkFile> = Vec::new();
        // The element whose text we are collecting, and what to do with it.
        let mut pending: Option<Pending> = None;
        let mut depth: i64 = 0;
        // The depth `<pieces>` opened at, while it is open. Version 4 writes
        // its per-piece hashes as bare `<hash>` children with no attributes at
        // all, so nothing on the child says it is a piece hash; only its
        // parent does. Version 3 marks each child with `piece="N"` instead,
        // and both are dropped. See the test against a real MirrorBrain
        // document.
        let mut pieces_at: Option<i64> = None;
        let mut buf = Vec::new();

        loop {
            let event = reader.read_event_into(&mut buf).map_err(|e| {
                Error::usage(format!("the metalink is not valid XML: {e}"))
                    .with("source_kind", "metalink")
            })?;
            match event {
                Event::Eof => {
                    if depth > 0 {
                        return Err(Error::usage(format!(
                            "the metalink is not valid XML: it ends with {depth} element(s) still open, so it is truncated"
                        ))
                        .with("source_kind", "metalink"));
                    }
                    break;
                }
                // `Empty` is `<url/>`, which has no text, so it can open a
                // `<file>` but must never leave a `Pending` behind: there
                // would be no text and no `End`, and the next element's text
                // would be applied to it.
                Event::Empty(start) => {
                    let name = local_name(start.name().as_ref());
                    match name.as_str() {
                        "metalink" => version = Some(detect_version(&start)),
                        "file" => files.push(MetalinkFile {
                            name: attribute(&start, "name").unwrap_or_default(),
                            ..Default::default()
                        }),
                        _ => {}
                    }
                    pending = None;
                }
                Event::Start(start) => {
                    depth += 1;
                    let name = local_name(start.name().as_ref());
                    match name.as_str() {
                        "metalink" => {
                            version = Some(detect_version(&start));
                        }
                        "file" => {
                            files.push(MetalinkFile {
                                name: attribute(&start, "name").unwrap_or_default(),
                                ..Default::default()
                            });
                        }
                        "size" => pending = Some(Pending::Size),
                        "pieces" => pieces_at = pieces_at.or(Some(depth)),
                        "hash" => {
                            // A per-piece hash is not a whole-file checksum
                            // and must not be collected as if it were. Being
                            // inside `<pieces>` is what says so in version 4,
                            // where the child carries no attributes; version 3
                            // marks the child with `piece="N"` as well.
                            match pieces_at.is_some() || attribute(&start, "piece").is_some() {
                                true => pending = Some(Pending::Ignore),
                                false => {
                                    pending = Some(Pending::Hash(
                                        attribute(&start, "type").unwrap_or_default(),
                                    ));
                                }
                            }
                        }
                        "url" => {
                            pending = Some(Pending::Url {
                                priority: priority_of(&start),
                                location: attribute(&start, "location"),
                                // Version 3 marks the torrent with
                                // `type="bittorrent"` on an ordinary `<url>`.
                                torrent: attribute(&start, "type")
                                    .is_some_and(|t| t.eq_ignore_ascii_case("bittorrent")),
                                meta: false,
                            });
                        }
                        "metaurl" => {
                            pending = Some(Pending::Url {
                                priority: priority_of(&start),
                                location: attribute(&start, "location"),
                                // A `<metaurl>` may point at another metalink
                                // or at a torrent. Only the torrent is used.
                                torrent: attribute(&start, "mediatype")
                                    .is_some_and(|t| t.eq_ignore_ascii_case("torrent")),
                                meta: true,
                            });
                        }
                        _ => {}
                    }
                }
                Event::Text(text) => {
                    let Some(what) = pending.take() else { continue };
                    let value = text
                        .decode()
                        .map_err(|e| {
                            Error::usage(format!("the metalink has undecodable text: {e}"))
                                .with("source_kind", "metalink")
                        })?
                        .trim()
                        .to_string();
                    let Some(file) = files.last_mut() else {
                        continue;
                    };
                    apply(file, what, &value);
                }
                Event::End(_) => {
                    if pieces_at == Some(depth) {
                        pieces_at = None;
                    }
                    depth -= 1;
                    pending = None;
                }
                _ => {}
            }
            buf.clear();
        }

        let Some(version) = version else {
            return Err(Error::usage(
                "the metalink has no <metalink> element, so it is not a metalink",
            )
            .with("source_kind", "metalink"));
        };
        if files.is_empty() {
            return Err(Error::usage("the metalink lists no files").with("source_kind", "metalink"));
        }
        Ok(Self { version, files })
    }

    /// The one file a download is about, and an error naming the rest when
    /// there is more than one.
    ///
    /// Multi-file Metalinks exist and are a different shape of run: several
    /// independent downloads rather than one. Refusing with the list beats
    /// silently taking the first, which would download one of several files
    /// and report success.
    pub fn single_file(&self) -> Result<&MetalinkFile> {
        match self.files.len() {
            1 => Ok(&self.files[0]),
            n => {
                let names: Vec<&str> = self.files.iter().map(|f| f.name.as_str()).collect();
                Err(Error::usage(format!(
                    "the metalink lists {n} files and this reads one: {}",
                    names.join(", ")
                ))
                .with("source_kind", "metalink")
                .with("files", n.to_string()))
            }
        }
    }
}

/// What the text of the element currently open should become.
enum Pending {
    Size,
    Hash(String),
    Url {
        priority: Option<u32>,
        location: Option<String>,
        torrent: bool,
        /// Whether this came from `<metaurl>` rather than `<url>`. A
        /// `<metaurl>` names another document, not the bytes, so one that is
        /// not a torrent is dropped rather than registered as a mirror.
        meta: bool,
    },
    /// Text that is read and discarded, so it cannot be mistaken for the value
    /// of an enclosing element.
    Ignore,
}

fn apply(file: &mut MetalinkFile, what: Pending, value: &str) {
    match what {
        Pending::Ignore => {}
        Pending::Size => file.size = value.parse().ok(),
        Pending::Hash(algorithm) => {
            let algorithm = algorithm.to_ascii_lowercase().replace('-', "");
            if !algorithm.is_empty() && !value.is_empty() {
                file.checksums.push(Checksum {
                    algorithm,
                    value: value.to_ascii_lowercase(),
                });
            }
        }
        Pending::Url {
            priority,
            location,
            torrent,
            meta,
        } => {
            if value.is_empty() {
                return;
            }
            let mirror = Mirror {
                url: value.to_string(),
                priority,
                location,
            };
            if torrent {
                file.torrents.push(mirror);
                return;
            }
            // A `<metaurl>` that is not a torrent names another document
            // rather than the bytes. Following it is a different feature with
            // its own loop problem, and registering it as a mirror would point
            // a source at a metalink and serve XML as payload.
            if meta {
                return;
            }
            let lower = value.to_ascii_lowercase();
            match lower.starts_with("http://") || lower.starts_with("https://") {
                true => file.mirrors.push(mirror),
                false => file.unsupported_mirrors.push(value.to_string()),
            }
        }
    }
}

/// Which version a `<metalink>` element declares.
///
/// Version 3 carries `version="3.0"` and the metalinker.org namespace; version
/// 4 carries neither and the IETF namespace. Either signal is enough, and the
/// attribute is checked first because it is the one a hand-written file is
/// most likely to have.
fn detect_version(start: &quick_xml::events::BytesStart<'_>) -> Version {
    if let Some(declared) = attribute(start, "version")
        && declared.starts_with('3')
    {
        return Version::V3;
    }
    match attribute(start, "xmlns") {
        Some(ns) if ns.contains("metalinker.org") => Version::V3,
        _ => Version::V4,
    }
}

/// The document's preference for a URL, normalised to "lower is preferred".
///
/// Version 4 says `priority`, 1 to 999999, lower first. Version 3 says
/// `preference`, 0 to 100, **higher** first. A caller reading `priority` on
/// the result should not have to know which file it came from.
fn priority_of(start: &quick_xml::events::BytesStart<'_>) -> Option<u32> {
    if let Some(priority) = attribute(start, "priority").and_then(|v| v.parse::<u32>().ok()) {
        return Some(priority);
    }
    attribute(start, "preference")
        .and_then(|v| v.parse::<u32>().ok())
        .map(|preference| 100u32.saturating_sub(preference.min(100)))
}

/// An element name without its namespace prefix.
fn local_name(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    match text.rsplit_once(':') {
        Some((_, local)) => local.to_ascii_lowercase(),
        None => text.to_ascii_lowercase(),
    }
}

/// One attribute by local name, ignoring any namespace prefix.
fn attribute(start: &quick_xml::events::BytesStart<'_>, want: &str) -> Option<String> {
    start.attributes().flatten().find_map(|attribute| {
        (local_name(attribute.key.as_ref()) == want).then(|| {
            String::from_utf8_lossy(attribute.value.as_ref())
                .trim()
                .to_string()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const V4: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <published>2009-05-15T12:23:23Z</published>
  <file name="example.iso">
    <size>14471447</size>
    <hash type="sha-256">3d6fece8033d146d8611eab4f032df3c2ecf0b1a04a2b12dcfc7c4d1c1cf01ab</hash>
    <hash type="md5">badbadbadbadbadbadbadbadbadbadba</hash>
    <url location="de" priority="2">https://mirror-de.example.com/example.iso</url>
    <url location="us" priority="1">https://mirror-us.example.com/example.iso</url>
    <url>ftp://ftp.example.com/example.iso</url>
    <metaurl mediatype="torrent" priority="1">https://example.com/example.iso.torrent</metaurl>
  </file>
</metalink>"#;

    const V3: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="example.iso">
      <size>14471447</size>
      <verification>
        <hash type="sha256">3d6fece8033d146d8611eab4f032df3c2ecf0b1a04a2b12dcfc7c4d1c1cf01ab</hash>
        <pieces length="262144" type="sha1">
          <hash piece="0">0000000000000000000000000000000000000000</hash>
          <hash piece="1">1111111111111111111111111111111111111111</hash>
        </pieces>
      </verification>
      <resources>
        <url type="http" location="de" preference="90">http://mirror-de.example.com/example.iso</url>
        <url type="http" location="us" preference="95">http://mirror-us.example.com/example.iso</url>
        <url type="bittorrent" preference="100">http://example.com/example.iso.torrent</url>
      </resources>
    </file>
  </files>
</metalink>"#;

    #[test]
    fn a_version_four_document_yields_its_mirrors_torrent_and_checksums() {
        let metalink = Metalink::parse(V4.as_bytes()).unwrap();
        assert_eq!(metalink.version, Version::V4);
        let file = metalink.single_file().unwrap();
        assert_eq!(file.name, "example.iso");
        assert_eq!(file.size, Some(14_471_447));
        assert_eq!(file.mirrors.len(), 2, "the ftp mirror is not a mirror here");
        assert_eq!(
            file.unsupported_mirrors,
            ["ftp://ftp.example.com/example.iso"]
        );
        assert_eq!(file.torrents.len(), 1);
        assert_eq!(
            file.torrents[0].url,
            "https://example.com/example.iso.torrent"
        );
        assert_eq!(file.checksums.len(), 2);
        assert_eq!(file.best_checksum().unwrap().algorithm, "sha256");
    }

    #[test]
    fn a_version_three_document_yields_the_same_facts() {
        let metalink = Metalink::parse(V3.as_bytes()).unwrap();
        assert_eq!(metalink.version, Version::V3);
        let file = metalink.single_file().unwrap();
        assert_eq!(file.name, "example.iso");
        assert_eq!(file.size, Some(14_471_447));
        assert_eq!(file.mirrors.len(), 2);
        assert_eq!(file.torrents.len(), 1);
        assert_eq!(
            file.torrents[0].url,
            "http://example.com/example.iso.torrent"
        );
    }

    /// The per-piece hashes under `<pieces>` are not whole-file checksums.
    ///
    /// Collecting them would give a file four checksums, two of which are
    /// twenty bytes of one piece, and `best_checksum` would still say sha256
    /// while `checksums` reported nonsense to anyone reading it.
    #[test]
    fn per_piece_hashes_are_not_collected_as_file_checksums() {
        let metalink = Metalink::parse(V3.as_bytes()).unwrap();
        let file = metalink.single_file().unwrap();
        assert_eq!(file.checksums.len(), 1, "{:?}", file.checksums);
        assert_eq!(file.checksums[0].algorithm, "sha256");
        assert_eq!(
            file.checksums[0].value,
            "3d6fece8033d146d8611eab4f032df3c2ecf0b1a04a2b12dcfc7c4d1c1cf01ab"
        );
    }

    /// Version 3 counts preference upwards and version 4 counts priority
    /// downwards, and a caller should not have to know which file it read.
    #[test]
    fn preference_and_priority_both_come_out_lowest_first() {
        let v4 = Metalink::parse(V4.as_bytes()).unwrap();
        let ordered: Vec<&str> = v4.files[0]
            .mirrors_by_priority()
            .iter()
            .map(|m| m.url.as_str())
            .collect();
        assert_eq!(
            ordered,
            [
                "https://mirror-us.example.com/example.iso",
                "https://mirror-de.example.com/example.iso"
            ],
            "priority 1 before priority 2"
        );

        let v3 = Metalink::parse(V3.as_bytes()).unwrap();
        let ordered: Vec<&str> = v3.files[0]
            .mirrors_by_priority()
            .iter()
            .map(|m| m.url.as_str())
            .collect();
        assert_eq!(
            ordered,
            [
                "http://mirror-us.example.com/example.iso",
                "http://mirror-de.example.com/example.iso"
            ],
            "preference 95 before preference 90, which is the other way round"
        );
    }

    /// `sha-256` and `sha256` are the same algorithm spelled two ways.
    #[test]
    fn hash_type_spellings_are_normalised() {
        let v4 = Metalink::parse(V4.as_bytes()).unwrap();
        let v3 = Metalink::parse(V3.as_bytes()).unwrap();
        assert_eq!(
            v4.files[0].best_checksum(),
            v3.files[0].best_checksum(),
            "the same digest under two spellings of its algorithm"
        );
    }

    #[test]
    fn the_strongest_checksum_wins() {
        let file = MetalinkFile {
            checksums: vec![
                Checksum {
                    algorithm: "md5".into(),
                    value: "a".into(),
                },
                Checksum {
                    algorithm: "sha1".into(),
                    value: "b".into(),
                },
                Checksum {
                    algorithm: "sha256".into(),
                    value: "c".into(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(file.best_checksum().unwrap().value, "c");
    }

    /// An algorithm nothing here can compute is not offered as one that will
    /// be checked.
    #[test]
    fn an_unsupported_algorithm_is_not_returned_as_the_best() {
        let file = MetalinkFile {
            checksums: vec![Checksum {
                algorithm: "sha512".into(),
                value: "a".into(),
            }],
            ..Default::default()
        };
        assert!(file.best_checksum().is_none());
    }

    #[test]
    fn a_document_with_several_files_is_refused_with_their_names() {
        let text = r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink">
          <file name="a.iso"><url>https://e.example.com/a</url></file>
          <file name="b.iso"><url>https://e.example.com/b</url></file>
        </metalink>"#;
        let metalink = Metalink::parse(text.as_bytes()).unwrap();
        assert_eq!(metalink.files.len(), 2);
        let error = metalink.single_file().unwrap_err().to_string();
        assert!(
            error.contains("a.iso") && error.contains("b.iso"),
            "{error}"
        );
    }

    #[test]
    fn a_file_entry_keeps_its_own_mirrors() {
        let text = r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink">
          <file name="a.iso"><url>https://e.example.com/a</url></file>
          <file name="b.iso"><url>https://e.example.com/b1</url><url>https://e.example.com/b2</url></file>
        </metalink>"#;
        let metalink = Metalink::parse(text.as_bytes()).unwrap();
        assert_eq!(metalink.files[0].mirrors.len(), 1);
        assert_eq!(metalink.files[1].mirrors.len(), 2);
        assert_eq!(metalink.files[1].mirrors[1].url, "https://e.example.com/b2");
    }

    #[test]
    fn a_namespace_prefix_does_not_hide_an_element() {
        let text = r#"<ml:metalink xmlns:ml="urn:ietf:params:xml:ns:metalink">
          <ml:file name="a.iso">
            <ml:size>7</ml:size>
            <ml:url>https://e.example.com/a</ml:url>
          </ml:file>
        </ml:metalink>"#;
        let metalink = Metalink::parse(text.as_bytes()).unwrap();
        let file = metalink.single_file().unwrap();
        assert_eq!(file.size, Some(7));
        assert_eq!(file.mirrors.len(), 1);
    }

    #[test]
    fn something_that_is_not_a_metalink_is_refused() {
        let error = Metalink::parse(b"<rss><channel/></rss>").unwrap_err();
        assert!(error.to_string().contains("not a metalink"), "{error}");
    }

    #[test]
    fn a_metalink_with_no_files_is_refused() {
        let error =
            Metalink::parse(br#"<metalink xmlns="urn:ietf:params:xml:ns:metalink"></metalink>"#)
                .unwrap_err();
        assert!(error.to_string().contains("no files"), "{error}");
    }

    /// A document that stops in the middle is refused rather than accepted
    /// with whatever it had got to.
    ///
    /// A truncated metalink is a mirror list missing mirrors, and this format
    /// exists to carry a mirror list. Taking the first half is the "plausible
    /// wrong answer" failure that this repository keeps finding.
    #[test]
    fn a_truncated_document_is_a_usage_error_rather_than_half_a_mirror_list() {
        let text = r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink">
          <file name="a.iso"><url>https://e.example.com/a</url>"#;
        let error = Metalink::parse(text.as_bytes()).unwrap_err();
        assert!(error.to_string().contains("not valid XML"), "{error}");
    }

    #[test]
    fn malformed_xml_is_a_usage_error_rather_than_a_panic() {
        let error = Metalink::parse(b"<metalink <<< />").unwrap_err();
        assert!(!error.to_string().is_empty());
    }

    /// A `<metaurl>` that is not a torrent is not treated as one.
    ///
    /// RFC 5854 allows a `<metaurl>` pointing at another metalink, and
    /// following that would be a different feature with its own loop problem.
    #[test]
    fn a_metaurl_that_is_not_a_torrent_is_left_alone() {
        let text = r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink">
          <file name="a.iso">
            <metaurl mediatype="metalink">https://e.example.com/other.meta4</metaurl>
            <metaurl mediatype="torrent">https://e.example.com/a.torrent</metaurl>
          </file>
        </metalink>"#;
        let file = Metalink::parse(text.as_bytes()).unwrap();
        let file = file.single_file().unwrap();
        assert_eq!(file.torrents.len(), 1);
        assert_eq!(file.torrents[0].url, "https://e.example.com/a.torrent");
        assert!(file.mirrors.is_empty(), "a metaurl is not a mirror");
    }

    fn layout(name: &str, files: &[(&str, u64)]) -> Layout {
        let mut offset = 0;
        let files: Vec<LayoutFile> = files
            .iter()
            .map(|(path, length)| {
                let file = LayoutFile::new(path, offset, *length);
                offset += *length;
                file
            })
            .collect();
        Layout {
            name: name.to_string(),
            multi_file: files.len() > 1,
            files,
            piece_length: 16384,
            total_length: offset,
        }
    }

    fn entry(name: &str, size: Option<u64>) -> MetalinkFile {
        MetalinkFile {
            name: name.to_string(),
            size,
            ..Default::default()
        }
    }

    /// The overwhelming case: one file in the document, one in the torrent.
    #[test]
    fn a_single_file_torrent_needs_no_name_to_be_matched() {
        let agreement = entry("whatever-the-document-called-it", Some(64))
            .agreement(&layout("a.iso", &[("a.iso", 64)]));
        assert_eq!(agreement.file_index, Some(0));
        assert_eq!(agreement.matched_by, "only_file");
        assert_eq!(agreement.size_agrees, Some(true));
        assert!(!agreement.disagrees());
    }

    /// The size is the one cross-check that costs nothing and runs before the
    /// download. Two documents that disagree about how long the file is
    /// describe different bytes.
    #[test]
    fn a_size_that_disagrees_with_the_torrent_is_reported_before_anything_is_fetched() {
        let agreement = entry("a.iso", Some(65)).agreement(&layout("a.iso", &[("a.iso", 64)]));
        assert_eq!(agreement.metalink_size, Some(65));
        assert_eq!(agreement.torrent_size, Some(64));
        assert_eq!(agreement.size_agrees, Some(false));
        assert!(agreement.disagrees());
    }

    /// A document with no `<size>` is neither agreement nor disagreement, and
    /// must not be reported as either.
    #[test]
    fn a_missing_size_is_not_agreement() {
        let agreement = entry("a.iso", None).agreement(&layout("a.iso", &[("a.iso", 64)]));
        assert_eq!(agreement.size_agrees, None);
        assert!(!agreement.disagrees());
    }

    #[test]
    fn a_multi_file_torrent_is_matched_by_path_then_by_name() {
        let layout = layout("rel", &[("docs/a.txt", 4), ("b.iso", 64)]);
        assert_eq!(entry("b.iso", None).agreement(&layout).matched_by, "path");
        assert_eq!(
            entry("rel/docs/a.txt", None).agreement(&layout).matched_by,
            "prefixed_path"
        );
        assert_eq!(
            entry("elsewhere/b.iso", None).agreement(&layout).matched_by,
            "file_name"
        );
        assert_eq!(
            entry("b.iso", None).agreement(&layout).file_index,
            Some(1),
            "the index has to be the torrent's, not the document's"
        );
    }

    /// Attributing a checksum to the wrong file would report a mismatch
    /// against a file the document never named, so two candidates is no
    /// candidate.
    #[test]
    fn two_files_with_the_same_name_are_ambiguous_rather_than_the_first_one() {
        let layout = layout("rel", &[("x/a.iso", 4), ("y/a.iso", 64)]);
        let agreement = entry("a.iso", Some(64)).agreement(&layout);
        assert_eq!(agreement.file_index, None);
        assert_eq!(agreement.matched_by, "ambiguous");
        assert_eq!(agreement.size_agrees, None);
    }

    #[test]
    fn a_name_that_matches_nothing_in_the_torrent_says_so() {
        let layout = layout("rel", &[("a.iso", 4), ("b.iso", 64)]);
        assert_eq!(
            entry("c.iso", None).agreement(&layout).matched_by,
            "no_match"
        );
        assert_eq!(entry("", None).agreement(&layout).matched_by, "no_name");
    }

    #[test]
    fn torrents_come_out_in_the_documents_preferred_order() {
        let text = r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink">
          <file name="a.iso">
            <metaurl mediatype="torrent" priority="7">https://slow.example.com/a.torrent</metaurl>
            <metaurl mediatype="torrent" priority="1">https://fast.example.com/a.torrent</metaurl>
          </file>
        </metalink>"#;
        let doc = Metalink::parse(text.as_bytes()).unwrap();
        let order: Vec<&str> = doc
            .single_file()
            .unwrap()
            .torrents_by_priority()
            .iter()
            .map(|m| m.url.as_str())
            .collect();
        assert_eq!(
            order,
            [
                "https://fast.example.com/a.torrent",
                "https://slow.example.com/a.torrent"
            ]
        );
    }

    /// A file in a directory of its own, kept alive by the returned handle.
    ///
    /// One shared directory with fixed names is a flake: `cargo test` runs
    /// these on several threads, and two of them writing the same path means
    /// one can hash a file the other has truncated to zero. That failed once
    /// in the full suite and passed every time in isolation, which is the
    /// shape of the bug.
    fn temp_file(name: &str, bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    /// The three algorithms against the empty string, whose digests are
    /// published constants, so the dispatch is checked against something other
    /// than itself.
    #[test]
    fn every_supported_algorithm_hashes_a_file() {
        let (_dir, path) = temp_file("empty", b"");
        for (algorithm, expected) in [
            (
                "sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            ("sha1", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            ("md5", "d41d8cd98f00b204e9800998ecf8427e"),
        ] {
            let checksum = Checksum {
                algorithm: algorithm.to_string(),
                value: expected.to_string(),
            };
            let verified = checksum.verify_file(&path).unwrap();
            assert!(verified.matched, "{algorithm}: {verified:?}");
            assert_eq!(verified.actual, expected);
            assert_eq!(verified.bytes_hashed, 0);
        }
    }

    #[test]
    fn a_checksum_that_disagrees_reports_both_digests() {
        let (_dir, path) = temp_file("payload", b"hello");
        let checksum = Checksum {
            algorithm: "md5".to_string(),
            value: "00000000000000000000000000000000".to_string(),
        };
        let verified = checksum.verify_file(&path).unwrap();
        assert!(!verified.matched);
        assert_eq!(verified.expected, "00000000000000000000000000000000");
        assert_eq!(verified.actual, "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(verified.bytes_hashed, 5);
    }

    /// The shape a real MirrorBrain 2.19.0 document has, which is what
    /// `download.documentfoundation.org` serves for every file it publishes.
    ///
    /// Three things here are not in a hand-written example and all three broke
    /// something on the way in. Version 4's per-piece hashes are bare `<hash>`
    /// children of `<pieces>` with **no attributes at all**, so the version 3
    /// rule of looking for `piece="N"` does not see them and only the parent
    /// says what they are. A `<signature>` holds an OpenPGP block whose text
    /// must not become the value of anything. And the mirrors carry
    /// `location` and a dense `priority` run, which is the ordering a caller
    /// depends on.
    #[test]
    fn a_real_mirrorbrain_document_yields_three_checksums_and_no_piece_hashes() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <generator>MirrorBrain/2.19.0</generator>
  <origin dynamic="true">http://example.org/f.msi.meta4</origin>
  <published>2026-08-21T04:34:52Z</published>
  <publisher><name>The Document Foundation</name><url>https://example.org</url></publisher>
  <file name="f.msi">
    <size>3801088</size>
    <signature mediatype="application/pgp-signature">
-----BEGIN PGP SIGNATURE-----
iQIzBAABCAAdFiEEwoOeytlAj76VMcPp9DSh76/urqMFAmoBn8UACgkQ9DSh76/u
-----END PGP SIGNATURE-----
    </signature>
    <hash type="md5">ba40858a0919d0b6a6f974345971f48f</hash>
    <hash type="sha-1">21f59bdf58ac102398a804c5ccf9e282559f7966</hash>
    <hash type="sha-256">2cb3264a09b84c7857a5de790402ebbe53beb4ab9b0a5c49dc9d2863775835d8</hash>
    <pieces length="262144" type="sha-1">
      <hash>eae90fb30c4aaabb5eb42c563f575e3b570dc7b6</hash>
      <hash>7b270c82a17878a99f0d3ea010835f7f768ba25b</hash>
    </pieces>
    <url location="sg" priority="1">https://a.example.org/f.msi</url>
    <url location="cn" priority="2">https://b.example.org/f.msi</url>
  </file>
</metalink>"#;
        let doc = Metalink::parse(text.as_bytes()).unwrap();
        assert_eq!(doc.version, Version::V4);
        let file = doc.single_file().unwrap();
        assert_eq!(file.name, "f.msi");
        assert_eq!(file.size, Some(3_801_088));
        // Three whole-file checksums and not five. The two `<pieces>` children
        // are piece hashes and the signature is not a checksum at all.
        assert_eq!(
            file.checksums
                .iter()
                .map(|c| c.algorithm.as_str())
                .collect::<Vec<_>>(),
            ["md5", "sha1", "sha256"]
        );
        assert_eq!(
            file.best_checksum().unwrap().value,
            "2cb3264a09b84c7857a5de790402ebbe53beb4ab9b0a5c49dc9d2863775835d8"
        );
        assert_eq!(file.mirrors.len(), 2);
        assert_eq!(file.mirrors[0].location.as_deref(), Some("sg"));
        assert_eq!(
            file.mirrors_by_priority()[0].url,
            "https://a.example.org/f.msi"
        );
        // No `<metaurl>`, which is what every MirrorBrain instance reachable
        // in August 2026 serves. The caller has to say so rather than fail
        // obscurely. See `TODO/cli-surface.md`, T-113.
        assert!(file.torrents.is_empty());
    }

    /// A checksum that cannot be computed is an error, not a pass. Returning
    /// "fine" for an algorithm nothing hashed is a claim of verification that
    /// did not happen.
    #[test]
    fn an_algorithm_that_cannot_be_computed_is_an_error_rather_than_a_pass() {
        let (_dir, path) = temp_file("payload", b"hello");
        let checksum = Checksum {
            algorithm: "sha512".to_string(),
            value: "ab".to_string(),
        };
        let error = checksum.verify_file(&path).unwrap_err();
        assert!(error.to_string().contains("sha512"), "{error}");
    }
}
