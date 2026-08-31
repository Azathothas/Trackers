//! Turning the `--web-seed*` flags into source specs.
//!
//! Sources arrive from six places: the command line, a URL list file, a URL
//! list fetched over HTTP, a binding table, the torrent's own `url-list`
//! (BEP 19), and its `httpseeds` (BEP 17). All six are merged here, and each
//! keeps the origin it came from so `--json` can report which set is which and
//! `--no-torrent-web-seed` can drop exactly one of them.
//!
//! Nothing in this module touches the torrent. Web seeds attach at runtime;
//! the `.torrent` is never rewritten, never re-hashed, and the info hash never
//! changes.

use std::collections::BTreeMap;

use bit_cli_core::error::{Context, Error, Result, from_io};
use bit_cli_core::metalink::MetalinkFile;
use bit_cli_core::torrent::Metainfo;
use bit_cli_core::units::{parse_duration_ms, parse_rate, parse_size};
use bit_cli_core::webseed::binding::{Auth, Origin, SourceLimits, SourceSpec, StatusSet};
use bit_cli_core::webseed::table::{Table, parse_url_list};
use bit_cli_core::webseed::{Mode, Scope};

use crate::cli::WebSeedArgs;
use crate::env::Env;

/// The most connections one source is presented over.
///
/// Each is a peer slot in the session and a bridge task here, and the measured
/// gain flattens between two and four (`TODO/webseed.md`, T-009). The cap is
/// well past where the curve stops rising, so it bounds a typo rather than a
/// legitimate setting.
const MAX_CONNECTIONS: usize = 32;

/// Split an optional `<INFOHASH>:` qualifier off the front of a selector.
///
/// A binding applies to every torrent in the invocation, which is wrong when
/// the same file sits at a different index in each. `-j 2` over two torrents
/// that share a file needs to say which one it means, and the info hash is the
/// only name a torrent has before its metadata is read.
///
/// The rule is mechanical: exactly forty hexadecimal characters followed by a
/// colon is an info hash, anything else is part of the selector. No scope
/// keyword is forty hex characters, so the only thing that could collide is a
/// torrent whose file path begins that way, and `file:N` selects that file
/// instead. See `TODO/multi-source.md`, T-133.
fn split_torrent(selector: &str) -> (Option<&str>, &str) {
    let Some((head, rest)) = selector.split_once(':') else {
        return (None, selector);
    };
    match head.len() == 40 && head.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => (Some(head), rest),
        false => (None, selector),
    }
}

/// Every info hash a `--web-seed-for` binding names, with the binding it came
/// from.
///
/// [`collect`] runs once per torrent and drops the bindings that are not for
/// that one, so a hash naming no torrent in the invocation would be silently
/// ignored. The caller checks this against the torrents it is about to add and
/// fails instead, because a typo in a forty character hash is the likeliest
/// mistake this flag has.
pub fn qualified_torrents(args: &WebSeedArgs) -> Vec<(String, String)> {
    args.web_seed_for
        .iter()
        .filter_map(|binding| {
            let selector = binding.split_once('=')?.0;
            let hash = split_torrent(selector).0?;
            Some((binding.clone(), hash.to_ascii_lowercase()))
        })
        .collect()
}

/// Parse one of the two status policy flags.
fn status_set(value: &Option<String>, flag: &str) -> Result<StatusSet> {
    match value {
        None => Ok(StatusSet::default()),
        Some(text) => StatusSet::parse(text)
            .map_err(|e| Error::usage(format!("{flag}: {e}")).with("value", text.clone())),
    }
}

/// Everything the flags say about how CLI-supplied sources behave.
struct Shared {
    mode: Mode,
    template: Option<String>,
    scope: Scope,
    limits: SourceLimits,
    headers: BTreeMap<String, String>,
    user_agent: Option<String>,
    auth: Auth,
    priority: i32,
    style: bit_cli_core::webseed::Style,
}

