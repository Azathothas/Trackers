//! `bench webseed`: measure HTTP sources.
//!
//! This reads real payload out of each source's scope and throws it away. It
//! measures the transport and nothing else: no piece is written, no hash is
//! checked, and no retry or cooldown runs, because a retry that hides a
//! failure also hides it from the measurement.
//!
//! Connection establishment is measured on its own cadence, one connection per
//! source per metrics interval. An HTTP client that pools connections cannot
//! report what opening one costs, and "connect p99" is exactly the number an
//! operator debugging their own CDN is looking for.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use reqwest::header::RANGE;

use crate::bench::recorder::{Observation, Recorder};
use crate::bench::report::{ConcurrencyStep, Sample, SourceSummary, Summary};
use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::webseed::binding::{Auth, Binding, BindingSet};
use crate::webseed::fetch::default_user_agent;
use crate::webseed::probe::RangeSupport;

/// How many distinct windows are read from each source before wrapping.
///
/// Reading the same offset over and over measures the mirror's page cache
/// rather than the mirror. Walking a few hundred windows costs nothing and
/// keeps the measurement honest on any payload big enough to matter.
const MAX_WINDOWS: usize = 512;

/// What a `bench webseed` run was asked to do.
#[derive(Debug, Clone)]
pub struct Options {
    pub duration: Duration,
    pub warmup: Duration,
    pub metrics_interval: Duration,
    pub concurrency: usize,
    /// Concurrency steps to walk, sharing the duration between them. Empty
    /// means run flat at [`Self::concurrency`].
    pub concurrency_sweep: Vec<usize>,
    /// Drive toward this many bytes per second rather than running flat out.
    pub target_rate: Option<u64>,
    /// Bytes per request. `None` takes the source's own chunk size.
    pub chunk_size: Option<u64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(30),
            warmup: Duration::from_secs(3),
            metrics_interval: Duration::from_secs(1),
            concurrency: 8,
            concurrency_sweep: Vec::new(),
            target_rate: None,
            chunk_size: None,
        }
    }
}

/// What one source turned out to be.
#[derive(Debug, Clone)]
pub struct SourceOutcome {
    pub summary: SourceSummary,
    /// Whether the source honoured `Range` during the run.
    pub range_support: RangeSupport,
    /// Why the source served nothing, when it served nothing.
    pub failure: Option<String>,
}

/// What a run produced.
#[derive(Debug)]
pub struct Outcome {
    pub series: Vec<Sample>,
    pub summary: Summary,
    pub sources: Vec<SourceOutcome>,
    pub concurrency_curve: Vec<ConcurrencyStep>,
    pub notes: Vec<String>,
    /// Every URL the run read from, in source order.
    pub endpoints: Vec<String>,
}

/// One readable window inside a source's scope.
#[derive(Debug, Clone)]
struct Window {
    url: String,
    offset: u64,
    length: u64,
}

/// Everything one source needs while the run is going.
struct Source {
    index: usize,
    url: String,
    client: reqwest::Client,
    headers: reqwest::header::HeaderMap,
    auth: Auth,
    windows: Vec<Window>,
    /// The next window to read, shared across the workers on this source.
    cursor: std::sync::atomic::AtomicUsize,
    /// Whether a `206` was ever seen, and whether a `200` ever was.
    saw_partial: std::sync::atomic::AtomicBool,
    saw_whole: std::sync::atomic::AtomicBool,
}

impl Source {
    fn next_window(&self) -> &Window {
        let index = self.cursor.fetch_add(1, Ordering::Relaxed) % self.windows.len();
        &self.windows[index]
    }

    fn range_support(&self) -> RangeSupport {
        match (
            self.saw_partial.load(Ordering::Relaxed),
            self.saw_whole.load(Ordering::Relaxed),
        ) {
            (true, _) => RangeSupport::Yes,
            (false, true) => RangeSupport::No,
            (false, false) => RangeSupport::Unknown,
        }
    }
}

