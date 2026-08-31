//! Command-line definitions.
//!
//! The target user is a script or an agent, not a person watching a terminal.
//! Two rules follow from that and they are not negotiable:
//!
//! - **stdout carries data only.** JSON, NDJSON, or the requested plain
//!   values. A caller doing `bit-cli ... --json | jq` must never see a log
//!   line in the pipe.
//! - **stderr carries logs, progress, warnings, and errors.**
//!
//! Short flags follow `aria2`. A letter `aria2` already assigns keeps its
//! meaning, and a letter it does not assign is only used where the meaning is
//! obvious. Reassigning an `aria2` letter to a different concept would let a
//! script written from muscle memory do something else silently, which is
//! worse than having no short flag at all. `docs/flags.md` holds the full
//! table and CI checks it.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Non-interactive BitTorrent and HTTP download tool.
#[derive(Debug, Parser)]
#[command(
    name = "bit-cli",
    version,
    about = "Fetch, create, verify, and seed torrents, with first-class web seed control.",
    long_about = None,
    disable_help_subcommand = true,
    args_conflicts_with_subcommands = false,
    subcommand_negates_reqs = true,
    // `clap` would give `--version` the short `-V`, which `aria2` assigns to
    // `--check-integrity`. Reassigning an `aria2` letter to a different
    // concept is what lets a script written from muscle memory do something
    // else silently, so `--version` has no short form here. See docs/flags.md.
    disable_version_flag = true,
)]
pub struct Cli {
    /// Print the version and exit.
    ///
    /// There is no short form: `-v` is verbosity and `-V` is
    /// `--check-integrity`, both following `aria2`.
    #[arg(long, action = clap::ArgAction::Version)]
    pub version: (),

    #[command(flatten)]
    pub global: Global,

    #[command(subcommand)]
    pub command: Option<Command>,

    /// Sources to download when no subcommand is given.
    ///
    /// `bit-cli <SOURCE>` is the same as `bit-cli download <SOURCE>`.
    ///
    /// `help_heading = None` because this field is declared after the `Global`
    /// flatten, which sets "Global options", and a positional inherits the
    /// running heading like any other argument. Without it `bit-cli --help`
    /// has no "Arguments" section at all and documents `[SOURCE]...` at the
    /// bottom of the global flags. `TODO/cli-surface.md`, T-159.
    #[arg(value_name = "SOURCE", help_heading = None)]
    pub sources: Vec<String>,
}

/// Flags that apply to every subcommand.
#[derive(Debug, Args, Clone)]
#[command(next_help_heading = "Global options")]
pub struct Global {
    /// Emit machine-readable JSON on stdout. Implies --progress=none.
    #[arg(long, global = true)]
    pub json: bool,

    /// Emit newline-delimited JSON events on stdout as they happen.
    #[arg(long, global = true, conflicts_with = "json")]
    pub jsonl: bool,

    /// Print the output schema version and exit.
    #[arg(long, global = true)]
    pub schema_version: bool,

    /// Suppress all non-error output.
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Increase verbosity. Repeatable: -v, -vv, -vvv.
    ///
    /// `aria2` uses -v for --version, so `bit-cli` does not: --version has no
    /// short form here. See docs/flags.md.
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Log level.
    #[arg(long, global = true, value_name = "LEVEL", default_value = "warn")]
    pub log_level: LogLevel,

    /// Log format.
    #[arg(long, global = true, value_name = "FMT", default_value = "text")]
    pub log_format: LogFormat,

    /// Append logs to a file. Rotates by size and count.
    ///
    /// A second destination, not a replacement: stderr still carries the logs,
    /// so `bit-cli ... --json | jq` behaves the same either way. Redirect
    /// stderr if you want only the file. The directory is created if it is not
    /// there.
    #[arg(short = 'l', long, global = true, value_name = "PATH")]
    pub log_file: Option<PathBuf>,

    /// Rotate the log at this size. `0` never rotates.
    #[arg(long, global = true, value_name = "SIZE", default_value = "16MiB")]
    pub log_max_size: String,

    /// Keep this many logs in total, the live one included.
    ///
    /// `--log-max-files 3` leaves `x.log`, `x.log.1`, and `x.log.2`. `1` keeps
    /// no history and starts the live file over instead.
    #[arg(long, global = true, value_name = "N", default_value_t = 5)]
    pub log_max_files: u32,

    /// Enable detailed tracing for one subsystem without raising the global level.
    ///
    /// Repeatable or comma-separated. Subsystems: peer, handshake, tracker,
    /// dht, http, piece, picker, disk, ratelimit, retry, config.
    #[arg(long, global = true, value_name = "SUBSYSTEM", value_delimiter = ',')]
    pub trace: Vec<String>,

    /// Show credentials in trace output instead of redacting them.
    #[arg(long, global = true)]
    pub no_redact: bool,

    /// When to use colour. Honours NO_COLOR.
    #[arg(long, global = true, value_name = "WHEN", default_value = "auto")]
    pub color: ColorWhen,

    /// Progress rendering. Defaults to none when stdout is not a terminal.
    #[arg(long, global = true, value_name = "MODE", default_value = "auto")]
    pub progress: ProgressMode,

    /// Config file path.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Ignore all config files.
    #[arg(long, global = true, conflicts_with = "config")]
    pub no_config: bool,

    /// Output directory.
    #[arg(short = 'd', long, global = true, value_name = "DIR")]
    pub dir: Option<PathBuf>,

    /// Resolve, validate, and report. Write nothing.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Print every field of the report, rather than the usual summary.
    ///
    /// A rendering flag and nothing else: it changes no behaviour, takes no
    /// measurement, and leaves `--json` byte for byte identical. Every number
    /// it prints was already computed and already in the JSON.
    #[arg(long, global = true)]
    pub stats: bool,

    /// Overall operation deadline.
    #[arg(long, global = true, value_name = "DUR")]
    pub timeout: Option<String>,

    /// Stop after this long regardless of state.
    #[arg(long, global = true, value_name = "DUR")]
    pub stop_after: Option<String>,
}

/// Log severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// The `tracing` filter directive this level means.
    pub const fn directive(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    /// Raise the level by `steps`, which is what repeated `-v` does.
    pub fn raised(self, steps: u8) -> Self {
        let ladder = [
            Self::Off,
            Self::Error,
            Self::Warn,
            Self::Info,
            Self::Debug,
            Self::Trace,
        ];
        let current = ladder.iter().position(|l| *l == self).unwrap_or(2);
        ladder[(current + steps as usize).min(ladder.len() - 1)]
    }
}

/// Log rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum LogFormat {
    Text,
    Json,
}

/// When to colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

impl From<ColorWhen> for crate::env::ColorChoice {
    fn from(when: ColorWhen) -> Self {
        match when {
            ColorWhen::Auto => Self::Auto,
            ColorWhen::Always => Self::Always,
            ColorWhen::Never => Self::Never,
        }
    }
}

/// How progress is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum ProgressMode {
    Auto,
    None,
    Plain,
    Json,
}

/// Composition mode for CLI-supplied web seeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum WebSeedMode {
    #[default]
    Auto,
    Exact,
    Prefix,
    Template,
}

impl From<WebSeedMode> for bit_cli_core::webseed::Mode {
    fn from(mode: WebSeedMode) -> Self {
        match mode {
            WebSeedMode::Auto => Self::Auto,
            WebSeedMode::Exact => Self::Exact,
            WebSeedMode::Prefix => Self::Prefix,
            WebSeedMode::Template => Self::Template,
        }
    }
}

/// BEP 19 or BEP 17 wire style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum WebSeedStyle {
    #[default]
    Auto,
    GetRight,
    Hoffman,
}

impl From<WebSeedStyle> for bit_cli_core::webseed::Style {
    fn from(style: WebSeedStyle) -> Self {
        match style {
            WebSeedStyle::Auto => Self::Auto,
            WebSeedStyle::GetRight => Self::GetRight,
            WebSeedStyle::Hoffman => Self::Hoffman,
        }
    }
}

/// The subcommands.
///
/// The variants differ a lot in size, which clippy notices. Boxing them would
/// mean a heap allocation and a deref at every match site to save stack on a
/// value that is parsed once and lives for the whole process. Not worth it.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fetch to completion in the foreground, then exit.
    Download(DownloadArgs),

    /// Parse a torrent, magnet, or metalink and print its metadata.
    Info(ReadSourceArgs),

    /// List files with index, path, size, and priority.
    Files(FilesArgs),

    /// Print the torrent's directory structure, rolled up.
    Tree(TreeArgs),

    /// Connect, sample the swarm, report peers, then exit.
    Peers(PeersArgs),

    /// Announce or scrape, report the result, then exit.
    Trackers(TrackersArgs),

    /// Inspect, validate, and read from HTTP sources.
    #[command(subcommand)]
    Webseed(WebseedCommand),

    /// Hash-check existing data against the torrent.
    Verify(VerifyArgs),

    /// Create a .torrent.
    Create(CreateArgs),

    /// Rewrite metainfo fields on an existing .torrent, writing a new file.
    Edit(EditArgs),

    /// Convert a torrent to a magnet URI, or resolve a magnet to metadata.
    Magnet(MagnetArgs),

    /// Seed existing data in the foreground.
    Seed(SeedArgs),

    /// Measure a target.
    #[command(subcommand)]
    Bench(BenchCommand),

    /// Configuration.
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Generate shell completions.
    Completions(CompletionsArgs),

    /// Generate a man page.
    Man(ManArgs),

    /// Version, build metadata, enabled features, and protocol support.
    Version,
}

/// A source and nothing else.
#[derive(Debug, Args)]
pub struct SourceArgs {
    /// A .torrent path, an HTTP(S) URL, a magnet URI, an info hash, a
    /// metalink, or `-` for stdin.
    ///
    /// `help_heading = None` keeps it in clap's own "Arguments" section
    /// wherever it is flattened. Without it, a command that sets a heading
    /// before flattening this would file its own positional under that
    /// heading. `TODO/cli-surface.md`, T-159.
    #[arg(value_name = "SOURCE", help_heading = None)]
    pub source: String,
}

/// How a magnet or a bare info hash is turned into metainfo.
///
/// Every other source kind is a document: a file to read, or a URL to fetch
/// with one `GET`. These two carry an info hash and nothing else, so the only
/// way to read one is to join the swarm it names and ask a peer for the
/// metadata over BEP 9. That is a different operation from a fetch, with a
/// different cost, so it has flags of its own rather than happening silently.
///
/// The four names are the ones `download` and `seed` already use, because a
/// caller who has restricted one swarm means the same thing here. They are not
/// on `SourceArgs` itself: `seed` and the three `bench` subcommands flatten
/// that and define `--peer` and `--no-dht` of their own, and clap refuses two
/// definitions of one flag. See `TODO/metainfo.md`, T-241.
#[derive(Debug, Args, Clone, Default)]
#[command(next_help_heading = "Resolving a magnet")]
pub struct SwarmSourceArgs {
    /// Try this peer before any are discovered, as HOST:PORT. Repeatable.
    ///
    /// With `--no-dht`, `--no-lsd` and `--no-tracker` this resolves the
    /// metadata from exactly the peers named here and nowhere else.
    #[arg(long = "peer", value_name = "ADDR")]
    pub peers: Vec<String>,

    /// Disable the DHT while resolving.
    #[arg(long)]
    pub no_dht: bool,

    /// Disable local service discovery while resolving.
    #[arg(long)]
    pub no_lsd: bool,

    /// Do not announce to the magnet's trackers while resolving.
    #[arg(long)]
    pub no_tracker: bool,
}

/// How a URL that turns out to be a web page is turned into one source.
///
/// A page is fetched with one `GET`, the same as a `.torrent` URL, and is only
/// told apart from one after the bencode parse has failed. What needs a flag
/// is the case where the page names **several** torrents: that is refused
/// rather than guessed at, and this is how a caller says which one they meant
/// without going and reading the page by hand. See `TODO/cli-surface.md`,
/// T-244.
#[derive(Debug, Args, Clone, Default)]
#[command(next_help_heading = "Resolving a web page")]
pub struct PageSourceArgs {
    /// Take the one link on a page whose URL or text contains TEXT.
    ///
    /// Matched case insensitively, as a substring, against both the resolved
    /// URL and the anchor text beside it. A page is still refused when this
    /// leaves more than one link, because a selector that matches two is not a
    /// selection.
    #[arg(long = "page-select", value_name = "TEXT")]
    pub page_select: Option<String>,

