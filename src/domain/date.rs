use std::fmt;
use std::str::FromStr;

use time::{Date, Month, OffsetDateTime, UtcOffset};

use super::DomainError;

/// A calendar date in the budget's local timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BudgetDate(Date);

impl BudgetDate {
    pub fn from_ymd(year: i32, month: u8, day: u8) -> Result<Self, DomainError> {
        let month = Month::try_from(month).map_err(|_| DomainError::InvalidDate {
            input: format!("{year:04}-{month:02}-{day:02}"),
        })?;
        Date::from_calendar_date(year, month, day)
            .map(Self)
            .map_err(|_| DomainError::InvalidDate {
                input: format!("{year:04}-{month:02}-{day:02}"),
            })
    }

    pub const fn from_date(date: Date) -> Self {
        Self(date)
    }

    pub const fn date(self) -> Date {
        self.0
    }

    pub fn today_in(offset: UtcOffset) -> Self {
        Self(OffsetDateTime::now_utc().to_offset(offset).date())
    }

    pub const fn year(self) -> i32 {
        self.0.year()
    }

    pub fn month(self) -> u8 {
        u8::from(self.0.month())
    }

    pub const fn day(self) -> u8 {
        self.0.day()
    }
}

impl fmt::Display for BudgetDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}",
            self.0.year(),
            self.month(),
            self.0.day()
        )
    }
}

impl FromStr for BudgetDate {
    type Err = DomainError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bytes = input.as_bytes();
        let invalid = || DomainError::InvalidDate {
            input: input.to_owned(),
        };
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(invalid());
        }
        if !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
        {
            return Err(invalid());
        }

        let year = input[0..4].parse::<i32>().map_err(|_| invalid())?;
        let month = input[5..7].parse::<u8>().map_err(|_| invalid())?;
        let day = input[8..10].parse::<u8>().map_err(|_| invalid())?;
        Self::from_ymd(year, month, day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_iso_dates() {
        let date: BudgetDate = "2026-11-10".parse().unwrap();
        assert_eq!((date.year(), date.month(), date.day()), (2026, 11, 10));
        assert_eq!(date.to_string(), "2026-11-10");
    }

    #[test]
    fn rejects_impossible_dates() {
        for input in [
            "2026-02-30",
            "2026-13-01",
            "2026-00-10",
            "2026-1-01",
            "2026/11/10",
            "20261110",
            "2026-11-1x",
        ] {
            assert!(
                matches!(
                    input.parse::<BudgetDate>(),
                    Err(DomainError::InvalidDate { .. })
                ),
                "expected `{input}` to be rejected"
            );
        }
    }

    #[test]
    fn leap_days_are_calendar_exact() {
        assert!("2024-02-29".parse::<BudgetDate>().is_ok());
        assert!("2025-02-29".parse::<BudgetDate>().is_err());
    }
}
