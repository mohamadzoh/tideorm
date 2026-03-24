use parking_lot::RwLock;
use std::sync::OnceLock;

#[cfg(feature = "attachments")]
use super::FileUrlGenerator;
use super::{Config, DatabaseType, PoolConfig};

static GLOBAL_CONFIG: OnceLock<RwLock<Config>> = OnceLock::new();
static GLOBAL_DB_TYPE: OnceLock<RwLock<Option<DatabaseType>>> = OnceLock::new();
static GLOBAL_POOL_CONFIG: OnceLock<RwLock<Option<PoolConfig>>> = OnceLock::new();
static SCHEMA_FILE_PATH: OnceLock<RwLock<Option<String>>> = OnceLock::new();

#[cfg(feature = "attachments")]
static GLOBAL_FILE_URL_GENERATOR: OnceLock<RwLock<Option<FileUrlGenerator>>> = OnceLock::new();

pub(super) fn global_config_state() -> &'static RwLock<Config> {
    GLOBAL_CONFIG.get_or_init(|| RwLock::new(Config::default()))
}

pub(super) fn global_db_type_state() -> &'static RwLock<Option<DatabaseType>> {
    GLOBAL_DB_TYPE.get_or_init(|| RwLock::new(None))
}

pub(super) fn global_pool_config_state() -> &'static RwLock<Option<PoolConfig>> {
    GLOBAL_POOL_CONFIG.get_or_init(|| RwLock::new(None))
}

pub(super) fn global_schema_file_path_state() -> &'static RwLock<Option<String>> {
    SCHEMA_FILE_PATH.get_or_init(|| RwLock::new(None))
}

#[cfg(feature = "attachments")]
pub(super) fn global_file_url_generator_state() -> &'static RwLock<Option<FileUrlGenerator>> {
    GLOBAL_FILE_URL_GENERATOR.get_or_init(|| RwLock::new(None))
}
