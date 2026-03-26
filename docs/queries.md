# Queries

This chapter covers the query builder, full-text search, database-specific behavior, raw SQL, logging, and error handling.

For execution metrics and slow-query counters, see [Profiling](profiling.md).

## Query Builder

TideORM provides a fluent query builder with all common operations.

Query execution paths are aligned on parameterized SQL generation for reads, JOIN clauses are validated before execution, and destructive mutations reject incompatible SELECT, JOIN, ORDER BY, GROUP BY, UNION, CTE, and window-function modifiers instead of silently ignoring them.

### WHERE Conditions

**Type-Safe Approach (Recommended)**

Use auto-generated typed columns for compile-time safety. The same `where_eq`, `where_gt`, etc. methods accept both strings and typed columns:

```rust
// Every model gets a `columns` constant with typed column accessors
// User::columns.id, User::columns.name, User::columns.active, etc.

// SAME method works with both strings AND typed columns:
User::query().where_eq("active", true)                    // String-based (runtime checked)
User::query().where_eq(User::columns.active, true)        // Typed column (compile-time checked)

// Type-safe queries with compile-time column name validation
User::query()
    .where_eq(User::columns.status, "active")
    .where_gt(User::columns.age, 18)
    .where_not_null(User::columns.email)
    .get()
    .await?;
```

**All WHERE Methods Support Both Approaches**

```rust
// Equality - works with string OR typed column
User::query().where_eq("status", "active")
User::query().where_eq(User::columns.status, "active")
User::query().where_not("role", "admin")
User::query().where_not(User::columns.role, "admin")

// Comparison - works with string OR typed column
User::query().where_gt("age", 18)
User::query().where_gt(User::columns.age, 18)
User::query().where_gte("age", 18)
User::query().where_lt("age", 65)
User::query().where_lte("age", 65)

// Pattern matching - works with string OR typed column
User::query().where_like("name", "%John%")
User::query().where_like(User::columns.name, "%John%")
User::query().where_not_like("email", "%spam%")

// IN / NOT IN - works with string OR typed column
User::query().where_in("role", vec!["admin", "moderator"])
User::query().where_in(User::columns.role, vec!["admin", "moderator"])
User::query().where_not_in("status", vec!["banned", "suspended"])

// NULL checks - works with string OR typed column
User::query().where_null("deleted_at")
User::query().where_null(User::columns.deleted_at)
User::query().where_not_null("email_verified_at")

// Range - works with string OR typed column
User::query().where_between("age", 18, 65)
User::query().where_between(User::columns.age, 18, 65)

// Combine conditions (AND)
User::query()
    .where_eq(User::columns.active, true)
    .where_gt(User::columns.age, 18)
    .where_not_null(User::columns.email)
    .get()
    .await?;
```

> ⚠️ **Tip**: Use typed columns like `User::columns.name` for compile-time safety. Typos in column names will be caught by the compiler instead of at runtime.

### OR Conditions

TideORM provides comprehensive OR support with two approaches: simple OR methods and the fluent OR API. All OR methods support both string column names and typed columns.

#### Simple OR Methods

```rust
// Basic OR conditions (applied at query level)
// Works with both strings and typed columns:
User::query()
    .or_where_eq("role", "admin")                    // String-based
    .or_where_eq(User::columns.role, "moderator")   // Typed column
    .get()
    .await?;

// OR with comparison operators
Product::query()
    .or_where_gt(Product::columns.price, 1000.0)   // price > 1000
    .or_where_lt(Product::columns.price, 50.0)     // OR price < 50
    .get()
    .await?;  // Gets premium OR budget products

// OR with pattern matching
User::query()
    .or_where_like(User::columns.name, "John%")    // name LIKE 'John%'
    .or_where_like(User::columns.name, "Jane%")    // OR name LIKE 'Jane%'
    .get()
    .await?;

// OR with IN clause
Product::query()
    .or_where_in(Product::columns.category, vec!["Electronics", "Books"])
    .or_where_eq(Product::columns.featured, true)
    .get()
    .await?;

// OR with NULL checks
User::query()
    .or_where_null(User::columns.deleted_at)
    .or_where_gt(User::columns.reactivated_at, some_date)
    .get()
    .await?;

// OR with BETWEEN
Product::query()
    .or_where_between(Product::columns.price, 10.0, 50.0)    // Budget range
    .or_where_between(Product::columns.price, 500.0, 1000.0) // Premium range
    .get()
    .await?;
```

