use super::*;
use crate::columns::{IntoColumnName, IntoColumnNames, map_columns};

impl<M: Model> QueryBuilder<M> {
    /// Select specific columns.
    ///
    /// This is the *typed* half of the projection and it replaces whatever a
    /// previous `select()` chose — the last call wins. The raw halves,
    /// [`select_raw()`](Self::select_raw) and
    /// [`select_subquery()`](Self::select_subquery), accumulate instead, and all
    /// three compose: a query that calls `select()` and `select_raw()` renders
    /// the typed columns first, then the raw expressions, then the subqueries.
    /// Only when nothing at all was selected does the projection fall back to
    /// the model's own columns.
    ///
    /// Read a subset of the columns with [`get_json()`](Self::get_json):
    /// [`get()`](Self::get) refuses a `select()` that leaves model columns out,
    /// because the models it built would carry defaults in their place.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().select(["id", "name"]).validate().unwrap();
    /// User::query().select((User::ID, User::ACTIVE)).validate().unwrap();
    /// ```
    ///
    /// Columns are names, field names or typed columns, in an array, a `Vec`
    /// or a slice, or a tuple of typed columns of different types (see
    /// [`IntoColumnNames`]). Another model's typed column is written with its
    /// table, for a joined query.
    pub fn select(mut self, columns: impl IntoColumnNames) -> Self {
        self.clauses.select_columns =
            Some(map_columns(&columns, |column| Self::own_column(&column)));
        self
    }

    /// Add one column to the typed projection.
    ///
    /// Unlike [`select()`](Self::select) it keeps the columns already
    /// selected; on a query that selected none, the projection becomes this
    /// one column rather than the model's own columns plus it.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().select([User::ID]).add_select(User::NAME).validate().unwrap();
    /// ```
    pub fn add_select(mut self, column: impl IntoColumnName) -> Self {
        let column = Self::own_column(&column);
        self.clauses
            .select_columns
            .get_or_insert_with(Vec::new)
            .push(column);
        self
    }

    /// Deduplicate the result rows with `SELECT DISTINCT`.
    ///
    /// The usual reason to reach for this is a join that fans rows out: a
    /// many-to-many pivot holding more than one row for the same pair repeats
    /// the related model once per pivot row. Collapsing the duplicates in the
    /// database keeps them off the wire, instead of transferring them and
    /// discarding them afterwards.
    ///
    /// It composes with every projection source — the typed columns of
    /// [`select()`](Self::select), the raw expressions of `select_raw()`, the
    /// scalar subqueries of `select_subquery()` and the `table.*` fallback all
    /// render behind the one `DISTINCT` keyword — and with the terminals:
    /// `count()` counts the deduplicated rows, and `exists()` is unaffected
    /// because deduplication cannot change whether a row exists. Calling it more
    /// than once is idempotent.
    ///
    /// Projections containing known encrypted model columns are rejected:
    /// randomized ciphertext does not preserve plaintext equality. Select
    /// unencrypted columns, or deduplicate the values after decryption in Rust.
    /// Trusted raw expressions remain the caller's responsibility.
    ///
    /// PostgreSQL requires every `ORDER BY` expression of a `SELECT DISTINCT` to
    /// appear in the select list. A query that breaks that rule is rejected by
    /// validation with a query error naming the column, rather than
    /// being sent to the server; ordering by an expression supplied through
    /// `order_by_raw()` cannot be checked and stays the caller's responsibility.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().distinct().validate().unwrap();
    /// ```
    pub fn distinct(mut self) -> Self {
        if !self.is_distinct() {
            self.clauses
                .raw_select_expressions
                .push(crate::query::builder::DISTINCT_SELECT_MARKER.to_string());
        }
        self
    }

