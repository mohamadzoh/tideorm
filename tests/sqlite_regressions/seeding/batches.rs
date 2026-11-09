//! Batch commands that fail part-way still report the seeds they finished.

use tideorm::prelude::*;
use tideorm::seeding::{Seed, Seeder, async_trait};

/// What a [`Step`] seed refuses to do.
#[derive(Clone, Copy)]
enum Refuses {
    Nothing,
    Run,
    Rollback,
}

/// A seed that writes nothing but its ledger entry, or refuses one action.
struct Step(&'static str, Refuses);

#[async_trait]
impl Seed for Step {
    fn name(&self) -> &str {
        self.0
    }
    async fn run(&self, _db: &Database) -> tideorm::Result<()> {
        match self.1 {
            Refuses::Run => Err(tideorm::Error::query("refused to run")),
            _ => Ok(()),
        }
    }
    async fn rollback(&self, _db: &Database) -> tideorm::Result<()> {
        match self.1 {
            Refuses::Rollback => Err(tideorm::Error::query("refused to roll back")),
            _ => Ok(()),
        }
    }
}

/// Each seed commits on its own, so a batch command that fails part-way keeps
/// the seeds it ran or rolled back before the failure, and its `SeedFailure`
/// lists them beside the error.
#[tokio::test]
async fn a_failed_seed_batch_reports_the_seeds_done_before_it() {
    let db = crate::fixtures::memory_database("").await;
    let names = |seeds: &[SeedInfo]| {
        seeds
            .iter()
            .map(|seed| seed.name.clone())
            .collect::<Vec<_>>()
    };
    let seeder = Seeder::new()
        .add(Step("a", Refuses::Nothing))
        .add(Step("b", Refuses::Nothing))
        .add(Step("c", Refuses::Run))
        .database(&db);

    let failure = seeder.run().await.unwrap_err();
    assert!(failure.to_string().contains("refused to run"), "{failure}");
    assert_eq!(names(&failure.result.executed), ["a", "b"]);
    assert_eq!(crate::fixtures::row_count(&db, "_seeds").await, 2);

    let failure = seeder.refresh().await.unwrap_err();
    assert_eq!(names(&failure.result.rolled_back), ["b", "a"]);
    assert_eq!(names(&failure.result.executed), ["a", "b"]);

    let seeder = Seeder::new()
        .add(Step("a", Refuses::Rollback))
        .add(Step("b", Refuses::Nothing))
        .database(&db);
    let failure = seeder.reset().await.unwrap_err();
    assert!(
        failure.to_string().contains("refused to roll back"),
        "{failure}"
    );
    assert_eq!(names(&failure.result.rolled_back), ["b"]);
    assert_eq!(crate::fixtures::row_count(&db, "_seeds").await, 1);

    let failure = seeder.rollback_steps(5).await.unwrap_err();
    assert!(failure.result.rolled_back.is_empty());
    let error: tideorm::Error = failure.into();
    assert!(
        error.to_string().contains("refused to roll back"),
        "{error}"
    );
}
