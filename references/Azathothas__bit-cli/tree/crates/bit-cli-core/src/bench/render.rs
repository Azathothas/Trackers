//! Writing a report out.
//!
//! Four formats, one report. `json` is the whole document. `ndjson` is the
//! same fields as one object per line, so a long run can be streamed and
//! filtered with `jq`. `csv` is the time series as a table, because that is
//! the part a spreadsheet or a plotting tool wants. `text` is for a person.
//!
//! `csv` is the one format that cannot carry everything: a report is nested
//! and a CSV is flat. It carries the time series and nothing else, and that
//! is said in the docs rather than left for a reader to discover.

use crate::bench::report::{Errors, Kind, Latencies, Percentiles, Report, Sample};
use crate::error::{Error, Result};
use crate::sysinfo::format_link_speed;
use crate::units::{format_duration_ms, format_rate, format_size};

/// How a report is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// One JSON document, indented.
    Json,
    /// One JSON object per line.
    Ndjson,
    /// The time series as a table.
    Csv,
    /// Lines a person reads.
    Text,
}

impl Format {
    /// The stable name used on the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Ndjson => "ndjson",
            Self::Csv => "csv",
            Self::Text => "text",
        }
    }
}

/// Render a report in one format.
pub fn render(report: &Report, format: Format) -> Result<String> {
    Ok(match format {
        Format::Json => serde_json::to_string_pretty(report)
            .map_err(|e| Error::generic(format!("cannot render the report as JSON: {e}")))?,
        Format::Ndjson => ndjson(report)?,
        Format::Csv => csv(report),
        Format::Text => text(report).join("\n"),
    })
}

/// One JSON object per line.
///
/// The first line is the whole report with the series and the per-source rows
/// removed, so a reader gets the environment and the summary without buffering
/// the whole run. Then one line per sample, one per sweep step, one per
/// source.
fn ndjson(report: &Report) -> Result<String> {
    let mut head = report.clone();
    head.series = Vec::new();
    head.sources = Vec::new();
    head.concurrency_curve = Vec::new();
    head.disk_steps = Vec::new();

    let mut lines = Vec::new();
    let mut push = |kind: &str, value: serde_json::Value| -> Result<()> {
        let mut object = serde_json::Map::new();
        object.insert("record".into(), serde_json::Value::String(kind.into()));
        if let serde_json::Value::Object(fields) = value {
            for (key, value) in fields {
                object.insert(key, value);
            }
        }
        lines.push(
            serde_json::to_string(&serde_json::Value::Object(object))
                .map_err(|e| Error::generic(format!("cannot render an NDJSON record: {e}")))?,
        );
        Ok(())
    };

    push("report", to_value(&head)?)?;
    for sample in &report.series {
        push("sample", to_value(sample)?)?;
    }
    for step in &report.concurrency_curve {
        push("concurrency_step", to_value(step)?)?;
    }
    for step in &report.disk_steps {
        push("disk_step", to_value(step)?)?;
    }
    for source in &report.sources {
        push("source", to_value(source)?)?;
    }
    Ok(lines.join("\n"))
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value)
        .map_err(|e| Error::generic(format!("cannot render the report: {e}")))
}

/// The columns a CSV report carries, in order.
pub const CSV_COLUMNS: &[&str] = &[
    "at",
    "elapsed_ms",
    "warmup",
    "concurrency",
    "bytes",
    "cumulative_bytes",
    "rate_bytes_per_sec",
    "requests",
    "errors",
    "connect_p50_ms",
    "connect_p99_ms",
    "first_byte_p50_ms",
    "first_byte_p99_ms",
    "complete_p50_ms",
    "complete_p90_ms",
    "complete_p99_ms",
    "complete_p999_ms",
    "complete_max_ms",
    "peak_rss_bytes",
    "cpu_ms",
    "open_handles",
    "peers",
    "pieces_verified",
    "queue_depth",
    "verify_ms",
    "verify_bytes",
    "disk_read_ms",
    "disk_read_bytes",
    "disk_write_ms",
    "disk_write_bytes",
    "mean_service_us",
];

/// The time series as a table.
fn csv(report: &Report) -> String {
    let mut out = vec![CSV_COLUMNS.join(",")];
    for sample in &report.series {
        out.push(csv_row(sample));
    }
    out.join("\n")
}

