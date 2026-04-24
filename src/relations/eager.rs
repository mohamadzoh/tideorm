use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::columns::IntoColumnName;
use crate::error::{Error, Result};
use crate::internal::InternalModel;
use crate::model::Model;
use crate::query::{Order, QueryBuilder};

fn apply_primary_key_filter<M: Model>(
    mut query: QueryBuilder<M>,
    primary_key: &M::PrimaryKey,
) -> Result<QueryBuilder<M>> {
    let values = match serde_json::to_value(primary_key)
        .map_err(|e| Error::conversion(format!("Failed to serialize primary key: {}", e)))?
    {
        serde_json::Value::Array(values) => values,
        value => vec![value],
    };

    let columns = M::primary_key_names();
    if values.len() != columns.len() {
        return Err(Error::invalid_query(format!(
            "Primary key value for {} did not match declared key columns",
            M::table_name()
        )));
    }

    for (column, value) in columns.iter().zip(values) {
        query = query.where_eq(*column, value);
    }

    Ok(query)
}

#[derive(Debug, Clone, Default)]
pub struct RelationConstraints {
    pub conditions: Vec<(String, serde_json::Value)>,
    pub order_by: Option<(String, Order)>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub with_trashed: bool,
    pub only_trashed: bool,
}

impl RelationConstraints {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn where_eq(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions
            .push((column.column_name().to_string(), value.into()));
        self
    }

    pub fn order_by(mut self, column: impl IntoColumnName, order: Order) -> Self {
        self.order_by = Some((column.column_name().to_string(), order));
        self
    }

    pub fn limit(mut self, n: u64) -> Self {
        self.limit = Some(n);
        self
    }

    pub fn offset(mut self, n: u64) -> Self {
        self.offset = Some(n);
        self
    }

    pub fn with_trashed(mut self) -> Self {
        self.with_trashed = true;
        self
    }

    pub fn only_trashed(mut self) -> Self {
        self.only_trashed = true;
        self
    }

    pub fn apply<M: Model>(self, mut query: QueryBuilder<M>) -> QueryBuilder<M> {
        for (column, value) in self.conditions {
            query = query.where_eq(&column, value);
        }

        if let Some((column, order)) = self.order_by {
            query = query.order_by(&column, order);
        }

        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }

        if let Some(offset) = self.offset {
            query = query.offset(offset);
        }

        if self.with_trashed {
            query = query.with_trashed();
        }

        if self.only_trashed {
            query = query.only_trashed();
        }

        query
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithRelations<M> {
    #[serde(flatten)]
    pub model: M,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub relations: HashMap<String, serde_json::Value>,
}

impl<M: Model> WithRelations<M> {
    pub fn new(model: M) -> Self {
        Self {
            model,
            relations: HashMap::new(),
        }
    }

    pub fn with_relation(mut self, name: &str, data: serde_json::Value) -> Self {
        self.relations.insert(name.to_string(), data);
        self
    }

    pub fn get_relation<R: for<'de> Deserialize<'de>>(&self, name: &str) -> Option<R> {
        self.relations
            .get(name)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn has_relation(&self, name: &str) -> bool {
        self.relations.contains_key(name)
    }

    pub fn into_inner(self) -> M {
        self.model
    }
}

impl<M> std::ops::Deref for WithRelations<M> {
    type Target = M;

    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl<M> std::ops::DerefMut for WithRelations<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.model
    }
}

#[derive(Debug, Clone)]
pub struct RelationPath {
    pub full_path: String,
    pub segments: Vec<String>,
}

impl RelationPath {
    pub fn parse(path: &str) -> Self {
        let segments: Vec<String> = path.split('.').map(|s| s.to_string()).collect();
        Self {
            full_path: path.to_string(),
            segments,
        }
    }

    pub fn root(&self) -> &str {
        self.segments.first().map(|s| s.as_str()).unwrap_or("")
    }

    pub fn nested(&self) -> Option<RelationPath> {
        if self.segments.len() > 1 {
            Some(RelationPath {
                full_path: self.segments[1..].join("."),
                segments: self.segments[1..].to_vec(),
            })
        } else {
            None
        }
    }

    pub fn is_nested(&self) -> bool {
        self.segments.len() > 1
    }

    pub fn depth(&self) -> usize {
        self.segments.len()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RelationTree {
    children: HashMap<String, RelationTree>,
}

impl RelationTree {
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
        }
    }

