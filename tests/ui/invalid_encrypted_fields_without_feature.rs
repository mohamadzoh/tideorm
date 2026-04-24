#[tideorm::model(table = "customers", encrypted = "phone_number")]
struct Customer {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    phone_number: String,
}

fn main() {}