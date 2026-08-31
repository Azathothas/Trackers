//! The shape of a `bench` report.
//!
//! One document describes any measurement `bit-cli` takes: what was measured,
//! on what machine, with what arguments, what happened second by second, and
//! what it adds up to. Every subcommand fills the same envelope, so a caller
//! parses one shape and `--baseline` compares any run against any other run of
//! the same kind.
//!
//! Every number is here twice at most, never once: a raw integer of bytes or
//! milliseconds, and a rendered string beside it. Nothing is only rendered.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::sysinfo::{Host, Process};
use crate::time::Timestamp;
use crate::units::{Millis, Rate, Size, format_rate, format_share, format_size};

/// The version of the report contract.
///
/// It changes when a field is removed or its meaning changes. Adding a field
/// is not a breaking change and does not bump it. `--baseline` refuses a
/// report from a future version because it cannot know what moved.
pub const REPORT_VERSION: u32 = 1;

/// Which measurement produced a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Download from a target and measure.
    Leech,
    /// Seed and measure what the swarm pulls.
    Seed,
    /// Measure HTTP sources.
    Webseed,
    /// Synthetic peer load against a target.
    Swarm,
    /// One-shot reachability and capability probe.
    Probe,
    /// Measure the payload file under several writers, with no session.
    Disk,
}

impl Kind {
    /// The stable name used in output and on the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Leech => "leech",
            Self::Seed => "seed",
            Self::Webseed => "webseed",
            Self::Swarm => "swarm",
            Self::Probe => "probe",
            Self::Disk => "disk",
        }
    }
}

/// What binary took the measurement.
///
/// `debug_assertions` is here because it is the difference between a number
/// and a number that means nothing: a debug build of this tool runs several
/// times slower and nothing else in the report would say so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub version: String,
    pub target: String,
    pub profile: String,
    pub debug_assertions: bool,
}

/// Everything about the run that is not a measurement.
///
/// A benchmark without this is not a result. Comparing two numbers taken on
/// different machines, or before and after a kernel update, without knowing
/// that is how a benchmark lies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub build: Build,
    pub host: Host,
    /// The exact command line, argument by argument, program name first.
    pub command_line: Vec<String>,
    pub working_directory: String,
    pub started_at: Timestamp,
    pub finished_at: Timestamp,
    pub elapsed: Millis,
    /// What the process cost: peak RSS, CPU time, and open handles.
    pub process: Process,
    /// Whether any subsystem trace was on. Tracing costs throughput, so a
    /// report taken with it on is not comparable to one taken without.
    pub tracing_enabled: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub trace_subsystems: Vec<String>,
}

impl Environment {
    /// Open an environment at the start of a run.
    ///
    /// The end fields are filled by [`Self::finish`]. Until then they hold the
    /// start time and a zero cost, so a report written after a panic is still
    /// a valid document.
    pub fn begin(
        build: Build,
        command_line: Vec<String>,
        working_directory: String,
        trace_subsystems: Vec<String>,
    ) -> Self {
        let started_at = Timestamp::now();
        Self {
            build,
            host: Host::capture(),
            command_line,
            working_directory,
            started_at,
            finished_at: started_at,
            elapsed: Millis(0),
            process: Process::sample(),
            tracing_enabled: !trace_subsystems.is_empty(),
            trace_subsystems,
        }
    }

    /// Close the environment: stamp the end time and take the final cost
    /// sample.
    ///
    /// The process figures are merged with whatever was already recorded, so a
    /// peak reached halfway through a run is not lost when memory is released
    /// before the end.
    pub fn finish(&mut self) {
        self.finished_at = Timestamp::now();
        self.elapsed = Millis(
            self.finished_at
                .epoch_ms()
                .saturating_sub(self.started_at.epoch_ms())
                .max(0) as u64,
        );
        self.process = self.process.max(&Process::sample());
    }

    /// Merge a cost sample taken during the run.
    pub fn observe(&mut self, process: &Process) {
        self.process = self.process.max(process);
    }
}

