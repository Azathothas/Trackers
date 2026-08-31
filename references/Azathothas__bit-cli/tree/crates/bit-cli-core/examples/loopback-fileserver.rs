//! A static HTTP/1.1 file server on loopback, with byte ranges.
//!
//! It exists so a web seed can be pointed at real files without reaching the
//! network. `scripts/interop-roundtrip.ps1` uses it to prove the `url-list`
//! that `bit-cli create --web-seed` writes is understood by another client
//! (`TODO/create-seed.md`, T-084).
//!
//! It is a test fixture, not a product. It serves `GET` and `HEAD` from one
//! directory, answers a single byte range per request, keeps a connection open
//! for the next request, and speaks no compression and no conditional
//! requests.
//!
//! ```text
//! cargo run -p bit-cli-core --example loopback-fileserver -- --root .tmp/x
//! ```
//!
//! Port `0` asks the OS for a free one. The base URL is printed to stdout as a
//! single line before the first request is served, so a script can read it and
//! pass it to `--web-seed`. Every request is logged to stderr with an ISO 8601
//! UTC millisecond timestamp.
//!
//! Six failure modes, so a client's handling of each can be measured from the
//! same binary rather than by finding a broken mirror in the wild:
//!
//! - `--ignore-range` answers every request with the whole entity and
//!   `200 OK`, which is the misconfigured mirror a client has to detect rather
//!   than accept.
//! - `--status <CODE>` answers every request with that status and no body.
//!   `--status 416` is the range a mirror refuses to serve.
//! - `--stall-after <BYTES>` sends that many bytes of the body and then holds
//!   the connection open without sending another byte or closing it, which is
//!   what a mirror does when its backend hangs. A client that has no read
//!   deadline waits forever here.
//! - `--fail-after <N>` serves the first N requests normally and then switches
//!   to `--status`, which is a mirror that falls over part way through a
//!   transfer.
//! - `--recover-after <M>` ends that failure window after M requests and goes
//!   back to serving, which is a mirror that falls over and comes back. It is
//!   what separates a status worth retrying from one that is not, without
//!   depending on a clock.
//! - `--down-for <SECONDS>` ends the same window on a clock instead, starting
//!   from the first request that falls into it. A client that is waiting out a
//!   cooldown is making no requests, so an outage counted in requests never
//!   advances while it waits and the mirror never comes back. Measuring a wait
//!   against an outage needs the outage to be a wall clock.
//!
//! Three more make it behave like a CDN that signs its URLs, which is what
//! `TODO/multi-source.md` T-131 asks for:
//!
//! - `--redirect-chain <N>` answers N plain `302`s before the request
//!   resolves, each to the same path with one fewer hop left.
//! - `--sign-redirect <SECONDS>` adds one more `302` after those, to the same
//!   path with `?sig=<value>&exp=<unix ms>`. The value rotates every SECONDS
//!   and the expiry is the end of the window it was minted in, so a signature
//!   handed out near the end of a window is already stale by the time the
//!   follow-up request lands. SECONDS may be fractional.
//! - `--require-sig` checks that signature: no `sig`, a `sig` that is not the
//!   one for its window, or an `exp` in the past all answer `403`. The next
//!   request to the stable path is redirected to a fresh signature and
//!   succeeds, which is the recovering `403` a real signed CDN produces.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bit_cli_core::time::{Timestamp, now_iso};

/// How the server answers, so a client's range handling can be tested both
/// ways from the same binary.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RangeMode {
    /// Honour `Range` and answer `206` with a `Content-Range`.
    Honour,
    /// Ignore `Range` and answer `200` with the whole entity.
    Ignore,
}