    /// Which client to present as when fetching a source document.
    ///
    /// `browser` sends a current Chrome's header set in Chrome's order, which
    /// is what an origin reads before it decides which page to serve.
    /// `plain` sends `bit-cli/<version>` and nothing else. This is the source
    /// document only: a web seed is a mirror you configured and always gets
    /// `bit-cli`.
    #[arg(
        long = "page-client",
        value_name = "PROFILE",
        default_value = "browser"
    )]
    pub client: ClientProfileArg,

    /// Read the page after its script has run, through an installed browser.
    ///
    /// A page that builds its links in script has none of them in the HTML the
    /// server sent. This drives a Chrome or Edge that is **already
    /// installed**, over the DevTools protocol, and extracts from the DOM
    /// afterwards. It never installs a browser and never bundles one.
    ///
    /// It needs a build with the `render` feature: `cargo build --release
    /// --features render`. Without one the flag is refused with a message
    /// saying so rather than silently reading the page unrendered.
    #[arg(long = "render")]
    pub render: bool,

    /// The browser `--render` drives, when it is not where this looks.
    ///
    /// Absent, an already-running instance on `--browser-port` is tried, then
    /// the executables on `PATH`, then the platform's usual locations. A path
    /// given here is tried first and alone: naming one and getting a different
    /// browser is the tool ignoring its own instruction.
    #[arg(long = "browser-path", value_name = "PATH")]
    pub browser_path: Option<std::path::PathBuf>,

    /// Attach to a browser already listening for the DevTools protocol.
    ///
    /// `HOST:PORT`, or just `PORT` for `127.0.0.1`. Attaching to a browser
    /// that is already up is cheaper than starting a second one.
    #[arg(long = "browser-port", value_name = "HOST:PORT")]
    pub browser_port: Option<String>,
}

/// The `--page-client` values, mapped to the core's own enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ClientProfileArg {
    #[default]
    Browser,
    Plain,
}

impl From<ClientProfileArg> for bit_cli_core::page::ClientProfile {
    fn from(value: ClientProfileArg) -> Self {
        match value {
            ClientProfileArg::Browser => Self::Browser,
            ClientProfileArg::Plain => Self::Plain,
        }
    }
}

/// A source that may be a magnet, and the swarm flags that resolve one.
///
/// `SourceArgs` on its own is the positional and nothing else, and it stays
/// that way for the commands that already own their swarm flags. This is what
/// a read-only command flattens.
#[derive(Debug, Args)]
pub struct ReadSourceArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// `bit-cli magnet`: a torrent to a magnet, or a magnet back to a torrent.
#[derive(Debug, Args)]
pub struct MagnetArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    /// Write the resolved metainfo here as a `.torrent`. `-` is stdout.
    ///
    /// The whole point of resolving a magnet twice is not having to: the
    /// metadata came off the swarm once and this keeps it. The bytes written
    /// carry the `info` dictionary exactly as it arrived, so the info hash of
    /// the file equals the one in the magnet, and `write_to_vec` proves that
    /// rather than trusting it. See `TODO/metainfo.md`, T-241.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<String>,

    /// Overwrite the output file if it is already there.
    #[arg(long)]
    pub force: bool,

    // Last, so the group's help heading does not swallow --output above it.
    // See the note where it is flattened into the other read-only commands.
    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// Web seed flags, shared by every command that can attach one.
#[derive(Debug, Args, Clone, Default)]
#[command(next_help_heading = "Web seeds")]
pub struct WebSeedArgs {
    /// Source for the whole torrent, under the current composition mode.
    #[arg(long = "web-seed", value_name = "URL")]
    pub web_seed: Vec<String>,

    /// Shorthand for a source with composition=exact.
    #[arg(long = "web-seed-exact", value_name = "URL")]
    pub web_seed_exact: Vec<String>,

    /// Bind a scope selector to a source, as SELECTOR=URL.
    ///
    /// The selector may name one torrent: prefix it with that torrent's info
    /// hash and a colon, as `<40 hex>:file:0=URL`. Without a prefix the
    /// binding applies to every torrent in the invocation, which is what a
    /// single torrent run wants and wrong when the same file sits at a
    /// different index in two of them.
    ///
    /// Exactly forty hexadecimal characters followed by a colon is read as an
    /// info hash. A hash naming no torrent in the run is a usage error rather
    /// than a binding that quietly does nothing.
    #[arg(long = "web-seed-for", value_name = "SEL=URL")]
    pub web_seed_for: Vec<String>,

    /// Composition mode for CLI-supplied sources.
    #[arg(long = "web-seed-mode", value_name = "MODE", default_value = "auto")]
    pub web_seed_mode: WebSeedMode,

    /// Template used when the mode is `template`.
    #[arg(long = "web-seed-template", value_name = "TMPL")]
    pub web_seed_template: Option<String>,

    /// Restrict CLI-supplied sources to these piece indices.
    #[arg(long = "web-seed-pieces", value_name = "RANGE")]
    pub web_seed_pieces: Option<String>,

    /// Restrict CLI-supplied sources to this byte range of the payload.
    #[arg(long = "web-seed-bytes", value_name = "RANGE")]
    pub web_seed_bytes: Option<String>,

    /// One URL per line. Blank lines and # comments are ignored.
    #[arg(long = "web-seed-file", value_name = "PATH")]
    pub web_seed_file: Vec<PathBuf>,

    /// Fetch a newline-separated URL list over HTTP.
    #[arg(long = "web-seed-list-url", value_name = "URL")]
    pub web_seed_list_url: Vec<String>,

    /// TOML or JSON binding table. Full control.
    #[arg(long = "web-seed-config", value_name = "PATH")]
    pub web_seed_config: Vec<PathBuf>,

    /// BEP 19 or BEP 17 wire style.
    #[arg(long = "web-seed-style", value_name = "STYLE", default_value = "auto")]
    pub web_seed_style: WebSeedStyle,

    /// Disable peers, DHT, PEX, LSD, and trackers. HTTP sources only.
    #[arg(long = "web-seed-only")]
    pub web_seed_only: bool,

    /// Ignore all web seeds, including the torrent's own url-list.
    #[arg(long = "no-web-seed", conflicts_with = "no_torrent_web_seed")]
    pub no_web_seed: bool,

    /// Ignore the torrent's url-list but keep CLI-supplied sources.
    #[arg(long = "no-torrent-web-seed")]
    pub no_torrent_web_seed: bool,

    /// Concurrent ranged requests per source.
    #[arg(long = "web-seed-concurrency", value_name = "N")]
    pub web_seed_concurrency: Option<usize>,

    /// `aria2` spelling of `--web-seed-concurrency`. Per source, not per server.
    ///
    /// In `aria2` this caps connections to one **server**. Here it caps
    /// concurrent ranged requests to one **source**, and the two differ when
    /// two sources share a host: `-x 4` with two sources on the same host is
    /// eight requests to that host, where `aria2` would hold it to four. A
    /// warning says so on first use.
    ///
    /// The number behind it is measured: 940 MiB/s at one, 3.44 GiB/s at
    /// eight, and past eight throughput stops while p99 doubles.
    /// `bench/split-20260823T182709577Z.json` is the curve. See
    /// `TODO/performance.md`, T-033.
    #[arg(short = 'x', long = "max-connection-per-server", value_name = "N")]
    pub max_connection_per_server: Option<usize>,

    /// `aria2` spelling of `--web-seed-concurrency`. The same knob as `-x`.
    ///
    /// `aria2` splits one file into N ranges and caps per-server connections
    /// separately, so `-s` and `-x` are two settings there. Here one source is
    /// fetched by concurrent ranged requests and that is the only knob, so
    /// these are two spellings of it. Passing both is not multiplied: the
    /// larger wins, and a warning names it when they differ.
    #[arg(short = 's', long = "split", value_name = "N")]
    pub split: Option<usize>,

    /// Peer connections each source is presented over. Default: 1.
    ///
    /// One source is one peer to the torrent session, and a peer's blocks are
    /// written and verified one at a time on that connection's own task, so
    /// that path is what bounds the transfer. Several connections give the
    /// source several of them. `--web-seed-concurrency` is divided between
    /// them rather than multiplied by them, so this does not hit the mirror
    /// harder. Measured in TODO/webseed.md, T-009.
    #[arg(long = "web-seed-connections", value_name = "N")]
    pub web_seed_connections: Option<usize>,

    /// Concurrent ranged requests across all sources.
    #[arg(long = "web-seed-max-total", value_name = "N")]
    pub web_seed_max_total: Option<usize>,

    /// Bytes per ranged request. Independent of the torrent's piece length.
    #[arg(long = "web-seed-chunk-size", value_name = "SIZE")]
    pub web_seed_chunk_size: Option<String>,

    /// `aria2` spelling of a floor under `--web-seed-chunk-size`.
    ///
    /// A floor rather than a value: `aria2` will not split a file into pieces
    /// smaller than this, so a request is at least this big. Where
    /// `--web-seed-chunk-size` is also given, the larger of the two is what a
    /// request asks for.
    #[arg(short = 'k', long = "min-split-size", value_name = "SIZE")]
    pub min_split_size: Option<String>,

    /// Per-request timeout.
    #[arg(long = "web-seed-timeout", value_name = "DUR")]
    pub web_seed_timeout: Option<String>,

    /// Connect timeout for web seed requests.
    #[arg(long = "web-seed-connect-timeout", value_name = "DUR")]
    pub web_seed_connect_timeout: Option<String>,

    /// Consecutive failed requests before a source is retired.
    ///
    /// A request that fails transiently after its own `--web-seed-retries`
    /// are spent drops the connection and reconnects, so a mirror that is
    /// down for a moment is not lost. This is how many of those in a row it
    /// takes before the source is out for the rest of the run. A success
    /// resets the count.
    #[arg(long = "web-seed-max-errors", value_name = "N")]
    pub web_seed_max_errors: Option<u32>,

    /// Give a source that spent its error budget another chance after this
    /// long. Zero, the default, means it does not come back.
    ///
    /// A source that runs out of its `--web-seed-max-errors` budget is out.
    /// With a cooldown set it is out for that long and then reconnects with
    /// the error run cleared, so a mirror that is down for five minutes is
    /// still usable at minute six. With the default of zero it is out for the
    /// rest of the run, which is what makes a run against one dead mirror fail
    /// in seconds instead of sitting on a timer.
    ///
    /// A cooling source is reported as `cooling`, not `failed`, so
    /// `--web-seed-require` and the "every source is dead" stop condition keep
    /// waiting for it. Set `--timeout` or `--stop-timeout` to bound that.
    #[arg(long = "web-seed-cooldown", value_name = "DUR")]
    pub web_seed_cooldown: Option<String>,

    /// Per-request retries before counting an error.
    #[arg(long = "web-seed-retries", value_name = "N")]
    pub web_seed_retries: Option<u32>,

    /// Statuses to retry that would otherwise retire the source.
    ///
    /// Codes and inclusive ranges: `403`, `403,429`, `500-599`. A CDN that
    /// signs URLs answers 403 when a signature expires and the next request
    /// to the stable URL is redirected to a fresh one, so `403` there is
    /// transient. `--web-seed-retries`, `--web-seed-max-errors`, and
    /// `--web-seed-cooldown` still bound it.
    #[arg(long = "web-seed-retry-status", value_name = "CODES")]
    pub web_seed_retry_status: Option<String>,

    /// Statuses that retire the source, which would otherwise be retried.
    ///
    /// The other direction of `--web-seed-retry-status`, same spelling. A
    /// code cannot be in both lists.
    #[arg(long = "web-seed-fatal-status", value_name = "CODES")]
    pub web_seed_fatal_status: Option<String>,

    /// User-Agent for web seed requests.
    #[arg(long = "web-seed-user-agent", value_name = "UA")]
    pub web_seed_user_agent: Option<String>,

    /// Extra header on web seed requests, as `Name: value`.
    #[arg(long = "web-seed-header", value_name = "K: V")]
    pub web_seed_header: Vec<String>,

    /// Credentials: basic:user:pass, bearer:TOKEN, netrc, or none.
    #[arg(long = "web-seed-auth", value_name = "SPEC")]
    pub web_seed_auth: Option<String>,

    /// Rate cap per source.
    #[arg(long = "web-seed-speed-limit", value_name = "RATE")]
    pub web_seed_speed_limit: Option<String>,

    /// When to hash-check HTTP-sourced data.
    #[arg(long = "web-seed-verify", value_name = "MODE", default_value = "piece")]
    pub web_seed_verify: VerifyWhen,

    /// Bias among sources. Higher wins when several can serve a piece.
    #[arg(long = "web-seed-priority", value_name = "N")]
    pub web_seed_priority: Option<i32>,

    /// Bias the picker toward HTTP when both a peer and a source have a piece.
    #[arg(long = "prefer-web-seed")]
    pub prefer_web_seed: bool,

    /// Fail the run if a declared source turns out to be unusable.
    #[arg(long = "web-seed-require")]
    pub web_seed_require: bool,
}

/// When HTTP-sourced data is hash-checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum VerifyWhen {
    #[default]
    Piece,
    File,
    None,
}

