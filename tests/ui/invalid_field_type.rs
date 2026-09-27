#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
enum Status {
    #[default]
    Active,
    Archived,
}

#[tideorm::model(table = "orders")]
struct Order {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    status: Status,
}

fn main() {}
