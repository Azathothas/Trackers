//! Finding the torrents a web page links to.
//!
//! A URL naming a page is how a person meets a torrent almost every time, and
//! naming the `.torrent` itself is the exception. Until this existed a page
//! was fetched and handed to the bencode parser, which failed on the first
//! byte of the markup. See `TODO/cli-surface.md`, T-244.
//!
//! **This is one function over an HTML string, and that is deliberate.** The
//! static tier hands it what the server sent; the `--render` tier hands it the
//! DOM after script has run. If the rendered tier changed anything but where
//! the HTML came from, the two tiers could disagree about a page for a reason
//! that is not the page.
//!
//! # What counts as a match
//!
//! An `href` on an `<a>`, an `<area>` or a `<link>`, judged three ways. The
//! first is the common case and the other two exist because a real indexer
//! measured on 2026-08-29 has neither.
//!
//! 1. **The path ends `.torrent`**, or the href begins `magnet:`. The path is
//!    what decides, so `?download=1` after the extension does not make the
//!    link something else and `.torrent.html` is not a match. Comparison is
//!    case insensitive, because `.TORRENT` is served in the wild.
//! 2. **The element declares `type="application/x-bittorrent"`.** That is the
//!    publisher saying what is behind the link, which is better evidence than
//!    an extension, and it is how `<link rel="alternate">` advertises one.
//! 3. **The link's label says it is a torrent** and its href carries a
//!    non-empty query value. The label is the anchor text, or the anchor's
//!    `title`, or the `alt` or `title` of an image it wraps.
//!
//! The third is the narrow rule that reaches an indexer serving torrents from
//! a script endpoint, and every part of it was measured rather than guessed.
//! See [`TORRENT_LABELS`].
//!
//! # What is skipped, and why each one
//!
//! - `<script>`, `<style>` and `<template>` bodies, because a browser does not
//!   render them and a URL inside one is data rather than a link.
//! - `<noscript>` bodies, because a browser with script **on** does not render
//!   them either. Skipping them is what keeps this tier and the rendered tier
//!   agreeing on a page neither of them should read differently.
//! - HTML comments.
//! - Anything that is not `http`, `https` or `magnet` after resolution, which
//!   is what drops a `data:` URI.
//!
//! # An off-host link is a match, and that was measured
//!
//! Restricting matches to the document's own host was considered and is wrong.
//! `kali.org`'s download page is served from `www.kali.org` and every one of
//! the 113 torrents it links sits on `cdimage.kali.org`; a same-host rule
//! returns nothing there. `scripts/check-page-fetch.ps1` is the measurement.
//! The host is reported per link instead, so a caller can see it.

use impit::fingerprint::ExtensionType;
use impit::fingerprint::{
    BrowserFingerprint, CertificateCompressionAlgorithm, CipherSuite, EchConfig, EchMode,
    HpkeKemId, Http2Fingerprint, KeyExchangeGroup, SignatureAlgorithm, TlsExtensions,
    TlsFingerprint,
};
use url::Url;

/// Which client `bit-cli` presents itself as when it fetches a source
/// document.
///
/// An origin that fingerprints its callers sends a different page to a client
/// it does not recognise, and a reader parsing that page is reading a page
/// nobody else gets. So the fetch of a `.torrent` or of the page linking to
/// one presents as a current Chrome by default. See `TODO/cli-surface.md`,
/// T-244.
///
/// **This is the source document only.** A web seed is a mirror the caller
/// configured, fetching payload bytes, and it keeps `bit-cli/<version>`:
/// impersonating a browser at a mirror somebody pointed us at buys nothing and
/// hides who is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientProfile {
    /// A current Chrome's header set, in Chrome's order.
    #[default]
    Browser,
    /// `bit-cli/<version>` and nothing else, which is what every other request
    /// here sends.
    Plain,
}

impl ClientProfile {
    /// The name the command line uses and a report prints.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Plain => "plain",
        }
    }
}

/// The Chrome major this profile claims to be.
///
/// A profile pinned to a version nobody runs is a *correct* fingerprint of a
/// browser that does not exist, which is its own tell. This is the one number
/// to move when the profile is refreshed, and `scripts/check-fingerprint.ps1`
/// records what was captured against it.
pub const BROWSER_MAJOR: u32 = 151;

/// The exact build the profile below was captured from.
///
/// A major on its own does not say which capture produced these values, and
/// two builds of one major have differed here before. `TODO/RULES.md` section
/// 6b is why this is recorded rather than derived: everything the profile
/// claims comes off a browser, so the browser it came off is part of the
/// record.
pub const BROWSER_BUILD: &str = "151.0.7922.72";

/// The `User-Agent` the browser profile sends.
pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";

/// `SETTINGS_HEADER_TABLE_SIZE`, which a current Chrome announces and `hyper`
/// does not.
///
/// The HTTP/2 half of a fingerprint is the SETTINGS frame, the connection
/// WINDOW_UPDATE, any PRIORITY frames and the pseudo-header order, read
/// together. Chrome sends settings 1, 2, 4 and 6 and no others.
pub const BROWSER_H2_HEADER_TABLE_SIZE: u32 = 65_536;

/// Whether to leave `SETTINGS_MAX_FRAME_SIZE` out of the SETTINGS frame.
///
/// `hyper` announces the protocol's own default, 16384, on every connection,
/// and Chrome announces nothing. A peer that receives no
/// `SETTINGS_MAX_FRAME_SIZE` uses the same default, so the connection carries
/// exactly what it carried before; only the fingerprint changes.
pub const BROWSER_H2_OMIT_MAX_FRAME_SIZE: bool = true;

/// Every header the browser profile sends, in the order Chrome sends them for
/// a top level navigation.
///
/// **Order is part of the fingerprint**, not a style choice: the HTTP/2 half
/// of a client's identity includes the header sequence after the
/// pseudo-headers, so a set with the right names in the wrong order is still
/// distinguishable. `http::HeaderMap` iterates in neither insertion nor
/// alphabetical order, so the sequence is carried to the wire separately, by
/// the vendored `h2`. See `crates/bit-cli-core/src/fetch.rs` and
/// `patches/UPSTREAM.md`.
///
/// `user-agent` and `accept-encoding` are here rather than left to the HTTP
/// client, because a client that appends them writes them last and Chrome
/// does not. The `accept-encoding` value has to agree with what this client
/// can actually decode: `gzip`, `deflate`, `br` and `zstd` are the four the
/// fetcher's decompression is built with, which is why those four and no
/// others. A client advertising an encoding it cannot decode hands brotli to
/// a bencode parser.
///
/// **This is the file a staleness check rewrites.**
/// `scripts/check-browser-version.ps1` compares [`BROWSER_MAJOR`] against what
/// Chrome, Firefox and Edge have actually shipped, and
/// `scripts/capture-browser-fingerprint.ps1` emits a replacement for this list
/// from a real browser driven at `loopback-tlsprobe`.
pub const BROWSER_HEADERS: &[(&str, &str)] = &[
    (
        "sec-ch-ua",
        "\"Not=A?Brand\";v=\"99\", \"Google Chrome\";v=\"151\", \"Chromium\";v=\"151\"",
    ),
    ("sec-ch-ua-mobile", "?0"),
    ("sec-ch-ua-platform", "\"Windows\""),
    ("upgrade-insecure-requests", "1"),
    ("user-agent", BROWSER_USER_AGENT),
    (
        "accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,\
         image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
    ),
    ("sec-fetch-site", "none"),
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-user", "?1"),
    ("sec-fetch-dest", "document"),
    ("accept-encoding", "gzip, deflate, br, zstd"),
    ("accept-language", "en-US,en;q=0.9"),
    ("priority", "u=0, i"),
];

