//! MySQL's and MariaDB's catalog statements, on `information_schema`.

use super::Queries;

pub(super) const QUERIES: Queries = Queries {
    tables: "SELECT table_name AS name FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_type IN ('BASE TABLE', 'SYSTEM VERSIONED') ORDER BY table_name",
    views: "SELECT table_name AS name FROM information_schema.views WHERE table_schema = DATABASE() ORDER BY table_name",
    sequences: Some(
        "SELECT table_name AS name FROM information_schema.tables \
         WHERE table_schema = DATABASE() AND table_type = 'SEQUENCE' ORDER BY table_name",
    ),
    enum_types: None,
    table_exists: "SELECT COUNT(*) > 0 AS present FROM information_schema.tables \
                   WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ?",
    columns: "SELECT column_name AS name, column_type AS data_type, is_nullable = 'YES' AS nullable, \
              column_default AS default_value, column_key = 'PRI' AS primary_key, NULLIF(extra, '') AS extra \
              FROM information_schema.columns WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ? \
              ORDER BY ordinal_position",
    // A functional key part (MySQL 8.0.13) has no column name; a prefix
    // part has a `sub_part`.
    indexes: "SELECT index_name AS name, column_name AS column_name, non_unique = 0 AS is_unique, \
              index_name = 'PRIMARY' AS is_primary, \
              column_name IS NOT NULL AND sub_part IS NULL AS whole_column FROM information_schema.statistics \
              WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ? ORDER BY index_name, seq_in_index",
    foreign_keys: "SELECT column_name AS column_name, referenced_table_name AS references_table, \
                   referenced_column_name AS references_column FROM information_schema.key_column_usage \
                   WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ? AND referenced_table_name IS NOT NULL \
                   ORDER BY constraint_name, ordinal_position",
    index_table: None,
};

/// Whether a table has an index: binds the database (`NULL` for the
/// connection's own), the table and the index.
pub(super) const INDEX_EXISTS: &str = "SELECT COUNT(*) > 0 AS present FROM information_schema.statistics \
     WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ? AND index_name = ?";