fn csv_row(sample: &Sample) -> String {
    let optional = |value: Option<u64>| value.map(|v| v.to_string()).unwrap_or_default();
    let costs = sample.costs.clone().unwrap_or_default();
    [
        sample.at.iso(),
        sample.elapsed.0.to_string(),
        sample.warmup.to_string(),
        sample.concurrency.to_string(),
        sample.bytes.0.to_string(),
        sample.cumulative_bytes.0.to_string(),
        sample.rate.0.to_string(),
        sample.requests.to_string(),
        sample.errors.to_string(),
        sample.latency.connect.p50_ms.to_string(),
        sample.latency.connect.p99_ms.to_string(),
        sample.latency.first_byte.p50_ms.to_string(),
        sample.latency.first_byte.p99_ms.to_string(),
        sample.latency.complete.p50_ms.to_string(),
        sample.latency.complete.p90_ms.to_string(),
        sample.latency.complete.p99_ms.to_string(),
        sample.latency.complete.p999_ms.to_string(),
        sample.latency.complete.max_ms.to_string(),
        sample.process.peak_rss_bytes.to_string(),
        sample.process.cpu_ms.to_string(),
        sample.process.open_handles.to_string(),
        optional(sample.peers.map(u64::from)),
        optional(sample.pieces_verified),
        optional(sample.queue_depth),
        costs.verify.0.to_string(),
        costs.verify_bytes.0.to_string(),
        costs.disk_read.0.to_string(),
        costs.disk_read_bytes.0.to_string(),
        costs.disk_write.0.to_string(),
        costs.disk_write_bytes.0.to_string(),
        optional(costs.mean_service_us),
    ]
    .join(",")
}

/// Two columns, aligned, for the text rendering.
fn field(key: &str, value: impl std::fmt::Display) -> String {
    format!("{key:<22} {value}")
}

