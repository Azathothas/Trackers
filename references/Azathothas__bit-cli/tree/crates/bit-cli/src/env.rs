//! The program's environment, injected rather than read from globals.
//!
//! Arguments, the working directory, the environment variables, and the three
//! streams all arrive through [`Env`]. Nothing in the binary calls
//! `std::env::args`, `std::env::current_dir`, or `println!`.
//!
//! That is what makes section 0.11 testable rather than aspirational: a test
//! constructs an `Env` with in-memory streams and runs the whole binary
//! in-process, so "the same thing happens with no terminal attached" is
//! asserted rather than assumed. The pattern comes from `intermodal`
//! (CC0-1.0), whose `src/env.rs` does the same thing for the same reason.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A writable stream the program can be pointed at.
pub enum Stream {
    /// The real process stream.
    Standard(Box<dyn Write + Send>),
    /// An in-memory buffer, for tests.
    Buffer(Arc<Mutex<Vec<u8>>>),
}

impl Stream {
    /// The real stdout.
    pub fn stdout() -> Self {
        Self::Standard(Box::new(std::io::stdout()))
    }

    /// The real stderr.
    pub fn stderr() -> Self {
        Self::Standard(Box::new(std::io::stderr()))
    }

    /// A buffer, and a handle to read it back.
    pub fn buffer() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let shared = Arc::new(Mutex::new(Vec::new()));
        (Self::Buffer(shared.clone()), shared)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Standard(inner) => inner.write(buf),
            Self::Buffer(shared) => {
                let mut guard = shared
                    .lock()
                    .map_err(|_| std::io::Error::other("output buffer is poisoned"))?;
                guard.extend_from_slice(buf);
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Standard(inner) => inner.flush(),
            Self::Buffer(_) => Ok(()),
        }
    }
}

/// Everything the program reads from outside itself.
pub struct Env {
    /// Command-line arguments, including the program name at index 0.
    pub args: Vec<String>,
    /// The working directory that relative paths resolve against.
    pub cwd: PathBuf,
    /// Environment variables.
    pub vars: BTreeMap<String, String>,
    /// Data output. Only data ever goes here.
    pub out: Stream,
    /// Logs, progress, warnings, and errors. Never data.
    pub err: Stream,
    /// Whether stdout is a terminal.
    ///
    /// This may decide colour and progress rendering and nothing else. It may
    /// never decide what the program does, what it computes, or what it
    /// reports.
    pub out_is_terminal: bool,
    /// Whether stderr is a terminal.
    pub err_is_terminal: bool,
    /// Whether stdout can carry a code point outside ASCII.
    ///
    /// `bit-cli` writes UTF-8 whatever is downstream, so this is a question
    /// about the sink rather than about the program: a file or a pipe takes
    /// the bytes verbatim and a console decodes them at its own code page.
    /// Only a terminal can therefore turn a box-drawing character into
    /// mojibake, which is what `bit-cli tree` asks this before it draws one.
    ///
    /// See `TODO/metainfo.md`, T-249, and `docs/schema.md`'s Windows section
    /// for the encodings a console gets wrong.
    pub out_is_unicode: bool,
}

impl Env {
    /// The real process environment.
    pub fn real() -> std::io::Result<Self> {
        let out_is_terminal = std::io::stdout().is_terminal();
        let vars: BTreeMap<String, String> = std::env::vars().collect();
        Ok(Self {
            args: std::env::args().collect(),
            cwd: std::env::current_dir()?,
            out: Stream::stdout(),
            err: Stream::stderr(),
            out_is_terminal,
            err_is_terminal: std::io::stderr().is_terminal(),
            out_is_unicode: terminal_takes_unicode(out_is_terminal, &vars),
            vars,
        })
    }

    /// A test environment with in-memory streams and no terminal.
    ///
    /// This is the harness the headless parity tests drive: same code path,
    /// no TTY, output captured.
    pub fn test(args: &[&str], cwd: impl Into<PathBuf>) -> (Self, Captured) {
        let (out, out_buf) = Stream::buffer();
        let (err, err_buf) = Stream::buffer();
        let mut full = vec!["bit-cli".to_string()];
        full.extend(args.iter().map(|a| a.to_string()));
        let env = Self {
            args: full,
            cwd: cwd.into(),
            vars: BTreeMap::new(),
            out,
            err,
            out_is_terminal: false,
            err_is_terminal: false,
            // A buffer takes the bytes exactly as they were written, so the
            // one thing that can garble a code point is not present here.
            out_is_unicode: true,
        };
        (
            env,
            Captured {
                out: out_buf,
                err: err_buf,
            },
        )
    }

