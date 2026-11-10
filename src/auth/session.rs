//! Server-controlled session tokens and CSRF state.

use sha2::{Digest, Sha256};

use super::AuthError;

/// Absolute server-enforced browser session lifetime (30 days).
pub const SESSION_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
/// Length of a raw session/CSRF token in hexadecimal characters.
pub const TOKEN_HEX_LEN: usize = 64;

/// The authenticated owner context attached to a protected request.
#[derive(Debug, Clone)]
pub struct Session {
    /// SHA-256 of the raw cookie token; the raw token is never stored.
    pub token_hash: String,
    /// Per-session secret submitted with state-changing forms.
    pub csrf_token: String,
    /// Server-enforced expiry as Unix seconds.
    pub expires_at: i64,
    /// Owner session-revocation generation this session belongs to.
    pub generation: i64,
}

impl Session {
    /// Constant-time comparison of a submitted CSRF token.
    pub fn accepts_csrf(&self, submitted: &str) -> bool {
        tokens_match(&self.csrf_token, submitted)
    }
}

/// Generates a 256-bit random token rendered as lowercase hexadecimal.
pub fn generate_token() -> Result<String, AuthError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Random)?;
    Ok(hex_lower(&bytes))
}

/// Hashes a raw session token for storage and lookup.
pub fn hash_token(token: &str) -> String {
    hex_lower(&Sha256::digest(token.as_bytes()))
}

/// Whether a header/cookie value has the shape of a generated token.
pub fn is_token_shape(value: &str) -> bool {
    value.len() == TOKEN_HEX_LEN && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Compares two tokens without an early exit on the first differing byte.
pub fn tokens_match(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_distinct_and_hashes_are_stable() {
        let first = generate_token().unwrap();
        let second = generate_token().unwrap();
        assert!(is_token_shape(&first));
        assert_ne!(first, second);
        assert_eq!(hash_token(&first), hash_token(&first));
        assert_ne!(hash_token(&first), hash_token(&second));
    }

    #[test]
    fn token_comparison_requires_an_exact_match() {
        assert!(tokens_match("abc", "abc"));
        assert!(!tokens_match("abc", "abd"));
        assert!(!tokens_match("abc", "ab"));
    }
}
