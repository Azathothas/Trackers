//! Collecting a measurement while it happens.
//!
//! A run records one observation per request and one sample per
//! `--metrics-interval`. Latency goes into a histogram rather than a vector,
//! so a six hour run costs the same memory as a six second one and the
//! percentiles are exact to three significant figures either way.
//!
//! The warmup window is recorded rather than dropped. A sample taken during
//! warmup is marked `warmup: true` and left out of the summary, because "it
//! was slow for the first three seconds" is itself a result and deleting it
//! makes the report lie by omission.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hdrhistogram::Histogram;

use crate::bench::report::{
    ConcurrencyStep, Costs, Disk, Errors, Latencies, Percentiles, Pipeline, Sample, Stalls, Summary,
};
use crate::sysinfo::Process;
use crate::time::Timestamp;
use crate::units::{Millis, Rate, Size};

/// The widest latency a histogram tracks, in milliseconds.
///
/// An hour is far past any single request worth waiting for, and a value above
/// the bound is clamped into the top bucket rather than lost.
const MAX_LATENCY_MS: u64 = 3_600_000;

/// Significant figures the histograms keep. Three means a 12,345 ms sample
/// reads back within 12,300 to 12,400 ms, which is finer than any decision
/// made from it.
const PRECISION: u8 = 3;

fn histogram() -> Histogram<u64> {
    Histogram::new_with_bounds(1, MAX_LATENCY_MS, PRECISION)
        .unwrap_or_else(|_| Histogram::new(PRECISION).expect("a default histogram always builds"))
}

/// Latency histograms for one window.
#[derive(Debug)]
struct Histograms {
    connect: Histogram<u64>,
    first_byte: Histogram<u64>,
    complete: Histogram<u64>,
}

impl Default for Histograms {
    fn default() -> Self {
        Self {
            connect: histogram(),
            first_byte: histogram(),
            complete: histogram(),
        }
    }
}

impl Histograms {
    fn snapshot(&self) -> Latencies {
        Latencies {
            connect: percentiles(&self.connect),
            first_byte: percentiles(&self.first_byte),
            complete: percentiles(&self.complete),
        }
    }
}

/// Read the percentiles A3.11 requires out of a histogram.
pub fn percentiles(histogram: &Histogram<u64>) -> Percentiles {
    if histogram.is_empty() {
        return Percentiles::default();
    }
    Percentiles {
        count: histogram.len(),
        p50_ms: histogram.value_at_quantile(0.50),
        p90_ms: histogram.value_at_quantile(0.90),
        p99_ms: histogram.value_at_quantile(0.99),
        p999_ms: histogram.value_at_quantile(0.999),
        max_ms: histogram.max(),
        mean_ms: histogram.mean().round() as u64,
    }
}

/// What one request did.
///
/// A failed request still carries whatever timing it reached, because "the
/// connection took four seconds and then reset" is more useful than a bare
/// error count.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    pub bytes: u64,
    /// Time to open the connection, when this request opened one. A reused
    /// connection has none, which is itself the measurement.
    pub connect: Option<Duration>,
    pub first_byte: Option<Duration>,
    pub complete: Option<Duration>,
    /// The failure class, from [`crate::webseed::fetch::FetchError::class`].
    pub error_class: Option<String>,
    pub status: Option<u16>,
    /// Which source served this, for the per-source breakdown.
    pub source: usize,
}

impl Observation {
    /// A request that moved bytes.
    pub fn success(source: usize, bytes: u64, first_byte: Duration, complete: Duration) -> Self {
        Self {
            bytes,
            connect: None,
            first_byte: Some(first_byte),
            complete: Some(complete),
            error_class: None,
            status: None,
            source,
        }
    }

    /// A request that failed.
    pub fn failure(source: usize, class: impl Into<String>, status: Option<u16>) -> Self {
        Self {
            error_class: Some(class.into()),
            status,
            source,
            ..Default::default()
        }
    }

    /// Note how long the connection took to open.
    pub fn with_connect(mut self, connect: Duration) -> Self {
        self.connect = Some(connect);
        self
    }

    /// Note how long the whole exchange took, on a request that failed part
    /// way through.
    pub fn with_complete(mut self, complete: Duration) -> Self {
        self.complete = Some(complete);
        self
    }

    fn failed(&self) -> bool {
        self.error_class.is_some()
    }
}

