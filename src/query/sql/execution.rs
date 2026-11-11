use super::*;

use std::future::Future;

use crate::logging::EngineLog;

mod chunks;
mod deletes;
mod hash_helpers;
mod mutation_safety;
mod reads;
mod shaped_reads;

pub use chunks::{Chunks, CursorPage};
pub use shaped_reads::Paginated;

use hash_helpers::{hash_or_group, hash_where_condition};

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    fn chunk_primary_key_column() -> Result<&'static str> {
        match M::primary_key_names() {
            [primary_key] => Ok(*primary_key),
            _ => Err(Error::query(format!(
                "chunk() only supports models with a single-column primary key; model '{}' uses {} key columns",
                M::table_name(),
                M::primary_key_names().len()
            ))),
        }
    }

    /// The direction `chunk()` pages in: the query's ordering, which may only
    /// be by the model's primary key, using its Rust field or database column
    /// name, optionally qualified by the model's table; otherwise ascending.
    fn chunk_order(&self, primary_key: &str) -> Result<crate::query::Order> {
        match self.clauses.order_by.as_slice() {
            [] => Ok(crate::query::Order::Asc),
            [(column, direction)] if M::own_column_name(column) == Some(primary_key) => {
                Ok(*direction)
            }
            _ => Err(Error::query(format!(
                "chunk() only supports explicit ordering by the single primary key '{}' for model '{}'",
                primary_key,
                M::table_name()
            ))),
        }
    }

    /// Cache this query's models for `ttl` in the global [`QueryCache`](crate::QueryCache).
    ///
    /// The cache starts disabled: until `QueryCache::global().enable()` or
    /// `QueryCache::init_global(..)` turns it on, nothing is cached, and the
    /// first such query logs a warning saying so.
    ///
    /// A TideORM write drops the cached reads of its table. Inside a
    /// transaction it drops them again once the transaction commits, since
    /// other requests read, and may cache, the replaced rows until then, and a
    /// read that a write overtook is not cached at all. Reads inside a
    /// transaction, and locked reads, bypass the cache. Writes TideORM does not
    /// make, from another process or straight through the driver, are only
    /// seen once the TTL expires.
    ///
    /// Entries are kept as JSON, so rows it cannot hold exactly, with a NaN or
    /// infinite float or a JSON `null` in an `Option` field, are not cached
    /// and are read from the database each time.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().cache(std::time::Duration::from_secs(30)).validate().unwrap();
    /// ```
    pub fn cache(mut self, ttl: std::time::Duration) -> Self {
        self.clauses.cache_options = Some(crate::cache::CacheOptions::new(ttl));
        self
    }

    /// [`cache()`](Self::cache), with `key` as part of the entry's identity.
    ///
    /// The key adds to the query's shape rather than replacing it: reads
    /// share an entry only when both their keys and their queries match, so
    /// `first()`, `find(id)`, each page of `paginate()` and each batch of
    /// `chunk()` still get entries of their own. Use it when something the
    /// SQL does not show decides the rows, such as a session setting or a
    /// row-level security role, so reads under different values never share
    /// one.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().cache_with_key("users:active", std::time::Duration::from_secs(30)).validate().unwrap();
    /// ```
    pub fn cache_with_key(mut self, key: &str, ttl: std::time::Duration) -> Self {
        self.clauses.cache_key = Some(key.to_string());
        self.clauses.cache_options = Some(crate::cache::CacheOptions::new(ttl));
        self
    }

    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().no_cache().validate().unwrap();
    /// ```
    pub fn no_cache(mut self) -> Self {
        self.clauses.cache_options = None;
        self.clauses.cache_key = None;
        self
    }

    /// Validate the builder and reject conditions that cannot be rendered.
    ///
    /// `ensure_query_is_valid` only checks fragment syntax. A condition whose
    /// operator and value do not pair up renders to nothing at all, so it is
    /// rejected here instead of being silently dropped from the WHERE clause.
    pub(crate) fn ensure_query_is_executable(&self) -> Result<()> {
        self.ensure_query_is_valid()?;
        self.ensure_conditions_are_representable()
    }

    /// Mix the identity of the connection this query runs on into the cache key.
    ///
    /// Without this, `query_with(&tenant_a).cache()` and `query_with(&tenant_b)`
    /// hash identically and one tenant's rows get served to the other, and so
    /// would a query cached before `Database::set_global()` switched databases.
    /// What is mixed in is the identity the pool was given when it opened,
    /// never its address: an allocator reuses an address as soon as the
    /// connection at it is dropped, so tenant A's closed connection and tenant
    /// B's freshly opened one can hash the same and B would be served A's rows
    /// for the rest of the TTL.
    fn hash_database_identity<H: std::hash::Hasher>(&self, hasher: &mut H) {
        use std::hash::Hash;

        let database = match self.current_db() {
            Ok(database) => database,
            Err(_) => {
                "tideorm::no-connection".hash(hasher);
                return;
            }
        };

        match database.current_inner() {
            Ok(connection) => connection.connection().identity().hash(hasher),
            // A transaction handle carries no pooled identity of its own; keep it
            // out of the ambient-connection keyspace rather than sharing it.
            Err(_) => "tideorm::unresolved-connection".hash(hasher),
        }
    }

    /// True when this query will execute inside a transaction scope.
    ///
    /// The handle the query will actually use is inspected, so an explicitly
    /// attached transaction (`query_with(&tx)`) is recognised as well as the
    /// ambient override that `Database::transaction` installs.
    fn runs_in_transaction(&self) -> bool {
        let Ok(database) = self.current_db() else {
            return false;
        };

        matches!(
            database.__get_connection(),
            Ok(crate::database::ConnectionRef::Transaction(..))
        )
    }

    /// Every table this query reads, used to tag its cache entry.
    ///
    /// A cached payload has to be dropped when *any* of its source tables is
    /// written, not just the model's own table, so the model's table and every
    /// joined table and composed operands retain structured source metadata.
    /// Raw SQL operands additionally use conservative lexical table extraction.
    pub(crate) fn cache_tables(&self) -> Vec<String> {
        // The first tag is the declared table name verbatim: every write
        // invalidates with `M::table_name()`, so that tag has to match it
        // character for character. Everything after it is a name read off a join
        // or a rendered operand and is normalized to a bare identifier.
        let mut tables = vec![M::table_name().to_string()];
        for table in &self.clauses.dependencies {
            if !tables.contains(table) {
                tables.push(table.clone());
            }
        }
        push_table_tag(&mut tables, M::table_name());

        for join in &self.clauses.joins {
            push_table_tag(&mut tables, &join.table);
        }

        // An embedded query's tables are among the dependencies already.
        for union in &self.clauses.unions {
            if let crate::query::Operand::Sql { sql, .. } = &union.operand {
                collect_tables_from_sql(sql, &mut tables);
            }
        }

        for cte in &self.clauses.ctes {
            collect_tables_from_sql(&cte.query_sql, &mut tables);
        }

        // Raw SQL in the projection, HAVING and ORDER BY can read other
        // tables through subqueries of its own.
        for expression in &self.clauses.raw_select_expressions {
            collect_tables_from_sql(expression, &mut tables);
        }
        for having in &self.clauses.having_conditions {
            if let crate::query::HavingClause::Raw { sql, .. } = having {
                collect_tables_from_sql(sql, &mut tables);
            }
        }
        for (column, _) in &self.clauses.order_by {
            if let Some(expression) = crate::query::builder::raw_order_by_expression(column) {
                collect_tables_from_sql(expression, &mut tables);
            }
        }

        for condition in self.all_conditions() {
            collect_condition_tables(condition, &mut tables);
        }

        tables
    }

    fn generate_cache_key(&self) -> String {
        use std::hash::Hash;

        let mut hasher = crate::cache::KeyDigest::new();
        self.hash_database_identity(&mut hasher);
        // A caller-supplied key tells apart reads the query's shape alone would
        // let share an entry; it never stands in for that shape, or `first()`,
        // `find(id)`, each page of `paginate()` and a relation load on the
        // builder would all be served one another's rows.
        self.clauses.cache_key.hash(&mut hasher);
        self.hash_query_shape(&mut hasher);

        crate::cache::QueryCache::global().key_for(M::table_name(), &hasher.hex())
    }

    /// Hash what the query reads, with every value it binds: two queries
    /// differing only in a bound value render the same SQL, so the values
    /// are hashed with it, embedded queries' included.
    pub(in crate::query) fn hash_query_shape<H: std::hash::Hasher>(&self, hasher: &mut H) {
        use std::hash::Hash;

        // Two models can read one table (a full model and a narrower one), and
        // two schemas can hold tables of one name.
        std::any::type_name::<M>().hash(hasher);
        M::schema_name().hash(hasher);
        M::table_name().hash(hasher);
        self.self_join_depth.hash(hasher);

        for condition in &self.clauses.conditions {
            hash_where_condition(condition, hasher);
        }

        for group in &self.clauses.or_groups {
            hash_or_group(group, hasher);
        }

        self.clauses.order_by.hash(hasher);
        self.clauses.limit_value.hash(hasher);
        self.clauses.offset_value.hash(hasher);
        self.clauses.include_trashed.hash(hasher);
        self.clauses.only_trashed.hash(hasher);
        self.clauses.select_columns.hash(hasher);
        self.clauses.raw_select_expressions.hash(hasher);

        for subquery in &self.clauses.subquery_select_expressions {
            subquery.query.hash_shape(hasher);
            subquery.alias.hash(hasher);
        }

        self.clauses.joins.hash(hasher);
        self.clauses.group_by.hash(hasher);

        for having in &self.clauses.having_conditions {
            having.hash_into(hasher);
        }

        for union in &self.clauses.unions {
            union.operand.hash_into(hasher);
            union.union_type.hash(hasher);
        }

        for cte in &self.clauses.ctes {
            cte.name.hash(hasher);
            cte.query_sql.hash(hasher);
            if let Some(query) = &cte.query {
                query.hash_shape(hasher);
            }
            cte.recursive.hash(hasher);
            cte.columns.hash(hasher);
        }

        self.clauses.window_functions.hash(hasher);
    }

    /// Run the write `future` to this model's table on `db`, then drop the
    /// cached reads and dirty-tracking snapshots it made stale, recorded in
    /// the transaction it ran in, which replays or restores them as it ends.
    /// The snapshots stay when it reports, through `rows`, writing no row. A
    /// write that fails or is dropped part way counts as one that wrote: a
    /// SQLite `FAIL` trigger keeps its writes when the statement errors, and
    /// the database may have applied a cancelled one.
    pub(crate) async fn invalidating_model_state<T>(
        db: &crate::database::Database,
        future: impl std::future::Future<Output = Result<T>>,
        rows: impl FnOnce(&T) -> u64,
    ) -> Result<T> {
        struct Snapshots<'a, M: Model> {
            db: &'a crate::database::Database,
            wrote: bool,
            model: std::marker::PhantomData<M>,
        }

        impl<M: Model> Drop for Snapshots<'_, M> {
            fn drop(&mut self) {
                if self.wrote {
                    self.db
                        .recording(crate::model::invalidate_dirty_snapshots::<M>);
                }
            }
        }

        let mut snapshots = Snapshots::<M> {
            db,
            wrote: true,
            model: std::marker::PhantomData,
        };
        // Every table in this database is invalidated: cascades and triggers can write other tables,
        // even when a trigger suppresses every directly affected row.
        let result = db.invalidating(future).await;
        snapshots.wrote = result.as_ref().map_or(true, |value| rows(value) > 0);
        result
    }

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
    /// let result = User::query().where_eq(User::ID, 1).get_json().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn get_json(self) -> Result<Vec<serde_json::Value>> {
        self.ensure_query_is_executable()?;
        let (sql, params) = self.build_select_sql_with_params()?;
        self.fetch_json(&sql, params).await
    }
}

