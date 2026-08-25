use super::*;

use crate::query::builder::{contains_raw_order_by_marker, raw_order_by_entry};

impl<M: Model> QueryBuilder<M> {
    /// Add an ORDER BY clause.
    ///
    /// The column must be one the model resolves, optionally qualified with a
    /// joined table or alias and optionally followed by `ASC`/`DESC`. Anything
    /// else — parentheses, operators, subselects — is rejected as an invalid
    /// query when the builder executes, because ORDER BY is rendered outside a
    /// quoted literal and is therefore a direct injection point. This makes it
    /// safe to feed a `?sort=` request parameter straight into this method.
    ///
    /// Use [`QueryBuilder::order_by_raw`] when you need a real SQL expression.
    #[must_use]
    pub fn order_by(
        mut self,
        column: impl crate::columns::IntoColumnName,
        direction: Order,
    ) -> Self {
        let column = column.column_name().to_string();

        if contains_raw_order_by_marker(&column) {
            self.invalidate_query(format!(
                "invalid ORDER BY column '{}': the raw-expression marker is reserved; use order_by_raw() for trusted SQL expressions",
                column
            ));
            return self;
        }

        self.order_by.push((column, direction));
        self
    }

    /// Add an ORDER BY clause from a raw SQL expression.
    ///
    /// **Trusted SQL only — never pass user input to this method.** The
    /// expression is rendered into the statement verbatim, so anything reaching
    /// it must be a literal or otherwise fully controlled by your code. Use
    /// [`QueryBuilder::order_by`] for anything derived from a request.
    ///
    /// The expression is still checked with the shared raw-fragment validator,
    /// so statement separators, SQL comments, and NUL bytes are rejected — but
    /// that check is a backstop, not a sanitizer.
    ///
    /// ```ignore
    /// User::query()
    ///     .order_by_raw("COALESCE(nickname, name)", Order::Asc)
    ///     .get()
    ///     .await?;
    /// ```
    #[must_use]
    pub fn order_by_raw(mut self, expression: &str, direction: Order) -> Self {
        self.order_by
            .push((raw_order_by_entry(expression), direction));
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

    /// Skip a number of results.
    ///
    /// An offset without a [`QueryBuilder::limit`] is portable: MySQL, MariaDB,
    /// and SQLite reject a bare `OFFSET`, so rendering supplies the
    /// backend-appropriate open-ended `LIMIT` for them.
    #[must_use]
    pub fn offset(mut self, n: u64) -> Self {
        self.offset_value = Some(n);
        self
    }

    /// Paginate results using 1-based page numbers.
    ///
    /// A zero page or page size, or a `(page - 1) * per_page` product that does
    /// not fit in a `u64`, invalidates the query instead of panicking in debug
    /// builds and wrapping to a bogus offset in release builds.
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

        let Some(offset) = (page - 1).checked_mul(per_page) else {
            query.invalidate_query(format!(
                "invalid pagination: page {} of {} per page overflows the maximum offset",
                page, per_page
            ));
            return query;
        };

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

#[cfg(test)]
mod tests {
    use crate::model::Model;

    #[tideorm::model(table = "pagination_users")]
    struct PaginationUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }

    #[test]
    fn test_page_rejects_an_overflowing_offset() {
        let err = PaginationUser::query()
            .page(u64::MAX, 2)
            .ensure_query_is_valid()
            .expect_err("an overflowing offset must invalidate the query");

        assert!(
            err.to_string().contains("overflows the maximum offset"),
            "{err}"
        );
    }

    #[test]
    fn test_page_rejects_zero_page_number() {
        let err = PaginationUser::query()
            .page(0, 10)
            .ensure_query_is_valid()
            .expect_err("page 0 must invalidate the query");

        assert!(err.to_string().contains("at least 1"), "{err}");
    }

    #[test]
    fn test_page_sets_limit_and_offset() {
        let query = PaginationUser::query().page(3, 25);

        assert!(query.ensure_query_is_valid().is_ok());
        assert_eq!(query.limit_value, Some(25));
        assert_eq!(query.offset_value, Some(50));
    }
}
