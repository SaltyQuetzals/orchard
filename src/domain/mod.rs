//! Exact money, typed identifiers, budget-local dates, and exhaustive state enums.

mod date;
mod error;
mod ids;
mod money;
mod types;

pub use date::BudgetDate;
pub use error::DomainError;
pub use ids::{
    AccountId, AllocationId, CategoryGroupId, CategoryId, ConnectionId, EvidenceId, JobId,
    ScheduleId, SourceId, TransactionId,
};
pub use money::Amount;
pub use types::{
    AccountKind, BudgetAccountKind, ClearedState, TransactionKind, TransactionOrigin,
};
