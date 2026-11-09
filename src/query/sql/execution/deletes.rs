//! Terminals that delete rows, softly or for good, and restore them.

use super::*;

use crate::orm::sql_builder::{BoundPiece, BoundSql};

/// The `deleted_at` a soft delete writes: `Utc::now()`, bound as a typed
/// write of the field binds it, and not the database clock, which MySQL and
/// MariaDB read in the session time zone.
fn deletion_stamp() -> Value {
    Value::ChronoDateTimeUtc(Some(chrono::Utc::now()))
}

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    /// Render `DELETE FROM <table> WHERE ..` for this query's filters.
    pub(super) fn build_delete_sql(
        &self,
        operation: &str,
        trash_scope_counts: bool,
    ) -> Result<(String, Vec<Value>)> {
        let db_type = self.db_type_for_sql()?;
        let (where_sql, params) = self
            .mutation_where_bound(db_type, operation, trash_scope_counts)?
            .build(db_type.dialect());
        let sql = format!(
            "DELETE FROM {} WHERE {}",
            db_sql::quote_table::<M>(db_type),
            where_sql
        );

        Ok((sql, params))
    }

    /// Delete the rows this query matches. On a soft-delete model they are
    /// marked deleted, as [`soft_delete()`](Self::soft_delete) marks them;
    /// [`force_delete()`](Self::force_delete) removes them for good. At least
    /// one explicit filter is required.
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
    /// let result = User::query().where_eq(User::ID, 1).delete().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn delete(self) -> Result<u64> {
        if M::soft_delete_enabled() {
            self.ensure_not_only_trashed("delete")?;
            return self.soft_delete().await;
        }
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("delete")?;
        self.ensure_mutation_has_explicit_filters("delete")?;

        let (sql, params) = self.build_delete_sql("delete", false)?;
        self.run_mutation(&sql, params).await
    }

    /// Delete every row in the table represented by this query.
    ///
    /// This is an explicit opt-in escape hatch for full-table deletion and is kept
    /// separate from `delete()` so accidental unfiltered bulk deletes remain blocked.
    ///
    /// On a soft-delete model the live rows are marked deleted, as `delete()`
    /// marks them; `only_trashed().force_delete()` empties the trash.
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
    /// let result = User::query().delete_all().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn delete_all(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("delete_all")?;
        self.ensure_mutation_has_no_explicit_filters("delete_all")?;

        let db_type = self.db_type_for_sql()?;
        if M::soft_delete_enabled() {
            self.ensure_not_only_trashed("delete_all")?;
            // The live rows: marking a trashed row again would change nothing
            // but its stamp.
            let query = self.live_rows_only();
            let (sql, params) =
                Self::deleted_at_update_sql(db_type, deletion_stamp(), query.where_bound(db_type)?);
            return query.run_mutation(&sql, params).await;
        }
        let (scope_sql, params) = self.build_where_clause_with_condition_for_db(db_type)?;
        let mut sql = format!("DELETE FROM {}", db_sql::quote_table::<M>(db_type));
        if !scope_sql.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&scope_sql);
        }
        self.run_mutation(&sql, params).await
    }

    /// Refuse to soft-delete the trash: its rows are deleted already, and a
    /// caller asking for it means to remove them.
    pub(super) fn ensure_not_only_trashed(&self, operation: &str) -> Result<()> {
        if self.clauses.only_trashed {
            return Err(Error::query(format!(
                "{}() on only_trashed() rows of {} would mark rows that are already deleted; force_delete() removes them for good",
                operation,
                M::table_name()
            )));
        }
        Ok(())
    }

    /// Render the soft-delete UPDATE and the values it binds, the stamp
    /// first.
    pub(super) fn build_soft_delete_sql(
        &self,
        db_type: DatabaseType,
    ) -> Result<(String, Vec<Value>)> {
        let filter = self.mutation_where_bound(db_type, "soft_delete", false)?;
        Ok(Self::deleted_at_update_sql(
            db_type,
            deletion_stamp(),
            filter,
        ))
    }

    /// `UPDATE <table> SET <deleted_at> = <value> WHERE <filter>`, the
    /// statement that trashes or restores rows, and the values it binds.
    pub(super) fn deleted_at_update_sql(
        db_type: DatabaseType,
        value: impl BoundPiece,
        filter: BoundSql,
    ) -> (String, Vec<Value>) {
        let mut sql = BoundSql::sql(format!(
            "UPDATE {} SET {} = ",
            db_sql::quote_table::<M>(db_type),
            db_sql::quote_ident(db_type, M::deleted_at_column()),
        ));
        sql.push(value).push(" WHERE ").push(filter);
        sql.build(db_type.dialect())
    }

    /// Mark the live rows this query matches deleted, stamping `deleted_at`;
    /// a trashed row keeps the stamp it has, even under `with_trashed()`. At
    /// least one explicit filter is required.
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
    /// let result = User::query().where_eq(User::ID, 1).soft_delete().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn soft_delete(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("soft_delete")?;

        if !M::soft_delete_enabled() {
            return Err(Error::query(
                "soft_delete() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_mutation_has_explicit_filters("soft_delete")?;
        self.ensure_not_only_trashed("soft_delete")?;

        // Marking a trashed row again would change nothing but its stamp,
        // which a retention purge reads.
        let query = self.live_rows_only();
        let (sql, params) = query.build_soft_delete_sql(query.db_type_for_sql()?)?;
        query.run_mutation(&sql, params).await
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
    /// # User::query().where_eq(User::ID, 1).soft_delete().await?;
    /// let result = User::query().only_trashed().where_eq(User::ID, 1).restore().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn restore(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("restore")?;

        if !M::soft_delete_enabled() {
            return Err(Error::query(
                "restore() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_trash_mutation_has_filters("restore")?;
        // Judged before the scope is forced below: only a caller's own
        // `only_trashed()` lets the filters match every trashed row.
        let db_type = self.db_type_for_sql()?;
        self.ensure_mutation_can_exclude_a_row(db_type, "restore", true)?;

        // `restore()` only ever targets trashed rows, so the scope is forced here
        // instead of relying on the caller remembering `with_trashed()`: under the
        // default active-only scope the query would carry `deleted_at IS NULL` and
        // never match anything. Routing the guard through the scope also keeps it
        // inside the rendered condition tree, rather than concatenating
        // `AND deleted_at IS NOT NULL` onto an unparenthesized body where a
        // top-level OR would bind it to the last branch only.
        let mut query = self;
        query.clauses.only_trashed = true;
        query.clauses.include_trashed = false;

        let filter = query.where_bound(db_type)?;
        let (sql, params) = Self::deleted_at_update_sql(db_type, "NULL", filter);

        query.run_mutation(&sql, params).await
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
    /// let result = User::query().where_eq(User::ID, 1).force_delete().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn force_delete(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("force_delete")?;
        self.ensure_trash_mutation_has_filters("force_delete")?;

        // `force_delete()` exists precisely to reach rows the default active-only
        // scope hides, so trashed rows are put back in scope — unless the caller
        // asked for trashed rows only, which must not widen to live ones.
        let mut query = self;
        if !query.clauses.only_trashed {
            query.clauses.include_trashed = true;
        }

        let (sql, params) = query.build_delete_sql("force_delete", true)?;
        query.run_mutation(&sql, params).await
    }
}
