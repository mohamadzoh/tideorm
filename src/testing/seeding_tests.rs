use super::*;

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