#### Fluent OR API (begin_or / end_or)

For complex queries with grouped OR conditions combined with AND, use the fluent OR API:

```rust
// Basic OR group: (category = 'Electronics' OR category = 'Home')
Product::query()
    .begin_or()
        .or_where_eq(Product::columns.category, "Electronics")
        .or_where_eq(Product::columns.category, "Home")
    .end_or()
    .get()
    .await?;

// OR with AND chains: (Apple AND active) OR (Samsung AND featured)
Product::query()
    .begin_or()
        .or_where_eq("brand", "Apple").and_where_eq("active", true)
        .or_where_eq("brand", "Samsung").and_where_eq("featured", true)
    .end_or()
    .get()
    .await?;

// Complex: active AND rating >= 4.0 AND ((Electronics AND price < 1000) OR (Home AND featured))
Product::query()
    .where_eq("active", true)
    .where_gte("rating", 4.0)
    .begin_or()
        .or_where_eq("category", "Electronics").and_where_lt("price", 1000.0)
        .or_where_eq("category", "Home").and_where_eq("featured", true)
    .end_or()
    .get()
    .await?;

// Multiple sequential OR groups
Product::query()
    .where_eq("active", true)
    .begin_or()
        .or_where_eq("category", "Electronics")
        .or_where_eq("category", "Home")
    .end_or()
    .begin_or()
        .or_where_eq("brand", "Apple")
        .or_where_eq("brand", "Samsung")
    .end_or()
    .get()
    .await?;
// SQL: WHERE active = true 
//      AND (category = 'Electronics' OR category = 'Home') 
//      AND (brand = 'Apple' OR brand = 'Samsung')
```

#### AND Methods within OR Groups

Use `and_where_*` methods to chain AND conditions within an OR branch:

```rust
Product::query()
    .begin_or()
        // First OR branch: Electronics with price > 500 and good rating
        .or_where_eq("category", "Electronics")
            .and_where_gt("price", 500.0)
            .and_where_gte("rating", 4.5)
        // Second OR branch: Home items that are featured
        .or_where_eq("category", "Home")
            .and_where_eq("featured", true)
        // Third OR branch: Any discounted item
        .or_where_not_null("discount_percent")
    .end_or()
    .get()
    .await?;
```

Available `and_where_*` methods:
- `and_where_eq(col, val)` - AND column = value
- `and_where_not(col, val)` - AND column != value  
- `and_where_gt(col, val)` - AND column > value
- `and_where_gte(col, val)` - AND column >= value
- `and_where_lt(col, val)` - AND column < value
- `and_where_lte(col, val)` - AND column <= value
- `and_where_like(col, pattern)` - AND column LIKE pattern
- `and_where_in(col, values)` - AND column IN (values)
- `and_where_not_in(col, values)` - AND column NOT IN (values)
- `and_where_null(col)` - AND column IS NULL
- `and_where_not_null(col)` - AND column IS NOT NULL
- `and_where_between(col, min, max)` - AND column BETWEEN min AND max

#### Real-World Examples

```rust
// E-commerce: Flash sale eligibility
let flash_sale_products = Product::query()
    .where_eq("active", true)
    .where_gt("stock", 100)
    .where_gte("rating", 4.3)
    .where_null("discount_percent")  // Not already discounted
    .get()
    .await?;

// Inventory: Reorder alerts
let reorder_needed = Product::query()
    .where_eq("active", true)
    .begin_or()
        .or_where_lt("stock", 50).and_where_gt("rating", 4.5)  // Popular items low
        .or_where_lt("stock", 30)  // Any item critically low
    .end_or()
    .order_by("stock", Order::Asc)
    .get()
    .await?;

// Marketing: Cross-sell recommendations
let recommendations = Product::query()
    .where_eq("active", true)
    .begin_or()
        .or_where_eq("brand", "Apple")
        .or_where_eq("brand", "Samsung").and_where_gt("price", 500.0)
        .or_where_eq("featured", true).and_where_not("category", "Electronics")
    .end_or()
    .order_by("rating", Order::Desc)
    .limit(10)
    .get()
    .await?;

// Search: Multi-pattern name matching
let search_results = Product::query()
    .begin_or()
        .or_where_like("name", "iPhone%")
        .or_where_like("name", "Galaxy%")
        .or_where_like("name", "%Pro%")
    .end_or()
    .where_eq("active", true)
    .get()
    .await?;

// Analytics: Price segmentation
let segmented = Product::query()
    .begin_or()
        .or_where_eq("category", "Electronics")
        .or_where_eq("category", "Books")
    .end_or()
    .begin_or()
        .or_where_gt("price", 1000.0)   // Premium
        .or_where_lt("price", 50.0)     // Budget
    .end_or()
    .order_by("price", Order::Desc)
    .get()
    .await?;
```

