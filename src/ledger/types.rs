use crate::domain::{
    AccountId, AccountKind, Amount, BudgetAccountKind, BudgetDate, CategoryGroupId, CategoryId,
    ClearedState, ConnectionId, SourceId, TransactionId, TransactionKind, TransactionOrigin,
};

use super::LedgerError;

/// An account as stored in the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    pub name: String,
    pub kind: AccountKind,
    /// Per-card payment category created with a credit-card account.
    pub payment_category_id: Option<CategoryId>,
    pub closed_on: Option<BudgetDate>,
    pub revision: i64,
}

/// Input for opening an account. A nonzero opening amount becomes an explicit
/// opening-balance entry rather than income.
#[derive(Debug, Clone, Copy)]
pub struct NewAccount<'a> {
    pub name: &'a str,
    pub kind: AccountKind,
    pub opened_on: BudgetDate,
    pub opening_balance: Amount,
}

/// An ordered category group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryGroup {
    pub id: CategoryGroupId,
    pub name: String,
    pub position: i64,
    pub archived: bool,
}

/// A spending or income category. Archived categories remain valid on
/// historical allocations but reject new ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: CategoryId,
    pub group_id: CategoryGroupId,
    pub name: String,
    pub position: i64,
    pub archived: bool,
}

/// One signed split allocation. `None` means the allocation is not yet
/// categorized and therefore keeps category work outstanding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub id: crate::domain::AllocationId,
    pub category_id: Option<CategoryId>,
    pub amount: Amount,
}

/// A new split allocation supplied by a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewAllocation {
    pub category_id: Option<CategoryId>,
    pub amount: Amount,
}

/// A ledger entry with its allocations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionRecord {
    pub id: TransactionId,
    pub account_id: AccountId,
    pub kind: TransactionKind,
    pub origin: TransactionOrigin,
    pub date: BudgetDate,
    pub payee: String,
    pub memo: String,
    pub amount: Amount,
    pub cleared: ClearedState,
    pub revision: i64,
    pub transfer_id: Option<i64>,
    pub allocations: Vec<Allocation>,
}

/// A new dated income, outflow, or refund entry.
#[derive(Debug, Clone)]
pub struct NewTransaction<'a> {
    pub account_id: AccountId,
    pub kind: TransactionKind,
    pub date: BudgetDate,
    pub payee: &'a str,
    pub memo: &'a str,
    pub amount: Amount,
    /// Empty for income. Empty for an outflow/refund means one uncategorized
    /// allocation for the full amount.
    pub allocations: Vec<NewAllocation>,
}

/// Revision-checked edit of a non-transfer entry. `allocations: None` keeps the
/// existing splits and only succeeds when they still total the new amount.
#[derive(Debug, Clone)]
pub struct EditTransaction<'a> {
    pub date: BudgetDate,
    pub payee: &'a str,
    pub memo: &'a str,
    pub amount: Amount,
    pub allocations: Option<Vec<NewAllocation>>,
    pub confirm_reconciled: bool,
}

/// A budget-side target for a transfer that crosses the budget/tracking
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryTarget {
    ReadyToAssign,
    Category(CategoryId),
}

/// Whether a transfer stays inside the budget or crosses into tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferBoundary {
    Budget,
    BudgetTracking,
}

/// A linked pair of equal-and-opposite transfer entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRecord {
    pub id: i64,
    pub from: TransactionRecord,
    pub to: TransactionRecord,
    pub boundary: TransferBoundary,
    pub boundary_target: Option<BoundaryTarget>,
    pub created_at: i64,
}

/// Input for a new linked transfer. `amount` is a positive magnitude; the
/// command writes the negative leg to `from_account_id`.
#[derive(Debug, Clone, Copy)]
pub struct NewTransfer<'a> {
    pub from_account_id: AccountId,
    pub to_account_id: AccountId,
    pub amount: Amount,
    pub date: BudgetDate,
    pub payee: &'a str,
    pub memo: &'a str,
    pub boundary: Option<BoundaryTarget>,
}

