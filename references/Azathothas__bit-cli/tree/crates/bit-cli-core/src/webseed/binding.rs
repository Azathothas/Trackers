//! Bindings: the `(source, scope, composition)` triple.
//!
//! Every other client treats a web seed as one flat thing, a URL that serves
//! the whole torrent. In `bit-cli` a web seed is three orthogonal parts:
//!
//! - **Source**: where the bytes come from, plus its headers, auth, timeouts,
//!   concurrency, and rate limit.
//! - **Scope**: which part of the torrent it is allowed to serve. See
//!   [`super::scope`].
//! - **Composition**: how the request URL is built. See [`super::composition`].
//!
//! Any source can serve any scope under any composition. That orthogonality is
//! the feature, and every combination the grammar allows has to work.
//!
//! Resolution happens before a single byte is requested. A [`BindingSet`]
//! answers "which source is responsible for piece N" and "which pieces has
//! nothing at all", so a misconfigured mirror list fails immediately with the
//! uncovered piece indices named rather than stalling at 94 percent.

use std::collections::BTreeMap;
use std::ops::Range;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::span::{SpanSet, summarize_indices};
use crate::webseed::composition::{Mode, RequestContext, check_mode, compose};
use crate::webseed::scope::{ResolvedScope, Scope};

/// Where a source came from. Reported in `--json` so a caller can tell what it
/// asked for apart from what the torrent already carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// A `--web-seed*` flag on the command line.
    CommandLine,
    /// The torrent's own `url-list` key (BEP 19).
    TorrentUrlList,
    /// The torrent's own `httpseeds` key (BEP 17).
    TorrentHttpSeeds,
    /// A `--web-seed-file` list.
    File,
    /// A `--web-seed-list-url` list fetched over HTTP.
    ListUrl,
    /// A `--web-seed-config` binding table.
    Config,
    /// A mirror listed in a Metalink.
    Metalink,
    /// A file another torrent in the same run already holds, proven to be the
    /// same bytes by its piece hashes.
    SharedFile,
}

impl Origin {
    /// A stable name for output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CommandLine => "command_line",
            Self::TorrentUrlList => "torrent_url_list",
            Self::TorrentHttpSeeds => "torrent_httpseeds",
            Self::File => "file",
            Self::ListUrl => "list_url",
            Self::Config => "config",
            Self::Metalink => "metalink",
            Self::SharedFile => "shared_file",
        }
    }

    /// Whether the source came out of the `.torrent` rather than from the
    /// caller. `--no-torrent-web-seed` drops exactly these.
    pub const fn is_from_torrent(self) -> bool {
        matches!(self, Self::TorrentUrlList | Self::TorrentHttpSeeds)
    }
}

/// Which wire style a source speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// Decide from the torrent key the URL came from, then from what the
    /// server answers to a probe.
    #[default]
    Auto,
    /// BEP 19: ranged GETs against a per-file URL.
    GetRight,
    /// BEP 17: piece-indexed URLs with `?info_hash=&piece=`.
    Hoffman,
}

impl Style {
    /// Parse a style name.
    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "getright" | "get-right" | "bep19" => Self::GetRight,
            "hoffman" | "bep17" => Self::Hoffman,
            other => {
                return Err(Error::binding(format!(
                    "`{other}` is not a web seed style (use getright, hoffman, or auto)"
                ))
                .with("style", other.to_string()));
            }
        })
    }

    /// The style name as written on the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::GetRight => "getright",
            Self::Hoffman => "hoffman",
        }
    }
}

/// How a source authenticates.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Auth {
    /// No credentials.
    #[default]
    None,
    /// HTTP Basic.
    Basic { user: String, password: String },
    /// A bearer token in the `Authorization` header.
    Bearer { token: String },
    /// Credentials from `.netrc`, looked up by host.
    Netrc,
}

impl Auth {
    /// Parse an auth spec: `basic:user:pass`, `bearer:TOKEN`, `netrc`, `none`.
    ///
    /// The password may itself contain colons, so only the first two are
    /// separators.
    pub fn parse(spec: &str) -> Result<Self> {
        let spec = spec.trim();
        if spec.eq_ignore_ascii_case("none") || spec.is_empty() {
            return Ok(Self::None);
        }
        if spec.eq_ignore_ascii_case("netrc") {
            return Ok(Self::Netrc);
        }
        if let Some(rest) = strip_prefix_ci(spec, "bearer:") {
            return Ok(Self::Bearer {
                token: rest.to_string(),
            });
        }
        if let Some(rest) = strip_prefix_ci(spec, "basic:") {
            let (user, password) = rest.split_once(':').ok_or_else(|| {
                Error::usage("basic auth needs `basic:user:password`").with("spec", "basic:...")
            })?;
            return Ok(Self::Basic {
                user: user.to_string(),
                password: password.to_string(),
            });
        }
        Err(Error::usage(format!(
            "`{spec}` is not an auth spec (use basic:user:pass, bearer:TOKEN, netrc, or none)"
        )))
    }

