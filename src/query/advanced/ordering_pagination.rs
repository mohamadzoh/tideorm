use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Add an ORDER BY clause.
    #[must_use]
    pub fn order_by(
        mut self,
        column: impl crate::columns::IntoColumnName,
        direction: Order,
    ) -> Self {
        self.order_by
            .push((column.column_name().to_string(), direction));
        self
    }

    /// Order by ascending
    #[must_use]
    pub fn order_asc(self, column: impl crate::columns::IntoColumnName) -> Self {
        self.order_by(column, Order::Asc)
    }

    /// Order by descending
    #[must_use]
    pub fn order_desc(self, column: impl crate::columns::IntoColumnName) -> Self {
        self.order_by(column, Order::Desc)
    }

    /// Order by latest (created_at DESC)
    #[must_use]
    pub fn latest(self) -> Self {
        self.order_desc("created_at")
    }

    /// Order by oldest (created_at ASC)
    #[must_use]
    pub fn oldest(self) -> Self {
        self.order_asc("created_at")
    }

    // =========================================================================
    // PAGINATION
    // =========================================================================

    /// Limit the number of results
    #[must_use]
    pub fn limit(mut self, n: u64) -> Self {
        self.limit_value = Some(n);
        self
    }

    /// Skip a number of results
    #[must_use]
    pub fn offset(mut self, n: u64) -> Self {
        self.offset_value = Some(n);
        self
    }

    /// Paginate results using 1-based page numbers.
    #[must_use]
    pub fn page(self, page: u64, per_page: u64) -> Self {
        let mut query = self;

        if page == 0 {
            query.invalidate_query("invalid pagination: page must be at least 1".to_string());
            return query;
        }

        if per_page == 0 {
            query.invalidate_query(
                "invalid pagination: per_page must be greater than 0".to_string(),
            );
            return query;
        }

        let offset = (page - 1) * per_page;
        query.limit(per_page).offset(offset)
    }

    /// Take only the first N records
    #[must_use]
    pub fn take(self, n: u64) -> Self {
        self.limit(n)
    }

    /// Skip the first N records
    #[must_use]
    pub fn skip(self, n: u64) -> Self {
        self.offset(n)
    }
}
