//! PostgreSQL values in their binary wire form, for the types no typed decoder
//! reads, arrays of dates and times among them, and for the arrays a typed
//! decoder refuses: one holding a `NULL`, or of more than one dimension.
//!
//! Dates, times and timestamps come back as the typed decoders give them;
//! `infinity` and `-infinity` as those words. An `INTERVAL` is its ISO 8601
//! duration, as PostgreSQL writes it under `intervalstyle = iso_8601`. A
//! `MONEY` is its amount as an exact decimal, with the two fraction digits
//! PostgreSQL itself assumes when the locale names none. `TIMETZ`, `INET`,
//! `CIDR` and the built-in ranges are the text PostgreSQL prints for them, a
//! `TSTZRANGE` in UTC.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta, TimeZone, Utc};
use serde_json::Value;
use std::fmt::Write;

use super::Database;

/// The bytes of `row`'s column `index` in binary form; `None` for a NULL, a
/// value in text form, and a column it cannot read.
pub(crate) fn binary_bytes(
    row: &crate::internal::sqlx::postgres::PgRow,
    index: usize,
) -> Option<&[u8]> {
    use crate::internal::sqlx::postgres::PgValueFormat;
    use crate::internal::sqlx::{Row, ValueRef};

    let raw = row.try_get_raw(index).ok()?;
    if raw.is_null() || raw.format() != PgValueFormat::Binary {
        return None;
    }
    raw.as_bytes().ok()
}

/// Whether the driver names `type_name` a date, a time or a timestamp, the
/// types holding a value its decoders cannot read (see [`unreadable_moment`]).
pub(crate) fn is_moment_type(type_name: &str) -> bool {
    matches!(type_name, "DATE" | "TIME" | "TIMESTAMP" | "TIMESTAMPTZ")
}

/// A value in binary form as JSON: read as `type_name`, or as the text its
/// bytes spell when this reads no such type.
pub(super) fn binary_or_text(type_name: &str, bytes: &[u8]) -> Option<Value> {
    binary_json(type_name, bytes)
        .or_else(|| std::str::from_utf8(bytes).ok().and_then(printable_text))
}

/// A value of the type the driver names `type_name` (upper-cased), in binary
/// form, as JSON. `None` for a type this does not read and for bytes that are
/// not a value of it.
pub(super) fn binary_json(type_name: &str, bytes: &[u8]) -> Option<Value> {
    if let Some(element) = type_name.strip_suffix("[]") {
        return array_json(bytes, &|bytes| binary_or_text(element, bytes));
    }
    let text = |text: Option<String>| text.map(Value::String);
    match type_name {
        "BOOL" => Some(Value::Bool(fixed::<1>(bytes)? != [0])),
        "INT2" => Some(i16::from_be_bytes(fixed(bytes)?).into()),
        "INT4" => Some(i32::from_be_bytes(fixed(bytes)?).into()),
        "INT8" => Some(i64::from_be_bytes(fixed(bytes)?).into()),
        "FLOAT4" => Some(Database::f64_to_json(
            f32::from_be_bytes(fixed(bytes)?).into(),
        )),
        "FLOAT8" => Some(Database::f64_to_json(f64::from_be_bytes(fixed(bytes)?))),
        "NUMERIC" => text(numeric_text(bytes)),
        "UUID" => Some(Value::String(
            uuid::Uuid::from_bytes(fixed(bytes)?).to_string(),
        )),
        "TEXT" | "VARCHAR" | "CHAR" | "NAME" | "CITEXT" => {
            text(std::str::from_utf8(bytes).ok().map(str::to_owned))
        }
        "JSON" => serde_json::from_slice(bytes).ok(),
        // A version byte, then the document's text.
        "JSONB" => serde_json::from_slice(bytes.strip_prefix(&[1])?).ok(),
        "BYTEA" => serde_json::to_value(bytes).ok(),
        "DATE" => moment_json(date(bytes)?),
        "TIME" => time_json(i64::from_be_bytes(fixed(bytes)?)),
        "TIMESTAMP" => moment_json(timestamp(bytes)?),
        "TIMESTAMPTZ" => moment_json(match timestamp(bytes)? {
            Moment::At(at) => Moment::At(Utc.fix().from_utc_datetime(&at)),
            Moment::Infinite(infinity) => Moment::Infinite(infinity),
        }),
        "TIMETZ" => text(timetz_text(bytes)),
        "INTERVAL" => text(interval_text(bytes)),
        "MONEY" => serde_json::to_value(rust_decimal::Decimal::new(
            i64::from_be_bytes(fixed(bytes)?),
            2,
        ))
        .ok(),
        "INET" | "CIDR" => text(inet_text(bytes)),
        "INT4RANGE" => text(range_text(bytes, |bytes| {
            Some(i32::from_be_bytes(fixed(bytes)?).to_string())
        })),
        "INT8RANGE" => text(range_text(bytes, |bytes| {
            Some(i64::from_be_bytes(fixed(bytes)?).to_string())
        })),
        "NUMRANGE" => text(range_text(bytes, numeric_text)),
        "DATERANGE" => text(range_text(bytes, |bytes| {
            Some(date(bytes)?.text(date_text))
        })),
        "TSRANGE" => text(range_text(bytes, |bytes| {
            Some(timestamp(bytes)?.text(|at| timestamp_text(at, "")))
        })),
        "TSTZRANGE" => text(range_text(bytes, |bytes| {
            Some(timestamp(bytes)?.text(|at| timestamp_text(at, "+00")))
        })),
        _ => None,
    }
}