    /// Whether credentials are present. Used to decide what to redact.
    pub fn is_some(&self) -> bool {
        !matches!(self, Self::None)
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

/// A set of HTTP status codes, written as codes and inclusive ranges.
///
/// `403`, `403,429`, `500-599`, and any mixture. Empty means the caller has no
/// opinion and the built-in classification stands.
///
/// It exists because whether a status is worth retrying is a property of the
/// server, not of the code. A CDN that signs URLs answers `403` when a
/// signature expires and the next request to the stable URL succeeds, so that
/// `403` is transient. A mirror behind a proxy that answers `404` from one
/// edge node while another has the file is the same shape. Neither is
/// knowable from the code alone, so the caller says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StatusSetRepr", into = "Vec<String>")]
pub struct StatusSet {
    /// Inclusive `(low, high)` pairs, in the order they were written.
    ranges: Vec<(u16, u16)>,
}

/// The lowest status an HTTP response can carry, and the highest.
///
/// A value outside this is a typo, not a status, and taking it silently would
/// leave a caller believing a policy is in force that can never match.
const STATUS_MIN: u16 = 100;
const STATUS_MAX: u16 = 599;

impl StatusSet {
    /// Parse `403,429,500-599`.
    pub fn parse(text: &str) -> Result<Self> {
        let mut ranges = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            ranges.push(parse_status_range(part)?);
        }
        Ok(Self { ranges })
    }

    /// Whether the set names this status.
    pub fn contains(&self, code: u16) -> bool {
        self.ranges
            .iter()
            .any(|(lo, hi)| code >= *lo && code <= *hi)
    }

    /// Whether the caller named anything at all.
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// The first status named by both sets, if any.
    ///
    /// A code in both `retry_status` and `fatal_status` has no defensible
    /// meaning, and picking one silently would hide the mistake, so the caller
    /// is told instead.
    pub fn overlap(&self, other: &Self) -> Option<u16> {
        self.ranges
            .iter()
            .find_map(|(lo, hi)| (*lo..=*hi).find(|code| other.contains(*code)))
    }

    /// The canonical spelling, as `webseed list` prints it.
    pub fn to_text(&self) -> String {
        let parts: Vec<String> = self.into();
        parts.join(",")
    }
}

impl From<&StatusSet> for Vec<String> {
    fn from(set: &StatusSet) -> Self {
        set.ranges
            .iter()
            .map(|(lo, hi)| match lo == hi {
                true => lo.to_string(),
                false => format!("{lo}-{hi}"),
            })
            .collect()
    }
}

impl From<StatusSet> for Vec<String> {
    fn from(set: StatusSet) -> Self {
        (&set).into()
    }
}

/// One code or one inclusive range.
fn parse_status_range(part: &str) -> Result<(u16, u16)> {
    let code = |text: &str| -> Result<u16> {
        let value: u16 = text.trim().parse().map_err(|_| {
            Error::usage(format!("`{text}` is not an HTTP status code")).with("value", part)
        })?;
        match (STATUS_MIN..=STATUS_MAX).contains(&value) {
            true => Ok(value),
            false => Err(Error::usage(format!(
                "{value} is not an HTTP status code; they run {STATUS_MIN} to {STATUS_MAX}"
            ))
            .with("value", part)),
        }
    };
    // `split_once` rather than `split`, so `500-599-600` is refused rather
    // than read as its first two parts.
    match part.split_once('-') {
        None => {
            let one = code(part)?;
            Ok((one, one))
        }
        Some((lo, hi)) => {
            let (lo, hi) = (code(lo)?, code(hi)?);
            match lo <= hi {
                true => Ok((lo, hi)),
                false => Err(Error::usage(format!(
                    "the range {lo}-{hi} runs backwards; write it {hi}-{lo}"
                ))
                .with("value", part)),
            }
        }
    }
}

/// How a status set is written in a table: one string, or a list of codes and
/// range strings.
#[derive(Deserialize)]
#[serde(untagged)]
enum StatusSetRepr {
    Text(String),
    Items(Vec<StatusItem>),
}

/// One entry in a status list. TOML writes a bare code as an integer and a
/// range has to be a string, so both are accepted.
#[derive(Deserialize)]
#[serde(untagged)]
enum StatusItem {
    Code(u16),
    Text(String),
}

impl TryFrom<StatusSetRepr> for StatusSet {
    type Error = String;

    fn try_from(repr: StatusSetRepr) -> std::result::Result<Self, String> {
        let text = match repr {
            StatusSetRepr::Text(text) => text,
            StatusSetRepr::Items(items) => items
                .into_iter()
                .map(|item| match item {
                    StatusItem::Code(code) => code.to_string(),
                    StatusItem::Text(text) => text,
                })
                .collect::<Vec<_>>()
                .join(","),
        };
        Self::parse(&text).map_err(|e| e.to_string())
    }
}

/// Per-source tuning. Every field has a default so a binding table only has to
/// name what it changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLimits {
    /// Concurrent ranged requests against this source.
    pub concurrency: usize,
    /// Peer connections the source is presented over.
    ///
    /// One source is one peer to the torrent session, and a peer's received
    /// blocks are written and verified one at a time on that connection's own
    /// task. That path is what bounds the transfer, so presenting the same
    /// source over several connections gives it several of them. See
    /// `TODO/webseed.md`, T-009, for the measurement.
    ///
    /// The concurrency above is divided between them rather than multiplied
    /// by them: the point is more receive paths, not more requests at the
    /// mirror.
    #[serde(default = "one")]
    pub connections: usize,
    /// Bytes per ranged request. Independent of the torrent's piece length.
    pub chunk_size: u64,
    /// Per-request timeout in milliseconds.
    pub timeout_ms: u64,
    /// Connect timeout in milliseconds.
    pub connect_timeout_ms: u64,
    /// Per-request retries before the attempt counts as an error.
    pub retries: u32,
    /// Errors before the source is cooled down.
    pub max_errors: u32,
    /// How long a source that spent its error budget stays out, in
    /// milliseconds. Zero, the default, means it does not come back.
    ///
    /// The bridge sleeps this out and then reconnects with the error run
    /// cleared. Zero retires the source for the rest of the run instead, which
    /// is what keeps an unattended run against one dead mirror failing in
    /// seconds rather than sitting on a timer. See `TODO/multi-source.md`,
    /// T-137.
    pub cooldown_ms: u64,
    /// Bytes per second cap, or `None` for unlimited.
    pub rate_limit: Option<u64>,
    /// Statuses this source retries that it would otherwise retire on.
    #[serde(default, skip_serializing_if = "StatusSet::is_empty")]
    pub retry_status: StatusSet,
    /// Statuses this source retires on that it would otherwise retry.
    #[serde(default, skip_serializing_if = "StatusSet::is_empty")]
    pub fatal_status: StatusSet,
}

