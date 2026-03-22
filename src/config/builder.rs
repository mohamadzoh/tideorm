use std::time::Duration;

#[cfg(feature = "attachments")]
use super::FileUrlGenerator;
use super::database::rewrite_driver_url;
use super::state::{
    global_config_state, global_db_type_state, global_pool_config_state,
    global_schema_file_path_state, local_db_type, local_pool_config, local_schema_file_path,
    set_local_config, set_local_db_type, set_local_pool_config, set_local_schema_file_path,
};
use super::{Config, DatabaseType, PoolConfig, RegisterMigrations, RegisterSeeds};

use crate::database::Database;
use crate::error::Result;
use crate::migration::Migration;
use crate::tide_info;

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

    pub fn migration<M: Migration + 'static>(mut self, migration: M) -> Self {
        self.migrations.push(Box::new(migration));
        self
    }

    pub fn migrations<T: RegisterMigrations>(mut self) -> Self {
        self.migrations.extend(T::collect());
        self
    }

    pub fn run_migrations(mut self, enabled: bool) -> Self {
        self.run_migrations = enabled;
        self
    }

    pub fn seed<S: crate::seeding::Seed + 'static>(mut self, seed: S) -> Self {
        self.seeds.push(Box::new(seed));
        self
    }

    pub fn seeds<T: RegisterSeeds>(mut self) -> Self {
        self.seeds.extend(T::collect());
        self
    }

    pub fn run_seeds(mut self, enabled: bool) -> Self {
        self.run_seeds = enabled;
        self
    }

    pub fn sync(mut self, enabled: bool) -> Self {
        self.sync_enabled = enabled;
        self
    }

    pub fn models<T: crate::sync::RegisterModels>(self) -> Self {
        T::register_all();
        self
    }

    pub fn force_sync(mut self, enabled: bool) -> Self {
        self.force_sync = enabled;
        self
    }

    pub fn schema_file(mut self, path: &str) -> Self {
        self.schema_file = Some(path.to_string());
        self
    }

    pub fn database_type(mut self, db_type: DatabaseType) -> Self {
        self.database_type = Some(db_type);
        self
    }

    pub fn database(mut self, url: &str) -> Self {
        self.database_url = Some(url.to_string());
        self
    }

    pub fn max_connections(mut self, n: u32) -> Self {
        self.pool.max_connections = n;
        self
    }

    pub fn min_connections(mut self, n: u32) -> Self {
        self.pool.min_connections = n;
        self
    }

    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.pool.connect_timeout = duration;
        self
    }

    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.pool.idle_timeout = duration;
        self
    }

    pub fn max_lifetime(mut self, duration: Duration) -> Self {
        self.pool.max_lifetime = duration;
        self
    }

    pub fn acquire_timeout(mut self, duration: Duration) -> Self {
        self.pool.acquire_timeout = duration;
        self
    }

    pub fn languages(mut self, langs: &[&str]) -> Self {
        self.config.languages = langs.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn fallback_language(mut self, lang: &str) -> Self {
        self.config.fallback_language = lang.to_string();
        self
    }

    pub fn hidden_attributes(mut self, attrs: &[&str]) -> Self {
        self.config.hidden_attributes = attrs.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn soft_delete_by_default(mut self, enabled: bool) -> Self {
        self.config.soft_delete_by_default = enabled;
        self
    }

    pub fn file_base_url(mut self, url: &str) -> Self {
        self.config.file_base_url = Some(url.to_string());
        self
    }

    pub fn file_base_url_for(mut self, field_name: &str, url: &str) -> Self {
        self.config
            .file_field_base_urls
            .insert(field_name.to_string(), url.to_string());
        self
    }

    #[cfg(feature = "attachments")]
    pub fn file_url_generator(self, generator: FileUrlGenerator) -> Self {
        Config::set_file_url_generator(generator);
        self
    }

    pub fn encryption_key(mut self, key: &str) -> Self {
        self.encryption_key = Some(key.to_string());
        self
    }

    pub fn token_encoder(mut self, encoder: crate::tokenization::TokenEncoder) -> Self {
        self.token_encoder = Some(encoder);
        self
    }

    pub fn token_decoder(mut self, decoder: crate::tokenization::TokenDecoder) -> Self {
        self.token_decoder = Some(decoder);
        self
    }

    pub async fn connect(self) -> Result<&'static Database> {
        *global_config_state().write() = self.config.clone();
        set_local_config(Some(self.config.clone()));

        if let Some(key) = &self.encryption_key {
            crate::tokenization::TokenConfig::set_encryption_key(key);
        }
        if let Some(encoder) = self.token_encoder {
            crate::tokenization::TokenConfig::set_encoder(encoder);
        }
        if let Some(decoder) = self.token_decoder {
            crate::tokenization::TokenConfig::set_decoder(decoder);
        }

        let url = self.database_url.ok_or_else(|| {
            crate::error::Error::configuration(
                "Database URL is required. Use .database(\"postgres://...\") to set it.",
            )
        })?;

        let mut db_type = match self.database_type {
            Some(t) => t,
            None => DatabaseType::from_url(&url).ok_or_else(|| {
                crate::error::Error::configuration(
                    "Could not detect database type from URL. \
                     Use .database_type(DatabaseType::Postgres) to set it explicitly.",
                )
            })?,
        };

        let connect_url = rewrite_driver_url(&url);

        *global_pool_config_state().write() = Some(self.pool.clone());
        set_local_pool_config(Some(self.pool.clone()));

        let db = Database::builder()
            .url(connect_url)
            .max_connections(self.pool.max_connections)
            .min_connections(self.pool.min_connections)
            .connect_timeout(self.pool.connect_timeout)
            .idle_timeout(self.pool.idle_timeout)
            .max_lifetime(self.pool.max_lifetime)
            .build()
            .await?;

        if db_type == DatabaseType::MySQL {
            if let Ok(version) = Self::detect_server_version(&db).await {
                if version.to_lowercase().contains("mariadb") {
                    db_type = DatabaseType::MariaDB;
                    tide_info!("Auto-detected MariaDB server: {}", version);
                }
            }
        }

        *global_db_type_state().write() = Some(db_type);
        set_local_db_type(Some(db_type));

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

        if self.sync_enabled {
            crate::sync::sync_database_with_options(db_ref, self.force_sync).await?;
        }

        if let Some(path) = &self.schema_file {
            *global_schema_file_path_state().write() = Some(path.clone());
            set_local_schema_file_path(Some(path.clone()));
            crate::schema::SchemaWriter::write_schema(path).await?;
        } else {
            *global_schema_file_path_state().write() = None;
            set_local_schema_file_path(None);
        }

        Ok(db_ref)
    }

    pub fn apply(self) {
        *global_config_state().write() = self.config.clone();
        set_local_config(Some(self.config));

        let database_type = self.database_type;
        *global_db_type_state().write() = database_type;
        set_local_db_type(database_type);

        *global_pool_config_state().write() = Some(self.pool.clone());
        set_local_pool_config(Some(self.pool));

        *global_schema_file_path_state().write() = self.schema_file.clone();
        set_local_schema_file_path(self.schema_file);
    }

    pub fn reset() {
        *global_config_state().write() = Config::default();
        set_local_config(None);

        *global_db_type_state().write() = None;
        set_local_db_type(None);

        *global_pool_config_state().write() = None;
        set_local_pool_config(None);

        *global_schema_file_path_state().write() = None;
        set_local_schema_file_path(None);

        #[cfg(feature = "attachments")]
        {
            *super::state::global_file_url_generator_state().write() = None;
            super::state::set_local_file_url_generator(None);
        }
    }

    pub fn db() -> crate::error::Result<Database> {
        crate::database::require_db()
    }

    pub fn try_db() -> Option<Database> {
        crate::database::try_db()
    }

    pub fn is_connected() -> bool {
        crate::database::has_global_db()
    }

    pub fn get_database_type() -> Option<DatabaseType> {
        local_db_type().or_else(|| *global_db_type_state().read())
    }

    pub fn is_postgres() -> bool {
        Self::get_database_type() == Some(DatabaseType::Postgres)
    }

    pub fn is_mysql() -> bool {
        Self::get_database_type() == Some(DatabaseType::MySQL)
    }

    pub fn is_mariadb() -> bool {
        Self::get_database_type() == Some(DatabaseType::MariaDB)
    }

    pub fn is_mysql_compatible() -> bool {
        matches!(
            Self::get_database_type(),
            Some(DatabaseType::MySQL) | Some(DatabaseType::MariaDB)
        )
    }

    pub fn is_sqlite() -> bool {
        Self::get_database_type() == Some(DatabaseType::SQLite)
    }

    pub fn current() -> Config {
        Config::global()
    }

    pub fn pool_config() -> PoolConfig {
        local_pool_config()
            .or_else(|| global_pool_config_state().read().clone())
            .unwrap_or_default()
    }

    pub fn schema_file_path() -> Option<String> {
        local_schema_file_path().or_else(|| global_schema_file_path_state().read().clone())
    }

    pub fn write_schema_with_generator(
        generator: &crate::schema::SchemaGenerator,
    ) -> std::io::Result<()> {
        let Some(path) = Self::schema_file_path() else {
            return Ok(());
        };

        let sql = generator.generate();
        std::fs::write(path, sql)?;
        Ok(())
    }

    pub fn write_schema_sql(sql: &str) -> std::io::Result<()> {
        let Some(path) = Self::schema_file_path() else {
            return Ok(());
        };

        std::fs::write(path, sql)?;
        Ok(())
    }

    async fn detect_server_version(db: &Database) -> Result<String> {
        use crate::internal::{ConnectionTrait, DbBackend, Statement};

        let conn = db.__internal_connection()?;
        let backend = conn.get_database_backend();

        if backend != DbBackend::MySql {
            return Err(crate::error::Error::internal("Not a MySQL-type connection"));
        }

        let stmt = Statement::from_string(backend, "SELECT VERSION() AS version".to_string());
        let result = conn
            .query_one_raw(stmt)
            .await
            .map_err(|e| crate::error::Error::query(e.to_string()))?;

        match result {
            Some(row) => {
                let version: String = row
                    .try_get("", "version")
                    .map_err(|e| crate::error::Error::query(e.to_string()))?;
                Ok(version)
            }
            None => Err(crate::error::Error::query(
                "Could not retrieve server version",
            )),
        }
    }
}