impl Shared {
    fn from(args: &WebSeedArgs) -> Result<Self> {
        let base = SourceLimits::default();
        let size = |value: &Option<String>, fallback: u64, what: &str| -> Result<u64> {
            match value {
                None => Ok(fallback),
                Some(text) => parse_size(text).map_err(|e| {
                    Error::usage(format!("--{what}: {e}")).with("value", text.clone())
                }),
            }
        };
        let duration = |value: &Option<String>, fallback: u64, what: &str| -> Result<u64> {
            match value {
                None => Ok(fallback),
                Some(text) => parse_duration_ms(text).map_err(|e| {
                    Error::usage(format!("--{what}: {e}")).with("value", text.clone())
                }),
            }
        };

        // The piece and byte restrictions are two spellings of one scope, so
        // asking for both would need a rule about which wins. There is no
        // useful answer, so it is a usage error instead.
        let scope = match (&args.web_seed_pieces, &args.web_seed_bytes) {
            (Some(_), Some(_)) => {
                return Err(Error::usage(
                    "--web-seed-pieces and --web-seed-bytes both restrict the same sources; use one, or --web-seed-for for per-source scopes",
                ));
            }
            (Some(pieces), None) => {
                Scope::parse(&format!("piece:{}", pieces.trim_start_matches("piece:")))?
            }
            (None, Some(bytes)) => {
                Scope::parse(&format!("byte:{}", bytes.trim_start_matches("byte:")))?
            }
            (None, None) => Scope::all(),
        };

        let mut headers = BTreeMap::new();
        for raw in &args.web_seed_header {
            let (name, value) = raw.split_once(':').ok_or_else(|| {
                Error::usage(format!("--web-seed-header `{raw}` is not `Name: value`"))
                    .with("value", raw.clone())
            })?;
            headers.insert(name.trim().to_string(), value.trim().to_string());
        }

        let shared = Self {
            mode: args.web_seed_mode.into(),
            template: args.web_seed_template.clone(),
            scope,
            limits: SourceLimits {
                concurrency: aria2_concurrency(args).unwrap_or(base.concurrency).max(1),
                connections: args
                    .web_seed_connections
                    .unwrap_or(base.connections)
                    .clamp(1, MAX_CONNECTIONS),
                // `-k` is a floor rather than a value, so the larger of the
                // two wins. See `TODO/performance.md`, T-033.
                chunk_size: size(
                    &args.web_seed_chunk_size,
                    base.chunk_size,
                    "web-seed-chunk-size",
                )?
                .max(size(&args.min_split_size, 0, "min-split-size")?)
                .max(1),
                timeout_ms: duration(&args.web_seed_timeout, base.timeout_ms, "web-seed-timeout")?,
                connect_timeout_ms: duration(
                    &args.web_seed_connect_timeout,
                    base.connect_timeout_ms,
                    "web-seed-connect-timeout",
                )?,
                retries: args.web_seed_retries.unwrap_or(base.retries),
                max_errors: args.web_seed_max_errors.unwrap_or(base.max_errors).max(1),
                cooldown_ms: duration(
                    &args.web_seed_cooldown,
                    base.cooldown_ms,
                    "web-seed-cooldown",
                )?,
                rate_limit: match &args.web_seed_speed_limit {
                    None => None,
                    Some(text) => Some(parse_rate(text).map_err(|e| {
                        Error::usage(format!("--web-seed-speed-limit: {e}"))
                            .with("value", text.clone())
                    })?),
                },
                retry_status: status_set(&args.web_seed_retry_status, "--web-seed-retry-status")?,
                fatal_status: status_set(&args.web_seed_fatal_status, "--web-seed-fatal-status")?,
            },
            headers,
            user_agent: args.web_seed_user_agent.clone(),
            auth: match &args.web_seed_auth {
                None => Auth::None,
                Some(spec) => Auth::parse(spec)?,
            },
            priority: args.web_seed_priority.unwrap_or(0),
            style: args.web_seed_style.into(),
        };
        shared.limits.check_status_policy()?;
        Ok(shared)
    }

    fn spec(&self, url: String, origin: Origin, scope: Scope, mode: Mode) -> SourceSpec {
        SourceSpec {
            url,
            scope,
            mode,
            template: self.template.clone(),
            style: self.style,
            priority: self.priority,
            headers: self.headers.clone(),
            user_agent: self.user_agent.clone(),
            auth: self.auth.clone(),
            limits: self.limits.clone(),
            origin,
        }
    }
}

