use super::{ColumnDefinition, ColumnType, DatabaseType, TableBuilder};

impl TableBuilder {
    pub(crate) fn build_create(&self) -> String {
        self.build_create_internal(false)
    }

    pub(crate) fn build_create_if_not_exists(&self) -> String {
        self.build_create_internal(true)
    }

    fn build_create_internal(&self, if_not_exists: bool) -> String {
        let exists_clause = if if_not_exists { "IF NOT EXISTS " } else { "" };
        let mut sql = format!(
            "CREATE TABLE {}{} (\n",
            exists_clause,
            self.quote_identifier(&self.name)
        );

        let column_defs: Vec<String> = self
            .columns
            .iter()
            .map(|column| self.build_column_def(column))
            .collect();

        sql.push_str(&column_defs.join(",\n"));

        if let Some(primary_key) = &self.primary_key {
            sql.push_str(",\n");
            sql.push_str(&format!(
                "    PRIMARY KEY ({})",
                self.quote_identifier(primary_key)
            ));
        }

        if let Some(composite_primary_key) = &self.composite_primary_key {
            sql.push_str(",\n");
            let columns: Vec<String> = composite_primary_key
                .columns
                .iter()
                .map(|column| self.quote_identifier(column))
                .collect();
            sql.push_str(&format!("    PRIMARY KEY ({})", columns.join(", ")));
        }

        for constraint in &self.unique_constraints {
            sql.push_str(",\n");
            let columns: Vec<String> = constraint
                .columns
                .iter()
                .map(|column| self.quote_identifier(column))
                .collect();
            if let Some(name) = &constraint.name {
                sql.push_str(&format!(
                    "    CONSTRAINT {} UNIQUE ({})",
                    self.quote_identifier(name),
                    columns.join(", ")
                ));
            } else {
                sql.push_str(&format!("    UNIQUE ({})", columns.join(", ")));
            }
        }

        sql.push_str("\n)");
        sql
    }

    pub(super) fn build_column_def(&self, column: &ColumnDefinition) -> String {
        let mut definition = format!(
            "    {} {}",
            self.quote_identifier(&column.name),
            self.type_to_sql(&column.column_type)
        );

        if column.auto_increment {
            match self.database_type {
                DatabaseType::Postgres => {
                    definition = match column.column_type {
                        ColumnType::Integer => {
                            format!("    {} SERIAL", self.quote_identifier(&column.name))
                        }
                        _ => format!("    {} BIGSERIAL", self.quote_identifier(&column.name)),
                    };
                }
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    definition.push_str(" AUTO_INCREMENT");
                }
                DatabaseType::SQLite => {}
            }
        }

        if !column.nullable && !column.primary_key {
            definition.push_str(" NOT NULL");
        }

        if let Some(default) = &column.default {
            definition.push_str(&format!(" DEFAULT {}", default));
        }

        if column.unique && !column.primary_key {
            definition.push_str(" UNIQUE");
        }

        if let Some(check) = &column.check {
            definition.push_str(&format!(" CHECK ({})", check));
        }

        if let Some(extra) = &column.extra {
            definition.push_str(&format!(" {}", extra));
        }

        definition
    }

    pub(crate) fn build_indexes(&self) -> Vec<String> {
        self.build_indexes_internal(false)
    }

    pub(crate) fn build_indexes_if_not_exists(&self) -> Vec<String> {
        self.build_indexes_internal(true)
    }

    fn build_indexes_internal(&self, if_not_exists: bool) -> Vec<String> {
        let exists_clause = if if_not_exists { "IF NOT EXISTS " } else { "" };
        self.indexes
            .iter()
            .map(|index| {
                let index_type = if index.unique {
                    "UNIQUE INDEX"
                } else {
                    "INDEX"
                };
                let columns: Vec<String> = index
                    .columns
                    .iter()
                    .map(|column| self.quote_identifier(column))
                    .collect();

                format!(
                    "CREATE {} {}{} ON {} ({})",
                    index_type,
                    exists_clause,
                    self.quote_identifier(&index.name),
                    self.quote_identifier(&self.name),
                    columns.join(", ")
                )
            })
            .collect()
    }

    fn type_to_sql(&self, column_type: &ColumnType) -> String {
        match self.database_type {
            DatabaseType::Postgres => column_type.to_postgres_sql(),
            DatabaseType::MySQL | DatabaseType::MariaDB => column_type.to_mysql_sql(),
            DatabaseType::SQLite => column_type.to_sqlite_sql(),
        }
    }

    fn quote_identifier(&self, name: &str) -> String {
        super::quote_identifier_for_db(name, self.database_type)
    }
}
