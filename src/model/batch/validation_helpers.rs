use super::*;

use crate::internal::sql_safety::is_safe_identifier_segment;

impl<M: Model> BatchUpdateBuilder<M> {
    pub(crate) fn validate_update_column(column: &str) -> Result<()> {
        if is_safe_identifier_segment(column) && M::column_from_str(column).is_some() {
            Ok(())
        } else {
            Err(Error::query(format!(
                "unsafe update column '{}': batch updates require a known model field/column name using only ASCII letters, numbers, and underscores",
                column
            )))
        }
    }

    pub(crate) fn quote_update_column(
        column: &str,
        db_type: crate::config::DatabaseType,
    ) -> Result<String> {
        Self::validate_update_column(column)?;
        let canonical_column = M::column_named(column);
        Ok(quote_ident(db_type, canonical_column))
    }
}
