use tideorm::relations::HasManyThrough;

#[tideorm::model(table = "roles")]
struct Role {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "user_roles")]
struct UserRole {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    role_id: i64,
}

#[tideorm::model(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,

    #[tideorm(has_many_through = "Role")]
    roles: HasManyThrough<Role, UserRole>,
}

fn main() {}