//! Lints for torrent creation.
//!
//! A lint catches something that is legal but is almost always a mistake, and
//! refuses to write the torrent until the caller says otherwise. That is the
//! failure mode a scripted pipeline needs caught: a private torrent with no
//! tracker is not an error under BEP 3, it is just useless, and finding out
//! after publishing it is expensive.
//!
//! Every lint has a stable name that can be passed to `--allow`, so a script
//! can permit exactly the one it means to permit.
//!
//! The idea and the flag shape come from `intermodal` (CC0-1.0).

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::layout::Layout;
use crate::units::format_size;

/// Everything `bit-cli create` refuses on unless told otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lint {
    /// A private torrent with no tracker can never find a peer.
    PrivateNoTracker,
    /// The piece length yields a piece count that will hurt.
    PieceCount,
    /// The piece count is above 65,535, which µTorrent will not open.
    ///
    /// Separate from [`Self::PieceCount`] on purpose, and the two thresholds
    /// are different kinds of thing: that one is an opinion about how much
    /// hash data is reasonable and this one is a client that refuses the file.
    /// A caller who has decided to live with 200,000 pieces of hash data has
    /// not thereby decided to ship a torrent µTorrent cannot read, so the two
    /// clear independently. See `TODO/create-seed.md`, T-176.
    PieceCountUnopenable,
    /// The piece length is not a power of two.
    PieceLengthNotPowerOfTwo,
    /// The piece length is above 16 MiB, which has been reported to break
    /// clients.
    ///
    /// The judgement lives here rather than in `piece_length::validate`,
    /// which refuses only zero because only zero is impossible. See
    /// `TODO/create-seed.md`, T-176.
    PieceLengthTooLarge,
    /// The payload is empty.
    EmptyPayload,
    /// A file is empty. Legal, but it usually means a glob went wrong.
    EmptyFile,
    /// A path will not work on Windows: a reserved device name, a trailing dot
    /// or space, or a character NTFS refuses.
    WindowsPath,
    /// Two files claim the exact same path, which makes the `files` list
    /// invalid rather than merely awkward.
    ///
    /// Split from [`Self::CaseCollision`], which caught it by accident and
    /// said the paths "differ only in case" when they were identical. A
    /// reader then went looking for a casing difference that was not there.
    /// See `TODO/create-seed.md`, T-176.
    DuplicatePath,
    /// Two paths differ only in case, so they collide on a case-insensitive
    /// filesystem.
    CaseCollision,
    /// A `url-list` entry is not an absolute HTTP URL.
    BadWebSeed,
    /// A tracker URL is not one of the schemes a client will announce to.
    BadTracker,
    /// A path component is long enough to break on some filesystems.
    LongPath,
}

impl Lint {
    /// The name used with `--allow`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::PrivateNoTracker => "private-no-tracker",
            Self::PieceCount => "piece-count",
            Self::PieceCountUnopenable => "piece-count-unopenable",
            Self::PieceLengthNotPowerOfTwo => "piece-length-not-power-of-two",
            Self::PieceLengthTooLarge => "piece-length-too-large",
            Self::EmptyPayload => "empty-payload",
            Self::EmptyFile => "empty-file",
            Self::WindowsPath => "windows-path",
            Self::DuplicatePath => "duplicate-path",
            Self::CaseCollision => "case-collision",
            Self::BadWebSeed => "bad-web-seed",
            Self::BadTracker => "bad-tracker",
            Self::LongPath => "long-path",
        }
    }

    /// One line describing what the lint catches, for the docs and `--help`.
    pub const fn description(self) -> &'static str {
        match self {
            Self::PrivateNoTracker => {
                "A private torrent with no announce URL can never find a peer"
            }
            Self::PieceCount => "The piece length gives a piece count that will not work well",
            Self::PieceCountUnopenable => {
                "The piece count is above 65,535, which µTorrent will not open"
            }
            Self::PieceLengthNotPowerOfTwo => "The piece length is not a power of two",
            Self::PieceLengthTooLarge => {
                "The piece length is above 16 MiB, which breaks some clients"
            }
            Self::EmptyPayload => "The torrent contains no data",
            Self::EmptyFile => "The torrent contains a zero-length file",
            Self::WindowsPath => "A path cannot be written on Windows",
            Self::DuplicatePath => "Two files in the torrent claim the same path",
            Self::CaseCollision => "Two paths differ only in case and collide on NTFS and APFS",
            Self::BadWebSeed => "A web seed URL is not an absolute http or https URL",
            Self::BadTracker => "A tracker URL uses a scheme clients do not announce to",
            Self::LongPath => "A path component is long enough to break on some filesystems",
        }
    }

    /// Parse a lint name.
    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|lint| lint.name() == name.trim())
            .ok_or_else(|| {
                let names: Vec<&str> = Self::ALL.iter().map(|l| l.name()).collect();
                Error::usage(format!(
                    "`{name}` is not a lint (known lints: {})",
                    names.join(", ")
                ))
                .with("lint", name.to_string())
            })
    }

    /// Every lint, in a stable order.
    pub const ALL: &'static [Lint] = &[
        Self::PrivateNoTracker,
        Self::PieceCount,
        Self::PieceCountUnopenable,
        Self::PieceLengthNotPowerOfTwo,
        Self::PieceLengthTooLarge,
        Self::EmptyPayload,
        Self::EmptyFile,
        Self::WindowsPath,
        Self::DuplicatePath,
        Self::CaseCollision,
        Self::BadWebSeed,
        Self::BadTracker,
        Self::LongPath,
    ];
}

