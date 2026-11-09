//! The value each column of a batch update is set to.

use super::*;

impl<M: Model> BatchUpdateBuilder<M> {
    /// Bind `value` with the type of the column it is assigned to, as a filter
    /// value is bound, refusing a value the column cannot hold: SQLite stores
    /// whatever it is given, and the model could then no longer read the row.
    /// A JSON column takes the value's document.
    fn column_value(
        column: &str,
        column_type: Option<&ColumnType>,
        value: &Value,
        db_type: DatabaseType,
    ) -> Result<Value> {
        if matches!(column_type, Some(ColumnType::Json | ColumnType::JsonBinary)) {
            return Ok(match crate::query::is_sql_null(value) {
                true => Value::Json(None),
                false => Value::from(crate::internal::value_document(value)),
            });
        }
        let bound = crate::internal::column_value(value, column_type);
        if crate::query::is_sql_null(&bound) {
            return Ok(bound);
        }
        let refuse = |problem: &str| -> Result<Value> {
            Err(Error::query(format!("value for {column} {problem}")))
        };
        let document = crate::internal::value_document(&bound);
        match column_type {
            Some(ColumnType::Boolean) if !matches!(bound, Value::Bool(_)) => {
                refuse("must be a boolean")
            }
            Some(ColumnType::Uuid) if !matches!(bound, Value::Uuid(_)) => refuse("must be a UUID"),
            Some(ColumnType::Blob | ColumnType::Binary(_) | ColumnType::VarBinary(_))
                if !matches!(bound, Value::Bytes(_)) =>
            {
                refuse("must be bytes")
            }
            Some(kind @ (ColumnType::Float | ColumnType::Double)) => {
                let single = matches!(kind, ColumnType::Float);
                let (maximum, name) = match single {
                    true => (f64::from(f32::MAX), "f32"),
                    false => (f64::MAX, "f64"),
                };
                let number = document.as_f64().filter(|n| n.abs() <= maximum);
                match number {
                    None => refuse(&format!("must be within the finite {name} range")),
                    // Match ordinary model writes, including on SQLite, whose
                    // REAL storage otherwise retains f64 precision.
                    Some(number) if single => Ok(Value::Float(Some(number as f32))),
                    Some(_) => Ok(bound),
                }
            }
            Some(kind) if db_type == DatabaseType::SQLite && crate::internal::is_integer(kind) => {
                // SQLite affinity would store a large number as REAL, or keep
                // invalid text, where the model expects an integer.
                let integer = document.as_i64().or_else(|| {
                    document
                        .as_f64()
                        .filter(|n| n.fract() == 0.0)
                        .and_then(|n| i64::try_from(n as i128).ok())
                });
                let (min, max) = Self::sqlite_integer_bounds(column, column_type);
                match integer.is_some_and(|integer| (min..=max).contains(&integer)) {
                    true => Ok(bound),
                    false => refuse("is outside the model's integer range or is not an integer"),
                }
            }
            Some(ColumnType::Decimal(_) | ColumnType::Money(_)) => match &bound {
                Value::Decimal(Some(decimal))
                    if db_type != DatabaseType::SQLite
                        || crate::internal::sqlite_decimal_is_storable(*decimal) =>
                {
                    Ok(bound)
                }
                _ => refuse("must be a valid decimal within its readable model range"),
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
                    | Value::ChronoDateTimeLocal(_)
                    | Value::ChronoDateTimeWithTimeZone(_)
            ) =>
            {
                refuse("must be a valid date, time or timestamp for its column type")
            }
            _ => Ok(bound),
        }
    }

