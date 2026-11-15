//! Models whose relations lead back to themselves compile with the entity
//! manager on, and their saves sync the relations.

use std::sync::Arc;

use crate::fixtures::memory_database;
use tideorm::entity_manager::EntityManager;
use tideorm::prelude::*;

/// A tree: its `has_many` is of its own type.
#[tideorm::model(table = "cycle_nodes")]
struct CycleNode {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    parent_id: Option<i64>,
    name: String,
    #[tideorm(has_many = "CycleNode", foreign_key = "parent_id")]
    children: HasMany<CycleNode>,
}

/// `has_many_through` both ways between two models.
#[tideorm::model(table = "cycle_books")]
struct CycleBook {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    title: String,
    #[tideorm(
        has_many_through = "CycleAuthor",
        pivot = "cycle_book_authors",
        foreign_key = "book_id",
        related_key = "author_id"
    )]
    authors: HasManyThrough<CycleAuthor, CycleBookAuthor>,
}

#[tideorm::model(table = "cycle_authors")]
struct CycleAuthor {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    #[tideorm(
        has_many_through = "CycleBook",
        pivot = "cycle_book_authors",
        foreign_key = "author_id",
        related_key = "book_id"
    )]
    books: HasManyThrough<CycleBook, CycleBookAuthor>,
}

#[tideorm::model(table = "cycle_book_authors")]
struct CycleBookAuthor {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    book_id: i64,
    author_id: i64,
}

#[tokio::test]
async fn a_tree_saves_the_children_it_was_given() {
    let db = memory_database(
        "CREATE TABLE cycle_nodes(id INTEGER PRIMARY KEY AUTOINCREMENT, \
             parent_id INTEGER, name TEXT); \
         INSERT INTO cycle_nodes VALUES (1, NULL, 'root')",
    )
    .await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let mut root = manager.find::<CycleNode>(1).await.unwrap().unwrap();
    manager.load(&mut root.children).await.unwrap();
    root.children.as_mut().unwrap().push(CycleNode {
        id: 0,
        parent_id: None,
        name: "leaf".into(),
        children: Default::default(),
    });
    manager.save(&root).await.unwrap();

    let leaves = CycleNode::query_with(&db)
        .where_eq(CycleNode::PARENT_ID, 1)
        .get()
        .await
        .unwrap();
    assert_eq!(leaves.len(), 1);
    assert_eq!(leaves[0].name, "leaf");
}

#[tokio::test]
async fn a_two_way_through_relation_saves_its_pivot_rows() {
    let db = memory_database(
        "CREATE TABLE cycle_books(id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT); \
         CREATE TABLE cycle_authors(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT); \
         CREATE TABLE cycle_book_authors(id INTEGER PRIMARY KEY AUTOINCREMENT, \
             book_id INTEGER, author_id INTEGER); \
         INSERT INTO cycle_books VALUES (1, 'b'); \
         INSERT INTO cycle_authors VALUES (1, 'a')",
    )
    .await;
    let manager = EntityManager::new(Arc::new(db.clone()));
    let mut book = manager.find::<CycleBook>(1).await.unwrap().unwrap();
    manager.load(&mut book.authors).await.unwrap();
    let author = manager.find::<CycleAuthor>(1).await.unwrap().unwrap();
    book.authors.as_mut().unwrap().push(author);
    manager.save(&book).await.unwrap();

    let pivots = CycleBookAuthor::query_with(&db).get().await.unwrap();
    assert_eq!(
        pivots
            .iter()
            .map(|row| (row.book_id, row.author_id))
            .collect::<Vec<_>>(),
        [(1, 1)]
    );
}