/// Revision-checked edit of both transfer legs and the boundary target.
#[derive(Debug, Clone, Copy)]
pub struct EditTransfer<'a> {
    pub amount: Amount,
    pub date: BudgetDate,
    pub payee: &'a str,
    pub memo: &'a str,
    pub boundary: Option<BoundaryTarget>,
    pub expected_from_revision: i64,
    pub expected_to_revision: i64,
    pub confirm_reconciled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationStatus {
    Open,
    Completed,
    Invalidated,
}

/// A statement comparison. A nonzero difference is visible and never balanced
/// automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationRecord {
    pub id: i64,
    pub account_id: AccountId,
    pub statement_date: BudgetDate,
    pub statement_balance: Amount,
    pub cleared_balance: Amount,
    pub difference: Amount,
    pub adjustment_transaction_id: Option<TransactionId>,
    pub status: ReconciliationStatus,
    pub revision: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    PendingReview,
    Approved,
    Dismissed,
    Deleted,
}

/// An imported provider record. Its external identity is retained even after
/// the entry is dismissed or deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub id: SourceId,
    pub connection_id: ConnectionId,
    pub institution_connection_id: String,
    pub external_account_id: String,
    pub external_transaction_id: String,
    pub account_id: AccountId,
    pub date: BudgetDate,
    pub amount: Amount,
    pub payee: String,
    pub memo: String,
    pub status: SourceStatus,
    pub transaction_id: Option<TransactionId>,
    pub revision: i64,
}

/// Input for ingesting one posted provider record.
#[derive(Debug, Clone, Copy)]
pub struct NewSource<'a> {
    pub connection_id: ConnectionId,
    pub institution_connection_id: &'a str,
    pub external_account_id: &'a str,
    pub external_transaction_id: &'a str,
    pub account_id: AccountId,
    pub date: BudgetDate,
    pub amount: Amount,
    pub payee: &'a str,
    pub memo: &'a str,
}

/// Result of ingesting a provider record. Repeats reuse the existing source and
/// never post the money again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIngest {
    pub source: SourceRecord,
    pub created_entry: bool,
    pub correction: Option<SourceCorrection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionStatus {
    Pending,
    Applied,
    Dismissed,
}

/// A provider amount/date change awaiting owner review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCorrection {
    pub id: i64,
    pub source_id: SourceId,
    pub previous_amount: Amount,
    pub previous_date: BudgetDate,
    pub proposed_amount: Amount,
    pub proposed_date: BudgetDate,
    pub status: CorrectionStatus,
}

/// A same-account, exact-amount transaction that could be matched to an import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchCandidate {
    pub transaction_id: TransactionId,
    pub date: BudgetDate,
    pub payee: String,
    pub amount: Amount,
    pub revision: i64,
}

/// Outstanding category work derived from allocations that still have no
/// category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryWork {
    pub transaction_id: TransactionId,
    pub account_id: AccountId,
    pub date: BudgetDate,
    pub amount: Amount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValuationSource {
    Provider,
    Manual,
}

/// A dated valuation for an off-budget tracking account. Transfer references
/// never add to this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Valuation {
    pub account_id: AccountId,
    pub as_of_date: BudgetDate,
    pub value: Amount,
    pub observed_at: i64,
    pub source: ValuationSource,
}

/// Latest-valuation total for tracking accounts. Accounts without a valuation
/// are reported as missing rather than silently counted as zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackingTotal {
    pub total: Amount,
    pub missing: Vec<AccountId>,
}

