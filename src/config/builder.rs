#[cfg(feature = "attachments")]
use super::FileUrlGenerator;
use super::state::{
    global_db_type, global_schema_file_path, set_global_db_type, set_global_schema_file_path,
    with_global_config_mut,
};
use super::{Config, DatabaseType, PoolConfig};

use crate::database::Database;
use crate::error::{Error, Result};
use crate::migration::Migration;
use crate::tide_info;
use crate::tide_warn;

/// The startup builder: everything TideORM needs to know before the first query.
///
/// Chain the settings you need and finish with one of three terminals:
///
/// - [`connect`](TideConfig::connect) opens the pool, installs it as the global
///   database, and runs whatever startup work was requested (migrations, seeds,
///   schema sync, schema-file export). This is what an application calls.
/// - [`connect_only`](TideConfig::connect_only) does the same without the
///   startup work, for tools such as the CLI's migration and seed runners.
/// - [`apply`](TideConfig::apply) installs the same settings **without** opening
///   a connection, for tests and for tools that only need the configuration.
///
/// ```ignore
/// TideConfig::init()
///     .database(&std::env::var("DATABASE_URL")?)
///     .max_connections(20)
///     .auto_models()
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
    #[cfg(feature = "attachments")]
    file_url_generator: Option<FileUrlGenerator>,
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

    /// Register one migration.
    ///
    /// Registering is not running: migrations only execute if
    /// [`run_migrations(true)`](TideConfig::run_migrations) is also set, and
    /// then only from [`connect`](TideConfig::connect). They run in the order
    /// of their versions, whatever order they were registered in, as
    /// [`Migrator::run`](crate::migration::Migrator::run) runs them.
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
    /// [`auto_models`](TideConfig::auto_models) or [`models`](TideConfig::models),
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
    /// `T` is a model type (`models::<User>()`) or a tuple of model types
    /// (`models::<(User, Post)>()`). Registration is what makes a model visible
    /// to schema sync and schema export; use
    /// [`auto_models`](TideConfig::auto_models) to discover all linked models
    /// instead.
    #[must_use]
    pub fn models<T: crate::sync::RegisterModels>(self) -> Self {
        T::register_all();
        self
    }

    /// Discover all linked models automatically, without source-path globs.
    ///
    /// ```
    /// use tideorm::TideConfig;
    /// let config = TideConfig::init().auto_models().sync(true);
    /// ```
    ///
    /// Call `models::<User>()` or `models::<(User, Post)>()` instead when only
    /// a subset should be synced. Discovery itself does not connect or change
    /// database tables.
    #[must_use]
    pub fn auto_models(self) -> Self {
        crate::sync::SyncRegistry::register_all_models();
        self
    }

    /// Register all compiled TideORM models whose source file path matches a glob pattern.
    ///
    /// This matches against the source path captured from each `#[tideorm::model]` invocation,
    /// so matching files still need to be compiled into the crate through normal `mod`
    /// declarations. Supported wildcards are `*` for one path segment and `**` across
    /// directories. A pattern that matches no model logs a warning.
    #[must_use]
    pub fn models_matching(self, pattern: &str) -> Self {
        if crate::sync::SyncRegistry::register_models_matching(pattern) == 0 {
            tide_warn!(
                "models_matching({pattern:?}) matched no compiled model; a model's file must be reachable through `mod` declarations to be compiled in"
            );
        }
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
    pub fn schema_file(mut self, path: impl Into<String>) -> Self {
        self.schema_file = Some(path.into());
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
    pub fn database(mut self, url: impl Into<String>) -> Self {
        self.database_url = Some(url.into());
        self
    }

    /// Set the languages available to translated fields. Defaults to `["en"]`.
    #[must_use]
    pub fn languages(mut self, langs: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.config.languages = langs.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    /// Set the language used when a translation is missing. Defaults to `"en"`.
    #[must_use]
    pub fn fallback_language(mut self, lang: impl Into<String>) -> Self {
        self.config.fallback_language = lang.into();
        self
    }

    /// Hide these attributes from `to_json()` for every model.
    ///
    /// Applied on top of each model's own hidden attributes, which is the place
    /// to put process-wide secrets such as `password_digest`. This only affects
    /// TideORM's JSON rendering — a direct `serde_json::to_value` still sees the
    /// fields.
    #[must_use]
    pub fn hidden_attributes(mut self, attrs: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.config.hidden_attributes = attrs.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    /// Prefix attachment URLs with this base for every field.
    ///
    /// Overridden per field by [`file_base_url_for`](TideConfig::file_base_url_for).
    /// With no base set, the raw storage key is used as the URL.
    #[must_use]
    pub fn file_base_url(mut self, url: impl Into<String>) -> Self {
        self.config.file_base_url = Some(url.into());
        self
    }

    /// Prefix one attachment field's URLs with its own base.
    ///
    /// Takes precedence over [`file_base_url`](TideConfig::file_base_url), which
    /// is how a single field can be served from a different CDN or bucket.
    #[must_use]
    pub fn file_base_url_for(
        mut self,
        field_name: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        self.config
            .file_field_base_urls
            .insert(field_name.into(), url.into());
        self
    }

    /// Replace URL generation for attachments entirely.
    ///
    /// The generator is called with the field name and the file for every
    /// attachment rendered by `to_json()` or [`FileAttachment::url`], which is
    /// where signed or expiring URLs belong; it may capture what it needs. It
    /// supersedes the base-URL settings unless it consults them itself.
    ///
    /// Like the other settings it is installed by [`connect`](TideConfig::connect)
    /// or [`apply`](TideConfig::apply), and one of them without a generator
    /// puts the default back.
    ///
    /// [`FileAttachment::url`]: crate::attachments::FileAttachment::url
    #[cfg(feature = "attachments")]
    #[must_use]
    pub fn file_url_generator(
        mut self,
        generator: impl Fn(&str, &crate::attachments::FileAttachment) -> String + Send + Sync + 'static,
    ) -> Self {
        self.file_url_generator = Some(std::sync::Arc::new(generator));
        self
    }

    /// Set the master key for tokenization and `#[tideorm(encrypted)]` fields.
    ///
    /// Per-column keys are derived from it, so changing this key makes every
    /// previously written ciphertext and token undecryptable. Load it from the
    /// environment or a secret manager, never from source.
    #[must_use]
    pub fn encryption_key(mut self, key: impl Into<String>) -> Self {
        self.encryption_key = Some(key.into());
        self
    }

    /// Replace how tokens are encoded, with a function of the serialized
    /// primary key and the model name; it may capture what it needs.
    ///
    /// Pair it with a matching [`token_decoder`](TideConfig::token_decoder);
    /// setting only one of the two produces tokens that cannot be read back.
    #[must_use]
    pub fn token_encoder(
        mut self,
        encoder: impl Fn(&str, &str) -> Result<String> + Send + Sync + 'static,
    ) -> Self {
        self.token_encoder = Some(std::sync::Arc::new(encoder));
        self
    }

    /// Replace how tokens are decoded, with a function of the token and the
    /// model name.
    ///
    /// Must accept whatever [`token_encoder`](TideConfig::token_encoder)
    /// produces. A token it rejects surfaces as `Ok(None)`, not an error, so a
    /// mismatched pair fails silently.
    #[must_use]
    pub fn token_decoder(
        mut self,
        decoder: impl Fn(&str, &str) -> Result<Option<String>> + Send + Sync + 'static,
    ) -> Self {
        self.token_decoder = Some(std::sync::Arc::new(decoder));
        self
    }

    /// Install the settings that live outside [`Config`]: the tokenization
    /// settings collected by the builder, into `TokenConfig`, and the
    /// attachment URL generator.
    ///
    /// Shared by `connect` and `apply` so that both entry points honor them.
    fn install_hooks(&mut self) {
        if let Some(key) = self.encryption_key.take() {
            crate::tokenization::TokenConfig::set_encryption_key(&key);
        }
        crate::tokenization::TokenConfig::install_codecs(
            self.token_encoder.take(),
            self.token_decoder.take(),
        );
        #[cfg(feature = "attachments")]
        super::state::set_global_file_url_generator(self.file_url_generator.take());
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
    pub async fn connect(mut self) -> Result<&'static Database> {
        self.install_hooks();
        with_global_config_mut(|c| *c = self.config);

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

        let db_type = Self::resolve_database_type(db_type, &db)?;
        set_global_db_type(Some(db_type));

        let db_ref = Database::set_global(db)?;

        if self.run_migrations && !self.migrations.is_empty() {
            let mut migrator = crate::migration::Migrator::new().database(db_ref);
            for migration in self.migrations {
                migrator = migrator.add_boxed(migration);
            }
            let result = migrator.run().await?;
            if result.has_applied() {
                tide_info!("{}", result);
            }
        }

        if self.sync_enabled {
            let mode = if self.force_sync {
                crate::sync::SyncMode::Recreate
            } else {
                crate::sync::SyncMode::Additive
            };
            crate::sync::sync_database_with(db_ref, mode).await?;
        }

        if self.run_seeds && !self.seeds.is_empty() {
            let mut seeder = crate::seeding::Seeder::new().database(db_ref);
            for seed in self.seeds {
                seeder = seeder.add_boxed(seed);
            }
            let result = match seeder.run().await {
                Ok(result) => result,
                Err(failure) => {
                    tide_warn!(
                        "Database seeding failed after initialization steps were already applied. The database may be partially initialized: migrations and schema sync may have run, but seed data is missing."
                    );
                    if failure.result.has_executed() {
                        tide_info!("{}", failure.result);
                    }
                    return Err(failure.error);
                }
            };
            if result.has_executed() {
                tide_info!("{}", result);
            }
        }

        set_global_schema_file_path(self.schema_file.clone());
        if let Some(path) = &self.schema_file {
            crate::schema::SchemaWriter::write_schema_for(db_ref, path).await?;
        }

        Ok(db_ref)
    }

    /// Connect as [`connect`](TideConfig::connect) does, without the startup
    /// work: no migrations, schema sync, seeds or schema-file write, whatever
    /// the builder enabled.
    ///
    /// For a tool that shares an application's configuration but does its own
    /// work on the database, such as the migration and seed runners the CLI
    /// scaffolds: a `force_sync` would drop the tables they are about to
    /// migrate or seed.
    pub async fn connect_only(mut self) -> Result<&'static Database> {
        self.run_migrations = false;
        self.sync_enabled = false;
        self.run_seeds = false;
        self.schema_file = None;
        self.connect().await
    }

    /// Install the settings globally without opening a connection.
    ///
    /// The offline half of [`connect`](TideConfig::connect): languages, hidden
    /// attributes, declared backend, schema-file path, tokenization keys and
    /// codecs, and the attachment URL generator all take effect. No pool is
    /// created, so the URL and pool settings go unused, and no migrations,
    /// seeds, sync, or schema-file write run. Use it in tests and in tools that
    /// need the configuration but not the database.
    pub fn apply(mut self) {
        self.install_hooks();

        with_global_config_mut(|c| *c = self.config);

        set_global_db_type(self.database_type);

        set_global_schema_file_path(self.schema_file);
    }

    /// Restore the global configuration to its defaults, everything
    /// [`connect`](TideConfig::connect) and [`apply`](TideConfig::apply)
    /// install: the tokenization key and codecs too, as
    /// [`TokenConfig::reset`](crate::tokenization::TokenConfig::reset) clears
    /// them.
    ///
    /// Intended for tests, where configuration set by one test would otherwise
    /// leak into the next. It clears configuration only — the global database
    /// connection is separate, and `Database::reset_global()` clears that.
    pub fn reset() {
        with_global_config_mut(|c| *c = Config::default());

        set_global_db_type(None);

        set_global_schema_file_path(None);

        crate::tokenization::TokenConfig::reset();

        #[cfg(feature = "attachments")]
        super::state::set_global_file_url_generator(None);
    }

    /// Return whether a global database connection has been installed.
    #[must_use]
    pub fn is_connected() -> bool {
        crate::database::has_global_db()
    }

    /// Return the configured backend, if one is known: the one
    /// [`connect`](TideConfig::connect) settled on (MariaDB when a `mysql://`
    /// URL reached one), or the one [`apply`](TideConfig::apply) declared.
    /// A database handle's own [`Database::backend`] is the answer for the
    /// statements it runs.
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
    /// A `mysql://` URL can reach MariaDB, so a MySQL connection is not
    /// assumed to be MySQL: the pool reads the server from each connection's
    /// handshake, and took one when it opened unless it connects lazily.
    pub(super) fn resolve_database_type(
        declared: DatabaseType,
        db: &Database,
    ) -> Result<DatabaseType> {
        // The pool asked the server when it opened.
        if declared == DatabaseType::MySQL && db.try_backend()? == DatabaseType::MariaDB {
            tide_info!("Auto-detected a MariaDB server");
            return Ok(DatabaseType::MariaDB);
        }
        Ok(declared)
    }
}
