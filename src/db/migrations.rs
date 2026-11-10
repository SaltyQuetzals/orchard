use rusqlite::{Connection, params};

use super::StoreError;

/// One ordered, transactional schema migration embedded in the binary.
#[derive(Debug)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial",
    sql: include_str!("../../migrations/0001_initial.sql"),
}];

pub fn latest_version() -> i64 {
    MIGRATIONS
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or(0)
}

/// Applies pending migrations in order and rejects stores written by a newer
/// schema or by a different migration history.
///
/// Each migration runs in its own transaction together with its tracking row,
/// so a failure leaves the previous schema version and no partial changes.
pub(crate) fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        ) STRICT;",
    )
    .map_err(StoreError::Database)?;

    let applied: Vec<(i64, String)> = {
        let mut statement = conn
            .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
            .map_err(StoreError::Database)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(StoreError::Database)?;
        rows.collect::<Result<_, _>>()
            .map_err(StoreError::Database)?
    };

    let supported = migrations
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or(0);
    let found = applied.last().map(|(version, _)| *version).unwrap_or(0);

    if found > supported {
        return Err(StoreError::SchemaTooNew { found, supported });
    }

    for (version, recorded) in &applied {
        let expected = migrations
            .iter()
            .find(|migration| migration.version == *version);
        match expected {
            Some(migration) if migration.name == recorded => {}
            Some(migration) => {
                return Err(StoreError::MigrationMismatch {
                    version: *version,
                    recorded: recorded.clone(),
                    expected: migration.name,
                });
            }
            None => {
                return Err(StoreError::MigrationMismatch {
                    version: *version,
                    recorded: recorded.clone(),
                    expected: "",
                });
            }
        }
    }

    for migration in migrations
        .iter()
        .filter(|migration| migration.version > found)
    {
        let transaction = conn.transaction().map_err(StoreError::Database)?;
        transaction
            .execute_batch(migration.sql)
            .map_err(|source| StoreError::MigrationFailed {
                version: migration.version,
                name: migration.name,
                source,
            })?;
        transaction
            .execute(
                "INSERT INTO schema_migrations (version, name, applied_at)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                params![migration.version, migration.name],
            )
            .map_err(StoreError::Database)?;
        transaction.commit().map_err(StoreError::Database)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn
    }

    fn applied(conn: &Connection) -> Vec<(i64, String)> {
        let mut statement = conn
            .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn embedded_migrations_are_ordered_and_unique() {
        let mut previous = 0;
        for migration in MIGRATIONS {
            assert!(migration.version > previous);
            previous = migration.version;
        }
        assert_eq!(latest_version(), previous);
    }

    #[test]
    fn applies_only_pending_migrations() {
        let mut conn = connection();
        migrate(&mut conn, MIGRATIONS).unwrap();
        assert_eq!(applied(&conn), vec![(1, "initial".to_owned())]);
        migrate(&mut conn, MIGRATIONS).unwrap();
        assert_eq!(applied(&conn), vec![(1, "initial".to_owned())]);
    }

    #[test]
    fn failed_migration_rolls_back_and_keeps_previous_version() {
        let mut conn = connection();
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

        assert!(matches!(
            migrate(&mut conn, &migrations),
            Err(StoreError::MigrationFailed { version: 2, .. })
        ));
        assert_eq!(applied(&conn), vec![(1, "good".to_owned())]);
        let partial: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'partial'",
                [],
                |row| row.get(0),
            )
            .ok();
        assert_eq!(partial, None);
    }

    #[test]
    fn refuses_newer_schema_without_writing() {
        let mut conn = connection();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL
            ) STRICT;
            INSERT INTO schema_migrations VALUES (99, 'future', '2026-01-01T00:00:00Z');",
        )
        .unwrap();

        assert!(matches!(
            migrate(&mut conn, MIGRATIONS),
            Err(StoreError::SchemaTooNew {
                found: 99,
                supported: 1
            })
        ));
        assert_eq!(applied(&conn), vec![(99, "future".to_owned())]);
    }

    #[test]
    fn refuses_mismatched_migration_history() {
        let mut conn = connection();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL
            ) STRICT;
            INSERT INTO schema_migrations VALUES (1, 'renamed', '2026-01-01T00:00:00Z');",
        )
        .unwrap();

        assert!(matches!(
            migrate(&mut conn, MIGRATIONS),
            Err(StoreError::MigrationMismatch { version: 1, .. })
        ));
    }
}
