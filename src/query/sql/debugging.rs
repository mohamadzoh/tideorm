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
            Operator::Contains => "CONTAINS",
            Operator::StartsWith => "STARTS_WITH",
            Operator::EndsWith => "ENDS_WITH",
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
        }
    }

    /// `value` as a description shows it; with `redact`, a value the caller
    /// bound is `?`, since an error context reaches logs.
    fn described_value(value: &ConditionValue, redact: bool) -> String {
        match value {
            ConditionValue::Single(_)
            | ConditionValue::List(_)
            | ConditionValue::Range(..)
            | ConditionValue::Subquery(_)
                if redact =>
            {
                "?".to_string()
            }
            value => value.to_string(),
        }
    }

    fn describe_condition(condition: &WhereCondition, redact: bool) -> String {
        let value = Self::described_value(&condition.value, redact);
        match (&condition.operator, &condition.value) {
            (
                Operator::Raw,
                ConditionValue::RawExpr(_)
                | ConditionValue::RawTemplate { .. }
                | ConditionValue::Subquery(_),
            ) => {
                if condition.column.is_empty() {
                    value
                } else {
                    format!("{} {}", condition.column, value)
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
                value
            ),
        }
    }

    /// A HAVING clause as `db_type` writes it, its values left as markers:
    /// an error context reaches logs.
    pub(in crate::query) fn describe_having_clause(
        &self,
        db_type: DatabaseType,
        clause: &crate::query::HavingClause,
    ) -> String {
        self.having_bound(db_type, clause)
            .build(db_type.dialect())
            .0
    }

    /// The backend a preview renders for: the one the query runs on, or
    /// PostgreSQL's dialect for a query with no database to run on.
    fn preview_db_type(&self) -> DatabaseType {
        self.db_type_for_sql().unwrap_or(DatabaseType::Postgres)
    }

    fn describe_or_group(group: &OrGroup, redact: bool) -> String {
        let mut parts: Vec<String> = group
            .conditions
            .iter()
            .map(|condition| Self::describe_condition(condition, redact))
            .collect();
        parts.extend(
            group
                .nested_groups
                .iter()
                .map(|nested| Self::describe_or_group(nested, redact)),
        );

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

    /// The description of each OR group that holds a condition.
    fn described_or_groups(&self, redact: bool) -> impl Iterator<Item = String> + '_ {
        self.clauses
            .or_groups
            .iter()
            .map(move |group| Self::describe_or_group(group, redact))
            .filter(|group| !group.is_empty())
    }

    pub(crate) fn build_query_error_context(
        &self,
        query: Option<&str>,
    ) -> crate::error::ErrorContext {
        let conditions: Vec<String> = self
            .clauses
            .conditions
            .iter()
            .map(|condition| Self::describe_condition(condition, true))
            .collect();
        let groups: Vec<String> = self.described_or_groups(true).collect();
        let db_type = self.preview_db_type();
        let having: Vec<String> = self
            .clauses
            .having_conditions
            .iter()
            .map(|clause| self.describe_having_clause(db_type, clause))
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
    pub(super) fn start_query_log(sql: &str) -> Option<crate::logging::QueryTimer> {
        if crate::logging::query_logging_enabled() {
            crate::tide_debug!("Query: {}", sql);
        }

        if crate::logging::QueryLogger::is_enabled() {
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

        crate::logging::QueryLogger::log(entry);
    }

    /// Describe this query: its preview and parameterized SQL, bound values,
    /// and the clauses it was built from.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// let info = User::query().where_eq(User::ACTIVE, true).debug();
    /// println!("{info:?}");
    /// ```
    pub fn debug(&self) -> crate::logging::QueryDebugInfo {
        self.debug_for(self.preview_db_type())
    }

    /// [`debug()`](Self::debug) in `db_type`'s dialect: its parameterized SQL
    /// has that backend's markers (`$1` or `?`) and quoting.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, active: bool }
    /// let info = User::query().where_eq(User::ACTIVE, true).debug_for(DatabaseType::SQLite);
    /// assert_eq!(info.params.len(), 1);
    /// ```
    pub fn debug_for(&self, db_type: DatabaseType) -> crate::logging::QueryDebugInfo {
        use crate::logging::QueryDebugInfo;

        let (sql, params, render_error) = match self.build_select_sql_with_params_for_db(db_type) {
            Ok((sql, params)) => (sql, params, None),
            Err(error) => (String::new(), Vec::new(), Some(error.to_string())),
        };
        let preview = db_sql::inline_parameters(db_type, &sql, &params);

        let mut info = QueryDebugInfo::new(M::table_name()).with_sql(format!(
            "{}\n{}\n-- PARAMETERIZED SQL\n{}",
            PREVIEW_BANNER, preview, sql
        ));
        info.params = params
            .into_iter()
            .map(|value| format!("{:?}", value))
            .collect();

        for condition in self
            .clauses
            .conditions
            .iter()
            .map(|condition| Self::describe_condition(condition, false))
            .chain(self.described_or_groups(false))
        {
            info.add_condition(condition);
        }
        info.error = self
            .validate()
            .err()
            .map(|error| error.to_string())
            .or(render_error);

        for (column, direction) in &self.clauses.order_by {
            info.add_order_by(format!("{} {}", column, direction.as_str()));
        }

        info.group_by = self.clauses.group_by.clone();
        info.limit = self.clauses.limit_value;
        info.offset = self.clauses.offset_value;

        if !self.clauses.raw_select_expressions.is_empty()
            || !self.clauses.subquery_select_expressions.is_empty()
        {
            info.select = self.clauses.raw_select_expressions.clone();
            info.select.extend(
                self.clauses
                    .subquery_select_expressions
                    .iter()
                    .map(|subquery| format!("(subquery) AS {}", subquery.alias)),
            );
        } else if let Some(columns) = &self.clauses.select_columns {
            info.select = columns.clone();
        }

        for join in &self.clauses.joins {
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
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().where_eq(User::ACTIVE, true).validate().unwrap();
    /// ```
    pub fn validate(&self) -> Result<()> {
        self.ensure_query_is_executable()
    }

    /// The statement [`get()`](Self::get) would run, with its bound values
    /// written in as literals, under a banner marking it display-only. A
    /// query with no database to run on is shown in PostgreSQL's dialect.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// let sql = User::query().where_eq(User::ACTIVE, true).build_sql_preview();
    /// assert!(sql.contains("SELECT"));
    /// ```
    pub fn build_sql_preview(&self) -> String {
        self.build_sql_preview_for(self.preview_db_type())
    }

    /// [`build_sql_preview()`](Self::build_sql_preview) in `db_type`'s
    /// dialect, whatever database the query would run on.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, active: bool }
    /// let sql = User::query().where_eq(User::ACTIVE, true).build_sql_preview_for(DatabaseType::MySQL);
    /// assert!(sql.contains("`users`"));
    /// ```
    pub fn build_sql_preview_for(&self, db_type: DatabaseType) -> String {
        format!(
            "{}\n{}",
            PREVIEW_BANNER,
            self.build_select_sql_for_db(db_type)
        )
    }
}