    /// Lock the rows this query reads until the transaction ends
    /// (`SELECT ... FOR UPDATE`).
    ///
    /// Use it for a read-check-write inside
    /// [`Database::transaction`](crate::database::Database::transaction): a
    /// second transaction that locks the same rows waits until the first one
    /// commits, then reads what it wrote. Without the lock both read the same
    /// value and the last `update()` wins, silently undoing the other:
    ///
    /// ```rust,ignore
    /// let shipped = Database::current()?
    ///     .atomically(async {
    ///         let mut item = Item::query()
    ///             .where_eq("id", id)
    ///             .lock_for_update()
    ///             .first_or_fail()
    ///             .await?;
    ///         if item.stock < quantity {
    ///             return Ok(false);
    ///         }
    ///         item.stock -= quantity;
    ///         item.update().await?;
    ///         Ok(true)
    ///     })
    ///     .await?;
    /// ```
    ///
    /// Outside a transaction the lock ends with the statement. `count()`,
    /// `exists()` and the aggregates lock the rows they read too, and a locked
    /// query never reads from the query cache.
    ///
    /// PostgreSQL rejects the lock on a query with `DISTINCT`, `GROUP BY` or a
    /// `UNION`. SQLite has no row locks and renders nothing: the first write of
    /// a transaction locks the whole database, so the second of two competing
    /// read-check-write transactions fails with a retryable
    /// [`LockNotAvailable`](crate::error::DbFailureKind::LockNotAvailable) error
    /// instead of racing.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().lock_for_update().validate().unwrap();
    /// ```
    pub fn lock_for_update(mut self) -> Self {
        self.clauses.lock_for_update = true;
        self
    }

    /// Select columns from this table and also from a linked/joined table
    ///
    /// Use this for partial model queries that need columns from a related
    /// table without loading the full related model.
    ///
    /// Like [`select()`](Self::select) this replaces the typed projection; the
    /// LEFT JOIN it registers is validated exactly as
    /// [`left_join()`](Self::left_join) validates its own.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().select_with_linked(["id"], "posts", "id", "user_id", ["title"]).validate().unwrap();
    /// ```
    ///
    /// The join matches this table's `local_column` with the linked table's
    /// `linked_column`: `users.id = posts.user_id` above. Columns are names
    /// or typed columns; only their names are used, written with their table.
    pub fn select_with_linked(
        self,
        local_columns: impl IntoColumnNames,
        linked_table: &str,
        local_column: impl IntoColumnName,
        linked_column: impl IntoColumnName,
        linked_columns: impl IntoColumnNames,
    ) -> Self {
        let table_name = M::table_name();
        let mut all_columns = map_columns(&local_columns, |c| {
            format!("{}.{}", table_name, c.column_name())
        });
        all_columns.extend(map_columns(&linked_columns, |c| {
            format!("{}.{}", linked_table, c.column_name())
        }));

        let mut query = self.push_join(
            JoinType::Left,
            linked_table,
            None,
            format!("{}.{}", table_name, local_column.column_name()),
            format!("{}.{}", linked_table, linked_column.column_name()),
        );
        query.clauses.select_columns = Some(all_columns);
        query
    }

    /// Select all columns from this table plus specific columns from a linked table
    ///
    /// Carries the same projection and JOIN-validation rules as
    /// [`select_with_linked()`](Self::select_with_linked).
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().select_also_linked("posts", "id", "user_id", ["title"]).validate().unwrap();
    /// ```
    pub fn select_also_linked(
        self,
        linked_table: &str,
        local_column: impl IntoColumnName,
        linked_column: impl IntoColumnName,
        linked_columns: impl IntoColumnNames,
    ) -> Self {
        self.select_with_linked(
            M::column_names(),
            linked_table,
            local_column,
            linked_column,
            linked_columns,
        )
    }

    /// Join model `R`'s table, matching `left` with `right`.
    ///
    /// The typed form of [`inner_join`](Self::inner_join) and its siblings:
    /// the table comes from `R`, and typed columns are written with their own
    /// tables.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// User::query().join::<Post>(JoinType::Left, User::ID, Post::USER_ID).validate().unwrap();
    /// ```
    pub fn join<R: Model>(
        self,
        join_type: JoinType,
        left: impl IntoColumnName,
        right: impl IntoColumnName,
    ) -> Self {
        self.join_columns(join_type, R::table_name(), None, left, right)
    }

    /// Join model `R`'s table under `alias`, matching `left` with `right`.
    ///
    /// A typed column of `R` is written with the alias, so a model can join
    /// its own table.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// User::query().join_as::<Post>(JoinType::Inner, "p", User::ID, Post::USER_ID).validate().unwrap();
    /// ```
    pub fn join_as<R: Model>(
        self,
        join_type: JoinType,
        alias: &str,
        left: impl IntoColumnName,
        right: impl IntoColumnName,
    ) -> Self {
        self.join_columns(join_type, R::table_name(), Some(alias), left, right)
    }

