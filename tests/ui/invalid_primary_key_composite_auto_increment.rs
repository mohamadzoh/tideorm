#[tideorm::model(table = "user_roles")]
struct UserRole {
    #[tideorm(primary_key, auto_increment)]
    user_id: i64,
    #[tideorm(primary_key)]
    role_id: i64,
}

fn main() {}