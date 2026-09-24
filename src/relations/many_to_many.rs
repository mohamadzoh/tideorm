//! Many-to-many relations reached through a pivot table.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

use crate::error::Result;
use crate::model::Model;
use crate::query::QueryBuilder;

#[cfg(feature = "entity-manager")]
mod entity_manager_support;

use super::helpers::{
    QuerySource, ensure_relation_configured, preserve_cached_value, quote_ident,
    require_scalar_relation_key, required_key,
};

/// A many-to-many relation: rows of `Related` reached by joining `Pivot`'s
/// table.
///
/// `Pivot` must be a real model, because the mutation methods
/// ([`attach`](Self::attach), [`detach`](Self::detach), [`sync`](Self::sync))
/// query and delete pivot rows through it. Reads go the other way — they select
/// from `Related` with the pivot joined in — so a pivot column that is not a
/// field on `Pivot` is invisible to `load()` but still available inside
/// [`load_with`](Self::load_with).
///
/// ```ignore
/// #[tideorm(has_many_through = "Role", pivot = "user_roles",
///           foreign_key = "user_id", related_key = "role_id")]
/// pub roles: HasManyThrough<Role, UserRole>,
/// ```
///
/// `pivot`, `foreign_key` and `related_key` are all required — the derive
/// rejects the field otherwise. `local_key` and `owner_key` default to `"id"`.
///
/// # Duplicate pivot rows
///
/// The join fans a related row out once per matching pivot row. `load()` collapses
/// that with a `GROUP BY` on `Related`'s primary key, and `count()` with
/// `COUNT(DISTINCT ..)`; `load_with()` deliberately does not, because its closure
/// owns the projection. `attach()` also refuses to create a second pivot row for a
/// pair that already exists.
///
/// `load()` and `count()` still disagree on one case, by construction: `count()`
/// counts distinct keys in the pivot table without joining `Related`, so a pivot
/// row pointing at a deleted or soft-deleted row is counted, while `load()` joins
/// and applies `Related`'s soft-delete scope and omits it. Use `load().len()` when
/// you need the number you could actually read back.
///
/// # Runtime-only state
///
/// The parent primary key and any scoped connection are runtime state serde does
/// not carry: serializing yields just the cached `Vec<Related>`, and a
/// deserialized wrapper can no longer query. A model rebuilt from JSON works
/// again only after
/// [`refresh_runtime_relations_from`](crate::internal::InternalModel::refresh_runtime_relations_from)
/// re-derives the wrappers. Like the direct wrappers, `load()` prefers the
/// database over the cache whenever a connection is reachable — see
/// [`get_cached`](Self::get_cached) for the non-querying view.
#[derive(Debug, Clone)]
pub struct HasManyThrough<Related: Model, Pivot: Model> {
    /// Pivot column pointing back at the owning model.
    pub foreign_key: &'static str,
    /// Pivot column pointing at `Related`. Not interchangeable with
    /// [`foreign_key`](Self::foreign_key) — swapping the two silently reads the
    /// relation backwards.
    pub related_key: &'static str,
    /// Column on the owning model whose value
    /// [`foreign_key`](Self::foreign_key) holds; normally its primary key.
    pub local_key: &'static str,
    /// Column on `Related` that [`related_key`](Self::related_key) points at;
    /// normally `Related`'s primary key. Set from the field's `owner_key`
    /// attribute, which defaults to `"id"`.
    pub related_local_key: &'static str,
    /// Name of the join table. Used directly in the join and in the pivot
    /// INSERT, so it must be a real table name, not an alias.
    pub pivot_table: &'static str,
    /// Name of the model field this relation was declared on, used as the
    /// entity manager's relation-snapshot key.
    #[cfg(feature = "entity-manager")]
    pub relation_name: &'static str,
    /// Table of the model owning the relation.
    #[cfg(feature = "entity-manager")]
    pub owner_table: &'static str,
    cached: Option<Vec<Related>>,
    parent_pk: Option<serde_json::Value>,
    #[cfg(feature = "entity-manager")]
    owner_key: Option<String>,
    source: QuerySource,
    _pivot: PhantomData<Pivot>,
}