struct Config {
    root: PathBuf,
    range: RangeMode,
    /// Whether a connection is reused for the next request.
    keep_alive: bool,
    /// Answer every request with this status and no body.
    status: Option<u16>,
    /// Send this many body bytes, then stop without closing.
    stall_after: Option<u64>,
    /// Serve this many requests before the failure mode starts.
    healthy_requests: u64,
    /// How many requests the failure mode lasts before service resumes.
    failing_requests: u64,
    /// How long the failure mode lasts in milliseconds, from the first
    /// request that falls into it. Zero leaves the outage counted in requests
    /// alone.
    ///
    /// A source that is cooling down makes no requests, so a request-counted
    /// outage does not advance while it sleeps and the mirror never recovers.
    /// Anything measuring a wait against an outage needs the outage to be a
    /// wall clock. See `TODO/multi-source.md`, T-137.
    down_ms: u64,
    /// Milliseconds since `started` when the failure mode began, or zero
    /// before it has.
    down_since_ms: AtomicU64,
    /// Requests served so far, across every connection.
    served: AtomicU64,
    /// Signature lifetime in milliseconds. Zero turns signing off.
    sign_ms: u64,
    /// Whether an unsigned or stale signature is refused.
    require_sig: bool,
    /// Plain redirect hops before the request resolves.
    redirect_chain: u32,
    /// `http://127.0.0.1:<port>`, for an absolute `Location`.
    base: String,
    /// Monotonic origin of the signature windows.
    started: Instant,
    /// Unix milliseconds at `started`, so an expiry is a wall-clock time a
    /// caller can read while the monotonic clock is what rotates the window.
    started_epoch_ms: i64,
    /// Keys the signature, so a caller cannot mint one by reading this file.
    secret: u64,
    /// Signatures refused, for the acceptance to count.
    refused: AtomicU64,
}

impl Config {
    /// Whether this request falls inside the failure mode.
    ///
    /// The window is `[healthy_requests, healthy_requests + failing_requests)`,
    /// so `--fail-after 6 --recover-after 4` fails requests 7 through 10 and
    /// serves everything on either side.
    ///
    /// With `--down-for` set the window closes on a clock as well: the first
    /// request that falls inside it starts the outage, and everything after
    /// `down_ms` from that moment is served whatever the request count says.
    /// That is the form an outage a caller waits out has to take, because a
    /// caller that is waiting is not making requests.
    fn failing(&self) -> bool {
        let n = self.served.fetch_add(1, Ordering::Relaxed);
        let by_count = n >= self.healthy_requests
            && n < self.healthy_requests.saturating_add(self.failing_requests);
        if self.down_ms == 0 || !by_count {
            return by_count;
        }
        let elapsed = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        // The clock starts on the first failing request rather than at
        // startup, so `--fail-after` still decides when the outage begins.
        // `elapsed` is never zero in practice, and zero is the sentinel for
        // "not started", so it is nudged past it.
        let began = match self.down_since_ms.compare_exchange(
            0,
            elapsed.max(1),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => elapsed.max(1),
            Err(existing) => existing,
        };
        elapsed.saturating_sub(began) < self.down_ms
    }

    /// The signature window a moment falls in, counted from startup.
    fn epoch_at(&self, elapsed_ms: u64) -> u64 {
        elapsed_ms / self.sign_ms.max(1)
    }

    /// The signature for one window.
    ///
    /// SplitMix64 over the secret and the window index. It has to be
    /// unguessable from the URL alone and stable for the length of the
    /// window; it does not have to be a real MAC, and pulling in a crypto
    /// dependency for a test fixture would be worse than this.
    fn sig_for(&self, epoch: u64) -> String {
        let mut z = self
            .secret
            .wrapping_add(epoch.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        format!("{:016x}", z ^ (z >> 31))
    }

    /// When a window's signature stops being valid, in unix milliseconds.
    fn exp_for(&self, epoch: u64) -> i64 {
        self.started_epoch_ms + ((epoch + 1) * self.sign_ms.max(1)) as i64
    }

    /// Mint the query string for the current window.
    fn mint(&self) -> String {
        let epoch = self.epoch_at(self.started.elapsed().as_millis() as u64);
        format!("sig={}&exp={}", self.sig_for(epoch), self.exp_for(epoch))
    }

    /// Check a signature a client presented, naming why it was refused.
    ///
    /// A signature is good when it is the one minted for the window its
    /// expiry names, and that expiry has not passed. Both halves matter: the
    /// first refuses a forged value, the second refuses a real one that was
    /// handed out at the end of its window and used after it.
    fn check(&self, query: &str) -> Result<(), &'static str> {
        let Some(sig) = param(query, "sig") else {
            return Err("no signature");
        };
        let Some(exp) = param(query, "exp").and_then(|v| v.parse::<i64>().ok()) else {
            return Err("no expiry");
        };
        let window = self.sign_ms.max(1) as i64;
        let epoch = (exp - self.started_epoch_ms).div_euclid(window) - 1;
        if epoch < 0 || sig != self.sig_for(epoch as u64) {
            return Err("bad signature");
        }
        if exp <= Timestamp::now().epoch_ms() {
            return Err("signature expired");
        }
        Ok(())
    }
}