### Ordering

```rust
// Basic ordering - works with both strings and typed columns
User::query()
    .order_by("created_at", Order::Desc)           // String-based
    .order_by(User::columns.name, Order::Asc)      // Typed column
    .get()
    .await?;

// Convenience methods - also work with typed columns
User::query().order_asc(User::columns.name)        // ORDER BY name ASC
User::query().order_desc(User::columns.created_at) // ORDER BY created_at DESC
User::query().latest()                              // ORDER BY created_at DESC
User::query().oldest()                              // ORDER BY created_at ASC
```

### Pagination

```rust
// Limit and offset
User::query()
    .limit(10)
    .offset(20)
    .get()
    .await?;

// Page-based pagination
User::query()
    .page(3, 25)  // Page 3, 25 per page
    .get()
    .await?;

// Aliases
User::query().take(10).skip(20)  // Same as limit(10).offset(20)
```

### Execution Methods

```rust
// Get all matching records
let users = User::query()
    .where_eq("active", true)
    .get()
    .await?;  // Vec<User>

// Get first record
let user = User::query()
    .where_eq("email", "admin@example.com")
    .first()
    .await?;  // Option<User>

// Get first or fail
let user = User::query()
    .where_eq("id", 1)
    .first_or_fail()
    .await?;  // Result<User>

// Count (efficient SQL COUNT)
let count = User::query()
    .where_eq("active", true)
    .count()
    .await?;  // u64

// Check existence
let exists = User::query()
    .where_eq("email", "admin@example.com")
    .exists()
    .await?;  // bool

// Bulk delete (efficient single DELETE statement)
let deleted = User::query()
    .where_eq("status", "inactive")
    .delete()
    .await?;  // u64 (rows affected)
```

### UNION Queries

Combine results from multiple queries:

```rust
// UNION - combines results and removes duplicates
let users = User::query()
    .where_eq("active", true)
    .union(User::query().where_eq("role", "admin"))
    .get()
    .await?;

// UNION ALL - includes all results (faster, keeps duplicates)
let orders = Order::query()
    .where_eq("status", "pending")
    .union_all(Order::query().where_eq("status", "processing"))
    .union_all(Order::query().where_eq("status", "shipped"))
    .order_by("created_at", Order::Desc)
    .get()
    .await?;

// Raw UNION for complex queries
let results = User::query()
    .union_raw("SELECT * FROM archived_users WHERE year = 2023")
    .get()
    .await?;
```

### Window Functions

Perform calculations across sets of rows:

```rust
use tideorm::prelude::*;

// ROW_NUMBER - assign sequential numbers
let products = Product::query()
    .row_number("row_num", Some("category"), "price", Order::Desc)
    .get_raw()
    .await?;
// SQL: ROW_NUMBER() OVER (PARTITION BY "category" ORDER BY "price" DESC) AS "row_num"

// RANK - rank with gaps for ties
let employees = Employee::query()
    .rank("salary_rank", Some("department_id"), "salary", Order::Desc)
    .get_raw()
    .await?;

// DENSE_RANK - rank without gaps
let students = Student::query()
    .dense_rank("score_rank", None, "score", Order::Desc)
    .get_raw()
    .await?;

// Running totals with SUM window
let sales = Sale::query()
    .running_sum("running_total", "amount", "date", Order::Asc)
    .get_raw()
    .await?;

// LAG - access previous row value
let orders = Order::query()
    .lag("prev_total", "total", 1, Some("0"), "user_id", "created_at", Order::Asc)
    .get_raw()
    .await?;

// LEAD - access next row value
let appointments = Appointment::query()
    .lead("next_date", "date", 1, None, "patient_id", "date", Order::Asc)
    .get_raw()
    .await?;

// NTILE - distribute into buckets
let products = Product::query()
    .ntile("price_quartile", 4, "price", Order::Asc)
    .get_raw()
    .await?;

// Custom window function with full control
let results = Order::query()
    .window(
        WindowFunction::new(WindowFunctionType::Sum("amount".to_string()), "total_sales")
            .partition_by("region")
            .order_by("month", Order::Asc)
            .frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow)
    )
    .get_raw()
    .await?;
```

