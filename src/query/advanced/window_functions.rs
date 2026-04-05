use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Add a window function to the SELECT clause
    #[must_use]
    pub fn window(mut self, window_fn: WindowFunction) -> Self {
        if let Err(reason) = Self::validate_window_function(&window_fn) {
            self.invalidate_query(reason);
        }

        self.window_functions.push(window_fn);
        self
    }

    /// Add ROW_NUMBER() window function
    #[must_use]
    pub fn row_number(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf =
            WindowFunction::new(WindowFunctionType::RowNumber, alias).order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }

    /// Add RANK() window function
    #[must_use]
    pub fn rank(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf = WindowFunction::new(WindowFunctionType::Rank, alias).order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }

    /// Add DENSE_RANK() window function
    ///
    /// Similar to RANK() but without gaps in ranking values.
    #[must_use]
    pub fn dense_rank(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf =
            WindowFunction::new(WindowFunctionType::DenseRank, alias).order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }

    /// Add LAG() window function
    ///
    /// Access data from a previous row.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn lag(
        mut self,
        alias: &str,
        column: &str,
        offset: i32,
        default: Option<&str>,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(
            WindowFunctionType::Lag(
                column.to_string(),
                Some(offset),
                default.map(|s| s.to_string()),
            ),
            alias,
        )
        .partition_by(partition_by)
        .order_by(order_by, order);

        if let Err(reason) = Self::validate_window_function(&wf) {
            self.invalidate_query(reason);
        }

        self.window_functions.push(wf);
        self
    }

    /// Add LEAD() window function
    ///
    /// Access data from a following row.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn lead(
        mut self,
        alias: &str,
        column: &str,
        offset: i32,
        default: Option<&str>,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(
            WindowFunctionType::Lead(
                column.to_string(),
                Some(offset),
                default.map(|s| s.to_string()),
            ),
            alias,
        )
        .partition_by(partition_by)
        .order_by(order_by, order);

        if let Err(reason) = Self::validate_window_function(&wf) {
            self.invalidate_query(reason);
        }

        self.window_functions.push(wf);
        self
    }

    /// Add running SUM() window function
    #[must_use]
    pub fn running_sum(mut self, alias: &str, column: &str, order_by: &str, order: Order) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Sum(column.to_string()), alias)
            .order_by(order_by, order)
            .frame(
                FrameType::Rows,
                FrameBound::UnboundedPreceding,
                FrameBound::CurrentRow,
            );
        self.window_functions.push(wf);
        self
    }

    /// Add running AVG() window function
    #[must_use]
    pub fn running_avg(mut self, alias: &str, column: &str, order_by: &str, order: Order) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Avg(column.to_string()), alias)
            .order_by(order_by, order)
            .frame(
                FrameType::Rows,
                FrameBound::UnboundedPreceding,
                FrameBound::CurrentRow,
            );
        self.window_functions.push(wf);
        self
    }

    /// Add NTILE() window function
    ///
    /// Distribute rows into specified number of groups.
    #[must_use]
    pub fn ntile(mut self, alias: &str, buckets: u32, order_by: &str, order: Order) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Ntile(buckets), alias)
            .order_by(order_by, order);
        self.window_functions.push(wf);
        self
    }

    /// Add FIRST_VALUE() window function
    #[must_use]
    pub fn first_value(
        mut self,
        alias: &str,
        column: &str,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::FirstValue(column.to_string()), alias)
            .partition_by(partition_by)
            .order_by(order_by, order);
        self.window_functions.push(wf);
        self
    }

    /// Add LAST_VALUE() window function
    /// Add LAST_VALUE() window function
    #[must_use]
    pub fn last_value(
        mut self,
        alias: &str,
        column: &str,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::LastValue(column.to_string()), alias)
            .partition_by(partition_by)
            .order_by(order_by, order)
            // Need to extend frame to see last value
            .frame(
                FrameType::Rows,
                FrameBound::UnboundedPreceding,
                FrameBound::UnboundedFollowing,
            );
        self.window_functions.push(wf);
        self
    }
}
