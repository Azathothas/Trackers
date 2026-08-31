//! Ranged HTTP fetching for one source.
//!
//! The session asks for 16 KiB blocks, which would be a pathological number of
//! HTTP requests. Reads are served out of aligned windows instead: a block
//! triggers one ranged GET for the window containing it, and the window is
//! cached so neighbouring blocks are answered from memory. `chunk_size` sets
//! the window and is deliberately independent of the torrent's piece length,
//! so one request can cover part of a piece or several pieces.
//!
//! Every request is clamped to the source's scope before it is issued. An
//! out-of-scope request is a bug in `bit-cli`, and letting it reach a server
//! would disguise it as a 416 from a perfectly healthy mirror.
//!
//! Failures are classified rather than counted. A 404 is not worth retrying, a
//! 503 is, and a server that ignores `Range` and returns the whole entity is
//! worse than either, because reading its body as if it were the requested
//! range serves wrong bytes at every offset.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::{
    ACCEPT_ENCODING, ACCEPT_RANGES, AUTHORIZATION, CONTENT_LENGTH, CONTENT_RANGE, HeaderMap,
    HeaderName, HeaderValue, RANGE, USER_AGENT,
};
use reqwest::{Response, StatusCode};
use tokio::sync::{Mutex, Semaphore};

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::time::Timestamp;
use crate::webseed::binding::{Auth, Binding, RangeRequest, Style};

/// Default `User-Agent`, used when a source does not set its own.
pub fn default_user_agent() -> String {
    format!("bit-cli/{}", env!("CARGO_PKG_VERSION"))
}

/// When HTTP-sourced data is hash-checked at the source.
///
/// The session verifies every piece it writes regardless, so this is not what
/// stops bad data reaching the disk. It is what makes bad data *attributable*:
/// with it on, a mirror serving a wrong piece is named, dropped, and reported
/// with the piece index and both hashes, instead of showing up as "a peer sent
/// something wrong" with no way to tell which mirror it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verify {
    /// Check every whole piece the source serves. The default.
    #[default]
    Piece,
    /// Check nothing at the source and leave it to the session.
    None,
}

impl Verify {
    /// The stable name used on the command line and in output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Piece => "piece",
            Self::None => "none",
        }
    }
}

/// Why a fetch failed, and whether it is worth trying again.
#[derive(Debug, Clone)]
pub enum FetchError {
    /// Worth retrying: connection errors, 5xx, short bodies.
    Transient { reason: String, status: Option<u16> },
    /// Not worth retrying: the URL is wrong, the credentials are wrong, or the
    /// server does not do ranges.
    Permanent { reason: String, status: Option<u16> },
    /// The request ran out of time with the mirror still holding the
    /// connection: a connect timeout, or a body that stopped arriving.
    ///
    /// Separate from [`Self::Transient`] because a mirror that answered
    /// **wrongly** and one that did not answer **at all** want opposite
    /// handling. A 503 is worth another request; a hung backend will hang the
    /// retry too, and every attempt spent on it is `--web-seed-timeout` the
    /// other sources were not asked. See `TODO/webseed.md`, T-007.
    Stalled { reason: String, status: Option<u16> },
    /// The bytes arrived but did not match the torrent's piece hash.
    HashMismatch { reason: String },
}

impl FetchError {
    fn transient(reason: impl Into<String>) -> Self {
        Self::Transient {
            reason: reason.into(),
            status: None,
        }
    }

    fn permanent(reason: impl Into<String>) -> Self {
        Self::Permanent {
            reason: reason.into(),
            status: None,
        }
    }

    /// The HTTP status that produced this, when there was one.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Transient { status, .. }
            | Self::Permanent { status, .. }
            | Self::Stalled { status, .. } => *status,
            Self::HashMismatch { .. } => None,
        }
    }

    /// Whether a retry could succeed.
    ///
    /// A stall is not retryable **within the request**: the mirror is holding
    /// the connection and will hold the next one, so the retry ladder buys
    /// another `--web-seed-timeout` of nothing. It is still recoverable from
    /// the bridge's point of view, which is a separate question and is
    /// [`Self::is_stall`]'s.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transient { .. })
    }

    /// Whether the mirror ran out of time rather than answering wrongly.
    pub fn is_stall(&self) -> bool {
        matches!(self, Self::Stalled { .. })
    }

    /// A stable name for the failure class, for the error counters a `bench`
    /// report breaks down by.
    pub fn class(&self) -> &'static str {
        match self {
            Self::HashMismatch { .. } => "hash_mismatch",
            Self::Permanent {
                status: Some(403 | 401),
                ..
            } => "auth",
            Self::Permanent {
                status: Some(404), ..
            } => "not_found",
            Self::Permanent {
                status: Some(416), ..
            } => "range_not_satisfiable",
            Self::Permanent {
                status: Some(200), ..
            } => "range_ignored",
            Self::Permanent { .. } => "permanent",
            Self::Transient {
                status: Some(_), ..
            } => "server_error",
            Self::Transient { .. } => "transport",
            // One class, whether it was the connect or the body that ran out
            // of time. What a reader does about either is the same: raise
            // `--web-seed-timeout` or stop using the mirror.
            Self::Stalled { .. } => "stalled",
        }
    }
}

/// A failed read, and the file it was addressed to.
///
/// Separate from [`FetchError`] rather than a field on it, because the file is
/// a property of the request that failed and not of the failure: the same
/// status means the same thing whichever file produced it, and every other
/// caller of [`Fetcher::read`] has no use for the attribution.
#[derive(Debug, Clone)]
pub struct ReadFailure {
    pub error: FetchError,
    /// The file index the failing request addressed, when it was addressed to
    /// one. `None` means the failure is the source's rather than a file's.
    pub file: Option<usize>,
}

impl ReadFailure {
    /// A failure that cannot be attributed to one file.
    fn whole_source(error: FetchError) -> Self {
        Self { error, file: None }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient { reason, .. }
            | Self::Permanent { reason, .. }
            | Self::Stalled { reason, .. }
            | Self::HashMismatch { reason } => f.write_str(reason),
        }
    }
}

impl From<FetchError> for Error {
    fn from(err: FetchError) -> Self {
        let code = match &err {
            FetchError::HashMismatch { .. } => crate::exit::ExitCode::HashMismatch,
            FetchError::Permanent { .. } => crate::exit::ExitCode::NoUsableSources,
            FetchError::Transient { .. } | FetchError::Stalled { .. } => {
                crate::exit::ExitCode::Network
            }
        };
        let mut error = Error::new(code, err.to_string()).with("class", err.class());
        if let Some(status) = err.status() {
            error = error.with("http_status", status);
        }
        error
    }
}

/// What one ranged GET did, recorded whether it succeeded or not.
///
/// This is what `--trace http` prints and what `bench` aggregates. It carries
/// everything needed to rebuild the request by hand, which is the standard the
/// trace is held to.
#[derive(Debug, Clone)]
pub struct RequestRecord {
    pub started_at: Timestamp,
    pub url: String,
    pub range: String,
    /// The URL after redirects, when it differs from the one requested.
    pub resolved_url: Option<String>,
    pub status: Option<u16>,
    pub bytes: u64,
    pub total_ms: u64,
    pub ttfb_ms: Option<u64>,
    pub server: Option<String>,
    pub error: Option<String>,
}

impl RequestRecord {
    /// The equivalent `curl` command.
    ///
    /// The standard for `--trace http` is that a failing request can be
    /// reproduced by hand from the log. This is that reproduction, with
    /// credentials redacted unless the caller passes them through.
    pub fn as_curl(&self, headers: &[(String, String)]) -> String {
        let mut parts = vec![
            "curl".to_string(),
            "-sS".to_string(),
            "-D".into(),
            "-".into(),
        ];
        parts.push("-H".into());
        parts.push(shell_quote(&format!("Range: {}", self.range)));
        for (name, value) in headers {
            parts.push("-H".into());
            parts.push(shell_quote(&format!("{name}: {value}")));
        }
        parts.push("-o".into());
        parts.push("/dev/null".into());
        parts.push(shell_quote(&self.url));
        parts.join(" ")
    }
}

