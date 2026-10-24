use std::sync::Arc;

use async_trait::async_trait;

use super::{
    EntityManager, EntityState, ManagedOps, RowLink, explain_flush_ordering_failure,
    flush_sort_key, order_self_references, plan_flush_order,
};
use crate::error::{DbFailure, DbFailureKind, Error};

/// A managed entry that only describes a table and its declared relations.
///
/// The flush planner never looks at the model behind an entry, so the whole
/// table-ordering surface can be exercised without a database.
struct OrderingEntry {
    table: &'static str,
    parents: Vec<&'static str>,
    children: Vec<&'static str>,
    state: EntityState,
    links: Vec<RowLink>,
}

impl OrderingEntry {
    fn new(table: &'static str, state: EntityState) -> Self {
        Self {
            table,
            parents: Vec::new(),
            children: Vec::new(),
            state,
            links: Vec::new(),
        }
    }

    /// A row whose `id` is `id` and whose `parent_id` is `parent`.
    fn node(id: &str, parent: Option<&str>, state: EntityState) -> Self {
        let mut entry = Self::new("nodes", state)
            .with_parents(&["nodes"])
            .with_children(&["nodes"]);
        entry.links.push(RowLink {
            foreign_key: "parent_id",
            referenced: "id",
            provides: Some(id.to_string()),
            requires: parent.map(str::to_string),
        });
        entry
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

    fn table_name(&self) -> &'static str {
        self.table
    }

    fn parent_tables(&self) -> Vec<&'static str> {
        self.parents.clone()
    }

    fn child_tables(&self) -> Vec<&'static str> {
        self.children.clone()
    }

    fn row_links(&self) -> Vec<RowLink> {
        self.links.clone()
    }

    async fn flush(
        self: Arc<Self>,
        _entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        Ok(())
    }
}

/// `entries` in the order `flush_in_scope` writes them.
fn flush_order(entries: &[Arc<dyn ManagedOps>]) -> Vec<Arc<dyn ManagedOps>> {
    let order = plan_flush_order(entries);
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|entry| flush_sort_key(entry.as_ref(), &order));
    order_self_references(&mut sorted);
    sorted
}

fn flush_tables_in_order(entries: &[Arc<dyn ManagedOps>]) -> Vec<&'static str> {
    flush_order(entries)
        .iter()
        .map(|entry| entry.table_name())
        .collect()
}

/// The `id` of each node, in the order a flush writes them.
fn flush_rows_in_order(entries: &[Arc<dyn ManagedOps>]) -> Vec<String> {
    flush_order(entries)
        .iter()
        .map(|entry| entry.row_links()[0].provides.clone().unwrap_or_default())
        .collect()
}