/// Counters for one window, either an interval or the whole run.
#[derive(Debug, Default)]
struct Window {
    bytes: u64,
    requests: u64,
    errors: Errors,
    latency: Histograms,
}

impl Window {
    /// Fold in bytes and requests that carry no timing of their own.
    fn add(&mut self, bytes: u64, requests: u64) {
        self.bytes += bytes;
        self.requests += requests;
    }

    fn record(&mut self, observation: &Observation) {
        self.bytes += observation.bytes;
        self.requests += 1;
        if let Some(class) = &observation.error_class {
            self.errors.record(class, observation.status);
        }
        let ms = |d: Duration| d.as_millis().clamp(1, u128::from(MAX_LATENCY_MS)) as u64;
        if let Some(connect) = observation.connect {
            let _ = self.latency.connect.record(ms(connect));
        }
        if let Some(first_byte) = observation.first_byte {
            let _ = self.latency.first_byte.record(ms(first_byte));
        }
        if let Some(complete) = observation.complete {
            let _ = self.latency.complete.record(ms(complete));
        }
    }
}

/// Per-source counters, kept apart so one slow mirror is visible rather than
/// averaged away.
///
/// Both windows are kept, the measured one and the whole run, so a run that
/// collapses its warmup still has a per-source breakdown to report.
#[derive(Debug, Default)]
struct PerSource {
    window: Window,
    total: Window,
}

/// Live counters a progress reporter can read without stopping the run.
#[derive(Debug, Default)]
pub struct Live {
    pub bytes: AtomicU64,
    pub requests: AtomicU64,
    pub errors: AtomicU64,
    /// Requests outstanding right now.
    pub in_flight: AtomicU64,
}

impl Live {
    /// Bytes moved so far.
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

/// Collects observations and turns them into a time series and a summary.
///
/// One recorder per run. It is `Sync`, so worker tasks record into it directly
/// rather than through a channel, and the sampling thread reads it on the
/// interval.
#[derive(Debug)]
pub struct Recorder {
    started: Instant,
    started_at: Timestamp,
    warmup: Duration,
    interval: Duration,
    state: Mutex<State>,
    /// Counters readable without taking the lock.
    pub live: Live,
}

#[derive(Debug)]
struct State {
    /// Since the last sample.
    current: Window,
    /// The whole run, warmup included.
    total: Window,
    /// The measured window only, warmup excluded.
    measured: Window,
    /// Since the current concurrency step began, warmup excluded.
    step: Window,
    /// When the current concurrency step began.
    step_started: Instant,
    /// One per source index seen, over the measured window.
    sources: Vec<PerSource>,
    /// Cumulative bytes at the last sample.
    cumulative: u64,
    /// When the last sample was taken.
    last_sample: Instant,
    /// Bytes moved during the measured window at the last sample, so a stall
    /// is detected from the series rather than from a separate timer.
    samples: Vec<Sample>,
    /// Concurrency the run is currently driving at.
    concurrency: usize,
    /// Where the measured window began, once warmup ended.
    measured_from: Option<Instant>,
    /// Where the measured window ended, once the run stopped.
    measured_to: Option<Instant>,
    /// Pieces verified so far, when the caller reports them.
    pieces_verified: u64,
    /// Pieces verified as of the last sample.
    pieces_at_last_sample: u64,
    /// Time spent hashing, when the caller reports it.
    hashing: Duration,
    /// Bytes hashed, when the caller reports them.
    hashed_bytes: u64,
    /// The largest peer count seen.
    peak_peers: Option<u32>,
    /// The worst process sample seen.
    process: Process,
    /// Choke traffic, for the peer benchmarks.
    choke_events: u64,
    unchoke_events: u64,
    peak_queue_depth: u64,
    /// Reads and writes over the measured window.
    disk: Disk,
    /// Reads and writes since the last sample.
    disk_interval: Disk,
    /// Verification since the last sample.
    verify_interval: (u64, u64),
    /// The block pipeline over the measured window, when one is being
    /// measured.
    pipeline: Option<Pipeline>,
    /// Mean block service time to report on the next sample.
    service_us: Option<u64>,
}

impl Recorder {
    /// Start recording.
    pub fn new(warmup: Duration, interval: Duration, concurrency: usize) -> Self {
        let started = Instant::now();
        Self {
            started,
            started_at: Timestamp::now(),
            warmup,
            interval: interval.max(Duration::from_millis(1)),
            live: Live::default(),
            state: Mutex::new(State {
                current: Window::default(),
                total: Window::default(),
                measured: Window::default(),
                step: Window::default(),
                step_started: started,
                sources: Vec::new(),
                cumulative: 0,
                last_sample: started,
                samples: Vec::new(),
                concurrency,
                measured_from: warmup.is_zero().then_some(started),
                measured_to: None,
                pieces_verified: 0,
                pieces_at_last_sample: 0,
                hashing: Duration::ZERO,
                hashed_bytes: 0,
                peak_peers: None,
                process: Process::default(),
                choke_events: 0,
                unchoke_events: 0,
                peak_queue_depth: 0,
                disk: Disk::default(),
                disk_interval: Disk::default(),
                verify_interval: (0, 0),
                pipeline: None,
                service_us: None,
            }),
        }
    }

