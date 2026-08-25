//! Many-to-many relations reached through a pivot table.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;
#[cfg(feature = "entity-manager")]
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

#[cfg(feature = "entity-manager")]
mod entity_manager_support;

use super::helpers::{cached_ref, ensure_relation_configured, preserve_cached_value, quote_ident};
use super::require_scalar_relation_key;

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
    /// Table of `Related`.
    #[cfg(feature = "entity-manager")]
    pub related_table: &'static str,
    cached: Option<Vec<Related>>,
    parent_pk: Option<serde_json::Value>,
    #[cfg(feature = "entity-manager")]
    owner_key: Option<String>,
    #[cfg(feature = "entity-manager")]
    entity_manager: Option<Arc<crate::entity_manager::EntityManager>>,
    #[cfg(feature = "entity-manager")]
    query_db: Option<crate::database::Database>,
    _marker: PhantomData<(Related, Pivot)>,
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
            #[cfg(feature = "entity-manager")]
            relation_name: "",
            #[cfg(feature = "entity-manager")]
            owner_table: "",
            #[cfg(feature = "entity-manager")]
            related_table: "",
            cached: None,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            owner_key: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            #[cfg(feature = "entity-manager")]
            query_db: None,
            _marker: PhantomData,
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
            && self.parent_pk == previous.parent_pk
            && {
                #[cfg(feature = "entity-manager")]
                {
                    self.relation_name == previous.relation_name
                        && self.owner_table == previous.owner_table
                        && self.related_table == previous.related_table
                }
                #[cfg(not(feature = "entity-manager"))]
                {
                    true
                }
            };

        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.parent_pk.is_none(),
            same_relation,
        );

        #[cfg(feature = "entity-manager")]
        if same_relation {
            if self.owner_key.is_none() {
                self.owner_key = previous.owner_key.clone();
            }
            if self.entity_manager.is_none() {
                self.entity_manager = previous.entity_manager.clone();
            }
            if self.query_db.is_none() {
                self.query_db = previous.query_db.clone();
            }
        }
    }

    /// Whether `load` should hit the database instead of serving `cached`.
    ///
    /// `cached` is also populated by the generated `Deserialize` impl, so a
    /// request body can plant relation contents. Whenever a connection is
    /// reachable the database wins, mirroring `HasMany::load`. The entity
    /// manager is the exception: it owns the cached instances (identity map),
    /// so its cache stays authoritative.
    fn should_requery(&self) -> bool {
        #[cfg(feature = "entity-manager")]
        {
            if self.entity_manager.is_some() {
                return false;
            }

            if self.query_db.is_some() {
                return true;
            }
        }

        crate::database::__current_db().is_ok()
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
            .inner_join(
                self.pivot_table,
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

    async fn query_related(&self, pk: &serde_json::Value) -> Result<Vec<Related>> {
        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Related::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Related::query()
            }
        };

        self.scope_to_pivot(query, pk, true).get().await
    }

    /// Fetch every related row joined through the pivot table.
    ///
    /// Groups on `Related`'s primary key, so a pivot table holding the same pair
    /// twice still yields one row — which is what makes this agree with
    /// [`count`](Self::count). Queries whenever a connection is reachable and
    /// falls back to the cache otherwise; the result is not stored back, so each
    /// call is a fresh read. Use [`load_with`](Self::load_with) when you need
    /// ordering, paging, or the pivot's own columns.
    pub async fn load(&self) -> Result<Vec<Related>> {
        if self.should_requery()
            && self.ensure_configured().is_ok()
            && let Some(pk) = self.parent_pk.as_ref()
        {
            let pk = require_scalar_relation_key(pk, "HasManyThrough::load")?;
            return self.query_related(pk).await;
        }

        if let Some(cached) = &self.cached {
            return Ok(cached.clone());
        }

        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::load")?;

        self.query_related(pk).await
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
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::load_with")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Related::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Related::query()
            }
        };

        constraint_fn(self.scope_to_pivot(query, pk, false))
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
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::count")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Pivot::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Pivot::query()
            }
        };

        // Counted over distinct related keys so that a duplicated pivot row does
        // not report more associations than `load` returns.
        query
            .where_eq(
                format!("{}.{}", self.pivot_table, self.foreign_key),
                pk.clone(),
            )
            .count_distinct(self.related_key)
            .await
    }

    /// Associate `related_id` with the owner by inserting a pivot row.
    ///
    /// Idempotent: an existence check runs first and an already-attached id is a
    /// silent no-op, so the pair cannot be duplicated. That costs one extra
    /// round trip, and it is a check-then-insert rather than an upsert —
    /// `ON CONFLICT` is spelled differently per backend and the backend is only
    /// known at runtime — so two concurrent `attach` calls can still race past it
    /// unless the pivot table has a unique constraint.
    ///
    /// Only the two key columns are written. A pivot table with extra
    /// `NOT NULL` columns and no defaults needs a direct `Pivot::create`
    /// instead.
    pub async fn attach(&self, related_id: impl Into<serde_json::Value>) -> Result<()> {
        self.ensure_configured()?;

        let db = {
            #[cfg(feature = "entity-manager")]
            {
                self.scoped_database()?
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                crate::database::require_db()?
            }
        };
        let db_type = db.backend();
        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::attach")?;
        let related_id = related_id.into();
        let related_id = require_scalar_relation_key(&related_id, "HasManyThrough::attach")?;

        // Attaching an already attached id is a no-op. A bare INSERT would add a
        // second pivot row, which duplicates the rows returned by `load` and
        // inflates `count`. `ON CONFLICT` is spelled differently per backend and
        // the backend is only known at runtime, so guard with an existence check
        // instead of a dialect-specific clause.
        let pivot_query = {
            #[cfg(feature = "entity-manager")]
            {
                Pivot::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Pivot::query()
            }
        };

        let already_attached = pivot_query
            .where_eq(self.foreign_key, pk.clone())
            .where_eq(self.related_key, related_id.clone())
            .exists()
            .await?;

        if already_attached {
            return Ok(());
        }

        let (sql, params) = build_pivot_insert(
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
    pub async fn detach(&self, related_id: impl Into<serde_json::Value>) -> Result<u64> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::detach")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Pivot::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Pivot::query()
            }
        };

        query
            .where_eq(self.foreign_key, pk.clone())
            .where_eq(self.related_key, related_id.into())
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
    pub async fn sync(&self, related_ids: Vec<serde_json::Value>) -> Result<()> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::sync")?.clone();

        // Reject every id up front: a bad id must not be discovered halfway
        // through the re-attach loop.
        let mut seen = std::collections::HashSet::new();
        let mut wanted = Vec::with_capacity(related_ids.len());
        for id in related_ids {
            require_scalar_relation_key(&id, "HasManyThrough::sync")?;
            if seen.insert(id.to_string()) {
                wanted.push(id);
            }
        }

        let db = {
            #[cfg(feature = "entity-manager")]
            {
                self.scoped_database()?
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                crate::database::__current_db()?
            }
        };

        let pivot_table = self.pivot_table;
        let foreign_key = self.foreign_key;
        let related_key = self.related_key;
        let scoped_db = db.clone();

        // The delete and the re-attaches are one unit of work: without a
        // transaction a failure part way through leaves the associations
        // permanently deleted. `Database::transaction` defers to an ambient
        // transaction installed by the caller, so this nests as a SAVEPOINT
        // instead of opening a second, independent top-level transaction.
        db.transaction(move |_| {
            Box::pin(async move {
                Pivot::query_with(&scoped_db)
                    .where_eq(foreign_key, pk.clone())
                    .delete()
                    .await?;

                let db_type = scoped_db.backend();
                for id in &wanted {
                    let (sql, params) =
                        build_pivot_insert(db_type, pivot_table, foreign_key, related_key, &pk, id);
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
        cached_ref(&self.cached)
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

    /// The cached rows as a `&Vec`. Identical to
    /// [`get_cached`](Self::get_cached) apart from the return type; prefer
    /// `get_cached` unless a caller specifically needs `Vec`'s own methods.
    pub fn items(&self) -> Option<&Vec<Related>> {
        self.cached.as_ref()
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

/// Build the parameterized pivot-row INSERT shared by `attach` and `sync`.
fn build_pivot_insert(
    db_type: crate::config::DatabaseType,
    pivot_table: &str,
    foreign_key: &str,
    related_key: &str,
    parent_pk: &serde_json::Value,
    related_id: &serde_json::Value,
) -> (String, Vec<crate::internal::Value>) {
    let mut params = Vec::with_capacity(2);
    let parent_placeholder = crate::internal::push_param(
        db_type,
        &mut params,
        crate::internal::json_to_db_value(parent_pk),
    );
    let related_placeholder = crate::internal::push_param(
        db_type,
        &mut params,
        crate::internal::json_to_db_value(related_id),
    );
    let sql = format!(
        "INSERT INTO {} ({}, {}) VALUES ({}, {})",
        quote_ident(db_type, pivot_table),
        quote_ident(db_type, foreign_key),
        quote_ident(db_type, related_key),
        parent_placeholder,
        related_placeholder
    );

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
            #[cfg(feature = "entity-manager")]
            related_table: "",
            cached: None,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            owner_key: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            #[cfg(feature = "entity-manager")]
            query_db: None,
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize, Pivot: Model> Serialize for HasManyThrough<Related, Pivot> {
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

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../../tests/unit/many_to_many_entity_manager_tests.rs"]
mod entity_manager_tests;

/// A related model paired with the pivot row that linked it in.
///
/// [`HasManyThrough::load`] discards the pivot columns; this is the container to
/// carry them when the join table holds data of its own (a role's `granted_at`,
/// a cart line's `quantity`). Nothing constructs it for you — build it in a
/// [`load_with`](HasManyThrough::load_with) closure that selects the pivot
/// columns, and remember `load_with` does not deduplicate, which is what you
/// want here since each pivot row is a distinct pairing.
///
/// Derefs to `M`, and serializes with the model flattened and the pivot under a
/// `"pivot"` key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithPivot<M, P> {
    /// The related model. Also reachable through `Deref`.
    #[serde(flatten)]
    pub model: M,
    /// The pivot row's own data.
    pub pivot: P,
}

impl<M, P> WithPivot<M, P> {
    /// Pair a related model with its pivot data.
    pub fn new(model: M, pivot: P) -> Self {
        Self { model, pivot }
    }

    /// Take the model and drop the pivot data.
    pub fn into_model(self) -> M {
        self.model
    }

    /// Borrow the pivot data. Needed because `Deref` resolves to `M`, so
    /// `.pivot` on a method-call chain would otherwise be ambiguous to read.
    pub fn pivot(&self) -> &P {
        &self.pivot
    }

    /// Split into the model and the pivot data.
    pub fn into_parts(self) -> (M, P) {
        (self.model, self.pivot)
    }
}

impl<M, P> std::ops::Deref for WithPivot<M, P> {
    type Target = M;

    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::HasManyThrough;
    use crate::config::DatabaseType;
    use crate::model::Model;
    use serde_json::json;

    #[tideorm::model(table = "m2m_render_tags")]
    struct M2mRenderTag {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }

    #[tideorm::model(table = "m2m_render_post_tags")]
    struct M2mRenderPostTag {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        post_id: i64,
        tag_id: i64,
    }

    /// A related model carrying a `json` column — the type PostgreSQL has no
    /// equality operator for, and therefore cannot `SELECT DISTINCT` over.
    #[tideorm::model(table = "m2m_render_documents")]
    struct M2mRenderDocument {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        metadata: serde_json::Value,
    }

    /// A related model whose identity spans two columns, so the grouping has to
    /// name both.
    #[tideorm::model(table = "m2m_render_regions")]
    struct M2mRenderRegion {
        #[tideorm(primary_key)]
        country_id: i64,
        #[tideorm(primary_key)]
        region_id: i64,
        name: String,
    }

    fn relation() -> HasManyThrough<M2mRenderTag, M2mRenderPostTag> {
        HasManyThrough::new("post_id", "tag_id", "id", "id", "m2m_render_post_tags")
    }

    /// The projection and the join `load` reads the relation through.
    const EXPECTED_LOAD_PREFIX: &str = "SELECT \"m2m_render_tags\".* FROM \"m2m_render_tags\" INNER JOIN \"m2m_render_post_tags\" ON \"m2m_render_post_tags\".\"tag_id\" = \"m2m_render_tags\".\"id\"";

    #[test]
    fn test_load_collapses_duplicate_pivot_rows_in_sql() {
        let sql = relation()
            .scope_to_pivot(M2mRenderTag::query(), &json!(1), true)
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert!(sql.starts_with(EXPECTED_LOAD_PREFIX), "{sql}");
        assert!(
            sql.contains("\"m2m_render_post_tags\".\"post_id\""),
            "{sql}"
        );
        assert!(
            sql.contains("GROUP BY \"m2m_render_tags\".\"id\""),
            "duplicate pivot rows are collapsed by grouping on the related primary key: {sql}"
        );
    }

    /// PostgreSQL has no equality operator for `json`, so `SELECT DISTINCT` over
    /// a projection containing one aborts the statement with "could not identify
    /// an equality operator for type json". Deduplication must therefore never
    /// compare whole rows.
    #[test]
    fn test_load_does_not_distinct_over_a_json_column() {
        let relation: HasManyThrough<M2mRenderDocument, M2mRenderPostTag> =
            HasManyThrough::new("post_id", "tag_id", "id", "id", "m2m_render_post_tags");

        let sql = relation
            .scope_to_pivot(M2mRenderDocument::query(), &json!(1), true)
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert!(
            !sql.contains("DISTINCT"),
            "SELECT DISTINCT over \"m2m_render_documents\".* cannot compare its json column: {sql}"
        );
        assert!(
            sql.starts_with("SELECT \"m2m_render_documents\".* FROM"),
            "{sql}"
        );
        assert!(
            sql.contains("GROUP BY \"m2m_render_documents\".\"id\""),
            "{sql}"
        );
    }

    #[test]
    fn test_load_groups_by_every_primary_key_column() {
        let relation: HasManyThrough<M2mRenderRegion, M2mRenderPostTag> = HasManyThrough::new(
            "post_id",
            "tag_id",
            "id",
            "country_id",
            "m2m_render_post_tags",
        );

        let sql = relation
            .scope_to_pivot(M2mRenderRegion::query(), &json!(1), true)
            .build_select_sql_for_db(DatabaseType::Postgres);

        let expected_grouping = concat!(
            "GROUP BY \"m2m_render_regions\".\"country_id\", ",
            "\"m2m_render_regions\".\"region_id\""
        );

        assert!(
            sql.contains(expected_grouping),
            "a composite identity needs every column to stay a functional dependency: {sql}"
        );
    }

    #[test]
    fn test_constrained_load_is_left_undeduplicated() {
        let sql = relation()
            .scope_to_pivot(M2mRenderTag::query(), &json!(1), false)
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert!(
            sql.starts_with("SELECT \"m2m_render_tags\".* FROM"),
            "load_with() leaves the projection to its closure: {sql}"
        );
        assert!(
            !sql.contains("GROUP BY"),
            "load_with() leaves deduplication to its closure: {sql}"
        );
    }
}
