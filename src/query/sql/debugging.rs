use super::*;

#[allow(missing_docs)]
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
            Operator::JsonContains => "JSON_CONTAINS",
            Operator::JsonContainedBy => "JSON_CONTAINED_BY",
            Operator::JsonKeyExists => "JSON_KEY_EXISTS",
            Operator::JsonKeyNotExists => "JSON_KEY_NOT_EXISTS",
            Operator::JsonPathExists => "JSON_PATH_EXISTS",
            Operator::JsonPathNotExists => "JSON_PATH_NOT_EXISTS",
            Operator::ArrayContains => "ARRAY_CONTAINS",
            Operator::ArrayContainedBy => "ARRAY_CONTAINED_BY",
            Operator::ArrayOverlaps => "ARRAY_OVERLAPS",
            Operator::ArrayContainsAny => "ARRAY_CONTAINS_ANY",
            Operator::ArrayContainsAll => "ARRAY_CONTAINS_ALL",
            Operator::SubqueryIn => "IN SUBQUERY",
            Operator::SubqueryNotIn => "NOT IN SUBQUERY",
            Operator::Raw => "RAW",
            Operator::EqAny => "= ANY",
            Operator::NeAll => "<> ALL",
        }
    }

    fn describe_condition_value(value: &ConditionValue) -> String {
        match value {
            ConditionValue::Single(value) => value.to_string(),
            ConditionValue::List(values) => format!("{:?}", values),
            ConditionValue::Range(low, high) => format!("{}..{}", low, high),
            ConditionValue::None => "NULL".to_string(),
            ConditionValue::Subquery(query_sql) => query_sql.clone(),
            ConditionValue::RawExpr(raw_sql) => raw_sql.clone(),
        }
    }

    fn describe_condition(condition: &WhereCondition) -> String {
        match (&condition.operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => raw_sql.clone(),
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
                Self::describe_condition_value(&condition.value)
            ),
        }
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

    fn error_context_conditions(&self) -> Vec<String> {
        let mut conditions: Vec<String> = self
            .conditions
            .iter()
            .map(Self::describe_condition)
            .collect();
        conditions.extend(
            self.or_groups
                .iter()
                .map(Self::describe_or_group)
                .filter(|group| !group.is_empty()),
        );
        conditions.extend(
            self.having_conditions
                .iter()
                .map(|having| format!("HAVING {}", having)),
        );
        conditions
    }

    fn error_context_operator_chain(&self) -> Option<String> {
        let mut parts = Vec::new();

        if !self.conditions.is_empty() {
            parts.push(
                self.conditions
                    .iter()
                    .map(Self::describe_condition)
                    .collect::<Vec<_>>()
                    .join(" AND "),
            );
        }

        parts.extend(
            self.or_groups
                .iter()
                .map(Self::describe_or_group)
                .filter(|group| !group.is_empty()),
        );

        if !self.having_conditions.is_empty() {
            parts.push(format!("HAVING {}", self.having_conditions.join(" AND ")));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" AND "))
        }
    }

    pub(crate) fn build_query_error_context(
        &self,
        query: Option<String>,
    ) -> crate::error::ErrorContext {
        let mut context = crate::error::ErrorContext::new()
            .table(M::table_name())
            .conditions(self.error_context_conditions());

        if let Some(operator_chain) = self.error_context_operator_chain() {
            context = context.operator_chain(operator_chain);
        }

        if let Some(query) = query {
            context = context.query(query);
        }

        context
    }

    pub(super) fn log_query(&self, sql: &str) {
        if std::env::var("TIDE_LOG_QUERIES")
            .map(|value| value.eq_ignore_ascii_case("true") || value == "1")
            .unwrap_or(false)
        {
            crate::tide_debug!("Query: {}", sql);
        }

        if crate::logging::QueryLogger::is_enabled() {
            let entry = crate::logging::QueryLogEntry::new(sql).with_table(M::table_name());
            crate::logging::QueryLogger::log(entry);
        }
    }

    pub fn debug(&self) -> crate::logging::QueryDebugInfo {
        use crate::logging::QueryDebugInfo;

        let (parameterized_sql, params) = self.build_select_sql_with_params();
        let preview_sql = self.build_sql_preview();
        let mut info = QueryDebugInfo::new(M::table_name()).with_sql(preview_sql.clone());
        info.params = params
            .into_iter()
            .map(|value| format!("{:?}", value))
            .collect();

        for condition in &self.conditions {
            info.add_condition(Self::describe_condition(condition));
        }

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
                    .map(|(query_sql, alias)| format!("({}) AS {}", query_sql, alias)),
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

        if !parameterized_sql.is_empty() {
            info.sql = format!(
                "{}\n-- PARAMETERIZED SQL\n{}",
                preview_sql, parameterized_sql
            );
        }

        info
    }

    pub fn build_sql_preview(&self) -> String {
        self.build_sql_preview_for_db(self.db_type_for_sql())
    }

    pub(crate) fn build_sql_preview_for_db(&self, db_type: DatabaseType) -> String {
        format!(
            "-- DEBUG PREVIEW (not executable, values are approximate)\n{}",
            self.build_select_sql_for_db(db_type)
        )
    }
}
