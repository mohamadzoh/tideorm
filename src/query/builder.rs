use super::{
    CTE, ConditionValue, Operator, QueryBuilder, QueryFragment, UnionClause, WhereCondition,
    WindowFunction, WindowFunctionType, db_sql,
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

/// A column reference and the `ASC`/`DESC` written after it, if one is.
pub(in crate::query) fn split_direction(term: &str) -> Option<(&str, super::Order)> {
    let (column, direction) = term.trim().rsplit_once(char::is_whitespace)?;
    let direction = if direction.eq_ignore_ascii_case("asc") {
        super::Order::Asc
    } else if direction.eq_ignore_ascii_case("desc") {
        super::Order::Desc
    } else {
        return None;
    };
    Some((column.trim_end(), direction))
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

    /// Split a SELECT expression at its last top-level ` AS `, into the
    /// expression and its alias, each trimmed; the whole value and `None`
    /// when it has no alias.
    ///
    /// The scan tracks parenthesis depth and string/identifier quote state, so
    /// an inner `AS` inside `CAST(col AS TEXT)` is ignored and only the outer
    /// alias boundary is returned.
    pub(in crate::query) fn split_alias(value: &str) -> (&str, Option<&str>) {
        let value = value.trim();
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

        let Some((start, end)) = last_as else {
            return (value, None);
        };
        let expression = value[..start].trim();
        let alias = value[end..].trim();
        if expression.is_empty() || alias.is_empty() {
            return (value, None);
        }
        (expression, Some(alias))
    }

    fn simple_column_reference(value: &str) -> Option<(&str, &str)> {
        let (value, _) = Self::split_alias(value);
        match crate::internal::sql_safety::identifier_reference_parts(value)?.as_slice() {
            [column] => Some(("", *column)),
            [table, column] => Some((*table, *column)),
            _ => None,
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
        let (reference, direction_is_valid) = match split_direction(trimmed) {
            Some((reference, _)) => (reference, allow_raw_and_direction),
            None => (trimmed, true),
        };

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

    /// Validate one `select()` column.
    ///
    /// Like ORDER BY and GROUP BY, the typed projection is an allowlist: a
    /// column the model resolves (optionally table-qualified and aliased), or a
    /// `*` / `table.*` wildcard. A blocklist cannot make an expression safe here
    /// — `(SELECT password FROM users LIMIT 1)` contains no forbidden token yet
    /// reads another table — so expressions go through `select_raw()`, whose
    /// callers vouch for them.
    fn validate_select_value(
        value: &str,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        let trimmed = value.trim();
        let (expression, alias) = Self::split_alias(trimmed);

        if alias.is_none() {
            if expression == "*" {
                return Ok(());
            }
            if let Some(qualifier) = expression.strip_suffix(".*") {
                return db_sql::validate_identifier("SELECT wildcard qualifier", qualifier);
            }
        }

        if db_sql::validate_identifier_reference("SELECT column", expression).is_err() {
            return Err(format!(
                "unsafe SELECT column '{}': select() takes a column, table.column or `column AS alias`; use select_raw() for trusted SQL expressions and never pass user input to it",
                trimmed
            ));
        }
        Self::validate_model_column_reference("SELECT column", expression, known_qualifiers)?;

        match alias {
            Some(alias) => db_sql::validate_identifier("SELECT alias", alias),
            None => Ok(()),
        }
    }

    /// Refuse to build models from a `select()` that leaves model columns out.
    ///
    /// The engine reads an unselected `Option` column as `None`, so such a model
    /// looks complete, and saving it writes those `None`s over the stored
    /// values. A projection with raw SQL in it is the caller's to vouch for.
    ///
    /// Over a join, a wildcard that reaches another table is refused too: its
    /// columns come back under the same names as the model's (`id`,
    /// `created_at`), and the model would be filled from whichever came last.
    pub(in crate::query) fn ensure_projection_covers_model(&self) -> Result<()> {
        let Some(columns) = &self.clauses.select_columns else {
            return Ok(());
        };
        if self.raw_projection().next().is_some() {
            return Ok(());
        }

        let own_wildcard = format!("{}.*", M::table_name());
        let mut covers_everything = false;
        for column in columns {
            let column = column.trim();
            if column == own_wildcard || (column == "*" && self.clauses.joins.is_empty()) {
                covers_everything = true;
            } else if column.ends_with('*') {
                return Err(Error::query(format!(
                    "select(\"{}\") also reads a joined table's columns, which get() would fill '{}' from wherever a name repeats; select \"{}\", and read other tables' columns with get_json()",
                    column,
                    M::table_name(),
                    own_wildcard
                )));
            }
        }
        if covers_everything {
            return Ok(());
        }

        let mut covered = BTreeSet::new();
        for column in columns {
            let column = column.trim();
            let output = match Self::split_alias(column).1 {
                Some(alias) => alias,
                None => match column.rsplit_once('.') {
                    // Another table's column fills none of the model's.
                    Some((table, _)) if table != M::table_name() => continue,
                    Some((_, name)) => name,
                    None => column,
                },
            };
            if let Some(name) = M::canonical_column_name(output) {
                covered.insert(name);
            }
        }

        let missing: Vec<&str> = M::column_names()
            .iter()
            .copied()
            .filter(|column| !covered.contains(column))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        Err(Error::query(format!(
            "select() leaves out {} of '{}', which get() would fill with defaults that a later save() writes back; select every column, or read the partial rows with get_json()",
            missing.join(", "),
            M::table_name()
        )))
    }

    /// Each named result column of the projection, with the table and column
    /// it is read from: its alias when it has one, otherwise the column's own
    /// name.
    ///
    /// A plain column is read from its qualifier's table, or the model's when
    /// unqualified; an expression, a `select_subquery()` among them, has no
    /// source. Wildcards and unaliased expressions name no column and are
    /// left out.
    pub(in crate::query) fn projection_outputs(&self) -> Vec<(String, Option<(String, String)>)> {
        let typed = self.clauses.select_columns.iter().flatten();
        let raw = self.raw_projection();
        let subqueries = self
            .clauses
            .subquery_select_expressions
            .iter()
            .map(|subquery| (subquery.alias.clone(), None));
        typed
            .chain(raw)
            .filter_map(|entry| {
                let entry = entry.trim();
                let (expression, alias) = Self::split_alias(entry);
                let source = Self::simple_column_reference(expression);
                let name = match alias {
                    Some(alias) => alias.trim_matches(|c| c == '"' || c == '`').to_string(),
                    None => {
                        let (_, column) = source?;
                        M::canonical_column_name(column)
                            .unwrap_or(column)
                            .to_string()
                    }
                };
                let source = source.map(|(table, column)| {
                    let table = match table {
                        "" => M::table_name(),
                        table => table,
                    };
                    (table.to_string(), column.to_string())
                });
                Some((name, source))
            })
            .chain(subqueries)
            .collect()
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
    fn validate_projection_names(&self) -> std::result::Result<(), String> {
        let outputs = self.projection_outputs();
        for (index, (name, _)) in outputs.iter().enumerate() {
            if outputs[..index].iter().any(|(earlier, _)| earlier == name) {
                return Err(format!(
                    "the projection names the column '{}' twice, and a result row keeps one value per name; alias one of them, as in `joined_table.{} AS joined_{}`",
                    name, name, name
                ));
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
            M::column_named(column)
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
        let has_opaque_projection = self.raw_projection().next().is_some()
            || !self.clauses.subquery_select_expressions.is_empty();

        if has_opaque_projection {
            return None;
        }

        let table = M::table_name();
        let mut references = BTreeSet::new();

        // An absent or empty column list renders as the `table.*` fallback,
        // which exposes every column of the model.
        let Some(columns) = self
            .clauses
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
            let (expression, alias) = Self::split_alias(column);

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

        for (column, _) in &self.clauses.order_by {
            if raw_order_by_expression(column).is_some() {
                continue;
            }

            let trimmed = column.trim();
            let reference = split_direction(trimmed).map_or(trimmed, |(reference, _)| reference);

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
            (Operator::Raw, ConditionValue::RawTemplate { sql, values }) => {
                db_sql::validate_raw_sql_fragment("WHERE raw SQL", sql)?;
                let placeholders = db_sql::count_template_placeholders(sql);
                if placeholders == values.len() {
                    Ok(())
                } else {
                    Err(format!(
                        "WHERE raw SQL '{}' has {} placeholders for {} values",
                        sql,
                        placeholders,
                        values.len()
                    ))
                }
            }
            _ => Ok(()),
        }?;

        // The column slot is rendered as SQL, so it is an allowlist like ORDER BY:
        // anything but a plain or table-qualified column would be spliced in
        // verbatim, and `1=1 OR name` turns `where_eq(.., x)` into a filter that
        // matches every row.
        if !condition.column.is_empty() {
            if db_sql::validate_identifier_reference("WHERE column", &condition.column).is_err() {
                return Err(format!(
                    "unsafe WHERE column '{}': expected a column or table.column reference; use where_raw() for trusted SQL expressions and never pass user input to it",
                    condition.column
                ));
            }
            Self::validate_model_column_reference(
                "WHERE column",
                &condition.column,
                known_qualifiers,
            )?;
        }

        // The other column of a column comparison is rendered the same way.
        if let ConditionValue::Column(other) = &condition.value {
            if db_sql::validate_identifier_reference("WHERE column", other).is_err() {
                return Err(format!(
                    "unsafe WHERE comparison column '{}': expected a column or table.column reference",
                    other
                ));
            }
            Self::validate_model_column_reference("WHERE column", other, known_qualifiers)?;
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

        let function = &window_function.function;
        if let Some(column) = function.column() {
            Self::validate_model_column_reference(
                "window function column",
                column,
                known_qualifiers,
            )?;
        }
        match function {
            WindowFunctionType::Lag(_, _, Some(default))
            | WindowFunctionType::Lead(_, _, Some(default)) => {
                db_sql::validate_raw_sql_fragment("LAG/LEAD default expression", default)?;
            }
            WindowFunctionType::Custom(expression) => {
                db_sql::validate_raw_sql_fragment("window function expression", expression)?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Fail this query when `operand`, a query it embeds through `method`,
    /// could not run on its own; `what` names the operand in the message.
    pub(in crate::query) fn absorb_operand_error<N: Model>(
        &mut self,
        what: &str,
        method: &str,
        operand: &QueryBuilder<N>,
    ) {
        if let Err(err) = operand.ensure_query_is_executable() {
            self.invalidate_query(format!("invalid {what} for {method}(): {err}"));
        }
    }

    pub(super) fn validate_cte_clause(cte: &CTE) -> std::result::Result<(), String> {
        db_sql::validate_identifier("CTE name", &cte.name)?;

        if let Some(columns) = &cte.columns {
            for column in columns {
                db_sql::validate_identifier("CTE column", column)?;
            }
        }

        // A CTE's body is parenthesized, so a UNION inside it stays inside it.
        db_sql::validate_compound_subquery_sql(&cte.query_sql)
    }

    fn validate_query_fragments(&self) -> Result<()> {
        let qualifiers = self.known_qualifiers();
        let qualifiers = Some(&qualifiers);

        for condition in self.all_conditions() {
            Self::validate_condition(condition, qualifiers).map_err(Error::query)?;
        }

        for (column, _) in &self.clauses.order_by {
            Self::validate_order_or_group_value("ORDER BY column", column, true, qualifiers)
                .map_err(Error::query)?;
        }

        for column in &self.clauses.group_by {
            Self::validate_order_or_group_value("GROUP BY column", column, false, qualifiers)
                .map_err(Error::query)?;
        }

        for (having, bindings) in self.having_clauses() {
            // Parameterized clauses are validated too: the stored template still
            // has to be a safe HAVING expression, and `?` is an accepted token
            // there, so carrying bindings is no reason to skip the check.
            db_sql::validate_having_sql_fragment("HAVING raw SQL", having).map_err(Error::query)?;

            // The HAVING renderer substitutes every `?` of the template outside
            // quotes, so that count is what has to agree with the bound values. A mismatch would shift PostgreSQL's `$n` numbering
            // for every later parameter or leave an unbound marker in the
            // statement, so it is rejected here rather than left to a
            // `debug_assert!` that disappears in release builds.
            let placeholder_count = db_sql::count_template_placeholders(having);
            if placeholder_count != bindings.len() {
                return Err(Error::query(format!(
                    "HAVING clause '{}' has {} placeholder(s) but {} bound value(s)",
                    having,
                    placeholder_count,
                    bindings.len()
                )));
            }
        }

        if let Some(columns) = &self.clauses.select_columns {
            for column in columns {
                Self::validate_select_value(column, qualifiers).map_err(Error::query)?;
            }
        }
        self.validate_projection_names().map_err(Error::query)?;

        if self.is_distinct() {
            self.validate_distinct_order_by().map_err(Error::query)?;
        }

        for union in &self.clauses.unions {
            Self::validate_union_clause(union).map_err(Error::query)?;
        }

        for window_function in &self.clauses.window_functions {
            Self::validate_window_function(window_function, qualifiers).map_err(Error::query)?;
        }

        for cte in &self.clauses.ctes {
            Self::validate_cte_clause(cte).map_err(Error::query)?;
        }

        Ok(())
    }

    /// Collect the table/alias qualifiers that are valid for column references
    /// in the current query: the model's own table plus the alias (or table
    /// name when no alias was provided) for each registered JOIN clause.
    pub(in crate::query) fn known_qualifiers(&self) -> BTreeSet<String> {
        let mut qualifiers = BTreeSet::new();
        qualifiers.insert(M::table_name().to_string());
        // A `schema.table` join is referenced by its table name.
        for join in &self.clauses.joins {
            qualifiers.insert(join.qualifier().to_string());
        }
        qualifiers
    }

    /// Create a new query builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            database: None,
            clauses: super::clauses::Clauses::default(),
            self_join_depth: 0,
        }
    }

    #[must_use]
    pub(crate) fn with_database(mut self, database: crate::database::Database) -> Self {
        self.database = Some(database);
        self
    }

    /// Start a bulk `UPDATE` of the rows this query matches, keeping its
    /// filters, its soft-delete scope and the database a
    /// [`query_with`](crate::model::Model::query_with) handle names:
    ///
    /// ```ignore
    /// User::query()
    ///     .inactive() // a scope
    ///     .where_lt("last_login_at", cutoff)
    ///     .update_all()
    ///     .set("status", "dormant")
    ///     .execute()
    ///     .await?;
    /// ```
    ///
    /// Unlike [`Model::update_all`], trashed
    /// rows stay out unless the query includes them. A query that joins,
    /// groups, pages or unions cannot be an `UPDATE`, and running one fails.
    #[must_use]
    pub fn update_all(self) -> crate::model::BatchUpdateBuilder<M> {
        crate::model::BatchUpdateBuilder::from_query(self)
    }

    /// The first part of this query an `UPDATE` cannot keep, if any: one
    /// that reshapes the rows rather than filters them.
    pub(crate) fn update_blocker(&self) -> Option<&'static str> {
        [
            (!self.clauses.joins.is_empty(), "a join"),
            (!self.clauses.unions.is_empty(), "a union"),
            (!self.clauses.ctes.is_empty(), "a CTE"),
            (!self.clauses.group_by.is_empty(), "group_by()"),
            (!self.clauses.having_conditions.is_empty(), "having()"),
            (
                !self.clauses.window_functions.is_empty(),
                "a window function",
            ),
            (
                self.clauses.limit_value.is_some() || self.clauses.offset_value.is_some(),
                "limit() or offset()",
            ),
        ]
        .into_iter()
        .find_map(|(present, part)| present.then_some(part))
    }

    /// The first part of this query a `DELETE` or a trash mutation cannot
    /// keep: an [`update_blocker`](Self::update_blocker), a projection or an
    /// ordering.
    pub(crate) fn mutation_blocker(&self) -> Option<&'static str> {
        self.update_blocker().or_else(|| {
            [
                (self.has_explicit_projection(), "a select()"),
                (self.is_distinct(), "distinct()"),
                (!self.clauses.order_by.is_empty(), "an ordering"),
            ]
            .into_iter()
            .find_map(|(present, part)| present.then_some(part))
        })
    }

    /// This query over live rows only, whatever scope it had.
    pub(crate) fn live_rows_only(mut self) -> Self {
        self.clauses.include_trashed = false;
        self.clauses.only_trashed = false;
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

    /// The database a `query_with` handle named, if one did.
    pub(crate) fn named_database(&self) -> Option<crate::database::Database> {
        self.database.clone()
    }

    /// The database the query runs on: the one a `query_with` handle named,
    /// else the scope's.
    pub(crate) fn current_db(&self) -> Result<crate::database::Database> {
        self.named_database()
            .map_or_else(crate::database::__current_db, Ok)
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
            clauses: self.clauses.clone(),
        }
    }

    /// Each HAVING clause paired with the values bound to its `?` placeholders.
    pub(in crate::query) fn having_clauses(
        &self,
    ) -> impl Iterator<Item = (&str, &[crate::internal::Value])> {
        self.clauses
            .having_conditions
            .iter()
            .enumerate()
            .map(|(index, clause)| {
                let bindings = self
                    .clauses
                    .having_bindings
                    .get(index)
                    .map_or(&[][..], Vec::as_slice);
                (clause.as_str(), bindings)
            })
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
    ///   ordered by, exactly as a second `order_desc()` call would. The
    ///   fragment's `or_where_*` conditions join the builder's own shared OR
    ///   group, exactly as those calls would.
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
        self.clauses.merge(&fragment.clauses);
        self
    }

    pub(super) fn invalidate_query(&mut self, reason: String) {
        if self.clauses.invalid_query_reason.is_none() {
            self.clauses.invalid_query_reason = Some(reason);
        }
    }

    pub(super) fn ensure_query_is_valid(&self) -> Result<()> {
        if let Some(reason) = &self.clauses.invalid_query_reason {
            return Err(Error::query(reason));
        }
        if let Some((field, message)) = &self.clauses.invalid_page {
            return Err(Error::validation(*field, message.clone()));
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
        if db_sql::validate_identifier_reference("JOIN table", table).is_err() {
            return Err(format!(
                "unsafe JOIN table '{}': expected a table or schema.table using only ASCII letters, numbers, and underscores",
                table
            ));
        }

        if let Some(alias) = alias {
            db_sql::validate_identifier("JOIN alias", alias)?;
        }

        db_sql::validate_join_column(left_column)?;
        db_sql::validate_join_column(right_column)?;
        Ok(())
    }
}