impl fmt::Display for Lint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One thing a lint found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Which lint fired.
    pub lint: Lint,
    /// What it found, naming the offending value.
    pub message: String,
}

/// What the torrent being created looks like, as the lints need to see it.
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    pub layout: &'a Layout,
    pub private: bool,
    pub trackers: &'a [String],
    pub web_seeds: &'a [String],
}

/// Run every lint that is not allowed.
pub fn check(candidate: &Candidate<'_>, allowed: &BTreeSet<Lint>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut fire = |lint: Lint, message: String| {
        if !allowed.contains(&lint) {
            findings.push(Finding { lint, message });
        }
    };
    let layout = candidate.layout;

    if layout.total_length == 0 {
        fire(
            Lint::EmptyPayload,
            "the torrent contains no data".to_string(),
        );
    }
    if candidate.private && candidate.trackers.is_empty() {
        fire(
            Lint::PrivateNoTracker,
            "the torrent is private but has no announce URL, so no peer can ever find it"
                .to_string(),
        );
    }
    if !super::piece_length::is_power_of_two(layout.piece_length) {
        fire(
            Lint::PieceLengthNotPowerOfTwo,
            format!("piece length {} is not a power of two", layout.piece_length),
        );
    }

    // A ceiling a client enforces, not an opinion about hash data. intermodal
    // Issue 499 records that µTorrent refuses a torrent with more than 65,535
    // pieces, so the band from 65,536 up to this repository's own 100,000
    // threshold used to pass every check and produce a file µTorrent cannot
    // open. See `TODO/create-seed.md`, T-176.
    const UNOPENABLE_PIECES: u32 = 65_535;

    // The practical ceiling on a piece length, from intermodal Issue 358.
    // `piece_length::MAX` is the same number and caps only the **automatic**
    // choice, so `--piece-length 64MiB` was accepted in silence.
    let too_large = crate::torrent::piece_length::MAX;
    if layout.piece_length > too_large {
        fire(
            Lint::PieceLengthTooLarge,
            format!(
                "piece length {} is above the {} clients are known to handle",
                format_size(u64::from(layout.piece_length)),
                format_size(u64::from(too_large))
            ),
        );
    }

    let pieces = layout.piece_count();
    if pieces > UNOPENABLE_PIECES {
        fire(
            Lint::PieceCountUnopenable,
            format!(
                "{pieces} pieces is above the {UNOPENABLE_PIECES} µTorrent will open; raise --piece-length"
            ),
        );
    }
    if pieces > 100_000 {
        fire(
            Lint::PieceCount,
            format!(
                "{pieces} pieces at {} each gives {} of piece hashes; raise --piece-length",
                format_size(u64::from(layout.piece_length)),
                format_size(u64::from(pieces) * 20)
            ),
        );
    } else if pieces > 0 && pieces < 8 && layout.total_length > TOO_FEW_PIECES_ABOVE {
        // Few pieces only hurt once the payload is large enough that peers
        // have to finish a lot of bytes before any of it verifies. A 45 KiB
        // torrent having three pieces is fine and must not be flagged.
        fire(
            Lint::PieceCount,
            format!(
                "only {pieces} pieces at {} each for {}; lower --piece-length so peers can share partial progress",
                format_size(u64::from(layout.piece_length)),
                format_size(layout.total_length)
            ),
        );
    }

    let mut lowercased: BTreeSet<String> = BTreeSet::new();
    let mut exact: BTreeSet<String> = BTreeSet::new();
    for file in &layout.files {
        let path = file.display_path();
        if file.length == 0 {
            fire(Lint::EmptyFile, format!("`{path}` is empty"));
        }
        if let Some(reason) = windows_path_problem(file) {
            fire(Lint::WindowsPath, format!("`{path}`: {reason}"));
        }
        if let Some(component) = file.path.iter().find(|c| c.len() > 255) {
            fire(
                Lint::LongPath,
                format!(
                    "`{path}` has a {} character path component",
                    component.len()
                ),
            );
        }
        // Two sets, not one. Keying only on the lower-cased path caught an
        // exact duplicate as well and told the reader the two paths "differ
        // only in case", which is false when they are identical. See
        // `TODO/create-seed.md`, T-176.
        if !exact.insert(path.clone()) {
            fire(
                Lint::DuplicatePath,
                format!("`{path}` is claimed by more than one file"),
            );
        } else if !lowercased.insert(path.to_lowercase()) {
            fire(
                Lint::CaseCollision,
                format!("`{path}` collides with another path that differs only in case"),
            );
        }
    }

    for url in candidate.web_seeds {
        let lower = url.to_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            fire(
                Lint::BadWebSeed,
                format!("`{url}` is not an absolute http or https URL"),
            );
        }
    }
    for url in candidate.trackers {
        let lower = url.to_lowercase();
        let known = ["http://", "https://", "udp://", "ws://", "wss://"];
        if !known.iter().any(|scheme| lower.starts_with(scheme)) {
            fire(
                Lint::BadTracker,
                format!("`{url}` does not use a scheme clients announce to"),
            );
        }
    }

    findings
}

