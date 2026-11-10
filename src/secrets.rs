//! Authenticated encryption for provider credentials and the mounted key.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload, consts::U12},
};
use rusqlite::{Connection, OptionalExtension, params};

const KEY_CHECK_META_KEY: &str = "credential_key_check";
const KEY_CHECK_AAD: &[u8] = b"orchard.credential-key-check.v1";
const KEY_CHECK_PLAINTEXT: &[u8] = b"orchard credential key v1";
const PROVIDER_SECRET_AAD_PREFIX: &str = "orchard.provider-secret.v1:";
const ENVELOPE_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const MIN_ENVELOPE_LEN: usize = 1 + NONCE_LEN + TAG_LEN;

/// A 32-byte AES-256 master key kept outside the application data directory.
#[derive(Clone)]
pub struct SecretKey([u8; 32]);

impl SecretKey {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn generate() -> Result<Self, SecretsError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| SecretsError::Random)?;
        Ok(Self(bytes))
    }

    /// Loads a key file containing exactly 64 hexadecimal characters.
    pub fn load(path: &Path) -> Result<Self, SecretsError> {
        let raw = fs::read_to_string(path).map_err(|source| SecretsError::KeyFile {
            path: path.to_path_buf(),
            source,
        })?;
        decode_hex(raw.trim())
            .map(Self)
            .ok_or_else(|| SecretsError::InvalidKeyFile {
                path: path.to_path_buf(),
            })
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&self.0).expect("AES-256 requires a 32-byte key")
    }

    /// Encrypts with a fresh random nonce and binds the ciphertext to `aad`.
    pub fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, SecretsError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce_bytes).map_err(|_| SecretsError::Random)?;
        let nonce: aes_gcm::Nonce<U12> = nonce_bytes.into();
        let ciphertext = self
            .cipher()
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| SecretsError::Encrypt)?;

        let mut envelope = Vec::with_capacity(1 + nonce_bytes.len() + ciphertext.len());
        envelope.push(ENVELOPE_VERSION);
        envelope.extend_from_slice(&nonce_bytes);
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    /// Decrypts an envelope produced by [`SecretKey::seal`].
    pub fn open(&self, aad: &[u8], envelope: &[u8]) -> Result<Vec<u8>, SecretsError> {
        if envelope.len() < MIN_ENVELOPE_LEN || envelope[0] != ENVELOPE_VERSION {
            return Err(SecretsError::Envelope);
        }
        let nonce_bytes: [u8; NONCE_LEN] = envelope[1..1 + NONCE_LEN]
            .try_into()
            .map_err(|_| SecretsError::Envelope)?;
        let nonce: aes_gcm::Nonce<U12> = nonce_bytes.into();
        self.cipher()
            .decrypt(
                &nonce,
                Payload {
                    msg: &envelope[1 + NONCE_LEN..],
                    aad,
                },
            )
            .map_err(|_| SecretsError::Authentication)
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

fn decode_hex(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        let high = hex_value(bytes[index * 2])?;
        let low = hex_value(bytes[index * 2 + 1])?;
        *byte = (high << 4) | low;
    }
    Some(key)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Whether opening the store initialized its key check or verified one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCheck {
    Initialized,
    Existing,
}

/// Verifies that `key` decrypts this installation's key check, initializing it
/// on a fresh store.
///
/// A wrong or tampered key check is an error; it never rewrites credentials and
/// never initializes a replacement installation.
pub fn verify_installation_key(
    conn: &mut Connection,
    key: &SecretKey,
) -> Result<KeyCheck, SecretsError> {
    let transaction = conn.transaction().map_err(SecretsError::Database)?;
    let existing: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT value FROM app_meta WHERE key = ?1",
            [KEY_CHECK_META_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(SecretsError::Database)?;

    match existing {
        Some(envelope) => {
            let plaintext = key
                .open(KEY_CHECK_AAD, &envelope)
                .map_err(|_| SecretsError::WrongKey)?;
            if plaintext != KEY_CHECK_PLAINTEXT {
                return Err(SecretsError::WrongKey);
            }
            Ok(KeyCheck::Existing)
        }
        None => {
            let envelope = key.seal(KEY_CHECK_AAD, KEY_CHECK_PLAINTEXT)?;
            transaction
                .execute(
                    "INSERT INTO app_meta (key, value) VALUES (?1, ?2)",
                    params![KEY_CHECK_META_KEY, envelope],
                )
                .map_err(SecretsError::Database)?;
            transaction.commit().map_err(SecretsError::Database)?;
            Ok(KeyCheck::Initialized)
        }
    }
}

fn provider_secret_aad(provider: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(PROVIDER_SECRET_AAD_PREFIX.len() + provider.len());
    aad.extend_from_slice(PROVIDER_SECRET_AAD_PREFIX.as_bytes());
    aad.extend_from_slice(provider.as_bytes());
    aad
}

/// Encrypts and stores a provider secret, bound to the provider name.
pub fn store_provider_secret(
    conn: &Connection,
    key: &SecretKey,
    provider: &str,
    plaintext: &[u8],
) -> Result<(), SecretsError> {
    let envelope = key.seal(&provider_secret_aad(provider), plaintext)?;
    conn.execute(
        "INSERT INTO provider_secrets (provider, envelope, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(provider) DO UPDATE SET
             envelope = excluded.envelope,
             updated_at = excluded.updated_at",
        params![provider, envelope],
    )
    .map_err(SecretsError::Database)?;
    Ok(())
}

/// Reads and authenticates a stored provider secret.
pub fn read_provider_secret(
    conn: &Connection,
    key: &SecretKey,
    provider: &str,
) -> Result<Vec<u8>, SecretsError> {
    let envelope: Option<Vec<u8>> = conn
        .query_row(
            "SELECT envelope FROM provider_secrets WHERE provider = ?1",
            [provider],
            |row| row.get(0),
        )
        .optional()
        .map_err(SecretsError::Database)?;
    let envelope = envelope.ok_or_else(|| SecretsError::MissingSecret {
        provider: provider.to_owned(),
    })?;
    key.open(&provider_secret_aad(provider), &envelope)
}

#[derive(Debug)]
pub enum SecretsError {
    KeyFile { path: PathBuf, source: io::Error },
    InvalidKeyFile { path: PathBuf },
    Random,
    Encrypt,
    Envelope,
    Authentication,
    WrongKey,
    MissingSecret { provider: String },
    Database(rusqlite::Error),
}

impl fmt::Display for SecretsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyFile { path, source } => write!(
                f,
                "cannot read credential key file `{}`: {source}",
                path.display()
            ),
            Self::InvalidKeyFile { path } => write!(
                f,
                "credential key file `{}` must contain exactly 64 hexadecimal characters",
                path.display()
            ),
            Self::Random => f.write_str("cannot obtain secure random bytes"),
            Self::Encrypt => f.write_str("cannot encrypt provider secret"),
            Self::Envelope => f.write_str("credential envelope is malformed"),
            Self::Authentication => {
                f.write_str("credential ciphertext failed authentication or was tampered with")
            }
            Self::WrongKey => f.write_str(
                "the mounted credential key does not match this store; refusing to replace credentials",
            ),
            Self::MissingSecret { provider } => {
                write!(f, "no stored secret exists for provider `{provider}`")
            }
            Self::Database(source) => write!(f, "credential storage error: {source}"),
        }
    }
}