/// What the run was asked to do.
///
/// Only the fields a given subcommand uses are populated; the rest are absent
/// from the JSON rather than present and zero, so a reader cannot mistake "not
/// applicable" for "none".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameters {
    pub duration: Millis,
    pub warmup: Millis,
    pub metrics_interval: Millis,
    pub concurrency: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub concurrency_sweep: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_rate: Option<Rate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_under: Option<Size>,
    /// A rate the measurement is a percentage of, stated by the caller. For
    /// `bench webseed` this is what `curl` reached against the same URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peers: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub torrents: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_size: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub piece_size: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_budget: Option<Size>,
}

/// What was measured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    /// The source as the caller wrote it.
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub piece_length: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub piece_count: Option<u32>,
    /// The URLs or addresses bytes were read from.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub endpoints: Vec<String>,
}

/// Latency percentiles for one measurement point.
///
/// Percentiles come from a histogram rather than from a sorted vector of every
/// sample, so a long run costs a fixed amount of memory rather than one entry
/// per request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Percentiles {
    pub count: u64,
    pub p50_ms: u64,
    pub p90_ms: u64,
    pub p99_ms: u64,
    pub p999_ms: u64,
    pub max_ms: u64,
    pub mean_ms: u64,
}

impl Percentiles {
    /// Whether anything was recorded.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// One line for a person: `p50 12ms p90 30ms p99 88ms max 120ms`.
    pub fn line(&self) -> String {
        format!(
            "p50 {}ms  p90 {}ms  p99 {}ms  p99.9 {}ms  max {}ms",
            self.p50_ms, self.p90_ms, self.p99_ms, self.p999_ms, self.max_ms
        )
    }
}

/// The three latencies A3.11 requires, kept apart.
///
/// A mirror with a fast connection and a slow first byte is a different
/// problem from one with a slow connection, and averaging them hides which.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Latencies {
    /// Time to open the connection, TLS included.
    pub connect: Percentiles,
    /// Time from sending the request to the first byte of the response.
    pub first_byte: Percentiles,
    /// Time from sending the request to the last byte of the response.
    pub complete: Percentiles,
}

impl Latencies {
    /// Whether anything was recorded.
    pub fn is_empty(&self) -> bool {
        self.connect.is_empty() && self.first_byte.is_empty() && self.complete.is_empty()
    }
}

/// Failures counted by class.
///
/// The classes are the ones a caller acts on differently: a refused connection
/// is a firewall, a 416 is a bad range, a hash mismatch is a corrupt mirror.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Errors {
    pub total: u64,
    /// Counts keyed by [`crate::webseed::fetch::FetchError::class`], plus
    /// `connection_refused`, `timeout`, `tls`, `reset`, and `short_read` for
    /// transport failures that never reached a status.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub by_class: BTreeMap<String, u64>,
    /// Counts keyed by HTTP status, for every response that carried one.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub by_status: BTreeMap<String, u64>,
}

impl Errors {
    /// Record one failure.
    pub fn record(&mut self, class: &str, status: Option<u16>) {
        self.total += 1;
        *self.by_class.entry(class.to_string()).or_default() += 1;
        if let Some(status) = status {
            *self.by_status.entry(status.to_string()).or_default() += 1;
        }
    }

    /// Fold another set in.
    pub fn merge(&mut self, other: &Self) {
        self.total += other.total;
        for (class, count) in &other.by_class {
            *self.by_class.entry(class.clone()).or_default() += count;
        }
        for (status, count) in &other.by_status {
            *self.by_status.entry(status.clone()).or_default() += count;
        }
    }

