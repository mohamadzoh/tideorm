use serde::{Deserialize, Serialize};
use tideorm::prelude::JoinResultConsolidator;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Order {
    id: i64,
    customer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct LineItem {
    id: i64,
    order_id: i64,
    product: String,
    qty: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Product {
    id: i64,
    name: String,
}

#[test]
fn test_consolidate_two_empty() {
    let flat: Vec<(Order, LineItem)> = vec![];
    let result = JoinResultConsolidator::consolidate_two(flat, |o| o.id);
    assert!(result.is_empty());
}

#[test]
fn test_consolidate_two_single_order_single_item() {
    let flat = vec![(
        Order {
            id: 1,
            customer: "Alice".into(),
        },
        LineItem {
            id: 1,
            order_id: 1,
            product: "Widget".into(),
            qty: 2,
        },
    )];

    let result = JoinResultConsolidator::consolidate_two(flat, |o| o.id);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0.id, 1);
    assert_eq!(result[0].1.len(), 1);
    assert_eq!(result[0].1[0].product, "Widget");
}

#[test]
fn test_consolidate_two_single_order_multiple_items() {
    let order = Order {
        id: 1,
        customer: "Alice".into(),
    };
    let flat = vec![
        (
            order.clone(),
            LineItem {
                id: 1,
                order_id: 1,
                product: "Widget".into(),
                qty: 2,
            },
        ),
        (
            order.clone(),
            LineItem {
                id: 2,
                order_id: 1,
                product: "Gadget".into(),
                qty: 1,
            },
        ),
        (
            order.clone(),
            LineItem {
                id: 3,
                order_id: 1,
                product: "Gizmo".into(),
                qty: 5,
            },
        ),
    ];

    let result = JoinResultConsolidator::consolidate_two(flat, |o| o.id);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0.customer, "Alice");
    assert_eq!(result[0].1.len(), 3);
}

#[test]
fn test_consolidate_two_multiple_orders() {
    let order1 = Order {
        id: 1,
        customer: "Alice".into(),
    };
    let order2 = Order {
        id: 2,
        customer: "Bob".into(),
    };

    let flat = vec![
        (
            order1.clone(),
            LineItem {
                id: 1,
                order_id: 1,
                product: "Widget".into(),
                qty: 2,
            },
        ),
        (
            order1.clone(),
            LineItem {
                id: 2,
                order_id: 1,
                product: "Gadget".into(),
                qty: 1,
            },
        ),
        (
            order2.clone(),
            LineItem {
                id: 3,
                order_id: 2,
                product: "Gizmo".into(),
                qty: 5,
            },
        ),
    ];

    let result = JoinResultConsolidator::consolidate_two(flat, |o| o.id);

    assert_eq!(result.len(), 2);

    let alice_order = result.iter().find(|(o, _)| o.customer == "Alice").unwrap();
    assert_eq!(alice_order.1.len(), 2);

    let bob_order = result.iter().find(|(o, _)| o.customer == "Bob").unwrap();
    assert_eq!(bob_order.1.len(), 1);
}

#[test]
fn test_consolidate_two_optional_with_nulls() {
    let order1 = Order {
        id: 1,
        customer: "Alice".into(),
    };
    let order2 = Order {
        id: 2,
        customer: "Bob".into(),
    };

    let flat: Vec<(Order, Option<LineItem>)> = vec![
        (
            order1.clone(),
            Some(LineItem {
                id: 1,
                order_id: 1,
                product: "Widget".into(),
                qty: 2,
            }),
        ),
        (order2.clone(), None),
    ];

    let result = JoinResultConsolidator::consolidate_two_optional(flat, |o| o.id);

    assert_eq!(result.len(), 2);

    let alice_order = result.iter().find(|(o, _)| o.customer == "Alice").unwrap();
    assert_eq!(alice_order.1.len(), 1);

    let bob_order = result.iter().find(|(o, _)| o.customer == "Bob").unwrap();
    assert_eq!(bob_order.1.len(), 0);
}

#[test]
fn test_consolidate_three_empty() {
    let flat: Vec<(Order, LineItem, Product)> = vec![];
    let result = JoinResultConsolidator::consolidate_three(flat, |o| o.id, |i| i.id);
    assert!(result.is_empty());
}

#[test]
fn test_consolidate_three_nested() {
    let order = Order {
        id: 1,
        customer: "Alice".into(),
    };
    let item1 = LineItem {
        id: 1,
        order_id: 1,
        product: "Widget".into(),
        qty: 2,
    };
    let item2 = LineItem {
        id: 2,
        order_id: 1,
        product: "Gadget".into(),
        qty: 1,
    };
    let prod1 = Product {
        id: 1,
        name: "Super Widget".into(),
    };
    let prod2 = Product {
        id: 2,
        name: "Mega Gadget".into(),
    };

    let flat = vec![
        (order.clone(), item1.clone(), prod1.clone()),
        (order.clone(), item2.clone(), prod2.clone()),
    ];

    let result = JoinResultConsolidator::consolidate_three(flat, |o| o.id, |i| i.id);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0.customer, "Alice");
    assert_eq!(result[0].1.len(), 2);

    for (_item, products) in &result[0].1 {
        assert_eq!(products.len(), 1);
    }
}

#[test]
fn test_consolidate_three_optional() {
    let order = Order {
        id: 1,
        customer: "Alice".into(),
    };
    let item1 = LineItem {
        id: 1,
        order_id: 1,
        product: "Widget".into(),
        qty: 2,
    };
    let item2 = LineItem {
        id: 2,
        order_id: 1,
        product: "Custom".into(),
        qty: 1,
    };
    let prod1 = Product {
        id: 1,
        name: "Super Widget".into(),
    };

    let flat: Vec<(Order, LineItem, Option<Product>)> = vec![
        (order.clone(), item1.clone(), Some(prod1.clone())),
        (order.clone(), item2.clone(), None),
    ];

    let result = JoinResultConsolidator::consolidate_three_optional(flat, |o| o.id, |i| i.id);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].1.len(), 2);

    let widget_item = result[0]
        .1
        .iter()
        .find(|(i, _)| i.product == "Widget")
        .unwrap();
    assert_eq!(widget_item.1.len(), 1);

    let custom_item = result[0]
        .1
        .iter()
        .find(|(i, _)| i.product == "Custom")
        .unwrap();
    assert_eq!(custom_item.1.len(), 0);
}

#[test]
fn test_consolidate_preserves_order() {
    let order = Order {
        id: 1,
        customer: "Test".into(),
    };

    let flat = vec![
        (
            order.clone(),
            LineItem {
                id: 1,
                order_id: 1,
                product: "First".into(),
                qty: 1,
            },
        ),
        (
            order.clone(),
            LineItem {
                id: 2,
                order_id: 1,
                product: "Second".into(),
                qty: 1,
            },
        ),
        (
            order.clone(),
            LineItem {
                id: 3,
                order_id: 1,
                product: "Third".into(),
                qty: 1,
            },
        ),
    ];

    let result = JoinResultConsolidator::consolidate_two(flat, |o| o.id);

    assert_eq!(result[0].1[0].product, "First");
    assert_eq!(result[0].1[1].product, "Second");
    assert_eq!(result[0].1[2].product, "Third");
}
