use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, Transaction};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::DbError;

/// Bounded blocking access to the single SQLite connection.
///
/// Tokio's async threads never execute SQL directly: every operation takes a
/// permit, then runs on the blocking pool while holding the connection mutex.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    permits: Arc<Semaphore>,
}

impl Db {
    pub fn new(conn: Connection, max_in_flight: usize) -> Self {
        assert!(max_in_flight > 0, "max_in_flight must be positive");
        Self {
            conn: Arc::new(Mutex::new(conn)),
            permits: Arc::new(Semaphore::new(max_in_flight)),
        }
    }

    /// Runs a read-only closure against the connection.
    pub async fn read<T, F>(&self, operation: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, DbError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let _permit = self.permit().await?;
        tokio::task::spawn_blocking(move || {
            let guard = lock(&conn);
            operation(&guard)
        })
        .await
        .map_err(DbError::Blocking)?
    }

    /// Runs a closure inside one SQL transaction, committing only on `Ok`.
    pub async fn transaction<T, F>(&self, operation: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T, DbError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let _permit = self.permit().await?;
        tokio::task::spawn_blocking(move || {
            let mut guard = lock(&conn);
            let transaction = guard.transaction()?;
            let value = operation(&transaction)?;
            transaction.commit()?;
            Ok(value)
        })
        .await
        .map_err(DbError::Blocking)?
    }

    /// Runs a read-only closure whose error type carries `DbError`.
    ///
    /// This is the ledger-friendly form of [`Db::read`]: domain commands can
    /// return their own error enum as long as it converts from `DbError`.
    pub async fn run_read<T, E, F>(&self, operation: F) -> Result<T, E>
    where
        T: Send + 'static,
        E: From<DbError> + From<rusqlite::Error> + Send + 'static,
        F: FnOnce(&Connection) -> Result<T, E> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let _permit = self.permit().await.map_err(E::from)?;
        tokio::task::spawn_blocking(move || {
            let guard = lock(&conn);
            operation(&guard)
        })
        .await
        .map_err(|source| E::from(DbError::Blocking(source)))?
    }

    /// Runs a closure inside one SQL transaction with a caller-chosen error
    /// type. Commit or rollback is decided exactly as in [`Db::transaction`].
    pub async fn run_transaction<T, E, F>(&self, operation: F) -> Result<T, E>
    where
        T: Send + 'static,
        E: From<DbError> + From<rusqlite::Error> + Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T, E> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let _permit = self.permit().await.map_err(E::from)?;
        tokio::task::spawn_blocking(move || {
            let mut guard = lock(&conn);
            let transaction = guard
                .transaction()
                .map_err(|source| E::from(DbError::from(source)))?;
            let value = operation(&transaction)?;
            transaction
                .commit()
                .map_err(|source| E::from(DbError::from(source)))?;
            Ok(value)
        })
        .await
        .map_err(|source| E::from(DbError::Blocking(source)))?
    }

    async fn permit(&self) -> Result<OwnedSemaphorePermit, DbError> {
        Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| DbError::Closed)
    }
}

fn lock(conn: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