    /// How long the run has been going.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Whether the warmup window is still open.
    pub fn in_warmup(&self) -> bool {
        self.started.elapsed() < self.warmup
    }

    /// How much of the warmup window is left, or zero once it has closed.
    ///
    /// A caller that has to keep a load going until the run is warm asks this
    /// rather than sleeping for the whole warmup, because it may already be
    /// part way through one. See `TODO/bench.md`, T-229.
    pub fn remaining_warmup(&self) -> Duration {
        self.warmup.saturating_sub(self.started.elapsed())
    }

    /// The interval between samples.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Record one request.
    pub fn observe(&self, observation: Observation) {
        self.live
            .bytes
            .fetch_add(observation.bytes, Ordering::Relaxed);
        self.live.requests.fetch_add(1, Ordering::Relaxed);
        if observation.failed() {
            self.live.errors.fetch_add(1, Ordering::Relaxed);
        }
        let in_warmup = self.in_warmup();
        let mut state = self.lock();
        state.current.record(&observation);
        state.total.record(&observation);
        let index = observation.source;
        if state.sources.len() <= index {
            state.sources.resize_with(index + 1, PerSource::default);
        }
        state.sources[index].total.record(&observation);
        if !in_warmup {
            state.measured.record(&observation);
            state.step.record(&observation);
            state.sources[index].window.record(&observation);
        }
    }

    /// Note the concurrency the run is driving at, for the next sample.
    pub fn set_concurrency(&self, concurrency: usize) {
        self.lock().concurrency = concurrency;
    }

    /// Open a concurrency step, discarding whatever the last one recorded.
    ///
    /// Call before each step of a `--concurrency-sweep`. Everything observed
    /// until [`Self::end_step`] belongs to this step as well as to the run.
    pub fn begin_step(&self, concurrency: usize) {
        let mut state = self.lock();
        state.concurrency = concurrency;
        state.step = Window::default();
        state.step_started = Instant::now();
    }

    /// Close a concurrency step and read what it did.
    ///
    /// The latency here is the step's own, which is the point of a sweep: the
    /// knee shows up as p99 climbing while throughput stops climbing.
    pub fn end_step(&self, concurrency: usize) -> ConcurrencyStep {
        let mut state = self.lock();
        let window = std::mem::take(&mut state.step);
        let elapsed_ms = state
            .step_started
            .elapsed()
            .as_millis()
            .max(1)
            .min(u128::from(u64::MAX)) as u64;
        ConcurrencyStep {
            concurrency,
            bytes: Size(window.bytes),
            elapsed: Millis(elapsed_ms),
            rate: Rate(window.bytes.saturating_mul(1000) / elapsed_ms),
            requests: window.requests,
            errors: window.errors.total,
            latency: window.latency.snapshot(),
        }
    }

    /// Note bytes that arrived without a request of their own to time.
    ///
    /// A download counts what a peer or a source delivered over an interval
    /// rather than one request at a time, because the session does the
    /// requesting and does not hand out per-request timings. The bytes and the
    /// request count are exact; there is no latency to record, so none is
    /// invented.
    pub fn observe_bulk(&self, source: usize, bytes: u64, requests: u64) {
        if bytes == 0 && requests == 0 {
            return;
        }
        self.live.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.live.requests.fetch_add(requests, Ordering::Relaxed);
        let in_warmup = self.in_warmup();
        let mut state = self.lock();
        state.current.add(bytes, requests);
        state.total.add(bytes, requests);
        if state.sources.len() <= source {
            state.sources.resize_with(source + 1, PerSource::default);
        }
        state.sources[source].total.add(bytes, requests);
        if !in_warmup {
            state.measured.add(bytes, requests);
            state.step.add(bytes, requests);
            state.sources[source].window.add(bytes, requests);
        }
    }

