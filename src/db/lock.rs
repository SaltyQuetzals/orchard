use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use super::StoreError;

/// An exclusive advisory lock on the store, held for the process lifetime.
///
/// The lock lives in a file beside the database and is released when the
/// process exits or this value is dropped.
#[derive(Debug)]
pub struct StoreLock {
    _file: File,
}

impl StoreLock {
    pub fn acquire(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|source| StoreError::LockIo {
                path: path.to_path_buf(),
                source,
            })?;

        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(StoreError::Locked {
                path: path.to_path_buf(),
            }),
            Err(TryLockError::Error(source)) => Err(StoreError::LockIo {
                path: path.to_path_buf(),
                source,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn one_owner_per_lock_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orchard.lock");
        let first = StoreLock::acquire(&path).unwrap();
        assert!(matches!(
            StoreLock::acquire(&path),
            Err(StoreError::Locked { .. })
        ));
        drop(first);
        assert!(StoreLock::acquire(&path).is_ok());
    }
}
