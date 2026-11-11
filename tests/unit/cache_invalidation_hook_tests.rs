use super::*;
use crate::cache::QueryCache;
use CacheInvalidation::{All, Database, Table};

fn recording_cache() -> (QueryCache, Arc<parking_lot::Mutex<Vec<CacheInvalidation>>>) {
    let cache = QueryCache::new();
    cache.enable();
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = events.clone();
    cache.set_invalidation_hook(move |event| sink.lock().push(event.clone()));
    (cache, events)
}

#[test]
fn the_hook_hears_each_invalidation_but_not_a_single_key() {
    let (cache, events) = recording_cache();
    cache.set("one", &1, None, "users").unwrap();
    assert!(cache.invalidate("one"));
    cache.invalidate_model("Users");
    cache.invalidate_database("postgres:db".into());
    cache.clear();
    assert_eq!(
        *events.lock(),
        [Table("users".into()), Database("postgres:db".into()), All]
    );
    cache.clear_invalidation_hook();
    cache.clear();
    assert_eq!(events.lock().len(), 3);
}

#[test]
fn an_applied_invalidation_drops_entries_without_calling_the_hook() {
    let (cache, events) = recording_cache();
    for (key, database, table) in [
        ("posts", "postgres:db", "posts"),
        ("users", "postgres:db", "users"),
        ("elsewhere", "postgres:other", "posts"),
    ] {
        let read = cache.fill_point_for_database(Some(database.into()));
        cache
            .fill_tagged(read, key, &1, None, &[table.into()])
            .unwrap();
    }

    cache.apply_invalidation(&Table("USERS".into()));
    assert_eq!(cache.get::<i32>("users"), None);
    assert_eq!(cache.get::<i32>("posts"), Some(1));
    cache.apply_invalidation(&Database("postgres:db".into()));
    assert_eq!(cache.get::<i32>("posts"), None);
    assert_eq!(cache.get::<i32>("elsewhere"), Some(1));
    cache.apply_invalidation(&All);
    assert!(cache.is_empty());
    assert!(events.lock().is_empty());
}

#[test]
fn an_invalidation_round_trips_through_json() {
    for event in [Database("postgres:db".into()), Table("users".into()), All] {
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            serde_json::from_str::<CacheInvalidation>(&json).unwrap(),
            event
        );
    }
    assert_eq!(serde_json::to_string(&All).unwrap(), "\"All\"");
}
