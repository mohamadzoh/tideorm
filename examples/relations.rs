use tideorm::prelude::*;

#[tideorm::model(table = "example_authors")]
struct Author {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    #[tideorm(has_many = "Post", foreign_key = "author_id")]
    posts: HasMany<Post>,
}

#[tideorm::model(table = "example_posts")]
struct Post {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    author_id: i64,
    title: String,
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .auto_models()
        .sync(true)
        .connect()
        .await?;
    let author = Author {
        id: 0,
        name: "Ada".into(),
        posts: HasMany::new("author_id", "id"),
    }
    .save()
    .await?;
    Post {
        id: 0,
        author_id: author.id,
        title: "First post".into(),
    }
    .save()
    .await?;
    let author = Author::query().with(Author::POSTS).first().await?.unwrap();
    let posts: Vec<Post> = author.get_relation(Author::POSTS).unwrap();
    assert_eq!(posts.len(), 1);
    Ok(())
}
