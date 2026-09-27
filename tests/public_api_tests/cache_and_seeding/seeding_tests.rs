use tideorm::seeding::{SeedInfo, SeedResult};

fn rolled_back(name: &str) -> SeedResult {
    SeedResult {
        executed: Vec::new(),
        skipped: Vec::new(),
        rolled_back: vec![SeedInfo {
            name: name.to_string(),
        }],
    }
}

#[test]
fn test_seed_result_has_rolled_back() {
    let result = rolled_back("test_seed");
    assert!(!result.has_executed());
}

#[test]
fn test_seed_result_display_rolled_back() {
    let display = format!("{}", rolled_back("test_seeder"));
    assert!(display.contains("test_seeder"));
    assert!(display.contains("Rolled back seeds"));
}

#[test]
fn test_seed_result_empty_display_is_empty() {
    let result = SeedResult {
        executed: Vec::new(),
        skipped: Vec::new(),
        rolled_back: Vec::new(),
    };
    assert_eq!(format!("{}", result), "");
}
