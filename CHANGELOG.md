# Changelog

All notable changes to TideORM will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.12.0] - 2026-09-24

A second repository-wide cleanup. Dead, duplicated and misleading code was removed, unused public
API was deleted outright rather than deprecated, and the defects the audit turned up are fixed.
This release is breaking: read the upgrade notes and the removal list before upgrading.

### Upgrading — Action Required

- **Query-level `or_where_*` calls now form one OR group.** They used to push one single-condition
  group per call, and groups are ANDed, so `.or_where_gt("price", 1000).or_where_lt("price", 50)`
  rendered `price > 1000 AND price < 50` and matched nothing. Every `or_where_*` call on a query now
  joins the same OR group, which is ANDed with the rest of the query — the model
  `BatchUpdateBuilder` already used. `.where_x(a).or_where_y(b)` therefore means `a AND (b)`; put
  both alternatives in `or_where_*`, or use `or_where(|g| ..)` / `begin_or()`, for `a OR b`.
- **`#[validate(custom = "..")]` is a compile error.** It compiled to a marker nothing evaluated,
  so it accepted every value. Put model-level checks in `Callbacks::before_validation` or
  `Callbacks::after_validation` and return `Err(Error::validation(field, message))`.
- **`#[validate]` rules on a `#[tideorm(skip)]` field run, and on a relation field are a compile
  error.** Both were never read. A loaded model holds a skip field's `Default`, so give a field
  with rules an `Option` type: `None` passes every rule except `required`.
- **Relation declarations are checked against the wrapper type.** The relation kind now comes from
  the field's wrapper (`HasOne<T>`, `HasMany<T>`, ...). A kind attribute that disagrees with the
  wrapper, or a missing `foreign_key` on `HasOne`/`HasMany`/`BelongsTo`, is a compile error; a
  `HasOne<T>` field without a kind attribute used to be silently left unwired.
- **`MorphOne`, `MorphMany`, `SelfRef` and `SelfRefMany` prefer the database.** Like the direct
  wrappers, `load()` now queries whenever a connection is reachable and serves its cache only
  without one, so a payload deserialized from a request body is never reported as stored rows.
  Read an eager-loaded value with `get_cached()`.
- **`connect()` fails when the MySQL/MariaDB version probe fails** instead of silently assuming
  MySQL.
- **`#[tideorm(hidden = "..")]` names are checked.** Each must be a field, a column or an
  attachment relation, so a typo is a compile error instead of a field `to_json` shows; a column
  name hides its field. Without `hidden`, a model hides its soft-delete field, which is
  `deleted_at` unless `deleted_at_column` names another.
- **A NaN or infinite float in a filter, a `set()` or a `having()` fails the query.** JSON writes
  one as `null`, so `where_eq("price", f64::NAN)` became `price IS NULL` and matched the rows with
  no price — `"NaN".parse::<f64>()` succeeds, so a request could send one. A `None` still means
  NULL.
- **`Seeder::rollback()` fails when the last seed the `_seeds` table records is not registered**
  (renamed or removed since it ran), naming it; it rolled nothing back and reported success, which
  left `reset()` and `refresh()` doing nothing. Register it again, or delete its `_seeds` row.
- **`BatchUpdateBuilder::execute_returning()` on MySQL and MariaDB returns
  `Error::BackendNotSupported`** (501) before anything runs, instead of a generic query error on
  MySQL and a syntax error on MariaDB, which has `UPDATE .. RETURNING` only from 13.0.
  `DatabaseType::MariaDB.supports_returning()` is `false` accordingly.
- **`CacheConfig` has a new `max_size_bytes` field**, so struct literals must set it (or end with
  `..CacheConfig::default()`).
- **`HighlightConfig` has a new `escape_html` field**, `true` by default: the record's text is
  HTML-escaped around the tags, so a stored `<script>` reaches the page as text. Struct literals
  must set it; set it `false` for tags that are not HTML.
- **A PostgreSQL full-text `language` must be a text search configuration name.** It is written
  into the statement as a constant, as `FullTextIndex` writes it into the index, so that searches
  can use the index at all; a name with other characters is refused.
- **Log lines use one prefix, `[TideORM]`** (query logs previously used `[TIDE]`).
- **`ModelMeta::field_names()` drops the `r#` of raw identifiers**, matching serde.
- Hand-written `InternalModel` implementations must implement `try_from_entity_model`; the
  infallible `from_entity_model` is gone. Generated models are unaffected.
- **`select()` and the column of every `where_*` take identifiers only.** `select()` accepts a
  column, `table.column`, `column AS alias`, `*` or `table.*`; a filter column, including a batch
  update's, is a column or `table.column`. Both slots were spliced in verbatim, so
  `where_eq("1=1 OR name", x)` matched every row and `select(vec!["(SELECT password FROM users
  LIMIT 1)"])` read another table. Move SQL expressions to `select_raw()` / `where_raw()`.
- **`get()` refuses a `select()` that leaves model columns out.** An unselected `Option` column
  decoded as `None`, and saving that model wrote the `None` back over the stored value. Read a
  partial row with `get_json()`, or select every column.
- **A `None` in an `IN` list means NULL.** `where_in(col, [a, None])` also matches NULL rows, and
  `where_not_in(col, [a, None])` keeps the non-NULL rows other than `a`. Taken literally,
  `IN (.., NULL)` never matched a NULL and `NOT IN (.., NULL)` matched nothing at all. A
  `where_not_in` of NULLs alone keeps every row of a `NOT NULL` column, so like an empty one it
  counts as no filter for `delete()` and `update_all()`.
- **`insert_all()` and upserts validate.** They wrote models that failed their `#[validate]`
  rules; the batch is now refused before anything is written, naming the failing model's position.
  Callbacks still do not run on those paths. `Validate` is a supertrait of `Model` for this;
  generated models implement it already, a hand-written `Model` impl needs one.
- **`NaiveDateTime` timestamps are managed.** A `created_at`/`updated_at` field of that type was
  written as the caller left it — `1970-01-01 00:00:00` for a default-constructed model, even on a
  `timestamps_naive()` table. It is set to the current UTC time like a `DateTime<Utc>` one, so a
  value set by hand is replaced.
- **A managed `created_at` is written once.** `update()` no longer writes it, and an upsert that
  finds the row already there keeps it unless `update_columns` names it. Code that changed a
  creation time through either should use `update_all().set("created_at", ..)`.
- **`get_json()` and `Database::raw_json` decode by declared type.** Text comes back verbatim:
  PostgreSQL and MySQL rows had numeric-looking text rewritten (`"00123"` became `"123"`, `"1e3"`
  became `1000.0`, `"null"` became `null`). `BYTEA`, MySQL `BLOB`/`BINARY` and PostgreSQL arrays,
  which came back `null`, are arrays; MySQL unsigned integers — every `ROW_NUMBER()` and `RANK()`
  value — are numbers instead of strings. `get_json()` returns a model's own columns as the
  model's JSON has them on every backend, so SQLite booleans, JSON and timestamps no longer arrive
  in storage form; raw SQL, which has no model, still reports what the database stores. MariaDB
  declares a JSON column as text, so raw SQL returns its text, which used to be parsed. SQLite
  reports a value it declares no type for (an expression, or a `DECIMAL` column) by what it
  stores, so a `REAL` such as a float `SUM()` or a running total is a number.
- **`sum()`, `avg()`, `min()` and `max()` return the type you ask for.** They returned an `f64`,
  which rounded an integer or decimal total past 2^53 and made the `min`/`max` of a text or date
  column `0`, a year, or an error. `sum::<T>()` returns `T` (the column's integer type, `Decimal`
  for an exact total, or `f64`); `avg`, `min` and `max` return `Option<T>`, `None` over no rows,
  in the model's own type for that column (`min::<DateTime<Utc>>("created_at")`). Annotate the
  binding — `let total: i64 = query.sum("price").await?;` — or use a turbofish. `aggregates()`
  returns the tuple you ask for the same way.
- **A soft-delete model hides and marks trashed rows everywhere.**
  - `find(id)`, `find_with`, `find_or_fail` and `exists` leave a trashed row out, as every other
    read does; `find` returned it, so `find(id)` and `exists(id)` could disagree. Read one with
    `Model::query().with_trashed().find(id)`; `reload()` still reads the record in hand.
  - `delete()`, `destroy(id)`, `query().delete()` and `delete_all()` mark rows deleted instead of
    removing them; `force_delete()` (on a model or a query) removes them for good. `delete()` or
    `delete_all()` of `only_trashed()` rows is an error pointing at `force_delete()`, and
    `only_trashed().delete_all()` no longer deletes every live row as it used to.
  - `Model::update_all()` leaves trashed rows out, as a query does; `.with_trashed()` reaches them,
    and `query().only_trashed().restore()` restores them.
- **`page(0, n)`, a zero page size and an out-of-range page fail with `Error::Validation`** naming
  `page` or `per_page`, the error `Model::paginate` gives the same numbers; `page()` reported
  `Error::Query`.
- **`order_by("name desc", Order::Asc)` fails** instead of sorting descending: a column may carry
  a direction only when it agrees with the argument. Read a request's sort string with `sort()`.
- **`HasManyThrough::count()` counts what `load()` returns**: a pivot row pointing at a deleted or
  soft-deleted related row no longer counts.
- **A query with a join qualifies the model's own columns.** A bare model column in a filter, the
  soft-delete predicate, `latest()`/`oldest()`, `select()` and the aggregates is written
  `table.column` once the query joins, so a name both tables have is the model's column instead
  of an "ambiguous column" error. Name a joined table's column as `table.column`, as before.
- **A projection whose outputs share a name is refused.** `select(vec!["items.id", "tags.id"])`
  returned one `id` from `get_json()`, and `get()` could fill the model from the joined table;
  MySQL rejected the same statement as a derived table. Alias one of them (`tags.id AS tag_id`).
- **Full-text search leaves soft-deleted rows out**, as a query does; `with_trashed()` and
  `only_trashed()` on the search builder read them. On SQLite a search reads only the columns it
  names, where it searched every column of the index, and `FullTextIndex::to_sqlite_sql()`
  returns a fifth statement that indexes the rows the table already holds: rows written before
  the index was created were never found.
- **A soft-deleted pivot row unlinks its pair.** `HasManyThrough::load()`, `load_with()`, eager
  loading and `count()` leave it out, `attach()` restores it instead of finding the pair present,
  and `sync()` deletes it with the live rows; it used to keep the pair loaded after `sync([])` and
  block re-attaching it.
- **The entity manager refuses to flush a managed entity whose primary key was changed.** The
  flush saved it under the new key, overwriting whichever row held that key. Detach the entity
  and persist a new one.
- **Model declarations the derive now refuses**, each of which compiled and then failed or went
  wrong at run time: `#[tideorm(encrypted = "..")]` on a primary key (every write stored a new
  ciphertext, so `find`, `update` and `delete` never matched and `save()` inserted duplicates);
  `auto_increment` on a key that is not an integer (the insert left the key out); a relation
  field inside an `Option`, `Box`, `Rc` or `Arc`; `#[validate(range(a..b))]`, which leaves `b`
  out in Rust while the rule includes it (write `range(a..=b)` or `range(a, b)`); a `min`, `max`
  or `range` bound that is not finite, which made the derive panic; and `#[index(..)]` with
  arguments on a field, where the bare attribute now indexes that field.
- **`connect()` syncs the schema before it seeds**, so a seed fills the tables sync creates, and
  a `force_sync` no longer drops what the seeds wrote.
- **Generated index names past 63 bytes are shortened with a hash** (`idx_<table>_<columns>`,
  and a model's `uidx_` and `idx_` names alike). PostgreSQL cut such names to 63 bytes, so two
  indexes whose names shared that prefix were one, and MySQL refused them. On PostgreSQL a sync
  creates such an index again under its new name; drop the one under the cut name.
- **`soft_delete()` is `delete()` on a soft-delete model**: the delete callbacks run, so a
  `before_delete` that refuses stops it, and a row already deleted keeps its stamp. It returns the
  row as stored.
- **`update_all_except` keeps the key and the conflict columns**, as the default upsert does; it
  wrote them, so a UUID-keyed row was re-keyed.
- **An upsert keyed only by an auto-increment key the database has not numbered is `create()`**,
  with its callbacks; it stored the placeholder `0`.
- **`save_with_many` and `NestedSaveBuilder::with_many` save each child**, updating one already
  stored, as `save_with_one` does; they inserted it again.
- **A model whose own `Serialize` or `Deserialize` derive renames a field is not query-cached**:
  TideORM's generated half reads and writes field names, so a cache hit lost the renamed fields.
- **An empty or blank encryption key configures no key**, so encrypting fails with the missing-key
  error; tokens and encrypted fields were sealed under a key anyone could derive.
- **A batch `set()` of an encrypted field takes a string or `None`**; any other value is refused
  rather than stored where no load could read it back.
- **A transaction in which a statement deadlocked refuses to commit** (`Error::Transaction`): the
  database rolled the whole transaction back, and on MySQL and MariaDB the statements after the
  deadlock ran outside it and were committed while the transaction reported success.
- **`Database::connect`, `Database::init` and `Database::builder().build()` ask a MySQL-protocol
  server whether it is MariaDB** (`SELECT VERSION()`), as `TideConfig::connect` does, and fail
  when that fails; they also take a `mariadb://` URL, in any case.
- **The driver's own statement logging is off**: `TIDE_LOG_QUERIES` and the `QueryLogger` decide
  what is printed, where sqlx logged every statement at `INFO` to any subscriber listening.
- **Validation errors come out in the order the fields failed**, which for a model is the order
  it declares them, so `create()` reports the same field every time.

### Removed — Breaking

- **`BatchUpdateBuilder::returning()`**: `execute_returning()` returns the rows by itself, and
  `execute()` refused a builder marked with `returning()`. Drop the call.
- **Query builder:** `QueryBuilder::{cache_with_options, begin_or_where_eq, begin_or_where_gt,
  begin_or_where_gte, begin_or_where_lt, begin_or_where_lte, begin_or_where_like,
  begin_or_where_contains, begin_or_where_starts_with, begin_or_where_ends_with, begin_or_where_in,
  begin_or_where_null, begin_or_where_not_null, begin_or_where_between, having_count_gte,
  having_count_lt, having_count_lte, group_by_columns, where_column_raw, without_trashed,
  running_avg, last_value}` (use `begin_or().or_where_*()` and the general helpers);
  `OrBranchBuilder::{branch_count, total_conditions}`; `query::OrBranch` (an AND-combined
  `OrGroup` now); `AggregateFunction`; `columns::ColumnOperator` (`ColumnCondition.operator` is a
  `query::Operator`); `WindowFunctionType::as_sql`, `WindowFunction::to_sql`, `CTE::to_sql`;
  `Operator::{SubqueryIn, SubqueryNotIn, ArrayContainsAny, ArrayContainsAll}`,
  `ConditionValue::Subquery` and the `RawExprWithValues.preview_sql` field.
- **Models and types:** `types::{Castable, CastType, CastValue}`; `model::{CreateBuilder,
  UpdateBuilder}`; `IndexDefinition::parse`; `ModelMeta::{default_presenter, all_indexes,
  has_indexes}`; `Model::{db, database, extract_translations, extract_files,
  process_file_for_json}`; the ignored `"presenter"` option of `to_json`; `UnixTimestamp` /
  `UnixTimestampMillis` `is_past`, `is_future`, `From<i64>` and the public tuple field.
- **Validation and callbacks:** `ValidationRule::{Confirmed, Custom}`, `ValidationBuilder::custom`,
  `ValidationErrors::into_error`, `Validate::{validate_all, custom_validations, validation_rules}`;
  `CallbackRunner::{run_create_callbacks, run_update_callbacks}` and the unused
  `BeforeCreateDispatch`/`BeforeUpdateDispatch`.
- **Relations and the entity manager:** `relations::{RelationInfo, RelationType,
  RelationConstraints, MorphResult, MorphResult3, MorphResult4, WithPivot}`;
  `RelationPath::{full_path, is_nested, depth}`; `RelationTree::has_nested`;
  `EagerQueryBuilder::get_relation_tree`; `WithRelations::with_relation`;
  `EagerLoadExt::with_relation`; `HasManyThrough::{items, current_keys, related_table}`;
  `HasOne::{with_entity_manager, current_key, child_table}`; `BelongsTo::with_entity_manager`;
  `MorphTo::{get_cached, set_cached}`; `TrackedHasMany::{items, child_table}`; the third
  (`related_table`) argument of `with_metadata`.
- **Errors and configuration:** `Error::InsertReturningNotSupported` and
  `Error::{insert_returning_not_supported, not_found_with_context, is_not_found,
  is_connection_error, is_validation_error, is_query_error, is_transaction_error,
  is_configuration_error, is_backend_not_supported, is_primary_key_not_set,
  is_insert_returning_not_supported, is_unique_violation, is_not_null_violation,
  is_check_violation, is_constraint_violation}` (match the variant, or use `failure_kind()` /
  `sqlstate()`); `config::{RegisterMigrations, RegisterSeeds}` and `TideConfig::{migrations, seeds,
  soft_delete_by_default, db, try_db, is_postgres, is_mysql, is_mariadb, is_mysql_compatible,
  is_sqlite, current, pool_config, write_schema_with_generator, write_schema_sql}`;
  `Config::{new, is_soft_delete_default, get_file_base_url}` and the `soft_delete_by_default`
  field (it was never read); `DatabaseType::{supports_native_json_operators,
  supports_fulltext_search, supports_schemas, is_mysql_compatible, default_port, url_scheme}`.
- **Database handle:** the `database::Connection` trait, `Database::try_global` (use `try_db`),
  `Transaction::__internal_transaction` (use `connection()`), `database::__current_backend`.
- **Migrations, schema and sync:** `SyncRegistry::{register_entity, build_schema_builder,
  entity_count}`, `sync::{EntityRegistrationFn, normalize_rust_type}`, `schema::rust_type_to_sql`;
  `SchemaWriter::{register_schema, get_registered_schemas, clear_registry, write_schema_from_db}`
  (`write_schema` reads the connected database, as `write_schema_from_db` did);
  the 18 typed `TableSchemaBuilder` helpers (`bigint` through `double`) — use
  `.column(ColumnSchema::new(..))`; `TableSchema.primary_key`; `AlterTableBuilder::{add_index,
  drop_index}` (use `Schema::create_index` / `drop_index`); `Schema::database_type`;
  `TableBuilder::index_named`; `MigrationResult::has_rolled_back`;
  `SeedResult::{has_rolled_back, total}`; `ModelSchema::columns`;
  `Migrator::migrations_table_name`; the public `CompositePrimaryKey` / `UniqueConstraint`;
  `RegisterModels` for tuples longer than 16. The `schema-sync` SeaORM feature is no longer
  enabled, so `sea-schema` is no longer compiled.
- **Full text, cache, logging, profiling, attachments, translations:**
  `FullTextSearch::search_highlighted` and `FullTextSearchBuilder::{with_highlights,
  highlight_config}` (they never produced highlights); `CacheOptions::{key, with_key}` and
  `QueryCache::set_key_prefix`; `QueryLogger::{log_timed, log_error, init_from_env}` (the
  environment is read on first use now); `QueryDebugInfo::{with_operation, with_params}`;
  `QueryOperation::Raw`; `QueryTimer::stop`; `QueryStats::{total_time_ms, threshold_ms,
  avg_query_time_ms, slow_query_percentage}` in favour of `total_time_ns`, `slow_threshold_ms`,
  `total_time()`, `avg_query_time()` and `slow_percentage()`; `AttachmentError::{ParseError,
  NotSupported}` and `TranslationError::NotSupported`; `SoftDelete::deleted_at_column` (use
  `ModelMeta::deleted_at_column`); `Tokenizable::tokenization_enabled`.
- **Full text:** `SearchMode::Fuzzy`, which searched exactly like `Natural`: no backend has a fuzzy
  match TideORM could rely on. Use `SearchMode::Natural`.

### Added

- **`QueryBuilder::lock_for_update()`** renders `SELECT ... FOR UPDATE` on PostgreSQL, MySQL and
  MariaDB, so a read-check-write inside a transaction can lock the rows it is about to change.
  Without it two concurrent transactions read the same row and the last `update()` wins: two
  orders of 3 against a stock of 4 both shipped. `count()`, `exists()` and the aggregates lock the
  rows they read too, a locked read never uses the query cache, and fragments keep the lock.
  SQLite has no row locks and renders nothing: the first write of a transaction locks the whole
  database, so the second of two competing transactions fails with a retryable
  `LockNotAvailable` error instead.
- **`QueryBuilder::aggregates(&[Aggregate::count(), Aggregate::sum("amount"), ..])`** computes
  several aggregates over the same rows in one statement and returns them as the tuple you ask
  for (`(u64, Decimal, Option<Decimal>)`), where five metrics used to take five queries.
- **`FullTextSearchBuilder::with_trashed()` and `only_trashed()`** select soft-deleted rows, which
  a search now leaves out.
- **`pluck::<T>(column)`, `value::<T>(column)`, `get_as::<T>()` and `paginate(page, per_page)` on
  `QueryBuilder`** read one column of every row, the column of the first row, each row as your own
  `Deserialize` type (the shape of a join or a grouped `select_raw()`), and a page together with
  the count of every matching row, as a `Paginated<M>` that serializes with its `last_page`.
- **`where_raw_with(sql, params)`**, with `or_where_raw_with` and `and_where_raw_with`, binds values
  into a raw fragment through `?` placeholders on every backend, so a raw condition no longer needs
  a value written into its SQL.
- **`having()` takes a typed comparison**, `having(Aggregate::sum("revenue").gt(5_000))`, with
  `gt`, `gte`, `lt`, `lte`, `eq` and `ne` on `Aggregate` and the value bound; a raw string still
  works. **`reorder(column, direction)`** replaces the orders a query has so far.
- **`QueryBuilder::validate()`** reports why a query would be refused — an unsafe `order_by`
  from a request, a bad raw fragment, a zero page — without running it, so a handler can answer
  400 first; `debug()` shows the same reason as `Invalid:` and now lists the OR groups too.
  `QueryDebugInfo` has a new `error` field.
- **`EagerQueryBuilder::query(|q| ..)`** shapes the root query of an eager load with any
  `QueryBuilder` method, so a `.with("posts")` no longer cuts a query off from the methods the
  eager builder does not forward.
- **`where_not_between`**, with `or_where_not_between` and `and_where_not_between` on every
  builder, renders `NOT BETWEEN`; like `where_between`, a NULL bound is refused.
- **`QueryBuilder::update_all()`** starts a bulk update of the rows a query selects — scopes,
  filters, OR groups, its soft-delete scope and the database `query_with` names carry over — so a
  chain of scopes can be updated without restating its filters, and the update's `or_where_*`
  calls join the query's OR group. A query that joins, groups, pages or unions is refused when the
  update runs. Batch updates also render for the backend of the connection that runs them.
- **`where_has::<R>(fk, lk, |q| ..)` and `where_doesnt_have::<R>(..)`** filter by a related
  model's own query: its soft-delete scope applies, its values bind as its columns' types, and
  any filter works inside the closure — the typed form of the table-level `has_related`. Over the
  model's own table (a tree's children) the related rows are read under an alias, so a nested
  `where_has` or a qualified column in the closure names the related row.
