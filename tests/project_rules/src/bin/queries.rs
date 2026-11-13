//! Models that keep the model rules of `../../tideorm.toml`, and queries that
//! must be refused or allowed by its query rules. Panics on the first query
//! that is not.

use tideorm::migration::*;
use tideorm::prelude::*;

#[tideorm::model(table = "users", hidden = "password")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub password: String,
    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
    #[tideorm(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,
}

#[tideorm::model(table = "posts")]
#[index("user_id")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    #[tideorm(belongs_to = "User", foreign_key = "user_id")]
    pub user: BelongsTo<User>,
}

#[tideorm::model(table = "profiles")]
#[index("user_id")]
pub struct Profile {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub bio: String,
}

/// Exempt from `forbid_raw_sql` and `max_rows`.
#[tideorm::model(table = "audit_logs")]
pub struct AuditLog {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub action: String,
}

#[derive(FromQueryResult)]
struct Total {
    #[allow(dead_code)]
    total: i64,
}

struct IndexTitles;

#[async_trait]
impl Migration for IndexTitles {
    fn version(&self) -> &str {
        "20260101_000001"
    }
    fn name(&self) -> &str {
        "index_titles"
    }
    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .raw("CREATE INDEX posts_title ON posts (title)")
            .await
    }
    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.raw("DROP INDEX posts_title").await
    }
}

fn refused<T>(what: &str, result: tideorm::Result<T>, rule: &str) {
    match result {
        Ok(_) => panic!("{what} ran, but {rule} should refuse it"),
        Err(error) => assert!(
            error.to_string().contains(&format!("({rule} in tideorm.toml)")),
            "{what} was refused for another reason than {rule}: {error}"
        ),
    }
}

