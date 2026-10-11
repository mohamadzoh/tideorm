use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Add a window function to the SELECT clause
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().window(WindowFunction::new(WindowFunctionType::RowNumber, "position").order_by("id", Order::Asc)).validate().unwrap();
    /// ```
    #[must_use]
    pub fn window(mut self, window_fn: WindowFunction) -> Self {
        self.clauses.window_functions.push(window_fn);
        self
    }

    /// Add `function` over an optional partition, ordered by `order_by`.
    fn ordered_window(
        self,
        function: WindowFunctionType,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut window = WindowFunction::new(function, alias).order_by(order_by, order);
        if let Some(partition) = partition_by {
            window = window.partition_by(partition);
        }
        self.window(window)
    }

    /// Add ROW_NUMBER() window function
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().row_number("position", None, "id", Order::Asc).validate().unwrap();
    /// ```
    #[must_use]
    pub fn row_number(
        self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::RowNumber,
            alias,
            partition_by,
            order_by,
            order,
        )
    }

    /// Add RANK() window function
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().rank("position", None, "id", Order::Asc).validate().unwrap();
    /// ```
    #[must_use]
    pub fn rank(
        self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::Rank,
            alias,
            partition_by,
            order_by,
            order,
        )
    }

    /// Add DENSE_RANK() window function
    ///
    /// Similar to RANK() but without gaps in ranking values.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().dense_rank("position", None, "id", Order::Asc).validate().unwrap();
    /// ```
    #[must_use]
    pub fn dense_rank(
        self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::DenseRank,
            alias,
            partition_by,
            order_by,
            order,
        )
    }

    /// Add LAG() window function
    ///
    /// Access data from a previous row.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().lag("previous_id", "id", 1, None, "active", "id", Order::Asc).validate().unwrap();
    /// ```
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn lag(
        self,
        alias: &str,
        column: &str,
        offset: i32,
        default: Option<&str>,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::Lag(
                column.to_string(),
                Some(offset),
                default.map(str::to_string),
            ),
            alias,
            Some(partition_by),
            order_by,
            order,
        )
    }

    /// Add LEAD() window function
    ///
    /// Access data from a following row.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().lead("next_id", "id", 1, None, "active", "id", Order::Asc).validate().unwrap();
    /// ```
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn lead(
        self,
        alias: &str,
        column: &str,
        offset: i32,
        default: Option<&str>,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::Lead(
                column.to_string(),
                Some(offset),
                default.map(str::to_string),
            ),
            alias,
            Some(partition_by),
            order_by,
            order,
        )
    }

    /// Add running SUM() window function
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().running_sum("total", "id", "id", Order::Asc).validate().unwrap();
    /// ```
    #[must_use]
    pub fn running_sum(self, alias: &str, column: &str, order_by: &str, order: Order) -> Self {
        self.window(
            WindowFunction::new(WindowFunctionType::Sum(column.to_string()), alias)
                .order_by(order_by, order)
                .frame(
                    FrameType::Rows,
                    FrameBound::UnboundedPreceding,
                    FrameBound::CurrentRow,
                ),
        )
    }

    /// Add NTILE() window function
    ///
    /// Distribute rows into specified number of groups.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().ntile("bucket", 4, "id", Order::Asc).validate().unwrap();
    /// ```
    #[must_use]
    pub fn ntile(self, alias: &str, buckets: u32, order_by: &str, order: Order) -> Self {
        self.ordered_window(
            WindowFunctionType::Ntile(buckets),
            alias,
            None,
            order_by,
            order,
        )
    }

    /// Add FIRST_VALUE() window function
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().first_value("first_id", "id", "active", "id", Order::Asc).validate().unwrap();
    /// ```
    #[must_use]
    pub fn first_value(
        self,
        alias: &str,
        column: &str,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::FirstValue(column.to_string()),
            alias,
            Some(partition_by),
            order_by,
            order,
        )
    }
}