impl AccountKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Budget(BudgetAccountKind::Checking) => "checking",
            Self::Budget(BudgetAccountKind::Savings) => "savings",
            Self::Budget(BudgetAccountKind::Cash) => "cash",
            Self::Budget(BudgetAccountKind::CreditCard) => "credit_card",
            Self::Tracking => "tracking",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "checking" => Ok(Self::Budget(BudgetAccountKind::Checking)),
            "savings" => Ok(Self::Budget(BudgetAccountKind::Savings)),
            "cash" => Ok(Self::Budget(BudgetAccountKind::Cash)),
            "credit_card" => Ok(Self::Budget(BudgetAccountKind::CreditCard)),
            "tracking" => Ok(Self::Tracking),
            other => Err(LedgerError::CorruptRow {
                table: "accounts",
                detail: format!("unknown account kind `{other}`"),
            }),
        }
    }
}

impl TransactionKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Income => "income",
            Self::Outflow => "outflow",
            Self::Refund => "refund",
            Self::Transfer => "transfer",
            Self::OpeningBalance => "opening_balance",
            Self::Adjustment => "adjustment",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "income" => Ok(Self::Income),
            "outflow" => Ok(Self::Outflow),
            "refund" => Ok(Self::Refund),
            "transfer" => Ok(Self::Transfer),
            "opening_balance" => Ok(Self::OpeningBalance),
            "adjustment" => Ok(Self::Adjustment),
            other => Err(LedgerError::CorruptRow {
                table: "transactions",
                detail: format!("unknown transaction kind `{other}`"),
            }),
        }
    }
}

impl TransactionOrigin {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Import => "import",
            Self::Opening => "opening",
            Self::Adjustment => "adjustment",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "manual" => Ok(Self::Manual),
            "import" => Ok(Self::Import),
            "opening" => Ok(Self::Opening),
            "adjustment" => Ok(Self::Adjustment),
            other => Err(LedgerError::CorruptRow {
                table: "transactions",
                detail: format!("unknown transaction origin `{other}`"),
            }),
        }
    }
}

impl ClearedState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Uncleared => "uncleared",
            Self::Cleared => "cleared",
            Self::Reconciled => "reconciled",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "uncleared" => Ok(Self::Uncleared),
            "cleared" => Ok(Self::Cleared),
            "reconciled" => Ok(Self::Reconciled),
            other => Err(LedgerError::CorruptRow {
                table: "transactions",
                detail: format!("unknown cleared state `{other}`"),
            }),
        }
    }
}

impl SourceStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PendingReview => "pending_review",
            Self::Approved => "approved",
            Self::Dismissed => "dismissed",
            Self::Deleted => "deleted",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "pending_review" => Ok(Self::PendingReview),
            "approved" => Ok(Self::Approved),
            "dismissed" => Ok(Self::Dismissed),
            "deleted" => Ok(Self::Deleted),
            other => Err(LedgerError::CorruptRow {
                table: "source_records",
                detail: format!("unknown source status `{other}`"),
            }),
        }
    }
}

impl CorrectionStatus {
    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "pending" => Ok(Self::Pending),
            "applied" => Ok(Self::Applied),
            "dismissed" => Ok(Self::Dismissed),
            other => Err(LedgerError::CorruptRow {
                table: "source_corrections",
                detail: format!("unknown correction status `{other}`"),
            }),
        }
    }
}

impl ReconciliationStatus {
    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "open" => Ok(Self::Open),
            "completed" => Ok(Self::Completed),
            "invalidated" => Ok(Self::Invalidated),
            other => Err(LedgerError::CorruptRow {
                table: "reconciliations",
                detail: format!("unknown reconciliation status `{other}`"),
            }),
        }
    }
}

impl TransferBoundary {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Budget => "budget",
            Self::BudgetTracking => "budget_tracking",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "budget" => Ok(Self::Budget),
            "budget_tracking" => Ok(Self::BudgetTracking),
            other => Err(LedgerError::CorruptRow {
                table: "transfers",
                detail: format!("unknown transfer boundary `{other}`"),
            }),
        }
    }
}

impl ValuationSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Manual => "manual",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, LedgerError> {
        match value {
            "provider" => Ok(Self::Provider),
            "manual" => Ok(Self::Manual),
            other => Err(LedgerError::CorruptRow {
                table: "account_valuations",
                detail: format!("unknown valuation source `{other}`"),
            }),
        }
    }
}