/// The text of a value no decoder reads, such as an enum label, when it is
/// printable: a binary form is not text.
pub(super) fn printable_text(text: &str) -> Option<Value> {
    text.chars()
        .all(|character| !character.is_control() || character.is_whitespace())
        .then(|| Value::String(text.to_string()))
}

/// `bytes` when they are exactly `N` long.
fn fixed<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.try_into().ok()
}

/// The next big-endian `i32` of `bytes`, which it moves past.
fn take_i32(bytes: &mut &[u8]) -> Option<i32> {
    let (head, tail) = bytes.split_first_chunk::<4>()?;
    *bytes = tail;
    Some(i32::from_be_bytes(*head))
}

/// The next length-prefixed value of `bytes`, which it moves past; `None`
/// inside for a `NULL`.
fn take_value<'a>(bytes: &mut &'a [u8]) -> Option<Option<&'a [u8]>> {
    let length = take_i32(bytes)?;
    if length == -1 {
        return Some(None);
    }
    let (value, tail) = bytes.split_at_checked(usize::try_from(length).ok()?)?;
    *bytes = tail;
    Some(Some(value))
}

/// An array: its dimension count, a has-`NULL` flag, the element type, each
/// dimension's length and lower bound, then the elements in row-major order.
/// A dimension past the first nests one level deeper.
fn array_json(bytes: &[u8], element: &dyn Fn(&[u8]) -> Option<Value>) -> Option<Value> {
    let mut rest = bytes;
    let dimensions = take_i32(&mut rest)?;
    take_i32(&mut rest)?;
    take_i32(&mut rest)?;
    let mut lengths = Vec::new();
    for _ in 0..dimensions {
        lengths.push(usize::try_from(take_i32(&mut rest)?).ok()?);
        take_i32(&mut rest)?;
    }
    if lengths.is_empty() {
        return rest.is_empty().then(|| Value::Array(Vec::new()));
    }
    let mut elements = Vec::new();
    let count = lengths
        .iter()
        .try_fold(1_usize, |count, length| count.checked_mul(*length))?;
    for _ in 0..count {
        elements.push(match take_value(&mut rest)? {
            Some(value) => element(value)?,
            None => Value::Null,
        });
    }
    if !rest.is_empty() {
        return None;
    }
    nest(&lengths, &mut elements.into_iter())
}

fn nest(lengths: &[usize], elements: &mut std::vec::IntoIter<Value>) -> Option<Value> {
    let Some((&length, inner)) = lengths.split_first() else {
        return elements.next();
    };
    (0..length)
        .map(|_| nest(inner, elements))
        .collect::<Option<Vec<_>>>()
        .map(Value::Array)
}