### Common Table Expressions (CTEs)

Define temporary named result sets:

```rust
use tideorm::prelude::*;

// Simple CTE
let orders = Order::query()
    .with_cte(CTE::new(
        "high_value_orders",
        "SELECT * FROM orders WHERE total > 1000".to_string()
    ))
    .where_raw("id IN (SELECT id FROM high_value_orders)")
    .get()
    .await?;

// CTE from another query builder
let active_users = User::query()
    .where_eq("active", true)
    .select(vec!["id", "name", "email"]);

let posts = Post::query()
    .with_query("active_users", active_users)
    .inner_join("active_users", "posts.user_id", "active_users.id")
    .get()
    .await?;

// CTE with column aliases
let stats = Sale::query()
    .with_cte_columns(
        "daily_stats",
        vec!["sale_date", "total_sales", "order_count"],
        "SELECT DATE(created_at), SUM(amount), COUNT(*) FROM sales GROUP BY DATE(created_at)"
    )
    .where_raw("date IN (SELECT sale_date FROM daily_stats WHERE total_sales > 10000)")
    .get()
    .await?;

// Recursive CTE for hierarchical data
let employees = Employee::query()
    .with_recursive_cte(
        "org_tree",
        vec!["id", "name", "manager_id", "level"],
        // Base case: top-level managers
        "SELECT id, name, manager_id, 0 FROM employees WHERE manager_id IS NULL",
        // Recursive: employees under managers
        "SELECT e.id, e.name, e.manager_id, t.level + 1 
         FROM employees e 
         INNER JOIN org_tree t ON e.manager_id = t.id"
    )
    .where_raw("id IN (SELECT id FROM org_tree)")
    .get()
    .await?;
```

---


---

## Full-Text Search

TideORM provides full-text search capabilities across PostgreSQL (tsvector/tsquery), MySQL (FULLTEXT), and SQLite (FTS5).

Enable the feature explicitly when you need the full-text search API:

```toml
tideorm = { version = "0.9.2", features = ["postgres", "fulltext"] }
```

### Search Basics

```rust
use tideorm::prelude::*;

// Simple full-text search
let results = Article::search(&["title", "content"], "rust programming")
    .await?;

// Search with ranking (ordered by relevance)
let ranked = Article::search_ranked(&["title", "content"], "rust async")
    .limit(10)
    .get_ranked()
    .await?;

for result in ranked {
    println!("{}: {} (rank: {:.2})", 
        result.record.id, 
        result.record.title, 
        result.rank
    );
}

// Count matching results
let count = Article::search(&["title", "content"], "rust")
    .count()
    .await?;

// Get first matching result
let first = Article::search(&["title"], "rust")
    .first()
    .await?;
```

### Search Modes

```rust
use tideorm::fulltext::{SearchMode, FullTextConfig};

// Natural language search (default)
Article::search(&["content"], "learn rust programming").await?;

// Boolean search with operators
Article::search(&["content"], "+rust +async -javascript")
    .mode(SearchMode::Boolean)
    .get()
    .await?;

// Phrase search (exact phrase matching)
Article::search(&["content"], "async await")
    .mode(SearchMode::Phrase)
    .get()
    .await?;

// Prefix search (for autocomplete)
Article::search(&["title"], "prog")
    .mode(SearchMode::Prefix)
    .get()
    .await?;
```

### Search Configuration

