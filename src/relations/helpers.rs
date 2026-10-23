#[cfg(feature = "entity-manager")]
use std::sync::Arc;

use crate::database::Database;
use crate::error::{Error, Result};
use crate::internal::Value;
use crate::model::Model;
use crate::query::QueryBuilder;

#[cfg(feature = "entity-manager")]
use crate::entity_manager::{EntityManager, TideEntityManagerMeta, model_entity_manager_key};

#[cfg(feature = "entity-manager")]
use super::state::RelationState;

pub(crate) use crate::internal::sql_safety::quote_ident;

pub(crate) fn ensure_relation_configured(
    relation_name: &str,
    required_values: &[&str],
) -> Result<()> {
    if required_values.iter().any(|value| value.is_empty()) {
        return Err(Error::query(format!(
            "{relation_name} relation is not configured; use {relation_name}::new(...) or a macro-generated relation field",
        )));
    }

    Ok(())
}

pub(crate) fn require_scalar_relation_key<'a>(
    value: &'a serde_json::Value,
    context: &str,
) -> Result<&'a serde_json::Value> {
    if value.is_array() || value.is_object() {
        return Err(Error::query(format!(
            "{} only supports scalar relation keys; composite primary keys require an explicit single-column relation key or a custom query",
            context
        )));
    }

    Ok(value)
}

/// `query` narrowed to the rows whose `column` holds `key`.
///
/// A NULL key relates to no row, as SQL's `=` has it, where `where_eq` reads
/// a null as `IS NULL`: an owner whose nullable `local_key` is unset would
/// otherwise load every row that points nowhere.
pub(crate) fn where_key<M: Model>(
    query: QueryBuilder<M>,
    column: impl Into<String>,
    key: &serde_json::Value,
) -> QueryBuilder<M> {
    let column = column.into();
    if key.is_null() {
        query.where_in(column, Vec::<serde_json::Value>::new())
    } else {
        query.where_eq(column, key.clone())
    }
}

/// `key`, which a write links rows by; a NULL one links none.
pub(crate) fn linkable_key<'a>(
    key: &'a serde_json::Value,
    context: &str,
) -> Result<&'a serde_json::Value> {
    if key.is_null() {
        return Err(Error::query(format!(
            "{context}: the relation key is NULL, so no row can be linked to it"
        )));
    }
    Ok(key)
}

/// A relation's lookup key, which must have been supplied and be a scalar.
///
/// `what` names the key in the error for a wrapper that never received one — a
/// bare `Default`, or a model deserialized without being refreshed.
pub(crate) fn required_key<'a>(
    key: &'a Option<serde_json::Value>,
    what: &str,
    context: &str,
) -> Result<&'a serde_json::Value> {
    let key = key
        .as_ref()
        .ok_or_else(|| Error::query(format!("{what} not set for relation")))?;
    require_scalar_relation_key(key, context)
}

/// Whether `key`, the owner key a relation wrapper was built with, names no
/// stored row yet: absent, or the placeholder a new model holds (`null`, `0`,
/// `""`, the nil UUID). The rows cached in such a wrapper are the caller's,
/// so they are kept when saving the owner rebuilds the wrapper under its
/// stored key: an entity manager then saves them as its children.
pub(crate) fn owner_is_unsaved(key: &Option<serde_json::Value>) -> bool {
    match key {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::Number(number)) => {
            number.as_i64() == Some(0) || number.as_u64() == Some(0)
        }
        Some(serde_json::Value::String(text)) => {
            text.is_empty() || uuid::Uuid::parse_str(text).is_ok_and(|uuid| uuid.is_nil())
        }
        Some(_) => false,
    }
}

/// Where a relation's statements run.
///
/// By default that is the scope's connection: the enclosing transaction, else
/// the global database. Loaded relations retain their owner's database;
/// under `entity-manager` they can also belong to a manager's identity map.
#[derive(Debug, Clone)]
pub(crate) struct QuerySource {
    /// The manager that loaded the relation. It owns the cached instances, so
    /// `load` serves them rather than re-querying.
    #[cfg(feature = "entity-manager")]
    pub(crate) entity_manager: Option<Arc<EntityManager>>,
    /// The database the owning model was loaded from.
    pub(crate) database: Option<Database>,
}

