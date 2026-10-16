use super::*;

use crate::internal::push_param;

/// An SQLite expression that fails with "integer overflow" when evaluated. A
/// `CASE` that reaches it aborts the whole UPDATE, rows already changed
/// included, which is how bulk arithmetic refuses a result the model cannot read.
const SQLITE_ABORT: &str = "abs(-9223372036854775808)";

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

/// Round a finite, in-range SQLite REAL to binary32, using ties-to-even.
/// Eight binary-search steps find its spacing (never below 2^-149), so no
/// SQLite math extension or per-connection function registration is required.
fn sqlite_f32_round(expression: &str) -> String {
    let factors = (0..8)
        .map(|step| format!("WHEN {step} THEN {:e}", 2_f64.powi(128 >> step)))
        .collect::<Vec<_>>()
        .join(" ");
    let factor = format!("(CASE step {factors} END)");
    let minimum = 2_f64.powi(-149);
    format!(
        "(WITH RECURSIVE input(value) AS (SELECT {expression}), \
         spacing(step, quantum) AS (VALUES(0, {minimum:e}) UNION ALL \
         SELECT step + 1, CASE WHEN abs(value) >= quantum * {factor} * 8388608.0 \
         THEN quantum * {factor} ELSE quantum END FROM spacing, input WHERE step < 8), \
         scaled(value, quantum, units) AS \
         (SELECT value, quantum, abs(value) / quantum FROM input, spacing WHERE step = 8), \
         parts(value, quantum, units, whole) AS \
         (SELECT value, quantum, units, CAST(units AS INTEGER) FROM scaled) \
         SELECT CASE WHEN value IS NULL THEN NULL ELSE \
         (CASE WHEN value < 0 THEN -1.0 ELSE 1.0 END) * quantum * \
         (whole + CASE WHEN units - whole > 0.5 OR \
         (units - whole = 0.5 AND whole % 2 = 1) THEN 1 ELSE 0 END) END FROM parts)"
    )
}

impl<M: Model> BatchUpdateBuilder<M> {
    /// SQLite has one integer storage width; enforce the model's narrower type too.
    fn sqlite_integer_bounds(column: &str) -> (i64, i64) {
        use crate::orm::ColumnType;
        if let Some(bounds) = M::__sqlite_integer_bounds(column) {
            return bounds;
        }
        match crate::internal::column_type_of::<M>(column) {
            Some(ColumnType::TinyInteger) => (i8::MIN.into(), i8::MAX.into()),
            Some(ColumnType::SmallInteger) => (i16::MIN.into(), i16::MAX.into()),
            Some(ColumnType::Integer) => (i32::MIN.into(), i32::MAX.into()),
            Some(ColumnType::TinyUnsigned) => (0, u8::MAX.into()),
            Some(ColumnType::SmallUnsigned) => (0, u16::MAX.into()),
            Some(ColumnType::Unsigned) => (0, u32::MAX.into()),
            Some(ColumnType::BigUnsigned) => (0, i64::MAX),
            _ => (i64::MIN, i64::MAX),
        }
    }