fn main() {
    let mut root = PathBuf::from(".");
    let mut port: u16 = 0;
    let mut range = RangeMode::Honour;
    let mut status: Option<u16> = None;
    let mut stall_after: Option<u64> = None;
    let mut healthy_requests: u64 = 0;
    let mut failing_requests: u64 = u64::MAX;
    let mut down_ms: u64 = 0;
    let mut sign_ms: u64 = 0;
    let mut require_sig = false;
    let mut redirect_chain: u32 = 0;
    let mut keep_alive = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = PathBuf::from(next_value(&mut args, "--root")),
            "--port" => port = next_value(&mut args, "--port").parse().expect("--port"),
            "--ignore-range" => range = RangeMode::Ignore,
            "--no-keep-alive" => keep_alive = false,
            "--status" => {
                status = Some(next_value(&mut args, "--status").parse().expect("--status"))
            }
            "--stall-after" => {
                stall_after = Some(
                    next_value(&mut args, "--stall-after")
                        .parse()
                        .expect("--stall-after"),
                )
            }
            "--fail-after" => {
                healthy_requests = next_value(&mut args, "--fail-after")
                    .parse()
                    .expect("--fail-after")
            }
            "--down-for" => {
                let seconds: f64 = next_value(&mut args, "--down-for")
                    .parse()
                    .expect("--down-for");
                if !(seconds.is_finite() && seconds >= 0.0) {
                    eprintln!("loopback-fileserver: --down-for needs a number of seconds");
                    std::process::exit(2);
                }
                down_ms = (seconds * 1000.0) as u64;
            }
            "--recover-after" => {
                failing_requests = next_value(&mut args, "--recover-after")
                    .parse()
                    .expect("--recover-after")
            }
            "--sign-redirect" => {
                let seconds: f64 = next_value(&mut args, "--sign-redirect")
                    .parse()
                    .expect("--sign-redirect");
                if !(seconds.is_finite() && seconds > 0.0) {
                    eprintln!("loopback-fileserver: --sign-redirect needs a positive number");
                    std::process::exit(2);
                }
                sign_ms = ((seconds * 1000.0).round() as u64).max(1);
            }
            "--require-sig" => require_sig = true,
            "--redirect-chain" => {
                redirect_chain = next_value(&mut args, "--redirect-chain")
                    .parse()
                    .expect("--redirect-chain")
            }
            "--help" | "-h" => {
                println!(
                    "usage: loopback-fileserver [--root DIR] [--port PORT] [--ignore-range]\n\
                     \x20                          [--no-keep-alive] [--status CODE]\n\
                     \x20                          [--stall-after BYTES] [--fail-after N]\n\
                     \x20                          [--recover-after M] [--sign-redirect SECONDS]\n\
                     \x20                          [--require-sig] [--redirect-chain N]"
                );
                return;
            }
            other => {
                eprintln!("loopback-fileserver: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }

    let root = root.canonicalize().unwrap_or_else(|err| {
        eprintln!(
            "loopback-fileserver: {} is unreadable: {err}",
            root.display()
        );
        std::process::exit(2);
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("bind loopback");
    let bound = listener.local_addr().expect("local addr");
    // The script reads this line to learn the port, so it goes out before
    // anything else and is flushed immediately. The trailing slash matters:
    // BEP 19 appends the torrent name to a URL that ends in one.
    println!("http://127.0.0.1:{}/", bound.port());
    std::io::stdout().flush().ok();
    eprintln!(
        "{} fileserver listening on {bound}, root {}",
        now_iso(),
        root.display()
    );

    if require_sig && sign_ms == 0 {
        eprintln!(
            "{} --require-sig with no --sign-redirect: every request is refused",
            now_iso()
        );
    }
    let config = Arc::new(Config {
        root,
        range,
        keep_alive,
        status,
        stall_after,
        healthy_requests,
        failing_requests,
        down_ms,
        down_since_ms: AtomicU64::new(0),
        served: AtomicU64::new(0),
        sign_ms,
        require_sig,
        redirect_chain,
        base: format!("http://127.0.0.1:{}", bound.port()),
        started: Instant::now(),
        started_epoch_ms: Timestamp::now().epoch_ms(),
        // The port is already unique to this process and the clock separates
        // two runs on the same one. A fixture does not need more than that.
        secret: (bound.port() as u64) << 48 | (Timestamp::now().epoch_ms() as u64),
        refused: AtomicU64::new(0),
    });
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let config = config.clone();
        std::thread::spawn(move || {
            if let Err(err) = serve(stream, &config) {
                eprintln!("{} connection failed: {err}", now_iso());
            }
        });
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> String {
    match args.next() {
        Some(value) => value,
        None => {
            eprintln!("loopback-fileserver: {flag} needs a value");
            std::process::exit(2);
        }
    }
}

/// Serve requests on one connection until the client goes away.
///
/// HTTP/1.1 connections are persistent by default, and this server has to be
/// too. A server that closes after every response burns one ephemeral port per
/// request: at a few thousand requests a second, a benchmark run exhausts the
/// 16,384 port dynamic range in seconds and then measures nothing but
/// `connect` failures. `--no-keep-alive` restores the closing behaviour,
/// because a mirror that does that is its own case worth measuring.
fn serve(stream: TcpStream, config: &Config) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut stream = stream;
    loop {
        match serve_one(&mut reader, &mut stream, config)? {
            Disposition::KeepAlive => continue,
            Disposition::Close => return Ok(()),
        }
    }
}

/// What to do with the connection after one response.
enum Disposition {
    KeepAlive,
    Close,
}

fn serve_one(
    reader: &mut BufReader<TcpStream>,
    stream: &mut TcpStream,
    config: &Config,
) -> std::io::Result<Disposition> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(Disposition::Close);
    }
    if request_line.trim().is_empty() {
        return Ok(Disposition::Close);
    }
    let mut range_header = None;
    let mut wants_close = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range_header = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("connection") {
                wants_close = value.trim().eq_ignore_ascii_case("close");
            }
        }
    }
    let keep_alive = config.keep_alive && !wants_close;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/");
    let without_fragment = target.split('#').next().unwrap_or("/");
    let (path, query) = match without_fragment.split_once('?') {
        Some((path, query)) => (path, query),
        None => (without_fragment, ""),
    };

    // The request counter advances once per request, whatever the outcome, so
    // `--fail-after N` means "the first N requests work" no matter which
    // failure mode follows.
    let failing = config.failing();
    if failing && let Some(code) = config.status {
        return respond_status(
            stream,
            code,
            reason_for(code),
            &method,
            path,
            "forced",
            keep_alive,
        );
    }

    match route(config, path, query) {
        Route::Serve => {}
        Route::Redirect(location) => {
            return respond_redirect(stream, &method, target, &location, keep_alive);
        }
        Route::Refuse(why) => {
            // The running total goes in the reason so a harness can read the
            // final count off the last line of stderr rather than counting
            // lines that interleave from every connection thread.
            let n = config.refused.fetch_add(1, Ordering::Relaxed) + 1;
            let why = format!("{why}, {n} refused so far");
            return respond_status(stream, 403, "Forbidden", &method, target, &why, keep_alive);
        }
    }

    let Some(file_path) = resolve(&config.root, path) else {
        return respond_status(
            stream,
            404,
            "Not Found",
            &method,
            path,
            "bad path",
            keep_alive,
        );
    };
    let Ok(mut file) = File::open(&file_path) else {
        return respond_status(
            stream,
            404,
            "Not Found",
            &method,
            path,
            "no such file",
            keep_alive,
        );
    };
    let length = file.metadata()?.len();

    let wanted = match (&range_header, config.range) {
        (Some(header), RangeMode::Honour) => match parse_range(header, length) {
            Some(range) => Some(range),
            None => {
                eprintln!("{} {method} {path} -> 416 {header}", now_iso());
                write!(
                    stream,
                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{length}\r\nContent-Length: 0\r\nConnection: {}\r\n\r\n",
                    connection_header(keep_alive)
                )?;
                stream.flush()?;
                return Ok(disposition(keep_alive));
            }
        },
        _ => None,
    };

    let (status, reason, start, count) = match wanted {
        Some((start, end)) => (206, "Partial Content", start, end - start + 1),
        None => (200, "OK", 0, length),
    };

    eprintln!(
        "{} {method} {path} range={} -> {status} {count} byte(s)",
        now_iso(),
        range_header.as_deref().unwrap_or("-"),
    );

    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nAccept-Ranges: bytes\r\nContent-Type: {}\r\nContent-Length: {count}\r\nConnection: {}\r\n",
        content_type(path),
        connection_header(keep_alive)
    );
    if status == 206 {
        let end = start + count - 1;
        head.push_str(&format!("Content-Range: bytes {start}-{end}/{length}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if method.eq_ignore_ascii_case("HEAD") {
        stream.flush()?;
        return Ok(disposition(keep_alive));
    }

    // A stall is a `Content-Length` the server never satisfies: the promised
    // bytes stop arriving and the connection stays open. A client with no read
    // deadline waits here forever, which is the behaviour being measured.
    let stall_at = match failing {
        true => config.stall_after,
        false => None,
    };

    file.seek(SeekFrom::Start(start))?;
    let mut remaining = count;
    let mut sent = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    while remaining > 0 {
        if let Some(limit) = stall_at
            && sent >= limit
        {
            eprintln!(
                "{} {method} {path} stalled after {sent} of {count} byte(s)",
                now_iso()
            );
            stream.flush()?;
            // Hold the connection open without closing it. The client decides
            // how long to wait; this thread is reaped when the process exits.
            std::thread::park();
            return Ok(Disposition::Close);
        }
        let ceiling = stall_at.map_or(remaining, |limit| remaining.min(limit - sent));
        let want = ceiling.max(1).min(buffer.len() as u64) as usize;
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            break;
        }
        stream.write_all(&buffer[..read])?;
        remaining -= read as u64;
        sent += read as u64;
    }
    stream.flush()?;
    Ok(disposition(keep_alive))
}

