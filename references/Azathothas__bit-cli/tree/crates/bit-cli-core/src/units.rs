//! Sizes, rates, durations, percentages, and ratios.
//!
//! Every number that reaches a user or a JSON document is formatted here, so
//! the rules hold everywhere at once:
//!
//! - Sizes use binary units. 1 KiB is 1024 B. `KB` and `MB` always mean 1000.
//! - Rates are a size per second: `KiB/s`, `MiB/s`.
//! - Durations are integer milliseconds in JSON, and a compact string for
//!   people.
//! - Percentages carry two decimal places, ratios three.
//!
//! JSON never carries only the formatted string. A size is emitted as a `u64`
//! of bytes with the human string beside it, a duration as integer
//! milliseconds with the human string beside it. [`Size`] and [`Millis`] exist
//! so that pairing cannot be forgotten.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const KIB: u64 = 1024;
pub const MIB: u64 = KIB * 1024;
pub const GIB: u64 = MIB * 1024;
pub const TIB: u64 = GIB * 1024;
pub const PIB: u64 = TIB * 1024;

/// A byte count that serializes as both an integer and a human string.
///
/// Serializing a bare `u64` is what leads to output where a caller can read
/// the number but not the unit, or read the unit but not the number. This type
/// makes both available without the caller deciding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "SizeWire", into = "SizeRepr")]
pub struct Size(pub u64);

/// Wire shape of a [`Size`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SizeRepr {
    bytes: u64,
    human: String,
}

/// What a [`Size`] accepts on the way back in.
///
/// A document `bit-cli` wrote carries the object form, and `bench --baseline`
/// reads its own reports back, so that form has to parse. A bare integer
/// parses too, because a threshold file written by hand should not have to
/// carry a rendered string that nothing reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum SizeWire {
    Bytes(u64),
    Object { bytes: u64 },
}

impl From<SizeWire> for Size {
    fn from(wire: SizeWire) -> Self {
        match wire {
            SizeWire::Bytes(bytes) | SizeWire::Object { bytes } => Self(bytes),
        }
    }
}

impl From<u64> for Size {
    fn from(bytes: u64) -> Self {
        Self(bytes)
    }
}

impl From<Size> for SizeRepr {
    fn from(size: Size) -> Self {
        Self {
            bytes: size.0,
            human: format_size(size.0),
        }
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_size(self.0))
    }
}

/// A byte rate that serializes as both an integer and a human string.
///
/// The same wire shape as [`Size`], so a report written before this type
/// existed still reads back and `--baseline` compares the same field. What
/// differs is the string beside the integer: a rate renders as `2.75 MiB/s`
/// where a size renders as `2.75 MiB`. Ground rule 0.2 says rates carry their
/// unit, and a field named `rate` whose human form reads like a size is a
/// number that says something it is not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "SizeWire", into = "RateRepr")]
pub struct Rate(pub u64);

/// Wire shape of a [`Rate`]. `bytes` rather than `bytes_per_second`, because
/// it has to stay the field an older report already carries.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RateRepr {
    bytes: u64,
    human: String,
}

impl From<SizeWire> for Rate {
    fn from(wire: SizeWire) -> Self {
        match wire {
            SizeWire::Bytes(bytes) | SizeWire::Object { bytes } => Self(bytes),
        }
    }
}

impl From<u64> for Rate {
    fn from(bytes: u64) -> Self {
        Self(bytes)
    }
}

impl From<Rate> for RateRepr {
    fn from(rate: Rate) -> Self {
        Self {
            bytes: rate.0,
            human: format_rate(rate.0),
        }
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_rate(self.0))
    }
}

/// A duration in whole milliseconds that serializes as both an integer and a
/// human string.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "MillisWire", into = "MillisRepr")]
pub struct Millis(pub u64);

/// Wire shape of a [`Millis`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MillisRepr {
    ms: u64,
    human: String,
}

/// What a [`Millis`] accepts on the way back in. See [`SizeWire`].
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum MillisWire {
    Ms(u64),
    Object { ms: u64 },
}

impl From<MillisWire> for Millis {
    fn from(wire: MillisWire) -> Self {
        match wire {
            MillisWire::Ms(ms) | MillisWire::Object { ms } => Self(ms),
        }
    }
}

impl From<u64> for Millis {
    fn from(ms: u64) -> Self {
        Self(ms)
    }
}

impl From<Duration> for Millis {
    fn from(d: Duration) -> Self {
        Self(d.as_millis().min(u128::from(u64::MAX)) as u64)
    }
}

