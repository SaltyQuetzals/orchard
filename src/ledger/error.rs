use std::error::Error;
use std::fmt;

use crate::db::DbError;
use crate::domain::{
    AccountId, Amount, CategoryGroupId, CategoryId, ConnectionId, DomainError, SourceId,
    TransactionId, TransactionKind,
};

/// Failures from ledger commands. Variants are exhaustive so callers cannot
/// accidentally treat a stale write or a financial mismatch as success.
#[derive(Debug)]
pub enum LedgerError {
    Database(DbError),
    Domain(DomainError),
    CorruptRow {
        table: &'static str,
        detail: String,
    },
    AccountNotFound {
        id: AccountId,
    },
    AccountClosed {
        id: AccountId,
    },
    AccountAlreadyClosed {
        id: AccountId,
    },
    NonzeroAccountBalance {
        id: AccountId,
        balance: Amount,
    },
    TrackingAccountOrdinaryActivity {
        id: AccountId,
    },
    InvalidAccountName,
    InvalidCategoryName,
    InvalidCategoryGroupName,
    InvalidPayee,
    InvalidMemo,
    InvalidExternalIdentity,
    CategoryGroupNotFound {
        id: CategoryGroupId,
    },
    ArchivedCategoryGroup {
        id: CategoryGroupId,
    },
    CategoryNotFound {
        id: CategoryId,
    },
    ArchivedCategory {
        id: CategoryId,
    },
    DuplicateCategoryGroupName {
        name: String,
    },
    DuplicateCategoryName {
        name: String,
    },
    TransactionNotFound {
        id: TransactionId,
    },
    StaleRevision {
        entity: &'static str,
        id: i64,
        expected: i64,
        actual: i64,
    },
    ReservedTransactionKind {
        kind: TransactionKind,
    },
    InvalidAmountSign {
        kind: TransactionKind,
        amount: Amount,
    },
    ZeroAmount,
    IncomeCannotHaveAllocations,
    AllocationSumMismatch {
        amount: Amount,
        allocations: Amount,
    },
    ZeroAllocation,
    AllocationSignMismatch {
        kind: TransactionKind,
    },
    UseTransferCommands,
    TransferNotFound {
        id: i64,
    },
    TransferAccountsMustDiffer,
    TrackingToTrackingTransfer,
    BoundaryTargetRequired,
    BoundaryTargetNotAllowed,
    InvalidTransferAmount,
    ReconciliationNotFound {
        id: i64,
    },
    ReconciliationNotOpen {
        id: i64,
    },
    ReconciliationMismatch {
        difference: Amount,
    },
    NoAdjustmentNeeded,
    ReconciliationRequiresBudgetAccount {
        id: AccountId,
    },
    ReconciliationRequired,
    ReconciledEditRequiresConfirmation {
        id: TransactionId,
    },
    TransferReconciledEditRequiresConfirmation {
        id: i64,
    },
    SourceNotFound {
        id: SourceId,
    },
    SourceDeleted {
        id: SourceId,
    },
    SourceDismissed {
        id: SourceId,
    },
    SourceHasNoEntry {
        id: SourceId,
    },
    SourceAlreadyMatched {
        id: SourceId,
        transaction: TransactionId,
    },
    TransactionAlreadyMatched {
        id: TransactionId,
    },
    MatchAccountMismatch,
    MatchAmountMismatch {
        source: Amount,
        transaction: Amount,
    },
    CorrectionNotFound {
        id: i64,
    },
    CorrectionNotPending {
        id: i64,
    },
    CorrectionRequiresManualEdit {
        id: TransactionId,
    },
    ProviderConnectionNotFound {
        id: ConnectionId,
    },
    ValuationRequiresTrackingAccount {
        id: AccountId,
    },
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(source) => source.fmt(f),
            Self::Domain(source) => source.fmt(f),
            Self::CorruptRow { table, detail } => {
                write!(f, "stored {table} row is invalid: {detail}")
            }
            Self::AccountNotFound { id } => write!(f, "account {id} does not exist"),
            Self::AccountClosed { id } => write!(f, "account {id} is closed"),
            Self::AccountAlreadyClosed { id } => write!(f, "account {id} is already closed"),
            Self::NonzeroAccountBalance { id, balance } => write!(
                f,
                "account {id} has ledger balance {balance}; a budget account must reach zero before closing"
            ),
            Self::TrackingAccountOrdinaryActivity { id } => write!(
                f,
                "account {id} is an off-budget tracking account and accepts only transfer references and valuations"
            ),
            Self::InvalidAccountName => {
                f.write_str("the account name must contain 1 to 120 characters")
            }
            Self::InvalidCategoryName => {
                f.write_str("the category name must contain 1 to 120 characters")
            }
            Self::InvalidCategoryGroupName => {
                f.write_str("the category group name must contain 1 to 120 characters")
            }
            Self::InvalidPayee => f.write_str("the payee must contain at most 200 characters"),
            Self::InvalidMemo => f.write_str("the memo must contain at most 1000 characters"),
            Self::InvalidExternalIdentity => {
                f.write_str("provider connection and transaction identifiers must be nonblank")
            }
            Self::CategoryGroupNotFound { id } => {
                write!(f, "category group {id} does not exist")
            }
            Self::ArchivedCategoryGroup { id } => {
                write!(f, "category group {id} is archived")
            }
            Self::CategoryNotFound { id } => write!(f, "category {id} does not exist"),
            Self::ArchivedCategory { id } => {
                write!(f, "category {id} is archived and cannot receive new allocations")
            }
            Self::DuplicateCategoryGroupName { name } => {
                write!(f, "a category group named `{name}` already exists")
            }
            Self::DuplicateCategoryName { name } => {
                write!(f, "a category named `{name}` already exists in that group")
            }
            Self::TransactionNotFound { id } => write!(f, "transaction {id} does not exist"),
            Self::StaleRevision {
                entity,
                id,
                expected,
                actual,
            } => write!(
                f,
                "{entity} {id} changed since it was loaded (expected revision {expected}, found {actual})"
            ),
            Self::ReservedTransactionKind { kind } => write!(
                f,
                "transaction kind {kind:?} is created by a dedicated command, not an ordinary entry"
            ),
            Self::InvalidAmountSign { kind, amount } => write!(
                f,
                "amount {amount} is not valid for a {kind:?} entry (income and refunds are positive; outflows are negative)"
            ),
            Self::ZeroAmount => f.write_str("an amount must not be zero"),
            Self::IncomeCannotHaveAllocations => {
                f.write_str("income entries do not carry category allocations")
            }
            Self::AllocationSumMismatch { amount, allocations } => write!(
                f,
                "split allocations total {allocations} but the entry amount is {amount}"
            ),
            Self::ZeroAllocation => f.write_str("a split allocation must not be zero"),
            Self::AllocationSignMismatch { kind } => {
                write!(f, "a {kind:?} entry's allocations must share its sign")
            }
            Self::UseTransferCommands => {
                f.write_str("transfer legs must be changed with the linked-transfer commands")
            }
            Self::TransferNotFound { id } => write!(f, "transfer {id} does not exist"),
            Self::TransferAccountsMustDiffer => {
                f.write_str("a transfer requires two different accounts")
            }
            Self::TrackingToTrackingTransfer => {
                f.write_str("transfers between two tracking accounts are not supported")
            }
            Self::BoundaryTargetRequired => {
                f.write_str("a budget/tracking transfer requires a category or Ready to Assign target")
            }
            Self::BoundaryTargetNotAllowed => {
                f.write_str("a budget-internal transfer does not take a category or Ready to Assign target")
            }
            Self::InvalidTransferAmount => {
                f.write_str("the transfer amount must be a positive magnitude")
            }
            Self::ReconciliationNotFound { id } => {
                write!(f, "reconciliation {id} does not exist")
            }
            Self::ReconciliationNotOpen { id } => {
                write!(f, "reconciliation {id} is not open")
            }
            Self::ReconciliationMismatch { difference } => write!(
                f,
                "cleared activity differs from the statement by {difference}; correct entries or record an explicit adjustment"
            ),
            Self::NoAdjustmentNeeded => {
                f.write_str("cleared activity already matches the statement balance")
            }
            Self::ReconciliationRequiresBudgetAccount { id } => write!(
                f,
                "account {id} is off-budget; tracking accounts use valuations instead of statement reconciliation"
            ),
            Self::ReconciliationRequired => {
                f.write_str("marking an entry reconciled requires a statement reconciliation")
            }
            Self::ReconciledEditRequiresConfirmation { id } => write!(
                f,
                "transaction {id} is reconciled; confirm the edit to invalidate its reconciliation"
            ),
            Self::TransferReconciledEditRequiresConfirmation { id } => write!(
                f,
                "transfer {id} has a reconciled leg; confirm the change to invalidate its reconciliation"
            ),
            Self::SourceNotFound { id } => write!(f, "import source {id} does not exist"),
            Self::SourceDeleted { id } => {
                write!(f, "import source {id} is deleted and cannot be changed")
            }
            Self::SourceDismissed { id } => {
                write!(f, "import source {id} is dismissed and cannot be changed")
            }
            Self::SourceHasNoEntry { id } => {
                write!(f, "import source {id} has no linked ledger entry")
            }
            Self::SourceAlreadyMatched { id, transaction } => write!(
                f,
                "import source {id} is already matched to transaction {transaction}"
            ),
            Self::TransactionAlreadyMatched { id } => {
                write!(f, "transaction {id} is already matched to another import source")
            }
            Self::MatchAccountMismatch => {
                f.write_str("import matching requires the same account")
            }
            Self::MatchAmountMismatch {
                source,
                transaction,
            } => write!(
                f,
                "import matching requires the exact signed amount (source {source}, transaction {transaction})"
            ),
            Self::CorrectionNotFound { id } => {
                write!(f, "import correction {id} does not exist")
            }
            Self::CorrectionNotPending { id } => {
                write!(f, "import correction {id} is not pending")
            }
            Self::CorrectionRequiresManualEdit { id } => write!(
                f,
                "transaction {id} has multiple allocations; edit its splits before applying the provider correction"
            ),
            Self::ProviderConnectionNotFound { id } => {
                write!(f, "provider connection {id} does not exist")
            }
            Self::ValuationRequiresTrackingAccount { id } => write!(
                f,
                "account {id} is a budget account; valuations apply only to off-budget tracking accounts"
            ),
        }
    }
}

impl Error for LedgerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(source) => Some(source),
            Self::Domain(source) => Some(source),
            _ => None,
        }
    }
}

impl From<DbError> for LedgerError {
    fn from(source: DbError) -> Self {
        Self::Database(source)
    }
}

impl From<DomainError> for LedgerError {
    fn from(source: DomainError) -> Self {
        Self::Domain(source)
    }
}

impl From<rusqlite::Error> for LedgerError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Database(DbError::from(source))
    }
}