impl<Related: Model, Pivot: Model> HasManyThrough<Related, Pivot> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured(
            "HasManyThrough",
            &[
                self.foreign_key,
                self.related_key,
                self.local_key,
                self.related_local_key,
                self.pivot_table,
            ],
        )
    }

    fn parent_key(&self, context: &str) -> Result<&serde_json::Value> {
        self.ensure_configured()?;
        required_key(&self.parent_pk, "Parent primary key", context)
    }

    /// Declare the four columns and the join table.
    ///
    /// All five must be non-empty; every method rejects a wrapper built by
    /// [`Default`] with "HasManyThrough relation is not configured". Argument
    /// order pairs each key with the table it belongs to: the two pivot columns
    /// first, then the owner-side and related-side columns they point at.
    /// Normally the derive supplies all of this.
    pub fn new(
        foreign_key: &'static str,
        related_key: &'static str,
        local_key: &'static str,
        related_local_key: &'static str,
        pivot_table: &'static str,
    ) -> Self {
        Self {
            foreign_key,
            related_key,
            local_key,
            related_local_key,
            pivot_table,
            ..Self::default()
        }
    }

    /// Supply the owning model's [`local_key`](Self::local_key) value, which is
    /// what pivot rows are matched on.
    ///
    /// Must be a scalar; composite keys are rejected at call time. Without it the
    /// wrapper is inert — every read and every mutation errors with "Parent
    /// primary key not set for relation", so an unsaved model cannot
    /// `attach()`.
    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, models: Vec<Related>) {
        self.cached = Some(models);
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.foreign_key == previous.foreign_key
            && self.related_key == previous.related_key
            && self.local_key == previous.local_key
            && self.related_local_key == previous.related_local_key
            && self.pivot_table == previous.pivot_table
            && self.parent_pk == previous.parent_pk;
        #[cfg(feature = "entity-manager")]
        let same_relation = same_relation
            && self.relation_name == previous.relation_name
            && self.owner_table == previous.owner_table;

        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.parent_pk.is_none(),
            same_relation,
        );

        #[cfg(feature = "entity-manager")]
        if same_relation {
            self.source.preserve_from(&previous.source);
            if self.owner_key.is_none() {
                self.owner_key = previous.owner_key.clone();
            }
        }
    }

    /// Apply the pivot join and the owner filter that both load paths read the
    /// relation through.
    ///
    /// The join fans a related row out once per matching pivot row, so a pivot
    /// table holding the same pair twice repeats the related model.
    /// `deduplicate` collapses that in the database — see
    /// [`deduplicate_by_identity`] for why it groups rather than `DISTINCT`s —
    /// which keeps the duplicates off the wire and makes `count()` over the same
    /// shape agree with what `load` returns. `load_with` passes `false`: its
    /// closure owns the projection and the ordering, both of which
    /// deduplication constrains.
    fn scope_to_pivot(
        &self,
        query: QueryBuilder<Related>,
        pk: &serde_json::Value,
        deduplicate: bool,
    ) -> QueryBuilder<Related> {
        let pivot_related_column = format!("{}.{}", self.pivot_table, self.related_key);
        let related_local_column = format!("{}.{}", Related::table_name(), self.related_local_key);

        let query = query
            .bind_columns_of::<Pivot>()
            .inner_join(
                &self.pivot_table_reference(),
                &pivot_related_column,
                &related_local_column,
            )
            .where_eq(
                format!("{}.{}", self.pivot_table, self.foreign_key),
                pk.clone(),
            );

        if deduplicate {
            deduplicate_by_identity(query)
        } else {
            query
        }
    }

    /// The pivot table as a statement names it: with the `Pivot` model's schema
    /// when the relation's pivot is that model's table.
    fn pivot_table_reference(&self) -> String {
        match Pivot::schema_name() {
            Some(schema) if self.pivot_table == Pivot::table_name() => {
                format!("{}.{}", schema, self.pivot_table)
            }
            _ => self.pivot_table.to_string(),
        }
    }

    /// The query `load` reads the related rows through, one row per related
    /// model.
    fn load_query(&self, context: &str) -> Result<QueryBuilder<Related>> {
        let pk = self.parent_key(context)?;
        Ok(self.scope_to_pivot(self.source.query(), pk, true))
    }

    /// Fetch every related row joined through the pivot table.
    ///
    /// Groups on `Related`'s primary key, so a pivot table holding the same pair
    /// twice still yields one row — which is what makes this agree with
    /// [`count`](Self::count). Queries whenever a connection is reachable and
    /// falls back to the cache otherwise; the result is not stored back, so each
    /// call is a fresh read. Under the `entity-manager` feature an attached
    /// manager owns the cached instances, so its cache wins. Use
    /// [`load_with`](Self::load_with) when you need ordering, paging, or the
    /// pivot's own columns.
    pub async fn load(&self) -> Result<Vec<Related>> {
        let can_query = self.source.prefers_database()
            && self.parent_pk.is_some()
            && self.ensure_configured().is_ok();
        if let Some(cached) = &self.cached
            && !can_query
        {
            return Ok(cached.clone());
        }

        self.load_query("HasManyThrough::load")?.get().await
    }

    /// Load the relation through a caller-supplied constraint on the join query.
    ///
    /// Unlike [`load`](Self::load) this does **not** deduplicate. The closure
    /// owns the projection and the ordering, and both are things deduplication
    /// constrains: it cannot order by a pivot column outside the grouping, and a
    /// caller reading pivot columns through `select_raw()` wants exactly the one
    /// row per pivot row that deduplication would remove. Callers who want the
    /// deduplicated shape can add `.group_by()` on `Related`'s primary key
    /// inside the closure.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let pk = self.parent_key("HasManyThrough::load_with")?;
        constraint_fn(self.scope_to_pivot(self.source.query(), pk, false))
            .get()
            .await
    }

    /// Count associated rows.
    ///
    /// Counts distinct [`related_key`](Self::related_key) values on the *pivot*
    /// table rather than joining to `Related`, so it is cheaper than
    /// `load().len()` — but it therefore counts associations, and a pivot row
    /// pointing at a deleted `Related` row still counts. Always queries; the
    /// cache is not consulted.
    pub async fn count(&self) -> Result<u64> {
        let pk = self.parent_key("HasManyThrough::count")?;

        // Counted over distinct related keys so that a duplicated pivot row does
        // not report more associations than `load` returns.
        self.source
            .query::<Pivot>()
            .where_eq(
                format!("{}.{}", self.pivot_table, self.foreign_key),
                pk.clone(),
            )
            .count_distinct(self.related_key)
            .await
    }

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
    /// instead.
    pub async fn attach(&self, related_id: impl serde::Serialize) -> Result<()> {
        let pk = self.parent_key("HasManyThrough::attach")?;
        let related_id = crate::query::filter_value(related_id);
        let related_id = require_scalar_relation_key(&related_id, "HasManyThrough::attach")?;
        let db = self.source.database()?;
        let db_type = db.backend();

        if matches!(
            db_type,
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB
        ) && self
            .source
            .query::<Pivot>()
            .with_trashed()
            .where_eq(self.foreign_key, pk.clone())
            .where_eq(self.related_key, related_id.clone())
            .exists()
            .await?
        {
            return Ok(());
        }

        let (sql, params) = build_pivot_insert::<Pivot>(
            db_type,
            self.pivot_table,
            self.foreign_key,
            self.related_key,
            pk,
            related_id,
        );

        db.__execute_with_params(&sql, params).await?;
        // The pivot row is written as raw SQL, which carries no model context, so
        // the cache has to be told which table changed. `detach` needs no
        // equivalent: it goes through `Pivot::query().delete()`, which invalidates
        // as part of the typed mutation path.
        crate::QueryCache::global().invalidate_model(self.pivot_table);
        Ok(())
    }

    /// Remove the association with `related_id`, returning how many pivot rows
    /// were deleted (`0` if it was not attached).
    ///
    /// Deletes pivot rows only — neither the owner nor the related row is
    /// touched.
    pub async fn detach(&self, related_id: impl serde::Serialize) -> Result<u64> {
        let pk = self.parent_key("HasManyThrough::detach")?;

        self.source
            .query::<Pivot>()
            .where_eq(self.foreign_key, pk.clone())
            .where_eq(self.related_key, related_id)
            .delete()
            .await
    }

    /// Replace the whole association set with exactly `related_ids`.
    ///
    /// Delete-then-reinsert, not a diff: every existing pivot row for this owner
    /// is removed and the wanted ids are inserted fresh, so any extra columns on
    /// a pivot row are lost. Passing an empty vector detaches everything.
    /// Duplicates in `related_ids` are collapsed, and every id is validated up
    /// front so a bad one cannot be discovered halfway through.
    ///
    /// The delete and the re-inserts run as one transaction — a failure part way
    /// through would otherwise leave the associations permanently deleted. It
    /// joins an ambient transaction as a SAVEPOINT rather than opening a second
    /// top-level one.
    pub async fn sync<V: serde::Serialize>(&self, related_ids: Vec<V>) -> Result<()> {
        let pk = self.parent_key("HasManyThrough::sync")?.clone();

        let mut seen = std::collections::HashSet::new();
        let mut wanted = Vec::with_capacity(related_ids.len());
        for id in related_ids.into_iter().map(crate::query::filter_value) {
            require_scalar_relation_key(&id, "HasManyThrough::sync")?;
            if seen.insert(id.to_string()) {
                wanted.push(id);
            }
        }

        let db = self.source.database()?;
        let pivot_table = self.pivot_table;
        let foreign_key = self.foreign_key;
        let related_key = self.related_key;
        let scoped_db = db.clone();

        // `Database::transaction` defers to an ambient transaction installed by
        // the caller, so this nests as a SAVEPOINT instead of opening a second,
        // independent top-level transaction.
        db.transaction(move |_| {
            Box::pin(async move {
                Pivot::query_with(&scoped_db)
                    .where_eq(foreign_key, pk.clone())
                    .delete()
                    .await?;

                let db_type = scoped_db.backend();
                for id in &wanted {
                    let (sql, params) = build_pivot_insert::<Pivot>(
                        db_type,
                        pivot_table,
                        foreign_key,
                        related_key,
                        &pk,
                        id,
                    );
                    scoped_db.__execute_with_params(&sql, params).await?;
                }

                Ok(())
            })
        })
        .await?;

        // Invalidate after the transaction commits, not inside it: a rolled-back
        // sync leaves the pivot table untouched, so evicting mid-transaction would
        // throw away a still-valid cache. The re-inserts are raw SQL and carry no
        // model context of their own.
        crate::QueryCache::global().invalidate_model(self.pivot_table);
        Ok(())
    }

    /// The eagerly-loaded rows, if this relation was populated. Never queries
    /// and never awaits.
    ///
    /// `Some(&[])` means "loaded, and there are none"; `None` means nothing was
    /// ever loaded. Filled by eager loading or by deserializing a payload that
    /// contained the key — not by [`load`](Self::load), which does not write its
    /// result back.
    pub fn get_cached(&self) -> Option<&[Related]> {
        self.cached.as_deref()
    }

    /// Mutable access to the cached rows.
    ///
    /// Edits are local to the cache: pushing or removing an element does *not*
    /// create or delete a pivot row. Use [`attach`](Self::attach),
    /// [`detach`](Self::detach) or [`sync`](Self::sync) to change the
    /// association itself.
    pub fn as_mut(&mut self) -> Option<&mut Vec<Related>> {
        self.cached.as_mut()
    }

    /// Whether the cache has been populated. `true` for a loaded-but-empty
    /// relation.
    pub fn is_loaded(&self) -> bool {
        self.cached.is_some()
    }
}

