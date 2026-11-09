//! What a query's projection reads: its output columns, their sources and
//! encryption, and the shape a compound operand must match.

use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Each named result column of the projection, with the table and column
    /// it is read from: its alias when it has one, otherwise the column's own
    /// name.
    ///
    /// A plain column is read from its qualifier's table, or the model's when
    /// unqualified. Scalar subqueries retain their source metadata, and joined
    /// wildcards expand using registered model columns. The own-table wildcard
    /// stays implicit; unaliased expressions have no known output name.
    pub(in crate::query) fn projection_outputs(&self) -> Vec<(String, Option<(String, String)>)> {
        let typed = self.clauses.select_columns.iter().flatten();
        let raw = self.raw_projection();
        let subqueries = self
            .clauses
            .subquery_select_expressions
            .iter()
            .map(|subquery| (subquery.alias.clone(), subquery.source.clone()));
        typed
            .chain(raw)
            .flat_map(|entry| {
                Self::named_projection_output(entry)
                    .into_iter()
                    .chain(self.joined_wildcard_outputs(entry))
            })
            .chain(subqueries)
            .chain(self.clauses.window_functions.iter().map(|window| {
                let source = window.function.value_column().and_then(Self::column_source);
                (window.alias.clone(), source)
            }))
            .collect()
    }

    /// The table and column a plain column reference reads, the model's table
    /// when it is unqualified; `None` for an expression.
    pub(super) fn column_source(reference: &str) -> Option<(String, String)> {
        let (table, column) = Self::simple_column_reference(reference)?;
        let table = if table.is_empty() {
            M::table_name()
        } else {
            table
        };
        Some((table.to_string(), column.to_string()))
    }

    pub(super) fn named_projection_output(
        entry: &str,
    ) -> Option<(String, Option<(String, String)>)> {
        let entry = entry.trim();
        let (expression, alias) = Self::split_alias(entry);
        let source = Self::column_source(expression);
        let name = match alias {
            Some(alias) => alias.trim_matches(|c| c == '"' || c == '`').to_string(),
            None => {
                let (table, column) = source.as_ref()?;
                match table == M::table_name() {
                    true => M::canonical_column_name(column).unwrap_or(column),
                    false => column,
                }
                .to_string()
            }
        };
        Some((name, source))
    }

    /// Resolve joined wildcards using model metadata. The own-table wildcard
    /// stays implicit, as model_projection_count already accounts for it.
    pub(super) fn joined_wildcard_outputs(
        &self,
        entry: &str,
    ) -> Vec<(String, Option<(String, String)>)> {
        let entry = entry.trim();
        let qualifier = if entry == "*" {
            None
        } else if let Some(qualifier) = entry.strip_suffix(".*") {
            Some(qualifier)
        } else {
            return Vec::new();
        };
        self.clauses
            .joins
            .iter()
            .filter(|join| qualifier.is_none_or(|qualifier| qualifier == join.qualifier()))
            .flat_map(|join| {
                let columns = self.joined_column_names(join);
                columns.iter().map(move |column| {
                    (
                        column.to_string(),
                        Some((join.qualifier().to_string(), column.to_string())),
                    )
                })
            })
            .collect()
    }

    pub(in crate::query) fn joined_column_names(
        &self,
        join: &crate::query::JoinClause,
    ) -> &'static [&'static str] {
        if self.is_model_table_qualifier(join.qualifier()) {
            M::column_names()
        } else {
            let (schema, table) = schema_and_table(&join.table);
            crate::sync::registered_column_names(schema, table)
        }
    }

    /// The join a `qualifier.column` reference reads: the one with that
    /// alias, or with that table and no alias.
    pub(in crate::query) fn join_named(
        &self,
        qualifier: &str,
    ) -> Option<&crate::query::JoinClause> {
        self.clauses.joins.iter().find(|join| {
            join.qualifier() == qualifier || (join.alias.is_none() && join.table == qualifier)
        })
    }

    /// Chunk cursors compare the stored key, so the model must read that same
    /// key rather than a different column or expression under its output name.
    pub(in crate::query) fn ensure_chunk_key_is_original(&self, primary_key: &str) -> Result<()> {
        let original = self.model_projection_count() > 0
            || self.projection_outputs().iter().any(|(name, source)| {
                name == primary_key
                    && source.as_ref().is_some_and(|(table, column)| {
                        table == M::table_name() && M::column_named(column) == primary_key
                    })
            });
        if original {
            Ok(())
        } else {
            Err(Error::query(format!(
                "chunk() requires the original primary key '{}.{}' in its projection; use get() for a projection that replaces it",
                M::table_name(),
                primary_key
            )))
        }
    }

    /// Whether a qualifier names this model's table, including a self-join
    /// alias. Keep explicit schemas distinct from a same-named table elsewhere.
    pub(in crate::query) fn is_model_table_qualifier(&self, qualifier: &str) -> bool {
        M::owns_table_reference(qualifier)
            || self.clauses.joins.iter().any(|join| {
                join.qualifier() == qualifier
                    && schema_and_table(&join.table) == (M::schema_name(), M::table_name())
            })
    }

    /// Resolve an input in this query's scope before looking up model metadata.
    pub(in crate::query) fn encrypted_source_column(
        &self,
        table: &str,
        column: &str,
    ) -> Option<crate::model::EncryptedColumn> {
        if self.is_model_table_qualifier(table) {
            return crate::model::__encrypted_column::<M>(column);
        }
        let join = self.join_named(table)?;
        let (schema, table) = schema_and_table(&join.table);
        crate::sync::registered_encrypted_column(schema, table, column)
    }

    pub(in crate::query) fn projected_encrypted_column(
        &self,
        output: &str,
        source: Option<&(String, String)>,
    ) -> Option<crate::model::EncryptedColumn> {
        if let Some(subquery) = self
            .clauses
            .subquery_select_expressions
            .iter()
            .find(|subquery| subquery.alias == output && subquery.source.as_ref() == source)
        {
            return subquery.encrypted_column;
        }
        let (table, column) = source?;
        self.encrypted_source_column(table, column)
    }

    /// Whether the projection names every column itself: an explicit one
    /// with no `*` or `table.*` wildcard, whose result columns
    /// [`projection_outputs`](Self::projection_outputs) lists in full.
    pub(in crate::query) fn projection_is_closed(&self) -> bool {
        self.has_explicit_projection()
            && !self
                .clauses
                .select_columns
                .iter()
                .flatten()
                .chain(&self.clauses.raw_select_expressions)
                .any(|entry| {
                    let entry = entry.trim();
                    entry == "*" || entry.ends_with(".*")
                })
    }

    /// Refuse a projection that names one result column twice: rows come back
    /// keyed by name, so one of the two would be lost, and a derived table over
    /// the query is rejected by MySQL and MariaDB.
    pub(super) fn validate_projection_names(&self) -> std::result::Result<(), String> {
        let outputs = self.projection_outputs();
        let model_columns =
            (0..self.model_projection_count()).flat_map(|_| M::column_names().iter().copied());
        let names: Vec<&str> = outputs
            .iter()
            .map(|(name, _)| name.as_str())
            .chain(model_columns)
            .collect();
        for (index, name) in names.iter().enumerate() {
            if names[..index].contains(name) {
                return Err(format!(
                    "the projection names the column '{}' twice, and a result row keeps one value per name; alias one of them, as in `joined_table.{} AS joined_{}`",
                    name, name, name
                ));
            }
        }
        Ok(())
    }

    /// How often the model's columns are selected implicitly or by wildcard.
    pub(super) fn model_projection_count(&self) -> usize {
        // Match the renderer's fallback, including select([]) and DISTINCT.
        // Window expressions are appended after this implicit projection.
        let implicit = self
            .clauses
            .select_columns
            .iter()
            .flatten()
            .next()
            .is_none()
            && self.raw_projection().next().is_none()
            && self.clauses.subquery_select_expressions.is_empty();
        let wildcards = self
            .clauses
            .select_columns
            .iter()
            .flatten()
            .chain(self.raw_projection())
            .filter(|entry| {
                let entry = entry.trim();
                entry == "*" || entry.strip_suffix(".*") == Some(M::table_name())
            })
            .count();
        usize::from(implicit) + wildcards
    }

    pub(super) fn ensure_distinct_projection_is_comparable(&self) -> Result<()> {
        if self.model_projection_count() > 0 {
            for &(_, column) in M::encrypted_fields() {
                self.ensure_encrypted_column_is_comparable(column, "DISTINCT")?;
            }
        }
        for (output, source) in self.projection_outputs() {
            Self::reject_encrypted_source(
                self.projected_encrypted_column(&output, source.as_ref()),
                "DISTINCT",
            )?;
        }
        Ok(())
    }

    /// Capture positional types and encryption scopes before a compound operand becomes SQL.
    pub(in crate::query) fn compound_projection(&self) -> crate::query::structure::UnionProjection {
        let outputs = self.projection_outputs();
        let mut projection = crate::query::structure::UnionProjection {
            columns: Vec::new(),
            types: Vec::new(),
            complete: true,
            has_encrypted: false,
        };
        let entries: Vec<_> = self
            .clauses
            .select_columns
            .iter()
            .flatten()
            .chain(self.raw_projection())
            .collect();
        if entries.is_empty() && self.clauses.subquery_select_expressions.is_empty() {
            // The default SELECT enumerates model columns in their declared order.
            for column in M::column_names() {
                projection
                    .types
                    .push(crate::internal::column_type_of::<M>(column));
                projection
                    .columns
                    .push(crate::model::__encrypted_column::<M>(column));
            }
        }
        for entry in entries {
            match Self::named_projection_output(entry) {
                Some((name, Some(source)))
                    if db_sql::is_safe_identifier_segment(&source.0)
                        && db_sql::is_safe_identifier_segment(&source.1) =>
                {
                    projection
                        .types
                        .push(self.column_type(&format!("{}.{}", source.0, source.1)));
                    projection
                        .columns
                        .push(self.projected_encrypted_column(&name, Some(&source)));
                }
                _ => {
                    // Wildcard order depends on the live schema; raw expressions may
                    // contribute several columns. Do not guess their positions.
                    projection.complete = false;
                    projection.columns.push(None);
                    projection.types.push(None);
                }
            }
        }
        for subquery in &self.clauses.subquery_select_expressions {
            projection.columns.push(subquery.encrypted_column);
            projection.types.push(subquery.column_type.clone());
        }
        for window in &self.clauses.window_functions {
            projection
                .types
                .push(self.projected_column_type(&window.alias));
            let source = outputs
                .iter()
                .find(|(name, _)| *name == window.alias)
                .and_then(|(_, source)| source.as_ref());
            projection
                .columns
                .push(self.projected_encrypted_column(&window.alias, source));
        }
        // Include encrypted wildcard outputs even when their positions are unknown.
        projection.has_encrypted = projection.columns.iter().any(Option::is_some);
        if self.model_projection_count() > 0 {
            projection.has_encrypted |= M::has_encrypted_fields();
        }
        for (name, source) in &outputs {
            projection.has_encrypted |= self
                .projected_encrypted_column(name, source.as_ref())
                .is_some();
        }
        projection
    }

    /// Whether the query's default projection is `table.*`: a raw union arm's
    /// shape is unknown, typically `SELECT *`, so it reads the table's
    /// columns in the table's order rather than the model's.
    pub(in crate::query) fn reads_table_wildcard(&self) -> bool {
        !self.has_explicit_projection() && self.clauses.unions.iter().any(|union| union.raw)
    }

    pub(in crate::query) fn ensure_union_projections_are_valid(&self) -> Result<()> {
        if self.clauses.unions.is_empty() {
            return Ok(());
        }
        // A raw arm makes the default projection `table.*`, in the table's
        // column order, while a typed arm reads the model's declared order: a
        // column the two order differently would land under another's name.
        if self.reads_table_wildcard() && self.clauses.unions.iter().any(|union| !union.raw) {
            return Err(Error::query(format!(
                "a query of '{}' with a raw union arm reads its rows as {}.*, in the table's \
                 column order, which a typed arm does not follow; use raw arms only, typed arms \
                 only, or select() explicit columns in every arm",
                M::table_name(),
                M::table_name()
            )));
        }
        let left = self.compound_projection();
        for union in &self.clauses.unions {
            let right = union.projection.as_ref();
            let encrypted = left.has_encrypted || right.is_some_and(|right| right.has_encrypted);
            if encrypted && union.union_type == crate::query::UnionType::Union {
                return Err(Error::query(
                    "UNION is not supported for encrypted projections: randomized ciphertext cannot deduplicate plaintext values; use compatible UNION ALL projections and compare after decryption",
                ));
            }
            let compatible = right.is_some_and(|right| {
                left.complete
                    && right.complete
                    && left.columns.len() == right.columns.len()
                    && left.columns.iter().zip(&right.columns).all(|(a, b)| {
                        a.map(|source| (source.table, source.column))
                            == b.map(|source| (source.table, source.column))
                    })
            });
            if encrypted && !compatible {
                return Err(Error::query(
                    "UNION ALL encrypted projections require the same encryption table and column at each output position; select explicit columns with matching scopes, or read incompatible queries separately",
                ));
            }
            if let Some(right) = right
                && left.complete
                && right.complete
                && left.types.len() == right.types.len()
            {
                for (index, (a, b)) in left.types.iter().zip(&right.types).enumerate() {
                    if let (Some(a), Some(b)) = (a, b)
                        && !crate::query::structure::union_column_types_compatible(a, b)
                    {
                        return Err(Error::query(format!(
                            "UNION output position {} has incompatible column types {:?} and {:?}; select matching types or read the queries separately",
                            index + 1,
                            a,
                            b,
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether the caller chose the projection through `select()`,
    /// `select_raw()` or `select_subquery()`.
    pub(in crate::query) fn has_explicit_projection(&self) -> bool {
        self.clauses.select_columns.is_some()
            || !self.clauses.subquery_select_expressions.is_empty()
            || self.raw_projection().next().is_some()
    }

    /// This query without its ORDER BY, for a terminal that does not render
    /// one (`count()`, `exists()`), so an ordering it discards cannot fail it.
    pub(in crate::query) fn discarding_order(mut self) -> Self {
        self.clauses.order_by.clear();
        self
    }

    /// [`discarding_order`](Self::discarding_order) for a `distinct()`
    /// query, whose ORDER BY has to name selected columns, a rule an ordering
    /// that is never rendered does not have to meet. Any other query keeps its
    /// ORDER BY, so an unsafe one fails `count()` as it fails `get()`.
    pub(in crate::query) fn without_distinct_ordering(self) -> Self {
        if self.is_distinct() {
            self.discarding_order()
        } else {
            self
        }
    }

    /// The `select_raw()` expressions, without the `distinct()` marker that
    /// shares their list.
    pub(in crate::query) fn raw_projection(&self) -> impl Iterator<Item = &String> {
        self.clauses
            .raw_select_expressions
            .iter()
            .filter(|expression| expression.as_str() != DISTINCT_SELECT_MARKER)
    }

    /// Whether [`distinct()`](Self::distinct) was requested for this query.
    pub(in crate::query) fn is_distinct(&self) -> bool {
        self.clauses.is_distinct()
    }
}