/// Payload size above which a very low piece count is worth complaining about.
///
/// Below this, few pieces is simply what a small torrent looks like.
const TOO_FEW_PIECES_ABOVE: u64 = 64 * crate::units::MIB;

/// Windows device names, which cannot be used as a path component with or
/// without an extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Characters NTFS refuses in a file name.
const ILLEGAL: &[char] = &['<', '>', ':', '"', '|', '?', '*', '\\', '/'];

/// Why a path cannot be written on Windows, if it cannot.
///
/// This runs on every platform. A torrent created on Linux that cannot be
/// downloaded on Windows is a real problem for whoever downloads it, and the
/// creator is the only one who can still fix it.
pub fn windows_path_problem(file: &crate::layout::LayoutFile) -> Option<String> {
    for component in &file.path {
        if component.is_empty() {
            return Some("has an empty path component".to_string());
        }
        let stem = component.split('.').next().unwrap_or(component);
        if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
            return Some(format!("`{component}` is a reserved Windows device name"));
        }
        if component.ends_with('.') || component.ends_with(' ') {
            return Some(format!("`{component}` ends with a dot or a space"));
        }
        if let Some(bad) = component.chars().find(|c| ILLEGAL.contains(c)) {
            return Some(format!(
                "`{component}` contains `{bad}`, which NTFS refuses"
            ));
        }
        if let Some(bad) = component.chars().find(|c| (*c as u32) < 0x20) {
            return Some(format!(
                "`{component}` contains control character U+{:04X}",
                bad as u32
            ));
        }
    }
    None
}

