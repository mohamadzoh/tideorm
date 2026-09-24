use super::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::time::Instant;

/// A cache entry storing query results
#[derive(Debug, Clone)]
pub(super) struct CacheEntry {
    /// Cached data as serialized JSON bytes.
    pub(super) data: Vec<u8>,
    /// Absolute expiration time for efficient TTL eviction.
    expires_at: Instant,
    /// Every table this entry reads, used for targeted invalidation.
    ///
    /// A query that reads past its own table — a join, union, CTE, or subquery
    /// — records all of those tables here, so a write to any one of them evicts
    /// the entry and not only a write to the primary model.
    tables: HashSet<String>,
    /// Monotonic insertion sequence used by FIFO/TTL eviction indexes.
    insert_order: u64,
    /// Monotonic access sequence used by the LRU eviction index.
    access_order: u64,
}

impl CacheEntry {
    pub(super) fn new(data: Vec<u8>, ttl: Duration, tables: HashSet<String>, order: u64) -> Self {
        let now = Instant::now();
        Self {
            data,
            expires_at: now.checked_add(ttl).unwrap_or(now),
            tables,
            insert_order: order,
            access_order: order,
        }
    }

    /// True when a write to `table` must evict this entry.
    pub(super) fn reads_table(&self, table: &str) -> bool {
        self.tables.contains(table)
    }

    pub(super) fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    fn lru_priority(&self) -> u64 {
        self.access_order
    }

    fn fifo_priority(&self) -> u64 {
        self.insert_order
    }

    fn ttl_priority(&self) -> (Instant, u64) {
        (self.expires_at, self.insert_order)
    }
}

/// One eviction-index slot: the entry's priority when it was pushed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Candidate<P> {
    priority: P,
    key: String,
}

impl<P> Candidate<P> {
    fn new(priority: P, key: &str) -> Reverse<Self> {
        Reverse(Self {
            priority,
            key: key.to_string(),
        })
    }
}

/// The cached entries plus one eviction index per strategy.
///
/// Each index is a min-heap that gains a candidate on every insert or LRU
/// touch and never updates one in place, so a popped candidate whose priority
/// no longer matches its entry is stale and skipped. The heaps are rebuilt once
/// stale candidates outnumber the live entries too far.
#[derive(Debug, Default)]
pub(super) struct CacheStore {
    entries: HashMap<String, CacheEntry>,
    lru_heap: BinaryHeap<Reverse<Candidate<u64>>>,
    fifo_heap: BinaryHeap<Reverse<Candidate<u64>>>,
    ttl_heap: BinaryHeap<Reverse<Candidate<(Instant, u64)>>>,
    /// Serialized size of every entry.
    size_bytes: usize,
    /// Advanced by every invalidation. A read that started at an older value
    /// may hold rows a write has replaced since, so it must not be stored.
    invalidation_seq: u64,
    /// The `invalidation_seq` at which each table was last invalidated.
    invalidated_at: HashMap<String, u64>,
    /// The `invalidation_seq` of the last `clear()`.
    cleared_at: u64,
}

