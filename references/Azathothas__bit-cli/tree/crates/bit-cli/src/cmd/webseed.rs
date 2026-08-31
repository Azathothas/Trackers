//! `bit-cli webseed`: inspect, validate, and read from HTTP sources.
//!
//! `list` is the dry-run for the addressing model. It resolves every binding
//! and prints the exact URL each file would be requested from, without
//! touching the network. Getting a mirror layout wrong is the most common way
//! a web seed silently does nothing, and this is how you find out before the
//! download rather than after it.

use std::sync::Arc;

use bit_cli_core::ExitCode;
use bit_cli_core::error::{Error, Result};
use bit_cli_core::layout::Layout;
use bit_cli_core::span::summarize_indices;
use bit_cli_core::torrent::Metainfo;
use bit_cli_core::units::{Size, format_size};
use bit_cli_core::webseed::binding::BindingSet;
use bit_cli_core::webseed::fetch::Fetcher;
use serde::Serialize;

use crate::cli::{
    Global, WebseedCommand, WebseedFetchArgs, WebseedListArgs, WebseedProbeArgs, WebseedTestArgs,
};
use crate::env::Env;
use crate::output::{Renderer, field, table};
use crate::source::{Kind, resolve_source};
use crate::webseed_args;

/// One source, as `webseed list` reports it.
#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub index: usize,
    pub url: String,
    pub origin: &'static str,
    pub scope: String,
    pub mode: &'static str,
    pub style: &'static str,
    pub priority: i32,
    pub in_scope: Size,
    pub in_scope_share: String,
    pub files: Vec<usize>,
    pub whole_pieces: usize,
    pub partial_pieces: usize,
    /// Statuses this source retries that would otherwise retire it, and the
    /// ones that retire it that would otherwise be retried. Both are empty on
    /// a source with no policy, which is almost all of them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub retry_status: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fatal_status: Vec<String>,
    /// Bytes this source's window cache may hold before it starts evicting,
    /// which is `cache_windows * chunk_size`. Reported because it is memory a
    /// run costs by construction and nothing said so before it started. See
    /// `TODO/memory.md`, T-041.
    pub cache_budget: Size,
    pub urls: Vec<UrlReport>,
}

/// One file and the URL it resolves to.
#[derive(Debug, Clone, Serialize)]
pub struct UrlReport {
    pub file: usize,
    pub path: String,
    pub size: Size,
    pub in_scope: Size,
    pub url: String,
}

/// What `bit-cli webseed list` reports.
#[derive(Debug, Clone, Serialize)]
pub struct ListReport {
    pub info_hash: String,
    pub name: String,
    pub total: Size,
    pub piece_count: u32,
    pub source_count: usize,
    pub sources: Vec<SourceReport>,
    pub covered: Size,
    pub uncovered: Size,
    pub uncovered_pieces: Vec<u32>,
    pub complete: bool,
    /// Windows each source caches. One number for the run: it is computed from
    /// the largest chunk size any source asked for. See `TODO/memory.md`,
    /// T-041.
    pub cache_windows: usize,
    /// Every source's `cache_budget` added up, which is what the run will hold
    /// in window caches with every mirror busy.
    pub cache_budget_total: Size,
}

impl ListReport {
    /// Build from resolved bindings.
    pub fn new(meta: &Metainfo, layout: &Layout, set: &BindingSet) -> Self {
        // The same function the run itself calls, so what this prints is what
        // a download of the same torrent with the same flags will hold rather
        // than a second estimate of it. See `TODO/memory.md`, T-041.
        let specs: Vec<_> = set.bindings.iter().map(|b| b.spec.clone()).collect();
        let (cache_windows, _, cache_budget_total) = crate::cmd::download::cache_budget(&specs);
        let sources = set
            .bindings
            .iter()
            .map(|binding| {
                let whole = binding.scope.whole_pieces(layout).len();
                SourceReport {
                    index: binding.index,
                    url: binding.spec.url.clone(),
                    origin: binding.spec.origin.as_str(),
                    scope: binding.scope.selector.clone(),
                    mode: binding.spec.mode.as_str(),
                    style: binding.spec.style.as_str(),
                    priority: binding.spec.priority,
                    in_scope: Size(binding.scope.bytes),
                    in_scope_share: bit_cli_core::units::percent_of(
                        binding.scope.bytes,
                        layout.total_length,
                    ),
                    files: binding.scope.files.clone(),
                    whole_pieces: whole,
                    partial_pieces: binding.scope.pieces.len().saturating_sub(whole),
                    retry_status: (&binding.spec.limits.retry_status).into(),
                    fatal_status: (&binding.spec.limits.fatal_status).into(),
                    cache_budget: Size(
                        binding
                            .spec
                            .limits
                            .chunk_size
                            .saturating_mul(cache_windows as u64),
                    ),
                    urls: binding
                        .file_urls
                        .iter()
                        .map(|f| UrlReport {
                            file: f.index,
                            path: f.path.clone(),
                            size: Size(f.length),
                            in_scope: Size(f.in_scope_bytes),
                            url: f.url.clone(),
                        })
                        .collect(),
                }
            })
            .collect();

        Self {
            info_hash: meta.info_hash().hex(),
            name: layout.name.clone(),
            total: Size(layout.total_length),
            piece_count: layout.piece_count(),
            source_count: set.bindings.len(),
            sources,
            covered: Size(set.covered.len()),
            uncovered: Size(set.uncovered.len()),
            uncovered_pieces: set.uncovered_pieces.clone(),
            complete: set.is_complete(),
            cache_windows,
            cache_budget_total: Size(cache_budget_total),
        }
    }

