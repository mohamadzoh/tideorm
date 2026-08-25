//! The settings [`TideConfig`](super::TideConfig) installs, and the readers for them.

use std::collections::HashMap;
use std::time::Duration;

use super::state::with_global_config;

/// Builds the public URL for one attachment.
///
/// Install one with `TideConfig::file_url_generator` to serve signed or
/// expiring URLs; the default joins the configured base URL with the file's
/// storage key.
#[cfg(feature = "attachments")]
pub type FileUrlGenerator =
    fn(field_name: &str, file: &crate::attachments::FileAttachment) -> String;

/// The process-wide behaviour settings, as installed by [`TideConfig`](super::TideConfig).
///
/// Build one through `TideConfig` rather than constructing it directly — that
/// is the only path that installs it globally. The associated functions here
/// (`get_languages`, `is_soft_delete_default`, ...) read whatever is currently
/// installed and are what TideORM itself calls at runtime.
#[derive(Debug, Clone)]
pub struct Config {
    /// Languages available to translated fields. Defaults to `["en"]`.
    pub languages: Vec<String>,
    /// Language used when a translation is missing. Defaults to `"en"`.
    pub fallback_language: String,
    /// Attributes omitted from every model's `to_json()`, on top of each
    /// model's own hidden list.
    pub hidden_attributes: Vec<String>,
    /// Whether models soft-delete unless they say otherwise. Defaults to `false`.
    pub soft_delete_by_default: bool,
    /// Base URL prepended to attachment keys when no per-field base applies.
    pub file_base_url: Option<String>,
    /// Per-field base URLs, which take precedence over `file_base_url`.
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
    /// Build a `Config` holding TideORM's defaults.
    ///
    /// Creating one does not install it; `TideConfig::apply()` or
    /// `TideConfig::connect()` does that.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take a snapshot of the currently installed configuration.
    ///
    /// A clone, so it does not track later changes.
    #[must_use]
    pub fn global() -> Config {
        with_global_config(Clone::clone)
    }

    #[inline]
    fn with_global<T>(f: impl FnOnce(&Config) -> T) -> T {
        with_global_config(f)
    }

    /// Read the configured languages.
    #[must_use]
    pub fn get_languages() -> Vec<String> {
        Self::with_global(|c| c.languages.clone())
    }

    /// Read the configured fallback language.
    #[must_use]
    pub fn get_fallback_language() -> String {
        Self::with_global(|c| c.fallback_language.clone())
    }

    /// Read the globally hidden attributes.
    ///
    /// These are added to each model's own hidden attributes when `to_json()`
    /// renders it; they do not replace them.
    #[must_use]
    pub fn get_hidden_attributes() -> Vec<String> {
        Self::with_global(|c| c.hidden_attributes.clone())
    }

    /// Read whether models soft-delete by default.
    #[must_use]
    pub fn is_soft_delete_default() -> bool {
        Self::with_global(|c| c.soft_delete_by_default)
    }

    /// Read the global attachment base URL.
    ///
    /// This ignores per-field overrides; use
    /// [`get_file_base_url_for`](Config::get_file_base_url_for) to resolve the
    /// base that actually applies to a field.
    #[must_use]
    pub fn get_file_base_url() -> Option<String> {
        Self::with_global(|c| c.file_base_url.clone())
    }

    pub(crate) fn resolve_file_base_url(&self, field_name: &str) -> Option<&str> {
        self.file_field_base_urls
            .get(field_name)
            .map(String::as_str)
            .or(self.file_base_url.as_deref())
    }

    /// Resolve the base URL that applies to one attachment field.
    ///
    /// The field's own base wins; otherwise the global one is used, and `None`
    /// means URLs fall back to the bare storage key.
    #[must_use]
    pub fn get_file_base_url_for(field_name: &str) -> Option<String> {
        Self::with_global(|c| c.resolve_file_base_url(field_name).map(str::to_string))
    }

    /// Read the installed attachment URL generator, or the default one.
    #[cfg(feature = "attachments")]
    #[must_use]
    pub fn get_file_url_generator() -> FileUrlGenerator {
        super::state::global_file_url_generator().unwrap_or(Self::default_file_url_generator)
    }

    /// Install the attachment URL generator process-wide.
    ///
    /// Takes effect immediately for every model. `TideConfig::file_url_generator`
    /// is the builder-style spelling of this.
    #[cfg(feature = "attachments")]
    pub fn set_file_url_generator(generator: FileUrlGenerator) {
        super::state::set_global_file_url_generator(Some(generator));
    }

    /// The URL generator used when none was installed.
    ///
    /// Joins the base URL that applies to `field_name` with the file's storage
    /// key, tolerating a trailing or leading slash on either side, and returns
    /// the bare key when no base URL is configured.
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

    /// Build the public URL for one attachment through the installed generator.
    ///
    /// This is what `to_json()` calls; reach for it directly when rendering an
    /// attachment outside of TideORM's own serialization.
    #[inline]
    #[cfg(feature = "attachments")]
    pub fn generate_file_url(
        field_name: &str,
        file: &crate::attachments::FileAttachment,
    ) -> String {
        Self::get_file_url_generator()(field_name, file)
    }
}

/// Connection-pool limits and timeouts.
///
/// Set these through the matching `TideConfig` methods; this struct is what
/// they accumulate into and what `TideConfig::pool_config()` hands back.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Ceiling on open connections for the whole process. Defaults to 10.
    pub max_connections: u32,
    /// Connections kept open even while idle. Defaults to 1.
    pub min_connections: u32,
    /// Time allowed to open a new connection. Defaults to 8 seconds.
    pub connect_timeout: Duration,
    /// Time an unused connection may stay in the pool. Defaults to 10 minutes.
    pub idle_timeout: Duration,
    /// Age at which a connection is retired regardless of use. Defaults to 30 minutes.
    pub max_lifetime: Duration,
    /// Time a query may wait for a free connection. Defaults to 8 seconds; this
    /// is the timeout that fires when the pool is saturated.
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
