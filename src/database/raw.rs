use crate::error::{Error, Result};

use super::{Connection, ConnectionRef, Database};

impl Database {
    /// Check whether the current database connection is responsive.
    pub async fn ping(&self) -> Result<()> {
        use crate::internal::ConnectionTrait;

        match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().execute_unprepared("SELECT 1"))
                    .await
            }
            ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().execute_unprepared("SELECT 1")).await
            }
        }
        .map_err(|e| Error::connection(e.to_string()))?;

        Ok(())
    }

    /// Execute a raw SQL query and return all results
    pub async fn raw<T: crate::model::Model>(sql: &str) -> Result<Vec<T>> {
        use crate::internal::{ConnectionTrait, FromQueryResult, Statement};

        let backend = crate::database::__current_backend()?;
        let stmt = Statement::from_string(backend, sql.to_string());

        let results = match crate::database::__current_connection()? {
            ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        let mut models = Vec::new();
        for row in results {
            let model =
                <T::Entity as crate::internal::EntityTrait>::Model::from_query_result(&row, "")
                    .map_err(|e| Error::query(e.to_string()))?;
            models.push(T::from_sea_model(model));
        }

        Ok(models)
    }

    /// Execute a raw SQL query with parameters
    pub async fn raw_with_params<T: crate::model::Model>(
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<Vec<T>> {
        crate::database::__current_db()?
            .__raw_with_params::<T>(sql, params)
            .await
    }

    #[doc(hidden)]
    pub async fn __raw_with_params<T: crate::model::Model>(
        &self,
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<Vec<T>> {
        use crate::internal::{ConnectionTrait, FromQueryResult, Statement};

        let results = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = Statement::from_sql_and_values(
                    conn.connection().get_database_backend(),
                    sql,
                    params,
                );
                crate::profiling::__profile_future(conn.connection().query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt =
                    Statement::from_sql_and_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        let mut models = Vec::new();
        for row in results {
            let model =
                <T::Entity as crate::internal::EntityTrait>::Model::from_query_result(&row, "")
                    .map_err(|e| Error::query(e.to_string()))?;
            models.push(T::from_sea_model(model));
        }

        Ok(models)
    }

    /// Execute a raw SQL statement (INSERT, UPDATE, DELETE) and return rows affected
    pub async fn execute(sql: &str) -> Result<u64> {
        use crate::internal::ConnectionTrait;

        let result = match crate::database::__current_connection()? {
            ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().execute_unprepared(sql)).await
            }
            ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().execute_unprepared(sql)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Execute a raw SQL statement with parameters
    pub async fn execute_with_params(
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<u64> {
        crate::database::__current_db()?
            .__execute_with_params(sql, params)
            .await
    }

    #[doc(hidden)]
    pub async fn __execute_with_params(
        &self,
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<u64> {
        use crate::internal::{ConnectionTrait, Statement};

        let result = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = Statement::from_sql_and_values(
                    conn.connection().get_database_backend(),
                    sql,
                    params,
                );
                crate::profiling::__profile_future(conn.connection().execute_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt =
                    Statement::from_sql_and_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().execute_raw(stmt)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Execute a raw SQL query and return results as JSON
    pub async fn raw_json(sql: &str) -> Result<Vec<serde_json::Value>> {
        use crate::internal::{ConnectionTrait, Statement};

        let backend = crate::database::__current_backend()?;
        let stmt = Statement::from_string(backend, sql.to_string());

        let results = match crate::database::__current_connection()? {
            ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        Self::query_rows_to_json(results)
    }

    /// Execute a raw SQL query with parameters and return results as JSON
    pub async fn raw_json_with_params(
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        crate::database::__current_db()?
            .__raw_json_with_params(sql, params)
            .await
    }

    #[doc(hidden)]
    pub async fn __raw_json_with_params(
        &self,
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        use crate::internal::{ConnectionTrait, Statement};

        let results = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = Statement::from_sql_and_values(
                    conn.connection().get_database_backend(),
                    sql,
                    params,
                );
                crate::profiling::__profile_future(conn.connection().query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt =
                    Statement::from_sql_and_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        Self::query_rows_to_json(results)
    }

    fn query_rows_to_json(
        results: Vec<crate::internal::QueryResult>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut json_results = Vec::new();
        for row in results {
            json_results.push(Self::query_row_to_json(&row));
        }

        Ok(json_results)
    }

    fn query_row_to_json(row: &crate::internal::QueryResult) -> serde_json::Value {
        #[cfg(feature = "postgres")]
        if let Some(pg_row) = row.try_as_pg_row() {
            return Self::sqlx_row_to_json(
                row,
                pg_row,
                |result, index, type_name| match type_name {
                    "BOOL" => Self::typed_or_fallback::<bool>(result, index),
                    "INT2" => Self::typed_or_fallback::<i16>(result, index),
                    "INT4" => Self::typed_or_fallback::<i32>(result, index),
                    "INT8" => Self::typed_or_fallback::<i64>(result, index),
                    "FLOAT4" => Self::typed_or_fallback::<f32>(result, index),
                    "FLOAT8" => Self::typed_or_fallback::<f64>(result, index),
                    "NUMERIC" => Self::decimal_or_fallback(result, index),
                    "UUID" => Self::typed_or_fallback::<uuid::Uuid>(result, index),
                    "JSON" | "JSONB" => Self::typed_or_fallback::<serde_json::Value>(result, index),
                    "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(result, index),
                    "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(result, index),
                    "TIMESTAMP" => Self::typed_or_fallback::<chrono::NaiveDateTime>(result, index),
                    "TIMESTAMPTZ" => {
                        Self::typed_or_fallback::<chrono::DateTime<chrono::FixedOffset>>(
                            result, index,
                        )
                    }
                    _ => Self::fallback_try_get_json(result, index),
                },
            );
        }

        #[cfg(feature = "mysql")]
        if let Some(mysql_row) = row.try_as_mysql_row() {
            return Self::sqlx_row_to_json(
                row,
                mysql_row,
                |result, index, type_name| match type_name {
                    "BOOLEAN" | "BOOL" => Self::typed_or_fallback::<bool>(result, index),
                    "TINYINT" => Self::typed_or_fallback::<i8>(result, index),
                    "SMALLINT" => Self::typed_or_fallback::<i16>(result, index),
                    "INT" | "INTEGER" | "MEDIUMINT" => {
                        Self::typed_or_fallback::<i32>(result, index)
                    }
                    "BIGINT" => Self::typed_or_fallback::<i64>(result, index),
                    "FLOAT" => Self::typed_or_fallback::<f32>(result, index),
                    "DOUBLE" => Self::typed_or_fallback::<f64>(result, index),
                    "DECIMAL" | "NUMERIC" => Self::decimal_or_fallback(result, index),
                    "JSON" => Self::typed_or_fallback::<serde_json::Value>(result, index),
                    "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(result, index),
                    "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(result, index),
                    "DATETIME" | "TIMESTAMP" => {
                        Self::typed_or_fallback::<chrono::NaiveDateTime>(result, index)
                    }
                    _ => Self::fallback_try_get_json(result, index),
                },
            );
        }

        #[cfg(feature = "sqlite")]
        if let Some(sqlite_row) = row.try_as_sqlite_row() {
            return Self::sqlx_row_to_json(row, sqlite_row, |result, index, type_name| {
                match type_name {
                    "BOOLEAN" | "BOOL" => Self::typed_or_fallback::<bool>(result, index),
                    "INTEGER" | "INT" => Self::typed_or_fallback::<i64>(result, index),
                    "REAL" | "FLOAT" | "DOUBLE" => Self::typed_or_fallback::<f64>(result, index),
                    "NUMERIC" | "DECIMAL" => Self::decimal_or_fallback(result, index),
                    "JSON" => Self::typed_or_fallback::<serde_json::Value>(result, index),
                    "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(result, index),
                    "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(result, index),
                    "DATETIME" | "TIMESTAMP" => {
                        Self::typed_or_fallback::<chrono::NaiveDateTime>(result, index)
                    }
                    "TEXT" => Self::typed_or_fallback::<String>(result, index),
                    "BLOB" => Self::typed_or_fallback::<Vec<u8>>(result, index),
                    _ => Self::sqlite_unknown_type_or_fallback(result, index),
                }
            });
        }

        let mut obj = serde_json::Map::new();
        for (index, col_name) in row.column_names().into_iter().enumerate() {
            obj.insert(col_name, Self::fallback_try_get_json(row, index));
        }

        serde_json::Value::Object(obj)
    }

    fn fallback_try_get_json(
        row: &crate::internal::QueryResult,
        index: usize,
    ) -> serde_json::Value {
        Self::try_get_json::<serde_json::Value>(row, index)
            .or_else(|| Self::try_get_json::<uuid::Uuid>(row, index))
            .or_else(|| Self::try_get_decimal_json(row, index))
            .or_else(|| Self::try_get_json::<chrono::DateTime<chrono::FixedOffset>>(row, index))
            .or_else(|| Self::try_get_json::<chrono::DateTime<chrono::Utc>>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveDateTime>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveDate>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveTime>(row, index))
            .or_else(|| Self::try_get_json::<i64>(row, index))
            .or_else(|| Self::try_get_json::<u64>(row, index))
            .or_else(|| Self::try_get_json::<f64>(row, index))
            .or_else(|| Self::try_get_json::<bool>(row, index))
            .or_else(|| Self::try_get_json::<String>(row, index))
            .unwrap_or(serde_json::Value::Null)
    }

    fn typed_or_fallback<T>(row: &crate::internal::QueryResult, index: usize) -> serde_json::Value
    where
        T: crate::internal::TryGetable + serde::Serialize,
    {
        Self::try_get_json::<T>(row, index)
            .unwrap_or_else(|| Self::fallback_try_get_json(row, index))
    }

    fn decimal_or_fallback(row: &crate::internal::QueryResult, index: usize) -> serde_json::Value {
        Self::try_get_decimal_json(row, index)
            .unwrap_or_else(|| Self::fallback_try_get_json(row, index))
    }

    #[cfg(feature = "sqlite")]
    fn sqlite_unknown_type_or_fallback(
        row: &crate::internal::QueryResult,
        index: usize,
    ) -> serde_json::Value {
        let value = Self::fallback_try_get_json(row, index);

        if let serde_json::Value::String(text) = &value {
            if let Ok(integer) = text.parse::<i64>() {
                return serde_json::json!(integer);
            }

            if let Ok(unsigned) = text.parse::<u64>() {
                return serde_json::json!(unsigned);
            }
        }

        value
    }

    fn try_get_decimal_json(
        row: &crate::internal::QueryResult,
        index: usize,
    ) -> Option<serde_json::Value> {
        if let Some(value) = Self::try_get_json::<rust_decimal::Decimal>(row, index) {
            return Some(value);
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<String>>(index) {
            return rust_decimal::Decimal::from_str_exact(&value)
                .ok()
                .and_then(|decimal| serde_json::to_value(decimal).ok())
                .or(Some(serde_json::Value::String(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<i64>>(index) {
            return serde_json::to_value(rust_decimal::Decimal::from(value))
                .ok()
                .or(Some(serde_json::json!(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<u64>>(index) {
            return serde_json::to_value(rust_decimal::Decimal::from(value))
                .ok()
                .or(Some(serde_json::json!(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<f64>>(index) {
            let value_text = value.to_string();
            return rust_decimal::Decimal::from_str_exact(&value_text)
                .ok()
                .and_then(|decimal| serde_json::to_value(decimal).ok())
                .or(Some(serde_json::json!(value)));
        }

        None
    }

    fn try_get_json<T>(
        row: &crate::internal::QueryResult,
        index: usize,
    ) -> Option<serde_json::Value>
    where
        T: crate::internal::TryGetable + serde::Serialize,
    {
        row.try_get_by_index::<Option<T>>(index)
            .ok()
            .map(Self::option_to_json)
    }

    fn option_to_json<T>(value: Option<T>) -> serde_json::Value
    where
        T: serde::Serialize,
    {
        match value {
            Some(value) => serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        }
    }

    fn sqlx_row_to_json<R, F>(
        result: &crate::internal::QueryResult,
        row: &R,
        decoder_for_type: F,
    ) -> serde_json::Value
    where
        R: sea_orm::sqlx::Row,
        F: Fn(&crate::internal::QueryResult, usize, &str) -> serde_json::Value,
    {
        use sea_orm::sqlx::{Column, TypeInfo};

        let mut obj = serde_json::Map::new();
        for (index, column) in row.columns().iter().enumerate() {
            let type_name = column.type_info().name().to_ascii_uppercase();
            obj.insert(
                column.name().to_string(),
                decoder_for_type(result, index, type_name.as_str()),
            );
        }

        serde_json::Value::Object(obj)
    }
}
