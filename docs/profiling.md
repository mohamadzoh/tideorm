# Logging and Profiling

TideORM gives you three ways to see what your queries do: print a query before it runs, log the statements as they run, and time them. The types are in `tideorm::logging` and `tideorm::profiling`, and the common ones are also in the prelude.

| You want to | Use |
|---|---|
| See the SQL one query will run | `debug()`, `build_sql_preview()` |
| Check a query is valid without running it | `validate()` |
| Print every statement while developing | `TIDE_LOG_QUERIES=true` |
| Record statements, slow ones and counts in-process, or send them to your logs | `QueryLogger` |
| Count and time every statement | `GlobalProfiler` |
| Build a report from statements you choose | `Profiler`, `QueryAnalyzer` |

## Inspecting a Query Before It Runs

Print `debug()` on a query-builder query to see the statement with its values written in, the parameterized statement with its parameters, and the clauses it was built from:

```rust
use tideorm::prelude::*;

let query = Post::query()
    .where_eq(Post::PUBLISHED, true)
    .order_by(params.sort.as_str(), Order::Asc)
    .limit(20);
println!("{}", query.debug());
query.validate()?;
```

A query that a terminal method would refuse (an unsafe `order_by` taken from a request, a bad raw fragment, a zero page) shows the reason on an `Invalid:` line. `validate()` returns the same error without running anything, so a handler can answer 400 before touching the database.

| Method | Returns |
|---|---|
| `debug()` | A `QueryDebugInfo`: table, operation, select list, conditions, joins, order, group, limit and offset, the SQL, the parameters, and any error. `Display` prints it as a block |
| `debug_for(DatabaseType::MySQL)` | The same in another backend's dialect, with its markers (`$1` or `?`) and quoting |
| `build_sql_preview()` | The statement `get()` would run, values written in as literals, under a banner marking it display-only |
| `build_sql_preview_for(DatabaseType::SQLite)` | The preview in another backend's dialect |
| `validate()` | `Ok(())`, or the error the query would fail with |

`debug()` and `build_sql_preview()` use the dialect of the database the query would run on, or PostgreSQL's when no database is connected. The preview inlines values for reading only: never run it, run the query.

## Printing Every Statement

Set `TIDE_LOG_QUERIES` to `1` or `true` while developing:

```bash
TIDE_LOG_QUERIES=true cargo run
```

Every statement is then printed to stderr as `[TideORM DEBUG] Query: <sql>`. Query-builder statements print before they run; typed CRUD (`find`, `save`, `update`, `delete`, upserts), raw SQL and migration DDL print once they complete. Bound values are never printed: a statement appears with its markers (`$1`, `?`). The variable is read once, when the first statement runs; any other value, `false` and `0` included, leaves it off.

`TIDE_LOG_QUERIES` also turns on the `QueryLogger`'s history and statistics (below), so `QueryLogger::history()` has the statements too.

## The Query Logger

`QueryLogger` is a process-wide structured logger. While it is enabled it records each statement as a `QueryLogEntry`, keeps a bounded history and counters, and prints entries according to its level.

```rust
use std::time::Duration;
use tideorm::prelude::*;

QueryLogger::global()
    .level(LogLevel::Debug)
    .slow_query_threshold(Duration::from_millis(50))
    .history_limit(200)
    .enable();
```

`QueryLogger::global()` returns a builder; nothing changes until `enable()`, `disable()` or `apply()` ends it (`apply()` keeps the current on/off state). The builder's settings:

| Builder method | Default |
|---|---|
| `level(LogLevel)` | `TIDE_LOG_LEVEL`, else `Off` |
| `slow_query_threshold(Duration)` | `TIDE_SLOW_QUERY_MS`, else 100 ms |
| `history_limit(usize)` | 100 entries; 0 keeps none |
| `timing(bool)` | `true`: printed entries show their duration |

The level decides only what is printed; an enabled logger keeps its history and statistics at any level, `Off` included.

| `LogLevel` | Prints |
|---|---|
| `Off` | Nothing |
| `Error` | Failed statements |
| `Warn` | Failed and slow statements |
| `Info` | Failed and slow statements, and a one-line summary of every other |
| `Debug` | Every statement with its timing |
| `Trace` | Every statement with timing, row count and any parameters attached to the entry |

