//! What the payload file costs when several receive paths write at once.
//!
//! `bench leech` found that the same 1 GiB of writes costs 995 ms totalled
//! across one receive path and 11,918 ms totalled across eight: the same
//! bytes, the same file, the same block size, and twelve times the time. A
//! download cannot say why, because the network, the session, the hash, and
//! the disk are all running at once. This measures the disk on its own: the
//! same bytes through the same [`crate::storage::SafeStorage`], from N
//! threads, with no session and no network anywhere.
//!
//! Three layouts, and reading them against each other is what makes the answer
//! readable. All three write the same bytes in the same block size from the
//! same number of threads onto the same volume:
//!
//! - [`Layout::Shared`] is the download shape. One file, one handle, every
//!   thread writing interleaved blocks into it.
//! - [`Layout::Split`] is one file per thread, each thread writing only its
//!   own. It says whether spreading the work over more files helps.
//! - [`Layout::Handles`] is one file opened N times, thread `i` writing
//!   through handle `i`, at the same offsets `shared` uses. It says whether
//!   the limit lives on the handle or on the file, which is what decides
//!   whether more handles could fix it.
//!
//! What it found, on NTFS: `handles` tracks `shared` and only `split` scales,
//! so writes to one file serialise whatever handle they arrive on, and the
//! serialisation is charged per operation rather than per byte. The numbers
//! and the commands are in `TODO/disk-io.md`, T-017.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use librqbit::storage::TorrentStorage;

use crate::alloc::Allocation;
use crate::bench::report::{ConcurrencyStep, Costs, Disk, Sample, Summary};
use crate::storage::{SafeStorageFactory, StorageCounts, StorageMetrics};
use crate::sysinfo::Process;
use crate::time::Timestamp;
use crate::units::{Millis, Rate, Size};

/// How the payload is spread over files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One file. Every thread writes interleaved blocks into it. This is what
    /// a torrent with one payload file and several peers does.
    Shared,
    /// One file per thread, each thread writing only its own. The control:
    /// the same work spread over as many file objects as there are threads.
    Split,
    /// One file, opened N times, thread `i` writing through its own handle.
    ///
    /// This is what separates a limit that lives on the handle from one that
    /// lives on the file. `shared` and `handles` write the same bytes to the
    /// same file at the same offsets; the only difference is how many open
    /// handles the writes are spread over. If `handles` scales and `shared`
    /// does not, the limit is per handle and giving a file more of them fixes
    /// it. If neither scales, the file itself is the limit and no number of
    /// handles helps.
    ///
    /// It writes through [`crate::storage::pwrite_all`] directly rather than
    /// through `SafeStorage`, because storage holds one handle per file by
    /// design and the question here is what happens when it does not.
    Handles,
}

impl Layout {
    /// The stable name used on the command line and in the report.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Split => "split",
            Self::Handles => "handles",
        }
    }
}

/// What one run is asked to write.
#[derive(Debug, Clone)]
pub struct Options {
    /// Total bytes written per step.
    pub payload_size: u64,
    /// Bytes per positioned write. The session's block size is 16 KiB.
    pub block_size: u64,
    /// How many threads write at once.
    pub threads: usize,
    /// Steps of a thread-count sweep. Empty means one step at `threads`.
    pub sweep: Vec<usize>,
    /// Consecutive blocks one thread writes before the next one takes over,
    /// under `shared` and `handles`. `1` strides block by block, which is the
    /// most contended arrangement there is and is what every measurement
    /// recorded before 2026-08-22 was taken at.
    ///
    /// A receive path does not write that way. It fetches a range and answers
    /// blocks out of it, so N paths are N sequential streams into one file,
    /// each contiguous for the length of a range. `--run-length 64` at a
    /// 16 KiB block is one 1 MiB range per turn, which is that shape. See
    /// `TODO/disk-io.md`, T-018.
    pub run_length: u64,
    pub layout: Layout,
    pub allocation: Allocation,
    /// How many payload files stay open. Zero is the storage default.
    pub max_open_files: usize,
    /// Stop a step early once this much wall time has passed.
    pub duration: Duration,
    pub metrics_interval: Duration,
    /// Read the payload back and check every block landed where it was sent.
    pub verify: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            payload_size: 1 << 30,
            block_size: 16 * 1024,
            threads: 8,
            sweep: Vec::new(),
            run_length: 1,
            layout: Layout::Shared,
            allocation: Allocation::Sparse,
            max_open_files: 0,
            duration: Duration::from_secs(300),
            metrics_interval: Duration::from_secs(1),
            verify: true,
        }
    }
}

