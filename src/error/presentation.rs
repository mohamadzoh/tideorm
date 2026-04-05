use super::Error;

impl Error {
    /// Return a user-facing next step derived from the variant and message.
    pub fn suggestion(&self) -> String {
        match self {
            Self::NotFound { message, .. } => {
                if message.contains("find") || message.contains("Find") {
                    "Check that the ID exists. Use `Model::exists(id).await?` to verify before find.".to_string()
                } else {
                    "Verify the record exists and hasn't been soft-deleted. Use `.with_trashed()` to include deleted records.".to_string()
                }
            }
            Self::Connection { message } => {
                if message.contains("refused") || message.contains("Refused") {
                    "Database server is not running or not accepting connections. Check that:\n\
                     1. The database server is running\n\
                     2. The host and port are correct\n\
                     3. Firewall allows the connection".to_string()
                } else if message.contains("password") || message.contains("authentication") {
                    "Check your database credentials in the connection URL.".to_string()
                } else if message.contains("does not exist") || message.contains("unknown database") {
                    "The database doesn't exist. Create it first: CREATE DATABASE dbname;".to_string()
                } else if message.contains("timeout") || message.contains("Timeout") {
                    "Connection timed out. Check network connectivity and increase `connect_timeout` if needed.".to_string()
                } else if message.contains("pool") || message.contains("Pool") {
                    "Connection pool exhausted. Consider:\n\
                     1. Increasing `max_connections` in TideConfig\n\
                     2. Reducing connection hold time\n\
                     3. Using `acquire_timeout` to wait for connections".to_string()
                } else {
                    "Verify your database URL format: postgres://user:pass@host:5432/database".to_string()
                }
            }
            Self::Query { message, context } => {
                let base_suggestion = if message.contains("syntax") || message.contains("Syntax") {
                    "SQL syntax error. Check column names and query structure."
                } else if message.contains("duplicate") || message.contains("unique") {
                    "Duplicate key violation. The value already exists in a unique column."
                } else if message.contains("foreign key") || message.contains("violates foreign key") {
                    "Foreign key constraint violation. The referenced record doesn't exist or can't be deleted."
                } else if message.contains("null") || message.contains("NOT NULL") {
                    "NULL value not allowed. Ensure all required fields are provided."
                } else if message.contains("column") && message.contains("does not exist") {
                    "Column doesn't exist. Check spelling and run migrations if needed."
                } else if message.contains("table") && message.contains("does not exist") {
                    "Table doesn't exist. Run migrations: `TideConfig::init().run_migrations(true).connect().await?`"
                } else if message.contains("permission") || message.contains("denied") {
                    "Permission denied. Check database user privileges."
                } else if message.contains("deadlock") {
                    "Deadlock detected. Retry the transaction or review query ordering."
                } else {
                    "Check the SQL query and ensure all referenced columns/tables exist."
                };

                if let Some(ctx) = context {
                    if let Some(ref query) = ctx.query {
                        format!("{}\n\nQuery: {}", base_suggestion, query)
                    } else {
                        base_suggestion.to_string()
                    }
                } else {
                    base_suggestion.to_string()
                }
            }
            Self::Validation { field, message: _ } => {
                format!("Validate the '{}' field before saving. Use Model::validate() for custom validation.", field)
            }
            Self::Conversion { message } => {
                if message.contains("type") {
                    "Type mismatch. Check that Rust types match database column types.".to_string()
                } else {
                    "Data conversion failed. Verify the data format matches expected type.".to_string()
                }
            }
            Self::Transaction { message } => {
                if message.contains("timeout") {
                    "Transaction timed out. Split into smaller transactions or increase timeout.".to_string()
                } else if message.contains("rollback") || message.contains("aborted") {
                    "Transaction was rolled back. Check for errors in transaction body.".to_string()
                } else {
                    "Transaction failed. Ensure all operations in the transaction are valid.".to_string()
                }
            }
            Self::Configuration { message } => {
                if message.contains("initialized") || message.contains("not set") {
                    "Database not initialized. Call `TideConfig::init().database(url).connect().await?` first.".to_string()
                } else if message.contains("already") {
                    "Configuration already set. TideConfig::init() should only be called once.".to_string()
                } else {
                    format!("Check your TideConfig settings: {}", message)
                }
            }
            Self::Internal { .. } => {
                "Internal error. Please report this issue at https://github.com/mohamadzoh/tideorm/issues".to_string()
            }
            Self::BackendNotSupported { backend, message } => {
                format!(
                    "Operation not supported on {} backend. {}\n\
                     Consider using a database-agnostic approach or checking backend with `db.backend()`.",
                    backend, message
                )
            }
            Self::PrimaryKeyNotSet { model, .. } => {
                format!(
                    "Set the primary key on your {} instance before this operation.\n\
                     Use `Model::find(id)` to load an existing record, or ensure auto-increment is configured.",
                    model
                )
            }
            Self::InsertReturningNotSupported { backend, .. } => {
                format!(
                    "{} does not support INSERT ... RETURNING syntax.\n\
                     Options:\n\
                     1. Use separate insert() and find() calls\n\
                     2. For MySQL, use last_insert_id() after insert\n\
                     3. Consider using PostgreSQL which supports RETURNING",
                    backend
                )
            }
            Self::Tokenization { message } => {
                format!(
                    "Tokenization failed: {}\n\
                     Ensure:\n\
                     1. An encryption key is configured via TideConfig::encryption_key()\n\
                     2. The model has tokenization enabled via #[tideorm(tokenize)]\n\
                     3. The record has a valid primary key",
                    message
                )
            }
            Self::InvalidToken { message } => {
                format!(
                    "Invalid token: {}\n\
                     Possible causes:\n\
                     1. Token was tampered with or corrupted\n\
                     2. Token is for a different model type\n\
                     3. Encryption key has changed since token was created",
                    message
                )
            }
        }
    }

