use super::{
    CTE, ConditionValue, Operator, OrGroup, QueryBuilder, QueryFragment, UnionClause,
    WhereCondition, WindowFunction, WindowFunctionType, db_sql,
};
use crate::error::{Error, Result};
use crate::model::Model;
use std::collections::BTreeSet;
use std::marker::PhantomData;

/// Marker stamped onto ORDER BY entries produced by `QueryBuilder::order_by_raw`.
///
/// ORDER BY is rendered outside any quoted literal, so `order_by()` only accepts
/// resolvable column references. `order_by_raw()` is the explicit opt-in for a
/// trusted SQL expression, and it tags the stored entry with this marker so both
/// validation and rendering can tell the two apart without widening the column
/// allowlist. The control characters keep the marker outside anything a column
/// name or a user-supplied sort parameter can legitimately contain, and
/// `order_by()` rejects values carrying it.
pub(in crate::query) const RAW_ORDER_BY_MARKER: &str = "\u{1}tideorm_raw_order_by\u{1}";

/// Tag a caller-trusted ORDER BY expression so it bypasses column validation.
pub(in crate::query) fn raw_order_by_entry(expression: &str) -> String {
    format!("{}{}", RAW_ORDER_BY_MARKER, expression.trim())
}

/// Return the trusted expression behind an `order_by_raw()` entry, if any.
pub(in crate::query) fn raw_order_by_expression(value: &str) -> Option<&str> {
    value.strip_prefix(RAW_ORDER_BY_MARKER)
}

/// Whether a value carries the `order_by_raw()` marker anywhere inside it.
///
/// `order_by()` uses this to refuse forged markers before they are stored.
pub(in crate::query) fn contains_raw_order_by_marker(value: &str) -> bool {
    value.contains(RAW_ORDER_BY_MARKER)
}

/// Sentinel entry recorded in `raw_select_expressions` by
/// [`QueryBuilder::distinct`](super::QueryBuilder::distinct).
///
/// `DISTINCT` is a prefix of the projection rather than an expression inside it,
/// but the builder keeps the whole projection in the select accumulators no
/// matter which of `select()`/`select_raw()`/`select_subquery()` contributed to
/// it. Recording the request as a sentinel in that same accumulator is what
/// keeps every existing consumer of the projection correct without a second
/// mechanism: `consolidate()`/`apply()` carry it, `generate_cache_key()` hashes
/// it so a distinct query never collides with its non-distinct twin, and
/// `build_count_sql_with_params_for_db()` leaves its `SELECT COUNT(*)` fast path
/// for the derived-table path — which is the difference between counting the
/// deduplicated rows and counting the duplicates `DISTINCT` exists to remove.
///
/// The renderer strips the sentinel and emits the keyword in its place, so it
/// never reaches SQL. The control characters keep it outside anything a real
/// SELECT expression can contain.
pub(in crate::query) const DISTINCT_SELECT_MARKER: &str = "\u{1}tideorm_select_distinct\u{1}";