/// `bit-cli download`.
#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Sources to fetch.
    #[arg(value_name = "SOURCE", required = true)]
    pub sources: Vec<String>,

    #[command(flatten)]
    pub web_seeds: WebSeedArgs,

    #[command(flatten)]
    pub trackers: TrackerArgs,

    #[command(flatten)]
    pub limits: LimitArgs,

    #[command(flatten)]
    pub hooks: HookArgs,

    /// Run this command after every verified piece. High frequency.
    ///
    /// `download` only. A seeder verifies every piece once, during the hash
    /// check on add, so the same flag there would fire in a burst at startup
    /// and then be silent for days. See `TODO/cli-surface.md`, T-214.
    #[arg(long, value_name = "COMMAND")]
    pub on_piece_verified: Option<String>,

    #[command(flatten)]
    pub selection: SelectionArgs,

    /// Listen port, or a range as START-END. `0` asks the OS for a free one.
    #[arg(long, value_name = "PORT")]
    pub port: Vec<String>,

    /// Try this peer before any are discovered, as HOST:PORT. Repeatable.
    ///
    /// A peer given here is dialled whether or not a tracker or the DHT ever
    /// answers, which is what makes a swarm of known members testable and a
    /// private one reachable without discovery.
    #[arg(long = "peer", value_name = "ADDR")]
    pub peers: Vec<String>,

    /// Disable the DHT.
    ///
    /// With `--peer` and `--no-tracker` this leaves a swarm of exactly the
    /// members named on the command line, which is what a measurement needs
    /// and what a private network wants.
    #[arg(long)]
    pub no_dht: bool,

    /// Disable local service discovery.
    #[arg(long)]
    pub no_lsd: bool,

    /// Drop every peer connection and dial again after this long with no
    /// progress. Off by default.
    ///
    /// A peer that dies is retried on a backoff with a 10 second minimum and a
    /// factor of 6, so attempts land at about 10s, 70s, 430s, and then 36
    /// minutes. A peer that comes back one second after an attempt fails is
    /// not tried again for six times the last wait. On a swarm of one, which
    /// is what `--peer` builds and what a private tracker often is, that is
    /// the difference between a download finishing and a download timing out.
    ///
    /// This throws the peer state away instead of waiting: the torrent is
    /// paused and started again, which drops the backoff counters and dials
    /// `--peer` and the trackers from scratch. Piece state is kept and nothing
    /// is re-hashed. What it costs is every live connection, so set it longer
    /// than a slow peer's quiet spell.
    ///
    /// Set it shorter than `--stop-timeout` or the run gives up first.
    #[arg(long, value_name = "DUR")]
    pub redial_after: Option<String>,

    /// How many times `--redial-after` may fire in one run.
    #[arg(long, value_name = "N", default_value_t = 10)]
    pub max_redials: u32,

    /// Sources fetched in parallel within this one invocation.
    ///
    /// Sources start in the order they were given, so `-j 1` is a sequence: a
    /// torrent whose source is a file an earlier torrent writes can name it,
    /// and the earlier one will have finished.
    #[arg(short = 'j', long, value_name = "N", default_value_t = 1)]
    pub max_concurrent_downloads: usize,

    /// Do not read a file from another torrent in this run that is proven to
    /// hold it.
    ///
    /// Two torrents in one invocation often share a file. When their piece
    /// hashes prove the bytes are the same, the second one reads the first
    /// one's copy instead of fetching it again. The proof is the same evidence
    /// `bit-cli files --against` reports, and the copy is checked per piece on
    /// the way in like any other source. This turns that off and fetches
    /// everything.
    #[arg(long)]
    pub no_share_files: bool,

    /// Hash-check before starting.
    #[arg(short = 'V', long)]
    pub check_integrity: bool,

    /// Hash-check and exit.
    #[arg(long)]
    pub hash_check_only: bool,

    /// Re-read the finished payload and report a hash per file.
    ///
    /// Redundant by construction, and that is the point: every byte has
    /// already been checked against the torrent's own piece hashes, once at the
    /// source and once by the session. This is the check a caller can run
    /// without trusting the thing that wrote the bytes, and it is the one whose
    /// output can be compared against a digest published somewhere else.
    ///
    /// It reads the whole payload from disk, so it costs one full read. See
    /// `docs/integrity.md` and `TODO/multi-source.md`, T-136.
    #[arg(long)]
    pub verify_on_complete: bool,

    /// Resume a partial download. On by default.
    #[arg(short = 'c', long, overrides_with = "no_continue")]
    pub r#continue: bool,

    /// Refuse to write into a file that is already there.
    ///
    /// `--continue` is the default, so this is how a run says "these files
    /// should not exist yet". Without it a partial download resumes and a
    /// complete one is hash-checked and left alone. A flag that could only
    /// ever be on would not be a flag.
    #[arg(long = "no-continue", overrides_with = "continue")]
    pub no_continue: bool,

    /// Overwrite existing files.
    #[arg(long)]
    pub allow_overwrite: bool,

    /// Emit a progress event this often.
    #[arg(long, value_name = "DUR", default_value = "1s")]
    pub report_interval: String,
}

impl DownloadArgs {
    /// The arguments `bit-cli <SOURCE>` means, with every other flag left at
    /// its default.
    ///
    /// The bare form has to behave exactly like `bit-cli download <SOURCE>`,
    /// so it goes through the same argument type rather than a second path
    /// that could drift from it.
    pub fn from_sources(sources: Vec<String>) -> Self {
        Self {
            sources,
            web_seeds: WebSeedArgs::default(),
            trackers: TrackerArgs::default(),
            limits: LimitArgs::default(),
            hooks: HookArgs::default(),
            on_piece_verified: None,
            selection: SelectionArgs::default(),
            port: Vec::new(),
            peers: Vec::new(),
            no_dht: false,
            no_lsd: false,
            redial_after: None,
            max_redials: 10,
            max_concurrent_downloads: 1,
            no_share_files: false,
            check_integrity: false,
            hash_check_only: false,
            verify_on_complete: false,
            r#continue: true,
            no_continue: false,
            allow_overwrite: false,
            report_interval: "1s".to_string(),
        }
    }
}

/// Tracker flags.
#[derive(Debug, Args, Clone, Default)]
#[command(next_help_heading = "Trackers")]
pub struct TrackerArgs {
    /// Add a tracker at runtime. The .torrent is never rewritten.
    #[arg(long, value_name = "URL")]
    pub tracker: Vec<String>,

    /// One tracker per line. A blank line separates BEP 12 tiers.
    #[arg(long, value_name = "PATH")]
    pub tracker_file: Vec<PathBuf>,

    /// Fetch a tracker list over HTTP.
    #[arg(long, value_name = "URL")]
    pub tracker_list_url: Vec<String>,

    /// Remove trackers. `*` removes all.
    #[arg(long, value_name = "URL")]
    pub exclude_tracker: Vec<String>,

    /// Replace the torrent's tracker list instead of adding to it.
    #[arg(long)]
    pub replace_trackers: bool,

    /// Tracker request timeout.
    #[arg(long, value_name = "DUR")]
    pub tracker_timeout: Option<String>,

    /// Tracker connect timeout.
    #[arg(long, value_name = "DUR")]
    pub tracker_connect_timeout: Option<String>,

    /// Override the announce interval.
    #[arg(long, value_name = "DUR")]
    pub tracker_interval: Option<String>,

    /// Disable tracker announces entirely.
    #[arg(long)]
    pub no_tracker: bool,
}

/// Rate, peer, and lifecycle limits.
#[derive(Debug, Args, Clone, Default)]
#[command(next_help_heading = "Limits and lifecycle")]
pub struct LimitArgs {
    /// Download rate cap, per torrent.
    #[arg(long, value_name = "RATE")]
    pub max_download_rate: Option<String>,

    /// Upload rate cap, per torrent.
    #[arg(short = 'u', long, value_name = "RATE")]
    pub max_upload_rate: Option<String>,

    /// Download rate cap across the whole run.
    #[arg(long, value_name = "RATE")]
    pub max_overall_download_rate: Option<String>,

    /// Upload rate cap across the whole run.
    #[arg(long, value_name = "RATE")]
    pub max_overall_upload_rate: Option<String>,

    /// Download rate cap for swarm peers, not for attached HTTP sources.
    #[arg(long, value_name = "RATE")]
    pub max_peer_rate: Option<String>,

    /// Peer connections per torrent.
    #[arg(long, value_name = "N")]
    pub max_peers: Option<usize>,

    /// Peer connections across the run.
    #[arg(long, value_name = "N")]
    pub max_peers_total: Option<usize>,

    /// Message stream encryption, for peer connections in both directions.
    ///
    /// `prefer` dials with MSE and dials again in plaintext when the peer does
    /// not speak it, and accepts either. `require` refuses a plaintext peer in
    /// both directions, which is what reaches a peer configured to require
    /// encryption, at the cost of every peer that cannot do it. `off` neither
    /// offers nor accepts it.
    ///
    /// One listening port serves both: an accepting end tells the two apart by
    /// reading the first twenty bytes, so there is no second port and nothing
    /// on the wire says which mode this run is in. `--json` reports what each
    /// peer settled on as `encryption`. See `TODO/peers.md`, T-163.
    #[arg(long, value_name = "MODE", default_value = "prefer")]
    pub encryption: EncryptionMode,

    /// Which transports this run listens on and dials.
    ///
    /// `tcp` is the default and is what every run before 2026-08-24 did.
    /// `utp` is BEP 29 over UDP, which carries the same peer wire protocol
    /// under LEDBAT congestion control: it yields to other traffic on the same
    /// link instead of competing with it, which is what keeps a seeding box
    /// from making its own connection unusable. `both` listens on each and
    /// lets the peer decide.
    ///
    /// One port number serves both, because they are different protocols:
    /// `--port 6881` is TCP 6881 and UDP 6881. See `TODO/bep-coverage.md`,
    /// T-101.
    #[arg(long, value_name = "MODE", default_value = "tcp")]
    pub transport: TransportMode,

    /// Refuse this peer for the whole run. Repeatable.
    ///
    /// An address, an inclusive `START-END` range, or a CIDR block, in either
    /// family. A `HOST:PORT` is refused rather than truncated: the session
    /// blocks an address, so accepting one would block every port on that host
    /// without saying so.
    ///
    /// Checked before an incoming handshake is read and before an outgoing
    /// connection is dialled, so a blocked address never takes a connection
    /// slot. There is no state file, so this lasts for the invocation. See
    /// `TODO/peers.md`, T-164.
    #[arg(long = "block-peer", value_name = "ADDR")]
    pub block_peer: Vec<String>,

    /// Payload files kept open at once.
    ///
    /// Files open when they are first touched and the least recently opened
    /// closes when this cap is reached, so a torrent with twenty thousand
    /// files does not need twenty thousand descriptors.
    #[arg(long, value_name = "N", default_value_t = bit_cli_core::storage::DEFAULT_MAX_OPEN_FILES)]
    pub max_open_files: usize,

    /// Stop when the process holds more than this many handles. Off by default.
    ///
    /// This is a backstop, not a tuning knob. A supervised deployment sets it
    /// and gets a loud exit 16 and a restart instead of a process that quietly
    /// runs the machine out of descriptors.
    ///
    /// It was written for a specific leak, one socket per peer that connected
    /// and closed before handshaking, which `TODO/peers.md` T-020 measured and
    /// which is fixed: six hours of `scripts/soak.ps1` see zero sockets in
    /// `CLOSE_WAIT` at every sample. A backstop for a defect that is fixed
    /// still earns its place, because the next one has not been found yet.
    ///
    /// Counted against the whole process, so it includes threads, sockets, and
    /// payload files. Read `cost` in the report for a healthy baseline before
    /// picking a number.
    #[arg(long, value_name = "N")]
    pub max_handles: Option<u64>,

    /// Stop when the process holds more than this much resident memory. Off by
    /// default.
    ///
    /// The other backstop, for the growth beside the handles. A seeder under
    /// load grows about 0.8 MiB an hour, and TODO/memory.md T-040 attributes
    /// most of it to the peer row librqbit keeps for every peer it has ever
    /// accepted and never reclaims: 2,907 bytes a row, measured over 2,000 of
    /// them, retained after a minute of no traffic. Nothing here frees one, so
    /// a supervised deployment sets this and gets a loud exit 16 and a restart.
    ///
    /// Read `cost` in a healthy run's report for a baseline before picking a
    /// number: a seeder with nothing connected sits near 12 MiB.
    #[arg(long, value_name = "SIZE")]
    pub max_rss: Option<String>,

    /// Stop seeding at this ratio. 0 means do not seed.
    #[arg(long, value_name = "RATIO")]
    pub seed_ratio: Option<f64>,

    /// Stop seeding after this long.
    #[arg(long, value_name = "DUR")]
    pub seed_time: Option<String>,

    /// Give up if there is no progress for this long.
    #[arg(long, value_name = "DUR")]
    pub stop_timeout: Option<String>,

    /// Give up if the hash check has not finished in this long.
    ///
    /// Initialisation is reading the metadata and hash-checking whatever is
    /// on disk, and it is where a torrent can stop making progress without
    /// failing. The error names the phase and how far the check got, which a
    /// plain deadline does not.
    #[arg(long, value_name = "DUR", default_value = "10m")]
    pub init_timeout: String,

