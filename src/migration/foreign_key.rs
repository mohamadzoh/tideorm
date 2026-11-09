//! Foreign key constraints for migrations.

use std::fmt::Write as _;

use crate::internal::sql_safety::{column_list, quote_ident};

use super::DatabaseType;
use super::ddl;

/// What the database does to the rows referencing a row whose key is
/// deleted or updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignKeyAction {
    /// Delete or update the referencing rows with it.
    Cascade,
    /// Refuse the change while a row references the key.
    Restrict,
    /// Set the referencing columns to NULL.
    SetNull,
    /// Set the referencing columns to their defaults; MySQL's InnoDB refuses
    /// to create such a key.
    SetDefault,
    /// Refuse the change, checked once the statement ends: what a key does
    /// when it names no action.
    NoAction,
}

impl ForeignKeyAction {
    fn sql(self) -> &'static str {
        match self {
            Self::Cascade => "CASCADE",
            Self::Restrict => "RESTRICT",
            Self::SetNull => "SET NULL",
            Self::SetDefault => "SET DEFAULT",
            Self::NoAction => "NO ACTION",
        }
    }
}

/// A foreign key from `columns` of the table declaring it to `references`
/// of `table`.
#[derive(Debug, Clone)]
pub(crate) struct ForeignKey {
    name: Option<String>,
    columns: Vec<String>,
    table: String,
    references: Vec<String>,
    on_delete: Option<ForeignKeyAction>,
    on_update: Option<ForeignKeyAction>,
}

impl ForeignKey {
    pub(crate) fn new(columns: &[&str], table: &str, references: &[&str]) -> Self {
        let owned = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
        Self {
            name: None,
            columns: owned(columns),
            table: table.to_string(),
            references: owned(references),
            on_delete: None,
            on_update: None,
        }
    }

    /// The constraint's name: the one given, or `fk_<owner>_<columns>`,
    /// shortened with a hash past 63 bytes.
    pub(crate) fn name(&self, owner: &str) -> String {
        self.name.clone().unwrap_or_else(|| {
            ddl::bounded_index_name(format!("fk_{}_{}", owner, self.columns.join("_")))
        })
    }

    /// `REFERENCES table (columns)` and the actions.
    fn references_sql(&self, db_type: DatabaseType) -> String {
        let mut sql = format!(
            "REFERENCES {} ({})",
            ddl::quote_table(db_type, &self.table),
            column_list(db_type, &self.references, "")
        );
        for (event, action) in [("DELETE", self.on_delete), ("UPDATE", self.on_update)] {
            if let Some(action) = action {
                let _ = write!(sql, " ON {event} {}", action.sql());
            }
        }
        sql
    }

    /// The table constraint declaring this key on the table `owner`.
    pub(crate) fn constraint_sql(&self, db_type: DatabaseType, owner: &str) -> String {
        format!(
            "CONSTRAINT {} FOREIGN KEY ({}) {}",
            quote_ident(db_type, &self.name(owner)),
            column_list(db_type, &self.columns, ""),
            self.references_sql(db_type)
        )
    }

    /// The column constraint declaring this key on its one column of the
    /// table `owner`, for SQLite's `ADD COLUMN`, which takes no table
    /// constraint.
    pub(crate) fn column_constraint_sql(&self, db_type: DatabaseType, owner: &str) -> String {
        format!(
            "CONSTRAINT {} {}",
            quote_ident(db_type, &self.name(owner)),
            self.references_sql(db_type)
        )
    }

    pub(crate) fn set_on_delete(&mut self, action: ForeignKeyAction) {
        self.on_delete = Some(action);
    }

    pub(crate) fn set_on_update(&mut self, action: ForeignKeyAction) {
        self.on_update = Some(action);
    }
}

/// Builder for a foreign key a [`TableBuilder`](super::TableBuilder) or an
/// [`AlterTableBuilder`](super::AlterTableBuilder) declares.
///
/// The key is added when the builder drops, so
/// `t.foreign(&["user_id"], "users", &["id"]).on_delete(ForeignKeyAction::Cascade);`
/// needs no terminating call. Unnamed, it is `fk_<table>_<columns>`, shortened
/// with a hash past 63 bytes, which is the name
/// [`drop_foreign_key`](super::AlterTableBuilder::drop_foreign_key) takes.
pub struct ForeignKeyBuilder<'a, T = super::TableBuilder> {
    target: &'a mut T,
    key: Option<ForeignKey>,
    add: fn(&mut T, ForeignKey),
}

impl<'a, T> ForeignKeyBuilder<'a, T> {
    pub(super) fn new(target: &'a mut T, key: ForeignKey, add: fn(&mut T, ForeignKey)) -> Self {
        Self {
            target,
            key: Some(key),
            add,
        }
    }

    fn key(&mut self) -> &mut ForeignKey {
        self.key
            .as_mut()
            .expect("the key is only taken when the builder drops")
    }

    /// Name the constraint `name`.
    pub fn name(mut self, name: &str) -> Self {
        self.key().name = Some(name.to_string());
        self
    }

    /// What deleting a referenced row does to the rows referencing it.
    pub fn on_delete(mut self, action: ForeignKeyAction) -> Self {
        self.key().set_on_delete(action);
        self
    }

    /// What updating a referenced key does to the rows referencing it.
    pub fn on_update(mut self, action: ForeignKeyAction) -> Self {
        self.key().set_on_update(action);
        self
    }
}

impl<T> Drop for ForeignKeyBuilder<'_, T> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            (self.add)(self.target, key);
        }
    }
}