/// What one request resolves to before the file is opened.
enum Route {
    /// Answer from the payload.
    Serve,
    /// Send the client somewhere else, at this absolute URL.
    Redirect(String),
    /// Answer `403`, for this reason.
    Refuse(&'static str),
}

/// Decide whether a request is served, redirected, or refused.
///
/// The order is the one a signed CDN uses. A request carrying a signature is
/// checked and nothing else happens to it. A request carrying none walks the
/// plain redirect chain first, then gets the signing redirect, and only a
/// server with neither turned on serves the stable path directly.
fn route(config: &Config, path: &str, query: &str) -> Route {
    if config.sign_ms > 0 && param(query, "sig").is_some() {
        return match config.require_sig {
            false => Route::Serve,
            true => match config.check(query) {
                Ok(()) => Route::Serve,
                Err(why) => Route::Refuse(why),
            },
        };
    }

    // `hop` counts down, so a client that follows the chain reaches zero and a
    // client that guesses a hop URL cannot loop the server forever.
    let hops_left = param(query, "hop")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(config.redirect_chain);
    if hops_left > 0 {
        return Route::Redirect(format!("{}{path}?hop={}", config.base, hops_left - 1));
    }
    if config.sign_ms > 0 {
        return Route::Redirect(format!("{}{path}?{}", config.base, config.mint()));
    }
    match config.require_sig {
        true => Route::Refuse("no signature"),
        false => Route::Serve,
    }
}

/// One query parameter, undecoded.
///
/// The only parameters this server reads are hex, decimal, and a small
/// integer, so percent-decoding them would change nothing and hide a
/// malformed value.
fn param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then_some(value)
    })
}

