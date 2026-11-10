use orchard::db::{DbError, Store, StoreError, StorePaths, latest_version};
use orchard::domain::{Amount, DomainError};
use orchard::secrets::{self, SecretKey, SecretsError};
use rusqlite::Connection;
use tempfile::TempDir;

fn key(byte: u8) -> SecretKey {
    SecretKey::from_bytes([byte; 32])
}

fn paths(directory: &TempDir) -> StorePaths {
    StorePaths::new(directory.path())
}

#[tokio::test]
async fn fresh_store_applies_schema_and_creates_layout() {
    let directory = TempDir::new().unwrap();
    let paths = paths(&directory);
    let store = Store::open(paths.clone(), &key(1)).unwrap();

    assert!(paths.database().is_file());
    assert!(paths.lock_file().is_file());
    assert!(paths.evidence_objects().is_dir());
    assert!(paths.evidence_tmp().is_dir());

    let version: i64 = store
        .db()
        .read(|conn| {
            Ok(
                conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get(0)
                })?,
            )
        })
        .await
        .unwrap();
    assert_eq!(version, latest_version());

    let key_checks: i64 = store
        .db()
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM app_meta WHERE key = 'credential_key_check'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(key_checks, 1);
}

#[test]
fn simultaneous_store_ownership_is_rejected() {
    let directory = TempDir::new().unwrap();
    let paths = paths(&directory);
    let _owner = Store::open(paths.clone(), &key(1)).unwrap();

    let error = Store::open(paths, &key(1)).unwrap_err();
    assert!(matches!(error, StoreError::Locked { .. }));
    assert!(
        error
            .to_string()
            .contains("held by another Orchard process")
    );
}

#[test]
fn newer_schema_is_refused_without_touching_the_store() {
    let directory = TempDir::new().unwrap();
    let paths = paths(&directory);
    std::fs::create_dir_all(paths.root()).unwrap();

    let newer = latest_version() + 1;
    {
        let conn = Connection::open(paths.database()).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL
            ) STRICT;
            INSERT INTO schema_migrations VALUES ({newer}, 'future', '2026-01-01T00:00:00Z');"
        ))
        .unwrap();
    }

    let error = Store::open(paths.clone(), &key(1)).unwrap_err();
    assert!(matches!(
        error,
        StoreError::SchemaTooNew { found, supported }
            if found == newer && supported == latest_version()
    ));

    let conn = Connection::open(paths.database()).unwrap();
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows, vec![(newer, "future".to_owned())]);
}

#[tokio::test]
async fn wrong_key_does_not_replace_credentials_or_reinitialize() {
    let directory = TempDir::new().unwrap();
    let paths = paths(&directory);
    let correct = key(1);

    let envelope_before = {
        let store = Store::open(paths.clone(), &correct).unwrap();
        store
            .db()
            .transaction(move |tx| {
                secrets::store_provider_secret(tx, &correct, "simplefin", b"access-url")?;
                Ok(())
            })
            .await
            .unwrap();
        read_envelope(&store, "simplefin").await
    };

    let error = Store::open(paths.clone(), &key(2)).unwrap_err();
    assert!(matches!(error, StoreError::Secrets(SecretsError::WrongKey)));

    let conn = Connection::open(paths.database()).unwrap();
    let envelope_after: Vec<u8> = conn
        .query_row(
            "SELECT envelope FROM provider_secrets WHERE provider = 'simplefin'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(envelope_before, envelope_after);

    let key_checks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM app_meta WHERE key = 'credential_key_check'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        key_checks, 1,
        "a failed key check must not add an installation"
    );
}

#[tokio::test]
async fn tampered_provider_ciphertext_is_refused() {
    let directory = TempDir::new().unwrap();
    let paths = paths(&directory);
    let correct = key(1);
    let store = Store::open(paths, &correct).unwrap();

    store
        .db()
        .transaction(move |tx| {
            secrets::store_provider_secret(tx, &correct, "simplefin", b"access-url")?;
            Ok(())
        })
        .await
        .unwrap();

    let mut envelope = read_envelope(&store, "simplefin").await;
    *envelope.last_mut().unwrap() ^= 0x01;
    store
        .db()
        .transaction(move |tx| {
            tx.execute(
                "UPDATE provider_secrets SET envelope = ?1 WHERE provider = 'simplefin'",
                [envelope],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let key = key(1);
    let read = store
        .db()
        .read(move |conn| Ok(secrets::read_provider_secret(conn, &key, "simplefin")))
        .await
        .unwrap();
    assert!(matches!(read, Err(SecretsError::Authentication)));
}

#[tokio::test]
async fn overflowing_amount_leaves_no_partial_write() {
    let directory = TempDir::new().unwrap();
    let store = Store::open(paths(&directory), &key(1)).unwrap();

    let outcome: Result<(), DbError> = store
        .db()
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO app_meta (key, value) VALUES ('probe', x'01')",
                [],
            )?;
            let _ = Amount::parse_decimal("92233720368547758.08")?;
            Ok(())
        })
        .await;
    assert!(matches!(
        outcome,
        Err(DbError::Domain(DomainError::AmountOverflow))
    ));

    let rows: i64 = store
        .db()
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM app_meta WHERE key = 'probe'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

async fn read_envelope(store: &Store, provider: &'static str) -> Vec<u8> {
    store
        .db()
        .read(move |conn| {
            Ok(conn.query_row(
                "SELECT envelope FROM provider_secrets WHERE provider = ?1",
                [provider],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap()
}
