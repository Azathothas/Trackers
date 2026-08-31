//! Find the browser this machine has, and point it at a URL.
//!
//! It exists so that the profile `bit-cli` impersonates can be checked against
//! a **real** browser rather than against a version number. The operator's
//! requirement for `TODO/cli-surface.md` T-244 is that staleness is detected
//! in time and that the fix is recommended with proof and new values; the
//! proof is what a browser actually emits, and this is what emits it.
//!
//! Two jobs and nothing else:
//!
//! - **Resolve.** `crates/bit-cli-core/src/browser.rs` is the search and this
//!   runs it against the real filesystem. With no browser it prints the typed
//!   `NoBrowser`, which names every path it looked at, and exits **2**.
//! - **Drive.** With `--url`, launch the resolved browser headless at that URL
//!   and wait for it to exit. The page it lands on is `loopback-tlsprobe`,
//!   which reads the handshake and the opening HTTP/2 flight off the wire.
//!
//! ```text
//! cargo run -p bit-cli-core --example browser-capture -- --json
//! cargo run -p bit-cli-core --example browser-capture -- --url https://127.0.0.1:9999/
//! ```
//!
//! **`--ignore-certificate-errors` is passed to the browser and not to
//! anything that ships.** The probe's certificate is minted per run and the
//! browser has no way to trust it; a browser refusing it aborts before it
//! sends a single HTTP/2 frame, and the HTTP/2 half of the fingerprint is
//! exactly what this is for. It changes what the browser accepts **after** the
//! handshake, not the `ClientHello` it sends, and the whole exchange is
//! between two processes on loopback. `bit-cli` itself has no such flag and is
//! not getting one: it reads the probe's own authority from
//! `BIT_CLI_EXTRA_CA_FILE` instead.
//!
//! Exit status is 0 when a browser was found and did what was asked, 2 when
//! there is no browser, which is the case that has to work on every CI runner.

use std::path::{Path, PathBuf};
use std::process::Command;

use bit_cli_core::browser::{self, Browser, Platform, Search};

const HELP: &str = "\
browser-capture: find an installed browser and point it at a URL

USAGE:
    browser-capture [OPTIONS]

OPTIONS:
        --url <URL>       launch the browser at this URL and wait for it
        --path <PATH>     an explicit browser, tried first and alone
        --json            one JSON object on stdout
        --timeout <SECS>  how long to let the browser run (default 20)
    -h, --help            this text

Exits 0 when a browser was found, 2 when there is none.";

struct Args {
    url: Option<String>,
    path: Option<PathBuf>,
    json: bool,
    timeout: u64,
}

fn next_value(argv: &[String], i: &mut usize) -> String {
    *i += 1;
    match argv.get(*i) {
        Some(v) => v.clone(),
        None => {
            eprintln!("browser-capture: {} needs a value", argv[*i - 1]);
            std::process::exit(2);
        }
    }
}

