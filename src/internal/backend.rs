use super::{OrmStatement, Value};
use crate::config::DatabaseType;

pub(crate) fn build_statement(backend: DatabaseType, sql: impl Into<String>) -> OrmStatement {
    OrmStatement::from_string(backend, sql.into())
}

pub(crate) fn build_statement_with_values(
    backend: DatabaseType,
    sql: &str,
    params: Vec<Value>,
) -> OrmStatement {
    OrmStatement::from_sql_and_values(backend, sql, params)
}