    /// Look up an environment variable.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    /// Resolve a path against the working directory.
    pub fn resolve(&self, path: &Path) -> PathBuf {
        match path.is_absolute() {
            true => path.to_path_buf(),
            false => self.cwd.join(path),
        }
    }

    /// Whether colour should be used, honouring `NO_COLOR` and `CLICOLOR`.
    ///
    /// `NO_COLOR` is respected whatever its value, per the convention: its
    /// presence is the signal.
    pub fn wants_color(&self, requested: ColorChoice) -> bool {
        match requested {
            ColorChoice::Never => false,
            ColorChoice::Always => true,
            ColorChoice::Auto => {
                if self.vars.contains_key("NO_COLOR") {
                    return false;
                }
                if self.var("CLICOLOR") == Some("0") {
                    return false;
                }
                if self.var("TERM") == Some("dumb") {
                    return false;
                }
                self.out_is_terminal
            }
        }
    }

    /// Write a line of data to stdout.
    pub fn say(&mut self, line: impl AsRef<str>) -> std::io::Result<()> {
        writeln!(self.out, "{}", line.as_ref())
    }

    /// Write a line of diagnostics to stderr.
    pub fn note(&mut self, line: impl AsRef<str>) -> std::io::Result<()> {
        writeln!(self.err, "{}", line.as_ref())
    }
}

/// Whether a terminal at the far end of stdout can render a code point
/// outside ASCII.
///
/// Asked only when stdout **is** a terminal: a file or a pipe carries the
/// bytes to whatever reads them next and decodes nothing, so there is nothing
/// there to get wrong.
///
/// On Windows the answer is the console output code page, which is `IBM437`
/// out of the box on an English install and is not UTF-8 until somebody
/// changes it. Elsewhere it is the locale, which is what a terminal emulator
/// and the C library both read; POSIX defaults to the C locale, so no locale
/// set is not a UTF-8 one.
fn terminal_takes_unicode(out_is_terminal: bool, vars: &BTreeMap<String, String>) -> bool {
    if !out_is_terminal {
        return true;
    }
    #[cfg(windows)]
    {
        let _ = vars;
        /// `CP_UTF8`.
        const CP_UTF8: u32 = 65001;

        unsafe extern "system" {
            fn GetConsoleOutputCP() -> u32;
        }

        // SAFETY: the function takes nothing, returns a plain integer, and
        // has no failure mode beyond returning zero when the process has no
        // console. Zero is not `CP_UTF8`, which is the conservative answer.
        unsafe { GetConsoleOutputCP() == CP_UTF8 }
    }
    #[cfg(not(windows))]
    {
        ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|name| vars.get(*name))
            .is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("utf-8") || value.contains("utf8")
            })
    }
}

/// When to use colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    /// Colour when stdout is a terminal and nothing objects.
    #[default]
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

/// Handles to the buffers a test environment writes into.
pub struct Captured {
    out: Arc<Mutex<Vec<u8>>>,
    err: Arc<Mutex<Vec<u8>>>,
}

impl Captured {
    /// Everything written to stdout, as raw bytes.
    ///
    /// Commands that write a payload to stdout, such as `create -o -`, emit
    /// binary. Reading that back through [`Self::out`] would replace every
    /// non-UTF-8 byte with U+FFFD, so a test checking binary output has to
    /// come through here.
    pub fn out_bytes(&self) -> Vec<u8> {
        self.out.lock().expect("stdout buffer").clone()
    }

    /// Everything written to stdout, as UTF-8.
    pub fn out(&self) -> String {
        String::from_utf8_lossy(&self.out_bytes()).into_owned()
    }

    /// Everything written to stderr, as UTF-8.
    pub fn err(&self) -> String {
        String::from_utf8_lossy(&self.err.lock().expect("stderr buffer").clone()).into_owned()
    }

    /// stdout parsed as JSON.
    pub fn json(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::from_str(&self.out())
    }