/// A date or a timestamp, or one of the infinities PostgreSQL keeps as the
/// extreme values of its form.
enum Moment<T> {
    At(T),
    Infinite(&'static str),
}

impl<T> Moment<T> {
    fn text(self, finite: impl FnOnce(T) -> String) -> String {
        match self {
            Moment::At(at) => finite(at),
            Moment::Infinite(infinity) => infinity.to_string(),
        }
    }
}

fn moment_json<T: serde::Serialize>(moment: Moment<T>) -> Option<Value> {
    match moment {
        Moment::At(at) => serde_json::to_value(at).ok(),
        Moment::Infinite(infinity) => Some(Value::String(infinity.to_string())),
    }
}

/// PostgreSQL's epoch, the day its dates and timestamps count from.
fn epoch() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2000, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .expect("2000-01-01 is a date")
}

/// A `DATE`: days since the epoch.
fn date(bytes: &[u8]) -> Option<Moment<NaiveDate>> {
    match i32::from_be_bytes(fixed(bytes)?) {
        i32::MAX => Some(Moment::Infinite("infinity")),
        i32::MIN => Some(Moment::Infinite("-infinity")),
        days => epoch()
            .date()
            .checked_add_signed(TimeDelta::try_days(days.into())?)
            .map(Moment::At),
    }
}

/// A `TIMESTAMP` or `TIMESTAMPTZ`: microseconds since the epoch, in UTC.
fn timestamp(bytes: &[u8]) -> Option<Moment<NaiveDateTime>> {
    match i64::from_be_bytes(fixed(bytes)?) {
        i64::MAX => Some(Moment::Infinite("infinity")),
        i64::MIN => Some(Moment::Infinite("-infinity")),
        micros => epoch()
            .checked_add_signed(TimeDelta::microseconds(micros))
            .map(Moment::At),
    }
}

/// A `TIME` as the typed decoder gives one; `24:00:00`, which PostgreSQL
/// allows and a `NaiveTime` cannot hold, as that text.
fn time_json(micros: i64) -> Option<Value> {
    if micros == 86_400_000_000 {
        return Some(Value::String("24:00:00".to_string()));
    }
    let seconds = u32::try_from(micros.div_euclid(1_000_000)).ok()?;
    let nanos = u32::try_from(micros.rem_euclid(1_000_000) * 1_000).ok()?;
    serde_json::to_value(NaiveTime::from_num_seconds_from_midnight_opt(
        seconds, nanos,
    )?)
    .ok()
}

/// Microseconds since midnight as `HH:MM:SS`, then any fraction of a second
/// without its trailing zeros.
fn time_text(micros: i64) -> String {
    let mut text = format!(
        "{:02}:{:02}:{:02}",
        micros / 3_600_000_000,
        micros / 60_000_000 % 60,
        micros / 1_000_000 % 60
    );
    push_fraction(&mut text, (micros % 1_000_000).unsigned_abs());
    text
}

fn push_fraction(text: &mut String, micros: u64) {
    if micros != 0 {
        let digits = format!("{micros:06}");
        text.push('.');
        text.push_str(digits.trim_end_matches('0'));
    }
}

/// A date as PostgreSQL prints one, with a year of four digits or more, and
/// the ` BC` it ends a date before year 1 with.
fn date_parts(date: NaiveDate) -> (String, &'static str) {
    use chrono::Datelike;
    let (year, era) = match date.year() {
        year if year > 0 => (year, ""),
        year => (1 - year, " BC"),
    };
    let text = format!("{year:04}-{:02}-{:02}", date.month(), date.day());
    (text, era)
}

fn date_text(date: NaiveDate) -> String {
    let (date, era) = date_parts(date);
    date + era
}

/// A timestamp as PostgreSQL prints one, with `zone` after the time.
fn timestamp_text(at: NaiveDateTime, zone: &str) -> String {
    use chrono::Timelike;
    let (date, era) = date_parts(at.date());
    let time = at.time();
    let micros = i64::from(time.num_seconds_from_midnight()) * 1_000_000
        + i64::from(time.nanosecond() / 1_000);
    format!("{date} {}{zone}{era}", time_text(micros))
}

/// A `TIMETZ`: microseconds since midnight, then the zone in seconds west of
/// UTC, printed as `+HH`, `+HH:MM` or `+HH:MM:SS` east of it.
fn timetz_text(bytes: &[u8]) -> Option<String> {
    let (time, zone) = bytes.split_first_chunk::<8>()?;
    let zone = i32::from_be_bytes(fixed(zone)?);
    let mut text = time_text(i64::from_be_bytes(*time));
    let seconds = zone.unsigned_abs();
    text.push(if zone <= 0 { '+' } else { '-' });
    let _ = write!(text, "{:02}", seconds / 3600);
    if seconds % 60 != 0 {
        let _ = write!(text, ":{:02}:{:02}", seconds / 60 % 60, seconds % 60);
    } else if seconds / 60 % 60 != 0 {
        let _ = write!(text, ":{:02}", seconds / 60 % 60);
    }
    Some(text)
}

