//! Global TideORM configuration
//!
//! This module provides global configuration for TideORM, including
//! database connection, pool settings, translation settings, and other defaults.
//!
//! ## Example
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! // Configure and connect in one unified call
//! TideConfig::init()
//!     .database_type(DatabaseType::Postgres)
//!     .database("postgres://localhost/mydb")
//!     .max_connections(20)
//!     .min_connections(5)
//!     .languages(&["en", "fr", "ar", "es"])
//!     .fallback_language("en")
//!     .connect()
//!     .await?;
//!
//! // Now use models - database is automatically available
//! let users = User::all().await?;
//! ```

use parking_lot::RwLock;
use std::sync::OnceLock;
use std::time::Duration;

use crate::database::Database;
use crate::error::Result;
use crate::migration::Migration;

/// Global configuration instance
static GLOBAL_CONFIG: OnceLock<RwLock<Config>> = OnceLock::new();

/// Global schema file path (set via TideConfig::schema_file())
static SCHEMA_FILE_PATH: OnceLock<String> = OnceLock::new();

/// Global pool configuration (set during connect)
static GLOBAL_POOL_CONFIG: OnceLock<PoolConfig> = OnceLock::new();

/// Supported database types
///
/// TideORM supports multiple database backends. Each has its own
/// specific features and SQL dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatabaseType {
    /// PostgreSQL database
    /// 
    /// Features: JSONB, Arrays, Full-text search, LISTEN/NOTIFY
    /// URL format: `postgres://user:pass@host:5432/database`
    #[default]
    Postgres,
    
    /// MySQL / MariaDB database
    /// 
    /// Features: JSON, Full-text search, Spatial data
    /// URL format: `mysql://user:pass@host:3306/database`
    MySQL,
    
    /// SQLite database (file-based or in-memory)
    /// 
    /// Features: Lightweight, zero-config, serverless
    /// URL format: `sqlite:./path/to/db.sqlite` or `sqlite::memory:`
    SQLite,
}

impl DatabaseType {
    /// Get the default port for this database type
    pub fn default_port(&self) -> u16 {
        match self {
            DatabaseType::Postgres => 5432,
            DatabaseType::MySQL => 3306,
            DatabaseType::SQLite => 0, // No port for SQLite
        }
    }
    
    /// Get the URL scheme for this database type
    pub fn url_scheme(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "postgres",
            DatabaseType::MySQL => "mysql",
            DatabaseType::SQLite => "sqlite",
        }
    }
    
    /// Check if this database supports JSON/JSONB columns
    pub fn supports_json(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL => true,
            DatabaseType::SQLite => false, // SQLite stores JSON as TEXT
        }
    }
    
    /// Check if this database supports array columns
    pub fn supports_arrays(&self) -> bool {
        matches!(self, DatabaseType::Postgres)
    }
    
    /// Check if this database supports RETURNING clause
    pub fn supports_returning(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL => false,
            DatabaseType::SQLite => true, // SQLite 3.35+
        }
    }
    
    /// Try to detect database type from URL
    pub fn from_url(url: &str) -> Option<Self> {
        let url_lower = url.to_lowercase();
        if url_lower.starts_with("postgres://") || url_lower.starts_with("postgresql://") {
            Some(DatabaseType::Postgres)
        } else if url_lower.starts_with("mysql://") || url_lower.starts_with("mariadb://") {
            Some(DatabaseType::MySQL)
        } else if url_lower.starts_with("sqlite:") {
            Some(DatabaseType::SQLite)
        } else {
            None
        }
    }
}

impl std::fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseType::Postgres => write!(f, "PostgreSQL"),
            DatabaseType::MySQL => write!(f, "MySQL"),
            DatabaseType::SQLite => write!(f, "SQLite"),
        }
    }
}

/// TideORM global configuration
///
/// Stores settings that apply to all models unless overridden.
#[derive(Debug, Clone)]
pub struct Config {
    /// Allowed languages for translations
    pub languages: Vec<String>,
    
    /// Default/fallback language
    pub fallback_language: String,
    