    /// The classes seen, most frequent first, for a one-line rendering.
    pub fn ranked(&self) -> Vec<(String, u64)> {
        let mut out: Vec<(String, u64)> =
            self.by_class.iter().map(|(k, v)| (k.clone(), *v)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

/// One point of the time series.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub at: Timestamp,
    /// Time since the run started.
    pub elapsed: Millis,
    /// Whether this sample falls inside the warmup window and is therefore
    /// excluded from the summary. Warmup samples are reported rather than
    /// dropped, because "it was slow for the first three seconds" is a result.
    pub warmup: bool,
    pub concurrency: usize,
    /// Bytes moved during this interval.
    pub bytes: Size,
    /// Bytes moved since the run started.
    pub cumulative_bytes: Size,
    /// Bytes per second during this interval.
    pub rate: Rate,
    pub requests: u64,
    pub errors: u64,
    #[serde(skip_serializing_if = "Latencies::is_empty", default)]
    pub latency: Latencies,
    pub process: Process,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peers: Option<u32>,
    /// Pieces verified during this interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pieces_verified: Option<u64>,
    /// Requests outstanding at the end of this interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<u64>,
    /// Where this interval's time went besides the wire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub costs: Option<Costs>,
}

/// What one interval spent off the wire.
///
/// Three numbers against the interval length answer the question a download
/// benchmark exists to answer: was the run waiting on the network, on the
/// hash, or on the disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Costs {
    /// Time reading pieces back and hashing them.
    pub verify: Millis,
    pub verify_bytes: Size,
    pub disk_read: Millis,
    pub disk_read_bytes: Size,
    pub disk_write: Millis,
    pub disk_write_bytes: Size,
    /// Mean time to answer one block request during this interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_service_us: Option<u64>,
}

/// What one writer thread cost, for `bench disk`.
///
/// The per-thread figure is what says whether the threads are waiting on each
/// other: eight threads that each take as long as one thread took on its own
/// are eight threads taking turns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskThread {
    pub index: usize,
    pub blocks: u64,
    pub bytes: Size,
    /// Wall time this thread spent inside `pwrite_all`.
    pub write_time: Millis,
    /// Mean time for one of this thread's writes.
    pub mean_write_us: u64,
}

/// The run length a report written before the field existed was taken at.
fn one_block() -> u64 {
    1
}

/// One step of a `bench disk` sweep.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskStep {
    pub threads: usize,
    /// `shared` or `split`.
    pub layout: String,
    /// Consecutive blocks one thread wrote before the next took over, under
    /// `shared` and `handles`. 1 strides block by block. `split` gives each
    /// thread one contiguous range and does not use this.
    ///
    /// Defaulted to 1 rather than to zero so a report written before this
    /// field existed reads back as the arrangement it was actually taken at,
    /// which is what `--baseline` needs. See `TODO/disk-io.md`, T-018.
    #[serde(default = "one_block")]
    pub run_length: u64,
    pub files: usize,
    pub bytes: Size,
    /// Wall time of the write phase.
    pub elapsed: Millis,
    pub rate: Rate,
    /// Every thread's write time added together. Against `elapsed` it says how
    /// many writes were really in flight.
    pub total_write_time: Millis,
    /// Positioned writes that reached the device, from the storage counters.
    ///
    /// Not the same as [`Self::write_calls`] since the write buffer landed:
    /// a run of blocks a thread wrote in order reaches the device as one
    /// operation. A report written before 2026-08-22 carries the same number
    /// under this name whichever meaning is read into it, because there was
    /// nothing between the two. See `TODO/disk-io.md`, T-018.
    pub write_ops: u64,
    /// Blocks the threads wrote, which is what the step asked storage for.
    ///
    /// Defaulted to zero, which is what a report from before this field reads
    /// back as. Its `write_ops` is the number that belongs here.
    #[serde(default)]
    pub write_calls: u64,
    /// Mean time for one positioned write, across every thread.
    pub mean_write_us: u64,
    /// Writes actually overlapping, as `total_write_time / elapsed`. It is the
    /// thread count when nothing serialises and 1.00 when everything does.
    pub concurrency_achieved: String,
    /// Time to push what the write phase left in the page cache out to the
    /// device, taken after `elapsed` and not counted in it.
    ///
    /// It is here for two reasons. A step that leaves gigabytes outstanding
    /// would otherwise charge them to the next step, and `rate` against this
    /// says whether a step was measuring the cache or the device.
    pub flush: Millis,
    pub threads_detail: Vec<DiskThread>,
    /// Whether the payload read back as what was written. `None` when the
    /// read-back was skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    /// What that read-back cost, when one ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_back: Option<Disk>,
}

/// One step of a concurrency sweep, so the knee is visible.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConcurrencyStep {
    pub concurrency: usize,
    pub bytes: Size,
    pub elapsed: Millis,
    pub rate: Rate,
    pub requests: u64,
    pub errors: u64,
    pub latency: Latencies,
}