    /// The text rendering.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field("torrent", &self.name),
            field("info hash", &self.info_hash),
            field("size", format_size(self.total.0)),
            field("sources", self.source_count),
            field(
                "coverage",
                format!(
                    "{} of {} ({})",
                    format_size(self.covered.0),
                    format_size(self.total.0),
                    bit_cli_core::units::percent_of(self.covered.0, self.total.0)
                ),
            ),
        ];
        if !self.complete {
            out.push(field(
                "uncovered pieces",
                summarize_indices(&self.uncovered_pieces),
            ));
        }
        out.push(field(
            "window cache",
            format!(
                "{} across {} source(s), {} window(s) each",
                format_size(self.cache_budget_total.0),
                self.source_count,
                self.cache_windows
            ),
        ));
        for source in &self.sources {
            out.push(String::new());
            out.push(format!("[{}] {}", source.index, source.url));
            out.push(field(
                "  scope",
                format!(
                    "{} ({}, {} files, {} whole pieces, {} partial)",
                    source.scope,
                    source.in_scope_share,
                    source.files.len(),
                    source.whole_pieces,
                    source.partial_pieces
                ),
            ));
            out.push(field(
                "  composition",
                format!(
                    "{} / {} / priority {}",
                    source.mode, source.style, source.priority
                ),
            ));
            out.push(field("  origin", source.origin));
            out.push(field("  window cache", format_size(source.cache_budget.0)));
            // Printed only where a policy was set, because "retry statuses:
            // none" on every source of every listing says nothing.
            if !source.retry_status.is_empty() {
                out.push(field("  retry status", source.retry_status.join(",")));
            }
            if !source.fatal_status.is_empty() {
                out.push(field("  fatal status", source.fatal_status.join(",")));
            }
            if source.urls.is_empty() {
                out.push(field("  urls", "built per request from the template"));
                continue;
            }
            let rows: Vec<Vec<String>> = source
                .urls
                .iter()
                .map(|u| {
                    vec![
                        u.file.to_string(),
                        format_size(u.in_scope.0),
                        u.path.clone(),
                        u.url.clone(),
                    ]
                })
                .collect();
            out.extend(
                table(&["FILE", "IN SCOPE", "PATH", "URL"], &rows)
                    .into_iter()
                    .map(|line| format!("  {line}")),
            );
        }
        out
    }
}

/// Resolve a source and its bindings.
///
/// The source itself may be fetched, which is what `SOURCE`'s help text has
/// always offered and what four `webseed` subcommands used to refuse. The
/// bindings are still resolved without the network: `no_network` below is
/// about `--web-seed-list-url`, which is a different fetch with a different
/// entry that T-183 already decided. See `TODO/cli-surface.md`, T-245.
fn resolve(
    source: &str,
    web_seeds: &crate::cli::WebSeedArgs,
    swarm: &crate::cli::SwarmSourceArgs,
    page: &crate::cli::PageSourceArgs,
    global: &Global,
    env: &mut Env,
) -> Result<(Metainfo, Layout, BindingSet)> {
    let kind = Kind::classify(source, env)?;
    let meta = resolve_source(
        &kind,
        env,
        global,
        web_seeds.web_seed_user_agent.as_deref(),
        swarm,
        page,
    )?;
    let layout = meta.layout();
    let specs = webseed_args::collect(web_seeds, Some(&meta), None, env, webseed_args::no_network)?;
    if specs.is_empty() {
        return Err(Error::no_usable_sources(
            "no web seed sources: the torrent declares none and none were given",
        )
        .with("hint", "pass --web-seed <URL> or --web-seed-config <PATH>"));
    }
    let set = BindingSet::resolve(&layout, &meta.info_hash().hex(), &specs)?;
    Ok((meta, layout, set))
}

/// `bit-cli webseed list`.
pub fn list(
    args: &WebseedListArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let (meta, layout, set) = resolve(
        &args.source.source,
        &args.web_seeds,
        &args.swarm,
        &args.page,
        global,
        env,
    )?;
    let report = ListReport::new(&meta, &layout, &set);
    warn_about_cache(&set, renderer, env);

    // `--web-seed-require` turns an incomplete mirror set into a failure. A
    // script that declared its mirrors wants to know they are wrong here, not
    // after a download stalls.
    if args.web_seeds.web_seed_require || args.web_seeds.web_seed_only {
        set.require_coverage(false)?;
    }
    renderer.emit(env, "webseed_list", &report, || report.lines())?;
    Ok(ExitCode::Success)
}

/// Say so when the window caches will cost more than the run's ceiling.
///
/// Raised here as well as on `download` because `webseed list` is the command
/// a caller runs to find out what a set of mirrors will do **before** running
/// it, and the memory it will hold is part of that. See `TODO/memory.md`,
/// T-041.
pub(crate) fn warn_about_cache(set: &BindingSet, renderer: &mut Renderer, env: &mut Env) {
    let specs: Vec<_> = set.bindings.iter().map(|b| b.spec.clone()).collect();
    if let Some(message) = crate::cmd::download::cache_budget_warning(&specs) {
        renderer.warn(env, message);
    }
}

/// A tokio runtime for the commands that do I/O.
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::generic(format!("cannot start the async runtime: {e}")))
}

/// What `bit-cli webseed fetch` reports.
#[derive(Debug, Clone, Serialize)]
pub struct FetchReport {
    pub url: String,
    pub source_index: usize,
    pub offset: u64,
    pub length: Size,
    pub pieces: Vec<u32>,
    pub verified: bool,
    pub elapsed: bit_cli_core::units::Millis,
    pub rate: Size,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub written_to: Option<String>,
    pub requests: Vec<RequestReport>,
}

/// One HTTP request, as the trace records it.
#[derive(Debug, Clone, Serialize)]
pub struct RequestReport {
    pub at: String,
    pub url: String,
    pub range: String,
    pub status: Option<u16>,
    pub bytes: u64,
    pub total_ms: u64,
    pub ttfb_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The equivalent `curl` command, so a failing request can be rerun by hand.
    pub curl: String,
}

