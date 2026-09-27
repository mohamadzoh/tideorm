// `0..100` leaves 100 out in Rust, where the rule includes its upper bound.
#[tideorm::model(table = "scores")]
struct Score {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[validate(range(0..100))]
    value: i32,
}

// A bound with no finite value has no literal to check against.
#[tideorm::model(table = "ratings")]
struct Rating {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[validate(max = "inf")]
    value: f64,
}

fn main() {}