impl Default for SourceLimits {
    fn default() -> Self {
        Self {
            concurrency: 4,
            connections: 1,
            chunk_size: 4 * crate::units::MIB,
            timeout_ms: 30_000,
            connect_timeout_ms: 10_000,
            retries: 3,
            max_errors: 5,
            cooldown_ms: 0,
            rate_limit: None,
            retry_status: StatusSet::default(),
            fatal_status: StatusSet::default(),
        }
    }
}

/// The default for [`SourceLimits::connections`], for `serde`.
///
/// A binding table written before this field existed leaves it out, and one
/// connection is what those tables meant.
fn one() -> usize {
    1
}

impl SourceLimits {
    /// Connections to present this source over, at least one.
    pub fn connections(&self) -> usize {
        self.connections.max(1)
    }

    /// Concurrent requests each connection gets.
    ///
    /// The source's whole budget divided between its connections, rounded up,
    /// so four connections sharing a budget of eight get two each. Dividing
    /// rather than multiplying is the point: a source presented over four
    /// connections should not hit the mirror four times harder.
    pub fn per_connection_concurrency(&self) -> usize {
        self.concurrency.max(1).div_ceil(self.connections()).max(1)
    }

    /// The per-request timeout.
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    /// The connect timeout.
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms)
    }

    /// How long a failed source stays out.
    pub fn cooldown(&self) -> Duration {
        Duration::from_millis(self.cooldown_ms)
    }

    /// Refuse a status policy that says two things about one code.
    pub fn check_status_policy(&self) -> Result<()> {
        match self.retry_status.overlap(&self.fatal_status) {
            None => Ok(()),
            Some(code) => Err(Error::usage(format!(
                "status {code} is in both the retry list and the fatal list; it can be one or the other"
            ))
            .with("retry_status", self.retry_status.to_text())
            .with("fatal_status", self.fatal_status.to_text())),
        }
    }

    /// Whether a failure carrying this status should be retried.
    ///
    /// `None` leaves the built-in classification alone, which is what an empty
    /// policy means and what almost every source runs with.
    pub fn status_is_retryable(&self, code: u16) -> Option<bool> {
        if self.fatal_status.contains(code) {
            return Some(false);
        }
        if self.retry_status.contains(code) {
            return Some(true);
        }
        None
    }
}

/// One declared source, before it is resolved against a torrent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpec {
    /// The base URL, or the template when `mode` is `template`.
    pub url: String,
    /// What part of the torrent this source may serve.
    #[serde(default = "Scope::all")]
    pub scope: Scope,
    /// How request URLs are built.
    #[serde(default)]
    pub mode: Mode,
    /// The template, when `mode` is `template` and the template is not `url`
    /// itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Wire style.
    #[serde(default)]
    pub style: Style,
    /// Bias among sources. Higher wins when several can serve a piece.
    #[serde(default)]
    pub priority: i32,
    /// Extra request headers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// User-Agent for this source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// Credentials.
    #[serde(default)]
    pub auth: Auth,
    /// Tuning.
    #[serde(default)]
    pub limits: SourceLimits,
    /// Where the source came from.
    #[serde(default = "default_origin")]
    pub origin: Origin,
}

fn default_origin() -> Origin {
    Origin::Config
}

impl SourceSpec {
    /// A source serving the whole torrent under BEP 19 defaults.
    pub fn new(url: impl Into<String>, origin: Origin) -> Self {
        Self {
            url: url.into(),
            scope: Scope::all(),
            mode: Mode::Auto,
            template: None,
            style: Style::Auto,
            priority: 0,
            headers: BTreeMap::new(),
            user_agent: None,
            auth: Auth::None,
            limits: SourceLimits::default(),
            origin,
        }
    }

    /// Set the scope.
    #[must_use]
    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// Set the composition mode.
    #[must_use]
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the template.
    #[must_use]
    pub fn with_template(mut self, template: impl Into<String>) -> Self {
        self.template = Some(template.into());
        self.mode = Mode::Template;
        self
    }

    /// Set the priority.
    #[must_use]
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// The template text, falling back to the URL itself.
    ///
    /// Writing the template in `url` is what a command line does
    /// (`--web-seed 'https://e/{piece}.bin' --web-seed-mode template`), and
    /// writing it in `template` is what a binding table does. Both work.
    pub fn template_text(&self) -> Option<&str> {
        match self.mode {
            Mode::Template => Some(self.template.as_deref().unwrap_or(&self.url)),
            _ => None,
        }
    }

    /// Whether this source reads a local path rather than the network.
    pub fn is_local(&self) -> bool {
        let probe = match self.template_text() {
            Some(template) => template.split('{').next().unwrap_or(template),
            None => &self.url,
        };
        crate::webseed::local::is_file_url(probe.trim())
    }