    /// Note pieces verified and what it cost.
    pub fn observe_hashing(&self, pieces: u64, bytes: u64, elapsed: Duration) {
        let mut state = self.lock();
        state.pieces_verified += pieces;
        state.hashed_bytes += bytes;
        state.hashing += elapsed;
        state.verify_interval.0 += elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        state.verify_interval.1 += bytes;
    }

    /// Note what storage did over one interval.
    pub fn observe_disk(&self, disk: &Disk) {
        let mut state = self.lock();
        state.disk.read_ops += disk.read_ops;
        state.disk.read_bytes = Size(state.disk.read_bytes.0 + disk.read_bytes.0);
        state.disk.read_time = Millis(state.disk.read_time.0 + disk.read_time.0);
        state.disk.write_ops += disk.write_ops;
        state.disk.write_calls += disk.write_calls;
        state.disk.write_bytes = Size(state.disk.write_bytes.0 + disk.write_bytes.0);
        state.disk.write_time = Millis(state.disk.write_time.0 + disk.write_time.0);
        state.disk_interval.read_ops += disk.read_ops;
        state.disk_interval.read_bytes = Size(state.disk_interval.read_bytes.0 + disk.read_bytes.0);
        state.disk_interval.read_time = Millis(state.disk_interval.read_time.0 + disk.read_time.0);
        state.disk_interval.write_ops += disk.write_ops;
        state.disk_interval.write_calls += disk.write_calls;
        state.disk_interval.write_bytes =
            Size(state.disk_interval.write_bytes.0 + disk.write_bytes.0);
        state.disk_interval.write_time =
            Millis(state.disk_interval.write_time.0 + disk.write_time.0);
    }

    /// Note where the block request pipeline is.
    ///
    /// `in_flight` is a level rather than a count, so it is sampled into the
    /// series and averaged over the measured window. The totals replace rather
    /// than accumulate, because the caller reads them from counters that only
    /// ever grow.
    pub fn observe_pipeline(&self, pipeline: Pipeline) {
        let mut state = self.lock();
        state.peak_queue_depth = state.peak_queue_depth.max(pipeline.peak_in_flight);
        state.service_us = (pipeline.mean_service_us > 0).then_some(pipeline.mean_service_us);
        state.pipeline = Some(pipeline);
    }

    /// Note the peer count, for the peak.
    pub fn observe_peers(&self, peers: u32) {
        let mut state = self.lock();
        state.peak_peers = Some(state.peak_peers.map_or(peers, |seen| seen.max(peers)));
    }

    /// Note choke traffic and the request queue depth.
    pub fn observe_choke(&self, choke_events: u64, unchoke_events: u64, queue_depth: u64) {
        let mut state = self.lock();
        state.choke_events += choke_events;
        state.unchoke_events += unchoke_events;
        state.peak_queue_depth = state.peak_queue_depth.max(queue_depth);
    }

    /// Take one sample of the time series.
    ///
    /// Called on the metrics interval. Returns the sample so a caller can emit
    /// it as an event as well as keep it.
    pub fn sample(&self) -> Sample {
        let now = Instant::now();
        let elapsed = now.duration_since(self.started);
        let warmup = elapsed < self.warmup;
        let process = Process::sample();
        let mut state = self.lock();

        // The first sample after warmup opens the measured window. Doing it
        // here rather than on a timer means the window starts at a sample
        // boundary, so the summary's duration and the series agree.
        if !warmup && state.measured_from.is_none() {
            state.measured_from = Some(now);
        }

        let window = std::mem::take(&mut state.current);
        let interval_ms = now
            .duration_since(state.last_sample)
            .as_millis()
            .max(1)
            .min(u128::from(u64::MAX)) as u64;
        state.last_sample = now;
        state.cumulative += window.bytes;
        state.process = state.process.max(&process);

        let pieces = state
            .pieces_verified
            .saturating_sub(state.pieces_at_last_sample);
        state.pieces_at_last_sample = state.pieces_verified;

        let depth = self.live.in_flight.load(Ordering::Relaxed);
        let disk = std::mem::take(&mut state.disk_interval);
        let (verify_ms, verify_bytes) = std::mem::take(&mut state.verify_interval);
        let costs = Costs {
            verify: Millis(verify_ms),
            verify_bytes: Size(verify_bytes),
            disk_read: disk.read_time,
            disk_read_bytes: disk.read_bytes,
            disk_write: disk.write_time,
            disk_write_bytes: disk.write_bytes,
            mean_service_us: state.service_us,
        };
        let costs = (costs != Costs::default()).then_some(costs);

        let sample = Sample {
            at: Timestamp::now(),
            elapsed: Millis(elapsed.as_millis().min(u128::from(u64::MAX)) as u64),
            warmup,
            concurrency: state.concurrency,
            bytes: Size(window.bytes),
            cumulative_bytes: Size(state.cumulative),
            rate: Rate(window.bytes.saturating_mul(1000) / interval_ms),
            requests: window.requests,
            errors: window.errors.total,
            latency: window.latency.snapshot(),
            process,
            peers: state.peak_peers,
            pieces_verified: (pieces > 0).then_some(pieces),
            queue_depth: Some(depth),
            costs,
        };
        state.samples.push(sample.clone());
        sample
    }

