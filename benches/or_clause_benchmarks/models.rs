use super::*;

#[derive(Model, PartialEq)]
#[tideorm(table = "or_bench_users")]
pub struct OrBenchUser {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,
    pub status: String,
    pub role: String,
    pub department: String,
    pub age: i32,
    pub active: bool,
}