impl Error for SecretsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::KeyFile { source, .. } => Some(source),
            Self::Database(source) => Some(source),
            Self::InvalidKeyFile { .. }
            | Self::Random
            | Self::Encrypt
            | Self::Envelope
            | Self::Authentication
            | Self::WrongKey
            | Self::MissingSecret { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const KEY_A: SecretKey = SecretKey::from_bytes([0x11; 32]);
    const KEY_B: SecretKey = SecretKey::from_bytes([0x22; 32]);

    fn connection() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        conn
    }

    #[test]
    fn ciphertext_is_authenticated_and_bound_to_aad() {
        let envelope = KEY_A.seal(b"context-a", b"sensitive").unwrap();
        assert_eq!(KEY_A.open(b"context-a", &envelope).unwrap(), b"sensitive");
        assert!(matches!(
            KEY_A.open(b"context-b", &envelope),
            Err(SecretsError::Authentication)
        ));

        let mut tampered = envelope.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        assert!(matches!(
            KEY_A.open(b"context-a", &tampered),
            Err(SecretsError::Authentication)
        ));
        assert!(matches!(
            KEY_B.open(b"context-a", &envelope),
            Err(SecretsError::Authentication)
        ));
    }

    #[test]
    fn loads_hex_key_files_and_rejects_bad_ones() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("credentials.key");
        fs::write(
            &path,
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\n",
        )
        .unwrap();
        let loaded = SecretKey::load(&path).unwrap();
        let expected = SecretKey::from_bytes([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ]);
        let envelope = expected.seal(b"aad", b"value").unwrap();
        assert_eq!(loaded.open(b"aad", &envelope).unwrap(), b"value");
        assert!(!format!("{loaded:?}").contains("0001"));

        fs::write(&path, "too-short").unwrap();
        assert!(matches!(
            SecretKey::load(&path),
            Err(SecretsError::InvalidKeyFile { .. })
        ));

        assert!(matches!(
            SecretKey::load(&directory.path().join("missing.key")),
            Err(SecretsError::KeyFile { .. })
        ));
    }

    #[test]
    fn key_check_initializes_once_and_rejects_other_keys() {
        let mut conn = connection();
        assert_eq!(
            verify_installation_key(&mut conn, &KEY_A).unwrap(),
            KeyCheck::Initialized
        );
        let stored: Vec<u8> = conn
            .query_row(
                "SELECT value FROM app_meta WHERE key = ?1",
                [KEY_CHECK_META_KEY],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(
            verify_installation_key(&mut conn, &KEY_A).unwrap(),
            KeyCheck::Existing
        );
        assert!(matches!(
            verify_installation_key(&mut conn, &KEY_B),
            Err(SecretsError::WrongKey)
        ));
        let unchanged: Vec<u8> = conn
            .query_row(
                "SELECT value FROM app_meta WHERE key = ?1",
                [KEY_CHECK_META_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, unchanged);
    }

    #[test]
    fn provider_secrets_round_trip_and_detect_tampering() {
        let conn = connection();
        store_provider_secret(&conn, &KEY_A, "simplefin", b"access-url").unwrap();
        assert_eq!(
            read_provider_secret(&conn, &KEY_A, "simplefin").unwrap(),
            b"access-url"
        );

        let mut envelope: Vec<u8> = conn
            .query_row(
                "SELECT envelope FROM provider_secrets WHERE provider = 'simplefin'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        *envelope.last_mut().unwrap() ^= 0x01;
        conn.execute(
            "UPDATE provider_secrets SET envelope = ?1 WHERE provider = 'simplefin'",
            [envelope],
        )
        .unwrap();
        assert!(matches!(
            read_provider_secret(&conn, &KEY_A, "simplefin"),
            Err(SecretsError::Authentication)
        ));
        assert!(matches!(
            read_provider_secret(&conn, &KEY_A, "telegram"),
            Err(SecretsError::MissingSecret { .. })
        ));
    }
}
