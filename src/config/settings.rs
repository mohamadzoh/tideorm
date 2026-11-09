//! The settings [`TideConfig`](super::TideConfig) installs, and the readers for them.

use std::collections::HashMap;
#[cfg(feature = "attachments")]
use std::sync::Arc;
use std::time::Duration;

use super::state::with_global_config;

/// Builds the public URL for one attachment from the field name and the file.
///
/// Install one with `TideConfig::file_url_generator` to serve signed or
/// expiring URLs; it may capture what it needs, such as a signing key read at
/// startup. The default joins the configured base URL with the file's storage
/// key.
#[cfg(feature = "attachments")]
pub type FileUrlGenerator =
    Arc<dyn Fn(&str, &crate::attachments::FileAttachment) -> String + Send + Sync>;

/// The process-wide behaviour settings, as installed by [`TideConfig`](super::TideConfig).
///
/// Build one through `TideConfig` rather than constructing it directly — that
/// is the only path that installs it globally. The associated functions here
/// (`get_languages`, `get_hidden_attributes`, ...) read whatever is currently
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
            file_base_url: None,
            file_field_base_urls: HashMap::new(),
        }
    }
}

impl Config {
    /// Take a snapshot of the currently installed configuration.
    ///
    /// A clone, so it does not track later changes.
    #[must_use]
    pub fn global() -> Config {
        with_global_config(Clone::clone)
    }

    /// Read the configured languages.
    #[must_use]
    pub fn get_languages() -> Vec<String> {
        with_global_config(|c| c.languages.clone())
    }

    /// Read the configured fallback language.
    #[must_use]
    pub fn get_fallback_language() -> String {
        with_global_config(|c| c.fallback_language.clone())
    }

    /// Read the globally hidden attributes.
    ///
    /// These are added to each model's own hidden attributes when `to_json()`
    /// renders it; they do not replace them.
    #[must_use]
    pub fn get_hidden_attributes() -> Vec<String> {
        with_global_config(|c| c.hidden_attributes.clone())
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
        with_global_config(|c| c.resolve_file_base_url(field_name).map(str::to_string))
    }

    /// Read the installed attachment URL generator, or the default one.
    #[cfg(feature = "attachments")]
    #[must_use]
    pub fn get_file_url_generator() -> FileUrlGenerator {
        super::state::global_file_url_generator()
            .unwrap_or_else(|| Arc::new(Self::default_file_url_generator))
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
}

/// Connection-pool limits and timeouts.
///
/// Set these through the matching `TideConfig` methods; this struct is what
/// they accumulate into, and what `TideConfig::connect()` opens the pool with.
/// To share one set of limits, build it from [`PoolConfig::default`] with the
/// setters of the same names and hand it to `TideConfig::pool`.
///
/// Marked `#[non_exhaustive]`, so a setting added later is not a breaking
/// change; the fields stay public to read and assign.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PoolConfig {
    /// Ceiling on open connections for the whole process. Defaults to 10.
    pub max_connections: u32,
    /// Connections kept open even while idle. Defaults to 1.
    pub min_connections: u32,
    /// The bound on each attempt to open a connection. Defaults to 8 seconds;
    /// see [`connect_timeout`](Self::connect_timeout()).
    pub connect_timeout: Duration,
    /// How long a connection may stay unused. Defaults to 10 minutes.
    pub idle_timeout: Duration,
    /// How old a connection may get, however busy. Defaults to 30 minutes.
    pub max_lifetime: Duration,
    /// The bound on a whole checkout. Defaults to 8 seconds; see
    /// [`acquire_timeout`](Self::acquire_timeout()).
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

impl PoolConfig {
    /// Cap the open connections for the whole process (10 by default).
    #[must_use]
    pub fn max_connections(mut self, connections: u32) -> Self {
        self.max_connections = connections;
        self
    }

    /// Keep `connections` open even while idle (1 by default).
    #[must_use]
    pub fn min_connections(mut self, connections: u32) -> Self {
        self.min_connections = connections;
        self
    }

    /// Bound each attempt to open a connection, TLS, authentication and
    /// connection hooks included (8 seconds by default). A checkout still ends at
    /// [`acquire_timeout`](Self::acquire_timeout) if that comes first.
    #[must_use]
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Close a connection left unused this long (10 minutes by default).
    #[must_use]
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Close a connection this old, however busy (30 minutes by default). Keep it under any idle
    /// or lifetime limit the server, or a proxy in front of it, enforces.
    #[must_use]
    pub fn max_lifetime(mut self, lifetime: Duration) -> Self {
        self.max_lifetime = lifetime;
        self
    }

    /// Bound a whole checkout: waiting for a free connection, its ping and
    /// hooks, and opening a new one (8 seconds by default). Each attempt to open one is also
    /// bounded by [`connect_timeout`](Self::connect_timeout).
    #[must_use]
    pub fn acquire_timeout(mut self, timeout: Duration) -> Self {
        self.acquire_timeout = timeout;
        self
    }
}