impl From<Millis> for MillisRepr {
    fn from(ms: Millis) -> Self {
        Self {
            ms: ms.0,
            human: format_duration_ms(ms.0),
        }
    }
}

impl fmt::Display for Millis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_duration_ms(self.0))
    }
}

/// Format a byte count with binary units, for example `1.40 GiB`.
///
/// Values below 1 KiB are exact byte counts with no decimal point, because
/// rounding a 3 byte file to `0.00 KiB` loses the only information there was.
pub fn format_size(bytes: u64) -> String {
    match bytes {
        b if b < KIB => format!("{b} B"),
        b if b < MIB => format!("{:.2} KiB", b as f64 / KIB as f64),
        b if b < GIB => format!("{:.2} MiB", b as f64 / MIB as f64),
        b if b < TIB => format!("{:.2} GiB", b as f64 / GIB as f64),
        b if b < PIB => format!("{:.2} TiB", b as f64 / TIB as f64),
        b => format!("{:.2} PiB", b as f64 / PIB as f64),
    }
}

/// Format a bytes-per-second rate, for example `1.40 MiB/s`.
pub fn format_rate(bytes_per_sec: u64) -> String {
    format!("{}/s", format_size(bytes_per_sec))
}

/// Format a whole number of milliseconds for a person to read.
///
/// Sub-second values keep millisecond precision because that is the resolution
/// latency reporting works at. Above a second the string is compact:
/// `4m12s`, `1h02m`, `2d03h`.
pub fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let secs = ms / 1000;
    let (d, h, m, s) = (
        secs / 86_400,
        (secs % 86_400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );
    match (d, h, m) {
        (0, 0, 0) => format!("{s}s"),
        (0, 0, _) => format!("{m}m{s:02}s"),
        (0, _, _) => format!("{h}h{m:02}m"),
        _ => format!("{d}d{h:02}h"),
    }
}

/// Format a duration for a person to read.
pub fn format_duration(d: Duration) -> String {
    format_duration_ms(Millis::from(d).0)
}

/// Format a fraction as a percentage with two decimal places, for example
/// `42.10%`. The input is clamped to `0.0..=1.0`.
pub fn format_percent(fraction: f64) -> String {
    let clamped = if fraction.is_nan() {
        0.0
    } else {
        fraction.clamp(0.0, 1.0)
    };
    format!("{:.2}%", clamped * 100.0)
}

/// Format a measured share as a percentage, which may exceed a hundred.
///
/// [`format_percent`] clamps, and it should: it renders progress, and a
/// progress bar past a hundred percent is a bug. A share is a comparison
/// rather than a progress, and a run that beat the rate it was compared
/// against reached more than a hundred percent of it. Clamping that to
/// `100.00%` reports a number that is not true, which is worse than an
/// awkward-looking one. See `TODO/webseed.md`, T-001, where the HTTP path
/// reached 156.71% of the `curl` reference it was measured against.
pub fn format_share(fraction: f64) -> String {
    let value = match fraction.is_finite() {
        true => fraction.max(0.0),
        false => 0.0,
    };
    format!("{:.2}%", value * 100.0)
}

/// Percentage of `part` out of `whole`, with two decimal places. A `whole` of
/// zero reads as `100.00%`, since nothing left to do is complete.
pub fn percent_of(part: u64, whole: u64) -> String {
    match whole {
        0 => "100.00%".to_string(),
        w => format_percent(part as f64 / w as f64),
    }
}

/// Format a share ratio with three decimal places, for example `1.234`.
pub fn format_ratio(ratio: f64) -> String {
    let value = if ratio.is_finite() {
        ratio.max(0.0)
    } else {
        0.0
    };
    format!("{value:.3}")
}

/// Why a size string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseSizeError {
    #[error("empty size")]
    Empty,
    #[error("`{0}` is not a number")]
    NotANumber(String),
    #[error("`{0}` is not a size unit (use B, KiB, MiB, GiB, TiB, or the decimal KB, MB, GB, TB)")]
    UnknownUnit(String),
    #[error("size `{0}` is negative")]
    Negative(String),
    #[error("size `{0}` does not fit in 64 bits")]
    Overflow(String),
}

