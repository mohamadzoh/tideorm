# Compatible Servers

Four distributed databases speak a protocol TideORM already has a driver for, and TideORM runs on
them with no change to your models or queries:

| Server | Protocol | Feature | URL |
|--------|----------|---------|-----|
| CockroachDB | PostgreSQL | `cockroachdb` | `postgresql://root@host:26257/app` |
| YugabyteDB (YSQL) | PostgreSQL | `yugabytedb` | `postgresql://yugabyte@host:5433/app` |
| TiDB | MySQL | `tidb` | `mysql://root@host:4000/app` |
| Vitess (`vtgate`) | MySQL | `vitess` | `mysql://user@host:15306/keyspace` |

Each feature is another name for its protocol's (`cockroachdb` and `yugabytedb` for `postgres`,
`tidb` and `vitess` for `mysql`) and builds the same code, so either name works (see [Installation](installation.md)). The backend
stays `DatabaseType::Postgres` or `DatabaseType::MySQL`, so the SQL dialect is that protocol's.
The connection also learns which server answered, from what the server reports as it connects
(CockroachDB's `crdb_version` parameter, the `-YB-`, `-TiDB-` and `-Vitess` marks in the version
string), and `Database::server()` returns it (see [Databases](databases.md)):

```rust,ignore
use tideorm::prelude::*;

match db.server() {
    Some(DatabaseServer::CockroachDb) => println!("CockroachDB"),
    Some(server) => println!("{server}"),
    None => println!("not connected yet"),
}
```

TideORM's own PostgreSQL and MySQL test suites run against each server in CI (CockroachDB
26.4, YugabyteDB 2025.2, TiDB 8.5 and Vitess 24 on MySQL 8.0). A scenario that needs something a
server does not have is skipped there; the lists below name each one.

## CockroachDB

Each new connection sets three session variables, so SQL written for PostgreSQL means the same
thing there:

- `serial_normalization = sql_sequence`: a `SERIAL` column takes its values from a sequence, as on
  PostgreSQL, instead of `unique_rowid()`'s large, unordered 64-bit values.
- `autocommit_before_ddl = off`: DDL inside a transaction stays in it, so a failed migration rolls
  back as a whole instead of committing the statements before it.
- `create_table_with_schema_locked = off`: a new table can be altered inside a transaction, which
  migrations do.

A variable the server's version lacks is left out.

**Integer widths.** CockroachDB reads a bare `INT` or `SERIAL` as 64-bit. Migrations therefore
write `INT4`, `SERIAL4` and `BIGINT`/`BIGSERIAL` explicitly (the same types on PostgreSQL), so an
`i32` field gets the 32-bit column it decodes from. Write `INT4` in your own SQL too. TideORM
leaves `default_int_size` at 8: at 4 every sequence, `BIGSERIAL`'s included, would stop at
2,147,483,647.