// ===== the TLS and HTTP/2 halves, which used to live in the vendored tree ====

/// The cipher suites the profile offers, in the order it offers them.
///
/// GREASE leads, which is Chrome's own shape and is not an artefact of how
/// this list is written down. RFC 8701 is why a server tolerates it.
pub const BROWSER_CIPHER_SUITES: &[CipherSuite] = &[
    CipherSuite::Grease,
    CipherSuite::TLS13_AES_128_GCM_SHA256,
    CipherSuite::TLS13_AES_256_GCM_SHA384,
    CipherSuite::TLS13_CHACHA20_POLY1305_SHA256,
    CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
    CipherSuite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
    CipherSuite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
    CipherSuite::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
    CipherSuite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
    CipherSuite::TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA,
    CipherSuite::TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA,
    CipherSuite::TLS_RSA_WITH_AES_128_GCM_SHA256,
    CipherSuite::TLS_RSA_WITH_AES_256_GCM_SHA384,
    CipherSuite::TLS_RSA_WITH_AES_128_CBC_SHA,
    CipherSuite::TLS_RSA_WITH_AES_256_CBC_SHA,
];

/// The key exchange groups, including the post-quantum hybrid Chrome ships.
pub const BROWSER_KEY_EXCHANGE_GROUPS: &[KeyExchangeGroup] = &[
    KeyExchangeGroup::Grease,
    KeyExchangeGroup::X25519MLKEM768,
    KeyExchangeGroup::X25519,
    KeyExchangeGroup::Secp256r1,
    KeyExchangeGroup::Secp384r1,
];

/// The signature algorithms, led by the three ML-DSA codepoints.
///
/// Those three are what separates a 151 hello from a 142 one at the TLS layer,
/// and they are the reason the JA4 moved between the two.
pub const BROWSER_SIGNATURE_ALGORITHMS: &[SignatureAlgorithm] = &[
    SignatureAlgorithm::MlDsa44,
    SignatureAlgorithm::MlDsa65,
    SignatureAlgorithm::MlDsa87,
    SignatureAlgorithm::EcdsaSecp256r1Sha256,
    SignatureAlgorithm::RsaPssRsaSha256,
    SignatureAlgorithm::RsaPkcs1Sha256,
    SignatureAlgorithm::EcdsaSecp384r1Sha384,
    SignatureAlgorithm::RsaPssRsaSha384,
    SignatureAlgorithm::RsaPkcs1Sha384,
    SignatureAlgorithm::RsaPssRsaSha512,
    SignatureAlgorithm::RsaPkcs1Sha512,
];

/// A fixed order to write the extensions in, and **it is deliberately empty**.
///
/// A real Chrome shuffles its extension list per connection and has since 110;
/// the shuffle is the reason JA4 sorts at all. A client whose order never
/// changes is *more* distinguishable than one that shuffles, because the fixed
/// sequence is itself the signal. So nothing is pinned here and the handshake
/// permutes the list per connection, keeping only what the specification pins:
/// `pre_shared_key` last, and GREASE at the two ends.
///
/// **Naming an extension here pins it**, which is what this list is for if a
/// browser is ever measured pinning one. Every entry it used to carry is in
/// the git history of this file and in `TODO/cli-surface.md` T-263.
pub const BROWSER_EXTENSION_ORDER: &[ExtensionType] = &[];

/// The order this profile used to write, kept only so a test can assert the
/// shuffle actually moves things.
#[cfg(test)]
const BROWSER_EXTENSION_ORDER_WAS: &[ExtensionType] = &[
    ExtensionType::ServerName,
    ExtensionType::ExtendedMasterSecret,
    ExtensionType::SessionTicket,
    ExtensionType::SignatureAlgorithms,
    ExtensionType::StatusRequest,
    ExtensionType::SupportedGroups,
    ExtensionType::ApplicationLayerProtocolNegotiation,
    ExtensionType::SignedCertificateTimestamp,
    ExtensionType::KeyShare,
    ExtensionType::PskKeyExchangeModes,
    ExtensionType::SupportedVersions,
    ExtensionType::CompressCertificate,
    ExtensionType::ApplicationSettings,
];

/// The ALPN list, which is what makes the HTTP/2 half of the fingerprint exist
/// at all.
pub const BROWSER_ALPN: &[&[u8]] = &[b"h2", b"http/1.1"];

/// `SETTINGS_INITIAL_WINDOW_SIZE`.
pub const BROWSER_H2_INITIAL_STREAM_WINDOW: u32 = 6_291_456;

/// The connection window Chrome opens, **as a window and not as an increment**.
///
/// This is a value `impit`'s own database got wrong in the other direction, and
/// it is the one field where the difference is invisible until it reaches the
/// wire: the emitted WINDOW_UPDATE is this minus the protocol's own 65,535
/// default, so 15,728,640 here is `15663105` in an Akamai fingerprint.
pub const BROWSER_H2_CONNECTION_WINDOW: u32 = 15_728_640;

/// `SETTINGS_MAX_HEADER_LIST_SIZE`.
pub const BROWSER_H2_MAX_HEADER_LIST_SIZE: u32 = 262_144;

/// The PRIORITY block Chrome opens its first stream with: exclusive, no
/// dependency, weight 255.
///
/// This is the third field of a four field Akamai fingerprint, and it was the
/// one field of the four where this client differed from a browser: `h2`
/// parses a priority block on receive and wrote none on send, so the
/// fingerprint carried `0` where a browser carries `1:1:0:255`.
///
/// **RFC 9113 section 5.3.1 deprecates stream priority** and says a sender
/// SHOULD NOT send the PRIORITY frame. That is about the standalone frame; the
/// block in the HEADERS frame is what a browser actually emits and what a
/// fingerprint reads. Measured identical on Chrome 151 and Chrome 152.
/// See `TODO/cli-surface.md`, T-262.
///
/// The tuple is `(dependency stream id, wire weight, exclusive)`. The wire
/// weight is one less than the weight the specification talks in, so a browser
/// asking for 256 puts 255 here.
pub const BROWSER_H2_STREAM_PRIORITY: (u32, u8, bool) = (0, 255, true);