    /// The value `column` is set to.
    fn assigned_value(column: &str, value: &UpdateValue, db_type: DatabaseType) -> Result<Expr> {
        let col = Self::quote_update_column(column, db_type)?;
        let kind = crate::internal::column_type_of::<M>(column);
        let kind = kind.as_ref();
        Self::ensure_setter_fits_column(column, value, db_type, kind)?;

        let assigned = match value {
            UpdateValue::Value(value) => {
                return Ok(Expr::val(Self::column_value(column, kind, value, db_type)?));
            }
            UpdateValue::UnsafeRaw(expression) => return Ok(Expr::cust(expression.clone())),
            UpdateValue::Increment(by) => Self::stepped(column, kind, &col, db_type, "+", *by),
            UpdateValue::Decrement(by) => Self::stepped(column, kind, &col, db_type, "-", *by),
            UpdateValue::Multiply(by) => Self::scaled(column, kind, &col, db_type, *by, false)?,
            UpdateValue::Divide(by) => Self::scaled(column, kind, &col, db_type, *by, true)?,
            UpdateValue::ArrayAppend(value) => {
                let element = Self::array_element(value, kind, db_type);
                db_type.dialect().array_append(&col, element)
            }
            UpdateValue::ArrayRemove(value) => {
                let source = format!("{}.{col}", db_sql::quote_ident(db_type, M::table_name()));
                let element = Self::array_element(value, kind, db_type);
                db_type.dialect().array_remove(&col, &source, element)
            }
            UpdateValue::JsonSet(path, value) => {
                let document = crate::internal::value_document(value);
                db_type
                    .dialect()
                    .json_set(&col, &Self::json_set_path(path)?, &document)
            }
            UpdateValue::Coalesce(default) => {
                let mut coalesced = BoundSql::sql(format!("COALESCE({col}, "));
                coalesced
                    .push(Self::column_value(column, kind, default, db_type)?)
                    .push(")");
                coalesced
            }
        };
        Ok(assigned.into())
    }

    /// Refuse a computed setter the column cannot take: the structured ones
    /// need a JSON column, or a PostgreSQL array for the array setters, and
    /// arithmetic a numeric one. `kind` is the column's type.
    fn ensure_setter_fits_column(
        column: &str,
        value: &UpdateValue,
        db_type: DatabaseType,
        kind: Option<&ColumnType>,
    ) -> Result<()> {
        match value {
            UpdateValue::ArrayAppend(_)
            | UpdateValue::ArrayRemove(_)
            | UpdateValue::JsonSet(..) => {
                let supported = matches!(
                    (value, db_type, kind),
                    (
                        UpdateValue::JsonSet(..),
                        _,
                        Some(ColumnType::Json | ColumnType::JsonBinary),
                    ) | (
                        UpdateValue::ArrayAppend(_) | UpdateValue::ArrayRemove(_),
                        DatabaseType::Postgres,
                        Some(ColumnType::Array(_)),
                    ) | (
                        _,
                        DatabaseType::SQLite | DatabaseType::MySQL | DatabaseType::MariaDB,
                        Some(ColumnType::Json | ColumnType::JsonBinary),
                    )
                );
                if !supported {
                    return Err(Error::query(format!(
                        "structured setter requires a compatible JSON or array column; {column} is not supported"
                    )));
                }
            }
            UpdateValue::Increment(_)
            | UpdateValue::Decrement(_)
            | UpdateValue::Multiply(_)
            | UpdateValue::Divide(_) => {
                let numeric = kind.is_some_and(|kind| {
                    crate::internal::is_integer(kind)
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
            _ => {}
        }
        Ok(())
    }

    /// `value` as an element of the array column of type `kind`: of its
    /// element type where arrays are the database's own, else a JSON value.
    fn array_element(value: &Value, kind: Option<&ColumnType>, db_type: DatabaseType) -> Value {
        if db_type.dialect().native_arrays() {
            crate::internal::array_element(value, kind)
        } else {
            Value::from(crate::internal::value_document(value))
        }
    }

    /// The steps of a `json_set` path, which names at least one.
    fn json_set_path(path: &str) -> Result<Vec<crate::orm::sql_builder::JsonPathStep>> {
        crate::query::db_sql::parse_json_path(path)
            .filter(|steps| !steps.is_empty())
            .ok_or_else(|| {
                Error::query(format!(
                    "unsupported JSON path '{}': json_set takes `$` followed by `.key`, `.\"key\"`, `['key']` or `[index]` steps",
                    path
                ))
            })
    }

    /// Add the update's assignments to `update`.
    pub(super) fn add_assignments(
        &self,
        update: &mut UpdateStatement,
        db_type: DatabaseType,
    ) -> Result<()> {
        // `updates` is a HashMap, so its iteration order varies run to run.
        // Render assignments in column order instead: identical logical updates
        // then produce byte-identical SQL, which keeps server-side prepared
        // statement caches and slow-query fingerprints usable.
        let mut ordered_updates: Vec<(&String, &UpdateValue)> = self.updates.iter().collect();
        ordered_updates.sort_unstable_by_key(|(column, _)| *column);

        for (column, value) in ordered_updates {
            let value = crate::model::prepare_batch_update_value::<M>(column, value.clone())?;
            let assigned = Self::assigned_value(column, &value, db_type)?;
            update.value(M::column_named(column).to_owned(), assigned);
        }
        Ok(())
    }
}
