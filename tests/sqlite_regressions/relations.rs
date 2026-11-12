//! Relation wrappers: their caches, many-to-many links changed in batches,
//! polymorphic reads, the eager-load terminals and nested saves.

use crate::fixtures::{global_memory_database, memory_database};
use tideorm::model::{NestedSave, NestedSaveBuilder};
use tideorm::prelude::*;

#[tideorm::model(table = "rel_authors")]
struct Author {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
    #[tideorm(has_many = "Book", foreign_key = "author_id")]
    books: HasMany<Book>,
    #[tideorm(morph_name = "imageable")]
    avatar: MorphOne<Image>,
}

#[tideorm::model(table = "rel_books")]
struct Book {
    #[tideorm(primary_key)]
    id: i64,
    author_id: i64,
    title: String,
    #[tideorm(belongs_to = "Author", foreign_key = "author_id")]
    author: BelongsTo<Author>,
    #[tideorm(
        has_many_through = "Tag",
        foreign_key = "book_id",
        related_key = "tag_id"
    )]
    tags: HasManyThrough<Tag, BookTag>,
    #[tideorm(morph_name = "imageable")]
    images: MorphMany<Image>,
}

#[tideorm::model(table = "rel_tags")]
struct Tag {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "rel_book_tags")]
struct BookTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    book_id: i64,
    tag_id: i64,
}

#[tideorm::model(table = "rel_images")]
struct Image {
    #[tideorm(primary_key)]
    id: i64,
    path: String,
    imageable_type: String,
    imageable_id: i64,
}

/// Ada wrote books 1 and 2 and Bo book 3; Ada and book 1 have images, and
/// tags 1 to 3 are linked to nothing.
const LIBRARY: &str = "CREATE TABLE rel_authors(id INTEGER PRIMARY KEY, name TEXT); \
     CREATE TABLE rel_books(id INTEGER PRIMARY KEY, author_id INTEGER, title TEXT); \
     CREATE TABLE rel_tags(id INTEGER PRIMARY KEY, name TEXT); \
     CREATE TABLE rel_book_tags(id INTEGER PRIMARY KEY AUTOINCREMENT, book_id INTEGER, \
     tag_id INTEGER, UNIQUE(book_id, tag_id)); \
     CREATE TABLE rel_images(id INTEGER PRIMARY KEY, path TEXT, imageable_type TEXT, \
     imageable_id INTEGER); \
     INSERT INTO rel_authors VALUES(1,'Ada'),(2,'Bo'); \
     INSERT INTO rel_books VALUES(1,1,'first'),(2,1,'second'),(3,2,'third'); \
     INSERT INTO rel_tags VALUES(1,'one'),(2,'two'),(3,'three'); \
     INSERT INTO rel_images VALUES(1,'ada.png','rel_authors',1),(2,'cover.png','rel_books',1), \
     (3,'back.png','rel_books',1)";

