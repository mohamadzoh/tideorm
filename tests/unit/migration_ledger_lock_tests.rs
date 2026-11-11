use super::*;

#[test]
fn a_mysql_lock_is_named_after_its_database_within_64_characters() {
    assert_eq!(
        mysql_lock_name(LockKind::Migration, Some("shop")),
        "tideorm_migrations:shop"
    );
    assert_eq!(
        mysql_lock_name(LockKind::Seed, Some("shop")),
        "tideorm_seeds:shop"
    );
    // a session with no default database still takes one lock per kind
    assert_eq!(
        mysql_lock_name(LockKind::Migration, None),
        "tideorm_migrations:"
    );
    let long = "é".repeat(64);
    let name = mysql_lock_name(LockKind::Migration, Some(&long));
    assert_eq!(name.chars().count(), 64);
    assert!(name.starts_with("tideorm_migrations:é"));
}