/// Parse a size with an optional unit suffix into bytes.
///
/// Accepted units, case-insensitive:
///
/// - `B`, or no suffix at all: bytes.
/// - `KiB`, `MiB`, `GiB`, `TiB`, `PiB`: powers of 1024.
/// - `K`, `M`, `G`, `T`, `P`: powers of 1024. This is what `aria2` means by a
///   bare suffix, so a script carried over from `aria2` keeps working.
/// - `KB`, `MB`, `GB`, `TB`, `PB`: powers of 1000. Spelled out, these always
///   mean the decimal value.
///
/// A fractional value is allowed (`1.5MiB`) and truncates toward zero.
/// Whitespace and underscores inside the number are ignored, so `1_048_576`
/// and `1 MiB` both parse.
pub fn parse_size(input: &str) -> Result<u64, ParseSizeError> {
    let trimmed: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect();
    if trimmed.is_empty() {
        return Err(ParseSizeError::Empty);
    }
    let split = trimmed
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split);
    if number.is_empty() {
        return Err(ParseSizeError::NotANumber(input.to_string()));
    }
    let value: f64 = number
        .parse()
        .map_err(|_| ParseSizeError::NotANumber(input.to_string()))?;
    if value < 0.0 {
        return Err(ParseSizeError::Negative(input.to_string()));
    }
    let multiplier =
        unit_multiplier(unit).ok_or_else(|| ParseSizeError::UnknownUnit(unit.to_string()))?;
    let bytes = value * multiplier as f64;
    if !bytes.is_finite() || bytes >= u64::MAX as f64 {
        return Err(ParseSizeError::Overflow(input.to_string()));
    }
    Ok(bytes as u64)
}

/// Bytes per unit, or `None` when the suffix is not a unit.
fn unit_multiplier(unit: &str) -> Option<u64> {
    let lower = unit.to_ascii_lowercase();
    Some(match lower.as_str() {
        "" | "b" => 1,
        "k" | "kib" => KIB,
        "m" | "mib" => MIB,
        "g" | "gib" => GIB,
        "t" | "tib" => TIB,
        "p" | "pib" => PIB,
        "kb" => 1_000,
        "mb" => 1_000_000,
        "gb" => 1_000_000_000,
        "tb" => 1_000_000_000_000,
        "pb" => 1_000_000_000_000_000,
        _ => return None,
    })
}

/// Parse a transfer rate into bytes per second.
///
/// Takes everything [`parse_size`] takes, plus an optional `/s` suffix, so
/// `5MiB/s` and `5MiB` are the same rate. Config files tend to write the
/// first, command lines the second.
pub fn parse_rate(input: &str) -> Result<u64, ParseSizeError> {
    let trimmed = input.trim();
    let without_suffix = trimmed
        .strip_suffix("/s")
        .or_else(|| trimmed.strip_suffix("/S"))
        .or_else(|| trimmed.strip_suffix("ps"))
        .unwrap_or(trimmed);
    parse_size(without_suffix)
}

/// Why a duration string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseDurationError {
    #[error("empty duration")]
    Empty,
    #[error("`{0}` is not a number")]
    NotANumber(String),
    #[error("`{0}` is not a duration unit (use ms, s, m, h, or d)")]
    UnknownUnit(String),
    #[error("duration `{0}` does not fit in 64 bits of milliseconds")]
    Overflow(String),
}

/// Parse a duration into milliseconds.
///
/// Accepts a bare number, which is seconds, or a number with a unit: `ms`,
/// `s`, `m`, `h`, `d`. Several terms may be concatenated, so `1h30m` and
/// `2d12h` both work. A bare number means seconds because that is what every
/// `aria2` timeout option means.
pub fn parse_duration_ms(input: &str) -> Result<u64, ParseDurationError> {
    let trimmed: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect();
    if trimmed.is_empty() {
        return Err(ParseDurationError::Empty);
    }
    let mut total: u64 = 0;
    let mut rest = trimmed.as_str();
    let mut terms = 0;
    while !rest.is_empty() {
        let split = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let (number, tail) = rest.split_at(split);
        if number.is_empty() {
            return Err(ParseDurationError::NotANumber(input.to_string()));
        }
        let value: f64 = number
            .parse()
            .map_err(|_| ParseDurationError::NotANumber(input.to_string()))?;
        let unit_len = tail
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(tail.len());
        let (unit, next) = tail.split_at(unit_len);
        let scale = duration_unit_ms(unit)
            .ok_or_else(|| ParseDurationError::UnknownUnit(unit.to_string()))?;
        let term = value * scale as f64;
        if !term.is_finite() || term >= u64::MAX as f64 {
            return Err(ParseDurationError::Overflow(input.to_string()));
        }
        total = total
            .checked_add(term as u64)
            .ok_or_else(|| ParseDurationError::Overflow(input.to_string()))?;
        rest = next;
        terms += 1;
    }
    if terms == 0 {
        return Err(ParseDurationError::Empty);
    }
    Ok(total)
}