    /// Check the URL is one `bit-cli` can fetch from.
    ///
    /// BEP 17 and BEP 19 define HTTP and FTP sources. FTP is out of scope.
    /// `file:` is not in either BEP and is not offered to a swarm: it is a
    /// source for this process only, for bytes that are already on the disk
    /// under some other name. See `TODO/multi-source.md`, T-133.
    pub fn validate_url(&self) -> Result<()> {
        // A template is not a URL until it is expanded, so only its literal
        // prefix can be checked here.
        let probe = match self.template_text() {
            Some(template) => template.split('{').next().unwrap_or(template),
            None => &self.url,
        };
        let lower = probe.trim().to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return Ok(());
        }
        if crate::webseed::local::is_file_url(&lower) {
            // Resolve it now rather than at the first read, so a malformed
            // path is reported beside the other binding errors.
            crate::webseed::local::path_of(probe.trim())?;
            if self.style == Style::Hoffman {
                return Err(Error::binding(format!(
                    "{}: BEP 17 is an HTTP wire style and a file: source does not speak it",
                    self.url
                ))
                .with("url", self.url.clone()));
            }
            return Ok(());
        }
        let reason = if lower.starts_with("ftp://") || lower.starts_with("sftp://") {
            "FTP is not a valid web seed transport under BEP 17 or BEP 19"
        } else if probe.contains("://") {
            "only http, https, and file sources are supported"
        } else {
            "a web seed source must be an absolute http, https, or file URL"
        };
        Err(Error::binding(format!("{}: {reason}", self.url)).with("url", self.url.clone()))
    }
}

/// A source resolved against one torrent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    /// Position in the binding set, stable for the run so output and logs can
    /// refer to a source by number.
    pub index: usize,
    /// The source as declared.
    pub spec: SourceSpec,
    /// What the scope resolved to.
    pub scope: ResolvedScope,
    /// The URL each in-scope file would be requested from. Absent for
    /// per-request compositions, where the URL is not a function of the file
    /// alone.
    pub file_urls: Vec<FileUrl>,
}

/// One file and the URL it resolves to under this binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUrl {
    /// File index within the torrent.
    pub index: usize,
    /// The `/`-separated path within the torrent.
    pub path: String,
    /// Total size of the file in bytes.
    pub length: u64,
    /// Bytes of this file that are in scope.
    pub in_scope_bytes: u64,
    /// The URL that would be requested.
    pub url: String,
}

impl Binding {
    /// Whether this binding may serve `range`.
    pub fn covers(&self, range: &Range<u64>) -> bool {
        self.scope.spans.contains_range(range)
    }

    /// The URL for one byte range.
    ///
    /// A range that crosses a file boundary has no single URL under any
    /// composition, so the caller splits by file first. [`Self::request_urls`]
    /// does that.
    pub fn url_for(
        &self,
        layout: &Layout,
        info_hash: &str,
        offset: u64,
        length: u64,
    ) -> Result<String> {
        let ctx = RequestContext::for_range(layout, info_hash, offset, length);
        compose(
            &self.spec.url,
            self.spec.mode,
            self.spec.template_text(),
            &ctx,
        )
    }

    /// Split `range` into the per-file requests this binding would issue.
    ///
    /// Every returned request is inside the binding's scope. A range that
    /// reaches outside is an error rather than a request the server answers
    /// with 416, because an out-of-scope request is a bug in `bit-cli` and
    /// should read as one.
    pub fn request_urls(
        &self,
        layout: &Layout,
        info_hash: &str,
        range: Range<u64>,
    ) -> Result<Vec<RangeRequest>> {
        if !self.covers(&range) {
            let outside = SpanSet::from_range(range.clone()).difference(&self.scope.spans);
            return Err(Error::binding(format!(
                "source {} is scoped to `{}` and cannot serve bytes {}-{}",
                self.spec.url,
                self.scope.selector,
                range.start,
                range.end.saturating_sub(1)
            ))
            .with("url", self.spec.url.clone())
            .with("selector", self.scope.selector.clone())
            .with("requested_start", range.start)
            .with("requested_end", range.end)
            .with(
                "out_of_scope",
                serde_json::to_value(&outside).unwrap_or_default(),
            ));
        }
        let mut out = Vec::new();
        for slice in layout.split_by_file(range) {
            let file = layout.file(slice.file).ok_or_else(|| {
                Error::generic(format!(
                    "file index {} vanished from the layout",
                    slice.file
                ))
            })?;
            let absolute = file.offset + slice.offset;
            let ctx = RequestContext {
                layout,
                info_hash,
                file: Some(slice.file),
                piece: layout.piece_at(absolute),
                offset: absolute,
                length: slice.length,
            };
            let url = compose(
                &self.spec.url,
                self.spec.mode,
                self.spec.template_text(),
                &ctx,
            )?;
            out.push(RangeRequest {
                url,
                file: slice.file,
                file_offset: slice.offset,
                torrent_offset: absolute,
                length: slice.length,
            });
        }
        Ok(out)
    }
}

/// One ranged GET this binding would issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeRequest {
    /// The URL to request.
    pub url: String,
    /// File index within the torrent.
    pub file: usize,
    /// Offset within that file, which is what the `Range` header carries.
    pub file_offset: u64,
    /// Offset within the torrent's linear payload.
    pub torrent_offset: u64,
    /// Length in bytes.
    pub length: u64,
}

