//! Magnet URIs (BEP 9, BEP 53).
//!
//! A magnet carries the info hash and, optionally, the name, the trackers, the
//! web seeds, peers to try directly, and a file selection. It does not carry
//! the piece hashes, so a magnet has to be resolved against the swarm before
//! the torrent's shape is known.
//!
//! Both `xt=urn:btih:` forms are accepted: 40 hex characters, and the 32
//! character base32 form older clients emit.

use std::fmt;

use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::error::{Error, Result};
use crate::torrent::metainfo::{InfoHash, Metainfo};

/// A parsed magnet URI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Magnet {
    /// `xt=urn:btih:` the v1 info hash.
    pub info_hash: Option<InfoHash>,
    /// `xt=urn:btmh:` the BEP 52 v2 multihash, kept as written.
    pub info_hash_v2: Option<String>,
    /// `dn=` display name. A hint only; the real name comes from the metadata.
    pub name: Option<String>,
    /// `tr=` trackers, in the order they appeared.
    pub trackers: Vec<String>,
    /// `ws=` web seeds (BEP 19).
    pub web_seeds: Vec<String>,
    /// `as=` acceptable source, an HTTP fallback for the payload.
    pub acceptable_sources: Vec<String>,
    /// `xs=` exact source.
    pub exact_sources: Vec<String>,
    /// `x.pe=` peers to contact directly.
    pub peers: Vec<String>,
    /// `xl=` exact length in bytes.
    pub length: Option<u64>,
    /// `so=` BEP 53 file selection, as index ranges.
    pub select_only: Vec<(u32, u32)>,
}

impl Magnet {
    /// Whether a string looks like a magnet URI.
    pub fn looks_like(text: &str) -> bool {
        text.trim_start().len() >= 8 && text.trim_start()[..8].eq_ignore_ascii_case("magnet:?")
    }

    /// Parse a magnet URI.
    pub fn parse(uri: &str) -> Result<Self> {
        let uri = uri.trim();
        let query = uri
            .get(..8)
            .filter(|head| head.eq_ignore_ascii_case("magnet:?"))
            .map(|_| &uri[8..])
            .ok_or_else(|| {
                Error::source_resolution(format!(
                    "`{uri}` is not a magnet URI (it must start with `magnet:?`)"
                ))
                .with("value", uri.to_string())
            })?;

        let mut magnet = Self::default();
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, raw) = pair.split_once('=').unwrap_or((pair, ""));
            let value = decode(raw);
            // Repeated parameters may be numbered (`tr.1=`), which some
            // clients emit and which the BEP allows.
            let key = key.split('.').next().unwrap_or(key);
            match key {
                "xt" => magnet.set_topic(&value)?,
                "dn" => magnet.name = Some(value),
                "tr" => magnet.trackers.push(value),
                "ws" => magnet.web_seeds.push(value),
                "as" => magnet.acceptable_sources.push(value),
                "xs" => magnet.exact_sources.push(value),
                "x" => magnet.peers.push(value),
                "xl" => magnet.length = value.parse().ok(),
                "so" => magnet.select_only = parse_select_only(&value),
                // Unknown parameters are ignored rather than refused. Magnet
                // URIs collect vendor extensions and refusing one would break
                // a link that is otherwise perfectly usable.
                _ => {}
            }
        }

        if magnet.info_hash.is_none() && magnet.info_hash_v2.is_none() {
            return Err(
                Error::source_resolution("magnet URI has no `xt=urn:btih:` info hash")
                    .with("value", uri.to_string()),
            );
        }
        Ok(magnet)
    }

    fn set_topic(&mut self, value: &str) -> Result<()> {
        if let Some(hash) = strip_ci(value, "urn:btih:") {
            self.info_hash = Some(InfoHash::parse(hash)?);
            return Ok(());
        }
        if let Some(hash) = strip_ci(value, "urn:btmh:") {
            self.info_hash_v2 = Some(hash.to_string());
            return Ok(());
        }
        // An unrecognised `xt` is not fatal on its own; the error comes later
        // if no usable topic was found at all.
        Ok(())
    }

    /// Build a magnet URI from a parsed torrent.
    pub fn from_metainfo(meta: &Metainfo) -> Self {
        Self {
            info_hash: Some(meta.info_hash()),
            info_hash_v2: None,
            name: Some(meta.info().name.clone()),
            trackers: meta.trackers(),
            web_seeds: meta.url_list(),
            acceptable_sources: Vec::new(),
            exact_sources: Vec::new(),
            peers: Vec::new(),
            length: Some(meta.info().total_length()),
            select_only: Vec::new(),
        }
    }

    /// Render as a magnet URI.
    ///
    /// Parameters are emitted in a fixed order so the same torrent always
    /// produces the same magnet, which is what makes `bit-cli magnet` usable
    /// in a reproducible pipeline.
    pub fn to_uri(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(hash) = &self.info_hash {
            parts.push(format!("xt=urn:btih:{}", hash.hex()));
        }
        if let Some(hash) = &self.info_hash_v2 {
            parts.push(format!("xt=urn:btmh:{}", encode(hash)));
        }
        if let Some(name) = &self.name {
            parts.push(format!("dn={}", encode(name)));
        }
        if let Some(length) = self.length {
            parts.push(format!("xl={length}"));
        }
        for tracker in &self.trackers {
            parts.push(format!("tr={}", encode(tracker)));
        }
        for seed in &self.web_seeds {
            parts.push(format!("ws={}", encode(seed)));
        }
        for source in &self.acceptable_sources {
            parts.push(format!("as={}", encode(source)));
        }
        for source in &self.exact_sources {
            parts.push(format!("xs={}", encode(source)));
        }
        for peer in &self.peers {
            parts.push(format!("x.pe={}", encode(peer)));
        }
        if !self.select_only.is_empty() {
            let ranges: Vec<String> = self
                .select_only
                .iter()
                .map(|(a, b)| {
                    if a == b {
                        a.to_string()
                    } else {
                        format!("{a}-{b}")
                    }
                })
                .collect();
            parts.push(format!("so={}", ranges.join(",")));
        }
        format!("magnet:?{}", parts.join("&"))
    }

    /// The file indices `so=` selects, expanded.
    pub fn selected_files(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for (first, last) in &self.select_only {
            out.extend(*first..=*last);
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

impl fmt::Display for Magnet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_uri())
    }
}

