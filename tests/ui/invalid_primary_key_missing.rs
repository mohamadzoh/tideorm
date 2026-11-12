#[tideorm::model(table = "notes")]
struct Note {
    id: i64,
    body: String,
}

fn main() {}
