use std::time::Duration;

#[cfg(feature = "attachments")]
use super::FileUrlGenerator;
use super::state::{
    global_db_type, global_schema_file_path, set_global_db_type, set_global_schema_file_path,
    with_global_config_mut,
};
use super::{Config, DatabaseType, PoolConfig};

use crate::database::Database;
use crate::error::{Error, Result};
use crate::internal::Backend;
use crate::migration::Migration;
use crate::tide_info;
use crate::tide_warn;

/// The startup builder: everything TideORM needs to know before the first query.
///
/// Chain the settings you need and finish with one of two terminals:
///
/// - [`connect`](TideConfig::connect) opens the pool, installs it as the global
///   database, and runs whatever startup work was requested (migrations, seeds,
///   schema sync, schema-file export). This is what an application calls.
/// - [`apply`](TideConfig::apply) installs the same settings **without** opening
///   a connection, for tests and for tools that only need the configuration.
///
/// ```ignore
/// TideConfig::init()
///     .database(&std::env::var("DATABASE_URL")?)
///     .max_connections(20)
///     .models_matching("src/models/*")
///     .sync(true)
///     .connect()
///     .await?;
/// ```
///
/// The settings are global, and both terminals overwrite the previous ones.
/// [`reset`](TideConfig::reset) puts them back to defaults, which is how tests
/// avoid leaking configuration into each other.
#[derive(Default)]
pub struct TideConfig {
    pub(crate) config: Config,
    pub(crate) database_type: Option<DatabaseType>,
    pub(crate) database_url: Option<String>,
    pub(crate) pool: PoolConfig,
    pub(crate) sync_enabled: bool,
    pub(crate) force_sync: bool,
    pub(crate) schema_file: Option<String>,
    migrations: Vec<Box<dyn Migration>>,
    run_migrations: bool,
    seeds: Vec<Box<dyn crate::seeding::Seed>>,
    run_seeds: bool,
    encryption_key: Option<String>,
    token_encoder: Option<crate::tokenization::TokenEncoder>,
    token_decoder: Option<crate::tokenization::TokenDecoder>,
}

impl TideConfig {
    /// Start a builder with TideORM's defaults.
    ///
    /// Nothing is registered and no database URL is set, so
    /// [`connect`](TideConfig::connect) will fail until
    /// [`database`](TideConfig::database) is called.
    #[must_use]
    pub fn init() -> Self {
        Self::default()
    }

