//! The migrator's operations on one migration it is told the name of:
//! running or reverting it out of turn, recording it without running it, and
//! showing what it would run. The CLI's migration runner maps its commands
//! onto them.

use std::sync::Arc;

use crate::error::{Error, Result};

use super::migrator::{info_of, run_migration, with_migration_lock};
use super::{Migration, MigrationInfo, MigrationResult, Migrator, Schema};

impl Migrator {
    /// Run the pending migration `migration` names, by its version or else
    /// its name, even when pending migrations come before it, holding the
    /// lock [`run`](Self::run) holds.
    pub async fn run_one(&self, migration: &str) -> Result<MigrationResult> {
        let migration = self.find(migration)?;
        self.change_one(migration, true).await?;
        Ok(MigrationResult {
            applied: vec![info_of(migration.as_ref())],
            ..MigrationResult::default()
        })
    }

    /// Revert the applied migration `migration` names, by its version or
    /// else its name, wherever it is in the ledger, holding the lock
    /// [`rollback`](Self::rollback) holds.
    pub async fn rollback_one(&self, migration: &str) -> Result<MigrationResult> {
        let migration = self.find(migration)?;
        self.change_one(migration, false).await?;
        Ok(MigrationResult {
            rolled_back: vec![info_of(migration.as_ref())],
            ..MigrationResult::default()
        })
    }

    /// Record the migration `migration` names as applied, or with `applied`
    /// false remove its record, without running it.
    ///
    /// MySQL and MariaDB commit DDL as it runs, so a migration whose ledger
    /// write failed there leaves its schema change unrecorded, and every
    /// later run fails on it; marking it applied reconciles the ledger, and
    /// unmarking covers a rollback that dropped the change but kept the row.
    pub async fn mark(&self, migration: &str, applied: bool) -> Result<MigrationInfo> {
        let migration = self.find(migration)?;
        let db = self.db()?;
        let ledger = self.ledger()?;
        with_migration_lock(&db, async {
            ledger.ensure(&db).await?;
            Self::ensure_recorded(&ledger.keys(&db).await?, migration.as_ref(), !applied)?;
            if applied {
                ledger
                    .record(&db, migration.version(), &[migration.name()])
                    .await
            } else {
                ledger.remove(&db, migration.version()).await
            }
        })
        .await?;
        Ok(info_of(migration.as_ref()))
    }

    /// The statements the `up()` of the migration `migration` names would
    /// run, or with `apply` false its `down()`, without running them.
    ///
    /// The migration runs with a [`Schema`] that collects its statements,
    /// inside a transaction that is rolled back, so what it runs other than
    /// through the schema (a data backfill) changes nothing, except DDL on
    /// MySQL and MariaDB, which commits as it runs. The schema still reads
    /// the database where it needs to (`has_table`, MySQL's `alter_table`),
    /// and sees it as it is, before the migrations it pretends. It takes no
    /// lock and writes no ledger row.
    pub async fn pretend(&self, migration: &str, apply: bool) -> Result<Vec<String>> {
        let migration = self.find(migration)?;
        let db = self.db()?;
        let mut schema = Schema::new(db.backend())
            .with_foreign_keys_off(migration.foreign_keys_off())
            .pretending();
        let mut finished = false;
        let outcome = db
            .atomically(async {
                if apply {
                    migration.up(&mut schema).await?;
                } else {
                    migration.down(&mut schema).await?;
                }
                finished = true;
                // An error is what rolls the transaction back.
                Err::<(), _>(Error::query("pretended migration rolled back"))
            })
            .await;
        match outcome {
            Err(error) if !finished => Err(error),
            _ => Ok(schema.into_pretended()),
        }
    }

    /// Apply the migration, or with `apply` false revert it, under the
    /// migration lock, once the ledger read under it says it is pending, or
    /// applied.
    async fn change_one(&self, migration: &Arc<dyn Migration>, apply: bool) -> Result<()> {
        let db = self.db()?;
        let ledger = self.ledger()?;
        with_migration_lock(&db, async {
            ledger.ensure(&db).await?;
            Self::ensure_recorded(&ledger.keys(&db).await?, migration.as_ref(), !apply)?;
            run_migration(
                &db,
                Arc::clone(migration),
                ledger.table().to_string(),
                apply,
            )
            .await
        })
        .await
    }

    /// Refuse to go on unless the ledger's `recorded` keys hold the
    /// migration exactly when `expected` says so.
    fn ensure_recorded(
        recorded: &[String],
        migration: &dyn Migration,
        expected: bool,
    ) -> Result<()> {
        if recorded.iter().any(|key| key == migration.version()) == expected {
            return Ok(());
        }
        let state = if expected {
            "has not run"
        } else {
            "has already run"
        };
        Err(Error::query(format!(
            "Migration {} - {} {state}",
            migration.version(),
            migration.name()
        )))
    }

    /// The registered migration `migration` names: the one of that version,
    /// else the one of that name. Two of one name are ambiguous.
    fn find(&self, migration: &str) -> Result<&Arc<dyn Migration>> {
        let registered = self.registered();
        if let Some(found) = registered
            .iter()
            .find(|candidate| candidate.version() == migration)
        {
            return Ok(found);
        }
        let mut named = registered
            .iter()
            .filter(|candidate| candidate.name() == migration);
        match (named.next(), named.next()) {
            (Some(found), None) => Ok(found),
            (Some(_), Some(_)) => Err(Error::query(format!(
                "Migration name '{migration}' is ambiguous: name it by its version"
            ))),
            (None, _) => Err(Error::query(format!(
                "Migration '{migration}' is not registered on this migrator"
            ))),
        }
    }
}
