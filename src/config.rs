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
use crate::{tide_info, tide_warn};

/// Global configuration instance
static GLOBAL_CONFIG: OnceLock<RwLock<Config>> = OnceLock::new();

/// Global schema file path (set via TideConfig::schema_file())
static SCHEMA_FILE_PATH: OnceLock<String> = OnceLock::new();

/// File URL generator function type
/// 
/// This function takes a field name and file attachment with full metadata and returns the full URL.
/// It can be customized globally via `TideConfig::file_url_generator()` or
/// per-model by overriding `ModelMeta::file_url_generator()`.
/// 
/// The generator receives:
/// - `field_name`: The name of the attachment field (e.g., "thumbnail", "avatar", "documents")
/// - `file`: The full `FileAttachment` struct with all metadata (key, filename, size, mime_type, etc.)
/// 
/// This allows URL generation based on both the field context and file metadata.
pub type FileUrlGenerator = fn(field_name: &str, file: &crate::attachments::FileAttachment) -> String;

/// Global file URL generator
static GLOBAL_FILE_URL_GENERATOR: OnceLock<FileUrlGenerator> = OnceLock::new();

/// Global pool configuration (set during connect)
static GLOBAL_POOL_CONFIG: OnceLock<PoolConfig> = OnceLock::new();

/// Supported database types
///
/// TideORM supports multiple database backends. Each has its own
/// specific features and SQL dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DatabaseType {
    /// PostgreSQL database
    /// 
    /// Features: JSONB, Arrays, Full-text search, LISTEN/NOTIFY
    /// URL format: `postgres://user:pass@host:5432/database`
    #[default]
    Postgres,
    
    /// MySQL database
    /// 
    /// Features: JSON, Full-text search, Spatial data
    /// URL format: `mysql://user:pass@host:3306/database`
    MySQL,
    
    /// MariaDB database
    ///
    /// MariaDB is a MySQL-compatible fork with additional features.
    /// Notably, MariaDB 10.5+ supports `INSERT ... RETURNING`.
    /// URL format: `mariadb://user:pass@host:3306/database`
    ///
    /// TideORM auto-detects MariaDB when connecting via `mysql://` to a
    /// MariaDB server, or when using `mariadb://` URLs explicitly.
    MariaDB,
    
    /// SQLite database (file-based or in-memory)
    /// 
    /// Features: Lightweight, zero-config, serverless
    /// URL format: `sqlite:./path/to/db.sqlite` or `sqlite::memory:`
    SQLite,
}

impl DatabaseType {
    /// Check if this database type uses the MySQL SQL dialect
    ///
    /// Returns `true` for both `MySQL` and `MariaDB`, since MariaDB
    /// is wire-compatible with MySQL and shares the same SQL dialect.
    pub fn is_mysql_compatible(&self) -> bool {
        matches!(self, DatabaseType::MySQL | DatabaseType::MariaDB)
    }
    
    /// Get the default port for this database type
    pub fn default_port(&self) -> u16 {
        match self {
            DatabaseType::Postgres => 5432,
            DatabaseType::MySQL | DatabaseType::MariaDB => 3306,
            DatabaseType::SQLite => 0, // No port for SQLite
        }
    }
    
