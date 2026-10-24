//! SQLite FTS5 indexes and searches.

use tideorm::fulltext::FullTextIndex;
use tideorm::prelude::*;

#[path = "fulltext/indexes.rs"]
mod indexes;

#[path = "fulltext/rowid.rs"]
mod rowid;

/// Run every statement of `index`'s SQLite installation on `db`.
async fn install(db: &Database, index: &FullTextIndex) -> tideorm::Result<()> {
    for sql in index.to_sqlite_sql() {
        db.exec_raw(&sql).await?;
    }
    Ok(())
}
