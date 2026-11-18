# Full-Text Search

Full-text search finds the rows whose text columns hold the words of a search, ranked by how well they match. It uses each backend's own engine: `tsvector`/`tsquery` on PostgreSQL, `FULLTEXT` indexes on MySQL and MariaDB, and FTS5 on SQLite.

It requires the `fulltext` feature:

```toml
tideorm = { version = "0.18.0", features = ["postgres", "fulltext"] }
```

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub body: String,
}

async fn search_posts(text: &str) -> tideorm::Result<()> {
    let ranked = Post::search((Post::TITLE, Post::BODY), text)
        .limit(10)
        .get_ranked()
        .await?;

    for result in ranked {
        println!("{} ({:.2})", result.record.title, result.rank);
    }
    Ok(())
}
```

The search types are in the prelude and in `tideorm::fulltext`.

## Creating the Index

MySQL and MariaDB search only columns a `FULLTEXT` index covers, and SQLite searches the FTS5 table the index creates, so create the index before the first search. PostgreSQL searches without one, but reads every row. `FullTextIndex` writes the statements for each backend; run them in a [migration](migrations.md):

```rust
use tideorm::prelude::*;
use tideorm::fulltext::FullTextIndex;
use tideorm::migration::*;

struct AddPostSearch;

#[async_trait]
impl Migration for AddPostSearch {
    fn version(&self) -> &str { "20260301_001" }
    fn name(&self) -> &str { "add_post_search" }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        let index = FullTextIndex::new("idx_posts_search", "posts", ["title", "body"]);
        for statement in index.to_sql(schema.database_type()) {
            schema.raw(&statement).await?;
        }
        Ok(())
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.raw("DROP INDEX idx_posts_search").await
    }
}
```

`to_sql(db_type)` returns the statements in the order to run them. Each backend also has its own method:

| Backend | Method | Statements |
|---|---|---|
| PostgreSQL | `to_postgres_sql()` | `CREATE INDEX "idx_posts_search" ON "posts" USING GIN ((to_tsvector('english', ..)))` |
| MySQL, MariaDB | `to_mysql_sql()` | ``CREATE FULLTEXT INDEX `idx_posts_search` ON `posts`(`title`, `body`)`` |
| SQLite | `to_sqlite_sql()` | the FTS5 table `posts_fts`, the triggers that keep it in step with `posts`, and a rebuild that fills it with the rows already there |

The `down` above suits PostgreSQL; MySQL and MariaDB write `DROP INDEX idx_posts_search ON posts`, and on SQLite you drop the `posts_fts` table and its triggers `posts_ai`, `posts_ad` and `posts_au`.

Options:

- `.language("english")` sets the PostgreSQL text search configuration (`english` by default). A search uses the index only when it names the same columns and language.
- `.pg_index_type(PgFullTextIndexType::GIN)` or `GiST` chooses the PostgreSQL index type (GIN by default).
- The public `config` field is a `FullTextIndexConfig` with `language`, `pg_index_type` and `mysql_parser`. Set `index.config.mysql_parser = Some("ngram".into())` to add a `WITH PARSER` clause naming that parser to the MySQL statement.

### SQLite Indexes

- A table has one full-text index, the `<table>_fts` table its searches read, whatever the index is named.
- Applying the same index again changes nothing. One over a column the existing index does not hold fails with `no such column` instead of keeping the old columns; drop `<table>_fts` and its triggers to index other columns. A trigger of another definition under one of those names is refused too.
- A table named with its attached database (`tenant.posts`) gets its index in that database.
- The index follows the table's rowid, which `VACUUM` keeps only for a table with an `INTEGER PRIMARY KEY`. After vacuuming a table keyed another way (a UUID or text key), run `index.sqlite_rebuild_sql()` before anything else touches it, or searches return other rows.
- A table that declares a column named `rowid` is supported only when that column is its `INTEGER PRIMARY KEY`; otherwise the index, the rebuild and every search fail.
- The statements check these rules through `tideorm_raise`, a function TideORM's SQLite connections define, so run them through TideORM (a migration or `Database::execute`), not another SQLite client.

## Searching

`Model::search(columns, text)` starts a search. The columns are names (`["title", "body"]`) or typed columns (`(Post::TITLE, Post::BODY)`), as for `select`.

| Method | Returns |
|---|---|
| `get()` | `Vec<Post>`, the matching rows |
| `get_ranked()` | `Vec<SearchResult<Post>>`, best match first |
| `first()` | `Option<Post>`, the first match |
| `count()` | `u64`, the number of matching rows |

```rust,ignore
let posts = Post::search(["title", "body"], "rust programming").get().await?;
let total = Post::search(["title", "body"], "rust").count().await?;
let first = Post::search(["title"], "rust").first().await?;
let page_two = Post::search(["title", "body"], "rust").limit(20).offset(20).get().await?;
```

A `SearchResult` holds the `record`, its `rank` (an `f64`) and the `highlights` described [below](#highlighting).

A search on a soft-delete model leaves trashed rows out, as a query does. `with_trashed()` reads them too and `only_trashed()` reads nothing else:

```rust,ignore
let binned = Post::search(["title"], "rust").only_trashed().get().await?;
```

`#[tideorm(searchable = "title,body")]` on a model records a column list in `Post::searchable_fields()`. A search does not read it on its own; pass it as the columns: `Post::search(Post::searchable_fields(), text)`.