    pub fn add_path(&mut self, path: &RelationPath) {
        if path.segments.is_empty() {
            return;
        }

        let root = path.root().to_string();
        let child = self.children.entry(root).or_default();

        if let Some(nested) = path.nested() {
            child.add_path(&nested);
        }
    }

    pub fn roots(&self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }

    pub fn get_nested(&self, name: &str) -> Option<&RelationTree> {
        self.children.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    pub fn has_nested(&self, name: &str) -> bool {
        self.children
            .get(name)
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    }
}

pub struct EagerQueryBuilder<M: Model> {
    query: QueryBuilder<M>,
    relation_tree: RelationTree,
}

#[async_trait]
#[doc(hidden)]
pub trait EagerLoadModel: Model + InternalModel {
    async fn __eager_load(
        models: &mut [WithRelations<Self>],
        relation_tree: &RelationTree,
    ) -> Result<()>
    where
        Self: Sized;
}

impl<M: Model> EagerQueryBuilder<M> {
    pub fn new() -> Self {
        Self {
            query: QueryBuilder::new(),
            relation_tree: RelationTree::new(),
        }
    }

    #[must_use]
    pub(crate) fn from_query(query: QueryBuilder<M>) -> Self {
        Self {
            query,
            relation_tree: RelationTree::new(),
        }
    }

    pub fn with(mut self, relation: &str) -> Self {
        let path = RelationPath::parse(relation);
        self.relation_tree.add_path(&path);
        self
    }

    pub fn with_many(mut self, relations: &[&str]) -> Self {
        for relation in relations {
            self = self.with(relation);
        }
        self
    }

    pub fn where_eq<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        value: V,
    ) -> Self {
        self.query = self.query.where_eq(column, value);
        self
    }

    pub fn where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.query = self.query.where_in(column, values);
        self
    }

    pub fn where_raw(mut self, sql: &str) -> Self {
        self.query = self.query.where_raw(sql);
        self
    }

    pub fn order_by(mut self, column: impl IntoColumnName, order: Order) -> Self {
        self.query = self.query.order_by(column, order);
        self
    }

    pub fn limit(mut self, n: u64) -> Self {
        self.query = self.query.limit(n);
        self
    }

    pub fn offset(mut self, n: u64) -> Self {
        self.query = self.query.offset(n);
        self
    }

    pub fn get_relation_tree(&self) -> &RelationTree {
        &self.relation_tree
    }

    pub async fn get(self) -> Result<Vec<WithRelations<M>>>
    where
        M: EagerLoadModel,
    {
        let models = self.query.get().await?;
        let mut results: Vec<WithRelations<M>> =
            models.into_iter().map(WithRelations::new).collect();
        M::__eager_load(&mut results, &self.relation_tree).await?;
        Ok(results)
    }

    pub async fn first(mut self) -> Result<Option<WithRelations<M>>>
    where
        M: EagerLoadModel,
    {
        self.query = self.query.limit(1);
        let results = self.get().await?;
        Ok(results.into_iter().next())
    }

    pub async fn find(mut self, id: M::PrimaryKey) -> Result<Option<WithRelations<M>>>
    where
        M: EagerLoadModel,
    {
        self.query = apply_primary_key_filter(self.query, &id)?.limit(1);
        self.first().await
    }
}

impl<M: Model> Default for EagerQueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RelationLoader<M> {
    pub name: String,
    #[allow(clippy::type_complexity)]
    pub loader: Box<
        dyn Fn(
                &[M],
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<HashMap<String, serde_json::Value>>>
                        + Send,
                >,
            > + Send
            + Sync,
    >,
}

pub trait EagerLoadExt: Model {
    fn eager() -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new()
    }

    fn with_relation(relation_name: &str) -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new().with(relation_name)
    }

    fn with_relations(relations: &[&str]) -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new().with_many(relations)
    }
}

impl<T: Model> EagerLoadExt for T {}

#[async_trait]
pub trait RelationExt: Model {
    fn get_field_value(&self, field: &str) -> Result<serde_json::Value> {
        if let Some(value) = <Self as InternalModel>::field_json_value(self, field)? {
            return Ok(value);
        }

        let json = serde_json::to_value(self)
            .map_err(|e| Error::query(format!("Failed to serialize model: {}", e)))?;

        json.get(field)
            .cloned()
            .ok_or_else(|| Error::query(format!("Field '{}' not found on model", field)))
    }
}

impl<T: Model> RelationExt for T {}
