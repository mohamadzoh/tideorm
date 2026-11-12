use super::*;

#[test]
fn table_history_is_bounded_and_compaction_blocks_old_fills() {
    let mut store = CacheStore::default();
    store.insert(
        "live".into(),
        CacheEntry::new(
            vec![1],
            Duration::from_secs(60),
            HashSet::from(["stable".into()]),
            1,
        ),
    );
    let old = FillPoint {
        sequence: store.invalidation_seq(),
        database: Some("database".into()),
    };
    let tables = HashSet::from(["old-table".into()]);
    store.note_invalidated("old-table");
    for index in 0..(INVALIDATION_HISTORY_LIMIT * 3) {
        store.note_invalidated(&format!("temporary-{index}"));
        assert!(store.invalidated_at.len() <= INVALIDATION_HISTORY_LIMIT);
    }
    assert!(!store.invalidated_at.contains_key("old-table"));
    assert!(store.invalidated_since(&old, &tables));
    assert!(store.get("live").is_some());
    let fresh = FillPoint {
        sequence: store.invalidation_seq(),
        database: old.database,
    };
    assert!(!store.invalidated_since(&fresh, &tables));
    store.note_invalidated("old-table");
    assert!(store.invalidated_since(&fresh, &tables));
}

#[test]
fn lru_hits_move_their_entry_on_without_growing_the_index() {
    let mut store = CacheStore::default();
    for (order, key) in [(1, "read"), (2, "unread")] {
        let entry = CacheEntry::new(vec![1], Duration::from_secs(60), HashSet::new(), order);
        store.insert(key.into(), entry);
    }
    for order in 3..1_000 {
        store.get("read").unwrap().touch(order);
    }
    // An older mark never undoes a newer one.
    store.get("read").unwrap().touch(1);
    assert_eq!(store.lru_heap.len(), 2);

    assert!(store.evict(CacheStrategy::LRU).is_some());
    assert!(store.get("read").is_some());
    assert!(store.get("unread").is_none());
}
