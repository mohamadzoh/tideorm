//! Writing pivot rows: attach, detach and sync, and the INSERT they share.

use crate::error::Result;
use crate::internal::Value;
use crate::model::Model;
use crate::orm::sql_builder::ValueKey;
use crate::query::QueryBuilder;
use crate::relations::helpers::{
    linkable_key, quote_ident, require_scalar_relation_key, where_key,
};

use super::HasManyThrough;

impl<Related: Model, Pivot: Model> HasManyThrough<Related, Pivot> {
    /// Associate `related_id` with the owner by inserting a pivot row.
    ///
    /// Idempotent: attaching an already-attached id is a silent no-op. The row
    /// is inserted only when it is missing, and a unique-key conflict counts as
    /// already attached, so concurrent calls for the same pair store one row:
    /// on SQLite always, since the insert runs under its write lock, and
    /// elsewhere when the pivot table has a unique key on the two columns.
    /// Without that key, PostgreSQL and MySQL can still let a concurrent pair
    /// through twice. PostgreSQL and SQLite check and insert in one statement;
    /// MySQL and MariaDB read first, because InnoDB locks what an
    /// `INSERT .. SELECT` reads and two such attaches can deadlock.
    ///
    /// Only the two key columns are written. A pivot table with extra
    /// `NOT NULL` columns and no defaults needs a direct `Pivot::create`
    /// instead. On a soft-delete pivot, a trashed row for the pair is restored
    /// rather than left to block the insert.
    pub async fn attach(&self, related_id: impl Into<Value>) -> Result<()> {
        let pk = linkable_key(
            self.parent_key("HasManyThrough::attach")?,
            "HasManyThrough::attach",
        )?;
        let related_id = related_id.into();
        let related_id = linkable_key(
            require_scalar_relation_key(&related_id, "HasManyThrough::attach")?,
            "HasManyThrough::attach related key",
        )?;
        let db = self.state.source.database()?;
        let db_type = db.execution_backend();

        let pair = || self.pair(self.state.source.query(), pk, related_id);
        if Pivot::soft_delete_enabled() && pair().restore().await? > 0 {
            return Ok(());
        }

        if matches!(
            db_type,
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB
        ) && pair().with_trashed().exists().await?
        {
            return Ok(());
        }

        insert_pivot_pair::<Pivot>(&db, self.foreign_key, self.related_key, pk, related_id).await?;
        Ok(())
    }

    /// [`attach`](Self::attach) each of `related_ids`, in one transaction:
    /// every id is checked first, and a failure attaches none of them.
    pub async fn attach_many<V: Into<Value>>(
        &self,
        related_ids: impl IntoIterator<Item = V>,
    ) -> Result<()> {
        self.parent_key("HasManyThrough::attach_many")?;
        let ids = linkable_ids(related_ids, "HasManyThrough::attach_many")?;
        let db = self.state.source.database()?;
        db.atomically(async {
            for id in ids {
                self.attach(id).await?;
            }
            Ok(())
        })
        .await
    }

    /// Remove the association with `related_id`, returning how many pivot rows
    /// were deleted (`0` if it was not attached).
    ///
    /// Deletes pivot rows only — neither the owner nor the related row is
    /// touched.
    pub async fn detach(&self, related_id: impl Into<Value>) -> Result<u64> {
        let pk = linkable_key(
            self.parent_key("HasManyThrough::detach")?,
            "HasManyThrough::detach",
        )?;

        self.pair(self.state.source.query(), pk, &related_id.into())
            .delete()
            .await
    }

    /// [`detach`](Self::detach) each of `related_ids` with one statement,
    /// returning how many pivot rows were deleted.
    pub async fn detach_many<V: Into<Value>>(
        &self,
        related_ids: impl IntoIterator<Item = V>,
    ) -> Result<u64> {
        let pk = linkable_key(
            self.parent_key("HasManyThrough::detach_many")?,
            "HasManyThrough::detach_many",
        )?;
        let ids = linkable_ids(related_ids, "HasManyThrough::detach_many")?;
        if ids.is_empty() {
            return Ok(0);
        }
        where_key(self.state.source.query::<Pivot>(), self.foreign_key, pk)
            .where_in(Pivot::column_named(self.related_key), ids)
            .delete()
            .await
    }