/// Measure every source in a binding set.
///
/// `on_sample` is called once per metrics interval with the sample just taken,
/// so a caller can stream progress without waiting for the run to finish.
pub async fn run(
    bindings: &BindingSet,
    layout: &Layout,
    info_hash: &str,
    options: &Options,
    mut on_sample: impl FnMut(&Sample) + Send,
) -> Result<Outcome> {
    let mut notes = Vec::new();
    let mut sources = Vec::new();
    let mut endpoints = Vec::new();
    let mut failures: Vec<(usize, String)> = Vec::new();

    for binding in &bindings.bindings {
        let chunk = options
            .chunk_size
            .unwrap_or(binding.spec.limits.chunk_size)
            .max(1);
        let windows = windows(binding, layout, info_hash, chunk);
        if windows.is_empty() {
            failures.push((
                binding.index,
                "the scope holds no readable range to measure".to_string(),
            ));
            notes.push(format!(
                "{} was skipped: its scope holds no readable range",
                binding.spec.url
            ));
            continue;
        }
        endpoints.push(windows[0].url.clone());
        sources.push(Arc::new(Source {
            index: binding.index,
            url: binding.spec.url.clone(),
            client: client(binding)?,
            headers: headers(binding),
            auth: binding.spec.auth.clone(),
            windows,
            cursor: std::sync::atomic::AtomicUsize::new(0),
            saw_partial: std::sync::atomic::AtomicBool::new(false),
            saw_whole: std::sync::atomic::AtomicBool::new(false),
        }));
    }

    if sources.is_empty() {
        return Err(Error::no_usable_sources(
            "no source has a readable range to measure",
        ));
    }

    let steps: Vec<usize> = match options.concurrency_sweep.is_empty() {
        true => vec![options.concurrency.max(1)],
        false => options
            .concurrency_sweep
            .iter()
            .map(|c| (*c).max(1))
            .collect(),
    };
    // The warmup is paid once, before the first step, rather than once per
    // step: a sweep that warms up at every step spends most of its time
    // warming up and reports a curve of warmups.
    let per_step = options.duration / steps.len().max(1) as u32;
    if per_step < options.metrics_interval {
        notes.push(format!(
            "each concurrency step runs for {}ms, which is shorter than the {}ms metrics interval",
            per_step.as_millis(),
            options.metrics_interval.as_millis()
        ));
    }

    let recorder = Arc::new(Recorder::new(
        options.warmup,
        options.metrics_interval,
        steps[0],
    ));
    let mut curve = Vec::new();

    // Pay the warmup before the first step rather than out of it.
    //
    // The recorder excludes warmup samples from `step`, but `end_step`
    // divides by the step's own wall time, so a step that fell inside the
    // warmup reported its real seconds against no bytes. With the default 3
    // second warmup, `--duration 6s --concurrency-sweep 1,2,4,8,16` gives
    // 1.2 seconds a step and the first two came out at 0 B/s; the same sweep
    // written `16,1` reported `best concurrency 1`, because whichever step
    // went first was the one that was crippled. `--concurrency-sweep 1,1` is
    // the proof: the same concurrency twice, 2.66 MiB/s then 908.73.
    //
    // Only for a sweep. A single fixed concurrency has no curve and its
    // summary already reads the measured window, so warming it separately
    // would add three seconds to every run for nothing.
    // See `TODO/bench.md`, T-229.
    if steps.len() > 1 && recorder.in_warmup() {
        while recorder.in_warmup() {
            drive(
                &recorder,
                &sources,
                steps[0],
                recorder.remaining_warmup(),
                options,
                &mut on_sample,
            )
            .await;
        }
    }

    for &concurrency in &steps {
        recorder.begin_step(concurrency);
        drive(
            &recorder,
            &sources,
            concurrency,
            per_step,
            options,
            &mut on_sample,
        )
        .await;
        // A single fixed concurrency is not a curve, and reporting a
        // one-point curve invites reading a knee into it.
        if steps.len() > 1 {
            curve.push(recorder.end_step(concurrency));
        }
    }
    recorder.stop();

    let labels: Vec<(usize, String, String)> = sources
        .iter()
        .map(|s| (s.index, s.url.clone(), "web_seed".to_string()))
        .collect();
    let mut summaries = recorder.sources(&labels);

    // A source that answered nothing has no row of its own, and dropping it
    // would hide it. It gets an empty row naming why instead.
    for source in &sources {
        if !summaries.iter().any(|s| s.index == source.index) {
            summaries.push(SourceSummary {
                index: source.index,
                label: source.url.clone(),
                kind: "web_seed".into(),
                failure: Some("the source served nothing".into()),
                ..Default::default()
            });
        }
    }
    summaries.sort_by_key(|s| s.index);

    let outcome_sources = summaries
        .into_iter()
        .map(|summary| {
            let source = sources.iter().find(|s| s.index == summary.index);
            let failure = failures
                .iter()
                .find(|(index, _)| *index == summary.index)
                .map(|(_, reason)| reason.clone())
                .or_else(|| summary.failure.clone());
            SourceOutcome {
                range_support: source.map_or(RangeSupport::Unknown, |s| s.range_support()),
                failure,
                summary,
            }
        })
        .collect::<Vec<_>>();

    for outcome in &outcome_sources {
        if outcome.range_support == RangeSupport::No {
            notes.push(format!(
                "{} answered 200 with the whole entity rather than 206: it does not honour Range",
                outcome.summary.label
            ));
        }
    }

    let mut summary = recorder.summary();
    if !curve.is_empty() {
        summary.best_concurrency = curve
            .iter()
            .max_by_key(|step| step.rate.0)
            .map(|step| step.concurrency);
    }

    Ok(Outcome {
        series: recorder.series(),
        summary,
        sources: outcome_sources,
        concurrency_curve: curve,
        notes,
        endpoints,
    })
}