/// The human rendering.
pub fn text(report: &Report) -> Vec<String> {
    let mut out = vec![
        format!("bench {}", report.kind.as_str()),
        String::new(),
        field("started", report.environment.started_at.iso()),
        field("finished", report.environment.finished_at.iso()),
        field("elapsed", report.environment.elapsed.to_string()),
    ];
    if !report.target.source.is_empty() {
        out.push(field("target", &report.target.source));
    }
    if let Some(hash) = &report.target.info_hash {
        out.push(field("info hash", hash));
    }
    if let Some(total) = report.target.total {
        out.push(field("size", format_size(total.0)));
    }
    for endpoint in &report.target.endpoints {
        out.push(field("endpoint", endpoint));
    }

    out.push(String::new());
    out.push("Environment".to_string());
    let environment = &report.environment;
    out.push(field(
        "  bit-cli",
        format!(
            "{} ({}, {})",
            environment.build.version, environment.build.target, environment.build.profile
        ),
    ));
    out.push(field(
        "  os",
        match &environment.host.os.distribution {
            Some(distribution) => format!(
                "{} {} ({distribution})",
                environment.host.os.name, environment.host.os.version
            ),
            None => format!(
                "{} {}",
                environment.host.os.name, environment.host.os.version
            ),
        },
    ));
    out.push(field(
        "  cpu",
        format!(
            "{} ({} logical, {})",
            environment.host.cpu.model,
            environment.host.cpu.logical_cores,
            environment.host.cpu.architecture
        ),
    ));
    out.push(field(
        "  memory",
        format_size(environment.host.memory_total.0),
    ));
    for nic in &environment.host.network {
        out.push(field(
            "  link",
            match nic.link_speed_bps {
                Some(bps) => format!("{} at {}", nic.name, format_link_speed(bps)),
                None => format!("{} (speed not reported)", nic.name),
            },
        ));
    }
    out.push(field("  command", environment.command_line.join(" ")));
    out.push(field(
        "  cost",
        format!(
            "peak RSS {}, CPU {}, {} handles",
            format_size(environment.process.peak_rss_bytes),
            format_duration_ms(environment.process.cpu_ms),
            environment.process.open_handles
        ),
    ));
    if environment.tracing_enabled {
        out.push(field(
            "  tracing",
            format!("on ({})", environment.trace_subsystems.join(", ")),
        ));
    }

    // A swarm run measures somebody else's process, so what a reader wants
    // first is how many of the peers it asked for actually got anywhere.
    if let Some(swarm) = &report.swarm {
        out.push(String::new());
        out.push("Swarm".to_string());
        out.push(field("  load", swarm.mode.as_str()));
        out.push(field("  dialled", &swarm.dialled));
        out.push(field(
            "  peers",
            format!(
                "{} dialled, {} connected, {} handshaked",
                swarm.peers_dialled, swarm.peers_connected, swarm.peers_handshaked
            ),
        ));
        if swarm.peers_wrong_info_hash > 0 {
            out.push(field(
                "  wrong info hash",
                format!("{} peers", swarm.peers_wrong_info_hash),
            ));
        }
        for failure in &swarm.failures {
            out.push(field(&format!("  {}", failure.class), failure.count));
        }
        if swarm.connect.count > 0 {
            out.push(field(
                "  connect",
                format!(
                    "p50 {} p99 {} max {}",
                    format_duration_ms(swarm.connect.p50_ms),
                    format_duration_ms(swarm.connect.p99_ms),
                    format_duration_ms(swarm.connect.max_ms),
                ),
            ));
        }
        if swarm.handshake.count > 0 {
            out.push(field(
                "  handshake",
                format!(
                    "p50 {} p99 {} max {}",
                    format_duration_ms(swarm.handshake.p50_ms),
                    format_duration_ms(swarm.handshake.p99_ms),
                    format_duration_ms(swarm.handshake.max_ms),
                ),
            ));
        }
        if swarm.mode == crate::bench::swarm::Mode::Leech {
            out.push(field(
                "  unchoked",
                format!("{} peers", swarm.peers_unchoked),
            ));
            out.push(field(
                "  choke events",
                format!(
                    "{} choke, {} unchoke",
                    swarm.choke_events, swarm.unchoke_events
                ),
            ));
            out.push(field("  received", format_size(swarm.bytes_received.0)));
            out.push(field("  blocks", swarm.blocks_received));
            out.push(field(
                "  pieces",
                format!(
                    "{} verified, {} failed",
                    swarm.pieces_verified, swarm.pieces_failed
                ),
            ));
            out.push(field(
                "  held",
                format!(
                    "{} of {} budget, {} pieces dropped",
                    format_size(swarm.bytes_held.0),
                    format_size(swarm.disk_budget.0),
                    swarm.pieces_dropped_over_budget
                ),
            ));
        }
    }

    // A probe has no throughput, so its findings are the report. Rendered
    // before the summary, which for a probe carries only the deadline and
    // whether the one exchange failed.
    if let Some(probe) = &report.probe {
        out.push(String::new());
        out.push("Probe".to_string());
        out.push(field("  target", &probe.target));
        out.push(field("  kind", &probe.kind));
        out.push(field(
            "  reachable",
            match probe.reachable {
                true => "yes",
                false => "no",
            },
        ));
        if let Some(connect) = probe.connect {
            out.push(field("  connect", format_duration_ms(connect.0)));
        }
        if let Some(first) = probe.first_response {
            out.push(field("  first response", format_duration_ms(first.0)));
        }
        if let Some(error) = &probe.error {
            out.push(field("  error", error));
        }
        if let Some(peer) = &probe.peer {
            out.push(field("  peer id", &peer.peer_id));
            if let Some(client) = &peer.client {
                out.push(field("  client", client));
            }
            out.push(field("  reserved", &peer.reserved));
            out.push(field(
                "  extensions",
                match peer.extensions.is_empty() {
                    true => "none claimed".to_string(),
                    false => peer.extensions.join(", "),
                },
            ));
            out.push(field(
                "  info hash",
                match peer.info_hash_matches {
                    true => "echoed",
                    false => "not echoed: it answered about a different torrent",
                },
            ));
            if let Some(extended) = &peer.extended {
                if let Some(client) = &extended.client {
                    out.push(field("  says it is", client));
                }
                if let Some(queue) = extended.request_queue {
                    out.push(field("  request queue", queue));
                }
                if !extended.extensions.is_empty() {
                    out.push(field(
                        "  extension messages",
                        extended.extensions.join(", "),
                    ));
                }
                if let Some(upload_only) = extended.upload_only {
                    out.push(field("  upload only", upload_only));
                }
            }
            if !peer.messages.is_empty() {
                out.push(field("  messages", peer.messages.join(", ")));
            }
            if let Some(pieces) = peer.pieces_advertised {
                out.push(field("  pieces advertised", pieces));
            }
        }
        if let Some(http) = &probe.http {
            out.push(field("  status", http.status));
            out.push(field(
                "  ranges",
                match http.range_support {
                    true => "supported",
                    false => "not answered with 206",
                },
            ));
            if let Some(length) = http.entity_length.or(http.content_length) {
                out.push(field("  length", format_size(length)));
            }
            if let Some(server) = &http.server {
                out.push(field("  server", server));
            }
            out.push(field("  http", &http.http_version));
            for hop in &http.redirects {
                out.push(field("  redirect", hop));
            }
            if let Some(resolved) = &http.resolved_url {
                out.push(field("  resolved to", resolved));
            }
            if let Some(tls) = &http.tls {
                out.push(field(
                    "  tls",
                    format!("{} with {}", tls.version, tls.cipher_suite),
                ));
            }
        }
    }
    out.push(String::new());
    out.push("Summary".to_string());
    let summary = &report.summary;
    out.push(field(
        "  measured over",
        format_duration_ms(summary.duration.0),
    ));
    out.push(field("  bytes", format_size(summary.bytes.0)));
    out.push(field(
        "  sustained",
        match &summary.ceiling_share {
            Some(share) => format!(
                "{} ({share} of the stated ceiling)",
                format_rate(summary.sustained_rate.0)
            ),
            None => format_rate(summary.sustained_rate.0),
        },
    ));
    out.push(field("  peak", format_rate(summary.peak_rate.0)));
    out.push(field(
        "  requests",
        format!("{} ({} failed)", summary.requests, summary.errors.total),
    ));
    if let Some(best) = summary.best_concurrency {
        out.push(field("  best concurrency", best));
    }
    if let Some(peers) = summary.peak_peers {
        out.push(field("  peak peers", peers));
    }
    out.extend(latency_lines(&summary.latency));
    if let Some(hashing) = &summary.hashing {
        out.push(field(
            "  verification",
            format!(
                "{} pieces, {} in {}",
                hashing.pieces,
                format_rate(hashing.rate.0),
                format_duration_ms(hashing.total.0)
            ),
        ));
    }
    if let Some(stalls) = &summary.stalls
        && stalls.count > 0
    {
        out.push(field(
            "  stalls",
            format!(
                "{} totalling {} (longest {})",
                stalls.count,
                format_duration_ms(stalls.total.0),
                format_duration_ms(stalls.longest.0)
            ),
        ));
    }
    if let Some(choke) = &summary.choke {
        out.push(field(
            "  choke",
            format!(
                "{} choke, {} unchoke, queue depth {}",
                choke.choke_events, choke.unchoke_events, choke.peak_queue_depth
            ),
        ));
    }
    if let Some(disk) = &summary.disk {
        out.push(field(
            "  disk read",
            format!(
                "{} in {} over {} reads",
                format_size(disk.read_bytes.0),
                format_duration_ms(disk.read_time.0),
                disk.read_ops
            ),
        ));
        out.push(field(
            "  disk write",
            format!(
                "{} in {} over {} writes",
                format_size(disk.write_bytes.0),
                format_duration_ms(disk.write_time.0),
                disk.write_ops
            ),
        ));
    }
    if let Some(pipeline) = &summary.pipeline {
        out.push(field(
            "  pipeline",
            format!(
                "{} blocks in flight on average, {} at peak, {} block, {}us to answer",
                pipeline.mean_in_flight,
                pipeline.peak_in_flight,
                format_size(pipeline.block_size.0),
                pipeline.mean_service_us
            ),
        ));
        out.push(field(
            "  window allows",
            format!(
                "{} at that depth and that service time; {} was measured, {} of it",
                format_rate(pipeline.window_ceiling.0),
                format_rate(summary.sustained_rate.0),
                match pipeline.window_ceiling.0 {
                    0 => "n/a".to_string(),
                    ceiling => crate::units::format_share(
                        summary.sustained_rate.0 as f64 / ceiling as f64
                    ),
                },
            ),
        ));
    }
    out.extend(error_lines(&summary.errors));

    // `bench disk` fills both `disk_steps` and `concurrency_curve`, and the
    // curve's latency columns are empty for it because a positioned write has
    // no connect time and no first byte. The step table carries the columns
    // that do mean something, so it replaces the curve rather than sitting
    // beside it.
    if !report.disk_steps.is_empty() {
        out.push(String::new());
        out.push("Writers".to_string());
        out.push(format!(
            "  {:<8} {:<8} {:<6} {:<14} {:<9} {:<9} {:<12} {:<11} {}",
            "THREADS",
            "LAYOUT",
            "FILES",
            "RATE",
            "WALL",
            "FLUSH",
            "WRITE TOTAL",
            "MEAN WRITE",
            "OVERLAP"
        ));
        for step in &report.disk_steps {
            out.push(format!(
                "  {:<8} {:<8} {:<6} {:<14} {:<9} {:<9} {:<12} {:<11} {}",
                step.threads,
                step.layout,
                step.files,
                format_rate(step.rate.0),
                format_duration_ms(step.elapsed.0),
                format_duration_ms(step.flush.0),
                format_duration_ms(step.total_write_time.0),
                format!("{}us", step.mean_write_us),
                step.concurrency_achieved,
            ));
        }
        if let Some(step) = report.disk_steps.iter().find(|s| s.verified == Some(false)) {
            out.push(field(
                "  verify",
                format!(
                    "the {}-thread step read back a block it did not write",
                    step.threads
                ),
            ));
        }
    } else if !report.concurrency_curve.is_empty() {
        out.push(String::new());
        out.push("Concurrency".to_string());
        out.push(format!(
            "  {:<6} {:<14} {:<10} {:<10} {:<10} {}",
            "CONC", "RATE", "REQS", "ERRS", "P50", "P99"
        ));
        for step in &report.concurrency_curve {
            let p50 = format!("{}ms", step.latency.complete.p50_ms);
            out.push(format!(
                "  {:<6} {:<14} {:<10} {:<10} {p50:<10} {}ms",
                step.concurrency,
                format_rate(step.rate.0),
                step.requests,
                step.errors,
                step.latency.complete.p99_ms,
            ));
        }
    }

    if !report.sources.is_empty() {
        out.push(String::new());
        // A seeding run's rows are the peers that pulled from it, not sources
        // it pulled from, and calling them sources would read as backwards.
        let (heading, row, verb) = match report.kind {
            Kind::Seed => ("Peers", "  peer", "    sent"),
            _ => ("Sources", "  source", "    served"),
        };
        out.push(heading.to_string());
        for source in &report.sources {
            out.push(field(row, &source.label));
            // A peer makes no requests this process counts, so a run of rows
            // reading "over 0 requests" is noise rather than information.
            let requests = match source.requests {
                0 => String::new(),
                count => format!(" over {count} requests ({} failed)", source.errors),
            };
            out.push(field(
                verb,
                format!(
                    "{} at {}{requests}",
                    format_size(source.bytes.0),
                    format_rate(source.rate.0),
                ),
            ));
            if let Some(connections) = source.connections {
                out.push(field(
                    "    connections",
                    format!(
                        "{connections} peer connection{}",
                        match connections {
                            1 => "",
                            _ => "s",
                        }
                    ),
                ));
            }
            if let Some(http) = source.http_bytes {
                out.push(field(
                    "    over HTTP",
                    match source.bytes.0 {
                        0 => format_size(http.0),
                        served => format!(
                            "{} ({}x what reached the session)",
                            format_size(http.0),
                            crate::units::format_ratio(http.0 as f64 / served as f64)
                        ),
                    },
                ));
            }
            if !source.latency.first_byte.is_empty() {
                out.push(field("    first byte", source.latency.first_byte.line()));
            }
            if let Some(failure) = &source.failure {
                out.push(field("    failed", failure));
            }
        }
    }

    if let Some(threshold) = &report.threshold {
        out.push(String::new());
        out.push(field(
            "threshold",
            format!(
                "{} required, {} observed: {}",
                format_rate(threshold.fail_under.0),
                format_rate(threshold.observed.0),
                match threshold.met {
                    true => "met",
                    false => "not met",
                }
            ),
        ));
    }

    if let Some(comparison) = &report.baseline {
        out.push(String::new());
        out.push(format!(
            "Against {} taken {}",
            comparison.path,
            comparison.baseline_started_at.iso()
        ));
        out.push(format!(
            "  {:<20} {:<16} {:<16} {:<16} {}",
            "METRIC", "BASELINE", "CURRENT", "CHANGE", "PERCENT"
        ));
        for delta in &comparison.deltas {
            out.push(format!(
                "  {:<20} {:<16} {:<16} {:<16} {}",
                delta.metric,
                delta.baseline,
                delta.current,
                delta.human,
                delta.change_percent.as_deref().unwrap_or("")
            ));
        }
    }

    if !report.notes.is_empty() {
        out.push(String::new());
        out.push("Notes".to_string());
        for note in &report.notes {
            out.push(format!("  {note}"));
        }
    }
    out
}

