use super::*;

use crate::query::Order;

/// A query's models in batches, read one batch at a time: what
/// [`QueryBuilder::chunks`] returns.
///
/// Between batches the caller may hold anything across an `.await`, which a
/// [`chunk`](QueryBuilder::chunk) callback cannot.
#[must_use = "a chunk cursor reads nothing until `next()` is awaited"]
pub struct Chunks<M: Model> {
    /// The query each batch narrows, ordered by the cursor column; `None`
    /// once the rows have run out.
    query: Option<QueryBuilder<M>>,
    chunk_size: u64,
    cursor_column: String,
    order: Order,
    /// The rows the query's own `limit()` still allows.
    remaining: Option<u64>,
    /// A join can repeat a row's primary key. A full batch that ends inside
    /// such a run hands its tail to the next batch, which starts at that key.
    joins_repeat_keys: bool,
    /// The last key handed out, and whether the next batch starts at it
    /// rather than after it.
    cursor: Option<(serde_json::Value, bool)>,
}

impl<M: Model> Chunks<M> {
    /// The next batch of at most the chunk size, or `None` once the query has
    /// no more rows.
    pub async fn next(&mut self) -> Result<Option<Vec<M>>> {
        let Some(base_query) = &self.query else {
            return Ok(None);
        };
        let chunk_size = self.chunk_size;
        let batch_limit = self
            .remaining
            .map_or(chunk_size, |limit| std::cmp::min(limit, chunk_size));
        if batch_limit == 0 {
            self.query = None;
            return Ok(None);
        }

        let mut batch_query = base_query.clone().limit(batch_limit);
        if let Some((key, inclusive)) = &self.cursor {
            let column = self.cursor_column.as_str();
            let key = key.clone();
            batch_query = match (self.order, inclusive) {
                (Order::Asc, false) => batch_query.where_gt(column, key),
                (Order::Asc, true) => batch_query.where_gte(column, key),
                (Order::Desc, false) => batch_query.where_lt(column, key),
                (Order::Desc, true) => batch_query.where_lte(column, key),
            };
        }

        let mut batch = batch_query.get().await?;
        let Some(last) = batch.last() else {
            self.query = None;
            return Ok(None);
        };
        let last_key = key_of(last)?;
        let fetched = batch.len() as u64;

        let mut starts_at_last_key = false;
        // Rows may follow a full batch unless the limit ends with it.
        let more_may_follow = self.remaining.is_none_or(|limit| limit > batch_limit);
        if self.joins_repeat_keys && fetched == batch_limit && more_may_follow {
            let mut tail = 0;
            for model in batch.iter().rev() {
                if key_of(model)? != last_key {
                    break;
                }
                tail += 1;
            }
            if tail == batch.len() {
                // Every row shares one key: read that key's whole run, which
                // fails only when a batch cannot hold it.
                let run = base_query
                    .clone()
                    .where_eq(self.cursor_column.as_str(), last_key.clone())
                    .limit(chunk_size.saturating_add(1))
                    .get()
                    .await?;
                if run.len() as u64 > chunk_size {
                    return Err(Error::query(format!(
                        "chunk({}) found more rows with the key {} than a batch holds: the join repeats it, so pass a larger chunk size",
                        chunk_size, last_key
                    )));
                }
                batch = run;
            } else {
                batch.truncate(batch.len() - tail);
                starts_at_last_key = true;
            }
        }

        if let Some(limit) = &mut self.remaining {
            *limit = limit.saturating_sub(batch.len() as u64);
        }
        if self.remaining == Some(0) || fetched < batch_limit {
            self.query = None;
        }
        self.cursor = Some((last_key, starts_at_last_key));
        Ok(Some(batch))
    }
}

/// The primary key of `model`, as the cursor compares it.
fn key_of<M: Model>(model: &M) -> Result<serde_json::Value> {
    serde_json::to_value(model.primary_key()).map_err(Error::from)
}

/// One page of a query's models read after a cursor, as
/// [`QueryBuilder::cursor_paginate`] reads it. Serializes as
/// `{"items": [..], "next_cursor": ".." | null}`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CursorPage<M> {
    /// The models on this page.
    pub items: Vec<M>,
    /// The cursor that reads the next page; `None` once a page comes back
    /// short. A page that ends exactly at the last row is followed by an
    /// empty one.
    pub next_cursor: Option<String>,
}

/// A chunk cursor as text a client hands back: the key and whether the next
/// page starts at it, as base64url JSON.
fn encode_cursor((key, inclusive): &(serde_json::Value, bool)) -> String {
    crate::tokenization::base64_url_encode(
        serde_json::json!([key, inclusive]).to_string().as_bytes(),
    )
}

fn decode_cursor(cursor: &str) -> Result<(serde_json::Value, bool)> {
    crate::tokenization::base64_url_decode(cursor)
        .and_then(|bytes| serde_json::from_slice::<(serde_json::Value, bool)>(&bytes).ok())
        .filter(|(key, _)| key.is_number() || key.is_string())
        .ok_or_else(|| Error::validation("cursor", "is not a cursor this query issued"))
}