fn strip_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

/// Percent-decode, treating `+` as a space the way a query string does.
fn decode(raw: &str) -> String {
    let spaces = raw.replace('+', " ");
    percent_decode_str(&spaces).decode_utf8_lossy().into_owned()
}

fn encode(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

/// Parse a BEP 53 `so=` value: `0,2,4-8`.
fn parse_select_only(value: &str) -> Vec<(u32, u32)> {
    value
        .split(',')
        .filter_map(|term| {
            let term = term.trim();
            match term.split_once('-') {
                None => term.parse().ok().map(|n| (n, n)),
                Some((first, last)) => {
                    Some((first.trim().parse().ok()?, last.trim().parse().ok()?))
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "0102030405060708090a0b0c0d0e0f1011121314";

    #[test]
    fn a_minimal_magnet_parses() {
        let magnet = Magnet::parse(&format!("magnet:?xt=urn:btih:{HEX}")).unwrap();
        assert_eq!(magnet.info_hash.unwrap().hex(), HEX);
        assert!(magnet.name.is_none());
        assert!(magnet.trackers.is_empty());
    }

    #[test]
    fn a_full_magnet_parses_every_parameter() {
        let uri = format!(
            "magnet:?xt=urn:btih:{HEX}&dn=My%20Album&xl=2000\
             &tr=udp%3A%2F%2Fa%3A80&tr=udp%3A%2F%2Fb%3A80\
             &ws=https%3A%2F%2Fe.com%2Fpub%2F\
             &as=https%3A%2F%2Ffallback.example.com%2Ffile\
             &xs=https%3A%2F%2Fe.com%2Ffile.torrent\
             &x.pe=1.2.3.4%3A6881&so=0,2,4-6"
        );
        let magnet = Magnet::parse(&uri).unwrap();
        assert_eq!(magnet.name.as_deref(), Some("My Album"));
        assert_eq!(magnet.length, Some(2000));
        assert_eq!(magnet.trackers, ["udp://a:80", "udp://b:80"]);
        assert_eq!(magnet.web_seeds, ["https://e.com/pub/"]);
        assert_eq!(
            magnet.acceptable_sources,
            ["https://fallback.example.com/file"]
        );
        assert_eq!(magnet.exact_sources, ["https://e.com/file.torrent"]);
        assert_eq!(magnet.peers, ["1.2.3.4:6881"]);
        assert_eq!(magnet.selected_files(), vec![0, 2, 4, 5, 6]);
    }

    #[test]
    fn a_base32_info_hash_is_accepted() {
        let magnet = Magnet::parse("magnet:?xt=urn:btih:AEBAGBAFAYDQQCIKBMGA2DQPCAIREEYU").unwrap();
        assert_eq!(magnet.info_hash.unwrap().hex(), HEX);
    }

    #[test]
    fn numbered_parameters_are_accepted() {
        let uri =
            format!("magnet:?xt=urn:btih:{HEX}&tr.1=udp%3A%2F%2Fa%3A80&tr.2=udp%3A%2F%2Fb%3A80");
        assert_eq!(Magnet::parse(&uri).unwrap().trackers.len(), 2);
    }

    #[test]
    fn a_plus_in_a_display_name_decodes_as_a_space() {
        let magnet = Magnet::parse(&format!("magnet:?xt=urn:btih:{HEX}&dn=My+Album")).unwrap();
        assert_eq!(magnet.name.as_deref(), Some("My Album"));
    }

    #[test]
    fn unknown_parameters_are_ignored_rather_than_refused() {
        let uri = format!("magnet:?xt=urn:btih:{HEX}&vendor=whatever&kt=keyword");
        assert!(Magnet::parse(&uri).is_ok());
    }

    #[test]
    fn a_magnet_without_a_topic_is_refused() {
        let err = Magnet::parse("magnet:?dn=nothing").unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::SourceResolution);
        assert!(
            err.message().contains("no `xt=urn:btih:`"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn a_non_magnet_is_refused() {
        assert!(Magnet::parse("https://e.com/x.torrent").is_err());
        assert!(Magnet::parse("").is_err());
    }

    #[test]
    fn the_scheme_is_matched_case_insensitively() {
        assert!(Magnet::looks_like(&format!("MAGNET:?xt=urn:btih:{HEX}")));
        assert!(
            Magnet::parse(&format!("MAGNET:?XT=urn:btih:{HEX}")).is_err(),
            "keys are case sensitive"
        );
        assert!(
            Magnet::parse(&format!("MAGNET:?xt=URN:BTIH:{HEX}")).is_ok(),
            "the urn prefix is not"
        );
    }

    #[test]
    fn looks_like_does_not_panic_on_short_input() {
        for text in ["", "m", "magnet", "magnet:"] {
            assert!(!Magnet::looks_like(text));
        }
        assert!(Magnet::looks_like("magnet:?x"));
    }

    #[test]
    fn a_magnet_round_trips_through_its_uri() {
        let uri = format!(
            "magnet:?xt=urn:btih:{HEX}&dn=My%20Album&xl=2000&tr=udp%3A%2F%2Fa%3A80&ws=https%3A%2F%2Fe.com%2Fpub%2F"
        );
        let magnet = Magnet::parse(&uri).unwrap();
        let rendered = magnet.to_uri();
        assert_eq!(Magnet::parse(&rendered).unwrap(), magnet);
    }

    #[test]
    fn rendering_is_stable_so_a_pipeline_can_compare_output() {
        let magnet = Magnet {
            info_hash: Some(InfoHash::parse(HEX).unwrap()),
            name: Some("album".into()),
            trackers: vec!["udp://a:80".into(), "udp://b:80".into()],
            length: Some(10),
            ..Default::default()
        };
        assert_eq!(magnet.to_uri(), magnet.to_uri());
        assert_eq!(
            magnet.to_uri(),
            format!(
                "magnet:?xt=urn:btih:{HEX}&dn=album&xl=10&tr=udp%3A%2F%2Fa%3A80&tr=udp%3A%2F%2Fb%3A80"
            )
        );
    }

    #[test]
    fn select_only_ranges_render_compactly() {
        let magnet = Magnet {
            info_hash: Some(InfoHash::parse(HEX).unwrap()),
            select_only: vec![(0, 0), (4, 6)],
            ..Default::default()
        };
        assert!(
            magnet.to_uri().ends_with("&so=0,4-6"),
            "{}",
            magnet.to_uri()
        );
    }
}
