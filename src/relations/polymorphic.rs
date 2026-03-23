use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::require_scalar_relation_key;

#[derive(Debug, Clone)]
pub struct MorphTo<Morphable> {
    pub type_column: &'static str,
    pub id_column: &'static str,
    type_value: Option<String>,
    id_value: Option<serde_json::Value>,
    _marker: PhantomData<Morphable>,
}

impl<Morphable> MorphTo<Morphable> {
    pub fn new(type_column: &'static str, id_column: &'static str) -> Self {
        Self {
            type_column,
            id_column,
            type_value: None,
            id_value: None,
            _marker: PhantomData,
        }
    }

    pub fn with_values(mut self, type_value: String, id_value: serde_json::Value) -> Self {
        self.type_value = Some(type_value);
        self.id_value = Some(id_value);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        if self.type_column == previous.type_column
            && self.id_column == previous.id_column
        {
            self.type_value = previous.type_value.clone();
            self.id_value = previous.id_value.clone();
        }
    }
}

impl<Morphable> Default for MorphTo<Morphable> {
    fn default() -> Self {
        Self {
            type_column: "",
            id_column: "",
            type_value: None,
            id_value: None,
            _marker: PhantomData,
        }
    }
}

impl<Morphable: Serialize> Serialize for MorphTo<Morphable> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_none()
    }
}

impl<'de, Morphable> Deserialize<'de> for MorphTo<Morphable> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

#[derive(Debug, Clone)]
pub struct MorphOne<Related: Model> {
    pub morph_name: &'static str,
    pub local_key: &'static str,
    cached: Option<Box<Related>>,
    parent_pk: Option<serde_json::Value>,
    parent_table: Option<String>,
    _marker: PhantomData<Related>,
}

impl<Related: Model> MorphOne<Related> {
    fn ensure_configured(&self) -> Result<()> {
        if self.morph_name.is_empty() || self.local_key.is_empty() {
            Err(Error::query(String::from(
                "MorphOne relation is not configured; use MorphOne::new(...) or a macro-generated relation field",
            )))
        } else {
            Ok(())
        }
    }

    pub fn new(morph_name: &'static str, local_key: &'static str) -> Self {
        Self {
            morph_name,
            local_key,
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
        }
    }

    pub fn with_parent(mut self, pk: serde_json::Value, table: String) -> Self {
        self.parent_pk = Some(pk);
        self.parent_table = Some(table);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        if self.morph_name == previous.morph_name
            && self.local_key == previous.local_key
            && self.parent_pk == previous.parent_pk
            && self.parent_table == previous.parent_table
        {
            self.cached = previous.cached.clone();
        }
    }

    pub async fn load(&self) -> Result<Option<Related>> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "MorphOne::load")?;
        let table = self
            .parent_table
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;

        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);

        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .first()
            .await
    }

    pub fn get_cached(&self) -> Option<&Related> {
        self.cached.as_deref()
    }
}

impl<Related: Model> Default for MorphOne<Related> {
    fn default() -> Self {
        Self {
            morph_name: "",
            local_key: "",
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize> Serialize for MorphOne<Related> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model> Deserialize<'de> for MorphOne<Related> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

#[derive(Debug, Clone)]
pub struct MorphMany<Related: Model> {
    pub morph_name: &'static str,
    pub local_key: &'static str,
    cached: Option<Vec<Related>>,
    parent_pk: Option<serde_json::Value>,
    parent_table: Option<String>,
    _marker: PhantomData<Related>,
}

impl<Related: Model> MorphMany<Related> {
    fn ensure_configured(&self) -> Result<()> {
        if self.morph_name.is_empty() || self.local_key.is_empty() {
            Err(Error::query(String::from(
                "MorphMany relation is not configured; use MorphMany::new(...) or a macro-generated relation field",
            )))
        } else {
            Ok(())
        }
    }

    pub fn new(morph_name: &'static str, local_key: &'static str) -> Self {
        Self {
            morph_name,
            local_key,
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
        }
    }

    pub fn with_parent(mut self, pk: serde_json::Value, table: String) -> Self {
        self.parent_pk = Some(pk);
        self.parent_table = Some(table);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        if self.morph_name == previous.morph_name
            && self.local_key == previous.local_key
            && self.parent_pk == previous.parent_pk
            && self.parent_table == previous.parent_table
        {
            self.cached = previous.cached.clone();
        }
    }

    pub async fn load(&self) -> Result<Vec<Related>> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "MorphMany::load")?;
        let table = self
            .parent_table
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;

        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);

        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .get()
            .await
    }

    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "MorphMany::load_with")?;
        let table = self
            .parent_table
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;

        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);

        let query = Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone());

        constraint_fn(query).get().await
    }

    pub async fn count(&self) -> Result<u64> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "MorphMany::count")?;
        let table = self
            .parent_table
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;

        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);

        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .count()
            .await
    }

    pub fn get_cached(&self) -> Option<&[Related]> {
        self.cached.as_deref()
    }
}

impl<Related: Model> Default for MorphMany<Related> {
    fn default() -> Self {
        Self {
            morph_name: "",
            local_key: "",
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize> Serialize for MorphMany<Related> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model> Deserialize<'de> for MorphMany<Related> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

#[derive(Debug, Clone)]
pub enum MorphResult<A, B> {
    TypeA(A),
    TypeB(B),
    Unknown(serde_json::Value),
}

impl<A, B> MorphResult<A, B> {
    pub fn is_type_a(&self) -> bool {
        matches!(self, MorphResult::TypeA(_))
    }

    pub fn is_type_b(&self) -> bool {
        matches!(self, MorphResult::TypeB(_))
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, MorphResult::Unknown(_))
    }

    pub fn as_type_a(&self) -> Option<&A> {
        match self {
            MorphResult::TypeA(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_type_b(&self) -> Option<&B> {
        match self {
            MorphResult::TypeB(b) => Some(b),
            _ => None,
        }
    }

    pub fn into_type_a(self) -> Option<A> {
        match self {
            MorphResult::TypeA(a) => Some(a),
            _ => None,
        }
    }

    pub fn into_type_b(self) -> Option<B> {
        match self {
            MorphResult::TypeB(b) => Some(b),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MorphResult3<A, B, C> {
    TypeA(A),
    TypeB(B),
    TypeC(C),
    Unknown(serde_json::Value),
}

#[derive(Debug, Clone)]
pub enum MorphResult4<A, B, C, D> {
    TypeA(A),
    TypeB(B),
    TypeC(C),
    TypeD(D),
    Unknown(serde_json::Value),
}