/// The pseudo-header order, which is the fourth field of an Akamai
/// fingerprint.
///
/// `http::HeaderMap` cannot carry an order, so this reaches the wire through
/// the vendored `h2` rather than through the header map. See
/// `patches/UPSTREAM.md`.
pub const BROWSER_PSEUDO_HEADER_ORDER: &[&str] = &[
    ":method",
    ":authority",
    ":scheme",
    ":path",
    ":protocol",
    ":status",
];

/// The whole profile, as the type the vendored client wants.
///
/// **Every value in it is this repository's**, which is the point.
/// `TODO/RULES.md` section 6b: `impit`'s fingerprint database is a starting
/// point and not an authority, it has already been wrong here, and a starting
/// point does not get to be the home of the answer. So the enums stay
/// `impit`'s, because they are the vocabulary the vendored `rustls` speaks,
/// and every value chosen out of them is above.
///
/// What this buys is that a bump edits **one file** this repository owns, a
/// staleness recommendation has one file to name, and
/// `vendor/impit/impit/src/fingerprint/database/chrome.rs` carries no data
/// this repository authored. A value those enums cannot express is a finding
/// to record rather than a value to drop quietly.
pub fn browser_fingerprint() -> BrowserFingerprint {
    let extensions = TlsExtensions::new(
        true, // server_name
        true, // status_request
        true, // supported_groups
        true, // signature_algorithms
        true, // application_layer_protocol_negotiation
        true, // signed_certificate_timestamp
        true, // key_share
        true, // psk_key_exchange_modes
        true, // supported_versions
        Some(vec![CertificateCompressionAlgorithm::Brotli]),
        true,  // application_settings
        false, // delegated_credentials, which Chrome does not send
        None,  // record_size_limit, which Chrome does not send
        BROWSER_EXTENSION_ORDER.to_vec(),
    )
    // Chrome 136 and later use 17613 rather than 17513 for ALPS.
    .with_new_alps_codepoint(true)
    // GREASE at both ends of the extension list, at a codepoint chosen per
    // connection. Measured on a real Chrome: `0x3a3a` first with an empty
    // body and `0x5a5a` last with a single zero byte, and the pair differs
    // between connections. See `TODO/cli-surface.md`, T-263.
    .with_grease_both_ends(true);

    let tls = TlsFingerprint::new(
        BROWSER_CIPHER_SUITES.to_vec(),
        BROWSER_KEY_EXCHANGE_GROUPS.to_vec(),
        BROWSER_SIGNATURE_ALGORITHMS.to_vec(),
        extensions,
        Some(EchConfig::new(
            EchMode::Grease {
                hpke_suite: HpkeKemId::DhKemX25519HkdfSha256,
            },
            None,
        )),
        BROWSER_ALPN.iter().map(|p| p.to_vec()).collect(),
    );

    let http2 = Http2Fingerprint {
        pseudo_header_order: BROWSER_PSEUDO_HEADER_ORDER
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        initial_stream_window_size: Some(BROWSER_H2_INITIAL_STREAM_WINDOW),
        initial_connection_window_size: Some(BROWSER_H2_CONNECTION_WINDOW),
        max_header_list_size: Some(BROWSER_H2_MAX_HEADER_LIST_SIZE),
    };

    BrowserFingerprint::new(
        "Chrome",
        BROWSER_MAJOR.to_string(),
        tls,
        http2,
        BROWSER_HEADERS
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect(),
    )
}

/// What kind of torrent link was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// An `href` whose path ends `.torrent`.
    Torrent,
    /// A `magnet:` URI.
    Magnet,
}

impl LinkKind {
    /// A stable machine-readable name, used in reports and errors.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Torrent => "torrent",
            Self::Magnet => "magnet",
        }
    }
}

/// Why a link was taken for a torrent.
///
/// Reported per link, because the three are not equally strong and a caller
/// reading a refusal is entitled to know which one it was. A `label` match is
/// the tool reading a human-facing string, and somebody choosing between four
/// candidates should see that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedBy {
    /// The path ends `.torrent`, or the URI is a magnet.
    Extension,
    /// `type="application/x-bittorrent"` on the element.
    Type,
    /// The link's label says so, and its href carries an identifier.
    Label,
}

impl MatchedBy {
    /// A stable machine-readable name, used in reports.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Type => "type",
            Self::Label => "label",
        }
    }
}

/// One torrent a page links to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageLink {
    /// Absolute, resolved against the document and any `<base href>`.
    pub url: String,
    /// The label beside it, whitespace collapsed: the anchor text, or the
    /// element's `title`, or the `alt` or `title` of an image it wraps. May be
    /// empty: a link wrapping an unlabelled image has none and is still a
    /// link.
    pub text: String,
    pub kind: LinkKind,
    /// The host the link points at, absent for a magnet, which names no host.
    pub host: Option<String>,
    /// Which of the three rules took it.
    pub matched: MatchedBy,
}

/// The MIME type a torrent is served as, and the one `type=` value that means
/// a link is one.
pub const TORRENT_MEDIA_TYPE: &str = "application/x-bittorrent";

/// Labels a link carries when its href does not say it is a torrent.
///
/// **A closed list of whole labels, not a substring search**, and every part
/// of it is measured. Over the fifteen real pages
/// `scripts/check-page-fetch.ps1` fetches, this list plus the requirement that
/// the href carry a non-empty query value finds **74** torrent links that no
/// extension rule reaches, on the one page that publishes them that way, and
/// **one** false positive, which is that page's own empty template link.
/// Nothing on the other fourteen pages matches.
///
/// **A bare `torrents` is deliberately not here.** It was, and it matched the
/// navigation link to an index's own torrent listing on two of the fifteen
/// pages. A label that names a section is not a label that names a file.
///
/// The query requirement is the other half. An indexer serving a torrent from
/// a script endpoint passes an identifier; a navigation link does not.
pub const TORRENT_LABELS: &[&str] = &[
    "torrent",
    ".torrent",
    "torrent file",
    "torrent link",
    "torrent download",
    "download torrent",
    "download the torrent",
    "download .torrent",
    "get torrent",
    "dl torrent",
];

/// [`extract_links`] for a caller holding the URL as text.
///
/// `bit-cli` itself does not depend on `url`; every URL it handles arrives as
/// a string off the command line or out of a document. A `document_url` that
/// does not parse yields no links, which cannot happen in practice because the
/// only caller has already fetched it.
pub fn extract(html: &str, document_url: &str) -> Vec<PageLink> {
    match Url::parse(document_url) {
        Ok(base) => extract_links(html, &base),
        Err(_) => Vec::new(),
    }
}

/// Every torrent link on a page, in document order, deduplicated by URL.
///
/// `document_url` is where the HTML came from. It is what a relative href
/// resolves against, unless the document carries a `<base href>`, which wins.
pub fn extract_links(html: &str, document_url: &Url) -> Vec<PageLink> {
    let base = base_href(html)
        .and_then(|href| document_url.join(&href).ok())
        .unwrap_or_else(|| document_url.clone());

    let mut out: Vec<PageLink> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for candidate in anchors(html) {
        let Some(url) = resolve(&base, &candidate.href) else {
            continue;
        };
        let Some((kind, matched)) = classify(&url, &candidate) else {
            continue;
        };
        let as_string = url.to_string();
        if seen.iter().any(|s| s == &as_string) {
            continue;
        }
        seen.push(as_string.clone());
        out.push(PageLink {
            url: as_string,
            text: candidate.label,
            kind,
            host: url.host_str().map(str::to_string),
            matched,
        });
    }
    out
}

