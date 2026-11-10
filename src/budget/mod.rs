//! Envelope-budget commands and deterministic monthly projection.
//!
//! Assignments and category moves are the only budget-money mutations. Monthly
//! availability, Ready to Assign, cash/credit overspending, card-payment
//! reserves, and target progress are always derived by replaying the ledger and
//! these records forward; no projection is stored as a second source of truth.

mod commands;
mod error;
mod projection;
mod rows;
mod types;

pub use commands::{
    assignments, budget_moves, category_target, category_targets, clear_assignment,
    clear_category_target, delete_budget_move, edit_budget_move, move_budget, set_assignment,
    set_category_target,
};
pub use error::BudgetError;
pub use projection::budget_month;
pub use types::{
    BudgetAssignment, BudgetBasis, BudgetCategoryView, BudgetConservation, BudgetGroupView,
    BudgetMonthView, BudgetMove, CardPaymentView, CategoryTarget, CategoryTargetRecord,
    TargetProgress, TargetProgressMeasure,
};