    /// Bind `value` with the type of the column it is assigned to, refusing a
    /// value the column cannot hold: SQLite stores whatever it is given, and
    /// the model could then no longer read the row.
    fn column_value(
        column: &str,
        value: &serde_json::Value,
        db_type: crate::config::DatabaseType,
    ) -> Result<crate::internal::Value> {
        use crate::config::DatabaseType;
        use crate::internal::Value;
        use crate::orm::ColumnType;

        let refuse = |problem: &str| -> Result<Value> {
            Err(Error::query(format!("value for {column} {problem}")))
        };
        let column_type = crate::internal::column_type_of::<M>(column);
        if !value.is_null() {
            match column_type.as_ref() {
                Some(ColumnType::Boolean) if !value.is_boolean() => {
                    return refuse("must be a boolean");
                }
                Some(ColumnType::Uuid)
                    if value
                        .as_str()
                        .and_then(|s| uuid::Uuid::parse_str(s).ok())
                        .is_none() =>
                {
                    return refuse("must be a UUID");
                }
                Some(ColumnType::Blob | ColumnType::Binary(_) | ColumnType::VarBinary(_))
                    if serde_json::from_value::<Vec<u8>>(value.clone()).is_err() =>
                {
                    return refuse("must be an array of bytes in 0..=255");
                }
                Some(kind @ (ColumnType::Float | ColumnType::Double)) => {
                    let number = value
                        .as_f64()
                        .or_else(|| value.as_str()?.trim().parse::<f64>().ok());
                    let single = matches!(kind, ColumnType::Float);
                    let (maximum, name) = match single {
                        true => (f64::from(f32::MAX), "f32"),
                        false => (f64::MAX, "f64"),
                    };
                    if !number.is_some_and(|n| n.is_finite() && n.abs() <= maximum) {
                        return refuse(&format!("must be within the finite {name} range"));
                    }
                    if single {
                        // Match ordinary model writes, including on SQLite,
                        // whose REAL storage otherwise retains f64 precision.
                        return Ok(Value::Float(number.map(|n| n as f32)));
                    }
                }
                Some(kind)
                    if db_type == DatabaseType::SQLite && crate::internal::is_integer(kind) =>
                {
                    // SQLite affinity would store a large number as REAL, or
                    // keep invalid text, where the model expects an integer.
                    let integer = match value {
                        serde_json::Value::String(text) => text.trim().parse::<i64>().ok(),
                        _ => value.as_i64().or_else(|| {
                            value
                                .as_f64()
                                .filter(|n| n.is_finite() && n.fract() == 0.0)
                                .and_then(|n| i64::try_from(n as i128).ok())
                        }),
                    };
                    let (min, max) = Self::sqlite_integer_bounds(column);
                    if !integer.is_some_and(|integer| (min..=max).contains(&integer)) {
                        return refuse("is outside the model's integer range or is not an integer");
                    }
                }
                _ => {}
            }
        }
        if matches!(column_type, Some(ColumnType::Json | ColumnType::JsonBinary)) {
            return Ok(Value::from(value.clone()));
        }
        let bound = crate::internal::json_to_assignment_value(value, column_type.as_ref());
        match column_type {
            Some(ColumnType::Decimal(_) | ColumnType::Money(_)) => match &bound {
                Value::Decimal(None) => {}
                Value::Decimal(Some(decimal))
                    if db_type != DatabaseType::SQLite
                        || crate::internal::sqlite_decimal_is_storable(*decimal) => {}
                _ => return refuse("must be a valid decimal within its readable model range"),
            },
            // The binding parsers decide which formats a temporal column takes,
            // as for filters; their fallback to text is refused here.
            Some(
                ColumnType::Date
                | ColumnType::Time
                | ColumnType::DateTime
                | ColumnType::Timestamp
                | ColumnType::TimestampWithTimeZone,
            ) if !matches!(
                bound,
                Value::ChronoDate(_)
                    | Value::ChronoTime(_)
                    | Value::ChronoDateTime(_)
                    | Value::ChronoDateTimeUtc(_)
            ) =>
            {
                return refuse("must be a valid date, time or timestamp for its column type");
            }
            _ => {}
        }
        Ok(bound)
    }

    /// Keep SQLite arithmetic within the model's readable Float/Double/Decimal range.
    /// SQLite otherwise accepts infinity (or a value too wide for f32), which
    /// cannot round-trip through JSON and typed reads. The error aborts the
    /// entire statement, including rows already updated; NULL stays NULL.
    fn sqlite_float_arithmetic(
        column: &str,
        col: &str,
        operator: &str,
        operand: crate::internal::Value,
        params: &mut Vec<crate::internal::Value>,
    ) -> Option<String> {
        let maximum = match crate::internal::column_type_of::<M>(column) {
            Some(crate::orm::ColumnType::Float) => f64::from(f32::MAX),
            Some(crate::orm::ColumnType::Double) => f64::MAX,
            Some(crate::orm::ColumnType::Decimal(_) | crate::orm::ColumnType::Money(_)) => {
                crate::internal::SQLITE_DECIMAL_MAX
            }
            _ => return None,
        };
        let db_type = crate::config::DatabaseType::SQLite;
        let bounded = push_param(db_type, params, operand.clone());
        let result = push_param(db_type, params, operand);
        let expression = format!(
            "CASE WHEN {col} IS NULL OR ({col} {operator} {bounded}) BETWEEN -{maximum:e} AND {maximum:e} THEN {col} {operator} {result} ELSE {SQLITE_ABORT} END"
        );
        Some(if maximum == f64::from(f32::MAX) {
            sqlite_f32_round(&expression)
        } else {
            expression
        })
    }

