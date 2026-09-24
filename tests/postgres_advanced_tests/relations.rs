use super::*;

pub(super) async fn test_relations() {
    let author1 = TestAuthor {
        id: 0,
        name: "J.K. Rowling".into(),
        country: "UK".into(),
        ..Default::default()
    }
    .save()
    .await
    .expect("Failed to save author 1");

    let author2 = TestAuthor {
        id: 0,
        name: "George R.R. Martin".into(),
        country: "USA".into(),
        ..Default::default()
    }
    .save()
    .await
    .expect("Failed to save author 2");

    let mut books = Vec::new();
    for (author_id, title, year) in [
        (author1.id, "Harry Potter and the Philosopher's Stone", 1997),
        (author1.id, "Harry Potter and the Chamber of Secrets", 1998),
        (author2.id, "A Game of Thrones", 1996),
    ] {
        books.push(
            TestBook {
                id: 0,
                author_id,
                title: title.into(),
                year,
                ..Default::default()
            }
            .save()
            .await
            .expect("Failed to save book"),
        );
    }

    for (book, isbn, pages) in [
        (&books[0], "978-0747532699", 223),
        (&books[1], "978-0747538493", 251),
        (&books[2], "978-0553103540", 694),
    ] {
        TestBookDetail {
            id: 0,
            book_id: book.id,
            isbn: isbn.into(),
            pages,
            ..Default::default()
        }
        .save()
        .await
        .expect("Failed to save book detail");
    }

    // Relation fields on loaded models arrive wired to their keys.
    let rowling = TestAuthor::find(author1.id)
        .await
        .expect("Query failed")
        .expect("Rowling should exist");
    let rowling_books = rowling.books.load().await.expect("Failed to load has_many");
    assert_eq!(
        rowling_books.len(),
        2,
        "HasMany loads both of Rowling's books"
    );

    let got = TestBook::query()
        .where_eq("title", "A Game of Thrones")
        .first()
        .await
        .expect("Query failed")
        .expect("Book should exist");
    let got_author = got
        .author
        .load()
        .await
        .expect("Failed to load belongs_to")
        .expect("BelongsTo should find the author");
    assert_eq!(got_author.name, "George R.R. Martin");

    let got_detail = got
        .detail
        .load()
        .await
        .expect("Failed to load has_one")
        .expect("HasOne should find the detail");
    assert_eq!(got_detail.isbn, "978-0553103540");
    assert_eq!(got_detail.pages, 694);

    // Wrappers built by hand load the same rows.
    let manual_books = HasMany::<TestBook>::new("author_id", "id")
        .with_parent_pk(serde_json::json!(author1.id))
        .load()
        .await
        .expect("Failed to load hand-built has_many");
    assert_eq!(manual_books.len(), 2);
    let manual_author = BelongsTo::<TestAuthor>::new("author_id", "id")
        .with_fk_value(serde_json::json!(got.author_id))
        .load()
        .await
        .expect("Failed to load hand-built belongs_to")
        .expect("hand-built BelongsTo should find the author");
    assert_eq!(manual_author.id, author2.id);
    let manual_detail = HasOne::<TestBookDetail>::new("book_id", "id")
        .with_parent_pk(serde_json::json!(books[0].id))
        .load()
        .await
        .expect("Failed to load hand-built has_one")
        .expect("hand-built HasOne should find the detail");
    assert_eq!(manual_detail.isbn, "978-0747532699");

    let uk_author_books = TestBook::query()
        .inner_join("test_authors", "test_books.author_id", "test_authors.id")
        .where_eq("test_authors.country", "UK")
        .get()
        .await
        .expect("Query failed");
    assert_eq!(uk_author_books.len(), 2, "UK authors should have 2 books");

    let books_with_many_pages = TestBook::query()
        .inner_join(
            "test_book_details",
            "test_books.id",
            "test_book_details.book_id",
        )
        .where_gt("test_book_details.pages", 500)
        .get()
        .await
        .expect("Query failed");
    assert_eq!(
        books_with_many_pages.len(),
        1,
        "one book has over 500 pages"
    );
    assert_eq!(books_with_many_pages[0].title, "A Game of Thrones");
}