impl<M: Model> QueryBuilder<M> {
    fn known_model_column_references() -> (&'static str, String) {
        let field_names = M::field_names()
            .iter()
            .copied()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        let column_names = M::column_names()
            .iter()
            .copied()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();

        if field_names == column_names {
            return (
                "known columns",
                column_names.into_iter().collect::<Vec<_>>().join(", "),
            );
        }

        (
            "known fields/columns",
            field_names
                .into_iter()
                .chain(column_names)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", "),
        )
    }

    fn split_select_alias(value: &str) -> &str {
        let trimmed = value.trim();
        match Self::find_top_level_alias(trimmed) {
            Some((expression, _)) => expression,
            None => trimmed,
        }
    }

    /// Find the last top-level ` AS ` separator in a SELECT expression.
    ///
    /// The scan tracks parenthesis depth and string/identifier quote state, so
    /// an inner `AS` inside `CAST(col AS TEXT)` is ignored and only the outer
    /// alias boundary is returned.
    fn find_top_level_alias(value: &str) -> Option<(&str, &str)> {
        let bytes = value.as_bytes();
        let mut depth: i32 = 0;
        let mut quote: Option<u8> = None;
        let mut last_as: Option<(usize, usize)> = None;

        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if let Some(q) = quote {
                if b == q {
                    // Handle SQL doubled-quote escapes: '', "", ``.
                    if bytes.get(i + 1).copied() == Some(q) {
                        i += 2;
                        continue;
                    }
                    quote = None;
                }
                i += 1;
                continue;
            }
            match b {
                b'\'' | b'"' | b'`' => {
                    quote = Some(b);
                    i += 1;
                }
                b'(' => {
                    depth += 1;
                    i += 1;
                }
                b')' => {
                    depth -= 1;
                    i += 1;
                }
                b' ' | b'\t' | b'\n' | b'\r' if depth == 0 => {
                    if i + 3 < bytes.len()
                        && matches!(bytes[i + 1], b'a' | b'A')
                        && matches!(bytes[i + 2], b's' | b'S')
                        && matches!(bytes[i + 3], b' ' | b'\t' | b'\n' | b'\r')
                    {
                        last_as = Some((i, i + 4));
                        i += 4;
                    } else {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }

        let (start, end) = last_as?;
        let expression = value[..start].trim();
        let alias = value[end..].trim();
        if expression.is_empty() || alias.is_empty() {
            return None;
        }
        Some((expression, alias))
    }

    fn simple_column_reference(value: &str) -> Option<(&str, &str)> {
        let value = Self::split_select_alias(value);
        if value.is_empty()
            || value.starts_with('"')
            || value.ends_with('"')
            || value.starts_with('`')
            || value.ends_with('`')
            || value.contains('(')
            || value.contains(')')
            || value.contains('*')
            || value.contains(' ')
        {
            return None;
        }

        match value.split_once('.') {
            Some((table, column))
                if !table.is_empty() && !column.is_empty() && !column.contains('.') =>
            {
                Some((table, column))
            }
            Some(_) => None,
            None => Some(("", value)),
        }
    }

    fn validate_model_column_reference(
        kind: &str,
        value: &str,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        let Some((table, column)) = Self::simple_column_reference(value) else {
            return Ok(());
        };

        let reference = if table.is_empty() {
            column.to_string()
        } else {
            format!("{}.{}", table, column)
        };
        db_sql::validate_identifier_reference(kind, &reference)?;

        if table.is_empty() || table == M::table_name() {
            if M::canonical_column_name(column).is_some() {
                Ok(())
            } else {
                let (known_label, known_names) = Self::known_model_column_references();
                Err(format!(
                    "unknown {} '{}' for model '{}'; {}: {}",
                    kind,
                    reference,
                    M::table_name(),
                    known_label,
                    known_names
                ))
            }
        } else if let Some(qualifiers) = known_qualifiers {
            if qualifiers.contains(table) {
                Ok(())
            } else {
                let known = qualifiers
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(format!(
                    "unknown {} qualifier '{}' in '{}' for model '{}'; known table/alias qualifiers: {}",
                    kind,
                    table,
                    reference,
                    M::table_name(),
                    known
                ))
            }
        } else {
            Ok(())
        }
    }

    /// Validate one ORDER BY or GROUP BY slot.
    ///
    /// Both clauses are rendered outside any quoted literal, so a blocklist over
    /// raw SQL cannot make them safe: an expression such as
    /// `(CASE WHEN (SELECT ...) THEN id ELSE name END)` contains no forbidden
    /// token yet leaks data one comparison at a time. These slots therefore
    /// accept an allowlist only — a column the model resolves, optionally
    /// table-qualified, and (for ORDER BY) optionally followed by `ASC`/`DESC`.
    ///
    /// Callers who genuinely need an expression opt in through
    /// `QueryBuilder::order_by_raw`, whose entries carry `RAW_ORDER_BY_MARKER`
    /// and are checked as trusted raw SQL instead.
    fn validate_order_or_group_value(
        kind: &str,
        value: &str,
        allow_raw_and_direction: bool,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        if allow_raw_and_direction && let Some(expression) = raw_order_by_expression(value) {
            return db_sql::validate_raw_sql_fragment(kind, expression);
        }

        let trimmed = value.trim();
        let (reference, direction) = match trimmed.split_once(char::is_whitespace) {
            Some((reference, rest)) => (reference, Some(rest.trim())),
            None => (trimmed, None),
        };

        let direction_is_valid = direction.is_none_or(|direction| {
            allow_raw_and_direction
                && (direction.eq_ignore_ascii_case("asc") || direction.eq_ignore_ascii_case("desc"))
        });

        if !direction_is_valid || Self::simple_column_reference(reference).is_none() {
            let hint = if allow_raw_and_direction {
                "expected a column or table.column reference, optionally followed by ASC or DESC; use order_by_raw() for trusted SQL expressions and never pass user input to it"
            } else {
                "expected a column or table.column reference; SQL expressions are not accepted here"
            };

            return Err(format!("unsafe {} '{}': {}", kind, trimmed, hint));
        }

        Self::validate_model_column_reference(kind, reference, known_qualifiers)
    }

    fn validate_select_value(
        value: &str,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        let trimmed = value.trim();

        if let Some((expression, alias)) = Self::find_top_level_alias(trimmed) {
            if Self::simple_column_reference(expression).is_some() {
                Self::validate_model_column_reference(
                    "SELECT column",
                    expression,
                    known_qualifiers,
                )?;
            } else {
                db_sql::validate_raw_sql_fragment("SELECT expression", expression)?;
            }

            db_sql::validate_identifier("SELECT alias", alias)
        } else if Self::simple_column_reference(trimmed).is_some() {
            Self::validate_model_column_reference("SELECT column", trimmed, known_qualifiers)
        } else {
            db_sql::validate_raw_sql_fragment("SELECT expression", trimmed)
        }
    }

    /// Whether [`distinct()`](Self::distinct) was requested for this query.
    pub(in crate::query) fn is_distinct(&self) -> bool {
        self.raw_select_expressions
            .iter()
            .any(|expression| expression == DISTINCT_SELECT_MARKER)
    }

    /// Normalize a split column reference to the `table.column` form the
    /// distinct-projection comparison uses.
    ///
    /// An unqualified reference belongs to the model's own table, and a
    /// reference into that table is resolved from its Rust field name to its
    /// database column name so the two spellings compare equal.
    fn normalized_column_reference(qualifier: &str, column: &str) -> String {
        let table = M::table_name();
        let qualifier = if qualifier.is_empty() {
            table
        } else {
            qualifier
        };
        let column = if qualifier == table {
            M::canonical_column_name(column).unwrap_or(column)
        } else {
            column
        };

        format!("{}.{}", qualifier, column)
    }

    /// The column references a `SELECT DISTINCT` projection exposes, or `None`
    /// when the projection carries an expression this cannot analyse.
    ///
    /// Each SELECT alias is recorded under its bare name as well, because
    /// `ORDER BY <alias>` resolves against the projection rather than the table.
    fn distinct_projection_references(&self) -> Option<BTreeSet<String>> {
        // A raw expression or a scalar subquery can project anything at all,
        // including the very column an ORDER BY term names, so once one is
        // present nothing can be proven missing.
        let has_opaque_projection = self
            .raw_select_expressions
            .iter()
            .any(|expression| expression != DISTINCT_SELECT_MARKER)
            || !self.subquery_select_expressions.is_empty();

        if has_opaque_projection {
            return None;
        }

        let table = M::table_name();
        let mut references = BTreeSet::new();

        // An absent or empty column list renders as the `table.*` fallback,
        // which exposes every column of the model.
        let Some(columns) = self
            .select_columns
            .as_ref()
            .filter(|columns| !columns.is_empty())
        else {
            for column in M::column_names() {
                references.insert(format!("{}.{}", table, column));
            }
            return Some(references);
        };

        for column in columns {
            let (expression, alias) = match Self::find_top_level_alias(column) {
                Some((expression, alias)) => (expression, Some(alias)),
                None => (column.trim(), None),
            };

            if let Some(alias) = alias {
                references.insert(alias.to_string());
            }

            let (qualifier, name) = Self::simple_column_reference(expression)?;
            references.insert(Self::normalized_column_reference(qualifier, name));
        }

        Some(references)
    }

    /// Reject an ORDER BY term a `SELECT DISTINCT` cannot sort by.
    ///
    /// PostgreSQL requires every ORDER BY expression of a `SELECT DISTINCT` to
    /// appear in the select list and otherwise fails the statement with
    /// `for SELECT DISTINCT, ORDER BY expressions must appear in select list`.
    /// Checking it here turns that into a builder error that names the offending
    /// column on every backend, instead of an opaque server error on one of
    /// them. Terms added through `order_by_raw()` are trusted opaque SQL and
    /// stay the caller's responsibility, as does an ORDER BY on a query whose
    /// projection contains a raw expression.
    fn validate_distinct_order_by(&self) -> std::result::Result<(), String> {
        let Some(projection) = self.distinct_projection_references() else {
            return Ok(());
        };

        for (column, _) in &self.order_by {
            if raw_order_by_expression(column).is_some() {
                continue;
            }

            let trimmed = column.trim();
            let reference = trimmed
                .split_once(char::is_whitespace)
                .map_or(trimmed, |(reference, _)| reference);

            let Some((qualifier, name)) = Self::simple_column_reference(reference) else {
                continue;
            };

            if projection.contains(&Self::normalized_column_reference(qualifier, name))
                || projection.contains(name)
            {
                continue;
            }

            return Err(format!(
                "ORDER BY '{}' is not part of the distinct() projection of model '{}'; a SELECT DISTINCT can only be ordered by expressions it selects, so add the column to select() or drop distinct()",
                trimmed,
                M::table_name()
            ));
        }

        Ok(())
    }

    fn validate_condition(
        condition: &WhereCondition,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        match (&condition.operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => {
                let kind = if condition.column.is_empty() {
                    "WHERE raw SQL"
                } else {
                    "WHERE raw column expression"
                };
                db_sql::validate_raw_sql_fragment(kind, raw_sql)
            }
            (Operator::SubqueryIn, ConditionValue::Subquery(query_sql))
            | (Operator::SubqueryNotIn, ConditionValue::Subquery(query_sql)) => {
                db_sql::validate_compound_subquery_sql(query_sql)
            }
            _ => Ok(()),
        }?;

        if !condition.column.is_empty() {
            Self::validate_model_column_reference(
                "WHERE column",
                &condition.column,
                known_qualifiers,
            )?;
        }

        Ok(())
    }

    fn validate_or_group(
        group: &OrGroup,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        for condition in &group.conditions {
            Self::validate_condition(condition, known_qualifiers)?;
        }

        for nested_group in &group.nested_groups {
            Self::validate_or_group(nested_group, known_qualifiers)?;
        }

        Ok(())
    }

    pub(super) fn validate_union_clause(union: &UnionClause) -> std::result::Result<(), String> {
        db_sql::validate_subquery_sql(&union.query_sql)
    }

    pub(super) fn validate_window_function(
        window_function: &WindowFunction,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        db_sql::validate_identifier("window alias", &window_function.alias)?;

        for column in &window_function.partition_by {
            Self::validate_model_column_reference(
                "window PARTITION BY column",
                column,
                known_qualifiers,
            )?;
        }

        for (column, _) in &window_function.order_by {
            Self::validate_model_column_reference(
                "window ORDER BY column",
                column,
                known_qualifiers,
            )?;
        }

        match &window_function.function {
            WindowFunctionType::Lag(column, _, default)
            | WindowFunctionType::Lead(column, _, default) => {
                Self::validate_model_column_reference(
                    "window function column",
                    column,
                    known_qualifiers,
                )?;

                if let Some(default) = default {
                    db_sql::validate_raw_sql_fragment("LAG/LEAD default expression", default)?;
                }
            }
            WindowFunctionType::FirstValue(column)
            | WindowFunctionType::LastValue(column)
            | WindowFunctionType::Sum(column)
            | WindowFunctionType::Avg(column)
            | WindowFunctionType::Min(column)
            | WindowFunctionType::Max(column) => {
                Self::validate_model_column_reference(
                    "window function column",
                    column,
                    known_qualifiers,
                )?;
            }
            WindowFunctionType::NthValue(column, _) => {
                Self::validate_model_column_reference(
                    "window function column",
                    column,
                    known_qualifiers,
                )?;
            }
            WindowFunctionType::Count(Some(column)) => {
                Self::validate_model_column_reference(
                    "window function column",
                    column,
                    known_qualifiers,
                )?;
            }
            WindowFunctionType::Custom(expression) => {
                db_sql::validate_raw_sql_fragment("window function expression", expression)?;
            }
            WindowFunctionType::RowNumber
            | WindowFunctionType::Rank
            | WindowFunctionType::DenseRank
            | WindowFunctionType::Ntile(_)
            | WindowFunctionType::Count(None) => {}
        }

        Ok(())
    }

    pub(super) fn validate_cte_clause(cte: &CTE) -> std::result::Result<(), String> {
        db_sql::validate_identifier("CTE name", &cte.name)?;

        if let Some(columns) = &cte.columns {
            for column in columns {
                db_sql::validate_identifier("CTE column", column)?;
            }
        }

        if cte.recursive {
            db_sql::validate_compound_subquery_sql(&cte.query_sql)
        } else {
            db_sql::validate_subquery_sql(&cte.query_sql)
        }
    }

    fn validate_query_fragments(&self) -> Result<()> {
        let qualifiers = self.known_qualifiers();
        let qualifiers = Some(&qualifiers);

        for condition in &self.conditions {
            Self::validate_condition(condition, qualifiers).map_err(Error::invalid_query)?;
        }

        for group in &self.or_groups {
            Self::validate_or_group(group, qualifiers).map_err(Error::invalid_query)?;
        }

        for (column, _) in &self.order_by {
            Self::validate_order_or_group_value("ORDER BY column", column, true, qualifiers)
                .map_err(Error::invalid_query)?;
        }

        for column in &self.group_by {
            Self::validate_order_or_group_value("GROUP BY column", column, false, qualifiers)
                .map_err(Error::invalid_query)?;
        }

        for (index, having) in self.having_conditions.iter().enumerate() {
            let bindings = self
                .having_bindings
                .get(index)
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            // Parameterized clauses are validated too: the stored template still
            // has to be a safe HAVING expression, and `?` is an accepted token
            // there, so carrying bindings is no reason to skip the check.
            db_sql::validate_having_sql_fragment("HAVING raw SQL", having)
                .map_err(Error::invalid_query)?;

            // Both HAVING renderers substitute every `?` character in the
            // template, so the placeholder count is what has to agree with the
            // bound values. A mismatch would shift PostgreSQL's `$n` numbering
            // for every later parameter or leave an unbound marker in the
            // statement, so it is rejected here rather than left to a
            // `debug_assert!` that disappears in release builds.
            let placeholder_count = having.matches('?').count();
            if placeholder_count != bindings.len() {
                return Err(Error::invalid_query(format!(
                    "HAVING clause '{}' has {} placeholder(s) but {} bound value(s)",
                    having,
                    placeholder_count,
                    bindings.len()
                )));
            }
        }

        if let Some(columns) = &self.select_columns {
            for column in columns {
                Self::validate_select_value(column, qualifiers).map_err(Error::invalid_query)?;
            }
        }

        if self.is_distinct() {
            self.validate_distinct_order_by()
                .map_err(Error::invalid_query)?;
        }

        for union in &self.unions {
            Self::validate_union_clause(union).map_err(Error::invalid_query)?;
        }

        for window_function in &self.window_functions {
            Self::validate_window_function(window_function, qualifiers)
                .map_err(Error::invalid_query)?;
        }

        for cte in &self.ctes {
            Self::validate_cte_clause(cte).map_err(Error::invalid_query)?;
        }

        Ok(())
    }

    /// Collect the table/alias qualifiers that are valid for column references
    /// in the current query: the model's own table plus the alias (or table
    /// name when no alias was provided) for each registered JOIN clause.
    pub(in crate::query) fn known_qualifiers(&self) -> BTreeSet<String> {
        let mut qualifiers = BTreeSet::new();
        qualifiers.insert(M::table_name().to_string());
        for join in &self.joins {
            if let Some(alias) = &join.alias {
                qualifiers.insert(alias.clone());
            } else {
                qualifiers.insert(join.table.clone());
            }
        }
        qualifiers
    }

    /// Create a new query builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            database: None,
            conditions: Vec::new(),
            or_groups: Vec::new(),
            order_by: Vec::new(),
            limit_value: None,
            offset_value: None,
            select_columns: None,
            raw_select_expressions: Vec::new(),
            subquery_select_expressions: Vec::new(),
            include_trashed: false,
            only_trashed: false,
            joins: Vec::new(),
            invalid_query_reason: None,
            group_by: Vec::new(),
            having_conditions: Vec::new(),
            having_bindings: Vec::new(),
            unions: Vec::new(),
            window_functions: Vec::new(),
            ctes: Vec::new(),
            cache_options: None,
            cache_key: None,
        }
    }

    #[must_use]
    pub(crate) fn with_database(mut self, database: crate::database::Database) -> Self {
        self.database = Some(database);
        self
    }

    /// Promote this query into the eager-loading builder and batch-load a relation.
    ///
    /// This preserves any filters, ordering, pagination, cache settings, and explicit
    /// database handle already attached to the query.
    #[must_use]
    pub fn with(self, relation: &str) -> crate::relations::EagerQueryBuilder<M> {
        crate::relations::EagerQueryBuilder::from_query(self).with(relation)
    }

    /// Promote this query into the eager-loading builder and batch-load multiple relations.
    ///
    /// This preserves any filters, ordering, pagination, cache settings, and explicit
    /// database handle already attached to the query.
    #[must_use]
    pub fn with_many(self, relations: &[&str]) -> crate::relations::EagerQueryBuilder<M> {
        crate::relations::EagerQueryBuilder::from_query(self).with_many(relations)
    }

    pub(super) fn current_db(&self) -> Result<crate::database::Database> {
        if let Some(database) = &self.database {
            Ok(database.clone())
        } else {
            crate::database::__current_db()
        }
    }

    /// Consolidate the current query clauses into a reusable fragment.
    ///
    /// The fragment is a verbatim snapshot of the builder's clauses, never
    /// rendered SQL. HAVING clauses in particular keep their `?` placeholders
    /// and are stored next to the values bound to them, the same way
    /// `UnionClause` and `CTE` operands carry their parameters: nothing is
    /// baked into an inline literal, so the fragment stays backend-agnostic and
    /// survives the round trip through [`apply`](Self::apply) with its
    /// parameters intact.
    ///
    /// See [`apply`](Self::apply) for how a fragment merges back into a builder.
    pub fn consolidate(&self) -> QueryFragment<M> {
        QueryFragment {
            _marker: PhantomData,
            conditions: self.conditions.clone(),
            or_groups: self.or_groups.clone(),
            order_by: self.order_by.clone(),
            limit_value: self.limit_value,
            offset_value: self.offset_value,
            select_columns: self.select_columns.clone(),
            raw_select_expressions: self.raw_select_expressions.clone(),
            subquery_select_expressions: self.subquery_select_expressions.clone(),
            group_by: self.group_by.clone(),
            having_conditions: self.having_conditions.clone(),
            having_bindings: self.having_bindings.clone(),
            joins: self.joins.clone(),
            unions: self.unions.clone(),
            window_functions: self.window_functions.clone(),
            ctes: self.ctes.clone(),
            cache_options: self.cache_options.clone(),
            cache_key: self.cache_key.clone(),
            invalid_query_reason: self.invalid_query_reason.clone(),
            include_trashed: self.include_trashed,
            only_trashed: self.only_trashed,
        }
    }

    /// Append a fragment's HAVING clauses, keeping every clause paired with the
    /// values bound to its `?` placeholders.
    ///
    /// `having_bindings` is indexed in lockstep with `having_conditions`, so the
    /// binding vector is topped up to the current clause count before appending:
    /// a fragment merged into a builder whose two vectors had drifted apart
    /// would otherwise shift the pairing for every clause that follows.
    fn extend_having_from_fragment(&mut self, fragment: &QueryFragment<M>) {
        self.having_bindings
            .resize(self.having_conditions.len(), Vec::new());

        for (index, condition) in fragment.having_conditions.iter().enumerate() {
            self.having_conditions.push(condition.clone());
            self.having_bindings.push(
                fragment
                    .having_bindings
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }

    /// Apply a reusable fragment to the current query builder.
    ///
    /// The merge replays the fragment's builder calls on top of this query, so
    /// every slot behaves exactly as the matching setter does:
    ///
    /// - **List slots are appended, never dropped**: WHERE conditions, OR
    ///   groups, ORDER BY terms, raw and subquery SELECT expressions, GROUP BY
    ///   columns, HAVING clauses (with their bound values), JOINs, compound
    ///   selects, window functions and CTEs. A fragment ordering by
    ///   `created_at` therefore adds a sort key to whatever the builder already
    ///   ordered by, exactly as a second `order_desc()` call would.
    /// - **Single-value slots are last-wins**: `limit`, `offset`, `select`,
    ///   cache options and cache key. A value the fragment carries overrides the
    ///   builder's, and a slot the fragment left unset keeps the builder's —
    ///   mirroring `.limit(5).limit(10)`.
    /// - **`distinct()` is a latch**: a fragment that requested it turns it on,
    ///   and a fragment that did not leaves the builder's own choice alone,
    ///   exactly as calling `distinct()` twice does.
    /// - **Soft-delete scope is last-wins** in the same sense: a fragment that
    ///   set neither `with_trashed()` nor `only_trashed()` leaves the scope
    ///   alone.
    /// - **`invalid_query_reason` is the sole first-wins slot**, matching
    ///   `invalidate_query()`: the earliest recorded failure is the one
    ///   reported.
    ///
    /// Nothing is silently discarded, so applying a fragment produced by
    /// [`consolidate`](Self::consolidate) to an empty builder reproduces the
    /// original query.
    #[must_use]
    pub fn apply(mut self, fragment: &QueryFragment<M>) -> Self {
        self.conditions.extend_from_slice(&fragment.conditions);
        self.or_groups.extend_from_slice(&fragment.or_groups);
        self.order_by.extend_from_slice(&fragment.order_by);

        if fragment.limit_value.is_some() {
            self.limit_value = fragment.limit_value;
        }

        if fragment.offset_value.is_some() {
            self.offset_value = fragment.offset_value;
        }

        if fragment.select_columns.is_some() {
            self.select_columns = fragment.select_columns.clone();
        }

        // The DISTINCT sentinel is a flag rather than an expression, so a
        // fragment merged into an already-distinct builder must not append a
        // second copy of it.
        let already_distinct = self.is_distinct();
        self.raw_select_expressions.extend(
            fragment
                .raw_select_expressions
                .iter()
                .filter(|expression| {
                    expression.as_str() != DISTINCT_SELECT_MARKER || !already_distinct
                })
                .cloned(),
        );
        self.subquery_select_expressions
            .extend_from_slice(&fragment.subquery_select_expressions);

        self.group_by.extend_from_slice(&fragment.group_by);
        self.extend_having_from_fragment(fragment);
        self.joins.extend_from_slice(&fragment.joins);
        self.unions.extend_from_slice(&fragment.unions);
        self.window_functions
            .extend_from_slice(&fragment.window_functions);
        self.ctes.extend_from_slice(&fragment.ctes);

        if fragment.cache_options.is_some() {
            self.cache_options = fragment.cache_options.clone();
        }

        if fragment.cache_key.is_some() {
            self.cache_key = fragment.cache_key.clone();
        }

        if self.invalid_query_reason.is_none() {
            self.invalid_query_reason = fragment.invalid_query_reason.clone();
        }

        if fragment.only_trashed {
            self.only_trashed = true;
            self.include_trashed = false;
        } else if fragment.include_trashed {
            self.include_trashed = true;
            self.only_trashed = false;
        }

        self
    }

    pub(super) fn invalidate_query(&mut self, reason: String) {
        if self.invalid_query_reason.is_none() {
            self.invalid_query_reason = Some(reason);
        }
    }

    pub(super) fn ensure_query_is_valid(&self) -> Result<()> {
        if let Some(reason) = &self.invalid_query_reason {
            return Err(Error::invalid_query(reason));
        }

        self.validate_query_fragments()?;

        Ok(())
    }

    pub(super) fn validate_join_clause(
        table: &str,
        alias: Option<&str>,
        left_column: &str,
        right_column: &str,
    ) -> std::result::Result<(), String> {
        db_sql::validate_identifier("JOIN table", table)?;

        if let Some(alias) = alias {
            db_sql::validate_identifier("JOIN alias", alias)?;
        }

        db_sql::validate_join_column(left_column)?;
        db_sql::validate_join_column(right_column)?;
        Ok(())
    }
}