    /// Remove every association of the owner, returning how many pivot rows
    /// were deleted. Neither the owner nor the related rows are touched; on a
    /// soft-delete pivot the rows are trashed, as [`detach`](Self::detach)
    /// trashes them.
    pub async fn detach_all(&self) -> Result<u64> {
        let pk = linkable_key(
            self.parent_key("HasManyThrough::detach_all")?,
            "HasManyThrough::detach_all",
        )?;
        where_key(self.state.source.query::<Pivot>(), self.foreign_key, pk)
            .delete()
            .await
    }

    /// `query` narrowed to the pivot rows linking the owner key `parent` with
    /// `related`.
    fn pair(
        &self,
        query: QueryBuilder<Pivot>,
        parent: &Value,
        related: &Value,
    ) -> QueryBuilder<Pivot> {
        pivot_pair(query, self.foreign_key, self.related_key, parent, related)
    }

    /// Replace the whole association set with exactly `related_ids`.
    ///
    /// Delete-then-reinsert, not a diff: every existing pivot row for this owner
    /// is removed, soft-deleted ones included, and the wanted ids are inserted
    /// fresh, so any extra columns on a pivot row are lost. Passing no ids
    /// detaches everything.
    /// Duplicates in `related_ids` are collapsed, and every id is validated up
    /// front so a bad one cannot be discovered halfway through.
    ///
    /// The delete and the re-inserts run as one transaction — a failure part way
    /// through would otherwise leave the associations permanently deleted. It
    /// joins an ambient transaction as a SAVEPOINT rather than opening a second
    /// top-level one.
    pub async fn sync<V: Into<Value>>(
        &self,
        related_ids: impl IntoIterator<Item = V>,
    ) -> Result<()> {
        let pk = linkable_key(
            self.parent_key("HasManyThrough::sync")?,
            "HasManyThrough::sync",
        )?
        .clone();
        let wanted = linkable_ids(related_ids, "HasManyThrough::sync")?;

        let db = self.state.source.database()?;
        let foreign_key = self.foreign_key;
        let related_key = self.related_key;
        let scoped_db = db.clone();

        // `Database::transaction` defers to an ambient transaction installed by
        // the caller, so this nests as a SAVEPOINT instead of opening a second,
        // independent top-level transaction.
        db.transaction(async move |_| {
            // A trashed pivot row left behind would keep a wanted pair
            // from being inserted again.
            Pivot::query_with(&scoped_db)
                .where_eq(foreign_key, pk.clone())
                .force_delete()
                .await?;

            for id in &wanted {
                insert_pivot_pair::<Pivot>(&scoped_db, foreign_key, related_key, &pk, id).await?;
            }

            Ok(())
        })
        .await?;

        Ok(())
    }
}

