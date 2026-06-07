use crate::config::DatabaseType;
use crate::internal::Value;

/// A helper for building raw SQL strings without `format!`.
///
/// Interpolates identifiers via `quote_ident` and accumulates bound
/// parameters into an external vector so placeholder indices stay correct
/// across sub-expression builders.
pub(crate) struct SqlBuilder<'a> {
    db_type: DatabaseType,
    sql: String,
    params: &'a mut Vec<Value>,
}

impl<'a> SqlBuilder<'a> {
    pub fn new(db_type: DatabaseType, params: &'a mut Vec<Value>) -> Self {
        Self {
            db_type,
            sql: String::new(),
            params,
        }
    }

    /// Append a raw SQL fragment (keywords, operators, spacing, etc.).
    pub fn raw(mut self, fragment: &str) -> Self {
        self.sql.push_str(fragment);
        self
    }

    /// Append a quoted identifier (`"table"`, `` `table` ``, etc.).
    pub fn ident(mut self, name: &str) -> Self {
        self.sql.push_str(&crate::internal::sql_safety::quote_ident(
            self.db_type,
            name,
        ));
        self
    }

    /// Push a bound parameter and append its placeholder (`?`, `$1`, etc.).
    pub fn param(mut self, value: Value) -> Self {
        let placeholder = super::push_param(self.db_type, self.params, value);
        self.sql.push_str(&placeholder);
        self
    }

    /// Append a placeholder that was already created via `push_param`.
    pub fn placeholder(mut self, ph: &str) -> Self {
        self.sql.push_str(ph);
        self
    }

    /// Consume the builder and return the built SQL string.
    pub fn into_sql(self) -> String {
        self.sql
    }
}
