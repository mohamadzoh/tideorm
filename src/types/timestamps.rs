use serde::{Deserialize, Serialize};
use std::fmt;

use super::{DateTime, Utc};

/// Unix timestamp as whole seconds since the epoch.
///
/// A value type for converting between `i64` epoch seconds and
/// `chrono::DateTime`. It has no database column mapping, so a model stores
/// the `i64` (see [`UnixTimestamp::as_seconds`]) and converts at the edges.
/// Serializes as the bare integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTimestamp(i64);

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
}

/// Unix timestamp as milliseconds since the epoch.
///
/// The sub-second counterpart of [`UnixTimestamp`], with the same caveat: it
/// has no database column mapping of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTimestampMillis(i64);

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
}

/// `From` cannot report failure, so seconds that do not fit in milliseconds saturate at
/// `i64::MIN`/`i64::MAX` instead of overflowing. Such values are ~292 million years from
/// the epoch and are not representable as a `chrono::DateTime` either.
impl From<UnixTimestamp> for UnixTimestampMillis {
    fn from(ts: UnixTimestamp) -> Self {
        Self(ts.0.saturating_mul(1000))
    }
}

/// What both timestamp types share: `now()` as the default, the raw count as
/// an `i64`, a `DateTime` in, and the date as their display.
macro_rules! unix_time_impls {
    ($name:ident, $display:literal) => {
        impl Default for $name {
            fn default() -> Self {
                Self::now()
            }
        }

        impl From<$name> for i64 {
            fn from(ts: $name) -> Self {
                ts.0
            }
        }

        impl From<DateTime<Utc>> for $name {
            fn from(dt: DateTime<Utc>) -> Self {
                Self::from_datetime(dt)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.to_datetime() {
                    Some(dt) => write!(f, "{}", dt.format($display)),
                    None => write!(f, "{}", self.0),
                }
            }
        }
    };
}

unix_time_impls!(UnixTimestamp, "%Y-%m-%d %H:%M:%S UTC");
unix_time_impls!(UnixTimestampMillis, "%Y-%m-%d %H:%M:%S%.3f UTC");
