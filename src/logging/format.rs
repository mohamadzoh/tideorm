use std::fmt::Write as _;
use std::time::Duration;

use super::entry::QueryLogEntry;

/// One line naming the operation, table, duration, row count and any failure.
pub(super) fn format_summary(entry: &QueryLogEntry) -> String {
    let mut output = format!("[TideORM][{}]", entry.operation);

    if let Some(ref table) = entry.table {
        let _ = write!(output, " {}", table);
    }

    if let Some(duration) = entry.duration {
        let _ = write!(output, " ({}ms)", duration.as_millis());
    }

    if let Some(rows) = entry.rows {
        let _ = write!(output, " [{} rows]", rows);
    }

    if !entry.success {
        output.push_str(" FAILED");
        if let Some(ref err) = entry.error {
            let _ = write!(output, ": {}", err);
        }
    }

    output
}

pub(super) fn format_debug(entry: &QueryLogEntry) -> String {
    let timing = entry
        .duration
        .map(|d| format!(" ({}ms)", d.as_millis()))
        .unwrap_or_default();

    format!("[TideORM][{}]{} {}", entry.operation, timing, entry.sql)
}

pub(super) fn format_slow(entry: &QueryLogEntry, threshold: Duration) -> String {
    match entry.duration {
        Some(duration) => format!(
            "[TideORM][SLOW QUERY] {} ({}ms > {:?} threshold)\n  SQL: {}",
            entry.operation,
            duration.as_millis(),
            threshold,
            entry.sql
        ),
        None => format!(
            "[TideORM][SLOW QUERY] {} (exceeded {:?} threshold)\n  SQL: {}",
            entry.operation, threshold, entry.sql
        ),
    }
}

pub(super) fn format_error(entry: &QueryLogEntry) -> String {
    let error = entry.error.as_deref().unwrap_or("Unknown error");
    format!(
        "[TideORM][ERROR] {} failed: {}\n  SQL: {}",
        entry.operation, error, entry.sql
    )
}
