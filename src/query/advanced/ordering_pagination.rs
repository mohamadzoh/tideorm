use super::*;

use crate::query::builder::{contains_raw_order_by_marker, raw_order_by_entry, split_direction};

impl<M: Model> QueryBuilder<M> {
    /// Add an ORDER BY clause.
    ///
    /// The column must be one the model resolves, optionally qualified with a
    /// joined table or alias and optionally followed by `ASC`/`DESC`. Anything
    /// else — parentheses, operators, subselects — is rejected as an invalid
    /// query when the builder executes, because ORDER BY is rendered outside a
    /// quoted literal and is therefore a direct injection point.
    ///
    /// A direction written after the column has to agree with `direction`:
    /// `order_by("name desc", Order::Asc)` names two, and fails the query. Take
    /// a request's sort string with [`sort()`](Self::sort) instead.
    ///
    /// Use [`QueryBuilder::order_by_raw`] when you need a real SQL expression.
    #[must_use]
    pub fn order_by(
        mut self,
        column: impl crate::columns::IntoColumnName,
        direction: Order,
    ) -> Self {
        let column = crate::columns::column_reference(&column, Some(M::table_name()));

        if contains_raw_order_by_marker(&column) {
            self.invalidate_query(format!(
                "invalid ORDER BY column '{}': the raw-expression marker is reserved; use order_by_raw() for trusted SQL expressions",
                column
            ));
            return self;
        }

        if let Some((_, written)) = split_direction(&column)
            && written != direction
        {
            self.invalidate_query(format!(
                "order_by('{}', Order::{:?}) names two directions; pass the direction once, or sort() a request's sort string",
                column, direction
            ));
            return self;
        }

        self.clauses.order_by.push((column, direction));
        self
    }

    /// Order by a sort string such as a request's `?sort=` parameter: terms
    /// separated by commas, each a column written as `order_by` takes it and,
    /// optionally, a direction — `name`, `name desc`, or `-created_at`, a
    /// leading `-` meaning descending and `+` ascending.
    ///
    /// ```ignore
    /// // ?sort=-created_at,name
    /// let posts = Post::query().sort(&params.sort).get().await?;
    /// ```
    ///
    /// Every column is checked as `order_by` checks one, so an expression in
    /// the string fails the query; an empty string orders nothing.
    #[must_use]
    pub fn sort(mut self, spec: &str) -> Self {
        for term in spec
            .split(',')
            .map(str::trim)
            .filter(|term| !term.is_empty())
        {
            let (column, direction) = if let Some(column) = term.strip_prefix('-') {
                (column.trim(), Order::Desc)
            } else if let Some(column) = term.strip_prefix('+') {
                (column.trim(), Order::Asc)
            } else {
                split_direction(term).unwrap_or((term, Order::Asc))
            };
            if column.is_empty() {
                self.invalidate_query(format!("sort('{}') has a term with no column", spec));
                return self;
            }
            self = self.order_by(column, direction);
        }
        self
    }

    /// Replace every ordering the query has so far — one a scope added, say —
    /// with this one, as [`order_by`](Self::order_by) takes it.
    #[must_use]
    pub fn reorder(
        mut self,
        column: impl crate::columns::IntoColumnName,
        direction: Order,
    ) -> Self {
        self.clauses.order_by.clear();
        self.order_by(column, direction)
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
        self.clauses
            .order_by
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

    /// Limit the number of results
    #[must_use]
    pub fn limit(mut self, n: u64) -> Self {
        self.clauses.limit_value = Some(n);
        self
    }

    /// Skip a number of results.
    ///
    /// An offset without a [`QueryBuilder::limit`] is portable: MySQL, MariaDB,
    /// and SQLite reject a bare `OFFSET`, so rendering supplies the
    /// backend-appropriate open-ended `LIMIT` for them.
    #[must_use]
    pub fn offset(mut self, n: u64) -> Self {
        self.clauses.offset_value = Some(n);
        self
    }

    /// Paginate results using 1-based page numbers.
    ///
    /// Give the query a unique order, such as one ending in the primary key:
    /// without one PostgreSQL returns rows in heap order, which an UPDATE
    /// changes, so consecutive pages can repeat one row and skip another.
    ///
    /// A zero page or page size, or a page whose offset passes `i64::MAX`, fails
    /// the query with the validation error
    /// [`Model::paginate`] gives the same
    /// numbers, naming `page` or `per_page` — what a request handler reports as
    /// bad input.
    #[must_use]
    pub fn page(mut self, page: u64, per_page: u64) -> Self {
        match page_offset(page, per_page) {
            Ok(offset) => self.limit(per_page).offset(offset),
            Err(refusal) => {
                if self.clauses.invalid_page.is_none() {
                    self.clauses.invalid_page = Some(refusal);
                }
                self
            }
        }
    }
}

/// The offset of page `page` (1-based) of `per_page` rows, or the field it
/// refuses and why: a zero page or page size, or a page or size past what the
/// backends take (`i64::MAX`). `page()` and `Model::paginate` share it, so the
/// same numbers are refused alike.
pub(crate) fn page_offset(
    page: u64,
    per_page: u64,
) -> std::result::Result<u64, (&'static str, String)> {
    if page == 0 {
        return Err(("page", "must be at least 1".to_string()));
    }
    if per_page == 0 {
        return Err(("per_page", "must be greater than 0".to_string()));
    }
    if i64::try_from(per_page).is_err() {
        return Err(("per_page", "must be at most i64::MAX".to_string()));
    }
    (page - 1)
        .checked_mul(per_page)
        .filter(|offset| i64::try_from(*offset).is_ok())
        .ok_or_else(|| {
            (
                "page",
                "page is too large for this page size; (page - 1) * per_page exceeds i64::MAX"
                    .to_string(),
            )
        })
}

#[cfg(test)]
#[path = "../../../tests/unit/query_ordering_pagination_tests.rs"]
mod tests;
