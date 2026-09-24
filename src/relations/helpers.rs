#[cfg(feature = "entity-manager")]
use std::sync::Arc;

use crate::database::Database;
use crate::error::{Error, Result};
use crate::internal::Value;
use crate::model::Model;
use crate::query::QueryBuilder;

#[cfg(feature = "entity-manager")]
use crate::entity_manager::{EntityManager, TideEntityManagerMeta, model_entity_manager_key};

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
        return Err(Error::invalid_query(format!(
            "{} only supports scalar relation keys; composite primary keys require an explicit single-column relation key or a custom query",
            context
        )));
    }

    Ok(value)
}

/// Whether a query issued now has a connection to run on — the ambient
/// transaction or the global database.
pub(crate) fn has_active_database() -> bool {
    crate::database::__current_db().is_ok()
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

pub(crate) fn preserve_cached_value<C: Clone>(
    cached: &mut Option<C>,
    previous_cached: &Option<C>,
    allow_cached_without_context: bool,
    same_runtime_context: bool,
) {
    if (allow_cached_without_context && previous_cached.is_some()) || same_runtime_context {
        *cached = previous_cached.clone();
    }
}

/// Where a relation's statements run.
///
/// By default that is the scope's connection: the enclosing transaction, else
/// the global database. Under the `entity-manager` feature a relation can also
/// be tied to the manager that loaded it, or to the database its owner was read
/// from, which keeps it loadable with no global connection at all.
#[derive(Debug, Clone, Default)]
pub(crate) struct QuerySource {
    /// The manager that loaded the relation. It owns the cached instances, so
    /// `load` serves them rather than re-querying.
    #[cfg(feature = "entity-manager")]
    pub(crate) entity_manager: Option<Arc<EntityManager>>,
    /// The database the owning model was loaded from.
    #[cfg(feature = "entity-manager")]
    pub(crate) database: Option<Database>,
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
            if self.database.is_some() {
                return true;
            }
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

    /// The database the relation is tied to, unless an enclosing transaction
    /// outranks it: statements inside one must join it, and raw SQL has to be
    /// rendered for the backend it then reaches.
    fn attached(&self) -> Option<Database> {
        #[cfg(feature = "entity-manager")]
        if !matches!(
            crate::database::__current_connection(),
            Ok(crate::database::ConnectionRef::Transaction(_))
        ) {
            return match &self.entity_manager {
                Some(entity_manager) => Some(entity_manager.database().clone()),
                None => self.database.clone(),
            };
        }
        None
    }

    /// Keep whatever `previous` was tied to wherever this source is not.
    #[cfg(feature = "entity-manager")]
    pub(crate) fn preserve_from(&mut self, previous: &Self) {
        if self.entity_manager.is_none() {
            self.entity_manager = previous.entity_manager.clone();
        }
        if self.database.is_none() {
            self.database = previous.database.clone();
        }
    }
}

/// The owning row a loaded relation's snapshot is recorded under.
#[cfg(feature = "entity-manager")]
pub(crate) struct SnapshotOwner<'a> {
    table: &'static str,
    key: &'a str,
    relation: &'static str,
}

#[cfg(feature = "entity-manager")]
impl<'a> SnapshotOwner<'a> {
    /// Fails when the wrapper never received its owner's identity key, which
    /// the derive supplies through `with_owner_key`.
    pub(crate) fn new(
        table: &'static str,
        key: &'a Option<String>,
        relation: &'static str,
    ) -> Result<Self> {
        let key = key.as_deref().ok_or_else(|| {
            Error::query(format!(
                "entity manager owner key not set for relation '{relation}'"
            ))
        })?;
        Ok(Self {
            table,
            key,
            relation,
        })
    }
}