    /// Register one migration, appended after any already registered.
    ///
    /// Registering is not running: migrations only execute if
    /// [`run_migrations(true)`](TideConfig::run_migrations) is also set, and
    /// then only from [`connect`](TideConfig::connect). They run in registration
    /// order.
    #[must_use]
    pub fn migration<M: Migration + 'static>(mut self, migration: M) -> Self {
        self.migrations.push(Box::new(migration));
        self
    }

    /// Choose whether [`connect`](TideConfig::connect) runs the registered migrations.
    ///
    /// Off by default, so registering a migration never surprises you by
    /// altering the database at startup.
    #[must_use]
    pub fn run_migrations(mut self, enabled: bool) -> Self {
        self.run_migrations = enabled;
        self
    }

    /// Register one seed, appended after any already registered.
    ///
    /// Like migrations, seeds only run when
    /// [`run_seeds(true)`](TideConfig::run_seeds) is set.
    #[must_use]
    pub fn seed<S: crate::seeding::Seed + 'static>(mut self, seed: S) -> Self {
        self.seeds.push(Box::new(seed));
        self
    }

    /// Choose whether [`connect`](TideConfig::connect) runs the registered seeds.
    ///
    /// Off by default. Seeds run after migrations and schema sync, so they can
    /// fill the tables either creates; if seeding fails, what already ran is
    /// **not** rolled back and `connect` returns the error with the database
    /// partially initialized.
    #[must_use]
    pub fn run_seeds(mut self, enabled: bool) -> Self {
        self.run_seeds = enabled;
        self
    }

    /// Choose whether [`connect`](TideConfig::connect) syncs the schema from the
    /// registered models.
    ///
    /// Schema sync creates and alters tables to match the models registered with
    /// [`models`](TideConfig::models) or [`models_matching`](TideConfig::models_matching),
    /// which is convenient in development and a poor substitute for migrations
    /// in production. It only adds missing tables and columns;
    /// [`force_sync`](TideConfig::force_sync) instead drops and recreates every
    /// registered table.
    #[must_use]
    pub fn sync(mut self, enabled: bool) -> Self {
        self.sync_enabled = enabled;
        self
    }

    /// Register models explicitly, by type.
    ///
    /// `T` is a tuple of model types. Registration is what makes a model visible
    /// to schema sync and schema export; use
    /// [`models_matching`](TideConfig::models_matching) to pick them up by path
    /// instead.
    #[must_use]
    pub fn models<T: crate::sync::RegisterModels>(self) -> Self {
        T::register_all();
        self
    }

    /// Register all compiled TideORM models whose source file path matches a glob pattern.
    ///
    /// This matches against the source path captured from each `#[tideorm::model]` invocation,
    /// so matching files still need to be compiled into the crate through normal `mod`
    /// declarations. Supported wildcards are `*` for one path segment and `**` across
    /// directories.
    #[must_use]
    pub fn models_matching(self, pattern: &str) -> Self {
        crate::sync::SyncRegistry::register_models_matching(pattern);
        self
    }

    /// Drop and recreate the table of every registered model on connect.
    ///
    /// Every row in those tables is deleted, on every start. Keep it to local
    /// development and tests, never production startup. Has no effect unless
    /// [`sync(true)`](TideConfig::sync) is also set.
    #[must_use]
    pub fn force_sync(mut self, enabled: bool) -> Self {
        self.force_sync = enabled;
        self
    }

    /// Write the generated schema SQL to `path` during [`connect`](TideConfig::connect).
    ///
    /// The file is a readable dump of what TideORM believes the schema is,
    /// meant to be committed and reviewed in diffs. It is written once per
    /// `connect`, after any schema sync; nothing rewrites it in between.
    /// [`schema_file_path`](TideConfig::schema_file_path) reports the path.
    #[must_use]
    pub fn schema_file(mut self, path: &str) -> Self {
        self.schema_file = Some(path.to_string());
        self
    }

    /// State the backend explicitly instead of inferring it from the URL.
    ///
    /// Only needed when the URL scheme is ambiguous. Note that a `mysql://` URL
    /// pointing at a MariaDB server is detected automatically during
    /// [`connect`](TideConfig::connect) by querying the server version, and
    /// [`get_database_type`](TideConfig::get_database_type) then reports MariaDB.
    #[must_use]
    pub fn database_type(mut self, db_type: DatabaseType) -> Self {
        self.database_type = Some(db_type);
        self
    }

    /// Set the connection URL. Required before [`connect`](TideConfig::connect).
    ///
    /// The scheme selects the backend: `postgres://`, `mysql://`, `mariadb://`,
    /// or `sqlite:`. Note the URL carries credentials — read it from the
    /// environment rather than hard-coding it.
    #[must_use]
    pub fn database(mut self, url: &str) -> Self {
        self.database_url = Some(url.to_string());
        self
    }

    /// Cap the connection pool size. Defaults to 10.
    ///
    /// This is the ceiling on concurrent database work for the whole process,
    /// so it should sit below what the server allows across all your instances.
    #[must_use]
    pub fn max_connections(mut self, n: u32) -> Self {
        self.pool.max_connections = n;
        self
    }

    /// Keep at least this many connections open. Defaults to 1.
    ///
    /// Raising it trades idle server connections for fewer cold-start latency
    /// spikes.
    #[must_use]
    pub fn min_connections(mut self, n: u32) -> Self {
        self.pool.min_connections = n;
        self
    }

    /// How long to wait when opening a new connection. Defaults to 8 seconds.
    ///
    /// The driver has one timeout for handing out a connection, opening one
    /// when the pool has room, so this and
    /// [`acquire_timeout`](TideConfig::acquire_timeout) set the same limit,
    /// and the longer of the two applies.
    #[must_use]
    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.pool.connect_timeout = duration;
        self
    }

    /// How long an unused connection may sit in the pool before it is closed.
    /// Defaults to 10 minutes.
    #[must_use]
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.pool.idle_timeout = duration;
        self
    }

    /// Retire connections after this long regardless of use. Defaults to 30 minutes.
    ///
    /// Keep it under any idle or lifetime limit enforced by the server or a
    /// proxy in front of it, so the pool recycles connections before they are
    /// cut from the other end.
    #[must_use]
    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.pool.max_lifetime = duration;
        self
    }

    /// How long a query may wait for a free pooled connection. Defaults to 8 seconds.
    ///
    /// This is the one that fires under load: exceeding it means the pool is
    /// saturated, not that the database is unreachable. It shares the
    /// driver's one timeout with [`connect_timeout`](TideConfig::connect_timeout),
    /// and the longer of the two applies.
    #[must_use]
    pub fn acquire_timeout(mut self, duration: Duration) -> Self {
        self.pool.acquire_timeout = duration;
        self
    }

    /// Set the languages available to translated fields. Defaults to `["en"]`.
    #[must_use]
    pub fn languages(mut self, langs: &[&str]) -> Self {
        self.config.languages = langs.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Set the language used when a translation is missing. Defaults to `"en"`.
    #[must_use]
    pub fn fallback_language(mut self, lang: &str) -> Self {
        self.config.fallback_language = lang.to_string();
        self
    }

    /// Hide these attributes from `to_json()` for every model.
    ///
    /// Applied on top of each model's own hidden attributes, which is the place
    /// to put process-wide secrets such as `password_digest`. This only affects
    /// TideORM's JSON rendering — a direct `serde_json::to_value` still sees the
    /// fields.
    #[must_use]
    pub fn hidden_attributes(mut self, attrs: &[&str]) -> Self {
        self.config.hidden_attributes = attrs.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Prefix attachment URLs with this base for every field.
    ///
    /// Overridden per field by [`file_base_url_for`](TideConfig::file_base_url_for).
    /// With no base set, the raw storage key is used as the URL.
    #[must_use]
    pub fn file_base_url(mut self, url: &str) -> Self {
        self.config.file_base_url = Some(url.to_string());
        self
    }

    /// Prefix one attachment field's URLs with its own base.
    ///
    /// Takes precedence over [`file_base_url`](TideConfig::file_base_url), which
    /// is how a single field can be served from a different CDN or bucket.
    #[must_use]
    pub fn file_base_url_for(mut self, field_name: &str, url: &str) -> Self {
        self.config
            .file_field_base_urls
            .insert(field_name.to_string(), url.to_string());
        self
    }

    /// Replace URL generation for attachments entirely.
    ///
    /// The function is called for every attachment rendered by `to_json()`,
    /// which is where signed or expiring URLs belong. It supersedes the base-URL
    /// settings unless your generator consults them itself.
    ///
    /// Unlike the other setters this takes effect immediately, not at
    /// [`connect`](TideConfig::connect) or [`apply`](TideConfig::apply).
    #[cfg(feature = "attachments")]
    #[must_use]
    pub fn file_url_generator(self, generator: FileUrlGenerator) -> Self {
        super::state::set_global_file_url_generator(Some(generator));
        self
    }

    /// Set the master key for tokenization and `#[tideorm(encrypted)]` fields.
    ///
    /// Per-column keys are derived from it, so changing this key makes every
    /// previously written ciphertext and token undecryptable. Load it from the
    /// environment or a secret manager, never from source.
    #[must_use]
    pub fn encryption_key(mut self, key: &str) -> Self {
        self.encryption_key = Some(key.to_string());
        self
    }

    /// Replace how tokens are encoded.
    ///
    /// Pair it with a matching [`token_decoder`](TideConfig::token_decoder);
    /// setting only one of the two produces tokens that cannot be read back.
    #[must_use]
    pub fn token_encoder(mut self, encoder: crate::tokenization::TokenEncoder) -> Self {
        self.token_encoder = Some(encoder);
        self
    }

    /// Replace how tokens are decoded.
    ///
    /// Must accept whatever [`token_encoder`](TideConfig::token_encoder)
    /// produces. A token it rejects surfaces as `Ok(None)`, not an error, so a
    /// mismatched pair fails silently.
    #[must_use]
    pub fn token_decoder(mut self, decoder: crate::tokenization::TokenDecoder) -> Self {
        self.token_decoder = Some(decoder);
        self
    }

    /// Install the tokenization settings collected by the builder into `TokenConfig`.
    ///
    /// Shared by `connect` and `apply` so that both entry points honor
    /// `encryption_key`, `token_encoder`, and `token_decoder`.
    fn install_tokenization_settings(
        encryption_key: Option<&str>,
        token_encoder: Option<crate::tokenization::TokenEncoder>,
        token_decoder: Option<crate::tokenization::TokenDecoder>,
    ) {
        if let Some(key) = encryption_key {
            crate::tokenization::TokenConfig::set_encryption_key(key);
        }
        if let Some(encoder) = token_encoder {
            crate::tokenization::TokenConfig::set_encoder(encoder);
        }
        if let Some(decoder) = token_decoder {
            crate::tokenization::TokenConfig::set_decoder(decoder);
        }
    }

    /// Apply the settings, open the connection pool, and run the requested startup work.
    ///
    /// In order: the configuration and tokenization settings are installed
    /// globally, the pool is opened (auto-detecting MariaDB behind a `mysql://`
    /// URL), the connection becomes the global handle every model uses, then —
    /// only when they were enabled — migrations run, schema sync runs, seeds
    /// run, and the schema file is written. Seeds come after sync so that they
    /// can fill the tables sync creates, and a `force_sync` does not drop what
    /// they wrote.
    ///
    /// The steps are **not** one atomic unit: a failure part-way through leaves
    /// the earlier steps applied, so a seed error can return `Err` with the
    /// migrations and sync already applied.
    ///
    /// Errors when no database URL was set, when the backend cannot be inferred
    /// from the URL, when a MySQL server cannot report its version, or when any
    /// startup step fails.
    pub async fn connect(self) -> Result<&'static Database> {
        with_global_config_mut(|c| *c = self.config);

        Self::install_tokenization_settings(
            self.encryption_key.as_deref(),
            self.token_encoder,
            self.token_decoder,
        );

        let url = self.database_url.ok_or_else(|| {
            Error::configuration(
                "Database URL is required. Use .database(\"postgres://...\") to set it.",
            )
        })?;

        let db_type = match self.database_type {
            Some(t) => t,
            None => DatabaseType::from_url(&url).ok_or_else(|| {
                Error::configuration(
                    "Could not detect database type from URL. \
                     Use .database_type(DatabaseType::Postgres) to set it explicitly.",
                )
            })?,
        };

        let db = Database::builder()
            .url(url)
            .pool(&self.pool)
            .build()
            .await?;

        let db_type = Self::resolve_database_type(db_type, &db).await?;
        set_global_db_type(Some(db_type));

        let db_ref = Database::set_global(db)?;

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

        if self.sync_enabled {
            crate::sync::sync_database_with_options(db_ref, self.force_sync).await?;
        }

        if self.run_seeds && !self.seeds.is_empty() {
            let mut seeder = crate::seeding::Seeder::new();
            for seed in self.seeds {
                seeder = seeder.add_boxed(seed);
            }
            let result = match seeder.run().await {
                Ok(result) => result,
                Err(error) => {
                    tide_warn!(
                        "Database seeding failed after initialization steps were already applied. The database may be partially initialized: migrations and schema sync may have run, but seed data is missing."
                    );
                    return Err(error);
                }
            };
            if result.has_executed() {
                tide_info!("{}", result);
            }
        }

        set_global_schema_file_path(self.schema_file.clone());
        if let Some(path) = &self.schema_file {
            crate::schema::SchemaWriter::write_schema(path).await?;
        }

        Ok(db_ref)
    }

    /// Install the settings globally without opening a connection.
    ///
    /// The offline half of [`connect`](TideConfig::connect): languages, hidden
    /// attributes, declared backend, schema-file path, and tokenization keys
    /// all take effect. No pool is created, so the URL and pool settings go
    /// unused, and no migrations, seeds, sync, or schema-file write run. Use it
    /// in tests and in tools that need the configuration but not the database.
    pub fn apply(self) {
        Self::install_tokenization_settings(
            self.encryption_key.as_deref(),
            self.token_encoder,
            self.token_decoder,
        );

        with_global_config_mut(|c| *c = self.config);

        set_global_db_type(self.database_type);

        set_global_schema_file_path(self.schema_file);
    }

    /// Restore the global configuration to its defaults.
    ///
    /// Intended for tests, where configuration set by one test would otherwise
    /// leak into the next. It clears configuration only — the global database
    /// connection is separate, and `Database::reset_global()` clears that.
    pub fn reset() {
        with_global_config_mut(|c| *c = Config::default());

        set_global_db_type(None);

        set_global_schema_file_path(None);

        #[cfg(feature = "attachments")]
        {
            super::state::set_global_file_url_generator(None);
        }
    }

    /// Return whether a global database connection has been installed.
    #[must_use]
    pub fn is_connected() -> bool {
        crate::database::has_global_db()
    }

    /// Return the configured backend, if one is known.
    ///
    /// This is the authoritative answer, and it distinguishes MySQL from
    /// MariaDB — which the driver-level backend does not, because SeaORM
    /// collapses them into one variant. Any MariaDB-specific decision must
    /// consult this rather than the driver.
    ///
    /// `None` before [`connect`](TideConfig::connect) or [`apply`](TideConfig::apply)
    /// has run.
    #[must_use]
    pub fn get_database_type() -> Option<DatabaseType> {
        global_db_type()
    }

    /// Return the schema-file path set by [`schema_file`](TideConfig::schema_file).
    #[must_use]
    pub fn schema_file_path() -> Option<String> {
        global_schema_file_path()
    }

    /// Settle the declared backend once the pool is open.
    ///
    /// A `mysql://` URL can reach MariaDB, so a MySQL connection is asked for
    /// its server version rather than assumed to be MySQL. The pool has only
    /// just opened: if detection fails,
    /// even on a statement as trivial as `SELECT VERSION()`, the connection is
    /// unusable and the error is returned instead of silently settling on MySQL.
    pub(super) async fn resolve_database_type(
        declared: DatabaseType,
        db: &Database,
    ) -> Result<DatabaseType> {
        if declared != DatabaseType::MySQL || db.__internal_backend()? != Backend::MySql {
            return Ok(declared);
        }

        // The pool asked the server when it opened.
        if db.current_inner()?.is_mariadb() {
            tide_info!("Auto-detected a MariaDB server");
            Ok(DatabaseType::MariaDB)
        } else {
            Ok(DatabaseType::MySQL)
        }
    }
}