    /// Abort if the rate drops below this.
    #[arg(long, value_name = "RATE")]
    pub lowest_speed_limit: Option<String>,
}

/// Hooks: a command to run when something happens.
///
/// Their own struct rather than part of [`LimitArgs`], because five commands
/// flatten that and only two of them can honour a hook. `peers`, `bench leech`
/// and `bench seed` accepted all three and ran none, which is the shape
/// `TODO/cli-surface.md` T-181, T-183 and T-185 each record: a flag that
/// parses, is documented, and does nothing. See T-214.
#[derive(Debug, Args, Clone, Default)]
#[command(next_help_heading = "Hooks")]
pub struct HookArgs {
    /// Run this command on success. Arguments arrive through the environment.
    ///
    /// On `download` that is a torrent finishing. On `seed` it is the payload
    /// passing its hash check with the listener up, which is the moment a
    /// seeder starts being useful: a seeder has no completion of its own.
    #[arg(long, value_name = "COMMAND")]
    pub on_complete: Option<String>,

    /// Run this command on failure.
    #[arg(long, value_name = "COMMAND")]
    pub on_error: Option<String>,
}

/// File selection and placement.
#[derive(Debug, Args, Clone, Default)]
#[command(next_help_heading = "File selection")]
pub struct SelectionArgs {
    /// Download only these files. Accepts ranges: 1-5,8,10-.
    #[arg(long, value_name = "INDEX", value_delimiter = ',')]
    pub select_file: Vec<String>,

    /// Skip these files.
    #[arg(long, value_name = "INDEX", value_delimiter = ',')]
    pub exclude_file: Vec<String>,

    /// Rename a file by index, as INDEX=PATH.
    #[arg(short = 'O', long, value_name = "INDEX=PATH")]
    pub index_out: Vec<String>,

    /// Write the payload here instead of using the torrent's name.
    ///
    /// For a single-file torrent this names the file. For a multi-file one it
    /// names the directory that replaces the torrent's own name, so the files
    /// land directly under it. Relative to `--dir` when one is given, and to
    /// the working directory otherwise. One source only: two torrents told to
    /// write to one path is a usage error. See `TODO/cli-surface.md`, T-226.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// How disk space is allocated.
    #[arg(long, value_name = "METHOD", default_value = "sparse")]
    pub file_allocation: FileAllocation,

    /// Which piece to ask for next. `sequential` and `in-order` are the same
    /// thing under two names, and both make a download readable front to back.
    #[arg(long, value_name = "STRATEGY", default_value = "default")]
    pub piece_selector: PieceSelector,
}

/// What a run does about peer encryption.
///
/// Mirrors [`bit_cli_core::mse::Encryption`] rather than deriving `ValueEnum`
/// on it, which is the same split every other enum flag here uses: the core
/// crate does not depend on `clap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum EncryptionMode {
    /// Never encrypt, and refuse a peer that opens with MSE.
    Off,
    /// Try MSE, fall back to plaintext, accept both.
    #[default]
    Prefer,
    /// MSE or nothing, in both directions.
    Require,
}

impl From<EncryptionMode> for bit_cli_core::mse::Encryption {
    fn from(mode: EncryptionMode) -> Self {
        match mode {
            EncryptionMode::Off => Self::Off,
            EncryptionMode::Prefer => Self::Prefer,
            EncryptionMode::Require => Self::Require,
        }
    }
}

/// Which transports a run listens on and dials.
///
/// Mirrors [`bit_cli_core::engine::Transport`] for the same reason
/// [`EncryptionMode`] mirrors its core type: the core crate does not depend on
/// `clap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum TransportMode {
    /// TCP only. What every run did before this flag existed.
    #[default]
    Tcp,
    /// uTP only, BEP 29. Nothing reaches a TCP-only peer.
    Utp,
    /// Listen on both and let the peer choose.
    Both,
}

impl From<TransportMode> for bit_cli_core::engine::Transport {
    fn from(mode: TransportMode) -> Self {
        match mode {
            TransportMode::Tcp => Self::Tcp,
            TransportMode::Utp => Self::Utp,
            TransportMode::Both => Self::Both,
        }
    }
}

/// How space is reserved for the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum FileAllocation {
    None,
    Prealloc,
    #[default]
    Sparse,
    Falloc,
}

/// Which piece to ask for next.
///
/// Three values, and there used to be four. `rarest-first` was the default and
/// was a name for behaviour nothing here has: `librqbit` 9.0.0's picker does
/// not count how many peers hold a piece anywhere. What it actually does is
/// [`Self::Default`]. `random` went for the same reason in the other
/// direction: nothing implemented it and there is no way to ask for it. Both
/// are written up in `TODO/performance.md`, T-032.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum PieceSelector {
    /// Whatever the session does on its own: the first piece of each file,
    /// then the last, then the middle in ascending order.
    ///
    /// Fastest, because every peer takes whatever it can get, and readable
    /// front to back except for the tail arriving early.
    #[default]
    Default,
    /// Front to back, by holding the session's priority window at the earliest
    /// piece still missing.
    ///
    /// This is what makes `bit-cli download` readable as it arrives. It costs
    /// about a tenth of the throughput at four connections and nothing at one:
    /// `scripts/check-piece-order.ps1` is the measurement.
    Sequential,
    /// The same as [`Self::Sequential`], under `aria2`'s spelling for it.
    InOrder,
}

/// `bit-cli files`.
#[derive(Debug, Args)]
pub struct FilesArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    /// Sort key, as KEY or KEY:ORDER. Keys: index, path, size.
    #[arg(long, value_name = "KEY", default_value = "index")]
    pub sort: String,

    /// Also report which files another torrent holds identically. Repeatable.
    ///
    /// Two torrents that hold the same file are two downloads of the same
    /// bytes unless something connects them, and connecting them safely means
    /// knowing they are the same first. Each match says what the answer rests
    /// on: `piece-hashes` when the pieces line up and their hashes agree,
    /// which proves the bytes equal; `length` when only the size matches,
    /// which proves nothing and is what a differing piece length leaves.
    #[arg(long = "against", value_name = "TORRENT")]
    pub against: Vec<String>,

    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// `bit-cli tree`.
#[derive(Debug, Args)]
pub struct TreeArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    /// Stop at this depth and roll the rest up. The root is depth 0.
    ///
    /// A directory whose children are cut off still reports what is below it,
    /// and the line under it says how many files and directories that was, so
    /// a limit never drops anything in silence.
    #[arg(long, value_name = "N")]
    pub depth: Option<usize>,

    /// Print the piece ranges without the size and file count columns.
    #[arg(long = "no-sizes")]
    pub no_sizes: bool,

    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// `bit-cli peers`.
#[derive(Debug, Args)]
pub struct PeersArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    /// How long to sample the swarm.
    #[arg(long, value_name = "DUR", default_value = "15s")]
    pub duration: String,

    /// Stop once this many distinct peers have been seen.
    #[arg(long, value_name = "N")]
    pub count: Option<usize>,

    /// Sort key, as KEY or KEY:ORDER. Keys: addr, client, speed, pieces.
    #[arg(long, value_name = "KEY", default_value = "addr")]
    pub sort: String,

    /// Listen port, or a range as START-END. `0` asks the OS for a free one.
    #[arg(long, value_name = "PORT")]
    pub port: Vec<String>,

    #[command(flatten)]
    pub trackers: TrackerArgs,

    #[command(flatten)]
    pub limits: LimitArgs,

    /// Try this peer before any are discovered, as HOST:PORT. Repeatable.
    #[arg(long = "peer", value_name = "ADDR")]
    pub peers: Vec<String>,

    /// Disable the DHT.
    ///
    /// With `--peer` and `--no-tracker` this samples a swarm of exactly the
    /// members named on the command line and reaches nothing else.
    #[arg(long)]
    pub no_dht: bool,

    /// Disable local service discovery.
    #[arg(long)]
    pub no_lsd: bool,
}

/// `bit-cli trackers`.
#[derive(Debug, Args)]
pub struct TrackersArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub trackers: TrackerArgs,

    /// Scrape instead of announcing.
    #[arg(long)]
    pub scrape: bool,

    /// The scrape endpoint, for a tracker that does not follow BEP 48.
    ///
    /// BEP 48 derives the endpoint by replacing a trailing `announce` path
    /// component with `scrape`. A tracker whose path does not end that way has
    /// no derivable endpoint, and guessing one produces a 404 that reads like
    /// the tracker being down. This is how a caller who knows the endpoint
    /// says so. It replaces the derivation, so it names one tracker and the
    /// run has to be narrowed to that tracker.
    #[arg(long, value_name = "URL", requires = "scrape")]
    pub scrape_url: Option<String>,

    /// Port to announce, or a range as START-END. `0` asks the OS for a free
    /// one.
    ///
    /// The port is bound for the length of the announce, so what the tracker
    /// is told is a port something is actually listening on. The command then
    /// announces `stopped`, so the record does not outlive it.
    #[arg(long, value_name = "PORT")]
    pub port: Vec<String>,

    /// Announce and leave the peer record behind.
    ///
    /// The default withdraws it with a second announce carrying
    /// `event=stopped`, because asking a tracker a question should not
    /// register a peer that is gone by the time anyone dials it.
    #[arg(long)]
    pub no_withdraw: bool,

    /// Which address family to announce over.
    ///
    /// A tracker records the source address of the connection it was
    /// announced over, so one announce registers one of this host's addresses.
    /// `auto` announces once per family the tracker resolves to and reports
    /// each separately, which is what says whether both are reachable.
    #[arg(long, value_name = "FAMILY", default_value = "auto")]
    pub family: AnnounceFamily,
}

/// `--family` for `bit-cli trackers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AnnounceFamily {
    /// One announce per family the tracker has an address in.
    Auto,
    /// IPv4 only.
    V4,
    /// IPv6 only.
    V6,
}

/// `bit-cli webseed`.
#[derive(Debug, Subcommand)]
pub enum WebseedCommand {
    /// Resolve every binding and print the exact URL each file maps to. No network.
    List(WebseedListArgs),
    /// Probe each source: range support, size, redirects, TLS, latency.
    Test(WebseedTestArgs),
    /// Measure ranged-GET latency and throughput as concurrency scales.
    Probe(WebseedProbeArgs),
    /// Fetch one range from one source and verify it against the torrent.
    Fetch(WebseedFetchArgs),
}

/// `bit-cli webseed list`.
#[derive(Debug, Args)]
pub struct WebseedListArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub web_seeds: WebSeedArgs,

    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// `bit-cli webseed test`.
#[derive(Debug, Args)]
pub struct WebseedTestArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub web_seeds: WebSeedArgs,

    /// Use HEAD rather than a one-byte ranged GET.
    #[arg(long)]
    pub head: bool,

    /// Sources probed at once.
    ///
    /// A real torrent can carry hundreds of web seeds: the Arch Linux ISO
    /// carries 468. Probing them one at a time takes minutes, and every probe
    /// is one request to a different host, so they do not contend.
    #[arg(long, value_name = "N", default_value_t = 16)]
    pub concurrency: usize,

    /// Report this response header as well. Repeatable, case insensitive.
    ///
    /// The report already keeps the headers that answer "was this served from
    /// cache" and "what does support need": `age`, `x-cache`,
    /// `cf-cache-status`, `x-served-by`, `via`, `cache-control`, `etag`,
    /// `last-modified`, `content-encoding`, `cf-ray`, `x-amz-request-id` and
    /// `x-amz-id-2`. This is for anything else.
    ///
    /// It is an allowlist rather than everything because a report is a thing
    /// people paste, and a header set can carry a signed URL or a session
    /// cookie. A header named here whose value is a credential is still
    /// redacted unless `--no-redact` is given.
    #[arg(long = "web-seed-report-header", value_name = "NAME")]
    pub report_headers: Vec<String>,

    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// `bit-cli webseed probe`.
#[derive(Debug, Args)]
pub struct WebseedProbeArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub web_seeds: WebSeedArgs,

    /// How long to run.
    #[arg(long, value_name = "DUR", default_value = "10s")]
    pub duration: String,

    /// Step concurrency and report the curve.
    #[arg(long, value_name = "SPEC", default_value = "1,2,4,8,16")]
    pub concurrency_sweep: String,

    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// `bit-cli webseed fetch`.
#[derive(Debug, Args)]
pub struct WebseedFetchArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub web_seeds: WebSeedArgs,

    /// Fetch from exactly this URL.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Fetch one piece.
    #[arg(long, value_name = "N", conflicts_with_all = ["pieces", "bytes"])]
    pub piece: Option<u32>,

    /// Fetch a piece range.
    #[arg(long, value_name = "RANGE", conflicts_with = "bytes")]
    pub pieces: Option<String>,

    /// Fetch a whole file by index.
    #[arg(long, value_name = "N")]
    pub file: Option<usize>,

    /// Fetch a byte range.
    #[arg(long, value_name = "RANGE")]
    pub bytes: Option<String>,

    /// Write the bytes here, or `-` for stdout. Writes nothing without this.
    #[arg(long, value_name = "PATH")]
    pub output: Option<String>,

    /// Verify against the torrent's piece hashes.
    #[arg(long, default_value_t = true)]
    pub verify: bool,

    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// `bit-cli verify`.