/// Concurrent ranged requests per source, from whichever of the three
/// spellings were given.
///
/// `--web-seed-concurrency`, `-x/--max-connection-per-server` and
/// `-s/--split` are one setting here and two in `aria2`, which splits a file
/// into `-s` ranges and caps per-server connections at `-x` separately. There
/// is one knob to point them at, so **the largest given wins** rather than the
/// product: a script passing `-x 4 -s 16` asks for sixteen and gets sixteen,
/// not sixty-four.
///
/// See `TODO/performance.md`, T-033, and `docs/flags.md` for why an `aria2`
/// letter is never given a different meaning here.
fn aria2_concurrency(args: &WebSeedArgs) -> Option<usize> {
    [
        args.web_seed_concurrency,
        args.max_connection_per_server,
        args.split,
    ]
    .into_iter()
    .flatten()
    .max()
}

/// What to say about the `aria2` aliases, if anything.
///
/// Two things, and both exist because the mapping is close and not exact.
/// `-x` caps per source here and per server in `aria2`, which differ when two
/// sources share a host; and `-x` and `-s` are one knob here and two there.
/// `docs/flags.md` forbids giving an `aria2` letter a different meaning, and
/// stating the difference is what satisfies that rule rather than refusing the
/// alias. The operator ruled on 2026-08-24: take all three, and warn.
pub fn aria2_notes(args: &WebSeedArgs) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(n) = args.max_connection_per_server {
        out.push(format!(
            "-x caps concurrent requests per source, not per server: -x {n} with two sources on one host is {} requests to that host. --web-seed-max-total is the run-wide cap.",
            n * 2
        ));
    }
    if let (Some(x), Some(s)) = (args.max_connection_per_server, args.split)
        && x != s
    {
        out.push(format!(
            "-x and -s are one setting here, so -x {x} -s {s} is {} concurrent requests per source rather than {}.",
            x.max(s),
            x * s
        ));
    }
    out
}