    /// Default hidden attributes (applied to all models)
    pub hidden_attributes: Vec<String>,
    
    /// Whether to enable soft delete by default
    pub soft_delete_by_default: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            languages: vec!["en".to_string()],
            fallback_language: "en".to_string(),
            hidden_attributes: vec![],
            soft_delete_by_default: false,
        }
    }
}

impl Config {
    /// Create a new configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Get the global configuration (read-only)
    pub fn global() -> Config {
        GLOBAL_CONFIG
            .get_or_init(|| RwLock::new(Config::default()))
            .read()
            .clone()
    }
    
    /// Get allowed languages from global config
    pub fn get_languages() -> Vec<String> {
        Self::global().languages
    }
    
    /// Get fallback language from global config
    pub fn get_fallback_language() -> String {
        Self::global().fallback_language
    }
    
    /// Get hidden attributes from global config
    pub fn get_hidden_attributes() -> Vec<String> {
        Self::global().hidden_attributes
    }
    
    /// Check if soft delete is enabled by default
    pub fn is_soft_delete_default() -> bool {
        Self::global().soft_delete_by_default
    }
}

/// Database pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool (default: 10)
    pub max_connections: u32,
    
    /// Minimum number of connections in the pool (default: 1)
    pub min_connections: u32,
    
    /// Connection timeout (default: 8 seconds)
    pub connect_timeout: Duration,
    
    /// Idle connection timeout (default: 10 minutes)
    pub idle_timeout: Duration,
    
    /// Maximum connection lifetime (default: 30 minutes)
    pub max_lifetime: Duration,
    
    /// Acquire timeout - how long to wait for a connection from pool (default: 8 seconds)
    pub acquire_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            connect_timeout: Duration::from_secs(8),
            idle_timeout: Duration::from_secs(600),      // 10 minutes
            max_lifetime: Duration::from_secs(1800),     // 30 minutes
            acquire_timeout: Duration::from_secs(8),
        }
    }
}

/// Builder for TideORM global configuration
///
/// This is the main entry point for configuring TideORM. It handles both
/// application configuration (languages, hidden attributes) and database
/// connection with pool settings.
///
/// # Example
///
/// ```rust,ignore
/// // Full configuration with database and pool settings
/// TideConfig::init()
///     .database_type(DatabaseType::Postgres)
///     .database("postgres://user:pass@localhost/mydb")
///     .max_connections(20)
///     .min_connections(5)
///     .sync(true)  // Enable auto-sync (development only!)
///     .schema_file("schema.sql")  // Generate schema file
///     .connect_timeout(Duration::from_secs(10))
///     .idle_timeout(Duration::from_secs(300))
///     .languages(&["en", "fr", "ar", "es"])
///     .fallback_language("en")
///     .hidden_attributes(&["password", "deleted_at"])
///     .connect()
///     .await?;
///
/// // Now use models - database is automatically available
/// let users = User::all().await?;
/// let user = User::find(1).await?;
/// 
/// // Access db if needed
/// let db = TideConfig::db();
/// ```
pub struct TideConfig {
    config: Config,
    database_type: Option<DatabaseType>,
    database_url: Option<String>,
    pool: PoolConfig,
    sync_enabled: bool,
    force_sync: bool,
    schema_file: Option<String>,
    migrations: Vec<Box<dyn Migration>>,
    run_migrations: bool,
}

/// Global database type (set after connect)
static GLOBAL_DB_TYPE: OnceLock<DatabaseType> = OnceLock::new();

impl TideConfig {
    /// Initialize a new configuration builder
    pub fn init() -> Self {
        Self {
            config: Config::default(),
            database_type: None,
            database_url: None,
            pool: PoolConfig::default(),
            sync_enabled: false,
            force_sync: false,
            schema_file: None,
            migrations: Vec::new(),
            run_migrations: false,
        }
    }
    
    // ========================================================================
    // DATABASE CONFIGURATION
    // ========================================================================
    