/// What one source contributed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSummary {
    pub index: usize,
    /// The URL for an HTTP source, the address for a peer.
    pub label: String,
    /// `web_seed` or `peer`.
    pub kind: String,
    pub bytes: Size,
    pub rate: Rate,
    pub requests: u64,
    pub errors: u64,
    /// Peer connections this source was presented over, for an HTTP source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connections: Option<usize>,
    /// Bytes pulled over HTTP, when that is a different number from `bytes`.
    ///
    /// It is larger when the same range was fetched more than once, so
    /// `http_bytes / bytes` is the amplification the transport paid for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_bytes: Option<Size>,
    #[serde(skip_serializing_if = "Latencies::is_empty", default)]
    pub latency: Latencies,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_detail: Option<Errors>,
    /// Why this source stopped serving, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

/// How long the transfer was stalled, and how often.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stalls {
    /// Intervals during which no byte arrived.
    pub count: u64,
    /// Total time with no bytes moving.
    pub total: Millis,
    /// The longest single stall.
    pub longest: Millis,
}

/// Piece verification, which becomes the bottleneck before the network does on
/// a fast link.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hashing {
    pub pieces: u64,
    pub bytes: Size,
    pub total: Millis,
    /// Bytes hashed per second.
    pub rate: Rate,
}

/// Choke and unchoke traffic, for the peer-facing benchmarks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChokeStats {
    pub choke_events: u64,
    pub unchoke_events: u64,
    pub peak_queue_depth: u64,
}

/// What the run cost in positioned reads and writes.
///
/// On a download the reads are the session reading each piece back to hash it,
/// so `read_bytes` is close to the payload and is not a second copy of the
/// wire traffic. `bytes_per_payload_byte` in the summary says which.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disk {
    pub read_ops: u64,
    pub read_bytes: Size,
    pub read_time: Millis,
    /// Positioned writes that reached the device.
    pub write_ops: u64,
    /// Writes the session asked for, before a run of them was combined into
    /// one. `write_ops` divided by this is the coalescing factor, and against
    /// `write_time` it is what says whether small blocks are still costing an
    /// operation each. See `TODO/disk-io.md`, T-018.
    ///
    /// Always written, even at zero, so the document shape does not depend on
    /// whether a run wrote anything. `#[serde(default)]` is what lets a report
    /// taken before this existed still read back.
    #[serde(default)]
    pub write_calls: u64,
    pub write_bytes: Size,
    pub write_time: Millis,
}

impl Disk {
    /// Whether anything touched the disk.
    pub fn is_empty(&self) -> bool {
        self.read_ops == 0 && self.write_ops == 0
    }
}

/// How deep the block request pipeline ran.
///
/// A peer answers a bounded number of outstanding block requests at a time. If
/// that bound is reached and stays reached, throughput is capped at the bound
/// times the block size over the time to answer one, whatever the link can do.
/// All of those numbers are here, so the cap can be read off the report rather
/// than inferred from a rate.
///
/// `peak_in_flight` covers the whole run, warmup included, because a
/// high-water mark cannot be narrowed to a window after the fact. Everything
/// else covers the measured window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pipeline {
    /// The most blocks outstanding at once, across every source. This is the
    /// session's request window whenever the run reached it.
    pub peak_in_flight: u64,
    /// Blocks outstanding on average, weighted by time.
    ///
    /// Total service time over the length of the window, which is Little's
    /// law. A gauge read once per metrics interval answers a much worse
    /// question, because the depth changes thousands of times between reads.
    pub mean_in_flight: u64,
    /// Blocks the session asked for, and blocks answered.
    ///
    /// They differ when the session asks again for a block it already has
    /// outstanding: the second answer is dropped rather than sent twice. See
    /// `TODO/webseed.md`, T-008.
    pub requests: u64,
    pub blocks: u64,
    /// Mean time from a request arriving to its block going back out.
    pub mean_service_us: u64,
    /// Mean bytes per block answered.
    pub block_size: Size,
    /// What a pipeline held at `peak_in_flight` would sustain at this service
    /// time. Arithmetic over two measured numbers rather than a measurement of
    /// its own, which is why it is named for what it would allow. Read it
    /// against `sustained_rate`: close together means the request window is
    /// the limit, far apart means something else is.
    pub window_ceiling: Size,
}

impl Pipeline {
    /// Fill the two derived fields from the length of the measured window.
    pub fn derive(mut self, window: std::time::Duration) -> Self {
        let window_us = window.as_micros().min(u128::from(u64::MAX)) as u64;
        self.mean_in_flight = match window_us {
            0 => 0,
            us => self.mean_service_us.saturating_mul(self.blocks) / us,
        };
        self.window_ceiling = Size(match self.mean_service_us {
            0 => 0,
            us => {
                self.peak_in_flight
                    .saturating_mul(self.block_size.0)
                    .saturating_mul(1_000_000)
                    / us
            }
        });
        self
    }
}

