//! The order a flush writes its entries in: tables parents-first for
//! inserts and children-first for deletes, and rows of one table by their
//! self-references.

use std::collections::HashMap;
use std::sync::Arc;

use crate::orm::sql_builder::ValueKey;

use super::{EntityState, ManagedOps};

/// Coarse flush ordering rank: inserts, then updates, then deletes.
///
/// A flush runs every pending operation inside a single transaction, so a
/// mis-ordered foreign key aborts the whole flush. Grouping the work by
/// operation kind is the first half of the ordering; [`plan_flush_order`]
/// supplies the second half, which orders tables against each other inside a
/// kind. Sorting must stay stable so entries that tie on both keep their
/// registration order.
fn flush_order_rank(state: EntityState) -> u8 {
    match state {
        EntityState::New => 0,
        EntityState::Managed => 1,
        EntityState::Removed => 2,
        EntityState::Detached => 3,
    }
}

/// Table-level dependency order for one pass of a flush.
///
/// Grouping by operation kind alone still writes `posts` before `users` when
/// the child was persisted first, which the database rejects. This ranks the
/// tables taking part in the pass so inserts can run parents-first and deletes
/// children-first.
pub(in crate::entity_manager) struct FlushOrder {
    ranks: HashMap<&'static str, usize>,
    unordered: Vec<&'static str>,
}

impl FlushOrder {
    fn rank(&self, table: &str) -> usize {
        self.ranks.get(table).copied().unwrap_or(0)
    }

    /// Tables whose declared foreign keys form a cycle, so no total order over
    /// them exists. They keep registration order and are named in the error if
    /// the database then rejects a write.
    pub(in crate::entity_manager) fn unordered_tables(&self) -> &[&'static str] {
        &self.unordered
    }
}

/// Record `parent -> child` if it is a real, expressible constraint.
///
/// Edges to tables outside this pass cannot change the order, and dropping
/// them stops an absent table from inventing a cycle the flush would never
/// hit. A self-edge is dropped too: a self-referencing foreign key
/// (`parent_id` on the same table) is a legal schema, not a table-level
/// ordering constraint, and keeping it would report every such table as an
/// unbreakable cycle.
fn add_dependency_edge(
    edges: &mut Vec<(&'static str, &'static str)>,
    tables: &[&'static str],
    parent: &'static str,
    child: &'static str,
) {
    if parent == child || !tables.contains(&parent) || !tables.contains(&child) {
        return;
    }

    if !edges.contains(&(parent, child)) {
        edges.push((parent, child));
    }
}

/// Topologically sort the tables taking part in this flush pass.
///
/// Edges come from both ends of every declared relation — a child's
/// `belongs_to` and a parent's `has_one`/`has_many` — so one declaration on
/// either side is enough. Kahn's algorithm runs over the tables in first-seen
/// order, which keeps the result deterministic; whatever it cannot drain is a
/// cycle and is appended in registration order rather than looping forever.
pub(in crate::entity_manager) fn plan_flush_order(entries: &[Arc<dyn ManagedOps>]) -> FlushOrder {
    let mut tables: Vec<&'static str> = Vec::new();
    for entry in entries {
        let table = entry.table_name();
        if !tables.contains(&table) {
            tables.push(table);
        }
    }

    let mut edges: Vec<(&'static str, &'static str)> = Vec::new();
    for entry in entries {
        let table = entry.table_name();
        for parent in entry.parent_tables() {
            add_dependency_edge(&mut edges, &tables, parent, table);
        }
        for child in entry.child_tables() {
            add_dependency_edge(&mut edges, &tables, table, child);
        }
    }

    let index_of = |table: &str| {
        tables
            .iter()
            .position(|known| *known == table)
            .expect("edges only join tables of this pass")
    };
    let edges: Vec<(usize, usize)> = edges
        .iter()
        .map(|(parent, child)| (index_of(parent), index_of(child)))
        .collect();
    let (mut ordered, cycle) =
        crate::internal::topological_order(tables.len(), &edges, |index| index);
    let unordered: Vec<&'static str> = cycle.iter().map(|&index| tables[index]).collect();
    ordered.extend(cycle);

    let ranks = ordered
        .into_iter()
        .enumerate()
        .map(|(rank, index)| (tables[index], rank))
        .collect();

    FlushOrder { ranks, unordered }
}

/// One self-referencing foreign key as it applies to one row: the value this
/// row holds in the referenced column, and the value its foreign key holds,
/// each as a key of the referenced column; `None` for a NULL, or a key the
/// database has not assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowLink {
    pub(crate) foreign_key: &'static str,
    pub(crate) referenced: &'static str,
    pub(crate) provides: Option<ValueKey>,
    pub(crate) requires: Option<ValueKey>,
}