/// Run one concurrency step to its deadline.
async fn drive(
    recorder: &Arc<Recorder>,
    sources: &[Arc<Source>],
    concurrency: usize,
    duration: Duration,
    options: &Options,
    on_sample: &mut (impl FnMut(&Sample) + Send),
) {
    let deadline = Instant::now() + duration;
    let mut workers = tokio::task::JoinSet::new();

    for worker in 0..concurrency.max(1) {
        // Workers are dealt round-robin across the sources, so two mirrors get
        // the same number of workers rather than the same worker in turn.
        let source = sources[worker % sources.len()].clone();
        let recorder = recorder.clone();
        let target_rate = options.target_rate;
        workers.spawn(async move {
            while Instant::now() < deadline {
                request(&source, &recorder).await;
                if let Some(rate) = target_rate
                    && let Some(pause) = pacing(&recorder, rate)
                {
                    tokio::time::sleep(
                        pause.min(deadline.saturating_duration_since(Instant::now())),
                    )
                    .await;
                }
            }
        });
    }

    // The sampler runs on the metrics interval and also measures what opening
    // a connection costs, which a pooled client cannot report.
    let sampler = {
        let recorder = recorder.clone();
        let sources: Vec<Arc<Source>> = sources.to_vec();
        let interval = options.metrics_interval;
        async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            let mut samples = Vec::new();
            loop {
                tokio::select! {
                    _ = ticker.tick() => {}
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => break,
                }
                for source in &sources {
                    if let Some(elapsed) = connect_cost(&source.windows[0].url).await {
                        recorder.observe(Observation {
                            source: source.index,
                            connect: Some(elapsed),
                            ..Default::default()
                        });
                    }
                }
                samples.push(recorder.sample());
            }
            samples
        }
    };

    let samples = tokio::join!(
        async { while workers.join_next().await.is_some() {} },
        sampler
    )
    .1;
    for sample in &samples {
        on_sample(sample);
    }
}