impl CacheStore {
    pub(super) fn get(&self, key: &str) -> Option<&CacheEntry> {
        self.entries.get(key)
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn size_bytes(&self) -> usize {
        self.size_bytes
    }

    /// Store `entry` under `key`, replacing whatever was there.
    pub(super) fn insert(&mut self, key: String, entry: CacheEntry) {
        self.lru_heap
            .push(Candidate::new(entry.lru_priority(), &key));
        self.fifo_heap
            .push(Candidate::new(entry.fifo_priority(), &key));
        self.ttl_heap
            .push(Candidate::new(entry.ttl_priority(), &key));
        self.size_bytes += entry.data.len();
        if let Some(previous) = self.entries.insert(key, entry) {
            self.size_bytes -= previous.data.len();
        }
        self.maybe_rebuild_indexes();
    }

    pub(super) fn remove(&mut self, key: &str) -> Option<CacheEntry> {
        let removed = self.entries.remove(key)?;
        self.size_bytes -= removed.data.len();
        self.maybe_rebuild_indexes();
        Some(removed)
    }

    /// Remove every entry `predicate` selects, returning how many there were.
    pub(super) fn remove_where(&mut self, predicate: impl Fn(&CacheEntry) -> bool) -> usize {
        let before = self.entries.len();
        let mut freed = 0;
        self.entries.retain(|_, entry| {
            let remove = predicate(entry);
            if remove {
                freed += entry.data.len();
            }
            !remove
        });
        self.size_bytes -= freed;

        let removed = before - self.entries.len();
        if removed > 0 {
            self.maybe_rebuild_indexes();
        }
        removed
    }

    /// Remove every entry, returning how many there were.
    pub(super) fn clear(&mut self) -> usize {
        let removed = self.entries.len();
        self.entries.clear();
        self.lru_heap.clear();
        self.fifo_heap.clear();
        self.ttl_heap.clear();
        self.size_bytes = 0;
        self.invalidation_seq += 1;
        self.cleared_at = self.invalidation_seq;
        self.invalidated_at.clear();
        removed
    }

    /// Record that the entries reading `table` were invalidated.
    pub(super) fn note_invalidated(&mut self, table: &str) {
        self.invalidation_seq += 1;
        self.invalidated_at
            .insert(table.to_string(), self.invalidation_seq);
    }

    /// The point a read starts from, for [`invalidated_since`](Self::invalidated_since).
    pub(super) fn invalidation_seq(&self) -> u64 {
        self.invalidation_seq
    }

    /// Whether any of `tables` was invalidated after `seq`.
    pub(super) fn invalidated_since(&self, seq: u64, tables: &HashSet<String>) -> bool {
        self.cleared_at > seq
            || tables
                .iter()
                .any(|table| self.invalidated_at.get(table).is_some_and(|&at| at > seq))
    }

    /// Mark `key` as the most recently used entry and return it.
    pub(super) fn touch(&mut self, key: &str, access_order: u64) -> Option<&CacheEntry> {
        let entry = self.entries.get_mut(key)?;
        entry.access_order = access_order;
        self.lru_heap.push(Candidate::new(access_order, key));
        self.maybe_rebuild_indexes();
        self.entries.get(key)
    }

    /// Remove and return the entry `strategy` evicts first.
    pub(super) fn evict(&mut self, strategy: CacheStrategy) -> Option<CacheEntry> {
        let key = match strategy {
            CacheStrategy::LRU => {
                pop_current(&mut self.lru_heap, &self.entries, CacheEntry::lru_priority)
            }
            CacheStrategy::FIFO => pop_current(
                &mut self.fifo_heap,
                &self.entries,
                CacheEntry::fifo_priority,
            ),
            CacheStrategy::TTL => {
                pop_current(&mut self.ttl_heap, &self.entries, CacheEntry::ttl_priority)
            }
        }?;
        self.remove(&key)
    }

    fn maybe_rebuild_indexes(&mut self) {
        const INDEX_REBUILD_MULTIPLIER: usize = 4;
        const INDEX_REBUILD_SLACK: usize = 64;

        let entry_count = self.entries.len();
        let threshold = entry_count
            .saturating_mul(INDEX_REBUILD_MULTIPLIER)
            .saturating_add(INDEX_REBUILD_SLACK);

        if entry_count == 0
            || self.lru_heap.len() > threshold
            || self.fifo_heap.len() > threshold
            || self.ttl_heap.len() > threshold
        {
            self.rebuild_indexes();
        }
    }

    fn rebuild_indexes(&mut self) {
        self.lru_heap.clear();
        self.fifo_heap.clear();
        self.ttl_heap.clear();

        for (key, entry) in &self.entries {
            self.lru_heap
                .push(Candidate::new(entry.lru_priority(), key));
            self.fifo_heap
                .push(Candidate::new(entry.fifo_priority(), key));
            self.ttl_heap
                .push(Candidate::new(entry.ttl_priority(), key));
        }
    }
}

/// Pop candidates until one still matches its entry's priority, and return
/// that entry's key.
fn pop_current<P: Ord>(
    heap: &mut BinaryHeap<Reverse<Candidate<P>>>,
    entries: &HashMap<String, CacheEntry>,
    priority_of: impl Fn(&CacheEntry) -> P,
) -> Option<String> {
    while let Some(Reverse(candidate)) = heap.pop() {
        if entries
            .get(&candidate.key)
            .is_some_and(|entry| priority_of(entry) == candidate.priority)
        {
            return Some(candidate.key);
        }
    }
    None
}

/// Statistics for the query cache
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    /// Total number of cache hits
    pub hits: u64,
    /// Total number of cache misses
    pub misses: u64,
    /// Current number of entries in cache
    pub entries: usize,
    /// Total size of cached data in bytes (approximate)
    pub size_bytes: usize,
    /// Number of evictions
    pub evictions: u64,
    /// Number of invalidations
    pub invalidations: u64,
}

impl CacheStats {
    /// Calculate the cache hit ratio
    pub fn hit_ratio(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}