/// Send a `302` and keep the connection.
///
/// `302` rather than `307` because that is what a signing CDN sends, and the
/// method is `GET` either way so the two behave identically here.
fn respond_redirect(
    stream: &mut TcpStream,
    method: &str,
    target: &str,
    location: &str,
    keep_alive: bool,
) -> std::io::Result<Disposition> {
    eprintln!("{} {method} {target} -> 302 {location}", now_iso());
    write!(
        stream,
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: {}\r\n\r\n",
        connection_header(keep_alive)
    )?;
    stream.flush()?;
    Ok(disposition(keep_alive))
}

/// What a `Connection` header says, and what it means for the socket.
///
/// HTTP/1.1 keeps a connection open unless told otherwise, but saying so
/// explicitly is what makes a packet capture of a failing run readable.
/// The `Content-Type` for a served path, from its extension.
///
/// Everything is `application/octet-stream` unless the extension says
/// otherwise, which is what a web seed serving payload bytes wants. The
/// exceptions exist so a fixture can serve a **page**: T-244 tells a page from
/// a `.torrent` by parsing the body first and reading this header second, and
/// a server that labels every page as octet-stream cannot exercise the second
/// half. See `TODO/cli-surface.md`, T-244.
fn content_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let lower = lower.split(['?', '#']).next().unwrap_or("");
    match () {
        () if lower.ends_with(".html") || lower.ends_with(".htm") => "text/html; charset=utf-8",
        () if lower.ends_with(".xhtml") => "application/xhtml+xml",
        () if lower.ends_with(".json") => "application/json",
        () if lower.ends_with(".txt") => "text/plain; charset=utf-8",
        () if lower.ends_with(".torrent") => "application/x-bittorrent",
        () => "application/octet-stream",
    }
}
const fn connection_header(keep_alive: bool) -> &'static str {
    match keep_alive {
        true => "keep-alive",
        false => "close",
    }
}