/// Collapse the duplicate rows a pivot join fans out, without comparing whole
/// rows.
///
/// `SELECT DISTINCT` is the obvious spelling and the wrong one: it compares
/// every projected column, and PostgreSQL has no equality operator for the
/// `json` type, so `SELECT DISTINCT "related".*` over a table carrying one
/// aborts with `could not identify an equality operator for type json` — and
/// tideorm's own schema builder emits exactly that column type. Grouping on the
/// primary key deduplicates by identity instead, never comparing the other
/// columns; they are functionally dependent on it, which is the one shape
/// PostgreSQL and MySQL's `ONLY_FULL_GROUP_BY` both accept alongside
/// `SELECT "related".*`. `build_self_ref_tree_sql` in the sibling `helpers`
/// module groups for the same reason.
///
/// A model that declares no primary key has no identity cheaper than the whole
/// row, so it falls back to `SELECT DISTINCT` and keeps the JSON hazard.
fn deduplicate_by_identity<E: Model>(query: QueryBuilder<E>) -> QueryBuilder<E> {
    let primary_key_columns = E::primary_key_names();
    if primary_key_columns.is_empty() {
        return query.distinct();
    }

    let table = E::table_name();
    primary_key_columns.iter().fold(query, |query, column| {
        query.group_by(format!("{}.{}", table, column))
    })
}

