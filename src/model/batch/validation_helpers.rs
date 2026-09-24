use super::*;

use crate::internal::sql_safety::is_safe_identifier_segment;

impl<M: Model> BatchUpdateBuilder<M> {
    pub(crate) fn validate_update_column(column: &str) -> Result<()> {
        if is_safe_identifier_segment(column) && M::column_from_str(column).is_some() {
            Ok(())
        } else {
            Err(Error::invalid_query(format!(
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
        let canonical_column = M::canonical_column_name(column).unwrap_or(column);
        Ok(quote_ident(db_type, canonical_column))
    }

    pub(crate) fn validate_json_path(path: &str) -> Result<Vec<&str>> {
        let stripped = path.strip_prefix("$.").ok_or_else(|| {
            Error::invalid_query(format!(
                "unsafe JSON path '{}': only $.field or $.field.subfield paths are supported",
                path
            ))
        })?;

        let segments: Vec<&str> = stripped.split('.').collect();
        if !segments
            .iter()
            .all(|segment| is_safe_identifier_segment(segment))
        {
            return Err(Error::invalid_query(format!(
                "unsafe JSON path '{}': only simple identifier segments are supported",
                path
            )));
        }

        Ok(segments)
    }

    pub(crate) fn postgres_json_path_literal(segments: &[&str]) -> String {
        format!(
            "{{{}}}",
            segments
                .iter()
                .map(|segment| format!("\"{}\"", segment))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}
