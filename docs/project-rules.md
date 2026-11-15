# Project Rules

Project rules let a team write down the shape its models and queries must keep, such as "no model over 20 columns", "at most 2 JSON columns per model" or "no raw SQL", and have TideORM refuse whatever breaks them. Model rules fail the build; query rules fail the query before it is sent.

Nothing is enforced by default. Without a `[rules]` table every model and query is allowed.

## Writing Rules

Rules go in a `[rules]` table of `tideorm.toml`, the file the [CLI](https://github.com/mohamadzoh/tideorm-cli) already reads:

```toml
[rules]
max_columns = 20
max_json_columns = 2
max_relation_depth = 3
sensitive_fields = ["password", "token", "secret"]
require_foreign_key_index = true
max_rows = 1000
forbid_raw_sql = true

[rules.allow]
# Exceptions: each model named here is exempt from the rules listed.
LegacyOrder = ["max_columns", "max_json_columns"]
Report = ["forbid_raw_sql"]
```

A rule left out of `[rules]`, or set to `false`, is off.

`[rules.allow]` keys are model struct names without their module path, so two structs of one name share their exceptions; a key naming no model exempts nothing.

TideORM uses the first `tideorm.toml` it finds, starting in the directory of the crate that declares the models and walking up, so one file at a workspace root covers every member crate. A dependency fetched from crates.io, git or a vendor directory reads none: the rules hold your own models. When crates of one binary read different files, a raw statement that names no model is refused if any of them forbids raw SQL.

## Model Rules

Model rules are checked as the models compile, so `cargo check`, like every build, fails on a model that breaks one. Each broken rule is an error of its own at the model, field or relation that breaks it, naming the rule:

```text
error: Account has 6 columns, over the limit of 5 (max_columns in tideorm.toml)
 --> src/models/account.rs:7:12
  |
7 | pub struct Account {
  |            ^^^^^^^
```

The rules that read a field's type (`max_json_columns`, `primary_key_types`, `forbid_float_columns`) or other models (`max_relation_depth`, `forbid_relation_cycles`, `require_foreign_key_index`) are checked by the compiler as it evaluates constants, so their errors read `error[E0080]: evaluation panicked: ..` with the same message. Nothing is checked when the application starts.

| Rule | Value | Refuses |
|---|---|---|
| `max_columns` | number | A model with more columns |
| `max_relations` | number | A model declaring more relations |
| `max_json_columns` | number | More `JSON`/`JSONB` columns in one model |
| `max_nullable_columns` | number | More nullable columns in one model |
| `max_indexes` | number | More declared indexes, unique ones included |
| `max_identifier_length` | number | A table, column or index name longer than this many bytes (PostgreSQL cuts names at 63) |
| `snake_case_names` | `true` | A table or column name that is not lowercase `snake_case` |
| `avoid_reserved_words` | `true` | A table or column named by an SQL reserved word (`order`, `user`, `group`, ..) |
| `primary_key_types` | list of `"integer"`, `"bigint"`, `"uuid"`, `"string"` | A primary key column of any other type |
| `forbid_composite_primary_keys` | `true` | A primary key of several columns |
| `require_timestamps` | `true` | A model without `created_at`/`updated_at` |
| `require_soft_delete` | `true` | A model without soft delete |
| `require_foreign_key_index` | `true` | A relation's foreign key column that does not lead an index (or the primary key) of its table |
| `sensitive_fields` | list of words | A column whose name contains one of the words (any case) and that is neither `hidden` nor [encrypted](encrypted-fields.md) |
| `forbid_float_columns` | `true` | `f32`/`f64` columns, which round money and counts; use a decimal or an integer |
| `max_relation_depth` | number | A longer chain of owned relations from the model |
| `forbid_relation_cycles` | `true` | A model that its owned relations lead back to |

Relation depth and cycles follow owned [relations](relations.md) only: `has_one`, `has_many`, `morph_one` and `morph_many`. A `belongs_to`, `has_many_through`, `morph_to` or self-reference points at rows the model does not own, so it adds no depth and closes no cycle; nor does an owned relation to the model's own table (a tree's children), whose depth is in the data. A relation leads to the struct it names, not to other structs reading the same table, and a foreign key must lead an index (or the primary key) that the struct holding the key declares.

## Query Rules

Query rules are checked as each query runs, and by `validate()`. A broken one fails the query with `Error::Query` before anything is sent.

| Rule | Value | Refuses |
|---|---|---|
| `max_rows` | number, 1 or more | A read of more rows: `get()`, `all()`, `get_json()` or `pluck()` without a `limit()` within it, `paginate()` with a larger page, `chunk()` with a larger batch, a full-text `search()` without such a `limit()`. `count()`, `exists()` and aggregates are not limited |
| `max_joins` | number | A query with more joins |
| `max_in_values` | number | A `where_in`/`where_not_in` listing more values |
| `max_eager_depth` | number | `with("a.b.c")` nesting deeper (that one is 3) |
| `max_eager_relations` | number | One eager load resolving more relations, at every level |
| `forbid_raw_sql` | `true` | Raw SQL: `where_raw`, `select_raw`, `order_by_raw`, `having_raw`, `union_raw`, a CTE written as SQL, `lag_with_raw_default`/`lead_with_raw_default`, a `WindowFunctionType::Custom` window function, a batch update's `set_trusted_raw`, and the raw statements of `Database` (`execute`, `raw`, `raw_json`, `raw_as`, their `_with_params` forms, and the handle forms `exec_raw`, `query_raw`, `query_raw_json` and `query_raw_as`) |

A query is held to the rules of its own model. A subquery, union operand or CTE is held to its own model's rules too, so `where_exists(Report::query().where_raw(..))` runs only if `Report` may use raw SQL.

To find out whether a query passes before running it, call `validate()` (see [Logging and Profiling](profiling.md)).

## What the Rules Leave Alone

- **TideORM's own statements.** The lookups TideORM writes itself (eager loading's key queries, existence checks, pivot reads) are not held to the query rules. A relation's `load()` reads all of the relation's rows, and refuses only raw SQL a `load_with` closure adds.
- **Migrations.** A migration's statements, `Schema::raw` included, may be raw SQL.
- **Raw statements that name no model.** `Database::execute`, `raw_json` and `raw_as` have no model to exempt, so they are refused whenever the project forbids raw SQL, even when some models are exempt. `Database::raw::<T>` is held to `T`'s rules.

## When Rules Change

The rules are read while the models compile and are built into the binary, so changing them needs a rebuild. Cargo rebuilds by itself when an existing `tideorm.toml` changes. When you create the file for the first time, touch a source file (or run `cargo clean -p your-crate`) so the models compile again and read it.

A mistake in `[rules]` (an unknown rule, a wrong value, an exception naming an unknown rule, invalid TOML) is a compile error on each model, naming the file and listing the valid rules.