const fn disposition(keep_alive: bool) -> Disposition {
    match keep_alive {
        true => Disposition::KeepAlive,
        false => Disposition::Close,
    }
}

/// The reason phrase for a status, for the forced-status mode.
///
/// Only the codes this server is asked to produce are named. Anything else
/// gets a phrase that is syntactically valid and says nothing, because a made
/// up phrase would be worse than a generic one.
fn reason_for(status: u16) -> &'static str {
    match status {
        403 => "Forbidden",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

#[allow(clippy::too_many_arguments)]
fn respond_status(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    method: &str,
    path: &str,
    why: &str,
    keep_alive: bool,
) -> std::io::Result<Disposition> {
    eprintln!("{} {method} {path} -> {status} ({why})", now_iso());
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: {}\r\n\r\n",
        connection_header(keep_alive)
    )?;
    stream.flush()?;
    Ok(disposition(keep_alive))
}

/// Map a request path onto a file under `root`, or refuse it.
///
/// Refusing rather than clamping is deliberate: a traversal attempt should
/// show up as a 404 in the log, not as a silently rewritten path.
fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(path.trim_start_matches('/'));
    let decoded = String::from_utf8(decoded).ok()?;
    let mut out = root.to_path_buf();
    for segment in decoded.split('/').filter(|s| !s.is_empty()) {
        let candidate = Path::new(segment);
        if candidate
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return None;
        }
        out.push(segment);
    }
    let out = out.canonicalize().ok()?;
    out.starts_with(root).then_some(out)
}