    /// Add a single migration
    ///
    /// Migrations are run in the order they are added, so add them in
    /// chronological order (oldest first). Migrations are tracked by version,
    /// so previously applied migrations will be skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .migration(CreateUsersTable)
    ///     .migration(CreatePostsTable)
    ///     .migration(AddEmailVerifiedToUsers)
    ///     .run_migrations(true)
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn migration<M: Migration + 'static>(mut self, migration: M) -> Self {
        self.migrations.push(Box::new(migration));
        self
    }
    
    /// Add multiple migrations using a tuple
    ///
    /// This provides a convenient way to register many migrations at once.
    /// Migrations are run in tuple order, so list them chronologically.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .migrations::<(
    ///         CreateUsersTable,
    ///         CreatePostsTable,
    ///         CreateCommentsTable,
    ///         AddEmailVerifiedToUsers,
    ///     )>()
    ///     .run_migrations(true)
    ///     .connect()
    ///     .await?;
    /// ```
    ///
    /// Supports up to 16 migrations in a single tuple. For more migrations,
    /// call `.migrations()` multiple times or use `.migration()` individually.
    pub fn migrations<T: RegisterMigrations>(mut self) -> Self {
        self.migrations.extend(T::collect());
        self
    }
    
    /// Enable or disable automatic migration execution on connect
    ///
    /// When enabled, all registered migrations will be run automatically
    /// after connecting to the database. Migrations are tracked in a
    /// `_migrations` table, so previously applied migrations are skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .migration(CreateUsersTable)
    ///     .migration(CreatePostsTable)
    ///     .run_migrations(true)  // Run migrations on connect
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn run_migrations(mut self, enabled: bool) -> Self {
        self.run_migrations = enabled;
        self
    }
    
    /// Enable or disable automatic database schema synchronization
    ///
    /// When enabled, TideORM will automatically create/update database tables
    /// based on your model definitions after connecting. This is similar to
    /// Sequelize's `sync()` feature.
    ///
    /// # ⚠️ Warning
    ///
    /// **DO NOT use in production!** This feature is for development only.
    /// Use proper database migrations for production deployments.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database_type(DatabaseType::Postgres)
    ///     .database("postgres://localhost/mydb")
    ///     .sync(true)  // Auto-create/update tables
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn sync(mut self, enabled: bool) -> Self {
        self.sync_enabled = enabled;
        self
    }
    
    /// Register models for database synchronization
    ///
    /// Specify the model types to sync as a tuple. This replaces the need
    /// for calling `register_models!()` separately.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .sync(true)
    ///     .models::<(User, Post, Comment)>()  // Register models here
    ///     .connect()
    ///     .await?;
    /// ```
    ///
    /// Supports up to 12 models in a single tuple. For more models,
    /// call `.models()` multiple times or use `register_models!()` macro.
    pub fn models<T: crate::sync::RegisterModels>(self) -> Self {
        T::register_all();
        self
    }
    
    /// Enable force sync mode (removes columns not in model)
    ///
    /// When enabled along with `sync(true)`, TideORM will also DROP columns
    /// from database tables that are not defined in your model. This is useful
    /// for keeping the database schema exactly in sync with your models.
    ///
    /// # ⚠️ DANGER
    ///
    /// **This WILL cause data loss!** Any columns not defined in your model
    /// will be permanently deleted along with their data.
    ///
    /// **NEVER use in production!** This is for development only.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .sync(true)
    ///     .force_sync(true)  // ⚠️ DANGER: Will drop columns not in model!
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn force_sync(mut self, enabled: bool) -> Self {
        self.force_sync = enabled;
        self
    }
    
    /// Set the path for schema SQL file generation
    ///
    /// When set, TideORM will generate a complete SQL schema file at startup
    /// containing all CREATE TABLE statements, indexes, and constraints.
    /// This is useful for documentation, migrations, or database setup.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database_type(DatabaseType::Postgres)
    ///     .database("postgres://localhost/mydb")
    ///     .schema_file("schema.sql")  // Generate schema.sql in project root
    ///     .connect()
    ///     .await?;
    /// ```
    ///
    /// You can also specify a full path:
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .schema_file("./database/schema.sql")
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn schema_file(mut self, path: &str) -> Self {
        self.schema_file = Some(path.to_string());
        self
    }
    
    /// Set the database type explicitly
    ///
    /// While TideORM can auto-detect the database type from the URL,
    /// it's recommended to set it explicitly for clarity and to enable
    /// database-specific optimizations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database_type(DatabaseType::Postgres)
    ///     .database("postgres://localhost/mydb")
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn database_type(mut self, db_type: DatabaseType) -> Self {
        self.database_type = Some(db_type);
        self
    }
    
    /// Set the database connection URL (required)
    ///
    /// # Supported Formats
    ///
    /// - PostgreSQL: `postgres://user:pass@host/database`
    /// - MySQL: `mysql://user:pass@host/database`
    /// - SQLite: `sqlite:./path/to/db.sqlite` or `sqlite::memory:`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://user:pass@localhost:5432/mydb")
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn database(mut self, url: &str) -> Self {
        self.database_url = Some(url.to_string());
        self
    }
    
    /// Set the maximum number of connections in the pool (default: 10)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .max_connections(20)  // Allow up to 20 concurrent connections
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn max_connections(mut self, n: u32) -> Self {
        self.pool.max_connections = n;
        self
    }
    
    /// Set the minimum number of connections in the pool (default: 1)
    ///
    /// The pool will maintain at least this many connections ready for use.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .min_connections(5)  // Keep at least 5 connections ready
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn min_connections(mut self, n: u32) -> Self {
        self.pool.min_connections = n;
        self
    }
    
    /// Set the connection timeout (default: 8 seconds)
    ///
    /// Maximum time to wait when establishing a new connection.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .connect_timeout(Duration::from_secs(30))
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.pool.connect_timeout = duration;
        self
    }
    
    /// Set the idle connection timeout (default: 10 minutes)
    ///
    /// Connections idle longer than this are closed and removed from pool.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .idle_timeout(Duration::from_secs(300))  // 5 minutes
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.pool.idle_timeout = duration;
        self
    }
    
    /// Set the maximum connection lifetime (default: 30 minutes)
    ///
    /// Connections older than this are closed, regardless of activity.
    /// Helps prevent issues with stale connections.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .max_lifetime(Duration::from_secs(3600))  // 1 hour
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.pool.max_lifetime = duration;
        self
    }
    
    /// Set the acquire timeout (default: 8 seconds)
    ///
    /// Maximum time to wait when acquiring a connection from the pool.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .acquire_timeout(Duration::from_secs(5))
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn acquire_timeout(mut self, duration: Duration) -> Self {
        self.pool.acquire_timeout = duration;
        self
    }
    
    // ========================================================================
    // APPLICATION CONFIGURATION
    // ========================================================================
    
    /// Set allowed languages for translations
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .languages(&["en", "fr", "ar", "es"])
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn languages(mut self, langs: &[&str]) -> Self {
        self.config.languages = langs.iter().map(|s| s.to_string()).collect();
        self
    }
    
    /// Set the fallback language (default: "en")
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .fallback_language("en")
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn fallback_language(mut self, lang: &str) -> Self {
        self.config.fallback_language = lang.to_string();
        self
    }
    
    /// Set globally hidden attributes (these will be hidden in all models)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .hidden_attributes(&["password", "deleted_at"])
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn hidden_attributes(mut self, attrs: &[&str]) -> Self {
        self.config.hidden_attributes = attrs.iter().map(|s| s.to_string()).collect();
        self
    }
    
    /// Enable soft delete by default for all models
    pub fn soft_delete_by_default(mut self, enabled: bool) -> Self {
        self.config.soft_delete_by_default = enabled;
        self
    }
    
    // ========================================================================
    // FINALIZATION
    // ========================================================================
    
    /// Connect to the database and apply all configuration
    ///
    /// This is the main entry point that:
    /// 1. Applies global configuration (languages, hidden attributes, etc.)
    /// 2. Detects or validates the database type
    /// 3. Connects to the database with pool settings
    /// 4. Makes the database globally available to all models
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database_type(DatabaseType::Postgres)
    ///     .database("postgres://user:pass@localhost/mydb")
    ///     .max_connections(20)
    ///     .sync(true)  // Auto-sync tables (dev only!)
    ///     .languages(&["en", "fr"])
    ///     .connect()
    ///     .await?;
    ///
    /// // Now use models directly
    /// let users = User::all().await?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No database URL was provided
    /// - Database type cannot be determined
    /// - The database connection fails
    /// - A global connection has already been initialized
    /// - Schema synchronization fails (if sync is enabled)
    pub async fn connect(self) -> Result<&'static Database> {
        // Apply configuration
        let config = GLOBAL_CONFIG.get_or_init(|| RwLock::new(Config::default()));
        *config.write() = self.config;
        
        // Get database URL
        let url = self.database_url.ok_or_else(|| {
            crate::error::Error::configuration(
                "Database URL is required. Use .database(\"postgres://...\") to set it."
            )
        })?;
        
        // Determine database type (explicit or auto-detect)
        let db_type = match self.database_type {
            Some(t) => t,
            None => DatabaseType::from_url(&url).ok_or_else(|| {
                crate::error::Error::configuration(
                    "Could not detect database type from URL. \
                     Use .database_type(DatabaseType::Postgres) to set it explicitly."
                )
            })?,
        };
        
        // Store database type globally
        let _ = GLOBAL_DB_TYPE.set(db_type);
        
        // Store pool config globally
        let _ = GLOBAL_POOL_CONFIG.set(self.pool.clone());
        
        // Build and connect to database with pool settings
        let db = Database::builder()
            .url(url)
            .max_connections(self.pool.max_connections)
            .min_connections(self.pool.min_connections)
            .connect_timeout(self.pool.connect_timeout)
            .idle_timeout(self.pool.idle_timeout)
            .max_lifetime(self.pool.max_lifetime)
            .build()
            .await?;
        
        // Set as global
        let db_ref = Database::set_global(db)?;
        
        // Run migrations if enabled and migrations were registered
        if self.run_migrations && !self.migrations.is_empty() {
            let mut migrator = crate::migration::Migrator::new();
            for migration in self.migrations {
                migrator = migrator.add_boxed(migration);
            }
            let result = migrator.run().await?;
            if result.has_applied() {
                eprintln!("{}", result);
            }
        }
        
        // Run schema sync if enabled
        if self.sync_enabled {
            crate::sync::sync_database_with_options(db_ref, self.force_sync).await?;
        }
        
        // Generate schema file if configured
        if let Some(path) = &self.schema_file {
            // Store schema path
            let _ = SCHEMA_FILE_PATH.set(path.clone());
            
            // Auto-generate schema file from database introspection
            crate::schema::SchemaWriter::write_schema(path).await?;
        }
        
        Ok(db_ref)
    }
    
    /// Apply configuration without connecting to database
    ///
    /// Use this when you only need to configure application settings
    /// without database connection (e.g., for testing).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .languages(&["en", "fr", "ar"])
    ///     .fallback_language("en")
    ///     .apply();
    /// ```
    pub fn apply(self) {
        let config = GLOBAL_CONFIG.get_or_init(|| RwLock::new(Config::default()));
        *config.write() = self.config;
        
        // Store database type if provided
        if let Some(db_type) = self.database_type {
            let _ = GLOBAL_DB_TYPE.set(db_type);
        }
    }
    
    // ========================================================================
    // STATIC ACCESSORS
    // ========================================================================
    
    /// Get the global database connection
    ///
    /// # Panics
    ///
    /// Panics if `connect()` has not been called yet.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let db = TideConfig::db();
    /// ```
    pub fn db() -> &'static Database {
        crate::database::db()
    }
    
    /// Try to get the global database connection
    ///
    /// Returns `None` if `connect()` has not been called yet.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(db) = TideConfig::try_db() {
    ///     // use db...
    /// }
    /// ```
    pub fn try_db() -> Option<&'static Database> {
        crate::database::try_db()
    }
    
    /// Check if database is connected
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if TideConfig::is_connected() {
    ///     let users = User::all().await?;
    /// }
    /// ```
    pub fn is_connected() -> bool {
        crate::database::has_global_db()
    }
    
    /// Get the configured database type
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let db_type = TideConfig::database_type();
    /// if db_type.supports_arrays() {
    ///     // Use PostgreSQL array features
    /// }
    /// ```
    pub fn get_database_type() -> Option<DatabaseType> {
        GLOBAL_DB_TYPE.get().copied()
    }
    
    /// Check if the database is PostgreSQL
    pub fn is_postgres() -> bool {
        GLOBAL_DB_TYPE.get() == Some(&DatabaseType::Postgres)
    }
    
    /// Check if the database is MySQL
    pub fn is_mysql() -> bool {
        GLOBAL_DB_TYPE.get() == Some(&DatabaseType::MySQL)
    }
    
    /// Check if the database is SQLite
    pub fn is_sqlite() -> bool {
        GLOBAL_DB_TYPE.get() == Some(&DatabaseType::SQLite)
    }
    
    /// Get the current configuration (for inspection)
    pub fn current() -> Config {
        Config::global()
    }
    
    /// Get the current pool configuration
    pub fn pool_config() -> PoolConfig {
        GLOBAL_POOL_CONFIG.get().cloned().unwrap_or_default()
    }
    
    /// Get the configured schema file path (if any)
    pub fn schema_file_path() -> Option<&'static str> {
        SCHEMA_FILE_PATH.get().map(|s| s.as_str())
    }
    
    /// Write schema to the configured file
    ///
    /// This method generates SQL schema for the provided models and writes
    /// it to the file configured via `.schema_file()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tideorm::prelude::*;
    /// use tideorm::schema::{SchemaGenerator, TableSchemaBuilder, ColumnSchema};
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .schema_file("schema.sql")
    ///     .connect()
    ///     .await?;
    ///
    /// // Generate schema from model metadata
    /// let db_type = TideConfig::get_database_type().unwrap_or_default();
    /// let mut generator = SchemaGenerator::new(db_type);
    /// 
    /// // Add User table schema
    /// generator.add_table(
    ///     TableSchemaBuilder::new(User::table_name())
    ///         .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
    ///         .column(ColumnSchema::new("email", "TEXT").not_null())
    ///         .indexes(User::all_indexes())
    ///         .build()
    /// );
    ///
    /// TideConfig::write_schema_with_generator(&generator)?;
    /// ```
    pub fn write_schema_with_generator(generator: &crate::schema::SchemaGenerator) -> std::io::Result<()> {
        let Some(path) = SCHEMA_FILE_PATH.get() else {
            return Ok(()); // No schema file configured
        };
        
        let sql = generator.generate();
        std::fs::write(path, sql)?;
        Ok(())
    }
    
    /// Write raw SQL schema to the configured file
    ///
    /// Use this for simple cases where you have the SQL already.
    pub fn write_schema_sql(sql: &str) -> std::io::Result<()> {
        let Some(path) = SCHEMA_FILE_PATH.get() else {
            return Ok(()); // No schema file configured
        };
        
        std::fs::write(path, sql)?;
        Ok(())
    }
}