/// The report carries the shape of a step; this module fills it. See
/// [`crate::bench::report::DiskStep`] and [`crate::bench::report::DiskThread`].
pub use crate::bench::report::{DiskStep as Step, DiskThread as ThreadCost};

/// What a whole run produced.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub summary: Summary,
    pub series: Vec<Sample>,
    pub concurrency_curve: Vec<ConcurrencyStep>,
    pub steps: Vec<Step>,
    pub notes: Vec<String>,
}

/// Fill one block with a pattern that says which block it is.
///
/// The index goes in the first eight bytes and the rest is a byte derived from
/// it, so a write that lands at the wrong offset is caught wherever the
/// read-back looks rather than only at the head.
fn fill(buf: &mut [u8], block: u64) {
    let marker = (block % 251) as u8;
    buf.fill(marker);
    let head = block.to_le_bytes();
    let take = head.len().min(buf.len());
    buf[..take].copy_from_slice(&head[..take]);
}

/// Whether a block read back is the block that was written to it.
fn check(buf: &[u8], block: u64) -> bool {
    let marker = (block % 251) as u8;
    let head = block.to_le_bytes();
    let take = head.len().min(buf.len());
    buf[..take] == head[..take] && buf[take..].iter().all(|b| *b == marker)
}

/// Remove a directory tree, waiting out a delete that has not landed yet.
///
/// Windows keeps a deleted file until the last handle to it closes, and while
/// that is pending the directory refuses to go and a fresh file at the same
/// name refuses to open. A step that has just written gigabytes is exactly
/// when that happens, so the removal between steps backs off rather than
/// failing the whole sweep.
fn remove_tree(path: &std::path::Path) -> std::io::Result<()> {
    let mut wait = Duration::from_millis(20);
    for _ in 0..6 {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => std::thread::sleep(wait),
        }
        wait *= 2;
    }
    std::fs::remove_dir_all(path)
}

fn rate_of(bytes: u64, elapsed: Duration) -> u64 {
    match elapsed.as_micros() {
        0 => 0,
        us => (u128::from(bytes) * 1_000_000 / us).min(u128::from(u64::MAX)) as u64,
    }
}