    /// Get the URL scheme for this database type
    pub fn url_scheme(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "postgres",
            DatabaseType::MySQL => "mysql",
            DatabaseType::MariaDB => "mariadb",
            DatabaseType::SQLite => "sqlite",
        }
    }
    
    /// Check if this database supports JSON/JSONB columns
    pub fn supports_json(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,  // MySQL 5.7+ / MariaDB 10.2+
            DatabaseType::SQLite => true, // SQLite JSON1 extension (3.9+)
        }
    }
    
    /// Check if this database supports native JSON operators
    pub fn supports_native_json_operators(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,  // @>, <@, ?, @?
            DatabaseType::MySQL | DatabaseType::MariaDB => true,     // JSON_CONTAINS, JSON_EXTRACT
            DatabaseType::SQLite => true,    // json_extract, json_each
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
            DatabaseType::MariaDB => true,   // MariaDB 10.5+
            DatabaseType::SQLite => true,    // SQLite 3.35+
        }
    }
    
    /// Check if this database supports UPSERT (INSERT ... ON CONFLICT/DUPLICATE)
    pub fn supports_upsert(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,  // ON CONFLICT
            DatabaseType::MySQL | DatabaseType::MariaDB => true,     // ON DUPLICATE KEY
            DatabaseType::SQLite => true,    // ON CONFLICT (3.24+)
        }
    }
    
    /// Check if this database supports full-text search
    pub fn supports_fulltext_search(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,  // tsvector, tsquery
            DatabaseType::MySQL | DatabaseType::MariaDB => true,     // FULLTEXT index
            DatabaseType::SQLite => true,    // FTS5 extension
        }
    }
    
    /// Check if this database supports window functions
    pub fn supports_window_functions(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,     // MySQL 8.0+ / MariaDB 10.2+
            DatabaseType::SQLite => true,    // SQLite 3.25+
        }
    }
    
    /// Check if this database supports CTEs (Common Table Expressions)
    pub fn supports_cte(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => true,     // MySQL 8.0+ / MariaDB 10.2+
            DatabaseType::SQLite => true,    // SQLite 3.8.3+
        }
    }
    
    /// Check if this database supports multiple schemas
    pub fn supports_schemas(&self) -> bool {
        match self {
            DatabaseType::Postgres => true,
            DatabaseType::MySQL | DatabaseType::MariaDB => false,    // Uses databases instead
            DatabaseType::SQLite => false,
        }
    }
    
    /// Get the recommended batch size for bulk inserts
    pub fn optimal_batch_size(&self) -> usize {
        match self {
            DatabaseType::Postgres => 1000,
            DatabaseType::MySQL | DatabaseType::MariaDB => 500,      // Lower due to max_allowed_packet
            DatabaseType::SQLite => 100,     // Lower for file-based DB
        }
    }
    
    /// Get the parameter placeholder style for this database
    pub fn param_style(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "$",   // $1, $2, $3
            DatabaseType::MySQL | DatabaseType::MariaDB => "?",
            DatabaseType::SQLite => "?",
        }
    }
    
    /// Get the identifier quote character for this database
    pub fn quote_char(&self) -> char {
        match self {
            DatabaseType::Postgres | DatabaseType::SQLite => '"',
            DatabaseType::MySQL | DatabaseType::MariaDB => '`',
        }
    }
    
    /// Try to detect database type from URL
    ///
    /// Note: `mariadb://` URLs return `MariaDB`. For `mysql://` URLs,
    /// the type is initially `MySQL` but may be auto-promoted to `MariaDB`
    /// after connecting if the server identifies as MariaDB.
    pub fn from_url(url: &str) -> Option<Self> {
        let url_lower = url.to_lowercase();
        if url_lower.starts_with("postgres://") || url_lower.starts_with("postgresql://") {
            Some(DatabaseType::Postgres)
        } else if url_lower.starts_with("mariadb://") {
            Some(DatabaseType::MariaDB)
        } else if url_lower.starts_with("mysql://") {
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
            DatabaseType::MariaDB => write!(f, "MariaDB"),
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
    
    /// Base URL for file attachments (e.g., "https://cdn.example.com/uploads")
    pub file_base_url: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            languages: vec!["en".to_string()],
            fallback_language: "en".to_string(),
            hidden_attributes: vec![],
            soft_delete_by_default: false,
            file_base_url: None,
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
    
    /// Read a value from the global config without cloning the entire struct
    #[inline]
    fn with_global<T>(f: impl FnOnce(&Config) -> T) -> T {
        let lock = GLOBAL_CONFIG.get_or_init(|| RwLock::new(Config::default()));
        let guard = lock.read();
        f(&guard)
    }
    
    /// Get allowed languages from global config
    pub fn get_languages() -> Vec<String> {
        Self::with_global(|c| c.languages.clone())
    }
    
    /// Get fallback language from global config
    pub fn get_fallback_language() -> String {
        Self::with_global(|c| c.fallback_language.clone())
    }
    
    /// Get hidden attributes from global config
    pub fn get_hidden_attributes() -> Vec<String> {
        Self::with_global(|c| c.hidden_attributes.clone())
    }
    
    /// Check if soft delete is enabled by default
    pub fn is_soft_delete_default() -> bool {
        Self::with_global(|c| c.soft_delete_by_default)
    }
    
    /// Get the file base URL from global config
    pub fn get_file_base_url() -> Option<String> {
        Self::with_global(|c| c.file_base_url.clone())
    }
    
    /// Get the global file URL generator function
    /// 
    /// Returns the custom generator if set, otherwise returns the default generator
    /// which uses `file_base_url` to construct URLs.
    pub fn get_file_url_generator() -> FileUrlGenerator {
        GLOBAL_FILE_URL_GENERATOR
            .get()
            .copied()
            .unwrap_or(Self::default_file_url_generator)
    }
    
    /// Set a custom global file URL generator
    /// 
    /// This allows full customization of how file URLs are generated.
    /// The function receives the field name and full `FileAttachment` with all metadata.
    /// 
    /// # Example
    /// 
    /// ```rust,ignore
    /// Config::set_file_url_generator(|field_name, file| {
    ///     // Use field name and file metadata for advanced URL generation
    ///     match field_name {
    ///         "thumbnail" => format!("https://images-cdn.example.com/thumb/{}", file.key),
    ///         "avatar" => format!("https://avatars.example.com/{}", file.key),
    ///         _ => format!("https://cdn.example.com/{}", file.key),
    ///     }
    /// });
    /// ```
    pub fn set_file_url_generator(generator: FileUrlGenerator) {
        if GLOBAL_FILE_URL_GENERATOR.set(generator).is_err() {
            tide_warn!("File URL generator already set, ignoring subsequent call");
        }
    }
    
    /// Default file URL generator
    /// 
    /// Uses `file_base_url` if set, otherwise returns the key as-is.
    /// The field_name parameter is available but not used in the default implementation.
    #[inline]
    pub fn default_file_url_generator(_field_name: &str, file: &crate::attachments::FileAttachment) -> String {
        if let Some(base_url) = Self::get_file_base_url() {
            let base = base_url.trim_end_matches('/');
            let key = file.key.trim_start_matches('/');
            format!("{}/{}", base, key)
        } else {
            file.key.clone()
        }
    }
    
    /// Generate a file URL using the global generator
    /// 
    /// This is a convenience method to generate URLs without needing
    /// to manually call the generator function.
    /// 
    /// # Arguments
    /// * `field_name` - The name of the attachment field (e.g., "thumbnail", "avatar")
    /// * `file` - The file attachment with metadata
    /// 
    /// # Example
    /// 
    /// ```rust,ignore
    /// let file = FileAttachment::new("uploads/2024/image.jpg");
    /// let url = Config::generate_file_url("thumbnail", &file);
    /// // Returns: "https://cdn.example.com/uploads/2024/image.jpg"
    /// ```
    #[inline]
    pub fn generate_file_url(field_name: &str, file: &crate::attachments::FileAttachment) -> String {
        Self::get_file_url_generator()(field_name, file)
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
/// let db = TideConfig::db()?;
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
    seeds: Vec<Box<dyn crate::seeding::Seed>>,
    run_seeds: bool,
    encryption_key: Option<String>,
    token_encoder: Option<crate::tokenization::TokenEncoder>,
    token_decoder: Option<crate::tokenization::TokenDecoder>,
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
            seeds: Vec::new(),
            run_seeds: false,
            encryption_key: None,
            token_encoder: None,
            token_decoder: None,
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
    
    /// Add a single seed
    ///
    /// Seeds are run in the order they are added. Seeds are tracked in a
    /// `_seeds` table, so previously executed seeds will be skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .seed(AdminUserSeeder)
    ///     .seed(CategorySeeder)
    ///     .seed(ProductSeeder)
    ///     .run_seeds(true)
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn seed<S: crate::seeding::Seed + 'static>(mut self, seed: S) -> Self {
        self.seeds.push(Box::new(seed));
        self
    }
    
    /// Add multiple seeds using a tuple
    ///
    /// This provides a convenient way to register many seeds at once.
    /// Seeds are run in tuple order, so list them in execution order.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .seeds::<(
    ///         AdminUserSeeder,
    ///         CategorySeeder,
    ///         ProductSeeder,
    ///         DevDataSeeder,
    ///     )>()
    ///     .run_seeds(true)
    ///     .connect()
    ///     .await?;
    /// ```
    ///
    /// Supports up to 16 seeds in a single tuple. For more seeds,
    /// call `.seeds()` multiple times or use `.seed()` individually.
    pub fn seeds<T: RegisterSeeds>(mut self) -> Self {
        self.seeds.extend(T::collect());
        self
    }
    
    /// Enable or disable automatic seed execution on connect
    ///
    /// When enabled, all registered seeds will be run automatically
    /// after connecting to the database (and after migrations if enabled).
    /// Seeds are tracked in a `_seeds` table, so previously executed seeds
    /// are skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .seed(AdminUserSeeder)
    ///     .seed(CategorySeeder)
    ///     .run_seeds(true)  // Run seeds on connect
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn run_seeds(mut self, enabled: bool) -> Self {
        self.run_seeds = enabled;
        self
    }
    
    /// Enable or disable automatic database schema synchronization
    ///
    /// When enabled, TideORM will automatically create/update database tables
    /// based on your model definitions after connecting.
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
    
    /// Set the base URL for file attachments
    ///
    /// This URL will be prepended to file keys when generating full URLs
    /// in JSON output. Use this for CDN URLs or storage service URLs.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .file_base_url("https://cdn.example.com/uploads")
    ///     .connect()
    ///     .await?;
    ///
    /// // Now file URLs in JSON will be like:
    /// // "https://cdn.example.com/uploads/2024/image.jpg"
    /// ```
    pub fn file_base_url(mut self, url: &str) -> Self {
        self.config.file_base_url = Some(url.to_string());
        self
    }
    
    /// Set a custom file URL generator function
    ///
    /// Use this when you need full control over URL generation, such as
    /// adding signed URLs, custom query parameters, or transformations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn generate_signed_url(key: &str) -> String {
    ///     let token = generate_access_token();
    ///     format!("https://cdn.example.com/{}?token={}", key, token)
    /// }
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .file_url_generator(generate_signed_url)
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn file_url_generator(self, generator: FileUrlGenerator) -> Self {
        Config::set_file_url_generator(generator);
        self
    }
    
    // ========================================================================
    // TOKENIZATION CONFIGURATION
    // ========================================================================
    
    /// Set the encryption key for record tokenization
    ///
    /// This key is used to encrypt/decrypt record IDs when generating tokens.
    /// The key should be at least 32 bytes for security. Keep this key secret
    /// and consistent - changing it will invalidate existing tokens.
    ///
    /// # Security Warning
    ///
    /// - **Never commit encryption keys to version control**
    /// - Use environment variables in production
    /// - Rotate keys only when necessary (invalidates existing tokens)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Load from environment variable
    /// let key = std::env::var("ENCRYPTION_KEY").expect("ENCRYPTION_KEY required");
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .encryption_key(&key)  // At least 32 characters
    ///     .connect()
    ///     .await?;
    ///
    /// // Now tokenization uses this key
    /// let user = User::find(1).await?;
    /// let token = user.to_token()?;  // Encrypted with your key
    /// ```
    pub fn encryption_key(mut self, key: &str) -> Self {
        self.encryption_key = Some(key.to_string());
        self
    }
    
    /// Set a custom token encoder function
    ///
    /// Override the default token encoding logic for all models.
    /// Model-level overrides take precedence over this global setting.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Simple prefix-based encoding
    /// fn custom_encoder(record_id: i64, model_name: &str) -> tideorm::Result<String> {
    ///     Ok(format!("{}-{}", model_name.to_lowercase(), record_id))
    /// }
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .token_encoder(custom_encoder)
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn token_encoder(mut self, encoder: crate::tokenization::TokenEncoder) -> Self {
        self.token_encoder = Some(encoder);
        self
    }
    
    /// Set a custom token decoder function
    ///
    /// Override the default token decoding logic for all models.
    /// Model-level overrides take precedence over this global setting.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Matching decoder for the prefix-based encoder
    /// fn custom_decoder(token: &str, model_name: &str) -> Option<i64> {
    ///     let prefix = format!("{}-", model_name.to_lowercase());
    ///     token.strip_prefix(&prefix)
    ///         .and_then(|id_str| id_str.parse().ok())
    /// }
    ///
    /// TideConfig::init()
    ///     .database("postgres://localhost/mydb")
    ///     .token_decoder(custom_decoder)
    ///     .connect()
    ///     .await?;
    /// ```
    pub fn token_decoder(mut self, decoder: crate::tokenization::TokenDecoder) -> Self {
        self.token_decoder = Some(decoder);
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
        
        // Apply tokenization settings
        if let Some(key) = &self.encryption_key {
            crate::tokenization::TokenConfig::set_encryption_key(key);
        }
        if let Some(encoder) = self.token_encoder {
            crate::tokenization::TokenConfig::set_encoder(encoder);
        }
        if let Some(decoder) = self.token_decoder {
            crate::tokenization::TokenConfig::set_decoder(decoder);
        }
        
        // Get database URL
        let url = self.database_url.ok_or_else(|| {
            crate::error::Error::configuration(
                "Database URL is required. Use .database(\"postgres://...\") to set it."
            )
        })?;
        
        // Determine database type (explicit or auto-detect)
        let mut db_type = match self.database_type {
            Some(t) => t,
            None => DatabaseType::from_url(&url).ok_or_else(|| {
                crate::error::Error::configuration(
                    "Could not detect database type from URL. \
                     Use .database_type(DatabaseType::Postgres) to set it explicitly."
                )
            })?,
        };
        
        // For MariaDB URLs, rewrite to mysql:// for the connection driver
        // (SeaORM/sqlx only understands mysql:// scheme)
        let connect_url = if url.to_lowercase().starts_with("mariadb://") {
            format!("mysql://{}", &url[url.find("://").unwrap() + 3..])
        } else {
            url.clone()
        };
        
        // Store pool config globally
        let _ = GLOBAL_POOL_CONFIG.set(self.pool.clone());
        
        // Build and connect to database with pool settings
        let db = Database::builder()
            .url(connect_url)
            .max_connections(self.pool.max_connections)
            .min_connections(self.pool.min_connections)
            .connect_timeout(self.pool.connect_timeout)
            .idle_timeout(self.pool.idle_timeout)
            .max_lifetime(self.pool.max_lifetime)
            .build()
            .await?;
        
        // Auto-detect MariaDB when connected via mysql:// to a MariaDB server
        if db_type == DatabaseType::MySQL {
            if let Ok(version) = Self::detect_server_version(&db).await {
                if version.to_lowercase().contains("mariadb") {
                    db_type = DatabaseType::MariaDB;
                    tide_info!("Auto-detected MariaDB server: {}", version);
                }
            }
        }
        
        // Store database type globally
        let _ = GLOBAL_DB_TYPE.set(db_type);
        
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
                tide_info!("{}", result);
            }
        }
        
        // Run seeds if enabled and seeds were registered
        if self.run_seeds && !self.seeds.is_empty() {
            let mut seeder = crate::seeding::Seeder::new();
            for seed in self.seeds {
                seeder = seeder.add_boxed(seed);
            }
            let result = seeder.run().await?;
            if result.has_executed() {
                tide_info!("{}", result);
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
    /// Returns an error if `connect()` has not been called yet.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let db = TideConfig::db()?;
    /// ```
    pub fn db() -> crate::error::Result<&'static Database> {
        crate::database::require_db()
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
    
    /// Check if the database is MySQL (strict — returns false for MariaDB)
    ///
    /// Use `is_mysql_compatible()` to check for both MySQL and MariaDB.
    pub fn is_mysql() -> bool {
        GLOBAL_DB_TYPE.get() == Some(&DatabaseType::MySQL)
    }
    
    /// Check if the database is MariaDB
    pub fn is_mariadb() -> bool {
        GLOBAL_DB_TYPE.get() == Some(&DatabaseType::MariaDB)
    }
    
    /// Check if the database is MySQL or MariaDB
    ///
    /// Use this when you need to check for MySQL-compatible syntax
    /// that applies to both MySQL and MariaDB.
    pub fn is_mysql_compatible() -> bool {
        matches!(GLOBAL_DB_TYPE.get(), Some(DatabaseType::MySQL) | Some(DatabaseType::MariaDB))
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
    
    /// Detect the server version string by running `SELECT VERSION()`
    ///
    /// Used internally to auto-detect MariaDB servers connected via `mysql://`.
    async fn detect_server_version(db: &Database) -> Result<String> {
        use crate::internal::{ConnectionTrait, Statement, DbBackend};
        
        let conn = db.__internal_connection();
        let backend = conn.get_database_backend();
        
        // Only probe MySQL-type connections
        if backend != DbBackend::MySql {
            return Err(crate::error::Error::internal("Not a MySQL-type connection"));
        }
        
        let stmt = Statement::from_string(backend, "SELECT VERSION() AS version".to_string());
        let result = conn.query_one_raw(stmt)
            .await
            .map_err(|e| crate::error::Error::query(e.to_string()))?;
        
        match result {
            Some(row) => {
                let version: String = row.try_get("", "version")
                    .map_err(|e| crate::error::Error::query(e.to_string()))?;
                Ok(version)
            }
            None => Err(crate::error::Error::query("Could not retrieve server version")),
        }
    }
}

// ============================================================================
// REGISTER MIGRATIONS TRAIT
// ============================================================================

/// Trait for registering multiple migrations at once via tuples
///
/// This is implemented for tuples of up to 200 migration types.
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

/// Generate `RegisterMigrations` and `RegisterSeeds` for tuples of size 1..N
macro_rules! impl_register_tuples {
    // Base case: single type
    ($first:ident) => {
        impl<$first: Migration + Default + 'static> RegisterMigrations for ($first,) {
            fn collect() -> Vec<Box<dyn Migration>> {
                vec![Box::new($first::default())]
            }
        }
        impl<$first: Seed + Default + 'static> RegisterSeeds for ($first,) {
            fn collect() -> Vec<Box<dyn Seed>> {
                vec![Box::new($first::default())]
            }
        }
    };
    // Recursive case: first + rest
    ($first:ident, $($rest:ident),+) => {
        // Recurse for smaller tuples first
        impl_register_tuples!($($rest),+);

        impl<$first: Migration + Default + 'static, $($rest: Migration + Default + 'static),+>
            RegisterMigrations for ($first, $($rest),+)
        {
            fn collect() -> Vec<Box<dyn Migration>> {
                vec![Box::new($first::default()), $(Box::new($rest::default())),+]
            }
        }

        impl<$first: Seed + Default + 'static, $($rest: Seed + Default + 'static),+>
            RegisterSeeds for ($first, $($rest),+)
        {
            fn collect() -> Vec<Box<dyn Seed>> {
                vec![Box::new($first::default()), $(Box::new($rest::default())),+]
            }
        }
    };
}