// ============================================================================
// REGISTER MIGRATIONS TRAIT
// ============================================================================

/// Trait for registering multiple migrations at once via tuples
///
/// This is implemented for tuples of up to 16 migration types.
/// Used by `TideConfig::migrations::<(Migration1, Migration2, ...)>()`.
///
/// # Example
///
/// ```rust,ignore
/// TideConfig::init()
///     .database("postgres://localhost/mydb")
///     .migrations::<(CreateUsersTable, CreatePostsTable)>()
///     .run_migrations(true)
///     .connect()
///     .await?;
/// ```
pub trait RegisterMigrations {
    /// Collect all migrations into a vector
    fn collect() -> Vec<Box<dyn Migration>>;
}

// Implement for empty tuple
impl RegisterMigrations for () {
    fn collect() -> Vec<Box<dyn Migration>> {
        Vec::new()
    }
}

// Implement for single migration
impl<A: Migration + Default + 'static> RegisterMigrations for (A,) {
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default())]
    }
}

// Implement for 2 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static> RegisterMigrations for (A, B) {
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default())]
    }
}

// Implement for 3 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default())]
    }
}

// Implement for 4 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default())]
    }
}

// Implement for 5 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default())]
    }
}

// Implement for 6 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default())]
    }
}

// Implement for 7 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default())]
    }
}