/// `related_ids`, each checked as a key a pivot row can hold, without
/// repeats: integers of any width are one key, as a key column compares them.
fn linkable_ids<V: Into<Value>>(
    related_ids: impl IntoIterator<Item = V>,
    context: &str,
) -> Result<Vec<Value>> {
    let mut seen = std::collections::HashSet::new();
    let mut ids = Vec::new();
    for id in related_ids.into_iter().map(Into::into) {
        linkable_key(
            require_scalar_relation_key(&id, context)?,
            &format!("{context} related key"),
        )?;
        if seen.insert(ValueKey::new(id.clone())) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Insert the pivot row linking `parent_pk` and `related_id` unless it is
/// there, and fail if the pair is still missing afterwards.
///
/// An insert that wrote no row is read back: on PostgreSQL and SQLite the
/// conflict clause that turns a lost race into a no-op also swallows a
/// conflict on any other unique key of the pivot, and on MySQL and MariaDB a
/// unique key refuses the insert either way.
async fn insert_pivot_pair<Pivot: Model>(
    db: &crate::database::Database,
    foreign_key: &str,
    related_key: &str,
    parent_pk: &Value,
    related_id: &Value,
) -> Result<()> {
    use crate::config::DatabaseType;

    let db_type = db.execution_backend();
    let (sql, params) =
        build_pivot_insert::<Pivot>(db_type, foreign_key, related_key, parent_pk, related_id);
    // A trigger can change other tables before failing or suppressing the insert.
    let written = match db
        .invalidating(db.__execute_with_params(&sql, params))
        .await
    {
        Ok(written) => written,
        // A duplicate fails only the statement there, not the transaction.
        Err(error)
            if error.is_unique_violation()
                && matches!(db_type, DatabaseType::MySQL | DatabaseType::MariaDB) =>
        {
            0
        }
        Err(error) => return Err(error),
    };
    if written == 0
        && !pivot_pair(
            Pivot::query_with(db),
            foreign_key,
            related_key,
            parent_pk,
            related_id,
        )
        .exists()
        .await?
    {
        let table = Pivot::table_name();
        return Err(crate::error::Error::query(format!(
            "no {table} row links {parent_pk} with {related_id}: another unique key of \
             {table} refused it"
        )));
    }
    Ok(())
}

/// `query` narrowed to the pivot rows linking `parent` with `related`.
fn pivot_pair<Pivot: Model>(
    query: QueryBuilder<Pivot>,
    foreign_key: &str,
    related_key: &str,
    parent: &Value,
    related: &Value,
) -> QueryBuilder<Pivot> {
    query
        .where_eq(foreign_key, parent.clone())
        .where_eq(related_key, related.clone())
}

/// Build the parameterized pivot-row INSERT shared by `attach` and `sync`, with
/// each key bound as the type of its `Pivot` column.
///
/// On PostgreSQL and SQLite it inserts the row only when it is missing. On
/// MySQL and MariaDB it inserts unconditionally, and the caller checks first:
/// InnoDB takes shared next-key locks for the reads inside an
/// `INSERT .. SELECT`, which two concurrent attaches turn into a deadlock. A
/// pair that is there already fails the insert on its unique key, which
/// [`insert_pivot_pair`] reads as a row not written.
pub(super) fn build_pivot_insert<Pivot: Model>(
    db_type: crate::config::DatabaseType,
    foreign_key: &str,
    related_key: &str,
    parent_pk: &Value,
    related_id: &Value,
) -> (String, Vec<Value>) {
    use crate::config::DatabaseType;
    use crate::internal::push_param;

    let mut params = Vec::with_capacity(4);
    let key_value = |column: &str, value: &Value| {
        crate::internal::column_value(
            value,
            crate::internal::column_type_of::<Pivot>(column).as_ref(),
        )
    };
    let parent = key_value(foreign_key, parent_pk);
    let related = key_value(related_key, related_id);
    let new_parent = push_param(db_type, &mut params, parent.clone());
    let new_related = push_param(db_type, &mut params, related.clone());
    let table = crate::query::db_sql::quote_table::<Pivot>(db_type);
    let foreign_key = quote_ident(db_type, Pivot::column_named(foreign_key));
    let related_key = quote_ident(db_type, Pivot::column_named(related_key));
    let sql = match db_type {
        // With no conflict clause the statement says whether it wrote the
        // row: MySQL counts a duplicate an `ON DUPLICATE KEY UPDATE` left
        // alone as a row it matched.
        DatabaseType::MySQL | DatabaseType::MariaDB => format!(
            "INSERT INTO {table} ({foreign_key}, {related_key}) VALUES ({new_parent}, {new_related})"
        ),
        // `NOT EXISTS` keeps a pivot table without a unique key free of
        // duplicates, and the conflict clause covers a race on one with a key.
        DatabaseType::Postgres | DatabaseType::SQLite => {
            let existing_parent = push_param(db_type, &mut params, parent);
            let existing_related = push_param(db_type, &mut params, related);
            format!(
                "INSERT INTO {table} ({foreign_key}, {related_key}) SELECT {new_parent}, {new_related} \
                 WHERE NOT EXISTS (SELECT 1 FROM {table} WHERE {foreign_key} = {existing_parent} \
                 AND {related_key} = {existing_related}) ON CONFLICT DO NOTHING"
            )
        }
    };

    (sql, params)
}