/// Parse a single `bytes=start-end` range against a known entity length.
///
/// Multipart ranges are not supported: no BitTorrent client asks for one, and
/// answering a multi-range request with a single range would be wrong.
fn parse_range(header: &str, length: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let (start, end) = match (start.trim(), end.trim()) {
        // `bytes=-N` is the last N bytes.
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (length.checked_sub(n.min(length))?, length.saturating_sub(1))
        }
        (start, "") => (start.parse().ok()?, length.saturating_sub(1)),
        (start, end) => (start.parse().ok()?, end.parse().ok()?),
    };
    if length == 0 || start > end || start >= length {
        return None;
    }
    Some((start, end.min(length - 1)))
}

fn percent_decode(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&value[i + 1..i + 3], 16) {
                    Ok(byte) => out.push(byte),
                    Err(_) => out.push(b'%'),
                }
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server whose signature windows started `age_ms` ago.
    ///
    /// The wall-clock origin is what decides whether a signature has expired,
    /// so moving it into the past is how a stale one is built without waiting
    /// for a real clock.
    fn config(sign_ms: u64, require_sig: bool, redirect_chain: u32, age_ms: i64) -> Config {
        Config {
            root: PathBuf::from("."),
            range: RangeMode::Honour,
            keep_alive: true,
            status: None,
            stall_after: None,
            healthy_requests: 0,
            failing_requests: u64::MAX,
            down_ms: 0,
            down_since_ms: AtomicU64::new(0),
            served: AtomicU64::new(0),
            sign_ms,
            require_sig,
            redirect_chain,
            base: "http://127.0.0.1:1".to_string(),
            started: Instant::now(),
            started_epoch_ms: Timestamp::now().epoch_ms() - age_ms,
            secret: 0x1234_5678_9abc_def0,
            refused: AtomicU64::new(0),
        }
    }

    fn redirect(route: Route) -> String {
        match route {
            Route::Redirect(location) => location,
            Route::Serve => panic!("served, expected a redirect"),
            Route::Refuse(why) => panic!("refused ({why}), expected a redirect"),
        }
    }

    #[test]
    fn a_chain_counts_down_one_hop_at_a_time_and_ends_at_a_signature() {
        let config = config(1000, true, 2, 0);
        assert_eq!(
            redirect(route(&config, "/blob.bin", "")),
            "http://127.0.0.1:1/blob.bin?hop=1"
        );
        assert_eq!(
            redirect(route(&config, "/blob.bin", "hop=1")),
            "http://127.0.0.1:1/blob.bin?hop=0"
        );
        let signed = redirect(route(&config, "/blob.bin", "hop=0"));
        assert!(signed.contains("sig="), "{signed}");
        assert!(signed.contains("exp="), "{signed}");
    }

    #[test]
    fn a_chain_with_no_signing_ends_at_the_payload() {
        let config = config(0, false, 1, 0);
        assert_eq!(
            redirect(route(&config, "/blob.bin", "")),
            "http://127.0.0.1:1/blob.bin?hop=0"
        );
        assert!(matches!(route(&config, "/blob.bin", "hop=0"), Route::Serve));
    }

    #[test]
    fn a_signature_minted_now_is_served() {
        let config = config(1000, true, 0, 0);
        let signed = redirect(route(&config, "/blob.bin", ""));
        let query = signed.split_once('?').unwrap().1;
        assert!(matches!(route(&config, "/blob.bin", query), Route::Serve));
    }

    #[test]
    fn a_signature_from_a_window_that_has_passed_is_refused() {
        // Ten windows of one second have gone by, so window zero's signature
        // is genuine and nine seconds stale, which is the case a signing CDN
        // answers 403 for.
        let config = config(1000, true, 0, 10_000);
        let query = format!("sig={}&exp={}", config.sig_for(0), config.exp_for(0));
        assert!(matches!(
            route(&config, "/blob.bin", &query),
            Route::Refuse("signature expired")
        ));
    }

    #[test]
    fn a_forged_signature_is_refused_even_with_an_expiry_far_ahead() {
        let config = config(1000, true, 0, 0);
        let query = format!("sig=0000000000000000&exp={}", config.exp_for(0));
        assert!(matches!(
            route(&config, "/blob.bin", &query),
            Route::Refuse("bad signature")
        ));
    }

    #[test]
    fn requiring_a_signature_the_server_never_mints_refuses_everything() {
        let config = config(0, true, 0, 0);
        assert!(matches!(
            route(&config, "/blob.bin", ""),
            Route::Refuse("no signature")
        ));
    }

    #[test]
    fn a_signature_is_only_checked_when_it_is_required() {
        let config = config(1000, false, 0, 10_000);
        let query = format!("sig={}&exp={}", config.sig_for(0), config.exp_for(0));
        assert!(matches!(route(&config, "/blob.bin", &query), Route::Serve));
    }

    #[test]
    fn each_window_gets_its_own_signature() {
        let config = config(1000, true, 0, 0);
        assert_ne!(config.sig_for(0), config.sig_for(1));
        assert_eq!(config.sig_for(7), config.sig_for(7));
        assert_eq!(config.exp_for(1) - config.exp_for(0), 1000);
    }

    #[test]
    fn the_failure_window_opens_after_the_healthy_run_and_closes_after_its_own() {
        let mut config = config(0, false, 0, 0);
        config.healthy_requests = 2;
        config.failing_requests = 3;
        let outcome: Vec<bool> = (0..7).map(|_| config.failing()).collect();
        assert_eq!(
            outcome,
            vec![false, false, true, true, true, false, false],
            "requests 3, 4, and 5 fail and the rest are served"
        );
    }

    #[test]
    fn a_failure_window_with_no_recovery_never_closes() {
        let mut config = config(0, false, 0, 0);
        config.healthy_requests = 1;
        let outcome: Vec<bool> = (0..4).map(|_| config.failing()).collect();
        assert_eq!(outcome, vec![false, true, true, true]);
    }

    #[test]
    fn a_query_parameter_is_matched_on_the_whole_key() {
        assert_eq!(param("hop=3&sig=ab", "hop"), Some("3"));
        assert_eq!(param("hop=3&sig=ab", "sig"), Some("ab"));
        assert_eq!(param("nohop=3", "hop"), None);
        assert_eq!(param("", "hop"), None);
        assert_eq!(param("hop", "hop"), None, "a bare key carries no value");
    }

    /// An outage on a clock closes even though no request advanced it.
    ///
    /// A source waiting out `--web-seed-cooldown` makes no requests, so an
    /// outage counted in requests alone would never end. See
    /// `TODO/multi-source.md`, T-137.
    #[test]
    fn a_timed_outage_closes_on_the_clock_rather_than_on_a_request_count() {
        let mut config = config(0, false, 0, 0);
        config.healthy_requests = 1;
        config.down_ms = 60;
        assert!(!config.failing(), "the first request is served");
        assert!(config.failing(), "the second opens the outage");
        assert!(config.failing(), "and it is still open");
        std::thread::sleep(std::time::Duration::from_millis(90));
        assert!(
            !config.failing(),
            "the clock closed it with no request in between"
        );
    }

    /// The clock starts at the first failing request, not at startup, so
    /// `--fail-after` still decides when the outage begins.
    #[test]
    fn a_timed_outage_starts_when_the_failure_window_does() {
        let mut config = config(0, false, 0, 0);
        config.healthy_requests = 1;
        config.down_ms = 80;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!config.failing(), "the healthy run is still served");
        assert!(config.failing(), "the outage opens now, not 100ms ago");
    }

    /// Without it, the window is counted in requests exactly as before.
    #[test]
    fn no_down_for_leaves_the_window_counted_in_requests() {
        let mut config = config(0, false, 0, 0);
        config.healthy_requests = 1;
        config.failing_requests = 2;
        let outcome: Vec<bool> = (0..4).map(|_| config.failing()).collect();
        assert_eq!(outcome, vec![false, true, true, false]);
    }
}