/// An `INTERVAL`: microseconds, days, then months, each counted apart, as
/// PostgreSQL writes it under `intervalstyle = iso_8601`.
fn interval_text(bytes: &[u8]) -> Option<String> {
    let (micros, mut rest) = bytes.split_first_chunk::<8>()?;
    let micros = i64::from_be_bytes(*micros);
    let days = take_i32(&mut rest)?;
    let months = take_i32(&mut rest)?;
    if !rest.is_empty() {
        return None;
    }
    // PostgreSQL 17's infinite intervals.
    match (micros, days, months) {
        (i64::MAX, i32::MAX, i32::MAX) => return Some("infinity".to_string()),
        (i64::MIN, i32::MIN, i32::MIN) => return Some("-infinity".to_string()),
        (0, 0, 0) => return Some("PT0S".to_string()),
        _ => {}
    }
    // Each part keeps the sign of what it is taken from, as PostgreSQL's
    // truncating division leaves it.
    let mut text = String::from("P");
    for (value, unit) in [
        (i64::from(months / 12), 'Y'),
        (i64::from(months % 12), 'M'),
        (i64::from(days), 'D'),
    ] {
        if value != 0 {
            let _ = write!(text, "{value}{unit}");
        }
    }
    if micros != 0 {
        text.push('T');
        for (value, unit) in [
            (micros / 3_600_000_000, 'H'),
            (micros / 60_000_000 % 60, 'M'),
        ] {
            if value != 0 {
                let _ = write!(text, "{value}{unit}");
            }
        }
        let (seconds, fraction) = (micros / 1_000_000 % 60, micros % 1_000_000);
        if seconds != 0 || fraction != 0 {
            if seconds < 0 || fraction < 0 {
                text.push('-');
            }
            let _ = write!(text, "{}", seconds.unsigned_abs());
            push_fraction(&mut text, fraction.unsigned_abs());
            text.push('S');
        }
    }
    Some(text)
}

/// An `INET` or `CIDR`: its family, mask length, whether it is a `CIDR`, the
/// address length, then the address. An `INET` names its mask only when it
/// does not cover the whole address.
fn inet_text(bytes: &[u8]) -> Option<String> {
    let [_family, bits, is_cidr, length, address @ ..] = bytes else {
        return None;
    };
    if usize::from(*length) != address.len() {
        return None;
    }
    let (address, full) = match address.len() {
        4 => (
            std::net::Ipv4Addr::from(fixed::<4>(address)?).to_string(),
            32,
        ),
        16 => (
            ipv6_text(std::net::Ipv6Addr::from(fixed::<16>(address)?)),
            128,
        ),
        _ => return None,
    };
    Some(if *is_cidr != 0 || *bits != full {
        format!("{address}/{bits}")
    } else {
        address
    })
}

/// An IPv6 address as PostgreSQL prints one, which is how Rust does, except
/// that it writes an address whose first six groups alone are zero with its
/// last 32 bits as an IPv4 address.
fn ipv6_text(address: std::net::Ipv6Addr) -> String {
    let segments = address.segments();
    if segments[..6] == [0; 6] && segments[6] != 0 {
        let [.., a, b, c, d] = address.octets();
        return format!("::{}", std::net::Ipv4Addr::new(a, b, c, d));
    }
    address.to_string()
}

const RANGE_EMPTY: u8 = 0x01;
const RANGE_LOWER_INCLUSIVE: u8 = 0x02;
const RANGE_UPPER_INCLUSIVE: u8 = 0x04;
const RANGE_LOWER_UNBOUNDED: u8 = 0x08 | 0x20;
const RANGE_UPPER_UNBOUNDED: u8 = 0x10 | 0x40;