fn latency_lines(latency: &Latencies) -> Vec<String> {
    let mut out = Vec::new();
    for (name, percentiles) in [
        ("  connect", &latency.connect),
        ("  first byte", &latency.first_byte),
        ("  complete", &latency.complete),
    ] {
        if !Percentiles::is_empty(percentiles) {
            out.push(field(name, percentiles.line()));
        }
    }
    out
}

fn error_lines(errors: &Errors) -> Vec<String> {
    if errors.total == 0 {
        return Vec::new();
    }
    let ranked = errors.ranked();
    let mut out = vec![field(
        "  errors",
        ranked
            .iter()
            .map(|(class, count)| format!("{class} {count}"))
            .collect::<Vec<_>>()
            .join(", "),
    )];
    if !errors.by_status.is_empty() {
        out.push(field(
            "  http status",
            errors
                .by_status
                .iter()
                .map(|(status, count)| format!("{status} {count}"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    out
}

/// The one-line verdict, for a caller who wants a single string.
pub fn headline(report: &Report) -> String {
    format!(
        "bench {}: {} sustained over {} ({} requests, {} failed)",
        report.kind.as_str(),
        format_rate(report.summary.sustained_rate.0),
        format_duration_ms(report.summary.duration.0),
        report.summary.requests,
        report.summary.errors.total
    )
}

/// Whether a kind reads bytes off a network, which decides whether a rate of
/// zero is worth a note.
pub const fn moves_bytes(kind: Kind) -> bool {
    !matches!(kind, Kind::Probe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::report::{Build, ConcurrencyStep, Environment, SourceSummary, Threshold};
    use crate::time::Timestamp;
    use crate::units::{Millis, Rate, Size};

    fn report() -> Report {
        let mut report = Report::new(
            Kind::Webseed,
            Environment::begin(
                Build {
                    version: "0.1.0".into(),
                    target: "x86_64-pc-windows-msvc".into(),
                    profile: "release".into(),
                    debug_assertions: false,
                },
                vec!["bit-cli".into(), "bench".into(), "webseed".into()],
                "/w".into(),
                Vec::new(),
            ),
        );
        report.environment.finish();
        report.target.source = "album.torrent".into();
        report.target.info_hash = Some("abc123".into());
        report.target.total = Some(Size(4096));
        report.summary.bytes = Size(10 * 1024 * 1024);
        report.summary.duration = Millis(10_000);
        report.summary.sustained_rate = Rate(1024 * 1024);
        report.summary.peak_rate = Rate(2 * 1024 * 1024);
        report.summary.requests = 40;
        report.summary.errors.record("timeout", None);
        report.summary.latency.first_byte = Percentiles {
            count: 40,
            p50_ms: 12,
            p90_ms: 30,
            p99_ms: 88,
            p999_ms: 120,
            max_ms: 130,
            mean_ms: 20,
        };
        report.series.push(Sample {
            at: Timestamp::from_epoch_ms(1_787_140_323_418),
            elapsed: Millis(1000),
            bytes: Size(1024),
            cumulative_bytes: Size(1024),
            rate: Rate(1024),
            requests: 4,
            concurrency: 8,
            ..Default::default()
        });
        report.concurrency_curve.push(ConcurrencyStep {
            concurrency: 8,
            rate: Rate(1024 * 1024),
            requests: 40,
            ..Default::default()
        });
        report.sources.push(SourceSummary {
            index: 0,
            label: "https://mirror.example/pub/".into(),
            kind: "web_seed".into(),
            bytes: Size(10 * 1024 * 1024),
            rate: Rate(1024 * 1024),
            requests: 40,
            errors: 1,
            ..Default::default()
        });
        report
    }

    #[test]
    fn json_is_a_single_document_that_reads_back() {
        let rendered = render(&report(), Format::Json).unwrap();
        let back: Report = serde_json::from_str(&rendered).unwrap();
        assert_eq!(back.summary.sustained_rate, Rate(1024 * 1024));
        assert_eq!(back.environment.build.version, "0.1.0");
    }

    #[test]
    fn json_carries_the_environment_with_every_field_populated() {
        let rendered = render(&report(), Format::Json).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        let environment = &doc["environment"];
        assert!(
            environment["build"]["target"]
                .as_str()
                .unwrap()
                .contains('-')
        );
        assert!(
            !environment["host"]["cpu"]["model"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        assert!(
            environment["host"]["cpu"]["logical_cores"]
                .as_u64()
                .unwrap()
                >= 1
        );
        assert!(
            environment["host"]["memory_total"]["bytes"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            environment["started_at"]["iso"]
                .as_str()
                .unwrap()
                .ends_with('Z')
        );
        assert!(
            environment["finished_at"]["iso"]
                .as_str()
                .unwrap()
                .ends_with('Z')
        );
        assert!(environment["process"]["peak_rss_bytes"].as_u64().unwrap() > 0);
        assert_eq!(environment["command_line"][0], "bit-cli");
    }

    #[test]
    fn ndjson_is_one_object_per_line_and_every_line_parses() {
        let rendered = render(&report(), Format::Ndjson).unwrap();
        let lines: Vec<serde_json::Value> = rendered
            .lines()
            .map(|line| serde_json::from_str(line).expect("every line is JSON"))
            .collect();
        assert_eq!(lines[0]["record"], "report");
        assert!(
            lines[0].get("series").is_none(),
            "the head line does not repeat the series"
        );
        let kinds: Vec<&str> = lines
            .iter()
            .map(|line| line["record"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            ["report", "sample", "concurrency_step", "source"],
            "one record per part of the report"
        );
    }

    #[test]
    fn csv_has_a_header_and_one_row_per_sample() {
        let rendered = render(&report(), Format::Csv).unwrap();
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines[0], CSV_COLUMNS.join(","));
        assert_eq!(lines.len(), 2);
        let cells: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(cells.len(), CSV_COLUMNS.len(), "every column is filled");
        assert_eq!(cells[0], "2026-08-19T11:52:03.418Z");
        assert_eq!(cells[1], "1000");
    }

    #[test]
    fn csv_of_an_empty_run_is_a_header_and_nothing_else() {
        let mut report = report();
        report.series.clear();
        assert_eq!(render(&report, Format::Csv).unwrap(), CSV_COLUMNS.join(","));
    }

    #[test]
    fn text_names_the_machine_the_command_and_the_result() {
        let rendered = render(&report(), Format::Text).unwrap();
        assert!(rendered.contains("bench webseed"));
        assert!(rendered.contains("Environment"));
        assert!(rendered.contains("Summary"));
        assert!(rendered.contains("1.00 MiB/s"), "{rendered}");
        assert!(rendered.contains("bit-cli bench webseed"));
        assert!(rendered.contains("timeout 1"));
        assert!(rendered.contains("p50 12ms"));
        assert!(rendered.contains("https://mirror.example/pub/"));
    }

    #[test]
    fn text_reports_a_threshold_and_says_which_way_it_went() {
        let mut report = report();
        report.threshold = Some(Threshold {
            fail_under: Rate(2 * 1024 * 1024),
            observed: Rate(1024 * 1024),
            met: false,
        });
        let rendered = render(&report, Format::Text).unwrap();
        assert!(rendered.contains("not met"), "{rendered}");
    }

    #[test]
    fn no_format_writes_anything_to_a_stream_of_its_own() {
        // Every format is a string the caller places. Nothing here prints.
        for format in [Format::Json, Format::Ndjson, Format::Csv, Format::Text] {
            assert!(!render(&report(), format).unwrap().is_empty());
        }
    }

    #[test]
    fn the_headline_fits_on_one_line() {
        let line = headline(&report());
        assert!(!line.contains('\n'));
        assert!(line.contains("1.00 MiB/s"));
    }
}
