use super::*;

#[derive(Model, PartialEq)]
#[tideorm(table = "bench_cache_users")]
pub(super) struct BenchCacheUser {
    #[tideorm(primary_key, auto_increment)]
    pub(super) id: i64,
    pub(super) email: String,
    pub(super) name: String,
    pub(super) active: bool,
}
