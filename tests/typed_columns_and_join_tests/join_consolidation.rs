use tideorm::prelude::JoinResultConsolidator;

#[derive(Debug, Clone, PartialEq)]
struct Order {
    id: i64,
    customer: String,
}

#[derive(Debug, Clone, PartialEq)]
struct LineItem {
    id: i64,
    order_id: i64,
    product: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Product {
    id: i64,
    name: String,
}

fn order(id: i64, customer: &str) -> Order {
    Order {
        id,
        customer: customer.into(),
    }
}

fn item(id: i64, order_id: i64, product: &str) -> LineItem {
    LineItem {
        id,
        order_id,
        product: product.into(),
    }
}

fn product(id: i64, name: &str) -> Product {
    Product {
        id,
        name: name.into(),
    }
}

/// Rows group by parent key even when they are not adjacent; parents keep the
/// order they first appear in, and each parent's children their row order.
#[test]
fn test_consolidate_two_groups_rows_by_parent_key() {
    let empty: Vec<(Order, LineItem)> = vec![];
    assert!(JoinResultConsolidator::consolidate_two(empty, |o| o.id).is_empty());

    let (alice, bob) = (order(1, "Alice"), order(2, "Bob"));
    let flat = vec![
        (alice.clone(), item(1, 1, "Widget")),
        (bob.clone(), item(3, 2, "Gizmo")),
        (alice.clone(), item(2, 1, "Gadget")),
    ];

    assert_eq!(
        JoinResultConsolidator::consolidate_two(flat, |o| o.id),
        vec![
            (alice, vec![item(1, 1, "Widget"), item(2, 1, "Gadget")]),
            (bob, vec![item(3, 2, "Gizmo")]),
        ]
    );
}

#[test]
fn test_consolidate_two_optional_keeps_parents_without_children() {
    let (alice, bob) = (order(1, "Alice"), order(2, "Bob"));
    let flat = vec![
        (alice.clone(), Some(item(1, 1, "Widget"))),
        (bob.clone(), None),
    ];

    assert_eq!(
        JoinResultConsolidator::consolidate_two_optional(flat, |o| o.id),
        vec![(alice, vec![item(1, 1, "Widget")]), (bob, vec![])]
    );
}

#[test]
fn test_consolidate_three_nests_two_levels() {
    let empty: Vec<(Order, LineItem, Product)> = vec![];
    assert!(JoinResultConsolidator::consolidate_three(empty, |o| o.id, |i| i.id).is_empty());

    let alice = order(1, "Alice");
    let (widget, gadget) = (item(1, 1, "Widget"), item(2, 1, "Gadget"));
    let flat = vec![
        (alice.clone(), widget.clone(), product(1, "Super Widget")),
        (alice.clone(), gadget.clone(), product(2, "Mega Gadget")),
    ];

    assert_eq!(
        JoinResultConsolidator::consolidate_three(flat, |o| o.id, |i| i.id),
        vec![(
            alice,
            vec![
                (widget, vec![product(1, "Super Widget")]),
                (gadget, vec![product(2, "Mega Gadget")]),
            ],
        )]
    );
}

#[test]
fn test_consolidate_three_optional_keeps_middle_rows_without_children() {
    let alice = order(1, "Alice");
    let (widget, custom) = (item(1, 1, "Widget"), item(2, 1, "Custom"));
    let flat = vec![
        (
            alice.clone(),
            widget.clone(),
            Some(product(1, "Super Widget")),
        ),
        (alice.clone(), custom.clone(), None),
    ];

    assert_eq!(
        JoinResultConsolidator::consolidate_three_optional(flat, |o| o.id, |i| i.id),
        vec![(
            alice,
            vec![(widget, vec![product(1, "Super Widget")]), (custom, vec![]),],
        )]
    );
}