// Generate impls for tuples of 1 through 200 elements
impl_register_tuples!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18,
    T19, T20, T21, T22, T23, T24, T25, T26, T27, T28, T29, T30, T31, T32, T33, T34, T35,
    T36, T37, T38, T39, T40, T41, T42, T43, T44, T45, T46, T47, T48, T49, T50, T51, T52,
    T53, T54, T55, T56, T57, T58, T59, T60, T61, T62, T63, T64, T65, T66, T67, T68, T69,
    T70, T71, T72, T73, T74, T75, T76, T77, T78, T79, T80, T81, T82, T83, T84, T85, T86,
    T87, T88, T89, T90, T91, T92, T93, T94, T95, T96, T97, T98, T99, T100, T101, T102,
    T103, T104, T105, T106, T107, T108, T109, T110, T111, T112, T113, T114, T115, T116,
    T117, T118, T119, T120, T121, T122, T123, T124, T125, T126, T127, T128, T129, T130,
    T131, T132, T133, T134, T135, T136, T137, T138, T139, T140, T141, T142, T143, T144,
    T145, T146, T147, T148, T149, T150, T151, T152, T153, T154, T155, T156, T157, T158,
    T159, T160, T161, T162, T163, T164, T165, T166, T167, T168, T169, T170, T171, T172,
    T173, T174, T175, T176, T177, T178, T179, T180, T181, T182, T183, T184, T185, T186,
    T187, T188, T189, T190, T191, T192, T193, T194, T195, T196, T197, T198, T199, T200
);

