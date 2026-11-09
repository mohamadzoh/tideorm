//! The column references a query names: resolved against the model, split
//! from their aliases, and refused when they do not resolve.

use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(super) fn known_model_column_references() -> (&'static str, String) {
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

    /// How this query names `column`: another model's typed column is
    /// qualified with its table.
    pub(in crate::query) fn own_column(column: &impl crate::columns::IntoColumnName) -> String {
        crate::columns::column_reference(column, crate::columns::model_table::<M>())
    }

    pub(in crate::query) fn simple_column_reference(value: &str) -> Option<(&str, &str)> {
        let (value, _) = Self::split_alias(value);
        match crate::internal::sql_safety::identifier_reference_parts(value)?.as_slice() {
            [column] => Some(("", *column)),
            [table, column] => Some((*table, *column)),
            [_, _, column] => {
                let table = value.rsplit_once('.')?.0;
                Some((
                    if M::owns_table_reference(table) {
                        M::table_name()
                    } else {
                        table
                    },
                    *column,
                ))
            }
            _ => None,
        }
    }

    pub(super) fn validate_model_column_reference(
        kind: &str,
        value: &str,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        let Some((table, column)) = Self::simple_column_reference(value) else {
            return Ok(());
        };

        let reference = if table.is_empty() {
            std::borrow::Cow::Borrowed(column)
        } else {
            std::borrow::Cow::Owned(format!("{}.{}", table, column))
        };
        db_sql::validate_identifier_reference(kind, &reference)?;

        if table.is_empty() || M::owns_table_reference(table) {
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
    pub(in crate::query) fn validate_order_or_group_value(
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
    pub(super) fn validate_select_value(
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
    /// values. A raw expression's alias counts as the column it names; a raw
    /// expression without one, such as a wildcard, may hold any column and is
    /// the caller's to vouch for.
    ///
    /// Over a join, a wildcard that reaches another table is refused too: its
    /// columns come back under the same names as the model's (`id`,
    /// `created_at`), and the model would be filled from whichever came last.
    pub(in crate::query) fn ensure_projection_covers_model(&self) -> Result<()> {
        let columns = self.clauses.select_columns.as_deref().unwrap_or_default();
        if columns.is_empty()
            && self.raw_projection().next().is_none()
            && self.clauses.subquery_select_expressions.is_empty()
        {
            return Ok(());
        }
        let mut raw_aliases = Vec::new();
        for expression in self.raw_projection() {
            match Self::split_alias(expression).1 {
                Some(alias) => raw_aliases.push(alias),
                None => return Ok(()),
            }
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
                // Aliases are literal result names, not Rust field references.
                // The model decoder looks up the database column name.
                Some(alias) => {
                    if M::column_names().contains(&alias) {
                        covered.insert(alias);
                    }
                    continue;
                }
                None => match column.rsplit_once('.') {
                    // Another table's column fills none of the model's.
                    Some((table, _)) if !M::owns_table_reference(table) => continue,
                    Some((_, name)) => name,
                    None => column,
                },
            };
            if let Some(name) = M::canonical_column_name(output) {
                covered.insert(name);
            }
        }

        // Raw expressions and scalar subqueries suppress the default model
        // projection too. Their aliases can complete an explicit projection,
        // but cannot hide omissions.
        for alias in raw_aliases {
            if M::column_names().contains(&alias) {
                covered.insert(alias);
            }
        }
        for subquery in &self.clauses.subquery_select_expressions {
            if M::column_names().contains(&subquery.alias.as_str()) {
                covered.insert(subquery.alias.as_str());
            }
        }
        for window in &self.clauses.window_functions {
            if M::column_names().contains(&window.alias.as_str()) {
                covered.insert(window.alias.as_str());
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
}