/// Order the rows of one table that reference each other through a
/// self-referencing foreign key (`nodes.parent_id -> nodes.id`): inserts
/// referenced row first, deletes referencing row first.
///
/// The table order cannot express it, since the table depends on itself;
/// `entries` is already sorted by [`flush_sort_key`], so the inserts and the
/// deletes of each table are adjacent, and each such run is reordered on its
/// own. Rows in a cycle keep registration order.
pub(in crate::entity_manager) fn order_self_references(entries: &mut [Arc<dyn ManagedOps>]) {
    let mut start = 0;
    while start < entries.len() {
        let state = entries[start].current_state();
        let table = entries[start].table_name();
        let end = start
            + entries[start..]
                .iter()
                .take_while(|entry| entry.current_state() == state && entry.table_name() == table)
                .count();
        if matches!(state, EntityState::New | EntityState::Removed) && end - start > 1 {
            order_rows(&mut entries[start..end], state == EntityState::Removed);
        }
        start = end;
    }
}

/// Topologically order `rows`, referenced rows first, or last when
/// `referencing_first`.
fn order_rows(rows: &mut [Arc<dyn ManagedOps>], referencing_first: bool) {
    let links: Vec<Vec<RowLink>> = rows.iter().map(|row| row.row_links()).collect();
    // The rows holding each referenced value, so a batch of one table pairs
    // in linear time.
    let mut providers: HashMap<(&str, &str, &ValueKey), Vec<usize>> = HashMap::new();
    for (index, row_links) in links.iter().enumerate() {
        for link in row_links {
            if let Some(provided) = &link.provides {
                providers
                    .entry((link.foreign_key, link.referenced, provided))
                    .or_default()
                    .push(index);
            }
        }
    }
    let mut edges = Vec::new();
    for (referencing, row_links) in links.iter().enumerate() {
        for link in row_links {
            let Some(required) = &link.requires else {
                continue;
            };
            let referenced_rows = providers.get(&(link.foreign_key, link.referenced, required));
            for &referenced in referenced_rows.into_iter().flatten() {
                if referenced != referencing {
                    edges.push(if referencing_first {
                        (referencing, referenced)
                    } else {
                        (referenced, referencing)
                    });
                }
            }
        }
    }
    if edges.is_empty() {
        return;
    }
    let (mut order, cycle) = crate::internal::topological_order(rows.len(), &edges, |index| index);
    order.extend(cycle);
    let ordered: Vec<Arc<dyn ManagedOps>> =
        order.into_iter().map(|index| rows[index].clone()).collect();
    rows.clone_from_slice(&ordered);
}

/// Stable sort key placing an entry inside its operation kind.
///
/// Inserts ascend the table order so a parent row exists before the child that
/// references it; deletes descend it so children go before the parent they
/// reference. Updates and detached entries add no constraint of their own and
/// tie, which the stable sort resolves as registration order.
pub(in crate::entity_manager) fn flush_sort_key(
    entry: &dyn ManagedOps,
    order: &FlushOrder,
) -> (u8, usize) {
    let state = entry.current_state();
    let rank = order.rank(entry.table_name());
    let within_kind = match state {
        EntityState::New => rank,
        EntityState::Removed => order.ranks.len().saturating_sub(rank),
        EntityState::Managed | EntityState::Detached => 0,
    };

    (flush_order_rank(state), within_kind)
}

/// Say why a flush was ordered the way it was when the database rejects it.
///
/// The ordering only covers foreign keys a model actually declares, so a flush
/// can still abort on SQLSTATE 23503 — previously with nothing but the driver's
/// message, which never mentions that an entity manager chose the write order.
/// Name the table being written and either the cycle that could not be broken
/// or the fact that no relation declared the missing dependency. The driver
/// failure stays attached as the source, so the SQLSTATE, constraint name and
/// error chain survive.
pub(in crate::entity_manager) fn explain_flush_ordering_failure(
    error: crate::error::Error,
    table: &'static str,
    unordered: &[&'static str],
) -> crate::error::Error {
    if !error.is_foreign_key_violation() {
        return error;
    }

    let detail = if unordered.is_empty() {
        String::from(
            "every table in this flush was ordered parent-before-child, so the row it \
             references belongs to a table this model declares no relation to; declare the \
             relation (`belongs_to` or `has_many`/`has_one`) or persist the parent first",
        )
    } else {
        format!(
            "these tables declare foreign keys in a cycle and could not be ordered: {}; \
             flush one side of the cycle before the other",
            unordered.join(", ")
        )
    };

    let rendered = error.to_string();
    let failure = error.into_db_failure().map(|failure| *failure);

    crate::error::Error::query(format!(
        "entity manager flush failed writing `{table}`: {rendered} — {detail}"
    ))
    .with_db_failure(failure)
}
