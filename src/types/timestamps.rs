use serde::{Deserialize, Serialize};
use std::fmt;

use super::{DateTime, Utc};

/// Unix timestamp stored as seconds since epoch (i64)
///
/// This provides a portable integer-based timestamp format that works across
/// all databases. Convert to/from `chrono::DateTime` as needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTimestamp(pub i64);

impl UnixTimestamp {
    /// Create a new Unix timestamp from seconds since epoch
    pub fn new(seconds: i64) -> Self {
        Self(seconds)
    }

    /// Get the current time as a Unix timestamp
    pub fn now() -> Self {
        Self(chrono::Utc::now().timestamp())
    }

    /// Create from a chrono DateTime
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp())
    }

    /// Convert to a chrono DateTime
    pub fn to_datetime(self) -> Option<DateTime<Utc>> {
        chrono::DateTime::from_timestamp(self.0, 0)
    }

    /// Get the raw seconds value
    pub fn as_seconds(&self) -> i64 {
        self.0
    }

    /// Check if this timestamp is in the past
    pub fn is_past(&self) -> bool {
        self.0 < chrono::Utc::now().timestamp()
    }

    /// Check if this timestamp is in the future
    pub fn is_future(&self) -> bool {
        self.0 > chrono::Utc::now().timestamp()
    }
}

impl Default for UnixTimestamp {
    fn default() -> Self {
        Self::now()
    }
}

impl From<i64> for UnixTimestamp {
    fn from(seconds: i64) -> Self {
        Self(seconds)
    }
}

impl From<UnixTimestamp> for i64 {
    fn from(ts: UnixTimestamp) -> Self {
        ts.0
    }
}

impl From<DateTime<Utc>> for UnixTimestamp {
    fn from(dt: DateTime<Utc>) -> Self {
        Self::from_datetime(dt)
    }
}

impl fmt::Display for UnixTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dt) = self.to_datetime() {
            write!(f, "{}", dt.format("%Y-%m-%d %H:%M:%S UTC"))
        } else {
            write!(f, "{}", self.0)
        }
    }
}

/// Unix timestamp stored as milliseconds since epoch (i64)
///
/// Higher precision version of `UnixTimestamp` for sub-second accuracy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTimestampMillis(pub i64);

impl UnixTimestampMillis {
    /// Create a new Unix timestamp from milliseconds since epoch
    pub fn new(millis: i64) -> Self {
        Self(millis)
    }

    /// Get the current time as a Unix timestamp in milliseconds
    pub fn now() -> Self {
        Self(chrono::Utc::now().timestamp_millis())
    }

    /// Create from a chrono DateTime
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp_millis())
    }

    /// Convert to a chrono DateTime
    pub fn to_datetime(self) -> Option<DateTime<Utc>> {
        chrono::DateTime::from_timestamp_millis(self.0)
    }

    /// Get the raw milliseconds value
    pub fn as_millis(&self) -> i64 {
        self.0
    }

    /// Get as seconds (losing millisecond precision)
    ///
    /// Rounds toward negative infinity so that pre-epoch values stay consistent with
    /// [`Self::to_datetime`]: `-1500` milliseconds is `-2` seconds, not `-1`.
    pub fn as_seconds(&self) -> i64 {
        self.0.div_euclid(1000)
    }

    /// Convert to UnixTimestamp (seconds)
    ///
    /// Uses the same floor semantics as [`Self::as_seconds`].
    pub fn to_unix_timestamp(self) -> UnixTimestamp {
        UnixTimestamp(self.as_seconds())
    }

    /// Check if this timestamp is in the past
    pub fn is_past(&self) -> bool {
        self.0 < chrono::Utc::now().timestamp_millis()
    }

    /// Check if this timestamp is in the future
    pub fn is_future(&self) -> bool {
        self.0 > chrono::Utc::now().timestamp_millis()
    }
}

impl Default for UnixTimestampMillis {
    fn default() -> Self {
        Self::now()
    }
}

impl From<i64> for UnixTimestampMillis {
    fn from(millis: i64) -> Self {
        Self(millis)
    }
}

impl From<UnixTimestampMillis> for i64 {
    fn from(ts: UnixTimestampMillis) -> Self {
        ts.0
    }
}

impl From<DateTime<Utc>> for UnixTimestampMillis {
    fn from(dt: DateTime<Utc>) -> Self {
        Self::from_datetime(dt)
    }
}

/// `From` cannot report failure, so seconds that do not fit in milliseconds saturate at
/// `i64::MIN`/`i64::MAX` instead of overflowing. Such values are ~292 million years from
/// the epoch and are not representable as a `chrono::DateTime` either.
impl From<UnixTimestamp> for UnixTimestampMillis {
    fn from(ts: UnixTimestamp) -> Self {
        Self(ts.0.saturating_mul(1000))
    }
}

impl fmt::Display for UnixTimestampMillis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dt) = self.to_datetime() {
            write!(f, "{}", dt.format("%Y-%m-%d %H:%M:%S%.3f UTC"))
        } else {
            write!(f, "{}", self.0)
        }
    }
}
