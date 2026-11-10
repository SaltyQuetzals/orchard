//! Argon2id password policy and hashing.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

use super::AuthError;

/// Minimum password length enforced for initialization and recovery.
const MIN_PASSWORD_CHARS: usize = 12;
/// Upper bound that keeps hashing input and request bodies bounded.
const MAX_PASSWORD_BYTES: usize = 1024;

/// Validates a new owner password before hashing it.
pub fn validate_new_password(password: &str) -> Result<(), AuthError> {
    if password.chars().count() < MIN_PASSWORD_CHARS || password.trim().is_empty() {
        return Err(AuthError::PasswordTooShort {
            minimum: MIN_PASSWORD_CHARS,
        });
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(AuthError::PasswordTooLong {
            maximum: MAX_PASSWORD_BYTES,
        });
    }
    Ok(())
}

/// Hashes a password with Argon2id and default cost parameters.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let mut salt_bytes = [0u8; 16];
    getrandom::fill(&mut salt_bytes).map_err(|_| AuthError::Random)?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| AuthError::PasswordHashing)?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::PasswordHashing)
}

/// Verifies a password against a stored PHC string. A malformed stored hash is
/// a failed verification, never an accepted password.
pub fn verify_password(encoded: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_with_argon2id_and_verifies_only_the_same_password() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password(&hash, "correct horse battery staple"));
        assert!(!verify_password(&hash, "correct horse battery stapler"));
        assert!(!verify_password("not-a-phc-string", "correct horse battery staple"));
    }

    #[test]
    fn enforces_password_bounds() {
        assert!(matches!(
            validate_new_password("too short"),
            Err(AuthError::PasswordTooShort { .. })
        ));
        assert!(validate_new_password("correct horse battery staple").is_ok());
        assert!(matches!(
            validate_new_password(&"a".repeat(MAX_PASSWORD_BYTES + 1)),
            Err(AuthError::PasswordTooLong { .. })
        ));
    }
}
