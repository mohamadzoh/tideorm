use super::entry::QueryLogEntry;

pub(super) fn format_debug(entry: &QueryLogEntry) -> String {
    let timing = entry
        .duration
        .map(|d| format!(" ({}ms)", d.as_millis()))
        .unwrap_or_default();

    format!("[TIDE][{}]{} {}", entry.operation, timing, entry.sql)
}

pub(super) fn format_slow(entry: &QueryLogEntry, threshold: u64) -> String {
    match entry.duration {
        Some(duration) => format!(
            "[TIDE][SLOW QUERY] {} ({}ms > {}ms threshold)\n  SQL: {}",
            entry.operation,
            duration.as_millis(),
            threshold,
            entry.sql
        ),
        None => format!(
            "[TIDE][SLOW QUERY] {} (exceeded {}ms threshold)\n  SQL: {}",
            entry.operation, threshold, entry.sql
        ),
    }
}

pub(super) fn format_error(entry: &QueryLogEntry) -> String {
    let error = entry.error.as_deref().unwrap_or("Unknown error");
    format!(
        "[TIDE][ERROR] {} failed: {}\n  SQL: {}",
        entry.operation, error, entry.sql
    )
}
