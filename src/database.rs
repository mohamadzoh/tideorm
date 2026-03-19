//! Database connection and pool management
//!
//! This module provides the main `Database` struct for connecting to and
//! interacting with databases. It completely hides the underlying connection
//! pool and ORM implementation.
//!
//! ## Example
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! // Simple connection
//! let db = Database::connect("postgres://localhost/myapp").await?;
//!
//! // With options
//! let db = Database::builder()
//!     .url("postgres://localhost/myapp")
//!     .max_connections(10)
//!     .min_connections(2)
//!     .connect_timeout(Duration::from_secs(5))
//!     .build()
//!     .await?;
//!
//! // Transactions
//! db.transaction(|tx| Box::pin(async move {
//!     // tx.connection() gives you the transaction connection
//!     Ok(())
//! })).await?;
//! ```
//!
//! ## Global Database Connection
//!
//! TideORM supports a global database connection, allowing models to access
//! the database without explicitly passing a connection reference:
//!
//! ```rust,ignore
//! // Initialize global connection (call once at startup)
//! Database::connect_global("postgres://localhost/myapp").await?;
//!
//! // Now models can use the global connection automatically
//! let user = User {
//!     id: 0,
//!     email: "john@example.com".to_string(),
//!     name: "John".to_string(),
//! };
//!
//! // No need to pass &db - uses global connection automatically
//! let user = user.save().await?;
//! ```

use parking_lot::RwLock;
use std::cell::RefCell;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::internal::InternalConnection;
use crate::tide_warn;

// ============================================================================
// GLOBAL DATABASE CONNECTION
// ============================================================================

/// Global database connection instance
static GLOBAL_DB: OnceLock<Database> = OnceLock::new();

thread_local! {
    static THREAD_DB_OVERRIDE: RefCell<Option<Database>> = const { RefCell::new(None) };
}

#[derive(Clone)]
enum DatabaseHandle {
    Connection(Arc<InternalConnection>),
    Transaction(Arc<crate::internal::DatabaseTransaction>),
}

fn global_db_handle() -> &'static Database {
    GLOBAL_DB.get_or_init(Database::disconnected)
}

fn panic_missing_global_db(message: &str) -> ! {
    panic!("{}", message)
}

/// Get a reference to the global database connection
///
/// This function returns the global database connection that was initialized
/// with `Database::connect_global()` or `Database::set_global()`.
///
/// # Panics
///
/// Panics if the global database connection has not been initialized.
/// Use `try_db()` for a non-panicking version.
///
/// # Example
///
/// ```rust,ignore
/// // After initializing with connect_global()
/// let users = User::all().await?;
/// ```
pub fn db() -> &'static Database {
    let db = global_db_handle();
    if db.is_connected() {
        db
    } else {
        panic_missing_global_db(
            "Global database connection not initialized. \
             Call Database::init() or Database::set_global() before using models. \
             Use try_db() for a non-panicking alternative.",
        )
    }
}

/// Get the global database handle, returning an error if not initialized.
///
/// Prefer this over `db()` inside functions that already return `Result`.
pub fn require_db() -> Result<Database> {
    let db = global_db_handle();
    if db.is_connected() {
        Ok(db.clone())
    } else {
        Err(Error::connection(
            "Global database connection not initialized. \
             Call Database::init() or Database::set_global() before using models."
                .to_string(),
        ))
    }
}

/// Try to get the global database handle
///
/// Returns `None` if the global connection has not been initialized.
///
/// # Example
///
/// ```rust,ignore
/// if let Some(db) = try_db() {
///     // use db...
/// }
/// ```
pub fn try_db() -> Option<Database> {
    let db = global_db_handle();
    db.is_connected().then(|| db.clone())
}

/// Check if a global database connection has been initialized
///
/// # Example
///
/// ```rust,ignore
/// if has_global_db() {
///     let user = user.save().await?;
/// }
/// ```
pub fn has_global_db() -> bool {
    global_db_handle().is_connected()
}