    /// Add an INNER JOIN clause
    ///
    /// Returns only rows with matches in both tables. Name both columns as
    /// `table.column` (or `alias.column`), such as
    /// `inner_join("posts", "users.id", "posts.user_id")`: a bare column name
    /// invalidates the query, and the error surfaces when it runs.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().inner_join("posts", "users.id", "posts.user_id").validate().unwrap();
    /// ```
    pub fn inner_join(
        self,
        table: &str,
        left_column: impl IntoColumnName,
        right_column: impl IntoColumnName,
    ) -> Self {
        self.join_columns(JoinType::Inner, table, None, left_column, right_column)
    }

    /// Add an INNER JOIN clause with an alias
    ///
    /// The columns are `alias.column` or `table.column`, as for
    /// [`inner_join`](Self::inner_join).
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().inner_join_as("posts", "p", "users.id", "p.user_id").validate().unwrap();
    /// ```
    pub fn inner_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: impl IntoColumnName,
        right_column: impl IntoColumnName,
    ) -> Self {
        self.join_columns(
            JoinType::Inner,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Add a LEFT JOIN clause
    ///
    /// Returns all rows from the left table, and matched rows from the right.
    /// The columns are `table.column`, as for [`inner_join`](Self::inner_join).
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().left_join("posts", "users.id", "posts.user_id").validate().unwrap();
    /// ```
    pub fn left_join(
        self,
        table: &str,
        left_column: impl IntoColumnName,
        right_column: impl IntoColumnName,
    ) -> Self {
        self.join_columns(JoinType::Left, table, None, left_column, right_column)
    }

    /// Add a LEFT JOIN clause with an alias
    ///
    /// The alias is what lets a table join itself. The columns are
    /// `alias.column` or `table.column`, as for [`inner_join`](Self::inner_join).
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().left_join_as("posts", "p", "users.id", "p.user_id").validate().unwrap();
    /// ```
    pub fn left_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: impl IntoColumnName,
        right_column: impl IntoColumnName,
    ) -> Self {
        self.join_columns(
            JoinType::Left,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Add a RIGHT JOIN clause
    ///
    /// Returns all rows from the right table, and matched rows from the left.
    /// A right-table row with no match has `NULL` in every model column, which
    /// `get()` cannot turn into a model, so read the query with
    /// [`get_json()`](Self::get_json) and `select()` the columns you need. The
    /// columns are `table.column`, as for [`inner_join`](Self::inner_join).
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().right_join("posts", "users.id", "posts.user_id").validate().unwrap();
    /// ```
    pub fn right_join(
        self,
        table: &str,
        left_column: impl IntoColumnName,
        right_column: impl IntoColumnName,
    ) -> Self {
        self.join_columns(JoinType::Right, table, None, left_column, right_column)
    }

    /// Add a RIGHT JOIN clause with an alias
    ///
    /// The columns are `alias.column` or `table.column`, as for
    /// [`inner_join`](Self::inner_join).
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().right_join_as("posts", "p", "users.id", "p.user_id").validate().unwrap();
    /// ```
    pub fn right_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: impl IntoColumnName,
        right_column: impl IntoColumnName,
    ) -> Self {
        self.join_columns(
            JoinType::Right,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Register a join of columns named or typed: a typed column is written
    /// with its table, or with `alias` when that is the joined table.
    fn join_columns(
        self,
        join_type: JoinType,
        table: &str,
        alias: Option<&str>,
        left_column: impl IntoColumnName,
        right_column: impl IntoColumnName,
    ) -> Self {
        let reference = |column: &dyn IntoColumnName| match (alias, column.column_table()) {
            (Some(alias), Some(own)) if own == table && column.column_schema().is_none() => {
                format!("{alias}.{}", column.column_name())
            }
            _ => crate::columns::column_reference(&column, None),
        };
        let (left, right) = (reference(&left_column), reference(&right_column));
        self.push_join(join_type, table, alias, left, right)
    }

    /// Register a join, invalidating the query instead when any part of it is
    /// not a plain identifier.
    fn push_join(
        mut self,
        join_type: JoinType,
        table: &str,
        alias: Option<&str>,
        left_column: String,
        right_column: String,
    ) -> Self {
        if let Err(reason) = Self::validate_join_clause(table, alias, &left_column, &right_column) {
            self.invalidate_query(reason);
            return self;
        }

        self.clauses.joins.push(JoinClause {
            join_type,
            table: table.to_string(),
            alias: alias.map(|s| s.to_string()),
            left_column,
            right_column,
        });
        self
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/query_select_and_joins_tests.rs"]
mod tests;