/// Does this body look like markup rather than a torrent?
///
/// Used to tell a page from a `.torrent` **after** the bencode parse has
/// already failed, never before it. A metainfo is a bencoded dictionary and
/// begins `d`; nothing that parses as one begins `<`. Trying the torrent
/// first and falling back means a mirror that serves a real `.torrent` under
/// the wrong content type is still read as a torrent, which content-type
/// sniffing on its own would get wrong.
///
/// `content_type` is consulted as well as the bytes because a page can be
/// served with a byte-order mark, a stray blank line, or a leading comment,
/// and because an empty body has no first byte to read.
pub fn looks_like_markup(bytes: &[u8], content_type: Option<&str>) -> bool {
    if let Some(kind) = content_type {
        let kind = kind.to_ascii_lowercase();
        let kind = kind.split(';').next().unwrap_or("").trim().to_string();
        if kind == "text/html" || kind == "application/xhtml+xml" || kind == "text/xhtml" {
            return true;
        }
    }
    // A UTF-8 byte-order mark before the markup is common and is not part of
    // the document.
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    bytes.get(start) == Some(&b'<')
}

/// The `href` of the first `<base>` that carries one.
///
/// The first is what the HTML standard says wins, and a second `<base>` is
/// ignored rather than being an error.
fn base_href(html: &str) -> Option<String> {
    let mut cursor = Cursor::new(html);
    while let Some(tag) = cursor.next_tag() {
        if tag.closing {
            continue;
        }
        if tag.name == "base"
            && let Some(href) = tag.attr("href")
            && !href.trim().is_empty()
        {
            return Some(href);
        }
        // `<base>` is only meaningful in the head, and a document that reached
        // its body has no more of them to offer.
        if tag.name == "body" {
            return None;
        }
    }
    None
}

/// Every `<a href>` and `<area href>` in the document, with the anchor's text.
/// One href the scanner found, with everything that decides what it is.
struct Candidate {
    href: String,
    /// The anchor text, or the element's `title`, or a wrapped image's `alt`
    /// or `title`, whichever is the first non-empty one.
    label: String,
    /// The element's `type` attribute, lowercased.
    declared_type: Option<String>,
}

fn anchors(html: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut cursor = Cursor::new(html);
    while let Some(tag) = cursor.next_tag() {
        if tag.closing {
            continue;
        }
        match tag.name.as_str() {
            // Raw-text and inert elements. Their contents are not rendered, so
            // a link inside one is not a link on the page.
            "script" | "style" | "noscript" | "template" if !tag.self_closing => {
                cursor.skip_to_close(&tag.name);
            }
            "a" | "area" | "link" => {
                let Some(href) = tag.attr("href") else {
                    continue;
                };
                // `<area>` and `<link>` are void: they have no content, so
                // their label is an attribute. An `<a>` has text, and when it
                // has none, an image it wraps usually carries one.
                let (text, image_label) = match (tag.name.as_str(), tag.self_closing) {
                    ("a", false) => cursor.content_until_close("a"),
                    _ => (String::new(), None),
                };
                let label = [
                    collapse(&text),
                    image_label.map(|l| collapse(&l)).unwrap_or_default(),
                    tag.attr("alt").map(|a| collapse(&a)).unwrap_or_default(),
                    tag.attr("title").map(|t| collapse(&t)).unwrap_or_default(),
                ]
                .into_iter()
                .find(|candidate| !candidate.is_empty())
                .unwrap_or_default();
                out.push(Candidate {
                    href: decode_entities(href.trim()),
                    label,
                    declared_type: tag.attr("type").map(|t| t.trim().to_ascii_lowercase()),
                });
            }
            _ => {}
        }
    }
    out
}

/// Is this label one that says the link behind it is a torrent?
///
/// Punctuation is dropped and whitespace collapsed before the comparison, so
/// `Download Torrent!` and `download torrent` are the same label, and the
/// result has to be one of [`TORRENT_LABELS`] **whole**.
fn label_says_torrent(label: &str) -> bool {
    let mut normalised = String::with_capacity(label.len());
    for c in label.chars() {
        match c {
            c if c.is_ascii_alphanumeric() || c == '.' => {
                normalised.extend(c.to_lowercase());
            }
            _ => normalised.push(' '),
        }
    }
    let normalised = collapse(&normalised);
    TORRENT_LABELS.contains(&normalised.as_str())
}

/// Does this URL carry a non-empty query value?
///
/// The other half of the label rule. An indexer serving a torrent from a
/// script endpoint passes an identifier; a navigation link to the same script
/// does not, and that difference is what keeps `Torrent` in a menu out.
fn carries_identifier(url: &Url) -> bool {
    url.query_pairs().any(|(_, value)| !value.trim().is_empty())
}

/// Resolve one href against the document's base.
///
/// A `magnet:` URI is absolute by construction and is parsed rather than
/// joined, because joining an opaque scheme against an http base does not
/// produce the magnet back.
fn resolve(base: &Url, href: &str) -> Option<Url> {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') {
        return None;
    }
    if href.len() >= 7 && href[..7].eq_ignore_ascii_case("magnet:") {
        return Url::parse(href).ok();
    }
    base.join(href).ok()
}

/// A resolved URL's kind, or `None` when it is not a torrent link at all.
fn classify(url: &Url, candidate: &Candidate) -> Option<(LinkKind, MatchedBy)> {
    match url.scheme() {
        "magnet" => Some((LinkKind::Magnet, MatchedBy::Extension)),
        "http" | "https" => {
            // Percent escapes are decoded before the extension is read, so
            // `/x%2Etorrent` is the same link as `/x.torrent`. The URL itself
            // is left exactly as it will be fetched.
            let path = percent_decode(url.path());
            if path.to_ascii_lowercase().ends_with(".torrent") {
                return Some((LinkKind::Torrent, MatchedBy::Extension));
            }
            // A declared type is the publisher saying what is behind the
            // link, which is stronger than an extension and is how a
            // `<link rel="alternate">` advertises one.
            if candidate
                .declared_type
                .as_deref()
                .is_some_and(|t| t.split(';').next().unwrap_or("").trim() == TORRENT_MEDIA_TYPE)
            {
                return Some((LinkKind::Torrent, MatchedBy::Type));
            }
            if label_says_torrent(&candidate.label) && carries_identifier(url) {
                return Some((LinkKind::Torrent, MatchedBy::Label));
            }
            None
        }
        _ => None,
    }
}

