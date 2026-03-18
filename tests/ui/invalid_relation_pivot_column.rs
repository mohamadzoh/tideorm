use tideorm::relations::HasManyThrough;
use tideorm::Model;

#[derive(Model)]
#[tideorm(table = "roles")]
struct Role {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[derive(Model)]
#[tideorm(table = "user_roles")]
struct UserRole {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    role_id: i64,
}

#[derive(Model)]
#[tideorm(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    email: String,

    #[tideorm(
        has_many_through = "Role",
        pivot = "user_roles",
        foreign_key = "account_id",
        related_key = "role_id"
    )]
    roles: HasManyThrough<Role, UserRole>,
}

fn main() {}