fn allowed<T>(what: &str, result: tideorm::Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{what} was refused: {error}"))
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .models::<(User, Post, Profile, AuditLog)>()
        .sync(true)
        .connect()
        .await?;
    let mut users = Vec::new();
    for (n, posts) in [4, 2, 2].into_iter().enumerate() {
        let user = User {
            email: format!("user{n}@example.com"),
            password: "secret".into(),
            ..Default::default()
        }
        .save()
        .await?;
        for p in 0..posts {
            Post {
                user_id: user.id,
                title: format!("post {p}"),
                ..Default::default()
            }
            .save()
            .await?;
        }
        Profile {
            user_id: user.id,
            bio: "bio".into(),
            ..Default::default()
        }
        .save()
        .await?;
        users.push(user);
    }
    for n in 0..5 {
        AuditLog {
            action: format!("action {n}"),
            ..Default::default()
        }
        .save()
        .await?;
    }

    // max_rows = 3
    refused("Post::all()", Post::all().await, "max_rows");
    refused("get() without a limit", Post::query().get().await, "max_rows");
    refused("limit(4)", Post::query().limit(4).get().await, "max_rows");
    refused(
        "get_json() without a limit",
        Post::query().get_json().await,
        "max_rows",
    );
    refused(
        "pluck() without a limit",
        Post::query().pluck(Post::ID).await,
        "max_rows",
    );
    refused("Post::paginate(1, 4)", Post::paginate(1, 4).await, "max_rows");
    refused(
        "query().paginate(1, 4)",
        Post::query().paginate(1, 4).await,
        "max_rows",
    );
    refused(
        "chunk(4)",
        Post::query()
            .chunk(4, |_| async { Ok(()) })
            .await,
        "max_rows",
    );
    let limited = allowed("limit(3)", Post::query().limit(3).get().await);
    assert_eq!(limited.len(), 3);
    allowed("pluck() with limit(3)", Post::query().limit(3).pluck(Post::ID).await);
    assert_eq!(allowed("count()", Post::query().count().await), 8);
    allowed("exists()", Post::query().exists().await);
    allowed("first()", Post::query().first().await);
    allowed("Post::find(1)", Post::find(1).await);
    allowed("Post::paginate(1, 3)", Post::paginate(1, 3).await);
    let page = allowed("query().paginate(2, 3)", Post::query().paginate(2, 3).await);
    assert_eq!((page.items.len(), page.total), (3, 8));
    let mut seen = 0;
    allowed(
        "chunk(3)",
        Post::query()
            .chunk(3, |batch| {
                seen += batch.len();
                async { Ok(()) }
            })
            .await,
    );
    assert_eq!(seen, 8, "chunk(3) reads every row in batches");
    let cursor = allowed(
        "cursor_paginate(3)",
        Post::query().cursor_paginate(3, None).await,
    );
    assert_eq!(cursor.items.len(), 3);

    // max_in_values = 2
    allowed(
        "where_in of 2",
        Post::query().where_in(Post::ID, [1, 2]).limit(3).get().await,
    );
    refused(
        "where_in of 3",
        Post::query().where_in(Post::ID, [1, 2, 3]).limit(3).get().await,
        "max_in_values",
    );
    refused(
        "where_not_in of 3 in an OR group",
        Post::query()
            .begin_or()
            .or_where_not_in(Post::ID, [1, 2, 3])
            .end_or()
            .limit(3)
            .get()
            .await,
        "max_in_values",
    );

    // max_joins = 1
    allowed(
        "one join",
        Post::query()
            .inner_join("users", "posts.user_id", "users.id")
            .limit(3)
            .get()
            .await,
    );
    refused(
        "two joins",
        Post::query()
            .inner_join("users", "posts.user_id", "users.id")
            .inner_join("profiles", "profiles.user_id", "users.id")
            .limit(3)
            .get()
            .await,
        "max_joins",
    );

    // forbid_raw_sql, with AuditLog exempt
    let raw = || Post::query().limit(3);
    refused("where_raw()", raw().where_raw("1 = 1").get().await, "forbid_raw_sql");
    refused("select_raw()", raw().select_raw("1 AS one").get_json().await, "forbid_raw_sql");
    refused(
        "order_by_raw()",
        raw().order_by_raw("LOWER(title)", Order::Asc).get().await,
        "forbid_raw_sql",
    );
    refused("having_raw()", raw().having_raw("COUNT(*) > 0").count().await, "forbid_raw_sql");
    refused(
        "union_raw()",
        raw().union_raw("SELECT id, user_id, title FROM posts").get().await,
        "forbid_raw_sql",
    );
    refused("validate()", raw().where_raw("1 = 1").validate(), "forbid_raw_sql");
    refused(
        "a Custom window function",
        raw()
            .window(WindowFunction::new(
                WindowFunctionType::Custom("NTILE(2)".into()),
                "half",
            ))
            .get_json()
            .await,
        "forbid_raw_sql",
    );
    refused(
        "set_trusted_raw()",
        Post::update_all()
            .set_trusted_raw("title", "title")
            .where_eq(Post::ID, 1)
            .execute()
            .await,
        "forbid_raw_sql",
    );
    refused(
        "Database::execute()",
        Database::execute("UPDATE posts SET title = title WHERE id = 0").await,
        "forbid_raw_sql",
    );
    refused(
        "Database::raw_json()",
        Database::raw_json("SELECT 1 AS total").await,
        "forbid_raw_sql",
    );
    refused(
        "Database::raw_as()",
        Database::raw_as::<Total>("SELECT 1 AS total").await,
        "forbid_raw_sql",
    );
    refused(
        "Database::raw::<Post>()",
        Database::raw::<Post>("SELECT * FROM posts").await,
        "forbid_raw_sql",
    );
    refused(
        "an exempt model's query embedding raw SQL of Post",
        AuditLog::query()
            .where_exists(Post::query().where_raw("posts.id = audit_logs.id"))
            .get()
            .await,
        "forbid_raw_sql",
    );
    let logs = allowed(
        "Database::raw::<AuditLog>()",
        Database::raw::<AuditLog>("SELECT * FROM audit_logs").await,
    );
    assert_eq!(logs.len(), 5);
    let logs = allowed(
        "AuditLog's where_raw() and get() without a limit",
        AuditLog::query().where_raw("1 = 1").get().await,
    );
    assert_eq!(logs.len(), 5);
    allowed(
        "Post's query embedding the exempt AuditLog's raw SQL",
        Post::query()
            .where_in_subquery(Post::ID, AuditLog::query().select(["id"]).where_raw("1 = 1"))
            .limit(3)
            .get()
            .await,
    );
    let migrated = allowed(
        "a migration's raw DDL",
        Migrator::new()
            .add(IndexTitles)
            .run()
            .await
            .map_err(|failure| failure.error),
    );
    assert_eq!(migrated.applied.len(), 1);

    // max_eager_depth = 1, max_eager_relations = 1. The key lookups and rows
    // of an eager load are TideORM's own: 3 keys pass max_in_values = 2, and
    // 8 posts max_rows = 3.
    let loaded = allowed(
        "with(\"posts\")",
        User::query().limit(3).with("posts").get().await,
    );
    let posts: usize = loaded
        .iter()
        .map(|user| user.posts.get_cached().map_or(0, <[Post]>::len))
        .sum();
    assert_eq!(posts, 8);
    refused(
        "with(\"posts.user\")",
        User::query().limit(3).with("posts.user").get().await,
        "max_eager_depth",
    );
    refused(
        "with(\"posts\").with(\"profile\")",
        User::query().limit(3).with("posts").with("profile").get().await,
        "max_eager_relations",
    );
    // A relation's own reads are not held to max_rows.
    let first = allowed("User::find()", User::find(users[0].id).await)
        .expect("the first user");
    assert_eq!(allowed("posts.load()", first.posts.load().await).len(), 4);

    println!("all project rules held");
    Ok(())
}
