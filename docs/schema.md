# Schema Sync and Export

TideORM can create your tables from your models while you develop (schema sync), write a connected database's schema to a SQL file (schema export), and read a database's tables, columns, indexes and foreign keys (the catalog). For the schema of a deployed database, use [migrations](migrations.md).

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub active: bool,
}

async fn connect_for_development() -> tideorm::Result<()> {
    TideConfig::init()
        .database("postgres://localhost/mydb")
        .auto_models()
        .sync(true)
        .schema_file("schema.sql")
        .connect()
        .await?;
    Ok(())
}
```

## Schema Sync

Sync is for local development and tests. Do not enable it in production: a schema change that is not additive can still fail or damage data there. Deployed databases change through [migrations](migrations.md).

`sync(true)` makes `connect()` bring the database in line with the registered models: it creates the tables that are missing and adds the columns an existing table lacks. It never drops or changes a table or a column.

### Registering Models

Sync works on the models you register. Register them on `TideConfig`:

| Method | Registers |
|---|---|
| `auto_models()` | Every model linked into the program |
| `models::<User>()` | One model |
| `models::<(User, Post)>()` | Each model of a tuple (up to 16) |
| `models_matching("src/models/*")` | Every model whose source file matches the pattern |

`models_matching` matches the path of the file each `#[tideorm::model]` is written in. `*` matches within one path segment and `**` crosses directories, so `src/models/*`, `src/models/*.model.rs` and `src/models/**/*.rs` all work. A pattern that matches no model logs a warning.

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .models_matching("src/models/*.model.rs")
    .sync(true)
    .connect()
    .await?;
```

Every model registers itself at link time, but only when its file is compiled: a file no `mod` declaration reaches registers nothing, whatever the pattern says. Discovery itself does not connect or change any table.

With sync on and no model registered, `connect()` fails with `No models registered for schema sync; call auto_models() or models::<YourModel>()`, followed by the tables of the models the program does compile.

### What Sync Creates

A missing table is created with a column per field, typed by the field's Rust type: the type's `ValueType::column_type()`, which a model's `Column::def()` reports too, rendered as [migrations render it](migrations.md#column-types) (a `String` is `TEXT`, `LONGTEXT` on MySQL; a `u32` is `BIGINT` on PostgreSQL). An encrypted field's column is text whatever the field's type. A field type with no `ValueType` does not compile: derive `DeriveValueType` on a newtype or `DeriveActiveEnum` on an enum, or mark a field that is not a column `#[tideorm(skip)]`. A field marked `auto_increment` whose type cannot auto-increment gets a column without it, and sync logs a warning.

Sync also creates the indexes a model declares with `#[index]` and `#[unique_index]`:

- On a new table, a failing index fails the sync.
- On an existing table, sync adds the indexes that are missing; a failure there, such as a unique index over rows that repeat a value, is logged as a warning rather than stopping startup.
- An index name that another table already owns is an error: sync does not reuse that name.