#[derive(Debug, Args)]
pub struct VerifyArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    /// Where the payload lives. Defaults to --dir.
    #[arg(long, value_name = "PATH")]
    pub data: Option<PathBuf>,

    /// Report the result of every piece, not just the failures.
    #[arg(long)]
    pub per_piece: bool,

    /// Verify only the files a `--select-file` download asked for. Accepts
    /// ranges: 1-5,8.
    ///
    /// A piece the selection does not cover is reported as not selected rather
    /// than as bad, and does not fail the run. Without this, verifying what
    /// `download --select-file` wrote reports every piece outside the
    /// selection as a failure, which is true of the bytes and wrong about the
    /// run. See `TODO/disk-io.md`, T-184.
    #[arg(long, value_name = "INDEX", value_delimiter = ',')]
    pub select_file: Vec<String>,

    /// Skip these files, as `--select-file`'s complement.
    #[arg(long, value_name = "INDEX", value_delimiter = ',')]
    pub exclude_file: Vec<String>,

    /// Where a file was written, as INDEX=PATH, for a payload downloaded with
    /// `-O`/`--index-out`.
    ///
    /// `verify` looks where the bytes went rather than where the torrent said
    /// they would go, and a file the caller renamed is somewhere only the
    /// caller knows. Without this, verifying what `download -O` wrote reports
    /// that file as missing. See `TODO/cli-surface.md`, T-116.
    #[arg(short = 'O', long, value_name = "INDEX=PATH")]
    pub index_out: Vec<String>,

    #[command(flatten)]
    pub swarm: SwarmSourceArgs,

    #[command(flatten)]
    pub page: PageSourceArgs,
}

/// `bit-cli create`.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// File or directory to build a torrent from.
    #[arg(value_name = "PATH")]
    pub path: PathBuf,

    /// Write here, or `-` for stdout. Defaults to alongside the input.
    #[arg(short = 'o', long, value_name = "TARGET")]
    pub output: Option<String>,

    /// Torrent name. Defaults to the input filename.
    #[arg(long, value_name = "TEXT")]
    pub name: Option<String>,

    /// Piece length. Accepts binary units. Chosen by heuristic when absent.
    #[arg(long, value_name = "SIZE")]
    pub piece_length: Option<String>,

    /// Metainfo version.
    #[arg(long, value_name = "V", default_value = "v1")]
    pub version: TorrentVersion,

    /// Primary tracker.
    #[arg(long, value_name = "URL")]
    pub announce: Option<String>,

    /// Add a BEP 12 tier. Repeatable. Comma-separates within a tier.
    #[arg(long, value_name = "URLS", value_delimiter = ',')]
    pub announce_tier: Vec<String>,

    /// Web seed written into `url-list` (BEP 19).
    #[arg(long, value_name = "URL")]
    pub web_seed: Vec<String>,

    /// HTTP seed written into `httpseeds` (BEP 17).
    #[arg(long, value_name = "URL")]
    pub http_seed: Vec<String>,

    /// DHT bootstrap node written into the torrent.
    #[arg(long, value_name = "HOST:PORT")]
    pub node: Vec<String>,

    /// Free-text comment.
    #[arg(long, value_name = "TEXT")]
    pub comment: Option<String>,

    /// The `source` key in the info dict. Changes the info hash.
    #[arg(long, value_name = "TEXT")]
    pub source: Option<String>,

    /// BEP 39 feed URL.
    #[arg(long, value_name = "URL")]
    pub update_url: Option<String>,

    /// Set the private flag (BEP 27).
    #[arg(long)]
    pub private: bool,

    /// Write per-file MD5 checksums. MD5 is not collision resistant.
    #[arg(long)]
    pub md5: bool,

    /// Include or, with a leading `!`, exclude paths.
    #[arg(long, value_name = "GLOB")]
    pub glob: Vec<String>,

    /// Respect .gitignore, .ignore, and .git/info/exclude.
    #[arg(long)]
    pub ignore: bool,

    /// Include hidden files.
    #[arg(long)]
    pub include_hidden: bool,

    /// Include junk files such as .DS_Store and Thumbs.db.
    #[arg(long)]
    pub include_junk: bool,

    /// Follow symlinks.
    #[arg(long)]
    pub follow_symlinks: bool,

    /// Deterministic file ordering, as KEY:ORDER.
    #[arg(long, value_name = "KEY:ORDER", default_value = "path:asc")]
    pub sort_by: String,

    /// Omit the `created by` field.
    #[arg(long)]
    pub no_created_by: bool,

    /// Omit the creation date. Required for byte-reproducible output.
    #[arg(long)]
    pub no_creation_date: bool,

    /// Permit a lint that would otherwise refuse the build. Repeatable.
    #[arg(long, value_name = "LINT")]
    pub allow: Vec<String>,

    /// Overwrite an existing output file.
    #[arg(long)]
    pub force: bool,

    /// Print the magnet URI to stdout.
    #[arg(long)]
    pub link: bool,

    /// Print a summary of what was created.
    #[arg(long)]
    pub show: bool,
}

/// Metainfo version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum TorrentVersion {
    V1,
    V2,
    Hybrid,
}

/// `bit-cli edit`.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// The torrent to read.
    #[arg(value_name = "TORRENT")]
    pub torrent: PathBuf,

    /// Write here, or `-` for stdout. Never edits in place.
    #[arg(short = 'o', long, value_name = "TARGET")]
    pub output: Option<String>,

    /// Replace the primary tracker.
    #[arg(long, value_name = "URL")]
    pub announce: Option<String>,

    /// Add a BEP 12 tier. Repeatable.
    #[arg(long, value_name = "URLS", value_delimiter = ',')]
    pub announce_tier: Vec<String>,

    /// Drop every tracker.
    #[arg(long)]
    pub no_announce: bool,

    /// Add a web seed to `url-list`.
    #[arg(long, value_name = "URL")]
    pub web_seed: Vec<String>,

    /// Replace `url-list` rather than adding to it.
    #[arg(long)]
    pub replace_web_seeds: bool,

    /// Drop every web seed.
    #[arg(long, conflicts_with = "web_seed")]
    pub no_web_seed: bool,

    /// Add an HTTP seed to `httpseeds`.
    #[arg(long, value_name = "URL")]
    pub http_seed: Vec<String>,

    /// Replace the comment.
    #[arg(long, value_name = "TEXT")]
    pub comment: Option<String>,

    /// Drop the comment.
    #[arg(long, conflicts_with = "comment")]
    pub no_comment: bool,

    /// Replace the `created by` field.
    #[arg(long, value_name = "TEXT")]
    pub created_by: Option<String>,

    /// Drop the creation date.
    #[arg(long)]
    pub no_creation_date: bool,

    /// Add a DHT bootstrap node.
    #[arg(long, value_name = "HOST:PORT")]
    pub node: Vec<String>,

    /// Replace the BEP 39 feed URL.
    #[arg(long, value_name = "URL")]
    pub update_url: Option<String>,

    /// Permit an edit that changes the info hash.
    #[arg(long)]
    pub allow_new_infohash: bool,

    /// Overwrite an existing output file.
    #[arg(long)]
    pub force: bool,
}

/// `bit-cli seed`.
#[derive(Debug, Args)]
pub struct SeedArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub trackers: TrackerArgs,

    #[command(flatten)]
    pub limits: LimitArgs,

    #[command(flatten)]
    pub hooks: HookArgs,

    /// Where the payload already lives. Defaults to --dir.
    #[arg(long, value_name = "PATH")]
    pub data: Option<PathBuf>,

    /// Serve this file from this path, as INDEX=PATH.
    ///
    /// The same flag `download` writes with and `verify` reads with, and for
    /// the same reason: a payload fetched with `download -O 0=renamed.bin` is
    /// on disk under a name only the caller knows, and a seeder that looks
    /// where the torrent said finds nothing there. See
    /// `TODO/cli-surface.md`, T-213.
    #[arg(short = 'O', long, value_name = "INDEX=PATH")]
    pub index_out: Vec<String>,

    /// Hash-check before announcing.
    ///
    /// `full` is what happens today whatever this says: the session hash-checks
    /// the whole payload on add and offers no way to skip it. `quick` and
    /// `none` are accepted, warn, and do the same thing. `--fastresume` is the
    /// flag that skips the check. See `TODO/disk-io.md`, T-016.
    #[arg(long, value_name = "MODE", default_value = "full")]
    pub verify: SeedVerify,

    /// Reuse the previous run's hash check when the payload has not changed.
    #[arg(long)]
    pub fastresume: bool,

    /// Where the resume cache lives. Default: .bit-cli-resume beside the data.
    #[arg(long, value_name = "DIR")]
    pub fastresume_dir: Option<PathBuf>,

    /// BEP 16 superseeding for initial distribution.
    #[arg(long)]
    pub superseed: bool,

    /// Announce, report the tracker response, do not serve.
    #[arg(long)]
    pub announce_only: bool,

    /// Listen port, or a range as START-END.
    #[arg(long, value_name = "PORT")]
    pub port: Vec<String>,

    /// Disable the DHT.
    #[arg(long)]
    pub no_dht: bool,

    /// Disable peer exchange.
    #[arg(long)]
    pub no_pex: bool,

    /// Disable local service discovery.
    #[arg(long)]
    pub no_lsd: bool,

    /// Emit a progress event this often.
    #[arg(long, value_name = "DUR", default_value = "5s")]
    pub report_interval: String,

    /// Exit after this long with no connected peers.
    #[arg(long, value_name = "DUR")]
    pub exit_when_idle: Option<String>,

    /// Check this often that our own listener still answers. Off by default.
    ///
    /// A seeder that cannot be handshaked is down, and nothing a supervisor
    /// normally watches says so: the process is alive, the port is open, and
    /// the ratio still gets reported.
    ///
    /// The failure it was written for is fixed. `librqbit` 9.0.0's accept loop
    /// disabled its own drain arm on the first handshake check that errored,
    /// so a run of peers closing before they handshook left a backlog and
    /// twenty were enough to stop a seeder serving anybody; the vendored tree
    /// handles every outcome and the backlog clears in one connection. See
    /// `TODO/peers.md`, T-020. What the check is for now is the general
    /// question rather than that one answer: whether this process still
    /// answers a handshake.
    ///
    /// Each check dials this run's own listen port over loopback and completes
    /// a real handshake for a torrent it is serving. Three failures in a row
    /// stop the run with `"stopped": "listener_unhealthy"` and exit 17, which
    /// is a restart a supervisor can act on. It costs the session one peer row
    /// per check, which is dropped from the reported peer list and never
    /// counted as a swarm member.
    #[arg(long, value_name = "DUR")]
    pub listener_check: Option<String>,
}

/// How much to hash-check before seeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum SeedVerify {
    Full,
    Quick,
    None,
}

/// `bit-cli bench`.
///
/// Same size spread as [`Command`], and the same reasoning: parsed once, lives
/// for the run, not worth a box.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub enum BenchCommand {
    /// Download from a target and measure.
    Leech(BenchLeechArgs),
    /// Seed and measure what the swarm pulls.
    Seed(BenchSeedArgs),
    /// Measure HTTP sources: latency percentiles, concurrency scaling, ranges.
    Webseed(BenchWebseedArgs),
    /// Measure the payload file under several writers, with no session.
    Disk(BenchDiskArgs),
    /// Synthetic peer load against a target.
    Swarm(BenchSwarmArgs),
    /// One-shot capability and reachability probe.
    Probe(BenchProbeArgs),
}

/// Flags shared by every `bench` subcommand.
///
/// The report goes to stdout unless `--report <PATH>` names a file, in which
/// case stdout carries the text summary instead. `--format` decides how the
/// report is written; `--json` and `--jsonl` set it to `json` and `ndjson`.
#[derive(Debug, Args, Clone)]
#[command(next_help_heading = "Benchmark options")]
pub struct BenchShared {
    /// How long to run.
    #[arg(long, value_name = "DUR", default_value = "30s")]
    pub duration: String,

    /// Discard measurements from this initial window.
    #[arg(long, value_name = "DUR", default_value = "3s")]
    pub warmup: String,

    /// Time series resolution.
    #[arg(long, value_name = "DUR", default_value = "1s")]
    pub metrics_interval: String,

    /// Drive toward this rate rather than running flat out.
    #[arg(long, value_name = "RATE")]
    pub target_rate: Option<String>,

    /// Fixed concurrency.
    #[arg(long, value_name = "N", default_value_t = 8)]
    pub concurrency: usize,

    /// Step concurrency and report the curve.
    #[arg(long, value_name = "SPEC")]
    pub concurrency_sweep: Option<String>,

    /// Cap generated payload on disk.
    #[arg(long, value_name = "SIZE", default_value = "8GiB")]
    pub disk_budget: String,