/// Parse a duration into a [`Duration`].
pub fn parse_duration(input: &str) -> Result<Duration, ParseDurationError> {
    parse_duration_ms(input).map(Duration::from_millis)
}

/// Milliseconds per unit, or `None` when the suffix is not a unit.
fn duration_unit_ms(unit: &str) -> Option<u64> {
    Some(match unit.to_ascii_lowercase().as_str() {
        "ms" => 1,
        "" | "s" | "sec" | "secs" => 1_000,
        "m" | "min" | "mins" => 60_000,
        "h" | "hr" | "hrs" => 3_600_000,
        "d" | "day" | "days" => 86_400_000,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_binary_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(1536), "1.50 KiB");
        assert_eq!(format_size(MIB), "1.00 MiB");
        assert_eq!(format_size(GIB), "1.00 GiB");
        assert_eq!(format_size(TIB), "1.00 TiB");
        assert_eq!(format_size(PIB), "1.00 PiB");
    }

    #[test]
    fn rates_append_per_second() {
        assert_eq!(format_rate(MIB), "1.00 MiB/s");
        assert_eq!(format_rate(0), "0 B/s");
    }

    /// A rate carries its unit in the string beside the integer, and a size
    /// does not. Ground rule 0.2: a field named `rate` whose human form reads
    /// like a size is a number that says something it is not.
    #[test]
    fn a_rate_and_a_size_share_a_wire_shape_and_differ_in_the_string() {
        let rate = serde_json::to_value(Rate(2 * MIB)).unwrap();
        let size = serde_json::to_value(Size(2 * MIB)).unwrap();
        assert_eq!(rate["bytes"], 2 * MIB);
        assert_eq!(size["bytes"], 2 * MIB);
        assert_eq!(rate["human"], "2.00 MiB/s");
        assert_eq!(size["human"], "2.00 MiB");
    }

    /// A report written before `Rate` existed carried the same object, so it
    /// still reads back and `--baseline` still compares the same field. A bare
    /// integer parses too, for a threshold written by hand.
    #[test]
    fn a_rate_reads_back_from_an_older_report_and_from_a_bare_integer() {
        let object: Rate = serde_json::from_str(r#"{"bytes":1048576,"human":"1.00 MiB"}"#).unwrap();
        assert_eq!(object, Rate(MIB));
        let bare: Rate = serde_json::from_str("1048576").unwrap();
        assert_eq!(bare, Rate(MIB));
        assert_eq!(Rate(MIB).to_string(), "1.00 MiB/s");
    }

    #[test]
    fn binary_suffixes_are_powers_of_1024() {
        assert_eq!(parse_size("1KiB"), Ok(1024));
        assert_eq!(parse_size("1K"), Ok(1024));
        assert_eq!(parse_size("1MiB"), Ok(MIB));
        assert_eq!(parse_size("1M"), Ok(MIB));
        assert_eq!(parse_size("2GiB"), Ok(2 * GIB));
        assert_eq!(parse_size("1TiB"), Ok(TIB));
    }

    #[test]
    fn spelled_out_decimal_suffixes_are_powers_of_1000() {
        assert_eq!(parse_size("1KB"), Ok(1_000));
        assert_eq!(parse_size("1MB"), Ok(1_000_000));
        assert_eq!(parse_size("1GB"), Ok(1_000_000_000));
    }

    #[test]
    fn bare_numbers_are_bytes() {
        assert_eq!(parse_size("512"), Ok(512));
        assert_eq!(parse_size("1_048_576"), Ok(MIB));
        assert_eq!(parse_size("1 MiB"), Ok(MIB));
    }

    #[test]
    fn fractional_sizes_truncate() {
        assert_eq!(parse_size("1.5MiB"), Ok(MIB + MIB / 2));
        assert_eq!(parse_size("0.5KiB"), Ok(512));
    }

    #[test]
    fn bad_sizes_say_what_is_wrong() {
        assert_eq!(parse_size(""), Err(ParseSizeError::Empty));
        assert!(matches!(
            parse_size("MiB"),
            Err(ParseSizeError::NotANumber(_))
        ));
        assert!(matches!(
            parse_size("1QiB"),
            Err(ParseSizeError::UnknownUnit(_))
        ));
        assert!(matches!(parse_size("-1"), Err(ParseSizeError::Negative(_))));
    }

    #[test]
    fn rates_accept_a_per_second_suffix() {
        assert_eq!(parse_rate("5MiB/s"), Ok(5 * MIB));
        assert_eq!(parse_rate("5MiB"), Ok(5 * MIB));
        assert_eq!(parse_rate("500KiB/s"), Ok(500 * KIB));
    }

    #[test]
    fn bare_durations_are_seconds() {
        assert_eq!(parse_duration_ms("30"), Ok(30_000));
        assert_eq!(parse_duration_ms("30s"), Ok(30_000));
        assert_eq!(parse_duration_ms("500ms"), Ok(500));
    }

    #[test]
    fn duration_terms_concatenate() {
        assert_eq!(parse_duration_ms("1h30m"), Ok(5_400_000));
        assert_eq!(parse_duration_ms("2d12h"), Ok(216_000_000));
        assert_eq!(parse_duration_ms("1m30s"), Ok(90_000));
    }

    #[test]
    fn bad_durations_say_what_is_wrong() {
        assert_eq!(parse_duration_ms(""), Err(ParseDurationError::Empty));
        assert!(matches!(
            parse_duration_ms("5years"),
            Err(ParseDurationError::UnknownUnit(_))
        ));
    }

    #[test]
    fn durations_format_compactly() {
        assert_eq!(format_duration_ms(0), "0ms");
        assert_eq!(format_duration_ms(999), "999ms");
        assert_eq!(format_duration_ms(1_000), "1s");
        assert_eq!(format_duration_ms(252_000), "4m12s");
        assert_eq!(format_duration_ms(3_720_000), "1h02m");
        assert_eq!(format_duration_ms(183_600_000), "2d03h");
    }

    #[test]
    fn percentages_carry_two_decimals_and_ratios_three() {
        assert_eq!(format_percent(0.421), "42.10%");
        assert_eq!(format_percent(1.0), "100.00%");
        assert_eq!(format_percent(2.0), "100.00%");
        assert_eq!(format_percent(-1.0), "0.00%");
        assert_eq!(percent_of(1, 3), "33.33%");
        assert_eq!(percent_of(0, 0), "100.00%");
        assert_eq!(format_ratio(1.2345), "1.234");
        assert_eq!(format_ratio(f64::NAN), "0.000");
    }

    /// A share is a comparison, not a progress. Clamping one at a hundred
    /// reports a number that is not true, which is how `bench --ceiling` came
    /// to read `100.00%` for a run that reached 156.71% of its reference.
    #[test]
    fn a_share_above_one_is_reported_rather_than_clamped() {
        assert_eq!(format_share(0.421), "42.10%");
        assert_eq!(format_share(1.0), "100.00%");
        assert_eq!(format_share(1.5671), "156.71%");
        assert_eq!(format_share(3.8118), "381.18%");
        assert_eq!(format_share(-1.0), "0.00%");
        assert_eq!(format_share(f64::NAN), "0.00%");
        assert_eq!(format_share(f64::INFINITY), "0.00%");
        assert_eq!(
            format_percent(2.0),
            "100.00%",
            "the clamping one still clamps: it renders progress"
        );
    }

    #[test]
    fn size_serializes_with_both_forms() {
        let json = serde_json::to_value(Size(MIB)).unwrap();
        assert_eq!(json["bytes"], 1_048_576u64);
        assert_eq!(json["human"], "1.00 MiB");
    }

    #[test]
    fn millis_serializes_with_both_forms() {
        let json = serde_json::to_value(Millis(90_000)).unwrap();
        assert_eq!(json["ms"], 90_000u64);
        assert_eq!(json["human"], "1m30s");
    }

    #[test]
    fn sizes_round_trip_through_parsing() {
        for bytes in [0u64, 1, 1023, KIB, MIB, GIB, 3 * GIB + 7] {
            let parsed = parse_size(&bytes.to_string()).unwrap();
            assert_eq!(parsed, bytes);
        }
    }

    #[test]
    fn a_size_reads_back_from_what_it_wrote() {
        for bytes in [0u64, 1, KIB, 3 * GIB + 7] {
            let json = serde_json::to_string(&Size(bytes)).unwrap();
            assert_eq!(serde_json::from_str::<Size>(&json).unwrap(), Size(bytes));
        }
    }

    #[test]
    fn a_size_also_reads_a_bare_integer() {
        assert_eq!(serde_json::from_str::<Size>("4096").unwrap(), Size(4096));
    }

    #[test]
    fn a_duration_reads_back_from_what_it_wrote() {
        for ms in [0u64, 1, 999, 90_000] {
            let json = serde_json::to_string(&Millis(ms)).unwrap();
            assert_eq!(serde_json::from_str::<Millis>(&json).unwrap(), Millis(ms));
        }
        assert_eq!(serde_json::from_str::<Millis>("250").unwrap(), Millis(250));
    }
}