**Transactions.** CockroachDB runs every transaction `SERIALIZABLE` and may abort one that
conflicts with another (SQLSTATE `40001`, "restart transaction"). That error's `failure_kind()` is
`SerializationFailure` and `is_retryable()` is true: run the transaction again (see
[Retrying a Transaction](errors.md#retrying-a-transaction)).

**Decimals in a list.** CockroachDB reads each value of an `IN` list as the column's type, so
against an integer column it would round a decimal `5.4` to `5`, and fail on a `u64` past
`i64::MAX` (which TideORM binds as a decimal). TideORM casts a decimal in a `where_in` list to
`NUMERIC`, so it compares as on PostgreSQL. Write the cast in your own SQL too.

**Ordering.** In ascending order CockroachDB puts `NULL`s first, PostgreSQL last. TideORM keeps
CockroachDB's order (its `null_ordered_last` setting makes it refuse a `SELECT DISTINCT` ordered
by a nullable column); write `NULLS LAST` in a raw `order_by` if the order matters.

**[Migrations](migrations.md) and [seeds](seeding.md)** are serialized with a transaction-scoped advisory lock
(`pg_advisory_xact_lock`), held on a pinned session for the whole run: CockroachDB has no
session-level `pg_advisory_lock`. `drop_index_if_exists` on a table in a schema that does not
exist fails there (`3F000`), where PostgreSQL skips it.

**Schema export.** [`SchemaWriter`](schema.md) writes CockroachDB's own `SHOW CREATE ALL SCHEMAS`, `TYPES`,
`TABLES`, `ROUTINES` and `TRIGGERS` output: its `pg_catalog` lacks what the PostgreSQL export
reads. Indexes and defaults are inside each `CREATE TABLE`, and foreign keys follow the tables,
then an `ALTER SEQUENCE .. OWNED BY` for each sequence a column owns, which `SHOW CREATE` leaves out.
A table or view calling a routine comes before that routine in the file.

**Not available on CockroachDB:** a raw JSONPath in [`where_json_path_exists`](json-and-arrays.md) (wildcards, filters:
PostgreSQL's `@?`; a plain path such as `a.b[1]` works), multirange and multidimensional array
values, `NULLS NOT DISTINCT` unique constraints, deferrable constraints, arrays of `JSONB`, a
`where_in` of arrays on an array column, and the `cidr`, geometric and range types.

## YugabyteDB

TideORM connects to YugabyteDB's YSQL API (port 5433) as it does to PostgreSQL; nothing is set on
its connections.

**Keys.** Each connection caches 100 sequence values by default, so auto-increment keys are
unique but have gaps and are not in insertion order across connections. For dense keys start the
servers with `--tserver_flags=ysql_sequence_cache_minval=1`.

**Ordering.** Tables are hash-sharded, so rows come back in hash order unless the query has an
`order_by`: never rely on insertion order, as with `HasMany::load()`, which adds no `ORDER BY`.

**Indexes.** A `GIN` index (`FullTextIndex`, a JSONB index) is created as YugabyteDB's `ybgin`,
and the schema export writes it so. New tables get YugabyteDB's `REPLICA IDENTITY CHANGE`, which
the export keeps.

**DDL commits as it runs.** A `CREATE`, `ALTER` or `DROP` inside a transaction is not rolled
back with it, so a migration that fails part-way leaves the DDL before the failure applied, as on
MySQL; [`mark`](migrations.md) reconciles the ledger afterwards.

**Read restarts.** YugabyteDB retries a read restart by itself only for a single statement. A
DDL statement just after another connection's DDL, or a statement in a multi-statement
`exec_raw`, can fail with SQLSTATE `40001` ("Restart read required"); `is_retryable()` is true,
so run it again, and send raw statements one per call.

**Clocks.** A YugabyteDB server stops when its clock drifts more than 0.5 s from the others
(`max_clock_skew_usec`); open connections then fail with SQLSTATE `57P01`.

**Not available on YugabyteDB:** nondeterministic (case-insensitive) collations,
`DEFERRABLE` unique constraints, `REPLICA IDENTITY USING INDEX`, `CLUSTER`,
`ALTER DOMAIN .. ADD CONSTRAINT`, `ALTER TABLE .. ENABLE RULE`, and a partition with a generation
expression of its own (as on PostgreSQL before 16). `UNLOGGED` and storage parameters such as `fillfactor` are
accepted and ignored. A SQL function `RETURNS SETOF <table>` whose body selects a whole row
(`SELECT t FROM t`) fails when called (SQLSTATE `42804`); select the columns instead.

## TiDB

TiDB is tested as one TiDB server; nothing is set on its connections beyond what MySQL's get.

**Collation.** TiDB's default collation is `utf8mb4_bin`, so text compares case-sensitively:
`where_eq("email", "Ada@example.com")` does not find `ada@example.com`, as it does under MySQL 8's
`utf8mb4_0900_ai_ci`. Declare the column with a `_ci` collation if comparisons should ignore case.

**Keys.** Each TiDB server hands out `AUTO_INCREMENT` values from its own cached range, so keys
are unique but not consecutive, and not in insertion order across servers.

**CHECK constraints** are neither stored nor enforced unless `tidb_enable_check_constraint` is on
(`SET GLOBAL tidb_enable_check_constraint = ON`); foreign keys are enforced from TiDB 8.5.

**Not available on TiDB:** `SERIALIZABLE` transactions (error 8048), `FULLTEXT` indexes and so
full-text search, `JSON_TABLE` and so `array_remove` on a JSON array column, invisible columns,
`CONCAT` in an expression default, and stored procedures, functions, triggers and events (the
schema export has none to write).

## Vitess

TideORM connects to `vtgate`; the database in the URL is the keyspace. It is tested on one
unsharded keyspace.

**Several keyspaces.** A `vtgate` serving more than one keyspace cannot drop a view: an
unqualified `DROP VIEW` fails with `VT09005` ("no database selected") even with the keyspace
selected, and a keyspace-qualified one reaches the tablet as written, where the MySQL database is
named `vt_<keyspace>` unless the tablet sets `--init_db_name_override`, so `DROP VIEW IF EXISTS`
finds nothing there. `tideorm db wipe` and `migrate fresh` therefore fail on a keyspace that holds
a view; drop views with Online DDL (`SET @@ddl_strategy = 'vitess'`) instead.

**Row cap.** By default a tablet returns at most 10,000 rows from one statement
(`--queryserver-config-max-result-size`) and fails a longer result with error 1317. TideORM splits
an `insert_all` into statements of at most 10,000 rows, since each is read back with one `SELECT`.
Read a larger result with `chunk()` or `paginate()` (see [Query Builder](queries.md)), or raise
the limit.

**Transaction timeout.** A tablet kills a transaction open for 30 seconds by default
(`--queryserver-config-transaction-timeout`). A long batch in one transaction, such as an
`insert_all` of tens of thousands of models whose keys are text (inserted a row at a time), needs
more.

**Locks.** [Migrations](migrations.md) and seeds take `GET_LOCK`, which `vtgate` holds on a reserved connection of
the session and releases when the session ends.

**Schema export.** [`SchemaWriter`](schema.md) leaves out the `ALTER DATABASE COLLATE` statements it writes
for MySQL, since Vitess manages a keyspace's collation; each table names its own in its
`CREATE TABLE`.

**Not available through Vitess:** `CREATE DATABASE` and `ALTER DATABASE` (keyspaces are managed by
Vitess), and `CREATE FUNCTION`, `TRIGGER` and `EVENT` (`vtgate` accepts `CREATE PROCEDURE`), so
an export holding functions, triggers or events does not replay through it. In a sharded
keyspace, an `AUTO_INCREMENT` key needs a Vitess sequence.
