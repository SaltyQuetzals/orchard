use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use crate::secrets::{self, SecretKey};

use super::{Db, MIGRATIONS, Migration, StoreError, lock::StoreLock, migrations};

/// Maximum number of blocking database operations admitted concurrently.
pub const DB_MAX_IN_FLIGHT: usize = 4;

/// Locations owned by one Orchard installation.
#[derive(Debug, Clone)]
pub struct StorePaths {
    root: PathBuf,
}

impl StorePaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("orchard.sqlite3")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.root.join("orchard.lock")
    }

    pub fn evidence_objects(&self) -> PathBuf {
        self.root.join("evidence").join("objects")
    }

    pub fn evidence_tmp(&self) -> PathBuf {
        self.root.join("evidence").join("tmp")
    }

    fn create_directories(&self) -> Result<(), StoreError> {
        for directory in [
            self.root.clone(),
            self.evidence_objects(),
            self.evidence_tmp(),
        ] {
            fs::create_dir_all(&directory).map_err(|source| StoreError::Io {
                path: directory,
                source,
            })?;
        }
        Ok(())
    }
}

/// An opened installation: exclusive lock, migrated database, and verified key.
pub struct Store {
    paths: StorePaths,
    db: Db,
    _lock: StoreLock,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Opens an existing or fresh store under the exclusive lock.
    ///
    /// Migrations run before the key is checked. A newer schema, a failed
    /// migration, or a mismatched key all prevent the store from being used.
    pub fn open(paths: StorePaths, key: &SecretKey) -> Result<Self, StoreError> {
        Self::open_with_migrations(paths, key, MIGRATIONS)
    }

    pub(crate) fn open_with_migrations(
        paths: StorePaths,
        key: &SecretKey,
        migrations: &[Migration],
    ) -> Result<Self, StoreError> {
        paths.create_directories()?;
        let lock = StoreLock::acquire(&paths.lock_file())?;

        let mut conn = Connection::open(paths.database()).map_err(StoreError::Database)?;
        configure(&conn)?;
        migrations::migrate(&mut conn, migrations)?;
        secrets::verify_installation_key(&mut conn, key).map_err(StoreError::Secrets)?;

        Ok(Self {
            db: Db::new(conn, DB_MAX_IN_FLIGHT),
            paths,
            _lock: lock,
        })
    }

    pub fn paths(&self) -> &StorePaths {
        &self.paths
    }

    pub fn db(&self) -> &Db {
        &self.db
    }
}

fn configure(conn: &Connection) -> Result<(), StoreError> {
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(StoreError::Database)?;
    let _: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(StoreError::Database)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(StoreError::Database)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(StoreError::Database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;
    use tempfile::TempDir;

    fn key() -> SecretKey {
        SecretKey::from_bytes([7; 32])
    }

    #[test]
    fn failed_migration_prevents_open_and_leaves_no_partial_schema() {
        let directory = TempDir::new().unwrap();
        let paths = StorePaths::new(directory.path());
        let migrations = [
            Migration {
                version: 1,
                name: "good",
                sql: "CREATE TABLE good (id INTEGER PRIMARY KEY) STRICT;",
            },
            Migration {
                version: 2,
                name: "bad",
                sql: "CREATE TABLE partial (id INTEGER PRIMARY KEY) STRICT;
                     CREATE TABLE good (id INTEGER PRIMARY KEY) STRICT;",
            },
        ];

        let error = Store::open_with_migrations(paths.clone(), &key(), &migrations).unwrap_err();
        assert!(matches!(
            error,
            StoreError::MigrationFailed { version: 2, .. }
        ));

        let conn = Connection::open(paths.database()).unwrap();
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 1);
        let partial: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'partial'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(partial, None);

        let lock_works = StoreLock::acquire(&paths.lock_file()).unwrap();
        drop(lock_works);
    }
}