/// Build every source for one torrent, in priority-tie order.
///
/// `fetch_list` fetches a `--web-seed-list-url`. It is a parameter so the
/// assembly is testable without a network.
pub fn collect(
    args: &WebSeedArgs,
    meta: Option<&Metainfo>,
    metalink: Option<&MetalinkFile>,
    env: &Env,
    fetch_list: impl Fn(&str) -> Result<String>,
) -> Result<Vec<SourceSpec>> {
    if args.no_web_seed {
        return Ok(Vec::new());
    }
    let shared = Shared::from(args)?;
    let mut specs = Vec::new();

    // The torrent's own sources come first, so a caller-supplied source with
    // an equal priority is tried after them only if it was written later. The
    // caller controls that with --web-seed-priority.
    //
    // A Metalink's mirrors are declared the same way and are dropped by the
    // same flag. Both are "the sources that came with the source document
    // rather than from you", and a Metalink whose mirrors were ignored is a
    // Metalink with nothing left in it but a torrent URL. Mirrors arrive in
    // the document's own preferred order, and that order is preserved:
    // `--web-seed-priority` is one number for every CLI source, so the
    // document's ranking survives only as position.
    if !args.no_torrent_web_seed {
        if let Some(file) = metalink {
            // A Metalink `<url>` is the whole resource, never a directory to
            // append a name to, so the composition is `exact` and not BEP 19's
            // `auto`. `exact` on a multi-file torrent is a binding error
            // unless the scope resolves to one file, so the scope is the file
            // the document was attributed to. A document that could not be
            // attributed to one registers nothing rather than a source whose
            // bytes belong to a piece range nobody knows.
            if let Some(scope) = metalink_scope(file, meta)? {
                for mirror in file.mirrors_by_priority() {
                    specs.push(shared.spec(
                        mirror.url.clone(),
                        Origin::Metalink,
                        scope.clone(),
                        Mode::Exact,
                    ));
                }
            }
        }
        if let Some(meta) = meta {
            // Which key a URL came from **is** its style: BEP 19 specifies
            // `url-list` and BEP 17 specifies `httpseeds`. That is the one
            // signal for style that needs no network round trip, so it is
            // taken here and the two lists are never merged. An explicit
            // `--web-seed-style` still wins, because a caller who named a
            // style has said something about the server the metainfo cannot.
            // See `TODO/webseed.md`, T-004.
            let declared = shared.style != bit_cli_core::webseed::Style::Auto;
            for url in meta.url_list() {
                let mut spec = shared.spec(url, Origin::TorrentUrlList, Scope::all(), Mode::Auto);
                if !declared {
                    spec.style = bit_cli_core::webseed::Style::GetRight;
                }
                specs.push(spec);
            }
            for url in meta.http_seeds() {
                let mut spec = shared.spec(url, Origin::TorrentHttpSeeds, Scope::all(), Mode::Auto);
                if !declared {
                    spec.style = bit_cli_core::webseed::Style::Hoffman;
                }
                specs.push(spec);
            }
        }
    }

    for url in &args.web_seed {
        specs.push(shared.spec(
            url.clone(),
            Origin::CommandLine,
            shared.scope.clone(),
            shared.mode,
        ));
    }
    for url in &args.web_seed_exact {
        specs.push(shared.spec(
            url.clone(),
            Origin::CommandLine,
            shared.scope.clone(),
            Mode::Exact,
        ));
    }
    for binding in &args.web_seed_for {
        let (selector, url) = binding.split_once('=').ok_or_else(|| {
            Error::usage(format!("--web-seed-for `{binding}` is not `SELECTOR=URL`"))
                .with("value", binding.clone())
        })?;
        let (wanted, selector) = split_torrent(selector);
        if let Some(wanted) = wanted {
            let Some(meta) = meta else {
                return Err(Error::usage(format!(
                    "--web-seed-for `{binding}` names a torrent by info hash, and this source's info hash is not known until its metadata resolves. Pass the .torrent, or drop the `{wanted}:` prefix to bind every torrent in the invocation."
                ))
                .with("value", binding.clone()));
            };
            if !meta.info_hash().hex().eq_ignore_ascii_case(wanted) {
                continue;
            }
        }
        let scope =
            Scope::parse(selector).with_context(|| format!("--web-seed-for `{binding}`"))?;
        specs.push(shared.spec(url.to_string(), Origin::CommandLine, scope, shared.mode));
    }

    for path in &args.web_seed_file {
        let path = env.resolve(path);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| from_io(e, format!("cannot read {}", path.display())))?;
        for url in parse_url_list(&text) {
            specs.push(shared.spec(url, Origin::File, shared.scope.clone(), shared.mode));
        }
    }
    for url in &args.web_seed_list_url {
        let text = fetch_list(url)?;
        for entry in parse_url_list(&text) {
            specs.push(shared.spec(entry, Origin::ListUrl, shared.scope.clone(), shared.mode));
        }
    }

    // The binding table comes last so its per-source settings are not
    // overwritten by the shared flags, which is the whole reason to use one.
    for path in &args.web_seed_config {
        let path = env.resolve(path);
        let table = Table::load(&path)?;
        specs.extend(
            table.into_specs(Origin::Config, meta.map(|m| m.info_hash().hex()).as_deref())?,
        );
    }

    if specs.is_empty() && args.web_seed_only {
        return Err(Error::no_usable_sources(
            "--web-seed-only was given but no web seed sources were declared",
        ));
    }
    Ok(specs)
}

/// The scope a Metalink's mirrors bind to, or `None` when they cannot bind.
///
/// A Metalink entry describes one file. On a single-file torrent that is the
/// whole payload. On a multi-file one it is whichever file the entry was
/// attributed to, and if it could not be attributed to exactly one then a
/// mirror serving it covers a byte range nobody has identified. Registering it
/// anyway would either be a binding error or, worse, an accepted source
/// serving one file's bytes into another file's pieces.
fn metalink_scope(file: &MetalinkFile, meta: Option<&Metainfo>) -> Result<Option<Scope>> {
    let Some(meta) = meta else {
        return Ok(Some(Scope::all()));
    };
    let layout = meta.layout();
    if layout.files.len() == 1 {
        return Ok(Some(Scope::all()));
    }
    match file.agreement(&layout).file_index {
        Some(index) => Scope::parse(&format!("file:{index}")).map(Some),
        None => Ok(None),
    }
}

