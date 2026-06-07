use super::*;
use crate::internal::sql_builder::SqlBuilder;

impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_mysql_sql(&self) -> Result<(String, Vec<Value>)> {
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

        let mut params = Vec::new();
        let mut sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT * FROM ")
            .ident(T::table_name())
            .raw(" WHERE MATCH(")
            .raw(&columns_str)
            .raw(") AGAINST(")
            .param(Value::String(Some(self.query.clone())))
            .raw(mode_modifier)
            .raw(") ")
            .into_sql();

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_mysql_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
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

        let mut params = Vec::new();
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

        let mut sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT *, MATCH(")
            .raw(&columns_str)
            .raw(") AGAINST(")
            .placeholder(&rank_placeholder)
            .raw(mode_modifier)
            .raw(") AS _fts_rank FROM ")
            .ident(T::table_name())
            .raw(" WHERE MATCH(")
            .raw(&columns_str)
            .raw(") AGAINST(")
            .placeholder(&where_placeholder)
            .raw(mode_modifier)
            .raw(") ")
            .into_sql();

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
            sql.push_str("AND MATCH(");
            sql.push_str(&columns_str);
            sql.push_str(") AGAINST(");
            sql.push_str(&against_placeholder);
            sql.push_str(mode_modifier);
            sql.push_str(") >= ");
            sql.push_str(&min_rank_placeholder);
            sql.push(' ');
        }

        sql.push_str("ORDER BY _fts_rank DESC ");

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_mysql_count_sql(&self) -> Result<(String, Vec<Value>)> {
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

        let mut params = Vec::new();
        let sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT COUNT(*) as count FROM ")
            .ident(T::table_name())
            .raw(" WHERE MATCH(")
            .raw(&columns_str)
            .raw(") AGAINST(")
            .param(Value::String(Some(self.query.clone())))
            .raw(mode_modifier)
            .raw(")")
            .into_sql();

        Ok((sql, params))
    }

    // =========================================================================
    // SQLITE IMPLEMENTATION (FTS5)
    // =========================================================================

    pub(super) fn build_sqlite_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let fts_table_name = format!("{}_fts", table_name);

        let mut params = Vec::new();
        let mut sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT t.* FROM ")
            .ident(table_name)
            .raw(" t INNER JOIN ")
            .ident(&fts_table_name)
            .raw(" fts ON t.rowid = fts.rowid WHERE ")
            .ident(&fts_table_name)
            .raw(" MATCH ")
            .param(Value::String(Some(escape_fts5_query(&self.query))))
            .raw(" ")
            .into_sql();

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_sqlite_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let fts_table_name = format!("{}_fts", table_name);

        let mut params = Vec::new();
        let mut sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT t.*, bm25(")
            .ident(&fts_table_name)
            .raw(") AS _fts_rank FROM ")
            .ident(table_name)
            .raw(" t INNER JOIN ")
            .ident(&fts_table_name)
            .raw(" fts ON t.rowid = fts.rowid WHERE ")
            .ident(&fts_table_name)
            .raw(" MATCH ")
            .param(Value::String(Some(escape_fts5_query(&self.query))))
            .raw(" ")
            .into_sql();

        if let Some(min_rank) = self.min_rank {
            let min_rank_placeholder = crate::internal::push_param(
                DatabaseType::SQLite,
                &mut params,
                Value::Double(Some(-min_rank)),
            );
            sql.push_str("AND bm25(");
            sql.push_str(&quote_ident(DatabaseType::SQLite, &fts_table_name));
            sql.push_str(") <= ");
            sql.push_str(&min_rank_placeholder);
            sql.push(' ');
        }

        sql.push_str("ORDER BY bm25(");
        sql.push_str(&quote_ident(DatabaseType::SQLite, &fts_table_name));
        sql.push_str(") ");

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_sqlite_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let fts_table_name = format!("{}_fts", table_name);

        let mut params = Vec::new();
        let sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT COUNT(*) as count FROM ")
            .ident(table_name)
            .raw(" t INNER JOIN ")
            .ident(&fts_table_name)
            .raw(" fts ON t.rowid = fts.rowid WHERE ")
            .ident(&fts_table_name)
            .raw(" MATCH ")
            .param(Value::String(Some(escape_fts5_query(&self.query))))
            .into_sql();

        Ok((sql, params))
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
            sql.push_str(" LIMIT ");
            sql.push_str(&placeholder);
        }
        if let Some(offset) = self.offset {
            let offset_value = i64::try_from(offset)
                .map_err(|_| Error::query("Full-text search offset exceeds i64 range"))?;
            let placeholder =
                crate::internal::push_param(db_type, params, Value::BigInt(Some(offset_value)));
            sql.push_str(" OFFSET ");
            sql.push_str(&placeholder);
        }
        Ok(())
    }
}
