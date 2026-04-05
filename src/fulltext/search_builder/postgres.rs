use super::*;

impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_postgres_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::Postgres, T::table_name());
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

        let mut sql = format!(
            "SELECT * FROM {} WHERE {} @@ {}",
            table, tsvector_expr, tsquery_expr
        );

        if self.with_ranking {
            let weights_placeholder = self.pg_weights_placeholder(&mut params);
            sql = format!(
                "SELECT *, ts_rank_cd(CAST({} AS real[]), {}, {}) AS _fts_rank FROM {} WHERE {} @@ {} ORDER BY _fts_rank DESC",
                weights_placeholder,
                tsvector_expr,
                tsquery_expr,
                table,
                tsvector_expr,
                tsquery_expr
            );
        }

        self.append_limit_offset(DatabaseType::Postgres, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_postgres_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::Postgres, T::table_name());
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

        let mut sql = format!(
            "SELECT *, ts_rank_cd(CAST({} AS real[]), {}, {}) AS _fts_rank FROM {} WHERE {} @@ {}",
            weights_placeholder, tsvector_expr, tsquery_expr, table, tsvector_expr, tsquery_expr
        );

        if let Some(min_rank) = self.min_rank {
            let min_rank_placeholder = crate::internal::push_param(
                DatabaseType::Postgres,
                &mut params,
                Value::Double(Some(min_rank)),
            );
            sql.push_str(&format!(
                " AND ts_rank_cd(CAST({} AS real[]), {}, {}) >= {}",
                weights_placeholder, tsvector_expr, tsquery_expr, min_rank_placeholder
            ));
        }

        sql.push_str(" ORDER BY _fts_rank DESC");
        self.append_limit_offset(DatabaseType::Postgres, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_postgres_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::Postgres, T::table_name());
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

        Ok((
            format!(
                "SELECT COUNT(*) as count FROM {} WHERE {} @@ {}",
                table, tsvector_expr, tsquery_expr
            ),
            params,
        ))
    }

    fn build_pg_tsvector_expr(&self, language_placeholder: &str) -> String {
        if self.columns.len() == 1 {
            format!(
                "to_tsvector(CAST({} AS regconfig), COALESCE({}, ''))",
                language_placeholder,
                quote_ident(DatabaseType::Postgres, &self.columns[0])
            )
        } else {
            let cols: Vec<String> = self
                .columns
                .iter()
                .map(|c| format!("COALESCE({}, '')", quote_ident(DatabaseType::Postgres, c)))
                .collect();
            format!(
                "to_tsvector(CAST({} AS regconfig), {})",
                language_placeholder,
                cols.join(" || ' ' || ")
            )
        }
    }

    fn build_pg_tsquery_expr(&self, language_placeholder: &str, params: &mut Vec<Value>) -> String {
        match self.config.mode {
            SearchMode::Natural => {
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(self.query.clone())),
                );
                format!(
                    "plainto_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
            SearchMode::Boolean => {
                let tsquery = sanitize_postgres_tsquery(&self.query, false);
                let use_plain = tsquery.is_empty();
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(if use_plain {
                        self.query.clone()
                    } else {
                        tsquery
                    })),
                );
                if use_plain {
                    format!(
                        "plainto_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                } else {
                    format!(
                        "to_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                }
            }
            SearchMode::Phrase => {
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(self.query.clone())),
                );
                format!(
                    "phraseto_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
            SearchMode::Prefix => {
                let prefixed = sanitize_postgres_tsquery(&self.query, true);
                let use_plain = prefixed.is_empty();
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(if use_plain {
                        self.query.clone()
                    } else {
                        prefixed
                    })),
                );
                if use_plain {
                    format!(
                        "plainto_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                } else {
                    format!(
                        "to_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                }
            }
            SearchMode::Fuzzy => {
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(self.query.clone())),
                );
                format!(
                    "plainto_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
            SearchMode::Proximity(distance) => {
                let proximity = sanitize_postgres_proximity_tsquery(&self.query, distance);
                let use_plain = proximity.is_empty();
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(if use_plain {
                        self.query.clone()
                    } else {
                        proximity
                    })),
                );
                if use_plain {
                    format!(
                        "plainto_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                } else {
                    format!(
                        "to_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                }
            }
        }
    }
}