// Implement for 8 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default())]
    }
}

// Implement for 9 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static, I: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H, I) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default()), Box::new(I::default())]
    }
}

// Implement for 10 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static, I: Migration + Default + 'static, J: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H, I, J) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default()), Box::new(I::default()), Box::new(J::default())]
    }
}

// Implement for 11 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static, I: Migration + Default + 'static, J: Migration + Default + 'static, K: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H, I, J, K) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default()), Box::new(I::default()), Box::new(J::default()), Box::new(K::default())]
    }
}

// Implement for 12 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static, I: Migration + Default + 'static, J: Migration + Default + 'static, K: Migration + Default + 'static, L: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H, I, J, K, L) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default()), Box::new(I::default()), Box::new(J::default()), Box::new(K::default()), Box::new(L::default())]
    }
}

// Implement for 13 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static, I: Migration + Default + 'static, J: Migration + Default + 'static, K: Migration + Default + 'static, L: Migration + Default + 'static, M: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H, I, J, K, L, M) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default()), Box::new(I::default()), Box::new(J::default()), Box::new(K::default()), Box::new(L::default()), Box::new(M::default())]
    }
}

// Implement for 14 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static, I: Migration + Default + 'static, J: Migration + Default + 'static, K: Migration + Default + 'static, L: Migration + Default + 'static, M_: Migration + Default + 'static, N: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H, I, J, K, L, M_, N) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default()), Box::new(I::default()), Box::new(J::default()), Box::new(K::default()), Box::new(L::default()), Box::new(M_::default()), Box::new(N::default())]
    }
}

