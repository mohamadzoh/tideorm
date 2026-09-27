use parking_lot::RwLock;
use std::sync::OnceLock;

#[cfg(feature = "attachments")]
use super::FileUrlGenerator;
use super::{Config, DatabaseType};

#[derive(Default)]
struct GlobalConfigState {
    config: Config,
    db_type: Option<DatabaseType>,
    schema_file_path: Option<String>,
    #[cfg(feature = "attachments")]
    file_url_generator: Option<FileUrlGenerator>,
}

static GLOBAL_CONFIG_STATE: OnceLock<RwLock<GlobalConfigState>> = OnceLock::new();

fn global_state() -> &'static RwLock<GlobalConfigState> {
    GLOBAL_CONFIG_STATE.get_or_init(|| RwLock::new(GlobalConfigState::default()))
}

pub(super) fn with_global_config<T>(f: impl FnOnce(&Config) -> T) -> T {
    let guard = global_state().read();
    f(&guard.config)
}

pub(super) fn with_global_config_mut(f: impl FnOnce(&mut Config)) {
    let mut guard = global_state().write();
    f(&mut guard.config);
}

pub(super) fn global_db_type() -> Option<DatabaseType> {
    global_state().read().db_type
}

pub(super) fn set_global_db_type(db_type: Option<DatabaseType>) {
    global_state().write().db_type = db_type;
}

/// Record that the global connection reaches MariaDB, unless another backend
/// was configured, so the statements of its transactions, which cannot ask
/// the pool, render for MariaDB too.
pub(crate) fn note_detected_mariadb() {
    let mut state = global_state().write();
    if matches!(state.db_type, None | Some(DatabaseType::MySQL)) {
        state.db_type = Some(DatabaseType::MariaDB);
    }
}

pub(super) fn global_schema_file_path() -> Option<String> {
    global_state().read().schema_file_path.clone()
}

pub(super) fn set_global_schema_file_path(path: Option<String>) {
    global_state().write().schema_file_path = path;
}

#[cfg(feature = "attachments")]
pub(super) fn global_file_url_generator() -> Option<FileUrlGenerator> {
    global_state().read().file_url_generator
}

#[cfg(feature = "attachments")]
pub(super) fn set_global_file_url_generator(generator: Option<FileUrlGenerator>) {
    global_state().write().file_url_generator = generator;
}
