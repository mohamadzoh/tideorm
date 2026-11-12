#[tideorm::model(table = "notes")]
#[derive(tideorm::Model)]
struct Note {
    #[tideorm(primary_key)]
    id: i64,
}

fn main() {}