/// Run the measurement.
///
/// `root` is a directory the run owns: every step writes a fresh set of files
/// under it and removes them afterwards, so a sweep of four steps measures the
/// disk four times rather than measuring a volume filling up.
pub fn run(
    root: &std::path::Path,
    options: &Options,
    mut on_sample: impl FnMut(&Sample),
) -> anyhow::Result<Outcome> {
    let mut outcome = Outcome::default();
    let steps = match options.sweep.is_empty() {
        true => vec![options.threads.max(1)],
        false => options.sweep.clone(),
    };

    let started = Instant::now();
    let mut total_bytes = 0u64;
    let mut total_elapsed = Duration::ZERO;
    let mut total_disk = Disk::default();
    let mut peak_rate = 0u64;

    for threads in steps {
        let step_root = root.join(format!("{}-{threads}", options.layout.as_str()));
        if step_root.exists() {
            remove_tree(&step_root)
                .map_err(|e| anyhow::anyhow!("cannot clear {}: {e}", step_root.display()))?;
        }
        std::fs::create_dir_all(&step_root)
            .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", step_root.display()))?;

        let step = one_step(
            &step_root,
            threads,
            options,
            started,
            &mut on_sample,
            &mut outcome,
        )?;

        total_bytes += step.bytes.0;
        total_elapsed += Duration::from_millis(step.elapsed.0);
        total_disk.write_ops += step.write_ops;
        total_disk.write_calls += step.write_calls;
        total_disk.write_bytes = Size(total_disk.write_bytes.0 + step.bytes.0);
        total_disk.write_time = Millis(total_disk.write_time.0 + step.total_write_time.0);
        if let Some(read) = &step.read_back {
            total_disk.read_ops += read.read_ops;
            total_disk.read_bytes = Size(total_disk.read_bytes.0 + read.read_bytes.0);
            total_disk.read_time = Millis(total_disk.read_time.0 + read.read_time.0);
        }
        peak_rate = peak_rate.max(step.rate.0);

        outcome.concurrency_curve.push(ConcurrencyStep {
            concurrency: threads,
            bytes: step.bytes,
            elapsed: step.elapsed,
            rate: step.rate,
            requests: step.write_calls,
            errors: 0,
            latency: Default::default(),
        });
        outcome.steps.push(step);

        // The payload is removed between steps so a sweep does not measure a
        // volume filling up. A failure here is said out loud rather than
        // swallowed: the next step would be writing onto a fuller disk and
        // reporting it as a slower one.
        if let Err(e) = remove_tree(&step_root) {
            outcome.notes.push(format!(
                "could not remove {} after the step: {e}",
                step_root.display()
            ));
        }
    }

    outcome.summary = Summary {
        bytes: Size(total_bytes),
        duration: Millis::from(total_elapsed),
        sustained_rate: Rate(rate_of(total_bytes, total_elapsed)),
        peak_rate: Rate(peak_rate),
        requests: total_disk.write_calls,
        disk: Some(total_disk),
        best_concurrency: outcome
            .concurrency_curve
            .iter()
            .max_by_key(|step| step.rate.0)
            .map(|step| step.concurrency),
        ..Default::default()
    };
    Ok(outcome)
}

/// Where one block lives and who writes it.
///
/// The two layouts differ only here:
///
/// - `shared`: every thread interleaves into file 0, so the owner is the block
///   index divided by [`Options::run_length`], modulo the thread count. At the
///   default run length of 1 that is the block index modulo the thread count,
///   which is where this started and what every measurement before
///   2026-08-22 was taken at.
/// - `split`: thread `t` owns file `t` and writes it end to end.
fn assignment(options: &Options, threads: usize, blocks: u64, block: u64) -> (usize, u64, usize) {
    match options.layout {
        // `handles` interleaves exactly as `shared` does, run length included.
        // The two differ only in how many handles the writes go through, which
        // is what makes them a pair, and a pair measured at two different
        // arrangements would answer nothing.
        Layout::Shared | Layout::Handles => (
            0,
            block * options.block_size.max(1),
            ((block / options.run_length.max(1)) % threads as u64) as usize,
        ),
        Layout::Split => {
            let per_thread = blocks.div_ceil(threads as u64).max(1);
            let owner = (block / per_thread).min(threads as u64 - 1) as usize;
            let within = block - owner as u64 * per_thread;
            (owner, within * options.block_size.max(1), owner)
        }
    }
}

/// Where a step's writes go.
///
/// `shared` and `split` go through the storage a download uses, because what
/// they measure is that storage. `handles` cannot: storage holds exactly one
/// handle per file by design, and the whole question `handles` asks is what
/// changes when a file has more than one. So it holds its own, and counts into
/// the same metrics so the series and the totals line up either way.
enum Sink {
    Storage(Arc<crate::storage::SafeStorage>),
    Handles {
        files: Vec<std::fs::File>,
        metrics: Arc<StorageMetrics>,
    },
}

impl Sink {
    /// Write one block. `lane` picks the handle where there is a choice.
    fn write(&self, lane: usize, file: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        match self {
            Self::Storage(storage) => storage.pwrite_all(file, offset, buf),
            Self::Handles { files, metrics } => {
                let handle = files
                    .get(lane % files.len())
                    .ok_or_else(|| anyhow::anyhow!("no handle for lane {lane}"))?;
                let at = Instant::now();
                crate::storage::pwrite_all(handle, offset, buf)?;
                metrics.observe_write(buf.len() as u64, at.elapsed());
                Ok(())
            }
        }
    }