    /// Return a stable error code for logs, metrics, or API responses.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "TIDE_NOT_FOUND",
            Self::Connection { .. } => "TIDE_CONNECTION",
            Self::Query { .. } => "TIDE_QUERY",
            Self::Validation { .. } => "TIDE_VALIDATION",
            Self::Conversion { .. } => "TIDE_CONVERSION",
            Self::Transaction { .. } => "TIDE_TRANSACTION",
            Self::Configuration { .. } => "TIDE_CONFIG",
            Self::Internal { .. } => "TIDE_INTERNAL",
            Self::BackendNotSupported { .. } => "TIDE_BACKEND_NOT_SUPPORTED",
            Self::PrimaryKeyNotSet { .. } => "TIDE_PRIMARY_KEY_NOT_SET",
            Self::InsertReturningNotSupported { .. } => "TIDE_INSERT_RETURNING_NOT_SUPPORTED",
            Self::Tokenization { .. } => "TIDE_TOKENIZATION",
            Self::InvalidToken { .. } => "TIDE_INVALID_TOKEN",
        }
    }

    /// Map the error to a generic HTTP status code.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotFound { .. } => 404,
            Self::Connection { .. } => 503,
            Self::Query { .. } => 400,
            Self::Validation { .. } => 422,
            Self::Conversion { .. } => 400,
            Self::Transaction { .. } => 409,
            Self::Configuration { .. } => 500,
            Self::Internal { .. } => 500,
            Self::BackendNotSupported { .. } => 501,
            Self::PrimaryKeyNotSet { .. } => 400,
            Self::InsertReturningNotSupported { .. } => 501,
            Self::Tokenization { .. } => 400,
            Self::InvalidToken { .. } => 401,
        }
    }

    /// Best-effort retry hint based on transient-looking error messages.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Connection { message } => {
                message.contains("timeout")
                    || message.contains("pool")
                    || message.contains("refused")
            }
            Self::Query { message, .. } => {
                message.contains("deadlock")
                    || message.contains("lock")
                    || message.contains("timeout")
            }
            Self::Transaction { message } => {
                message.contains("deadlock")
                    || message.contains("timeout")
                    || message.contains("serialization")
            }
            _ => false,
        }
    }

    /// Render the error with its code, context, and suggestion for logs.
    pub fn log_format(&self) -> String {
        let mut output = format!("[{}] {}", self.code(), self);

        if let Some(ctx) = self.context() {
            if let Some(ref table) = ctx.table {
                output.push_str(&format!("\n  Table: {}", table));
            }
            if let Some(ref column) = ctx.column {
                output.push_str(&format!("\n  Column: {}", column));
            }
            if !ctx.conditions.is_empty() {
                output.push_str(&format!("\n  Conditions: {}", ctx.conditions.join(" | ")));
            }
            if let Some(ref operator_chain) = ctx.operator_chain {
                output.push_str(&format!("\n  Operator chain: {}", operator_chain));
            }
            if let Some(ref query) = ctx.query {
                output.push_str(&format!("\n  Query: {}", query));
            }
        }

        output.push_str(&format!("\n  Suggestion: {}", self.suggestion()));
        output
    }
}
