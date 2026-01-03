//! Error types for TideORM
//!
//! This module provides user-friendly error types that NEVER expose SeaORM internals.
//! All database errors are translated into these types before reaching user code.

use std::fmt;
use thiserror::Error;

/// A specialized Result type for TideORM operations
/// 
/// Note: Named `TideResult` to avoid conflicts with `std::result::Result` in derive macros.
/// You can also use the re-exported `Result` from the prelude in most contexts.
pub type Result<T> = std::result::Result<T, Error>;

/// The main error type for TideORM
///
/// These errors are designed to be helpful and actionable for developers.
/// They never expose internal ORM implementation details.
#[derive(Error, Debug)]
pub enum Error {
    /// Record was not found in the database
    #[error("Record not found: {message}")]
    NotFound {
        /// Description of what was not found
        message: String,
        /// Optional table name for context
        #[source]
        context: Option<ErrorContext>,
    },
    
    /// Database connection failed
    #[error("Connection error: {message}")]
    Connection {
        /// Details about the connection failure
        message: String,
    },
    
    /// Query execution failed
    #[error("Query error: {message}")]
    Query {
        /// Details about the query failure
        message: String,
        /// Optional context about the query
        #[source]
        context: Option<ErrorContext>,
    },
    
    /// Data validation failed
    #[error("Validation error: {field} - {message}")]
    Validation {
        /// The field that failed validation
        field: String,
        /// Description of the validation failure
        message: String,
    },
    
    /// Type conversion failed
    #[error("Conversion error: {message}")]
    Conversion {
        /// Details about the conversion failure
        message: String,
    },
    
    /// Transaction failed
    #[error("Transaction error: {message}")]
    Transaction {
        /// Details about the transaction failure
        message: String,
    },
    
    /// Configuration error
    #[error("Configuration error: {message}")]
    Configuration {
        /// Details about the configuration issue
        message: String,
    },
    
    /// Internal error (should rarely happen)
    #[error("Internal error: {message}")]
    Internal {
        /// Details about the internal error
        message: String,
    },
}

/// Additional context for errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Table name involved in the error
    pub table: Option<String>,
    /// Column name involved in the error
    pub column: Option<String>,
    /// The SQL query that caused the error (if available)
    pub query: Option<String>,
}

impl std::fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if let Some(ref table) = self.table {
            parts.push(format!("table: {}", table));
        }
        if let Some(ref column) = self.column {
            parts.push(format!("column: {}", column));
        }
        if let Some(ref query) = self.query {
            parts.push(format!("query: {}", query));
        }
        write!(f, "{}", parts.join(", "))
    }
}

impl std::error::Error for ErrorContext {}

impl ErrorContext {
    /// Create a new error context
    pub fn new() -> Self {
        Self {
            table: None,
            column: None,
            query: None,
        }
    }
    
    /// Set the table name
    pub fn table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }
    
    /// Set the column name
    pub fn column(mut self, column: impl Into<String>) -> Self {
        self.column = Some(column.into());
        self
    }
    
    /// Set the query
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Error {
    /// Create a NotFound error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
            context: None,
        }
    }
    
    /// Create a NotFound error with context
    pub fn not_found_with_context(message: impl Into<String>, context: ErrorContext) -> Self {
        Self::NotFound {
            message: message.into(),
            context: Some(context),
        }
    }
    
    /// Create a Connection error
    pub fn connection(message: impl Into<String>) -> Self {
        Self::Connection {
            message: message.into(),
        }
    }
    
    /// Create a Query error
    pub fn query(message: impl Into<String>) -> Self {
        Self::Query {
            message: message.into(),
            context: None,
        }
    }
    
    /// Create a Query error with context
    pub fn query_with_context(message: impl Into<String>, context: ErrorContext) -> Self {
        Self::Query {
            message: message.into(),
            context: Some(context),
        }
    }
    
    /// Create a Validation error
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }
    
    /// Create a Conversion error
    pub fn conversion(message: impl Into<String>) -> Self {
        Self::Conversion {
            message: message.into(),
        }
    }
    
    /// Create a Transaction error
    pub fn transaction(message: impl Into<String>) -> Self {
        Self::Transaction {
            message: message.into(),
        }
    }
    
    /// Create a Configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }
    
    /// Create an Internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
    
    /// Get the error context if available
    pub fn context(&self) -> Option<&ErrorContext> {
        match self {
            Self::NotFound { context, .. } => context.as_ref(),
            Self::Query { context, .. } => context.as_ref(),
            _ => None,
        }
    }
    
    /// Add context to an error
    pub fn with_context(self, ctx: ErrorContext) -> Self {
        match self {
            Self::NotFound { message, .. } => Self::NotFound { message, context: Some(ctx) },
            Self::Query { message, .. } => Self::Query { message, context: Some(ctx) },
            other => other,
        }
    }
    
    /// Check if this is a NotFound error
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
    
    /// Check if this is a Connection error
    pub fn is_connection_error(&self) -> bool {
        matches!(self, Self::Connection { .. })
    }
    
    /// Check if this is a Validation error
    pub fn is_validation_error(&self) -> bool {
        matches!(self, Self::Validation { .. })
    }
    
    /// Check if this is a Query error
    pub fn is_query_error(&self) -> bool {
        matches!(self, Self::Query { .. })
    }
}

/// Validation error builder for collecting multiple validation errors
#[derive(Debug, Default)]
pub struct ValidationErrors {
    errors: Vec<(String, String)>,
}

impl ValidationErrors {
    /// Create a new empty ValidationErrors
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add a validation error
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors.push((field.into(), message.into()));
    }
    
    /// Check if there are any errors
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
    
    /// Get all errors
    pub fn errors(&self) -> &[(String, String)] {
        &self.errors
    }
    
    /// Convert to a single Error (takes the first error)
    pub fn into_error(self) -> Option<Error> {
        self.errors
            .into_iter()
            .next()
            .map(|(field, message)| Error::validation(field, message))
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (field, message)) in self.errors.iter().enumerate() {
            if i > 0 {
                write!(f, "; ")?;
            }
            write!(f, "{}: {}", field, message)?;
        }
        Ok(())
    }
}