    /// Bytes per request. Defaults to the source's own chunk size.
    #[arg(long, value_name = "SIZE")]
    pub request_size: Option<String>,

    /// A rate to report the result as a share of, such as what curl reached
    /// against the same URL.
    #[arg(long, value_name = "RATE")]
    pub ceiling: Option<String>,

    #[command(flatten)]
    pub report: ReportArgs,
}

/// Where a `bench` report goes and what it is checked against.
///
/// These are separate from the rest of [`BenchShared`] because every
/// subcommand has them and not every subcommand has a duration or a
/// concurrency. A flag that a subcommand cannot honour does not appear on it.
#[derive(Debug, Args, Clone, Default)]
#[command(next_help_heading = "Report options")]
pub struct ReportArgs {
    /// Write the full report here, or `-` for stdout. Default: stdout.
    #[arg(long, value_name = "PATH")]
    pub report: Option<String>,

    /// Report format: json, ndjson, csv, or text. `csv` carries the time
    /// series only, because a report is nested and a table is not.
    #[arg(long, value_name = "FMT", default_value = "json")]
    pub format: ReportFormat,

    /// Compare against a previous report and print the delta.
    #[arg(long, value_name = "PATH")]
    pub baseline: Option<PathBuf>,

    /// Exit 14 if sustained throughput falls below this.
    #[arg(long, value_name = "RATE")]
    pub fail_under: Option<String>,
}

/// How a bench report is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum ReportFormat {
    #[default]
    Json,
    Ndjson,
    Csv,
    Text,
}

/// `bit-cli bench probe`.
///
/// One exchange with one target, which is either a peer address or an HTTP
/// endpoint. It moves no payload and has no time series, so it takes only the
/// report flags and a deadline rather than the shared benchmark set.
#[derive(Debug, Args)]
#[command(next_help_heading = "Probe options")]
pub struct BenchProbeArgs {
    /// `HOST:PORT` for a peer, or an `http(s)://` URL for a mirror.
    #[arg(value_name = "TARGET", help_heading = None)]
    pub target: String,

    /// The torrent to ask a peer about, as a `.torrent`, a magnet, or an info
    /// hash.
    ///
    /// A BitTorrent handshake names a torrent, and a peer that does not have
    /// the one it was asked about is entitled to hang up. Without this the
    /// probe sends a zero info hash, which reaches the handshake and usually
    /// no further.
    #[arg(long = "for", value_name = "SOURCE")]
    pub source: Option<String>,

    /// How long to wait for each step, and how long to listen after the
    /// handshake.
    #[arg(long, value_name = "DUR", default_value = "10s")]
    pub timeout: String,

    #[command(flatten)]
    pub report: ReportArgs,
}
/// `bit-cli bench swarm`.
///
/// Two loads under one verb, and `--for` is what chooses. With it, the target
/// already serves the torrent and the synthetic peers leech it, which measures
/// its serving path. Without it, the peers handshake for generated info hashes
/// the target does not have, which measures its accept path. See
/// `TODO/bench.md`, T-092, for why those are two loads and not one.
#[derive(Debug, Args)]
#[command(next_help_heading = "Swarm options")]
pub struct BenchSwarmArgs {
    /// `HOST:PORT` of the peer to load. The only address this ever connects
    /// to: it announces to no tracker, uses no DHT, and reads no peer list.
    ///
    /// `help_heading = None` keeps the positional in clap's own "Arguments"
    /// section rather than under this struct's heading, which is where every
    /// other command's positional is.
    #[arg(value_name = "TARGET", help_heading = None)]
    pub target: String,

    /// A torrent the target already serves, as a `.torrent` path. Repeatable.
    ///
    /// With none, `--torrents` info hashes are generated instead and the
    /// target will not have any of them, which measures how it handles
    /// connections it cannot serve rather than how fast it serves.
    #[arg(long = "for", value_name = "TORRENT")]
    pub for_torrents: Vec<PathBuf>,

    /// Synthetic peer count.
    #[arg(long, value_name = "N", default_value_t = 8)]
    pub peers: usize,

    /// How many torrents to generate. Ignored when `--for` is given.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub torrents: usize,

    /// The length a generated torrent declares. No payload is written for it:
    /// the target does not have the torrent, so nothing will ever be fetched
    /// or checked against it.
    #[arg(long, value_name = "SIZE", default_value = "256MiB")]
    pub payload_size: String,

    /// The piece length a generated torrent declares.
    #[arg(long, value_name = "SIZE", default_value = "1MiB")]
    pub piece_size: String,

    /// Where verified pieces and generated torrents are written. A directory
    /// this run makes and removes when not given.
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,

    /// How long one connect attempt gets before the peer gives up on it.
    #[arg(long, value_name = "DUR", default_value = "10s")]
    pub connect_timeout: String,

    /// Keep the scratch directory instead of removing it.
    #[arg(long)]
    pub keep: bool,

    // Flattened last, deliberately. `next_help_heading` is a running setting on
    // the `clap` command rather than a property of the struct that set it, so
    // every argument declared after a flatten inherits whatever heading that
    // flatten left behind. `BenchShared` ends by flattening `ReportArgs`, so
    // anything after it here would be filed under "Report options".
    // `TODO/cli-surface.md`, T-159.
    #[command(flatten)]
    pub shared: BenchShared,
}

/// `bit-cli bench leech`.
///
/// A download with the clock running, so it carries the same source, tracker,
/// and limit flags `download` does. The payload has to land somewhere real:
/// the point of the measurement is what a download costs, and one that never
/// writes is measuring something else.
#[derive(Debug, Args)]
#[command(next_help_heading = "Leech options")]
pub struct BenchLeechArgs {
    /// Listen port, or a range as START-END. `0` asks the OS for a free one.
    #[arg(long, value_name = "PORT")]
    pub port: Vec<String>,

    /// Try this peer before any are discovered, as HOST:PORT. Repeatable.
    #[arg(long = "peer", value_name = "ADDR")]
    pub peers: Vec<String>,

    /// How disk space is allocated for the payload.
    #[arg(long, value_name = "METHOD", default_value = "sparse")]
    pub file_allocation: FileAllocation,

    /// Overwrite whatever is already in the output directory.
    ///
    /// A benchmark run twice against the same directory would otherwise find
    /// the payload already there, hash-check it, finish immediately, and
    /// report a rate that is the hash checker's rather than the network's.
    #[arg(long, default_value_t = true, overrides_with = "keep_existing")]
    pub allow_overwrite: bool,

    /// Keep what is already in the output directory and resume onto it.
    #[arg(long, overrides_with = "allow_overwrite")]
    pub keep_existing: bool,

    /// Stop once the torrent completes, rather than running out `--duration`.
    /// On by default.
    #[arg(long, default_value_t = true, overrides_with = "run_full_duration")]
    pub stop_on_complete: bool,

    /// Keep running until `--duration` elapses even after the payload is in.
    #[arg(long, overrides_with = "stop_on_complete")]
    pub run_full_duration: bool,

    // Flattened last, so that every argument above keeps this struct's heading.
    // T-159 has why.
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub web_seeds: WebSeedArgs,

    #[command(flatten)]
    pub trackers: TrackerArgs,

    #[command(flatten)]
    pub limits: LimitArgs,

    #[command(flatten)]
    pub shared: BenchShared,
}

/// `bit-cli bench seed`.
///
/// The same envelope as `bench leech` with the counters facing the other way:
/// what leaves rather than what arrives, per peer rather than per source. See
/// `TODO/bench.md`, T-090.
#[derive(Debug, Args)]
#[command(next_help_heading = "Seed options")]
pub struct BenchSeedArgs {
    /// Where the payload already lives, when that is not `--dir`.
    #[arg(long, value_name = "DIR")]
    pub data: Option<PathBuf>,

    /// Listen port, or a range as START-END. `0` asks the OS for a free one.
    #[arg(long, value_name = "PORT")]
    pub port: Vec<String>,

    /// Disable the DHT.
    #[arg(long)]
    pub no_dht: bool,

    /// Disable local service discovery.
    #[arg(long)]
    pub no_lsd: bool,

    /// Stop once no peer has been connected for this long.
    ///
    /// A seeder nobody pulls from measures nothing, and waiting out
    /// `--duration` to find that out wastes the caller's time. Off by default,
    /// because a run that expects a leecher to arrive late wants to wait.
    #[arg(long, value_name = "DUR")]
    pub exit_when_idle: Option<String>,

    /// Measure the payload's hash check on add as well.
    ///
    /// A seeder reads and hashes the whole payload before it serves a byte,
    /// and that read is normally not part of what is being measured. With this
    /// on, the report carries how long it took and how fast it went.
    #[arg(long)]
    pub include_hash_check: bool,

    // Flattened last. T-159.
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub trackers: TrackerArgs,

    #[command(flatten)]
    pub limits: LimitArgs,

    #[command(flatten)]
    pub shared: BenchShared,
}

/// `bit-cli bench webseed`.
#[derive(Debug, Args)]
pub struct BenchWebseedArgs {
    #[command(flatten)]
    pub source: SourceArgs,

    #[command(flatten)]
    pub web_seeds: WebSeedArgs,

    #[command(flatten)]
    pub shared: BenchShared,
}

/// `bit-cli bench disk`.
///
/// No torrent and no session: the same storage a download writes through,
/// driven straight from N threads. It takes only the shared flags it can
/// honour, because a fixed number of bytes has no warmup window and no target
/// rate. See `TODO/disk-io.md`, T-017.
#[derive(Debug, Args)]
#[command(next_help_heading = "Disk options")]
pub struct BenchDiskArgs {
    /// Where the payload is written. Defaults to a directory this run makes
    /// under the system temporary directory and removes afterwards.
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,

    /// Total bytes written per step.
    #[arg(long, value_name = "SIZE", default_value = "1GiB")]
    pub payload_size: String,

    /// Bytes per positioned write. The peer protocol's block is 16 KiB.
    #[arg(long, value_name = "SIZE", default_value = "16KiB")]
    pub block_size: String,

    /// How many threads write at once.
    #[arg(long, value_name = "N", default_value_t = 8)]
    pub concurrency: usize,

    /// Step the thread count and report the curve, for example `1,2,4,8`.
    #[arg(long, value_name = "SPEC")]
    pub concurrency_sweep: Option<String>,

    /// How the payload is spread over files. `shared` is one file with every
    /// thread interleaving into it, which is where writes contend. `split`
    /// gives each thread its own file, which is the control.
    #[arg(long, value_name = "LAYOUT", default_value = "shared")]
    pub layout: DiskLayout,

    /// Consecutive blocks one thread writes before the next takes over, under
    /// `shared` and `handles`. 1 strides block by block, which contends most.
    /// A receive path writes a whole fetched range at a time, so `64` at the
    /// default block size is the shape a download has.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub run_length: u64,

    /// How disk space is allocated for the payload.
    #[arg(long, value_name = "METHOD", default_value = "sparse")]
    pub file_allocation: FileAllocation,

    /// How many payload files stay open at once. 0 uses the storage default.
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub max_open_files: usize,

    /// Time series resolution.
    #[arg(long, value_name = "DUR", default_value = "1s")]
    pub metrics_interval: String,

    /// Stop a step once this much wall time has passed.
    #[arg(long, value_name = "DUR", default_value = "300s")]
    pub duration: String,

    /// Skip the read-back that checks every block landed where it was sent.
    #[arg(long)]
    pub no_verify: bool,

    // Flattened last. T-159.
    #[command(flatten)]
    pub report: ReportArgs,
}

/// How `bench disk` spreads the payload over files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum DiskLayout {
    /// One file, every thread interleaving blocks into it.
    #[default]
    Shared,
    /// One file per thread, each writing only its own.
    Split,
    /// One file opened once per thread, each writing through its own handle.
    Handles,
}

impl From<DiskLayout> for bit_cli_core::bench::disk::Layout {
    fn from(layout: DiskLayout) -> Self {
        match layout {
            DiskLayout::Shared => Self::Shared,
            DiskLayout::Split => Self::Split,
            DiskLayout::Handles => Self::Handles,
        }
    }
}

/// `bit-cli config`.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the fully resolved configuration with the origin of every value.
    Show,
}

/// `bit-cli completions`.
#[derive(Debug, Args)]
pub struct CompletionsArgs {
    /// Which shell to generate for.
    #[arg(value_name = "SHELL")]
    pub shell: Shell,
}

/// Shells completions can be generated for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
    Nushell,
}

/// `bit-cli man`.
#[derive(Debug, Args)]
pub struct ManArgs {
    /// Write the man page here instead of to stdout.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// What to render. `roff` is the man page; `json` is the same surface as a
    /// CLIspec document, for a reader that cannot parse roff.
    #[arg(long, value_name = "FMT", default_value = "roff")]
    pub format: ManFormat,
}

