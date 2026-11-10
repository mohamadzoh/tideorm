//! Many-to-many relations reached through a pivot table.

use std::marker::PhantomData;

use crate::error::Result;
use crate::internal::Value;
use crate::model::Model;
use crate::query::QueryBuilder;

#[cfg(feature = "entity-manager")]
mod entity_manager_support;
mod mutations;

use super::helpers::{ensure_relation_configured, required_key, where_key};
use super::state::{RelationState, relation_serde};

/// A many-to-many relation: rows of `Related` reached by joining `Pivot`'s
/// table.
///
/// `Pivot` must be a real model, because the mutation methods
/// ([`attach`](Self::attach), [`detach`](Self::detach), [`sync`](Self::sync)
/// and their batch forms) query and delete pivot rows through it. Reads select
/// from `Related`: `load()`, `count()` and `exists()` keep the rows whose key
/// one of the owner's pivot rows holds, and [`load_with`](Self::load_with)
/// joins the pivot, so its closure can read a pivot column.
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
/// `load()` and `count()` read each related row once, however many pivot rows
/// link it. `load_with()` joins the pivot, so there a pair stored twice repeats
/// its related row. `attach()` refuses to create a second pivot row for a pair
/// that already exists.
///
/// # Soft-delete pivots
///
/// When `Pivot` has soft delete, a trashed pivot row no longer links its pair:
/// `load()`, `load_with()`, eager loading and `count()` all leave it out,
/// `attach()` restores it, and `sync()` removes it with the live rows.
///
/// # Runtime-only state
///
/// The parent primary key and any scoped connection are runtime state serde does
/// not carry: serializing yields just the cached `Vec<Related>`, and a
/// deserialized wrapper can no longer query. A model rebuilt from JSON works
/// again only after
/// [`__rebuild_relations`](crate::internal::InternalModel::__rebuild_relations)
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
    /// Name of the model field this relation was declared on, which the
    /// entity manager names the relation by in its errors, and with
    /// `owner_table` what tells it apart from another relation when a rebuilt
    /// model takes over a previous copy's state.
    #[cfg(feature = "entity-manager")]
    pub relation_name: &'static str,
    /// Table of the model owning the relation.
    #[cfg(feature = "entity-manager")]
    pub owner_table: &'static str,
    state: RelationState<Vec<Related>>,
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
        )?;
        // The pivot rows are read, deleted and loaded eagerly through the
        // `Pivot` model, whose table another name would not be.
        if self.pivot_table != Pivot::table_name() {
            return Err(crate::error::Error::query(format!(
                "HasManyThrough pivot table '{}' is not the table of its pivot model, '{}'; \
                 the relation reads and writes its pivot rows through that model, so name its table",
                self.pivot_table,
                Pivot::table_name()
            )));
        }
        Ok(())
    }

    fn parent_key(&self, context: &str) -> Result<&Value> {
        self.ensure_configured()?;
        required_key(&self.state.key, "Parent primary key", context)
    }

    /// Declare the four columns and the join table.
    ///
    /// All five must be non-empty; every method rejects a wrapper built by
    /// [`Default`] with "HasManyThrough relation is not configured". Argument
    /// order pairs each key with the table it belongs to: the two pivot columns
    /// first, then the owner-side and related-side columns they point at.
    /// Normally the derive supplies all of this.
    #[doc(hidden)]
    #[must_use]
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
    /// what pivot rows are matched on, and whether the owner is new (its
    /// `is_new()`), whose cached rows carry over when saving it rebuilds the
    /// relation.
    ///
    /// Must be a scalar; composite keys are rejected at call time. Without it the
    /// wrapper is inert — every read and every mutation errors with "Parent
    /// primary key not set for relation", so an unsaved model cannot
    /// `attach()`.
    #[doc(hidden)]
    pub fn with_parent_pk(mut self, pk: impl Into<Value>, owner_is_new: bool) -> Self {
        self.state.set_owner_key(pk.into(), owner_is_new);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_columns = self.foreign_key == previous.foreign_key
            && self.related_key == previous.related_key
            && self.local_key == previous.local_key
            && self.related_local_key == previous.related_local_key
            && self.pivot_table == previous.pivot_table;
        #[cfg(feature = "entity-manager")]
        let same_columns = same_columns
            && self.relation_name == previous.relation_name
            && self.owner_table == previous.owner_table;

        self.state
            .preserve_from(&previous.state, same_columns, previous.state.owner_is_new);
    }

    /// Apply the pivot join and the owner filter `load_with` reads the
    /// relation through.
    ///
    /// The join fans a related row out once per matching pivot row, so a pivot
    /// table holding the same pair twice repeats the related model; the
    /// closure owns the projection, so it may read the pivot's columns.
    fn scope_to_pivot(&self, query: QueryBuilder<Related>, pk: &Value) -> QueryBuilder<Related> {
        let pivot_related_column = format!(
            "{}.{}",
            self.pivot_table,
            Pivot::column_named(self.related_key)
        );
        let related_local_column = format!("{}.{}", Related::table_name(), self.related_local_key);

        let query = query.inner_join(
            &Self::pivot_table_reference(),
            &pivot_related_column,
            &related_local_column,
        );
        let query = where_key(
            query,
            format!(
                "{}.{}",
                self.pivot_table,
                Pivot::column_named(self.foreign_key)
            ),
            pk,
        );
        // A soft-deleted pivot row no longer links the pair.
        if Pivot::soft_delete_enabled() {
            query.where_null(format!(
                "{}.{}",
                self.pivot_table,
                Pivot::deleted_at_column()
            ))
        } else {
            query
        }
    }

    /// The pivot table as a statement names it, with the `Pivot` model's
    /// schema.
    fn pivot_table_reference() -> String {
        match Pivot::schema_name() {
            Some(schema) => format!("{}.{}", schema, Pivot::table_name()),
            None => Pivot::table_name().to_string(),
        }
    }

    /// The query `load` and `count` read the related rows through: the rows
    /// whose key one of the owner's live pivot rows holds.
    ///
    /// A semi-join lists each related row once however many pivot rows link
    /// it, where a join would repeat it and collapsing that takes a `GROUP BY`
    /// on the key beside `SELECT related.*`, which MariaDB refuses under
    /// `ONLY_FULL_GROUP_BY`, or a `DISTINCT`, which PostgreSQL refuses for a
    /// `json` column. The pivot's own query leaves its trashed rows out.
    fn load_query(&self, context: &str) -> Result<QueryBuilder<Related>> {
        let pk = self.parent_key(context)?;
        let links = where_key(self.state.source.query::<Pivot>(), self.foreign_key, pk)
            .select(vec![Pivot::column_named(self.related_key)]);
        Ok(self
            .state
            .source
            .query()
            .where_in_subquery(self.related_local_key, links))
    }

    /// Fetch every related row linked through the pivot table.
    ///
    /// Each related row comes back once, however many pivot rows link it,
    /// which is what makes this agree with [`count`](Self::count). Queries
    /// whenever a connection is reachable and
    /// falls back to the cache otherwise; the result is not stored back, so each
    /// call is a fresh read. Under the `entity-manager` feature an attached
    /// manager owns the cached instances, so its cache wins. Use
    /// [`load_with`](Self::load_with) when you need ordering, paging, or the
    /// pivot's own columns.
    pub async fn load(&self) -> Result<Vec<Related>> {
        let configured = self.ensure_configured().is_ok();
        let query = || self.load_query("HasManyThrough::load");
        self.state.load_many(configured, query).await
    }

    /// Load the relation through a caller-supplied constraint on the join query.
    ///
    /// Unlike [`load`](Self::load) this joins the pivot table, so the closure
    /// can order by or read a pivot column (`select_raw()`), and a related row
    /// comes back once per pivot row that links it. Add `.group_by()` on
    /// `Related`'s primary key inside the closure for one row each.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let pk = self.parent_key("HasManyThrough::load_with")?;
        constraint_fn(self.scope_to_pivot(self.state.source.query(), pk))
            .get()
            .await
    }

    /// Count the related rows [`load`](Self::load) would return, without
    /// reading them: the same query and soft-delete scopes, so a
    /// pivot row pointing at a deleted or soft-deleted `Related` row is not
    /// counted. Always queries; the cache is not consulted.
    pub async fn count(&self) -> Result<u64> {
        self.load_query("HasManyThrough::count")?.count().await
    }

    /// Whether a related row is linked, as [`count`](Self::count) would count
    /// it, without counting them all. Always queries.
    pub async fn exists(&self) -> Result<bool> {
        self.load_query("HasManyThrough::exists")?.exists().await
    }

    crate::relations::state::many_rows_cache!(
        Related,
        "Adding or removing a row creates or deletes no pivot row: use [`attach`](Self::attach), [`detach`](Self::detach) or [`sync`](Self::sync). Under an entity manager, the owner's next save detaches the rows the relation no longer holds."
    );
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
            state: RelationState::default(),
            _pivot: PhantomData,
        }
    }
}

relation_serde!(HasManyThrough<Related, Pivot>);

#[cfg(test)]
#[path = "../../tests/unit/many_to_many_tests.rs"]
mod tests;

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../../tests/unit/many_to_many_entity_manager_tests.rs"]
mod entity_manager_tests;
