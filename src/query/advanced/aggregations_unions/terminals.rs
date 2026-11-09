//! Aggregate terminals, and the checks that keep comparisons off encrypted
//! columns.

use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Run a scalar aggregate and return the value of its single `alias`
    /// column; `None` when the backend returned no such column.
    ///
    /// The query is validated like every other terminal, so a condition that
    /// cannot be rendered is rejected instead of silently widening the rows the
    /// aggregate covers.
    ///
    /// `reads_as_column` is set for a `MIN`/`MAX`, whose value is one of the
    /// column's and is decoded by its type; a sum or a count is a number
    /// whatever it adds up, so a `SUM` of a boolean is not read as one.
    pub(super) async fn execute_scalar_aggregate(
        &self,
        terminal: &str,
        column: &str,
        alias: &str,
        reads_as_column: bool,
        render_expression: impl Fn(&str) -> String,
    ) -> Result<Option<serde_json::Value>> {
        self.ensure_query_is_executable()?;
        self.ensure_scalar_aggregate_is_representable(terminal)?;

        let (sql, params) = self.build_aggregate_sql_with_params_for_db(
            self.db_type_for_sql()?,
            column,
            alias,
            render_expression,
        )?;
        let (output_types, sources) =
            self.extreme_outputs(reads_as_column.then_some((alias, column)));
        let rows = self
            .fetch_aggregate_json(&sql, params, &output_types, &sources)
            .await?;

        Ok(rows.first().and_then(|row| row.get(alias)).cloned())
    }

    /// Sum a column, read as `T`: the sum of no rows is zero.
    ///
    /// `T` is whatever the sum should be read as, usually inferred: an integer
    /// type for an integer column, `Decimal` for an exact total of a decimal
    /// column, `f64` for an approximate one. The sum is never read through a
    /// float first, so an integer total past 2^53 and a decimal total stay
    /// exact.
    ///
    /// ```ignore
    /// let revenue: Decimal = Order::query().where_eq("paid", true).sum("total").await?;
    /// ```
    ///
    /// Joins, CTEs, unions, and `limit()`/`offset()` are honoured; `group_by()`,
    /// `having()`, and `window()` are rejected because a grouped aggregate has no
    /// single scalar answer.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).sum::<i64>(User::ID).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn sum<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<T> {
        let column = Self::own_column(&column);
        self.ensure_encrypted_column_is_comparable(&column, "SUM")?;
        self.decoded_aggregate(
            "sum()",
            &column,
            AGGREGATE_RESULT_ALIAS,
            false,
            sum_expression,
        )
        .await
    }

    /// Average a column, read as `T` (usually `f64`); `None` over no rows.
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).avg::<f64>(User::ID).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn avg<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        let column = Self::own_column(&column);
        self.ensure_encrypted_column_is_comparable(&column, "AVG")?;
        self.decoded_aggregate("avg()", &column, AGGREGATE_RESULT_ALIAS, false, |column| {
            format!("AVG({})", column)
        })
        .await
    }

    /// The smallest value of a column, read as `T`; `None` over no rows.
    ///
    /// Any column type works — a number, a text, a date — and `T` is read the
    /// way the model reads that column:
    ///
    /// ```ignore
    /// let first_signup: Option<DateTime<Utc>> = User::query().min("created_at").await?;
    /// ```
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).min::<i64>(User::ID).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn min<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        self.extreme("MIN", &Self::own_column(&column)).await
    }

    /// The largest value of a column, read as `T`; `None` over no rows.
    ///
    /// Works like [`min()`](Self::min).
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).max::<i64>(User::ID).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn max<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        self.extreme("MAX", &Self::own_column(&column)).await
    }

    /// Run a `MIN` or `MAX`, aliased as the model column it reads so the
    /// result is decoded as the model decodes that column.
    ///
    /// `function` is one of the two hardcoded names above, never caller input.
    pub(super) async fn extreme<T: serde::de::DeserializeOwned>(
        &self,
        function: &str,
        column: &str,
    ) -> Result<Option<T>> {
        self.ensure_encrypted_column_is_comparable(column, function)?;
        let terminal = format!("{}()", function.to_ascii_lowercase());
        let alias = Self::extreme_alias(column).unwrap_or(AGGREGATE_RESULT_ALIAS);
        self.decoded_aggregate(&terminal, column, alias, true, |column| {
            format!("{}({})", function, column)
        })
        .await
    }

    /// Resolve the column actually read by an aggregate. DISTINCT and UNION
    /// preserve projections, so their aliases take precedence over model fields.
    pub(in crate::query) fn aggregate_source(&self, column: &str) -> Option<(String, String)> {
        if self.rows_fixed_by_projection() {
            let output = self.derived_output_name(column)?;
            if let Some((_, source)) = self
                .projection_outputs()
                .into_iter()
                .find(|(name, _)| *name == output)
            {
                return source;
            }
        }
        if let Some(column) = M::own_column_name(column) {
            return Some((M::table_name().to_string(), column.to_string()));
        }
        column
            .rsplit_once('.')
            .map(|(table, field)| (table.to_string(), field.to_string()))
    }

    pub(in crate::query) fn ensure_encrypted_column_is_comparable(
        &self,
        column: &str,
        operation: &str,
    ) -> Result<()> {
        // Derived queries read output aliases; ordinary aggregates read source columns.
        let output = if self.rows_fixed_by_projection() {
            self.derived_output_name(column)
        } else if self.aggregate_source(column).is_none() {
            Some(column.to_string())
        } else {
            None
        };
        if let Some(output) = output
            && let Some((name, source)) = self
                .projection_outputs()
                .into_iter()
                .find(|(name, _)| *name == output)
        {
            return Self::reject_encrypted_source(
                self.projected_encrypted_column(&name, source.as_ref()),
                operation,
            );
        }
        if let Some((table, field)) = self.aggregate_source(column) {
            self.ensure_encrypted_source_is_comparable(&table, &field, operation)?;
        }
        Ok(())
    }

    /// GROUP BY and HAVING read inputs before DISTINCT is applied. A model
    /// column takes precedence over an identically named output alias; aliases
    /// remain usable when they do not shadow an input column.
    pub(in crate::query) fn ensure_encrypted_input_is_comparable(
        &self,
        column: &str,
        operation: &str,
    ) -> Result<()> {
        let (table, column) = Self::simple_column_reference(column).unwrap_or(("", column));
        if !table.is_empty() {
            return self.ensure_encrypted_source_is_comparable(table, column, operation);
        }
        if let Some(column) = M::own_column_name(column) {
            return self.ensure_encrypted_source_is_comparable(M::table_name(), column, operation);
        }
        // An unqualified joined input also takes precedence over an output alias.
        let mut joined_input = false;
        for join in &self.clauses.joins {
            if self.joined_column_names(join).contains(&column) {
                joined_input = true;
                self.ensure_encrypted_source_is_comparable(join.qualifier(), column, operation)?;
            }
        }
        if joined_input {
            return Ok(());
        }
        if let Some((name, source)) = self
            .projection_outputs()
            .into_iter()
            .find(|(name, _)| name == column)
        {
            Self::reject_encrypted_source(
                self.projected_encrypted_column(&name, source.as_ref()),
                operation,
            )?;
        }
        Ok(())
    }

    pub(in crate::query) fn ensure_encrypted_source_is_comparable(
        &self,
        table: &str,
        field: &str,
        operation: &str,
    ) -> Result<()> {
        Self::reject_encrypted_source(self.encrypted_source_column(table, field), operation)
    }

    pub(in crate::query) fn reject_encrypted_source(
        source: Option<crate::model::EncryptedColumn>,
        operation: &str,
    ) -> Result<()> {
        match source {
            Some(source) => Err(source.refuse(
                operation,
                "randomized ciphertext cannot compare plaintext values; read the values and \
                 compare them after decryption",
            )),
            None => Ok(()),
        }
    }

    pub(super) fn aggregate_column_type(&self, column: &str) -> Option<crate::orm::ColumnType> {
        if self.rows_fixed_by_projection()
            && let Some(output) = self.derived_output_name(column)
            && let Some(column_type) = self.projected_column_type(&output)
        {
            return Some(column_type);
        }
        let (table, field) = self.aggregate_source(column)?;
        self.column_type(&format!("{table}.{field}"))
    }

    /// How the `MIN`/`MAX` outputs in `extremes`, each an alias and the column
    /// it reads, are read back: decoded by the column's type, the model's or
    /// a joined model's, so a second `MAX` of a timestamp, under an alias of
    /// its own, still reads as one, and decrypted as that column.
    #[allow(clippy::type_complexity)]
    pub(super) fn extreme_outputs<'a>(
        &self,
        extremes: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> (Vec<(String, crate::orm::ColumnType)>, Vec<(String, String)>) {
        let mut output_types = Vec::new();
        let mut sources = Vec::new();
        for (alias, column) in extremes {
            if let Some(column_type) = self.aggregate_column_type(column) {
                output_types.push((alias.to_string(), column_type));
            }
            sources.push((alias.to_string(), column.to_string()));
        }
        (output_types, sources)
    }

    /// The name a `MIN`/`MAX` over `column` is selected as: the model's own
    /// column name when it is one of the model's columns, so the value is
    /// decoded by that column's type.
    pub(super) fn extreme_alias(column: &str) -> Option<&'static str> {
        crate::internal::column_type_of::<M>(column)?;
        M::own_column_name(column)
    }

    /// Run a scalar aggregate selected as `alias` and decode its value as `T`.
    pub(super) async fn decoded_aggregate<T: serde::de::DeserializeOwned>(
        &self,
        terminal: &str,
        column: &str,
        alias: &str,
        reads_as_column: bool,
        render_expression: impl Fn(&str) -> String,
    ) -> Result<T> {
        let value = self
            .execute_scalar_aggregate(terminal, column, alias, reads_as_column, render_expression)
            .await?
            .ok_or_else(|| {
                Error::query(format!(
                    "Database returned no '{}' column for {}",
                    alias, terminal
                ))
            })?;
        let retry = number_from_text(&value);
        decode_aggregate(value, retry, terminal)
    }

    /// Count distinct values of a column
    ///
    /// Encrypted model columns are rejected because randomized ciphertext
    /// cannot count distinct plaintext values.
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).count_distinct(User::NAME).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn count_distinct(self, column: impl crate::columns::IntoColumnName) -> Result<u64> {
        let column = Self::own_column(&column);
        self.ensure_encrypted_column_is_comparable(&column, "DISTINCT")?;
        let value = self
            .execute_scalar_aggregate(
                "count_distinct()",
                &column,
                COUNT_RESULT_ALIAS,
                false,
                |column_sql| format!("COUNT(DISTINCT {})", column_sql),
            )
            .await?;

        Self::decode_count_value(value.as_ref(), COUNT_RESULT_ALIAS)
    }

    /// Compute several aggregates over the same rows in one statement.
    ///
    /// The results are read into `T` in the order of `aggregates`: a tuple
    /// with one type per aggregate, or a `Vec`. Each value is read as the
    /// matching single-aggregate terminal reads it, so a `SUM` over no rows is
    /// zero and an `AVG`, `MIN` or `MAX` over no rows is NULL, which needs an
    /// `Option`. Carries the same modifier rules as [`sum()`](Self::sum), so
    /// unlike [`count()`](Self::count), [`Aggregate::Count`] counts only the
    /// rows `limit()` and `offset()` leave.
    ///
    /// ```ignore
    /// let (orders, revenue, largest): (u64, Decimal, Option<Decimal>) = Sale::query()
    ///     .where_eq("region", "EU")
    ///     .aggregates(&[Aggregate::count(), Aggregate::sum("amount"), Aggregate::max("amount")])
    ///     .await?;
    /// ```
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).aggregates::<serde_json::Value>(&[Aggregate::count()]).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn aggregates<T: serde::de::DeserializeOwned>(
        self,
        aggregates: &[Aggregate],
    ) -> Result<T> {
        if aggregates.is_empty() {
            return decode_aggregate(serde_json::Value::Array(Vec::new()), None, "aggregates()");
        }
        self.ensure_query_is_executable()?;
        self.ensure_scalar_aggregate_is_representable("aggregates()")?;
        for (column, operation) in aggregates.iter().filter_map(Aggregate::compared_column) {
            self.ensure_encrypted_column_is_comparable(column, operation)?;
        }

        let db_type = self.db_type_for_sql()?;
        // A MIN or MAX is selected as its model column's name, the first time
        // that name comes up, so it is decoded as the model decodes the column.
        let mut aliases: Vec<String> = Vec::with_capacity(aggregates.len());
        for (index, aggregate) in aggregates.iter().enumerate() {
            let base = aggregate
                .extreme_column()
                .and_then(Self::extreme_alias)
                .filter(|name| !aliases.iter().any(|alias| alias == name))
                .map_or_else(|| format!("aggregate_{index}"), str::to_string);
            let alias =
                db_sql::unused_name(&base, |name| aliases.iter().any(|alias| alias == name));
            aliases.push(alias);
        }
        let (sql, params) = self.build_projected_aggregate_sql(db_type, |format_column| {
            aggregates
                .iter()
                .zip(&aliases)
                .map(|(aggregate, alias)| {
                    format!(
                        "{} AS {}",
                        aggregate.render(format_column),
                        db_sql::quote_ident(db_type, alias)
                    )
                })
                .collect()
        })?;
        // Every MIN or MAX reads as its column.
        let (output_types, sources) =
            self.extreme_outputs(aggregates.iter().zip(&aliases).filter_map(
                |(aggregate, alias)| {
                    aggregate
                        .extreme_column()
                        .map(|column| (alias.as_str(), column))
                },
            ));
        let rows = self
            .fetch_aggregate_json(&sql, params, &output_types, &sources)
            .await?;
        let row = rows.first();

        let values = aliases
            .iter()
            .map(|alias| {
                row.and_then(|row| row.get(alias.as_str()))
                    .cloned()
                    .ok_or_else(|| {
                        Error::query(format!(
                            "Database returned no '{}' column for aggregates()",
                            alias
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        // The MIN or MAX of a text column is text, even when it spells a number.
        let retry = aliases
            .iter()
            .zip(&values)
            .map(|(alias, value)| {
                let reads_numbers = !matches!(
                    output_types
                        .iter()
                        .find(|(output, _)| output == alias)
                        .map(|(_, column_type)| column_type),
                    Some(
                        crate::orm::ColumnType::String(_)
                            | crate::orm::ColumnType::Text
                            | crate::orm::ColumnType::Char(_)
                            | crate::orm::ColumnType::Enum { .. }
                    )
                );
                reads_numbers
                    .then(|| number_from_text(value))
                    .flatten()
                    .unwrap_or_else(|| value.clone())
            })
            .collect();
        decode_aggregate(
            serde_json::Value::Array(values),
            Some(serde_json::Value::Array(retry)),
            "aggregates()",
        )
    }
}
