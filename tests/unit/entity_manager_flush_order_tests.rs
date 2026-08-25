use std::sync::Arc;

use async_trait::async_trait;

use super::{
    EntityManager, EntityState, ManagedCheckpoint, ManagedOps, explain_flush_ordering_failure,
    flush_sort_key, plan_flush_order,
};
use crate::error::{DbFailure, DbFailureKind, Error};

struct NoopFlushOrderCheckpoint;

impl ManagedCheckpoint for NoopFlushOrderCheckpoint {
    fn rollback(self: Box<Self>, _entity_manager: &EntityManager) {}
}

/// A managed entry that only describes a table and its declared relations.
///
/// The flush planner never looks at the model behind an entry, so the whole
/// table-ordering surface can be exercised without a database.
struct OrderingEntry {
    table: &'static str,
    parents: Vec<&'static str>,
    children: Vec<&'static str>,
    state: EntityState,
}

impl OrderingEntry {
    fn new(table: &'static str, state: EntityState) -> Self {
        Self {
            table,
            parents: Vec::new(),
            children: Vec::new(),
            state,
        }
    }

    fn with_parents(mut self, parents: &[&'static str]) -> Self {
        self.parents = parents.to_vec();
        self
    }

    fn with_children(mut self, children: &[&'static str]) -> Self {
        self.children = children.to_vec();
        self
    }

    fn shared(self) -> Arc<dyn ManagedOps> {
        Arc::new(self)
    }
}

#[async_trait]
impl ManagedOps for OrderingEntry {
    fn current_state(&self) -> EntityState {
        self.state
    }

    fn detach_from_context(&self, _entity_manager: &EntityManager) {}

    fn checkpoint(self: Arc<Self>) -> Box<dyn ManagedCheckpoint> {
        Box::new(NoopFlushOrderCheckpoint)
    }

    fn table_name(&self) -> &'static str {
        self.table
    }

    fn parent_tables(&self) -> Vec<&'static str> {
        self.parents.clone()
    }

    fn child_tables(&self) -> Vec<&'static str> {
        self.children.clone()
    }

    async fn flush(
        self: Arc<Self>,
        _entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        Ok(())
    }
}

/// Apply exactly the ordering `flush_in_scope_with_checkpoints` applies.
fn flush_tables_in_order(entries: &[Arc<dyn ManagedOps>]) -> Vec<&'static str> {
    let order = plan_flush_order(entries);
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|entry| flush_sort_key(entry.as_ref(), &order));
    sorted.iter().map(|entry| entry.table_name()).collect()
}

#[test]
fn inserts_run_parents_before_children_whatever_the_registration_order() {
    let entries = vec![
        OrderingEntry::new("posts", EntityState::New)
            .with_parents(&["users"])
            .shared(),
        OrderingEntry::new("users", EntityState::New).shared(),
    ];

    assert_eq!(flush_tables_in_order(&entries), vec!["users", "posts"]);
}

#[test]
fn a_relation_declared_only_on_the_parent_still_orders_the_insert() {
    let entries = vec![
        OrderingEntry::new("posts", EntityState::New).shared(),
        OrderingEntry::new("users", EntityState::New)
            .with_children(&["posts"])
            .shared(),
    ];

    assert_eq!(flush_tables_in_order(&entries), vec!["users", "posts"]);
}

#[test]
fn deletes_run_children_before_parents() {
    let entries = vec![
        OrderingEntry::new("users", EntityState::Removed)
            .with_children(&["posts"])
            .shared(),
        OrderingEntry::new("posts", EntityState::Removed).shared(),
    ];

    assert_eq!(flush_tables_in_order(&entries), vec!["posts", "users"]);
}

#[test]
fn operation_kind_still_partitions_ahead_of_table_order() {
    let entries = vec![
        OrderingEntry::new("posts", EntityState::Removed).shared(),
        OrderingEntry::new("posts", EntityState::New)
            .with_parents(&["users"])
            .shared(),
        OrderingEntry::new("users", EntityState::Managed).shared(),
        OrderingEntry::new("users", EntityState::New).shared(),
    ];

    assert_eq!(
        flush_tables_in_order(&entries),
        vec!["users", "posts", "users", "posts"]
    );
}

#[test]
fn a_self_referencing_foreign_key_orders_without_reporting_a_cycle() {
    let entries = vec![
        OrderingEntry::new("categories", EntityState::New)
            .with_parents(&["categories"])
            .with_children(&["categories"])
            .shared(),
        OrderingEntry::new("categories", EntityState::New).shared(),
    ];

    let order = plan_flush_order(&entries);

    assert!(order.unordered_tables().is_empty());
    assert_eq!(
        flush_tables_in_order(&entries),
        vec!["categories", "categories"]
    );
}

#[test]
fn a_two_table_cycle_is_reported_instead_of_looping() {
    let entries = vec![
        OrderingEntry::new("orders", EntityState::New)
            .with_parents(&["customers"])
            .shared(),
        OrderingEntry::new("customers", EntityState::New)
            .with_parents(&["orders"])
            .shared(),
    ];

    let order = plan_flush_order(&entries);
    let mut unordered = order.unordered_tables().to_vec();
    unordered.sort_unstable();

    assert_eq!(unordered, vec!["customers", "orders"]);
    assert_eq!(flush_tables_in_order(&entries).len(), 2);
}

#[test]
fn a_dependency_on_a_table_outside_the_flush_is_not_a_cycle() {
    let entries = vec![
        OrderingEntry::new("posts", EntityState::New)
            .with_parents(&["users"])
            .shared(),
    ];

    let order = plan_flush_order(&entries);

    assert!(order.unordered_tables().is_empty());
    assert_eq!(flush_tables_in_order(&entries), vec!["posts"]);
}

#[test]
fn ordering_failures_leave_non_foreign_key_errors_alone() {
    let error = explain_flush_ordering_failure(Error::invalid_query("boom"), "posts", &[]);

    assert_eq!(error.to_string(), "Query error: boom");
}

#[test]
fn a_foreign_key_violation_names_the_table_and_the_cycle() {
    let violation = Error::query("insert on posts violates a foreign key constraint")
        .with_db_failure(Some(DbFailure::new(DbFailureKind::ForeignKeyViolation)));

    let error = explain_flush_ordering_failure(violation, "posts", &["orders", "customers"]);
    let message = error.to_string();

    assert!(
        error.is_foreign_key_violation(),
        "the driver failure must survive so callers can still classify the error"
    );
    assert!(message.contains("writing `posts`"));
    assert!(message.contains("orders, customers"));
}

#[test]
fn a_foreign_key_violation_without_a_cycle_says_the_relation_was_not_declared() {
    let violation = Error::query("insert on posts violates a foreign key constraint")
        .with_db_failure(Some(DbFailure::new(DbFailureKind::ForeignKeyViolation)));

    let message = explain_flush_ordering_failure(violation, "posts", &[]).to_string();

    assert!(message.contains("declares no relation"));
}
