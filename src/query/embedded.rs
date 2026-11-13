//! Queries a query embeds: subqueries, union operands, CTE bodies and the
//! related rows of `where_has`. Each is kept as its builder and rendered with
//! the statement that embeds it, for the backend that statement runs on, its
//! values bound in place.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::{QueryBuilder, db_sql};
use crate::config::DatabaseType;
use crate::error::Result;
use crate::internal::Value;
use crate::model::Model;
use crate::orm::sql_builder::{BoundSql, SelectStatement};

/// A query another one embeds, rendered when the embedding statement is.
pub(crate) trait Embedded: Send + Sync {
    /// Its whole statement.
    fn select(&self, db_type: DatabaseType) -> Result<SelectStatement>;
    /// Its statement as an operand of a `UNION`.
    fn compound_operand(&self, db_type: DatabaseType) -> Result<SelectStatement>;
    /// Its table as a `FROM` names it, the name its rows go by there, and its
    /// `WHERE` predicate, empty without one.
    fn correlated(&self, db_type: DatabaseType) -> Result<(String, String, BoundSql)>;
    /// Whether it limits or skips rows.
    fn pages(&self) -> bool;
    /// Hash its shape and values into the embedding query's cache key.
    fn hash_shape(&self, hasher: &mut dyn Hasher);
    /// Write its model's table and clauses, for `Debug`.
    fn debug(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
    /// Why the project rules refuse raw SQL in it, if they do.
    fn raw_sql_refusal(&self) -> Option<crate::Error>;
}

impl std::fmt::Debug for dyn Embedded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.debug(f)
    }
}

impl<N: Model> Embedded for QueryBuilder<N> {
    fn select(&self, db_type: DatabaseType) -> Result<SelectStatement> {
        self.select_statement(db_type)
    }

    fn compound_operand(&self, db_type: DatabaseType) -> Result<SelectStatement> {
        self.compound_operand_statement(db_type)
    }

    fn correlated(&self, db_type: DatabaseType) -> Result<(String, String, BoundSql)> {
        // A model related to itself reads the related rows under an alias of
        // their own, so the key on the outer row stays reachable.
        let related = db_sql::quote_ident(db_type, &self.own_table_ref());
        let from = if self.self_join_depth > 0 {
            format!("{} AS {}", db_sql::quote_table::<N>(db_type), related)
        } else {
            db_sql::quote_table::<N>(db_type)
        };
        Ok((from, related, self.where_bound(db_type)?))
    }

    fn pages(&self) -> bool {
        self.clauses.limit_value.is_some() || self.clauses.offset_value.is_some()
    }

    fn hash_shape(&self, mut hasher: &mut dyn Hasher) {
        self.hash_query_shape(&mut hasher);
    }

    fn debug(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryBuilder")
            .field("table", &N::table_name())
            .field("clauses", &self.clauses)
            .finish()
    }

    fn raw_sql_refusal(&self) -> Option<crate::Error> {
        QueryBuilder::raw_sql_refusal(self)
    }
}

/// A query a WHERE condition embeds — [`where_in_subquery`], [`where_exists`],
/// [`where_has`], the [`has_related`] family — rendered with the statement
/// holding the condition, for the backend that statement runs on.
///
/// [`where_in_subquery`]: QueryBuilder::where_in_subquery
/// [`where_exists`]: QueryBuilder::where_exists
/// [`where_has`]: QueryBuilder::where_has
/// [`has_related`]: QueryBuilder::has_related
#[derive(Debug, Clone)]
pub struct Subquery(Arc<SubqueryKind>);

#[derive(Debug)]
enum SubqueryKind {
    /// `<keyword> (<query>)`: `IN`, `NOT IN`, `EXISTS` or `NOT EXISTS`.
    Select {
        keyword: &'static str,
        query: Box<dyn Embedded>,
    },
    /// `[NOT] EXISTS` over the related rows `related` keeps.
    Related {
        correlation: Correlation,
        related: Box<dyn Embedded>,
    },
    /// `[NOT] EXISTS` over the rows of a table named, not modeled, with
    /// `column`'s value, `None` for NULL, when `condition` is set.
    Table {
        correlation: Correlation,
        table: String,
        alias: Option<String>,
        condition: Option<(String, Option<Value>)>,
    },
}

/// How a correlated `EXISTS` ties the related rows to the outer row.
#[derive(Debug, Hash)]
pub(crate) struct Correlation {
    pub(crate) negated: bool,
    /// The related rows' column holding the outer row's key.
    pub(crate) foreign_key: String,
    /// The name the outer row's table goes by.
    pub(crate) outer: String,
    /// The outer row's key column.
    pub(crate) local_key: String,
}

impl Correlation {
    /// `[NOT ]EXISTS (SELECT 1 FROM <from> WHERE <related>.<fk> = <outer>.<lk>`,
    /// left open for the caller's conditions.
    fn head(&self, db_type: DatabaseType, from: &str, related: &str) -> BoundSql {
        BoundSql::sql(format!(
            "{}EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{}",
            if self.negated { "NOT " } else { "" },
            from,
            related,
            db_sql::quote_ident(db_type, &self.foreign_key),
            db_sql::quote_ident(db_type, &self.outer),
            db_sql::quote_ident(db_type, &self.local_key),
        ))
    }
}