    /// Whether the measured window ever opened.
    pub fn measured_anything(&self) -> bool {
        self.lock().measured_from.is_some()
    }

    /// Measure the whole run, warmup included.
    ///
    /// For a run that ended before the warmup window closed, which is what a
    /// download faster than its own warmup does. Reporting zero bytes because
    /// the transfer beat the clock is worse than reporting the transfer, so
    /// the caller collapses the window and says so in a note.
    pub fn collapse_warmup(&self) {
        let mut state = self.lock();
        state.measured_from = Some(self.started);
        state.measured = std::mem::take(&mut state.total);
        for source in &mut state.sources {
            source.window = std::mem::take(&mut source.total);
        }
        for sample in &mut state.samples {
            sample.warmup = false;
        }
    }

    /// Close the measured window. Call once, when the run stops.
    pub fn stop(&self) {
        let mut state = self.lock();
        if state.measured_to.is_none() {
            state.measured_to = Some(Instant::now());
        }
    }

    /// The samples taken so far.
    pub fn series(&self) -> Vec<Sample> {
        self.lock().samples.clone()
    }

    /// The worst process cost seen.
    pub fn process(&self) -> Process {
        self.lock().process.clone()
    }

    /// Everything the run adds up to, over the measured window.
    pub fn summary(&self) -> Summary {
        let state = self.lock();
        let measured_from = state.measured_from.unwrap_or(self.started);
        let measured_to = state.measured_to.unwrap_or_else(Instant::now);
        let duration_ms = measured_to
            .saturating_duration_since(measured_from)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let bytes = state.measured.bytes;
        let sustained = match duration_ms {
            0 => 0,
            ms => bytes.saturating_mul(1000) / ms,
        };
        let peak = state
            .samples
            .iter()
            .filter(|s| !s.warmup)
            .map(|s| s.rate.0)
            .max()
            .unwrap_or(0);

        Summary {
            bytes: Size(bytes),
            duration: Millis(duration_ms),
            sustained_rate: Rate(sustained),
            peak_rate: Rate(peak),
            ceiling_share: None,
            requests: state.measured.requests,
            errors: state.measured.errors.clone(),
            latency: state.measured.latency.snapshot(),
            hashing: (state.pieces_verified > 0).then(|| crate::bench::report::Hashing {
                pieces: state.pieces_verified,
                bytes: Size(state.hashed_bytes),
                total: Millis(state.hashing.as_millis().min(u128::from(u64::MAX)) as u64),
                rate: Rate(match state.hashing.as_millis() {
                    0 => 0,
                    ms => state.hashed_bytes.saturating_mul(1000) / ms as u64,
                }),
            }),
            stalls: Some(stalls(&state.samples, self.interval)),
            choke: (state.choke_events > 0
                || state.unchoke_events > 0
                || state.peak_queue_depth > 0)
                .then(|| crate::bench::report::ChokeStats {
                    choke_events: state.choke_events,
                    unchoke_events: state.unchoke_events,
                    peak_queue_depth: state.peak_queue_depth,
                }),
            disk: (!state.disk.is_empty()).then(|| state.disk.clone()),
            pipeline: state
                .pipeline
                .clone()
                .map(|pipeline| pipeline.derive(Duration::from_millis(duration_ms))),
            peak_peers: state.peak_peers,
            best_concurrency: None,
        }
    }