/// Quote one argument for a POSIX shell.
fn shell_quote(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c))
    {
        return text.to_string();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Counters for one source, readable while it is running.
#[derive(Debug, Default)]
pub struct SourceStats {
    pub requests: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
    pub consecutive_errors: AtomicU32,
    pub retries: AtomicU64,
    /// Epoch milliseconds the source may be used again. Zero means now.
    cooldown_until_ms: AtomicU64,
    /// How many times the error budget has run out over this run.
    cooldowns: AtomicU64,
    /// Retries charged to each HTTP status.
    ///
    /// A plain `std::sync::Mutex` rather than an async one: it is taken only
    /// when a request has already failed, never on the path a byte travels,
    /// and it is never held across an await.
    retries_by_status: std::sync::Mutex<std::collections::BTreeMap<u16, u64>>,
    /// Why this source was convicted of serving wrong bytes, once it has been.
    ///
    /// Set from outside the fetch path, by whoever resolved a disputed piece
    /// against the verified payload. It lives here rather than on the bridge
    /// because a source is one mirror however many connections it is presented
    /// over, and a mirror caught lying on one connection is the same mirror on
    /// all of them. See `TODO/webseed.md`, T-179.
    banned: std::sync::Mutex<Option<String>>,
}

impl SourceStats {
    /// Charge one retry to the status that caused it.
    fn record_retry_status(&self, code: u16) {
        if let Ok(mut by_status) = self.retries_by_status.lock() {
            *by_status.entry(code).or_default() += 1;
        }
    }

    /// Retries per HTTP status, highest count first is not the order: this is
    /// by code, so two reports line up column for column.
    pub fn retries_by_status(&self) -> std::collections::BTreeMap<u16, u64> {
        self.retries_by_status
            .lock()
            .map(|by_status| by_status.clone())
            .unwrap_or_default()
    }

    /// Convict this source of serving wrong bytes, with the reason.
    ///
    /// The first conviction is the one kept. A mirror that got two blocks
    /// wrong is retired by the first of them, and overwriting the reason with
    /// the second would report a later symptom as the cause.
    pub fn ban(&self, reason: impl Into<String>) {
        let mut banned = self.banned.lock().unwrap_or_else(|e| e.into_inner());
        if banned.is_none() {
            *banned = Some(reason.into());
        }
    }

    /// Why this source was convicted, if it was.
    pub fn banned(&self) -> Option<String> {
        self.banned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl SourceStats {
    /// Whether the source has run out of its error budget.
    ///
    /// True from the moment `max_errors` consecutive requests fail until
    /// something clears it, which only [`Self::end_cooldown`] does. A source
    /// with `cooldown_ms` at zero, the default, is never cleared and so is out
    /// for the rest of the run.
    ///
    /// This is the guard on the fetch path. [`Self::is_cooling_down`] is the
    /// narrower question of whether the deadline is still ahead, which is what
    /// the bridge sleeps on. The two differ exactly when the cooldown is zero:
    /// the budget is spent and there is nothing to wait for.
    pub fn budget_spent(&self) -> bool {
        self.cooldown_until_ms.load(Ordering::Relaxed) != 0
    }

    /// Whether the source is inside a cooldown it will come out of.
    pub fn is_cooling_down(&self) -> bool {
        let until = self.cooldown_until_ms.load(Ordering::Relaxed);
        until != 0 && Timestamp::now().epoch_ms() < until as i64
    }

    /// When the source becomes usable again, if its budget is spent.
    pub fn cooldown_until(&self) -> Option<Timestamp> {
        match self.cooldown_until_ms.load(Ordering::Relaxed) {
            0 => None,
            ms => Some(Timestamp::from_epoch_ms(ms as i64)),
        }
    }

    /// How long is left of the cooldown, or `None` when there is nothing to
    /// wait for.
    pub fn cooldown_remaining(&self) -> Option<Duration> {
        let until = self.cooldown_until_ms.load(Ordering::Relaxed);
        if until == 0 {
            return None;
        }
        let left = until as i64 - Timestamp::now().epoch_ms();
        (left > 0).then(|| Duration::from_millis(left as u64))
    }

    fn record_success(&self, bytes: u64) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.consecutive_errors.store(0, Ordering::Relaxed);
    }

    /// How many times this source has been cooled down.
    ///
    /// Counted rather than derived, because a source that came back and went
    /// out again looks exactly like one that never came back in a report that
    /// only carries the current state.
    pub fn cooldowns(&self) -> u64 {
        self.cooldowns.load(Ordering::Relaxed)
    }

    /// Clear a cooldown the caller has waited out, so the source may be used
    /// again. Returns whether it was still the one in force.
    ///
    /// Called by a bridge once it has slept out the deadline it read. The
    /// deadline is passed back rather than assumed, because several
    /// connections share one `SourceStats`: without it, a bridge waking from
    /// an old cooldown could clear a newer one that another connection had
    /// only just tripped.
    ///
    /// The error and request totals are not touched. They are the run's
    /// history, and a source that failed five times before recovering should
    /// still say so.
    pub fn end_cooldown(&self, deadline: Timestamp) -> bool {
        let expected = deadline.epoch_ms().max(0) as u64;
        let cleared = self
            .cooldown_until_ms
            .compare_exchange(expected, 0, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok();
        if cleared {
            self.consecutive_errors.store(0, Ordering::Relaxed);
        }
        cleared
    }

    /// Count an error, returning whether it tripped the cooldown.
    fn record_error(&self, max_errors: u32, cooldown: Duration) -> bool {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.errors.fetch_add(1, Ordering::Relaxed);
        let consecutive = self.consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;
        // The cooldown half of what `--trace retry` promises, and the reason
        // it is here rather than at the call site: the error budget is spent
        // in three places and tripped in one. See `TODO/cli-surface.md`,
        // T-219.
        tracing::trace!(
            target: "bit_cli::retry",
            consecutive,
            max_errors,
            cooldown_ms = cooldown.as_millis() as u64,
            tripped = consecutive >= max_errors,
            "error budget"
        );
        if consecutive < max_errors {
            return false;
        }
        // A zero cooldown is still a cooldown. It says "do not come back",
        // which the bridge reads as retiring the source, and it has to be
        // distinguishable from "never tripped" or the budget would never be
        // reached. One millisecond in the past is in the past, so
        // `is_cooling_down` is false and `cooldown_until` is `Some`.
        let until = Timestamp::now().epoch_ms() + cooldown.as_millis().min(i64::MAX as u128) as i64;
        self.cooldown_until_ms
            .store(until.max(1) as u64, Ordering::Relaxed);
        self.cooldowns.fetch_add(1, Ordering::Relaxed);
        true
    }
}

/// One rung of a retry ladder, for `--trace retry`.
///
/// Both ladders call it, and they are the same shape with different request
/// kinds, so the record says which. Emitted before the backoff is slept rather
/// than after: a caller watching a source that never comes back needs to see
/// how long the run is about to wait, and a record written after the sleep is
/// the one that never arrives. See `TODO/cli-surface.md`, T-219.
fn trace_retry(
    kind: &str,
    url: &str,
    attempt: u32,
    of: u32,
    backoff: Duration,
    last: &Option<FetchError>,
) {
    tracing::trace!(
        target: "bit_cli::retry",
        kind,
        url = %url,
        attempt,
        of,
        backoff_ms = backoff.as_millis() as u64,
        status = ?last.as_ref().and_then(FetchError::status),
        reason = ?last.as_ref().map(ToString::to_string),
        "retrying"
    );
}

/// A byte-rate cap for one source.
///
/// A token bucket refilled continuously at the configured rate, holding one
/// second of burst. Tokens are taken before a request goes out rather than
/// after its bytes arrive: a limiter that lets the bytes land and then sleeps
/// has not limited anything the mirror can see, it has only delayed the next
/// request by the wrong amount.
///
/// The bucket is allowed to go negative. A request larger than one second of
/// burst can never be satisfied from a full bucket, and taking what it needs
/// and waiting out the deficit is what keeps the average right instead of
/// deadlocking. It also makes concurrent callers queue in the order they
/// arrived rather than racing for a refill.
#[derive(Debug)]
struct RateLimiter {
    /// Bytes per second.
    rate: f64,
    state: std::sync::Mutex<Bucket>,
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    /// `tokio::time::Instant` rather than the standard one, so the bucket
    /// follows the same clock the sleep does. Outside a test they are the
    /// same clock; under a paused one they are not, and a limiter that
    /// refills on a clock its own sleeps do not advance cannot be tested.
    last: tokio::time::Instant,
}

impl RateLimiter {
    fn new(rate: u64) -> Self {
        let rate = rate.max(1) as f64;
        Self {
            rate,
            state: std::sync::Mutex::new(Bucket {
                tokens: rate,
                last: tokio::time::Instant::now(),
            }),
        }
    }

    /// Wait until `bytes` may be requested.
    async fn take(&self, bytes: u64) {
        let (wait, left) = {
            let mut bucket = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let now = tokio::time::Instant::now();
            let refill = now.duration_since(bucket.last).as_secs_f64() * self.rate;
            bucket.tokens = (bucket.tokens + refill).min(self.rate);
            bucket.last = now;
            bucket.tokens -= bytes as f64;
            let wait = match bucket.tokens < 0.0 {
                true => Duration::from_secs_f64(-bucket.tokens / self.rate),
                false => Duration::ZERO,
            };
            (wait, bucket.tokens)
        };
        // What `--trace ratelimit` promises: the bucket decision and the
        // stall. Emitted for every take rather than only for the ones that
        // wait, because "the limiter let this through immediately" is the
        // answer a caller asking why a run is slow needs just as much as the
        // waits are. `stalled` is the field to filter on. See
        // `TODO/cli-surface.md`, T-219.
        tracing::trace!(
            target: "bit_cli::ratelimit",
            bytes,
            rate = self.rate as u64,
            tokens_left = left as i64,
            wait_micros = wait.as_micros() as u64,
            stalled = !wait.is_zero(),
            "took tokens"
        );
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

/// One window of a file held in memory.
struct CachedWindow {
    url: String,
    start: u64,
    data: Bytes,
}

/// Identifies a window as its URL and the offset within the file.
type WindowKey = (String, u64);

/// Fetches torrent byte ranges from one source over HTTP.
pub struct Fetcher {
    client: reqwest::Client,
    binding: Binding,
    layout: Arc<Layout>,
    info_hash: String,
    headers: HeaderMap,
    stats: Arc<SourceStats>,
    limiter: Arc<Semaphore>,
    /// The source's byte-rate cap, from `--web-seed-speed-limit` or the
    /// binding table's `rate_limit`. `None` is unlimited.
    rate: Option<RateLimiter>,
    window: u64,
    capacity: usize,
    cache: Mutex<VecDeque<CachedWindow>>,
    /// One gate per window in flight, so concurrent workers wanting the same
    /// window wait on one request rather than each issuing their own. Without
    /// it every worker misses the cache at the same moment and traffic is
    /// multiplied by the concurrency.
    inflight: Mutex<HashMap<WindowKey, Arc<Mutex<()>>>>,
    /// Records of every request, kept when tracing is on.
    trace: Option<Mutex<Vec<RequestRecord>>>,
    /// When to hash-check what this source serves.
    verify: Verify,
    /// The torrent's piece hashes, when the caller supplied them.
    piece_hashes: Option<Arc<Vec<[u8; 20]>>>,
}

impl Fetcher {
    /// Build a fetcher for one binding.
    ///
    /// `cache_windows` caps how many windows are held, bounding memory at
    /// `cache_windows * chunk_size` per source.
    pub fn new(
        binding: Binding,
        layout: Arc<Layout>,
        info_hash: impl Into<String>,
        cache_windows: usize,
        trace: bool,
    ) -> Result<Self> {
        let limits = &binding.spec.limits;
        let mut headers = HeaderMap::new();
        for (name, value) in &binding.spec.headers {
            let name = HeaderName::try_from(name.as_str()).map_err(|e| {
                Error::usage(format!("`{name}` is not a valid header name: {e}"))
                    .with("header", name.clone())
            })?;
            let value = HeaderValue::from_str(value).map_err(|e| {
                Error::usage(format!(
                    "header `{name}` has a value HTTP cannot carry: {e}"
                ))
            })?;
            headers.insert(name, value);
        }
        // A web seed asks for a byte range and hashes what comes back against
        // the torrent, so the bytes on the wire have to be the bytes on the
        // server. A transcoding proxy that re-encodes the body changes what a
        // range means, and the result is a correct request returning wrong
        // bytes from a healthy mirror: the piece fails its hash and the mirror
        // is blamed. Set on every request rather than only the first, because
        // any of them can meet a proxy. See `TODO/webseed.md`, T-004.
        //
        // Inserted after the caller's own headers and only when absent, so
        // `--web-seed-header Accept-Encoding: gzip` still wins. A caller who
        // sets it has said what they want.
        headers
            .entry(ACCEPT_ENCODING)
            .or_insert(HeaderValue::from_static("identity"));
        let agent = binding
            .spec
            .user_agent
            .clone()
            .unwrap_or_else(default_user_agent);
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&agent)
                .map_err(|e| Error::usage(format!("invalid user agent: {e}")))?,
        );
        if let Auth::Bearer { token } = &binding.spec.auth {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| Error::usage(format!("invalid bearer token: {e}")))?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }

        let client = reqwest::Client::builder()
            .timeout(limits.timeout())
            .connect_timeout(limits.connect_timeout())
            .user_agent(agent)
            .build()
            .map_err(|e| Error::network(format!("cannot build an HTTP client: {e}")))?;

        Ok(Self {
            client,
            layout,
            info_hash: info_hash.into(),
            headers,
            stats: Arc::new(SourceStats::default()),
            limiter: Arc::new(Semaphore::new(limits.concurrency.max(1))),
            rate: limits.rate_limit.map(RateLimiter::new),
            window: limits.chunk_size.max(1),
            capacity: cache_windows.max(1),
            cache: Mutex::new(VecDeque::new()),
            inflight: Mutex::new(HashMap::new()),
            trace: trace.then(|| Mutex::new(Vec::new())),
            verify: Verify::None,
            piece_hashes: None,
            binding,
        })
    }

    /// Hash-check whole pieces at the source.
    ///
    /// Without the piece hashes there is nothing to check against, so passing
    /// `Verify::Piece` with no hashes leaves verification off rather than
    /// pretending to do it.
    #[must_use]
    pub fn with_verification(mut self, mode: Verify, hashes: Option<Arc<Vec<[u8; 20]>>>) -> Self {
        self.verify = match hashes.is_some() {
            true => mode,
            false => Verify::None,
        };
        self.piece_hashes = hashes;
        self
    }

    /// The binding this fetcher serves.
    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    /// Live counters.
    pub fn stats(&self) -> &Arc<SourceStats> {
        &self.stats
    }

    /// Every request recorded so far, when tracing is on.
    pub async fn records(&self) -> Vec<RequestRecord> {
        match &self.trace {
            Some(trace) => trace.lock().await.clone(),
            None => Vec::new(),
        }
    }

    /// Read `length` bytes at torrent offset `offset`.
    ///
    /// The range is checked against the source's scope first, so a caller
    /// asking for bytes this source was never bound to gets a binding error
    /// rather than an HTTP one.
    pub async fn read(&self, offset: u64, length: u64) -> std::result::Result<Vec<u8>, FetchError> {
        self.read_block(offset, length)
            .await
            .map_err(|failure| failure.error)
    }

    /// [`Self::read`], naming the file a failure came from.
    ///
    /// A byte range may span several files, so a failed read is a failure of
    /// one of them and not of the source. Which one is what makes a per-file
    /// retirement possible: a mirror that answers 404 for one file of twelve
    /// should lose that file's pieces and keep serving the other eleven. See
    /// `TODO/webseed.md`, T-005.
    ///
    /// `file` is `None` when the failure was not addressed to one file: a
    /// scope error, a range the torrent does not cover, or a BEP 17 source,
    /// which addresses pieces rather than files and so has no per-file request
    /// to attribute anything to.
    pub async fn read_block(
        &self,
        offset: u64,
        length: u64,
    ) -> std::result::Result<Vec<u8>, ReadFailure> {
        // BEP 17 addresses pieces, not files, so it does not go through the
        // per-file window path at all.
        if self.binding.spec.style == Style::Hoffman {
            return self
                .read_hoffman(offset, length)
                .await
                .map_err(ReadFailure::whole_source);
        }
        let requests = self
            .binding
            .request_urls(&self.layout, &self.info_hash, offset..offset + length)
            .map_err(|e| ReadFailure::whole_source(FetchError::permanent(e.to_string())))?;

        let covered: u64 = requests.iter().map(|r| r.length).sum();
        if covered != length {
            return Err(ReadFailure::whole_source(FetchError::permanent(format!(
                "asked for {length} bytes at {offset}, but the torrent only covers {covered}"
            ))));
        }

        let mut out = Vec::with_capacity(length as usize);
        for request in requests {
            if let Err(error) = self.read_one(&request, &mut out).await {
                return Err(ReadFailure {
                    error,
                    file: Some(request.file),
                });
            }
        }
        Ok(out)
    }

    /// Read a byte range under BEP 17.
    ///
    /// Hoffman-style seeding addresses the torrent by piece rather than by
    /// file: one request per piece, with the sub-range inside the piece given
    /// as a query parameter instead of a `Range` header. There is no window
    /// cache here, because the server already decides the granularity.
    async fn read_hoffman(
        &self,
        offset: u64,
        length: u64,
    ) -> std::result::Result<Vec<u8>, FetchError> {
        let range = offset..offset + length;
        if !self.binding.covers(&range) {
            return Err(FetchError::permanent(format!(
                "{} is scoped to `{}` and cannot serve bytes {}-{}",
                self.binding.spec.url,
                self.binding.scope.selector,
                range.start,
                range.end.saturating_sub(1)
            )));
        }

        let mut out = Vec::with_capacity(length as usize);
        let mut pos = offset;
        while pos < range.end {
            let piece = self
                .layout
                .piece_at(pos)
                .ok_or_else(|| FetchError::permanent(format!("byte {pos} is past the payload")))?;
            let piece_range = self.layout.piece_range(piece).ok_or_else(|| {
                FetchError::permanent(format!("piece {piece} is past the payload"))
            })?;
            let take = (piece_range.end - pos).min(range.end - pos);
            let begin = pos - piece_range.start;
            let url = hoffman_url(&self.binding.spec.url, &self.info_hash, piece, begin, take)?;

            let data = self.fetch_hoffman(&url, take).await?;
            out.extend_from_slice(&data);
            pos += take;
        }
        Ok(out)
    }

    /// Hash-check every whole piece inside a freshly fetched window.
    ///
    /// A window rarely lines up with piece boundaries, so only the pieces it
    /// contains end to end can be checked here. Partial pieces are left to the
    /// session, which sees the whole thing. The point is attribution: a wrong
    /// piece caught here names the mirror that served it.
    fn verify_window(&self, absolute: u64, data: &[u8]) -> std::result::Result<(), FetchError> {
        if self.verify == Verify::None {
            return Ok(());
        }
        let Some(hashes) = &self.piece_hashes else {
            return Ok(());
        };
        let end = absolute + data.len() as u64;

        for piece in self.layout.pieces_overlapping(&(absolute..end)) {
            let Some(range) = self.layout.piece_range(piece) else {
                continue;
            };
            if range.start < absolute || range.end > end {
                continue;
            }
            let Some(expected) = hashes.get(piece as usize) else {
                continue;
            };
            let from = (range.start - absolute) as usize;
            let to = (range.end - absolute) as usize;
            let actual = sha1_of(&data[from..to]);
            if &actual != expected {
                return Err(FetchError::HashMismatch {
                    reason: format!(
                        "{} served piece {piece} with hash {} but the torrent says {}",
                        self.binding.spec.url,
                        hex(&actual),
                        hex(expected)
                    ),
                });
            }
        }
        Ok(())
    }

    /// One BEP 17 request, with the same retry and cooldown policy as a
    /// ranged GET.
    async fn fetch_hoffman(
        &self,
        url: &str,
        length: u64,
    ) -> std::result::Result<Bytes, FetchError> {
        let limits = &self.binding.spec.limits;
        if self.stats.budget_spent() {
            return Err(FetchError::permanent(format!(
                "{url}: source is cooling down after {} errors",
                self.stats.errors.load(Ordering::Relaxed)
            )));
        }

        let mut backoff = Duration::from_millis(500);
        let mut last: Option<FetchError> = None;
        for attempt in 0..=limits.retries {
            if attempt > 0 {
                self.stats.retries.fetch_add(1, Ordering::Relaxed);
                trace_retry("BEP 17", url, attempt, limits.retries, backoff, &last);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(16));
            }
            if let Some(rate) = &self.rate {
                rate.take(length).await;
            }
            let permit =
                self.limiter.acquire().await.map_err(|e| {
                    FetchError::permanent(format!("concurrency limiter closed: {e}"))
                })?;
            let outcome = self.hoffman_once(url, length).await;
            drop(permit);
            match outcome {
                Ok(data) => {
                    self.stats.record_success(data.len() as u64);
                    return Ok(data);
                }
                // A stall spends the whole budget at once and stops the
                // ladder. The mirror is holding the connection: the retry
                // will be held the same way, and so will the request after
                // it, so the four more attempts and the four reconnect
                // backoffs between them are `--web-seed-timeout` each of
                // waiting for a source that has already answered the
                // question. Measured at the defaults, that is the difference
                // between 133 seconds and ten. `--web-seed-cooldown` still
                // decides whether it may come back, because tripping the
                // budget is what a cooldown hangs off. See `TODO/webseed.md`,
                // T-007.
                Err(err) if err.is_stall() => {
                    self.stats.record_error(1, limits.cooldown());
                    return Err(err);
                }
                Err(err) if err.is_retryable() => last = Some(err),
                // A permanent failure does not spend the error budget.
                //
                // That budget is `--web-seed-max-errors` and it exists to
                // retire a source that keeps failing **transiently**, with
                // `--web-seed-cooldown` deciding when it may come back. See
                // `TODO/multi-source.md`, T-130 and T-137. A permanent failure
                // is not a run of bad luck, it is one fact learned once, and
                // it already has its own outcome: the source is retired, or,
                // when the failure is attributable to one file, narrowed to
                // what it can still serve. Counting it here as well charged it
                // twice, and with a small budget one 404 on one file of twelve
                // put the whole mirror into cooldown through the back door.
                // See `TODO/webseed.md`, T-005.
                Err(err) => return Err(err),
            }
        }
        self.stats
            .record_error(limits.max_errors, limits.cooldown());
        Err(last.unwrap_or_else(|| FetchError::transient(format!("{url}: no attempt was made"))))
    }

    async fn hoffman_once(&self, url: &str, length: u64) -> std::result::Result<Bytes, FetchError> {
        let began = Instant::now();
        let started_at = Timestamp::now();
        let mut request = self.client.get(url).headers(self.headers.clone());
        if let Auth::Basic { user, password } = &self.binding.spec.auth {
            request = request.basic_auth(user, Some(password));
        }

        let sent = request.send().await;
        let ttfb_ms = began.elapsed().as_millis() as u64;
        let response = match sent {
            Ok(response) => response,
            Err(e) => {
                let err = classify_transport(url, &e);
                self.record(
                    started_at,
                    url,
                    "bep17",
                    None,
                    0,
                    began,
                    Some(ttfb_ms),
                    None,
                    Some(&err),
                )
                .await;
                return Err(err);
            }
        };

        let status = response.status();
        let server = response
            .headers()
            .get(reqwest::header::SERVER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if !status.is_success() {
            let err = self.reclassify(classify_status(url, status));
            self.record(
                started_at,
                url,
                "bep17",
                Some(status.as_u16()),
                0,
                began,
                Some(ttfb_ms),
                server,
                Some(&err),
            )
            .await;
            return Err(err);
        }

        let body = match response.bytes().await {
            Ok(body) => body,
            Err(e) => {
                let err = body_failure(url, status.as_u16(), &e);
                self.record(
                    started_at,
                    url,
                    "bep17",
                    Some(status.as_u16()),
                    0,
                    began,
                    Some(ttfb_ms),
                    server,
                    Some(&err),
                )
                .await;
                return Err(err);
            }
        };
        if body.len() as u64 != length {
            let err = FetchError::Transient {
                reason: format!("{url}: asked for {length} bytes, got {}", body.len()),
                status: Some(status.as_u16()),
            };
            self.record(
                started_at,
                url,
                "bep17",
                Some(status.as_u16()),
                body.len() as u64,
                began,
                Some(ttfb_ms),
                server,
                Some(&err),
            )
            .await;
            return Err(err);
        }

        self.record(
            started_at,
            url,
            "bep17",
            Some(status.as_u16()),
            body.len() as u64,
            began,
            Some(ttfb_ms),
            server,
            None,
        )
        .await;
        Ok(body)
    }

    /// Assemble one per-file request out of cached or freshly fetched windows.
    async fn read_one(
        &self,
        request: &RangeRequest,
        out: &mut Vec<u8>,
    ) -> std::result::Result<(), FetchError> {
        // A per-request composition has a different URL for every byte range,
        // so windowing it would fetch the wrong resource. Those go straight
        // out as written.
        if self.binding.spec.mode.is_per_request() {
            let data = self
                .fetch_with_retry(&request.url, request.file_offset, request.length)
                .await?;
            out.extend_from_slice(&data);
            return Ok(());
        }

        // Windows are aligned in file coordinates, but the scope is written in
        // torrent coordinates, so the offset of this file within the payload
        // is what converts between them.
        let file_base = request.torrent_offset - request.file_offset;
        let file_len = self
            .layout
            .file(request.file)
            .map(|f| f.length)
            .ok_or_else(|| FetchError::permanent(format!("no file at index {}", request.file)))?;
        let end = request.file_offset + request.length;
        let mut pos = request.file_offset;
        while pos < end {
            let (start, length) = self.window_for(file_base, pos, file_len)?;
            let data = self.window(&request.url, file_base, start, length).await?;
            let inner = (pos - start) as usize;
            let available = data.len().saturating_sub(inner);
            if available == 0 {
                return Err(FetchError::permanent(format!(
                    "{} is shorter than the torrent says",
                    request.url
                )));
            }
            let take = available.min((end - pos) as usize);
            out.extend_from_slice(&data[inner..inner + take]);
            pos += take as u64;
        }
        Ok(())
    }

    /// The window covering `pos`, as a file offset and a length.
    ///
    /// Two things bound a window besides `chunk_size`: the end of the file,
    /// and the edge of the source's scope. The second matters more than it
    /// looks in both directions.
    ///
    /// Aligning to absolute file offsets would make a request at byte 163840
    /// with a four megabyte window start at byte zero, which is outside the
    /// scope of a source bound to the second half of the payload. So windows
    /// are aligned from the start of the scope span they fall in, and tile
    /// that span. Neighbouring blocks still share one window, which is the
    /// whole point of windowing, but no window ever reaches a byte the
    /// operator did not bind to this source.
    fn window_for(
        &self,
        file_base: u64,
        pos: u64,
        file_len: u64,
    ) -> std::result::Result<(u64, u64), FetchError> {
        let absolute = file_base + pos;
        let Some(span) = self.binding.scope.spans.span_containing(absolute) else {
            return Err(FetchError::permanent(format!(
                "byte {absolute} is outside the scope `{}` of {}",
                self.binding.scope.selector, self.binding.spec.url
            )));
        };
        // The span in this file's coordinates. It may start before this file
        // and end after it, so both ends are clamped to the file.
        let span_start = span.start.saturating_sub(file_base);
        let span_end = (span.end - file_base).min(file_len);

        let offset_in_span = pos - span_start;
        let start = span_start + (offset_in_span - offset_in_span % self.window);
        let length = self.window.min(span_end.saturating_sub(start));
        if length == 0 {
            return Err(FetchError::permanent(format!(
                "window at {start} is past the end of {}",
                self.binding.spec.url
            )));
        }
        Ok((start, length))
    }

    /// The window at file offset `start`, from cache or over HTTP.
    async fn window(
        &self,
        url: &str,
        file_base: u64,
        start: u64,
        length: u64,
    ) -> std::result::Result<Bytes, FetchError> {
        if let Some(hit) = self.cached(url, start).await {
            return Ok(hit);
        }
        let key = (url.to_string(), start);
        let gate = self.gate(key.clone()).await;
        let result = {
            let _guard = gate.lock().await;
            // Whoever held the gate before us may have filled the cache.
            match self.cached(url, start).await {
                Some(hit) => Ok(hit),
                None => {
                    let fetched = self.fetch_with_retry(url, start, length).await;
                    if let Ok(data) = &fetched {
                        self.verify_window(file_base + start, data)?;
                        self.store(url, start, data.clone()).await;
                    }
                    fetched
                }
            }
        };
        self.release(&key, &gate).await;
        result
    }

    /// Apply the source's status policy to a classified failure.
    ///
    /// Whether a status is worth retrying is a property of the server, not of
    /// the code, and the built-in classification is only a good default. A
    /// signing CDN's `403` recovers on the next request; a mirror that answers
    /// `503` forever does not. `--web-seed-retry-status` and
    /// `--web-seed-fatal-status` are how the caller says which they have.
    ///
    /// The reason text is kept exactly as it was, so the message a caller sees
    /// still names the status and the fix. Only the retryability changes.
    fn reclassify(&self, err: FetchError) -> FetchError {
        let Some(code) = err.status() else {
            return err;
        };
        let limits = &self.binding.spec.limits;
        match (limits.status_is_retryable(code), err) {
            (Some(true), FetchError::Permanent { reason, status }) => {
                FetchError::Transient { reason, status }
            }
            (Some(false), FetchError::Transient { reason, status }) => {
                FetchError::Permanent { reason, status }
            }
            (_, err) => err,
        }
    }

    /// One ranged GET with retries and backoff.
    async fn fetch_with_retry(
        &self,
        url: &str,
        start: u64,
        length: u64,
    ) -> std::result::Result<Bytes, FetchError> {
        let limits = &self.binding.spec.limits;
        if self.stats.budget_spent() {
            return Err(FetchError::permanent(format!(
                "{url}: source is cooling down after {} errors",
                self.stats.errors.load(Ordering::Relaxed)
            )));
        }

        let mut backoff = Duration::from_millis(500);
        let mut last: Option<FetchError> = None;
        for attempt in 0..=limits.retries {
            if attempt > 0 {
                self.stats.retries.fetch_add(1, Ordering::Relaxed);
                // Charged to the status of the failure being retried, so a
                // report says what the retries were spent on rather than only
                // how many there were.
                if let Some(code) = last.as_ref().and_then(FetchError::status) {
                    self.stats.record_retry_status(code);
                }
                trace_retry("ranged GET", url, attempt, limits.retries, backoff, &last);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(16));
            }
            // The cap is on bytes off the mirror, so it is taken here and not
            // where a block is served: a block answered from the window cache
            // crossed no wire and throttling it would cap the wrong thing.
            if let Some(rate) = &self.rate {
                rate.take(length).await;
            }
            let permit =
                self.limiter.acquire().await.map_err(|e| {
                    FetchError::permanent(format!("concurrency limiter closed: {e}"))
                })?;
            let outcome = self.fetch_once(url, start, length).await;
            drop(permit);
            match outcome {
                Ok(data) => {
                    self.stats.record_success(data.len() as u64);
                    return Ok(data);
                }
                // A stall spends the whole budget at once and stops the
                // ladder. The mirror is holding the connection: the retry
                // will be held the same way, and so will the request after
                // it, so the four more attempts and the four reconnect
                // backoffs between them are `--web-seed-timeout` each of
                // waiting for a source that has already answered the
                // question. Measured at the defaults, that is the difference
                // between 133 seconds and ten. `--web-seed-cooldown` still
                // decides whether it may come back, because tripping the
                // budget is what a cooldown hangs off. See `TODO/webseed.md`,
                // T-007.
                Err(err) if err.is_stall() => {
                    self.stats.record_error(1, limits.cooldown());
                    return Err(err);
                }
                Err(err) if err.is_retryable() => last = Some(err),
                // A permanent failure does not spend the error budget.
                //
                // That budget is `--web-seed-max-errors` and it exists to
                // retire a source that keeps failing **transiently**, with
                // `--web-seed-cooldown` deciding when it may come back. See
                // `TODO/multi-source.md`, T-130 and T-137. A permanent failure
                // is not a run of bad luck, it is one fact learned once, and
                // it already has its own outcome: the source is retired, or,
                // when the failure is attributable to one file, narrowed to
                // what it can still serve. Counting it here as well charged it
                // twice, and with a small budget one 404 on one file of twelve
                // put the whole mirror into cooldown through the back door.
                // See `TODO/webseed.md`, T-005.
                Err(err) => return Err(err),
            }
        }
        self.stats
            .record_error(limits.max_errors, limits.cooldown());
        Err(last.unwrap_or_else(|| FetchError::transient(format!("{url}: no attempt was made"))))
    }

    /// Issue one ranged GET and check the server honoured it.
    ///
    /// A `file:` source takes the local branch and everything above this stays
    /// the same: the same window cache, the same concurrency limit, the same
    /// rate cap, the same retries, the same per-piece verification, and the
    /// same trace record. Only where the bytes come from differs.
    async fn fetch_once(
        &self,
        url: &str,
        start: u64,
        length: u64,
    ) -> std::result::Result<Bytes, FetchError> {
        let range = format!("bytes={}-{}", start, start + length - 1);
        let began = Instant::now();
        let started_at = Timestamp::now();

        if crate::webseed::local::is_file_url(url) {
            let outcome = read_local(url, start, length).await;
            let ttfb_ms = began.elapsed().as_millis() as u64;
            let bytes = outcome.as_ref().map(|data| data.len() as u64).unwrap_or(0);
            self.record(
                started_at,
                url,
                &range,
                None,
                bytes,
                began,
                Some(ttfb_ms),
                Some("local file".to_string()),
                outcome.as_ref().err(),
            )
            .await;
            return outcome;
        }

        let mut request = self.client.get(url).headers(self.headers.clone());
        request = request.header(RANGE, &range);
        if let Auth::Basic { user, password } = &self.binding.spec.auth {
            request = request.basic_auth(user, Some(password));
        }

        let sent = request.send().await;
        let ttfb_ms = began.elapsed().as_millis() as u64;
        let response = match sent {
            Ok(response) => response,
            Err(e) => {
                let err = classify_transport(url, &e);
                self.record(
                    started_at,
                    url,
                    &range,
                    None,
                    0,
                    began,
                    Some(ttfb_ms),
                    None,
                    Some(&err),
                )
                .await;
                return Err(err);
            }
        };

        let status = response.status();
        let server = response
            .headers()
            .get(reqwest::header::SERVER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let resolved = (response.url().as_str() != url).then(|| response.url().to_string());

        if let Err(err) =
            check_status(url, &response, start, length).map_err(|e| self.reclassify(e))
        {
            self.record(
                started_at,
                url,
                &range,
                Some(status.as_u16()),
                0,
                began,
                Some(ttfb_ms),
                server.clone(),
                Some(&err),
            )
            .await;
            return Err(err);
        }

        let body = match response.bytes().await {
            Ok(body) => body,
            Err(e) => {
                let err = body_failure(url, status.as_u16(), &e);
                self.record(
                    started_at,
                    url,
                    &range,
                    Some(status.as_u16()),
                    0,
                    began,
                    Some(ttfb_ms),
                    server.clone(),
                    Some(&err),
                )
                .await;
                return Err(err);
            }
        };

        if body.len() as u64 != length {
            let err = FetchError::Transient {
                reason: format!("{url}: asked for {length} bytes, got {}", body.len()),
                status: Some(status.as_u16()),
            };
            self.record(
                started_at,
                url,
                &range,
                Some(status.as_u16()),
                body.len() as u64,
                began,
                Some(ttfb_ms),
                server,
                Some(&err),
            )
            .await;
            return Err(err);
        }

        self.record(
            started_at,
            url,
            &range,
            Some(status.as_u16()),
            body.len() as u64,
            began,
            Some(ttfb_ms),
            server,
            None,
        )
        .await;
        if let Some(url) = resolved {
            tracing::debug!(target: "bit_cli::http", resolved_url = %url, "followed a redirect");
        }
        Ok(body)
    }

    #[allow(clippy::too_many_arguments)]
    async fn record(
        &self,
        started_at: Timestamp,
        url: &str,
        range: &str,
        status: Option<u16>,
        bytes: u64,
        began: Instant,
        ttfb_ms: Option<u64>,
        server: Option<String>,
        error: Option<&FetchError>,
    ) {
        let total_ms = began.elapsed().as_millis() as u64;
        tracing::trace!(
            target: "bit_cli::http",
            at = %started_at,
            url = %url,
            range = %range,
            status = ?status,
            bytes,
            total_ms,
            ttfb_ms = ?ttfb_ms,
            error = ?error.map(ToString::to_string),
            "ranged GET"
        );
        let Some(trace) = &self.trace else { return };
        trace.lock().await.push(RequestRecord {
            started_at,
            url: url.to_string(),
            range: range.to_string(),
            resolved_url: None,
            status,
            bytes,
            total_ms,
            ttfb_ms,
            server,
            error: error.map(ToString::to_string),
        });
    }

    /// The gate for one window, creating it if this is the first request.
    async fn gate(&self, key: WindowKey) -> Arc<Mutex<()>> {
        self.inflight.lock().await.entry(key).or_default().clone()
    }

    /// Drop a window's gate once nobody else is waiting on it.
    async fn release(&self, key: &WindowKey, gate: &Arc<Mutex<()>>) {
        let mut inflight = self.inflight.lock().await;
        // Two references means the map's and ours, so nobody else is waiting.
        // Insertion also takes this lock, so a new waiter cannot slip in here.
        if Arc::strong_count(gate) == 2 {
            inflight.remove(key);
        }
    }

    /// Look a window up, promoting it to most-recently-used.
    async fn cached(&self, url: &str, start: u64) -> Option<Bytes> {
        let mut cache = self.cache.lock().await;
        let index = cache
            .iter()
            .position(|w| w.url == url && w.start == start)?;
        let window = cache.remove(index)?;
        let data = window.data.clone();
        cache.push_front(window);
        Some(data)
    }

    /// Insert a window, evicting the least-recently-used one when full.
    async fn store(&self, url: &str, start: u64, data: Bytes) {
        let mut cache = self.cache.lock().await;
        cache.push_front(CachedWindow {
            url: url.to_string(),
            start,
            data,
        });
        while cache.len() > self.capacity {
            cache.pop_back();
        }
    }
}

/// The BEP 17 request URL for one sub-range of one piece.
///
/// The query is `info_hash`, `piece`, and `ranges`, in that order. `info_hash`
/// carries the raw twenty bytes percent-encoded, not the hex rendering: a
/// server given hex answers for a torrent it does not have.
///
/// `ranges` is inclusive at both ends, as BEP 17 defines it. Sending it for a
/// whole piece is allowed and harmless, and sending it always keeps one code
/// path rather than two.
pub fn hoffman_url(
    base: &str,
    info_hash_hex: &str,
    piece: u32,
    begin: u64,
    length: u64,
) -> std::result::Result<String, FetchError> {
    let raw = decode_hex(info_hash_hex)
        .ok_or_else(|| FetchError::permanent(format!("`{info_hash_hex}` is not an info hash")))?;
    let separator = match base.contains('?') {
        true => '&',
        false => '?',
    };
    Ok(format!(
        "{base}{separator}info_hash={}&piece={piece}&ranges={begin}-{}",
        percent_encode_bytes(&raw),
        begin + length - 1
    ))
}

/// The SHA-1 of one piece.
fn sha1_of(data: &[u8]) -> [u8; 20] {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Hex, for a hash in an error message.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Twenty bytes from forty hex characters.
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Percent-encode raw bytes for a query string, per RFC 3986.
fn percent_encode_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for byte in bytes {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Read one byte range out of a local file.
///
/// Open, seek, and read on a blocking thread, once per window, rather than
/// through `tokio::fs`, which is three hops for the same three calls. The
/// handle is not pooled: at the default four megabyte window a one gigabyte
/// file is 256 opens, which is not what bounds this path.
///
/// What is permanent and what is transient follows the same rule as HTTP. A
/// path that is not there, or is shorter than the torrent says, is the source
/// being wrong and will still be wrong next time. An I/O error is the disk
/// being busy or a network share dropping, which is worth another attempt.
async fn read_local(url: &str, start: u64, length: u64) -> std::result::Result<Bytes, FetchError> {
    let (path, read) = crate::webseed::local::read_range(url, start, length).await;
    let display = path.display().to_string();
    match read {
        Ok(data) => Ok(Bytes::from(data)),
        Err(err) => Err(match err.kind() {
            std::io::ErrorKind::NotFound => {
                FetchError::permanent(format!("{display}: no such file"))
            }
            std::io::ErrorKind::UnexpectedEof => FetchError::permanent(format!(
                "{display}: is shorter than the torrent says, asked for bytes {start}-{}",
                start + length - 1
            )),
            std::io::ErrorKind::PermissionDenied => {
                FetchError::permanent(format!("{display}: permission denied"))
            }
            std::io::ErrorKind::InvalidInput => FetchError::permanent(err.to_string()),
            _ => FetchError::transient(format!("{display}: {err}")),
        }),
    }
}

/// Classify an unsuccessful HTTP status.
fn classify_status(url: &str, status: StatusCode) -> FetchError {
    let code = status.as_u16();
    match code {
        404 | 410 => FetchError::Permanent {
            reason: format!("{url}: {status}"),
            status: Some(code),
        },
        401 | 403 => FetchError::Permanent {
            reason: format!("{url}: {status}"),
            status: Some(code),
        },
        416 => FetchError::Permanent {
            reason: format!("{url}: {status}"),
            status: Some(code),
        },
        _ => FetchError::Transient {
            reason: format!("{url}: {status}"),
            status: Some(code),
        },
    }
}

/// One line naming what a transport failure was, in the reader's terms.
///
/// Shared with [`crate::webseed::probe`] so `webseed test` and a download say
/// the same thing about the same failure. They used to disagree: the download
/// path classified, and the probe path printed `reqwest`'s own
/// `error sending request for url (...)`, which names neither the cause nor
/// the flag that bounds it.
pub(crate) fn transport_reason(url: &str, err: &reqwest::Error) -> String {
    // The connect case is asked first because a connect timeout sets both
    // `is_connect` and `is_timeout`, so asking about the timeout first
    // reported every one of them as an ordinary request timeout and the
    // reader turned the wrong knob. The two are bounded by two different
    // flags with two different defaults, and which one expired is the only
    // thing the message has to say. See TODO/webseed.md, T-141.
    if err.is_connect() {
        return match err.is_timeout() {
            true => format!("{url}: connect timed out, raise --web-seed-connect-timeout"),
            false => format!("{url}: could not connect: {err}"),
        };
    }
    if err.is_timeout() {
        return format!("{url}: timed out waiting for the response, raise --web-seed-timeout");
    }
    if err.is_redirect() {
        return format!("{url}: too many redirects: {err}");
    }
    if err.is_builder() {
        return format!("{url}: malformed request: {err}");
    }
    format!("{url}: {err}")
}

/// Turn a transport failure into a classified error.
///
/// Three shapes. A redirect loop and a request this client could not build
/// will not come out differently on a retry, so they are permanent. A request
/// that ran out of time is a **stall**: the mirror is holding the connection
/// and the retry will be held the same way, so retrying it buys another
/// `--web-seed-timeout` of nothing. Everything else, a refused connection or a
/// reset one, is worth another attempt.
fn classify_transport(url: &str, err: &reqwest::Error) -> FetchError {
    let reason = transport_reason(url, err);
    if err.is_redirect() || err.is_builder() {
        return FetchError::permanent(reason);
    }
    match err.is_timeout() {
        true => FetchError::Stalled {
            reason,
            status: None,
        },
        false => FetchError::transient(reason),
    }
}

/// Whether a body that stopped arriving stopped because time ran out.
///
/// `reqwest` reports a request timeout that fires part way through a body as a
/// decode error on the body stream, so the class alone cannot tell a mirror
/// that stalled from one that sent a short body and closed. `is_timeout` on
/// the error is what separates them, and it is the whole basis of T-007's
/// stall detection.
fn body_failure(url: &str, status: u16, err: &reqwest::Error) -> FetchError {
    let reason = format!("{url}: body was cut short: {err}");
    match err.is_timeout() {
        true => FetchError::Stalled {
            reason: format!("{url}: the body stopped arriving, raise --web-seed-timeout"),
            status: Some(status),
        },
        false => FetchError::Transient {
            reason,
            status: Some(status),
        },
    }
}

/// Check the response is actually the range that was asked for.
fn check_status(
    url: &str,
    response: &Response,
    start: u64,
    length: u64,
) -> std::result::Result<(), FetchError> {
    let status = response.status();
    match status {
        StatusCode::PARTIAL_CONTENT => {}
        // A 200 means the server ignored `Range` and is sending the whole
        // entity. Reading the body as if it were the requested range would
        // serve wrong bytes at every offset, so refuse rather than guess.
        StatusCode::OK => {
            let whole_file_asked_for = start == 0
                && response
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    == Some(length);
            if whole_file_asked_for {
                // The request happened to cover the entire entity, so a 200
                // carries exactly the right bytes.
                return Ok(());
            }
            let accepts = response
                .headers()
                .get(ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("none");
            return Err(FetchError::Permanent {
                reason: format!(
                    "{url}: server ignored the Range header and returned the whole entity (Accept-Ranges: {accepts})"
                ),
                status: Some(200),
            });
        }
        StatusCode::RANGE_NOT_SATISFIABLE => {
            let total = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next().map(str::to_string))
                .unwrap_or_else(|| "unknown".to_string());
            return Err(FetchError::Permanent {
                reason: format!(
                    "{url}: 416 for bytes {start}-{}, the server says the resource is {total} bytes; the mirror does not match the torrent",
                    start + length - 1
                ),
                status: Some(416),
            });
        }
        StatusCode::NOT_FOUND => {
            return Err(FetchError::Permanent {
                reason: format!("{url}: 404, the composed URL does not exist on this mirror"),
                status: Some(404),
            });
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            return Err(FetchError::Permanent {
                reason: format!("{url}: {status}, check --web-seed-auth and --web-seed-header"),
                status: Some(status.as_u16()),
            });
        }
        StatusCode::TOO_MANY_REQUESTS => {
            return Err(FetchError::Transient {
                reason: format!(
                    "{url}: 429, lower --web-seed-concurrency or set --web-seed-speed-limit"
                ),
                status: Some(429),
            });
        }
        s if s.is_server_error() => {
            return Err(FetchError::Transient {
                reason: format!("{url}: {s}"),
                status: Some(s.as_u16()),
            });
        }
        s => {
            return Err(FetchError::Permanent {
                reason: format!("{url}: {s}"),
                status: Some(s.as_u16()),
            });
        }
    }

    // A 206 for a different range than was asked for is as wrong as a 200, and
    // far cheaper to catch here than as a hash failure once per piece.
    if let Some(header) = response.headers().get(CONTENT_RANGE)
        && let Some(got) = header.to_str().ok().and_then(parse_content_range_start)
        && got != start
    {
        return Err(FetchError::Permanent {
            reason: format!("{url}: asked for byte {start} but the server sent byte {got}"),
            status: Some(206),
        });
    }
    Ok(())
}

/// Extract the first byte position from `Content-Range: bytes 0-99/200`.
pub fn parse_content_range_start(value: &str) -> Option<u64> {
    let spec = value.trim().strip_prefix("bytes ")?;
    let (start, _) = spec.split_once('-')?;
    start.trim().parse().ok()
}

/// Extract the total size from `Content-Range: bytes 0-99/200`.
pub fn parse_content_range_total(value: &str) -> Option<u64> {
    let total = value.trim().rsplit('/').next()?;
    total.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_start_is_parsed_and_junk_is_rejected() {
        assert_eq!(parse_content_range_start("bytes 0-99/200"), Some(0));
        assert_eq!(
            parse_content_range_start("bytes 1024-2047/4096"),
            Some(1024)
        );
        assert_eq!(parse_content_range_start("bytes */200"), None);
        assert_eq!(parse_content_range_start("items 0-99/200"), None);
        assert_eq!(parse_content_range_start(""), None);
    }

    #[test]
    fn content_range_total_is_parsed() {
        assert_eq!(parse_content_range_total("bytes 0-99/200"), Some(200));
        assert_eq!(parse_content_range_total("bytes 0-99/*"), None);
    }

    #[test]
    fn error_classes_are_stable_names() {
        let cases = [
            (
                FetchError::Permanent {
                    reason: "x".into(),
                    status: Some(404),
                },
                "not_found",
            ),
            (
                FetchError::Permanent {
                    reason: "x".into(),
                    status: Some(416),
                },
                "range_not_satisfiable",
            ),
            (
                FetchError::Permanent {
                    reason: "x".into(),
                    status: Some(403),
                },
                "auth",
            ),
            (
                FetchError::Permanent {
                    reason: "x".into(),
                    status: Some(200),
                },
                "range_ignored",
            ),
            (
                FetchError::Transient {
                    reason: "x".into(),
                    status: Some(503),
                },
                "server_error",
            ),
            (
                FetchError::Transient {
                    reason: "x".into(),
                    status: None,
                },
                "transport",
            ),
            (
                FetchError::HashMismatch { reason: "x".into() },
                "hash_mismatch",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.class(), expected);
        }
    }

    #[test]
    fn only_transient_errors_are_retried() {
        assert!(FetchError::transient("x").is_retryable());
        assert!(!FetchError::permanent("x").is_retryable());
        assert!(!FetchError::HashMismatch { reason: "x".into() }.is_retryable());
    }

    /// A fetcher for one source over a ten-piece single-file torrent.
    fn fetcher(scope: &str, chunk_size: u64) -> Fetcher {
        use crate::webseed::binding::{BindingSet, Origin, SourceSpec};
        use crate::webseed::scope::Scope;

        let layout = Arc::new(Layout::from_lengths(
            "movie.bin",
            false,
            32 * 1024,
            [("movie.bin".to_string(), 320 * 1024u64)],
        ));
        let mut spec = SourceSpec::new("https://mirror.example.com/movie.bin", Origin::CommandLine)
            .with_scope(Scope::parse(scope).unwrap());
        spec.limits.chunk_size = chunk_size;
        let hash = "0".repeat(40);
        let set = BindingSet::resolve(&layout, &hash, &[spec]).unwrap();
        Fetcher::new(set.bindings[0].clone(), layout, hash, 4, false).unwrap()
    }

    #[test]
    fn a_window_tiles_the_file_when_the_whole_torrent_is_in_scope() {
        let fetcher = fetcher("*", 64 * 1024);
        assert_eq!(fetcher.window_for(0, 0, 320 * 1024).unwrap(), (0, 65536));
        assert_eq!(fetcher.window_for(0, 1000, 320 * 1024).unwrap(), (0, 65536));
        assert_eq!(
            fetcher.window_for(0, 65536, 320 * 1024).unwrap(),
            (65536, 65536)
        );
    }

    #[test]
    fn the_last_window_stops_at_the_end_of_the_file() {
        let fetcher = fetcher("*", 256 * 1024);
        // 320 KiB with a 256 KiB window leaves 64 KiB in the second one.
        assert_eq!(
            fetcher.window_for(0, 300 * 1024, 320 * 1024).unwrap(),
            (262144, 65536)
        );
    }

    #[test]
    fn a_window_never_starts_before_the_scope_it_serves() {
        // This is the trap that a naive alignment falls into: a request at
        // 160 KiB with a 4 MiB window aligns to zero, which this source was
        // never bound to.
        let fetcher = fetcher("piece:5-", 4 * crate::units::MIB);
        let (start, length) = fetcher.window_for(0, 5 * 32 * 1024, 320 * 1024).unwrap();
        assert_eq!(
            start,
            5 * 32 * 1024,
            "the window starts where the scope does"
        );
        assert_eq!(length, 5 * 32 * 1024, "and runs to the end of the payload");
    }

    #[test]
    fn a_window_never_reaches_past_the_end_of_its_scope() {
        let fetcher = fetcher("piece:0-4", 4 * crate::units::MIB);
        let (start, length) = fetcher.window_for(0, 0, 320 * 1024).unwrap();
        assert_eq!(start, 0);
        assert_eq!(length, 5 * 32 * 1024, "five pieces, not the whole file");
    }

    #[test]
    fn windows_inside_one_scope_span_still_share_a_cache_entry() {
        // Two blocks a few kilobytes apart have to resolve to the same window,
        // or the cache never hits and every block costs an HTTP request.
        let fetcher = fetcher("piece:5-", 64 * 1024);
        let first = fetcher.window_for(0, 5 * 32 * 1024, 320 * 1024).unwrap();
        let second = fetcher
            .window_for(0, 5 * 32 * 1024 + 16384, 320 * 1024)
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn a_byte_outside_the_scope_is_refused_before_any_request_is_made() {
        let fetcher = fetcher("piece:5-", 4 * crate::units::MIB);
        let err = fetcher.window_for(0, 0, 320 * 1024).unwrap_err();
        assert!(
            !err.is_retryable(),
            "an out-of-scope read is a bug, not a transient failure"
        );
        assert!(err.to_string().contains("outside the scope"), "{err}");
    }

    const HASH: &str = "0102030405060708090a0b0c0d0e0f1011121314";

    #[test]
    fn a_bep_17_url_carries_the_raw_info_hash_not_the_hex_one() {
        let url = hoffman_url("https://seed.example.com/data", HASH, 3, 0, 16384).unwrap();
        assert!(
            url.contains("info_hash=%01%02%03%04%05%06%07%08%09%0A%0B%0C%0D%0E%0F%10%11%12%13%14"),
            "{url}"
        );
        assert!(
            !url.contains(HASH),
            "the hex rendering is the wrong twenty bytes: {url}"
        );
    }

    #[test]
    fn a_bep_17_url_names_the_piece_and_an_inclusive_range() {
        let url = hoffman_url("https://seed.example.com/data", HASH, 3, 0, 16384).unwrap();
        assert!(url.contains("&piece=3"), "{url}");
        assert!(
            url.ends_with("&ranges=0-16383"),
            "the range is inclusive at both ends: {url}"
        );

        let url = hoffman_url("https://seed.example.com/data", HASH, 0, 16384, 16384).unwrap();
        assert!(url.ends_with("&ranges=16384-32767"), "{url}");
    }

    #[test]
    fn a_bep_17_base_that_already_has_a_query_gets_an_ampersand() {
        let url = hoffman_url("https://seed.example.com/s?k=v", HASH, 0, 0, 1).unwrap();
        assert!(
            url.starts_with("https://seed.example.com/s?k=v&info_hash="),
            "{url}"
        );
    }

    #[test]
    fn a_malformed_info_hash_is_refused_rather_than_sent() {
        assert!(hoffman_url("https://s.example.com/", "not-hex", 0, 0, 1).is_err());
        assert!(hoffman_url("https://s.example.com/", "abc", 0, 0, 1).is_err());
    }

    #[test]
    fn statuses_are_classified_the_same_way_on_both_wire_styles() {
        assert!(!classify_status("u", StatusCode::NOT_FOUND).is_retryable());
        assert!(!classify_status("u", StatusCode::FORBIDDEN).is_retryable());
        assert!(!classify_status("u", StatusCode::RANGE_NOT_SATISFIABLE).is_retryable());
        assert!(classify_status("u", StatusCode::SERVICE_UNAVAILABLE).is_retryable());
        assert_eq!(
            classify_status("u", StatusCode::NOT_FOUND).class(),
            "not_found"
        );
    }

    /// A transport failure says which timeout expired, or which did not.
    ///
    /// The two timeouts are two flags with two defaults, and `reqwest` sets
    /// `is_timeout()` on a connect timeout as well as on a request one, so
    /// asking about the timeout first reports every connect timeout as a
    /// request timeout and the reader raises the wrong flag. That is what
    /// this pins. See `TODO/webseed.md`, T-141.
    ///
    /// Two of the three shapes are reachable with no network and no firewall:
    /// a listener that accepts and never answers is a request timeout, and a
    /// port nothing listens on is a refused connect. The third, a connect that
    /// never completes, needs an address the network does not route, which is
    /// what `scripts/check-connect-timeout.ps1` drives.
    #[tokio::test(flavor = "current_thread")]
    async fn a_transport_failure_names_the_timeout_that_expired() {
        use std::time::Duration;

        // Accept and hold. Nothing is ever written, so the client waits out
        // its request timeout with the connection already established.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let held = tokio::spawn(async move {
            let mut sockets = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                sockets.push(socket);
            }
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(300))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}/x");
        let err = client.get(&url).send().await.expect_err("it cannot answer");
        assert!(err.is_timeout(), "{err}");
        assert!(!err.is_connect(), "the connection was established: {err}");
        let reason = transport_reason(&url, &err);
        assert!(
            reason.contains("timed out waiting for the response")
                && reason.contains("--web-seed-timeout"),
            "{reason}"
        );
        assert!(
            !reason.contains("connect timed out"),
            "a request timeout must not be reported as a connect timeout: {reason}"
        );
        // A stall, not a transient failure, and the distinction is T-007's.
        // This used to assert `is_retryable`, on the reading that the same
        // request might work on the next attempt. It will not: the mirror has
        // the connection open and is not writing to it, and it will hold the
        // retry the same way. Every attempt spent here is one
        // `--web-seed-timeout` the other sources were not asked, and at the
        // defaults the ladder and the reconnect backoff above it turned one
        // hung mirror into 133 seconds.
        let classified = classify_transport(&url, &err);
        assert!(classified.is_stall(), "{reason}");
        assert!(
            !classified.is_retryable(),
            "a stall must not spend the retry ladder: {reason}"
        );
        assert_eq!(classified.class(), "stalled");
        held.abort();

        // A port nothing listens on is refused rather than timed out, and the
        // message says so without naming either flag, because neither would
        // have helped.
        let closed = {
            let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = socket.local_addr().unwrap().port();
            drop(socket);
            port
        };
        let url = format!("http://127.0.0.1:{closed}/x");
        let err = client
            .get(&url)
            .send()
            .await
            .expect_err("nothing is listening");
        if err.is_connect() && !err.is_timeout() {
            let reason = transport_reason(&url, &err);
            assert!(reason.contains("could not connect"), "{reason}");
            assert!(!reason.contains("--web-seed-connect-timeout"), "{reason}");
        }
    }

    #[test]
    fn a_file_offset_is_converted_into_the_torrent_offset_the_scope_uses() {
        // The second file of a multi-file torrent starts at byte 160 KiB, so
        // its file offset zero is torrent offset 163840.
        let fetcher = fetcher("piece:5-", 64 * 1024);
        let (start, _) = fetcher.window_for(5 * 32 * 1024, 0, 160 * 1024).unwrap();
        assert_eq!(
            start, 0,
            "the scope span starts exactly where this file does"
        );
        assert!(fetcher.window_for(0, 0, 320 * 1024).is_err());
    }

    #[test]
    fn errors_carry_the_right_exit_code() {
        use crate::exit::ExitCode;
        assert_eq!(
            Error::from(FetchError::transient("x")).code(),
            ExitCode::Network
        );
        assert_eq!(
            Error::from(FetchError::permanent("x")).code(),
            ExitCode::NoUsableSources
        );
        assert_eq!(
            Error::from(FetchError::HashMismatch { reason: "x".into() }).code(),
            ExitCode::HashMismatch
        );
    }

    #[test]
    fn a_record_reproduces_the_request_as_curl() {
        let record = RequestRecord {
            started_at: Timestamp::from_epoch_ms(0),
            url: "https://mirror.example.com/pub/album/disc 1/a.flac".to_string(),
            range: "bytes=1024-2047".to_string(),
            resolved_url: None,
            status: Some(206),
            bytes: 1024,
            total_ms: 42,
            ttfb_ms: Some(11),
            server: Some("nginx".to_string()),
            error: None,
        };
        let curl = record.as_curl(&[("X-Region".to_string(), "apac".to_string())]);
        assert!(curl.contains("'Range: bytes=1024-2047'"), "{curl}");
        assert!(curl.contains("'X-Region: apac'"), "{curl}");
        assert!(
            curl.contains("'https://mirror.example.com/pub/album/disc 1/a.flac'"),
            "{curl}"
        );
    }

    #[test]
    fn shell_quoting_survives_a_single_quote() {
        assert_eq!(shell_quote("plain"), "plain");
        assert_eq!(shell_quote("https://e.com/a?b=c"), "'https://e.com/a?b=c'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn cooldown_trips_only_after_the_configured_run_of_errors() {
        let stats = SourceStats::default();
        let cooldown = Duration::from_secs(600);
        assert!(!stats.record_error(3, cooldown));
        assert!(!stats.record_error(3, cooldown));
        assert!(
            stats.record_error(3, cooldown),
            "the third consecutive error trips it"
        );
        assert!(stats.is_cooling_down());
        assert!(stats.budget_spent());
        assert!(stats.cooldown_until().is_some());
        assert_eq!(stats.cooldowns(), 1);
    }

    /// A zero cooldown still spends the budget. It is the default, and it
    /// means the source does not come back, so the two questions have to be
    /// answerable apart: the budget is spent and there is nothing to wait for.
    /// See `TODO/multi-source.md`, T-137.
    #[test]
    fn a_zero_cooldown_spends_the_budget_with_nothing_to_wait_for() {
        let stats = SourceStats::default();
        let none = Duration::ZERO;
        assert!(!stats.record_error(2, none));
        assert!(stats.record_error(2, none), "the second error trips it");
        assert!(
            stats.budget_spent(),
            "a source out for the run is still out for the run"
        );
        assert!(!stats.is_cooling_down(), "there is nothing to wait for");
        assert_eq!(stats.cooldown_remaining(), None);
        assert_eq!(stats.cooldowns(), 1);
    }

    /// Waiting the deadline out puts the source back to work, and the run's
    /// history is kept.
    #[test]
    fn ending_a_cooldown_clears_the_error_run_but_not_the_totals() {
        let stats = SourceStats::default();
        let cooldown = Duration::from_millis(50);
        stats.record_error(1, cooldown);
        let deadline = stats.cooldown_until().expect("a deadline");
        assert!(stats.budget_spent());

        assert!(
            !stats.end_cooldown(Timestamp::from_epoch_ms(deadline.epoch_ms() + 1)),
            "a deadline that is not the one in force clears nothing"
        );
        assert!(stats.budget_spent());

        assert!(stats.end_cooldown(deadline));
        assert!(!stats.budget_spent(), "the source is usable again");
        assert_eq!(stats.consecutive_errors.load(Ordering::Relaxed), 0);
        assert_eq!(stats.errors.load(Ordering::Relaxed), 1, "history is kept");
        assert_eq!(stats.cooldowns(), 1, "and so is the count of cooldowns");
    }

    #[test]
    fn a_success_clears_the_consecutive_error_run() {
        let stats = SourceStats::default();
        let cooldown = Duration::from_secs(600);
        stats.record_error(3, cooldown);
        stats.record_error(3, cooldown);
        stats.record_success(1024);
        assert_eq!(stats.consecutive_errors.load(Ordering::Relaxed), 0);
        assert!(!stats.record_error(3, cooldown), "the run started over");
        assert!(!stats.is_cooling_down());
        assert_eq!(stats.bytes.load(Ordering::Relaxed), 1024);
    }

    #[test]
    fn the_default_user_agent_names_the_tool_and_version() {
        let agent = default_user_agent();
        assert!(agent.starts_with("bit-cli/"), "{agent}");
        assert!(agent.len() > "bit-cli/".len());
    }

    /// The bucket hands out one second of burst immediately and then paces.
    ///
    /// Timed with the tokio test clock rather than a wall clock, so the
    /// assertion is about the delay the limiter asked for and not about how
    /// busy the machine was.
    #[tokio::test(start_paused = true)]
    async fn a_rate_limit_paces_after_the_first_second_of_burst() {
        let limiter = RateLimiter::new(1024);
        let began = tokio::time::Instant::now();

        // The bucket starts full, so a second of traffic is free.
        limiter.take(1024).await;
        assert_eq!(began.elapsed(), Duration::ZERO, "the burst is not paced");

        // The next second's worth has to be waited for.
        limiter.take(1024).await;
        assert_eq!(began.elapsed(), Duration::from_secs(1));

        // And a request larger than the whole bucket is served rather than
        // deadlocking, by waiting out its own deficit.
        limiter.take(4096).await;
        assert_eq!(began.elapsed(), Duration::from_secs(5));
    }

    /// A source with no cap never waits.
    #[tokio::test(start_paused = true)]
    async fn no_rate_limit_never_waits() {
        let limiter = RateLimiter::new(u64::MAX / 2);
        let began = tokio::time::Instant::now();
        for _ in 0..64 {
            limiter.take(4 * 1024 * 1024).await;
        }
        assert_eq!(began.elapsed(), Duration::ZERO);
    }

    /// The cap reaches the fetcher from the spec, which is what
    /// `--web-seed-speed-limit` and a binding table's `rate_limit` both set.
    #[test]
    fn a_source_limit_becomes_a_fetcher_rate() {
        use crate::webseed::binding::{Origin, SourceSpec};

        let layout = std::sync::Arc::new(crate::layout::Layout::from_lengths(
            "payload.bin",
            false,
            1024,
            [("payload.bin".to_string(), 4096u64)],
        ));
        let mut spec = SourceSpec::new("http://127.0.0.1:9/", Origin::CommandLine);
        spec.limits.rate_limit = Some(5 * crate::units::MIB);
        let bindings =
            crate::webseed::binding::BindingSet::resolve(&layout, &"a".repeat(40), &[spec])
                .expect("resolve");
        let fetcher = Fetcher::new(
            bindings.bindings[0].clone(),
            layout,
            "a".repeat(40),
            2,
            false,
        )
        .expect("fetcher");
        assert!(fetcher.rate.is_some(), "the cap did not reach the fetcher");
    }

    /// A fetcher whose source carries a status policy.
    fn fetcher_with_policy(retry: &str, fatal: &str) -> Fetcher {
        use crate::webseed::binding::{BindingSet, Origin, SourceSpec, StatusSet};

        let layout = Arc::new(Layout::from_lengths(
            "movie.bin",
            false,
            32 * 1024,
            [("movie.bin".to_string(), 320 * 1024u64)],
        ));
        let mut spec = SourceSpec::new("https://cdn.example.com/movie.bin", Origin::CommandLine);
        spec.limits.retry_status = StatusSet::parse(retry).unwrap();
        spec.limits.fatal_status = StatusSet::parse(fatal).unwrap();
        let hash = "0".repeat(40);
        let set = BindingSet::resolve(&layout, &hash, &[spec]).unwrap();
        Fetcher::new(set.bindings[0].clone(), layout, hash, 4, false).unwrap()
    }

    #[test]
    fn a_403_a_signing_cdn_recovers_from_becomes_retryable() {
        let fetcher = fetcher_with_policy("403", "");
        let err = fetcher.reclassify(classify_status("u", StatusCode::FORBIDDEN));
        assert!(err.is_retryable());
        assert_eq!(err.status(), Some(403));
    }

    #[test]
    fn the_reason_text_survives_the_reclassification() {
        let fetcher = fetcher_with_policy("403", "");
        let before = check_status_reason(403);
        let after = fetcher.reclassify(classify_status("u", StatusCode::FORBIDDEN));
        assert_eq!(reason_of(&after), before);
    }

    /// The reason `classify_status` gives a code, for comparison.
    fn check_status_reason(code: u16) -> String {
        reason_of(&classify_status(
            "u",
            StatusCode::from_u16(code).expect("status"),
        ))
    }

    fn reason_of(err: &FetchError) -> String {
        match err {
            FetchError::Transient { reason, .. }
            | FetchError::Permanent { reason, .. }
            | FetchError::Stalled { reason, .. }
            | FetchError::HashMismatch { reason } => reason.clone(),
        }
    }

    /// Headers promising a kilobyte, and less than a kilobyte behind them.
    ///
    /// The line endings are escapes rather than a multi-line literal, because
    /// a control byte written as itself is invisible in a diff and makes the
    /// file harder to search. RULES.md section 5 has what that has already
    /// cost twice.
    const SHORT_RESPONSE: &[u8] =
        b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\nnot the whole body";

    /// A body that stops arriving is a stall, and it looks exactly like a
    /// short body until `is_timeout` is asked.
    ///
    /// This is the case the whole of T-007 turns on. `reqwest` reports a
    /// request timeout that fires part way through a body as a decode error on
    /// the body stream, so the failure class is the same one a mirror that
    /// closed early produces: `Transient`, status 200, "body was cut short".
    /// Told apart by the class alone, a hung backend spends the retry ladder
    /// and the error budget and the reconnect backoff between them, which
    /// measured 133 seconds at the defaults.
    #[tokio::test]
    async fn a_body_that_stops_arriving_is_a_stall_and_not_a_short_body() {
        use std::time::Duration;
        use tokio::io::AsyncWriteExt;

        // Headers, a little body, and then silence with the connection held.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let held = tokio::spawn(async move {
            let mut sockets = Vec::new();
            while let Ok((mut socket, _)) = listener.accept().await {
                let _ = socket.write_all(SHORT_RESPONSE).await;
                let _ = socket.flush().await;
                sockets.push(socket);
            }
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}/x");
        let response = client.get(&url).send().await.expect("headers arrive");
        let status = response.status().as_u16();
        let err = response.bytes().await.expect_err("the body never finishes");
        held.abort();

        assert!(err.is_timeout(), "the body read ran out of time: {err}");
        let classified = body_failure(&url, status, &err);
        assert!(classified.is_stall(), "{classified}");
        assert!(!classified.is_retryable(), "{classified}");
        assert_eq!(classified.class(), "stalled");
        assert_eq!(classified.status(), Some(200));
        assert!(
            classified.to_string().contains("--web-seed-timeout"),
            "the message names the flag that bounds it: {classified}"
        );
    }

    /// A mirror that closed early is still worth another request.
    ///
    /// The other half of the pair above: same class, same status, and the
    /// opposite handling, because this one answered wrongly rather than not
    /// answering.
    #[tokio::test]
    async fn a_body_that_ends_early_is_still_transient() {
        use std::time::Duration;
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // The write half is closed so the client sees the body end early, and
        // the socket is **kept** so the read half stays open. Dropping it here
        // resets the connection before the client has read the headers, and
        // then the failure is an aborted request rather than the short body
        // this is about.
        let held = tokio::spawn(async move {
            let mut sockets = Vec::new();
            while let Ok((mut socket, _)) = listener.accept().await {
                let _ = socket.write_all(SHORT_RESPONSE).await;
                let _ = socket.shutdown().await;
                sockets.push(socket);
            }
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}/x");
        let response = client.get(&url).send().await.expect("headers arrive");
        let status = response.status().as_u16();
        let err = response.bytes().await.expect_err("the body is short");
        held.abort();

        assert!(!err.is_timeout(), "it closed rather than hanging: {err}");
        let classified = body_failure(&url, status, &err);
        assert!(!classified.is_stall(), "{classified}");
        assert!(classified.is_retryable(), "{classified}");
    }

    #[test]
    fn a_503_a_mirror_never_recovers_from_becomes_fatal() {
        let fetcher = fetcher_with_policy("", "503");
        let err = fetcher.reclassify(classify_status("u", StatusCode::SERVICE_UNAVAILABLE));
        assert!(!err.is_retryable());
        assert_eq!(err.status(), Some(503));
    }

    #[test]
    fn a_status_no_policy_names_keeps_its_built_in_classification() {
        let fetcher = fetcher_with_policy("403", "503");
        assert!(
            !fetcher
                .reclassify(classify_status("u", StatusCode::NOT_FOUND))
                .is_retryable()
        );
        assert!(
            fetcher
                .reclassify(classify_status("u", StatusCode::BAD_GATEWAY))
                .is_retryable()
        );
    }

    #[test]
    fn a_source_with_no_policy_changes_nothing() {
        let fetcher = fetcher_with_policy("", "");
        for code in [401u16, 403, 404, 410, 416, 429, 500, 503] {
            let status = StatusCode::from_u16(code).unwrap();
            assert_eq!(
                fetcher
                    .reclassify(classify_status("u", status))
                    .is_retryable(),
                classify_status("u", status).is_retryable(),
                "status {code} was reclassified with no policy set"
            );
        }
    }

    #[test]
    fn a_hash_mismatch_is_never_reclassified_because_it_carries_no_status() {
        let fetcher = fetcher_with_policy("403,500-599", "");
        let err = fetcher.reclassify(FetchError::HashMismatch {
            reason: "piece 3".into(),
        });
        assert!(!err.is_retryable());
    }

    #[test]
    fn retries_are_charged_to_the_status_that_caused_them() {
        let stats = SourceStats::default();
        stats.record_retry_status(403);
        stats.record_retry_status(403);
        stats.record_retry_status(503);
        let by_status = stats.retries_by_status();
        assert_eq!(by_status[&403], 2);
        assert_eq!(by_status[&503], 1);
        assert_eq!(by_status.len(), 2);
    }

    #[test]
    fn a_source_that_never_retried_reports_no_statuses() {
        assert!(SourceStats::default().retries_by_status().is_empty());
    }
}