The level also governs TideORM's own status lines (`[TideORM] ..`) and warnings (`[TideORM WARN] ..`), which print unless the level is set to `Off` or `Error`.

What the logger records: query-builder statements, tagged with their table, and every statement the engine runs (typed CRUD, raw SQL, migration DDL), once it completes, with its duration, row count and error. Bound values are never recorded.

### Reading the History and Statistics

```rust
use tideorm::prelude::*;

for entry in QueryLogger::slow_queries() {
    println!("{:?} {} {:?}", entry.table, entry.sql, entry.duration);
}

let stats = QueryLogger::stats();
println!("{stats}"); // total, slow count and share, total and average time
```

| Function | Returns |
|---|---|
| `QueryLogger::history()` | The recorded entries, oldest first |
| `QueryLogger::slow_queries()` | The entries at or above the slow-query threshold |
| `QueryLogger::stats()` | A `QueryStats`: `total_queries`, `slow_queries`, `total_time()`, `avg_query_time()`, `slow_percentage()` |
| `QueryLogger::clear_history()`, `reset_stats()` | Empty the history, zero the counters |
| `is_enabled()`, `level()`, `slow_query_threshold()` | The current settings |
| `enable()`, `disable()`, `set_level(LogLevel)` | Change them without the builder |

A `QueryLogEntry` has `sql`, `params`, `operation` (a `QueryOperation`: `Select`, `Insert`, `Update`, `Delete`, `Transaction` or `Unknown`), `table`, `duration`, `rows`, `success`, `error` and `timestamp`. `QueryLogger::log(entry)` records an entry of your own.

### Sending Log Output Elsewhere

By default the logger prints to stderr. `set_sink` hands each printed line, with its entry, to your function instead, such as `tracing`; `clear_sink()` goes back to stderr:

```rust
use tideorm::prelude::*;

QueryLogger::set_sink(|entry, text| {
    if entry.success {
        tracing::debug!("{text}");
    } else {
        tracing::warn!("{text}");
    }
});
```

A sink runs on the task that ran the statement, so one that writes to a file or the network should hand the text off and return. A bounded channel keeps that from holding up queries, dropping lines instead of waiting when the writer falls behind:

```rust
let (lines, mut pending) = tokio::sync::mpsc::channel::<String>(1024);
QueryLogger::set_sink(move |_entry, text| {
    let _ = lines.try_send(text.to_owned());
});
tokio::spawn(async move {
    while let Some(text) = pending.recv().await {
        write_log_line(&text).await; // your writer
    }
});
```

### Environment Variables

The logger reads these once, the first time it is used. Settings made in code afterwards take precedence.

| Variable | Effect |
|---|---|
| `TIDE_LOG_QUERIES` | `1` or `true`: print every statement and enable the logger |
| `TIDE_LOG_LEVEL` | `off`, `error`, `warn`, `info`, `debug` or `trace`: set the level, and enable the logger for any level but `off`. An unknown value means `off` |
| `TIDE_SLOW_QUERY_MS` | The slow-query threshold in milliseconds, for both the logger and `GlobalProfiler` (default 100) |

`QueryLogger::reset()` puts the logger back as the environment configures it: `TIDE_LOG_LEVEL` and `TIDE_SLOW_QUERY_MS` read again, counters and history emptied, the history limit at 100, timing shown, output on stderr. Call it in tests that change the global logger.

## Global Profiling

`GlobalProfiler` counts and times every statement the engine runs while it is enabled: typed CRUD, query-builder reads and writes, raw SQL and migration statements, each timed around the driver call. Transaction control (`BEGIN`, `COMMIT`, savepoints) is not counted. It is off by default and keeps no SQL text, only counters.

```rust
use std::time::Duration;
use tideorm::prelude::*;

GlobalProfiler::enable();
GlobalProfiler::reset();
GlobalProfiler::set_slow_threshold(Duration::from_millis(200));

let users = User::query().where_eq(User::ACTIVE, true).limit(25).get().await?;

let stats = GlobalProfiler::stats();
println!("{} queries, {} slow, avg {:?}", stats.total_queries, stats.slow_queries, stats.avg_query_time());

GlobalProfiler::disable();
```