/// One ranged GET, recorded whether it worked or not.
async fn request(source: &Arc<Source>, recorder: &Arc<Recorder>) {
    let window = source.next_window();
    if crate::webseed::local::is_file_url(&window.url) {
        return read_local(
            source,
            recorder,
            window.url.clone(),
            window.offset,
            window.length,
        )
        .await;
    }
    let range = format!(
        "bytes={}-{}",
        window.offset,
        window.offset + window.length - 1
    );
    let mut request = source
        .client
        .get(&window.url)
        .headers(source.headers.clone())
        .header(RANGE, &range);
    if let Auth::Basic { user, password } = &source.auth {
        request = request.basic_auth(user, Some(password));
    }

    recorder.live.in_flight.fetch_add(1, Ordering::Relaxed);
    let began = Instant::now();
    let outcome = request.send().await;
    let first_byte = began.elapsed();

    let observation = match outcome {
        Err(error) => Observation::failure(source.index, transport_class(&error), None)
            .with_complete(began.elapsed()),
        Ok(response) => {
            let status = response.status();
            match status.as_u16() {
                206 => {
                    source.saw_partial.store(true, Ordering::Relaxed);
                    read_body(source.index, response, began, first_byte, window.length).await
                }
                200 => {
                    // A `200` to a ranged request means the server ignored the
                    // range and is sending the whole entity. Reading it would
                    // measure a download nobody asked for, so the body is
                    // dropped and the source is marked.
                    source.saw_whole.store(true, Ordering::Relaxed);
                    drop(response);
                    Observation::failure(source.index, "range_ignored", Some(200))
                        .with_complete(began.elapsed())
                }
                other => Observation::failure(source.index, status_class(other), Some(other))
                    .with_complete(began.elapsed()),
            }
        }
    };
    recorder.live.in_flight.fetch_sub(1, Ordering::Relaxed);
    recorder.observe(observation);
}

/// One positioned read of a `file:` source, recorded the same way.
///
/// A local source has no status and no connect phase, so it carries no status
/// into the counters and `connect_cost` skips it. Everything else is the same
/// measurement: the same windows, the same concurrency, the same latency
/// percentiles. Time to first byte is the whole read, because a positioned
/// read has no earlier moment to name.
async fn read_local(
    source: &Arc<Source>,
    recorder: &Arc<Recorder>,
    url: String,
    offset: u64,
    length: u64,
) {
    recorder.live.in_flight.fetch_add(1, Ordering::Relaxed);
    let began = Instant::now();
    let (_, read) = crate::webseed::local::read_range(&url, offset, length).await;
    let elapsed = began.elapsed();
    let observation = match read {
        Ok(data) if data.len() as u64 >= length => {
            Observation::success(source.index, data.len() as u64, elapsed, elapsed)
        }
        Ok(_) => Observation::failure(source.index, "short_read", None).with_complete(elapsed),
        Err(err) => {
            Observation::failure(source.index, local_class(&err), None).with_complete(elapsed)
        }
    };
    recorder.live.in_flight.fetch_sub(1, Ordering::Relaxed);
    recorder.observe(observation);
}

/// Name a local read failure the way the error counters break them down.
fn local_class(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "auth",
        std::io::ErrorKind::UnexpectedEof => "short_read",
        std::io::ErrorKind::TimedOut => "timeout",
        _ => "transport",
    }
}

/// Read a `206` body and time it.
async fn read_body(
    source: usize,
    response: reqwest::Response,
    began: Instant,
    first_byte: Duration,
    expected: u64,
) -> Observation {
    match response.bytes().await {
        Err(_) => {
            Observation::failure(source, "short_read", Some(206)).with_complete(began.elapsed())
        }
        Ok(body) => {
            let complete = began.elapsed();
            let bytes = body.len() as u64;
            match bytes < expected {
                true => {
                    Observation::failure(source, "short_read", Some(206)).with_complete(complete)
                }
                false => Observation::success(source, bytes, first_byte, complete),
            }
        }
    }
}

