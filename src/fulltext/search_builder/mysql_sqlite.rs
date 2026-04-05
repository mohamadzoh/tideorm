use super::*;

impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_mysql_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::MySQL, T::table_name());
        let mut params = Vec::new();

        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = match self.config.mode {
            SearchMode::Natural => "",
            SearchMode::Boolean => " IN BOOLEAN MODE",
            SearchMode::Phrase => " WITH QUERY EXPANSION",
            _ => "",
        };

        let query_placeholder = crate::internal::push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        let mut sql = format!(
            "SELECT * FROM {} WHERE MATCH({}) AGAINST({}{}) ",
            table, columns_str, query_placeholder, mode_modifier
        );

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_mysql_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::MySQL, T::table_name());
        let mut params = Vec::new();

        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = match self.config.mode {
            SearchMode::Natural => "",
            SearchMode::Boolean => " IN BOOLEAN MODE",
            SearchMode::Phrase => " WITH QUERY EXPANSION",
            _ => "",
        };

        let rank_placeholder = crate::internal::push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        let where_placeholder = crate::internal::push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        let mut sql = format!(
            "SELECT *, MATCH({}) AGAINST({}{}) AS _fts_rank FROM {} \
             WHERE MATCH({}) AGAINST({}{}) ",
            columns_str,
            rank_placeholder,
            mode_modifier,
            table,
            columns_str,
            where_placeholder,
            mode_modifier
        );

        if let Some(min_rank) = self.min_rank {
            let min_rank_placeholder = crate::internal::push_param(
                DatabaseType::MySQL,
                &mut params,
                Value::Double(Some(min_rank)),
            );
            let against_placeholder = crate::internal::push_param(
                DatabaseType::MySQL,
                &mut params,
                Value::String(Some(self.query.clone())),
            );
            sql.push_str(&format!(
                "AND MATCH({}) AGAINST({}{}) >= {} ",
                columns_str, against_placeholder, mode_modifier, min_rank_placeholder
            ));
        }

        sql.push_str("ORDER BY _fts_rank DESC ");

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_mysql_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::MySQL, T::table_name());
        let mut params = Vec::new();

        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = match self.config.mode {
            SearchMode::Natural => "",
            SearchMode::Boolean => " IN BOOLEAN MODE",
            _ => "",
        };

        let query_placeholder = crate::internal::push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        Ok((
            format!(
                "SELECT COUNT(*) as count FROM {} WHERE MATCH({}) AGAINST({}{})",
                table, columns_str, query_placeholder, mode_modifier
            ),
            params,
        ))
    }

    // =========================================================================
    // SQLITE IMPLEMENTATION (FTS5)
    // =========================================================================

    pub(super) fn build_sqlite_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let table = quote_ident(DatabaseType::SQLite, table_name);
        let fts_table_name = format!("{}_fts", table_name);
        let fts_table = quote_ident(DatabaseType::SQLite, &fts_table_name);
        let mut params = Vec::new();
        let query_placeholder = crate::internal::push_param(
            DatabaseType::SQLite,
            &mut params,
            Value::String(Some(escape_fts5_query(&self.query))),
        );

        // SQLite FTS5 requires a separate virtual table
        // This assumes the FTS5 table exists with the same columns
        let mut sql = format!(
            "SELECT t.* FROM {} t \
             INNER JOIN {} fts ON t.rowid = fts.rowid \
             WHERE {} MATCH {} ",
            table, fts_table, fts_table, query_placeholder
        );

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_sqlite_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let table = quote_ident(DatabaseType::SQLite, table_name);
        let fts_table_name = format!("{}_fts", table_name);
        let fts_table = quote_ident(DatabaseType::SQLite, &fts_table_name);
        let mut params = Vec::new();
        let query_placeholder = crate::internal::push_param(
            DatabaseType::SQLite,
            &mut params,
            Value::String(Some(escape_fts5_query(&self.query))),
        );

        let mut sql = format!(
            "SELECT t.*, bm25({}) AS _fts_rank FROM {} t \
             INNER JOIN {} fts ON t.rowid = fts.rowid \
             WHERE {} MATCH {} ",
            fts_table, table, fts_table, fts_table, query_placeholder
        );

        if let Some(min_rank) = self.min_rank {
            // Note: BM25 returns negative values, lower is better
            let min_rank_placeholder = crate::internal::push_param(
                DatabaseType::SQLite,
                &mut params,
                Value::Double(Some(-min_rank)),
            );
            sql.push_str(&format!(
                "AND bm25({}) <= {} ",
                fts_table, min_rank_placeholder
            ));
        }

        // BM25 returns negative values, so ORDER BY ASC for best matches
        sql.push_str(&format!("ORDER BY bm25({}) ", fts_table));

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_sqlite_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let table = quote_ident(DatabaseType::SQLite, table_name);
        let fts_table_name = format!("{}_fts", table_name);
        let fts_table = quote_ident(DatabaseType::SQLite, &fts_table_name);
        let mut params = Vec::new();
        let query_placeholder = crate::internal::push_param(
            DatabaseType::SQLite,
            &mut params,
            Value::String(Some(escape_fts5_query(&self.query))),
        );

        Ok((
            format!(
                "SELECT COUNT(*) as count FROM {} t \
                 INNER JOIN {} fts ON t.rowid = fts.rowid \
                 WHERE {} MATCH {}",
                table, fts_table, fts_table, query_placeholder
            ),
            params,
        ))
    }

    pub(super) fn pg_weights_placeholder(&self, params: &mut Vec<Value>) -> String {
        let weights = self
            .config
            .weights
            .as_ref()
            .map(|w| w.to_pg_array().trim_matches('\'').to_string())
            .unwrap_or_else(|| "{0.1,0.2,0.4,1.0}".to_string());

        crate::internal::push_param(DatabaseType::Postgres, params, Value::String(Some(weights)))
    }

    pub(super) fn append_limit_offset(
        &self,
        db_type: DatabaseType,
        sql: &mut String,
        params: &mut Vec<Value>,
    ) -> Result<()> {
        if let Some(limit) = self.limit {
            let limit_value = i64::try_from(limit)
                .map_err(|_| Error::query("Full-text search limit exceeds i64 range"))?;
            let placeholder =
                crate::internal::push_param(db_type, params, Value::BigInt(Some(limit_value)));
            sql.push_str(&format!(" LIMIT {}", placeholder));
        }
        if let Some(offset) = self.offset {
            let offset_value = i64::try_from(offset)
                .map_err(|_| Error::query("Full-text search offset exceeds i64 range"))?;
            let placeholder =
                crate::internal::push_param(db_type, params, Value::BigInt(Some(offset_value)));
            sql.push_str(&format!(" OFFSET {}", placeholder));
        }
        Ok(())
    }
}
