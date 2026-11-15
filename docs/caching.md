# Query Cache

The query cache keeps the models a query read, in memory, for a time you choose, and drops them when TideORM writes to their database. It is one cache per process, it starts disabled, and a query uses it only when you ask.

```rust
use std::time::Duration;
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub active: bool,
}

async fn active_users() -> tideorm::Result<Vec<User>> {
    User::query()
        .where_eq(User::columns.active, true)
        .cache(Duration::from_secs(60))
        .get()
        .await
}

fn enable_cache() {
    // Once at startup
    QueryCache::init_global(CacheConfig::default().enabled(true));
}
```

The cache types are in the prelude and in `tideorm::cache`.

## Turning It On

`QueryCache::init_global(config)` configures the process-wide cache and returns it. It can run after the cache already exists (any model write creates it with the defaults): the configuration then replaces the live one and the cached entries stay. `QueryCache::global().enable()` and `.disable()` switch it at any time, and `is_enabled()` tells which it is.

While the cache is disabled, `cache(ttl)` does nothing; the first query that asks for it logs a warning saying so, once per process.

## Caching a Query

| Method | Effect |
|---|---|
| `cache(ttl)` | keep this query's models for `ttl` |
| `cache_with_key(key, ttl)` | the same, with `key` as part of the entry's identity |
| `no_cache()` | do not cache this query, undoing an earlier `cache` |

`cache` stores what `get()` returns, so it serves everything that reads through `get()`: `first()`, `first_or_fail()`, a query's `find(id)` and `find_or_fail(id)`, the items of each `paginate()` page (its total count is read every time) and each batch of `chunk()`. Other terminals (`count()`, `exists()`, `pluck()`, `get_json()`, aggregates) always read the database. A hit returns models as a read does: relations attached to the database and dirty tracking started.

An entry is found by the query's SQL and values, the database handle it runs on, and the `cache_with_key` key if given. Two queries share an entry only when all of them match: `User::query_with(&tenant_a)` and `User::query_with(&tenant_b)` never do, and neither do `first()` and `get()` of one query.

`cache_with_key` is for rows that depend on something the SQL does not show, such as a session setting or a row-level security role: give each value its own key, so reads under different values never share an entry.

```rust,ignore
let posts = Post::query()
    .where_eq(Post::columns.published, true)
    .cache_with_key(&format!("role:{role}"), Duration::from_secs(30))
    .get()
    .await?;
```

A query asking for the cache reads the database instead, and stores nothing, when:

- it runs inside a [transaction](transactions.md), where a stored copy could outlive a rollback and a hit would hide the transaction's own writes;
- it locks its rows (`lock_for_update()`);
- its model's own serde derives would not read every stored field back, such as a `#[serde(skip)]` on a column or no serde at all;
- a row holds a value JSON cannot keep exactly: a NaN or infinite float, or a JSON `null` in an `Option` field (entries are stored as JSON);
- a write to its database landed while it ran, so the rows it read may already be replaced;
- the result is empty and `cache_empty_results` is off.

## Invalidation

Every write TideORM makes drops the cached reads of the database it wrote to, not only of its table, since foreign-key actions and triggers can change any table there. That covers model writes, bulk updates and deletes, the entity manager's flushes and every [raw statement](raw-sql.md), reads included.

