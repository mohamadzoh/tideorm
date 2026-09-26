use super::*;

const PG: DatabaseType = DatabaseType::Postgres;

impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_postgres_sql(&self) -> Result<(String, Vec<Value>)> {
        if self.with_ranking {
            return self.build_pg_ranked_sql(None);
        }

        let mut params = Vec::new();
        let predicate = self.pg_predicate(&mut params)?;
        let mut sql = SqlBuilder::new(PG, &mut params)
            .raw("SELECT ")
            .raw(&crate::query::db_sql::model_columns_sql::<T>(PG, None))
            .raw(" FROM ")
            .raw(&crate::query::db_sql::quote_table::<T>(PG))
            .raw(" WHERE ")
            .raw(&predicate)
            .into_sql();

        self.append_limit_offset(PG, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_postgres_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        self.build_pg_ranked_sql(self.min_rank)
    }

    pub(super) fn build_postgres_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let predicate = self.pg_predicate(&mut params)?;
        let sql = SqlBuilder::new(PG, &mut params)
            .raw("SELECT COUNT(*) as count FROM ")
            .raw(&crate::query::db_sql::quote_table::<T>(PG))
            .raw(" WHERE ")
            .raw(&predicate)
            .into_sql();

        Ok((sql, params))
    }

    /// Every column plus its `_fts_rank`, best match first, leaving out rows
    /// that score below `min_rank`.
    fn build_pg_ranked_sql(&self, min_rank: Option<f64>) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let Some((tsvector, tsquery)) = self.pg_match_parts(&mut params)? else {
            // No word to rank by, and no row to rank.
            let sql = SqlBuilder::new(PG, &mut params)
                .raw("SELECT ")
                .raw(&crate::query::db_sql::model_columns_sql::<T>(PG, None))
                .raw(", CAST(0 AS double precision) AS _fts_rank FROM ")
                .raw(&crate::query::db_sql::quote_table::<T>(PG))
                .raw(" WHERE ")
                .raw(MATCH_NOTHING)
                .into_sql();
            return Ok((sql, params));
        };
        let weights = self.config.weights.clone().unwrap_or_default().pg_array();
        let rank = SqlBuilder::new(PG, &mut params)
            .raw("ts_rank_cd(CAST(")
            .param(Value::String(Some(weights)))
            .raw(" AS real[]), ")
            .raw(&tsvector)
            .raw(", ")
            .raw(&tsquery)
            .raw(")")
            .into_sql();

        // `ts_rank_cd` returns `real`, which does not decode as the `f64` rank.
        let mut sql = SqlBuilder::new(PG, &mut params)
            .raw("SELECT ")
            .raw(&crate::query::db_sql::model_columns_sql::<T>(PG, None))
            .raw(", CAST(")
            .raw(&rank)
            .raw(" AS double precision) AS _fts_rank FROM ")
            .raw(&crate::query::db_sql::quote_table::<T>(PG))
            .raw(" WHERE ")
            .raw(&self.scoped(PG, format!("{tsvector} @@ {tsquery}"), None))
            .into_sql();

        if let Some(min_rank) = min_rank {
            let threshold = push_param(PG, &mut params, Value::Double(Some(min_rank)));
            sql.push_str(" AND ");
            sql.push_str(&rank);
            sql.push_str(" >= ");
            sql.push_str(&threshold);
        }

        sql.push_str(" ORDER BY _fts_rank DESC");
        self.append_limit_offset(PG, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    /// `tsvector @@ tsquery`, binding the query, or a predicate that matches
    /// nothing when the search text holds no word, where `plainto_tsquery('')`
    /// would read the whole table to match nothing.
    fn pg_predicate(&self, params: &mut Vec<Value>) -> Result<String> {
        Ok(match self.pg_match_parts(params)? {
            Some((tsvector, tsquery)) => self.scoped(PG, format!("{tsvector} @@ {tsquery}"), None),
            None => MATCH_NOTHING.to_string(),
        })
    }

    /// The `tsvector` and `tsquery` expressions, binding the query, or `None`
    /// when the search text holds no word.
    fn pg_match_parts(&self, params: &mut Vec<Value>) -> Result<Option<(String, String)>> {
        if !self.has_search_terms() {
            return Ok(None);
        }
        let language = self.pg_language()?;
        let tsvector = format!(
            "to_tsvector({language}, {})",
            pg_search_document(&self.column_names())
        );
        let tsquery = self.build_pg_tsquery_expr(&language, params);
        Ok(Some((tsvector, tsquery)))
    }

    /// The text search configuration, written into the SQL as a constant.
    ///
    /// An index built by [`FullTextIndex`] names its configuration as a
    /// constant, and only an expression naming the same constant can use it: a
    /// bound configuration is cast when the statement runs, which the planner
    /// cannot match to the index, so every search read the whole table. The
    /// name is checked to be one, which keeps the constant safe to write.
    fn pg_language(&self) -> Result<String> {
        let language = self.config.language.as_deref().unwrap_or("english");
        if language.split('.').all(is_safe_identifier_segment) {
            Ok(format!("'{language}'"))
        } else {
            Err(Error::invalid_query(format!(
                "'{language}' is not a text search configuration name"
            )))
        }
    }

    fn build_pg_tsquery_expr(&self, language: &str, params: &mut Vec<Value>) -> String {
        let query = self.query_text();
        let (function, text) = match self.config.mode {
            SearchMode::Natural => ("plainto_tsquery", query),
            SearchMode::Phrase => ("phraseto_tsquery", query),
            SearchMode::Boolean => {
                let tsquery = sanitize_postgres_boolean_tsquery(&query);
                Self::pg_to_tsquery(tsquery, query)
            }
            SearchMode::Prefix => {
                let tsquery = sanitize_postgres_tsquery(&query, true);
                Self::pg_to_tsquery(tsquery, query)
            }
            SearchMode::Proximity(distance) => {
                let tsquery = sanitize_postgres_proximity_tsquery(&query, distance);
                Self::pg_to_tsquery(tsquery, query)
            }
        };

        SqlBuilder::new(PG, params)
            .raw(function)
            .raw("(")
            .raw(language)
            .raw(", ")
            .param(Value::String(Some(text)))
            .raw(")")
            .into_sql()
    }

    /// Parse sanitized literal terms with `to_tsquery`, or fall back to
    /// `plainto_tsquery` over the search text when sanitizing left no lexeme,
    /// since `to_tsquery` rejects an empty query.
    fn pg_to_tsquery(tsquery: String, text: String) -> (&'static str, String) {
        if tsquery.is_empty() {
            ("plainto_tsquery", text)
        } else {
            ("to_tsquery", tsquery)
        }
    }
}