/// Refuse to fetch a list over HTTP.
///
/// Commands that must not touch the network pass this, so a list URL on one of
/// them fails clearly instead of quietly reaching out. It backs both
/// `--web-seed-list-url` and `--tracker-list-url`, so the message names the URL
/// rather than a flag.
pub fn no_network(url: &str) -> Result<String> {
    Err(Error::usage(format!(
        "fetching the list at {url} needs the network, and this command does not use it"
    ))
    .with("url", url.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    use crate::cli::{Cli, Command};

    fn args(extra: &[&str]) -> WebSeedArgs {
        let mut full = vec!["bit-cli", "webseed", "list"];
        full.extend_from_slice(extra);
        full.push("x.torrent");
        let cli = Cli::try_parse_from(full).unwrap();
        let Some(Command::Webseed(crate::cli::WebseedCommand::List(list))) = cli.command else {
            panic!("expected webseed list");
        };
        list.web_seeds
    }

    fn env() -> Env {
        Env::test(&[], "/w").0
    }

    fn collect_ok(extra: &[&str]) -> Vec<SourceSpec> {
        collect(&args(extra), None, None, &env(), no_network).unwrap()
    }

    /// T-033, ruled on 2026-08-24. Each of the three `aria2` spellings reaches
    /// the knob it names.
    #[test]
    fn the_aria2_aliases_reach_the_flags_they_name() {
        let source = ["--web-seed", "https://a.example.com/pub/"];
        let with = |extra: &[&str]| {
            let mut full = source.to_vec();
            full.extend_from_slice(extra);
            collect_ok(&full)[0].limits.clone()
        };

        assert_eq!(with(&["-x", "8"]).concurrency, 8);
        assert_eq!(with(&["--max-connection-per-server", "8"]).concurrency, 8);
        assert_eq!(with(&["-s", "8"]).concurrency, 8);
        assert_eq!(with(&["--split", "8"]).concurrency, 8);
        // `-k` is a floor, so it raises the default and never lowers it. The
        // default is 4 MiB, which is what the second line holds.
        let default = with(&[]).chunk_size;
        assert_eq!(with(&["-k", "8MiB"]).chunk_size, 8 * 1024 * 1024);
        assert_eq!(
            with(&["--min-split-size", "8MiB"]).chunk_size,
            8 * 1024 * 1024
        );
        assert_eq!(with(&["-k", "1MiB"]).chunk_size, default);
    }

    /// `-x` and `-s` are one knob here and two in `aria2`, so a script passing
    /// both gets the larger rather than the product. Sixty-four concurrent
    /// requests where sixteen were asked for is the failure this prevents.
    #[test]
    fn passing_both_aria2_spellings_is_not_multiplied() {
        let limits = collect_ok(&[
            "--web-seed",
            "https://a.example.com/pub/",
            "-x",
            "4",
            "-s",
            "16",
        ])[0]
            .limits
            .clone();
        assert_eq!(limits.concurrency, 16);
    }

    /// `-k` is a floor, so the larger of it and `--web-seed-chunk-size` wins
    /// whichever way round they are given.
    #[test]
    fn min_split_size_is_a_floor_under_the_chunk_size() {
        let both = |chunk: &str, floor: &str| {
            collect_ok(&[
                "--web-seed",
                "https://a.example.com/pub/",
                "--web-seed-chunk-size",
                chunk,
                "-k",
                floor,
            ])[0]
                .limits
                .chunk_size
        };
        assert_eq!(both("1MiB", "4MiB"), 4 * 1024 * 1024);
        assert_eq!(both("4MiB", "1MiB"), 4 * 1024 * 1024);
    }

    /// The warning is what `docs/flags.md`'s rule asks for in place of
    /// refusing the alias: the difference is stated rather than hidden.
    #[test]
    fn the_aliases_warn_about_what_they_do_not_mean() {
        assert!(aria2_notes(&args(&[])).is_empty());

        let notes = aria2_notes(&args(&["-x", "4"]));
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("per source, not per server"), "{notes:?}");

        // Both given and equal is the common migrating case and says nothing
        // extra: the number a script asked for is the number it gets.
        let notes = aria2_notes(&args(&["-x", "8", "-s", "8"]));
        assert_eq!(notes.len(), 1, "{notes:?}");

        let notes = aria2_notes(&args(&["-x", "4", "-s", "16"]));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[1].contains("16 concurrent requests"), "{notes:?}");
        assert!(notes[1].contains("rather than 64"), "{notes:?}");

        // `-s` alone says nothing: it means here what it means there, up to
        // the one knob, and there is no per-server reading of it to correct.
        assert!(aria2_notes(&args(&["-s", "16"])).is_empty());
    }

    #[test]
    fn a_plain_web_seed_becomes_one_source() {
        let specs = collect_ok(&["--web-seed", "https://a.example.com/pub/"]);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].url, "https://a.example.com/pub/");
        assert_eq!(specs[0].mode, Mode::Auto);
        assert_eq!(specs[0].origin, Origin::CommandLine);
        assert!(specs[0].scope.is_all());
    }

    #[test]
    fn web_seed_exact_sets_the_composition_mode_regardless_of_the_shared_one() {
        let specs = collect_ok(&[
            "--web-seed-mode",
            "prefix",
            "--web-seed",
            "https://a.example.com/pub/",
            "--web-seed-exact",
            "https://cdn.example.com/blob",
        ]);
        assert_eq!(specs[0].mode, Mode::Prefix);
        assert_eq!(specs[1].mode, Mode::Exact);
    }

    #[test]
    fn web_seed_for_binds_a_scope_to_a_source() {
        let specs = collect_ok(&["--web-seed-for", "piece:0-511=https://b.example.com/"]);
        assert_eq!(specs[0].url, "https://b.example.com/");
        assert_eq!(specs[0].scope.text(), "piece:0-511");
    }

    #[test]
    fn a_malformed_web_seed_for_says_what_it_wanted() {
        let err = collect(
            &args(&["--web-seed-for", "no-equals-sign"]),
            None,
            None,
            &env(),
            no_network,
        )
        .unwrap_err();
        assert_eq!(err.code(), bit_cli_core::ExitCode::Usage);
        assert!(err.message().contains("SELECTOR=URL"), "{}", err.message());
    }

    #[test]
    fn a_bad_selector_in_web_seed_for_names_the_binding() {
        let err = collect(
            &args(&["--web-seed-for", "piece:9-2=https://b.example.com/"]),
            None,
            None,
            &env(),
            no_network,
        )
        .unwrap_err();
        assert_eq!(err.code(), bit_cli_core::ExitCode::Binding);
        assert!(err.to_string().contains("piece:9-2"), "{err}");
    }

    #[test]
    fn piece_and_byte_restrictions_apply_to_cli_sources() {
        let specs = collect_ok(&[
            "--web-seed-pieces",
            "0-511",
            "--web-seed",
            "https://a.example.com/",
        ]);
        assert_eq!(specs[0].scope.text(), "piece:0-511");

        let specs = collect_ok(&[
            "--web-seed-bytes",
            "0-1MiB",
            "--web-seed",
            "https://a.example.com/",
        ]);
        assert_eq!(specs[0].scope.text(), "byte:0-1MiB");
    }

    #[test]
    fn asking_for_both_restrictions_at_once_is_a_usage_error() {
        let err = collect(
            &args(&["--web-seed-pieces", "0-1", "--web-seed-bytes", "0-1MiB"]),
            None,
            None,
            &env(),
            no_network,
        )
        .unwrap_err();
        assert_eq!(err.code(), bit_cli_core::ExitCode::Usage);
    }

    #[test]
    fn headers_auth_and_limits_reach_every_cli_source() {
        let specs = collect_ok(&[
            "--web-seed",
            "https://a.example.com/",
            "--web-seed",
            "https://b.example.com/",
            "--web-seed-header",
            "X-Region: apac",
            "--web-seed-header",
            "X-Trace:on",
            "--web-seed-auth",
            "bearer:tok",
            "--web-seed-concurrency",
            "12",
            "--web-seed-chunk-size",
            "8MiB",
            "--web-seed-timeout",
            "45s",
            "--web-seed-speed-limit",
            "5MiB/s",
            "--web-seed-priority",
            "7",
        ]);
        assert_eq!(specs.len(), 2);
        for spec in &specs {
            assert_eq!(spec.headers["X-Region"], "apac");
            assert_eq!(spec.headers["X-Trace"], "on");
            assert_eq!(
                spec.auth,
                Auth::Bearer {
                    token: "tok".into()
                }
            );
            assert_eq!(spec.limits.concurrency, 12);
            assert_eq!(spec.limits.chunk_size, 8 * bit_cli_core::units::MIB);
            assert_eq!(spec.limits.timeout_ms, 45_000);
            assert_eq!(spec.limits.rate_limit, Some(5 * bit_cli_core::units::MIB));
            assert_eq!(spec.priority, 7);
        }
    }

    #[test]
    fn a_malformed_header_is_a_usage_error() {
        let err = collect(
            &args(&["--web-seed-header", "no-colon"]),
            None,
            None,
            &env(),
            no_network,
        )
        .unwrap_err();
        assert!(err.message().contains("Name: value"), "{}", err.message());
    }

    #[test]
    fn a_url_list_file_contributes_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mirrors.txt");
        std::fs::write(
            &path,
            "# mirrors\nhttps://a.example.com/\n\nhttps://b.example.com/\n",
        )
        .unwrap();
        let mut env = env();
        env.cwd = dir.path().to_path_buf();
        let specs = collect(
            &args(&["--web-seed-file", "mirrors.txt"]),
            None,
            None,
            &env,
            no_network,
        )
        .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].origin, Origin::File);
    }

    #[test]
    fn a_list_url_is_fetched_through_the_injected_fetcher() {
        let specs = collect(
            &args(&["--web-seed-list-url", "https://e.com/mirrors.txt"]),
            None,
            None,
            &env(),
            |_| Ok("https://a.example.com/\nhttps://b.example.com/\n".to_string()),
        )
        .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].origin, Origin::ListUrl);
    }

    #[test]
    fn a_list_url_on_a_no_network_command_fails_clearly() {
        let err = collect(
            &args(&["--web-seed-list-url", "https://e.com/mirrors.txt"]),
            None,
            None,
            &env(),
            no_network,
        )
        .unwrap_err();
        assert!(
            err.message().contains("needs the network"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn a_binding_table_contributes_its_own_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seeds.toml");
        std::fs::write(
            &path,
            "[[source]]\nurl = \"https://a.example.com/\"\nscope = \"piece:0-1\"\npriority = 9\n",
        )
        .unwrap();
        let mut env = env();
        env.cwd = dir.path().to_path_buf();
        let specs = collect(
            &args(&["--web-seed-config", "seeds.toml"]),
            None,
            None,
            &env,
            no_network,
        )
        .unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].origin, Origin::Config);
        assert_eq!(specs[0].priority, 9);
        assert_eq!(specs[0].scope.text(), "piece:0-1");
    }

    #[test]
    fn no_web_seed_drops_everything() {
        let specs = collect_ok(&["--no-web-seed", "--web-seed", "https://a.example.com/"]);
        assert!(specs.is_empty());
    }

    #[test]
    fn web_seed_only_with_nothing_declared_is_refused() {
        let err = collect(&args(&["--web-seed-only"]), None, None, &env(), no_network).unwrap_err();
        assert_eq!(err.code(), bit_cli_core::ExitCode::NoUsableSources);
    }

    #[test]
    fn sources_keep_the_origin_they_came_from() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.txt"), "https://f.example.com/\n").unwrap();
        let mut env = env();
        env.cwd = dir.path().to_path_buf();
        let specs = collect(
            &args(&[
                "--web-seed",
                "https://c.example.com/",
                "--web-seed-file",
                "m.txt",
            ]),
            None,
            None,
            &env,
            no_network,
        )
        .unwrap();
        let origins: Vec<Origin> = specs.iter().map(|s| s.origin).collect();
        assert_eq!(origins, [Origin::CommandLine, Origin::File]);
    }

    /// A binding may name one torrent, and one that names a different one is
    /// dropped for this torrent rather than applied to it.
    ///
    /// Without this, `-j 2` over two torrents that share a file cannot say
    /// which one a binding means, and the shared file is at a different index
    /// in each. See `TODO/multi-source.md`, T-133.
    #[test]
    fn a_binding_qualified_by_info_hash_applies_to_that_torrent_alone() {
        let fixture = crate::test_support::TorrentFixture::single_file();
        let meta = Metainfo::read(&std::path::PathBuf::from(fixture.path_str())).unwrap();
        let mine = &fixture.info_hash;
        let other = "0000000000000000000000000000000000000000";

        let specs = collect(
            &args(&[
                "--web-seed-for",
                &format!("{mine}:file:0=https://mine.example.com/blob"),
                "--web-seed-for",
                &format!("{other}:file:0=https://other.example.com/blob"),
                "--no-torrent-web-seed",
            ]),
            Some(&meta),
            None,
            &env(),
            no_network,
        )
        .unwrap();

        let urls: Vec<&str> = specs.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(urls, ["https://mine.example.com/blob"]);
        assert_eq!(specs[0].scope.text(), "file:0");
    }

    /// An unqualified binding still applies to every torrent, which is what a
    /// single torrent run has always done.
    #[test]
    fn an_unqualified_binding_still_applies_to_every_torrent() {
        let fixture = crate::test_support::TorrentFixture::single_file();
        let meta = Metainfo::read(&std::path::PathBuf::from(fixture.path_str())).unwrap();
        let specs = collect(
            &args(&[
                "--web-seed-for",
                "file:0=https://any.example.com/blob",
                "--no-torrent-web-seed",
            ]),
            Some(&meta),
            None,
            &env(),
            no_network,
        )
        .unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].scope.text(), "file:0");
    }

    /// A qualified binding needs the metadata. A magnet does not have it yet,
    /// so it is a usage error rather than a binding that silently does
    /// nothing.
    #[test]
    fn a_qualified_binding_without_metadata_is_a_usage_error() {
        let hash = "0102030405060708090a0b0c0d0e0f1011121314";
        let error = collect(
            &args(&[
                "--web-seed-for",
                &format!("{hash}:file:0=https://cdn.example.com/blob"),
            ]),
            None,
            None,
            &env(),
            no_network,
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("info hash"), "{text}");
    }

    /// The qualifier is exactly forty hexadecimal characters and a colon.
    /// Anything else is part of the selector, so a path that happens to start
    /// with a colon-separated word is unaffected.
    #[test]
    fn only_forty_hex_characters_are_read_as_an_info_hash() {
        assert_eq!(
            split_torrent("0102030405060708090a0b0c0d0e0f1011121314:file:0"),
            (Some("0102030405060708090a0b0c0d0e0f1011121314"), "file:0")
        );
        assert_eq!(split_torrent("file:0"), (None, "file:0"));
        assert_eq!(split_torrent("piece:0-511"), (None, "piece:0-511"));
        assert_eq!(split_torrent("a/b.iso"), (None, "a/b.iso"));
        // Thirty-nine characters, so not a hash.
        assert_eq!(
            split_torrent("0102030405060708090a0b0c0d0e0f101112131:x"),
            (None, "0102030405060708090a0b0c0d0e0f101112131:x")
        );
        // Forty characters, one of them not hex.
        assert_eq!(
            split_torrent("0102030405060708090a0b0c0d0e0f101112131z:x"),
            (None, "0102030405060708090a0b0c0d0e0f101112131z:x")
        );
    }

    /// The hashes a run has to check against the torrents it is adding.
    #[test]
    fn qualified_torrents_lists_every_hash_a_binding_names() {
        let hash = "0102030405060708090A0B0C0D0E0F1011121314";
        let parsed = qualified_torrents(&args(&[
            "--web-seed-for",
            &format!("{hash}:file:0=https://a.example.com/x"),
            "--web-seed-for",
            "file:1=https://b.example.com/y",
        ]));
        assert_eq!(parsed.len(), 1, "only the qualified one: {parsed:?}");
        assert_eq!(
            parsed[0].1,
            hash.to_ascii_lowercase(),
            "normalised to lower case"
        );
    }
}
