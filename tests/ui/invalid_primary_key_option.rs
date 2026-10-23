#[tideorm::model(table = "tickets")]
struct Ticket {
    #[tideorm(primary_key, auto_increment)]
    id: Option<i64>,
    title: String,
}

fn main() {}