## Search Modes

Every mode builds the backend's syntax from the words of the search text, so an operator a user types is never read as syntax.

```rust,ignore
use tideorm::fulltext::SearchMode;

// Natural language (the default)
Post::search(["body"], "learn rust programming").get().await?;

// Every term required, "-term" excluded
Post::search(["body"], "+rust +async -javascript").mode(SearchMode::Boolean).get().await?;

// The words in order
Post::search(["body"], "async await").mode(SearchMode::Phrase).get().await?;

// Words beginning with each search word, for autocomplete
Post::search(["title"], "prog").mode(SearchMode::Prefix).get().await?;

// The words within 5 words of each other, in either order
Post::search(["body"], "borrow checker").mode(SearchMode::Proximity(5)).get().await?;
```

| Mode | Matches |
|---|---|
| `Natural` | the words as the backend's natural-language search reads them: PostgreSQL and SQLite require every word, MySQL and MariaDB rank rows holding any of them |
| `Boolean` | every term; a term written `-term` or `-"a phrase"` is left out, and a quoted phrase matches as a phrase. MySQL and MariaDB also read their `~ < > *` operators, and there a term without `+` is optional. On SQLite a search of exclusions only matches nothing, since FTS5's `NOT` needs something to subtract from |
| `Phrase` | the words, in order, as one phrase |
| `Prefix` | words beginning with each search word, all of them required |
| `Proximity(n)` | the words within `n` words of each other, in either order; at most 64 on PostgreSQL |

A search with no word left to search for matches nothing on every backend.

| Mode | PostgreSQL | MySQL, MariaDB | SQLite (FTS5) |
|---|---|---|---|
| `Natural` | `plainto_tsquery` | `MATCH (..) AGAINST (?)` | each word quoted |
| `Boolean` | `to_tsquery` of the terms | `IN BOOLEAN MODE`, the terms sanitized | `AND` and `NOT` |
| `Phrase` | `phraseto_tsquery` | `IN BOOLEAN MODE`, `"the words"` | one phrase |
| `Prefix` | `to_tsquery` with `:*` prefixes | `IN BOOLEAN MODE`, `+word*` for each word | `word*` for each word |
| `Proximity(n)` | `to_tsquery` with the `<n>` distances, both orders | `IN BOOLEAN MODE`, `"the words" @n` | `NEAR(.., n - 1)` |

On PostgreSQL a `to_tsquery` mode that leaves no word after sanitizing falls back to `plainto_tsquery`. On SQLite the search reads only the named columns of the FTS5 table, through a column filter.

## Configuration

`FullTextConfig` sets the mode, the language, term filters and weights together:

```rust,ignore
use tideorm::fulltext::{FullTextConfig, SearchMode};

let config = FullTextConfig::new()
    .language("english")
    .mode(SearchMode::Boolean)
    .stop_words(["the", "a", "of"])
    .min_word_length(3);

// Searches for "rust" and "ownership" only
let posts = Post::search(["body"], "the rust of ownership").config(config).get().await?;
```

The builder also takes `.mode(..)` and `.language(..)` directly.

- **`language`** is the PostgreSQL text search configuration (`english` by default). It is written into the statement as a constant, the way `FullTextIndex` writes it into the index, so a search can use that index; a name that is not a configuration name (identifiers separated by dots) fails the search. MySQL, MariaDB and SQLite ignore it.
- **`stop_words`**, **`min_word_length`** and **`max_word_length`** leave terms out of the search text before it is sent, on every backend; the index is untouched. Stop words compare without regard to case, and the operators around a term (`+`, `-`, `*`, quotes, parentheses) count toward neither its length nor its stop-word match. A quoted phrase, and a `Phrase` search, is sent as written. A search whose every term is left out matches nothing.
- **`weights`** are PostgreSQL's rank weights; see [Ranking](#ranking).

## Ranking

`get_ranked()` ranks on every backend and returns the best match first. A higher rank is a better match everywhere, but the numbers mean different things per backend, so compare ranks only within one backend:

| Backend | Rank |
|---|---|
| PostgreSQL | `ts_rank_cd(weights, tsvector, tsquery)` |
| MySQL, MariaDB | the `MATCH (..) AGAINST (..)` relevance |
| SQLite | `-bm25(<table>_fts)`, the negated FTS5 score |

