use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::domain::DomainError;
use crate::secrets::SecretsError;

/// Outcome of opening the persistent store. Every variant prevents normal
/// serving or writes; nothing is silently replaced or reinitialized.
#[derive(Debug)]
pub enum StoreError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Database(rusqlite::Error),
    Locked {
        path: PathBuf,
    },
    LockIo {
        path: PathBuf,
        source: io::Error,
    },
    SchemaTooNew {
        found: i64,
        supported: i64,
    },
    MigrationMismatch {
        version: i64,
        recorded: String,
        expected: &'static str,
    },
    MigrationFailed {
        version: i64,
        name: &'static str,
        source: rusqlite::Error,
    },
    Secrets(SecretsError),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "cannot prepare store directory `{}`: {source}",
                    path.display()
                )
            }
            Self::Database(source) => write!(f, "store database error: {source}"),
            Self::Locked { path } => write!(
                f,
                "store lock `{}` is held by another Orchard process or maintenance command",
                path.display()
            ),
            Self::LockIo { path, source } => {
                write!(
                    f,
                    "cannot lock store lock file `{}`: {source}",
                    path.display()
                )
            }
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "store schema version {found} is newer than this build supports ({supported}); run the matching Orchard version"
            ),
            Self::MigrationMismatch {
                version,
                recorded,
                expected,
            } => write!(
                f,
                "store migration {version} is recorded as `{recorded}` but this build expects `{expected}`"
            ),
            Self::MigrationFailed {
                version,
                name,
                source,
            } => write!(f, "migration {version} (`{name}`) failed: {source}"),
            Self::Secrets(source) => source.fmt(f),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Database(source) => Some(source),
            Self::LockIo { source, .. } => Some(source),
            Self::MigrationFailed { source, .. } => Some(source),
            Self::Secrets(source) => Some(source),
            Self::Locked { .. } | Self::SchemaTooNew { .. } | Self::MigrationMismatch { .. } => {
                None
            }
        }
    }
}

/// Errors from the bounded blocking database executor.
#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Domain(DomainError),
    Secrets(SecretsError),
    Blocking(tokio::task::JoinError),
    Closed,
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(source) => write!(f, "database error: {source}"),
            Self::Domain(source) => source.fmt(f),
            Self::Secrets(source) => source.fmt(f),
            Self::Blocking(source) => write!(f, "database task failed: {source}"),
            Self::Closed => f.write_str("database executor is unavailable"),
        }
    }
}

impl Error for DbError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(source) => Some(source),
            Self::Domain(source) => Some(source),
            Self::Secrets(source) => Some(source),
            Self::Blocking(source) => Some(source),
            Self::Closed => None,
        }
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite(source)
    }
}

impl From<DomainError> for DbError {
    fn from(source: DomainError) -> Self {
        Self::Domain(source)
    }
}

impl From<SecretsError> for DbError {
    fn from(source: SecretsError) -> Self {
        Self::Secrets(source)
    }
}
