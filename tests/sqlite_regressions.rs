//! Regressions found by code reviews and behavioral audits, reproduced on SQLite.
//!
//! Each test owns its database, in memory or in a file of its own under the
//! target directory, so tests share no rows. The modules follow the area under
//! test; models and databases that several of them use live in `fixtures`.
//! Run the whole suite with every module feature:
//!
//! ```text
//! cargo test --test sqlite_regressions --no-default-features --features "sqlite,runtime-tokio,attachments,translations,fulltext,entity-manager,dirty-tracking,encrypted-fields"
//! ```
#![cfg(all(feature = "sqlite", feature = "runtime-tokio"))]

#[path = "sqlite_regressions/fixtures.rs"]
mod fixtures;

#[cfg(feature = "attachments")]
#[path = "sqlite_regressions/attachments.rs"]
mod attachments;
#[path = "sqlite_regressions/batch_updates.rs"]
mod batch_updates;
#[path = "sqlite_regressions/cache.rs"]
mod cache;
#[path = "sqlite_regressions/databases.rs"]
mod databases;
#[path = "sqlite_regressions/derive.rs"]
mod derive;
#[cfg(feature = "encrypted-fields")]
#[path = "sqlite_regressions/encryption.rs"]
mod encryption;
#[cfg(feature = "entity-manager")]
#[path = "sqlite_regressions/entity_manager.rs"]
mod entity_manager;
#[cfg(feature = "fulltext")]
#[path = "sqlite_regressions/fulltext.rs"]
mod fulltext;
#[path = "sqlite_regressions/migrations.rs"]
mod migrations;
#[path = "sqlite_regressions/queries.rs"]
mod queries;
#[path = "sqlite_regressions/seeding.rs"]
mod seeding;
#[path = "sqlite_regressions/tokenization.rs"]
mod tokenization;
#[path = "sqlite_regressions/transactions.rs"]
mod transactions;
#[cfg(feature = "translations")]
#[path = "sqlite_regressions/translations.rs"]
mod translations;
#[path = "sqlite_regressions/validation.rs"]
mod validation;
#[path = "sqlite_regressions/writes.rs"]
mod writes;
