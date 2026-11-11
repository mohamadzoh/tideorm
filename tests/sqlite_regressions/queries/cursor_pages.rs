//! `cursor_paginate()` pages by primary key and hands back a cursor to continue from.

use crate::fixtures::{Item, items_database};
use tideorm::prelude::*;

fn ids(page: &CursorPage<Item>) -> Vec<i64> {
    page.items.iter().map(|item| item.id).collect()
}

#[tokio::test]
async fn cursor_pages_walk_the_rows_in_key_order() {
    let db = items_database().await;
    let first = Item::query_with(&db)
        .cursor_paginate(2, None)
        .await
        .unwrap();
    assert_eq!(ids(&first), [1, 2]);
    // A row written between requests is neither skipped nor repeated.
    db.exec_raw("INSERT INTO behavior_items VALUES (4, 40, 4.5), (0, 0, 0.5)")
        .await
        .unwrap();
    let second = Item::query_with(&db)
        .cursor_paginate(2, first.next_cursor.as_deref())
        .await
        .unwrap();
    assert_eq!(ids(&second), [3, 4]);
    let last = Item::query_with(&db)
        .cursor_paginate(2, second.next_cursor.as_deref())
        .await
        .unwrap();
    assert_eq!((ids(&last), last.next_cursor), (vec![], None));

    let descending = Item::query_with(&db)
        .where_gt("rank", 0)
        .order_desc("id")
        .cursor_paginate(3, None)
        .await
        .unwrap();
    assert_eq!(ids(&descending), [4, 3, 2]);
    let rest = Item::query_with(&db)
        .where_gt("rank", 0)
        .order_desc("id")
        .cursor_paginate(3, descending.next_cursor.as_deref())
        .await
        .unwrap();
    assert_eq!((ids(&rest), rest.next_cursor), (vec![1], None));
}

#[tokio::test]
async fn cursor_pages_refuse_bad_cursors_and_limits() {
    let db = items_database().await;
    // not base64url, not JSON, and a key that is neither a number nor a string
    for cursor in ["not a cursor", "AAAA", "W251bGwsZmFsc2Vd"] {
        let error = Item::query_with(&db)
            .cursor_paginate(2, Some(cursor))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, Error::Validation { field, .. } if field == "cursor"),
            "{cursor}: {error}"
        );
    }
    let error = Item::query_with(&db)
        .limit(1)
        .cursor_paginate(2, None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("limit()"), "{error}");
}