- **Transactions.** A write inside a transaction drops the reads as it runs and again when the transaction commits, since other requests read, and may cache, the old rows until then. A commit that fails or is cancelled drops them again too, since it may have committed.
- **Outside writes.** Writes TideORM does not make, from another process or another program, are seen only once the TTL runs out. To share invalidations between your own processes, see [Several Processes](#several-processes).

Which pools count as one database:

| Backend | One database is |
|---|---|
| SQLite | one file; a shared in-memory database by its name; a private in-memory database is its own |
| PostgreSQL | one socket or host, port and database |
| MySQL, MariaDB | one socket or host and port: the whole server, since triggers and foreign keys can cross databases there |

Hosts are compared as the URL writes them, ignoring case, so `localhost` and `127.0.0.1` are two databases. A SQLite pool that uses `ATTACH`, and a connection whose database cannot be identified, drop every entry on each write.

To drop entries yourself:

```rust,ignore
let cache = QueryCache::global();
cache.invalidate("my-key");     // one entry; returns whether it was there
cache.invalidate_model("users"); // every entry that reads the table, compared without case
cache.clear();                   // everything
```

## Configuration

`CacheConfig` starts from `CacheConfig::default()`; its fields are public to read and assign.

| Setting | Default | Meaning |
|---|---|---|
| `.enabled(bool)` | `false` | whether the cache serves and stores |
| `.max_entries(n)` | 1000 | entries kept before evicting |
| `.max_size_bytes(n)` | 64 MiB | total size of the stored JSON; `0` for no byte limit |
| `.default_ttl(duration)` | 60 s | the TTL of a manual entry set without one |
| `.strategy(CacheStrategy)` | `LRU` | which entry to evict first |
| `.cache_empty_results(bool)` | `true` | whether an empty result is stored |
| `.key_prefix(prefix)` | none | a prefix on every key, to keep namespaces apart |

| `CacheStrategy` | Evicts first |
|---|---|
| `LRU` | the entry read least recently |
| `FIFO` | the entry stored first |
| `TTL` | the entry closest to expiring |

Whatever the strategy, every entry also expires at its TTL. An entry larger than the whole byte limit is never stored.

The live cache takes `set_max_entries`, `set_max_size_bytes`, `set_default_ttl`, `set_strategy` and `set_cache_empty_results`; lowering a limit evicts at once. `config()` returns the current configuration and `apply_config(config)` replaces it, keeping the entries. `QueryCache::new()` and `QueryCache::with_config(config)` build a separate cache of your own, which model writes do not invalidate.

## Manual Entries

The cache also holds values of your own, stored as JSON:

```rust,ignore
let cache = QueryCache::global();

cache.set("stats:users", &totals, Some(Duration::from_secs(300)), "users")?;
cache.set_tagged("report:orders", &report, None, ["orders", "users"])?;

let totals: Option<Vec<Totals>> = cache.get("stats:users");
```

- `set(key, &value, ttl, table)` tags the entry with one table and `set_tagged(key, &value, ttl, tables)` with several, so `invalidate_model` on any of them drops it. A `None` TTL takes `default_ttl`; a zero TTL stores nothing.
- A manual entry belongs to no database, so a write to any database drops it.
- A value JSON cannot keep exactly (a NaN or infinite float, or `Some` of a JSON `null`) is not stored, and replacing an entry with one removes the old entry. A serialization error is returned and leaves the old entry in place.
- `get::<T>(key)` returns `None` while the cache is disabled, and for a missing or expired entry or one that does not read as `T` (which it removes).

`CacheKeyBuilder` builds keys from parts, escaping its separators so that two different lists of parts never build one key:

```rust,ignore
let key = CacheKeyBuilder::new()
    .table("posts")
    .condition("user_id", 42)
    .order("created_at", "DESC")
    .limit(20)
    .offset(40)
    .raw("v2")
    .build();
```

`build_hash()` returns a 64-bit hash of the key instead, which can collide: keep the full key wherever the wrong entry would be a bug. `cache.generate_key(table, hash)` builds the key `table:hash`, after the `key_prefix` when one is set.

## Statistics

```rust,ignore
let stats = QueryCache::global().stats();
println!("{} hits, {} misses, {:.0}% hit rate", stats.hits, stats.misses, stats.hit_ratio() * 100.0);
```

`CacheStats` holds `hits`, `misses`, `entries`, `size_bytes`, `evictions` and `invalidations`. `reset_stats()` restarts the counters but not the entry count or size. `len()`, `is_empty()` and `contains(key)` describe the cache, and `evict_expired()` drops expired entries now rather than as they are next touched.

## Several Processes

The cache lives in one process. To keep several processes' caches in step, give the cache a hook: it is called with a `CacheInvalidation` after each invalidation, a write's (once as it runs and again when its transaction commits), `invalidate_model`'s and `clear`'s, but not a single key's `invalidate` or an expiry. Publish each event on a channel of your own, such as Redis or NATS, and hand what the other processes publish to `apply_invalidation`, which drops the same entries without calling the hook again.

The hook runs on the writing task, so hand the event off rather than wait on the network:

```rust,ignore
use tideorm::prelude::*;

let (events, mut outgoing) = tokio::sync::mpsc::unbounded_channel::<CacheInvalidation>();
QueryCache::global().set_invalidation_hook(move |event| {
    let _ = events.send(event.clone());
});
tokio::spawn(async move {
    while let Some(event) = outgoing.recv().await {
        publish(serde_json::to_string(&event)?).await; // your broker
    }
    Ok::<_, serde_json::Error>(())
});

// In the task subscribed to the other processes:
let event: CacheInvalidation = serde_json::from_str(&message)?;
QueryCache::global().apply_invalidation(&event);
```

`CacheInvalidation` is `Database(name)`, `Table(name)` or `All`, and serializes as `{"Database": ".."}`, `{"Table": ".."}` or `"All"`. A database's name is built from the connection's socket, host, port and database, so connect every process the same way. `clear_invalidation_hook()` removes the hook.