impl RangeRequest {
    /// The `Range` header value, with an inclusive end as HTTP requires.
    pub fn range_header(&self) -> String {
        format!(
            "bytes={}-{}",
            self.file_offset,
            self.file_offset + self.length - 1
        )
    }
}

/// Every binding for one torrent, plus what they do and do not cover.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingSet {
    /// The resolved bindings, in declaration order.
    pub bindings: Vec<Binding>,
    /// Bytes at least one binding can serve.
    pub covered: SpanSet,
    /// Bytes no binding can serve.
    pub uncovered: SpanSet,
    /// Piece indices no binding covers in full. A piece only partly covered
    /// appears here, because a partial piece never verifies.
    pub uncovered_pieces: Vec<u32>,
}

impl BindingSet {
    /// Resolve every source against a torrent.
    ///
    /// Validation happens for all sources before any is used: URLs are
    /// checked, scopes are resolved, and `exact` is rejected where it cannot
    /// work. One bad entry in a generated mirror list fails the run with that
    /// entry named.
    pub fn resolve(layout: &Layout, info_hash: &str, specs: &[SourceSpec]) -> Result<Self> {
        let mut bindings = Vec::with_capacity(specs.len());
        let mut covered = SpanSet::new();
        for (index, spec) in specs.iter().enumerate() {
            spec.validate_url()?;
            let scope = spec.scope.resolve(layout)?;
            check_mode(
                spec.mode,
                layout,
                scope.files.len(),
                &scope.selector,
                &spec.url,
            )?;

            let file_urls = match spec.mode.is_per_request() {
                true => Vec::new(),
                false => scope
                    .files
                    .iter()
                    .map(|&file| {
                        let entry = layout.file(file).ok_or_else(|| {
                            Error::generic(format!("file index {file} vanished from the layout"))
                        })?;
                        let ctx = RequestContext::for_file(layout, info_hash, file)
                            .ok_or_else(|| Error::generic("could not build a request context"))?;
                        let in_scope = scope
                            .spans
                            .intersection(&SpanSet::from_range(entry.range()))
                            .len();
                        Ok(FileUrl {
                            index: file,
                            path: entry.display_path(),
                            length: entry.length,
                            in_scope_bytes: in_scope,
                            url: compose(&spec.url, spec.mode, spec.template_text(), &ctx)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            };

            covered = covered.union(&scope.spans);
            bindings.push(Binding {
                index,
                spec: spec.clone(),
                scope,
                file_urls,
            });
        }

        let uncovered = covered.gaps_in(layout.payload());
        let uncovered_pieces = (0..layout.piece_count())
            .filter(|&piece| {
                layout
                    .piece_range(piece)
                    .is_some_and(|range| !covered.contains_range(&range))
            })
            .collect();

        Ok(Self {
            bindings,
            covered,
            uncovered,
            uncovered_pieces,
        })
    }

    /// Whether the bindings cover the whole payload.
    pub fn is_complete(&self) -> bool {
        self.uncovered.is_empty()
    }

    /// Bindings that can serve every byte of `piece`, best first.
    ///
    /// The order is the picker's preference: priority descending, then
    /// declaration order so the result is deterministic. Measured throughput
    /// is layered on top of this by the caller, which is the only part that
    /// cannot be decided statically.
    pub fn sources_for_piece(&self, layout: &Layout, piece: u32) -> Vec<&Binding> {
        let Some(range) = layout.piece_range(piece) else {
            return Vec::new();
        };
        let mut out: Vec<&Binding> = self.bindings.iter().filter(|b| b.covers(&range)).collect();
        out.sort_by_key(|b| (std::cmp::Reverse(b.spec.priority), b.index));
        out
    }

    /// Bindings that can serve any part of `range`, best first.
    pub fn sources_for_range(&self, range: &Range<u64>) -> Vec<&Binding> {
        let wanted = SpanSet::from_range(range.clone());
        let mut out: Vec<&Binding> = self
            .bindings
            .iter()
            .filter(|b| !b.scope.spans.intersection(&wanted).is_empty())
            .collect();
        out.sort_by_key(|b| (std::cmp::Reverse(b.spec.priority), b.index));
        out
    }

    /// Fail when the bindings leave a gap that the given peer coverage cannot
    /// close.
    ///
    /// `peers_cover_everything` is what the caller knows about the swarm: with
    /// peers available, a gap in the HTTP sources is fine. With
    /// `--web-seed-only`, or with no peers, it is a hard error naming the
    /// uncovered pieces, because the alternative is a run that stalls at 94
    /// percent and never says why.
    pub fn require_coverage(&self, peers_cover_everything: bool) -> Result<()> {
        if peers_cover_everything || self.is_complete() {
            return Ok(());
        }
        let summary = summarize_indices(&self.uncovered_pieces);
        Err(Error::coverage_gap(format!(
            "no source can serve piece(s) {summary} and no peers are available to cover them"
        ))
        .with(
            "uncovered_pieces",
            serde_json::to_value(&self.uncovered_pieces).unwrap_or_default(),
        )
        .with("uncovered_piece_count", self.uncovered_pieces.len())
        .with("uncovered_bytes", self.uncovered.len())
        .with(
            "uncovered_spans",
            serde_json::to_value(&self.uncovered).unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0102030405060708090a0b0c0d0e0f1011121314";

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

    fn spec(url: &str, scope: &str) -> SourceSpec {
        SourceSpec::new(url, Origin::CommandLine).with_scope(Scope::parse(scope).unwrap())
    }

    #[test]
    fn a_whole_torrent_source_covers_everything() {
        let set = BindingSet::resolve(&layout(), HASH, &[spec("https://e.com/pub/", "*")]).unwrap();
        assert!(set.is_complete());
        assert!(set.uncovered.is_empty());
        assert!(set.uncovered_pieces.is_empty());
        assert_eq!(set.bindings[0].file_urls.len(), 3);
        assert_eq!(
            set.bindings[0].file_urls[0].url,
            "https://e.com/pub/album/disc%201/a.flac"
        );
    }

    #[test]
    fn file_urls_report_how_much_of_each_file_is_in_scope() {
        let set =
            BindingSet::resolve(&layout(), HASH, &[spec("https://e.com/", "byte:0-1000")]).unwrap();
        let urls = &set.bindings[0].file_urls;
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].index, 0);
        assert_eq!(urls[0].length, 1500);
        assert_eq!(urls[0].in_scope_bytes, 1000);
    }

    #[test]
    fn two_partial_sources_can_add_up_to_full_coverage() {
        let set = BindingSet::resolve(
            &layout(),
            HASH,
            &[
                spec("https://a.com/", "piece:0-1"),
                spec("https://b.com/", "piece:2"),
            ],
        )
        .unwrap();
        assert!(set.is_complete(), "uncovered: {}", set.uncovered);
        assert!(set.require_coverage(false).is_ok());
    }

    #[test]
    fn a_gap_names_the_uncovered_pieces() {
        let set =
            BindingSet::resolve(&layout(), HASH, &[spec("https://a.com/", "piece:0")]).unwrap();
        assert!(!set.is_complete());
        assert_eq!(set.uncovered_pieces, vec![1, 2]);
        let err = set.require_coverage(false).unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::CoverageGap);
        assert!(err.message().contains("piece(s) 1-2"), "{}", err.message());
        assert_eq!(err.context()["uncovered_piece_count"], 2);
    }

    #[test]
    fn a_gap_is_fine_when_peers_can_cover_it() {
        let set =
            BindingSet::resolve(&layout(), HASH, &[spec("https://a.com/", "piece:0")]).unwrap();
        assert!(set.require_coverage(true).is_ok());
    }

    #[test]
    fn a_partially_covered_piece_counts_as_uncovered() {
        // File 1 is 1500..2000; piece 1 is 1024..2048, so scoping to file 1
        // covers no whole piece.
        let set = BindingSet::resolve(&layout(), HASH, &[spec("https://a.com/", "1")]).unwrap();
        assert!(
            set.uncovered_pieces.contains(&1),
            "a half-covered piece never verifies"
        );
    }

    #[test]
    fn priority_orders_the_sources_for_a_piece() {
        let layout = layout();
        let fallback = spec("https://slow.com/", "*").with_priority(1);
        let primary = spec("https://fast.com/", "*").with_priority(10);
        let set = BindingSet::resolve(&layout, HASH, &[fallback, primary]).unwrap();
        let ordered = set.sources_for_piece(&layout, 0);
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].spec.url, "https://fast.com/");
        assert_eq!(ordered[1].spec.url, "https://slow.com/");
    }

    #[test]
    fn equal_priority_falls_back_to_declaration_order() {
        let layout = layout();
        let set = BindingSet::resolve(
            &layout,
            HASH,
            &[
                spec("https://first.com/", "*"),
                spec("https://second.com/", "*"),
            ],
        )
        .unwrap();
        let ordered = set.sources_for_piece(&layout, 0);
        assert_eq!(ordered[0].spec.url, "https://first.com/");
    }

    #[test]
    fn a_source_is_only_offered_pieces_it_holds_in_full() {
        let layout = layout();
        let set = BindingSet::resolve(&layout, HASH, &[spec("https://a.com/", "piece:0")]).unwrap();
        assert_eq!(set.sources_for_piece(&layout, 0).len(), 1);
        assert!(set.sources_for_piece(&layout, 1).is_empty());
    }

    #[test]
    fn requests_split_at_file_boundaries() {
        let layout = layout();
        let set = BindingSet::resolve(&layout, HASH, &[spec("https://e.com/pub/", "*")]).unwrap();
        let requests = set.bindings[0]
            .request_urls(&layout, HASH, 1400..2050)
            .unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].url, "https://e.com/pub/album/disc%201/a.flac");
        assert_eq!(requests[0].file_offset, 1400);
        assert_eq!(requests[0].length, 100);
        assert_eq!(requests[0].range_header(), "bytes=1400-1499");
        assert_eq!(requests[1].file_offset, 0);
        assert_eq!(requests[1].length, 500);
        assert_eq!(requests[2].url, "https://e.com/pub/album/notes.nfo");
        assert_eq!(requests[2].length, 50);
    }