- `min_rank(rank)` drops results scoring below `rank`. Only `get_ranked()` applies it.
- `with_ranking()` orders `get()` and `first()` by relevance on PostgreSQL. MySQL, MariaDB and SQLite ignore it; use `get_ranked()` there.
- `SearchWeights::new(a, b, c, d)` (default `1.0, 0.4, 0.2, 0.1`) sets PostgreSQL's weights for the four label categories. TideORM builds the `tsvector` without `setweight`, so every word carries the `D` label: only `d` counts, and it scales every rank by the same factor. It moves the score `min_rank` compares against, not the order.

```rust,ignore
let strong = Post::search(["title", "body"], "rust async")
    .min_rank(0.1)
    .limit(10)
    .get_ranked()
    .await?;
```

## Highlighting

`highlight(config)` marks the whole-word matches in the searched columns of each `get_ranked()` result, without regard to case. The record's text is HTML-escaped around the tags, so a stored `<script>` reaches the page as text:

```rust,ignore
use tideorm::fulltext::HighlightConfig;

let ranked = Post::search((Post::TITLE, Post::BODY), "rust async")
    .highlight(HighlightConfig::default())
    .get_ranked()
    .await?;

for field in &ranked[0].highlights {
    println!("{}: {} ({} matches)", field.field, field.highlighted, field.match_count);
}
```

Each `HighlightedField` holds the column (`field`), the marked text (`highlighted`), the text as stored (`original`) and the number of matches (`match_count`). A term a `Boolean` search excludes is not marked.

| `HighlightConfig` | Default | Effect |
|---|---|---|
| `.tags(start, end)` | `<mark>`, `</mark>` | the tags around each match |
| `.fragment_words(n)` | 10 | keep `n` words on each side of the first match |
| `.whole_text()` | off | mark the whole text instead of a fragment |
| `.max_length(chars)` | none | cut the result to `chars` characters, followed by `...` |
| `.escape_html(bool)` | `true` | turn escaping off for tags that are not HTML |

`HighlightConfig::highlight(text, query)` marks any text the same way, escaping it as a search does:

```rust,ignore
let marked = HighlightConfig::default().whole_text().highlight("a <script> about the fox", "fox");
assert_eq!(marked, "a &lt;script&gt; about the <mark>fox</mark>");
```

`highlight_text` and `generate_snippet` mark text without escaping it, so keep them for text that is not HTML:

```rust,ignore
use tideorm::fulltext::{generate_snippet, highlight_text};

let text = "The quick brown fox jumps over the lazy dog.";

let marked = highlight_text(text, "fox lazy", "<mark>", "</mark>");
// "The quick brown <mark>fox</mark> jumps over the <mark>lazy</mark> dog."

let snippet = generate_snippet(text, "fox", 2, "<b>", "</b>");
// "...quick brown <b>fox</b> jumps over..."
```

`generate_snippet(text, query, fragment_words, start, end)` keeps `fragment_words` words on each side of the first match, with `...` where it cut; with no match it keeps the opening words.

### PostgreSQL Headlines

`pg_headline_sql(column, query, language, start_tag, end_tag)` returns a `ts_headline` expression for the server to mark the matches, and the language, search text and options it binds as `$1` to `$3`:

```rust,ignore
use tideorm::fulltext::pg_headline_sql;

let (headline, params) = pg_headline_sql("body", "search query", "english", "<b>", "</b>");
// ts_headline($1::regconfig, "body", plainto_tsquery($1::regconfig, $2), $3)

let rows = Database::raw_json_with_params(
    &format!("SELECT id, {headline} AS headline FROM posts"),
    params,
)
.await?;
```

The options are `StartSel`, `StopSel`, `MaxWords=35` and `MinWords=15`; the tags are double-quoted inside them, so a tag cannot add an option of its own. The text is not escaped: `ts_headline` returns the column as stored. See [Raw SQL](raw-sql.md) for `raw_json_with_params`.

## MySQL and MariaDB Operators

MySQL's boolean-mode operators (`+ - < > ( ) ~ * " @`) are sanitized before the text reaches `MATCH .. AGAINST`. In `Boolean` mode each term keeps one leading `+ - ~ < >` and a trailing `*`, and a quoted phrase stays a phrase; in the other modes the operator characters become spaces before TideORM writes its own. A search left with no searchable term matches nothing instead of failing with a syntax error.

## Limits

- An [encrypted field](encrypted-fields.md) cannot be searched: an index over randomized ciphertext matches no search text. A search naming one fails, and so does a `searchable` list naming one, at compile time.
- `get()` and `get_ranked()` are held to the `max_rows` [project rule](project-rules.md): with the rule set, a search needs a `limit()` within it.