// Implement for 15 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static, I: Migration + Default + 'static, J: Migration + Default + 'static, K: Migration + Default + 'static, L: Migration + Default + 'static, M_: Migration + Default + 'static, N: Migration + Default + 'static, O: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H, I, J, K, L, M_, N, O) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default()), Box::new(I::default()), Box::new(J::default()), Box::new(K::default()), Box::new(L::default()), Box::new(M_::default()), Box::new(N::default()), Box::new(O::default())]
    }
}

// Implement for 16 migrations
impl<A: Migration + Default + 'static, B: Migration + Default + 'static, C: Migration + Default + 'static, D: Migration + Default + 'static, E: Migration + Default + 'static, F: Migration + Default + 'static, G: Migration + Default + 'static, H: Migration + Default + 'static, I: Migration + Default + 'static, J: Migration + Default + 'static, K: Migration + Default + 'static, L: Migration + Default + 'static, M_: Migration + Default + 'static, N: Migration + Default + 'static, O: Migration + Default + 'static, P: Migration + Default + 'static> 
    RegisterMigrations for (A, B, C, D, E, F, G, H, I, J, K, L, M_, N, O, P) 
{
    fn collect() -> Vec<Box<dyn Migration>> {
        vec![Box::new(A::default()), Box::new(B::default()), Box::new(C::default()), Box::new(D::default()), Box::new(E::default()), Box::new(F::default()), Box::new(G::default()), Box::new(H::default()), Box::new(I::default()), Box::new(J::default()), Box::new(K::default()), Box::new(L::default()), Box::new(M_::default()), Box::new(N::default()), Box::new(O::default()), Box::new(P::default())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.languages, vec!["en".to_string()]);
        assert_eq!(config.fallback_language, "en");
    }
    
    #[test]
    fn test_config_builder() {
        TideConfig::init()
            .languages(&["en", "fr"])
            .fallback_language("fr")
            .apply();
        
        let config = Config::global();
        assert!(config.languages.contains(&"fr".to_string()));
        assert_eq!(config.fallback_language, "fr");
    }
}