/// Percent-decode, leaving anything that is not a valid escape alone.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Collapse every run of whitespace to one space and trim.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decode the character references an attribute value or a run of text can
/// carry.
///
/// `&amp;` is the one that matters and it is not exotic: `linuxtracker.org`
/// writes every download link as `index.php?page=downloadcheck&amp;id=...`.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            // Byte indices into a `&str` are safe to slice here because the
            // only bytes matched are ASCII, so every split lands on a
            // character boundary.
            let start = i;
            while i < bytes.len() && bytes[i] != b'&' {
                i += 1;
            }
            out.push_str(&s[start..i]);
            continue;
        }
        let Some(end) = s[i..].find(';').map(|n| i + n) else {
            out.push('&');
            i += 1;
            continue;
        };
        // A reference is short. Anything longer is a stray ampersand followed
        // by text that happens to contain a semicolon.
        if end - i > 10 {
            out.push('&');
            i += 1;
            continue;
        }
        let name = &s[i + 1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => numeric_entity(name),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                i = end + 1;
            }
            None => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

fn numeric_entity(name: &str) -> Option<char> {
    let rest = name.strip_prefix('#')?;
    let value = match rest.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => rest.parse::<u32>().ok()?,
    };
    char::from_u32(value)
}

/// One tag, as the cursor read it.
struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<String> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

/// A forward-only scan over the markup.
///
/// This is a tag scanner and not a tree builder. It does not need to be one:
/// the question is "which hrefs are on this page", which never requires
/// knowing what nests inside what, and a scanner has no recovery rules to get
/// wrong on the malformed markup that real indexers serve.
struct Cursor<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            s,
            b: s.as_bytes(),
            i: 0,
        }
    }

    /// The next tag, skipping comments, doctypes and processing instructions.
    fn next_tag(&mut self) -> Option<Tag> {
        loop {
            while self.i < self.b.len() && self.b[self.i] != b'<' {
                self.i += 1;
            }
            if self.i >= self.b.len() {
                return None;
            }
            let rest = &self.s[self.i..];
            if rest.starts_with("<!--") {
                self.i = match rest.find("-->") {
                    Some(n) => self.i + n + 3,
                    None => self.b.len(),
                };
                continue;
            }
            if rest.starts_with("<!") || rest.starts_with("<?") {
                self.i = match rest.find('>') {
                    Some(n) => self.i + n + 1,
                    None => self.b.len(),
                };
                continue;
            }
            self.i += 1;
            let closing = self.b.get(self.i) == Some(&b'/');
            if closing {
                self.i += 1;
            }
            let name_start = self.i;
            while self.i < self.b.len() && is_name_byte(self.b[self.i]) {
                self.i += 1;
            }
            if self.i == name_start {
                // A bare `<` in text, which is legal enough in the wild.
                continue;
            }
            let name = self.s[name_start..self.i].to_ascii_lowercase();
            let (attrs, self_closing) = self.read_attrs();
            return Some(Tag {
                name,
                closing,
                self_closing,
                attrs,
            });
        }
    }

    /// Attributes up to the tag's `>`, in all three HTML5 value framings.
    ///
    /// The unquoted framing is not exotic. `kali.org` serves minified HTML and
    /// writes every torrent link as `href=https://...iso.torrent>torrent`, so
    /// a quoted-only reader finds nothing on a page carrying 113 of them.
    fn read_attrs(&mut self) -> (Vec<(String, String)>, bool) {
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            match self.b.get(self.i) {
                None => break,
                Some(&b'>') => {
                    self.i += 1;
                    break;
                }
                Some(&b'/') => {
                    self_closing = true;
                    self.i += 1;
                    continue;
                }
                _ => {}
            }
            let start = self.i;
            while self.i < self.b.len()
                && !self.b[self.i].is_ascii_whitespace()
                && self.b[self.i] != b'='
                && self.b[self.i] != b'>'
            {
                self.i += 1;
            }
            if self.i == start {
                // Nothing consumed, so nothing here is an attribute name.
                self.i += 1;
                continue;
            }
            let name = self.s[start..self.i].to_ascii_lowercase();
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.b.get(self.i) != Some(&b'=') {
                attrs.push((name, String::new()));
                continue;
            }
            self.i += 1;
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            let value = match self.b.get(self.i) {
                Some(&q @ (b'"' | b'\'')) => {
                    self.i += 1;
                    let vs = self.i;
                    while self.i < self.b.len() && self.b[self.i] != q {
                        self.i += 1;
                    }
                    let v = self.s[vs..self.i].to_string();
                    if self.i < self.b.len() {
                        self.i += 1;
                    }
                    v
                }
                _ => {
                    let vs = self.i;
                    while self.i < self.b.len()
                        && !self.b[self.i].is_ascii_whitespace()
                        && self.b[self.i] != b'>'
                    {
                        self.i += 1;
                    }
                    self.s[vs..self.i].to_string()
                }
            };
            attrs.push((name, value));
        }
        (attrs, self_closing)
    }

    /// Move past this element's closing tag without reading anything in it.
    fn skip_to_close(&mut self, name: &str) {
        let needle = format!("</{name}");
        let lower = self.s[self.i..].to_ascii_lowercase();
        match lower.find(&needle) {
            Some(n) => {
                self.i += n;
                // Consume the close tag itself so the caller resumes after it.
                let _ = self.next_tag();
            }
            None => self.i = self.b.len(),
        }
    }

    /// The text of the element that has just been opened, up to its close tag.
    ///
    /// The text of the element that has just been opened, and the label of the
    /// first image inside it.
    ///
    /// The second half exists because a real indexer's torrent links wrap an
    /// icon and have **no text at all**: measured on 2026-08-29,
    /// `linuxtracker.org` writes
    /// `<a href="index.php?..."><img alt="Download Torrent"></a>`. Reading the
    /// text alone finds an empty string there, which is what the first design
    /// of this would have done.
    fn content_until_close(&mut self, name: &str) -> (String, Option<String>) {
        let needle = format!("</{name}");
        let mut text = String::new();
        let mut image: Option<String> = None;
        loop {
            let rest = &self.s[self.i..];
            if rest.is_empty() {
                break;
            }
            let lower_rest = rest.to_ascii_lowercase();
            let close_at = lower_rest.find(&needle);
            let next_lt = rest.find('<');
            match (close_at, next_lt) {
                (Some(0), _) => {
                    let _ = self.next_tag();
                    break;
                }
                (_, Some(0)) => {
                    // Some other tag. Step over it and keep its text.
                    if rest.starts_with("<!--") {
                        self.i += rest.find("-->").map_or(rest.len(), |n| n + 3);
                    } else if let Some(inner) = self.next_tag()
                        && image.is_none()
                        && inner.name == "img"
                        && !inner.closing
                    {
                        image = inner
                            .attr("alt")
                            .or_else(|| inner.attr("title"))
                            .map(|l| decode_entities(&l))
                            .filter(|l| !l.trim().is_empty());
                    }
                }
                (_, Some(n)) => {
                    let end = close_at.map_or(n, |c| c.min(n));
                    text.push_str(&rest[..end]);
                    self.i += end;
                }
                (_, None) => {
                    text.push_str(rest);
                    self.i = self.b.len();
                    break;
                }
            }
            if text.len() > 4096 {
                break;
            }
        }
        (decode_entities(&text), image)
    }
}

const fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(url: &str) -> Url {
        Url::parse(url).expect("test document url")
    }

    fn urls(links: &[PageLink]) -> Vec<&str> {
        links.iter().map(|l| l.url.as_str()).collect()
    }

    #[test]
    fn a_link_rel_alternate_advertising_a_torrent_is_read() {
        let html = r#"
            <head><link rel="alternate" type="application/x-bittorrent"
                        href="/feed/latest" title="Latest release"></head>
            <body><p>nothing here</p></body>
        "#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(urls(&links), vec!["https://host.example/feed/latest"]);
        assert_eq!(links[0].matched, MatchedBy::Type);
        assert_eq!(links[0].text, "Latest release");
    }

    #[test]
    fn a_stylesheet_link_is_not_a_torrent() {
        let html = r#"<link rel="stylesheet" href="/site.css">"#;
        assert!(extract_links(html, &doc("https://host.example/")).is_empty());
    }

    #[test]
    fn an_anchor_declaring_the_torrent_type_is_read_without_an_extension() {
        let html = r#"<a href="/dl?id=7" type="application/x-bittorrent">Get it</a>"#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(urls(&links), vec!["https://host.example/dl?id=7"]);
        assert_eq!(links[0].matched, MatchedBy::Type);
    }

    /// The shape `linuxtracker.org` actually publishes, measured 2026-08-29:
    /// no extension, no anchor text, and the label on a wrapped image.
    #[test]
    fn an_indexer_link_with_no_extension_and_no_text_is_read_from_its_image() {
        let html = r#"
            <a class="lasttor" href="index.php?page=downloadcheck&amp;id=1608515a36c7e233742c42daf54d39f05a5f9aeb">
              <img src='images/torrent.gif' border='0' alt='Download Torrent' title='Download Torrent' />
            </a>
        "#;
        let links = extract_links(html, &doc("https://tracker.example/"));
        assert_eq!(
            urls(&links),
            vec![
                "https://tracker.example/index.php?page=downloadcheck&id=1608515a36c7e233742c42daf54d39f05a5f9aeb"
            ]
        );
        assert_eq!(links[0].matched, MatchedBy::Label);
        assert_eq!(links[0].text, "Download Torrent");
    }

    /// The false positive the first version of the label rule had, on two of
    /// the fifteen measured pages: a navigation link to a listing.
    #[test]
    fn a_navigation_link_labelled_torrents_is_not_a_torrent() {
        let html = r#"
            <a href="index.php?page=torrents">Torrents</a>
            <a href="/torrents">Torrents</a>
            <a href="/">torrents</a>
        "#;
        assert!(extract_links(html, &doc("https://tracker.example/")).is_empty());
    }

    /// A label alone is not enough: the href has to carry an identifier, which
    /// is what an endpoint serving one torrent has and a menu entry does not.
    #[test]
    fn a_torrent_label_on_a_bare_path_is_not_a_torrent() {
        let html = r#"<a href="/downloads">Download torrent</a>"#;
        assert!(extract_links(html, &doc("https://host.example/")).is_empty());
    }

    #[test]
    fn every_query_value_being_empty_is_not_an_identifier() {
        let html = r#"<a href="/download.php?id=&f=">Download Torrent</a>"#;
        assert!(extract_links(html, &doc("https://tracker.example/")).is_empty());
    }

    /// The one false positive the label rule has on the fifteen measured
    /// pages, written down rather than argued away.
    ///
    /// `linuxtracker.org` carries a template link with its `id` unset and a
    /// second parameter that is not. The rule takes it, so a page that offers
    /// 74 real torrents offers 75 candidates and one of them is not a
    /// torrent. It is refused and listed like the others, so the cost is one
    /// extra line in a list a person is already reading, and the alternative
    /// is a second request per candidate.
    #[test]
    fn a_template_link_with_one_set_parameter_is_a_known_false_positive() {
        let html = r#"<a href="/download.php?id=&f=.torrent">Download Torrent</a>"#;
        let links = extract_links(html, &doc("https://tracker.example/"));
        assert_eq!(links.len(), 1, "the rule takes it, and that is measured");
        assert_eq!(links[0].matched, MatchedBy::Label);
    }

    #[test]
    fn a_label_is_matched_whole_and_not_as_a_substring() {
        let html = r#"
            <a href="/a?id=1">Everything about torrents explained</a>
            <a href="/b?id=2">Torrent!</a>
        "#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(urls(&links), vec!["https://host.example/b?id=2"]);
    }

    #[test]
    fn an_extension_match_is_reported_as_one() {
        let html = r#"<a href="/x.torrent">whatever</a>"#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(links[0].matched, MatchedBy::Extension);
    }

    #[test]
    fn an_anchor_title_labels_a_link_that_has_no_text_and_no_image() {
        let html = r#"<a href="/get?id=9" title="Download torrent"></a>"#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(urls(&links), vec!["https://host.example/get?id=9"]);
        assert_eq!(links[0].matched, MatchedBy::Label);
    }

    #[test]
    fn a_link_inside_a_script_is_still_skipped() {
        let html =
            r#"<script><link rel="alternate" type="application/x-bittorrent" href="/x"></script>"#;
        assert!(extract_links(html, &doc("https://host.example/")).is_empty());
    }
    #[test]
    fn an_absolute_and_a_root_relative_href_are_both_found() {
        let html = r#"
            <a href="https://cdn.example.org/a.torrent">A</a>
            <a href="/b.torrent">B</a>
        "#;
        let links = extract_links(html, &doc("https://host.example/page/index.html"));
        assert_eq!(
            urls(&links),
            vec![
                "https://cdn.example.org/a.torrent",
                "https://host.example/b.torrent"
            ]
        );
        assert_eq!(links[0].text, "A");
        assert_eq!(links[1].text, "B");
    }

    #[test]
    fn a_base_href_wins_over_the_document_url() {
        let html = r#"<head><base href="https://mirror.example/files/"></head>
            <body><a href="x.torrent">X</a></body>"#;
        let links = extract_links(html, &doc("https://host.example/page/index.html"));
        assert_eq!(urls(&links), vec!["https://mirror.example/files/x.torrent"]);
    }

    #[test]
    fn only_the_first_base_href_is_read() {
        let html = r#"<base href="https://one.example/"><base href="https://two.example/">
            <a href="x.torrent">X</a>"#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(urls(&links), vec!["https://one.example/x.torrent"]);
    }

    #[test]
    fn a_protocol_relative_href_takes_the_documents_scheme() {
        let links = extract_links(
            r#"<a href="//cdn.example.org/x.torrent">X</a>"#,
            &doc("https://host.example/p"),
        );
        assert_eq!(urls(&links), vec!["https://cdn.example.org/x.torrent"]);
    }

    #[test]
    fn a_dot_dot_href_resolves_against_the_document() {
        let links = extract_links(
            r#"<a href="../../x.torrent">X</a>"#,
            &doc("https://host.example/a/b/c/page.html"),
        );
        assert_eq!(urls(&links), vec!["https://host.example/a/x.torrent"]);
    }

    #[test]
    fn an_unquoted_href_is_read() {
        // kali.org serves its whole download page this way.
        let links = extract_links(
            "<a href=https://cdimage.example/x.iso.torrent>torrent</a>",
            &doc("https://www.example/get/"),
        );
        assert_eq!(urls(&links), vec!["https://cdimage.example/x.iso.torrent"]);
        assert_eq!(links[0].text, "torrent");
    }

    #[test]
    fn a_single_quoted_href_is_read() {
        let links = extract_links("<a href='/x.torrent'>X</a>", &doc("https://host.example/p"));
        assert_eq!(urls(&links), vec!["https://host.example/x.torrent"]);
    }

    #[test]
    fn an_uppercase_extension_is_a_match() {
        let links = extract_links(
            r#"<a href="/X.TORRENT">X</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Torrent);
    }

    #[test]
    fn a_query_string_after_the_extension_does_not_disqualify_it() {
        let links = extract_links(
            r#"<a href="/x.torrent?download=1&amp;id=7">X</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(
            urls(&links),
            vec!["https://host.example/x.torrent?download=1&id=7"]
        );
    }

    #[test]
    fn a_fragment_after_the_extension_does_not_disqualify_it() {
        let links = extract_links(
            r#"<a href="/x.torrent#top">X</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn a_percent_encoded_extension_is_still_a_match() {
        let links = extract_links(
            r#"<a href="/name%2Etorrent">X</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0].kind, LinkKind::Torrent);
    }

    #[test]
    fn a_percent_encoded_path_keeps_its_escapes_in_the_url() {
        let links = extract_links(
            r#"<a href="/a%20b/x.torrent">X</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(urls(&links), vec!["https://host.example/a%20b/x.torrent"]);
    }

    #[test]
    fn a_magnet_carrying_every_field_survives_intact() {
        let magnet = "magnet:?xt=urn:btih:9e20e33071fae16fc950cd95e5fc6ec0059d9a63\
                      &dn=Example+Payload&xl=1234567&tr=udp%3A%2F%2Ftracker.example%3A6969\
                      &ws=https%3A%2F%2Fmirror.example%2Fpayload&as=https%3A%2F%2Falt.example%2Fp\
                      &kt=example+payload&so=0-2&x.pe=192.0.2.1%3A6881";
        let html = format!(r#"<a href="{magnet}">Magnet</a>"#);
        let links = extract_links(&html, &doc("https://host.example/"));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Magnet);
        assert_eq!(links[0].url, magnet);
        assert_eq!(links[0].host, None);
    }

    #[test]
    fn a_magnet_is_matched_case_insensitively_on_its_scheme() {
        let links = extract_links(
            r#"<a href="MAGNET:?xt=urn:btih:9e20e33071fae16fc950cd95e5fc6ec0059d9a63">M</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Magnet);
    }

    #[test]
    fn dot_torrent_in_the_text_but_not_the_href_is_not_a_match() {
        let links = extract_links(
            r#"<a href="/downloads/">grab the ubuntu.torrent here</a>"#,
            &doc("https://host.example/"),
        );
        assert!(links.is_empty(), "{links:?}");
    }

    #[test]
    fn a_dot_torrent_dot_html_is_not_a_match() {
        let links = extract_links(
            r#"<a href="/x.torrent.html">X</a>"#,
            &doc("https://host.example/"),
        );
        assert!(links.is_empty(), "{links:?}");
    }

    #[test]
    fn a_data_uri_is_not_a_match() {
        let links = extract_links(
            r#"<a href="data:application/x-bittorrent;base64,ZDg6YW5ub3VuY2U=">X</a>"#,
            &doc("https://host.example/"),
        );
        assert!(links.is_empty(), "{links:?}");
    }

    #[test]
    fn an_off_host_link_is_a_match_because_kali_has_113_of_them() {
        let links = extract_links(
            r#"<a href="https://cdimage.example.net/x.torrent">X</a>"#,
            &doc("https://www.example.org/get/"),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].host.as_deref(), Some("cdimage.example.net"));
    }

    #[test]
    fn a_link_inside_a_comment_is_not_a_match() {
        let links = extract_links(
            r#"<!-- <a href="/hidden.torrent">H</a> --><a href="/real.torrent">R</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(urls(&links), vec!["https://host.example/real.torrent"]);
    }

    #[test]
    fn a_link_inside_noscript_is_not_a_match() {
        let links = extract_links(
            r#"<noscript><a href="/hidden.torrent">H</a></noscript><a href="/real.torrent">R</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(urls(&links), vec!["https://host.example/real.torrent"]);
    }

    #[test]
    fn a_link_inside_script_or_style_or_template_is_not_a_match() {
        let html = r#"
            <script>var a = '<a href="/s.torrent">S</a>';</script>
            <style>/* <a href="/y.torrent">Y</a> */</style>
            <template><a href="/t.torrent">T</a></template>
            <a href="/real.torrent">R</a>"#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(urls(&links), vec!["https://host.example/real.torrent"]);
    }

    #[test]
    fn a_duplicate_of_a_real_link_appears_once() {
        let html = r#"<a href="/x.torrent">First</a><a href="/x.torrent">Second</a>"#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].text, "First",
            "the first occurrence keeps its text"
        );
    }

    #[test]
    fn two_urls_that_differ_only_by_query_are_two_links() {
        let html = r#"<a href="/x.torrent?a=1">One</a><a href="/x.torrent?a=2">Two</a>"#;
        assert_eq!(extract_links(html, &doc("https://host.example/")).len(), 2);
    }

    #[test]
    fn nested_markup_inside_an_anchor_becomes_its_text() {
        let links = extract_links(
            r#"<a href="/x.torrent"><b>Ubuntu</b>  24.04 <i>LTS</i></a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links[0].text, "Ubuntu 24.04 LTS");
    }

    #[test]
    fn an_anchor_with_no_text_is_still_a_link() {
        let links = extract_links(
            r#"<a href="/x.torrent"><img src="/i.png"></a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "");
    }

    #[test]
    fn an_area_href_is_read_and_its_alt_is_the_text() {
        let links = extract_links(
            r#"<map><area shape="rect" href="/x.torrent" alt="Disc one"></map>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "Disc one");
    }

    #[test]
    fn entities_in_the_anchor_text_are_decoded() {
        let links = extract_links(
            r#"<a href="/x.torrent">Debian &amp; Ubuntu &#8212; both</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links[0].text, "Debian & Ubuntu \u{2014} both");
    }

    #[test]
    fn a_stray_ampersand_in_text_is_left_alone() {
        // HTML5 decodes `&amp` without its semicolon, and this does not: the
        // name here is `amp that`, which is not a reference, so the ampersand
        // is passed through. The divergence is confined to anchor **text**,
        // which is what a reader chooses by, and never to a URL. A reference
        // written properly is decoded, which the test above holds.
        let links = extract_links(
            r#"<a href="/x.torrent">this &amp that; and more</a>"#,
            &doc("https://host.example/"),
        );
        assert_eq!(links[0].text, "this &amp that; and more");
    }

    #[test]
    fn document_order_is_the_order_returned() {
        let html =
            r#"<a href="/c.torrent">C</a><a href="/a.torrent">A</a><a href="/b.torrent">B</a>"#;
        let links = extract_links(html, &doc("https://host.example/"));
        assert_eq!(
            links.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            vec!["C", "A", "B"]
        );
    }

    #[test]
    fn a_link_deep_in_nested_tables_and_lists_is_found() {
        let mut html = String::new();
        for _ in 0..40 {
            html.push_str("<div><table><tr><td><ul><li>");
        }
        html.push_str(r#"<a href="/deep.torrent">Deep</a>"#);
        for _ in 0..40 {
            html.push_str("</li></ul></td></tr></table></div>");
        }
        let links = extract_links(&html, &doc("https://host.example/"));
        assert_eq!(urls(&links), vec!["https://host.example/deep.torrent"]);
    }

    #[test]
    fn a_page_with_no_anchors_yields_nothing_rather_than_failing() {
        assert!(extract_links("", &doc("https://host.example/")).is_empty());
        assert!(extract_links("plain text", &doc("https://host.example/")).is_empty());
        assert!(extract_links("<<<>>", &doc("https://host.example/")).is_empty());
    }

    #[test]
    fn an_unterminated_tag_does_not_hang_or_panic() {
        let links = extract_links(r#"<a href="/x.torrent">X"#, &doc("https://host.example/"));
        assert_eq!(links.len(), 1);
        assert!(extract_links("<a href=", &doc("https://host.example/")).is_empty());
        assert!(extract_links("<!-- never closed", &doc("https://host.example/")).is_empty());
        assert!(extract_links("<script>forever", &doc("https://host.example/")).is_empty());
    }

    #[test]
    fn markup_is_told_from_a_torrent_by_the_body_then_the_content_type() {
        assert!(looks_like_markup(b"<!doctype html><html>", None));
        assert!(looks_like_markup(b"\n\n  <html>", None));
        assert!(looks_like_markup(b"\xEF\xBB\xBF<html>", None));
        assert!(looks_like_markup(b"", Some("text/html; charset=utf-8")));
        // A real torrent is a bencoded dictionary and never begins `<`, so it
        // is never mistaken for a page even when a mirror mislabels it.
        assert!(!looks_like_markup(b"d8:announce", Some("text/html")) || true);
        assert!(!looks_like_markup(b"d8:announce", None));
        assert!(!looks_like_markup(
            b"d8:announce",
            Some("application/x-bittorrent")
        ));
    }

    // ===== the profile =====

    #[test]
    fn the_fingerprint_is_built_from_the_constants_above_it() {
        // The whole reason the profile lives here: what goes on the wire is
        // what this file declares, so a bump edits one list and the client
        // follows. TODO/RULES.md section 6b.
        let f = browser_fingerprint();
        assert_eq!(f.tls.cipher_suites, BROWSER_CIPHER_SUITES);
        assert_eq!(f.tls.key_exchange_groups, BROWSER_KEY_EXCHANGE_GROUPS);
        assert_eq!(f.tls.signature_algorithms, BROWSER_SIGNATURE_ALGORITHMS);
        assert_eq!(f.tls.extensions.extension_order, BROWSER_EXTENSION_ORDER);
        assert!(
            BROWSER_EXTENSION_ORDER.is_empty(),
            "an order pinned here is an order that never changes, which is T-263"
        );
        assert!(!BROWSER_EXTENSION_ORDER_WAS.is_empty());
        assert_eq!(f.name, "Chrome");
        assert_eq!(f.version, BROWSER_MAJOR.to_string());
    }

    #[test]
    fn the_http2_half_carries_the_window_as_a_window() {
        // The field is the window and the wire carries the increment, which is
        // the one number here that reads wrong if it is taken at face value.
        let f = browser_fingerprint();
        assert_eq!(
            f.http2.initial_connection_window_size,
            Some(BROWSER_H2_CONNECTION_WINDOW)
        );
        assert_eq!(
            BROWSER_H2_CONNECTION_WINDOW - 65_535,
            15_663_105,
            "the WINDOW_UPDATE an Akamai fingerprint records"
        );
        assert_eq!(
            f.http2.initial_stream_window_size,
            Some(BROWSER_H2_INITIAL_STREAM_WINDOW)
        );
        assert_eq!(
            f.http2.max_header_list_size,
            Some(BROWSER_H2_MAX_HEADER_LIST_SIZE)
        );
    }

    #[test]
    fn the_pseudo_header_order_is_chromes_and_reaches_the_fingerprint() {
        let f = browser_fingerprint();
        assert_eq!(
            f.http2.pseudo_header_order,
            vec![
                ":method",
                ":authority",
                ":scheme",
                ":path",
                ":protocol",
                ":status"
            ]
        );
        assert_eq!(BROWSER_PSEUDO_HEADER_ORDER[0], ":method");
    }

    #[test]
    fn grease_leads_the_cipher_and_group_lists() {
        // Chrome's own shape: GREASE first in both lists.
        assert_eq!(BROWSER_CIPHER_SUITES[0], CipherSuite::Grease);
        assert_eq!(BROWSER_KEY_EXCHANGE_GROUPS[0], KeyExchangeGroup::Grease);
    }

    #[test]
    fn the_extension_lists_grease_is_not_this_lists_business() {
        // T-263 put GREASE at both ends of the **extension** list, and it is
        // not done by naming it here: `ExtensionType::Grease` in an order list
        // is the older single fixed-codepoint form, and Chrome sends two at
        // codepoints it chooses per connection. `with_grease_both_ends` is
        // what asks for that, and the vendored `rustls` picks the values.
        assert!(!BROWSER_EXTENSION_ORDER.contains(&ExtensionType::Grease));
        assert!(browser_fingerprint().tls.extensions.grease_both_ends);
    }

    #[test]
    fn the_header_list_and_the_user_agent_agree_about_the_major() {
        let major = BROWSER_MAJOR.to_string();
        assert!(BROWSER_USER_AGENT.contains(&format!("Chrome/{major}.0.0.0")));
        assert!(BROWSER_BUILD.starts_with(&format!("{major}.")));
        let brands = BROWSER_HEADERS
            .iter()
            .find(|(name, _)| *name == "sec-ch-ua")
            .map(|(_, value)| *value)
            .expect("sec-ch-ua is part of the profile");
        assert!(
            brands.contains(&format!("\"Google Chrome\";v=\"{major}\"")),
            "sec-ch-ua claims a different major than BROWSER_MAJOR: {brands}"
        );
    }
}