    /// The per-source breakdown over the measured window.
    ///
    /// `label` names each source index; an index with no label is reported by
    /// number, because losing the row entirely would hide bytes.
    pub fn sources(
        &self,
        labels: &[(usize, String, String)],
    ) -> Vec<crate::bench::report::SourceSummary> {
        let state = self.lock();
        let measured_from = state.measured_from.unwrap_or(self.started);
        let measured_to = state.measured_to.unwrap_or_else(Instant::now);
        let duration_ms = measured_to
            .saturating_duration_since(measured_from)
            .as_millis()
            .max(1)
            .min(u128::from(u64::MAX)) as u64;

        let mut out = Vec::new();
        for (index, source) in state.sources.iter().enumerate() {
            if source.window.requests == 0 && source.window.bytes == 0 {
                continue;
            }
            let (label, kind) = labels
                .iter()
                .find(|(i, _, _)| *i == index)
                .map(|(_, label, kind)| (label.clone(), kind.clone()))
                .unwrap_or_else(|| (format!("source {index}"), "unknown".to_string()));
            let errors = source.window.errors.clone();
            out.push(crate::bench::report::SourceSummary {
                index,
                label,
                kind,
                bytes: Size(source.window.bytes),
                rate: Rate(source.window.bytes.saturating_mul(1000) / duration_ms),
                requests: source.window.requests,
                errors: errors.total,
                connections: None,
                http_bytes: None,
                latency: source.window.latency.snapshot(),
                error_detail: (errors.total > 0).then_some(errors),
                failure: None,
            });
        }
        out
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// When the run started, as a timestamp.
    pub fn started_at(&self) -> Timestamp {
        self.started_at
    }
}

/// Stalls, read out of the series.
///
/// An interval that moved no bytes is a stall. Counting them from the series
/// rather than from a separate timer means the number in the summary and the
/// zeroes in the series always agree.
fn stalls(samples: &[Sample], interval: Duration) -> Stalls {
    let interval_ms = interval.as_millis().max(1).min(u128::from(u64::MAX)) as u64;
    let mut out = Stalls::default();
    let mut run = 0u64;
    for sample in samples.iter().filter(|s| !s.warmup) {
        if sample.bytes.0 == 0 {
            run += interval_ms;
            out.total = Millis(out.total.0 + interval_ms);
            if run == interval_ms {
                out.count += 1;
            }
            out.longest = Millis(out.longest.0.max(run));
        } else {
            run = 0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder() -> Recorder {
        Recorder::new(Duration::ZERO, Duration::from_millis(10), 4)
    }

    #[test]
    fn a_successful_request_moves_bytes_and_records_three_latencies() {
        let recorder = recorder();
        recorder.observe(
            Observation::success(
                0,
                1024,
                Duration::from_millis(12),
                Duration::from_millis(40),
            )
            .with_connect(Duration::from_millis(5)),
        );
        recorder.stop();
        let summary = recorder.summary();
        assert_eq!(summary.bytes, Size(1024));
        assert_eq!(summary.requests, 1);
        assert_eq!(summary.errors.total, 0);
        assert_eq!(summary.latency.connect.count, 1);
        assert_eq!(summary.latency.connect.p50_ms, 5);
        assert_eq!(summary.latency.first_byte.p50_ms, 12);
        assert_eq!(summary.latency.complete.p50_ms, 40);
    }

    #[test]
    fn a_failed_request_is_counted_by_class_and_still_carries_its_timing() {
        let recorder = recorder();
        recorder.observe(
            Observation::failure(0, "timeout", None).with_complete(Duration::from_millis(500)),
        );
        recorder.observe(Observation::failure(0, "not_found", Some(404)));
        recorder.stop();
        let summary = recorder.summary();
        assert_eq!(summary.requests, 2);
        assert_eq!(summary.errors.total, 2);
        assert_eq!(summary.errors.by_class["timeout"], 1);
        assert_eq!(summary.errors.by_status["404"], 1);
        assert_eq!(
            summary.latency.complete.count, 1,
            "the timing of a failure is still a measurement"
        );
    }

    #[test]
    fn warmup_observations_are_excluded_from_the_summary() {
        let recorder = Recorder::new(Duration::from_millis(200), Duration::from_millis(10), 1);
        recorder.observe(Observation::success(
            0,
            5000,
            Duration::from_millis(1),
            Duration::from_millis(1),
        ));
        assert!(recorder.in_warmup());
        assert_eq!(
            recorder.summary().bytes,
            Size(0),
            "a warmup byte is not a measured byte"
        );
        assert_eq!(
            recorder.live.bytes(),
            5000,
            "the live counter still sees everything"
        );
    }

    #[test]
    fn percentiles_come_out_in_order() {
        let recorder = recorder();
        for ms in 1..=1000u64 {
            recorder.observe(Observation::success(
                0,
                1,
                Duration::from_millis(ms),
                Duration::from_millis(ms),
            ));
        }
        recorder.stop();
        let p = recorder.summary().latency.complete;
        assert_eq!(p.count, 1000);
        assert!(p.p50_ms <= p.p90_ms);
        assert!(p.p90_ms <= p.p99_ms);
        assert!(p.p99_ms <= p.p999_ms);
        assert!(p.p999_ms <= p.max_ms);
        // Three significant figures over this range means p50 lands within a
        // few milliseconds of 500.
        assert!((495..=505).contains(&p.p50_ms), "p50 was {}", p.p50_ms);
        assert!(p.max_ms >= 1000);
    }

    #[test]
    fn an_empty_run_reports_zeroes_rather_than_failing() {
        let recorder = recorder();
        recorder.stop();
        let summary = recorder.summary();
        assert_eq!(summary.bytes, Size(0));
        assert_eq!(summary.sustained_rate, Rate(0));
        assert_eq!(summary.requests, 0);
        assert!(summary.latency.is_empty());
        assert!(summary.hashing.is_none());
    }

    #[test]
    fn a_sample_carries_the_interval_and_the_cumulative_total() {
        let recorder = recorder();
        recorder.observe(Observation::success(
            0,
            100,
            Duration::from_millis(1),
            Duration::from_millis(1),
        ));
        let first = recorder.sample();
        assert_eq!(first.bytes, Size(100));
        assert_eq!(first.cumulative_bytes, Size(100));
        assert!(!first.warmup);

        recorder.observe(Observation::success(
            0,
            50,
            Duration::from_millis(1),
            Duration::from_millis(1),
        ));
        let second = recorder.sample();
        assert_eq!(second.bytes, Size(50), "an interval is not cumulative");
        assert_eq!(second.cumulative_bytes, Size(150));
        assert_eq!(recorder.series().len(), 2);
    }

    #[test]
    fn a_sample_taken_during_warmup_says_so() {
        let recorder = Recorder::new(Duration::from_secs(60), Duration::from_millis(10), 1);
        assert!(recorder.sample().warmup);
    }

    #[test]
    fn the_peak_rate_comes_from_the_best_measured_interval() {
        let recorder = recorder();
        for bytes in [100u64, 900, 200] {
            recorder.observe(Observation::success(
                0,
                bytes,
                Duration::from_millis(1),
                Duration::from_millis(1),
            ));
            recorder.sample();
        }
        recorder.stop();
        let summary = recorder.summary();
        assert!(summary.peak_rate.0 > 0);
        let best = recorder.series().iter().map(|s| s.rate.0).max().unwrap();
        assert_eq!(summary.peak_rate.0, best);
    }

    #[test]
    fn sources_are_reported_separately_so_one_slow_mirror_is_visible() {
        let recorder = recorder();
        recorder.observe(Observation::success(
            0,
            1000,
            Duration::from_millis(5),
            Duration::from_millis(10),
        ));
        recorder.observe(Observation::success(
            1,
            10,
            Duration::from_millis(500),
            Duration::from_millis(900),
        ));
        recorder.observe(Observation::failure(1, "timeout", None));
        recorder.stop();

        let sources = recorder.sources(&[
            (0, "https://fast.example/".into(), "web_seed".into()),
            (1, "https://slow.example/".into(), "web_seed".into()),
        ]);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].bytes, Size(1000));
        assert_eq!(sources[0].label, "https://fast.example/");
        assert_eq!(sources[1].bytes, Size(10));
        assert_eq!(sources[1].errors, 1);
        assert_eq!(sources[1].latency.first_byte.p50_ms, 500);
        assert_eq!(sources[1].error_detail.as_ref().unwrap().total, 1);
        assert!(sources[0].error_detail.is_none());
    }

    #[test]
    fn a_source_with_no_label_is_still_reported() {
        let recorder = recorder();
        recorder.observe(Observation::success(
            3,
            1,
            Duration::from_millis(1),
            Duration::from_millis(1),
        ));
        recorder.stop();
        let sources = recorder.sources(&[]);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].index, 3);
        assert_eq!(sources[0].label, "source 3");
    }