impl Default for QuerySource {
    fn default() -> Self {
        Self {
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            database: crate::database::loading_database(),
        }
    }
}

impl QuerySource {
    /// Whether `load` should read the database rather than serve a cached
    /// value: a connection is reachable and no entity manager owns the cache.
    ///
    /// The cache is also filled by deserializing, so a request body can plant
    /// relation contents; preferring the database keeps such a payload from
    /// passing itself off as stored rows.
    pub(crate) fn prefers_database(&self) -> bool {
        #[cfg(feature = "entity-manager")]
        {
            if self.entity_manager.is_some() {
                return false;
            }
        }
        if self.database.is_some() {
            return true;
        }
        crate::database::__current_db().is_ok()
    }

    /// A query over `M` that runs where this relation's statements do.
    pub(crate) fn query<M: Model>(&self) -> QueryBuilder<M> {
        match self.attached() {
            Some(database) => M::query_with(&database),
            None => M::query(),
        }
    }

    /// The database this relation's raw statements run on.
    pub(crate) fn database(&self) -> Result<Database> {
        match self.attached() {
            Some(database) => Ok(database),
            None => crate::database::__current_db(),
        }
    }

    /// Database handles themselves join an enclosing transaction only when it
    /// belongs to the same pool. Never discard an attached database here.
    fn attached(&self) -> Option<Database> {
        #[cfg(feature = "entity-manager")]
        if let Some(entity_manager) = &self.entity_manager {
            return Some(entity_manager.database().clone());
        }
        self.database.clone()
    }

    /// Keep whatever `previous` was tied to wherever this source is not.
    pub(crate) fn preserve_from(&mut self, previous: &Self) {
        #[cfg(feature = "entity-manager")]
        if self.entity_manager.is_none() {
            self.entity_manager = previous.entity_manager.clone();
        }
        if self.database.is_none() {
            self.database = previous.database.clone();
        }
    }
}

/// Load a to-one relation into `entity_manager` and cache it in `state`:
/// the row the manager already tracks under `lookup`'s column and key, else
/// the one its query reads. A relation that has loaded keeps what it holds,
/// which gives way to the instance the manager tracks. The caller ties
/// `state` to the manager before building the query, so `load` then serves
/// that cache, which the manager owns, and the query runs on its database.
/// A NULL key has no related row, as [`where_key`] reads it. An `owned`
/// relation records the key it holds as the baseline a save diffs it against.
#[cfg(feature = "entity-manager")]
pub(crate) async fn load_one_in_entity_manager<'s, E>(
    state: &'s mut RelationState<Box<E>>,
    entity_manager: &Arc<EntityManager>,
    lookup: Result<(&str, serde_json::Value, QueryBuilder<E>)>,
    owned: bool,
) -> Result<Option<&'s E>>
where
    E: Model + TideEntityManagerMeta,
{
    if !state.is_loaded() {
        let (column, key, query) = lookup?;
        let found = if key.is_null() {
            None
        } else {
            match entity_manager.find_by_field::<E>(column, &key)? {
                Some(tracked) => Some(tracked),
                None => query.first().await?,
            }
        };
        state.set_cached(found.map(Box::new));
    }

    let keys =
        register_loaded(entity_manager, state.cached_mut().map(|model| &mut **model)).await?;
    if owned {
        state.loaded_keys = Some(keys);
    }
    Ok(state.cached().map(|model| &**model))
}

/// Load a to-many relation into `entity_manager` and cache it in `state`:
/// the rows it holds already, else those `query` reads, each giving way to
/// the instance the manager tracks, and recorded as the baseline a save diffs
/// the relation against; the caller ties `state` to the manager first, as for
/// a to-one relation.
#[cfg(feature = "entity-manager")]
pub(crate) async fn load_many_in_entity_manager<'s, E>(
    state: &'s mut RelationState<Vec<E>>,
    entity_manager: &Arc<EntityManager>,
    query: Result<QueryBuilder<E>>,
) -> Result<&'s Vec<E>>
where
    E: Model + TideEntityManagerMeta,
{
    let models = match state.take() {
        Some(models) => models,
        None => query?.get().await?,
    };
    let models = state.insert(models);
    let keys = register_loaded(entity_manager, models.iter_mut()).await?;
    state.loaded_keys = Some(keys);
    Ok(state.cached().expect("the rows were just cached"))
}