/// What `bit-cli man` renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ManFormat {
    /// A troff man page, the committed `man/bit-cli.1`.
    #[default]
    Roff,
    /// A CLIspec 0.3 document, the committed `man/bit-cli.json`.
    Json,
    /// The same manual as Markdown, the committed `man/bit-cli.md`.
    Markdown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_definition_is_internally_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_bare_source_is_a_download() {
        let cli = Cli::try_parse_from(["bit-cli", "a.torrent"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.sources, ["a.torrent"]);
    }

    #[test]
    fn the_download_subcommand_takes_the_same_source() {
        let cli = Cli::try_parse_from(["bit-cli", "download", "a.torrent"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Download(_))));
    }

    #[test]
    fn global_flags_work_before_and_after_the_subcommand() {
        for args in [
            ["bit-cli", "--json", "info", "a.torrent"],
            ["bit-cli", "info", "--json", "a.torrent"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(cli.global.json, "{args:?}");
        }
    }

    #[test]
    fn json_and_jsonl_cannot_both_be_asked_for() {
        assert!(
            Cli::try_parse_from(["bit-cli", "--json", "--jsonl", "info", "a.torrent"]).is_err()
        );
    }

    #[test]
    fn short_flags_keep_their_aria2_meanings() {
        let cli = Cli::try_parse_from([
            "bit-cli",
            "download",
            "-d",
            "/out",
            "-j",
            "4",
            "-V",
            "-c",
            "-u",
            "1MiB",
            "a.torrent",
        ])
        .unwrap();
        assert_eq!(
            cli.global.dir.as_deref(),
            Some(std::path::Path::new("/out"))
        );
        let Some(Command::Download(args)) = cli.command else {
            panic!("expected download")
        };
        assert_eq!(args.max_concurrent_downloads, 4);
        assert!(args.check_integrity);
        assert!(args.r#continue);
        assert_eq!(args.limits.max_upload_rate.as_deref(), Some("1MiB"));
    }

    #[test]
    fn v_is_verbosity_and_not_version() {
        let cli = Cli::try_parse_from(["bit-cli", "-vvv", "info", "a.torrent"]).unwrap();
        assert_eq!(cli.global.verbose, 3);
        // --version still works in its long form.
        let err = Cli::try_parse_from(["bit-cli", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn verbosity_raises_the_log_level_without_passing_the_top() {
        assert_eq!(LogLevel::Warn.raised(0), LogLevel::Warn);
        assert_eq!(LogLevel::Warn.raised(1), LogLevel::Info);
        assert_eq!(LogLevel::Warn.raised(2), LogLevel::Debug);
        assert_eq!(LogLevel::Warn.raised(3), LogLevel::Trace);
        assert_eq!(LogLevel::Warn.raised(99), LogLevel::Trace);
    }

    #[test]
    fn trace_subsystems_accept_commas_and_repetition() {
        let cli = Cli::try_parse_from([
            "bit-cli",
            "--trace",
            "http,piece",
            "--trace",
            "picker",
            "info",
            "a.torrent",
        ])
        .unwrap();
        assert_eq!(cli.global.trace, ["http", "piece", "picker"]);
    }

    #[test]
    fn every_web_seed_flag_parses() {
        let cli = Cli::try_parse_from([
            "bit-cli",
            "download",
            "--web-seed",
            "https://a.example.com/pub/",
            "--web-seed-exact",
            "https://cdn.example.com/blob",
            "--web-seed-for",
            "piece:0-511=https://b.example.com/",
            "--web-seed-mode",
            "prefix",
            "--web-seed-chunk-size",
            "4MiB",
            "--web-seed-header",
            "X-Region: apac",
            "--web-seed-auth",
            "bearer:tok",
            "--web-seed-only",
            "--web-seed-require",
            "a.torrent",
        ])
        .unwrap();
        let Some(Command::Download(args)) = cli.command else {
            panic!("expected download")
        };
        let ws = &args.web_seeds;
        assert_eq!(ws.web_seed, ["https://a.example.com/pub/"]);
        assert_eq!(ws.web_seed_exact, ["https://cdn.example.com/blob"]);
        assert_eq!(ws.web_seed_for, ["piece:0-511=https://b.example.com/"]);
        assert_eq!(ws.web_seed_mode, WebSeedMode::Prefix);
        assert_eq!(ws.web_seed_chunk_size.as_deref(), Some("4MiB"));
        assert_eq!(ws.web_seed_header, ["X-Region: apac"]);
        assert!(ws.web_seed_only);
        assert!(ws.web_seed_require);
    }

    #[test]
    fn no_web_seed_and_no_torrent_web_seed_are_mutually_exclusive() {
        assert!(
            Cli::try_parse_from([
                "bit-cli",
                "download",
                "--no-web-seed",
                "--no-torrent-web-seed",
                "a.torrent"
            ])
            .is_err()
        );
    }

    #[test]
    fn webseed_fetch_refuses_conflicting_range_selectors() {
        assert!(
            Cli::try_parse_from([
                "bit-cli",
                "webseed",
                "fetch",
                "--piece",
                "1",
                "--bytes",
                "0-100",
                "a.torrent"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["bit-cli", "webseed", "fetch", "--piece", "1", "a.torrent"])
                .is_ok()
        );
    }

    #[test]
    fn create_takes_the_full_metainfo_surface() {
        let cli = Cli::try_parse_from([
            "bit-cli",
            "create",
            "--announce",
            "udp://a:80",
            "--announce-tier",
            "udp://b:80,udp://c:80",
            "--web-seed",
            "https://e.com/pub/",
            "--piece-length",
            "1MiB",
            "--private",
            "--no-creation-date",
            "--allow",
            "empty-file",
            "--sort-by",
            "path:asc",
            "./payload",
        ])
        .unwrap();
        let Some(Command::Create(args)) = cli.command else {
            panic!("expected create")
        };
        assert_eq!(args.announce.as_deref(), Some("udp://a:80"));
        assert_eq!(args.announce_tier, ["udp://b:80", "udp://c:80"]);
        assert_eq!(args.piece_length.as_deref(), Some("1MiB"));
        assert!(args.private);
        assert!(args.no_creation_date);
        assert_eq!(args.allow, ["empty-file"]);
    }

    #[test]
    fn every_subcommand_has_help_text() {
        for sub in Cli::command().get_subcommands() {
            assert!(
                sub.get_about().is_some(),
                "`{}` has no help text",
                sub.get_name()
            );
        }
    }

    /// A help heading exists so a reader can find a flag by what it does, and
    /// one that files `--peers` beside `--fail-under` is worse than none.
    ///
    /// `next_help_heading` is a running setting on the `clap` command rather
    /// than a property of the struct that set it, so every argument declared
    /// after a `#[command(flatten)]` inherits whatever heading that flatten
    /// left behind. `BenchShared` ends by flattening `ReportArgs`, so four of
    /// the six `bench` subcommands filed their own flags under "Report
    /// options". `TODO/cli-surface.md`, T-159.
    ///
    /// This asserts the property rather than the fix, so flattening last is
    /// not the only shape that passes and the next subcommand cannot
    /// reintroduce the defect by declaring one flag in the wrong place.
    #[test]
    fn only_report_flags_are_filed_under_report_options() {
        const REPORT_FLAGS: [&str; 4] = ["report", "format", "baseline", "fail_under"];
        let command = Cli::command();
        let bench = command
            .get_subcommands()
            .find(|sub| sub.get_name() == "bench")
            .expect("bench is a subcommand");
        let mut checked = 0;
        for sub in bench.get_subcommands() {
            for arg in sub.get_arguments() {
                if arg.get_help_heading() != Some("Report options") {
                    continue;
                }
                let id = arg.get_id().as_str();
                assert!(
                    REPORT_FLAGS.contains(&id),
                    "`bench {}`: `--{}` is filed under \"Report options\" and is not a report \
                     option. Declare it before the flatten that sets that heading, or give it \
                     its own. TODO/cli-surface.md, T-159.",
                    sub.get_name(),
                    id.replace('_', "-")
                );
            }
            checked += 1;
        }
        assert_eq!(checked, 6, "every bench subcommand is walked");
    }

    /// The other half of the same rule: the report flags are still *there*.
    /// A heading that files nothing under it would pass the case above.
    #[test]
    fn every_bench_subcommand_files_its_report_flags_under_report_options() {
        let command = Cli::command();
        let bench = command
            .get_subcommands()
            .find(|sub| sub.get_name() == "bench")
            .expect("bench is a subcommand");
        for sub in bench.get_subcommands() {
            let filed: Vec<_> = sub
                .get_arguments()
                .filter(|arg| arg.get_help_heading() == Some("Report options"))
                .map(|arg| arg.get_id().to_string())
                .collect();
            assert_eq!(
                filed.len(),
                4,
                "`bench {}` files {filed:?} under \"Report options\"",
                sub.get_name()
            );
        }
    }

    /// A positional belongs in `clap`'s own "Arguments" section on every
    /// command, and a struct-level heading would otherwise pull it into that
    /// heading's section and render it after the flags. T-159.
    #[test]
    fn no_positional_is_pulled_into_a_help_heading() {
        fn walk(cmd: &clap::Command, path: &str) {
            for arg in cmd.get_arguments() {
                if arg.is_positional() {
                    assert_eq!(
                        arg.get_help_heading(),
                        None,
                        "`{path}{}`: a positional carries the heading `{:?}`",
                        arg.get_id(),
                        arg.get_help_heading()
                    );
                }
            }
            for sub in cmd.get_subcommands() {
                walk(sub, &format!("{path}{} ", sub.get_name()));
            }
        }
        walk(&Cli::command(), "");
    }

    #[test]
    fn no_short_flag_is_defined_twice() {
        use std::collections::HashMap;
        let command = Cli::command();
        let mut seen: HashMap<char, Vec<String>> = HashMap::new();
        let mut collect = |cmd: &clap::Command, prefix: &str| {
            for arg in cmd.get_arguments() {
                if let Some(short) = arg.get_short() {
                    seen.entry(short)
                        .or_default()
                        .push(format!("{prefix}{}", arg.get_id()));
                }
            }
        };
        collect(&command, "");
        for sub in command.get_subcommands() {
            let mut local: HashMap<char, Vec<String>> = HashMap::new();
            for arg in sub.get_arguments() {
                if let Some(short) = arg.get_short() {
                    local
                        .entry(short)
                        .or_default()
                        .push(arg.get_id().to_string());
                }
            }
            for (short, ids) in local {
                assert_eq!(
                    ids.len(),
                    1,
                    "`-{short}` is defined twice in `{}`: {ids:?}",
                    sub.get_name()
                );
            }
        }
    }

    #[test]
    fn short_flags_never_contradict_aria2() {
        // Letters aria2 assigns, and the `bit-cli` flag names that mean the
        // same concept. A short flag carrying one of these letters must name
        // one of the listed ids or not exist at all. Several names appear
        // where `bit-cli` spells the same concept differently in different
        // subcommands (`--out` for a payload, `--output` for a file it writes).
        let aria2: &[(char, &[&str])] = &[
            ('d', &["dir"]),
            ('o', &["out", "output"]),
            ('j', &["max-concurrent-downloads"]),
            ('u', &["max-upload-rate"]),
            ('q', &["quiet"]),
            ('c', &["continue"]),
            ('V', &["check-integrity"]),
            ('O', &["index-out"]),
            ('l', &["log-file"]),
        ];
        let command = Cli::command();
        let mut found: Vec<(char, String)> = Vec::new();
        for arg in command.get_arguments() {
            if let Some(short) = arg.get_short() {
                found.push((short, arg.get_id().to_string()));
            }
        }
        for sub in command.get_subcommands() {
            for arg in sub.get_arguments() {
                if let Some(short) = arg.get_short() {
                    found.push((short, arg.get_id().to_string()));
                }
            }
        }
        for (short, id) in found {
            if let Some((_, accepted)) = aria2.iter().find(|(c, _)| *c == short) {
                let name = id.replace('_', "-");
                assert!(
                    accepted.contains(&name.as_str()),
                    "`-{short}` means {accepted:?} in aria2 but `{name}` here"
                );
            }
        }
    }

    #[test]
    fn version_has_no_short_form() {
        // `clap` would give it `-V`, which `aria2` assigns to
        // `--check-integrity`. Reassigning an `aria2` letter to a different
        // concept is exactly what lets a script do something else silently.
        assert!(
            Cli::try_parse_from(["bit-cli", "-V"]).is_err(),
            "-V at the top level must not be --version"
        );
        let cli = Cli::try_parse_from(["bit-cli", "download", "-V", "a.torrent"]).unwrap();
        let Some(Command::Download(args)) = cli.command else {
            panic!("expected download")
        };
        assert!(args.check_integrity, "-V has to keep its aria2 meaning");
    }

    /// Every flag reaches code, or is on a named list of the ones that do not.
    ///
    /// The audit that found [T-181](../../../TODO/cli-surface.md) was one
    /// command: every `pub` field in this file grepped for a reader outside
    /// it. Four flags had none, and nothing had noticed because a flag that
    /// parses and is never read looks exactly like one that works. This is
    /// that command, mechanised, so a fifth cannot be added silently.
    ///
    /// "Reaches code" here means the field name appears somewhere in the
    /// workspace outside this file. That is deliberately weak: it cannot tell
    /// a flag that works from one that only warns, and warning is the honest
    /// behaviour for a flag that cannot yet do what it says
    /// (`cmd/seed.rs`, `--superseed` and `--no-pex`). What it does catch is
    /// the case that hid for a whole session, which is a field nothing reads
    /// at all.
    #[test]
    fn every_flag_reaches_code_or_is_a_named_exception() {
        /// Fields nothing outside `cli.rs` reads, each with the entry that
        /// owns it.
        ///
        /// A name belongs here only while an entry is open that will remove
        /// it. Adding one without an entry is how a review list stops being a
        /// review.
        // Empty, and that is the state to keep it in. T-116 and T-115 were
        // the last two rows and both closed on 2026-08-23.
        const ACCEPTED_WITHOUT_A_READER: &[(&str, &str)] = &[];

        // Read the workspace source rather than `include_str!`ing a fixed
        // list, because a file added later would otherwise silently stop
        // being searched, which is the same class of gap this test exists for.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let roots = [
            manifest.join("src"),
            manifest.join("tests"),
            manifest.join("../bit-cli-core/src"),
            manifest.join("../bit-cli-core/tests"),
        ];
        let this_file = manifest.join("src").join("cli.rs");
        let mut haystack = String::new();
        let mut files_read = 0usize;
        for root in &roots {
            let mut stack = vec![root.clone()];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|e| e == "rs")
                        && path.canonicalize().ok() != this_file.canonicalize().ok()
                        && let Ok(text) = std::fs::read_to_string(&path)
                    {
                        haystack.push_str(&text);
                        files_read += 1;
                    }
                }
            }
        }
        assert!(
            files_read > 20,
            "only {files_read} source files were read, so this test is not looking at the workspace and would pass whatever it was given"
        );

        let command = Cli::command();
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut collect = |cmd: &clap::Command, path: &str| {
            for arg in cmd.get_arguments() {
                // `--help` and `--version` are clap's own and have no field.
                if arg.get_long().is_none() || arg.is_global_set() && arg.get_id() == "help" {
                    continue;
                }
                if matches!(arg.get_id().as_str(), "help" | "version") {
                    continue;
                }
                fields.push((arg.get_id().to_string(), path.to_string()));
            }
        };
        collect(&command, "bit-cli");
        for sub in command.get_subcommands() {
            let name = format!("bit-cli {}", sub.get_name());
            collect(sub, &name);
            for nested in sub.get_subcommands() {
                collect(nested, &format!("{name} {}", nested.get_name()));
            }
        }
        assert!(
            fields.len() > 100,
            "only {} flags found, which cannot be right",
            fields.len()
        );

        let accepted: std::collections::HashMap<&str, &str> =
            ACCEPTED_WITHOUT_A_READER.iter().copied().collect();
        let mut unread: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (field, path) in &fields {
            if !seen.insert(field.clone()) {
                continue;
            }
            if accepted.contains_key(field.as_str()) {
                continue;
            }
            if !haystack.contains(field.as_str()) {
                unread.push(format!("  {field}  ({path})"));
            }
        }
        assert!(
            unread.is_empty(),
            "these flags parse and nothing outside cli.rs reads them. Wire each one up, or warn the way `--superseed` does in cmd/seed.rs and add it to ACCEPTED_WITHOUT_A_READER with the TODO/ entry that owns it:\n{}",
            unread.join(
                "
"
            )
        );

        // The list is a review, so an entry that is no longer needed has to
        // leave it. A name here that something does read is stale.
        for (field, owner) in ACCEPTED_WITHOUT_A_READER {
            assert!(
                !haystack.contains(field),
                "`{field}` is on ACCEPTED_WITHOUT_A_READER for {owner}, and something now reads it. Remove the exception."
            );
        }
    }

    #[test]
    fn every_short_flag_is_documented_in_the_flags_table() {
        // `docs/flags.md` is the table A3.2 requires, and a table nothing
        // checks drifts within a week. This is the check: a short flag with no
        // row fails here rather than being discovered by a user whose script
        // did the wrong thing.
        //
        // It fails in both directions. A flag with no row is a table that has
        // gone stale behind the binary; a row naming a flag that no longer
        // exists is one that has gone stale in front of it, and until
        // 2026-08-23 only the first was checked. See `TODO/cli-surface.md`,
        // T-118.
        let path = flags_path();
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
            .replace("\r\n", "\n");
        let defined = short_flags();
        assert!(
            !defined.is_empty(),
            "no short flags found, which cannot be right"
        );

        if std::env::var_os("BIT_CLI_UPDATE_FLAGS").is_some() {
            std::fs::write(&path, merge_flags_table(&committed, &defined))
                .expect("write docs/flags.md");
            return;
        }

        let documented = documented_flags(&committed);
        let missing: Vec<String> = defined
            .iter()
            .filter(|pair| !documented.contains(*pair))
            .map(|(short, long)| format!("| `-{short}` | `--{long}` |  |  |  |"))
            .collect();
        assert!(
            missing.is_empty(),
            "docs/flags.md has no row for {} short flag(s); add them, or regenerate with \
             BIT_CLI_UPDATE_FLAGS=1 cargo test -p bit-cli --lib short_flag:\n{}",
            missing.len(),
            missing.join("\n")
        );

        let stale: Vec<String> = documented
            .iter()
            .filter(|pair| !defined.contains(*pair))
            .map(|(short, long)| format!("`-{short}`/`--{long}`"))
            .collect();
        assert!(
            stale.is_empty(),
            "docs/flags.md has {} row(s) for short flags the binary does not define: {}. \
             Move them to \"Reserved and not assigned\", or regenerate with \
             BIT_CLI_UPDATE_FLAGS=1 cargo test -p bit-cli --lib short_flag",
            stale.len(),
            stale.join(", ")
        );
    }

    /// The regenerating half, tested on its own rather than against the real
    /// file, because the real file is a no-op for it by construction: the
    /// assertion above fails the build whenever it would not be.
    ///
    /// Three properties, and the first is the one T-158 is about: a row that
    /// is already there is kept **verbatim**, hand-written cells and all. A
    /// generator that rewrites those columns from the command tree cannot,
    /// because the command tree does not know what `aria2` calls a letter.
    #[test]
    fn regenerating_the_flags_table_adds_and_removes_rows_without_touching_prose() {
        let table = "\
# Short flags

## Assigned

| Flag | Long form | Scope | `aria2` | Note |
| --- | --- | --- | --- | --- |
| `-c` | `--continue` | download | `-c` continue | Same concept. |
| `-z` | `--gone` | download | unclaimed | This flag no longer exists. |

## Reserved and not assigned

| `-k` | min split size | Reserved. |
";
        let defined = vec![
            ('c', "continue".to_string()),
            ('d', "dir".to_string()),
            ('h', "help".to_string()),
        ];
        let merged = merge_flags_table(table, &defined);

        // Kept, with every hand-written cell.
        assert!(
            merged.contains("| `-c` | `--continue` | download | `-c` continue | Same concept. |"),
            "{merged}"
        );
        // Added, with the three hand-written cells empty for a person.
        assert!(merged.contains("| `-d` | `--dir` |  |  |  |"), "{merged}");
        assert!(merged.contains("| `-h` | `--help` |  |  |  |"), "{merged}");
        // Removed, because the binary no longer defines it.
        assert!(!merged.contains("`--gone`"), "{merged}");
        // The other section is untouched: its rows name letters that are
        // deliberately not defined, and reading them as assigned rows would
        // delete every one of them.
        assert!(
            merged.contains("| `-k` | min split size | Reserved. |"),
            "{merged}"
        );
        // The prose and the headings survive, and so does the header row.
        assert!(merged.contains("# Short flags"), "{merged}");
        assert!(merged.contains("## Reserved and not assigned"), "{merged}");
        assert!(
            merged.contains("| Flag | Long form | Scope | `aria2` | Note |"),
            "{merged}"
        );
        assert!(
            merged.ends_with('\n'),
            "the file keeps its trailing newline"
        );

        // Idempotent: regenerating a file that is already right changes
        // nothing, which is what makes the no-op on the committed file mean
        // something.
        assert_eq!(merge_flags_table(&merged, &defined), merged);
    }

    /// Where `docs/flags.md` is, from the crate directory a test runs in.
    fn flags_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/flags.md")
    }

    /// Every `(short, long)` the command tree defines, deduplicated and in the
    /// order the table is sorted in.
    ///
    /// A global flag is on the root and on every subcommand, and `-o` is
    /// `--output` on three of them, so the raw walk yields the same pair many
    /// times over.
    ///
    /// `-h`/`--help` is added by hand. `clap` creates it while **building** a
    /// command and `Cli::command()` hands back one that is not built, so it is
    /// not in `get_arguments()` and the walk cannot see it. It is a real flag
    /// and the table documents it, so leaving it out would make that row look
    /// stale. `--version` needs no such entry: `disable_version_flag` is set,
    /// and `-v` is verbosity here.
    fn short_flags() -> Vec<(char, String)> {
        let command = Cli::command();
        let mut shorts: Vec<(char, String)> = vec![('h', "help".to_string())];
        let mut collect = |cmd: &clap::Command| {
            for arg in cmd.get_arguments() {
                if let Some(short) = arg.get_short() {
                    shorts.push((
                        short,
                        arg.get_long().unwrap_or(arg.get_id().as_str()).to_string(),
                    ));
                }
            }
        };
        collect(&command);
        for sub in command.get_subcommands() {
            collect(sub);
            for nested in sub.get_subcommands() {
                collect(nested);
            }
        }
        shorts.sort_by_key(flag_order);
        shorts.dedup();
        shorts
    }

    /// The sort key the "Assigned" table is in: by letter, uppercase before
    /// lowercase of the same letter, then by long name.
    fn flag_order((short, long): &(char, String)) -> (char, bool, String) {
        (
            short.to_ascii_lowercase(),
            short.is_ascii_lowercase(),
            long.clone(),
        )
    }

    /// Every `(short, long)` the "Assigned" table has a row for.
    ///
    /// Only that section. "Reserved and not assigned" names letters `bit-cli`
    /// deliberately does not define, and reading it here would make every one
    /// of them look like a stale row.
    fn documented_flags(table: &str) -> Vec<(char, String)> {
        assigned_rows(table)
            .iter()
            .filter_map(|row| flag_of_row(row))
            .collect()
    }

    /// The lines of the "Assigned" table's body, in file order.
    fn assigned_rows(table: &str) -> Vec<&str> {
        table
            .split("\n## ")
            .find(|section| section.starts_with("Assigned\n"))
            .map(|section| {
                section
                    .lines()
                    .filter(|line| line.starts_with("| `-"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `(short, long)` a row names, or `None` when it is not that shape.
    fn flag_of_row(row: &str) -> Option<(char, String)> {
        let mut cells = row.split('|').map(str::trim);
        cells.next()?;
        let short = cells.next()?.trim_matches('`').strip_prefix('-')?;
        let long = cells.next()?.trim_matches('`').strip_prefix("--")?;
        let mut chars = short.chars();
        let letter = chars.next()?;
        match chars.next() {
            None => Some((letter, long.to_string())),
            Some(_) => None,
        }
    }

    /// Rewrite the "Assigned" table to hold exactly the defined flags.
    ///
    /// A merge rather than a render. Three of the five columns, `Scope`,
    /// `aria2` and `Note`, are hand written and nothing in the command tree
    /// knows them, so an existing row is kept **verbatim** and only a row for
    /// a flag with none is added, with those three cells empty for a person to
    /// fill. A row whose flag the binary no longer defines is dropped.
    ///
    /// `TODO/cli-surface.md` T-158 is why it is written this way: regenerating
    /// `docs/schema.md` by rendering over it deleted rows the sample did not
    /// happen to produce. A generator that can only add and remove whole rows
    /// cannot do that.
    fn merge_flags_table(table: &str, defined: &[(char, String)]) -> String {
        let existing = assigned_rows(table);
        let mut rows: Vec<String> = Vec::new();
        for pair in defined {
            let kept = existing
                .iter()
                .find(|row| flag_of_row(row).as_ref() == Some(pair));
            rows.push(match kept {
                Some(row) => (*row).to_string(),
                None => format!("| `-{}` | `--{}` |  |  |  |", pair.0, pair.1),
            });
        }

        let mut out = String::with_capacity(table.len());
        let mut written = false;
        let mut in_assigned = false;
        for line in table.split('\n') {
            if line.starts_with("## ") {
                in_assigned = line == "## Assigned";
            }
            if in_assigned && line.starts_with("| `-") {
                if !written {
                    out.push_str(&rows.join("\n"));
                    out.push('\n');
                    written = true;
                }
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        // `split` on the trailing newline gives a final empty piece, and the
        // loop above turned it into one newline of its own.
        out.pop();
        out
    }
}