`GlobalStats` is the same type as the logger's `QueryStats`, with its own counters and threshold. `GlobalProfiler::is_enabled()` and `slow_threshold()` read the settings, and `record(duration)` adds a duration of your own while it is enabled.

The profiler is process-wide, so reset it between tests that assert on its counts.

## Manual Profile Reports

When you need SQL text, tables, row counts and the slowest statements, build a report with `Profiler`. It records only what you hand it; it does not subscribe to TideORM's statements.

```rust
use std::time::Duration;
use tideorm::prelude::*;

let mut profiler = Profiler::start();

profiler.record("SELECT id, email FROM users WHERE active = true", Duration::from_millis(12));
profiler.record_full(
    ProfiledQuery::new("SELECT * FROM posts WHERE user_id = 42", Duration::from_millis(37))
        .with_table("posts")
        .with_rows(8),
);

let report = profiler.stop();
println!("{report}");
for hint in report.suggestions() {
    println!("{hint}");
}
```

This is useful when you instrument a benchmark, compare two shapes of a query, or build a focused performance report in a test.

| Item | Gives |
|---|---|
| `Profiler::query_count()`, `elapsed()` | The statements recorded so far and the time since `start()` |
| `ProfiledQuery::cached()` | Marks a statement served from a cache, which the N+1 count skips |
| `ProfiledQuery::operation_kind()` | Its `QueryOperation`, read from the SQL |
| `ProfileReport` fields | `total_duration`, `query_duration`, `queries`, `operations` (count per operation), `slowest`, `tables` (count per table) |
| `report.query_count()`, `avg_query_time()`, `query_time_percentage()` | Totals, and the share of the session's time spent in statements |
| `report.queries_slower_than(duration)` | The statements at or above a duration |
| `report.suggestions()` | Hints: more than 10 uncached reads of one table (a likely N+1), slow statements, `SELECT *` |
| `report.suggestions_with(duration)` | The same hints with your own slow threshold |

`suggestions()` counts a statement slow at `GlobalProfiler::slow_threshold()`.

## Query Analysis

`QueryAnalyzer::analyze(sql)` reads a statement and returns suggestions for its shape: `SELECT *`, an `UPDATE` or `DELETE` with no `WHERE`, a `LIKE` with a leading wildcard, `OR` conditions, `ORDER BY` without `LIMIT`, `NOT IN`, a function applied to a column in `WHERE`, and an ID column compared with a quoted number.

```rust
use tideorm::prelude::*;

for suggestion in QueryAnalyzer::analyze("SELECT * FROM users WHERE LOWER(email) = 'ada@example.com'") {
    println!("[{}] {}: {}", suggestion.level.label(), suggestion.title, suggestion.suggestion);
}

let complexity = QueryAnalyzer::estimate_complexity("SELECT u.id FROM users u JOIN posts p ON p.user_id = u.id");
println!("{complexity:?}: {}", complexity.description());
```

A `QuerySuggestion` has a `level` (`SuggestionLevel::Info`, `Warning` or `Critical`), a `title`, an `explanation` and a `suggestion`, and prints all of them through `Display`. `estimate_complexity` returns a `QueryComplexity`: `Simple`, `Moderate`, `Complex` or `VeryComplex`.

The analyzer reads text; it does not ask the database for a plan. Run `EXPLAIN` through [Raw SQL](raw-sql.md) for that.

## Slow-Query Thresholds

The logger and `GlobalProfiler` each keep their own slow-query threshold. Both start at `TIDE_SLOW_QUERY_MS`, or 100 ms, and a statement counts as slow at or above it. Change the logger's with the builder's `slow_query_threshold`, the profiler's with `GlobalProfiler::set_slow_threshold`. `ProfileReport::suggestions()` follows the profiler's.

## Related Pages

- [Errors](errors.md): `log_format()` writes an error with its query context for your logs.
- [Benchmarking](benchmarking.md): TideORM's own Criterion benchmarks.
- [Query Cache](caching.md): the cache keeps its own hit and miss statistics.