/// The entity-manager identity keys of the rows `models` holds, skipping
/// those not stored yet.
#[cfg(feature = "entity-manager")]
pub(crate) fn identity_keys<E: Model>(models: Option<&Vec<E>>) -> Result<Vec<String>> {
    models
        .into_iter()
        .flatten()
        .filter_map(|model| model_entity_manager_key(model).transpose())
        .collect()
}

/// Hand a relation's loaded models to `entity_manager`, replacing each in place
/// with the instance its identity map holds, and return the identity keys of
/// those stored.
///
/// `register` keeps an instance the manager already tracks — in-memory edits
/// and all — rather than letting this copy, possibly a stale eager load,
/// overwrite it, so the relation, `find` and the managed baselines share one
/// instance per row.
#[cfg(feature = "entity-manager")]
pub(crate) async fn register_loaded<'m, E>(
    entity_manager: &EntityManager,
    models: impl IntoIterator<Item = &'m mut E>,
) -> Result<Vec<String>>
where
    E: Model + TideEntityManagerMeta,
{
    let mut keys = Vec::new();
    for model in models {
        *model = entity_manager.register(model.clone()).await;
        if let Some(key) = model_entity_manager_key(&*model)? {
            keys.push(key);
        }
    }
    Ok(keys)
}

pub(crate) fn scoped_column(
    db_type: crate::config::DatabaseType,
    scope: &str,
    column: &str,
) -> String {
    format!(
        "{}.{}",
        quote_ident(db_type, scope),
        quote_ident(db_type, column)
    )
}

/// `E`'s deleted-at test, `IS NULL` for live rows or `IS NOT NULL` for trashed
/// ones, with the column qualified by `scope` when one is given; `None` for a
/// model without soft delete.
pub(crate) fn soft_delete_clause<E: Model>(
    db_type: crate::config::DatabaseType,
    scope: Option<&str>,
    trashed: bool,
) -> Option<String> {
    if !E::soft_delete_enabled() {
        return None;
    }
    let column = match scope {
        Some(scope) => scoped_column(db_type, scope, E::deleted_at_column()),
        None => quote_ident(db_type, E::deleted_at_column()),
    };
    let test = if trashed { "IS NOT NULL" } else { "IS NULL" };
    Some(format!("{column} {test}"))
}

