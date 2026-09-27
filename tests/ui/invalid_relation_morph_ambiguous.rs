use tideorm::prelude::*;

// Two relations to one child model under one morph name, keyed by different
// columns: a photo row could not tell which one it belongs to.
#[tideorm::model(table = "albums")]
struct Album {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    code: String,
    #[tideorm(morph_name = "imageable")]
    photos: MorphMany<Photo>,
    #[tideorm(morph_name = "imageable", local_key = "code")]
    covers: MorphMany<Photo>,
}

#[tideorm::model(table = "photos")]
struct Photo {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    imageable_type: String,
    imageable_id: String,
}

fn main() {}