/// A range: its flags, then each bound it has, length-prefixed, written by
/// `bound`; as PostgreSQL prints it, `empty` or `[lower,upper)`.
fn range_text(bytes: &[u8], bound: impl Fn(&[u8]) -> Option<String>) -> Option<String> {
    let (&flags, mut rest) = bytes.split_first()?;
    if flags & RANGE_EMPTY != 0 {
        return rest.is_empty().then(|| "empty".to_string());
    }
    let mut text = String::from(if flags & RANGE_LOWER_INCLUSIVE != 0 {
        '['
    } else {
        '('
    });
    for (unbounded, separator) in [
        (RANGE_LOWER_UNBOUNDED, Some(',')),
        (RANGE_UPPER_UNBOUNDED, None),
    ] {
        if flags & unbounded == 0 {
            push_range_bound(&mut text, &bound(take_value(&mut rest)??)?);
        }
        text.extend(separator);
    }
    text.push(if flags & RANGE_UPPER_INCLUSIVE != 0 {
        ']'
    } else {
        ')'
    });
    rest.is_empty().then_some(text)
}

/// A range bound, double-quoted when it is empty or holds a character the
/// range syntax uses, with each `"` and `\` in it doubled.
fn push_range_bound(text: &mut String, bound: &str) {
    let quoted = bound.is_empty()
        || bound.chars().any(|c| {
            matches!(c, '"' | '\\' | '(' | ')' | '[' | ']' | ',') || c.is_ascii_whitespace()
        });
    if quoted {
        text.push('"');
    }
    for c in bound.chars() {
        if matches!(c, '"' | '\\') {
            text.push(c);
        }
        text.push(c);
    }
    if quoted {
        text.push('"');
    }
}

/// The decimal text of a PostgreSQL `NUMERIC` in its binary wire form: a
/// digit count, the weight of the first base-10000 digit, the sign, the
/// display scale, then the digits.
pub(super) fn numeric_text(bytes: &[u8]) -> Option<String> {
    let word = |at: usize| -> Option<u16> {
        Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
    };
    let ndigits = usize::from(word(0)?);
    let weight = i32::from(word(2)? as i16);
    let sign = word(4)?;
    let scale = usize::from(word(6)?);
    match sign {
        0xC000 => return Some("NaN".to_string()),
        0xD000 => return Some("Infinity".to_string()),
        0xF000 => return Some("-Infinity".to_string()),
        0x0000 | 0x4000 => {}
        _ => return None,
    }
    let digits = (0..ndigits)
        .map(|position| word(8 + 2 * position))
        .collect::<Option<Vec<_>>>()?;
    // The digit of base-10000 place `weight - group`; zero past the stored ones.
    let digit = |group: i32| {
        usize::try_from(group)
            .ok()
            .and_then(|group| digits.get(group).copied())
            .unwrap_or(0)
    };

    let mut text = String::new();
    if sign == 0x4000 {
        text.push('-');
    }
    if weight < 0 {
        text.push('0');
    } else {
        text.push_str(&digit(0).to_string());
        for group in 1..=weight {
            text.push_str(&format!("{:04}", digit(group)));
        }
    }
    if scale > 0 {
        let mut fraction = String::with_capacity(scale + 4);
        let mut group = weight + 1;
        while fraction.len() < scale {
            fraction.push_str(&format!("{:04}", digit(group)));
            group += 1;
        }
        fraction.truncate(scale);
        text.push('.');
        text.push_str(&fraction);
    }
    Some(text)
}

/// Whether the binary value of a column of `type_name` is one sqlx cannot read
/// as a Rust date or time: a date or timestamp holding `infinity` or
/// `-infinity`, on which its decoders panic, and `TIME '24:00:00'`,
/// PostgreSQL's one time past the last microsecond of a day, which they read
/// as midnight.
pub(crate) fn unreadable_moment(type_name: &str, bytes: &[u8]) -> bool {
    match type_name {
        "DATE" => fixed::<4>(bytes)
            .is_some_and(|days| matches!(i32::from_be_bytes(days), i32::MAX | i32::MIN)),
        "TIME" => {
            fixed::<8>(bytes).is_some_and(|micros| i64::from_be_bytes(micros) >= 86_400_000_000)
        }
        "TIMESTAMP" | "TIMESTAMPTZ" => fixed::<8>(bytes)
            .is_some_and(|micros| matches!(i64::from_be_bytes(micros), i64::MAX | i64::MIN)),
        _ => false,
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/database_json_rows_tests.rs"]
mod tests;