    /// stdout parsed as one JSON value per line.
    pub fn jsonl(&self) -> serde_json::Result<Vec<serde_json::Value>> {
        self.out()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(serde_json::from_str)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_env_captures_both_streams_separately() {
        let (mut env, captured) = Env::test(&["info", "x.torrent"], "/tmp");
        env.say("data").unwrap();
        env.note("a warning").unwrap();
        assert_eq!(captured.out(), "data\n");
        assert_eq!(captured.err(), "a warning\n");
    }

    #[test]
    fn the_program_name_is_argument_zero() {
        let (env, _) = Env::test(&["info"], "/tmp");
        assert_eq!(env.args, ["bit-cli", "info"]);
    }

    #[test]
    fn a_test_env_never_reports_a_terminal() {
        let (env, _) = Env::test(&[], "/tmp");
        assert!(!env.out_is_terminal);
        assert!(!env.err_is_terminal);
        assert!(!env.wants_color(ColorChoice::Auto));
    }

    /// A file and a pipe carry whatever bytes were written, so the question
    /// only has a wrong answer when a terminal is decoding them.
    #[test]
    fn anything_that_is_not_a_terminal_takes_unicode() {
        assert!(terminal_takes_unicode(false, &BTreeMap::new()));
    }

    #[cfg(not(windows))]
    #[test]
    fn a_terminal_takes_unicode_when_the_locale_says_so() {
        let locale =
            |name: &str, value: &str| BTreeMap::from([(name.to_string(), value.to_string())]);
        assert!(terminal_takes_unicode(true, &locale("LANG", "en_US.UTF-8")));
        assert!(terminal_takes_unicode(true, &locale("LC_ALL", "C.utf8")));
        assert!(!terminal_takes_unicode(true, &locale("LANG", "C")));
        assert!(!terminal_takes_unicode(true, &locale("LANG", "en_US")));
        // POSIX defaults to the C locale, so nothing set is not UTF-8.
        assert!(!terminal_takes_unicode(true, &BTreeMap::new()));
        // The most specific variable set is the one that answers.
        assert!(!terminal_takes_unicode(
            true,
            &BTreeMap::from([
                ("LC_ALL".to_string(), "C".to_string()),
                ("LANG".to_string(), "en_US.UTF-8".to_string()),
            ])
        ));
    }

    #[test]
    fn color_always_wins_over_a_missing_terminal_and_never_wins_over_everything() {
        let (env, _) = Env::test(&[], "/tmp");
        assert!(env.wants_color(ColorChoice::Always));
        assert!(!env.wants_color(ColorChoice::Never));
    }

    #[test]
    fn no_color_is_honoured_whatever_its_value() {
        for value in ["", "0", "1", "false"] {
            let (mut env, _) = Env::test(&[], "/tmp");
            env.out_is_terminal = true;
            env.vars.insert("NO_COLOR".to_string(), value.to_string());
            assert!(
                !env.wants_color(ColorChoice::Auto),
                "NO_COLOR={value:?} should disable colour"
            );
            assert!(
                env.wants_color(ColorChoice::Always),
                "--color=always still wins"
            );
        }
    }

    #[test]
    fn clicolor_zero_and_a_dumb_terminal_disable_colour() {
        for (name, value) in [("CLICOLOR", "0"), ("TERM", "dumb")] {
            let (mut env, _) = Env::test(&[], "/tmp");
            env.out_is_terminal = true;
            env.vars.insert(name.to_string(), value.to_string());
            assert!(
                !env.wants_color(ColorChoice::Auto),
                "{name}={value} should disable colour"
            );
        }
    }

    #[test]
    fn colour_is_on_at_a_terminal_when_nothing_objects() {
        let (mut env, _) = Env::test(&[], "/tmp");
        env.out_is_terminal = true;
        assert!(env.wants_color(ColorChoice::Auto));
    }

    #[test]
    fn relative_paths_resolve_against_the_injected_working_directory() {
        let (env, _) = Env::test(&[], "/work");
        assert_eq!(
            env.resolve(Path::new("a/b.torrent")),
            PathBuf::from("/work/a/b.torrent")
        );
        let absolute = if cfg!(windows) {
            Path::new(r"C:\x")
        } else {
            Path::new("/x")
        };
        assert_eq!(env.resolve(absolute), absolute);
    }

    #[test]
    fn captured_output_parses_as_json_and_as_ndjson() {
        let (mut env, captured) = Env::test(&[], "/tmp");
        env.say(r#"{"a":1}"#).unwrap();
        assert_eq!(captured.json().unwrap()["a"], 1);

        let (mut env, captured) = Env::test(&[], "/tmp");
        env.say(r#"{"seq":0}"#).unwrap();
        env.say(r#"{"seq":1}"#).unwrap();
        let events = captured.jsonl().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["seq"], 1);
    }

    #[test]
    fn variables_are_read_from_the_injected_map_only() {
        let (mut env, _) = Env::test(&[], "/tmp");
        assert!(env.var("BIT_CLI_MAX_PEERS").is_none());
        env.vars
            .insert("BIT_CLI_MAX_PEERS".to_string(), "50".to_string());
        assert_eq!(env.var("BIT_CLI_MAX_PEERS"), Some("50"));
    }
}
