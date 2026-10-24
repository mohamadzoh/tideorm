use super::*;

mod comparison_pattern_list;
mod dispatch;
mod range_json_array;
mod sea_condition_helpers;

/// Whether a column of `column_type` holds JSON, which compares as documents,
/// not text.
fn is_json_type(column_type: Option<&crate::orm::ColumnType>) -> bool {
    matches!(
        column_type,
        Some(crate::orm::ColumnType::Json | crate::orm::ColumnType::JsonBinary)
    )
}