    fn read(&self, lane: usize, file: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        match self {
            Self::Storage(storage) => storage.pread_exact(file, offset, buf),
            Self::Handles { files, metrics } => {
                let handle = files
                    .get(lane % files.len())
                    .ok_or_else(|| anyhow::anyhow!("no handle for lane {lane}"))?;
                let at = Instant::now();
                crate::storage::pread_exact(handle, offset, buf)?;
                metrics.observe_read(buf.len() as u64, at.elapsed());
                Ok(())
            }
        }
    }

    fn flush(&self) -> anyhow::Result<()> {
        match self {
            Self::Storage(storage) => storage.flush_all(),
            Self::Handles { files, .. } => {
                // Every handle points at the same file, so one flush covers
                // the lot.
                match files.first() {
                    Some(file) => file.sync_all().map_err(anyhow::Error::from),
                    None => Ok(()),
                }
            }
        }
    }
}

/// One step: build the files, write the payload from `threads` threads, read
/// it back, and report what it cost.
fn one_step(
    root: &std::path::Path,
    threads: usize,
    options: &Options,
    run_started: Instant,
    on_sample: &mut impl FnMut(&Sample),
    outcome: &mut Outcome,
) -> anyhow::Result<Step> {
    let threads = threads.max(1);
    let block_size = options.block_size.max(1);
    let blocks = options.payload_size.div_ceil(block_size);
    let payload = blocks * block_size;

    let files = match options.layout {
        Layout::Shared | Layout::Handles => 1,
        Layout::Split => threads,
    };
    // In `split` the last thread may own fewer blocks than the rest, so every
    // file is sized for a full share. A file longer than its data costs
    // nothing here and keeps the offsets simple.
    let per_file = match options.layout {
        Layout::Shared | Layout::Handles => payload,
        Layout::Split => blocks.div_ceil(threads as u64).max(1) * block_size,
    };
    let names: Vec<String> = (0..files).map(|i| format!("payload{i}.bin")).collect();

    let metrics = Arc::new(StorageMetrics::default());
    let factory = SafeStorageFactory::new(root, true, false)
        .with_allocation(options.allocation)
        .with_max_open_files(options.max_open_files)
        .with_metrics(metrics.clone());
    let mut storage = factory.for_paths(&names, root.to_path_buf());
    storage.init_paths()?;
    // Reserving space first is what the session does, and it matters here for
    // a reason beyond fairness: a write past the end of a file is handled
    // synchronously by the filesystem whatever the handle says, so measuring
    // extending writes would measure the allocator rather than the handle.
    for index in 0..files {
        storage.ensure_file_length(index, per_file)?;
    }
    // `handles` needs the file created and sized, which storage has just done,
    // and then its own handles onto it. Storage's own handle is dropped so it
    // does not sit in the middle of what is being measured.
    let sink = match options.layout {
        Layout::Handles => {
            let path = root.join(&names[0]);
            drop(storage);
            let mut opened = Vec::with_capacity(threads);
            for _ in 0..threads {
                opened.push(
                    std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .truncate(false)
                        .open(&path)
                        .map_err(|e| anyhow::anyhow!("cannot open {}: {e}", path.display()))?,
                );
            }
            Sink::Handles {
                files: opened,
                metrics: metrics.clone(),
            }
        }
        _ => Sink::Storage(Arc::new(storage)),
    };
    let sink = Arc::new(sink);

    let deadline = run_started + options.duration;
    let before_writes = metrics.read();
    let done_blocks = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let phase_started = Instant::now();

    let costs = std::thread::scope(|scope| -> anyhow::Result<Vec<ThreadCost>> {
        let mut handles = Vec::with_capacity(threads);
        for index in 0..threads {
            let sink = sink.clone();
            let done_blocks = done_blocks.clone();
            let stop = stop.clone();
            handles.push(scope.spawn(move || -> anyhow::Result<ThreadCost> {
                let mut buf = vec![0u8; block_size as usize];
                let mut written = 0u64;
                let mut ops = 0u64;
                let mut spent = Duration::ZERO;
                for block in 0..blocks {
                    let (file, offset, owner) = assignment(options, threads, blocks, block);
                    if owner != index {
                        continue;
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    fill(&mut buf, block);
                    let at = Instant::now();
                    sink.write(index, file, offset, &buf)?;
                    spent += at.elapsed();
                    written += block_size;
                    ops += 1;
                    done_blocks.fetch_add(1, Ordering::Relaxed);
                    if Instant::now() >= deadline {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                Ok(ThreadCost {
                    index,
                    blocks: ops,
                    bytes: Size(written),
                    write_time: Millis::from(spent),
                    mean_write_us: match ops {
                        0 => 0,
                        ops => (spent.as_micros().min(u128::from(u64::MAX)) as u64) / ops,
                    },
                })
            }));
        }

        // One point of the series, from the counters as they stand.
        //
        // A closure rather than inline code because the loop is not the only
        // caller: the phase emits one last point after the writers stop. See
        // the comment on that call.
        let point =
            |now: Instant, delta: &StorageCounts, window: Duration, cumulative: u64| Sample {
                at: Timestamp::now(),
                elapsed: Millis::from(now.duration_since(phase_started)),
                warmup: false,
                concurrency: threads,
                bytes: Size(delta.write_bytes),
                cumulative_bytes: Size(cumulative),
                rate: Rate(rate_of(delta.write_bytes, window)),
                requests: delta.write_ops,
                errors: 0,
                process: Process::sample(),
                costs: Some(Costs {
                    verify: Millis(0),
                    verify_bytes: Size(0),
                    disk_read: Millis(delta.read_nanos / 1_000_000),
                    disk_read_bytes: Size(delta.read_bytes),
                    disk_write: Millis(delta.write_nanos / 1_000_000),
                    disk_write_bytes: Size(delta.write_bytes),
                    mean_service_us: match delta.write_ops {
                        0 => None,
                        ops => Some(delta.write_nanos / 1_000 / ops),
                    },
                }),
                ..Default::default()
            };

        // The sampler runs on this thread while the writers work, so the
        // series has the same shape `bench leech` produces and the two read
        // side by side.
        let mut next = Instant::now() + options.metrics_interval;
        let mut last = StorageCounts::default();
        let mut last_at = phase_started;
        let mut cumulative = 0u64;
        while done_blocks.load(Ordering::Relaxed) < blocks && !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now >= next {
                let counts = metrics.read();
                let delta = counts.since(&last);
                cumulative += delta.write_bytes;
                let sample = point(now, &delta, now.duration_since(last_at), cumulative);
                on_sample(&sample);
                outcome.series.push(sample);
                last = counts;
                last_at = now;
                next = now + options.metrics_interval;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let mut costs = Vec::with_capacity(threads);
        for handle in handles {
            costs.push(
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("a writer thread panicked"))??,
            );
        }

        // Held bytes go to the device before the last point is taken, or the
        // point undercounts by whatever the write buffer was still holding
        // and the series does not add up to the payload. `Layout::Shared` and
        // `Layout::Split` write through `SafeStorage`, which coalesces, so
        // this is where the last run of a sequential stream lands. See
        // `TODO/disk-io.md`, T-018.
        sink.flush()?;

        // One last point, once the writers have stopped and everything they
        // did is in the counters.
        //
        // The loop above only emits on an interval boundary, so the window
        // between the last boundary and the end of the phase was never
        // reported, and a phase that finished inside a single interval
        // reported nothing at all: its series was empty. 64 MiB on a fast NVMe
        // is about twenty milliseconds against a ten millisecond interval,
        // which is the margin that made the schema generator produce no
        // `bench_sample` on `macos-latest` and one everywhere else. Same
        // defect as `bench leech` had, recorded as T-149 in `TODO/bench.md`.
        let now = Instant::now();
        let counts = metrics.read();
        let delta = counts.since(&last);
        if delta.write_ops > 0 || outcome.series.is_empty() {
            cumulative += delta.write_bytes;
            let sample = point(now, &delta, now.duration_since(last_at), cumulative);
            on_sample(&sample);
            outcome.series.push(sample);
        }
        Ok(costs)
    })?;

    let elapsed = phase_started.elapsed();

    // Draining the writeback queue is what makes one step comparable to the
    // next. Without it a step that filled the page cache hands its cost to
    // whichever step runs after it, and a sweep reports the order the steps
    // ran in rather than the thread count.
    let flush_started = Instant::now();
    sink.flush()?;
    let flush = flush_started.elapsed();

    if stop.load(Ordering::Relaxed) {
        outcome.notes.push(format!(
            "the {threads}-thread step stopped at --duration before the whole payload was written"
        ));
    }

    let written: u64 = costs.iter().map(|c| c.bytes.0).sum();
    let ops: u64 = costs.iter().map(|c| c.blocks).sum();
    let total_write_time: u64 = costs.iter().map(|c| c.write_time.0).sum();
    // What actually reached the device, taken after the flush so the buffer's
    // last runs are in it. The threads counted their own calls, which is a
    // different number now that a run of them can become one operation, and
    // reporting the calls as `write_ops` would have made this instrument
    // blind to the one change it exists to measure.
    let device_write_ops = metrics.read().since(&before_writes).write_ops;

    let (verified, read_back) = match options.verify {
        false => (None, None),
        true => {
            let before = metrics.read();
            let mut buf = vec![0u8; block_size as usize];
            let mut ok = true;
            for block in 0..done_blocks.load(Ordering::Relaxed).min(blocks) {
                let (file, offset, _) = assignment(options, threads, blocks, block);
                if sink.read(0, file, offset, &mut buf).is_err() || !check(&buf, block) {
                    ok = false;
                    break;
                }
            }
            let delta = metrics.read().since(&before);
            (
                Some(ok),
                Some(Disk {
                    read_ops: delta.read_ops,
                    read_bytes: Size(delta.read_bytes),
                    read_time: Millis(delta.read_nanos / 1_000_000),
                    write_ops: 0,
                    write_calls: 0,
                    write_bytes: Size(0),
                    write_time: Millis(0),
                }),
            )
        }
    };
    if verified == Some(false) {
        outcome.notes.push(format!(
            "the {threads}-thread step read back a block that is not the one written to it"
        ));
    }

    Ok(Step {
        threads,
        layout: options.layout.as_str().to_string(),
        run_length: options.run_length.max(1),
        files,
        bytes: Size(written),
        elapsed: Millis::from(elapsed),
        rate: Rate(rate_of(written, elapsed)),
        total_write_time: Millis(total_write_time),
        write_ops: device_write_ops,
        write_calls: ops,
        mean_write_us: match ops {
            0 => 0,
            ops => total_write_time * 1_000 / ops,
        },
        concurrency_achieved: format!(
            "{:.2}",
            match elapsed.as_micros() {
                0 => 0.0,
                us => total_write_time as f64 * 1_000.0 / us as f64,
            }
        ),
        flush: Millis::from(flush),
        threads_detail: costs,
        verified,
        read_back,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick(layout: Layout, threads: usize, sweep: Vec<usize>, payload: u64) -> Options {
        Options {
            payload_size: payload,
            block_size: 16 * 1024,
            threads,
            sweep,
            layout,
            allocation: Allocation::None,
            metrics_interval: Duration::from_millis(50),
            ..Default::default()
        }
    }

    #[test]
    fn a_block_reads_back_as_the_block_that_was_written() {
        let mut buf = vec![0u8; 4096];
        fill(&mut buf, 7);
        assert!(check(&buf, 7));
        assert!(!check(&buf, 8), "a different block must not check out");
    }

    #[test]
    fn a_block_shorter_than_its_marker_still_round_trips() {
        let mut buf = vec![0u8; 4];
        fill(&mut buf, 0x0102_0304_0506_0708);
        assert!(check(&buf, 0x0102_0304_0506_0708));
    }

    #[test]
    fn a_run_writes_the_payload_and_reads_every_block_back() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = run(
            dir.path(),
            &quick(Layout::Shared, 4, vec![], 1 << 20),
            |_| {},
        )
        .unwrap();
        assert_eq!(outcome.summary.bytes.0, 1 << 20);
        let step = &outcome.steps[0];
        assert_eq!(step.files, 1);
        // `write_calls` and not `write_ops`: the property here is that the
        // step asked storage for one write per block, and how many of those
        // the buffer combined is a scheduling outcome. It asserted `write_ops`
        // while that field carried the calls, which is the mislabelling
        // `TODO/disk-io.md` T-018 corrects.
        assert_eq!(step.write_calls, 64);
        assert!(
            step.write_ops <= step.write_calls,
            "the device cannot see more writes than were asked for: {} against {}",
            step.write_ops,
            step.write_calls
        );
        assert_eq!(step.verified, Some(true));
        assert!(outcome.notes.is_empty(), "{:?}", outcome.notes);
    }

    /// A run shorter than one sample interval still produces a series.
    ///
    /// The sampler emits on interval boundaries, so a phase that finished
    /// before the first one emitted nothing and the report carried an empty
    /// series: a measurement with no points, which reads as a measurement that
    /// was not taken. A one hour metrics interval is the same thing every fast
    /// disk was already doing to a ten millisecond one, made deterministic.
    /// See `TODO/bench.md`, T-152.
    #[test]
    fn a_phase_shorter_than_one_interval_still_reports_a_sample() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = quick(Layout::Shared, 2, vec![], 1 << 20);
        options.metrics_interval = Duration::from_secs(3600);
        let mut seen = 0usize;
        let outcome = run(dir.path(), &options, |_| seen += 1).unwrap();
        assert_eq!(
            outcome.series.len(),
            1,
            "one point, and exactly one: the loop cannot have emitted any"
        );
        assert_eq!(seen, 1, "the callback sees the same point the series has");
        let sample = &outcome.series[0];
        assert_eq!(
            sample.cumulative_bytes.0,
            1 << 20,
            "the one point accounts for the whole payload"
        );
        // Fewer than the 64 blocks that were handed over, and that is the
        // measurement rather than a loss: `Layout::Shared` writes through
        // `SafeStorage`, which coalesces a run of blocks into one operation.
        // What has to hold is that the point carries every operation that did
        // reach the device, which the byte count above already says.
        assert!(
            (1..=64).contains(&sample.requests),
            "{} operations for 64 blocks",
            sample.requests
        );
    }

    #[test]
    fn the_split_layout_gives_every_thread_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = run(
            dir.path(),
            &quick(Layout::Split, 4, vec![], 1 << 20),
            |_| {},
        )
        .unwrap();
        let step = &outcome.steps[0];
        assert_eq!(step.files, 4);
        assert_eq!(step.verified, Some(true));
        assert_eq!(step.threads_detail.len(), 4);
        for thread in &step.threads_detail {
            assert_eq!(
                thread.blocks, 16,
                "thread {} wrote the wrong share",
                thread.index
            );
        }
    }

    /// `TODO/disk-io.md` T-018: the run length is what decides whether the
    /// shared layout writes the shape a download writes.
    ///
    /// At 1 a thread's next block is `threads` blocks past its last and
    /// nothing is ever contiguous, which is the arrangement every measurement
    /// before 2026-08-22 was taken at and the reason the coalescer can combine
    /// nothing there. At `n` a thread writes `n` blocks in a row, which is a
    /// receive path answering blocks out of one fetched range.
    #[test]
    fn the_run_length_decides_how_far_a_thread_writes_before_the_next_one() {
        let mut options = quick(Layout::Shared, 4, vec![], 1 << 20);
        let owners = |options: &Options| -> Vec<usize> {
            (0..8)
                .map(|block| assignment(options, 4, 64, block).2)
                .collect()
        };

        assert_eq!(
            owners(&options),
            vec![0, 1, 2, 3, 0, 1, 2, 3],
            "the default strides block by block"
        );

        options.run_length = 2;
        assert_eq!(
            owners(&options),
            vec![0, 0, 1, 1, 2, 2, 3, 3],
            "a run length of two gives each thread two blocks in a row"
        );

        // Offsets are a pure function of the block index, so a run length
        // changes who writes a block and never where it lands. That is what
        // keeps the read-back check meaningful at any run length.
        options.run_length = 1;
        let offsets: Vec<u64> = (0..8)
            .map(|block| assignment(&options, 4, 64, block).1)
            .collect();
        options.run_length = 4;
        let strided: Vec<u64> = (0..8)
            .map(|block| assignment(&options, 4, 64, block).1)
            .collect();
        assert_eq!(offsets, strided, "the run length must not move a block");
    }

    /// A run as long as the write buffer's region reaches the device as one
    /// operation, and the report says so.
    ///
    /// This is the property the acceptance clause rests on and the one
    /// `--layout shared` could not show before: the step now reports what
    /// reached the device beside what the threads asked for, and at a 64 block
    /// run the two differ by exactly the region size.
    ///
    /// The count is asserted only on this side. The strided side is not a
    /// fixed number: the buffer belongs to the storage rather than to a
    /// thread, so two threads writing adjacent blocks in order do extend one
    /// run, and how often that happens is a scheduling outcome this test does
    /// not control. Measured at 225 of 256 on one machine, and asserting it
    /// would be asserting the scheduler.
    #[test]
    fn a_run_the_length_of_the_write_region_reaches_the_device_as_one_operation() {
        let dir = tempfile::tempdir().unwrap();
        let payload = 4 << 20;
        let block_size = 16 * 1024;
        let blocks = payload / block_size;
        // 64 blocks of 16 KiB is the 1 MiB region the buffer flushes at, so
        // each thread's turn is exactly one operation and no run is displaced:
        // four threads against eight run slots.
        let run_length = 64;

        let mut options = quick(Layout::Shared, 4, vec![], payload);
        options.run_length = run_length;
        let outcome = run(dir.path(), &options, |_| {}).unwrap();
        let step = &outcome.steps[0];

        assert_eq!(
            step.write_calls, blocks,
            "the threads still ask for one write per block"
        );
        assert_eq!(
            step.write_ops,
            blocks / run_length,
            "and every {run_length} of them reach the device as one"
        );
        assert_eq!(
            step.verified,
            Some(true),
            "every block still reads back as itself"
        );
        assert_eq!(
            step.run_length, run_length,
            "the report says which run length"
        );
    }

    #[test]
    fn a_sweep_reports_one_step_per_thread_count() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = run(
            dir.path(),
            &quick(Layout::Shared, 1, vec![1, 2], 256 * 1024),
            |_| {},
        )
        .unwrap();
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(outcome.concurrency_curve.len(), 2);
        assert_eq!(outcome.steps[0].threads, 1);
        assert_eq!(outcome.steps[1].threads, 2);
        // Every step writes the whole payload, so the summary is the sum.
        assert_eq!(outcome.summary.bytes.0, 512 * 1024);
    }

    #[test]
    fn each_step_removes_its_payload_so_a_sweep_does_not_fill_the_volume() {
        let dir = tempfile::tempdir().unwrap();
        run(
            dir.path(),
            &quick(Layout::Shared, 1, vec![1, 2], 128 * 1024),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "a step left its payload behind"
        );
    }

    #[test]
    fn a_deadline_that_has_already_passed_stops_the_step_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = quick(Layout::Shared, 2, vec![], 8 << 20);
        options.duration = Duration::ZERO;
        let outcome = run(dir.path(), &options, |_| {}).unwrap();
        assert!(
            outcome.steps[0].bytes.0 < 8 << 20,
            "the deadline did not stop the step"
        );
        assert!(
            outcome.notes.iter().any(|n| n.contains("--duration")),
            "{:?}",
            outcome.notes
        );
    }
}
