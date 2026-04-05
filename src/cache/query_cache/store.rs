use super::*;

/// A cache entry storing query results
#[derive(Debug, Clone)]
pub(super) struct CacheEntry {
    /// Cached data as serialized JSON bytes.
    pub(super) data: Vec<u8>,
    /// Serialized size in bytes.
    pub(super) size_bytes: usize,
    /// Absolute expiration time for efficient TTL eviction.
    pub(super) expires_at: Instant,
    /// The model/table this entry is for (for targeted invalidation)
    pub(super) model_name: String,
    /// Number of times this entry has been accessed
    pub(super) hit_count: u64,
    /// Monotonic insertion sequence used by FIFO/TTL eviction indexes.
    pub(super) insert_order: u64,
    /// Monotonic access sequence used by the LRU eviction index.
    pub(super) access_order: u64,
}

impl CacheEntry {
    pub(super) fn new(
        data: Vec<u8>,
        size_bytes: usize,
        ttl: Duration,
        model_name: &str,
        order: u64,
    ) -> Self {
        let now = Instant::now();
        Self {
            data,
            size_bytes,
            expires_at: now.checked_add(ttl).unwrap_or(now),
            model_name: model_name.to_string(),
            hit_count: 0,
            insert_order: order,
            access_order: order,
        }
    }

    pub(super) fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    pub(super) fn touch(&mut self, access_order: u64) {
        self.access_order = access_order;
        self.hit_count += 1;
    }

    pub(super) fn lru_candidate(&self, key: &str) -> OrderCandidate {
        OrderCandidate::new(self.access_order, key)
    }

    fn fifo_candidate(&self, key: &str) -> OrderCandidate {
        OrderCandidate::new(self.insert_order, key)
    }

    fn ttl_candidate(&self, key: &str) -> TtlCandidate {
        TtlCandidate::new(self.expires_at, self.insert_order, key)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct OrderCandidate {
    pub(super) order: u64,
    pub(super) key: String,
}

impl OrderCandidate {
    fn new(order: u64, key: &str) -> Self {
        Self {
            order,
            key: key.to_string(),
        }
    }
}

impl Ord for OrderCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.order
            .cmp(&other.order)
            .then_with(|| self.key.cmp(&other.key))
    }
}

impl PartialOrd for OrderCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct TtlCandidate {
    pub(super) expires_at: Instant,
    pub(super) order: u64,
    pub(super) key: String,
}

impl TtlCandidate {
    fn new(expires_at: Instant, order: u64, key: &str) -> Self {
        Self {
            expires_at,
            order,
            key: key.to_string(),
        }
    }
}

impl Ord for TtlCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.expires_at
            .cmp(&other.expires_at)
            .then_with(|| self.order.cmp(&other.order))
            .then_with(|| self.key.cmp(&other.key))
    }
}

impl PartialOrd for TtlCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Default)]
pub(super) struct CacheStore {
    pub(super) entries: HashMap<String, CacheEntry>,
    pub(super) lru_heap: BinaryHeap<Reverse<OrderCandidate>>,
    pub(super) fifo_heap: BinaryHeap<Reverse<OrderCandidate>>,
    pub(super) ttl_heap: BinaryHeap<Reverse<TtlCandidate>>,
}

impl CacheStore {
    pub(super) fn insert(&mut self, key: String, entry: CacheEntry) -> Option<CacheEntry> {
        self.lru_heap.push(Reverse(entry.lru_candidate(&key)));
        self.fifo_heap.push(Reverse(entry.fifo_candidate(&key)));
        self.ttl_heap.push(Reverse(entry.ttl_candidate(&key)));
        self.entries.insert(key, entry)
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.lru_heap.clear();
        self.fifo_heap.clear();
        self.ttl_heap.clear();
    }

    pub(super) fn maybe_rebuild_indexes(&mut self) {
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
            self.lru_heap.push(Reverse(entry.lru_candidate(key)));
            self.fifo_heap.push(Reverse(entry.fifo_candidate(key)));
            self.ttl_heap.push(Reverse(entry.ttl_candidate(key)));
        }
    }
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
