//! Writing pivot rows: attach, detach and sync, and the INSERT they share.

use std::collections::HashSet;

use crate::config::DatabaseType;
use crate::error::Result;
use crate::internal::Value;
use crate::model::Model;
use crate::orm::sql_builder::ValueKey;
use crate::query::QueryBuilder;
use crate::relations::eager::keys::KeyColumn;
use crate::relations::helpers::{
    linkable_key, quote_ident, require_scalar_relation_key, where_key,
};

use super::HasManyThrough;

/// The most pivot rows one statement writes: within the 500 terms SQLite
/// takes in a compound `SELECT`, and every backend's bind-parameter limit.
const PIVOT_ROWS_PER_STATEMENT: usize = 200;

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

        if matches!(db_type, DatabaseType::MySQL | DatabaseType::MariaDB)
            && pair().with_trashed().exists().await?
        {
            return Ok(());
        }

        let related_id = std::slice::from_ref(related_id);
        insert_pivot_pairs::<Pivot>(&db, self.foreign_key, self.related_key, pk, related_id).await
    }

    /// [`attach`](Self::attach) each of `related_ids`, in one transaction:
    /// every id is checked first, and a failure attaches none of them.
    ///
    /// Integer and UUID keys, which every backend compares as themselves,
    /// are attached a few hundred at a time, each step one statement for all
    /// of them; other keys one at a time, as `attach` does.
    pub async fn attach_many<V: Into<Value>>(
        &self,
        related_ids: impl IntoIterator<Item = V>,
    ) -> Result<()> {
        self.parent_key("HasManyThrough::attach_many")?;
        let ids = linkable_ids(related_ids, "HasManyThrough::attach_many")?;
        let db = self.state.source.database()?;
        db.atomically(async {
            match exact_keys::<Pivot>(self.related_key, &ids) {
                Some(keys) if keys.len() > 1 => {
                    for chunk in keys.chunks(PIVOT_ROWS_PER_STATEMENT) {
                        self.attach_together(&db, chunk).await?;
                    }
                }
                _ => {
                    for id in ids {
                        self.attach(id).await?;
                    }
                }
            }
            Ok(())
        })
        .await
    }

    /// [`attach`](Self::attach) `keys`, values of the pivot's related key
    /// column that compare exactly ([`exact_keys`]), with one statement for
    /// all of them at each step.
    async fn attach_together(&self, db: &crate::database::Database, keys: &[Value]) -> Result<()> {
        let pk = linkable_key(
            self.parent_key("HasManyThrough::attach")?,
            "HasManyThrough::attach",
        )?;
        let pairs = || {
            where_key(self.state.source.query::<Pivot>(), self.foreign_key, pk)
                .where_in(Pivot::column_named(self.related_key), keys.to_vec())
        };
        if Pivot::soft_delete_enabled() {
            pairs().restore().await?;
        }
        let mut missing = keys.to_vec();
        if matches!(
            db.execution_backend(),
            DatabaseType::MySQL | DatabaseType::MariaDB
        ) {
            let attached = pivot_keys(pairs().with_trashed(), self.related_key).await?;
            missing.retain(|key| !attached.contains(&ValueKey::new(key.clone())));
        }
        insert_pivot_pairs::<Pivot>(db, self.foreign_key, self.related_key, pk, &missing).await
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
    /// top-level one. Integer and UUID keys are inserted a few hundred to a
    /// statement, other keys one at a time.
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

            match exact_keys::<Pivot>(related_key, &wanted) {
                Some(keys) => {
                    insert_pivot_pairs::<Pivot>(&scoped_db, foreign_key, related_key, &pk, &keys)
                        .await?;
                }
                None => {
                    for id in wanted.chunks(1) {
                        insert_pivot_pairs::<Pivot>(&scoped_db, foreign_key, related_key, &pk, id)
                            .await?;
                    }
                }
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

/// `ids` as values of the pivot's `related_key` column, each once, when every
/// one compares there as itself: an integer or a UUID, which no collation
/// folds into another key. Such keys are written with one statement for
/// several, and the rows read back are matched to them by value.
fn exact_keys<Pivot: Model>(related_key: &str, ids: &[Value]) -> Option<Vec<Value>> {
    let kind = crate::internal::column_type_of::<Pivot>(related_key);
    let mut seen = HashSet::new();
    let mut keys = Vec::with_capacity(ids.len());
    for id in ids {
        let key = crate::internal::column_value(id, kind.as_ref());
        if !(crate::internal::is_integer_value(&key) || matches!(key, Value::Uuid(Some(_)))) {
            return None;
        }
        if seen.insert(ValueKey::new(key.clone())) {
            keys.push(key);
        }
    }
    Some(keys)
}

/// The `related_key` values of the pivot rows `query` reads, as keys of that
/// column.
async fn pivot_keys<Pivot: Model>(
    query: QueryBuilder<Pivot>,
    related_key: &str,
) -> Result<HashSet<ValueKey>> {
    let column = KeyColumn::of::<Pivot>(related_key);
    let keys: Vec<serde_json::Value> = query.pluck(related_key).await?;
    Ok(keys.iter().map(|key| column.key(key)).collect())
}

/// Insert the pivot rows linking `parent_pk` with each of `related_ids`
/// unless they are there, and fail if a pair is still missing afterwards.
/// Several ids must be [`exact_keys`]: they are inserted together.
///
/// An insert that wrote fewer rows than it was given is read back: on
/// PostgreSQL and SQLite the conflict clause that turns a lost race into a
/// no-op also swallows a conflict on any other unique key of the pivot, and on
/// MySQL and MariaDB a unique key refuses the insert either way.
async fn insert_pivot_pairs<Pivot: Model>(
    db: &crate::database::Database,
    foreign_key: &str,
    related_key: &str,
    parent_pk: &Value,
    related_ids: &[Value],
) -> Result<()> {
    for chunk in related_ids.chunks(PIVOT_ROWS_PER_STATEMENT) {
        let rows = insert_pivot_rows::<Pivot>(db, foreign_key, related_key, parent_pk, chunk);
        let written = match rows.await? {
            Some(written) => written,
            None if chunk.len() == 1 => 0,
            // MySQL refused the whole statement for one pair: a row at a time.
            None => {
                let mut written = 0;
                for id in chunk.chunks(1) {
                    let row =
                        insert_pivot_rows::<Pivot>(db, foreign_key, related_key, parent_pk, id);
                    written += row.await?.unwrap_or(0);
                }
                written
            }
        };
        if written < chunk.len() as u64 {
            ensure_pivot_pairs::<Pivot>(db, foreign_key, related_key, parent_pk, chunk).await?;
        }
    }
    Ok(())
}

/// Run the pivot INSERT for `related_ids` and return how many rows it wrote,
/// or `None` when MySQL or MariaDB refused it for a duplicate, which fails
/// only the statement there, not the transaction.
async fn insert_pivot_rows<Pivot: Model>(
    db: &crate::database::Database,
    foreign_key: &str,
    related_key: &str,
    parent_pk: &Value,
    related_ids: &[Value],
) -> Result<Option<u64>> {
    let db_type = db.execution_backend();
    let (sql, params) =
        build_pivot_insert::<Pivot>(db_type, foreign_key, related_key, parent_pk, related_ids);
    // A trigger can change other tables before failing or suppressing the insert.
    match db
        .invalidating(db.__execute_with_params(&sql, params))
        .await
    {
        Ok(written) => Ok(Some(written)),
        Err(error)
            if error.is_unique_violation()
                && matches!(db_type, DatabaseType::MySQL | DatabaseType::MariaDB) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Fail unless a pivot row links `parent_pk` with each of `related_ids`:
/// one is looked up as the database compares it, several, which are
/// [`exact_keys`], read back together.
async fn ensure_pivot_pairs<Pivot: Model>(
    db: &crate::database::Database,
    foreign_key: &str,
    related_key: &str,
    parent_pk: &Value,
    related_ids: &[Value],
) -> Result<()> {
    let missing = match related_ids {
        [related_id] => {
            let query = pivot_pair(
                Pivot::query_with(db),
                foreign_key,
                related_key,
                parent_pk,
                related_id,
            );
            (!query.exists().await?).then_some(related_id)
        }
        _ => {
            let query = Pivot::query_with(db)
                .with_rule_scope(crate::rules::RuleScope::Relation)
                .where_eq(foreign_key, parent_pk.clone())
                .where_in(Pivot::column_named(related_key), related_ids.to_vec());
            let linked = pivot_keys(query, related_key).await?;
            related_ids
                .iter()
                .find(|id| !linked.contains(&ValueKey::new((*id).clone())))
        }
    };
    if let Some(related_id) = missing {
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

/// Build the parameterized pivot-row INSERT shared by `attach` and `sync`, a
/// row linking `parent_pk` with each of `related_ids`, with each key bound as
/// the type of its `Pivot` column.
///
/// On PostgreSQL and SQLite it inserts each row only when it is missing. On
/// MySQL and MariaDB it inserts unconditionally, and the caller checks first:
/// InnoDB takes shared next-key locks for the reads inside an
/// `INSERT .. SELECT`, which two concurrent attaches turn into a deadlock. A
/// pair that is there already fails the insert on its unique key, which
/// [`insert_pivot_rows`] reports as a statement refused.
pub(super) fn build_pivot_insert<Pivot: Model>(
    db_type: DatabaseType,
    foreign_key: &str,
    related_key: &str,
    parent_pk: &Value,
    related_ids: &[Value],
) -> (String, Vec<Value>) {
    use crate::internal::push_param;
    use std::fmt::Write;

    let mut params = Vec::with_capacity(4 * related_ids.len());
    let key_value = |column: &str, value: &Value| {
        crate::internal::column_value(
            value,
            crate::internal::column_type_of::<Pivot>(column).as_ref(),
        )
    };
    let parent = key_value(foreign_key, parent_pk);
    let related: Vec<Value> = related_ids
        .iter()
        .map(|id| key_value(related_key, id))
        .collect();
    let table = crate::query::db_sql::quote_table::<Pivot>(db_type);
    let foreign_key = quote_ident(db_type, Pivot::column_named(foreign_key));
    let related_key = quote_ident(db_type, Pivot::column_named(related_key));
    let mut sql = format!("INSERT INTO {table} ({foreign_key}, {related_key}) ");
    for (index, related) in related.into_iter().enumerate() {
        let new_parent = push_param(db_type, &mut params, parent.clone());
        let new_related = push_param(db_type, &mut params, related.clone());
        // Writing to a `String` cannot fail.
        let _ = match db_type {
            // With no conflict clause the statement says whether it wrote the
            // rows: MySQL counts a duplicate an `ON DUPLICATE KEY UPDATE`
            // left alone as a row it matched.
            DatabaseType::MySQL | DatabaseType::MariaDB => write!(
                sql,
                "{}({new_parent}, {new_related})",
                if index == 0 { "VALUES " } else { ", " }
            ),
            // `NOT EXISTS` keeps a pivot table without a unique key free of
            // duplicates, and the conflict clause covers a race on one with a
            // key.
            DatabaseType::Postgres | DatabaseType::SQLite => {
                let existing_parent = push_param(db_type, &mut params, parent.clone());
                let existing_related = push_param(db_type, &mut params, related);
                write!(
                    sql,
                    "{}SELECT {new_parent}, {new_related} WHERE NOT EXISTS (SELECT 1 FROM {table} \
                     WHERE {foreign_key} = {existing_parent} AND {related_key} = {existing_related})",
                    if index == 0 { "" } else { " UNION ALL " }
                )
            }
        };
    }
    if matches!(db_type, DatabaseType::Postgres | DatabaseType::SQLite) {
        sql.push_str(" ON CONFLICT DO NOTHING");
    }

    (sql, params)
}