/// What the run adds up to, over the measured window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Bytes moved during the measured window, warmup excluded.
    pub bytes: Size,
    /// Length of the measured window.
    pub duration: Millis,
    /// Bytes per second across the whole measured window.
    pub sustained_rate: Rate,
    /// The best single interval.
    pub peak_rate: Rate,
    /// Sustained rate as a share of `parameters.ceiling`, when one was stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ceiling_share: Option<String>,
    pub requests: u64,
    pub errors: Errors,
    #[serde(skip_serializing_if = "Latencies::is_empty", default)]
    pub latency: Latencies,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hashing: Option<Hashing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stalls: Option<Stalls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choke: Option<ChokeStats>,
    /// What the run cost in reads and writes. Present when storage was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<Disk>,
    /// How deep the block request pipeline ran. Present when a source served
    /// blocks through the peer protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<Pipeline>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_peers: Option<u32>,
    /// The concurrency that reached the highest sustained rate, when a sweep
    /// ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_concurrency: Option<usize>,
}

impl Summary {
    /// Sustained rate as a share of a stated ceiling.
    ///
    /// It may exceed a hundred percent. `--ceiling` is a reference the caller
    /// states, not a physical limit, and a run that beat its reference should
    /// say so rather than read as having exactly matched it.
    pub fn share_of(&self, ceiling: u64) -> Option<String> {
        match ceiling {
            0 => None,
            c => Some(format_share(self.sustained_rate.0 as f64 / c as f64)),
        }
    }
}

/// Whether a `--fail-under` threshold was met.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Threshold {
    /// The rate the run had to reach.
    pub fail_under: Rate,
    /// The rate it reached.
    pub observed: Rate,
    pub met: bool,
}

/// The whole report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub report_version: u32,
    pub kind: Kind,
    pub environment: Environment,
    pub parameters: Parameters,
    pub target: Target,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub series: Vec<Sample>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub concurrency_curve: Vec<ConcurrencyStep>,
    /// Per-step detail for `bench disk`, which measures threads rather than
    /// requests and so needs the per-thread cost the curve cannot carry.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub disk_steps: Vec<DiskStep>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sources: Vec<SourceSummary>,
    pub summary: Summary,
    /// What a `bench probe` found. Present only for that subcommand, which
    /// measures reachability and capability rather than throughput.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe: Option<crate::bench::probe::ProbeReport>,
    /// What a `bench swarm` found. Present only for that subcommand, which
    /// measures somebody else's process and so carries per-peer detail no
    /// other report has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swarm: Option<crate::bench::swarm::Outcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<Threshold>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Comparison>,
    /// Anything the run needs the reader to know: a source that never
    /// answered, a warmup longer than the run, a debug build.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub notes: Vec<String>,
}

impl Report {
    /// An empty report of one kind, ready to be filled.
    pub fn new(kind: Kind, environment: Environment) -> Self {
        Self {
            report_version: REPORT_VERSION,
            kind,
            environment,
            parameters: Parameters::default(),
            target: Target::default(),
            series: Vec::new(),
            concurrency_curve: Vec::new(),
            disk_steps: Vec::new(),
            sources: Vec::new(),
            summary: Summary::default(),
            probe: None,
            swarm: None,
            threshold: None,
            baseline: None,
            notes: Vec::new(),
        }
    }

    /// Add a note for the reader.
    pub fn note(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    /// Apply `--fail-under`, filling [`Self::threshold`].
    ///
    /// Returns whether the run met it. A run with no threshold always passes.
    pub fn apply_threshold(&mut self, fail_under: Option<u64>) -> bool {
        let Some(rate) = fail_under else {
            return true;
        };
        let met = self.summary.sustained_rate.0 >= rate;
        self.threshold = Some(Threshold {
            fail_under: Rate(rate),
            observed: self.summary.sustained_rate,
            met,
        });
        met
    }
}

/// One metric compared against a baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    pub metric: String,
    pub baseline: i64,
    pub current: i64,
    /// Current less baseline. Negative means the number went down, whatever
    /// down means for that metric.
    pub change: i64,
    /// The change as a percentage of the baseline, with a sign. Absent when
    /// the baseline is zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_percent: Option<String>,
    /// Whether a larger number is better for this metric, so a reader knows
    /// which direction the sign points.
    pub higher_is_better: bool,
    /// The rendered change, for example `+12.40 MiB/s` or `-8ms`.
    pub human: String,
}