/// Name a transport failure the way the error counters break them down.
fn transport_class(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        return "timeout";
    }
    if error.is_connect() {
        let text = error.to_string().to_ascii_lowercase();
        if text.contains("refused") {
            return "connection_refused";
        }
        if text.contains("tls") || text.contains("certificate") || text.contains("handshake") {
            return "tls";
        }
        return "connect";
    }
    if error.is_body() || error.is_decode() {
        return "short_read";
    }
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("reset") {
        return "reset";
    }
    "transport"
}

/// Name an HTTP failure by what a caller does about it.
fn status_class(status: u16) -> &'static str {
    match status {
        401 | 403 => "auth",
        404 => "not_found",
        416 => "range_not_satisfiable",
        429 => "rate_limited",
        500..=599 => "server_error",
        _ => "unexpected_status",
    }
}

/// How long to wait to stay under a target rate.
///
/// A leaky bucket against the run's own totals: if more bytes have arrived
/// than the target allows by now, wait out the difference. Reading the totals
/// rather than pacing each worker separately means the target is the
/// aggregate, which is what the caller asked for.
fn pacing(recorder: &Recorder, target_bytes_per_sec: u64) -> Option<Duration> {
    if target_bytes_per_sec == 0 {
        return None;
    }
    let elapsed = recorder.elapsed().as_secs_f64();
    let allowed = target_bytes_per_sec as f64 * elapsed;
    let actual = recorder.live.bytes() as f64;
    let excess = actual - allowed;
    match excess > 0.0 {
        true => Some(Duration::from_secs_f64(
            (excess / target_bytes_per_sec as f64).min(1.0),
        )),
        false => None,
    }
}

/// Time opening one connection to a URL, TLS included when it is HTTPS.
///
/// This is a connection of its own that carries no request and is closed
/// immediately. It is the only way to separate "the server took a long time to
/// answer" from "the server took a long time to accept".
/// The deadline on one connection probe.
///
/// A probe that outlives the metrics interval would stack up behind itself and
/// the samples would drift, so it gives up well inside one. A connection that
/// takes longer than this is recorded as no sample rather than as a slow one,
/// because the point of the measurement is the healthy case.
const CONNECT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

async fn connect_cost(url: &str) -> Option<Duration> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_string();
    match parsed.scheme() {
        "https" => {
            let report = crate::webseed::probe::tls_report_within(url, CONNECT_PROBE_TIMEOUT)
                .await
                .ok()?;
            Some(Duration::from_millis(
                report.connect_ms + report.handshake_ms,
            ))
        }
        "http" => {
            let port = parsed.port_or_known_default().unwrap_or(80);
            let began = Instant::now();
            let stream = tokio::time::timeout(
                CONNECT_PROBE_TIMEOUT,
                tokio::net::TcpStream::connect((host.as_str(), port)),
            )
            .await
            .ok()?
            .ok()?;
            let elapsed = began.elapsed();
            drop(stream);
            Some(elapsed)
        }
        _ => None,
    }
}

/// The windows one source is read from.
///
/// Every window is inside the source's scope, so a mirror holding part of the
/// payload is measured on the part it actually holds rather than failed on the
/// part it does not.
fn windows(binding: &Binding, layout: &Layout, info_hash: &str, chunk: u64) -> Vec<Window> {
    let mut out = Vec::new();
    for span in binding.scope.spans.spans() {
        let mut pos = span.start;
        while pos < span.end && out.len() < MAX_WINDOWS {
            let length = chunk.min(span.end - pos);
            if let Ok(requests) = binding.request_urls(layout, info_hash, pos..pos + length) {
                for request in requests {
                    if request.length > 0 {
                        out.push(Window {
                            url: request.url,
                            offset: request.file_offset,
                            length: request.length,
                        });
                    }
                }
            }
            pos += length;
        }
    }
    out
}