/// `bit-cli webseed fetch`.
pub fn fetch(
    args: &WebseedFetchArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let (meta, layout, set) = resolve(
        &args.source.source,
        &args.web_seeds,
        &args.swarm,
        &args.page,
        global,
        env,
    )?;

    // Work out which bytes were asked for. Exactly one selector is allowed,
    // and the clap definition already refuses the conflicting combinations.
    let range = if let Some(piece) = args.piece {
        layout.piece_range(piece).ok_or_else(|| {
            Error::usage(format!(
                "piece {piece} is out of range: this torrent has {} pieces",
                layout.piece_count()
            ))
        })?
    } else if let Some(spec) = &args.pieces {
        let scope = bit_cli_core::webseed::Scope::parse(&format!("piece:{spec}"))?;
        scope
            .resolve(&layout)?
            .spans
            .bounds()
            .ok_or_else(|| Error::usage(format!("piece range `{spec}` selects nothing")))?
    } else if let Some(spec) = &args.bytes {
        let scope = bit_cli_core::webseed::Scope::parse(&format!("byte:{spec}"))?;
        scope
            .resolve(&layout)?
            .spans
            .bounds()
            .ok_or_else(|| Error::usage(format!("byte range `{spec}` selects nothing")))?
    } else if let Some(index) = args.file {
        layout
            .file(index)
            .map(|f| f.range())
            .ok_or_else(|| Error::usage(format!("file {index} is out of range")))?
    } else {
        return Err(Error::usage(
            "`webseed fetch` needs one of --piece, --pieces, --file, or --bytes",
        ));
    };

    // Pick the source: the one named by --url, else the highest-priority one
    // that can serve the whole range.
    let candidates = set.sources_for_range(&range);
    let binding = match &args.url {
        Some(url) => set
            .bindings
            .iter()
            .find(|b| &b.spec.url == url)
            .ok_or_else(|| {
                Error::usage(format!("--url {url} is not one of the declared sources"))
                    .with("url", url.clone())
            })?,
        None => candidates
            .iter()
            .find(|b| b.covers(&range))
            .copied()
            .ok_or_else(|| {
                Error::coverage_gap(format!(
                    "no declared source covers bytes {}-{}",
                    range.start,
                    range.end - 1
                ))
                .with("requested_start", range.start)
                .with("requested_end", range.end)
            })?,
    };

    let layout = Arc::new(layout);
    let fetcher = Fetcher::new(
        binding.clone(),
        layout.clone(),
        meta.info_hash().hex(),
        4,
        true,
    )?;

    let started = std::time::Instant::now();
    let runtime = runtime()?;
    let data = runtime
        .block_on(fetcher.read(range.start, range.end - range.start))
        .map_err(Error::from)?;
    let elapsed = started.elapsed();
    let records = runtime.block_on(fetcher.records());

    let pieces: Vec<u32> = layout.pieces_overlapping(&range).collect();
    let mut verified = false;
    if args.verify {
        verified = verify_pieces(&meta, &layout, &range, &data, &pieces)?;
    }

    let written_to = match &args.output {
        None => None,
        Some(target) if target == "-" => {
            use std::io::Write;
            env.out.write_all(&data).map_err(|e| {
                bit_cli_core::error::from_io(e, "cannot write the fetched bytes to stdout")
            })?;
            Some("-".to_string())
        }
        Some(target) => {
            let path = env.resolve(std::path::Path::new(target));
            if global.dry_run {
                renderer.warn(env, format!("--dry-run: not writing {}", path.display()));
            } else {
                std::fs::write(&path, &data).map_err(|e| {
                    bit_cli_core::error::from_io(e, format!("cannot write {}", path.display()))
                })?;
            }
            Some(path.display().to_string())
        }
    };

    let headers: Vec<(String, String)> = binding
        .spec
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), redact(k, v, global.no_redact)))
        .collect();

    let report = FetchReport {
        url: binding.spec.url.clone(),
        source_index: binding.index,
        offset: range.start,
        length: Size(data.len() as u64),
        pieces,
        verified,
        elapsed: bit_cli_core::units::Millis::from(elapsed),
        rate: Size(rate_per_second(data.len() as u64, elapsed)),
        written_to,
        requests: records
            .iter()
            .map(|r| RequestReport {
                at: r.started_at.iso(),
                url: r.url.clone(),
                range: r.range.clone(),
                status: r.status,
                bytes: r.bytes,
                total_ms: r.total_ms,
                ttfb_ms: r.ttfb_ms,
                server: r.server.clone(),
                error: r.error.clone(),
                curl: r.as_curl(&headers),
            })
            .collect(),
    };

    // Writing the payload to stdout and a JSON report to stdout would
    // interleave two different things in one stream, so the report goes to
    // stderr in that one case.
    if report.written_to.as_deref() == Some("-") {
        let _ = env.note(format!(
            "fetched {} from {} in {}",
            format_size(report.length.0),
            report.url,
            report.elapsed
        ));
    } else {
        renderer.emit(env, "webseed_fetch", &report, || fetch_lines(&report))?;
    }
    Ok(ExitCode::Success)
}

fn fetch_lines(report: &FetchReport) -> Vec<String> {
    let mut out = vec![
        field("url", &report.url),
        field("offset", report.offset),
        field("length", format_size(report.length.0)),
        field("pieces", summarize_indices(&report.pieces)),
        field("verified", report.verified),
        field("elapsed", report.elapsed),
        field("rate", format!("{}/s", format_size(report.rate.0))),
    ];
    if let Some(target) = &report.written_to {
        out.push(field("written to", target));
    }
    for request in &report.requests {
        out.push(String::new());
        out.push(field("request", &request.range));
        out.push(field(
            "  status",
            request
                .status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".into()),
        ));
        out.push(field("  total", format!("{}ms", request.total_ms)));
        if let Some(error) = &request.error {
            out.push(field("  error", error));
        }
        out.push(field("  curl", &request.curl));
    }
    out
}

/// Redact a header value unless the caller asked to see it.
fn redact(name: &str, value: &str, no_redact: bool) -> String {
    if no_redact {
        return value.to_string();
    }
    let sensitive = [
        "authorization",
        "proxy-authorization",
        "cookie",
        "x-api-key",
    ];
    match sensitive.contains(&name.to_ascii_lowercase().as_str()) {
        true => "<redacted>".to_string(),
        false => value.to_string(),
    }
}

fn rate_per_second(bytes: u64, elapsed: std::time::Duration) -> u64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        return 0;
    }
    (bytes as f64 / secs) as u64
}

/// Check the fetched bytes against the torrent's piece hashes.
///
/// Only pieces the range covers in full can be checked; a partial piece has no
/// hash of its own. That is reported rather than silently passing, because
/// "verified" has to mean the same thing every time.
fn verify_pieces(
    meta: &Metainfo,
    layout: &Layout,
    range: &std::ops::Range<u64>,
    data: &[u8],
    pieces: &[u32],
) -> Result<bool> {
    use sha1::{Digest, Sha1};
    let mut checked = 0;
    for &piece in pieces {
        let Some(piece_range) = layout.piece_range(piece) else {
            continue;
        };
        if piece_range.start < range.start || piece_range.end > range.end {
            continue;
        }
        let from = (piece_range.start - range.start) as usize;
        let to = (piece_range.end - range.start) as usize;
        let expected =
            meta.info().pieces.get(piece as usize).ok_or_else(|| {
                Error::generic(format!("the torrent has no hash for piece {piece}"))
            })?;
        let mut hasher = Sha1::new();
        hasher.update(&data[from..to]);
        let actual: [u8; 20] = hasher.finalize().into();
        if &actual != expected {
            return Err(
                Error::hash_mismatch(format!("piece {piece} does not match the torrent"))
                    .with("piece", piece)
                    .with("expected", hex(expected))
                    .with("actual", hex(&actual)),
            );
        }
        checked += 1;
    }
    if checked == 0 {
        return Err(Error::usage(format!(
            "bytes {}-{} do not cover any whole piece, so nothing can be verified; use --piece or widen the range, or pass --verify=false",
            range.start,
            range.end - 1
        ))
        .with("requested_start", range.start)
        .with("requested_end", range.end));
    }
    Ok(true)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What `bit-cli webseed test` reports.
#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub info_hash: String,
    pub name: String,
    pub source_count: usize,
    pub usable: usize,
    pub unusable: usize,
    pub sources: Vec<bit_cli_core::webseed::probe::SourceTest>,
}