/// What kind of number a metric is, which decides how a delta is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricUnit {
    /// Bytes per second.
    Rate,
    /// Bytes.
    Bytes,
    /// Milliseconds.
    Duration,
    /// A plain count.
    Count,
}

impl MetricUnit {
    fn render(self, value: i64) -> String {
        let magnitude = value.unsigned_abs();
        let sign = if value < 0 { "-" } else { "+" };
        match self {
            Self::Rate => format!("{sign}{}", format_rate(magnitude)),
            Self::Bytes => format!("{sign}{}", format_size(magnitude)),
            Self::Duration => format!("{sign}{magnitude}ms"),
            Self::Count => format!("{sign}{magnitude}"),
        }
    }
}

/// A report compared against an earlier one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comparison {
    /// Where the baseline was read from.
    pub path: String,
    pub baseline_started_at: Timestamp,
    pub baseline_version: String,
    pub deltas: Vec<Delta>,
}

/// Compare a report against a baseline.
///
/// The environments have to be comparable: measurements from two different
/// CPUs are two different questions, and subtracting one from the other
/// produces a number that describes nothing. That check is [`Host::comparable_to`].
pub fn compare(current: &Report, baseline: &Report, path: &str) -> Result<Comparison, String> {
    if current.kind != baseline.kind {
        return Err(format!(
            "the baseline is a `bench {}` report and this is `bench {}`",
            baseline.kind.as_str(),
            current.kind.as_str()
        ));
    }
    if baseline.report_version > REPORT_VERSION {
        return Err(format!(
            "the baseline is report version {} and this build understands {REPORT_VERSION}",
            baseline.report_version
        ));
    }
    if !current
        .environment
        .host
        .comparable_to(&baseline.environment.host)
    {
        return Err(format!(
            "the baseline was taken on different hardware: {}",
            current
                .environment
                .host
                .differences(&baseline.environment.host)
                .join(", ")
        ));
    }

    let mut deltas = Vec::new();
    let mut push = |metric: &str, base: u64, now: u64, unit: MetricUnit, higher_is_better: bool| {
        deltas.push(delta(metric, base, now, unit, higher_is_better));
    };
    push(
        "sustained_rate",
        baseline.summary.sustained_rate.0,
        current.summary.sustained_rate.0,
        MetricUnit::Rate,
        true,
    );
    push(
        "peak_rate",
        baseline.summary.peak_rate.0,
        current.summary.peak_rate.0,
        MetricUnit::Rate,
        true,
    );
    push(
        "bytes",
        baseline.summary.bytes.0,
        current.summary.bytes.0,
        MetricUnit::Bytes,
        true,
    );
    push(
        "requests",
        baseline.summary.requests,
        current.summary.requests,
        MetricUnit::Count,
        true,
    );
    push(
        "errors",
        baseline.summary.errors.total,
        current.summary.errors.total,
        MetricUnit::Count,
        false,
    );
    for (name, base, now) in [
        (
            "connect_p50_ms",
            baseline.summary.latency.connect.p50_ms,
            current.summary.latency.connect.p50_ms,
        ),
        (
            "first_byte_p50_ms",
            baseline.summary.latency.first_byte.p50_ms,
            current.summary.latency.first_byte.p50_ms,
        ),
        (
            "first_byte_p99_ms",
            baseline.summary.latency.first_byte.p99_ms,
            current.summary.latency.first_byte.p99_ms,
        ),
        (
            "complete_p50_ms",
            baseline.summary.latency.complete.p50_ms,
            current.summary.latency.complete.p50_ms,
        ),
        (
            "complete_p99_ms",
            baseline.summary.latency.complete.p99_ms,
            current.summary.latency.complete.p99_ms,
        ),
        (
            "complete_p999_ms",
            baseline.summary.latency.complete.p999_ms,
            current.summary.latency.complete.p999_ms,
        ),
    ] {
        push(name, base, now, MetricUnit::Duration, false);
    }
    push(
        "peak_rss_bytes",
        baseline.environment.process.peak_rss_bytes,
        current.environment.process.peak_rss_bytes,
        MetricUnit::Bytes,
        false,
    );
    push(
        "cpu_ms",
        baseline.environment.process.cpu_ms,
        current.environment.process.cpu_ms,
        MetricUnit::Duration,
        false,
    );
    push(
        "open_handles",
        baseline.environment.process.open_handles,
        current.environment.process.open_handles,
        MetricUnit::Count,
        false,
    );
    if let (Some(base), Some(now)) = (&baseline.summary.hashing, &current.summary.hashing) {
        push("hash_rate", base.rate.0, now.rate.0, MetricUnit::Rate, true);
    }

    Ok(Comparison {
        path: path.to_string(),
        baseline_started_at: baseline.environment.started_at,
        baseline_version: baseline.environment.build.version.clone(),
        deltas,
    })
}