    #[test]
    fn an_out_of_scope_request_is_refused_before_it_reaches_the_network() {
        let layout = layout();
        let set = BindingSet::resolve(&layout, HASH, &[spec("https://e.com/", "0")]).unwrap();
        let err = set.bindings[0]
            .request_urls(&layout, HASH, 1400..1600)
            .unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Binding);
        assert!(
            err.message().contains("cannot serve bytes 1400-1599"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn template_sources_have_no_static_file_urls() {
        let layout = layout();
        let templated = SourceSpec::new("https://e.com/chunks/{piece}.bin", Origin::Config)
            .with_template("https://e.com/chunks/{piece}.bin");
        let set = BindingSet::resolve(&layout, HASH, &[templated]).unwrap();
        assert!(
            set.bindings[0].file_urls.is_empty(),
            "a per-request URL is not a function of the file"
        );
        let requests = set.bindings[0].request_urls(&layout, HASH, 0..100).unwrap();
        assert_eq!(requests[0].url, "https://e.com/chunks/0.bin");
    }

    #[test]
    fn exact_mode_is_refused_where_it_cannot_work() {
        let layout = layout();
        let bad = spec("https://cdn.e.com/blob", "*").with_mode(Mode::Exact);
        let err = BindingSet::resolve(&layout, HASH, &[bad]).unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Binding);

        let good = spec("https://cdn.e.com/blob", "1").with_mode(Mode::Exact);
        let set = BindingSet::resolve(&layout, HASH, &[good]).unwrap();
        assert_eq!(set.bindings[0].file_urls[0].url, "https://cdn.e.com/blob");
    }

    #[test]
    fn non_http_sources_are_refused_with_the_reason() {
        let ftp = SourceSpec::new("ftp://e.com/pub/", Origin::CommandLine);
        let err = ftp.validate_url().unwrap_err();
        assert!(
            err.message()
                .contains("FTP is not a valid web seed transport"),
            "{}",
            err.message()
        );

        let relative = SourceSpec::new("/pub/files", Origin::CommandLine);
        assert!(
            relative
                .validate_url()
                .unwrap_err()
                .message()
                .contains("absolute http, https, or file URL")
        );

        // A local path is a source. `file:` is not in BEP 17 or BEP 19 and is
        // never offered to a swarm; it exists so bytes already on the disk can
        // be reused. See T-133.
        assert!(
            SourceSpec::new("file:///tmp/payload.bin", Origin::CommandLine)
                .validate_url()
                .is_ok()
        );
        let hoffman = SourceSpec {
            style: Style::Hoffman,
            ..SourceSpec::new("file:///tmp/payload.bin", Origin::CommandLine)
        };
        assert!(
            hoffman
                .validate_url()
                .unwrap_err()
                .message()
                .contains("BEP 17 is an HTTP wire style"),
        );
        assert!(
            SourceSpec::new("file://fileserver/share/x", Origin::CommandLine)
                .validate_url()
                .unwrap_err()
                .message()
                .contains("remote host")
        );

        assert!(
            SourceSpec::new("https://e.com/", Origin::CommandLine)
                .validate_url()
                .is_ok()
        );
        assert!(
            SourceSpec::new("HTTP://e.com/", Origin::CommandLine)
                .validate_url()
                .is_ok()
        );
    }

    #[test]
    fn a_template_is_validated_by_its_literal_prefix() {
        let good = SourceSpec::new("x", Origin::Config).with_template("https://e.com/{piece}.bin");
        assert!(good.validate_url().is_ok());
        let bad = SourceSpec::new("x", Origin::Config).with_template("ftp://e.com/{piece}.bin");
        assert!(bad.validate_url().is_err());
    }

    #[test]
    fn auth_specs_parse_every_documented_form() {
        assert_eq!(Auth::parse("none").unwrap(), Auth::None);
        assert_eq!(Auth::parse("").unwrap(), Auth::None);
        assert_eq!(Auth::parse("netrc").unwrap(), Auth::Netrc);
        assert_eq!(
            Auth::parse("bearer:abc123").unwrap(),
            Auth::Bearer {
                token: "abc123".into()
            }
        );
        assert_eq!(
            Auth::parse("basic:user:pass").unwrap(),
            Auth::Basic {
                user: "user".into(),
                password: "pass".into()
            }
        );
        // Only the first two colons separate, so a password may contain them.
        assert_eq!(
            Auth::parse("basic:user:p:a:s:s").unwrap(),
            Auth::Basic {
                user: "user".into(),
                password: "p:a:s:s".into()
            }
        );
        assert!(Auth::parse("basic:user").is_err());
        assert!(Auth::parse("kerberos").is_err());
    }

    #[test]
    fn styles_parse_by_name_and_by_bep_number() {
        assert_eq!(Style::parse("getright").unwrap(), Style::GetRight);
        assert_eq!(Style::parse("bep19").unwrap(), Style::GetRight);
        assert_eq!(Style::parse("hoffman").unwrap(), Style::Hoffman);
        assert_eq!(Style::parse("bep17").unwrap(), Style::Hoffman);
        assert_eq!(Style::parse("auto").unwrap(), Style::Auto);
        assert!(Style::parse("bep99").is_err());
    }

    #[test]
    fn origins_report_whether_the_torrent_supplied_them() {
        assert!(Origin::TorrentUrlList.is_from_torrent());
        assert!(Origin::TorrentHttpSeeds.is_from_torrent());
        for origin in [
            Origin::CommandLine,
            Origin::File,
            Origin::ListUrl,
            Origin::Config,
            Origin::Metalink,
        ] {
            assert!(!origin.is_from_torrent());
        }
    }

    #[test]
    fn a_spec_round_trips_through_json() {
        let original = spec("https://e.com/", "0-1")
            .with_mode(Mode::Prefix)
            .with_priority(5);
        let json = serde_json::to_string(&original).unwrap();
        let back: SourceSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn every_composition_mode_works_with_every_scope_form() {
        let layout = layout();
        let scopes = [
            "*",
            "0",
            "0-1",
            "piece:0-1",
            "byte:0-1024",
            "file:0:byte:0-100",
            "*.flac",
            "!*.nfo",
        ];
        for selector in scopes {
            for mode in [Mode::Auto, Mode::Prefix, Mode::Template] {
                let mut s = spec("https://e.com/pub/", selector).with_mode(mode);
                if mode == Mode::Template {
                    s.template = Some("https://e.com/{raw:path}".to_string());
                }
                BindingSet::resolve(&layout, HASH, &[s])
                    .unwrap_or_else(|e| panic!("{selector} x {mode} failed: {e}"));
            }
        }
        // `exact` needs a single-file scope, which is the documented limit.
        for selector in ["0", "1", "file:0:byte:0-100"] {
            let s = spec("https://e.com/blob", selector).with_mode(Mode::Exact);
            BindingSet::resolve(&layout, HASH, &[s])
                .unwrap_or_else(|e| panic!("{selector} x exact failed: {e}"));
        }
    }
}

#[cfg(test)]
mod status_policy_tests {
    use super::*;

    #[test]
    fn a_set_reads_codes_and_inclusive_ranges() {
        let set = StatusSet::parse("403,429,500-599").unwrap();
        assert!(set.contains(403));
        assert!(set.contains(429));
        assert!(set.contains(500));
        assert!(set.contains(599));
        assert!(!set.contains(404));
        assert!(!set.contains(499));
        assert!(!set.contains(600));
    }

    #[test]
    fn an_empty_set_names_nothing() {
        let set = StatusSet::default();
        assert!(set.is_empty());
        assert!(!set.contains(403));
        assert_eq!(StatusSet::parse("").unwrap(), set);
        assert_eq!(StatusSet::parse(" , ").unwrap(), set);
    }

    #[test]
    fn a_value_that_is_not_a_status_is_refused_by_name() {
        let err = StatusSet::parse("4o3").unwrap_err().to_string();
        assert!(err.contains("not an HTTP status code"), "{err}");
        let err = StatusSet::parse("42").unwrap_err().to_string();
        assert!(err.contains("100 to 599"), "{err}");
        let err = StatusSet::parse("600").unwrap_err().to_string();
        assert!(err.contains("100 to 599"), "{err}");
    }

    #[test]
    fn a_backwards_range_says_how_to_write_it() {
        let err = StatusSet::parse("599-500").unwrap_err().to_string();
        assert!(err.contains("500-599"), "{err}");
    }

    #[test]
    fn a_range_with_three_ends_is_refused_rather_than_truncated() {
        assert!(StatusSet::parse("500-599-600").is_err());
    }

    #[test]
    fn a_set_round_trips_through_its_canonical_spelling() {
        let set = StatusSet::parse(" 403 , 500-599 ").unwrap();
        assert_eq!(set.to_text(), "403,500-599");
        assert_eq!(StatusSet::parse(&set.to_text()).unwrap(), set);
    }

    #[test]
    fn a_table_writes_a_status_list_as_integers_or_as_strings() {
        #[derive(Deserialize)]
        struct Holder {
            codes: StatusSet,
        }
        let from_ints: Holder = toml::from_str("codes = [403, 429]").unwrap();
        assert!(from_ints.codes.contains(403) && from_ints.codes.contains(429));
        let mixed: Holder = toml::from_str(r#"codes = [403, "500-599"]"#).unwrap();
        assert!(mixed.codes.contains(403) && mixed.codes.contains(503));
        let text: Holder = toml::from_str(r#"codes = "403,500-599""#).unwrap();
        assert_eq!(text.codes, mixed.codes);
    }

    #[test]
    fn a_bad_status_in_a_table_fails_the_parse_rather_than_being_dropped() {
        #[derive(Deserialize)]
        struct Holder {
            #[allow(dead_code)]
            codes: StatusSet,
        }
        assert!(toml::from_str::<Holder>("codes = [42]").is_err());
    }

    #[test]
    fn the_policy_says_nothing_about_a_status_neither_list_names() {
        let limits = SourceLimits {
            retry_status: StatusSet::parse("403").unwrap(),
            ..SourceLimits::default()
        };
        assert_eq!(limits.status_is_retryable(403), Some(true));
        assert_eq!(limits.status_is_retryable(404), None);
    }

    #[test]
    fn a_fatal_status_overrides_the_default_the_other_way() {
        let limits = SourceLimits {
            fatal_status: StatusSet::parse("503").unwrap(),
            ..SourceLimits::default()
        };
        assert_eq!(limits.status_is_retryable(503), Some(false));
    }

    #[test]
    fn a_status_in_both_lists_is_a_usage_error_rather_than_a_precedence_rule() {
        let limits = SourceLimits {
            retry_status: StatusSet::parse("403,500-599").unwrap(),
            fatal_status: StatusSet::parse("503").unwrap(),
            ..SourceLimits::default()
        };
        let err = limits.check_status_policy().unwrap_err().to_string();
        assert!(err.contains("503"), "{err}");
        assert!(err.contains("one or the other"), "{err}");
    }

    #[test]
    fn two_disjoint_lists_are_accepted() {
        let limits = SourceLimits {
            retry_status: StatusSet::parse("403,429").unwrap(),
            fatal_status: StatusSet::parse("404,410").unwrap(),
            ..SourceLimits::default()
        };
        limits.check_status_policy().unwrap();
    }

    #[test]
    fn a_default_source_has_no_policy_and_serialises_without_the_fields() {
        let limits = SourceLimits::default();
        assert!(limits.retry_status.is_empty());
        assert!(limits.fatal_status.is_empty());
        assert_eq!(limits.status_is_retryable(403), None);
        let json = serde_json::to_string(&limits).unwrap();
        assert!(!json.contains("retry_status"), "{json}");
        assert!(!json.contains("fatal_status"), "{json}");
    }
}