```rust
use tideorm::fulltext::{FullTextConfig, SearchMode, SearchWeights};

let config = FullTextConfig::new()
    .language("english")        // Text analysis language
    .mode(SearchMode::Boolean)  // Search mode
    .min_word_length(3)         // Minimum word length to index
    .max_word_length(50)        // Maximum word length
    // Custom weights for ranking (title > summary > content)
    .weights(SearchWeights::new(1.0, 0.5, 0.3, 0.1));

let results = Article::search_with_config(
    &["title", "summary", "content"],
    "rust programming",
    config
).get().await?;
```

### Text Highlighting

```rust
use tideorm::fulltext::{highlight_text, generate_snippet};

let text = "The quick brown fox jumps over the lazy dog.";

// Highlight search terms
let highlighted = highlight_text(text, "fox lazy", "<mark>", "</mark>");
// Result: "The quick brown <mark>fox</mark> jumps over the <mark>lazy</mark> dog."

// Generate snippet with context
let long_text = "Lorem ipsum... The fox jumped... More text here...";
let snippet = generate_snippet(long_text, "fox", 5, "<b>", "</b>");
// Result: "...dolor sit amet. The <b>fox</b> jumped over the..."
```

### Creating Full-Text Indexes

```rust
use tideorm::fulltext::{FullTextIndex, PgFullTextIndexType};
use tideorm::config::DatabaseType;

// Create index definition
let index = FullTextIndex::new(
    "idx_articles_search",
    "articles",
    vec!["title".to_string(), "content".to_string()]
)
.language("english")
.pg_index_type(PgFullTextIndexType::GIN);

// Generate SQL for your database
let sql = index.to_sql(DatabaseType::Postgres);
// PostgreSQL: CREATE INDEX "idx_articles_search" ON "articles" 
//             USING GIN ((to_tsvector('english', ...)))

let sql = index.to_sql(DatabaseType::MySQL);
// MySQL: CREATE FULLTEXT INDEX `idx_articles_search` ON `articles`(`title`, `content`)

let sql = index.to_sql(DatabaseType::SQLite);
// SQLite: Creates FTS5 virtual table + sync triggers
```

### PostgreSQL-Specific Features

```rust
use tideorm::fulltext::pg_headline_sql;

// Generate ts_headline SQL for server-side highlighting
let headline_sql = pg_headline_sql(
    "content",           // column
    "search query",      // search terms
    "english",           // language
    "<b>", "</b>"        // highlight tags
);
// Result: ts_headline('english', "content", plainto_tsquery(...), ...)
```

---

## Multi-Database Support

TideORM automatically detects your database type and generates appropriate SQL syntax. The same code works seamlessly across PostgreSQL, MySQL, and SQLite.

### Connecting to Different Databases

```rust
// PostgreSQL
TideConfig::init()
    .database("postgres://user:pass@localhost/mydb")
    .connect()
    .await?;

// MySQL / MariaDB
TideConfig::init()
    .database("mysql://user:pass@localhost/mydb")
    .connect()
    .await?;

// SQLite
TideConfig::init()
    .database("sqlite://./data.db?mode=rwc")
    .connect()
    .await?;
```

### Explicit Database Type

```rust
TideConfig::init()
    .database_type(DatabaseType::MySQL)
    .database("mysql://localhost/mydb")
    .connect()
    .await?;
```

### Database Feature Detection

Check which features are supported by the current database:

```rust
let db_type = require_db()?.backend();

// Feature checks
if db_type.supports_json() {
    // JSON/JSONB operations available
}

if db_type.supports_arrays() {
    // Native array operations (PostgreSQL only)
}

if db_type.supports_returning() {
    // RETURNING clause for INSERT/UPDATE
}

if db_type.supports_upsert() {
    // ON CONFLICT / ON DUPLICATE KEY support
}

if db_type.supports_window_functions() {
    // OVER(), ROW_NUMBER(), etc.
}

if db_type.supports_cte() {
    // WITH ... AS (Common Table Expressions)
}

if db_type.supports_fulltext_search() {
    // Full-text search capabilities
}
```

### Database-Specific JSON Operations

automatically translates JSON queries to the appropriate syntax:

```rust
// This query works on all databases with JSON support
Product::query()
    .where_json_contains("metadata", serde_json::json!({"featured": true}))
    .get()
    .await?;
```

**Generated SQL by database:**

