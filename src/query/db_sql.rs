use crate::config::DatabaseType;

mod placeholders;

pub(crate) use crate::internal::sql_safety::{
    format_identifier_reference, is_safe_identifier_segment, push_quoted_ident,
    push_quoted_qualified, quote_ident, validate_compound_subquery_sql,
    validate_having_sql_fragment, validate_identifier, validate_identifier_reference,
    validate_join_column, validate_raw_sql_fragment, validate_subquery_sql,
};
pub(crate) use crate::orm::sql_builder::{BoundSql, MATCH_NOTHING, parse_json_path};
use crate::orm::sql_builder::{Expr, SchemaName, SelectStatement, TableName};

/// `base`, with `_` appended until `taken` no longer holds: a name for a
/// generated output that no model column or other output already uses.
pub(crate) fn unused_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut name = base.to_string();
    while taken(&name) {
        name.push('_');
    }
    name
}

/// `SELECT <projection> FROM (<inner>) AS <alias>`: a query's rows read
/// through a derived table.
pub(crate) fn select_from_derived(
    projection: impl Into<String>,
    inner: SelectStatement,
    alias: &'static str,
) -> SelectStatement {
    let mut select = SelectStatement::new();
    select
        .expr(Expr::cust(projection.into()))
        .from_subquery(inner, alias);
    select
}

/// `statement` rendered for `db_type`: its SQL and the values it binds.
pub(crate) fn built(
    db_type: DatabaseType,
    statement: &impl crate::orm::StatementBuilder,
) -> crate::error::Result<(String, Vec<crate::internal::Value>)> {
    let statement = db_type
        .build(statement)
        .map_err(crate::internal::translate_error)?;
    Ok((
        statement.sql,
        statement.values.map_or_else(Vec::new, |values| values.0),
    ))
}

/// A table a statement reads, qualified with `schema` when given.
pub(crate) fn table_name(schema: Option<&str>, table: &str) -> TableName {
    TableName(
        schema.map(|schema| SchemaName(None, schema.to_owned().into())),
        table.to_owned().into(),
    )
}

pub(crate) use placeholders::{count_template_placeholders, inline_parameters, template_bound};

/// `M`'s table as a statement names it: quoted, and qualified with the model's
/// schema when it declares one.
///
/// Column references stay qualified with the bare table name, which every
/// backend resolves against a schema-qualified `FROM`.
pub(crate) fn quote_table<M: crate::model::ModelMeta>(db_type: DatabaseType) -> String {
    let mut table = String::new();
    push_quoted_table::<M>(&mut table, db_type);
    table
}

/// Append [`quote_table`]'s rendering to `out`.
pub(crate) fn push_quoted_table<M: crate::model::ModelMeta>(
    out: &mut String,
    db_type: DatabaseType,
) {
    push_quoted_qualified(out, db_type, M::schema_name(), M::table_name());
}

/// `M`'s own columns as a projection, each qualified with `qualifier` when given.
///
/// Model reads name their columns instead of selecting `*`, so a column added
/// to the table cannot change the result shape of a statement a connection has
/// already prepared: PostgreSQL rejects that statement (`cached plan must not
/// change result type`) until the connection is recycled, and SQLite's driver
/// mis-decodes its rows.
pub(crate) fn model_columns_sql<M: crate::model::ModelMeta>(
    db_type: DatabaseType,
    qualifier: Option<&str>,
) -> String {
    let columns = M::column_names();
    let mut sql = String::with_capacity(columns.len() * 24);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        if let Some(qualifier) = qualifier {
            push_quoted_ident(&mut sql, db_type, qualifier);
            sql.push('.');
        }
        push_quoted_ident(&mut sql, db_type, column);
    }
    sql
}

/// Format a trusted column/expression slot for rendering paths that intentionally
/// allow raw SQL expressions after higher-level validation.
///
/// Anything that is not a plain identifier reference is emitted verbatim, so a
/// caller must have validated it first. ORDER BY and GROUP BY no longer reach
/// here with unvalidated text: `QueryBuilder` restricts those slots to a
/// resolvable column and routes real expressions through `order_by_raw()`.
pub(crate) fn format_column_or_trusted_expression(
    db_type: DatabaseType,
    column_or_expression: &str,
) -> String {
    let trimmed = column_or_expression.trim();
    format_identifier_reference(db_type, trimmed).unwrap_or_else(|| trimmed.to_string())
}

/// Format a column identifier for the database.
///
/// This helper is intentionally strict: if the input is not a simple
/// identifier reference like `column` or `table.column`, it is quoted as a
/// single identifier instead of being passed through as raw SQL. Call
/// `format_column_or_trusted_expression()` only from rendering paths that
/// intentionally support validated raw expressions.
pub(crate) fn format_column(db_type: DatabaseType, column: &str) -> String {
    let trimmed = column.trim();
    format_identifier_reference(db_type, trimmed).unwrap_or_else(|| quote_ident(db_type, trimmed))
}
