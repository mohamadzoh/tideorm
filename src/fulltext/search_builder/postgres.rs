use super::*;
use crate::internal::sql_builder::SqlBuilder;

impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_postgres_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let language_placeholder = crate::internal::push_param(
            DatabaseType::Postgres,
            &mut params,
            Value::String(Some(
                self.config
                    .language
                    .clone()
                    .unwrap_or_else(|| "english".to_string()),
            )),
        );

        let tsvector_expr = self.build_pg_tsvector_expr(&language_placeholder);
        let tsquery_expr = self.build_pg_tsquery_expr(&language_placeholder, &mut params);

        let mut sql = SqlBuilder::new(DatabaseType::Postgres, &mut params)
            .raw("SELECT * FROM ")
            .ident(T::table_name())
            .raw(" WHERE ")
            .raw(&tsvector_expr)
            .raw(" @@ ")
            .raw(&tsquery_expr)
            .into_sql();

        if self.with_ranking {
            let weights_placeholder = self.pg_weights_placeholder(&mut params);
            sql = SqlBuilder::new(DatabaseType::Postgres, &mut params)
                .raw("SELECT *, ts_rank_cd(CAST(")
                .placeholder(&weights_placeholder)
                .raw(" AS real[]), ")
                .raw(&tsvector_expr)
                .raw(", ")
                .raw(&tsquery_expr)
                .raw(") AS _fts_rank FROM ")
                .ident(T::table_name())
                .raw(" WHERE ")
                .raw(&tsvector_expr)
                .raw(" @@ ")
                .raw(&tsquery_expr)
                .raw(" ORDER BY _fts_rank DESC")
                .into_sql();
        }

        self.append_limit_offset(DatabaseType::Postgres, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_postgres_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let language_placeholder = crate::internal::push_param(
            DatabaseType::Postgres,
            &mut params,
            Value::String(Some(
                self.config
                    .language
                    .clone()
                    .unwrap_or_else(|| "english".to_string()),
            )),
        );

        let tsvector_expr = self.build_pg_tsvector_expr(&language_placeholder);
        let tsquery_expr = self.build_pg_tsquery_expr(&language_placeholder, &mut params);
        let weights_placeholder = self.pg_weights_placeholder(&mut params);

        let mut sql = SqlBuilder::new(DatabaseType::Postgres, &mut params)
            .raw("SELECT *, ts_rank_cd(CAST(")
            .placeholder(&weights_placeholder)
            .raw(" AS real[]), ")
            .raw(&tsvector_expr)
            .raw(", ")
            .raw(&tsquery_expr)
            .raw(") AS _fts_rank FROM ")
            .ident(T::table_name())
            .raw(" WHERE ")
            .raw(&tsvector_expr)
            .raw(" @@ ")
            .raw(&tsquery_expr)
            .into_sql();

        if let Some(min_rank) = self.min_rank {
            let min_rank_placeholder = crate::internal::push_param(
                DatabaseType::Postgres,
                &mut params,
                Value::Double(Some(min_rank)),
            );
            sql.push_str(" AND ts_rank_cd(CAST(");
            sql.push_str(&weights_placeholder);
            sql.push_str(" AS real[]), ");
            sql.push_str(&tsvector_expr);
            sql.push_str(", ");
            sql.push_str(&tsquery_expr);
            sql.push_str(") >= ");
            sql.push_str(&min_rank_placeholder);
        }

        sql.push_str(" ORDER BY _fts_rank DESC");
        self.append_limit_offset(DatabaseType::Postgres, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_postgres_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let language_placeholder = crate::internal::push_param(
            DatabaseType::Postgres,
            &mut params,
            Value::String(Some(
                self.config
                    .language
                    .clone()
                    .unwrap_or_else(|| "english".to_string()),
            )),
        );

        let tsvector_expr = self.build_pg_tsvector_expr(&language_placeholder);
        let tsquery_expr = self.build_pg_tsquery_expr(&language_placeholder, &mut params);

        let sql = SqlBuilder::new(DatabaseType::Postgres, &mut params)
            .raw("SELECT COUNT(*) as count FROM ")
            .ident(T::table_name())
            .raw(" WHERE ")
            .raw(&tsvector_expr)
            .raw(" @@ ")
            .raw(&tsquery_expr)
            .into_sql();

        Ok((sql, params))
    }

    fn build_pg_tsvector_expr(&self, language_placeholder: &str) -> String {
        if self.columns.len() == 1 {
            let mut params = Vec::new();
            SqlBuilder::new(DatabaseType::Postgres, &mut params)
                .raw("to_tsvector(CAST(")
                .placeholder(language_placeholder)
                .raw(" AS regconfig), COALESCE(")
                .ident(&self.columns[0])
                .raw(", ''))")
                .into_sql()
        } else {
            let cols: Vec<String> = self
                .columns
                .iter()
                .map(|c| {
                    let mut params = Vec::new();
                    SqlBuilder::new(DatabaseType::Postgres, &mut params)
                        .raw("COALESCE(")
                        .ident(c)
                        .raw(", '')")
                        .into_sql()
                })
                .collect();
            let mut params = Vec::new();
            SqlBuilder::new(DatabaseType::Postgres, &mut params)
                .raw("to_tsvector(CAST(")
                .placeholder(language_placeholder)
                .raw(" AS regconfig), ")
                .raw(&cols.join(" || ' ' || "))
                .raw(")")
                .into_sql()
        }
    }

    fn build_pg_tsquery_expr(&self, language_placeholder: &str, params: &mut Vec<Value>) -> String {
        match self.config.mode {
            SearchMode::Natural => SqlBuilder::new(DatabaseType::Postgres, params)
                .raw("plainto_tsquery(CAST(")
                .placeholder(language_placeholder)
                .raw(" AS regconfig), ")
                .param(Value::String(Some(self.query.clone())))
                .raw(")")
                .into_sql(),
            SearchMode::Boolean => {
                let tsquery = sanitize_postgres_tsquery(&self.query, false);
                let use_plain = tsquery.is_empty();
                let value = if use_plain {
                    self.query.clone()
                } else {
                    tsquery
                };
                SqlBuilder::new(DatabaseType::Postgres, params)
                    .raw(if use_plain {
                        "plainto_tsquery(CAST("
                    } else {
                        "to_tsquery(CAST("
                    })
                    .placeholder(language_placeholder)
                    .raw(" AS regconfig), ")
                    .param(Value::String(Some(value)))
                    .raw(")")
                    .into_sql()
            }
            SearchMode::Phrase => SqlBuilder::new(DatabaseType::Postgres, params)
                .raw("phraseto_tsquery(CAST(")
                .placeholder(language_placeholder)
                .raw(" AS regconfig), ")
                .param(Value::String(Some(self.query.clone())))
                .raw(")")
                .into_sql(),
            SearchMode::Prefix => {
                let prefixed = sanitize_postgres_tsquery(&self.query, true);
                let use_plain = prefixed.is_empty();
                let value = if use_plain {
                    self.query.clone()
                } else {
                    prefixed
                };
                SqlBuilder::new(DatabaseType::Postgres, params)
                    .raw(if use_plain {
                        "plainto_tsquery(CAST("
                    } else {
                        "to_tsquery(CAST("
                    })
                    .placeholder(language_placeholder)
                    .raw(" AS regconfig), ")
                    .param(Value::String(Some(value)))
                    .raw(")")
                    .into_sql()
            }
            SearchMode::Fuzzy => SqlBuilder::new(DatabaseType::Postgres, params)
                .raw("plainto_tsquery(CAST(")
                .placeholder(language_placeholder)
                .raw(" AS regconfig), ")
                .param(Value::String(Some(self.query.clone())))
                .raw(")")
                .into_sql(),
            SearchMode::Proximity(distance) => {
                let proximity = sanitize_postgres_proximity_tsquery(&self.query, distance);
                let use_plain = proximity.is_empty();
                let value = if use_plain {
                    self.query.clone()
                } else {
                    proximity
                };
                SqlBuilder::new(DatabaseType::Postgres, params)
                    .raw(if use_plain {
                        "plainto_tsquery(CAST("
                    } else {
                        "to_tsquery(CAST("
                    })
                    .placeholder(language_placeholder)
                    .raw(" AS regconfig), ")
                    .param(Value::String(Some(value)))
                    .raw(")")
                    .into_sql()
            }
        }
    }
}
