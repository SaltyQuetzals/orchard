use std::error::Error;
use std::fmt;

/// Errors produced while constructing or combining domain values.
///
/// Variants are matched exhaustively by callers; no catch-all internal state is
/// represented here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// The input is not a signed decimal number.
    InvalidAmount { input: String },
    /// The input carries more than two fractional digits.
    FractionalCent { input: String },
    /// The value cannot be represented as signed 64-bit cents.
    AmountOverflow,
    /// The input is not a valid `YYYY-MM-DD` calendar date.
    InvalidDate { input: String },
    /// The input is not a valid `YYYY-MM` budget month.
    InvalidMonth { input: String },
    /// The input is not a valid numeric identifier.
    InvalidIdentifier { kind: &'static str, input: String },
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAmount { input } => write!(
                f,
                "invalid amount `{input}`: expected a decimal number such as `12.34`"
            ),
            Self::FractionalCent { input } => write!(
                f,
                "invalid amount `{input}`: USD amounts support at most two fractional digits"
            ),
            Self::AmountOverflow => f.write_str("amount exceeds the supported range"),
            Self::InvalidMonth { input } => write!(
                f,
                "invalid month `{input}`: expected a calendar month as `YYYY-MM`"
            ),
            Self::InvalidDate { input } => {
                write!(
                    f,
                    "invalid date `{input}`: expected a real calendar date as `YYYY-MM-DD`"
                )
            }
            Self::InvalidIdentifier { kind, input } => {
                write!(f, "invalid {kind} identifier `{input}`")
            }
        }
    }
}

impl Error for DomainError {}
