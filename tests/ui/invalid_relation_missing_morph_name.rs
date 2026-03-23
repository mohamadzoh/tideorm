#[tideorm::model(table = "posts")]
struct Post {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    images: tideorm::relations::MorphMany<Image>,
}

#[tideorm::model(table = "images")]
struct Image {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    imageable_type: String,
    imageable_id: i64,
}

fn main() {}