#[doc(hidden)]
pub fn __current_db() -> Result<Database> {
    if let Some(db) = THREAD_DB_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Ok(db);
    }

    require_db()
}

/// Database connection handle
///
/// This is the main entry point for all database operations in TideORM.
/// It manages the connection pool and provides transaction support.
///
/// # Thread Safety
///
/// `Database` is `Clone`, `Send`, and `Sync`. It can be safely shared across
/// threads and cloned without duplicating the underlying connection pool.
#[derive(Clone)]
pub struct Database {
    inner: Arc<RwLock<Option<DatabaseHandle>>>,
}

impl Database {
    fn disconnected() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    fn from_internal_connection(inner: InternalConnection) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Some(DatabaseHandle::Connection(Arc::new(inner))))),
        }
    }

    fn from_internal_transaction(inner: Arc<crate::internal::DatabaseTransaction>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Some(DatabaseHandle::Transaction(inner)))),
        }
    }

    fn current_handle(&self) -> Result<DatabaseHandle> {
        self.inner.read().as_ref().cloned().ok_or_else(|| {
            Error::connection(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models."
                    .to_string(),
            )
        })
    }

    fn current_inner(&self) -> Result<Arc<InternalConnection>> {
        match self.current_handle()? {
            DatabaseHandle::Connection(inner) => Ok(inner),
            DatabaseHandle::Transaction(_) => Err(Error::connection(
                "Current database context is a transaction, not a pooled database connection"
                    .to_string(),
            )),
        }
    }

    fn replace_inner(&self, inner: Arc<InternalConnection>) {
        *self.inner.write() = Some(DatabaseHandle::Connection(inner));
    }

    fn clear_inner(&self) {
        self.inner.write().take();
    }

    fn replace_thread_override(db: Option<Self>) -> Option<Self> {
        THREAD_DB_OVERRIDE.with(|slot| slot.replace(db))
    }

    fn set_thread_override(db: Option<Self>) {
        THREAD_DB_OVERRIDE.with(|slot| *slot.borrow_mut() = db);
    }

    fn is_connected(&self) -> bool {
        self.inner.read().is_some()
    }

    /// Connect to a database using a connection URL
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let db = Database::connect("postgres://user:pass@localhost/mydb").await?;
    /// ```
    ///
    /// # Supported URL Formats
    ///
    /// - PostgreSQL: `postgres://user:pass@host/database`
    /// - MySQL: `mysql://user:pass@host/database`
    /// - SQLite: `sqlite:./path/to/db.sqlite` or `sqlite::memory:`
    pub async fn connect(url: &str) -> Result<Self> {
        let inner = InternalConnection::connect(url).await?;
        Ok(Self::from_internal_connection(inner))
    }

    /// Initialize the global database connection
    ///
    /// This is the recommended way to initialize TideORM in your application.
    /// Call this once at startup, then all models will use this connection
    /// automatically.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // At application startup
    /// Database::init("postgres://localhost/myapp").await?;
    ///
    /// // Now all models use the global connection automatically
    /// let users = User::all().await?;
    /// let user = User { id: 0, name: "John".into(), email: "john@example.com".into() };
    /// let user = user.save().await?;
    /// user.delete().await?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The connection URL is invalid
    /// - The database connection fails
    pub async fn init(url: &str) -> Result<&'static Self> {
        let db = Self::connect(url).await?;
        Self::set_global(db)
    }

    /// Set an existing database connection as the global connection
    ///
    /// Use this when you have an existing `Database` instance and want to
    /// make it globally available.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let db = Database::builder()
    ///     .url("postgres://localhost/myapp")
    ///     .max_connections(20)
    ///     .build()
    ///     .await?;
    ///
    /// Database::set_global(db)?;
    /// ```
    ///
    pub fn set_global(db: Self) -> Result<&'static Self> {
        let inner = db.current_inner()?;
        let global = global_db_handle();
        global.replace_inner(inner);
        Self::set_thread_override(Some(db));
        Ok(global)
    }

    /// Clear the global database connection and current thread override.
    pub fn reset_global() {
        global_db_handle().clear_inner();
        Self::set_thread_override(None);
    }

    /// Get a reference to the global database connection
    ///
    /// # Panics
    ///
    /// Panics if the global connection has not been initialized.
    pub fn global() -> &'static Self {
        let db = global_db_handle();
        if db.is_connected() {
            db
        } else {
            panic_missing_global_db(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models.",
            )
        }
    }

    /// Try to get the global database connection
    ///
    /// Returns `None` if the global connection has not been initialized.
    pub fn try_global() -> Option<Self> {
        try_db()
    }

    /// Create a new database builder for advanced configuration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let db = Database::builder()
    ///     .url("postgres://localhost/mydb")
    ///     .max_connections(20)
    ///     .build()
    ///     .await?;
    /// ```
    pub fn builder() -> DatabaseBuilder {
        DatabaseBuilder::new()
    }

    /// Execute a closure within a database transaction
    ///
    /// The transaction is automatically committed if the closure returns `Ok`,
    /// or rolled back if it returns `Err` or panics.
    ///
    /// # Breaking Change (v0.7)
    ///
    /// The closure now receives `&Transaction` (a reference) instead of an owned
    /// `Transaction`. This ensures the transaction is properly committed on
    /// success instead of being silently rolled back.
    ///
    /// The closure must return a pinned, boxed future to satisfy lifetime bounds:
    ///
    /// ```rust,ignore
    /// require_db()?.transaction(|tx| Box::pin(async move {
    ///     // Use tx.connection() with SeaORM operations
    ///     Ok(())
    /// })).await?;
    /// ```
    pub async fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'c> FnOnce(
                &'c Transaction,
            )
                -> std::pin::Pin<Box<dyn Future<Output = Result<T>> + Send + 'c>>
            + Send,
        T: Send,
    {
        use crate::internal::TransactionTrait;

        let txn = match self.__get_connection()? {
            ConnectionRef::Database(conn) => conn
                .begin()
                .await
                .map_err(|e| Error::transaction(e.to_string()))?,
            ConnectionRef::Transaction(tx) => tx
                .as_ref()
                .begin()
                .await
                .map_err(|e| Error::transaction(e.to_string()))?,
        };

        let txn = Arc::new(txn);
        let tx = Transaction { inner: txn.clone() };
        let previous_override =
            Self::replace_thread_override(Some(Self::from_internal_transaction(txn.clone())));

        let outcome = f(&tx).await;

        Self::set_thread_override(previous_override);
        drop(tx);

        match outcome {
            Ok(result) => {
                let txn = Arc::try_unwrap(txn).map_err(|_| {
                    Error::transaction(
                        "transaction handle leaked outside the transaction scope".to_string(),
                    )
                })?;
                txn.commit()
                    .await
                    .map_err(|e| Error::transaction(e.to_string()))?;
                Ok(result)
            }
            Err(e) => {
                let txn = Arc::try_unwrap(txn).map_err(|_| {
                    Error::transaction(
                        "transaction handle leaked outside the transaction scope".to_string(),
                    )
                })?;
                let _ = txn.rollback().await;
                Err(e)
            }
        }
    }

    /// Check if the database connection is healthy
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if db.ping().await? {
    ///     println!("Database is healthy!");
    /// }
    /// ```
    pub async fn ping(&self) -> Result<bool> {
        use crate::internal::ConnectionTrait;

        let conn = self.__internal_connection()?;
        conn
            .execute_unprepared("SELECT 1")
            .await
            .map(|_| true)
            .map_err(|e| Error::connection(e.to_string()))
    }

    /// Synchronize database schema with registered models
    ///
    /// This will create missing tables and add missing columns.
    /// Call this method only if you want to sync the database schema.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Using TideConfig (recommended)
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .sync(true)  // Enable sync during initialization
    ///     .connect()
    ///     .await?;
    ///
    /// // Or manually call sync
    /// let db = Database::connect("postgres://localhost/mydb").await?;
    /// db.sync().await?; // Creates/updates tables based on models
    /// ```
    ///
    /// # Warning
    ///
    /// **DO NOT use in production!** This is for development only.
    /// Use proper migrations for production deployments.
    pub async fn sync(&self) -> Result<()> {
        crate::sync::sync_database(self).await
    }

    // =========================================================================
    // RAW SQL QUERIES
    // =========================================================================

    /// Execute a raw SQL query and return all results
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tideorm::prelude::*;
    ///
    /// // Simple query
    /// let users: Vec<User> = Database::raw::<User>("SELECT * FROM users WHERE active = true")
    ///     .await?;
    ///
    /// // With parameters
    /// let users: Vec<User> = Database::raw_with_params::<User>(
    ///     "SELECT * FROM users WHERE age > $1 AND status = $2",
    ///     vec![18.into(), "active".into()]
    /// ).await?;
    /// ```
    pub async fn raw<T: crate::model::Model>(sql: &str) -> Result<Vec<T>> {
        use crate::internal::{ConnectionTrait, FromQueryResult, Statement};

        let db = crate::database::__current_db()?;
        let backend = db.__internal_backend()?;
        let stmt = Statement::from_string(backend, sql.to_string());

        let results = match db.__get_connection()? {
            ConnectionRef::Database(conn) => crate::profiling::__profile_future(conn.query_all_raw(stmt))
                .await,
            ConnectionRef::Transaction(tx) => crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt))
                .await,
        }
        .map_err(|e| Error::query(e.to_string()))?;

        let mut models = Vec::new();
        for row in results {
            // Convert QueryResult to model
            let model =
                <T::Entity as crate::internal::EntityTrait>::Model::from_query_result(&row, "")
                    .map_err(|e| Error::query(e.to_string()))?;
            models.push(T::from_sea_model(model));
        }

        Ok(models)
    }

    /// Execute a raw SQL query with parameters
    ///
    /// Parameters are passed as a vector of values. Use `$1`, `$2`, etc. for PostgreSQL
    /// or `?` for MySQL/SQLite.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let users: Vec<User> = Database::raw_with_params::<User>(
    ///     "SELECT * FROM users WHERE age > $1",
    ///     vec![18.into()]
    /// ).await?;
    /// ```
    pub async fn raw_with_params<T: crate::model::Model>(
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<Vec<T>> {
        crate::database::__current_db()?
            .__raw_with_params::<T>(sql, params)
            .await
    }

    #[doc(hidden)]
    pub async fn __raw_with_params<T: crate::model::Model>(
        &self,
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<Vec<T>> {
        use crate::internal::{ConnectionTrait, FromQueryResult, Statement};

        let results = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = Statement::from_sql_and_values(conn.get_database_backend(), sql, params);
                crate::profiling::__profile_future(conn.query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt = Statement::from_sql_and_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        let mut models = Vec::new();
        for row in results {
            let model =
                <T::Entity as crate::internal::EntityTrait>::Model::from_query_result(&row, "")
                    .map_err(|e| Error::query(e.to_string()))?;
            models.push(T::from_sea_model(model));
        }

        Ok(models)
    }

    /// Execute a raw SQL statement (INSERT, UPDATE, DELETE) and return rows affected
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let affected = Database::execute("UPDATE users SET active = false WHERE last_login < NOW() - INTERVAL '1 year'")
    ///     .await?;
    /// println!("Deactivated {} users", affected);
    /// ```
    pub async fn execute(sql: &str) -> Result<u64> {
        use crate::internal::ConnectionTrait;

        let db = crate::database::__current_db()?;
        let result = match db.__get_connection()? {
            ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.execute_unprepared(sql)).await
            }
            ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().execute_unprepared(sql)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Execute a raw SQL statement with parameters
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let affected = Database::execute_with_params(
    ///     "DELETE FROM users WHERE status = $1",
    ///     vec!["banned".into()]
    /// ).await?;
    /// ```
    pub async fn execute_with_params(
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<u64> {
        crate::database::__current_db()?
            .__execute_with_params(sql, params)
            .await
    }

    #[doc(hidden)]
    pub async fn __execute_with_params(
        &self,
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<u64> {
        use crate::internal::{ConnectionTrait, Statement};

        let result = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = Statement::from_sql_and_values(conn.get_database_backend(), sql, params);
                crate::profiling::__profile_future(conn.execute_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt = Statement::from_sql_and_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().execute_raw(stmt)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Execute a raw SQL query and return results as JSON
    ///
    /// This is useful when executing queries with raw select expressions
    /// that don't map directly to a model structure.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Aggregation query
    /// let results = Database::raw_json(
    ///     "SELECT user_id, SUM(total) as total_spent FROM orders GROUP BY user_id"
    /// ).await?;
    ///
    /// for row in results {
    ///     println!("User {}: ${}", row["user_id"], row["total_spent"]);
    /// }
    ///
    /// // Query with calculated columns
    /// let results = Database::raw_json(
    ///     "SELECT *, (price * quantity) as total FROM order_items"
    /// ).await?;
    /// ```
    pub async fn raw_json(sql: &str) -> Result<Vec<serde_json::Value>> {
        use crate::internal::{ConnectionTrait, Statement};

        let db = crate::database::__current_db()?;
        let backend = db.__internal_backend()?;
        let stmt = Statement::from_string(backend, sql.to_string());

        let results = match db.__get_connection()? {
            ConnectionRef::Database(conn) => crate::profiling::__profile_future(conn.query_all_raw(stmt))
                .await,
            ConnectionRef::Transaction(tx) => crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt))
                .await,
        }
        .map_err(|e| Error::query(e.to_string()))?;

        Self::query_rows_to_json(results)
    }

    /// Execute a raw SQL query with parameters and return results as JSON
    pub async fn raw_json_with_params(
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        crate::database::__current_db()?
            .__raw_json_with_params(sql, params)
            .await
    }

    #[doc(hidden)]
    pub async fn __raw_json_with_params(
        &self,
        sql: &str,
        params: Vec<crate::internal::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        use crate::internal::{ConnectionTrait, Statement};

        let results = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = Statement::from_sql_and_values(conn.get_database_backend(), sql, params);
                crate::profiling::__profile_future(conn.query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt = Statement::from_sql_and_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        Self::query_rows_to_json(results)
    }

    fn query_rows_to_json(
        results: Vec<crate::internal::QueryResult>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut json_results = Vec::new();
        for row in results {
            let mut obj = serde_json::Map::new();

            for col_name in row.column_names() {
                let json_val = if let Ok(val) = row.try_get::<Option<i64>>("", &col_name) {
                    match val {
                        Some(v) => serde_json::json!(v),
                        None => serde_json::Value::Null,
                    }
                } else if let Ok(val) = row.try_get::<Option<bool>>("", &col_name) {
                    match val {
                        Some(v) => serde_json::json!(v),
                        None => serde_json::Value::Null,
                    }
                } else if let Ok(val) = row.try_get::<Option<f64>>("", &col_name) {
                    match val {
                        Some(v) => serde_json::json!(v),
                        None => serde_json::Value::Null,
                    }
                } else if let Ok(val) = row.try_get::<Option<String>>("", &col_name) {
                    match val {
                        Some(v) => serde_json::json!(v),
                        None => serde_json::Value::Null,
                    }
                } else {
                    serde_json::Value::Null
                };

                obj.insert(col_name.to_string(), json_val);
            }

            json_results.push(serde_json::Value::Object(obj));
        }

        Ok(json_results)
    }

    /// Get the raw internal connection (for internal use only)
    #[doc(hidden)]
    pub fn __internal_connection(&self) -> Result<crate::internal::DatabaseConnection> {
        Ok(self.current_inner()?.connection().clone())
    }

    /// Get the database backend type
    ///
    /// Returns the type of database (PostgreSQL, MySQL, MariaDB, or SQLite) that
    /// this connection is using. This is useful for writing database-specific
    /// queries or handling database-specific features.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let backend = require_db()?.backend();
    /// match backend {
    ///     crate::config::DatabaseType::Postgres => println!("Using PostgreSQL"),
    ///     crate::config::DatabaseType::MySQL => println!("Using MySQL"),
    ///     crate::config::DatabaseType::MariaDB => println!("Using MariaDB"),
    ///     crate::config::DatabaseType::SQLite => println!("Using SQLite"),
    ///     _ => println!("Unknown"),
    /// }
    /// ```
    pub fn backend(&self) -> crate::config::DatabaseType {
        // Prefer the globally configured type (which accounts for MariaDB auto-detection)
        if let Some(db_type) = crate::config::TideConfig::get_database_type() {
            return db_type;
        }
        // Fallback to SeaORM backend detection
        use crate::internal::DbBackend;
        match self.__internal_backend() {
            Ok(DbBackend::Postgres) => crate::config::DatabaseType::Postgres,
            Ok(DbBackend::MySql) => crate::config::DatabaseType::MySQL,
            Ok(DbBackend::Sqlite) => crate::config::DatabaseType::SQLite,
            Ok(other) => {
                tide_warn!(
                    "Unknown database backend {:?}, defaulting to Postgres",
                    other
                );
                crate::config::DatabaseType::Postgres
            }
            Err(err) => {
                tide_warn!(
                    "Unable to inspect database backend for disconnected handle: {}. Defaulting to Postgres",
                    err
                );
                crate::config::DatabaseType::Postgres
            }
        }
    }

    /// Get the raw SeaORM database backend (for internal use only)
    #[doc(hidden)]
    pub fn __internal_backend(&self) -> Result<crate::internal::DbBackend> {
        use crate::internal::ConnectionTrait;

        Ok(match self.current_handle()? {
            DatabaseHandle::Connection(inner) => inner.connection().get_database_backend(),
            DatabaseHandle::Transaction(tx) => tx.as_ref().get_database_backend(),
        })
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("connected", &true)
            .finish()
    }
}

/// A database transaction handle
///
/// Transactions are created via `Database::transaction()` and provide
/// the same query capabilities as a regular database connection.
///
/// # Automatic Commit/Rollback
///
/// - If the transaction closure returns `Ok`, the transaction is committed
/// - If it returns `Err` or panics, the transaction is rolled back
pub struct Transaction {
    inner: Arc<crate::internal::DatabaseTransaction>,
}

impl Transaction {
    /// Get a reference to the underlying connection.
    ///
    /// The returned reference implements SeaORM's `ConnectionTrait`,
    /// so it can be used with any SeaORM query operation.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// require_db()?.transaction(|tx| Box::pin(async move {
    ///     let conn = tx.connection();
    ///     // Use conn with SeaORM operations
    ///     Ok(())
    /// })).await?;
    /// ```
    pub fn connection(&self) -> &crate::internal::DatabaseTransaction {
        self.inner.as_ref()
    }

    /// Get the raw internal transaction (for internal use only)
    #[doc(hidden)]
    pub fn __internal_transaction(&self) -> &crate::internal::DatabaseTransaction {
        self.inner.as_ref()
    }
}

/// Builder for configuring database connections
///
/// # Example
///
/// ```rust,ignore
/// let db = Database::builder()
///     .url("postgres://localhost/mydb")
///     .max_connections(20)
///     .min_connections(5)
///     .connect_timeout(Duration::from_secs(10))
///     .idle_timeout(Duration::from_secs(300))
///     .build()
///     .await?;
/// ```
#[derive(Debug, Clone)]
pub struct DatabaseBuilder {
    url: Option<String>,
    max_connections: Option<u32>,
    min_connections: Option<u32>,
    connect_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    max_lifetime: Option<Duration>,
}

impl DatabaseBuilder {
    /// Create a new DatabaseBuilder
    pub fn new() -> Self {
        Self {
            url: None,
            max_connections: None,
            min_connections: None,
            connect_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
        }
    }

    /// Set the database connection URL
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set the maximum number of connections in the pool
    pub fn max_connections(mut self, n: u32) -> Self {
        self.max_connections = Some(n);
        self
    }

    /// Set the minimum number of connections in the pool
    pub fn min_connections(mut self, n: u32) -> Self {
        self.min_connections = Some(n);
        self
    }

    /// Set the connection timeout
    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.connect_timeout = Some(duration);
        self
    }

    /// Set the idle connection timeout
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.idle_timeout = Some(duration);
        self
    }

    /// Set the maximum connection lifetime
    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.max_lifetime = Some(duration);
        self
    }

    /// Build and connect to the database with pool configuration
    pub async fn build(self) -> Result<Database> {
        let url = self
            .url
            .ok_or_else(|| Error::configuration("Database URL is required"))?;

        // Build ConnectOptions with pool settings
        let mut opts = crate::internal::ConnectOptions::new(url);

        // Apply pool settings (methods return &mut self)
        if let Some(max) = self.max_connections {
            opts.max_connections(max);
        }
        if let Some(min) = self.min_connections {
            opts.min_connections(min);
        }
        if let Some(timeout) = self.connect_timeout {
            opts.connect_timeout(timeout);
        }
        if let Some(timeout) = self.idle_timeout {
            opts.idle_timeout(timeout);
        }
        if let Some(lifetime) = self.max_lifetime {
            opts.max_lifetime(lifetime);
        }

        // Connect with options
        let conn = crate::internal::SeaDatabase::connect(opts)
            .await
            .map_err(|e| Error::connection(e.to_string()))?;

        Ok(Database::from_internal_connection(InternalConnection { conn }))
    }
}

impl Default for DatabaseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for types that can be used as a database connection
///
/// This is implemented for both `Database` and `Transaction`, allowing
/// the same query methods to work with either.
pub trait Connection: Send + Sync {
    /// Get the internal connection for query execution
    #[doc(hidden)]
    fn __get_connection(&self) -> Result<ConnectionRef>;
}

/// Internal connection reference (hidden from users)
#[doc(hidden)]
pub enum ConnectionRef {
    Database(crate::internal::DatabaseConnection),
    Transaction(Arc<crate::internal::DatabaseTransaction>),
}

impl Connection for Database {
    fn __get_connection(&self) -> Result<ConnectionRef> {
        Ok(match self.current_handle()? {
            DatabaseHandle::Connection(inner) => ConnectionRef::Database(inner.connection().clone()),
            DatabaseHandle::Transaction(tx) => ConnectionRef::Transaction(tx),
        })
    }
}

impl Connection for Transaction {
    fn __get_connection(&self) -> Result<ConnectionRef> {
        Ok(ConnectionRef::Transaction(self.inner.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Connection, Database};

    #[test]
    fn hidden_accessors_return_errors_for_disconnected_database() {
        let db = Database::disconnected();

        assert!(db.__internal_connection().is_err());
        assert!(db.__internal_backend().is_err());
        assert!(db.__get_connection().is_err());
    }

    #[test]
    fn backend_defaults_safely_for_disconnected_database() {
        let db = Database::disconnected();

        assert_eq!(db.backend(), crate::config::DatabaseType::Postgres);
    }
}