fn delta(metric: &str, base: u64, now: u64, unit: MetricUnit, higher_is_better: bool) -> Delta {
    let baseline = base.min(i64::MAX as u64) as i64;
    let current = now.min(i64::MAX as u64) as i64;
    let change = current - baseline;
    Delta {
        metric: metric.to_string(),
        baseline,
        current,
        change,
        change_percent: match baseline {
            0 => None,
            b => Some(format!("{:+.2}%", (change as f64 / b as f64) * 100.0)),
        },
        higher_is_better,
        human: unit.render(change),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> Build {
        Build {
            version: "0.1.0".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            debug_assertions: false,
        }
    }

    fn report(kind: Kind, rate: u64) -> Report {
        let mut report = Report::new(
            kind,
            Environment::begin(build(), vec!["bit-cli".into()], "/w".into(), Vec::new()),
        );
        report.summary.sustained_rate = Rate(rate);
        report.summary.bytes = Size(rate * 10);
        report
    }

    #[test]
    fn a_report_round_trips_through_json() {
        let mut original = report(Kind::Webseed, 5 * 1024 * 1024);
        original.environment.finish();
        original.note("a note");
        original.parameters.duration = Millis(30_000);
        original.series.push(Sample {
            at: Timestamp::now(),
            elapsed: Millis(1000),
            bytes: Size(1024),
            ..Default::default()
        });
        let json = serde_json::to_string(&original).unwrap();
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn an_environment_records_the_command_line_and_both_timestamps() {
        let mut env = Environment::begin(
            build(),
            vec!["bit-cli".into(), "bench".into(), "webseed".into()],
            "/w".into(),
            vec!["http".into()],
        );
        assert!(env.tracing_enabled);
        assert_eq!(env.command_line.len(), 3);
        env.finish();
        assert!(env.finished_at >= env.started_at);
        assert!(env.process.peak_rss_bytes > 0);
        assert!(env.host.cpu.logical_cores >= 1);
    }

    #[test]
    fn an_environment_keeps_the_highest_cost_it_was_shown() {
        let mut env = Environment::begin(build(), Vec::new(), "/w".into(), Vec::new());
        let spike = Process {
            peak_rss_bytes: u64::MAX / 2,
            ..Default::default()
        };
        env.observe(&spike);
        env.finish();
        assert_eq!(env.process.peak_rss_bytes, u64::MAX / 2);
    }

    #[test]
    fn a_threshold_below_the_observed_rate_passes_and_above_it_fails() {
        let mut report = report(Kind::Webseed, 1000);
        assert!(report.apply_threshold(Some(999)));
        assert!(report.threshold.as_ref().unwrap().met);
        assert!(!report.apply_threshold(Some(1001)));
        assert!(!report.threshold.as_ref().unwrap().met);
        assert_eq!(report.threshold.as_ref().unwrap().observed, Rate(1000));
    }

    #[test]
    fn no_threshold_always_passes_and_records_nothing() {
        let mut report = report(Kind::Webseed, 0);
        assert!(report.apply_threshold(None));
        assert!(report.threshold.is_none());
    }

    #[test]
    fn a_comparison_signs_every_delta_and_names_the_better_direction() {
        let base = report(Kind::Webseed, 1000);
        let mut now = report(Kind::Webseed, 1500);
        now.summary.errors.record("timeout", None);
        let comparison = compare(&now, &base, "prior.json").unwrap();

        let rate = comparison
            .deltas
            .iter()
            .find(|d| d.metric == "sustained_rate")
            .unwrap();
        assert_eq!(rate.change, 500);
        assert_eq!(rate.change_percent.as_deref(), Some("+50.00%"));
        assert!(rate.higher_is_better);

        let errors = comparison
            .deltas
            .iter()
            .find(|d| d.metric == "errors")
            .unwrap();
        assert_eq!(errors.change, 1);
        assert!(
            !errors.higher_is_better,
            "more errors is not an improvement"
        );
        assert_eq!(errors.human, "+1");
    }

    #[test]
    fn a_delta_against_a_zero_baseline_has_no_percentage() {
        let base = report(Kind::Webseed, 0);
        let now = report(Kind::Webseed, 100);
        let comparison = compare(&now, &base, "prior.json").unwrap();
        let rate = comparison
            .deltas
            .iter()
            .find(|d| d.metric == "sustained_rate")
            .unwrap();
        assert_eq!(rate.change, 100);
        assert!(rate.change_percent.is_none());
    }

    #[test]
    fn comparing_two_different_benchmarks_is_refused() {
        let base = report(Kind::Leech, 1000);
        let now = report(Kind::Webseed, 1000);
        let error = compare(&now, &base, "prior.json").unwrap_err();
        assert!(error.contains("leech"), "{error}");
        assert!(error.contains("webseed"), "{error}");
    }

    #[test]
    fn comparing_across_hardware_is_refused_and_says_what_differs() {
        let mut base = report(Kind::Webseed, 1000);
        base.environment.host.cpu.model = "Some Other Processor".into();
        let now = report(Kind::Webseed, 1000);
        let error = compare(&now, &base, "prior.json").unwrap_err();
        assert!(error.contains("different hardware"), "{error}");
        assert!(error.contains("Some Other Processor"), "{error}");
    }

    #[test]
    fn a_baseline_from_a_newer_contract_is_refused() {
        let mut base = report(Kind::Webseed, 1000);
        base.report_version = REPORT_VERSION + 1;
        let now = report(Kind::Webseed, 1000);
        let error = compare(&now, &base, "prior.json").unwrap_err();
        assert!(error.contains("report version"), "{error}");
    }

    #[test]
    fn errors_count_by_class_and_by_status() {
        let mut errors = Errors::default();
        errors.record("not_found", Some(404));
        errors.record("not_found", Some(404));
        errors.record("timeout", None);
        assert_eq!(errors.total, 3);
        assert_eq!(errors.by_class["not_found"], 2);
        assert_eq!(errors.by_status["404"], 2);
        assert!(!errors.by_status.contains_key("0"));
        assert_eq!(errors.ranked()[0], ("not_found".to_string(), 2));

        let mut other = Errors::default();
        other.record("timeout", None);
        errors.merge(&other);
        assert_eq!(errors.total, 4);
        assert_eq!(errors.by_class["timeout"], 2);
    }

    #[test]
    fn a_share_of_a_ceiling_is_a_percentage_and_a_zero_ceiling_is_none() {
        let mut summary = Summary {
            sustained_rate: Rate(500),
            ..Default::default()
        };
        assert_eq!(summary.share_of(1000).as_deref(), Some("50.00%"));
        assert!(summary.share_of(0).is_none());
        summary.sustained_rate = Rate(2000);
        // `--ceiling` is a reference the caller states, such as what `curl`
        // reached against the same URL, and a run can beat it. Reporting that
        // as `100.00%` would hide a result: `TODO/webseed.md` T-001 measured
        // the HTTP path at 156.71% of its `curl` reference over a real
        // network, which is the finding rather than an error in the ceiling.
        assert_eq!(summary.share_of(1000).as_deref(), Some("200.00%"));
    }

    #[test]
    fn a_note_is_recorded_once() {
        let mut report = report(Kind::Webseed, 0);
        report.note("same");
        report.note("same");
        report.note("other");
        assert_eq!(report.notes, vec!["same".to_string(), "other".to_string()]);
    }

    #[test]
    fn every_kind_has_a_stable_name_that_round_trips() {
        for kind in [
            Kind::Leech,
            Kind::Seed,
            Kind::Webseed,
            Kind::Swarm,
            Kind::Probe,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            assert_eq!(serde_json::from_str::<Kind>(&json).unwrap(), kind);
        }
    }
}