#[test]
fn tables_flush_in_the_order_their_declared_relations_need() {
    let new = |table| OrderingEntry::new(table, EntityState::New);
    let cases: Vec<(&str, Vec<OrderingEntry>, Vec<&str>)> = vec![
        (
            "inserts run parents first, whatever the registration order",
            vec![new("posts").with_parents(&["users"]), new("users")],
            vec!["users", "posts"],
        ),
        (
            "a relation declared only on the parent still orders the insert",
            vec![new("posts"), new("users").with_children(&["posts"])],
            vec!["users", "posts"],
        ),
        (
            "deletes run children first",
            vec![
                OrderingEntry::new("users", EntityState::Removed).with_children(&["posts"]),
                OrderingEntry::new("posts", EntityState::Removed),
            ],
            vec!["posts", "users"],
        ),
        (
            "the operation kind partitions ahead of the table order",
            vec![
                OrderingEntry::new("posts", EntityState::Removed),
                new("posts").with_parents(&["users"]),
                OrderingEntry::new("users", EntityState::Managed),
                new("users"),
            ],
            vec!["users", "posts", "users", "posts"],
        ),
        (
            "a self-referencing foreign key is not a cycle",
            vec![
                new("categories")
                    .with_parents(&["categories"])
                    .with_children(&["categories"]),
                new("categories"),
            ],
            vec!["categories", "categories"],
        ),
        (
            "a dependency on a table outside the flush is not a cycle",
            vec![new("posts").with_parents(&["users"])],
            vec!["posts"],
        ),
    ];

    for (case, entries, expected) in cases {
        let entries: Vec<_> = entries.into_iter().map(OrderingEntry::shared).collect();
        assert!(
            plan_flush_order(&entries).unordered_tables().is_empty(),
            "{case}"
        );
        assert_eq!(flush_tables_in_order(&entries), expected, "{case}");
    }
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
fn ordering_failures_explain_only_foreign_key_violations() {
    let error = explain_flush_ordering_failure(Error::query("boom"), "posts", &[]);
    assert_eq!(error.to_string(), "Query error: boom");

    let violation = || {
        Error::query("insert on posts violates a foreign key constraint")
            .with_db_failure(Some(DbFailure::new(DbFailureKind::ForeignKeyViolation)))
    };

    let error = explain_flush_ordering_failure(violation(), "posts", &["orders", "customers"]);
    let message = error.to_string();
    assert!(
        error.is_foreign_key_violation(),
        "the driver failure must survive so callers can still classify the error"
    );
    assert!(message.contains("writing `posts`"), "{message}");
    assert!(message.contains("orders, customers"), "{message}");

    let message = explain_flush_ordering_failure(violation(), "posts", &[]).to_string();
    assert!(
        message.contains("declares no relation"),
        "without a cycle the relation was not declared: {message}"
    );
}

#[test]
fn rows_of_a_self_referencing_table_flush_in_reference_order() {
    let node = |id, parent, state| OrderingEntry::node(id, parent, state).shared();
    let cases = [
        (
            "inserts run parents first",
            vec![
                node("3", Some("2"), EntityState::New),
                node("2", Some("1"), EntityState::New),
                node("1", None, EntityState::New),
                node("4", None, EntityState::New),
            ],
            ["1", "2", "3", "4"].as_slice(),
        ),
        (
            "deletes run children first",
            vec![
                node("1", None, EntityState::Removed),
                node("2", Some("1"), EntityState::Removed),
                node("3", Some("2"), EntityState::Removed),
            ],
            ["3", "2", "1"].as_slice(),
        ),
        (
            "rows in a reference cycle keep their registration order",
            vec![
                node("1", Some("2"), EntityState::New),
                node("2", Some("1"), EntityState::New),
            ],
            ["1", "2"].as_slice(),
        ),
    ];

    for (case, entries, expected) in cases {
        assert_eq!(flush_rows_in_order(&entries), expected, "{case}");
    }
}

#[tideorm::model(table = "flush_merge_readings")]
struct Reading {
    #[tideorm(primary_key)]
    id: i64,
    value: f64,
    note: String,
}

/// A merge copies values rather than their JSON, so a NaN or infinite float,
/// which JSON writes as `null`, survives it either way, and its undo puts the
/// value it replaced back.
#[test]
fn merging_keeps_and_restores_non_finite_floats() {
    let reading = |value: f64, note: &str| Reading {
        id: 1,
        value,
        note: note.to_string(),
    };
    let base = reading(1.0, "a");
    let saved = reading(f64::NAN, "a");
    // The holder edited the note while the stored row's value became NaN.
    let holder = reading(1.0, "edited");
    let mut target = holder.clone();
    let changed = super::merge_fields(&mut target, &holder, &base, &saved).unwrap();
    assert_eq!(changed, ["value"]);
    assert!(target.value.is_nan());
    assert_eq!(target.note, "edited");

    super::FieldChanges {
        fields: changed,
        before: holder.clone(),
        after: saved.clone(),
    }
    .undo(&mut target);
    assert_eq!(target.value, 1.0);
    assert_eq!(target.note, "edited");

    // An infinite value set while the row was written stays over the stored one.
    let edited = reading(f64::INFINITY, "a");
    let mut target = saved.clone();
    super::merge_fields(&mut target, &edited, &base, &saved).unwrap();
    assert_eq!(target.value, f64::INFINITY);
}