/// `bit-cli webseed test`: probe each source, one request each.
pub fn test(
    args: &WebseedTestArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let (meta, layout, set) = resolve(
        &args.source.source,
        &args.web_seeds,
        &args.swarm,
        &args.page,
        global,
        env,
    )?;
    if global.dry_run {
        // A dry run of a probe is `webseed list`: the addressing without the
        // network. Saying so is more useful than probing anyway.
        return list(
            &WebseedListArgs {
                source: crate::cli::SourceArgs {
                    source: args.source.source.clone(),
                },
                web_seeds: args.web_seeds.clone(),
                swarm: args.swarm.clone(),
                page: args.page.clone(),
            },
            global,
            renderer,
            env,
        );
    }

    let head = args.head;
    let concurrency = args.concurrency.max(1);
    // Cloned per worker below, so they are owned here rather than borrowed
    // from `args` across a spawn.
    let report_headers = std::sync::Arc::new(args.report_headers.clone());
    let redact = !global.no_redact;
    let deadline = crate::swarm::optional_duration(&global.timeout, "timeout")?;
    let runtime = runtime()?;
    let info_hash = meta.info_hash().hex();
    // Resolve `auto` first, so the probe below addresses each source the way a
    // download would. Probing a BEP 17 seed with a BEP 19 URL measures a 404
    // and reports a healthy mirror as broken, which is the failure this whole
    // entry is about. See `TODO/webseed.md`, T-004.
    let mut set = set;
    let styles = runtime.block_on(bit_cli_core::webseed::probe::resolve_auto_styles(
        &mut set, &info_hash,
    ));
    let set = set;
    // Sources are probed in parallel. Each probe is one request to a different
    // host, so they do not contend, and a real torrent carries enough of them
    // that doing this one at a time takes minutes: the Arch Linux ISO torrent
    // carries 468 web seeds.
    let results = runtime.block_on(async {
        let probe_all = async {
            let mut out: Vec<Option<bit_cli_core::webseed::probe::SourceTest>> =
                vec![None; set.bindings.len()];
            let mut workers = tokio::task::JoinSet::new();
            let mut next = 0usize;
            loop {
                while workers.len() < concurrency && next < set.bindings.len() {
                    let index = next;
                    next += 1;
                    let binding = set.bindings[index].clone();
                    let layout = layout.clone();
                    let info_hash = info_hash.clone();
                    let report_headers = report_headers.clone();
                    workers.spawn(async move {
                        (
                            index,
                            bit_cli_core::webseed::probe::test_source(
                                &binding,
                                &layout,
                                &info_hash,
                                head,
                                &report_headers,
                                redact,
                            )
                            .await,
                        )
                    });
                }
                match workers.join_next().await {
                    Some(Ok((index, result))) => out[index] = Some(result),
                    Some(Err(_)) => {}
                    None => break,
                }
            }
            out
        };
        match deadline {
            // `--timeout` bounds the whole command, not each probe. A source
            // that has not answered by then is reported as unfinished rather
            // than dropped, because a caller counting mirrors needs the count
            // to add up.
            Some(limit) => tokio::time::timeout(limit, probe_all)
                .await
                .unwrap_or_else(|_| vec![None; set.bindings.len()]),
            None => probe_all.await,
        }
    });

    let results: Vec<bit_cli_core::webseed::probe::SourceTest> = results
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            let mut test = result.unwrap_or_else(|| {
                bit_cli_core::webseed::probe::SourceTest::unfinished(
                    &set.bindings[index],
                    "the run reached --timeout before this source answered",
                )
            });
            // How the style was decided, which the probe itself has no way to
            // know: it is handed a binding whose style is already resolved.
            test.style_decided_by = styles
                .iter()
                .find(|decision| decision.index == test.index)
                .map(|decision| decision.decided_by);
            test
        })
        .collect();

    let report = TestReport {
        info_hash: meta.info_hash().hex(),
        name: layout.name.clone(),
        source_count: results.len(),
        usable: results.iter().filter(|r| r.ok).count(),
        unusable: results.iter().filter(|r| !r.ok).count(),
        sources: results,
    };
    for source in &report.sources {
        if let Some(error) = &source.error {
            renderer.warn(env, format!("{}: {error}", source.url));
        }
    }

    // A script asked to check its mirrors needs the exit code to say whether
    // they are usable. Every source failing is a different problem from one
    // failing, so the two get different codes.
    let code = match (report.usable, report.unusable) {
        (0, n) if n > 0 => ExitCode::NoUsableSources,
        (_, 0) => ExitCode::Success,
        _ if args.web_seeds.web_seed_require => ExitCode::NoUsableSources,
        _ => ExitCode::Success,
    };
    renderer.emit(env, "webseed_test", &report, || test_lines(&report))?;
    Ok(code)
}

