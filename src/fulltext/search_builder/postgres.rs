use super::*;

const PG: DatabaseType = DatabaseType::Postgres;

impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_postgres_sql(&self) -> Result<(String, Vec<Value>)> {
        if self.with_ranking {
            return self.build_pg_ranked_sql(None);
        }
        Ok(self.select_where(PG, self.pg_predicate()?))
    }

    pub(super) fn build_postgres_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        self.build_pg_ranked_sql(self.min_rank)
    }

    pub(super) fn build_postgres_count_sql(&self) -> Result<(String, Vec<Value>)> {
        Ok(Self::count_where(PG, self.pg_predicate()?))
    }

    /// Every column plus its `_fts_rank`, best match first, leaving out rows
    /// that score below `min_rank`.
    fn build_pg_ranked_sql(&self, min_rank: Option<f64>) -> Result<(String, Vec<Value>)> {
        let rank_alias = Self::rank_alias();
        let mut sql = BoundSql::sql(format!(
            "SELECT {}, ",
            crate::query::db_sql::model_columns_sql::<T>(PG, None)
        ));
        let from = format!(
            " FROM {} WHERE ",
            crate::query::db_sql::quote_table::<T>(PG)
        );
        let Some((tsvector, tsquery)) = self.pg_match_parts()? else {
            // No word to rank by, and no row to rank.
            sql.push(format!(
                "CAST(0 AS double precision) AS {rank_alias}{from}{MATCH_NOTHING}"
            ));
            return Ok(sql.build(PG.dialect()));
        };
        let weights = self.config.weights.clone().unwrap_or_default().pg_array();
        let mut rank = BoundSql::sql("ts_rank_cd(CAST(");
        rank.push(Value::String(Some(weights)))
            .push(format!(" AS real[]), {tsvector}, "))
            .push(tsquery.clone())
            .push(")");

        // `ts_rank_cd` returns `real`, which does not decode as the `f64` rank.
        let mut matched = BoundSql::sql(format!("{tsvector} @@ "));
        matched.push(tsquery);
        sql.push("CAST(")
            .push(rank.clone())
            .push(format!(" AS double precision) AS {rank_alias}{from}"))
            .push(self.scoped(PG, matched, None));
        if let Some(min_rank) = min_rank {
            sql.push(" AND ")
                .push(rank)
                .push(" >= ")
                .push(Value::Double(Some(min_rank)));
        }
        sql.push(format!(" ORDER BY {rank_alias} DESC"))
            .push(PG.dialect().limit_offset(self.limit, self.offset));
        Ok(sql.build(PG.dialect()))
    }

    /// `tsvector @@ tsquery`, binding the query, or a predicate that matches
    /// nothing when the search text holds no word, where `plainto_tsquery('')`
    /// would read the whole table to match nothing.
    fn pg_predicate(&self) -> Result<BoundSql> {
        Ok(match self.pg_match_parts()? {
            Some((tsvector, tsquery)) => {
                let mut matched = BoundSql::sql(format!("{tsvector} @@ "));
                matched.push(tsquery);
                self.scoped(PG, matched, None)
            }
            None => BoundSql::sql(MATCH_NOTHING),
        })
    }

    /// The `tsvector` expression and the `tsquery` binding the query, or
    /// `None` when the search text holds no word.
    fn pg_match_parts(&self) -> Result<Option<(String, BoundSql)>> {
        let language = self.config.language.as_deref();
        if let Some(name) = language
            && !name.split('.').all(is_safe_identifier_segment)
        {
            return Err(Error::query(format!(
                "'{name}' is not a text search configuration name"
            )));
        }
        if !self.has_search_terms() {
            return Ok(None);
        }
        let tsvector = pg_tsvector(language, &self.column_names());
        let tsquery = self.build_pg_tsquery_expr(&pg_language(language));
        Ok(Some((tsvector, tsquery)))
    }

    fn build_pg_tsquery_expr(&self, language: &str) -> BoundSql {
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

        let mut tsquery = BoundSql::sql(format!("{function}({language}, "));
        tsquery.push(Value::String(Some(text))).push(")");
        tsquery
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
