use tideorm::query::Order;
use tideorm::relations::RelationConstraints;

#[test]
fn test_relation_constraints_new() {
    let constraints = RelationConstraints::new();
    assert!(constraints.conditions.is_empty());
    assert!(constraints.order_by.is_none());
    assert!(constraints.limit.is_none());
    assert!(constraints.offset.is_none());
    assert!(!constraints.with_trashed);
}

#[test]
fn test_relation_constraints_where_eq() {
    let constraints = RelationConstraints::new().where_eq("status", "active");

    assert_eq!(constraints.conditions.len(), 1);
    assert_eq!(constraints.conditions[0].0, "status");
    assert_eq!(constraints.conditions[0].1, serde_json::json!("active"));
}

#[test]
fn test_relation_constraints_multiple_where() {
    let constraints = RelationConstraints::new()
        .where_eq("status", "active")
        .where_eq("verified", true);

    assert_eq!(constraints.conditions.len(), 2);
}

#[test]
fn test_relation_constraints_order_by_asc() {
    let constraints = RelationConstraints::new().order_by("created_at", Order::Asc);

    let (col, order) = constraints.order_by.unwrap();
    assert_eq!(col, "created_at");
    match order {
        Order::Asc => {}
        _ => panic!("Expected Order::Asc"),
    }
}

#[test]
fn test_relation_constraints_order_by_desc() {
    let constraints = RelationConstraints::new().order_by("created_at", Order::Desc);

    let (col, order) = constraints.order_by.unwrap();
    assert_eq!(col, "created_at");
    match order {
        Order::Desc => {}
        _ => panic!("Expected Order::Desc"),
    }
}

#[test]
fn test_relation_constraints_limit() {
    let constraints = RelationConstraints::new().limit(10);

    assert_eq!(constraints.limit, Some(10));
}

#[test]
fn test_relation_constraints_offset() {
    let constraints = RelationConstraints::new().offset(20);

    assert_eq!(constraints.offset, Some(20));
}

#[test]
fn test_relation_constraints_with_trashed() {
    let constraints = RelationConstraints::new().with_trashed();

    assert!(constraints.with_trashed);
}

#[test]
fn test_relation_constraints_chained() {
    let constraints = RelationConstraints::new()
        .where_eq("status", "published")
        .where_eq("visible", true)
        .order_by("created_at", Order::Desc)
        .limit(10)
        .offset(0)
        .with_trashed();

    assert_eq!(constraints.conditions.len(), 2);
    assert!(constraints.order_by.is_some());
    assert_eq!(constraints.limit, Some(10));
    assert_eq!(constraints.offset, Some(0));
    assert!(constraints.with_trashed);
}