/// Turn findings into the error `bit-cli create` exits with.
pub fn refuse(findings: &[Finding]) -> Error {
    let names: Vec<&str> = findings.iter().map(|f| f.lint.name()).collect();
    let lines: Vec<String> = findings
        .iter()
        .map(|f| format!("{}: {}", f.lint.name(), f.message))
        .collect();
    Error::lint_refused(format!(
        "{} lint(s) refused this torrent:\n  {}\nPass --allow <LINT> to proceed anyway.",
        findings.len(),
        lines.join("\n  ")
    ))
    .with("lints", serde_json::to_value(&names).unwrap_or_default())
    .with(
        "findings",
        serde_json::to_value(findings).unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{GIB, KIB, MIB};

    fn layout(files: &[(&str, u64)], piece_length: u32) -> Layout {
        Layout::from_lengths(
            "t",
            files.len() > 1,
            piece_length,
            files.iter().map(|(p, l)| (p.to_string(), *l)),
        )
    }

    fn check_one(
        layout: &Layout,
        private: bool,
        trackers: &[String],
        seeds: &[String],
    ) -> Vec<Finding> {
        check(
            &Candidate {
                layout,
                private,
                trackers,
                web_seeds: seeds,
            },
            &BTreeSet::new(),
        )
    }

    fn fired(findings: &[Finding], lint: Lint) -> bool {
        findings.iter().any(|f| f.lint == lint)
    }

    #[test]
    fn a_clean_torrent_produces_no_findings() {
        let layout = layout(&[("a.bin", 100 * MIB)], 256 * 1024);
        let trackers = vec!["udp://tracker.example.com:80".to_string()];
        let seeds = vec!["https://mirror.example.com/pub/".to_string()];
        assert!(check_one(&layout, false, &trackers, &seeds).is_empty());
    }

    #[test]
    fn a_private_torrent_with_no_tracker_is_caught() {
        let layout = layout(&[("a.bin", 100 * MIB)], 256 * 1024);
        let findings = check_one(&layout, true, &[], &[]);
        assert!(fired(&findings, Lint::PrivateNoTracker));
        // Not private, so no finding.
        assert!(!fired(
            &check_one(&layout, false, &[], &[]),
            Lint::PrivateNoTracker
        ));
    }

    #[test]
    fn an_absurd_piece_count_is_caught_in_both_directions() {
        let too_many = layout(&[("a.bin", 100 * GIB)], 16 * 1024);
        assert!(fired(
            &check_one(&too_many, false, &[], &[]),
            Lint::PieceCount
        ));

        // 512 MiB in 128 MiB pieces is four pieces: a peer has to finish
        // 128 MiB before any of it verifies.
        let too_few = layout(&[("a.bin", 512 * MIB)], 128 * MIB as u32);
        assert!(fired(
            &check_one(&too_few, false, &[], &[]),
            Lint::PieceCount
        ));
    }

    #[test]
    fn a_small_torrent_with_few_pieces_is_not_flagged() {
        // 45 KiB in three 16 KiB pieces is exactly what a small torrent looks
        // like, and flagging it would make `create` unusable on small inputs.
        let small = layout(&[("a.bin", 45_000)], 16 * 1024);
        assert!(!fired(
            &check_one(&small, false, &[], &[]),
            Lint::PieceCount
        ));
    }

    #[test]
    fn a_non_power_of_two_piece_length_is_caught() {
        let layout = layout(&[("a.bin", 100 * MIB)], 100_000);
        assert!(fired(
            &check_one(&layout, false, &[], &[]),
            Lint::PieceLengthNotPowerOfTwo
        ));
    }

    #[test]
    fn empty_payloads_and_empty_files_are_caught() {
        let empty = layout(&[("a.bin", 0)], 16 * 1024);
        let findings = check_one(&empty, false, &[], &[]);
        assert!(fired(&findings, Lint::EmptyPayload));
        assert!(fired(&findings, Lint::EmptyFile));
    }

    #[test]
    fn reserved_windows_names_are_caught_with_and_without_an_extension() {
        for name in ["CON", "con.txt", "NUL", "lpt9.log", "COM1.bin"] {
            let l = layout(&[(name, 100 * MIB), ("other.bin", 100 * MIB)], 256 * 1024);
            assert!(
                fired(&check_one(&l, false, &[], &[]), Lint::WindowsPath),
                "{name} should be caught"
            );
        }
    }

    #[test]
    fn trailing_dots_spaces_and_illegal_characters_are_caught() {
        for name in [
            "file.", "file ", "a:b.bin", "a|b.bin", "a?b.bin", "a\"b.bin",
        ] {
            let l = layout(&[(name, 100 * MIB), ("other.bin", 100 * MIB)], 256 * 1024);
            assert!(
                fired(&check_one(&l, false, &[], &[]), Lint::WindowsPath),
                "{name:?} should be caught"
            );
        }
    }

    #[test]
    fn ordinary_names_with_dots_and_spaces_inside_are_not_caught() {
        for name in [
            "my file.tar.gz",
            "disc 1/a.flac",
            "a.b.c",
            "console.log",
            "communicate.txt",
        ] {
            let l = layout(&[(name, 100 * MIB), ("other.bin", 100 * MIB)], 256 * 1024);
            assert!(
                !fired(&check_one(&l, false, &[], &[]), Lint::WindowsPath),
                "{name:?} should not be caught"
            );
        }
    }

    #[test]
    fn case_collisions_are_caught() {
        let l = layout(&[("README", 100 * MIB), ("readme", 100 * MIB)], 256 * 1024);
        assert!(fired(&check_one(&l, false, &[], &[]), Lint::CaseCollision));
    }

    #[test]
    fn a_long_path_component_is_caught() {
        let long = "x".repeat(300);
        let l = layout(
            &[(long.as_str(), 100 * MIB), ("other.bin", 100 * MIB)],
            256 * 1024,
        );
        assert!(fired(&check_one(&l, false, &[], &[]), Lint::LongPath));
    }

    #[test]
    fn bad_web_seed_and_tracker_urls_are_caught() {
        let l = layout(&[("a.bin", 100 * MIB)], 256 * 1024);
        let findings = check_one(
            &l,
            false,
            &["ftp://tracker.example.com".to_string()],
            &["not a url".to_string()],
        );
        assert!(fired(&findings, Lint::BadWebSeed));
        assert!(fired(&findings, Lint::BadTracker));
    }

    #[test]
    fn allowing_a_lint_silences_exactly_that_lint() {
        let l = layout(&[("CON", 100 * MIB), ("other.bin", 100 * MIB)], 100_000);
        let all = check(
            &Candidate {
                layout: &l,
                private: true,
                trackers: &[],
                web_seeds: &[],
            },
            &BTreeSet::new(),
        );
        assert!(all.len() >= 3);

        let allowed: BTreeSet<Lint> = [Lint::WindowsPath].into_iter().collect();
        let some = check(
            &Candidate {
                layout: &l,
                private: true,
                trackers: &[],
                web_seeds: &[],
            },
            &allowed,
        );
        assert!(!fired(&some, Lint::WindowsPath));
        assert!(
            fired(&some, Lint::PrivateNoTracker),
            "other lints still fire"
        );
    }

    #[test]
    fn lint_names_round_trip_and_are_kebab_case() {
        for lint in Lint::ALL {
            assert_eq!(Lint::parse(lint.name()).unwrap(), *lint);
            assert!(
                lint.name()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-'),
                "{} is not kebab-case",
                lint.name()
            );
            assert!(!lint.description().is_empty());
        }
        assert!(Lint::parse("no-such-lint").is_err());
    }

    #[test]
    fn lint_names_are_unique() {
        let names: BTreeSet<&str> = Lint::ALL.iter().map(|l| l.name()).collect();
        assert_eq!(names.len(), Lint::ALL.len());
    }

    #[test]
    fn a_refusal_names_every_lint_and_exits_thirteen() {
        let findings = vec![Finding {
            lint: Lint::EmptyFile,
            message: "`a` is empty".into(),
        }];
        let err = refuse(&findings);
        assert_eq!(err.code(), crate::exit::ExitCode::LintRefused);
        assert!(err.message().contains("empty-file"), "{}", err.message());
        assert!(err.message().contains("--allow"), "{}", err.message());
        assert_eq!(err.context()["lints"], serde_json::json!(["empty-file"]));
    }

    // ---------------------------------------------------------------------
    // T-176: two lints for what a client refuses, and one message that was
    // false. `TODO/create-seed.md` has the corpus references.
    // ---------------------------------------------------------------------

    /// The band between 65,536 pieces and this repository's own 100,000
    /// threshold used to pass every check and produce a torrent µTorrent
    /// cannot open.
    #[test]
    fn a_piece_count_above_65535_is_unopenable_and_says_so() {
        // 70,000 pieces at 16 KiB, which is the band the entry names.
        let layout = layout(&[("a.bin", 70_000 * 16 * KIB)], 16 * KIB as u32);
        let findings = check_one(&layout, false, &[], &[]);
        assert!(fired(&findings, Lint::PieceCountUnopenable), "{findings:?}");
        // And the other threshold has not fired, because 70,000 is under it.
        // That is the point of two lints: one is a client refusing the file
        // and the other is an opinion about hash data.
        assert!(!fired(&findings, Lint::PieceCount), "{findings:?}");
        let message = findings
            .iter()
            .find(|f| f.lint == Lint::PieceCountUnopenable)
            .map(|f| f.message.clone())
            .unwrap_or_default();
        assert!(message.contains("µTorrent"), "{message}");
    }

    /// Above 100,000 both fire, and clearing one leaves the other, which is
    /// what "they clear independently" means.
    #[test]
    fn the_two_piece_count_lints_clear_independently() {
        let layout = layout(&[("a.bin", 120_000 * 16 * KIB)], 16 * KIB as u32);
        let findings = check_one(&layout, false, &[], &[]);
        assert!(fired(&findings, Lint::PieceCountUnopenable), "{findings:?}");
        assert!(fired(&findings, Lint::PieceCount), "{findings:?}");

        let allowed = check(
            &Candidate {
                layout: &layout,
                private: false,
                trackers: &[],
                web_seeds: &[],
            },
            &BTreeSet::from([Lint::PieceCount]),
        );
        assert!(fired(&allowed, Lint::PieceCountUnopenable), "{allowed:?}");
        assert!(!fired(&allowed, Lint::PieceCount), "{allowed:?}");
    }

    /// `--piece-length 64MiB` was accepted in silence: `validate` refuses only
    /// zero and `piece_length::MAX` caps only the automatic choice.
    #[test]
    fn a_piece_length_above_16_mib_is_reported() {
        let too_large = layout(&[("a.bin", 4 * GIB)], 64 * MIB as u32);
        let findings = check_one(&too_large, false, &[], &[]);
        assert!(fired(&findings, Lint::PieceLengthTooLarge), "{findings:?}");
        // And exactly 16 MiB is not too large, because the ceiling is the
        // largest that works rather than the smallest that does not.
        let at_the_ceiling = layout(&[("a.bin", 4 * GIB)], 16 * MIB as u32);
        assert!(!fired(
            &check_one(&at_the_ceiling, false, &[], &[]),
            Lint::PieceLengthTooLarge
        ));
    }

    /// Two identical paths are a duplicate, and the message no longer sends
    /// the reader looking for a casing difference that is not there.
    #[test]
    fn two_identical_paths_are_a_duplicate_and_not_a_case_collision() {
        let layout = layout(&[("dir/a.bin", MIB), ("dir/a.bin", MIB)], 256 * 1024);
        let findings = check_one(&layout, false, &[], &[]);
        assert!(fired(&findings, Lint::DuplicatePath), "{findings:?}");
        assert!(!fired(&findings, Lint::CaseCollision), "{findings:?}");
        let message = findings
            .iter()
            .find(|f| f.lint == Lint::DuplicatePath)
            .map(|f| f.message.clone())
            .unwrap_or_default();
        assert!(!message.contains("case"), "{message}");
    }

    /// And the case that is a case collision still is one.
    #[test]
    fn two_paths_differing_only_in_case_still_collide() {
        let layout = layout(&[("dir/README", MIB), ("dir/readme", MIB)], 256 * 1024);
        let findings = check_one(&layout, false, &[], &[]);
        assert!(fired(&findings, Lint::CaseCollision), "{findings:?}");
        assert!(!fired(&findings, Lint::DuplicatePath), "{findings:?}");
    }
}