fn test_lines(report: &TestReport) -> Vec<String> {
    let mut out = vec![
        field("torrent", &report.name),
        field("info hash", &report.info_hash),
        field("sources", report.source_count),
        field("usable", report.usable),
        field("unusable", report.unusable),
    ];
    for source in &report.sources {
        out.push(String::new());
        out.push(field("source", &source.url));
        out.push(field("  requested", &source.request_url));
        out.push(field("  scope", &source.scope));
        out.push(field(
            "  status",
            source
                .status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".into()),
        ));
        out.push(field("  ranges", source.range_support.as_str()));
        out.push(field(
            "  length",
            match (source.content_length, source.length_matches) {
                (Some(len), Some(true)) => format!("{} (matches the torrent)", format_size(len)),
                (Some(len), Some(false)) => format!(
                    "{} (the torrent says {})",
                    format_size(len),
                    format_size(source.expected_length)
                ),
                _ => "not reported".to_string(),
            },
        ));
        if let Some(resolved) = &source.resolved_url {
            out.push(field("  resolved to", resolved));
        }
        for hop in &source.redirects {
            out.push(field("  redirect", format!("{} -> {}", hop.status, hop.to)));
        }
        if let Some(server) = &source.server {
            out.push(field("  server", server));
        }
        // One line per header rather than a joined list: a value can carry a
        // comma of its own, and `x-cache: MISS, MISS` from a two hop chain is
        // one header with a comma in it. See `TODO/webseed.md`, T-254.
        for (name, value) in &source.headers {
            out.push(field(&format!("  {name}"), value));
        }
        out.push(field("  http", &source.http_version));
        if let Some(tls) = &source.tls {
            out.push(field(
                "  tls",
                format!("{} {}", tls.version, tls.cipher_suite),
            ));
            out.push(field(
                "  handshake",
                format!("connect {}ms, tls {}ms", tls.connect_ms, tls.handshake_ms),
            ));
            if let Some(alpn) = &tls.alpn {
                out.push(field("  alpn", alpn));
            }
        }
        out.push(field("  ttfb", format!("{}ms", source.ttfb_ms)));
        out.push(field("  total", format!("{}ms", source.total_ms)));
        if let Some(error) = &source.error {
            out.push(field("  error", error));
        }
    }
    out
}

/// What `bit-cli webseed probe` reports.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeReport {
    pub info_hash: String,
    pub name: String,
    pub duration_ms: u64,
    pub concurrency_sweep: Vec<usize>,
    pub sources: Vec<bit_cli_core::webseed::probe::SourceProbe>,
}

/// `bit-cli webseed probe`: measure latency and the concurrency curve.
pub fn probe(
    args: &WebseedProbeArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let (meta, layout, set) = resolve(
        &args.source.source,
        &args.web_seeds,
        &args.swarm,
        &args.page,
        global,
        env,
    )?;
    let duration = bit_cli_core::units::parse_duration(&args.duration)
        .map_err(|e| Error::usage(format!("--duration: {e}")))?;
    let sweep = parse_sweep(&args.concurrency_sweep)?;

    if global.dry_run {
        let report = serde_json::json!({
            "dry_run": true,
            "info_hash": meta.info_hash().hex(),
            "sources": set.bindings.iter().map(|b| &b.spec.url).collect::<Vec<_>>(),
            "concurrency_sweep": sweep,
            "duration_ms": duration.as_millis() as u64,
        });
        renderer.emit(env, "webseed_probe", &report, || {
            vec![
                field("dry run", "no requests will be made"),
                field("sources", set.bindings.len()),
                field("sweep", format!("{sweep:?}")),
            ]
        })?;
        return Ok(ExitCode::Success);
    }

    let runtime = runtime()?;
    let results = runtime.block_on(async {
        let mut out = Vec::with_capacity(set.bindings.len());
        for binding in &set.bindings {
            out.push(
                bit_cli_core::webseed::probe::probe_source(
                    binding,
                    &layout,
                    &meta.info_hash().hex(),
                    &sweep,
                    duration,
                )
                .await,
            );
        }
        out
    });

    let report = ProbeReport {
        info_hash: meta.info_hash().hex(),
        name: layout.name.clone(),
        duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        concurrency_sweep: sweep,
        sources: results,
    };
    let all_dead = report.sources.iter().all(|s| s.best_throughput == 0);
    renderer.emit(env, "webseed_probe", &report, || probe_lines(&report))?;
    Ok(match all_dead {
        true => ExitCode::NoUsableSources,
        false => ExitCode::Success,
    })
}

fn probe_lines(report: &ProbeReport) -> Vec<String> {
    let mut out = vec![
        field("torrent", &report.name),
        field("info hash", &report.info_hash),
        field(
            "per step",
            bit_cli_core::units::format_duration_ms(report.duration_ms),
        ),
    ];
    for source in &report.sources {
        out.push(String::new());
        out.push(field("source", &source.url));
        out.push(field("  scope", &source.scope));
        out.push(field("  chunk", format_size(source.chunk_size.0)));
        if let Some(error) = &source.error {
            out.push(field("  error", error));
            continue;
        }
        if let Some(best) = source.best_concurrency {
            out.push(field(
                "  best",
                format!("{} at concurrency {best}", source.best_throughput_human),
            ));
        }
        let rows: Vec<Vec<String>> = source
            .steps
            .iter()
            .map(|step| {
                vec![
                    step.concurrency.to_string(),
                    step.requests.to_string(),
                    step.errors.to_string(),
                    step.throughput_human.clone(),
                    format!("{}ms", step.p50_ms),
                    format!("{}ms", step.p90_ms),
                    format!("{}ms", step.p99_ms),
                    format!("{}ms", step.p999_ms),
                    format!("{}ms", step.max_ms),
                    format!("{}ms", step.ttfb_p50_ms),
                ]
            })
            .collect();
        out.extend(
            table(
                &[
                    "CONC", "REQS", "ERRS", "RATE", "P50", "P90", "P99", "P99.9", "MAX", "TTFB P50",
                ],
                &rows,
            )
            .into_iter()
            .map(|line| format!("  {line}")),
        );
    }
    out
}

/// Parse a `--concurrency-sweep` spec such as `1,2,4,8,16`.
fn parse_sweep(spec: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for term in spec.split(',') {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        let value: usize = term.parse().map_err(|_| {
            Error::usage(format!("--concurrency-sweep `{term}` is not a number"))
                .with("value", term.to_string())
        })?;
        if value == 0 {
            return Err(Error::usage("--concurrency-sweep cannot include zero"));
        }
        out.push(value);
    }
    if out.is_empty() {
        return Err(Error::usage(
            "--concurrency-sweep needs at least one concurrency",
        ));
    }
    Ok(out)
}