impl Subquery {
    /// Why the project rules refuse raw SQL in the query it embeds, if they do.
    pub(crate) fn raw_sql_refusal(&self) -> Option<crate::Error> {
        match &*self.0 {
            SubqueryKind::Select { query, .. } => query.raw_sql_refusal(),
            SubqueryKind::Related { related, .. } => related.raw_sql_refusal(),
            SubqueryKind::Table { .. } => None,
        }
    }

    pub(crate) fn select(keyword: &'static str, query: impl Embedded + 'static) -> Self {
        Self(Arc::new(SubqueryKind::Select {
            keyword,
            query: Box::new(query),
        }))
    }

    pub(crate) fn related(correlation: Correlation, related: impl Embedded + 'static) -> Self {
        Self(Arc::new(SubqueryKind::Related {
            correlation,
            related: Box::new(related),
        }))
    }

    pub(crate) fn table(
        correlation: Correlation,
        table: String,
        alias: Option<String>,
        condition: Option<(String, Option<Value>)>,
    ) -> Self {
        Self(Arc::new(SubqueryKind::Table {
            correlation,
            table,
            alias,
            condition,
        }))
    }

    /// The condition for `db_type`, with its values bound in place.
    pub(crate) fn render(&self, db_type: DatabaseType) -> Result<BoundSql> {
        let mut sql = match &*self.0 {
            SubqueryKind::Select { keyword, query } => {
                let mut inner = query.select(db_type)?;
                if keyword.ends_with("IN")
                    && !db_type.dialect().limit_in_subquery()
                    && query.pages()
                {
                    inner = db_sql::select_from_derived("*", inner, "tideorm_in_subquery");
                }
                let mut sql = BoundSql::sql(format!("{keyword} ("));
                sql.push(
                    db_type
                        .build_bound(&inner)
                        .map_err(crate::internal::translate_error)?,
                );
                sql
            }
            SubqueryKind::Related {
                correlation,
                related,
            } => {
                let (from, related, filters) = related.correlated(db_type)?;
                let mut sql = correlation.head(db_type, &from, &related);
                if !filters.is_empty() {
                    sql.push(" AND (").push(filters).push(")");
                }
                sql
            }
            SubqueryKind::Table {
                correlation,
                table,
                alias,
                condition,
            } => {
                let quoted = db_sql::quote_ident(db_type, table);
                let (from, related) = match alias {
                    Some(alias) => {
                        let alias = db_sql::quote_ident(db_type, alias);
                        (format!("{} AS {}", quoted, alias), alias)
                    }
                    None => (quoted.clone(), quoted),
                };
                let mut sql = correlation.head(db_type, &from, &related);
                if let Some((column, value)) = condition {
                    sql.push(format!(
                        " AND {}.{}",
                        related,
                        db_sql::quote_ident(db_type, column)
                    ));
                    match value {
                        Some(value) => sql.push(" = ").push(value.clone()),
                        None => sql.push(" IS NULL"),
                    };
                }
                sql
            }
        };
        sql.push(")");
        Ok(sql)
    }

    /// The table a `has_related` condition names, which no model's cache
    /// dependencies list.
    pub(crate) fn named_table(&self) -> Option<&str> {
        match &*self.0 {
            SubqueryKind::Table { table, .. } => Some(table),
            _ => None,
        }
    }

    pub(crate) fn hash_into<H: Hasher>(&self, hasher: &mut H) {
        match &*self.0 {
            SubqueryKind::Select { keyword, query } => {
                keyword.hash(hasher);
                query.hash_shape(hasher);
            }
            SubqueryKind::Related {
                correlation,
                related,
            } => {
                correlation.hash(hasher);
                related.hash_shape(hasher);
            }
            SubqueryKind::Table {
                correlation,
                table,
                alias,
                condition,
            } => {
                correlation.hash(hasher);
                (table, alias).hash(hasher);
                if let Some((column, value)) = condition {
                    column.hash(hasher);
                    if let Some(value) = value {
                        value.hash_binding(hasher);
                    }
                }
            }
        }
    }
}

/// Two conditions are equal when they embed the same query.
impl PartialEq for Subquery {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// What the condition tests, without its SQL, which depends on the backend.
impl std::fmt::Display for Subquery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &*self.0 {
            SubqueryKind::Select { keyword, .. } => write!(f, "{keyword} (subquery)"),
            SubqueryKind::Related { correlation, .. } | SubqueryKind::Table { correlation, .. } => {
                write!(
                    f,
                    "{}EXISTS (related rows)",
                    if correlation.negated { "NOT " } else { "" }
                )
            }
        }
    }
}

/// The body of a union operand or a CTE: another query, or trusted SQL whose
/// `?` placeholders bind `values`, written as given without them.
#[derive(Debug, Clone)]
pub(crate) enum Operand {
    Query(Arc<dyn Embedded>),
    Sql {
        sql: String,
        values: Option<Vec<Value>>,
    },
}

impl Operand {
    pub(crate) fn hash_into<H: Hasher>(&self, hasher: &mut H) {
        match self {
            Self::Query(query) => query.hash_shape(hasher),
            Self::Sql { sql, values } => {
                sql.hash(hasher);
                for value in values.iter().flatten() {
                    value.hash_binding(hasher);
                }
            }
        }
    }
}
