//! Errors that carry their exit code.
//!
//! Every failure in `bit-cli` answers three questions: what happened, what the
//! caller should do about it, and what the process should exit with. [`Error`]
//! carries all three, so the exit code is decided where the failure is known
//! rather than guessed at in `main`.
//!
//! The `context` field is a JSON object. It is where the machine-readable
//! detail goes: the uncovered piece indices for a coverage gap, the failing
//! URL and status for an HTTP error, the lint name for a refused torrent. A
//! caller reads `context` instead of parsing `message`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::exit::ExitCode;

/// The result type used across the library.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A failure with an exit code, a stable kind, and structured context.
#[derive(Debug)]
pub struct Error {
    code: ExitCode,
    message: String,
    context: Map<String, Value>,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

/// The JSON shape of an [`Error`]. This is what `--json` writes to stdout when
/// a command fails, alongside a non-zero exit code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorReport {
    /// The numeric exit code the process will use.
    pub code: u8,
    /// A stable string naming the failure class. Safe to match on.
    pub kind: String,
    /// One sentence for a person to read.
    pub message: String,
    /// The full cause chain, outermost first. Empty when there is no cause.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<String>,
    /// Machine-readable detail specific to this failure.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub context: Map<String, Value>,
}

impl Error {
    /// A new error with a code and a message.
    pub fn new(code: ExitCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Map::new(),
            source: None,
        }
    }

    /// Attach a machine-readable field. Call it once per fact worth branching
    /// on; a caller should never have to parse [`Self::message`].
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Attach the underlying cause.
    #[must_use]
    pub fn caused_by(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// The exit code this failure produces.
    pub fn code(&self) -> ExitCode {
        self.code
    }

    /// The one-sentence message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The machine-readable context.
    pub fn context(&self) -> &Map<String, Value> {
        &self.context
    }

    /// The cause chain, outermost first.
    pub fn causes(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut next = self
            .source
            .as_ref()
            .map(|s| s.as_ref() as &dyn std::error::Error);
        while let Some(err) = next {
            out.push(err.to_string());
            next = err.source();
        }
        out
    }

    /// Render as the JSON object a caller parses.
    pub fn report(&self) -> ErrorReport {
        ErrorReport {
            code: self.code.code(),
            kind: self.code.kind().to_string(),
            message: self.message.clone(),
            causes: self.causes(),
            context: self.context.clone(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        for cause in self.causes() {
            write!(f, ": {cause}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn std::error::Error + 'static))
    }
}

/// Classify an [`std::io::Error`] and turn it into an [`Error`].
///
/// Disk problems and network problems have different exit codes and a caller
/// branches on that, so the classification happens once here rather than at
/// every call site.
pub fn from_io(err: std::io::Error, what: impl Into<String>) -> Error {
    use std::io::ErrorKind;
    let code = match err.kind() {
        ErrorKind::NotFound
        | ErrorKind::PermissionDenied
        | ErrorKind::AlreadyExists
        | ErrorKind::StorageFull
        | ErrorKind::ReadOnlyFilesystem
        | ErrorKind::InvalidFilename
        | ErrorKind::IsADirectory
        | ErrorKind::NotADirectory
        | ErrorKind::DirectoryNotEmpty => ExitCode::Disk,
        ErrorKind::ConnectionRefused
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::NotConnected
        | ErrorKind::AddrInUse
        | ErrorKind::AddrNotAvailable
        | ErrorKind::HostUnreachable
        | ErrorKind::NetworkUnreachable
        | ErrorKind::NetworkDown
        | ErrorKind::BrokenPipe => ExitCode::Network,
        ErrorKind::TimedOut => ExitCode::Timeout,
        ErrorKind::Interrupted => ExitCode::Interrupted,
        _ => ExitCode::Generic,
    };
    Error::new(code, what)
        .with("io_kind", format!("{:?}", err.kind()))
        .caused_by(err)
}

/// Shorthand constructors, one per code, so a call site never has to name the
/// enum and the message stays the only thing being written.
macro_rules! constructors {
    ($($name:ident => $variant:ident),+ $(,)?) => {
        impl Error {
            $(
                #[doc = concat!("An [`ExitCode::", stringify!($variant), "`] error.")]
                pub fn $name(message: impl Into<String>) -> Self {
                    Self::new(ExitCode::$variant, message)
                }
            )+
        }
    };
}

constructors! {
    generic => Generic,
    usage => Usage,
    config => Config,
    source_resolution => SourceResolution,
    network => Network,
    no_usable_sources => NoUsableSources,
    hash_mismatch => HashMismatch,
    disk => Disk,
    timeout => Timeout,
    interrupted => Interrupted,
    coverage_gap => CoverageGap,
    binding => Binding,
    lint_refused => LintRefused,
    threshold_not_met => ThresholdNotMet,
    would_change_infohash => WouldChangeInfoHash,
}

/// Add context to a `Result` without losing the error's code.
pub trait Context<T> {
    /// Wrap the error's message, keeping its code and context.
    fn context(self, message: impl Into<String>) -> Result<T>;

    /// Wrap the error's message, building it only on the error path.
    fn with_context<S: Into<String>>(self, f: impl FnOnce() -> S) -> Result<T>;
}

impl<T> Context<T> for Result<T> {
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.map_err(|e| Error {
            code: e.code,
            message: message.into(),
            context: e.context.clone(),
            source: Some(Box::new(e)),
        })
    }

    fn with_context<S: Into<String>>(self, f: impl FnOnce() -> S) -> Result<T> {
        self.map_err(|e| Error {
            code: e.code,
            message: f().into(),
            context: e.context.clone(),
            source: Some(Box::new(e)),
        })
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Error::new(ExitCode::Generic, format!("{err}")).with("cause_chain", format!("{err:#}"))
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        from_io(err, "I/O failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_carries_its_code_into_the_report() {
        let err =
            Error::coverage_gap("pieces 10-12 have no source").with("uncovered", vec![10, 11, 12]);
        let report = err.report();
        assert_eq!(report.code, 11);
        assert_eq!(report.kind, "coverage_gap");
        assert_eq!(report.context["uncovered"], serde_json::json!([10, 11, 12]));
    }

    #[test]
    fn context_keeps_the_original_code() {
        let inner: Result<()> = Err(Error::hash_mismatch("piece 4 does not match"));
        let outer = inner.context("web seed fetch failed").unwrap_err();
        assert_eq!(outer.code(), ExitCode::HashMismatch);
        assert_eq!(outer.causes(), vec!["piece 4 does not match".to_string()]);
    }

    #[test]
    fn display_shows_the_whole_cause_chain() {
        let inner: Result<()> = Err(Error::network("connection refused"));
        let outer = inner.context("could not reach the tracker").unwrap_err();
        assert_eq!(
            outer.to_string(),
            "could not reach the tracker: connection refused"
        );
    }

    #[test]
    fn io_errors_are_classified_by_kind() {
        let disk = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        assert_eq!(from_io(disk, "write failed").code(), ExitCode::Disk);

        let net = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        assert_eq!(from_io(net, "connect failed").code(), ExitCode::Network);

        let slow = std::io::Error::new(std::io::ErrorKind::TimedOut, "slow");
        assert_eq!(from_io(slow, "read failed").code(), ExitCode::Timeout);
    }

    #[test]
    fn a_report_round_trips_through_json() {
        let err = Error::binding("selector `*.iso` matched no files").with("selector", "*.iso");
        let json = serde_json::to_string(&err.report()).unwrap();
        let back: ErrorReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, 12);
        assert_eq!(back.context["selector"], "*.iso");
    }
}