- **`QueryBuilder::find(id)` and `find_or_fail(id)`** look a key up among the rows a query
  matches, its filters and scope applied; a union is refused, since the key would filter its first
  query only.
- **`sort("-created_at,name")`** orders by a request's sort string — terms separated by commas,
  `-` or `desc` for descending — checking every column as `order_by` does.
- **Column comparisons**: `where_column_eq`, `_ne`, `_gt`, `_gte`, `_lt` and `_lte`, with
  `or_`/`and_` forms, compare two columns of the same row, across a join too.
- **Every builder has every filter**: the JSON and array filters gained `or_`/`and_` forms and
  work inside OR groups and batch updates, and `when`/`when_some` work on batch updates, OR
  groups and eager queries.
- **Lists come from any iterable.** `where_in`, `where_not_in`, `eq_any`, `ne_all`, the
  `where_array_*` filters and a typed column's `is_in`/`not_in` take a `Vec`, an array, `&ids` or
  a set; the array filters take any `Serialize` value, as the other filters do, where they
  refused a `Uuid`, a timestamp or a `Decimal`.
- **A typed column of any type compares**: `eq`, `ne`, `is_in` and `not_in` work for a column of
  any `Serialize` type — an enum, a newtype, a JSON value — and a nullable column
  (`Column<Option<T>>`) compares with a plain `T`.
- **`SortOrder`**, the `Order` enum under a second name, for a crate whose own `Order` type (an
  `Order` model) shadows the prelude's.
- **`t.string_with(name, length)` and `ColumnType::Varchar(n)`** declare a `VARCHAR(n)` column;
  `t.string` stays `VARCHAR(255)`.
- **Full text:** `highlight(HighlightConfig)` marks the whole-word matches in each `get_ranked()`
  result's searched columns, filling `SearchResult::highlights`, and `stop_words`,
  `min_word_length` and `max_word_length` leave terms out of the search text on every backend,
  leaving quoted phrases as written. All of these existed and did nothing.
- **`Seeder::names()`** lists the seeds a seeder holds without a database, so a seed name can be
  checked before anything runs; the CLI checks `--seeder` against it before dropping tables.
- **`FullTextIndex::sqlite_rebuild_sql()`** re-indexes a table's SQLite FTS5 index by its current
  rowids, which a `VACUUM` renumbers for a table without an `INTEGER PRIMARY KEY`.
- **`HasTranslations::serialized_key(field)`** names the key a field is serialized under, so
  `to_translated_json` writes each translation where a `rename_all` model's JSON has the field;
  the default is the field's name.
- **`#[index]` and `#[unique_index]` on a field** index that field's column; they were accepted
  there and ignored.
- **Two relations to one model**, such as an `author` and an `editor` that are both
  `BelongsTo<User>`, compile, and each loads eagerly by its own keys.
- **Validation:** the string rules apply to `Text` fields, `min`, `max` and `range` to `Decimal`,
  `required` to an `Option` of a `Uuid`, a date, a `bool`, JSON or a `Vec`, and
  `range(min..=max)` is accepted. `ValidatableValue::numeric_bound` rounds a bound to a value's
  precision (a defaulted method).
- **`MorphTo::with_values` takes an `Option<String>` type value**, so a `MorphTo` may have a
  nullable type column; a row holding NULL in both loads no owner.

### Fixed

- **Queries.** `debug()` and `build_sql_preview()` came from a second, hand-written renderer that
  had drifted from the executed SQL (`eq_any` previewed as `= ANY(ARRAY[..])` but ran as `IN (..)`,
  union and CTE operands showed bare placeholders); the preview is now the executed statement with
  its values inlined. `select_subquery` executed display-only SQL with inlined literals rendered for
  the subquery's backend; it binds parameters now. The typed `count_distinct`/aggregate path
  dropped conditions it could not render — aggregating over more rows than asked — skipped query
  logging, and returned `0` for a count it could not decode. The batch-update OR guard accepted a
  group with a vacuous member.
- **Models.** An invalid `#[validate(regex = "..")]` pattern accepted every value; it now fails
  validation naming the pattern. `ValidationRule::Confirmed` passed any value. `update_with_one`,
  `update_with_many` and `delete_with_many` were not atomic; they run in a transaction like
  `save_with_*`. `to_json` put a hidden field back when it was also translatable. Raw-identifier
  fields such as `r#type` broke builders and `#[validate]`. The soft-delete column check rejected
  qualified `Option`/`chrono` spellings, and `Vec<DateTime<Utc>>` was treated as a timestamp
  column. `Model::deleted_at_column()` was ambiguous (E0034) with the prelude in scope. A failed
  dirty-tracking bookkeeping step was silently discarded; it is logged now.
- **Connection errors.** Generated `find`/`create`/`update`/`delete`/upsert reported a missing or
  unreachable database as `Error::Internal`; it is `Error::Connection` again. Connect and ping
  failures keep their SQLSTATE and source chain, and a failed rollback after a failed transaction
  closure is logged instead of swallowed. The "not initialized" error now tells you to initialize
  the connection instead of suggesting the URL is malformed.
- **Relations and the entity manager.** The eager-cache branches of `HasOne`, `BelongsTo` and
  `HasManyThrough` `load_in_entity_manager` overwrote the tracked instance with a possibly stale
  copy — the bug 0.10.0 fixed only for `TrackedHasMany`. `em.load` on a `HasManyThrough` returned
  duplicate rows for duplicate pivot links. `TrackedHasMany` kept two copies of its rows, so edits
  through `as_mut()` were invisible to `load()` without a connection. `SelfRef`/`SelfRefMany` (and
  now `MorphOne`/`MorphMany`) served deserialized payloads as stored rows. Rebuilding a `MorphTo`
  overwrote the discriminator and id just read from its columns. `attach` ignored the ambient
  transaction. `RelationPath::parse("")` produced an empty relation name. Each bad relation key
  was reported twice at compile time, and `MorphTo` targets were forced to implement
  `InternalModel`.
- **Migrations, schema and seeding.** Seeder priority was ignored for seeds that became ready
  after their dependencies (a FIFO queue behind a comment promising a heap). Schema sync rendered
  different DDL from migrations (`IDENTITY`/`AUTOINCREMENT` versus `BIGSERIAL`); both use one
  renderer now. `SchemaWriter` exported indexes in random order, collapsed composite primary keys
  to their first column, turned catalog decode failures into empty strings, used the configured
  instead of the connected backend, and exported SQLite auto-indexes under names SQLite rejects.
  Thirty-one database errors in the migrator, schema writer, seeder and sync were stringified,
  losing their SQLSTATE; they keep it now, with the SQL as context. `Schema::execute` joins the
  ambient transaction and flushes the query cache after DDL.
- **Full text.** `get`/`get_ranked`/`first`/`count` required `T: FromQueryResult`, which generated
  models do not implement, so they could not be called on a model at all; rows that failed to
  decode were dropped and undecodable ranks and counts became `0`. PostgreSQL's `real` rank was
  never decoded (always `0.0`), and MySQL bound the query and the `min_rank` threshold in swapped
  positions.
- **Logging, profiling and cache.** `TIDE_LOG_LEVEL` and `TIDE_SLOW_QUERY_MS` were never read, and
  `TIDE_LOG_QUERIES=false` turned query logging *on*. `tide_debug!` printed unconditionally and the
  internal macros ignored the logger level. `LogLevel::Info` behaved exactly like `Warn`.
  `QueryLoggerBuilder::disable()` discarded the builder's other settings. Query-time statistics
  truncated sub-millisecond queries to zero, the analyzer's id-versus-string check could never
  match, the profile report ignored the configured slow threshold, and the prepared-statement
  cache's entry count went stale.
- **Typed filter values.** Filter, batch-update and relation values travel as JSON, where a UUID,
  a timestamp or a decimal is a string. Bound as text, `where_eq("id", uuid)` failed on PostgreSQL
  (`uuid = text`) and matched nothing on SQLite and MySQL, which store a UUID as 16 bytes; the same
  broke `HasManyThrough` and eager loads on UUID keys. Values are bound as their column's type, and
  every `where_*`, `set()`, `attach()` and friend takes any `impl Serialize`.
- **Statements past the bind-parameter limit.** A `where_in` of more than 65,535 ids, an eager load
  over that many parents, and an `insert_all` whose rows × columns passed the limit (32,766 on
  SQLite) failed. Long integer lists are rendered inline, eager loads query in chunks of 5,000, and
  `insert_all` splits into several statements inside a transaction.
- **Types.** A `Vec<u8>` field did not compile, and neither did a `std::string::String` one.
  SQLite's `where_json_contains` compared documents as text, so `{"a": 1}` never matched, and its
  `where_json_contained_by` matched when the column's text appeared inside the operand's (`1` was
  "contained by" `{"a": 1}`, and a `%` in the column was a wildcard); both are rebuilt from
  `json_each` with PostgreSQL's `@>` and `<@` semantics. `min`, `max` and `range` validation
  accepted `NaN`.
- **JSON filters failed on a PostgreSQL `json` column.** `where_json_contains`,
  `where_json_contained_by`, `where_json_key_exists`, `where_json_key_not_exists`,
  `where_json_path_exists` and `update_all().json_set(..)` use operators only `jsonb` has, so on a
  `json` column, which `t.json(..)` creates, each failed with "operator does not exist". They cast
  the column to `jsonb`, which PostgreSQL drops for a `jsonb` column, so its indexes still apply.
- **A model deriving `Serialize` itself with renamed keys** (`#[serde(rename_all = "camelCase")]`)
  leaked its hidden fields through `to_json()`, which removed them by field name while serde had
  written `passwordHash`, and `save_with_many` stored every child under the foreign key it already
  had (0), because the parent's key was written under the field name too. `to_json()` finds fields
  under the keys serde writes, and nested saves, attachments and translations set the one field
  directly instead of round-tripping the model through serde. The model docs now say where such a
  derive must go: below `#[tideorm::model]`, since TideORM cannot see one above it or in the same
  list as `Model`, and the two impls then conflict.
- **An unset `Uuid` primary key was stored as the nil UUID.** The first such insert kept
  `00000000-0000-0000-0000-000000000000` as a real key, the second failed as a duplicate, and a
  database default such as `gen_random_uuid()` never applied because the nil value was sent. A
  `Uuid` key still nil at insert now gets a random (v4) key on every insert path, upserts
  included; a key the caller set is kept.
- **A connection error could leak the database password.** A URL the engine could not parse (a
  port past 65535, say) or had no driver for (a mistyped scheme, or a backend whose feature is not
  compiled in) was quoted whole in the error, password included, and `DatabaseBuilder`'s `Debug`
  printed it too. Both mask the URL's credentials now (`postgres://***@db:5432/app`).
- **Upserts and request bodies broke managed timestamps.** `insert_or_update()` and
  `on_conflict().insert()` overwrote an existing row's `created_at` with the time of the upsert,
  and with a natural (non-auto-increment) key they wrote `1970-01-01` into both `created_at` and
  `updated_at`, because that insert skipped the stamping `create()` does. `update()` wrote
  whatever `created_at` the model carried, so a request body could rewrite a row's creation time,
  while deserializing a model required both timestamps, so a create body without them was
  rejected. The insert half of an upsert is stamped like `create()`, a stored `created_at` is only
  ever written by the insert that created the row, and a body may leave out the managed timestamps
  and `#[tideorm(skip)]` fields.
- **A field of an unsupported type** (an enum, `i128`, a `HashMap`) failed with an error on the
  derive that said to "set an explicit column type", which no attribute does, followed by a page
  of SeaORM trait-bound errors. It is now one error on the field's type that lists the supported
  types and suggests a `String` for an enum and `#[tideorm(skip)]` for a field that is not a
  column.
- **MySQL full text.** Boolean-mode operators reached `AGAINST(..)` as typed, so a search for
  `c++` or a dangling `-` failed with a syntax error. The query is sanitized, and one with no
  searchable term left matches nothing.
- **`#[tideorm(schema = "..")]` only reached schema sync.** Every query, insert, update and delete
  named the bare table, so on PostgreSQL it went wherever the search path found a table of that
  name — a same-named table in `public` silently received the writes and served the reads. The
  engine's entity and every statement TideORM renders now name `schema.table` (a database, on
  MySQL, where sync creates the table there too), and joins accept `"schema.table"`.
- **Feature gates in generated code.** A model's entity-manager items were gated by
  `#[cfg(feature = "entity-manager")]`, which rustc evaluates against the crate defining the
  model: every crate without such a feature warned `unexpected cfg condition value` once per model
  (an error under `-D warnings`), and one that enabled `tideorm/entity-manager` without declaring
  the feature itself silently lost `find_in_entity_manager` and the rest. They follow TideORM's
  own feature now, so no crate has to declare it.
- **`multiply()`/`divide()` on an integer column** left SQLite holding a REAL
  (`price_cents * 1.1`), after which every read of the table failed to decode. The result is
  rounded back to an integer, as PostgreSQL and MySQL do when they store it.
- **A seed that failed part way kept what it had written.** A seed and its `_seeds` entry were
  separate statements, so a failure left the rows written so far and no ledger entry, and the next
  run wrote them again (or failed on a unique key). Each seed now runs in one transaction with its
  entry, and so does each rollback.
- **`.cache(ttl)` against the disabled default cache** did nothing without a word; the first such
  query now logs a warning naming `QueryCache::global().enable()`.
