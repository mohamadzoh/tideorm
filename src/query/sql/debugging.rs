use super::*;

/// The banner every SQL preview starts with.
const PREVIEW_BANNER: &str = "-- DEBUG PREVIEW (not executable, values are approximate)";

impl<M: Model> QueryBuilder<M> {
    fn operator_label(operator: &Operator) -> &'static str {
        match operator {
            Operator::Eq => "=",
            Operator::NotEq => "!=",
            Operator::Gt => ">",
            Operator::Gte => ">=",
            Operator::Lt => "<",
            Operator::Lte => "<=",
            Operator::Like => "LIKE",
            Operator::LikeEscaped => "LIKE",
            Operator::NotLike => "NOT LIKE",
            Operator::In => "IN",
            Operator::NotIn => "NOT IN",
            Operator::IsNull => "IS NULL",
            Operator::IsNotNull => "IS NOT NULL",
            Operator::Between => "BETWEEN",
            Operator::NotBetween => "NOT BETWEEN",
            Operator::JsonContains => "JSON_CONTAINS",
            Operator::JsonContainedBy => "JSON_CONTAINED_BY",
            Operator::JsonKeyExists => "JSON_KEY_EXISTS",
            Operator::JsonKeyNotExists => "JSON_KEY_NOT_EXISTS",
            Operator::JsonPathExists => "JSON_PATH_EXISTS",
            Operator::JsonPathNotExists => "JSON_PATH_NOT_EXISTS",
            Operator::ArrayContains => "ARRAY_CONTAINS",
            Operator::ArrayContainedBy => "ARRAY_CONTAINED_BY",
            Operator::ArrayOverlaps => "ARRAY_OVERLAPS",
            Operator::Raw => "RAW",
            Operator::EqAny => "= ANY",
            Operator::NeAll => "<> ALL",
        }
    }

    fn describe_condition(condition: &WhereCondition) -> String {
        match (&condition.operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(sql))
            | (Operator::Raw, ConditionValue::RawExprWithValues { sql, .. })
            | (Operator::Raw, ConditionValue::RawTemplate { sql, .. }) => {
                if condition.column.is_empty() {
                    sql.clone()
                } else {
                    format!("{} {}", condition.column, sql)
                }
            }
            (Operator::IsNull | Operator::IsNotNull, ConditionValue::None) => {
                format!(
                    "{} {}",
                    condition.column,
                    Self::operator_label(&condition.operator)
                )
            }
            _ => format!(
                "{} {} {}",
                condition.column,
                Self::operator_label(&condition.operator),
                condition.value
            ),
        }
    }

    /// A HAVING template with each `?` replaced by the JSON value bound to it.
    fn describe_having_clause(template: &str, bindings: &[crate::internal::Value]) -> String {
        let mut values = bindings.iter();
        db_sql::map_template_placeholders(template, || match values.next() {
            // The value as the statement preview writes it.
            Some(value) => {
                db_sql::inline_parameters(DatabaseType::Postgres, "$1", std::slice::from_ref(value))
            }
            None => "?".to_string(),
        })
    }

    fn describe_or_group(group: &OrGroup) -> String {
        let mut parts: Vec<String> = group
            .conditions
            .iter()
            .map(Self::describe_condition)
            .collect();
        parts.extend(group.nested_groups.iter().map(Self::describe_or_group));

        if parts.is_empty() {
            String::new()
        } else if parts.len() == 1 {
            parts[0].clone()
        } else {
            format!(
                "({})",
                parts.join(&format!(" {} ", group.combine_with.as_sql()))
            )
        }
    }

    pub(crate) fn build_query_error_context(
        &self,
        query: Option<&str>,
    ) -> crate::error::ErrorContext {
        let conditions: Vec<String> = self
            .conditions
            .iter()
            .map(Self::describe_condition)
            .collect();
        let groups: Vec<String> = self
            .or_groups
            .iter()
            .map(Self::describe_or_group)
            .filter(|group| !group.is_empty())
            .collect();
        let having: Vec<String> = self
            .having_clauses()
            .map(|(template, bindings)| Self::describe_having_clause(template, bindings))
            .collect();

        let mut operator_chain = Vec::new();
        if !conditions.is_empty() {
            operator_chain.push(conditions.join(" AND "));
        }
        operator_chain.extend(groups.iter().cloned());
        if !having.is_empty() {
            operator_chain.push(format!("HAVING {}", having.join(" AND ")));
        }

        let mut context = crate::error::ErrorContext::new().table(M::table_name());
        if !operator_chain.is_empty() {
            context = context.operator_chain(operator_chain.join(" AND "));
        }
        context = context.conditions(
            conditions
                .into_iter()
                .chain(groups)
                .chain(
                    having
                        .into_iter()
                        .map(|clause| format!("HAVING {}", clause)),
                )
                .collect(),
        );

        if let Some(query) = query {
            context = context.query(query);
        }

        context
    }

    /// Emit the pre-execution trace and start measuring this statement.
    ///
    /// The returned timer must be handed to [`Self::finish_query_log`] once the
    /// statement has completed, so the recorded entry carries the real elapsed
    /// time and the real outcome instead of a `None` duration and an assumed
    /// success. `None` means nothing is observing this query — two atomic loads
    /// decide that, and no timer or SQL copy is allocated.
    pub(super) fn start_query_log(&self, sql: &str) -> Option<crate::logging::QueryTimer> {
        if crate::logging::query_logging_enabled() {
            crate::tide_debug!("Query: {}", sql);
        }

        if crate::logging::QueryLogger::is_enabled()
            || crate::cache::PreparedStatementCache::global().is_enabled()
        {
            Some(crate::logging::QueryTimer::start(sql).with_table(M::table_name()))
        } else {
            None
        }
    }

    /// Record a finished statement with its measured duration and real outcome.
    ///
    /// A failed statement is recorded as a failure, which is what makes
    /// `LogLevel::Error`/`Warn` emit anything at all, and the measured duration
    /// is what feeds the slow-query threshold and the aggregate timing counters.
    /// The same measurement is handed to the prepared-statement cache so its
    /// statistics describe real traffic.
    pub(super) fn finish_query_log<T>(
        timer: Option<crate::logging::QueryTimer>,
        result: &Result<T>,
        row_count: impl FnOnce(&T) -> u64,
    ) {
        let Some(timer) = timer else {
            return;
        };

        let entry = match result {
            Ok(value) => timer.finish_with_rows(row_count(value)),
            Err(error) => timer.finish_with_error(error.to_string()),
        };

        if let Some(duration) = entry.duration {
            crate::cache::PreparedStatementCache::global()
                .observe_execution(&entry.sql, duration.as_micros() as u64);
        }

        crate::logging::QueryLogger::log(entry);
    }

    /// Describe this query: its preview and parameterized SQL, bound values,
    /// and the clauses it was built from.
    pub fn debug(&self) -> crate::logging::QueryDebugInfo {
        use crate::logging::QueryDebugInfo;

        let db_type = self.db_type_for_sql();
        let (sql, params) = self.build_select_sql_with_params_for_db(db_type);
        let preview = db_sql::inline_parameters(db_type, &sql, &params);

        let mut info = QueryDebugInfo::new(M::table_name()).with_sql(format!(
            "{}\n{}\n-- PARAMETERIZED SQL\n{}",
            PREVIEW_BANNER, preview, sql
        ));
        info.params = params
            .into_iter()
            .map(|value| format!("{:?}", value))
            .collect();

        for condition in &self.conditions {
            info.add_condition(Self::describe_condition(condition));
        }
        for group in &self.or_groups {
            let group = Self::describe_or_group(group);
            if !group.is_empty() {
                info.add_condition(group);
            }
        }
        info.error = self.validate().err().map(|error| error.to_string());

        for (column, direction) in &self.order_by {
            info.add_order_by(format!("{} {}", column, direction.as_str()));
        }

        info.group_by = self.group_by.clone();
        info.limit = self.limit_value;
        info.offset = self.offset_value;

        if !self.raw_select_expressions.is_empty() || !self.subquery_select_expressions.is_empty() {
            info.select = self.raw_select_expressions.clone();
            info.select.extend(
                self.subquery_select_expressions
                    .iter()
                    .map(|subquery| format!("({}) AS {}", subquery.query_sql, subquery.alias)),
            );
        } else if let Some(columns) = &self.select_columns {
            info.select = columns.clone();
        }

        for join in &self.joins {
            info.joins.push(format!(
                "{:?} JOIN {} ON {} = {}",
                join.join_type, join.table, join.left_column, join.right_column
            ));
        }

        info
    }

    /// Check the query without running it: `Err` with the reason when a
    /// terminal would refuse it — an unsafe column, a bad raw fragment, a
    /// zero page — which is otherwise reported only when it runs.
    ///
    /// ```ignore
    /// let query = Post::query().order_by(params.sort.as_str(), Order::Asc);
    /// query.validate()?; // answer 400 before touching the database
    /// ```
    pub fn validate(&self) -> Result<()> {
        self.ensure_query_is_executable()
    }

    /// The statement [`get()`](Self::get) would run, with its bound values
    /// written in as literals, under a banner marking it display-only.
    pub fn build_sql_preview(&self) -> String {
        self.build_sql_preview_for_db(self.db_type_for_sql())
    }

    pub(crate) fn build_sql_preview_for_db(&self, db_type: DatabaseType) -> String {
        format!(
            "{}\n{}",
            PREVIEW_BANNER,
            self.build_select_sql_for_db(db_type)
        )
    }
}
