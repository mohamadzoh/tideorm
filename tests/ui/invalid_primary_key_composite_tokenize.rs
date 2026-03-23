#[tideorm::model(table = "user_roles", tokenize)]
struct UserRole {
    #[tideorm(primary_key)]
    user_id: i64,
    #[tideorm(primary_key)]
    role_id: i64,
}

fn main() {}