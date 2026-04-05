// =============================================================================
// SEEDING TESTS
// =============================================================================

use tideorm::seeding::{SeedInfo, SeedResult, SeedStatus};

#[test]
fn test_seed_result_has_rolled_back() {
    let result = SeedResult {
        executed: Vec::new(),
        skipped: Vec::new(),
        rolled_back: vec![SeedInfo {
            name: "test_seed".to_string(),
        }],
    };
    assert!(!result.has_executed());
    assert!(result.has_rolled_back());
}

#[test]
fn test_seed_result_display_rolled_back() {
    let result = SeedResult {
        executed: Vec::new(),
        skipped: Vec::new(),
        rolled_back: vec![SeedInfo {
            name: "test_seeder".to_string(),
        }],
    };
    let display = format!("{}", result);
    assert!(display.contains("test_seeder"));
    assert!(display.contains("Rolled back seeds"));
}

#[test]
fn test_seed_info_clone() {
    let info = SeedInfo {
        name: "test_seed".to_string(),
    };
    let cloned = info.clone();
    assert_eq!(info.name, cloned.name);
}

#[test]
fn test_seed_status_clone() {
    let status = SeedStatus {
        name: "test".to_string(),
        executed: true,
        priority: 10,
    };
    let cloned = status.clone();
    assert_eq!(status.name, cloned.name);
    assert_eq!(status.executed, cloned.executed);
    assert_eq!(status.priority, cloned.priority);
}

#[test]
fn test_seed_result_clone() {
    let result = SeedResult {
        executed: vec![SeedInfo {
            name: "s1".to_string(),
        }],
        skipped: vec![SeedInfo {
            name: "s2".to_string(),
        }],
        rolled_back: vec![SeedInfo {
            name: "s3".to_string(),
        }],
    };
    let cloned = result.clone();
    assert_eq!(result.executed.len(), cloned.executed.len());
    assert_eq!(result.skipped.len(), cloned.skipped.len());
    assert_eq!(result.rolled_back.len(), cloned.rolled_back.len());
}

#[test]
fn test_seed_info_debug() {
    let info = SeedInfo {
        name: "test_seed".to_string(),
    };
    let debug = format!("{:?}", info);
    assert!(debug.contains("test_seed"));
}

#[test]
fn test_seed_status_debug() {
    let status = SeedStatus {
        name: "test".to_string(),
        executed: true,
        priority: 100,
    };
    let debug = format!("{:?}", status);
    assert!(debug.contains("test"));
    assert!(debug.contains("true"));
    assert!(debug.contains("100"));
}

#[test]
fn test_seed_result_empty_display() {
    let result = SeedResult {
        executed: Vec::new(),
        skipped: Vec::new(),
        rolled_back: Vec::new(),
    };
    let display = format!("{}", result);
    assert!(display.is_empty() || !display.contains("Executed"));
}

#[test]
fn test_seed_result_multiple_seeds() {
    let result = SeedResult {
        executed: vec![
            SeedInfo {
                name: "seed1".to_string(),
            },
            SeedInfo {
                name: "seed2".to_string(),
            },
            SeedInfo {
                name: "seed3".to_string(),
            },
        ],
        skipped: Vec::new(),
        rolled_back: Vec::new(),
    };
    assert_eq!(result.total(), 3);
    assert!(result.has_executed());
}

#[test]
fn test_seed_status_high_priority() {
    let status = SeedStatus {
        name: "critical_seeder".to_string(),
        executed: false,
        priority: 1,
    };
    assert_eq!(status.priority, 1);
}

#[test]
fn test_seed_status_low_priority() {
    let status = SeedStatus {
        name: "optional_seeder".to_string(),
        executed: false,
        priority: 1000,
    };
    assert_eq!(status.priority, 1000);
}
