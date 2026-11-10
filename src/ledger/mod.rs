//! Account ledger commands: accounts, exact dated entries, splits, linked
//! transfers, reconciliation, import review, and tracking valuations.
//!
//! Commands are the only supported mutation path. Each one validates domain
//! invariants and runs in a single SQL transaction, so a rejected command
//! leaves no partial financial state.

mod accounts;
mod categories;
mod entries;
mod error;
mod imports;
mod queries;
mod reconciliation;
mod rows;
mod transfers;
mod types;

pub use accounts::{
    account, account_balance, account_kind, accounts, close_account, create_account,
};
pub use categories::{
    archive_category, categories, category, category_groups, create_category,
    create_category_group,
};
pub use entries::{
    create_transaction, delete_transaction, edit_transaction, set_cleared_state, transaction,
    transactions_for_account,
};
pub use error::LedgerError;
pub use imports::{
    apply_source_correction, approve_source, create_provider_connection, delete_source,
    dismiss_source, dismiss_source_correction, ingest_source, match_candidates, match_source,
    source, source_corrections,
};
pub use queries::{
    latest_valuation, net_worth_tracking_total, outstanding_category_work, recognized_income,
    record_valuation,
};
pub use reconciliation::{
    compare_statement, complete_reconciliation, create_reconciliation_adjustment, reconciliation,
    reconciliations_for_account,
};
pub use transfers::{create_transfer, delete_transfer, edit_transfer, transfer};
pub use types::*;

pub use crate::domain::{
    AccountId, AccountKind, AllocationId, Amount, BudgetAccountKind, BudgetDate, CategoryGroupId,
    CategoryId, ClearedState, ConnectionId, SourceId, TransactionId, TransactionKind,
    TransactionOrigin,
};
