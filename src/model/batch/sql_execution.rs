use super::*;

use crate::internal::push_param;

/// `factor` as the decimal it is written as (`0.1`, not the binary fraction
/// nearest to it), when a `Decimal` holds it.
fn exact_decimal(factor: f64) -> Option<rust_decimal::Decimal> {
    use std::str::FromStr;

    rust_decimal::Decimal::from_str(&factor.to_string())
        .ok()
        .map(|exact| exact.normalize())
}

/// `col` scaled by `factor` in SQLite's integer arithmetic: multiplied by the
/// factor's numerator and divided by its denominator (the other way round to
/// divide), with the quotient rounded half away from zero, as `ROUND` does.
///
/// The column is divided first, `(col / d) * n` plus its remainder's share
/// `(col % d) * n / d` rounded, so no intermediate product outgrows the
/// result: SQLite turns an integer overflow into a REAL, which would lose
/// the low digits of a large value. `None` when the remainder's share could
/// overflow on its own.
fn sqlite_integer_scale(
    col: &str,
    factor: rust_decimal::Decimal,
    divide: bool,
    db_type: crate::config::DatabaseType,
    params: &mut Vec<crate::internal::Value>,
) -> Option<String> {
    let numerator = i64::try_from(factor.mantissa()).ok()?;
    let denominator = 10_i64.checked_pow(factor.scale())?;
    let common = i64::try_from(gcd(numerator.unsigned_abs(), denominator.unsigned_abs())).ok()?;
    let (numerator, denominator) = (numerator / common, denominator / common);
    let (times, over) = if divide {
        (denominator, numerator)
    } else {
        (numerator, denominator)
    };
    // The divisor stays positive, so its half rounds away from zero.
    let (times, over) = if over < 0 {
        (times.checked_neg()?, over.checked_neg()?)
    } else {
        (times, over)
    };

    // The remainder is smaller than the divisor, so its share stays within
    // `(over - 1) * |times| + over / 2`.
    (over - 1)
        .checked_mul(times.checked_abs()?)?
        .checked_add(over / 2)?;

    // Each value is bound where it stands in the statement, in order.
    let mut bind =
        |value: i64| push_param(db_type, params, crate::internal::Value::BigInt(Some(value)));
    let scaled = |operand: String, bind: &mut dyn FnMut(i64) -> String| match times {
        1 => operand,
        _ => format!("{operand} * {}", bind(times)),
    };
    if over == 1 {
        return Some(scaled(col.to_string(), &mut bind));
    }
    let divided = format!("({col} / {})", bind(over));
    let quotient = scaled(divided, &mut bind);
    let remainder = format!("({col} % {})", bind(over));
    let remainder = scaled(remainder, &mut bind);
    // The remainder's share takes the sign of `col * times`, and its half
    // rounds away from zero.
    let half = over / 2;
    let (below_zero, otherwise) = if times < 0 {
        (half, -half)
    } else {
        (-half, half)
    };
    let below_zero = bind(below_zero);
    let otherwise = bind(otherwise);
    let divisor = bind(over);
    Some(format!(
        "{quotient} + ({remainder} + CASE WHEN {col} < 0 THEN {below_zero} ELSE {otherwise} END) / {divisor}"
    ))
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl<M: Model> BatchUpdateBuilder<M> {
    /// Bind `value` with the type of the column it is assigned to.
    fn column_value(column: &str, value: &serde_json::Value) -> crate::internal::Value {
        crate::internal::json_to_assignment_value(
            value,
            crate::internal::column_type_of::<M>(column).as_ref(),
        )
    }

    /// `col` multiplied, or with `divide` divided, by `factor`.
    ///
    /// An integer column is scaled exactly and rounded half away from zero,
    /// since an `f64` holds integers exactly only up to 2^53: PostgreSQL,
    /// MySQL and MariaDB take the factor as a decimal, and SQLite, which has
    /// no decimal arithmetic, as a fraction of two integers. SQLite would
    /// otherwise store the REAL product, after which the model cannot read
    /// the row. A decimal column takes the factor as a decimal too, where the
    /// backend has one, and any other column as an `f64`.
    fn scaled(
        column: &str,
        col: &str,
        db_type: crate::config::DatabaseType,
        params: &mut Vec<crate::internal::Value>,
        factor: f64,
        divide: bool,
    ) -> String {
        use crate::config::DatabaseType;
        use crate::orm::ColumnType;

        let column_type = crate::internal::column_type_of::<M>(column);
        let integral = column_type
            .as_ref()
            .is_some_and(crate::internal::is_integer);
        let decimal = matches!(
            column_type,
            Some(ColumnType::Decimal(_) | ColumnType::Money(_))
        );
        let operator = if divide { "/" } else { "*" };

        match (db_type, exact_decimal(factor)) {
            (DatabaseType::SQLite, Some(exact)) if integral => {
                if let Some(scaled) = sqlite_integer_scale(col, exact, divide, db_type, params) {
                    return format!("CAST({scaled} AS INTEGER)");
                }
            }
            (DatabaseType::Postgres | DatabaseType::MySQL | DatabaseType::MariaDB, Some(exact))
                if integral || decimal =>
            {
                let placeholder = push_param(
                    db_type,
                    params,
                    crate::internal::Value::Decimal(Some(exact)),
                );
                return format!("{col} {operator} {placeholder}");
            }
            _ => {}
        }

        let placeholder = push_param(
            db_type,
            params,
            crate::internal::Value::Double(Some(factor)),
        );
        let scaled = format!("{col} {operator} {placeholder}");
        if integral && db_type == DatabaseType::SQLite {
            format!("CAST(ROUND({scaled}) AS INTEGER)")
        } else {
            scaled
        }
    }

    fn build_assignment_sql(
        column: &str,
        value: &UpdateValue,
        db_type: crate::config::DatabaseType,
        params: &mut Vec<crate::internal::Value>,
    ) -> Result<String> {
        let col = Self::quote_update_column(column, db_type)?;

        match value {
            UpdateValue::Value(value) => {
                let placeholder = push_param(db_type, params, Self::column_value(column, value));
                Ok(format!("{} = {}", col, placeholder))
            }
            UpdateValue::UnsafeRaw(expression) => Ok(format!("{} = {}", col, expression)),
            UpdateValue::Increment(by) => {
                let placeholder =
                    push_param(db_type, params, crate::internal::Value::BigInt(Some(*by)));
                Ok(format!("{} = {} + {}", col, col, placeholder))
            }
            UpdateValue::Decrement(by) => {
                let placeholder =
                    push_param(db_type, params, crate::internal::Value::BigInt(Some(*by)));
                Ok(format!("{} = {} - {}", col, col, placeholder))
            }
            UpdateValue::Multiply(by) => Ok(format!(
                "{col} = {}",
                Self::scaled(column, &col, db_type, params, *by, false)
            )),
            UpdateValue::Divide(by) => Ok(format!(
                "{col} = {}",
                Self::scaled(column, &col, db_type, params, *by, true)
            )),
            UpdateValue::ArrayAppend(value) => Ok(match db_type {
                crate::config::DatabaseType::Postgres => {
                    let placeholder =
                        push_param(db_type, params, crate::internal::json_to_db_value(value));
                    format!("{} = array_append({}, {})", col, col, placeholder)
                }
                // The element is bound as its JSON text and read back as JSON,
                // so an object stays an object rather than becoming a string,
                // and a NULL column starts a new array.
                crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                    let placeholder = push_param(
                        db_type,
                        params,
                        crate::query::db_sql::json_scalar_parameter(value),
                    );
                    format!(
                        "{} = JSON_ARRAY_APPEND(COALESCE({}, JSON_ARRAY()), '$', JSON_EXTRACT({}, '$'))",
                        col, col, placeholder
                    )
                }
                crate::config::DatabaseType::SQLite => {
                    let placeholder = push_param(
                        db_type,
                        params,
                        crate::query::db_sql::json_scalar_parameter(value),
                    );
                    format!(
                        "{} = json_insert(COALESCE({}, '[]'), '$[#]', json({}))",
                        col, col, placeholder
                    )
                }
            }),
            UpdateValue::ArrayRemove(value) => Ok(match db_type {
                crate::config::DatabaseType::Postgres => {
                    let placeholder =
                        push_param(db_type, params, crate::internal::json_to_db_value(value));
                    format!("{} = array_remove({}, {})", col, col, placeholder)
                }
                // Every element equal to the value goes, compared as JSON as
                // `where_eq` compares a document (`1`, `"1"` and `true` differ,
                // an object's key order does not, an array's order does), and
                // the rest keep their order and types; a NULL column stays
                // NULL. JSON_TABLE reads a JSON null as SQL NULL on MySQL,
                // hence the extra test when null is what is removed.
                crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                    let equal = crate::query::db_sql::json_equals_bound(
                        db_type,
                        "tideorm_element.v",
                        value,
                        false,
                    );
                    params.extend(equal.values);
                    let null_guard = if value.is_null() {
                        "tideorm_element.v IS NOT NULL AND "
                    } else {
                        ""
                    };
                    format!(
                        "{col} = CASE WHEN {col} IS NULL THEN NULL ELSE (SELECT COALESCE(JSON_ARRAYAGG(JSON_EXTRACT(tideorm_element.v, '$')), JSON_ARRAY()) FROM JSON_TABLE({col}, '$[*]' COLUMNS (v JSON PATH '$')) AS tideorm_element WHERE {null_guard}NOT COALESCE({}, FALSE)) END",
                        equal.sql
                    )
                }
                // Each kept element is rebuilt as the JSON it was: `json_each`
                // reports true and false as 1 and 0 and null as NULL.
                crate::config::DatabaseType::SQLite => {
                    let equal =
                        crate::query::db_sql::sqlite_json_element_equals("tideorm_element", value);
                    params.extend(equal.values);
                    let element = "CASE tideorm_element.type WHEN 'true' THEN json('true') WHEN 'false' THEN json('false') WHEN 'null' THEN json('null') WHEN 'object' THEN json(tideorm_element.value) WHEN 'array' THEN json(tideorm_element.value) ELSE json_quote(tideorm_element.value) END";
                    format!(
                        "{col} = CASE WHEN {col} IS NULL THEN NULL ELSE (SELECT json_group_array({element} ORDER BY tideorm_element.key) FROM json_each({col}) AS tideorm_element WHERE NOT {}) END",
                        equal.sql
                    )
                }
            }),
            UpdateValue::JsonSet(path, value) => {
                let steps = crate::query::db_sql::parse_json_path(path)
                    .filter(|steps| !steps.is_empty())
                    .ok_or_else(|| {
                        Error::query(format!(
                            "unsupported JSON path '{}': json_set takes `$` followed by `.key`, `.\"key\"`, `['key']` or `[index]` steps",
                            path
                        ))
                    })?;
                let bound_path = match db_type {
                    crate::config::DatabaseType::Postgres => {
                        crate::query::db_sql::postgres_json_path_array(&steps)
                    }
                    crate::config::DatabaseType::MySQL
                    | crate::config::DatabaseType::MariaDB
                    | crate::config::DatabaseType::SQLite => {
                        crate::query::db_sql::json_path_text(&steps)
                    }
                };
                let path_placeholder = push_param(
                    db_type,
                    params,
                    crate::internal::Value::String(Some(bound_path)),
                );
                let value_placeholder = push_param(
                    db_type,
                    params,
                    crate::query::db_sql::json_scalar_parameter(value),
                );

                Ok(match db_type {
                    crate::config::DatabaseType::Postgres => format!(
                        "{} = jsonb_set({}, {}::text[], CAST({} AS jsonb))",
                        col,
                        crate::query::db_sql::postgres_jsonb(&col),
                        path_placeholder,
                        value_placeholder
                    ),
                    // `JSON_EXTRACT(?, '$')` reads the text as JSON on both
                    // servers; MariaDB has no `CAST(.. AS JSON)`.
                    crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                        format!(
                            "{} = JSON_SET({}, {}, JSON_EXTRACT({}, '$'))",
                            col, col, path_placeholder, value_placeholder
                        )
                    }
                    crate::config::DatabaseType::SQLite => {
                        format!(
                            "{} = json_set({}, {}, json({}))",
                            col, col, path_placeholder, value_placeholder
                        )
                    }
                })
            }
            UpdateValue::Coalesce(default) => {
                let placeholder = push_param(db_type, params, Self::column_value(column, default));
                Ok(format!("{} = COALESCE({}, {})", col, col, placeholder))
            }
        }
    }

    fn build_set_clause_with_params_for_db(
        &self,
        db_type: crate::config::DatabaseType,
    ) -> Result<(Vec<String>, Vec<crate::internal::Value>)> {
        let mut params = Vec::new();
        let mut set_parts = Vec::with_capacity(self.updates.len());

        // `updates` is a HashMap, so its iteration order varies run to run.
        // Render assignments in column order instead: identical logical updates
        // then produce byte-identical SQL, which keeps server-side prepared
        // statement caches and slow-query fingerprints usable. Parameters are
        // pushed in the same pass, so they stay aligned with the placeholders.
        let mut ordered_updates: Vec<(&String, &UpdateValue)> = self.updates.iter().collect();
        ordered_updates.sort_unstable_by_key(|(column, _)| *column);

        for (column, value) in ordered_updates {
            let value = crate::model::__prepare_batch_update_value::<M>(column, value.clone())?;
            set_parts.push(Self::build_assignment_sql(
                column,
                &value,
                db_type,
                &mut params,
            )?);
        }

        Ok((set_parts, params))
    }

    pub(crate) fn ensure_backend_supports_returning(
        db_type: crate::config::DatabaseType,
    ) -> Result<()> {
        if !db_type.supports_returning() {
            return Err(Error::backend_not_supported(
                format!(
                    "execute_returning() is not supported on {}: it needs UPDATE .. RETURNING",
                    db_type
                ),
                db_type.to_string(),
            ));
        }

        Ok(())
    }

    fn build_where_query(&self) -> QueryBuilder<M> {
        // Live rows, as a query reads them, unless the update chose otherwise;
        // an update started from a query keeps that query's scope.
        let mut query = self.base.clone().unwrap_or_default();
        match self.include_trashed {
            Some(true) => query = query.with_trashed(),
            Some(false) => query = query.live_rows_only(),
            None => {}
        }

        query
            .clauses
            .conditions
            .extend(self.conditions.iter().cloned());
        // The update's `or_where_*` calls join the query's own OR group.
        for condition in &self.or_group.conditions {
            query = query.push_or_condition(condition.clone());
        }

        query
    }

    /// Report a value a setter refused, such as a NaN float; its assignment
    /// was never staged, so the update would otherwise run without it.
    fn ensure_values_are_bindable(&self) -> Result<()> {
        match &self.invalid_reason {
            Some(reason) => Err(Error::query(format!(
                "update of {}: {}",
                M::table_name(),
                reason
            ))),
            None => Ok(()),
        }
    }

    /// Refuse the parts of a query an `UPDATE` cannot keep: it takes the
    /// query's filters, scope and database, and nothing that reshapes rows.
    fn ensure_base_is_updatable(&self) -> Result<()> {
        match self.base.as_ref().and_then(QueryBuilder::update_blocker) {
            Some(part) => Err(Error::query(format!(
                "update_all() of a {} query keeps its filters and scope only; {} cannot be part of an UPDATE",
                M::table_name(),
                part
            ))),
            None => Ok(()),
        }
    }

    /// The database the update runs on: the one its query named, else the
    /// scope's.
    fn database(&self) -> Result<crate::database::Database> {
        self.base
            .as_ref()
            .map_or_else(crate::database::__current_db, QueryBuilder::current_db)
    }

    /// A batch update's filters must pass the query builder's validation, and
    /// at least one of them must be able to exclude a row; these are the same
    /// guards the `QueryBuilder` mutation terminals use.
    pub(crate) fn ensure_explicit_filters(&self, operation: &str) -> Result<()> {
        self.ensure_base_is_updatable()?;
        let query = self.build_where_query();
        query.ensure_query_is_executable()?;
        query.ensure_mutation_has_explicit_filters(operation)
    }

    /// Whether the backend accepts `LIMIT` directly on an `UPDATE` statement.
    fn backend_supports_update_limit(db_type: crate::config::DatabaseType) -> bool {
        matches!(
            db_type,
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB
        )
    }

    /// Quoted primary-key column used to cap an `UPDATE` on backends that
    /// cannot take `LIMIT` directly.
    fn limit_scope_primary_key(db_type: crate::config::DatabaseType) -> Result<String> {
        match M::primary_key_names() {
            [column] => Ok(quote_ident(db_type, column)),
            columns => Err(Error::query(format!(
                "limit() is not supported for '{}' on {}: that backend cannot cap an UPDATE \
                 directly, so the row limit has to be scoped through a primary-key subquery, \
                 which needs a single primary key column (found {})",
                M::table_name(),
                db_type,
                columns.len()
            ))),
        }
    }

    /// Render the `UPDATE` statement (without any `RETURNING` clause) and its
    /// parameters for `db_type`.
    fn build_update_statement(
        &self,
        db_type: crate::config::DatabaseType,
    ) -> Result<(String, Vec<crate::internal::Value>)> {
        let (set_parts, mut params) = self.build_set_clause_with_params_for_db(db_type)?;

        let query = self.build_where_query();
        let (where_sql, where_params) = query.build_where_clause_with_condition_for_db(db_type);
        let where_sql =
            crate::query::db_sql::rebase_placeholders(db_type, &where_sql, params.len());
        params.extend(where_params);

        let table = crate::query::db_sql::quote_table::<M>(db_type);
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        match self.limit_value {
            // Postgres and SQLite reject `UPDATE .. LIMIT`, so the cap is
            // enforced by scoping the update to a bounded primary-key subquery.
            // `limit()` is a blast-radius control; dropping it on those
            // backends would widen exactly what the caller asked to contain.
            Some(limit) if !Self::backend_supports_update_limit(db_type) => {
                let primary_key = Self::limit_scope_primary_key(db_type)?;
                sql.push_str(&format!(
                    " WHERE {} IN (SELECT {} FROM {}",
                    primary_key, primary_key, table
                ));
                if !where_sql.is_empty() {
                    sql.push_str(" WHERE ");
                    sql.push_str(&where_sql);
                }
                // `limit` is a `u64`, so it can only ever render as digits.
                sql.push_str(&format!(" LIMIT {})", limit));
            }
            limit => {
                if !where_sql.is_empty() {
                    sql.push_str(" WHERE ");
                    sql.push_str(&where_sql);
                }
                if let Some(limit) = limit {
                    sql.push_str(&format!(" LIMIT {}", limit));
                }
            }
        }

        Ok((sql, params))
    }

    /// Run the update and report how many rows it changed.
    ///
    /// This is a single `UPDATE` statement: model callbacks, validations, and
    /// automatic timestamp columns are **not** applied, and no rows are loaded.
    /// Use it for bulk maintenance writes; use `model.save()` when the model's
    /// own lifecycle matters.
    ///
    /// Returns `Ok(0)` without touching the database when no assignment was
    /// staged, and errors when the builder carries no explicit filter. Use
    /// [`execute_returning`](Self::execute_returning) for the rows.
    ///
    /// On success the query cache for this table is invalidated.
    pub async fn execute(self) -> Result<u64> {
        self.ensure_values_are_bindable()?;
        if self.updates.is_empty() {
            return Ok(0);
        }

        self.ensure_explicit_filters("update")?;

        // Resolve the dialect from the very handle that will run the statement.
        // `require_db()` only ever sees the global connection, so a batch update
        // inside `some_db.transaction(..)` with no global connection used to
        // fail before it rendered any SQL — and could pick the wrong dialect
        // when the scoped handle spoke a different backend.
        let db = self.database()?;
        let db_type = db.execution_backend();

        let (sql, params) = self.build_update_statement(db_type)?;

        let rows_affected = db.__execute_with_params(&sql, params).await?;
        QueryBuilder::<M>::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    /// Run the update and return the rows it wrote.
    ///
    /// Appends `RETURNING`, so it needs a backend whose `UPDATE` takes one:
    /// PostgreSQL and SQLite. MySQL has no `RETURNING`, and MariaDB has
    /// `UPDATE .. RETURNING` only from 13.0, so both are refused with an error
    /// before anything runs, whatever the server's version.
    ///
    /// Returns an empty vector without touching the database when no assignment
    /// was staged, and errors when the builder carries no explicit filter. Like
    /// [`execute`](Self::execute), no callbacks or validations run.
    ///
    /// The update and the decoding of the rows it returns are not one unit: if
    /// a row does not decode into the model, the error comes back but the rows
    /// are already written, and retrying repeats a non-idempotent change such
    /// as `increment`. Run it inside `Database::transaction` when that matters.
    pub async fn execute_returning(self) -> Result<Vec<M>> {
        self.ensure_values_are_bindable()?;
        if self.updates.is_empty() {
            return Ok(vec![]);
        }

        self.ensure_explicit_filters("update")?;

        let db = self.database()?;
        let db_type = db.execution_backend();
        Self::ensure_backend_supports_returning(db_type)?;
        // The rows are written before they are decoded, as with `update()`.
        crate::internal::ensure_fields_storable::<M, _>(&db.__get_connection()?.executor())?;

        let (mut sql, params) = self.build_update_statement(db_type)?;
        sql.push_str(" RETURNING ");
        sql.push_str(&crate::query::db_sql::model_columns_sql::<M>(db_type, None));

        let models = db.__raw_with_params::<M>(&sql, params).await;
        // A row that fails to decode was still written, so the table's cached
        // reads are dropped whether or not the decoding succeeds.
        QueryBuilder::<M>::invalidate_model_state(
            models.as_ref().map_or(1, |models| models.len() as u64),
        );
        let models = models?;
        #[cfg(feature = "dirty-tracking")]
        crate::model::__remember_dirty_snapshots(&models);
        Ok(models)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/model_batch_sql_execution_tests.rs"]
mod tests;
