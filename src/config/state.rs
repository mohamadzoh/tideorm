use parking_lot::RwLock;
use std::cell::{Cell, RefCell};
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

thread_local! {
    static LOCAL_CONFIG: RefCell<Option<Config>> = const { RefCell::new(None) };
    static LOCAL_DB_TYPE: Cell<Option<DatabaseType>> = const { Cell::new(None) };
    static LOCAL_POOL_CONFIG: RefCell<Option<PoolConfig>> = const { RefCell::new(None) };
    static LOCAL_SCHEMA_FILE_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
    #[cfg(feature = "attachments")]
    static LOCAL_FILE_URL_GENERATOR: Cell<Option<FileUrlGenerator>> = const { Cell::new(None) };
}

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

pub(super) fn local_config() -> Option<Config> {
    LOCAL_CONFIG.with(|slot| slot.borrow().clone())
}

pub(super) fn set_local_config(value: Option<Config>) {
    LOCAL_CONFIG.with(|slot| *slot.borrow_mut() = value);
}

pub(super) fn local_db_type() -> Option<DatabaseType> {
    LOCAL_DB_TYPE.with(|slot| slot.get())
}

pub(super) fn set_local_db_type(value: Option<DatabaseType>) {
    LOCAL_DB_TYPE.with(|slot| slot.set(value));
}

pub(super) fn local_pool_config() -> Option<PoolConfig> {
    LOCAL_POOL_CONFIG.with(|slot| slot.borrow().clone())
}

pub(super) fn set_local_pool_config(value: Option<PoolConfig>) {
    LOCAL_POOL_CONFIG.with(|slot| *slot.borrow_mut() = value);
}

pub(super) fn local_schema_file_path() -> Option<String> {
    LOCAL_SCHEMA_FILE_PATH.with(|slot| slot.borrow().clone())
}

pub(super) fn set_local_schema_file_path(value: Option<String>) {
    LOCAL_SCHEMA_FILE_PATH.with(|slot| *slot.borrow_mut() = value);
}

#[cfg(feature = "attachments")]
pub(super) fn local_file_url_generator() -> Option<FileUrlGenerator> {
    LOCAL_FILE_URL_GENERATOR.with(|slot| slot.get())
}

#[cfg(feature = "attachments")]
pub(super) fn set_local_file_url_generator(value: Option<FileUrlGenerator>) {
    LOCAL_FILE_URL_GENERATOR.with(|slot| slot.set(value));
}