/// Run a `webseed` subcommand.
pub fn run(
    command: &WebseedCommand,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    match command {
        WebseedCommand::List(args) => list(args, global, renderer, env),
        WebseedCommand::Test(args) => test(args, global, renderer, env),
        WebseedCommand::Probe(args) => probe(args, global, renderer, env),
        WebseedCommand::Fetch(args) => fetch(args, global, renderer, env),
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{TorrentFixture, run_err, run_json, run_json_code, run_ok};
    use bit_cli_core::ExitCode;

    /// `webseed list --json` says what the window caches will hold, per source
    /// and in total, before anything is fetched. It is memory a run costs by
    /// construction and nothing said so. See `TODO/memory.md`, T-041.
    #[test]
    fn the_listing_carries_the_window_cache_budget() {
        let fixture = TorrentFixture::single_file();
        let doc = run_json(
            &[
                "webseed",
                "list",
                fixture.path_str(),
                // The fixture carries its own `url-list`, and this is about
                // the two named here.
                "--no-torrent-web-seed",
                "--web-seed",
                "https://a.example.com/",
                "--web-seed",
                "https://b.example.com/",
            ],
            fixture.dir(),
        );
        assert_eq!(doc["source_count"], 2);
        // The default chunk size is 4 MiB and four windows is the budget, so
        // each source is 16 MiB and the run is 32.
        let windows = doc["cache_windows"].as_u64().expect("a window count");
        let per_source = doc["sources"][0]["cache_budget"]["bytes"]
            .as_u64()
            .expect("a per-source budget");
        let total = doc["cache_budget_total"]["bytes"]
            .as_u64()
            .expect("a total");
        assert!(windows >= 2, "{doc}");
        assert_eq!(total, per_source * 2, "the total is the sum: {doc}");
        assert_eq!(
            doc["sources"][1]["cache_budget"]["bytes"].as_u64(),
            Some(per_source),
            "two sources at one chunk size have one budget"
        );

        let text = run_ok(
            &[
                "webseed",
                "list",
                fixture.path_str(),
                "--no-torrent-web-seed",
                "--web-seed",
                "https://a.example.com/",
            ],
            fixture.dir(),
        );
        assert!(text.contains("window cache"), "{text}");
    }

    /// And a chunk size the caller chose says so on stderr, which is the half
    /// a script that is not reading `--json` still sees. T-041.
    #[test]
    fn a_chunk_size_that_costs_a_gigabyte_warns() {
        let fixture = TorrentFixture::single_file();
        let mut args = vec![
            "webseed".to_string(),
            "list".to_string(),
            fixture.path_str().to_string(),
            "--no-torrent-web-seed".to_string(),
            "--web-seed-chunk-size".to_string(),
            "64MiB".to_string(),
        ];
        for index in 0..10 {
            args.push("--web-seed".to_string());
            args.push(format!("https://m{index}.example.com/"));
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let (mut env, captured) = crate::env::Env::test(&borrowed, fixture.dir());
        crate::run(&mut env);
        let said = captured.err();
        assert!(said.contains("window caches"), "{said}");
        assert!(said.contains("1.25 GiB"), "{said}");
        // Warnings never reach stdout, which is the rule the whole surface
        // keeps: stdout carries data only.
        assert!(
            !captured.out().contains("window caches"),
            "{}",
            captured.out()
        );
    }

    /// `TODO/bep-coverage.md`, T-103, and the half that costs bytes rather
    /// than legibility. `webseed list` exists to print the exact URL each file
    /// maps to, and this torrent's names are not UTF-8. Composing from the
    /// lossy decode produced a path of `%EF%BF%BD` runs, which is a 404 on
    /// every mirror there is, and which is not what the same run requested.
    #[test]
    fn a_url_is_composed_from_the_decoded_path_rather_than_the_lossy_one() {
        let fixture = TorrentFixture::names_that_are_not_utf8();
        let doc = run_json(
            &[
                "webseed",
                "list",
                fixture.path_str(),
                "--web-seed",
                "https://mirror.example.com/pub/",
            ],
            fixture.dir(),
        );
        let url = doc["sources"][0]["urls"][0]["url"].as_str().expect("a URL");
        assert_eq!(
            url,
            "https://mirror.example.com/pub/%E9%9F%B3%E6%A5%BD/%E6%9B%B2.bin"
        );
        assert!(
            !url.contains("%EF%BF%BD"),
            "the URL carries a replacement character: {url}"
        );
    }

    #[test]
    fn list_resolves_the_torrents_own_web_seed_by_default() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(&["webseed", "list", fixture.path_str()], fixture.dir());
        assert_eq!(doc["source_count"], 1);
        assert_eq!(doc["sources"][0]["origin"], "torrent_url_list");
        assert_eq!(doc["sources"][0]["url"], "https://mirror.example.com/pub/");
        assert_eq!(doc["complete"], true);
    }

    /// `TODO/metainfo.md` T-171. The same fixture `info` reads, resolved into
    /// sources: two of them, one per key, and the `httpseeds` one still
    /// carrying BEP 17 style. Which key a URL came from is the only signal for
    /// style that needs no network round trip, so the two lists must not be
    /// merged on the way through.
    #[test]
    fn a_web_seed_key_written_as_a_string_still_resolves_to_a_source() {
        let fixture = TorrentFixture::web_seed_keys_as_strings();
        let doc = run_json(&["webseed", "list", fixture.path_str()], fixture.dir());
        assert_eq!(doc["source_count"], 2);

        let sources = doc["sources"].as_array().unwrap();
        let get_right = sources
            .iter()
            .find(|s| s["origin"] == "torrent_url_list")
            .expect("a source from url-list");
        assert_eq!(get_right["url"], "https://getright.example.com/pub/");
        assert_eq!(
            get_right["style"], "getright",
            "`url-list` is BEP 19 by the key it came from, which is what BEP 19 specifies"
        );

        let hoffman = sources
            .iter()
            .find(|s| s["origin"] == "torrent_httpseeds")
            .expect("a source from httpseeds");
        assert_eq!(hoffman["url"], "https://hoffman.example.com/");
        assert_eq!(hoffman["style"], "hoffman");
    }

    /// `--web-seed-style` overrides what the key says, for both keys.
    ///
    /// T-254. The headers that say whether a request was served from cache are
    /// received on every probe and were dropped on every probe.
    #[test]
    fn a_cdn_response_reports_the_headers_that_say_it_was_cached() {
        let fixture = crate::test_support::TorrentFixture::single_file();
        fixture.place(&fixture.payload_dir(), &[]);
        let server = crate::test_support::FileServer::start_cdn(fixture.payload_dir());
        let doc = crate::test_support::run_json(
            &[
                "webseed",
                "test",
                fixture.path_str(),
                "--web-seed",
                &server.base,
                "--no-torrent-web-seed",
            ],
            fixture.dir(),
        );
        let headers = &doc["sources"][0]["headers"];
        assert_eq!(headers["age"], "41", "{doc}");
        assert_eq!(headers["x-cache"], "HIT", "{doc}");
        assert_eq!(headers["cache-control"], "public, max-age=3600", "{doc}");
        assert!(headers["etag"].is_string(), "{doc}");

        // The two the fixture also sends and the allowlist does not carry.
        assert!(headers["x-cache-hits"].is_null(), "{doc}");
        assert!(headers["x-frame-options"].is_null(), "{doc}");

        // `server` is not in the map: it has its own field, and moving it
        // would break a reader. The loopback fixture sends no `Server` header
        // at all, so that field is absent here rather than a string, which is
        // exactly what makes the absence from the map worth asserting.
        assert!(headers["server"].is_null(), "{doc}");

        let text = crate::test_support::run_ok(
            &[
                "webseed",
                "test",
                fixture.path_str(),
                "--web-seed",
                &server.base,
                "--no-torrent-web-seed",
            ],
            fixture.dir(),
        );
        assert!(text.contains("x-cache"), "{text}");
        assert!(text.contains("HIT"), "{text}");
    }

    /// A plain origin carries none of them and says nothing about them: the
    /// field is absent rather than an empty object.
    #[test]
    fn a_plain_origin_reports_no_headers_and_no_empty_field() {
        let fixture = crate::test_support::TorrentFixture::single_file();
        fixture.place(&fixture.payload_dir(), &[]);
        let server = crate::test_support::FileServer::start(fixture.payload_dir());
        let doc = crate::test_support::run_json(
            &[
                "webseed",
                "test",
                fixture.path_str(),
                "--web-seed",
                &server.base,
                "--no-torrent-web-seed",
            ],
            fixture.dir(),
        );
        assert!(doc["sources"][0]["headers"].is_null(), "{doc}");
        assert!(doc["sources"][0]["server"].is_null(), "{doc}");
    }

    /// `--web-seed-report-header` reaches the report, and reaches only what it
    /// names.
    #[test]
    fn a_header_asked_for_by_name_reaches_the_report() {
        let fixture = crate::test_support::TorrentFixture::single_file();
        fixture.place(&fixture.payload_dir(), &[]);
        let server = crate::test_support::FileServer::start_cdn(fixture.payload_dir());
        let doc = crate::test_support::run_json(
            &[
                "webseed",
                "test",
                fixture.path_str(),
                "--web-seed",
                &server.base,
                "--no-torrent-web-seed",
                "--web-seed-report-header",
                "X-Cache-Hits",
            ],
            fixture.dir(),
        );
        let headers = &doc["sources"][0]["headers"];
        assert_eq!(headers["x-cache-hits"], "12", "{doc}");
        assert!(headers["x-frame-options"].is_null(), "{doc}");
    }

    /// A caller who names a style has said something about the server that the
    /// metainfo cannot, and before this the `httpseeds` keying overwrote it.
    /// See `TODO/webseed.md`, T-004.
    #[test]
    fn a_declared_style_overrides_the_metainfo_key_for_both_lists() {
        let fixture = TorrentFixture::web_seed_keys_as_strings();
        let doc = run_json(
            &[
                "webseed",
                "list",
                fixture.path_str(),
                "--web-seed-style",
                "getright",
            ],
            fixture.dir(),
        );
        for source in doc["sources"].as_array().unwrap() {
            assert_eq!(source["style"], "getright", "{source}");
        }
    }

    #[test]
    fn auto_composition_appends_the_name_and_the_path() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(&["webseed", "list", fixture.path_str()], fixture.dir());
        let urls = doc["sources"][0]["urls"].as_array().unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(
            urls[0]["url"],
            "https://mirror.example.com/pub/album/disc%201/a.flac"
        );
        assert_eq!(
            urls[1]["url"],
            "https://mirror.example.com/pub/album/notes.nfo"
        );
    }

    #[test]
    fn prefix_composition_leaves_the_torrent_name_out() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed-mode",
                "prefix",
                "--web-seed",
                "https://m.example.com/pub/",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        let urls = doc["sources"][0]["urls"].as_array().unwrap();
        assert_eq!(urls[0]["url"], "https://m.example.com/pub/disc%201/a.flac");
    }

    #[test]
    fn exact_composition_needs_a_single_file_scope_and_says_so() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed-exact",
                "https://cdn.example.com/blob",
                fixture.path_str(),
            ],
            fixture.dir(),
            ExitCode::Binding,
        );
        assert!(err.contains("selects 2 files"), "{err}");

        let doc = run_json(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed-for",
                "1=https://cdn.example.com/blob",
                "--web-seed-mode",
                "exact",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(
            doc["sources"][0]["urls"][0]["url"],
            "https://cdn.example.com/blob"
        );
    }

    #[test]
    fn template_composition_reports_that_urls_are_per_request() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed",
                "https://e.example.com/chunks/{piece}.bin",
                "--web-seed-mode",
                "template",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["sources"][0]["mode"], "template");
        assert!(doc["sources"][0]["urls"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_scoped_source_reports_what_it_covers_and_what_it_does_not() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed-for",
                "piece:0=https://a.example.com/",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["complete"], false);
        assert_eq!(doc["uncovered_pieces"], serde_json::json!([1]));
        assert_eq!(doc["sources"][0]["whole_pieces"], 1);
    }

    #[test]
    fn two_partial_sources_can_add_up_to_complete_coverage() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed-for",
                "piece:0=https://a.example.com/",
                "--web-seed-for",
                "piece:1=https://b.example.com/",
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["complete"], true);
        assert_eq!(doc["uncovered"]["bytes"], 0);
    }

    #[test]
    fn web_seed_require_turns_a_gap_into_a_failure() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed-require",
                "--web-seed-for",
                "piece:0=https://a.example.com/",
                fixture.path_str(),
            ],
            fixture.dir(),
            ExitCode::CoverageGap,
        );
        assert!(err.contains("piece(s) 1"), "{err}");
    }

    #[test]
    fn the_text_form_prints_the_exact_urls() {
        let fixture = TorrentFixture::multi_file();
        let out = run_ok(&["webseed", "list", fixture.path_str()], fixture.dir());
        assert!(
            out.contains("https://mirror.example.com/pub/album/disc%201/a.flac"),
            "{out}"
        );
        assert!(out.contains("coverage"), "{out}");
    }

    #[test]
    fn no_web_seed_leaves_nothing_to_list() {
        let fixture = TorrentFixture::multi_file();
        run_err(
            &["webseed", "list", "--no-web-seed", fixture.path_str()],
            fixture.dir(),
            ExitCode::NoUsableSources,
        );
    }

    #[test]
    fn a_selector_matching_nothing_is_a_binding_error() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed-for",
                "*.iso=https://a.example.com/",
                fixture.path_str(),
            ],
            fixture.dir(),
            ExitCode::Binding,
        );
        assert!(err.contains("matched no bytes"), "{err}");
    }

    #[test]
    fn a_binding_table_drives_the_same_listing() {
        let fixture = TorrentFixture::multi_file();
        let table = fixture.root.join("seeds.toml");
        std::fs::write(
            &table,
            "[[source]]\nurl = \"https://a.example.com/\"\nscope = \"file:0\"\nmode = \"prefix\"\n\n\
             [[source]]\nurl = \"https://b.example.com/\"\nscope = \"file:1\"\nmode = \"prefix\"\n",
        )
        .unwrap();
        let doc = run_json(
            &[
                "webseed",
                "list",
                "--no-torrent-web-seed",
                "--web-seed-config",
                table.to_str().unwrap(),
                fixture.path_str(),
            ],
            fixture.dir(),
        );
        assert_eq!(doc["source_count"], 2);
        assert_eq!(
            doc["sources"][0]["urls"][0]["url"],
            "https://a.example.com/disc%201/a.flac"
        );
        assert_eq!(
            doc["sources"][1]["urls"][0]["url"],
            "https://b.example.com/notes.nfo"
        );
    }

    #[test]
    fn fetch_needs_a_range_selector() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &["webseed", "fetch", fixture.path_str()],
            fixture.dir(),
            ExitCode::Usage,
        );
        assert!(err.contains("--piece"), "{err}");
    }

    #[test]
    fn fetch_refuses_a_piece_index_past_the_end() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &["webseed", "fetch", "--piece", "99", fixture.path_str()],
            fixture.dir(),
            ExitCode::Usage,
        );
        assert!(err.contains("out of range"), "{err}");
    }

    #[test]
    fn fetch_refuses_a_url_that_was_not_declared() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &[
                "webseed",
                "fetch",
                "--piece",
                "0",
                "--url",
                "https://not-declared.example.com/",
                fixture.path_str(),
            ],
            fixture.dir(),
            ExitCode::Usage,
        );
        assert!(err.contains("not one of the declared sources"), "{err}");
    }

    /// The acceptance for `TODO/webseed.md` T-004: a BEP 17 seed named on the
    /// command line, with no `--web-seed-style`, is reported as `hoffman`.
    ///
    /// The style is decided before the probe runs, so the probe addresses the
    /// source the way a download would. Addressing a BEP 17 seed with a BEP 19
    /// URL measures a refusal and reports a healthy mirror as broken, which is
    /// the failure this entry is about.
    #[test]
    fn a_command_line_hoffman_source_is_reported_as_hoffman_without_the_flag() {
        let fixture = TorrentFixture::single_file();
        let server = crate::test_support::FileServer::start_hoffman(fixture.payload_dir());
        let source = format!("{}payload.bin", server.base);
        let report = run_json(
            &[
                "webseed",
                "test",
                fixture.path_str(),
                "--no-torrent-web-seed",
                "--web-seed",
                &source,
                "--web-seed-mode",
                "exact",
            ],
            fixture.dir(),
        );
        assert_eq!(report["source_count"], 1);
        let probed = &report["sources"][0];
        assert_eq!(probed["style"], "hoffman", "{probed}");
        assert_eq!(probed["style_decided_by"], "probe", "{probed}");
        assert_eq!(probed["ok"], true, "{probed}");
        assert_eq!(report["usable"], 1);
    }

    /// The same command against an ordinary mirror keeps BEP 19, so the
    /// detector is not simply answering "hoffman".
    #[test]
    fn a_command_line_getright_source_is_reported_as_getright() {
        let fixture = TorrentFixture::single_file();
        let server = crate::test_support::FileServer::start(fixture.payload_dir());
        let source = format!("{}payload.bin", server.base);
        let report = run_json(
            &[
                "webseed",
                "test",
                fixture.path_str(),
                "--no-torrent-web-seed",
                "--web-seed",
                &source,
                "--web-seed-mode",
                "exact",
            ],
            fixture.dir(),
        );
        let probed = &report["sources"][0];
        assert_eq!(probed["style"], "getright", "{probed}");
        assert_eq!(probed["style_decided_by"], "probe", "{probed}");
        assert_eq!(probed["ok"], true, "{probed}");
    }

    /// `--web-seed-style` still decides, and costs no probe.
    #[test]
    fn a_declared_style_is_taken_as_given() {
        let fixture = TorrentFixture::single_file();
        let server = crate::test_support::FileServer::start_hoffman(fixture.payload_dir());
        let source = format!("{}payload.bin", server.base);
        let report = run_json(
            &[
                "webseed",
                "test",
                fixture.path_str(),
                "--no-torrent-web-seed",
                "--web-seed",
                &source,
                "--web-seed-mode",
                "exact",
                "--web-seed-style",
                "hoffman",
            ],
            fixture.dir(),
        );
        let probed = &report["sources"][0];
        assert_eq!(probed["style"], "hoffman");
        assert_eq!(probed["style_decided_by"], "declared", "{probed}");
        assert_eq!(probed["ok"], true, "{probed}");
    }

    /// `--timeout` bounds the whole command, and every declared source is
    /// still reported when it fires.
    ///
    /// A caller counting its mirrors needs the count to add up: a source whose
    /// probe was cut short has to appear as unusable, not vanish. The URLs
    /// here point at a port nothing answers on, so the deadline is what ends
    /// the run.
    #[test]
    fn a_timeout_ends_the_command_and_every_source_is_still_reported() {
        let fixture = TorrentFixture::multi_file();
        let report = run_json_code(
            &[
                "webseed",
                "test",
                fixture.path_str(),
                "--no-torrent-web-seed",
                "--web-seed",
                "http://127.0.0.1:1/",
                "--web-seed",
                "http://127.0.0.1:2/",
                "--web-seed",
                "http://127.0.0.1:3/",
                "--concurrency",
                "8",
                "--timeout",
                "1s",
            ],
            fixture.dir(),
            ExitCode::NoUsableSources,
        );
        assert_eq!(report["source_count"], 3);
        assert_eq!(report["usable"], 0);
        assert_eq!(report["unusable"], 3);
        let sources = report["sources"].as_array().unwrap();
        assert_eq!(sources.len(), 3, "no source is dropped by the deadline");
        for (index, source) in sources.iter().enumerate() {
            assert_eq!(source["ok"], false);
            assert!(source["error"].is_string(), "{source}");
            assert_eq!(
                source["index"], index,
                "the report keeps the order the sources were declared in"
            );
        }
    }
}
