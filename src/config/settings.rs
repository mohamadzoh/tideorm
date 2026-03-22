use std::collections::HashMap;
use std::time::Duration;

use super::state::{global_config_state, local_config};

#[cfg(feature = "attachments")]
pub type FileUrlGenerator =
    fn(field_name: &str, file: &crate::attachments::FileAttachment) -> String;

#[derive(Debug, Clone)]
pub struct Config {
    pub languages: Vec<String>,
    pub fallback_language: String,
    pub hidden_attributes: Vec<String>,
    pub soft_delete_by_default: bool,
    pub file_base_url: Option<String>,
    pub file_field_base_urls: HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            languages: vec!["en".to_string()],
            fallback_language: "en".to_string(),
            hidden_attributes: vec![],
            soft_delete_by_default: false,
            file_base_url: None,
            file_field_base_urls: HashMap::new(),
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn global() -> Config {
        if let Some(config) = local_config() {
            return config;
        }

        global_config_state().read().clone()
    }

    #[inline]
    fn with_global<T>(f: impl FnOnce(&Config) -> T) -> T {
        if let Some(value) = local_config() {
            return f(&value);
        }

        let guard = global_config_state().read();
        f(&guard)
    }

    pub fn get_languages() -> Vec<String> {
        Self::with_global(|c| c.languages.clone())
    }

    pub fn get_fallback_language() -> String {
        Self::with_global(|c| c.fallback_language.clone())
    }

    pub fn get_hidden_attributes() -> Vec<String> {
        Self::with_global(|c| c.hidden_attributes.clone())
    }

    pub fn is_soft_delete_default() -> bool {
        Self::with_global(|c| c.soft_delete_by_default)
    }

    pub fn get_file_base_url() -> Option<String> {
        Self::with_global(|c| c.file_base_url.clone())
    }

    pub(crate) fn resolve_file_base_url(&self, field_name: &str) -> Option<&str> {
        self.file_field_base_urls
            .get(field_name)
            .map(String::as_str)
            .or(self.file_base_url.as_deref())
    }

    pub fn get_file_base_url_for(field_name: &str) -> Option<String> {
        Self::with_global(|c| c.resolve_file_base_url(field_name).map(str::to_string))
    }

    #[cfg(feature = "attachments")]
    pub fn get_file_url_generator() -> FileUrlGenerator {
        super::state::local_file_url_generator()
            .or_else(|| *super::state::global_file_url_generator_state().read())
            .unwrap_or(Self::default_file_url_generator)
    }

    #[cfg(feature = "attachments")]
    pub fn set_file_url_generator(generator: FileUrlGenerator) {
        *super::state::global_file_url_generator_state().write() = Some(generator);
        super::state::set_local_file_url_generator(Some(generator));
    }

    #[inline]
    #[cfg(feature = "attachments")]
    pub fn default_file_url_generator(
        field_name: &str,
        file: &crate::attachments::FileAttachment,
    ) -> String {
        if let Some(base_url) = Self::get_file_base_url_for(field_name) {
            let base = base_url.trim_end_matches('/');
            let key = file.key.trim_start_matches('/');
            format!("{}/{}", base, key)
        } else {
            file.key.clone()
        }
    }

    #[inline]
    #[cfg(feature = "attachments")]
    pub fn generate_file_url(
        field_name: &str,
        file: &crate::attachments::FileAttachment,
    ) -> String {
        Self::get_file_url_generator()(field_name, file)
    }
}

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
    pub acquire_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            connect_timeout: Duration::from_secs(8),
            idle_timeout: Duration::from_secs(600),
            max_lifetime: Duration::from_secs(1800),
            acquire_timeout: Duration::from_secs(8),
        }
    }
}
