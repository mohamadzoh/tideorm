//! Opt-in and URL of a MySQL-family server's suites. The MySQL and MariaDB
//! ones differ only in the prefix of their variables (`MYSQL_`, `MARIADB_`).

use std::sync::OnceLock;

pub struct Server {
    /// The server's name, for messages.
    pub name: &'static str,
    prefix: &'static str,
    url: OnceLock<String>,
}

impl Server {
    pub const fn new(name: &'static str, prefix: &'static str) -> Self {
        Self {
            name,
            prefix,
            url: OnceLock::new(),
        }
    }

    /// `<PREFIX>_DATABASE_URL`, or a local server's default.
    pub fn database_url(&self) -> &str {
        self.url.get_or_init(|| {
            let _ = dotenvy::dotenv();
            std::env::var(format!("{}_DATABASE_URL", self.prefix))
                .unwrap_or_else(|_| "mysql://root:@localhost:3306/test_tide_orm".to_string())
        })
    }

    /// Opt-in via `RUN_<PREFIX>_TESTS` or `<PREFIX>_DATABASE_URL`;
    /// `SKIP_<PREFIX>_TESTS` wins over both, so a URL kept in `.env` can be
    /// muted.
    pub fn enabled(&self) -> bool {
        let _ = dotenvy::dotenv();
        let set = |name: String| std::env::var_os(name).is_some();
        !set(format!("SKIP_{}_TESTS", self.prefix))
            && (set(format!("RUN_{}_TESTS", self.prefix))
                || set(format!("{}_DATABASE_URL", self.prefix)))
    }

    /// The line a suite prints when it is not enabled.
    pub fn skipped(&self, suite: &str) -> String {
        let prefix = self.prefix;
        format!(
            "Skipping {} {suite}: set RUN_{prefix}_TESTS or {prefix}_DATABASE_URL \
             (SKIP_{prefix}_TESTS overrides both)",
            self.name
        )
    }
}
