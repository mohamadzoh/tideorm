use tideorm::prelude::*;

#[tideorm::model(table = "tickets")]
struct Ticket {
    #[tideorm(primary_key, auto_increment)]
    id: Uuid,
    title: String,
}

fn main() {}