/// Build the recursive-CTE query that walks a self-referencing tree.
///
/// The recursive term uses `UNION ALL`, so a cycle in the foreign key (`a -> b -> a`)
/// visits the same row once per level until `max_depth` stops it. The outer query
/// therefore collapses the walk to one row per node, keeping the shallowest depth it
/// was reached at, rather than emitting the node `max_depth` times. `GROUP BY` on the
/// primary key is used instead of `DISTINCT` because `SELECT DISTINCT` cannot order by
/// a column outside the select list on PostgreSQL, and because `DISTINCT` over
/// `result_node.*` is not valid for JSON/BLOB columns on several backends.
///
/// The walk carries every primary key column (`pk`, `pk_2`, ..) and the outer
/// join matches all of them: a composite key's first column alone can name a
/// row of another tenant.
pub(crate) fn build_self_ref_tree_sql<E: Model>(
    foreign_key: &str,
    local_key: &str,
    parent_pk: &serde_json::Value,
    max_depth: usize,
    db_type: crate::config::DatabaseType,
) -> Result<(String, Vec<Value>)> {
    let column = |name: &str| {
        E::canonical_column_name(name).ok_or_else(|| {
            Error::query(format!(
                "Unknown self-reference column '{}' for table '{}'",
                name,
                E::table_name()
            ))
        })
    };
    let foreign_key = column(foreign_key)?;
    let local_key = column(local_key)?;
    let primary_keys = match E::primary_key_names() {
        [] => vec![column(E::primary_key_name())?],
        names => names
            .iter()
            .map(|name| column(name))
            .collect::<Result<Vec<_>>>()?,
    };

    let table = crate::query::db_sql::quote_table::<E>(db_type);
    let cte = quote_ident(db_type, "tide_tree");
    let node = quote_ident(db_type, "node");
    let child = quote_ident(db_type, "child");
    let tree = quote_ident(db_type, "tree");
    let result = quote_ident(db_type, "result_node");
    let result_tree = quote_ident(db_type, "result_tree");
    let pk_aliases: Vec<String> = (1..=primary_keys.len())
        .map(|position| match position {
            1 => quote_ident(db_type, "pk"),
            n => quote_ident(db_type, &format!("pk_{n}")),
        })
        .collect();
    let tree_key_alias = quote_ident(db_type, "tree_key");
    let depth_alias = quote_ident(db_type, "depth");
    let select_keys = |scope: &str| {
        primary_keys
            .iter()
            .zip(&pk_aliases)
            .map(|(key, alias)| format!("{} AS {alias}", scoped_column(db_type, scope, key)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let grouped_keys = pk_aliases
        .iter()
        .map(|alias| format!("{cte}.{alias}"))
        .collect::<Vec<_>>();
    let result_join = primary_keys
        .iter()
        .zip(&pk_aliases)
        .map(|(key, alias)| {
            format!(
                "{} = {result_tree}.{alias}",
                scoped_column(db_type, "result_node", key)
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ");

    let mut params = Vec::with_capacity(2);
    let parent_placeholder = crate::internal::push_param(
        db_type,
        &mut params,
        crate::internal::json_to_column_value(
            parent_pk,
            crate::internal::column_type_of::<E>(foreign_key).as_ref(),
        ),
    );
    let max_depth = i64::try_from(max_depth)
        .map_err(|_| Error::query("Self-reference tree depth exceeds i64 range"))?;
    let depth_placeholder =
        crate::internal::push_param(db_type, &mut params, Value::BigInt(Some(max_depth)));

    let mut base_predicates = vec![format!(
        "{} = {}",
        scoped_column(db_type, "node", foreign_key),
        parent_placeholder
    )];
    if let Some(clause) = soft_delete_clause::<E>(db_type, Some("node"), false) {
        base_predicates.push(clause);
    }

    let mut recursive_predicates =
        vec![format!("{}.{} < {}", tree, depth_alias, depth_placeholder)];
    if let Some(clause) = soft_delete_clause::<E>(db_type, Some("child"), false) {
        recursive_predicates.push(clause);
    }

    let sql = format!(
        "WITH RECURSIVE {cte} ({pk_aliases}, {tree_key_alias}, {depth_alias}) AS ( \
         SELECT {node_keys}, {node_local_key} AS {tree_key_alias}, 1 AS {depth_alias} \
         FROM {table} {node} \
         WHERE {base_where} \
         UNION ALL \
         SELECT {child_keys}, {child_local_key} AS {tree_key_alias}, {tree}.{depth_alias} + 1 AS {depth_alias} \
         FROM {table} {child} \
         INNER JOIN {cte} {tree} ON {child_foreign_key} = {tree}.{tree_key_alias} \
         WHERE {recursive_where} \
         ) \
         SELECT {result_columns} \
         FROM {table} {result} \
         INNER JOIN ( \
         SELECT {grouped_select}, MIN({cte}.{depth_alias}) AS {depth_alias} \
         FROM {cte} \
         GROUP BY {group_by} \
         ) {result_tree} ON {result_join} \
         ORDER BY {result_tree}.{depth_alias}",
        cte = cte,
        result_columns = crate::query::db_sql::model_columns_sql::<E>(db_type, Some("result_node")),
        pk_aliases = pk_aliases.join(", "),
        tree_key_alias = tree_key_alias,
        depth_alias = depth_alias,
        node_keys = select_keys("node"),
        node_local_key = scoped_column(db_type, "node", local_key),
        table = table,
        node = node,
        base_where = base_predicates.join(" AND "),
        child_keys = select_keys("child"),
        child_local_key = scoped_column(db_type, "child", local_key),
        child = child,
        tree = tree,
        child_foreign_key = scoped_column(db_type, "child", foreign_key),
        recursive_where = recursive_predicates.join(" AND "),
        result = result,
        grouped_select = grouped_keys
            .iter()
            .zip(&pk_aliases)
            .map(|(key, alias)| format!("{key} AS {alias}"))
            .collect::<Vec<_>>()
            .join(", "),
        group_by = grouped_keys.join(", "),
        result_tree = result_tree,
        result_join = result_join,
    );

    Ok((sql, params))
}
