//! Terminals that read rows in a shape other than the model: a caller's own
//! type, one column, one value, or a page with its total.

use super::*;

/// One page of a query's models, with the count of every matching row, as
/// [`QueryBuilder::paginate`] reads it. Serializes as
/// `{"items": [..], "total": .., "page": .., "per_page": .., "last_page": ..}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Paginated<M> {
    /// The models on this page.
    pub items: Vec<M>,
    /// How many rows match the query across every page.
    pub total: u64,
    /// This page's 1-based number.
    pub page: u64,
    /// The most models a page holds.
    pub per_page: u64,
}

impl<M> Paginated<M> {
    /// The number of the last page; `1` when nothing matches, so page 1 always
    /// exists.
    pub fn last_page(&self) -> u64 {
        match self.per_page {
            0 => 1,
            per_page => Ord::max(self.total.div_ceil(per_page), 1),
        }
    }

    /// Whether a page follows this one.
    pub fn has_next_page(&self) -> bool {
        self.page < self.last_page()
    }
}

impl<M: serde::Serialize> serde::Serialize for Paginated<M> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut page = serializer.serialize_struct("Paginated", 5)?;
        page.serialize_field("items", &self.items)?;
        page.serialize_field("total", &self.total)?;
        page.serialize_field("page", &self.page)?;
        page.serialize_field("per_page", &self.per_page)?;
        page.serialize_field("last_page", &self.last_page())?;
        page.end()
    }
}

impl<M: Model> QueryBuilder<M> {
    /// The row with primary key `id` among the rows this query matches, or
    /// `None`. Unlike [`Model::find`], the query's
    /// filters and soft-delete scope apply, so a trashed row is found only when
    /// the query includes it:
    ///
    /// ```ignore
    /// let post = Post::query().where_eq("author_id", me).find(post_id).await?;
    /// ```
    ///
    /// A composite key is the tuple `Model::find` takes.
    pub async fn find(self, id: M::PrimaryKey) -> Result<Option<M>> {
        self.where_primary_key(&id)?.first().await
    }

    /// [`find()`](Self::find), with a not-found error when no row matches.
    pub async fn find_or_fail(self, id: M::PrimaryKey) -> Result<M> {
        self.find(id).await?.ok_or_else(|| {
            Error::not_found(format!(
                "No {} found with that key matching the query",
                M::table_name()
            ))
        })
    }

    /// Filter by primary key, one condition per key column; a key whose shape
    /// does not match the declared columns is refused.
    pub(crate) fn where_primary_key(mut self, primary_key: &M::PrimaryKey) -> Result<Self> {
        if !self.clauses.unions.is_empty() {
            return Err(Error::query(format!(
                "a primary-key lookup on a union of {} would filter its first query only; filter each query of the union instead",
                M::table_name()
            )));
        }
        let values = match serde_json::to_value(primary_key)
            .map_err(|e| Error::conversion(format!("Failed to serialize primary key: {}", e)))?
        {
            serde_json::Value::Array(values) => values,
            value => vec![value],
        };

        let columns = M::primary_key_names();
        if values.len() != columns.len() {
            return Err(Error::query(format!(
                "Primary key value for {} did not match declared key columns",
                M::table_name()
            )));
        }

        for (column, value) in columns.iter().zip(values) {
            self = self.where_eq(*column, value);
        }

        Ok(self)
    }

    /// Read each row as `T`, which names the columns or their aliases as its
    /// fields: the shape for a join, a `select_raw()` or an aggregate that no
    /// model has.
    ///
    /// ```ignore
    /// #[derive(Deserialize)]
    /// struct AuthorPosts { author: String, posts: i64 }
    ///
    /// let rows: Vec<AuthorPosts> = Post::query()
    ///     .inner_join("users", "posts.user_id", "users.id")
    ///     .select_raw("users.name AS author, COUNT(*) AS posts")
    ///     .group_by("users.name")
    ///     .get_as()
    ///     .await?;
    /// ```
    ///
    /// The model's own columns arrive as its JSON has them, as in
    /// [`get_json()`](Self::get_json).
    pub async fn get_as<T: serde::de::DeserializeOwned>(self) -> Result<Vec<T>> {
        self.get_json()
            .await?
            .into_iter()
            .map(|row| {
                serde_json::from_value(row).map_err(|error| {
                    Error::conversion(format!(
                        "get_as() could not read a {} row as {}: {error}",
                        M::table_name(),
                        std::any::type_name::<T>()
                    ))
                })
            })
            .collect()
    }

    /// One column of every matching row, read as `T`, in the query's order.
    ///
    /// ```ignore
    /// let emails: Vec<String> = User::query().where_eq("active", true).pluck("email").await?;
    /// ```
    ///
    /// It replaces the query's `select()`; `distinct()` still applies, so
    /// `.distinct().pluck("country")` lists each country once. On a
    /// `union()`, whose other queries keep the projection they were given,
    /// the column is read off the union's rows.
    pub async fn pluck<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Vec<T>> {
        let column = crate::columns::column_reference(&column, Some(M::table_name()));
        let union_distinct = (!self.clauses.unions.is_empty()).then(|| self.is_distinct());
        let query = match union_distinct {
            None => self.select(vec![column.as_str()]),
            Some(_) => self,
        };
        // The row names the column by its output name; other projections the
        // query carries, such as `select_raw()`, come back beside it.
        let output = query.derived_output_name(&column).ok_or_else(|| {
            Error::query(format!(
                "pluck() on a union() reads a column the query selects, and '{}' is not one of them; select() it",
                column
            ))
        })?;
        let rows = query.get_json().await?;
        let mut seen = std::collections::HashSet::new();
        rows.into_iter()
            .filter(|row| {
                union_distinct != Some(true)
                    || seen.insert(row.get(output.as_str()).map(ToString::to_string))
            })
            .map(|mut row| {
                let value = row
                    .get_mut(output.as_str())
                    .map(serde_json::Value::take)
                    .ok_or_else(|| {
                        Error::query(format!(
                            "pluck() found no '{}' column in the row {}",
                            output, row
                        ))
                    })?;
                serde_json::from_value(value).map_err(|error| {
                    Error::conversion(format!(
                        "pluck() could not read {}.{} as {}: {error}",
                        M::table_name(),
                        column,
                        std::any::type_name::<T>()
                    ))
                })
            })
            .collect()
    }

    /// The column of the first matching row, read as `T`; `None` when no row
    /// matches.
    ///
    /// ```ignore
    /// let newest: Option<String> = Post::query().latest().value("title").await?;
    /// ```
    pub async fn value<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        Ok(self.limit(1).pluck(column).await?.into_iter().next())
    }

    /// The models of page `page` (1-based) with `per_page` to a page, and how
    /// many rows match across every page — two statements, one for the rows
    /// and one for the count.
    ///
    /// ```ignore
    /// let page = Post::query().where_eq("published", true).order_desc("id").paginate(2, 20).await?;
    /// println!("page {} of {}", page.page, page.last_page());
    /// ```
    ///
    /// Give the query a unique order, as for [`page()`](Self::page). A zero
    /// page or page size is refused like `page()` refuses it.
    pub async fn paginate(self, page: u64, per_page: u64) -> Result<Paginated<M>> {
        let rows = self.clone().page(page, per_page);
        // `page()` checks the numbers; its verdict comes before any statement.
        rows.ensure_query_is_executable()?;
        let total = self.count().await?;
        let items = rows.get().await?;
        Ok(Paginated {
            items,
            total,
            page,
            per_page,
        })
    }
}
