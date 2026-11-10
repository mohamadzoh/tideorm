use super::*;

use std::hash::{Hash, Hasher};

use crate::config::DatabaseType;
use crate::internal::Value;
use crate::query::db_sql::BoundSql;

/// One HAVING condition, rendered with the statement for its backend.
#[derive(Debug, Clone)]
pub(crate) enum HavingClause {
    /// Trusted SQL whose `?` placeholders bind `values`.
    Raw { sql: String, values: Vec<Value> },
    /// An aggregate compared with `value`; `None` compares it with NULL.
    Aggregate {
        aggregate: Aggregate,
        operator: &'static str,
        value: Option<Value>,
    },
}

impl HavingClause {
    pub(crate) fn hash_into<H: Hasher>(&self, hasher: &mut H) {
        std::mem::discriminant(self).hash(hasher);
        match self {
            Self::Raw { sql, values } => {
                sql.hash(hasher);
                values.len().hash(hasher);
                values.iter().for_each(|value| value.hash_binding(hasher));
            }
            Self::Aggregate {
                aggregate,
                operator,
                value,
            } => {
                aggregate.hash(hasher);
                operator.hash(hasher);
                value.is_some().hash(hasher);
                if let Some(value) = value {
                    value.hash_binding(hasher);
                }
            }
        }
    }
}

/// The operand of a numeric HAVING comparison: a number as itself, numeric
/// text as an exact decimal, `None` for anything else.
fn numeric_operand(value: &Value) -> Option<Value> {
    let decimal = match value {
        Value::Json(Some(json)) if json.is_number() => {
            return Some(crate::internal::json_to_db_value(json));
        }
        Value::Json(Some(json)) => json.as_str()?.trim().parse().ok()?,
        Value::String(Some(text)) => text.trim().parse().ok()?,
        Value::Decimal(Some(decimal)) => *decimal,
        value
            if crate::internal::is_integer_value(value)
                || matches!(value, Value::Float(Some(_)) | Value::Double(Some(_))) =>
        {
            return Some(crate::internal::bindable_value(value.clone()));
        }
        _ => return None,
    };
    Some(Value::Decimal(Some(decimal)))
}

/// A numeric operand as `db_type` compares it: SQLite has no decimal type,
/// so a decimal is the integer or float it spells there.
fn backend_number(value: &Value, db_type: DatabaseType) -> Value {
    use rust_decimal::prelude::ToPrimitive;
    match (db_type, value) {
        (DatabaseType::SQLite, Value::Decimal(Some(decimal))) => decimal
            .to_i64()
            .filter(|_| decimal.fract().is_zero())
            .map(Value::from)
            .or_else(|| decimal.to_f64().map(Value::from))
            .unwrap_or_else(|| value.clone()),
        _ => value.clone(),
    }
}

