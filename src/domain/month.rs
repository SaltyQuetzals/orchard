use std::fmt;
use std::str::FromStr;

use super::{BudgetDate, DomainError};

/// A calendar month in the budget's local timezone, stored as `YYYY-MM`.
///
/// Months are the unit owners assign money to. They order chronologically and
/// always map to a real first and last calendar day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BudgetMonth {
    year: i32,
    month: u8,
}

impl BudgetMonth {
    pub fn from_ym(year: i32, month: u8) -> Result<Self, DomainError> {
        if !(1..=12).contains(&month) || !(-9999..=9999).contains(&year) {
            return Err(DomainError::InvalidMonth {
                input: format!("{year:04}-{month:02}"),
            });
        }
        Ok(Self { year, month })
    }

    /// The month containing a budget-local calendar date.
    pub fn containing(date: BudgetDate) -> Self {
        Self {
            year: date.year(),
            month: date.month(),
        }
    }

    pub const fn year(self) -> i32 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    /// The next calendar month, or `None` past the supported year range.
    pub fn next(self) -> Option<Self> {
        let (year, month) = if self.month == 12 {
            (self.year.checked_add(1)?, 1)
        } else {
            (self.year, self.month + 1)
        };
        Self::from_ym(year, month).ok()
    }

    /// The previous calendar month, or `None` before the supported year range.
    pub fn previous(self) -> Option<Self> {
        let (year, month) = if self.month == 1 {
            (self.year.checked_sub(1)?, 12)
        } else {
            (self.year, self.month - 1)
        };
        Self::from_ym(year, month).ok()
    }

    pub fn first_day(self) -> BudgetDate {
        BudgetDate::from_ymd(self.year, self.month, 1)
            .expect("budget months are validated on construction")
    }

    pub fn last_day(self) -> BudgetDate {
        BudgetDate::from_ymd(self.year, self.month, days_in_month(self.year, self.month))
            .expect("budget months are validated on construction")
    }

    /// Signed number of whole months from `self` to `later`.
    pub const fn months_until(self, later: Self) -> i64 {
        (later.year as i64 - self.year as i64) * 12 + (later.month as i64 - self.month as i64)
    }
}

impl fmt::Display for BudgetMonth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

impl FromStr for BudgetMonth {
    type Err = DomainError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bytes = input.as_bytes();
        let invalid = || DomainError::InvalidMonth {
            input: input.to_owned(),
        };
        if bytes.len() != 7 || bytes[4] != b'-' {
            return Err(invalid());
        }
        if !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || byte.is_ascii_digit())
        {
            return Err(invalid());
        }
        let year = input[0..4].parse::<i32>().map_err(|_| invalid())?;
        let month = input[5..7].parse::<u8>().map_err(|_| invalid())?;
        Self::from_ym(year, month)
    }
}

const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
    }
}

const fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_months() {
        let month: BudgetMonth = "2026-11".parse().unwrap();
        assert_eq!((month.year(), month.month()), (2026, 11));
        assert_eq!(month.to_string(), "2026-11");
        assert_eq!(BudgetMonth::containing("2026-11-10".parse().unwrap()), month);
    }

    #[test]
    fn rejects_malformed_months() {
        for input in ["2026-13", "2026-00", "2026-1", "202611", "2026/11", "20a6-11"] {
            assert!(
                matches!(
                    input.parse::<BudgetMonth>(),
                    Err(DomainError::InvalidMonth { .. })
                ),
                "expected `{input}` to be rejected"
            );
        }
    }

    #[test]
    fn advances_across_year_boundaries() {
        let december = BudgetMonth::from_ym(2026, 12).unwrap();
        assert_eq!(december.next().unwrap(), BudgetMonth::from_ym(2027, 1).unwrap());
        assert_eq!(december.next().unwrap().previous().unwrap(), december);
    }

    #[test]
    fn clamps_month_ends_and_counts_months() {
        let february = BudgetMonth::from_ym(2026, 2).unwrap();
        assert_eq!(february.last_day().to_string(), "2026-02-28");
        assert_eq!(
            BudgetMonth::from_ym(2024, 2).unwrap().last_day().to_string(),
            "2024-02-29"
        );
        let january = BudgetMonth::from_ym(2026, 1).unwrap();
        let march = BudgetMonth::from_ym(2026, 3).unwrap();
        assert_eq!(january.months_until(march), 2);
        assert_eq!(march.months_until(january), -2);
    }
}
