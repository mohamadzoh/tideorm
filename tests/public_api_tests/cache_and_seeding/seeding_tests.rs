use tideorm::seeding::{SeedInfo, SeedResult};

#[test]
fn test_seed_result_reports_rolled_back_seeds() {
    let result = SeedResult {
        rolled_back: vec![SeedInfo {
            name: "test_seeder".to_string(),
        }],
        ..SeedResult::default()
    };

    assert!(!result.has_executed(), "a rollback executes no seed");
    let display = result.to_string();
    assert!(display.contains("Rolled back seeds"), "{display}");
    assert!(display.contains("test_seeder"), "{display}");
}

#[test]
fn test_seed_result_empty_display_is_empty() {
    assert_eq!(SeedResult::default().to_string(), "");
}