/// Hand a relation's loaded models to `entity_manager`, replacing each in place
/// with the instance its identity map holds.
///
/// `register` keeps an instance the manager already tracks — in-memory edits
/// and all — rather than letting this copy, possibly a stale eager load,
/// overwrite it, so the relation, `find` and the managed baselines share one
/// instance per row. With an `owner`, the models' keys are then recorded as the
/// snapshot a later save diffs the relation against.
#[cfg(feature = "entity-manager")]
pub(crate) async fn register_loaded<'m, E>(
    entity_manager: &EntityManager,
    models: impl IntoIterator<Item = &'m mut E>,
    owner: Option<SnapshotOwner<'_>>,
) -> Result<()>
where
    E: Model + TideEntityManagerMeta,
{
    let mut keys = Vec::new();
    for model in models {
        *model = entity_manager.register(model.clone()).await;
        if owner.is_some()
            && let Some(key) = model_entity_manager_key(&*model)?
        {
            keys.push(key);
        }
    }

    if let Some(owner) = owner {
        entity_manager.snapshot::<E>(owner.table, owner.key, owner.relation, &keys);
    }
    Ok(())
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

pub(crate) fn soft_delete_clause<E: Model>(
    db_type: crate::config::DatabaseType,
    scope: &str,
) -> Option<String> {
    if E::soft_delete_enabled() {
        Some(format!(
            "{} IS NULL",
            scoped_column(db_type, scope, E::deleted_at_column())
        ))
    } else {
        None
    }
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
    let primary_key = column(E::primary_key_name())?;

    let table = crate::query::db_sql::quote_table::<E>(db_type);
    let cte = quote_ident(db_type, "tide_tree");
    let node = quote_ident(db_type, "node");
    let child = quote_ident(db_type, "child");
    let tree = quote_ident(db_type, "tree");
    let result = quote_ident(db_type, "result_node");
    let pk_alias = quote_ident(db_type, "pk");
    let tree_key_alias = quote_ident(db_type, "tree_key");
    let depth_alias = quote_ident(db_type, "depth");

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
    if let Some(clause) = soft_delete_clause::<E>(db_type, "node") {
        base_predicates.push(clause);
    }

    let mut recursive_predicates =
        vec![format!("{}.{} < {}", tree, depth_alias, depth_placeholder)];
    if let Some(clause) = soft_delete_clause::<E>(db_type, "child") {
        recursive_predicates.push(clause);
    }

    let sql = format!(
        "WITH RECURSIVE {cte} ({pk_alias}, {tree_key_alias}, {depth_alias}) AS ( \
         SELECT {node_pk} AS {pk_alias}, {node_local_key} AS {tree_key_alias}, 1 AS {depth_alias} \
         FROM {table} {node} \
         WHERE {base_where} \
         UNION ALL \
         SELECT {child_pk} AS {pk_alias}, {child_local_key} AS {tree_key_alias}, {tree}.{depth_alias} + 1 AS {depth_alias} \
         FROM {table} {child} \
         INNER JOIN {cte} {tree} ON {child_foreign_key} = {tree}.{tree_key_alias} \
         WHERE {recursive_where} \
         ) \
         SELECT {result_columns} \
         FROM {table} {result} \
         INNER JOIN ( \
         SELECT {cte}.{pk_alias} AS {pk_alias}, MIN({cte}.{depth_alias}) AS {depth_alias} \
         FROM {cte} \
         GROUP BY {cte}.{pk_alias} \
         ) {result_tree} ON {result_pk} = {result_tree}.{pk_alias} \
         ORDER BY {result_tree}.{depth_alias}",
        cte = cte,
        result_columns = crate::query::db_sql::model_columns_sql::<E>(db_type, Some("result_node")),
        pk_alias = pk_alias,
        tree_key_alias = tree_key_alias,
        depth_alias = depth_alias,
        node_pk = scoped_column(db_type, "node", primary_key),
        node_local_key = scoped_column(db_type, "node", local_key),
        table = table,
        node = node,
        base_where = base_predicates.join(" AND "),
        child_pk = scoped_column(db_type, "child", primary_key),
        child_local_key = scoped_column(db_type, "child", local_key),
        child = child,
        tree = tree,
        child_foreign_key = scoped_column(db_type, "child", foreign_key),
        recursive_where = recursive_predicates.join(" AND "),
        result = result,
        result_tree = quote_ident(db_type, "result_tree"),
        result_pk = scoped_column(db_type, "result_node", primary_key),
    );

    Ok((sql, params))
}
