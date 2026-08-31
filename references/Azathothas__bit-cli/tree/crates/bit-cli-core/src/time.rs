//! Timestamps.
//!
//! Every timestamp `bit-cli` emits is ISO 8601 in UTC with millisecond
//! precision and a `Z` suffix: `2026-08-19T11:52:03.418Z`. Never local time,
//! never second-only precision, never a bare epoch in output a person reads.
//!
//! [`Timestamp`] is the type to reach for. It serializes as the string above
//! and carries the epoch milliseconds alongside it for callers doing
//! arithmetic.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

/// A point in time, rendered as ISO 8601 UTC with millisecond precision.
///
/// The default is the epoch, which renders as `1970-01-01T00:00:00.000Z`. That
/// is deliberately conspicuous: a timestamp nobody set should look wrong
/// rather than look like now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp {
    epoch_ms: i64,
}

/// Wire shape of a [`Timestamp`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TimestampRepr {
    iso: String,
    epoch_ms: i64,
}

impl Timestamp {
    /// The current time.
    pub fn now() -> Self {
        Self::from_datetime(Utc::now())
    }

    /// A timestamp from epoch milliseconds.
    pub const fn from_epoch_ms(epoch_ms: i64) -> Self {
        Self { epoch_ms }
    }

    /// A timestamp from epoch seconds, as a `.torrent` `creation date` carries.
    pub const fn from_epoch_secs(epoch_secs: i64) -> Self {
        Self {
            epoch_ms: epoch_secs * 1000,
        }
    }

    /// A timestamp from a [`SystemTime`], clamped at the epoch for times before
    /// it, since no field `bit-cli` reports can legitimately predate 1970.
    pub fn from_system_time(time: SystemTime) -> Self {
        let epoch_ms = time
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or(0);
        Self { epoch_ms }
    }

    fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self {
            epoch_ms: dt.timestamp_millis(),
        }
    }

    /// Milliseconds since the Unix epoch.
    pub const fn epoch_ms(self) -> i64 {
        self.epoch_ms
    }

    /// Whole seconds since the Unix epoch, as a `.torrent` `creation date`
    /// wants them.
    pub const fn epoch_secs(self) -> i64 {
        self.epoch_ms.div_euclid(1000)
    }

    /// The ISO 8601 UTC rendering, for example `2026-08-19T11:52:03.418Z`.
    pub fn iso(self) -> String {
        DateTime::<Utc>::from_timestamp_millis(self.epoch_ms)
            .unwrap_or_else(|| DateTime::<Utc>::from_timestamp_nanos(0))
            .to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.iso())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TimestampRepr {
            iso: self.iso(),
            epoch_ms: self.epoch_ms,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let repr = TimestampRepr::deserialize(deserializer)?;
        Ok(Self {
            epoch_ms: repr.epoch_ms,
        })
    }
}

/// Format the current time as an ISO 8601 UTC millisecond string.
pub fn now_iso() -> String {
    Timestamp::now().iso()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_renders_as_the_documented_shape() {
        assert_eq!(
            Timestamp::from_epoch_ms(0).iso(),
            "1970-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn milliseconds_are_never_dropped() {
        let t = Timestamp::from_epoch_ms(1_787_140_323_418);
        let iso = t.iso();
        assert!(iso.ends_with(".418Z"), "got {iso}");
        assert_eq!(iso.len(), 24, "fixed width keeps logs aligned: {iso}");
    }

    #[test]
    fn seconds_and_milliseconds_agree() {
        let t = Timestamp::from_epoch_secs(1_787_140_323);
        assert_eq!(t.epoch_ms(), 1_787_140_323_000);
        assert_eq!(t.epoch_secs(), 1_787_140_323);
    }

    #[test]
    fn negative_milliseconds_still_floor_to_the_right_second() {
        assert_eq!(Timestamp::from_epoch_ms(-1).epoch_secs(), -1);
        assert_eq!(Timestamp::from_epoch_ms(-1000).epoch_secs(), -1);
        assert_eq!(Timestamp::from_epoch_ms(-1001).epoch_secs(), -2);
    }

    #[test]
    fn serializes_with_both_forms() {
        let json = serde_json::to_value(Timestamp::from_epoch_ms(1_787_140_323_418)).unwrap();
        assert_eq!(json["iso"], "2026-08-19T11:52:03.418Z");
        assert_eq!(json["epoch_ms"], 1_787_140_323_418i64);
    }

    #[test]
    fn now_is_after_the_epoch_and_before_the_far_future() {
        let now = Timestamp::now().epoch_ms();
        assert!(now > 1_700_000_000_000, "clock looks wrong: {now}");
    }
}