fn parse_args() -> Args {
    let mut a = Args {
        url: None,
        path: None,
        json: false,
        timeout: 20,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--url" => a.url = Some(next_value(&argv, &mut i)),
            "--path" => a.path = Some(PathBuf::from(next_value(&argv, &mut i))),
            "--json" => a.json = true,
            "--timeout" => {
                let raw = next_value(&argv, &mut i);
                a.timeout = raw.parse().unwrap_or(20);
            }
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            other => {
                eprintln!("browser-capture: unknown argument {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    a
}

/// Escape a string for a JSON scalar. A Windows path is full of backslashes,
/// so this is not optional.
fn esc(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            c if (c as u32) < 0x20 => vec![' '],
            c => vec![c],
        })
        .collect()
}

/// Ask the browser what version it is.
///
/// Two ways, in this order, and the order is not a preference.
///
/// **The filesystem first.** Chrome keeps a directory named after its own
/// version beside the executable, which is how it finds its own resources, so
/// the newest such sibling is the answer and reading it costs nothing.
///
/// **`--version` second, and never on Windows.** It prints
/// `Google Chrome 151.0.7258.67` on Linux and macOS. On Windows `chrome.exe`
/// is a GUI subsystem binary with no console to print to, and rather than
/// printing nothing it **starts the browser**: a window opens, and on a
/// machine with more than one profile it stops and waits for somebody to pick
/// one. A version check that opens a browser is not a version check, and this
/// one did until it was watched doing it.
///
/// A browser that answers neither way is not an error: the fingerprint is the
/// measurement and the version is the label on it.
fn version_of(path: &PathBuf) -> Option<String> {
    if let Some(found) = version_from_siblings(path) {
        return Some(found);
    }
    if cfg!(windows) {
        return None;
    }
    let out = Command::new(path).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match text.is_empty() {
        true => None,
        false => Some(text),
    }
}

/// The newest `151.0.7922.76`-shaped directory beside the executable.
fn version_from_siblings(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    let mut versions: Vec<String> = std::fs::read_dir(parent)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            let parts: Vec<&str> = name.split('.').collect();
            parts.len() == 4
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    versions.sort_by_key(|name| {
        name.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    });
    versions.pop()
}

/// The major version out of whatever `version_of` returned.
///
/// `Google Chrome 151.0.7258.67` and `151.0.7922.76` both answer 151.
fn major_of(version: &str) -> Option<u32> {
    version
        .split_whitespace()
        .last()?
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// Launch the browser at `url`, headless, in a throwaway profile, and let it
/// finish on its own.
///
/// Every flag earns its place, and the last one is the one that matters:
///
/// - `--headless=new` so no window opens on a developer's desktop and so it
///   runs on a CI runner with no display.
/// - `--user-data-dir` into a fresh temporary directory, so an existing
///   profile's extensions, proxies and enterprise policy cannot change what
///   goes on the wire. A capture taken through somebody's ad blocker is not
///   the browser's fingerprint.
/// - `--no-first-run`, `--no-default-browser-check`,
///   `--disable-search-engine-choice-screen` so nothing prompts.
/// - `--ignore-certificate-errors` for the reason in the module header.
/// - `--disable-gpu` because a headless capture has nothing to draw.
/// - **`--dump-dom <url>`, which is what makes this terminate.** Headless
///   Chrome given a URL as a plain argument navigates and then goes on
///   running, exactly like the browser it is, so the capture would depend on
///   the deadline below and on this machine having nobody looking at it.
///   `--dump-dom` navigates, prints the document and exits, in about half a
///   second. Its output goes nowhere: the request is the measurement and the
///   page is a probe that answers almost nothing.
///
/// The deadline stays as a backstop, because a browser that hangs on a
/// network stack rather than on a page still has to be killed.
fn launch(path: &PathBuf, url: &str, timeout: u64) -> std::io::Result<i32> {
    let profile =
        std::env::temp_dir().join(format!("bit-cli-browser-capture-{}", std::process::id()));
    let mut child = Command::new(path)
        .arg("--headless=new")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-search-engine-choice-screen")
        .arg("--disable-gpu")
        .arg("--ignore-certificate-errors")
        // `--dump-dom` is a mode and the URL is the positional argument it
        // acts on. Written as `--dump-dom=<url>` Chrome sees a switch it does
        // not know and no page to open, falls back to a normal window, and
        // waits for a person to choose a profile. That cost an interruption.
        .arg("--dump-dom")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .spawn()?;

    // Wait on the process, then on the deadline, and kill it either way. A
    // capture that leaves a browser running is a capture that poisons the next
    // one, and on a CI runner it is a job that never ends.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        match child.try_wait()? {
            Some(status) => {
                let _ = std::fs::remove_dir_all(&profile);
                return Ok(status.code().unwrap_or(0));
            }
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_dir_all(&profile);
                    return Ok(0);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

fn main() {
    let a = parse_args();

    let search = Search {
        explicit: a.path.clone(),
        attach: None,
        extra: Vec::new(),
    };
    let platform = Platform::host();
    let mut candidates = browser::path_candidates(
        &std::env::var("PATH").unwrap_or_default(),
        if cfg!(windows) { ';' } else { ':' },
    );
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        candidates.extend(browser::home_paths(platform, std::path::Path::new(&home)));
    }
    candidates.extend(browser::default_paths(platform));

    let found = browser::resolve(&search, &candidates, |p| p.is_file());
    let path = match found {
        Ok(Browser::Executable(path)) => path,
        Ok(Browser::Attached { host, port }) => {
            eprintln!("browser-capture: an attached instance at {host}:{port} cannot be launched");
            std::process::exit(2);
        }
        Err(e) => {
            if a.json {
                println!("{{\"found\":false,\"reason\":\"{}\"}}", esc(&e.to_string()));
            }
            eprintln!("browser-capture: {e}");
            std::process::exit(2);
        }
    };

    let version = version_of(&path);
    let major = version.as_deref().and_then(major_of);
    if a.json {
        println!(
            "{{\"found\":true,\"path\":\"{}\",\"version\":{},\"major\":{}}}",
            esc(&path.display().to_string()),
            match &version {
                Some(v) => format!("\"{}\"", esc(v)),
                None => "null".to_string(),
            },
            match major {
                Some(m) => m.to_string(),
                None => "null".to_string(),
            }
        );
    } else {
        println!("{}", path.display());
        if let Some(v) = &version {
            println!("{v}");
        }
    }

    if let Some(url) = &a.url {
        eprintln!("browser-capture: launching {} at {url}", path.display());
        match launch(&path, url, a.timeout) {
            Ok(code) => eprintln!("browser-capture: the browser exited {code}"),
            Err(e) => {
                eprintln!("browser-capture: cannot launch {}: {e}", path.display());
                std::process::exit(2);
            }
        }
    }
}