// ============================================================================
// REGISTER SEEDS TRAIT
// ============================================================================

use crate::seeding::Seed;

/// Trait for registering multiple seeds via tuple syntax
///
/// This is implemented for tuples of up to 200 seed types.
/// Used by `TideConfig::seeds::<(Seed1, Seed2, ...)>()`.
///
/// # Example
///
/// ```rust,ignore
/// TideConfig::init()
///     .database("postgres://localhost/mydb")
///     .seeds::<(AdminUserSeeder, CategorySeeder, ProductSeeder)>()
///     .run_seeds(true)
///     .connect()
///     .await?;
/// ```
pub trait RegisterSeeds {
    /// Collect all seeds into a vector
    fn collect() -> Vec<Box<dyn Seed>>;
}

// Implement for empty tuple
impl RegisterSeeds for () {
    fn collect() -> Vec<Box<dyn Seed>> {
        Vec::new()
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
    
    #[test]
    fn test_database_type_from_url() {
        assert_eq!(
            DatabaseType::from_url("postgres://localhost/test"),
            Some(DatabaseType::Postgres)
        );
        assert_eq!(
            DatabaseType::from_url("postgresql://localhost/test"),
            Some(DatabaseType::Postgres)
        );
        assert_eq!(
            DatabaseType::from_url("mysql://localhost/test"),
            Some(DatabaseType::MySQL)
        );
        assert_eq!(
            DatabaseType::from_url("mariadb://localhost/test"),
            Some(DatabaseType::MariaDB)
        );
        assert_eq!(
            DatabaseType::from_url("sqlite:./test.db"),
            Some(DatabaseType::SQLite)
        );
        assert_eq!(
            DatabaseType::from_url("sqlite::memory:"),
            Some(DatabaseType::SQLite)
        );
        assert_eq!(
            DatabaseType::from_url("invalid://localhost"),
            None
        );
    }
    
    #[test]
    fn test_database_type_supports_json() {
        assert!(DatabaseType::Postgres.supports_json());
        assert!(DatabaseType::MySQL.supports_json());
        assert!(DatabaseType::MariaDB.supports_json());
        assert!(DatabaseType::SQLite.supports_json());
    }
    
    #[test]
    fn test_database_type_supports_arrays() {
        assert!(DatabaseType::Postgres.supports_arrays());
        assert!(!DatabaseType::MySQL.supports_arrays());
        assert!(!DatabaseType::MariaDB.supports_arrays());
        assert!(!DatabaseType::SQLite.supports_arrays());
    }
    
    #[test]
    fn test_database_type_supports_returning() {
        assert!(DatabaseType::Postgres.supports_returning());
        assert!(!DatabaseType::MySQL.supports_returning());
        assert!(DatabaseType::MariaDB.supports_returning());  // MariaDB 10.5+
        assert!(DatabaseType::SQLite.supports_returning());
    }
    
    #[test]
    fn test_database_type_supports_upsert() {
        assert!(DatabaseType::Postgres.supports_upsert());
        assert!(DatabaseType::MySQL.supports_upsert());
        assert!(DatabaseType::MariaDB.supports_upsert());
        assert!(DatabaseType::SQLite.supports_upsert());
    }
    
    #[test]
    fn test_database_type_supports_fulltext_search() {
        assert!(DatabaseType::Postgres.supports_fulltext_search());
        assert!(DatabaseType::MySQL.supports_fulltext_search());
        assert!(DatabaseType::MariaDB.supports_fulltext_search());
        assert!(DatabaseType::SQLite.supports_fulltext_search());
    }
    
    #[test]
    fn test_database_type_supports_window_functions() {
        assert!(DatabaseType::Postgres.supports_window_functions());
        assert!(DatabaseType::MySQL.supports_window_functions());
        assert!(DatabaseType::MariaDB.supports_window_functions());
        assert!(DatabaseType::SQLite.supports_window_functions());
    }
    
    #[test]
    fn test_database_type_supports_cte() {
        assert!(DatabaseType::Postgres.supports_cte());
        assert!(DatabaseType::MySQL.supports_cte());
        assert!(DatabaseType::MariaDB.supports_cte());
        assert!(DatabaseType::SQLite.supports_cte());
    }
    
    #[test]
    fn test_database_type_supports_schemas() {
        assert!(DatabaseType::Postgres.supports_schemas());
        assert!(!DatabaseType::MySQL.supports_schemas());
        assert!(!DatabaseType::MariaDB.supports_schemas());
        assert!(!DatabaseType::SQLite.supports_schemas());
    }
    
    #[test]
    fn test_database_type_optimal_batch_size() {
        assert_eq!(DatabaseType::Postgres.optimal_batch_size(), 1000);
        assert_eq!(DatabaseType::MySQL.optimal_batch_size(), 500);
        assert_eq!(DatabaseType::MariaDB.optimal_batch_size(), 500);
        assert_eq!(DatabaseType::SQLite.optimal_batch_size(), 100);
    }
    
    #[test]
    fn test_database_type_param_style() {
        assert_eq!(DatabaseType::Postgres.param_style(), "$");
        assert_eq!(DatabaseType::MySQL.param_style(), "?");
        assert_eq!(DatabaseType::MariaDB.param_style(), "?");
        assert_eq!(DatabaseType::SQLite.param_style(), "?");
    }
    
    #[test]
    fn test_database_type_quote_char() {
        assert_eq!(DatabaseType::Postgres.quote_char(), '"');
        assert_eq!(DatabaseType::MySQL.quote_char(), '`');
        assert_eq!(DatabaseType::MariaDB.quote_char(), '`');
        assert_eq!(DatabaseType::SQLite.quote_char(), '"');
    }
    
    #[test]
    fn test_database_type_default_port() {
        assert_eq!(DatabaseType::Postgres.default_port(), 5432);
        assert_eq!(DatabaseType::MySQL.default_port(), 3306);
        assert_eq!(DatabaseType::MariaDB.default_port(), 3306);
        assert_eq!(DatabaseType::SQLite.default_port(), 0);
    }
    
    #[test]
    fn test_database_type_url_scheme() {
        assert_eq!(DatabaseType::Postgres.url_scheme(), "postgres");
        assert_eq!(DatabaseType::MySQL.url_scheme(), "mysql");
        assert_eq!(DatabaseType::MariaDB.url_scheme(), "mariadb");
        assert_eq!(DatabaseType::SQLite.url_scheme(), "sqlite");
    }
    
    #[test]
    fn test_database_type_display() {
        assert_eq!(format!("{}", DatabaseType::Postgres), "PostgreSQL");
        assert_eq!(format!("{}", DatabaseType::MySQL), "MySQL");
        assert_eq!(format!("{}", DatabaseType::MariaDB), "MariaDB");
        assert_eq!(format!("{}", DatabaseType::SQLite), "SQLite");
    }
    
    #[test]
    fn test_database_type_is_mysql_compatible() {
        assert!(!DatabaseType::Postgres.is_mysql_compatible());
        assert!(DatabaseType::MySQL.is_mysql_compatible());
        assert!(DatabaseType::MariaDB.is_mysql_compatible());
        assert!(!DatabaseType::SQLite.is_mysql_compatible());
    }
    
    #[test]
    fn test_tide_config_schema_file() {
        // Test that schema_file can be set on the builder
        let config = TideConfig::init()
            .database_type(DatabaseType::Postgres)
            .database("postgres://localhost/test")
            .schema_file("test_schema.sql");
        
        // Verify the schema_file was set
        assert_eq!(config.schema_file, Some("test_schema.sql".to_string()));
    }
    
    #[test]
    fn test_tide_config_schema_file_with_path() {
        // Test with a full path
        let config = TideConfig::init()
            .database("postgres://localhost/test")
            .schema_file("./database/schema.sql");
        
        assert_eq!(config.schema_file, Some("./database/schema.sql".to_string()));
    }
    
    #[test]
    fn test_tide_config_no_schema_file() {
        // Test that schema_file is None by default
        let config = TideConfig::init()
            .database("postgres://localhost/test");
        
        assert!(config.schema_file.is_none());
    }
    
    #[test]
    fn test_pool_config_defaults() {
        let pool = PoolConfig::default();
        
        assert_eq!(pool.max_connections, 10);
        assert_eq!(pool.min_connections, 1);
        assert_eq!(pool.connect_timeout, Duration::from_secs(8));
        assert_eq!(pool.idle_timeout, Duration::from_secs(600));
        assert_eq!(pool.max_lifetime, Duration::from_secs(1800));
        assert_eq!(pool.acquire_timeout, Duration::from_secs(8));
    }
    
    #[test]
    fn test_tide_config_full_chain() {
        // Test that all config options can be chained together
        let config = TideConfig::init()
            .database_type(DatabaseType::Postgres)
            .database("postgres://localhost/test")
            .max_connections(20)
            .min_connections(5)
            .connect_timeout(Duration::from_secs(10))
            .idle_timeout(Duration::from_secs(300))
            .max_lifetime(Duration::from_secs(3600))
            .acquire_timeout(Duration::from_secs(5))
            .schema_file("schema.sql")
            .sync(false)
            .languages(&["en", "fr", "ar"])
            .fallback_language("en")
            .hidden_attributes(&["password", "secret"]);
        
        assert_eq!(config.database_type, Some(DatabaseType::Postgres));
        assert_eq!(config.database_url, Some("postgres://localhost/test".to_string()));
        assert_eq!(config.pool.max_connections, 20);
        assert_eq!(config.pool.min_connections, 5);
        assert_eq!(config.pool.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.pool.idle_timeout, Duration::from_secs(300));
        assert_eq!(config.pool.max_lifetime, Duration::from_secs(3600));
        assert_eq!(config.pool.acquire_timeout, Duration::from_secs(5));
        assert_eq!(config.schema_file, Some("schema.sql".to_string()));
        assert!(!config.sync_enabled);
        assert_eq!(config.config.languages, vec!["en", "fr", "ar"]);
        assert_eq!(config.config.fallback_language, "en");
        assert_eq!(config.config.hidden_attributes, vec!["password", "secret"]);
    }
}