/// Record `table` as a cache tag, ignoring duplicates and unusable names.
fn push_table_tag(tables: &mut Vec<String>, table: &str) {
    let Some(table) = normalize_table_name(table) else {
        return;
    };

    if !tables.contains(&table) {
        tables.push(table);
    }
}

/// Reduce a rendered table reference to the bare name used as a cache tag.
///
/// Handles the quoting styles the SQL builders emit (`"users"`, `` `users` ``,
/// `[users]`) and drops any schema qualifier. Anything that is not a plain
/// identifier is rejected so keywords and expressions never become tags.
fn normalize_table_name(token: &str) -> Option<String> {
    use crate::internal::sql_lexer::{Kind, significant};
    let mut parts = significant(token);
    let mut name = parts.next()?;
    loop {
        if !matches!(name.kind, Kind::Word | Kind::Identifier) {
            return None;
        }
        match parts.next() {
            None => break,
            Some(dot) if dot.text == "." => name = parts.next()?,
            _ => return None,
        }
    }
    let decoded = name.unquoted()?;
    (!decoded.is_empty()).then_some(decoded)
}

/// Pull table names out of rendered SQL: the identifier after each `JOIN`, and
/// every table of a `FROM` list, `FROM a, b` naming two.
///
/// Composed builders retain structured dependencies. This fallback additionally
/// handles caller-written SQL in operands and expressions. Over-collecting is
/// harmless — a spurious tag just makes invalidation more eager — while
/// under-collecting would keep serving stale rows, so every identifier-shaped
/// token is kept.
fn collect_tables_from_sql(sql: &str, tables: &mut Vec<String>) {
    const NOT_A_TABLE: [&str; 3] = ["select", "unnest", "values"];
    // Words a table may follow: `FROM ONLY posts` reads `posts`.
    const BEFORE_A_TABLE: [&str; 2] = ["only", "lateral"];
    // The keywords that end a `FROM` list.
    const LIST_END: [&str; 13] = [
        "where",
        "group",
        "order",
        "limit",
        "offset",
        "having",
        "union",
        "intersect",
        "except",
        "window",
        "for",
        "returning",
        "fetch",
    ];

    let mut depth = 0usize;
    // The parenthesis depth of each `FROM` list being read, innermost last.
    let mut lists: Vec<usize> = Vec::new();
    let mut expect_table = false;
    for token in sql_tokens(sql) {
        match token {
            "(" => {
                depth += 1;
                // A table factor may be a parenthesized join or comma list.
                // Keep looking for its first table; SELECT/VALUES below still
                // distinguish a derived query from such a group.
                if expect_table {
                    lists.push(depth);
                }
            }
            ")" => {
                depth = depth.saturating_sub(1);
                lists.retain(|&list| list <= depth);
            }
            "," => expect_table = lists.last() == Some(&depth),
            word if word.eq_ignore_ascii_case("from") => {
                lists.push(depth);
                expect_table = true;
            }
            word if word.eq_ignore_ascii_case("join") => {
                expect_table = true;
            }
            word => {
                if lists.last() == Some(&depth)
                    && LIST_END.iter().any(|end| word.eq_ignore_ascii_case(end))
                {
                    lists.pop();
                }
                if std::mem::take(&mut expect_table) {
                    if BEFORE_A_TABLE
                        .iter()
                        .any(|keyword| word.eq_ignore_ascii_case(keyword))
                    {
                        expect_table = true;
                    } else if !NOT_A_TABLE
                        .iter()
                        .any(|keyword| word.eq_ignore_ascii_case(keyword))
                    {
                        push_table_tag(tables, word);
                    }
                }
            }
        }
    }
}