fn book(id: i64, title: &str) -> Book {
    Book {
        id,
        title: title.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn relation_caches_read_edit_and_clear() {
    let db = memory_database(LIBRARY).await;

    let plain = Author::query_with(&db).first().await.unwrap().unwrap();
    assert!(!plain.books.is_loaded());
    assert!(plain.books.get_cached().is_none());

    let mut ada = Author::query_with(&db)
        .with(Author::BOOKS)
        .find_or_fail(1)
        .await
        .unwrap();
    assert!(ada.books.is_loaded());
    assert_eq!(ada.books.get_cached().map(<[Book]>::len), Some(2));
    ada.books.as_mut().unwrap().retain(|book| book.id == 2);
    assert_eq!(ada.books.get_cached().unwrap()[0].title, "second");
    ada.books.clear();
    assert!(
        ada.books.is_loaded(),
        "a cleared relation is loaded and empty"
    );
    assert_eq!(ada.books.get_cached().map(<[Book]>::len), Some(0));

    let mut first = Book::query_with(&db)
        .with(Book::AUTHOR)
        .find_or_fail(1)
        .await
        .unwrap();
    first.author.as_mut().unwrap().name = "Ada L.".into();
    assert_eq!(first.author.get_cached().unwrap().name, "Ada L.");
    first.author.clear();
    assert!(first.author.is_loaded());
    assert!(first.author.get_cached().is_none());
}

#[tokio::test]
async fn many_to_many_links_change_in_batches() {
    async fn tag_ids(book: &Book) -> Vec<i64> {
        let mut ids: Vec<i64> = book
            .tags
            .load()
            .await
            .unwrap()
            .iter()
            .map(|tag| tag.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    let _db = global_memory_database(LIBRARY).await;
    let first = Book::find(1).await.unwrap().unwrap();
    assert!(!first.tags.exists().await.unwrap());

    first.tags.attach_many([1_i64, 2, 2]).await.unwrap();
    assert_eq!(tag_ids(&first).await, [1, 2]);
    assert!(first.tags.exists().await.unwrap());
    // Every id is checked before any is linked.
    assert!(first.tags.attach_many([Some(3_i64), None]).await.is_err());
    assert_eq!(tag_ids(&first).await, [1, 2]);

    assert_eq!(first.tags.detach_many([1_i64, 3]).await.unwrap(), 1);
    assert_eq!(first.tags.detach_many(Vec::<i64>::new()).await.unwrap(), 0);
    assert_eq!(tag_ids(&first).await, [2]);

    first
        .tags
        .sync((1..=3).filter(|id| id % 2 == 1))
        .await
        .unwrap();
    assert_eq!(tag_ids(&first).await, [1, 3]);
    assert_eq!(first.tags.detach_all().await.unwrap(), 2);
    assert!(!first.tags.exists().await.unwrap());
    Database::reset_global();
}

#[tokio::test]
async fn polymorphic_relations_check_and_refine_their_rows() {
    let _db = global_memory_database(LIBRARY).await;
    let ada = Author::find(1).await.unwrap().unwrap();
    assert!(ada.avatar.exists().await.unwrap());
    let found = ada
        .avatar
        .load_with(|query| query.where_eq(Image::PATH, "ada.png"))
        .await
        .unwrap();
    assert_eq!(found.map(|image| image.id), Some(1));
    // The refinement narrows the owner's rows; it never reaches another's.
    let other = ada
        .avatar
        .load_with(|query| query.where_eq(Image::PATH, "cover.png"))
        .await
        .unwrap();
    assert!(other.is_none());
    assert!(
        !Author::find(2)
            .await
            .unwrap()
            .unwrap()
            .avatar
            .exists()
            .await
            .unwrap()
    );

    assert!(
        Book::find(1)
            .await
            .unwrap()
            .unwrap()
            .images
            .exists()
            .await
            .unwrap()
    );
    assert!(
        !Book::find(2)
            .await
            .unwrap()
            .unwrap()
            .images
            .exists()
            .await
            .unwrap()
    );
    Database::reset_global();
}

#[tokio::test]
async fn eager_queries_page_count_and_fail_when_empty() {
    let db = memory_database(LIBRARY).await;

    let page = Author::query_with(&db)
        .order_asc(Author::ID)
        .with(Author::BOOKS)
        .paginate(2, 1)
        .await
        .unwrap();
    assert_eq!((page.total, page.items.len()), (2, 1));
    assert!(page.has_previous_page());
    assert_eq!(page.items[0].id, 2);
    assert_eq!(page.items[0].books.get_cached().map(<[Book]>::len), Some(1));

    let eager = || Author::query_with(&db).with_many([Author::BOOKS]);
    assert_eq!(eager().count().await.unwrap(), 2);
    let ada = eager().first_or_fail().await.unwrap();
    assert_eq!(ada.books.get_cached().map(<[Book]>::len), Some(2));
    assert!(matches!(
        eager()
            .query(|query| query.where_eq(Author::ID, 9))
            .first_or_fail()
            .await,
        Err(Error::NotFound { .. })
    ));
    assert_eq!(eager().find_or_fail(2).await.unwrap().name, "Bo");
    assert!(matches!(
        eager().find_or_fail(9).await,
        Err(Error::NotFound { .. })
    ));

    // Only the relations asked for are loaded.
    assert!(!ada.avatar.is_loaded());
}

#[tokio::test]
async fn nested_saves_take_typed_keys_and_any_list() {
    let _db = global_memory_database(LIBRARY).await;
    let author = Author {
        id: 5,
        name: "Le Guin".into(),
        ..Default::default()
    };

    let (author, books) = author
        .save_with_many([book(10, "a"), book(11, "b")], Book::AUTHOR_ID)
        .await
        .unwrap();
    assert!(books.iter().all(|book| book.author_id == 5));
    let (author, twelfth) = author
        .save_with_one(book(12, "c"), Book::AUTHOR_ID)
        .await
        .unwrap();
    assert_eq!(twelfth.author_id, 5);

    let (_, saved) = NestedSaveBuilder::new(Author {
        id: 6,
        name: "Butler".into(),
        ..Default::default()
    })
    .with_many([book(14, "e")], Book::AUTHOR_ID)
    .with_one(book(13, "d"), "author_id")
    .save()
    .await
    .unwrap();
    // In the order the relations were added.
    assert_eq!(saved[0].as_many::<Book>().map(<[Book]>::len), Some(1));
    assert!(saved[0].as_one::<Book>().is_none());
    assert_eq!(saved[1].as_one::<Book>().map(|book| book.id), Some(13));
    assert!(saved[1].as_many::<Book>().is_none());
    assert!(saved[1].as_one::<Author>().is_none(), "another model type");

    assert_eq!(author.delete_with_one(twelfth).await.unwrap(), 2);
    assert!(Author::find(5).await.unwrap().is_none());
    assert!(Book::find(12).await.unwrap().is_none());
    assert!(Book::find(11).await.unwrap().is_some());
    Database::reset_global();
}