- **The query cache served rows a committed transaction had replaced.** A write invalidated its
  table's cached reads when it ran, but until the transaction committed other requests still read
  the old rows, and one that cached them in the meantime served them for the rest of the TTL. A
  read that a plain write overtook was cached the same way. A transaction now invalidates its
  writes' tables again once it commits (a savepoint's at its enclosing transaction's commit), and a
  read is not cached when a write to one of its tables landed while it ran.
- **Failure classification.** Beyond constraint violations, SQLite failures were unclassified; a
  busy or locked database is now `LockNotAvailable`, and a missing table, a missing column and a
  syntax error are told apart.
  MySQL's lock wait timeout (`HY000`) and a missing privilege (`42000`, which read as a syntax
  error) are classified by error number, and its deadlock is `Deadlock`. A connection closed under a
  statement without an error packet — MySQL's `KILL`, a server restart — was unclassified and not
  retryable; it is `ConnectionClosed`. A value that does not fit its column (SQLSTATE class `22`:
  too long, out of range, invalid for the type, and MySQL's "Incorrect string value") was
  unclassified; it is the new `InvalidValue`.
- **A `u64` past `i64::MAX` panicked on SQLite and PostgreSQL.** Their drivers panic converting
  such a value to their signed integer, so `where_eq("id", u64::MAX)`, `where_in`, a comparison,
  `update_all().set(..)`, `Model::paginate(1, u64::MAX)`, or a `u64` key passed to `find()` or
  held by a model passed to `delete()`, crashed the task instead of returning. A value past `i64::MAX` is bound as an exact decimal now, which
  matches nothing an integer column holds, and `paginate()` rejects a page size or offset past
  `i64::MAX` with a validation error.
- **A model field the driver cannot read back was written first.** A `u64` field on SQLite or
  PostgreSQL, or an `i8`, `u8` or `u16` field on PostgreSQL, was inserted and then failed the
  read-back, so the caller got an error for a stored row and a retry stored it twice; a `u64` past
  `i64::MAX` panicked the driver instead. `save()`, `create()`, `update()`, upserts, `insert_all()`
  and `update_all().execute_returning()` refuse such a model before writing, naming every such
  field and the type to use.
- **Concurrent `attach()` calls failed against a unique pivot key**, and without one could store a
  pair twice: `attach` checked for the row and then inserted it. On PostgreSQL and SQLite it is one
  statement now, which inserts the row only when it is missing and treats a key conflict as already
  attached; MySQL and MariaDB, where InnoDB locks what that statement reads and two of them
  deadlock, check with a plain read and insert with `ON DUPLICATE KEY UPDATE`. A pivot named with
  its schema (`pivot = "billing.user_roles"`) is quoted part by part, where the insert named one
  identifier `"billing.user_roles"`.
- **`only_trashed().force_delete()` also deleted live rows** matching its other filters, because
  `force_delete()` widened the scope to every row; it keeps the trashed-only scope now. And
  `only_trashed()` counts as the filter `restore()` and `force_delete()` require, so the whole
  trash can be restored or emptied. On a model without soft delete, which has no trash, it is
  refused there instead of being dropped.
- **One typo in a `#[validate]` rule buried its error under a page of others**, because the derive
  then generated nothing for the model. The model is generated without that rule, and the rule's
  error is the only one.
- **`QueryLogger` and `TIDE_LOG_QUERIES` missed every statement the query builder does not render:**
  `find`, `save`, `update`, `delete`, upserts and raw SQL, `Database::execute` included. They are
  logged as they complete; query-builder and migration statements are still logged once, before
  they run.
- **A crate defining a model needed `serde` and `serde_json` as direct dependencies**, because the
  generated code named both; it reaches them through `tideorm` now.
- **A model in a module that imports `tideorm::Result` did not compile**, and `use tideorm::*;`
  imports it: the engine's derives and the generated `validate()` write a bare `Result<_, _>`,
  which resolved to TideORM's one-parameter alias.
- **Full text:** an `offset()` without a `limit()` rendered a bare `OFFSET`, a syntax error on SQLite
  and MySQL, and `highlight_text` marked text inside the tags it had inserted when a search term was
  part of one (`b` in `<b>`), and marked `rust` in `rust-lang` when both were searched for.
- **Full-text search modes meant different things per backend.** A boolean `-javascript` was a
  required term on PostgreSQL and SQLite, which stripped the operator: `+rust -javascript` found
  only the articles it meant to leave out. It excludes now, as on MySQL, and so does a `-` written
  before a quoted phrase. MySQL ran `SearchMode::Phrase` as `WITH QUERY
  EXPANSION`, which widens a search instead of matching a phrase, and searched `Prefix` and
  `Proximity` in natural-language mode; PostgreSQL read `Proximity(n)` as exactly `n` words apart,
  in order; SQLite ignored the mode altogether. Each mode is built from the search's words on
  every backend now. SQLite's `get_ranked()` reported raw `bm25()`
  scores, where a better match is lower and negative, so `min_rank(r)` kept only the rows
  scoring below `-r`; the rank is negated now, higher is better everywhere, and `min_rank` means
  the same on every backend.
- **PostgreSQL full-text searches never used their index.** The text search configuration was a
  bound parameter, cast when the statement ran, which the planner cannot match to the constant in
  the index `FullTextIndex` builds, so every search read the whole table. A search left with no
  word, which the term filters make common, read it too, to match nothing; it matches nothing
  without reading it now, as on the other backends.
- **A union operand and a `with_query()` CTE lost their ordering and limit**, so
  `a.union(b.order_desc("score").limit(10))` returned every row of `b`. They keep them now: on
  SQLite, which takes no parenthesized operand, a limited one is read through a derived table.
- **MySQL's JSON containment disagreed with PostgreSQL's.** `JSON_CONTAINS` lets a value match an
  array holding it at any depth, so `where_json_contains("metadata", {"tags": "a"})` matched
  `{"tags": ["a", "b"]}` on MySQL only, and `where_json_contained_by` had the mirror case. A type
  check for each path of the document passed makes MySQL and MariaDB answer as PostgreSQL does,
  except below an array, where MySQL's reading stays. On MariaDB, which has no `CAST(.. AS JSON)`,
  these filters and `where_array_*` failed with a syntax error; the document is bound as JSON text
  now.
- **SQLite's JSON key and path tests disagreed with PostgreSQL and MySQL.** A member holding JSON
  `null` counted as missing, so `where_json_key_exists` and `where_json_path_exists` skipped it,
  and a `NULL` column matched `where_json_key_not_exists`, which on the other backends matches
  neither way. SQLite now tests with `json_type` and leaves a `NULL` column unknown.
- **`where_in` with more non-integer values than one statement binds failed** (32,766 on SQLite,
  65,535 elsewhere). Past 1,000 values PostgreSQL takes the list as one array parameter and SQLite
  a text list as one JSON value; MySQL keeps its limit.
- **`insert_all()` of two or more rows failed on MariaDB** with `BackendNotSupported`: it asked the
  engine for a multi-row `INSERT .. RETURNING`, which the engine renders for PostgreSQL and SQLite
  only. MariaDB batches like MySQL now. A batch whose `INSERT .. RETURNING` gives back fewer rows
  than it sent, because a trigger skipped some, is an error instead of a shorter result that no
  longer lines up with the models passed in.
- **MariaDB stored a `DateTime<Utc>` outside its `TIMESTAMP` range as NULL**: before 1970, or after
  2038 (2106 from 11.5), so saving one failed on a `NOT NULL` column and filtering by one matched
  nothing. The driver binds a zoned timestamp as a `TIMESTAMP` parameter, which MariaDB checks
  against that type's range even for a `DATETIME` column. TideORM binds it as the UTC `DATETIME`
  it is stored as, on MySQL too, where the stored value does not change.
- **`update_all().json_set()` failed on MariaDB** with a syntax error on `CAST(.. AS JSON)`; the
  value is read with `JSON_EXTRACT(?, '$')` on both servers.
- **`lag()` and `lead()` with a default failed on MariaDB** with a syntax error: its `LAG` and
  `LEAD` take no default. On MySQL and MariaDB a `CASE` around the call supplies it, exactly where
  no row is that far away, as the three-argument form does; a NULL value one row away stays NULL.
- **Query builder, statements.** `group_by(..).count()` failed on PostgreSQL and MySQL: the counted
  subquery kept the model's columns. `union()`/`union_all()` never validated their operand, so an
  unsafe `order_by` and a deferred error went through, and dropped the operand's own unions and
  CTEs. `distinct()` slipped past the check that `get()` reads every model column.
  `select("*")` over a join filled the model from the joined table. An aggregate of a joined
  column on a limited or distinct query read the model's column of that name, and
  `count_distinct` over a join was ambiguous. `exists()` dropped the select aliases its `HAVING`
  used. Window functions named Rust fields, so a renamed column was not found. A `HAVING`
  template counted a `?` inside a quoted literal as a parameter. `where_in_subquery(q.limit(n))`
  failed on MySQL and MariaDB, which take no `LIMIT` in an `IN` subquery; it reads through a
  derived table. `count()` and `exists()` of a `distinct()` query with an order failed. A statement
  inside a transaction or on a `query_with` handle was written for the global connection's backend.
  The check on raw subqueries refused a function named like a statement keyword (`REPLACE(..)`)
  and a CTE body holding a `UNION`, and a raw `HAVING` refused `LEFT(..)`/`RIGHT(..)`.
- **Query builder, filters.** `has_related()` and `has_no_related()` bound a `None` value as
  `= NULL`, which is never true, so `has_no_related(.., None).delete()` deleted every row; it means
  `IS NULL` now. `where_not(col, None)` alone passed the guard against unfiltered writes although it
  keeps every row of a `NOT NULL` column. `where_array_contained_by` matched a NULL array and an
  array holding a NULL on SQLite, and a NULL in the list made every array match on PostgreSQL and
  SQLite; a NULL in the list now allows NULL elements. A filter on a joined table's column —
  `where_eq("users.token", uuid)`, `where_gt("u.created_at", ts)` through an alias — bound its
  value as text, which PostgreSQL refused (`uuid = text`) and MySQL and SQLite, which store a UUID
  as bytes, matched nothing on; it is bound as the column's type whenever a model maps that table.
  A typed column of another model named the query's own column of that name:
  `Post::query().where_eq(User::columns.id, 5)` filtered `posts.id`. A typed column carries its
  model's table now, and a filter, `order_by`, `group_by`, aggregate, window, `pluck` or IN
  subquery on another model's column is written `users.id`, which fails loudly unless the query
  joins that table. `has_related`/`has_no_related` bound their value as text and refused a `Uuid` or a
  timestamp; they take any `Serialize` value and bind it as the related column's type.
  `where_eq`, `where_not`, `where_in` and `where_not_in` on a JSON column failed on PostgreSQL, whose
  `json` type has no `=`, and never matched on MySQL, which compared the document with a string;
  they compare documents now, ignoring key order and spacing (SQLite compares the minified text).
- **Query builder, results.** `get_json()` decoded a joined column that shares a name with a model
  column as the model's type. `chunk()` over a union repeated rows and never ended, and over a join
  that repeats a key skipped rows (8 of 10); a union is refused, and a join reads every row. The query
  cache keyed entries by table name only, so two models on one table, a model in two schemas, and
  two databases — a replaced global connection, a `query_with` handle — shared cached rows; keys
  carry the model type, schema and connection now. Cached rows went through the model's serde, so
  a `#[serde(skip)]` column came back empty and relations came back unusable. Reads through
  `select_raw()`, `having()` and raw orders were never invalidated by writes to their tables.
  `insert_all()` left the cache stale, and `execute_returning()` invalidated it only once its rows
  decoded.
- **JSON arrays.** `array_remove()` turned the whole array NULL on MySQL and MariaDB when the value
  was absent, never found a number or boolean, and read `%` and `_` as wildcards; on SQLite it
  turned booleans into `1`/`0`, dropped nulls and turned a NULL column into `[]`. `array_append()`
  on SQLite stored an object or array as a string.
- **Validation.** `min`, `max` and `range` compared integers through `f64`, so past 2^53 a value
  one past the bound passed. Integers are compared exactly.
- **Serialization.** `to_json` put an attachment back when it was hidden, and
  `to_translated_json` read a `translations` key instead of the model's translations. A
  highlighted snippet with `fragment_words: Some(0)` left the match out.
- **Dirty tracking** kept one snapshot for rows of two databases that share a key, so a model
  loaded from one could save against the other's baseline.
- **Entity manager.** A flush cancelled part way, or rolled back with a transaction around it
  after it had succeeded, left the context holding ids and clean snapshots for rows never
  committed, so the next flush skipped them. The context is restored to its state before the
  flush in both cases, as it already was when the flush failed.
- **Migrations and seeding.** A `Migrator::run()` cancelled on MySQL or MariaDB left the named
  migration lock held by a pooled connection, so every other migrator waited 300 s and failed; the
  lock lives on a connection of its own now, which closes with the run. Seeding inside a
  transaction read and created its ledger outside it: a second run in the same transaction ran
  every seed again, and a one-connection SQLite pool waited forever for a second connection.
  `change_column` on MySQL and MariaDB made the column nullable and dropped its default,
  `AUTO_INCREMENT` and comment, since `MODIFY COLUMN` restates the whole column; it reads them
  from `SHOW CREATE TABLE` and keeps them, as PostgreSQL's `ALTER COLUMN .. TYPE` does.
- **Field names with a digit before a letter** (`s3key`, `md5sum`) failed to compile: SeaORM's
  derives named the generated column variant `S3key` while TideORM named it `S3Key`. A field named
  `self_` no longer produces the variant `Self`.
- **Upserts.** One with nothing to update — every column a conflict column, or
  `update_columns(vec![])` — failed with "None of the records are inserted" on a conflict, and on
  MySQL and MariaDB rendered `ON DUPLICATE KEY IGNORE`, which is not SQL, so even the first insert
  failed; it keeps the stored row now. On MySQL and MariaDB an upsert whose update changed no value
  failed the same way, since the server reports no affected row. `update_all_except` compared
  column names only, so a field name of a renamed column left it overwritten; an unknown name is
  an error.
- **Transactions.** On PostgreSQL a closure that caught a failed statement and returned `Ok`
  reported a commit, while the server, which aborts the transaction at the failure, rolled it
  back; `transaction()` now returns `Error::Transaction` and rolls back, and the entity manager's
  state is restored with it. The other backends keep the transaction going after a failed
  statement, as before. After the explicit rollback of a leaked transaction, statements through
  the stray handle committed one by one; a transaction is reopened for them, which the handle's
  drop rolls back.
- **Eager loading.** `with(..)` matched a parent's key with its children's by exact type, so an
  `i64` id and an `i32` foreign key (or an integer owner id and a text `*_id` column of a
  `MorphMany`) loaded no children while `load()` found them. Keys match by value now. A pair the
  pivot table held twice loaded its related row twice through `with(..)` of a `HasManyThrough`.
- **Relations and attachments.** `MorphTo::load_as` looked the owner up by its primary key even
  when the owner's `MorphOne`/`MorphMany` keyed its children by a `local_key`, and by the first
  column of a composite key. Replacing a `has_one` child through the entity manager inserted the
  new row before deleting the old one, which a unique foreign key refused. `detach(relation,
  Some(key))` on a has-one attachment cleared it whatever file it held. Attachment metadata named
  like a field (`size`, `key`) could not be read back, so `get_file()` returned `None`, or
  overwrote the field; an entry named like an unset optional field round-trips, and one named
  like a set field is not stored.
- **Queries.** `soft_delete()` under `with_trashed()` re-stamped rows already trashed, pushing
  back a retention purge that reads the stamp; it marks live rows only, and refuses
  `only_trashed()`. A delete or update whose one filter is `where_json_contains(col, json!({}))`
  (or `[]`), which every object (or array) document contains, passed the filter guard and wrote
  the whole table. An index naming a field of a renamed column (`#[index("display_name")]`) was
  created on a column of that name, which does not exist.
- **Caching.** A raw batch that begins with `SELECT` and goes on to write (`SELECT 1; DELETE ..`)
  left the cache in place, and a cached read's tags missed the second table of `FROM a, b`, so a
  write to it left the entry stale.
- **Schema sync, migrations and the schema file.** On SQLite, sync failed adding a column with a
  current-time default to a table that has rows ("Cannot add a column with non-constant
  default"); it adds the column without the default and warns. A migration string default kept
  its backslashes unescaped on MySQL and MariaDB, where `C:\temp\` broke the statement and
  `domain\user` stored `domainuser`. A current-time default spelled `now()` failed on MySQL
  (`DATETIME(6)` rejects a less precise default) and on SQLite; `now()`, `CURRENT_TIMESTAMP()`
  and `LOCALTIMESTAMP` now render as each backend accepts them. On MySQL a re-run of
  `create_table_if_not_exists` failed on the index it had already made. The schema file wrote a
  MySQL string default bare (`DEFAULT draft`), an expression default without its parentheses,
  a FULLTEXT or prefix index as a plain index, an index with an expression key part with a
  column missing, PostgreSQL's expression, partial and GIN indexes not at all, an identity column
  as a plain one, and SQLite's FTS5 table and its shadow tables as ordinary tables; each is
  written as the catalog declares it now. A sync warning carried a run of spaces from a lost
  line continuation.
- **Validation.** `#[validate(min = 0.7)]` refused `0.7_f32`, which is `0.699999988` as an `f64`;
  `numeric` accepted `NaN`, `inf` and `1e999`.
- **Encrypted fields.** `pluck`, `value`, `get_json` and `get_as` returned the ciphertext of an
  encrypted column. `Hashed`'s `Debug` printed the Argon2 hash its `Display` and `Serialize`
  hide, and `from_token`'s not-found error named the decoded key a token exists to hide.
- **Relations.** `query_with(db).with(..)` loaded the relations from the global database. An
  owner whose nullable `local_key` was NULL loaded every related row whose foreign key is NULL,
  counted them, and had `attach()` insert `(NULL, id)` pivot rows and `sync()` delete them. A
  `SelfRef` and `SelfRefMany::load_tree` of a model whose key is not named `id` queried an `id`
  column. `HasManyThrough` read a pivot key naming a renamed column's field as a column of that
  name, and its eager load failed on PostgreSQL when the pivot key's integer type differed from
  the owner's; it loads like the other relations now, matching keys by value. Its `load()` and
  `count()` grouped by the key beside `SELECT related.*`, which MariaDB refuses under
  `ONLY_FULL_GROUP_BY`; they read the rows through a semi-join. Eager loading compared each key
  with every other to drop duplicates, quadratic in the parents.
- **Models.** A batch `set()` bound a JSON document, bytes and a PostgreSQL array as JSON text:
  PostgreSQL refused a `jsonb` column set so, and SQLite and MySQL stored the bytes' JSON text in
  the BLOB. `find_with(id, db)` recorded its dirty-tracking baseline under the global database's
  pool. `insert_all` discarded every dirty-tracking baseline of the model, the new rows' among
  them. The generated `Deserialize` read a sequence (bincode, postcard) in another order than
  `Serialize` writes one, relations among the columns; relations now follow the columns in both,
  and a non-self-describing format gets every relation, as `None` when none is cached.
- **Entity manager.** A new root read from JSON, whose relations were built under its
  placeholder key `0`, was saved without its children. `persist()` onto an entity the context
  was removing kept the removal: the row was deleted and the new values never written. A
  managed entity whose row another path of the context saved flushed the whole row it had
  loaded back over the newer values; it moves onto what was stored, keeping its own edits. An
  edit to a field the model's own serde derive skips was not saved through a relation, whose
  change check compared JSON.
- **Queries.** `chunk()` of a join with a `limit()` failed on its last batch, and `chunk(1)` of any
  join on its first, taking a batch the limit cut short for a key repeated past the chunk size.
  `aggregates()` decoded a second `MIN`/`MAX` of one column by the driver's type (a MySQL
  timestamp without its zone, a SQLite boolean as `1`), and `sum`, `avg` and `count_distinct`
  of a joined column by that column's type (a count read as `true`). `pluck` and `value` on a
  union rewrote only its first query's projection; a union's `order_by` wrote a joined query's
  column as `table.column`, which PostgreSQL and MySQL refuse there; `count()` of a join grouped
  by two columns of one name failed on MySQL. A raw read of `FROM ONLY posts` was not tagged
  with `posts`, so a write to it left the cached read in place.
- **Connections, caching and logging.** `connect_timeout` did nothing: SeaORM maps it and
  `acquire_timeout` onto sqlx's one checkout timeout, and `acquire_timeout` won; the longer of the
  two applies now. URL masking left a `?password=` query parameter visible. A cache TTL too long
  for an `Instant`, such as `Duration::MAX`, expired the entry at once. The `QueryLogger`
  dropped a failure's error at `Debug` level and when the failed statement was also slow.
  Migration statements never reached the `QueryLogger`. A raw JSON row gave `null` for a
  non-NULL value no decoder took, such as a PostgreSQL `NUMERIC` past 28 digits or an enum; it
  gives the value's text, a `NUMERIC` exactly. Raw SQL write detection missed
  `EXPLAIN ANALYZE DELETE ..`, which runs the delete, and a MySQL literal written with `\'`,
  which hid a second statement from the scan, so neither flushed the cache.

### Changed — Breaking

- `ConditionValue` has `RawTemplate`, `Column` and `Invalid` variants — the last holds a value no
  SQL comparison takes, such as a NaN — so an exhaustive `match` on it needs arms, and it
  implements `PartialEq`. `ColumnCondition::value` is a `ConditionValue` rather than JSON, so
  a typed condition carries its list, range or refusal as the builders do; `ColumnIn::is_in`/`not_in` take `impl IntoIterator<Item = T>`, so an
  implementation outside TideORM changes its signature. Calls are unaffected.
- `DefaultValue::to_sql` takes the `DatabaseType` to render for, since a string default is
  escaped differently on MySQL and MariaDB.
- The crate root re-exports the whole prelude, plus `Result`, `chrono`, `async_trait` and
  `inventory`, so `tideorm::X` and `tideorm::prelude::X` can no longer drift apart.
- `profiling::GlobalStats` is a type alias of `QueryStats`, so `GlobalProfiler::stats()` prints in
  the `QueryStats` format; `QueryCache::config()` and `PreparedStatementCache::config()` return the
  configuration instead of an always-`Some` option.

### Changed

- **Model queries name their columns.** The default projection is the model's column list instead
  of `table.*`, so an `ALTER TABLE .. ADD COLUMN` under a running application no longer breaks
  every cached model query (PostgreSQL's "cached plan must not change result type", SQLite's stale
  column list), and `get_json()` returns the model's columns only. A `union_raw()` query keeps
  `table.*` so its operand still lines up.
- **New MySQL and MariaDB tables use types that hold Rust's values.** Timestamps are `DATETIME(6)`
  (`TIMESTAMP` spans only 1970–2038, and neither kept microseconds), times `TIME(6)`, text and
  binary `LONGTEXT`/`LONGBLOB` (`TEXT` and `BLOB` stop at 64 KB), and tables are declared
  `DEFAULT CHARSET=utf8mb4`, so a latin1 database default no longer rejects `日本語` or emoji.
  `CURRENT_TIMESTAMP` defaults become `CURRENT_TIMESTAMP(6)`, and defaults on text, JSON, binary and
  array columns (JSON there) are the parenthesized expressions MySQL requires. Existing tables are
  untouched.
- **Schema sync creates the indexes a model declares** (`#[index]`, `#[unique_index]`); it used
  to create the table alone. On MySQL a keyed or indexed string column is a `VARCHAR(255)`.
- **`Model::paginate()` orders by primary key.** Without an order, PostgreSQL returned rows in heap
  order, which an `UPDATE` changes, so consecutive pages repeated one row and skipped another.
- **SQLite writes take one statement.** The `sqlite` feature turns on the engine's `RETURNING`
  support, which the bundled SQLite (3.46 or later) has, so `save()`, `create()` and `update()` get
  the stored row back from the write itself instead of a second `SELECT`, in about half the time.
  `insert_all()` of a model keyed by one auto-increment column sends multi-row
  `INSERT .. RETURNING` statements, split at SQLite's 32,766 bind parameters, instead of one insert
  per row, and puts the rows back in input order: 1,000 rows take one statement instead of 2,000,
  about 12 times faster.
- **Single-row reads keep their prepared statement on current SQLite.** `find()`, `find_with()`,
  `exists()`, `find_or_fail()`, `reload()`, `first()`, `last()`, `count()`, `exists_any()` and the
  upsert reload bound a `LIMIT` parameter, and SQLite 3.50 and later — what a fresh build gets, as
  sqlx accepts libsqlite3-sys up to 0.37 (SQLite 3.51) — recompiles a statement with a bound
  `LIMIT` every time it runs. Key lookups and `count()` need no limit, the others write `LIMIT 1`
  into the SQL, and `Model::paginate()` writes its page size and binds the offset, so one statement
  serves every page: 15–30% faster per call on SQLite, and unchanged elsewhere.
- Query rendering allocates less (identifiers and column lists are quoted in place), and generated
  models build an error's context only when an error occurs. `TIDE_LOG_QUERIES` is read once, when
  the first statement runs, like `TIDE_LOG_LEVEL`, instead of before every statement.
- `DatabaseType::{supports_json, supports_upsert, supports_window_functions, supports_cte}` are
  documented as always true.
- **Paging binds its offset.** `limit()`/`offset()`/`page()` write the page size into the SQL and
  bind the offset, so every page of a query shares one prepared statement on PostgreSQL and MySQL
  instead of preparing one per page, and full-text search does the same; a bound `LIMIT` is what
  SQLite 3.50+ recompiles on every run.
- **`insert_all()` batches models that set their own keys** on SQLite, MySQL and MariaDB. A key the
  model sets is known before the insert, so the rows SQLite's `INSERT .. RETURNING` gives back are
  matched to their models by key, and MySQL and MariaDB insert an integer- or `Uuid`-keyed batch in
  statements of up to 1 MB of values, reading each one's rows back with one `SELECT`. 1,000
  UUID-keyed rows on SQLite take one statement instead of 1,000. A batch that repeats a key is still
  inserted a row at a time, so the database's own constraint decides it.
- `get_json()` works out how to decode each column once per result instead of once per row.
- **`having_sum_gt` compares `COALESCE(SUM(..), 0)`**, as `Aggregate::sum` and `sum()` read a
  sum, so a group whose values are all NULL counts as 0 instead of failing every comparison.
- **`get()` does not cache a model whose own serde does not read back what it writes** (a
  `#[serde(skip)]` field, `with`/`serialize_with`, a container `from`/`into`), since the cached copy
  would come back without those columns.
- **The MySQL and MariaDB migration lock takes a connection of its own**, closed after each run
  instead of returned to the pool; the pool still needs a second connection for the migrations.

### Internal

- One `ConnectionRef::executor()` replaces the copy-pasted connection-or-transaction `match`
  blocks across the runtime and the generated code; the `where_*` builder family is generated once
  for every builder; migrations and seeding share one DDL renderer and one ledger; the generated
  code per model shrank by 10–27%.
- The SQLite, MySQL and PostgreSQL integration suites share one body per scenario, as do the three
  entity-manager suites, so upserts, transactions, `insert_all`/`update_all`, restore and
  `force_delete` — and all 28 entity-manager scenarios — now run on every backend. Tests that
  asserted nothing were deleted. `seaorm2_features_tests` is now `typed_columns_and_join_tests`;
  `query_builder_tests` and `entity_manager_tests` were folded into other targets. A backend that
  is enabled but unreachable now fails the run instead of reporting green, and `SKIP_MYSQL_TESTS`
  (documented, never read) works.
- The shared integration scenarios cover typed values, JSON rows, bind-parameter limits, bulk
  validation, naive timestamps, paging order, JSON containment, integer scaling and a column added
  under a running query, on all three backends.
- Three fewer dependencies, with no change in behaviour. The global connection slot is a
  `parking_lot::RwLock` instead of `arc-swap`. Token and encrypted-field nonces come from
  `chacha20poly1305`'s own `OsRng` instead of `rand`, in the same format, so existing tokens and
  ciphertexts still decode. `tideorm-macros` derives names with its own function instead of
  `convert_case`, keeping the same word boundaries over the same grapheme clusters, so no table or
  column name changes. `arc-swap`, `rustversion` and `convert_case` leave every build; `rand` and
  the crates only it needed leave SQLite- and MySQL-only builds (sqlx's PostgreSQL driver still
  uses `rand`).
- The PostgreSQL test suites and benchmarks are opt-in, like the MySQL ones: they run when
  `POSTGRESQL_DATABASE_URL`, `TEST_DATABASE_URL` or `RUN_POSTGRES_TESTS` is set, so a bare
  `cargo test` or `cargo bench` passes without a server; an enabled but unreachable one still
  fails the run.
- CI runs the live-database suites, the integration, advanced and entity-manager ones, against
  PostgreSQL, MySQL and MariaDB 10.11 and 12.3 service containers, and runs the SQLite suites
  instead of only compiling them. The MariaDB suites (`mariadb_integration_tests`,
  `mariadb_entity_manager_tests`) are new and opt in through `MARIADB_DATABASE_URL` or
  `RUN_MARIADB_TESTS`; they run the shared scenarios, and the MySQL-family ones both servers share.
- The lockfile takes libsqlite3-sys 0.37 (SQLite 3.51), what a fresh build of TideORM resolves, so
  the tests run the SQLite users get, and replaces the yanked `chacha20` 0.10.0 and `spin` 0.9.8.
- docs.rs documents the six module features, which it used to leave out.
- `cargo test` runs from both published tarballs: `tideorm-macros` ships the unit tests its
  library includes, and `tideorm` ships `.cargo/config.toml`, which runs its tests on one thread.
- CI also runs the unit tests that need the SQLite driver together with a module feature, and the
  public API's feature-gated tests, which no job built before.

## [0.11.0] - 2026-08-26

Tracks the stable SeaORM 2.x line. The upgrade itself needs no code change; what moves the minor
version is the toolchain floor it brings with it.

### Changed

- **SeaORM now tracks stable `2.x`** as a caret range (`2.0.2`) instead of the exact pin
  `=2.0.0-rc.38`. Every 2.0.0 breaking change had already landed during the release-candidate
  series, so TideORM needed no source edit: the raw-SQL split (`query_all_raw` / `query_one_raw` /
  `execute_raw`), the `DatabaseConnection` enum-to-struct refactor, and the `Arc`-wrapped
  `RuntimeErr::SqlxError` were all present in rc.38 already. The exact pin existed because a
  resolver bump *within* the RC series could surface as errors inside macro expansion in downstream
  crates; inside a stable major that is precisely what semver rules out, so the pin is now a caret.
- **Minimum supported Rust version raised from 1.88 to 1.94.** This is not a preference. Every
  stable `sea-orm 2.x` declares `rust-version = "1.94.0"`, as do `sqlx` 0.9 and `sea-query-sqlx`
  0.9, and `resolver = "3"` *enforces* a dependency's declared MSRV rather than warning about it —
  so there is no stable SeaORM that builds on 1.88. The floor is the dependency tree's, not ours.
  This is what moves the minor version rather than the patch: shipped as a patch it would pull every
  existing `tideorm = "0.10"` dependent onto the new floor on their next `cargo update`, with a hard
  resolver error and no opt-in.

### Upgrade notes

- **A large `async fn` may now need `#![recursion_limit = "256"]`.** rustc computes the layout of an
  async fn body as one generator type, and SeaORM 2.x (through sqlx 0.9) nests deeply enough that a
  function awaiting a long chain of TideORM calls can cross the default limit of 128. It surfaces as
  `error: queries overflow the depth limit!` naming `computing layout of {async fn body of ...}`,
  which reads like a fault in your own code and is not one. Add the attribute to the crate root of
  the crate that fails; the limit is per-crate and does not inherit from a dependency. `tideorm-cli`
  hit exactly this and carries the attribute now. Scaffolded projects and the example suite do not.

### Fixed

- The README badge and `CONTRIBUTING.md` still advertised `rust-1.85`, stale since 0.10.1 raised the
  real floor to 1.88 without updating them — the crate documented a minimum two releases below the
  one it actually required. `CONTRIBUTING.md` now lists all five places an MSRV bump has to touch
  (it previously listed three, which is how both it and the README badge were missed) and records
  that raising `rust-version` also raises the floor clippy lints against, so a bump is not
  lint-neutral and needs a clippy re-run.

## [0.10.2] - 2026-08-25

Three defects behind the `entity-manager` feature and the schema type mapper.
Nothing here affects a default-feature build.

### Added

- `TideEntityManagerMeta::tide_pk_is_new`, reporting whether an entity's primary key is still the
  type's default. It defaults to `false`, so hand-written implementations are unaffected; the derive
  emits it from `ModelMeta::primary_key_is_new`.

### Fixed

- **Two unsaved entities registered with an entity manager aliased each other.** `tide_pk_key` is
  infallible and has no notion of "unsaved", so it renders a default primary key as an ordinary
  string — `"0"` for an `i64`. `register` and `put` filed entities under it, so the second new
  instance of a model collided with the first and was handed the first one back. A `HasMany` holding
  two new children silently dropped one and inserted the other twice. Both now return early for an
  entity whose primary key is still new: it has no identity to share until the insert assigns one.
- **`persist` left an identity-map entry that nothing could remove.** `persisted_key` answered two
  different questions — whether a row exists in the database, and which key the entry is filed under
  in the identity map. Those diverge for an entity given to `persist` with a client-assigned primary
  key: trackable immediately, but not yet inserted. All three removal paths keyed off
  `persisted_key`, so `detach` silently did nothing and `remove` + flush left a `find_managed`
  returning a row that was never written. The map key is tracked separately now, while the `DELETE`
  stays gated on whether a row actually exists.
- **`Text` and `JsonArray` could not be used as model field types.** Both are exported from
  `tideorm::types` and documented for model fields, and `canonical_schema_type` already recognised
  the names, but neither had an arm in the `ColumnType` match — so both fell through to the
  catch-all and failed to compile. `Vec<serde_json::Value>` is accepted for `JsonArray` too.

## [0.10.1] - 2026-08-25

### Changed

- **The declared MSRV is now `1.88`, up from `1.85`.** This corrects a promise the crate could not
  keep rather than dropping support for anything: `darling 0.23` and `sea-query 1.0.0-rc.33` both
  require 1.88, so 0.10.0 already failed to build on 1.85 despite what its manifest said. 1.88 is
  the exact ceiling of the dependency tree, not a margin.

### Fixed

- **`Database::backend()` answered for the ambient transaction instead of the handle it was called
  on.** `__internal_backend` resolved through `current_handle`, so a `Database` with its own
  connection reported whatever transaction happened to be open around it — and a handle with no
  connection of its own resolved to the *global* slot, which an `EntityManager` created without a
  global connection has never set. That failed, `backend()` warned and guessed PostgreSQL, and a
  SQLite-backed manager then rendered `$1` placeholders. It reads `own_handle` now, which is what
  the method's own documentation and `Database::backend()`'s public contract both describe.

### Internal

- The `features` CI job runs `cargo test --lib`, and the entity-manager relation tests under
  `tests/unit/` are `#[path]`-included into that suite and speak real PostgreSQL. With no server
  they did not skip — each waited out the connection-pool timeout and failed 30 seconds later. That
  job now has a PostgreSQL service. The rest of `cargo test --lib` remains database-free.
- Local verification now covers every configuration CI runs, including
  `--no-default-features --features sqlite,runtime-tokio`. The `Database::backend()` defect above
  only compiled under that combination, so no local gate had been running the test that caught it.

## [0.10.0] - 2026-07-25

This is a deliberately breaking release. A repository-wide audit produced a large batch of
correctness fixes, and several of them could not be made without changing public API. Dead or
misleading surface was removed outright rather than deprecated. Read the two breaking sections
before upgrading; everything else is a behavior fix that should only ever move you from wrong
results to right ones.

### Upgrading — Action Required

- **MySQL/MariaDB `uuid` columns need an `ALTER TABLE`.** Earlier versions created them as
  `CHAR(36)`; the correct type is `BINARY(16)`, because `sqlx-mysql` binds a `Uuid` as 16 raw bytes
  and its decoder rejects anything else. New tables get `BINARY(16)` automatically, but **existing
  tables are not migrated**: schema sync only adds missing columns, it never changes the type of an
  existing one. A table left at `CHAR(36)` will fail every insert of a `Uuid` value with
  `1366 Incorrect string value`. Convert each affected column once:

  ```sql
  -- back up first; this rewrites the stored representation
  ALTER TABLE your_table
    MODIFY external_id BINARY(16);
  ```

  If the column already holds hyphenated text, convert the data in the same statement with
  `UNHEX(REPLACE(external_id, '-', ''))` before narrowing the type. PostgreSQL (`UUID`) and SQLite
  (`TEXT`) are unaffected.

- **SQLite `Decimal`/`Numeric` columns are `REAL` again.** A pre-release build of 0.10.0 briefly
  emitted `TEXT`, which is lossless but unreadable — sea-orm decodes `Decimal` on SQLite through
  `f64`. If you created tables with such a build, change those columns back to `REAL`. Released
  versions before 0.10.0 already used `REAL` and need no action.

### Removed — Breaking

- Removed `Encrypted<T>` (`tideorm::types::Encrypted`, also re-exported from the prelude). It
  encrypted at the serde layer under a single process-wide key with no per-column context, so a
  ciphertext lifted out of any `Encrypted<T>` column decrypted cleanly in any other one — column
  identity was not part of the sealed payload. Use the `encrypted-fields` feature and the
  derive-level `#[tideorm(encrypted = "...")]` attribute instead, which derives a distinct key per
  `(table, column)` and stores plain Rust types on the model. A field declared as
  `pub secret: Encrypted<String>` becomes `pub secret: String` with `secret` named in the model's
  `encrypted = "..."` list. There is no in-place upgrade for existing data: decrypt with 0.9.x,
  then re-save under 0.10.0.
- Removed six `ModelMeta` methods:
  - `tokenization_enabled`, `token_encoder`, `token_decoder` — these collided with the identically
    named `Tokenizable` methods, so `Model::tokenization_enabled()` on a tokenizable model was an
    E0034 "multiple applicable items in scope" ambiguity that no call site could resolve without a
    fully qualified path. The `Tokenizable` methods are the surviving ones and are unchanged.
  - `default_order`, `option_set_label`, `option_set_search_fields` — no `#[tideorm(..)]` attribute
    ever set them and nothing in the crate ever read them; they always returned their defaults.
  If you implemented `ModelMeta` by hand, delete those six methods. Macro-generated models need no
  change beyond recompiling against the matching `tideorm-macros`.
- Removed `Model::has_dirty_baseline()` (feature `dirty-tracking`). It is redundant now that
  `changed_fields()` and `original_value()` report the missing-baseline case themselves.

### Changed — Breaking

- `Error` variants now carry a structured source instead of a flattened message string. SQLSTATE
  codes, constraint names, and the underlying driver error survive to the caller, and
  `Error::suggestion()` together with the `is_*` classifiers (`is_unique_violation`,
  `is_foreign_key_violation`, and friends) now inspect that structured data instead of
  substring-matching driver text — which was locale- and backend-dependent and misclassified any
  message that happened to contain a keyword. Added structured variants for migration failures and
  for access-denied/authentication errors, which previously collapsed into generic connection
  errors. Code that matched on `Error` variants exhaustively, or that formatted a variant's payload
  directly, needs updating; code that only uses `Display`, `?`, and the classifiers does not.
- `Model::changed_fields()` and `Model::original_value()` (feature `dirty-tracking`) now
  distinguish "no baseline was ever recorded" from "a baseline exists and nothing changed". A model
  that was never loaded or saved through TideORM no longer reports itself as clean. Callers that
  treated an empty change set as proof of a clean persisted row must handle the no-baseline case.
- `EntityManager::snapshot`, `deletions`, and `additions` (feature `entity-manager`) are no longer
  `async`. They only read in-memory persistence-context state and never touched the database. Drop
  the `.await`.
- `RelationInfo` no longer overloads `morph_type_column` to smuggle a through-relation's related
  key. The field now means only what its name says; the through-relation key is carried in its own
  field. This is visible to anything constructing or reading `RelationInfo` directly.

### Fixed

Roughly 160 individual defects from the audit. Grouped by theme:

- **Destructive mutations.** An `update_all()`/`delete_all()` chain whose filters were all dropped
  or unresolvable could render an unfiltered `DELETE`/`UPDATE`. The explicit-filter guard now
  covers those paths, so a whole-table mutation must be asked for explicitly.

  A filter that *renders* constant-true counts for nothing. An empty candidate set for a negative
  membership test (`where_not_in(col, [])`, `ne_all(col, [])`) or for `array_contains` matches every
  row, and a caller reaches that by accident whenever a filter list comes back empty from a form or
  an upstream query. `delete`, `force_delete`, `soft_delete`, `restore` and `update_all` all reject
  it now. The check is structural — on the operator and its operand — because the rendered SQL
  cannot be pattern-matched for it: sea-query emits an empty `NOT IN` as the bound pair `? = ?`
  rather than the literal `1 = 1`, and a soft-delete model appends its own `deleted_at IS NULL`
  conjunct to whatever the caller declared. One real predicate alongside a vacuous one is still
  enough, and the positive duals (`IN ()`, `= ANY ()`, `&& ()`) stay accepted because they render
  constant-*false* and so match nothing.
- **SQL parameterization.** Subquery, `EXISTS`/`NOT EXISTS`, `IN (subquery)`, `UNION`, CTE, and
  PostgreSQL array operands were rendered with inlined literals rather than bound parameters.
  They are now parameterized end to end, with the placeholder numbering carried correctly across
  composed fragments.

  Parameterizing the PostgreSQL array operators meant dropping the `ARRAY[..]` constructor, since
  sea-query's fragment tokenizer treats `[` as a string delimiter and never substitutes a
  placeholder inside one. `where_array_contained_by` therefore renders as `NOT EXISTS` over
  `unnest(column)`, which needs two explicit NULL guards to keep matching what `<@` matched:
  `unnest(NULL)` yields no rows (so a bare `NOT EXISTS` is true for a NULL column), and a NULL
  element makes `element NOT IN (..)` unknown rather than true (so the offending row goes
  uncounted). Both are in place; the rewrite agrees with native `<@` on NULL columns, NULL
  elements, and empty arrays.
- **`ORDER BY` / `GROUP BY`.** Both are now validated against the model's resolvable columns
  instead of being pasted through. `QueryBuilder::order_by_raw(expr, direction)` is the new,
  explicitly-trusted escape hatch for real SQL expressions — never pass user input to it.
- **Raw `WHERE` fragments.** Raw conditions are wrapped in parentheses before being combined, so a
  fragment containing a top-level `OR` can no longer swallow the surrounding filters and widen the
  result set.
- **Lifecycle callbacks.** `before_save`/`after_save` and friends now run for models persisted
  through a nested save rather than only for the root model.
- **Aggregates.** `count`, `sum`, `avg`, `min`, and `max` now honour the query's joins, CTEs, and
  `limit`, and reject a scalar terminal on a grouped query instead of silently returning the first
  group's value.
- **Eager loading.** Eager loads are soft-delete-scoped like every other read, and nested eager
  paths batch their queries instead of degrading into an N+1 loop.
- **Soft delete.** `restore()` and `force_delete()` work; both previously failed or no-opped
  depending on the path taken.
- **`LIKE` escaping.** The escape character changed from `\` to `!`, and generated `LIKE` clauses
  emit an explicit `ESCAPE '!'`. Backslash is a literal in some backends and an escape in others
  with `NO_BACKSLASH_ESCAPES` off; `!` behaves identically everywhere and keeps generated SQL free
  of backslashes. `%`, `_`, and `!` in a user-supplied `where_like`/`contains`/`starts_with`/
  `ends_with` operand are escaped for you.
- **Migrations.** Migration runs take a database advisory lock, so two processes starting at once
  no longer both apply the same migration.
- **Seeding.** Seeding works on PostgreSQL.
- **A raw-identifier field declared `encrypted` was written in plaintext.** The attribute's field
  list is stored un-raw'd (`type` for a `r#type` field) while five call sites compared the raw
  `r#type`. They never matched, so the generated setters took the plaintext branch and the column
  was never encrypted — silently, with no error. Reads did not decrypt either, while the batch
  `update_all()` path resolved against the un-raw'd list and *did* encrypt, so rows written both
  ways disagreed.
- **`Json` and `DateTime` could not be named in a model.** The generated entity module glob-imported
  both the user's module and sea-orm's entity prelude, and both export those names, so a field typed
  `Option<Json>` or `DateTime<Utc>` — the spellings this project's own docs use — raised an
  ambiguous-glob error (rust-lang/rust#114095, a future hard error). The module now imports what it
  needs from that prelude by name, leaving the user's `super::*` as the only glob in scope.
- **`ModelMeta::relation_payload_filters`** spelled its function-pointer type longhand through
  `::serde_json::Value`. It uses the crate's own `RelationPayloadFilter` alias instead, which is now
  re-exported from `tideorm::model` as the trait's signature always implied.

  Note that generated models still require `serde` and `serde_json` as direct dependencies of the
  consuming crate; 0.9.11 removed that requirement only for the `json!` macro in relation state
  helpers. `tideorm init` writes both into a scaffolded project, and the README now states the
  contract for hand-written ones.

### Internal

- CI now compiles and tests all six module feature flags (`attachments`, `translations`,
  `fulltext`, `entity-manager`, `dirty-tracking`, `encrypted-fields`), runs the `tideorm-macros`
  crate's own tests, and runs the DB-free integration targets that were previously built but never
  executed. Feature-gated code is no longer invisible to CI.
- The four entity-manager test targets (`entity_manager_tests`, and the PostgreSQL, SQLite, and
  MySQL variants) no longer report a vacuous green. Their inner `#![cfg(feature = "entity-manager")]`
  used to compile them to zero tests under the default feature set, which cargo reported as a
  passing "0 passed" run; `required-features` stanzas now make the skip visible and CI enables the
  feature so they actually run.
- Refreshed the main crate version, macro-crate version, macro-crate dependency version, README,
  and mdBook chapters to `0.10.0`.
- The `HasAttachments` and `HasTranslations` rustdoc asserted both halves of a contradiction:
  generating those impls from the derive was tried, reverted, and the correction was added without
  removing the original claim. Only the accurate half remains — you implement them yourself.
- `docs/models.md` showed the pre-0.10.0 dirty-tracking signatures. `changed_fields()` and
  `original_value()` return an outer `Option` distinguishing "no baseline" from "unchanged", and the
  example now uses it.
- The README states what a consuming crate needs beyond `tideorm` itself: `serde`, `serde_json`, and
  a locally declared feature for every TideORM feature enabled — a derive's `#[cfg(feature = ..)]`
  is evaluated against the *downstream* crate, so an undeclared one silently selects the
  feature-off branch and warns `unexpected cfg condition value`.
- As with every release, `cargo package`/`cargo publish` for `tideorm` stays blocked until
  `tideorm-macros 0.10.0` is on crates.io, because packaging strips the `path` dependency and
  resolves through the registry index.

## [0.9.19] - 2026-07-18

### Removed

- Removed unused public API that carried no behavior and had no implementors, constructors, or call sites: the `Collection`, `CommaSeparated`, `DbEnum`, `WithDefault`, `Accessor`, `Mutator`, and `AttributeCaster` items from `tideorm::types` (and the prelude); `RelationLoader` from `tideorm::relations`; `CacheWarmer` from `tideorm::cache`; and `CacheOptions::tags` together with its `with_tag`/`with_tags` builders (tags were stored but never read, and no tag-based cache invalidation exists). The live `CastType::Collection` and `CastType::CommaSeparated` cast variants are unaffected. This is the only source-visible breaking change; if you referenced any of these directly, drop the reference.

### Internal

- Collapsed duplicated code with no behavior change. The 17-method `where_*` condition-builder family that `OrGroup` and `OrBranch` implemented byte-for-byte identically now comes from a single declarative macro, and `QueryBuilder::or_where_*` delegate to `OrGroup` instead of re-inlining each condition literal. In the macro crate, the relation-wrapper field-init/state-refresh pair, the entity relation-definition key resolution, the generated `Model::find`/`find_with` bodies, and the entity-manager `should_persist` check are each now single-source; `save_with_one` reuses the existing `apply_foreign_key` helper.
- Removed dead code and inert plumbing the compiler could not surface: six never-read `auto_*` model attribute options in the macro crate, a discarded `exists` parameter on the JSON-path predicate helper, redundant `#[allow(unused_imports)]` markers, and self-restating step comments. Simplified the internal SeaORM-facade so the shared SQL-safety validators are re-exported directly instead of through pass-through wrapper functions.
- Removed abandoned working-tree scratch (`wip/`, `objsafe/`) and the now-stale `wip/**` entry from the package `exclude` list.
- Verified with `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all --check`, `cargo test --lib` across the PostgreSQL, SQLite, and MySQL feature sets, `cargo test -p tideorm-macros --lib`, the `relation_compile_fail` and `encrypted_fields_feature_compile_fail` trybuild suites, `cargo package -p tideorm-macros --allow-dirty`, `mdbook build`, and by running the full example binary suite against live PostgreSQL and MySQL servers.

## [0.9.18] - 2026-06-07

### Fixed

- Fixed a regression in the scoped connection-override future (`with_connection_override`) so it releases the overridden connection/transaction handle as soon as the wrapped future completes, while still being polled inside the async runtime, instead of retaining it until the wrapper future is dropped. Holding the handle past completion let a pooled database connection be dropped outside any runtime context (for example after the future is moved across threads), which panics with "this functionality requires a Tokio context" under the SQLite backend. This restores the pre-0.9.16 behavior that was lost when the per-runtime `tokio::task_local!` and thread-local override paths were unified into a single thread-local implementation.
- Replaced the email-validation fallback regex's unsupported `(?!)` look-ahead with the never-matching `\b\B` pattern. The `regex` crate rejects look-around, so the previous fallback would have panicked on `unwrap()` if the primary email pattern ever failed to compile, instead of conservatively rejecting all emails as intended. The branch remains effectively dead code because the hardcoded primary pattern always compiles.

### Internal

- Resolved all `cargo clippy --lib --all-features -- -D warnings` lints: derived `Default` for the global config state instead of a hand-written impl, dropped a needless `Ok(_?)` wrapper in the many-to-many entity-manager loader, and switched single-character `push_str(" ")` calls to `push(' ')` in the MySQL/SQLite full-text builders.
- Applied `cargo fmt` formatting across the config, entity-manager, full-text, internal SQL builder, query, sync, and tokenization modules; these are whitespace-only reflows with no behavior change.
- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, and macro-crate README to use `0.9.18`.
- Verified the release prep with `cargo clippy --lib --all-features -- -D warnings`, `cargo test --lib`, `cargo test --all-features --lib`, `cargo test --lib --no-default-features --features sqlite,runtime-tokio`, `cargo test -p tideorm-macros --lib`, `cargo package -p tideorm-macros --allow-dirty`, and `mdbook build`.
- Confirmed the main `tideorm` crate package step will remain blocked until `tideorm-macros 0.9.18` is published, because Cargo resolves the packaged dependency graph through the crates.io index instead of the workspace path dependency.

## [0.9.17] - 2026-06-07

### Changed — Breaking

- Reworked `Database::transaction()` and `Model::transaction()` to take a closure that returns a boxed, transaction-scoped future (`Box::pin(async move { ... })`) instead of a bare `async` future, so the closure body can borrow and use the provided `&Transaction` handle across await points. Update call sites from `db.transaction(|tx| async move { ... })` to `db.transaction(|tx| Box::pin(async move { ... }))`.

### Changed

- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, and macro-crate README to use `0.9.17`, and updated the documented transaction examples to the boxed-future closure form.

### Internal

- Reduced redundant clones and allocations across the model serialization, query builder, query execution, OR-clause, entity-manager flush/save, and transaction paths by borrowing instead of cloning — passing `&str`/`&HashMap`/`&ErrorContext` into helpers, using `as_ref()`/`as_deref()`/`extend_from_slice()`, and draining owned vectors with `into_iter()` rather than cloning their elements.
- Verified the release prep with `cargo test --lib`, `cargo test --all-features --lib`, `cargo test -p tideorm-macros --lib`, `cargo package -p tideorm-macros --allow-dirty`, and `mdbook build`.
- Confirmed the main `tideorm` crate package step will remain blocked until `tideorm-macros 0.9.17` is published, because Cargo resolves the packaged dependency graph through the crates.io index instead of the workspace path dependency.

## [0.9.16] - 2026-05-28

### Changed

- Upgraded SeaORM to `2.0.0-rc.38` and refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, and macro-crate README to use `0.9.16`.

### Internal

- Verified the release prep with `cargo test --lib`, `cargo test --all-features --lib`, `cargo test -p tideorm-macros --lib`, `cargo package -p tideorm-macros --allow-dirty`, and `mdbook build`.
- Confirmed the main `tideorm` crate package step will remain blocked until `tideorm-macros 0.9.16` is published, because Cargo resolves the packaged dependency graph through the crates.io index instead of the workspace path dependency.

## [0.9.15] - 2026-05-02

### Changed

- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, macro-crate README, and release-facing issue-template examples to use `0.9.15`.

### Fixed

- Fixed macro-generated relation state helpers to use TideORM's re-exported `json!` macro instead of referencing `serde_json::json!` directly, so downstream crates no longer need a direct `serde_json` dependency for generated relation code to compile.
- Fixed database JSON value conversion and `exists_any()` result decoding to preserve large unsigned integers and accept boolean or integer `EXISTS` row shapes across supported backends.
- Fixed attachment relation mutation helpers to reject unknown file relation names consistently, including attach, detach, and empty sync operations.
- Fixed backend-specific internal `sqlx` re-exports and raw-row JSON helpers to stay behind backend feature gates, reducing no-backend build breakage.

### Internal

- Added focused regression coverage for unsigned JSON-to-database values, backend-specific SQL parameter placeholders, attachment relation validation, and compile-fail relation fixtures.
- Tightened release-test coverage around optional postgres-only OR-clause integration tests and compile-fail fixture matching.
- Verified the release prep with `cargo test --lib`, `cargo test --all-features --lib`, `cargo test -p tideorm-macros --lib`, `cargo package -p tideorm-macros --allow-dirty`, and `mdbook build`.
- Confirmed the main `tideorm` crate package step will remain blocked until `tideorm-macros 0.9.15` is published, because Cargo resolves the packaged dependency graph through the crates.io index instead of the workspace path dependency.

## [0.9.14] - 2026-04-18

### Added

- Added model-level encrypted persisted fields through `#[tideorm::model(encrypted = "...")]`, automatically encrypting configured `String` and `Option<String>` columns on writes while decrypting them on model loads, eager loads, raw model hydration, nested saves, and batch updates.

### Changed

- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, macro-crate README, and release-facing issue-template examples to use `0.9.14`.
- Documented the current encrypted-field behavior and limitation that plaintext query predicates on encrypted columns are not yet transparently rewritten.

### Fixed

- Fixed generated SeaORM column and primary-key enum metadata for aliased TideORM fields so SQL uses the configured database column names instead of Rust field names.
- Fixed string-based query validation, SQL rendering, and batch-update column quoting so aliased fields work consistently when callers use either the Rust field name or the database column name.
- Scoped model-level encrypted fields to per-attribute keys derived from the configured secret plus the model table and column name, and tightened loads to reject plaintext rows or older global-scope encrypted payloads.

### Internal

- Added focused regression coverage for encrypted-field metadata, encrypted write/read flows, aliased-field query filters, and aliased batch updates.
- Verified the release prep with `cargo test --workspace --all-features --lib`, `cargo package -p tideorm-macros --allow-dirty`, and `mdbook build`.
- Confirmed the main `tideorm` crate package step will remain blocked until `tideorm-macros 0.9.14` is published, because Cargo resolves the packaged dependency graph through the crates.io index instead of the workspace path dependency.

## [0.9.13] - 2026-04-05

### Added

- Added model-local query-scope generation with `#[tideorm::scopes]`, so reusable filters can chain directly on `QueryBuilder` values such as `User::query().active().verified()`.
- Added large-result chunk processing through `QueryBuilder::chunk(...)` using primary-key cursor traversal, plus source-path model registration with `TideConfig::models_matching(...)` and `SyncRegistry::register_models_matching(...)` for compiled models under folders like `src/models/`.

### Changed

- Added query-builder eager-loading entry points with `with(...)` and `with_many(...)`, keeping batched relation loading discoverable from the main `QueryBuilder` API.
- Made dirty tracking opt-in behind the new `dirty-tracking` feature, including feature-gated public helpers, no-op internal hooks when disabled, and explicit dirty-tracking install guidance in the README and mdBook docs.
- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, macro-crate README, and release-facing issue-template examples to use `0.9.13`.

### Fixed

- Replaced offset-based chunk traversal with primary-key cursor traversal so batch callbacks can update or delete already-processed rows without skipping later matches, and tightened chunk validation to reject unsupported offset or non-primary-key ordering shapes.
- Simplified dirty-tracking baseline semantics to the latest persisted snapshot per model primary key, reduced snapshot storage overhead, and corrected generated delete paths for non-`Copy` primary keys.
- Fixed recursive glob source-path registration so patterns like `src/models/**/*.rs` match direct child files as well as nested directories.

### Internal

- Added focused regression coverage for eager loading, dirty-tracking lifecycle behavior, chunk traversal under mutation, model-local scope chaining, and source-path sync registration globs.
- Verified the release prep with `cargo test --lib`, `cargo test --all-features --lib`, `cargo test -p tideorm-macros --lib`, and `cargo package -p tideorm-macros --allow-dirty`.
- Confirmed the main `tideorm` crate package step will remain blocked until `tideorm-macros 0.9.13` is published, because Cargo resolves the packaged dependency graph through the crates.io index instead of the workspace path dependency.

## [0.9.12] - 2026-04-05

### Changed

- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, macro-crate README, and release-facing issue-template examples to use `0.9.12`.

### Internal

- Split the remaining oversized non-test runtime and proc-macro source files into focused sibling modules, including cache control/configuration, internal backend helpers, full-text PostgreSQL builders, query mutation safety, migration table SQL building, translation input/errors, sync schema helpers, validation builder/value helpers, entity-manager state helpers, relation entity-manager support, and proc-macro index parsing.
- Verified the release prep with `cargo test --lib`, `cargo test --all-features --lib`, `cargo test -p tideorm-macros --lib`, `cargo clippy --all-targets --all-features -- -D warnings`, `mdbook build`, and `cargo package -p tideorm-macros --allow-dirty`.
- Confirmed the main `tideorm` crate package step will remain blocked until `tideorm-macros 0.9.12` is published, because Cargo resolves the packaged dependency graph through the crates.io index instead of the workspace path dependency.

## [0.9.11] - 2026-04-04

### Added

- Added repository community-health coverage with a code of conduct, contributing guide, security policy, support guide, GitHub issue forms, and a pull request template so contributor and reporting workflows are documented directly in the repo.

### Changed

- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, macro-crate README, and release-facing issue-template examples to use `0.9.11`.

### Internal

- Consolidated repeated unit-test setup around shared Postgres config and sqlite mutation helpers, moved async test serialization off `std::sync::Mutex` guards held across `.await`, and kept test-only database/query helpers out of non-test builds.
- Tightened proc-macro parser maintenance by narrowing dead-code suppressions, removing an internal `unwrap()` from validation-rule parsing, and adding focused parser regressions.
- Verified the release prep with `cargo test --lib`, `cargo test --all-features`, `cargo test -p tideorm-macros --lib`, `mdbook build`, `cargo package --allow-dirty`, and `cargo package -p tideorm-macros --allow-dirty`.

## [0.9.10] - 2026-04-03

### Fixed

- Hardened backend-specific SQL preview escaping so MySQL and MariaDB preview paths now escape backslash-plus-quote payloads correctly, and HAVING/debug preview rendering now uses the active backend's literal rules instead of assuming PostgreSQL-style quote escaping everywhere.
- Split JSON predicate SQL generation into explicit preview-only renderers and executable bound-SQL helpers, keeping `QueryBuilder` JSON contains/key/path predicates parameterized across PostgreSQL, MySQL, MariaDB, and SQLite while preserving backend-native PostgreSQL placeholder forms in the shared helper layer.
- Tightened raw subquery validation from a prefix check into a top-level SQL shape scan, rejecting top-level `UNION`/`INTERSECT`/`EXCEPT` in raw union/CTE operands while still allowing builder-generated compound subqueries and recursive CTE bodies.
- Added `#[must_use]` coverage to the `OrGroup`, `OrBranch`, `OrBranchBuilder`, and `BatchUpdateBuilder` fluent builders, including `OrBranchBuilder::end_or()`, so dropped builder chains now warn at compile time instead of silently losing filters or updates.

### Changed

- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, and release-facing dependency snippets to use `0.9.10`.

### Internal

- Added focused regressions for backend-specific SQL preview escaping, parameterized JSON predicate helper SQL, and stricter raw subquery validation across the query layer.
- Verified the release prep with `cargo test --lib`, `cargo test --all-features --lib`, and `cargo test -p tideorm-macros --lib`.

## [0.9.9] - 2026-04-01

### Added

- Added the opt-in `entity-manager` feature, including the `EntityManager` persistence-context facade, managed lifecycle operations (`persist`, `merge`, `remove`, `detach`, `flush`), aggregate saves through `entity_manager.save(...)` and `save_with_entity_manager`, compatibility helpers such as `find_in_entity_manager` and `load_in_entity_manager(...)`, and tracked aggregate workflows across `HasOne`, `HasMany`, `BelongsTo`, and `HasManyThrough` relation helpers.
- Added dedicated README and mdBook documentation for `entity-manager`, including install guidance, aggregate and managed-lifecycle walkthroughs, and relation-chapter cross-links.

### Fixed

- Kept entity-manager relation helper reads on the parent context's database handle instead of falling back to the global connection, covering tracked and direct `HasMany`, `HasOne`, `BelongsTo`, and `HasManyThrough` helpers in multi-database setups.
- Hardened entity-manager save and flush behavior so scoped transactions restore managed state on rollback, late-added managed entries are checkpointed and processed correctly, detached `Managed<T>` handles cannot silently resurrect into untracked state, runaway flush growth fails after 16 passes instead of looping indefinitely, and relation-only sync refreshes runtime relation wrappers before syncing.

### Changed

- Refreshed the main crate version, macro-crate dependency version, README, mdBook chapters, and release-facing dependency snippets to use `0.9.9`, matching the first published TideORM release that exposes the optional `entity-manager` feature.

### Internal

- Added backend parity coverage plus focused unit regressions for entity-manager relation helpers, rollback checkpoints, detached-handle lifecycle behavior, runtime relation refresh, and flush cycle guards.

## [0.9.8] - 2026-03-30

### Changed

- Split the remaining large `cache`, `schema`, and `attachments` modules into focused submodules while preserving the public TideORM API surface.
- Moved crate unit-test source files out of `src/testing/` and into top-level `tests/unit/` directories for both the main crate and `tideorm-macros`, keeping private-module coverage without shipping test code inside library source trees.
- Refreshed README, mdBook chapters, and macro-crate install snippets to use the 0.9.8 release version.

### Internal

- Excluded `tests/**` from published crate artifacts and kept `wip/**` out of the main crate package so release tarballs only contain runtime and documentation sources.
- Verified the release prep with `cargo test --lib --all-features`, `cargo test -p tideorm-macros --lib`, and `cargo package --list` checks for both crates.

## [0.9.7] - 2026-03-29

### Changed

- Centralized the current ORM engine behind TideORM-owned seams by routing macro-generated paths through the hidden `tideorm::orm` facade and internal runtime code through `crate::internal`, removing the legacy `tideorm::sea_orm` compatibility export.
- Renamed internal engine-facing adapter types and helpers to TideORM-owned names such as `OrmConnection`, `OrmTransaction`, `OrmBackend`, and `OrmError`, and moved schema-sync/runtime backend handling onto TideORM's `Backend` abstraction.
- Refreshed README, mdBook chapters, and macro-crate install snippets to use the 0.9.7 release version and TideORM-owned ORM terminology.

### Internal

- Updated relation compile-fail fixtures and generated macro error paths to match the new `tideorm::orm` and `crate::internal` seams.
- Verified the release prep with `cargo test --lib --all-features`, `cargo test --test relation_serde_tests`, `cargo test --test relation_compile_fail`, `cargo clippy --workspace --all-targets -- -D warnings`, and `mdbook build`.

## [0.9.6] - 2026-03-28

### Fixed

- Restored schema generation for qualified table names by tracking schema names explicitly in `TableSchema`, emitting schema-qualified `CREATE TABLE` and `CREATE INDEX` targets when present, allowing safe dotted identifier references for direct generator usage, and preserving non-system PostgreSQL schemas during database introspection.
- Made schema registry de-duplication schema-aware so tables with the same name in different schemas no longer collapse into a single registered entry.
- Updated migration bookkeeping writes to use backend-specific parameter placeholders, so `_migrations` inserts and deletes stay parameterized on PostgreSQL as well as MySQL/MariaDB and SQLite.

### Internal

- Added schema-generation regression coverage for schema-qualified table output, dotted identifier references, and schema-aware registry behavior.
- Verified the release prep with `cargo test --all-features`, `cargo clippy --workspace --all-targets -- -D warnings`, and `mdbook build`.

## [0.9.5] - 2026-03-28

### Fixed

- Hardened the remaining runtime and tooling SQL assembly paths so seed and migration bookkeeping use bound parameters for user-controlled values, schema/sync helpers reuse backend-safe identifier quoting, and SQLite schema introspection no longer interpolates PRAGMA identifiers directly.
- Reworked non-Tokio connection override scoping so transaction and scoped-connection helpers reinstall the override on every poll instead of leaking thread-local state across await spans.
- Neutralized parser-injection edges in PostgreSQL full-text boolean, prefix, and proximity builders plus SQLite FTS5 query text by sanitizing user input into literal search terms.
- Restored cached relation round-trip serialization for models with generated serde implementations, so relation wrapper payloads now survive serialize/deserialize flows instead of being dropped.

### Changed

- `exists_any()` now uses a scalar `SELECT EXISTS(...)` probe instead of selecting a dummy row and hydrating an intermediate model.
- Centralized runtime SQL safety helpers in `src/internal/sql_safety.rs` so identifier quoting, raw-fragment validation, and full-text sanitization are shared across query, schema, sync, migration, relation, and seeding paths.
- Refreshed dependency snippets and relation-serialization docs to use the 0.9.5 release version and describe cached relation serde behavior accurately.

### Internal

- Added regression coverage for non-Tokio scoped overrides, HAVING validation, backend-specific subquery alias quoting, full-text sanitization, schema identifier escaping, cached relation round trips, and highlighted full-text SQL escaping.
- Verified the release prep with `cargo test --lib --all-features` and `cargo clippy --workspace --all-targets -- -D warnings`.

## [0.9.4] - 2026-03-28

### Fixed

- Rejected invalid 1-based pagination inputs so `page = 0` no longer aliases `page = 1`. `Model::paginate()` now returns a validation error for zero `page` or `per_page`, and `QueryBuilder::page()` now invalidates the builder before execution for the same misuse.

### Changed

- Refreshed public API docs to remove tutorial-heavy and promotional rustdoc blocks in favor of shorter operational guidance focused on behavior, failure modes, and debugging paths.
- Refreshed dependency snippets and macro-crate docs to use the 0.9.4 release version.

### Internal

- Added SQLite regression coverage for invalid page-zero pagination.
- Verified the release prep with `cargo test --all` and `cargo test --lib`.

## [0.9.3] - 2026-03-27

### Fixed

- Restored backend-safe `HasManyThrough::attach()` writes by switching the pivot insert path to quoted identifiers and backend-specific parameter placeholders, so SQLite and MySQL/MariaDB no longer depend on PostgreSQL-style `$n` bindings.
- Hardened raw query-builder escape hatches so `union_raw()`, `union_all_raw()`, `with_cte()`, `with_cte_columns()`, `with_recursive_cte()`, `lag()`, `lead()`, and custom window expressions reject unsafe or non-subquery SQL before any database lookup.

### Changed

- Refreshed dependency examples and macro-crate docs to use the 0.9.3 release version.

### Internal

- Reduced duplicated relation-helper and relation-test boilerplate while keeping the public API unchanged.
- Verified the release prep with `cargo test --lib`.

## [0.9.2] - 2026-03-27

### Fixed

- Direct relation `load()` helpers now refresh stale cached values from the active database only when the relation is configured and has a scalar key available, while preserving cached-payload fallback for deserialized or manually attached relations that do not have query context yet.
- SQL preview generation now supports explicit backend selection for deterministic test coverage, avoiding order-dependent SQLite failures when earlier tests leave a different global database backend active.
- Hardened raw query-builder entry points so `having()`, `select_raw()`, and `select_subquery()` reject unsafe raw SQL fragments before database access, and `select_subquery()` now validates its nested subquery builder plus the projected alias.

### Changed

- Refreshed dependency examples and macro-crate docs to use the 0.9.2 release version.

### Internal

- Added regression coverage for direct relation cache refresh and cached fallback behavior, deterministic escaped-LIKE SQL previews, and raw HAVING, SELECT, and SELECT-subquery validation.
- Verified the release prep with `cargo test --lib` and `cargo clippy --workspace --all-targets -- -D warnings`.

## [0.9.0] - 2026-03-26

### Changed — Breaking

- Removed the redundant `Model::insert_many()` and `Model::insert_many_returning()` aliases. `Model::insert_all()` remains the single batch-insert API and continues to return inserted models with database-generated values populated when the backend supports or emulates it.

### Fixed

- `Model::save()` and `Model::update()` now invoke `Validate::validate()` automatically during the lifecycle between `before_validation` and `after_validation`, so validation attributes and custom `Validate` implementations are enforced before writes hit the database.
- Direct read helpers such as `Model::all()`, `Model::count()`, `Model::first()`, `Model::last()`, `Model::exists_any()`, and `Model::paginate()` now honor the default soft-delete scope instead of bypassing it through the lower-level query executor.
- Escaped literal `LIKE` helpers now use backend-specific `ESCAPE` literals, keep PostgreSQL parameterized SQL correct, and avoid rewriting dollar-sign text inside raw PostgreSQL string literals, dollar-quoted strings, or comments when batch update placeholders are rebased.
- Query fragment consolidation now preserves nested OR groups, projections, raw selects, pagination, unions, window functions, CTEs, cache options, and soft-delete scope flags instead of silently dropping builder state.
- Validation length rules now count Unicode characters instead of UTF-8 bytes.
- Seeder dependency sorting now fails fast on circular dependencies instead of silently appending unresolved seeds.
- Macro-generated `Default` models now initialize runtime relation handles consistently with deserialized and loaded models, so relation wrappers are configured before a model is first saved.
- Batch `execute_returning()` now rejects unsupported backends through `DatabaseType::supports_returning()` instead of hardcoding a MySQL-only check.

### Changed

- Query-cache reads now use a read-first fast path and only promote to a write lock when recency updates or expiry cleanup are required.
- Refreshed dependency examples and macro-crate docs to use the 0.9.0 release version.

### Internal

- Added regression coverage for callback validation order, escaped literal `LIKE` SQL generation, query fragment consolidation, Unicode length validation, circular seed detection, relation-default initialization, and PostgreSQL placeholder rebasing in raw batch SQL.
- Verified the release prep with `cargo test --lib` and `cargo clippy --workspace --all-targets -- -D warnings`.

## [0.8.9] - 2026-03-22

### Changed

- Removed the unused direct `async-recursion` dependency from the main TideORM crate.
- Updated the macros crate to use `convert_case` 0.11.
- Refreshed dependency examples and macro-crate docs to use the 0.8.9 release version.

### Internal

- Verified the release prep with `cargo test --lib`.

## [0.8.8] - 2026-03-22

### Fixed

- Restored `Database::ping()` after the database module split so query-builder and backend integration tests keep the expected public API.
- Corrected the documented SQLite integration command to include the required `runtime-tokio` feature when running `sqlite_integration_tests` without default features.

### Changed

- Split the remaining large `config`, `database`, `migration`, `model`, `query`, `relations`, and `types` modules into focused submodules while preserving the public TideORM API surface.
- Refreshed dependency examples and macro-crate docs to use the 0.8.8 release version.

### Internal

- Verified the refactor and release prep with `cargo test --all-features`, `cargo test --lib`, `cargo test --features postgres`, `cargo test --test sqlite_integration_tests --features "sqlite runtime-tokio" --no-default-features`, `cargo test --test postgres_integration_tests`, `cargo test --test postgres_advanced_tests`, and `cargo test --test mysql_integration_tests --features mysql`.

## [0.8.7] - 2026-03-21

### Fixed

- Quoted simple identifier references consistently in the manual SQL builder paths so reserved column names such as `order` and `group` no longer break generated SELECT, WHERE, GROUP BY, JOIN, and ORDER BY clauses.
- Preserved embedded identifier-quote escaping while tightening the reserved-word quoting path, so names containing quote characters still render correctly for each backend dialect.
- Fixed SQLite raw JSON decoding for untyped aggregate expressions such as `COUNT(*)` and `SUM(...)`, so count/exists helpers and raw JSON reads no longer degrade numeric aggregate results into strings.

### Changed

- Refreshed dependency examples and macro-crate docs to use the 0.8.7 release version.

### Internal

- Added query SQL regressions covering reserved-word identifiers and embedded quote escaping.
- Added SQLite raw JSON regressions covering aggregate count decoding.
- Verified the release prep with `cargo test --all-features`, `cargo clippy --lib --all-features -- -D warnings`, and `mdbook build`.

## [0.8.6] - 2026-03-21

### Fixed

- Preserved loaded `HasOne`, `HasMany`, `BelongsTo`, and `HasManyThrough` relation runtime state when model helpers overwrite a model from JSON, so translation and file-attachment updates no longer silently discard cached relations.
- Reworked raw SQL JSON row conversion to decode backend-aware column types instead of relying on a fragile probe order, preserving booleans and improving JSON, UUID, date/time, and decimal handling.
- Added `Send` bounds to nested relation save operation erasure so `NestedSaveBuilder` remains `Send` and can safely cross await points or be moved into `tokio::spawn` tasks.

### Changed

- Removed the misleading `Model::load_all_translations()` helper. Use `get_all_translations()`, `get_translations_for_language()`, or `to_json_with_all_translations()` depending on whether you need per-field values, a single-language projection, or full JSON output.
- Cached the email validation regex with `OnceLock` instead of compiling it on every validation call.
- Refreshed dependency examples and macro-crate docs to use the 0.8.6 release version.

### Internal

- Updated relation compile-fail snapshots to match the attribute-macro test fixtures.
- Verified the release prep with `cargo test --all-features`, `cargo clippy --lib --all-features -- -D warnings`, and `mdbook build`.

## [0.8.5] - 2026-03-20

### Fixed

- Reworked `Database::transaction()` to restore the transaction-scoped database override on every future poll, so model and query helpers keep using the active transaction even if the async runtime resumes the future on a different thread.

### Changed

- Moved the remaining inline unit tests out of implementation modules and into dedicated owner test files under `src/testing/`, keeping private-module coverage while making the source files easier to maintain.
- Refreshed dependency examples and macro-crate docs to use the 0.8.5 release version.

### Internal

- Added a regression test that manually polls a transaction-scoped future on two different threads to verify the per-poll override behavior.
- Verified the release prep with `cargo test --lib --features sqlite` and `cargo clippy --workspace --all-targets -- -D warnings`.

## [0.8.4] - 2026-03-20

### Fixed

- Replaced silent translation and file-attachment serialization stubs so `Model::load_language_translations()`, `Model::get_files_attribute()`, and `Model::set_files_attribute()` now operate on the model state instead of succeeding without effect.
- Made `Model::load_all_translations()` fail loudly with a clear unsupported error instead of silently pretending to load all translations into scalar model fields.
- Hardened `Database::transaction()` so leaked transaction handles now fail consistently on both commit and rollback paths instead of silently relying on drop-based rollback in the error path.
- Ensured `NestedSaveBuilder::save()` persists related models with the parent foreign key instead of returning only FK-patched JSON payloads.

### Changed

- Reduced database-access overhead on model hot paths by resolving the current connection/backend directly from the active scope instead of repeatedly cloning the outer `Database` wrapper.
- Changed transaction-scoped thread-local overrides to store `DatabaseHandle` directly and updated `ConnectionRef::Database` to carry the shared internal connection handle instead of cloning `DatabaseConnection` per lookup.
- Refreshed public docs and examples to use the current global-database initialization API and the 0.8.4 crate version.

### Internal

- Kept the workspace warning-free after the connection-handle refactor by updating generated macro code, full-text execution paths, eager loading, nested bulk upserts, and query helpers to use shared internal connections correctly.
- Verified the release with `cargo test --lib` and `cargo clippy --workspace --all-targets -- -D warnings`.

## [0.8.1] - 2026-03-18

### Fixed

- Restored transaction scoping for TideORM model and query helpers so `save()`, `update()`, `delete()`, eager loading, nested operations, full-text reads, and aggregate helpers now honor the active transaction instead of bypassing it through the global pooled connection.
- Hardened raw SQL builder escape hatches by rejecting obvious injection markers in `where_raw`, raw column expressions, and nested subqueries before execution.
- Restored the original `Model::to_hash_map()` behavior that hides structured presenter `params` payloads entirely; `params` remains a reserved presenter key and is now documented as such.
- Removed disabled-path profiling overhead in `__profile_future()` by skipping `Instant::now()` when global profiling is off.
- Replaced leaked schema-path storage in `TideConfig` so repeated `apply()` and `connect()` calls no longer leak each configured schema file path.
- Reworked `SelfRefMany::load_tree()` to use a single recursive CTE query, eliminating per-node descendant lookups and honoring the configured `local_key` when walking self-referential trees.
- Repaired all-features build breakage after the reconfigurable global-database refactor by updating direct SeaORM call sites to borrow owned internal connections correctly.
- Restored consistent full-text SQL parameterization coverage across the query and full-text test suites, including PostgreSQL ranked search placeholders and SQLite FTS pagination bindings.
- Tightened encrypted-field missing-key coverage so integration tests now assert the actionable startup-configuration error message returned by `Encrypted<T>`.

### Changed

- `require_db()`, `try_db()`, `TideConfig::db()`, `TideConfig::try_db()`, `Model::db()`, and `Model::database()` now return owned `Database` handles for consistency with transaction-aware current-connection access.
- `TideConfig::schema_file_path()` now returns `Option<String>` instead of `Option<&'static str>` so schema path state can be replaced safely without leaking memory across reconfiguration.
- Documented resettable global configuration, tokenization override reset behavior, and the batched nested many-model save/update/delete paths in the README and mdBook chapters.

### Internal

- Removed stale imports left behind by the runtime-global refactor.
- Verified the release with `cargo test --all-features`.

## [0.8.0] - 2026-03-16

### Changed

- Moved the documentation to an mdBook with dedicated Getting Started, Models, Queries, Relations, and Migrations chapters, and wired the site for GitHub Pages deployment at `tideorm.com`.
- Standardized model and field metadata on the `#[tideorm(...)]` attribute form across docs, tests, benchmarks, and generated macro output.
- Split the `tideorm-macros` implementation into focused modules so entity generation, model trait generation, serde, relations, tokenization, validation, and parsing logic are maintained independently.
- Initialized supported relation wrappers through generated `with_relations()` setup for loaded models, so `HasOne`, `HasMany`, `BelongsTo`, and `HasManyThrough` fields are wired with the correct runtime context.

### Changed — Breaking

- Gated attachments, translations, and full-text search behind explicit Cargo features. Consumers now need to opt into `attachments`, `translations`, and `fulltext` when using those modules or APIs.
- Removed the legacy `#[tide(...)]` compatibility form and related prelude re-exports. Use `#[tideorm(...)]` attributes consistently.

### Fixed

- Excluded runtime-only relation helper fields from TideORM's generated serde output and restored them with defaults during deserialization.
- Restored valid PostgreSQL JSON and array operator SQL generation after the SeaQuery condition rewrite, keeping advanced query coverage green for `@>`, `<@`, `?`, `@?`, and array overlap/containment operators.
- Cleaned up the top-level documentation entry points so `DOCUMENTATION.md`, README links, and the split mdBook chapters no longer contain stale anchors or partially copied monolith content.

### Removed

- Dropped the explicit `sea-schema` dependency and removed unused development dependencies `lazy_static`, `pretty_assertions`, and `serial_test`.

### Internal

- Added docs verification to CI and a dedicated Pages workflow for publishing the mdBook.
- Verified the release with `cargo test`, `cargo test postgres_advanced_tests`, and `mdbook build`.

## [0.7.3] - 2026-03-16

### Fixed

- Relation helper fields generated by `#[derive(Model)]` are now excluded from TideORM's auto-generated serde output and restored with defaults on deserialize, preventing `HasOne`, `HasMany`, `BelongsTo`, and similar runtime-only relation wrappers from leaking into JSON payloads.

### Changed

- Refreshed the README and documentation examples to stop recommending manual `id: 0` initialization for common auto-increment create flows.
- Documented that relation helper fields are skipped by generated serde and restored with defaults during deserialization.

### Internal

- Aligned the tokenization test suite with the current XChaCha20-Poly1305 implementation, including randomized tokens per encode and the authenticated token length now produced by the runtime.
- Corrected the SQLite FTS5 SQL test expectation to match the current identifier-quoting behavior in generated statements.
- Verified the release with a full `cargo test` pass.

## [0.7.2] - 2026-03-15

### Changed

- Refactored the query subsystem so the oversized query builder implementation is now split into focused modules, reducing maintenance risk without changing the public builder API.
- Standardized read execution paths on parameterized SQL generation for get, first, count, exists, and JSON reads to keep backend-specific quoting and placeholder handling consistent.
- Replaced the default record tokenization internals with XChaCha20-Poly1305 authenticated encryption using randomized nonces while keeping the public token APIs intact.

### Fixed

- Repaired the extracted SQL query module after a bad split left overlapping implementations in place.
- Restored JOIN clause validation in the advanced query builder so invalid table, alias, and column identifiers invalidate the query instead of being accepted.
- Hardened mutation queries so delete, restore, soft-delete, and force-delete fail fast when combined with incompatible SELECT, JOIN, ORDER BY, GROUP BY, HAVING, UNION, CTE, or window-function modifiers.
- Improved cache key generation so more query-shaping fields are included, preventing collisions between distinct advanced query configurations.
- Corrected `Model::is_new()` so auto-increment primary keys with value `0` are treated as unsaved records.

### Added

- Added a GitHub Actions CI workflow that runs `cargo check` and `cargo test --lib` across PostgreSQL, MySQL, and SQLite feature sets.

### Internal

- Verified the release with a full cargo test pass after the query-module recovery and cleanup.
- Moved larger internal unit suites into dedicated files under `src/testing/` to keep implementation modules cleaner without losing private-module test coverage.

## [0.7.0] - 2025-03-09

### Added — MariaDB Support

- **`DatabaseType::MariaDB` variant**: Full first-class MariaDB support with auto-detection. Connecting via `mysql://` to a MariaDB server automatically detects the variant using `SELECT VERSION()`
- **`mariadb://` URL scheme**: `from_url()` and `TideConfig` now accept `mariadb://` URLs (rewritten to `mysql://` for the sqlx driver)
- **`#[non_exhaustive]` on `DatabaseType`**: Future-proofing the enum for new backends without breaking downstream matches
- **`is_mysql_compatible()` / `is_mariadb()` helpers**: Static methods on `TideConfig` for runtime backend detection
- **MariaDB RETURNING support**: MariaDB 10.5+ supports `INSERT ... RETURNING`, so the RETURNING-rejection check now only applies to MySQL (not MariaDB)

### Added — Error Ergonomics

- **`From<sea_orm::DbErr>` for `Error`**: Use `?` directly on SeaORM operations instead of `.map_err(translate_error)`
- **`From<std::io::Error>` for `Error`**: Converts to `Error::Internal`
- **`From<serde_json::Error>` for `Error`**: Converts to `Error::Conversion`

### Added — Transaction API

- **`Transaction::connection()`**: Public method to get the underlying `&DatabaseTransaction` for use with SeaORM operations inside transactions

### Changed — Breaking

- **`Database::transaction()` signature changed**: The closure now receives `&Transaction` (reference) instead of `Transaction` (owned), and must return `Pin<Box<dyn Future>>`. This fixes a critical bug where transactions were never committed. Usage: `db.transaction(|tx| Box::pin(async move { ... })).await?`
- **`Model::transaction()` signature changed**: Matches the new `Database::transaction()` signature
- **`DatabaseType` is now `#[non_exhaustive]`**: `match` on `DatabaseType` must include a wildcard arm

### Changed — API

- **`Model::db()` / `Model::database()` now return `Result`**: Changed from `&'static Database` to `Result<&'static Database>` — use `?` or `.unwrap()` at callsite
- **`TideConfig::db()` now returns `Result`**: Changed from `&'static Database` to `Result<&'static Database>`
- **`require_db()` re-exported in prelude**: Added to `prelude::*` alongside `db` and `try_db`

### Fixed — Critical

- **Transaction commit bug**: `Database::transaction()` now properly commits on success instead of silently rolling back. The previous implementation moved the transaction into the closure, causing it to auto-rollback on drop regardless of outcome
- **MySQL batch insert error swallowing**: `Model::insert_all()` no longer silently falls back to individual inserts when batch insert fails — errors are now propagated. The underlying `QueryExecutor::insert_many()` properly handles MySQL/SQLite by falling back to individual inserts internally

### Fixed — MySQL/MariaDB

- **MySQL array type mapping**: `Vec<i32>`, `IntArray`, `BigIntArray`, `TextArray`, `BoolArray`, `FloatArray`, `JsonArray` now correctly map to `JSON` on MySQL/MariaDB (previously fell through to `TEXT`)
- **All database-dispatch match arms updated**: 50+ match arms across `query.rs`, `schema.rs`, `migration.rs`, `fulltext.rs`, `model.rs`, `seeding.rs` now include `DatabaseType::MariaDB` alongside `DatabaseType::MySQL`

### Internal

- **`QueryExecutor::insert_many()` rewritten**: Now checks backend support for `INSERT ... RETURNING` — uses batch RETURNING on PostgreSQL, falls back to individual inserts on MySQL/SQLite
- **`database::backend()` improved**: Prefers `TideConfig::get_database_type()` for MariaDB-awareness before falling back to SeaORM backend detection

## [0.6.0] - 2026-02-21

### Added

- **`require_db()` function**: Non-panicking alternative to `db()` — returns `Result<&Database>` instead of panicking when the global connection is not initialized. Exported from `tideorm::require_db`
- **Batch insert via `QueryExecutor::insert_many`**: New internal method using a single multi-row INSERT statement, reducing database round trips from O(n) to O(1)
- **Structured logging macros**: `tide_info!`, `tide_warn!`, `tide_debug!` for consistent `[TideORM]`-prefixed log output across all modules
- **Primary key column in derive macro**: `primary_key_column()` now returns the actual primary key column, enabling proper `last()` ordering by PK descending

### Improved

- **Tuple registration expanded to 200**: `RegisterMigrations`, `RegisterSeeds`, and `RegisterModels` now support tuples of up to 200 types (previously limited to 12–16). Refactored from hand-written impls to recursive macros
- **`insert_all()` uses batch insert**: Single multi-row INSERT with automatic fallback to individual inserts if the backend doesn't support `INSERT ... RETURNING`
- **`last()` orders by primary key DESC**: Previously returned an arbitrary first record; now correctly returns the last record by primary key
- **`raw_json()` column extraction**: Improved type priority chain (bool before int, nullable-first) for more accurate JSON output
- **Replaced `lazy_static!` with `parking_lot::RwLock` const init**: In `logging` and `profiling` modules for simpler, zero-overhead static initialization
- **Better panic message for `db()`**: Now mentions `Database::set_global()` and suggests `try_db()` as alternative

### Changed

- **All `db()` calls replaced with `require_db()?`**: Throughout `model.rs`, `query.rs`, `migration.rs`, `seeding.rs`, `schema.rs`, `database.rs`, and macro-generated code — these now return descriptive errors instead of panicking
- **All `eprintln!` replaced with structured logging**: Consistent `[TideORM]`/`[TideORM WARN]`/`[TideORM DEBUG]` prefixed output across sync, migration, seeding, config, and query modules
- **Derive macro lint suppression narrowed**: From blanket `clippy::all` to specific `clippy::derivable_impls`, `clippy::enum_variant_names`, `clippy::redundant_closure`
- **Removed blanket `#![allow(dead_code, unused_imports)]`** from `internal/mod.rs` — now uses targeted `#[allow(unused_imports)]` on the specific import block

### Fixed

- **Attachment detach safety**: Uses `if let Some(first)` instead of `unwrap()` in `detach()` logic
- **OrBranch single-condition safety**: Uses `if let Some(condition)` instead of `unwrap()` in `OrBranchBuilder`
- **Migration rollback safety**: `match applied.last()` with early return instead of `unwrap()` when no migrations are applied
- **Seed rollback safety**: `match executed.last()` with early return instead of `unwrap()` when no seeds are executed
- **Changelog date typos**: Corrected years from 2026 to 2025 for historical entries (0.1.0, 0.4.3, 0.4.4, 0.4.5)

### Dependencies

- `sea-orm`: 2.0.0-rc.30 → 2.0.0-rc.32
- `sea-query`: 1.0.0-rc.30 → 1.0.0-rc.31
- `uuid`: 1.19.0 → 1.21.0
- `getrandom`: 0.3.4 → 0.4.1
- Plus transitive dependency updates

## [0.5.0] - 2025-07-22

### Improved

- **Zero clippy warnings**: Resolved all clippy warnings across lib, macros, tests, and benchmarks
- **Macro code quality**: `Model` derive macro no longer emits `needless_update` (`..Default::default()`) when structs have no relation fields — generates cleaner, more idiomatic output
- **Regex performance**: `highlight_text()` in fulltext module pre-compiles regex patterns outside the loop instead of re-creating them per word
- **Config access optimization**: Config accessor methods (`get_languages`, `get_fallback_language`, etc.) now read directly from the `RwLock` without cloning the entire `Config` struct
- **Macro lint fixes**: Converted `match` single-arm patterns to `if let`, removed useless `.into()` conversion, replaced `i.to_string() == "created_at"` comparisons with direct ident comparison

### Changed

- **`LogLevel::from_str()` → `LogLevel::parse_str()`**: Renamed to avoid confusion with `std::str::FromStr` trait (clippy `should_implement_trait`)
- **`CastType::from_str()` → `CastType::parse_str()`**: Same rename for consistency
- **`CommaSeparated::to_string()`** inherent method removed — `Display` trait implementation provides this automatically
- **`sort_seeds_by_priority_and_deps()`** return type changed from `Vec<&Box<dyn Seed>>` to `Vec<&dyn Seed>` (clippy `borrowed_box`)

### Fixed

- **`identity_map` in relations**: Removed `.map(|c| c)` identity mapping
- **Profiling scoring**: Combined identical UPDATE/DELETE score branches
- **Test assertions**: Replaced `assert!(true)` placeholders with proper empty test bodies
- **Benchmark code quality**: Fixed `iter().count()` → `.len()`, range loops → iterators, redundant closures, unnecessary borrows, redundant match guards

### Dependencies

- Updated all transitive dependencies to latest Rust 1.85-compatible versions
- `uuid`: 1.19.0 → 1.20.0
- `proc-macro2`: 1.0.105 → 1.0.106
- `quote`: 1.0.43 → 1.0.44
- Plus 40+ transitive dependency updates

## [0.4.5] - 2025-01-17

### Added

#### Record Tokenization

Note: The original tokenization implementation described below was replaced in 0.7.2 by XChaCha20-Poly1305 authenticated encryption with randomized nonces. These notes remain here as historical release context.

- **New `#[tideorm(tokenize)]` attribute**: Enable tokenization on any model with a single attribute
- **Secure ID encryption**: Convert record IDs to encrypted, URL-safe tokens via `Tokenizable` trait
- **Model-specific tokens**: Tokens include model name in HMAC, preventing cross-model token reuse
- **Tamper detection**: HMAC verification ensures tokens haven't been modified
- **Instance methods**: `user.tokenize()`, `user.to_token()`, `user.regenerate_token()`
- **Static methods**: `User::tokenize_id(42)`, `User::detokenize(&token)`, `User::decode_token(&token)` returning the model's primary key type
- **Async fetch**: `User::from_token(&token).await` - decode and fetch in one call
- **Configuration hierarchy**: Default → TideConfig → Model (most specific wins)
- **Global encryption key**: Configure via `TokenConfig::set_encryption_key("your-secret-key")`
- **Custom encoders/decoders**: Use `TokenConfig::set_encoder()` and `set_decoder()` for custom strategies
- **Model-level overrides**: Implement `Tokenizable` trait manually for custom logic
- **URL-safe output**: Base64-URL encoding (A-Za-z0-9-_) safe for URLs without escaping
- **New error types**: `TideError::Tokenization` and `TideError::InvalidToken` for clear error handling
- New types exported: `TokenConfig`, `TokenEncoder`, `TokenDecoder`, `Tokenizable`
- New comprehensive example: `examples/tokenization_demo.rs`
- New test file: `tests/tokenization_tests.rs` (48 tests total)
- New benchmarks: `benches/tokenization_benchmarks.rs`

### Example

```rust
use tideorm::prelude::*;

#[derive(Model)]
#[tideorm(table = "users", tokenize)]  // Just add `tokenize` here!
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
}

// Configure encryption key once
TokenConfig::set_encryption_key("my-super-secret-key-at-least-32-chars");

// Tokenize a record
let user = User::find(1).await?.unwrap();
let token = user.tokenize()?;  // "iIBmdKYhJh4_vSKFlBTP..."

// Decode token to the model's primary key type
let id = User::detokenize(&token)?;  // 1

// Or fetch directly from token
let same_user = User::from_token(&token).await?;

// Tokens are model-specific
let user_token = User::tokenize_id(1)?;
let product_token = Product::tokenize_id(1)?;
assert_ne!(user_token, product_token);  // Different!

// Cross-model decoding fails
assert!(User::detokenize(&product_token).is_err());
```

#### Strongly-Typed Column Support

- **Auto-generated typed columns**: `#[tideorm::model]` now generates a `{Model}Columns` struct with typed column accessors
- **Access columns via model attribute**: `User::columns.name`, `User::columns.age`, etc.
- **Unified query methods**: All query methods now accept both strings AND typed columns:
  - WHERE clauses: `where_eq`, `where_not`, `where_gt`, `where_gte`, `where_lt`, `where_lte`, `where_like`, `where_not_like`, `where_in`, `where_not_in`, `where_null`, `where_not_null`, `where_between`
  - OR conditions: `or_where_eq`, `or_where_not`, `or_where_gt`, `or_where_gte`, `or_where_lt`, `or_where_lte`, `or_where_like`, `or_where_in`, `or_where_not_in`, `or_where_null`, `or_where_not_null`, `or_where_between`
  - AND within OR branches: `and_where_eq`, `and_where_not`, `and_where_gt`, `and_where_gte`, `and_where_lt`, `and_where_lte`, `and_where_like`, `and_where_in`, `and_where_not_in`, `and_where_null`, `and_where_not_null`, `and_where_between`
  - ORDER BY: `order_by`, `order_asc`, `order_desc`
  - GROUP BY: `group_by`
  - Aggregations: `sum`, `avg`, `min`, `max`, `count_distinct`
  - HAVING: `having_sum_gt`, `having_avg_gt`
  - Window functions: `partition_by`, `order_by` (in `WindowFunctionBuilder`)
- **IDE autocomplete support**: Type `User::columns.` to see all available columns with their types
- **Compile-time type safety**: Wrong column names caught at compile time when using typed columns
- **`IntoColumnName` trait**: New trait allows any type implementing it to be used as a column name

### Example

```rust
// All of these work with the SAME methods:
User::query().where_eq("active", true)                      // String-based (runtime checked)
User::query().where_eq(User::columns.active, true)          // Typed column (compile-time checked)

// Works for all query methods:
User::query()
    .where_eq(User::columns.status, "active")
    .where_gt(User::columns.age, 18)
    .order_by(User::columns.created_at, Order::Desc)
    .group_by(User::columns.role)
    .get()
    .await?;
```

## [0.4.4] - 2025-01-16

### Added

#### File Attachment URL Generation

- **Field name context**: URL generators now receive the field name (e.g., "thumbnail", "avatar") for context-aware routing
- **Full metadata access**: URL generators also receive full `FileAttachment` struct with all metadata
- **Global base URL**: Configure via `TideConfig::file_base_url("https://cdn.example.com")`
- **Custom URL generators**: Use `TideConfig::file_url_generator(fn(field_name, file) -> String)` for smart URL routing
- **Model-specific overrides**: Override `file_url_generator()` in `ModelMeta` for per-model customization
- **Automatic URL in JSON**: `to_json()` now includes `url` field in file attachments
- **Manual URL generation**: `Config::generate_file_url()`, `Model::generate_file_url()`, `FileAttachment::url()`
- **FileUrlGenerator type**: Exported in prelude for custom generator functions
- New comprehensive example: `examples/attachment_url_demo.rs` with 24 test cases

### Changed

- **BREAKING**: `FileUrlGenerator` signature is now `fn(field_name: &str, file: &FileAttachment) -> String`
  - Migration: Change `|file| format!("...{}", file.key)` to `|_field_name, file| format!("...{}", file.key)`
  - Or use field_name: `|field_name, file| match field_name { "thumbnail" => ..., _ => ... }`
  - Benefit: Route URLs based on field type (thumbnails to image CDN, videos to streaming, etc.)

## [0.4.3] - 2025-01-14

### Added

#### Comprehensive OR Conditions Support

- **Simple OR methods**: `or_where_eq`, `or_where_not`, `or_where_gt`, `or_where_gte`, `or_where_lt`, `or_where_lte`, `or_where_like`, `or_where_not_like`, `or_where_in`, `or_where_not_in`, `or_where_null`, `or_where_not_null`, `or_where_between`
- **Fluent OR API**: `begin_or()` / `end_or()` for grouped OR conditions
- **AND chaining within OR groups**: `and_where_eq`, `and_where_not`, `and_where_gt`, `and_where_gte`, `and_where_lt`, `and_where_lte`, `and_where_like`, `and_where_in`, `and_where_not_in`, `and_where_null`, `and_where_not_null`, `and_where_between`
- Multiple sequential OR groups support for complex business logic
- New comprehensive example: `examples/where_and_or_demo.rs` with 50+ test cases

### Fixed

- Fixed critical bug where `or_groups` were not being applied to queries in `get()`, `first()`, `count()`, `delete()`, `count_distinct()`, and `aggregate_f64()` methods

## [0.1.0][0.1.0] - 2025-01-08

### 🎉 Initial Release

This is the first public release of TideORM, a developer-friendly ORM for Rust with clean, expressive syntax.

### Added

#### Core ORM

- `#[derive(Model)]` macro for defining models
- Global database configuration via `TideConfig`
- Connection pooling with configurable min/max connections
- Support for PostgreSQL (MySQL and SQLite planned)

#### CRUD Operations

- `Model::create()` - Create new records
- `Model::find()` / `Model::find_or_fail()` - Find by ID
- `Model::all()` - Get all records
- `Model::first()` / `Model::last()` - Get first/last record
- `Model::count()` - Count records
- `Model::exists()` - Check if record exists
- `model.update()` - Update existing records
- `model.delete()` / `Model::destroy()` - Delete records
- `model.reload()` - Refresh from database

#### Relations

- `#[belongs_to]` - Define belongs-to relationships
- `#[has_one]` - Define has-one relationships
- `#[has_many]` - Define has-many relationships
- `load_belongs_to()`, `load_has_one()`, `load_has_many()` - Eager loading

#### Query Builder

- Fluent query interface via `Model::query()`
- WHERE conditions: `where_eq`, `where_not`, `where_like`, `where_in`, `where_null`, `where_not_null`, `where_gt`, `where_lt`, `where_gte`, `where_lte`, `where_between`
- Ordering: `order_by`, `order_asc`, `order_desc`
- Pagination: `limit`, `offset`, `page`, `paginate`
- JOINs: `inner_join`, `left_join`, `right_join`, `inner_join_as`, `left_join_as`
- Aggregations: `sum`, `avg`, `min`, `max`, `count`, `count_distinct`
- Scopes: `scope`, `when`, `when_some`

#### PostgreSQL Features

- JSON/JSONB column support
- Array column support (`Vec<T>`)
- `where_json_contains` - Query JSON fields
- `where_json_key_exists` / `where_json_key_not_exists` - Check JSON keys
- `where_array_contains` - Query array fields
- `where_array_overlaps` - Array overlap queries

#### Migrations

- `Migration` trait for defining migrations
- `Schema` builder for creating/altering tables
- `TableBuilder` with column types (id, string, text, integer, bigint, boolean, timestamp, json, etc.)
- Index creation and management
- `Migrator` for running migrations
- Migration tracking in `_migrations` table
- `run()`, `rollback()`, `rollback_steps()`, `reset()`, `refresh()` operations
- `status()` for viewing migration status

#### Schema Generation

- `SchemaGenerator` for generating SQL from models
- `#[index("column")]` macro for defining indexes
- `#[unique_index("column")]` macro for unique indexes
- Database introspection support

#### Soft Deletes

- `#[tideorm(soft_delete)]` attribute
- `with_trashed()` - Include soft-deleted records
- `only_trashed()` - Only soft-deleted records
- Manual restore via setting `deleted_at = None`

#### Upsert Operations

- `Model::insert_or_update()` - Simple upsert
- `Model::on_conflict()` - Advanced upsert with column control
- `update_columns()` - Specify which columns to update
- `update_all_except()` - Update all except specified columns

#### Batch Operations

- `Model::insert_all()` - Bulk insert

#### Transactions

- `Model::transaction()` - Execute operations in a transaction

#### Callbacks

- `before_save` / `after_save`
- `before_create` / `after_create`
- `before_update` / `after_update`
- `before_delete` / `after_delete`
- `before_validation` / `after_validation`

#### JSON Serialization

- `to_json()` - Convert model to JSON
- `collection_to_json()` - Convert collection to JSON array
- `to_hash_map()` - Convert to HashMap
- `#[tideorm(hidden = "field1,field2")]` - Hide fields from JSON output

#### Configuration

- `#[tideorm(table = "name")]` - Custom table name
- `#[tideorm(primary_key)]` - Mark primary key
- `#[tideorm(auto_increment)]` - Auto-increment field
- `#[tideorm(searchable = "fields")]` - Searchable fields
- `#[tideorm(translatable = "fields")]` - Translatable fields
- `#[tideorm(has_one_files = "field")]` - Single file attachment config
- `#[tideorm(has_many_files = "fields")]` - Multiple file attachments config

#### Raw SQL

- `Database::raw()` - Execute raw SQL returning models
- `Database::raw_with_params()` - Raw SQL with parameters
- `Database::execute()` - Execute SQL without return
- `Database::execute_with_params()` - Execute with parameters

### Documentation

- Comprehensive README with quick start guide
- Example files for common use cases
- API documentation

### Examples

- `basic.rs` - Basic CRUD operations
- `postgres_demo.rs` - PostgreSQL features demo
- `postgres_complete.rs` - Complete feature showcase
- `query_builder.rs` - Query builder examples
- `upsert_demo.rs` - Upsert operations
- `migrations.rs` - Migration examples
- `migration_test_runner.rs` - Migration test suite
- `attachments_translations_demo.rs` - File attachments and translations demo

#### File Attachments System

- New `attachments` module with `HasAttachments` trait
- `attach(relation, file_key)` - Attach a single file to a relation
- `attach_many(relation, file_keys)` - Attach multiple files at once (hasMany only)
- `attach_with_metadata(relation, FileAttachment)` - Attach with custom metadata
- `detach(relation, file_key)` - Detach a specific file or all files
- `detach_many(relation, file_keys)` - Detach multiple files at once
- `sync(relation, file_keys)` - Replace all files in a relation
- `sync_with_metadata(relation, attachments)` - Sync with custom metadata
- `get_file(relation)` - Get single file attachment (hasOne)
- `get_files(relation)` - Get all file attachments (hasMany)
- `has_files(relation)` / `count_files(relation)` - Check/count attachments
- `FileAttachment` struct with key, filename, created_at, and optional metadata
- `FilesData` container for managing attachment data

#### Translations System

- New `translations` module with `HasTranslations` trait
- Translations stored in JSONB format: `{field: {lang: value}}`
- `set_translation(field, lang, value)` - Set a translation for a field
- `set_translations(field, translations)` - Set multiple translations at once
- `sync_translations(field, translations)` - Replace all translations for a field
- `get_translation(field, lang)` - Get translation for specific language
- `get_translated(field, lang)` - Get translation with fallback chain
- `get_all_translations(field)` - Get all translations for a field
- `get_translations_for_language(lang)` - Get all fields for a language
- `remove_translation(field, lang)` - Remove a specific translation
- `remove_field_translations(field)` - Remove all translations for a field
- `clear_translations()` - Clear all translations
- `has_translation(field, lang)` / `has_any_translation(field)` - Check translations
- `available_languages(field)` - Get languages available for a field
- `to_translated_json(options)` - Convert to JSON with translations applied
- `to_json_with_all_translations()` - Get JSON including all translations
- `TranslationInput` helper for processing API/form data
- `ApplyTranslations` trait for bulk applying translations
- Configurable fallback language chain

#### Testing

- 269 unit tests covering all modules
- Comprehensive test coverage for attachments and translations
- Extended trait implementation tests with mock models
- Edge case tests (Unicode, RTL languages, long text, special characters)
- `attachments_translations_benchmarks.rs` - Performance benchmarks

---

## Links

- **Website:** [https://tideorm.com](https://tideorm.com)
- **Repository:** [https://github.com/mohamadzoh/tideorm](https://github.com/mohamadzoh/tideorm)
- **Documentation:** See README.md and examples/

[0.12.0]: https://github.com/mohamadzoh/tideorm/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/mohamadzoh/tideorm/compare/v0.10.2...v0.11.0
[0.10.2]: https://github.com/mohamadzoh/tideorm/compare/v0.10.1...v0.10.2
[0.10.1]: https://github.com/mohamadzoh/tideorm/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/mohamadzoh/tideorm/compare/v0.9.19...v0.10.0
[0.9.19]: https://github.com/mohamadzoh/tideorm/compare/v0.9.18...v0.9.19
[0.9.18]: https://github.com/mohamadzoh/tideorm/compare/v0.9.17...v0.9.18
[0.9.17]: https://github.com/mohamadzoh/tideorm/compare/v0.9.16...v0.9.17
[0.9.16]: https://github.com/mohamadzoh/tideorm/compare/v0.9.15...v0.9.16
[0.9.15]: https://github.com/mohamadzoh/tideorm/compare/v0.9.14...v0.9.15
[0.9.14]: https://github.com/mohamadzoh/tideorm/compare/v0.9.13...v0.9.14
[0.9.13]: https://github.com/mohamadzoh/tideorm/compare/v0.9.12...v0.9.13
[0.9.12]: https://github.com/mohamadzoh/tideorm/compare/v0.9.11...v0.9.12
[0.9.11]: https://github.com/mohamadzoh/tideorm/compare/v0.9.10...v0.9.11
[0.9.10]: https://github.com/mohamadzoh/tideorm/compare/v0.9.9...v0.9.10
[0.9.9]: https://github.com/mohamadzoh/tideorm/compare/v0.9.8...v0.9.9
[0.9.8]: https://github.com/mohamadzoh/tideorm/compare/v0.9.7...v0.9.8
[0.9.3]: https://github.com/mohamadzoh/tideorm/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/mohamadzoh/tideorm/compare/v0.9.1...v0.9.2
[0.9.0]: https://github.com/mohamadzoh/tideorm/compare/v0.8.9...v0.9.0
[0.8.9]: https://github.com/mohamadzoh/tideorm/compare/v0.8.8...v0.8.9
[0.8.8]: https://github.com/mohamadzoh/tideorm/compare/v0.8.7...v0.8.8
[0.8.7]: https://github.com/mohamadzoh/tideorm/compare/v0.8.6...v0.8.7
[0.8.6]: https://github.com/mohamadzoh/tideorm/compare/v0.8.5...v0.8.6
[0.8.5]: https://github.com/mohamadzoh/tideorm/compare/v0.8.4...v0.8.5
[0.8.4]: https://github.com/mohamadzoh/tideorm/compare/v0.8.1...v0.8.4
[0.8.1]: https://github.com/mohamadzoh/tideorm/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/mohamadzoh/tideorm/compare/v0.7.3...v0.8.0
[0.7.3]: https://github.com/mohamadzoh/tideorm/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/mohamadzoh/tideorm/compare/v0.7.0...v0.7.2
[0.1.0]: https://github.com/mohamadzoh/tideorm/releases/tag/v0.1.0