impl<M: Model> QueryBuilder<M> {
    /// Keep the groups that meet `condition`, an aggregate compared with a
    /// bound value. Several `having` calls must all hold; a raw SQL condition
    /// goes through [`having_raw`](Self::having_raw).
    ///
    /// ```ignore
    /// Sale::query()
    ///     .select_raw("region, SUM(revenue) AS revenue")
    ///     .group_by("region")
    ///     .having(Aggregate::sum("revenue").gt(5_000))
    ///     .having(Aggregate::count().gte(3))
    ///     .get_json()
    ///     .await?;
    /// ```
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().having(Aggregate::count().gt(1)).validate().unwrap();
    /// ```
    pub fn having(mut self, condition: impl Into<HavingCondition>) -> Self {
        let condition = match condition.into() {
            HavingCondition::Aggregate(condition) => condition,
            HavingCondition::Raw(sql) => {
                self.clauses.having_conditions.push(HavingClause::Raw {
                    sql,
                    values: Vec::new(),
                });
                return self;
            }
        };
        // Checked against encrypted columns when the query is validated, once
        // every join this HAVING may read is known.
        if let Some((column, operation)) = condition.aggregate.compared_column() {
            self.clauses
                .having_comparisons
                .push((column.to_string(), operation));
        }
        let value = match &condition.value {
            Err(reason) => {
                self.invalidate_query(format!("having(): {}", reason));
                return self;
            }
            Ok(value) if crate::query::is_sql_null(value) || crate::query::is_json_null(value) => {
                if !matches!(condition.operator, "=" | "<>") {
                    self.invalidate_query(
                        "HAVING ordering comparisons cannot use NULL".to_string(),
                    );
                    return self;
                }
                None
            }
            // A count, a sum or an average is a number; a minimum or a
            // maximum compares as the column it reads, bound once the query's
            // joins are known.
            Ok(value) if condition.aggregate.extreme_column().is_some() => Some(value.clone()),
            Ok(value) => match numeric_operand(value) {
                Some(value) => Some(value),
                None => {
                    self.invalidate_query(
                        "numeric HAVING comparisons require a number".to_string(),
                    );
                    return self;
                }
            },
        };
        self.clauses
            .having_conditions
            .push(HavingClause::Aggregate {
                aggregate: condition.aggregate,
                operator: condition.operator,
                value,
            });
        self
    }

    /// Add a raw HAVING condition.
    ///
    /// **Trusted SQL only**: it is checked for statement separators and
    /// comments, never escaped. Use [`having_raw_with`](Self::having_raw_with)
    /// to bind values.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().group_by("name").having_raw("COUNT(*) > 1").validate().unwrap();
    /// ```
    pub fn having_raw(self, raw_sql: &str) -> Self {
        self.having(HavingCondition::Raw(raw_sql.to_string()))
    }

    /// Add a raw HAVING condition whose `?` placeholders bind `params`, in
    /// order.
    ///
    /// The fragment is **trusted SQL**, checked as for
    /// [`having_raw`](Self::having_raw); the values are bound, never written
    /// into it, so they may come from a request. Write `?` on every backend.
    /// A count of placeholders other than the number of values fails the
    /// query.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().group_by("name").having_raw_with("COUNT(*) > ?", [2_i64.into()]).validate().unwrap();
    /// ```
    pub fn having_raw_with(
        mut self,
        raw_sql: &str,
        params: impl IntoIterator<Item = crate::internal::DbValue>,
    ) -> Self {
        let values = params
            .into_iter()
            .map(crate::internal::bindable_value)
            .collect();
        self.clauses.having_conditions.push(HavingClause::Raw {
            sql: raw_sql.to_string(),
            values,
        });
        self
    }

    /// `clause` rendered for `db_type`, its values bound in place.
    pub(in crate::query) fn having_bound(
        &self,
        db_type: DatabaseType,
        clause: &HavingClause,
    ) -> BoundSql {
        let (aggregate, operator, value) = match clause {
            HavingClause::Raw { sql, values } => {
                return db_sql::template_bound(sql, values.clone());
            }
            HavingClause::Aggregate {
                aggregate,
                operator,
                value,
            } => (aggregate, *operator, value),
        };
        // The model's own columns are written with its table, as every
        // HAVING aggregate always has been.
        let mut sql = BoundSql::sql(aggregate.render(&|column| {
            db_sql::format_column(db_type, &self.model_identifier(column.trim(), true))
        }));
        match (value, aggregate.extreme_column()) {
            (None, _) if operator == "=" => sql.push(" IS NULL"),
            (None, _) => sql.push(" IS NOT NULL"),
            (Some(value), Some(column)) => {
                sql.push(format!(" {operator} "))
                    .push(crate::internal::column_value(
                        value,
                        self.column_type(column).as_ref(),
                    ))
            }
            (Some(value), None) => sql
                .push(format!(" {operator} "))
                .push(backend_number(value, db_type)),
        };
        sql
    }
}
