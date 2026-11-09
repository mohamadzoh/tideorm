use super::*;

#[test]
fn a_table_drops_before_the_tables_it_references() {
    // 0 = users, 1 = posts -> users, 2 = comments -> posts, users.
    let order = drop_order(&[vec![], vec![0], vec![1, 0]]);
    assert_eq!(order, vec![2, 1, 0]);
}

#[test]
fn unrelated_tables_keep_their_registration_order() {
    assert_eq!(drop_order(&[vec![], vec![], vec![]]), vec![0, 1, 2]);
    // Two foreign keys to the same table.
    assert_eq!(drop_order(&[vec![1, 1], vec![]]), vec![0, 1]);
}

#[test]
fn a_reference_cycle_goes_last_in_registration_order() {
    // 0 <-> 1 form a cycle; 2 references 0 and drops first.
    assert_eq!(drop_order(&[vec![1], vec![0], vec![0]]), vec![2, 0, 1]);
}

#[test]
fn a_reference_names_the_model_table_as_the_catalog_writes_it() {
    let users = ModelSchema::new("users");
    let audit = ModelSchema::new("users").schema("audit");
    assert!(names_table("users", &users, DatabaseType::Postgres));
    assert!(names_table("audit.users", &audit, DatabaseType::Postgres));
    assert!(!names_table("audit.users", &users, DatabaseType::Postgres));
    assert!(names_table("Users", &users, DatabaseType::SQLite));
    assert!(!names_table("Users", &users, DatabaseType::MySQL));
}