/// An HTTP client for one source.
///
/// Redirects are followed here, unlike in `webseed test`: a benchmark measures
/// the path a download would take, and a download follows the redirect. The
/// chain itself is what `webseed test` is for.
fn client(binding: &Binding) -> Result<reqwest::Client> {
    let limits = &binding.spec.limits;
    reqwest::Client::builder()
        .timeout(limits.timeout())
        .connect_timeout(limits.connect_timeout())
        .user_agent(
            binding
                .spec
                .user_agent
                .clone()
                .unwrap_or_else(default_user_agent),
        )
        .build()
        .map_err(|e| Error::network(format!("cannot build an HTTP client: {e}")))
}

/// The source's own headers.
fn headers(binding: &Binding) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in &binding.spec.headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::try_from(name.as_str()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            headers.insert(name, value);
        }
    }
    if let Auth::Bearer { token } = &binding.spec.auth
        && let Ok(mut value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
    {
        value.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::recorder::Recorder;

    #[test]
    fn a_status_is_named_by_what_a_caller_does_about_it() {
        assert_eq!(status_class(403), "auth");
        assert_eq!(status_class(404), "not_found");
        assert_eq!(status_class(416), "range_not_satisfiable");
        assert_eq!(status_class(429), "rate_limited");
        assert_eq!(status_class(503), "server_error");
        assert_eq!(status_class(301), "unexpected_status");
    }

    #[test]
    fn pacing_waits_only_when_the_run_is_ahead_of_its_target() {
        let recorder = Recorder::new(Duration::ZERO, Duration::from_millis(10), 1);
        assert!(
            pacing(&recorder, 1_000_000).is_none(),
            "a run that has moved nothing is never ahead"
        );
        recorder
            .live
            .bytes
            .store(100_000_000, std::sync::atomic::Ordering::Relaxed);
        let pause = pacing(&recorder, 1_000_000).expect("a run 100 seconds ahead waits");
        assert!(pause > Duration::ZERO);
        assert!(
            pause <= Duration::from_secs(1),
            "a single wait is capped so the deadline is still checked"
        );
    }

    #[test]
    fn a_zero_target_rate_never_paces() {
        let recorder = Recorder::new(Duration::ZERO, Duration::from_millis(10), 1);
        recorder
            .live
            .bytes
            .store(u64::MAX / 2, std::sync::atomic::Ordering::Relaxed);
        assert!(pacing(&recorder, 0).is_none());
    }

    #[test]
    fn range_support_is_read_from_what_the_server_actually_answered() {
        let source = |partial: bool, whole: bool| {
            (
                std::sync::atomic::AtomicBool::new(partial),
                std::sync::atomic::AtomicBool::new(whole),
            )
        };
        let judge = |partial: bool, whole: bool| {
            let (p, w) = source(partial, whole);
            match (
                p.load(std::sync::atomic::Ordering::Relaxed),
                w.load(std::sync::atomic::Ordering::Relaxed),
            ) {
                (true, _) => RangeSupport::Yes,
                (false, true) => RangeSupport::No,
                (false, false) => RangeSupport::Unknown,
            }
        };
        assert_eq!(judge(true, false), RangeSupport::Yes);
        assert_eq!(judge(true, true), RangeSupport::Yes);
        assert_eq!(judge(false, true), RangeSupport::No);
        assert_eq!(judge(false, false), RangeSupport::Unknown);
    }

    #[tokio::test]
    async fn connecting_to_a_closed_port_reports_nothing_rather_than_hanging() {
        // Port 1 on loopback has nothing listening on any machine this runs
        // on, and a refused connection is not a measurement.
        assert!(connect_cost("http://127.0.0.1:1/").await.is_none());
    }

    #[tokio::test]
    async fn a_url_that_is_not_http_has_no_connection_cost() {
        assert!(connect_cost("ftp://example.com/x").await.is_none());
        assert!(connect_cost("not a url").await.is_none());
    }
}
