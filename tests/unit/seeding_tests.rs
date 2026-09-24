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

#[test]
fn test_seed_result_new() {
    let result = SeedResult::new();
    assert!(result.executed.is_empty());
    assert!(result.skipped.is_empty());
    assert!(result.rolled_back.is_empty());
    assert!(!result.has_executed());
}

#[test]
fn test_seed_result_has_executed() {
    let mut result = SeedResult::new();
    result.executed.push(SeedInfo {
        name: "test_seed".to_string(),
    });
    assert!(result.has_executed());
}

#[test]
fn test_seed_result_display() {
    let mut result = SeedResult::new();
    result.executed.push(SeedInfo {
        name: "user_seeder".to_string(),
    });
    result.skipped.push(SeedInfo {
        name: "category_seeder".to_string(),
    });

    let display = format!("{}", result);
    assert!(display.contains("user_seeder"));
    assert!(display.contains("category_seeder"));
    assert!(display.contains("Executed seeds"));
    assert!(display.contains("Skipped seeds"));
}

#[test]
fn test_seed_status_display() {
    let status = SeedStatus {
        name: "user_seeder".to_string(),
        executed: true,
        priority: 100,
    };
    let display = format!("{}", status);
    assert!(display.contains("[✓]"));
    assert!(display.contains("user_seeder"));
    assert!(display.contains("priority: 100"));
}

#[test]
fn test_seed_status_not_executed() {
    let status = SeedStatus {
        name: "product_seeder".to_string(),
        executed: false,
        priority: 50,
    };
    let display = format!("{}", status);
    assert!(display.contains("[○]"));
    assert!(display.contains("product_seeder"));
}

#[test]
fn test_seeder_default() {
    let seeder = Seeder::default();
    assert!(seeder.seeds.is_empty());
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
fn test_seed_info() {
    let info = SeedInfo {
        name: "test_seed".to_string(),
    };
    assert_eq!(info.name, "test_seed");
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

    let ordered = seeder
        .sort_seeds_by_priority_and_deps()
        .unwrap()
        .into_iter()
        .map(|seed| seed.name().to_string())
        .collect::<Vec<_>>();

    assert_eq!(ordered, vec!["independent", "parent", "child"]);
}

fn sorted_names(seeder: &Seeder) -> Vec<String> {
    seeder
        .sort_seeds_by_priority_and_deps()
        .unwrap()
        .into_iter()
        .map(|seed| seed.name().to_string())
        .collect()
}

/// A seed and its ledger entry are one transaction, so a seed that fails part
/// way can be fixed and run again without duplicating what it had written.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn a_seed_that_fails_part_way_leaves_nothing_behind() {
    /// Writes a row, then fails.
    struct HalfDoneSeed;

    #[async_trait]
    impl Seed for HalfDoneSeed {
        fn name(&self) -> &str {
            "half_done"
        }

        async fn run(&self, db: &Database) -> Result<()> {
            db.exec_raw("INSERT INTO seeded_rows (id) VALUES (1)")
                .await?;
            Err(Error::configuration("the second step failed"))
        }
    }

    Database::reset_global();
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .build()
        .await
        .expect("sqlite in-memory connection should succeed");
    Database::set_global(db.clone()).expect("setting global database should succeed");
    db.exec_raw("CREATE TABLE seeded_rows (id INTEGER PRIMARY KEY)")
        .await
        .expect("creating the table should succeed");

    let seeder = Seeder::new().add(HalfDoneSeed);
    assert!(seeder.run().await.is_err());

    let rows = db
        .query_raw_json("SELECT COUNT(*) AS n FROM seeded_rows")
        .await
        .expect("counting should succeed");
    assert_eq!(rows[0]["n"], 0, "the failed seed kept its rows");
    let status = seeder.status().await.expect("status should load");
    assert!(!status[0].executed, "the failed seed was recorded as run");

    Database::reset_global();
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