Sync creates **no foreign keys and no enum types**. Declare those in a [migration](migrations.md#foreign-keys).

### Existing Tables

On an existing table sync only adds:

- It adds the columns the table lacks and never drops or changes one.
- It logs a warning naming the columns the model no longer declares. Remove them with a migration.
- A new non-`Option` field without a `#[tideorm(default = "..")]` can be added only while the table is empty, since its rows would otherwise hold `NULL`. With rows present, sync fails and names the column. Give the field a default, make it an `Option`, or add it with a migration that fills it in.

SQLite adds columns under its own limits, and sync logs a warning for each:

- It cannot add a `NOT NULL` column without a default at all, so sync adds such a column to an empty table as nullable.
- It cannot add a column whose default is the current time (`CURRENT_TIMESTAMP`, `CURRENT_DATE`, `CURRENT_TIME`) or a parenthesized expression to an existing table, so sync adds it without the default. A migration that [rebuilds the table](migrations.md#rebuilding-a-sqlite-table) keeps it.

### Tables in Another Schema

A model's `#[tideorm(schema = "..")]` qualifies every statement sync issues:

| Backend | `schema = "analytics"` means |
|---|---|
| PostgreSQL | The `analytics` schema, which must exist before sync runs |
| MySQL, MariaDB | The `analytics` database |
| SQLite | Nothing; it is ignored |

Two models of one table (in one schema) share it: the second one adds the columns and indexes the first lacks, and a column both declare keeps the first model's definition.

### Force Sync

`force_sync(true)` drops and recreates the table of every registered model on each connect, deleting all of its rows. It works only together with `sync(true)`.

```rust
TideConfig::init()
    .database("sqlite://./test.db?mode=rwc")
    .auto_models()
    .sync(true)
    .force_sync(true)
    .connect()
    .await?;
```

- It drops every table before creating any, each one before the registered tables its foreign keys reference. The recreated tables have no foreign keys.
- On PostgreSQL the drop is `CASCADE`, which drops the keys other tables hold on a table (not those tables), so a reference cycle or an unregistered table referencing a registered one is left to it. MySQL, MariaDB and SQLite (while the referencing table has rows) refuse to drop a referenced table, and report it.
- Seeds run after sync, so `force_sync` does not drop what [seeding](seeding.md) wrote at startup.

`force_sync` is for throwaway development and test databases only. In a process whose `TIDEORM_ENV` is `production` (in any case), it fails with `Error::Configuration` before dropping anything, unless `TIDEORM_ALLOW_FORCE_SYNC` is `1` or `true`. Set that variable only for a database you mean to empty, such as a staging one.

### Running Sync Yourself

`connect()` runs sync between migrations and seeds. To sync a database at another time, or a database other than the global one:

```rust
use tideorm::prelude::*;
use tideorm::sync::{SyncRegistry, sync_database_with};

async fn rebuild_test_schema(db: &Database) -> tideorm::Result<()> {
    SyncRegistry::register_all_models();
    sync_database_with(db, SyncMode::Recreate).await?; // what force_sync(true) does
    Ok(())
}
```

| Call | Does |
|---|---|
| `db.sync()` | Additive sync of `db` |
| `tideorm::sync::sync_database(&db)` | The same |
| `sync_database_with(&db, SyncMode::Additive)` | What `sync(true)` does |
| `sync_database_with(&db, SyncMode::Recreate)` | What `force_sync(true)` does, with the same production guard |

The registry is global to the process: `TideConfig::auto_models()`, `models()` and `models_matching()` fill the same one that `SyncRegistry` exposes.

| `SyncRegistry` method | Does |
|---|---|
| `register_all_models()` | Registers every linked model; returns how many |
| `register_models_matching(pattern)` | Registers the models whose source path matches; returns how many |
| `register_schema(ModelSchema)` | Registers one table description |
| `schema_count()` | The number of registered tables |
| `get_all_schemas()` | The registered `ModelSchema`s |
| `clear()` | Forgets every registration, for tests |

A single model registers with `User::register_for_sync()` (the `SyncModel` trait, in the prelude).

### Previewing the SQL

`SchemaGenerator` renders the `CREATE TABLE` and `CREATE INDEX` statements sync would run for a model, for a backend you name, without connecting:

```rust
use tideorm::prelude::*;

fn users_ddl() -> String {
    SchemaGenerator::new(DatabaseType::Postgres)
        .add_model::<User>()
        .generate()
}
```

It also renders tables you describe yourself, each column's SQL type written as you give it:

```rust
use tideorm::prelude::*;

fn audit_log_ddl() -> String {
    let audit_log = TableSchemaBuilder::new("audit_log")
        .column(ColumnSchema::new("id", "BIGINT").primary_key())
        .column(ColumnSchema::new("action", "TEXT").not_null())
        .column(ColumnSchema::new("created_at", "TIMESTAMPTZ").not_null().default("now()"))
        .index(IndexDefinition::new("idx_audit_log_action", vec!["action".to_string()], false))
        .build();

    SchemaGenerator::new(DatabaseType::Postgres)
        .add_table(audit_log)
        .generate()
}
```

- `ColumnSchema::new(name, sql_type)` is nullable until `not_null()` or `primary_key()`; `auto_increment()` and `default(expression)` complete it.
- `TableSchemaBuilder` takes `schema(name)` for a qualified table, and `index` or `indexes` for `IndexDefinition`s.
- The output starts with a comment naming the backend and the time it was generated, and writes `IF NOT EXISTS` on each statement (MySQL has none for an index).

## Schema Export

`schema_file(path)` makes `connect()` write the connected database's schema to `path` as SQL. The file is a readable dump of the schema, meant to be committed and reviewed in diffs:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .schema_file("schema.sql")
    .connect()
    .await?;
```

- It is written once per `connect()`, after migrations, sync and seeds; nothing rewrites it in between.
- It describes every table of the database, not only the registered models.
- A failed export fails `connect()`. `connect_only()` skips it.
- `TideConfig::schema_file_path()` returns the configured path.

`SchemaWriter` does the same at any time:

```rust
use tideorm::prelude::*;

async fn export(db: &Database) -> tideorm::Result<()> {
    SchemaWriter::write_schema("schema.sql").await?;            // the global database
    SchemaWriter::write_schema_for(db, "schema.sql").await?;    // another one
    let sql: String = SchemaWriter::schema_sql(db).await?;      // the script, not written
    println!("{sql}");
    Ok(())
}
```

`write_schema_for` writes nothing when the export fails. Keep the schema stable while it runs.

### PostgreSQL

- Schemas, extensions and collations come first.
- Each base table is read from the catalog with its columns and primary key. Generated columns are written with their expression.
- Every secondary index is written as PostgreSQL defines it, so ordering, collations, operator classes, full-text, expression and partial indexes, `INCLUDE` columns and `NULLS NOT DISTINCT` all survive.
- `CHECK` constraints that call no routine are declared before inheriting tables are created. Checks that call a routine, foreign keys and `EXCLUDE` constraints come after every table.
- Tables, views, functions and the types they reference follow their catalog dependencies.
- Domain checks, column defaults, rules and triggers follow the tables, with their enabled state.
- Partition column overrides, replica identities and table storage options are kept.
- Ownership, grantor chains, column grants and default privileges are kept.
- Unsupported types, custom aggregates, operators, foreign tables and servers, and text search objects fail the export, rather than producing a file that cannot recreate them.

### CockroachDB

CockroachDB's file is what its own `SHOW CREATE ALL` statements write, because its `pg_catalog` lacks what the PostgreSQL export reads: schemas, types, tables, sequences and views, with foreign keys after every table, then routines and triggers. Indexes and defaults are inside each `CREATE TABLE`. A table or view that calls a routine of its own database comes before that routine, so such a file cannot be replayed as written. See [Compatible Servers](compatible-servers.md).

### MySQL and MariaDB

- Tables, views, stored routines, triggers and events keep the definitions the server wrote (`SHOW CREATE`), and MariaDB sequences their own DDL.
- Foreign keys are moved out of each `CREATE TABLE` and added after every table exists, so tables that reference each other restore.
- Views are created through temporary stand-in tables that each view then replaces, as `mysqldump` does, so any view dependency graph resolves. No rows are inserted.
- Replay the file with the `mysql` or `mariadb` client on one connection: it uses `DELIMITER` for stored program bodies, and sets the SQL modes, collations and event time zones they were created under, restoring your session's settings at the end.
- Replaying needs `ALTER` on the destination database: its default collation is restored from the source, while each stored program is created under its own original database collation.
- References to the exported database are rebound to the destination. Explicit references to other databases, and dynamic SQL strings, keep their text.
- An object whose definition `SHOW CREATE` does not disclose to your account fails the export, naming it; grant more privileges and run it again.

### SQLite

SQLite keeps the statement that created each table, view, index and trigger, so the file is those statements, with generated columns, collations, constraints and `AUTOINCREMENT` included. An FTS5 table's shadow tables are left out, since the FTS5 table recreates them.

### Compatible Servers

| Server | Difference |
|---|---|
| Vitess | Leaves out the `ALTER DATABASE COLLATE` statements written for MySQL, since Vitess manages a keyspace's collation; each table names its own. Vitess does not accept `CREATE FUNCTION`, `TRIGGER` or `EVENT`, so a file holding them does not replay through it |
| YugabyteDB | A `GIN` index is written as `ybgin`, and tables keep YugabyteDB's `REPLICA IDENTITY CHANGE` |
| TiDB | TiDB has no stored routines, triggers or events, so there are none to write |

### What an Export Leaves Out

An export holds schema, not row data, current sequence values, database accounts or replication subscriptions. The roles and definers it names must exist where you replay it, and the replaying connection needs permission to restore their ownership and grants.

## Reading the Catalog

`db.catalog()` reads what a connected database holds. It is the reader sync, migrations and the schema export use themselves.

```rust
use tideorm::prelude::*;

async fn describe(db: &Database) -> tideorm::Result<()> {
    let catalog = db.catalog();
    for table in catalog.tables().await? {
        println!("{table}");
        for column in catalog.columns(&table).await? {
            let null = if column.nullable { "NULL" } else { "NOT NULL" };
            println!("  {} {} {null}", column.name, column.data_type);
        }
        for index in catalog.indexes(&table).await? {
            println!("  index {} on {:?} (unique: {})", index.name, index.columns, index.unique);
        }
        for key in catalog.foreign_keys(&table).await? {
            println!("  {} -> {}.{}", key.column, key.references_table, key.references_column);
        }
    }
    Ok(())
}
```

| Method | Returns |
|---|---|
| `tables()` | Base tables by name, PostgreSQL's partitioned tables and MariaDB's system-versioned ones included |
| `views()` | Views by name |
| `sequences()` | Sequences by name, on PostgreSQL and MariaDB; MySQL and SQLite have none |
| `enum_types()` | PostgreSQL's enum types by name; the other backends declare an enum on its column |
| `columns(table)` | The columns of a table or view, in order; empty when there is no such table |
| `indexes(table)` | The table's indexes; SQLite has none for an `INTEGER PRIMARY KEY`, which is the row id |
| `foreign_keys(table)` | The table's foreign keys, one entry per key column |

Each returns a `tideorm::Result`. The catalog reads the connection's current schema on PostgreSQL, its database on MySQL and MariaDB, and the `main` database on SQLite. On PostgreSQL a table in another schema is listed as `schema.name`, and a table passed that way is read from that schema; schemas named `pg_*` and `information_schema`, and what an extension installed, are left out. Inside a transaction the catalog reads on that transaction, so it sees the tables the transaction created.

The types are in `tideorm::schema`: `Catalog`, `ColumnInfo`, `IndexInfo` and `ForeignKeyInfo`.

| `ColumnInfo` field | Meaning |
|---|---|
| `name` | The column's name |
| `data_type` | Its type as the database writes it: `character varying(255)` on PostgreSQL, `int unsigned` on MySQL, the declared type on SQLite |
| `nullable` | Whether it accepts NULL |
| `default` | Its default expression as the catalog writes it, if any |
| `primary_key` | Whether it is part of the primary key |
| `unique` | Whether a unique index other than the primary key holds this column alone |
| `foreign_key` | Whether it is part of a foreign key |
| `extra` | What else the catalog says, if anything: MySQL's `auto_increment`, PostgreSQL's identity, a generated column's expression |

| `IndexInfo` field | Meaning |
|---|---|
| `name` | The index's name |
| `columns` | Its key columns in order. PostgreSQL writes an expression key as its expression; MySQL and SQLite leave one out |
| `unique` | Whether it is unique |
| `primary` | Whether it is the primary key's |
| `whole_columns` | Whether every key part is a whole column: false for an expression, or on MySQL and MariaDB for a column prefix |

| `ForeignKeyInfo` field | Meaning |
|---|---|
| `column` | The referencing column |
| `references_table` | The referenced table, written as `tables()` writes it |
| `references_column` | The referenced column |

SQLite names no referenced column for a foreign key to a parent table without a primary key, a key it does not enforce, so `foreign_keys` leaves such a key out.
