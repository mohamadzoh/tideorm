use super::*;

pub(super) async fn test_relations() {
    println!("🔗 Testing: Relations");

    let _ = Database::execute("TRUNCATE TABLE test_book_details RESTART IDENTITY CASCADE").await;
    let _ = Database::execute("TRUNCATE TABLE test_books RESTART IDENTITY CASCADE").await;
    let _ = Database::execute("TRUNCATE TABLE test_authors RESTART IDENTITY CASCADE").await;

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

    let book1 = TestBook {
        id: 0,
        author_id: author1.id,
        title: "Harry Potter and the Philosopher's Stone".into(),
        year: 1997,
        ..Default::default()
    }
    .save()
    .await
    .expect("Failed to save book 1");

    let book2 = TestBook {
        id: 0,
        author_id: author1.id,
        title: "Harry Potter and the Chamber of Secrets".into(),
        year: 1998,
        ..Default::default()
    }
    .save()
    .await
    .expect("Failed to save book 2");

    let book3 = TestBook {
        id: 0,
        author_id: author2.id,
        title: "A Game of Thrones".into(),
        year: 1996,
        ..Default::default()
    }
    .save()
    .await
    .expect("Failed to save book 3");

    let _detail1 = TestBookDetail {
        id: 0,
        book_id: book1.id,
        isbn: "978-0747532699".into(),
        pages: 223,
        ..Default::default()
    }
    .save()
    .await
    .expect("Failed to save detail 1");

    let _detail2 = TestBookDetail {
        id: 0,
        book_id: book2.id,
        isbn: "978-0747538493".into(),
        pages: 251,
        ..Default::default()
    }
    .save()
    .await
    .expect("Failed to save detail 2");

    let _detail3 = TestBookDetail {
        id: 0,
        book_id: book3.id,
        isbn: "978-0553103540".into(),
        pages: 694,
        ..Default::default()
    }
    .save()
    .await
    .expect("Failed to save detail 3");

    {
        let books = TestBook::query()
            .where_eq("author_id", author1.id)
            .get()
            .await
            .expect("Query failed");

        assert_eq!(books.len(), 2, "J.K. Rowling should have 2 books");
        println!("   ✓ BelongsTo - query by foreign key");
    }

    {
        let author2_books = TestBook::query()
            .where_eq("author_id", author2.id)
            .get()
            .await
            .expect("Query failed");

        assert_eq!(
            author2_books.len(),
            1,
            "George R.R. Martin should have 1 book"
        );
        println!("   ✓ HasMany - query related records");
    }

    {
        let detail = TestBookDetail::query()
            .where_eq("book_id", book1.id)
            .first()
            .await
            .expect("Query failed");

        assert!(detail.is_some(), "Book 1 should have details");
        let detail = detail.unwrap();
        assert_eq!(detail.isbn, "978-0747532699");
        assert_eq!(detail.pages, 223);
        println!("   ✓ HasOne - query related record");
    }

    {
        let uk_author_books = TestBook::query()
            .inner_join("test_authors", "test_books.author_id", "test_authors.id")
            .where_eq("test_authors.country", "UK")
            .get()
            .await
            .expect("Query failed");

        assert_eq!(uk_author_books.len(), 2, "UK authors should have 2 books");
        println!("   ✓ JOIN across relations");
    }

    {
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
            "Should find 1 book with > 500 pages"
        );
        println!("   ✓ JOIN with conditions on related table");
    }

    {
        let mut rowling = TestAuthor::query()
            .where_eq("name", "J.K. Rowling")
            .first()
            .await
            .expect("Query failed")
            .expect("Author should exist");

        rowling.books =
            HasMany::new("author_id", "id").with_parent_pk(serde_json::json!(rowling.id));
        let rowling_books = rowling.books.load().await.expect("Failed to load has_many");
        assert_eq!(
            rowling_books.len(),
            2,
            "Rowling should have 2 books via load()"
        );

        let mut got_book = TestBook::query()
            .where_eq("title", "A Game of Thrones")
            .first()
            .await
            .expect("Query failed")
            .expect("Book should exist");

        got_book.author =
            BelongsTo::new("author_id", "id").with_fk_value(serde_json::json!(got_book.author_id));
        let got_author = got_book
            .author
            .load()
            .await
            .expect("Failed to load belongs_to");
        assert_eq!(
            got_author.unwrap().name,
            "George R.R. Martin",
            "BelongsTo should fetch correct author"
        );

        got_book.detail =
            HasOne::new("book_id", "id").with_parent_pk(serde_json::json!(got_book.id));
        let got_detail = got_book
            .detail
            .load()
            .await
            .expect("Failed to load has_one");
        assert!(got_detail.is_some(), "HasOne should return a detail");
        let got_detail = got_detail.unwrap();
        assert_eq!(got_detail.isbn, "978-0553103540");
        assert_eq!(got_detail.pages, 694);
        println!("   ✓ Field-based relation loading (belongs_to / has_one / has_many)");
    }

    println!();
}
