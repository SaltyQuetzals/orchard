use std::error::Error;
use std::fmt;

use crate::db::DbError;

/// Failures from owner initialization, recovery, and session handling.
#[derive(Debug)]
pub enum AuthError {
    /// An owner already exists; initialization never replaces one.
    AlreadyInitialized,
    /// The host-access initialize command has not run.
    NotInitialized,
    /// The timezone is not a recognized IANA zone.
    InvalidTimezone { timezone: String },
    /// The budget name must be nonblank and reasonably short.
    InvalidBudgetName,
    /// The password is shorter than the documented minimum.
    PasswordTooShort { minimum: usize },
    /// The password exceeds the supported input bound.
    PasswordTooLong { maximum: usize },
    /// Argon2 hashing or verification failed.
    PasswordHashing,
    /// The system random source failed.
    Random,
    /// The store rejected the operation.
    Database(DbError),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInitialized => {
                f.write_str("this installation is already initialized; refusing to replace its owner")
            }
            Self::NotInitialized => {
                f.write_str("this installation has no owner; run the initialize command first")
            }
            Self::InvalidTimezone { timezone } => write!(
                f,
                "unknown timezone `{timezone}`: use an IANA name such as `America/Chicago` or `UTC`"
            ),
            Self::InvalidBudgetName => {
                f.write_str("the budget name must be 1 to 120 characters")
            }
            Self::PasswordTooShort { minimum } => {
                write!(f, "the password must contain at least {minimum} characters")
            }
            Self::PasswordTooLong { maximum } => {
                write!(f, "the password must not exceed {maximum} bytes")
            }
            Self::PasswordHashing => f.write_str("password hashing failed"),
            Self::Random => f.write_str("the system random source is unavailable"),
            Self::Database(source) => source.fmt(f),
        }
    }
}

impl Error for AuthError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(source) => Some(source),
            _ => None,
        }
    }
}

impl From<DbError> for AuthError {
    fn from(source: DbError) -> Self {
        Self::Database(source)
    }
}
