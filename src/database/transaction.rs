use std::future::Future;
use std::sync::Arc;

use crate::error::{Error, Result};

use super::state::with_connection_override;
use super::{Database, DatabaseHandle};

impl Database {
    /// Execute a closure within a database transaction
    pub async fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'c> FnOnce(
                &'c Transaction,
            )
                -> std::pin::Pin<Box<dyn Future<Output = Result<T>> + Send + 'c>>
            + Send,
        T: Send,
    {
        use crate::internal::TransactionTrait;

        let txn = match self.__get_connection()? {
            ConnectionRef::Database(conn) => conn
                .connection()
                .begin()
                .await
                .map_err(|e| Error::transaction(e.to_string()))?,
            ConnectionRef::Transaction(tx) => tx
                .as_ref()
                .begin()
                .await
                .map_err(|e| Error::transaction(e.to_string()))?,
        };

        let outcome = {
            let txn = Arc::new(txn);
            let tx = Transaction { inner: txn.clone() };
            let override_handle = DatabaseHandle::Transaction(txn.clone());
            let outcome = with_connection_override(override_handle, f(&tx)).await;

            (txn, outcome)
        };

        let (txn, outcome) = outcome;

        match outcome {
            Ok(result) => {
                let txn = Arc::try_unwrap(txn).map_err(|_| {
                    Error::transaction(
                        "transaction handle leaked outside the transaction scope".to_string(),
                    )
                })?;
                txn.commit()
                    .await
                    .map_err(|e| Error::transaction(e.to_string()))?;
                Ok(result)
            }
            Err(e) => {
                let txn = Arc::try_unwrap(txn).map_err(|_| {
                    Error::transaction(
                        "transaction handle leaked outside the transaction scope".to_string(),
                    )
                })?;
                let _ = txn.rollback().await;
                Err(e)
            }
        }
    }
}

/// A database transaction handle
pub struct Transaction {
    pub(super) inner: Arc<crate::internal::OrmTransaction>,
}

impl Transaction {
    /// Get a reference to the underlying connection.
    pub fn connection(&self) -> &crate::internal::OrmTransaction {
        self.inner.as_ref()
    }

    /// Get the raw internal transaction (for internal use only)
    #[doc(hidden)]
    pub fn __internal_transaction(&self) -> &crate::internal::OrmTransaction {
        self.inner.as_ref()
    }
}

/// Trait for types that can be used as a database connection
pub trait Connection: Send + Sync {
    /// Get the internal connection for query execution
    #[doc(hidden)]
    fn __get_connection(&self) -> Result<ConnectionRef>;
}

/// Internal connection reference (hidden from users)
#[doc(hidden)]
pub enum ConnectionRef {
    Database(Arc<crate::internal::InternalConnection>),
    Transaction(Arc<crate::internal::OrmTransaction>),
}

impl Connection for Database {
    fn __get_connection(&self) -> Result<ConnectionRef> {
        Ok(match self.current_handle()? {
            DatabaseHandle::Connection(inner) => ConnectionRef::Database(inner),
            DatabaseHandle::Transaction(tx) => ConnectionRef::Transaction(tx),
            #[cfg(test)]
            DatabaseHandle::TestScope => {
                unreachable!("test scope marker does not carry a connection reference")
            }
        })
    }
}

impl Connection for Transaction {
    fn __get_connection(&self) -> Result<ConnectionRef> {
        Ok(ConnectionRef::Transaction(self.inner.clone()))
    }
}