    #[test]
    fn hashing_is_reported_only_when_something_was_hashed() {
        let quiet = recorder();
        quiet.stop();
        assert!(quiet.summary().hashing.is_none());

        let recorder = recorder();
        recorder.observe_hashing(4, 4 * 1024 * 1024, Duration::from_millis(20));
        recorder.stop();
        let hashing = recorder.summary().hashing.unwrap();
        assert_eq!(hashing.pieces, 4);
        assert_eq!(hashing.bytes, Size(4 * 1024 * 1024));
        assert_eq!(hashing.total, Millis(20));
        assert_eq!(hashing.rate.0, 4 * 1024 * 1024 * 1000 / 20);
    }

    #[test]
    fn peers_and_choke_traffic_reach_the_summary() {
        let recorder = recorder();
        recorder.observe_peers(4);
        recorder.observe_peers(11);
        recorder.observe_peers(2);
        recorder.observe_choke(3, 5, 64);
        recorder.observe_choke(1, 0, 32);
        recorder.stop();
        let summary = recorder.summary();
        assert_eq!(summary.peak_peers, Some(11));
        let choke = summary.choke.unwrap();
        assert_eq!(choke.choke_events, 4);
        assert_eq!(choke.unchoke_events, 5);
        assert_eq!(choke.peak_queue_depth, 64);
    }

