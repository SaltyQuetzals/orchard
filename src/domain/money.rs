use std::fmt;
use std::str::FromStr;

use super::DomainError;

/// A signed USD amount stored exactly as integer cents.
///
/// Construction only happens through [`Amount::from_cents`] or decimal parsing;
/// arithmetic is checked so overflow can never wrap into a plausible balance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Amount(i64);

impl Amount {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(i64::MAX);
    pub const MIN: Self = Self(i64::MIN);

    pub const fn from_cents(cents: i64) -> Self {
        Self(cents)
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(DomainError::AmountOverflow)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(DomainError::AmountOverflow)
    }

    pub fn checked_neg(self) -> Result<Self, DomainError> {
        self.0
            .checked_neg()
            .map(Self)
            .ok_or(DomainError::AmountOverflow)
    }

    /// Parses a plain signed decimal such as `12`, `-4.5`, or `+0.05`.
    ///
    /// Fractional cents are rejected rather than rounded.
    pub fn parse_decimal(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim();
        let invalid = || DomainError::InvalidAmount {
            input: input.to_owned(),
        };

        let (negative, digits) = match trimmed.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
        };
        if digits.is_empty() {
            return Err(invalid());
        }

        let mut parts = digits.split('.');
        let whole = parts.next().unwrap_or_default();
        let fraction = parts.next();
        if parts.next().is_some() {
            return Err(invalid());
        }
        if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }

        let fraction = match fraction {
            None => "",
            Some(fraction) => {
                if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid());
                }
                if fraction.len() > 2 {
                    return Err(DomainError::FractionalCent {
                        input: input.to_owned(),
                    });
                }
                fraction
            }
        };

        let mut magnitude: i128 = 0;
        for byte in whole.bytes() {
            magnitude = magnitude
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(byte - b'0')))
                .ok_or(DomainError::AmountOverflow)?;
        }

        let mut cents = magnitude
            .checked_mul(100)
            .ok_or(DomainError::AmountOverflow)?;
        let mut fraction_bytes = fraction.bytes();
        if let Some(tenths) = fraction_bytes.next() {
            cents += i128::from(tenths - b'0') * 10;
            if let Some(hundredths) = fraction_bytes.next() {
                cents += i128::from(hundredths - b'0');
            }
        }

        let signed = if negative { -cents } else { cents };
        let cents = i64::try_from(signed).map_err(|_| DomainError::AmountOverflow)?;
        Ok(Self(cents))
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cents = i128::from(self.0);
        let sign = if cents < 0 { "-" } else { "" };
        let magnitude = cents.abs();
        write!(f, "{sign}{}.{:02}", magnitude / 100, magnitude % 100)
    }
}

impl FromStr for Amount {
    type Err = DomainError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse_decimal(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_exact_cents() {
        let ten = Amount::parse_decimal("0.10").unwrap();
        let twenty = Amount::parse_decimal("0.20").unwrap();
        assert_eq!(ten.checked_add(twenty).unwrap(), Amount::from_cents(30));
        assert_eq!(ten.checked_add(twenty).unwrap().to_string(), "0.30");
    }

    #[test]
    fn keeps_signs_exact() {
        let debit = Amount::parse_decimal("-5.00").unwrap();
        let credit = Amount::parse_decimal("+5.00").unwrap();
        assert_eq!(debit.cents(), -500);
        assert!(debit.is_negative());
        assert_eq!(debit.checked_add(credit).unwrap(), Amount::ZERO);
        assert_eq!(debit.to_string(), "-5.00");
        assert_eq!(Amount::parse_decimal("-0.05").unwrap().to_string(), "-0.05");
    }

    #[test]
    fn rejects_fractional_cents() {
        assert_eq!(
            Amount::parse_decimal("0.001"),
            Err(DomainError::FractionalCent {
                input: "0.001".to_owned()
            })
        );
        assert_eq!(
            Amount::parse_decimal("1.000"),
            Err(DomainError::FractionalCent {
                input: "1.000".to_owned()
            })
        );
    }

    #[test]
    fn rejects_malformed_decimals() {
        for input in ["", ".", ".50", "1.", "1,000.00", "12a", "1.2.3", "--1"] {
            assert!(
                matches!(
                    Amount::parse_decimal(input),
                    Err(DomainError::InvalidAmount { .. })
                ),
                "expected `{input}` to be rejected"
            );
        }
    }

    #[test]
    fn rejects_overflow() {
        let max = Amount::parse_decimal("92233720368547758.07").unwrap();
        assert_eq!(max, Amount::MAX);
        assert_eq!(
            Amount::parse_decimal("92233720368547758.08"),
            Err(DomainError::AmountOverflow)
        );
        assert_eq!(
            Amount::parse_decimal("9223372036854775808"),
            Err(DomainError::AmountOverflow)
        );
        assert_eq!(
            max.checked_add(Amount::from_cents(1)),
            Err(DomainError::AmountOverflow)
        );
        assert_eq!(
            Amount::MIN.checked_sub(Amount::from_cents(1)),
            Err(DomainError::AmountOverflow)
        );
    }

    #[test]
    fn round_trips_cents() {
        for cents in [0, 1, -1, 999, -12345, i64::MAX, i64::MIN] {
            let amount = Amount::from_cents(cents);
            assert_eq!(Amount::parse_decimal(&amount.to_string()).unwrap(), amount);
        }
    }
}