/// Split SQL into words and the `(`, `)` and `,` between them, leaving out
/// whitespace and single-quoted literals.
fn sql_tokens(sql: &str) -> impl Iterator<Item = &str> {
    use crate::internal::sql_lexer::{Kind, significant};
    let mut tokens = significant(sql)
        .filter(|token| token.kind != Kind::Literal)
        .peekable();
    std::iter::from_fn(move || {
        let first = tokens.next()?;
        let mut end = first.start + first.text.len();
        if matches!(first.kind, Kind::Word | Kind::Identifier) {
            while tokens.peek().is_some_and(|token| token.text == ".") {
                tokens.next();
                if let Some(part) = tokens.next() {
                    end = part.start + part.text.len();
                }
            }
        }
        Some(&sql[first.start..end])
    })
}

/// Collect the tables named inside a condition's SQL-carrying operands.
fn collect_condition_tables(condition: &crate::query::WhereCondition, tables: &mut Vec<String>) {
    match &condition.value {
        crate::query::ConditionValue::RawExpr(query_sql) => {
            collect_tables_from_sql(query_sql, tables);
        }
        crate::query::ConditionValue::RawTemplate { sql, .. } => {
            collect_tables_from_sql(sql, tables);
        }
        crate::query::ConditionValue::Subquery(subquery) => {
            if let Some(table) = subquery.named_table() {
                push_table_tag(tables, table);
            }
        }
        _ => {}
    }
}

/// Collect the tables named inside an OR group and everything nested under it.
#[cfg(test)]
#[path = "../../../tests/unit/query_execution_tests.rs"]
mod tests;