    /// `col` multiplied, or with `divide` divided, by `factor`.
    ///
    /// An integer column is scaled exactly and rounded half away from zero,
    /// since an `f64` holds integers exactly only up to 2^53: PostgreSQL,
    /// MySQL and MariaDB take the factor as a decimal, and SQLite, which has
    /// no decimal arithmetic, as a fraction of two integers. SQLite would
    /// otherwise store the REAL product, after which the model cannot read
    /// the row. A decimal column takes the factor as a decimal too, where the
    /// backend has one, and floating-point columns take an `f64`.
    fn scaled(
        column: &str,
        col: &str,
        db_type: crate::config::DatabaseType,
        params: &mut Vec<crate::internal::Value>,
        factor: f64,
        divide: bool,
    ) -> Result<String> {
        use crate::config::DatabaseType;
        use crate::orm::ColumnType;

        if !factor.is_finite() || (divide && factor == 0.0) {
            return Err(Error::query(
                "bulk arithmetic requires a finite factor and a nonzero divisor",
            ));
        }
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
                    // SQLite promotes overflowing integer arithmetic to REAL.
                    // Casting that REAL clamps it silently. Fail the statement
                    // instead; abs(i64::MIN) raises SQLite's integer-overflow
                    // error and rolls back every row of this UPDATE.
                    let bounded = sqlite_integer_scale(col, exact, divide, db_type, params)
                        .expect("the same exact factor was already accepted");
                    let (min, max) = Self::sqlite_integer_bounds(column);
                    let result = sqlite_integer_scale(col, exact, divide, db_type, params)
                        .expect("the same exact factor was already accepted");
                    return Ok(format!(
                        "CASE WHEN typeof({scaled}) IN ('integer', 'null') AND COALESCE(({bounded}) BETWEEN {min} AND {max}, 1) THEN {result} ELSE {SQLITE_ABORT} END"
                    ));
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
                return Ok(format!("{col} {operator} {placeholder}"));
            }
            _ => {}
        }

        if integral && db_type == DatabaseType::SQLite {
            return Err(Error::query(
                "SQLite cannot scale this integer column exactly with the requested factor",
            ));
        }
        if db_type == DatabaseType::SQLite
            && let Some(expression) = Self::sqlite_float_arithmetic(
                column,
                col,
                operator,
                crate::internal::Value::Double(Some(factor)),
                params,
            )
        {
            return Ok(expression);
        }
        let placeholder = push_param(
            db_type,
            params,
            crate::internal::Value::Double(Some(factor)),
        );
        let scaled = format!("{col} {operator} {placeholder}");
        Ok(scaled)
    }

    fn build_assignment_sql(
        column: &str,
        value: &UpdateValue,
        db_type: crate::config::DatabaseType,
        params: &mut Vec<crate::internal::Value>,
    ) -> Result<String> {
        let col = Self::quote_update_column(column, db_type)?;

        if matches!(
            value,
            UpdateValue::ArrayAppend(_) | UpdateValue::ArrayRemove(_) | UpdateValue::JsonSet(..)
        ) {
            let kind = crate::internal::column_type_of::<M>(column);
            let supported = matches!(
                (value, db_type, kind.as_ref()),
                (
                    UpdateValue::JsonSet(..),
                    _,
                    Some(crate::orm::ColumnType::Json | crate::orm::ColumnType::JsonBinary),
                ) | (
                    UpdateValue::ArrayAppend(_) | UpdateValue::ArrayRemove(_),
                    crate::config::DatabaseType::Postgres,
                    Some(crate::orm::ColumnType::Array(_)),
                ) | (
                    _,
                    crate::config::DatabaseType::SQLite
                        | crate::config::DatabaseType::MySQL
                        | crate::config::DatabaseType::MariaDB,
                    Some(crate::orm::ColumnType::Json | crate::orm::ColumnType::JsonBinary),
                )
            );
            if !supported {
                return Err(Error::query(format!(
                    "structured setter requires a compatible JSON or array column; {column} is not supported"
                )));
            }
        }

        if matches!(
            value,
            UpdateValue::Increment(_)
                | UpdateValue::Decrement(_)
                | UpdateValue::Multiply(_)
                | UpdateValue::Divide(_)
        ) {
            use crate::orm::ColumnType;
            let numeric = crate::internal::column_type_of::<M>(column).is_some_and(|kind| {
                crate::internal::is_integer(&kind)
                    || matches!(
                        kind,
                        ColumnType::Float
                            | ColumnType::Double
                            | ColumnType::Decimal(_)
                            | ColumnType::Money(_)
                    )
            });
            if !numeric {
                return Err(Error::query(format!(
                    "bulk arithmetic requires a numeric column; {column} is not numeric"
                )));
            }
        }

        match value {
            UpdateValue::SqlNull | UpdateValue::CoalesceSqlNull => {
                let null = crate::internal::json_to_column_value(
                    &serde_json::Value::Null,
                    crate::internal::column_type_of::<M>(column).as_ref(),
                );
                let parameter = push_param(db_type, params, null);
                Ok(if matches!(value, UpdateValue::SqlNull) {
                    format!("{col} = {parameter}")
                } else {
                    format!("{col} = COALESCE({col}, {parameter})")
                })
            }
            UpdateValue::Value(value) => {
                let placeholder =
                    push_param(db_type, params, Self::column_value(column, value, db_type)?);
                Ok(format!("{} = {}", col, placeholder))
            }
            UpdateValue::UnsafeRaw(expression) => Ok(format!("{} = {}", col, expression)),
            UpdateValue::Increment(by) | UpdateValue::Decrement(by) => {
                let operator = if matches!(value, UpdateValue::Increment(_)) {
                    "+"
                } else {
                    "-"
                };
                if db_type == crate::config::DatabaseType::SQLite
                    && let Some(expression) = Self::sqlite_float_arithmetic(
                        column,
                        &col,
                        operator,
                        crate::internal::Value::BigInt(Some(*by)),
                        params,
                    )
                {
                    return Ok(format!("{col} = {expression}"));
                }
                let placeholder =
                    push_param(db_type, params, crate::internal::Value::BigInt(Some(*by)));
                let expression = format!("{col} {operator} {placeholder}");
                if db_type == crate::config::DatabaseType::SQLite
                    && crate::internal::column_type_of::<M>(column)
                        .as_ref()
                        .is_some_and(crate::internal::is_integer)
                {
                    let bounded_parameter =
                        push_param(db_type, params, crate::internal::Value::BigInt(Some(*by)));
                    let (min, max) = Self::sqlite_integer_bounds(column);
                    let result_parameter =
                        push_param(db_type, params, crate::internal::Value::BigInt(Some(*by)));
                    Ok(format!(
                        "{col} = CASE WHEN typeof({expression}) IN ('integer', 'null') AND COALESCE(({col} {operator} {bounded_parameter}) BETWEEN {min} AND {max}, 1) THEN {col} {operator} {result_parameter} ELSE {SQLITE_ABORT} END"
                    ))
                } else {
                    Ok(format!("{col} = {expression}"))
                }
            }
            UpdateValue::Multiply(by) => Ok(format!(
                "{col} = {}",
                Self::scaled(column, &col, db_type, params, *by, false)?
            )),
            UpdateValue::Divide(by) => Ok(format!(
                "{col} = {}",
                Self::scaled(column, &col, db_type, params, *by, true)?
            )),
            UpdateValue::ArrayAppend(value) => Ok(match db_type {
                crate::config::DatabaseType::Postgres => {
                    let placeholder = push_param(
                        db_type,
                        params,
                        crate::internal::json_to_array_element(
                            value,
                            crate::internal::column_type_of::<M>(column).as_ref(),
                        ),
                    );
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
                    let placeholder = push_param(
                        db_type,
                        params,
                        crate::internal::json_to_array_element(
                            value,
                            crate::internal::column_type_of::<M>(column).as_ref(),
                        ),
                    );
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
                // Extract JSON text, not json_each.value: SQL REAL rounds large
                // integers and rebuilding from it would corrupt untouched values.
                crate::config::DatabaseType::SQLite => {
                    let equal =
                        crate::query::db_sql::sqlite_json_element_equals("tideorm_element", value);
                    params.extend(equal.values);
                    let element = "json(tideorm_element.json -> tideorm_element.fullkey)";
                    let source = format!(
                        "{}.{}",
                        crate::query::db_sql::quote_ident(db_type, M::table_name()),
                        col
                    );
                    format!(
                        "{col} = CASE WHEN {col} IS NULL THEN NULL ELSE (SELECT json_group_array({element} ORDER BY tideorm_element.key) FROM json_each({source}) AS tideorm_element WHERE NOT {}) END",
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
                let placeholder = push_param(
                    db_type,
                    params,
                    Self::column_value(column, default, db_type)?,
                );
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

        let result = db.__execute_with_params(&sql, params).await;
        QueryBuilder::<M>::invalidate_model_state(result.as_ref().copied().unwrap_or(1));
        result
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
