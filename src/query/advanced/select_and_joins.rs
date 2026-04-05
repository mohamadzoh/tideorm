use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Select specific columns
    #[must_use]
    pub fn select(mut self, columns: Vec<&str>) -> Self {
        self.select_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Select columns from this table and also from a linked/joined table
    ///
    /// Use this for partial model queries that need columns from a related
    /// table without loading the full related model.
    #[must_use]
    pub fn select_with_linked(
        mut self,
        local_columns: Vec<&str>,
        linked_table: &str,
        local_fk: &str,
        remote_pk: &str,
        linked_columns: Vec<&str>,
    ) -> Self {
        // Set local columns with table prefix
        let table_name = M::table_name();
        let mut all_columns: Vec<String> = local_columns
            .iter()
            .map(|c| format!("{}.{}", table_name, c))
            .collect();

        // Add linked columns with table prefix
        for col in linked_columns {
            all_columns.push(format!("{}.{}", linked_table, col));
        }

        self.select_columns = Some(all_columns);

        // Add the join
        self.joins.push(JoinClause {
            join_type: JoinType::Left,
            table: linked_table.to_string(),
            alias: None,
            left_column: format!("{}.{}", table_name, local_fk),
            right_column: format!("{}.{}", linked_table, remote_pk),
        });

        self
    }

    /// Select all columns from this table plus specific columns from a linked table
    #[must_use]
    pub fn select_also_linked(
        mut self,
        linked_table: &str,
        local_pk: &str,
        remote_fk: &str,
        linked_columns: Vec<&str>,
    ) -> Self {
        let table_name = M::table_name();

        // Start with all local columns
        let local_cols: Vec<String> = M::column_names()
            .iter()
            .map(|c| format!("{}.{}", table_name, c))
            .collect();

        // Add linked columns
        let mut all_columns = local_cols;
        for col in linked_columns {
            all_columns.push(format!("{}.{}", linked_table, col));
        }

        self.select_columns = Some(all_columns);

        // Add the join
        self.joins.push(JoinClause {
            join_type: JoinType::Left,
            table: linked_table.to_string(),
            alias: None,
            left_column: format!("{}.{}", table_name, local_pk),
            right_column: format!("{}.{}", linked_table, remote_fk),
        });

        self
    }

    // =========================================================================
    // JOIN OPERATIONS
    // =========================================================================

    /// Add an INNER JOIN clause
    ///
    /// Returns only rows with matches in both tables.
    #[must_use]
    pub fn inner_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Inner, table, None, left_column, right_column)
    }

    /// Add an INNER JOIN clause with an alias
    #[must_use]
    pub fn inner_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        self.join(
            JoinType::Inner,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Add a LEFT JOIN clause
    ///
    /// Returns all rows from the left table, and matched rows from the right.
    #[must_use]
    pub fn left_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Left, table, None, left_column, right_column)
    }

    /// Add a LEFT JOIN clause with an alias
    #[must_use]
    pub fn left_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        self.join(
            JoinType::Left,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Add a RIGHT JOIN clause
    ///
    /// Returns all rows from the right table, and matched rows from the left.
    #[must_use]
    pub fn right_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Right, table, None, left_column, right_column)
    }

    /// Add a RIGHT JOIN clause with an alias
    #[must_use]
    pub fn right_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        self.join(
            JoinType::Right,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Generic join method (internal)
    fn join(
        mut self,
        join_type: JoinType,
        table: &str,
        alias: Option<&str>,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        if let Err(reason) = Self::validate_join_clause(table, alias, left_column, right_column) {
            self.invalidate_query(reason);
            return self;
        }

        self.joins.push(JoinClause {
            join_type,
            table: table.to_string(),
            alias: alias.map(|s| s.to_string()),
            left_column: left_column.to_string(),
            right_column: right_column.to_string(),
        });
        self
    }
}
