#[tideorm::model(table = "posts")]
struct Post {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    published: bool,
}

// `QueryBuilder::latest` answered every `query.latest()`, so the scope never ran.
#[tideorm::scopes]
impl Post {
    fn latest(query: tideorm::QueryBuilder<Self>) -> tideorm::QueryBuilder<Self> {
        query.order_desc("published")
    }
}

fn main() {}
