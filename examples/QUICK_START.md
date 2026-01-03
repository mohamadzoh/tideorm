# TideORM Examples - Quick Reference

Run any example with: `cargo run --example <name>`

## By Category

### 🌊 CRUD Operations
```bash
cargo run --example basic          # Core create, read, update, delete
cargo run --example upsert_demo    # Insert or update with conflict handling
```

### 🔍 Query Building
```bash
cargo run --example query_builder  # WHERE, ORDER BY, LIMIT, COUNT
```

### 🐘 PostgreSQL Features
```bash
cargo run --example postgres_demo     # Config, pool, types, relations
cargo run --example postgres_complete # All features: JSON, arrays, JOINs, etc.
```

## Learning Order

1. `basic` → Core concepts
2. `query_builder` → Advanced queries
3. `upsert_demo` → Conflict handling
4. `postgres_demo` → Database features
5. `postgres_complete` → Everything combined

## Need Help?

- Full guide: [examples/README.md](README.md)
- Database setup: [../TEST_CONFIG.md](../TEST_CONFIG.md)
- Main docs: [../README.md](../README.md)
