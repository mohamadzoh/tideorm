//! SQLite's catalog statements, on its main database.

use super::Queries;

pub(super) const QUERIES: Queries = Queries {
    // Names starting `sqlite_` (in any case) are SQLite's own.
    tables: r"SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\_%' ESCAPE '\' ORDER BY name",
    views: "SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name",
    sequences: None,
    enum_types: None,
    table_exists: "SELECT COUNT(*) > 0 AS present FROM sqlite_master WHERE type = 'table' AND name = ? COLLATE NOCASE",
    // `table_xinfo` lists generated columns too; `hidden = 1` marks a
    // virtual table's hidden ones.
    columns: "SELECT name, type AS data_type, \"notnull\" = 0 AS nullable, dflt_value AS default_value, \
              pk > 0 AS primary_key, \
              CASE hidden WHEN 2 THEN 'VIRTUAL GENERATED' WHEN 3 THEN 'STORED GENERATED' END AS extra \
              FROM pragma_table_xinfo(?) WHERE hidden <> 1 ORDER BY cid",
    indexes: "SELECT l.name AS name, i.name AS column_name, l.\"unique\" AS is_unique, l.origin = 'pk' AS is_primary, \
              i.name IS NOT NULL AS whole_column FROM pragma_index_list(?) AS l, pragma_index_info(l.name) AS i ORDER BY l.name, i.seqno",
    // A key written `REFERENCES parent` names no column: it is the parent's
    // primary key, column by column.
    foreign_keys: "SELECT f.\"from\" AS column_name, f.\"table\" AS references_table, \
                   COALESCE(f.\"to\", (SELECT p.name FROM pragma_table_info(f.\"table\") AS p WHERE p.pk = f.seq + 1)) AS references_column \
                   FROM pragma_foreign_key_list(?) AS f ORDER BY f.id, f.seq",
    index_table: Some(
        "SELECT tbl_name AS name FROM main.sqlite_master WHERE type = 'index' AND name = ? COLLATE NOCASE",
    ),
};