impl<M: Model> QueryBuilder<M> {
    /// Read the matching models in batches of `chunk_size`, one batch per
    /// [`Chunks::next`], without loading the full result set into memory.
    ///
    /// The batches are [`chunk`](Self::chunk)'s, with its rules; a query it
    /// refuses is refused here, before any row is read. Unlike a `chunk`
    /// callback, the loop may hold any state across an `.await`.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let mut names = Vec::new();
    /// let mut batches = User::query().chunks(500)?;
    /// while let Some(batch) = batches.next().await? {
    ///     for user in batch {
    ///         tokio::task::yield_now().await;
    ///         names.push(user.name);
    ///     }
    /// }
    /// assert_eq!(names, ["Ada"]);
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub fn chunks(self, chunk_size: u64) -> Result<Chunks<M>> {
        self.ensure_query_is_executable()?;
        self.ensure_query_is_chunkable(chunk_size)?;
        if self.clauses.rule_scope.checks_rows() {
            crate::rules::ensure_rows_allowed::<M>(
                format_args!("a batch of {chunk_size}"),
                Some(chunk_size),
            )?;
        }

        let primary_key = Self::chunk_primary_key_column()?;
        self.ensure_chunk_key_is_original(primary_key)?;
        let cursor_column = format!("{}.{}", M::table_name(), primary_key);
        let order = self.chunk_order(primary_key)?;
        let remaining = self.clauses.limit_value;
        let mut query = self;
        query.clauses.limit_value = None;
        query.clauses.offset_value = None;
        if query.clauses.order_by.is_empty() {
            query = query.order_by(cursor_column.as_str(), order);
        }
        let joins_repeat_keys = !query.clauses.joins.is_empty();
        // A batch that ends in a run of one key reads a row past the chunk size.
        if query.clauses.rule_scope == crate::rules::RuleScope::Developer {
            query.clauses.rule_scope = crate::rules::RuleScope::RowsChecked;
        }
        Ok(Chunks {
            query: Some(query),
            chunk_size,
            cursor_column,
            order,
            remaining,
            joins_repeat_keys,
            cursor: None,
        })
    }

    /// One page of at most `per_page` models, keyset-paged by the primary key:
    /// pass `None` for the first page, then each page's
    /// [`next_cursor`](CursorPage::next_cursor). Unlike
    /// [`paginate`](Self::paginate) it counts nothing and skips no rows by
    /// OFFSET, so a deep page costs what the first does, and a row written
    /// between requests neither repeats nor skips another.
    ///
    /// The query follows [`chunk`](Self::chunk)'s rules, and `limit()` is
    /// refused: a page holds `per_page` rows. A cursor is opaque text for a
    /// client, not a secret: it names the key to continue from, and the
    /// query's filters still apply. One that does not decode fails with
    /// [`Error::Validation`] on the field `cursor`.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name) VALUES (1, 'Ada'), (2, 'Grace'), (3, 'Edsger')").await?;
    /// let first = User::query().cursor_paginate(2, None).await?;
    /// assert_eq!(first.items.len(), 2);
    /// let second = User::query().cursor_paginate(2, first.next_cursor.as_deref()).await?;
    /// assert_eq!(second.items[0].name, "Edsger");
    /// assert_eq!(second.next_cursor, None);
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn cursor_paginate(
        self,
        per_page: u64,
        cursor: Option<&str>,
    ) -> Result<CursorPage<M>> {
        if self.clauses.limit_value.is_some() {
            return Err(Error::query(
                "cursor_paginate() does not support limit(): each page holds per_page rows",
            ));
        }
        let mut pages = self.chunks(per_page)?;
        if let Some(cursor) = cursor {
            pages.cursor = Some(decode_cursor(cursor)?);
        }
        let items = pages.next().await?.unwrap_or_default();
        let next_cursor = match (&pages.query, &pages.cursor) {
            (Some(_), Some(cursor)) => Some(encode_cursor(cursor)),
            _ => None,
        };
        Ok(CursorPage { items, next_cursor })
    }

    /// Process matching models in batches without loading the full result set into memory.
    ///
    /// The traversal uses the model's single-column primary key as a cursor, so callbacks may
    /// safely update or delete already-processed rows without causing later batches to skip.
    /// Existing filters, caching, and any pre-applied `limit()` remain in effect. When you need
    /// descending traversal, order explicitly by the primary key before calling `chunk()`.
    ///
    /// A join can repeat a primary key: the rows sharing one key are always handed to the
    /// callback in the same batch, so a key that repeats more often than `chunk_size` fails the
    /// call. A `union()` is refused, since its other branches do not follow the cursor.
    /// GROUP BY and HAVING are refused because the cursor would change group membership.
    /// Window functions are also refused: each cursor step would change their input.
    /// The projection must keep the original primary key; aliasing a different
    /// value to its column name is refused before any callback runs.
    ///
    /// The closure may update state before it returns its future; to hold
    /// state across an `.await` between batches, loop over
    /// [`chunks`](Self::chunks) instead.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).chunk(10, |rows| async move { assert!(!rows.is_empty()); Ok(()) }).await?;
    /// // State the closure updates before it returns its future lasts across batches.
    /// let mut seen = 0;
    /// User::query().chunk(10, |rows| { seen += rows.len(); async { Ok(()) } }).await?;
    /// assert_eq!(seen, 1);
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn chunk<F, Fut>(self, chunk_size: u64, mut callback: F) -> Result<()>
    where
        F: FnMut(Vec<M>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let mut chunks = self.chunks(chunk_size)?;
        while let Some(batch) = chunks.next().await? {
            callback(batch).await?;
        }
        Ok(())
    }
}
