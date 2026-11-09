//! Typed columns through projections, joins, windows and conditions, raw
//! fragments with bound values, and the page and order helpers.

use crate::fixtures::{Post, User, memory_database};
use tideorm::prelude::*;

const USERS_AND_POSTS: &str = "CREATE TABLE review_users(id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN); \
     CREATE TABLE review_posts(id INTEGER PRIMARY KEY, user_id INTEGER, title TEXT); \
     INSERT INTO review_users VALUES(1,'Ada',1),(2,'Bo',0); \
     INSERT INTO review_posts VALUES(1,1,'first'),(2,1,'second'),(3,2,'third')";

fn ids<M: Model>(rows: &[M], key: impl Fn(&M) -> i64) -> Vec<i64> {
    rows.iter().map(key).collect()
}

#[tokio::test]
async fn typed_columns_select_join_and_filter() {
    let db = memory_database(USERS_AND_POSTS).await;

    let rows = Post::query_with(&db)
        .select((Post::ID, Post::TITLE))
        .add_select(Post::USER_ID)
        .order_asc(Post::ID)
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows, |post| post.id), [1, 2, 3]);

    // Another model's typed column is written with its table.
    let rows = Post::query_with(&db)
        .join::<User>(JoinType::Inner, Post::USER_ID, User::ID)
        .where_eq(User::ACTIVE, true)
        .order_asc(Post::ID)
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows, |post| post.id), [1, 2]);

    let rows = Post::query_with(&db)
        .join_as::<User>(JoinType::Left, "author", Post::USER_ID, "author.id")
        .where_raw("author.name = 'Bo'")
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows, |post| post.id), [3]);

    let rows = Post::query_with(&db)
        .inner_join("review_users", Post::USER_ID, User::ID)
        .where_ne(Post::TITLE, "first")
        .where_col(Post::ID.lt(3))
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows, |post| post.id), [2]);

    let rows = Post::query_with(&db)
        .where_eq(Post::ID, 1)
        .or_where_ne(Post::USER_ID, 1)
        .or_where_col(Post::TITLE.eq("second"))
        .order_asc(Post::ID)
        .get()
        .await
        .unwrap();
    assert!(rows.is_empty());
    let rows = Post::query_with(&db)
        .or_where_ne(Post::USER_ID, 1)
        .or_where_col(Post::TITLE.eq("second"))
        .order_asc(Post::ID)
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows, |post| post.id), [2, 3]);

    let newest = Post::query_with(&db)
        .latest_by(Post::ID)
        .first()
        .await
        .unwrap();
    assert_eq!(newest.map(|post| post.id), Some(3));
    let oldest = Post::query_with(&db)
        .oldest_by(Post::ID)
        .first()
        .await
        .unwrap();
    assert_eq!(oldest.map(|post| post.id), Some(1));
}

#[tokio::test]
async fn raw_having_and_union_fragments_bind_their_values() {
    let db = memory_database(USERS_AND_POSTS).await;

    let rows = Post::query_with(&db)
        .select_raw("user_id, COUNT(*) AS posts")
        .group_by("user_id")
        .having_raw_with("COUNT(*) > ?", [DbValue::from(1)])
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["user_id"], 1);

    let rows = Post::query_with(&db)
        .select_raw("user_id")
        .group_by("user_id")
        .having_raw("COUNT(*) = 1")
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["user_id"], 2);

    // The fragment's `?` binds after the first operand's values.
    let rows = Post::query_with(&db)
        .select([Post::ID])
        .where_eq(Post::ID, 1)
        .union_raw_with(
            "SELECT id FROM review_posts WHERE title = ?",
            [DbValue::from("third")],
        )
        .union_all_raw_with(
            "SELECT id FROM review_posts WHERE id = ?",
            [DbValue::from(1)],
        )
        .get_json()
        .await
        .unwrap();
    let mut found: Vec<i64> = rows.iter().map(|row| row["id"].as_i64().unwrap()).collect();
    found.sort_unstable();
    assert_eq!(found, [1, 1, 3]);

    // A value count that does not match the fragment's markers is refused.
    let refused = Post::query_with(&db)
        .union_raw_with("SELECT id FROM review_posts WHERE id = ?", [])
        .get_json()
        .await;
    assert!(refused.is_err());
}

#[tokio::test]
async fn window_helpers_and_ctes_take_typed_columns() {
    let db = memory_database(USERS_AND_POSTS).await;

    let rows = Post::query_with(&db)
        .row_number("position", (), Post::ID, Order::Desc)
        .running_sum("total", Post::ID, (), Post::ID, Order::Asc)
        .first_value(
            "first_title",
            Post::TITLE,
            Post::USER_ID,
            Post::ID,
            Order::Asc,
        )
        .order_asc(Post::ID)
        .get_json()
        .await
        .unwrap();
    let column = |name: &str| -> Vec<serde_json::Value> {
        rows.iter().map(|row| row[name].clone()).collect()
    };
    assert_eq!(column("position"), [3, 2, 1]);
    assert_eq!(column("total"), [1, 3, 6]);
    assert_eq!(column("first_title"), ["first", "first", "third"]);

    let rows = Post::query_with(&db)
        .window(
            WindowFunction::new(WindowFunctionType::count(Post::ID), "per_user")
                .partition_by(Post::USER_ID),
        )
        .order_asc(Post::ID)
        .get_json()
        .await
        .unwrap();
    let per_user: Vec<_> = rows.iter().map(|row| row["per_user"].clone()).collect();
    assert_eq!(per_user, [2, 2, 1]);

    let rows = Post::query_with(&db)
        .with_query_columns(
            "active_authors",
            ["author_id"],
            User::query_with(&db)
                .select([User::ID])
                .where_eq(User::ACTIVE, true),
        )
        .where_raw("user_id IN (SELECT author_id FROM active_authors)")
        .order_asc(Post::ID)
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows, |post| post.id), [1, 2]);

    let cte = CTE::with_columns("numbers", ["n"], String::from("SELECT 1"));
    assert_eq!(cte.columns.as_deref(), Some(&["n".to_string()][..]));
}

#[tokio::test]
async fn pages_map_and_report_their_neighbours() {
    let db = memory_database(USERS_AND_POSTS).await;

    let first = Post::query_with(&db)
        .order_asc(Post::ID)
        .paginate(1, 2)
        .await
        .unwrap();
    assert!(!first.has_previous_page());
    assert!(first.has_next_page());
    let titles = first.map(|post| post.title);
    assert_eq!(titles.items, ["first", "second"]);
    assert_eq!((titles.total, titles.page, titles.per_page), (3, 1, 2));

    let last = Post::query_with(&db)
        .order_asc(Post::ID)
        .paginate(2, 2)
        .await
        .unwrap();
    assert!(last.has_previous_page());
    assert!(!last.has_next_page());
}

#[test]
fn order_reads_from_requests_and_reverses() {
    assert_eq!(Order::default(), Order::Asc);
    assert_eq!("DESC".parse::<Order>().unwrap(), Order::Desc);
    assert_eq!(" ascending ".parse::<Order>().unwrap(), Order::Asc);
    assert!("sideways".parse::<Order>().is_err());
    assert_eq!(Order::Asc.reverse(), Order::Desc);
    assert_eq!(Order::Desc.to_string(), "DESC");
    assert_eq!(serde_json::to_value(Order::Desc).unwrap(), "desc");
    assert_eq!(
        serde_json::from_value::<Order>(serde_json::json!("Asc")).unwrap(),
        Order::Asc
    );
    assert!(serde_json::from_value::<Order>(serde_json::json!("up")).is_err());
}
