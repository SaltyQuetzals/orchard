use std::error::Error;
use std::fmt;

use crate::db::DbError;
use crate::domain::{BudgetMonth, CategoryId, DomainError};
use crate::ledger::LedgerError;

/// Failures from envelope-budget commands and projections.
#[derive(Debug)]
pub enum BudgetError {
    Database(DbError),
    Domain(DomainError),
    Ledger(LedgerError),
    CorruptRow {
        table: &'static str,
        detail: String,
    },
    CategoryNotFound {
        id: CategoryId,
    },
    ArchivedCategory {
        id: CategoryId,
    },
    AssignmentNotFound {
        month: BudgetMonth,
        category: CategoryId,
    },
    DuplicateAssignment {
        month: BudgetMonth,
        category: CategoryId,
    },
    ZeroAssignment,
    MoveNotFound {
        id: i64,
    },
    InvalidMoveAmount,
    SameCategoryMove,
    TargetNotFound {
        category: CategoryId,
    },
    DuplicateTarget {
        category: CategoryId,
    },
    InvalidTargetAmount,
    StaleRevision {
        entity: &'static str,
        id: i64,
        expected: i64,
        actual: i64,
    },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(source) => source.fmt(f),
            Self::Domain(source) => source.fmt(f),
            Self::Ledger(source) => source.fmt(f),
            Self::CorruptRow { table, detail } => {
                write!(f, "stored {table} row is invalid: {detail}")
            }
            Self::CategoryNotFound { id } => write!(f, "category {id} does not exist"),
            Self::ArchivedCategory { id } => write!(
                f,
                "category {id} is archived and cannot receive new budget money"
            ),
            Self::AssignmentNotFound { month, category } => {
                write!(f, "category {category} has no assignment in {month}")
            }
            Self::DuplicateAssignment { month, category } => write!(
                f,
                "category {category} already has an assignment in {month}; pass its revision to change it"
            ),
            Self::ZeroAssignment => f.write_str("an assignment must not be zero"),
            Self::MoveNotFound { id } => write!(f, "budget move {id} does not exist"),
            Self::InvalidMoveAmount => {
                f.write_str("a category move requires a positive amount")
            }
            Self::SameCategoryMove => {
                f.write_str("a category move requires two different categories")
            }
            Self::TargetNotFound { category } => {
                write!(f, "category {category} has no savings target")
            }
            Self::DuplicateTarget { category } => write!(
                f,
                "category {category} already has a savings target; pass its revision to change it"
            ),
            Self::InvalidTargetAmount => {
                f.write_str("a savings target requires a positive amount")
            }
            Self::StaleRevision {
                entity,
                id,
                expected,
                actual,
            } => write!(
                f,
                "{entity} {id} changed since it was loaded (expected revision {expected}, found {actual})"
            ),
        }
    }
}

impl Error for BudgetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(source) => Some(source),
            Self::Domain(source) => Some(source),
            Self::Ledger(source) => Some(source),
            _ => None,
        }
    }
}

impl From<DbError> for BudgetError {
    fn from(source: DbError) -> Self {
        Self::Database(source)
    }
}

impl From<DomainError> for BudgetError {
    fn from(source: DomainError) -> Self {
        Self::Domain(source)
    }
}

impl From<LedgerError> for BudgetError {
    fn from(source: LedgerError) -> Self {
        Self::Ledger(source)
    }
}

impl From<rusqlite::Error> for BudgetError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Database(DbError::from(source))
    }
}
