use super::*;
use crate::columns::{IntoColumnName, IntoColumnNames};

/// The shorthands below all take their arguments in one order: the output
/// column's alias, the function's own arguments, the `PARTITION BY` columns
/// (one column, a tuple or array of them, or `()` for none), and the window's
/// `ORDER BY` column and direction. For more than that (a compound order, a
/// frame, a `LAG` default), build a [`WindowFunction`] and pass it to
/// [`window`](QueryBuilder::window).
impl<M: Model> QueryBuilder<M> {
    /// Add a window function to the SELECT clause
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().window(WindowFunction::new(WindowFunctionType::RowNumber, "position").order_by("id", Order::Asc)).validate().unwrap();
    /// ```
    pub fn window(mut self, window_fn: WindowFunction) -> Self {
        self.clauses.window_functions.push(window_fn);
        self
    }

    /// Add `function` over `partition_by`, ordered by `order_by`.
    fn ordered_window(
        self,
        function: WindowFunctionType,
        alias: &str,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
        order: Order,
    ) -> Self {
        self.window(
            WindowFunction::new(function, alias)
                .partition_by(partition_by)
                .order_by(order_by, order),
        )
    }

    /// Add `ROW_NUMBER()`: each row's 1-based position in its partition.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().row_number("position", (), "id", Order::Asc).validate().unwrap();
    /// User::query().row_number("position", User::ACTIVE, User::ID, Order::Asc).validate().unwrap();
    /// ```
    pub fn row_number(
        self,
        alias: &str,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
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

    /// Add `RANK()`: tied rows share a rank, and the next rank skips the gap.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().rank("position", (), "id", Order::Asc).validate().unwrap();
    /// ```
    pub fn rank(
        self,
        alias: &str,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
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

    /// Add `DENSE_RANK()`: as [`rank`](Self::rank), without gaps.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().dense_rank("position", (), "id", Order::Asc).validate().unwrap();
    /// ```
    pub fn dense_rank(
        self,
        alias: &str,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
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

    /// Add `LAG(column, offset)`: the column's value `offset` rows earlier,
    /// NULL where there is none. For another value there, use
    /// [`WindowFunctionType::lag_with_raw_default`].
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().lag("previous_id", "id", 1, "active", "id", Order::Asc).validate().unwrap();
    /// ```
    pub fn lag(
        self,
        alias: &str,
        column: impl IntoColumnName,
        offset: u32,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::lag(column, offset),
            alias,
            partition_by,
            order_by,
            order,
        )
    }

    /// Add `LEAD(column, offset)`: the column's value `offset` rows later,
    /// NULL where there is none. For another value there, use
    /// [`WindowFunctionType::lead_with_raw_default`].
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().lead("next_id", "id", 1, "active", "id", Order::Asc).validate().unwrap();
    /// ```
    pub fn lead(
        self,
        alias: &str,
        column: impl IntoColumnName,
        offset: u32,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::lead(column, offset),
            alias,
            partition_by,
            order_by,
            order,
        )
    }

    /// Add a running `SUM(column)`: the sum over the partition's rows up to
    /// and including this one.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().running_sum("total", "id", (), "id", Order::Asc).validate().unwrap();
    /// ```
    pub fn running_sum(
        self,
        alias: &str,
        column: impl IntoColumnName,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
        order: Order,
    ) -> Self {
        self.window(
            WindowFunction::new(WindowFunctionType::sum(column), alias)
                .partition_by(partition_by)
                .order_by(order_by, order)
                .frame(
                    FrameType::Rows,
                    FrameBound::UnboundedPreceding,
                    FrameBound::CurrentRow,
                ),
        )
    }

    /// Add `NTILE(buckets)`: spread the partition's rows over `buckets`
    /// groups as evenly as they divide.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().ntile("bucket", 4, (), "id", Order::Asc).validate().unwrap();
    /// ```
    pub fn ntile(
        self,
        alias: &str,
        buckets: u32,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::Ntile(buckets),
            alias,
            partition_by,
            order_by,
            order,
        )
    }

    /// Add `FIRST_VALUE(column)`: the column's value in the partition's first
    /// row.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().first_value("first_id", "id", "active", "id", Order::Asc).validate().unwrap();
    /// ```
    pub fn first_value(
        self,
        alias: &str,
        column: impl IntoColumnName,
        partition_by: impl IntoColumnNames,
        order_by: impl IntoColumnName,
        order: Order,
    ) -> Self {
        self.ordered_window(
            WindowFunctionType::first_value(column),
            alias,
            partition_by,
            order_by,
            order,
        )
    }
}