/// Build the parameterized pivot-row INSERT shared by `attach` and `sync`, with
/// each key bound as the type of its `Pivot` column.
///
/// On PostgreSQL and SQLite it inserts the row only when it is missing. On
/// MySQL and MariaDB it inserts unconditionally, and the caller checks first:
/// InnoDB takes shared next-key locks for the reads inside an
/// `INSERT .. SELECT`, which two concurrent attaches turn into a deadlock.
fn build_pivot_insert<Pivot: Model>(
    db_type: crate::config::DatabaseType,
    pivot_table: &str,
    foreign_key: &str,
    related_key: &str,
    parent_pk: &serde_json::Value,
    related_id: &serde_json::Value,
) -> (String, Vec<crate::internal::Value>) {
    use crate::config::DatabaseType;
    use crate::internal::push_param;

    let mut params = Vec::with_capacity(4);
    let key_value = |column: &str, value: &serde_json::Value| {
        crate::internal::json_to_column_value(
            value,
            crate::internal::column_type_of::<Pivot>(column).as_ref(),
        )
    };
    let parent = key_value(foreign_key, parent_pk);
    let related = key_value(related_key, related_id);
    let new_parent = push_param(db_type, &mut params, parent.clone());
    let new_related = push_param(db_type, &mut params, related.clone());
    let table = if pivot_table == Pivot::table_name() {
        crate::query::db_sql::quote_table::<Pivot>(db_type)
    } else {
        crate::internal::sql_safety::format_identifier_reference(db_type, pivot_table)
            .unwrap_or_else(|| quote_ident(db_type, pivot_table))
    };
    let foreign_key = quote_ident(db_type, foreign_key);
    let related_key = quote_ident(db_type, related_key);
    let sql = match db_type {
        // The conflict clause makes losing a race on a unique key a no-op
        // instead of an error.
        DatabaseType::MySQL | DatabaseType::MariaDB => format!(
            "INSERT INTO {table} ({foreign_key}, {related_key}) VALUES ({new_parent}, {new_related}) \
             ON DUPLICATE KEY UPDATE {foreign_key} = {foreign_key}"
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

impl<Related: Model, Pivot: Model> Default for HasManyThrough<Related, Pivot> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            related_key: "",
            local_key: "",
            related_local_key: "",
            pivot_table: "",
            #[cfg(feature = "entity-manager")]
            relation_name: "",
            #[cfg(feature = "entity-manager")]
            owner_table: "",
            cached: None,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            owner_key: None,
            source: QuerySource::default(),
            _pivot: PhantomData,
        }
    }
}

impl<Related: Model, Pivot: Model> Serialize for HasManyThrough<Related, Pivot> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model, Pivot: Model> Deserialize<'de> for HasManyThrough<Related, Pivot> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<Vec<Related>>::deserialize(deserializer)?;
        Ok(Self {
            cached,
            ..Self::default()
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/many_to_many_tests.rs"]
mod tests;

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../../tests/unit/many_to_many_entity_manager_tests.rs"]
mod entity_manager_tests;
