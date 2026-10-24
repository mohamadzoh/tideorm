use super::*;

struct TestSeed {
    name: &'static str,
    priority: u32,
    dependencies: Vec<&'static str>,
}

#[async_trait]
impl Seed for TestSeed {
    fn name(&self) -> &str {
        self.name
    }

    async fn run(&self, _db: &Database) -> Result<()> {
        Ok(())
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    fn depends_on(&self) -> Vec<&str> {
        self.dependencies.clone()
    }
}

fn sorted_names(seeder: &Seeder) -> Vec<String> {
    seeder
        .sort_seeds_by_priority_and_deps()
        .unwrap()
        .into_iter()
        .map(|seed| seed.name().to_string())
        .collect()
}

#[test]
fn test_seed_result_display() {
    let mut result = SeedResult::default();
    result.executed.push(SeedInfo {
        name: "user_seeder".to_string(),
    });
    result.skipped.push(SeedInfo {
        name: "category_seeder".to_string(),
    });
    assert!(result.has_executed());

    let display = format!("{}", result);
    assert!(display.contains("user_seeder"));
    assert!(display.contains("category_seeder"));
    assert!(display.contains("Executed seeds"));
    assert!(display.contains("Skipped seeds"));
}

#[test]
fn test_seed_status_display() {
    for (executed, expected) in [
        (true, "[✓] user_seeder (priority: 100)"),
        (false, "[○] user_seeder (priority: 100)"),
    ] {
        let status = SeedStatus {
            name: "user_seeder".to_string(),
            executed,
            priority: 100,
        };
        assert_eq!(status.to_string(), expected);
    }
}

#[test]
fn names_lists_the_seeds_in_registration_order() {
    let seed = |name| TestSeed {
        name,
        // Registration order, not run order.
        priority: if name == "users" { 200 } else { 100 },
        dependencies: Vec::new(),
    };
    let seeder = Seeder::new().add(seed("users")).add(seed("posts"));
    assert_eq!(seeder.names().collect::<Vec<_>>(), ["users", "posts"]);
}

#[test]
fn test_sort_seeds_by_priority_and_deps_returns_cycle_error() {
    let seeder = Seeder::new()
        .add(TestSeed {
            name: "seed_a",
            priority: 10,
            dependencies: vec!["seed_b"],
        })
        .add(TestSeed {
            name: "seed_b",
            priority: 20,
            dependencies: vec!["seed_a"],
        });

    let err = match seeder.sort_seeds_by_priority_and_deps() {
        Ok(_) => panic!("expected circular dependency error"),
        Err(err) => err,
    };

    assert!(
        err.to_string()
            .contains("Circular seed dependency detected")
    );
    assert!(err.to_string().contains("seed_a"));
    assert!(err.to_string().contains("seed_b"));
}

#[test]
fn test_sort_seeds_by_priority_and_deps_orders_dependencies_before_priority() {
    let seeder = Seeder::new()
        .add(TestSeed {
            name: "independent",
            priority: 1,
            dependencies: vec![],
        })
        .add(TestSeed {
            name: "parent",
            priority: 100,
            dependencies: vec![],
        })
        .add(TestSeed {
            name: "child",
            priority: 0,
            dependencies: vec!["parent"],
        });

    assert_eq!(
        sorted_names(&seeder),
        vec!["independent", "parent", "child"]
    );
}

#[test]
fn test_a_seed_that_becomes_ready_later_still_runs_by_priority() {
    // B only becomes ready once A has run. A FIFO queue ran it after every
    // root already waiting - after C, whose priority is far lower.
    let seeder = Seeder::new()
        .add(TestSeed {
            name: "a",
            priority: 50,
            dependencies: vec![],
        })
        .add(TestSeed {
            name: "b",
            priority: 10,
            dependencies: vec!["a"],
        })
        .add(TestSeed {
            name: "c",
            priority: 100,
            dependencies: vec![],
        });

    assert_eq!(sorted_names(&seeder), vec!["a", "b", "c"]);
}

#[test]
fn test_equal_priorities_keep_registration_order() {
    let seeder = Seeder::new()
        .add(TestSeed {
            name: "first",
            priority: 100,
            dependencies: vec![],
        })
        .add(TestSeed {
            name: "unlocked",
            priority: 100,
            dependencies: vec!["first"],
        })
        .add(TestSeed {
            name: "second",
            priority: 100,
            dependencies: vec![],
        })
        .add(TestSeed {
            name: "third",
            priority: 100,
            dependencies: vec![],
        });

    assert_eq!(
        sorted_names(&seeder),
        vec!["first", "unlocked", "second", "third"]
    );
}

/// Inside a transaction the seeder reads and creates its ledger on that
/// transaction: on a one-connection pool, asking the pool for another
/// connection never returned, and a run again in the same transaction saw
/// nothing its first run had recorded.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn seeding_inside_a_transaction_uses_that_transaction() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    Database::reset_global();
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .build()
        .await
        .expect("sqlite in-memory connection should succeed");
    Database::set_global(db.clone()).expect("setting global database should succeed");

    let (first, second) = db
        .transaction(|_| {
            Box::pin(async move {
                let seeder = Seeder::new().add(TestSeed {
                    name: "once",
                    priority: 0,
                    dependencies: Vec::new(),
                });
                let first = seeder.run().await?;
                let second = seeder.run().await?;
                Ok((first, second))
            })
        })
        .await
        .expect("seeding inside a transaction should succeed");
    assert_eq!(first.executed.len(), 1);
    assert_eq!(second.executed.len(), 0);
    assert_eq!(second.skipped.len(), 1);

    Database::reset_global();
}
