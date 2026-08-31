//! Exit codes.
//!
//! The exit code is the primary success signal. A caller must be able to
//! branch on it without parsing any text, so every code means exactly one
//! thing and no code is ever reused for a second meaning.
//!
//! Codes 11 through 17 exist so a script can tell "your mirrors are
//! misconfigured" apart from "the network is down" apart from "your server is
//! slow" apart from "the process is out of handles" apart from "the port is
//! open and answers nobody". That distinction is the point of the table.
//!
//! On Windows the code is read from `$LASTEXITCODE` in PowerShell, not `$?`.

use serde::{Deserialize, Serialize};

/// Every exit code `bit-cli` can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum ExitCode {
    /// The command did what it was asked to do.
    Success = 0,
    /// Something failed that no other code describes. Treat a `Generic` in
    /// practice as a missing code and add one.
    Generic = 1,
    /// The arguments were wrong: unknown flag, bad value, conflicting options.
    Usage = 2,
    /// A configuration file or environment variable was unreadable or invalid.
    Config = 3,
    /// The source could not be resolved: bad magnet, unreadable torrent, 404
    /// fetching the `.torrent`, malformed metalink.
    SourceResolution = 4,
    /// The network failed: no route, DNS failure, TLS failure.
    Network = 5,
    /// Nothing can serve the data: no peers, no seeds, every web seed dead.
    NoUsableSources = 6,
    /// A hash did not match: a piece, a file, or a `webseed fetch` range.
    HashMismatch = 7,
    /// The disk failed: out of space, permission denied, path too long.
    Disk = 8,
    /// A deadline passed: `--timeout`, `--stop-after`, `--stop-timeout`.
    Timeout = 9,
    /// The user interrupted the run. Partial state was saved.
    Interrupted = 10,
    /// Web seed scopes and available peers cannot cover every piece. The error
    /// names the uncovered piece indices.
    CoverageGap = 11,
    /// A binding is wrong: a scope selector matched nothing, or `mode=exact`
    /// was used with a scope that resolves to more than one file.
    Binding = 12,
    /// A lint refused a torrent at creation. `bit-cli create` only. The error
    /// names the lint, and `--allow <LINT>` clears it.
    LintRefused = 13,
    /// A threshold was not met: `bench --fail-under`, or `seed
    /// --exit-when-idle` reached having never seen a peer.
    ThresholdNotMet = 14,
    /// The edit would change the info hash. `bit-cli edit` only, without
    /// `--allow-new-infohash`.
    WouldChangeInfoHash = 15,
    /// A resource ceiling the caller set was crossed: `--max-handles`.
    ResourceCeiling = 16,
    /// This run's own listener stopped answering: `--listener-check`. The
    /// process is alive and the port is open, which is why this is not
    /// `Generic`: a supervisor that restarts on it is restarting a seeder
    /// that serves nobody. See `TODO/peers.md`, T-020.
    ListenerUnhealthy = 17,
}

impl ExitCode {
    /// The numeric code handed to the operating system.
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// A stable machine-readable name. This is the `kind` field of a JSON
    /// error object and it never changes once released.
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Generic => "generic",
            Self::Usage => "usage",
            Self::Config => "config",
            Self::SourceResolution => "source_resolution",
            Self::Network => "network",
            Self::NoUsableSources => "no_usable_sources",
            Self::HashMismatch => "hash_mismatch",
            Self::Disk => "disk",
            Self::Timeout => "timeout",
            Self::Interrupted => "interrupted",
            Self::CoverageGap => "coverage_gap",
            Self::Binding => "binding",
            Self::LintRefused => "lint_refused",
            Self::ThresholdNotMet => "threshold_not_met",
            Self::WouldChangeInfoHash => "would_change_infohash",
            Self::ResourceCeiling => "resource_ceiling",
            Self::ListenerUnhealthy => "listener_unhealthy",
        }
    }

    /// One line describing what the code means, used by `bit-cli version` and
    /// by the generated documentation.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::Generic => "Generic failure",
            Self::Usage => "Usage or argument error",
            Self::Config => "Configuration error",
            Self::SourceResolution => "Source resolution failed",
            Self::Network => "Network failure",
            Self::NoUsableSources => "No usable sources",
            Self::HashMismatch => "Hash verification failed",
            Self::Disk => "Disk error",
            Self::Timeout => "Timeout or deadline exceeded",
            Self::Interrupted => "Interrupted by the user, partial state saved",
            Self::CoverageGap => "Coverage gap: some pieces have no source",
            Self::Binding => "Binding error: a scope selector or composition mode is invalid",
            Self::LintRefused => "Lint refused a torrent at creation",
            Self::ThresholdNotMet => "Threshold not met",
            Self::WouldChangeInfoHash => "Would change the info hash",
            Self::ResourceCeiling => "A resource ceiling was crossed",
            Self::ListenerUnhealthy => "This run's own listener stopped answering",
        }
    }

    /// Every code, in numeric order. Used to render the documented table and
    /// to check in tests that the table stays complete.
    pub const ALL: &'static [ExitCode] = &[
        Self::Success,
        Self::Generic,
        Self::Usage,
        Self::Config,
        Self::SourceResolution,
        Self::Network,
        Self::NoUsableSources,
        Self::HashMismatch,
        Self::Disk,
        Self::Timeout,
        Self::Interrupted,
        Self::CoverageGap,
        Self::Binding,
        Self::LintRefused,
        Self::ThresholdNotMet,
        Self::WouldChangeInfoHash,
        Self::ResourceCeiling,
        Self::ListenerUnhealthy,
    ];
}

impl std::fmt::Display for ExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.code(), self.kind())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn codes_are_contiguous_from_zero() {
        for (index, code) in ExitCode::ALL.iter().enumerate() {
            assert_eq!(code.code() as usize, index, "{code:?} is out of order");
        }
    }

    #[test]
    fn no_code_is_reused_for_two_meanings() {
        let codes: HashSet<u8> = ExitCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(codes.len(), ExitCode::ALL.len());
    }

    #[test]
    fn every_kind_is_unique_and_snake_case() {
        let kinds: HashSet<&str> = ExitCode::ALL.iter().map(|c| c.kind()).collect();
        assert_eq!(kinds.len(), ExitCode::ALL.len());
        for code in ExitCode::ALL {
            let kind = code.kind();
            assert!(
                kind.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{kind} is not snake_case"
            );
        }
    }

    #[test]
    fn the_documented_range_is_covered() {
        assert_eq!(ExitCode::ALL.len(), 18);
        assert_eq!(ExitCode::ListenerUnhealthy.code(), 17);
    }
}