    #[test]
    fn an_interval_that_moved_nothing_is_a_stall() {
        let interval = Duration::from_millis(100);
        let samples = vec![
            Sample {
                bytes: Size(10),
                ..Default::default()
            },
            Sample {
                bytes: Size(0),
                ..Default::default()
            },
            Sample {
                bytes: Size(0),
                ..Default::default()
            },
            Sample {
                bytes: Size(5),
                ..Default::default()
            },
            Sample {
                bytes: Size(0),
                ..Default::default()
            },
        ];
        let stalls = stalls(&samples, interval);
        assert_eq!(stalls.count, 2, "two runs of zero, not three zero samples");
        assert_eq!(stalls.total, Millis(300));
        assert_eq!(stalls.longest, Millis(200));
    }

    #[test]
    fn a_warmup_stall_is_not_counted() {
        let samples = vec![
            Sample {
                bytes: Size(0),
                warmup: true,
                ..Default::default()
            },
            Sample {
                bytes: Size(1),
                ..Default::default()
            },
        ];
        assert_eq!(stalls(&samples, Duration::from_millis(100)).count, 0);
    }

    #[test]
    fn a_sweep_step_carries_its_own_bytes_requests_and_latency() {
        let recorder = recorder();
        recorder.begin_step(4);
        recorder.observe(Observation::success(
            0,
            2048,
            Duration::from_millis(2),
            Duration::from_millis(4),
        ));
        let first = recorder.end_step(4);
        assert_eq!(first.concurrency, 4);
        assert_eq!(first.bytes, Size(2048));
        assert_eq!(first.requests, 1);
        assert_eq!(first.latency.complete.p50_ms, 4);

        recorder.begin_step(16);
        recorder.observe(Observation::success(
            0,
            8192,
            Duration::from_millis(20),
            Duration::from_millis(40),
        ));
        recorder.observe(Observation::failure(0, "timeout", None));
        let second = recorder.end_step(16);
        assert_eq!(
            second.bytes,
            Size(8192),
            "a step carries its own bytes, not the run's"
        );
        assert_eq!(second.requests, 2);
        assert_eq!(second.errors, 1);
        assert_eq!(second.latency.complete.p50_ms, 40);

        recorder.stop();
        assert_eq!(
            recorder.summary().bytes,
            Size(2048 + 8192),
            "the run still totals every step"
        );
    }

    #[test]
    fn a_step_that_ran_during_warmup_records_nothing() {
        let recorder = Recorder::new(Duration::from_secs(60), Duration::from_millis(10), 1);
        recorder.begin_step(8);
        recorder.observe(Observation::success(
            0,
            4096,
            Duration::from_millis(1),
            Duration::from_millis(1),
        ));
        let step = recorder.end_step(8);
        assert_eq!(step.bytes, Size(0));
        assert_eq!(step.requests, 0);
    }
}