| Operation | PostgreSQL | MySQL | SQLite |
|-----------|------------|-------|--------|
| JSON Contains | `col @> '{"key":1}'` | `JSON_CONTAINS(col, '{"key":1}')` | `json_each(col)` + subquery |
| Key Exists | `col ? 'key'` | `JSON_CONTAINS_PATH(col, 'one', '$.key')` | `json_extract(col, '$.key') IS NOT NULL` |
| Path Exists | `col @? '$.path'` | `JSON_CONTAINS_PATH(col, 'one', '$.path')` | `json_extract(col, '$.path') IS NOT NULL` |

### Database-Specific Array Operations

Array operations are fully supported on PostgreSQL. On MySQL/SQLite, arrays are stored as JSON:

```rust
// PostgreSQL native arrays
Product::query()
    .where_array_contains("tags", vec!["sale", "featured"])
    .get()
    .await?;
```

**Generated SQL:**

| Operation | PostgreSQL | MySQL/SQLite |
|-----------|------------|--------------|
| Contains | `col @> ARRAY['a','b']` | `JSON_CONTAINS(col, '["a","b"]')` |
| Contained By | `col <@ ARRAY['a','b']` | `JSON_CONTAINS('["a","b"]', col)` |
| Overlaps | `col && ARRAY['a','b']` | `JSON_OVERLAPS(col, '["a","b"]')` (MySQL 8+) |

### Database-Specific Optimizations

applies optimizations based on your database:

| Feature | PostgreSQL | MySQL | SQLite |
|---------|------------|-------|--------|
| Optimal Batch Size | 1000 | 1000 | 500 |
| Parameter Style | `$1, $2, ...` | `?, ?, ...` | `?, ?, ...` |
| Identifier Quoting | `"column"` | `` `column` `` | `"column"` |
| Float Casting | `FLOAT8` | `DOUBLE` | `REAL` |

### Feature Compatibility Matrix

| Feature | PostgreSQL | MySQL | SQLite |
|---------|:----------:|:-----:|:------:|
| JSON/JSONB | ✅ | ✅ | ✅ (JSON1) |
| Native JSON Operators | ✅ | ✅ | ❌ |
| Native Arrays | ✅ | ❌ | ❌ |
| RETURNING Clause | ✅ | ❌ | ✅ (3.35+) |
| Upsert | ✅ | ✅ | ✅ |
| Window Functions | ✅ | ✅ (8.0+) | ✅ (3.25+) |
| CTEs | ✅ | ✅ (8.0+) | ✅ (3.8+) |
| Full-Text Search | ✅ | ✅ | ✅ (FTS5) |
| Schemas | ✅ | ✅ | ❌ |

---

## Raw SQL Queries

For complex queries that can't be expressed with the query builder:

```rust
// Execute raw SQL and return model instances
let users: Vec<User> = Database::raw::<User>(
    "SELECT * FROM users WHERE age > 18"
).await?;

// With parameters (use $1, $2 for PostgreSQL, ? for MySQL/SQLite)
let users: Vec<User> = Database::raw_with_params::<User>(
    "SELECT * FROM users WHERE age > $1 AND status = $2",
    vec![18.into(), "active".into()]
).await?;

// Execute raw SQL statement (INSERT, UPDATE, DELETE)
let affected = Database::execute(
    "UPDATE users SET active = false WHERE last_login < NOW() - INTERVAL '1 year'"
).await?;

// Execute with parameters
let affected = Database::execute_with_params(
    "DELETE FROM users WHERE status = $1",
    vec!["banned".into()]
).await?;
```

---

## Query Logging

Enable SQL query logging for development/debugging:

```bash
# Set environment variable
TIDE_LOG_QUERIES=true cargo run
```

When enabled, all SQL queries will be logged to stderr.

---

## Error Handling

TideORM provides rich error types with optional context:

```rust
// Get context from errors
if let Err(e) = User::find_or_fail(999).await {
    if let Some(ctx) = e.context() {
        println!("Error in table: {:?}", ctx.table);
        println!("Column: {:?}", ctx.column);
        println!("Query: {:?}", ctx.query);
    }
}

// Create errors with context
use tideorm::error::{Error, ErrorContext};

let ctx = ErrorContext::new()
    .table("users")
    .column("email")
    .query("SELECT * FROM users WHERE email = $1");

return Err(Error::not_found("User not found").with_context(ctx));
```

---

