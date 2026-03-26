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
    assert!(!result.has_rolled_back());
}

#[test]
fn test_seed_result_has_executed() {
    let mut result = SeedResult::new();
    result.executed.push(SeedInfo {
        name: "test_seed".to_string(),
    });
    assert!(result.has_executed());
    assert!(!result.has_rolled_back());
}

#[test]
fn test_seed_result_total() {
    let mut result = SeedResult::new();
    result.executed.push(SeedInfo {
        name: "seed1".to_string(),
    });
    result.skipped.push(SeedInfo {
        name: "seed2".to_string(),
    });
    result.rolled_back.push(SeedInfo {
        name: "seed3".to_string(),
    });
    assert_eq!(result.total(), 3);
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
        .map(Seed::name)
        .collect::<Vec<_>>();

    assert_eq!(ordered, vec!["independent", "parent", "child"]);
}
