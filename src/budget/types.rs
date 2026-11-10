use crate::domain::{
    AccountId, Amount, BudgetDate, BudgetMonth, CategoryGroupId, CategoryId, DomainError,
};

/// One month's assignment of money from Ready to Assign to a category.
///
/// A positive amount moves money into the category; a negative amount returns
/// money to Ready to Assign. A month holds at most one assignment record per
/// category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetAssignment {
    pub id: i64,
    pub month: BudgetMonth,
    pub category_id: CategoryId,
    pub amount: Amount,
    pub revision: i64,
}

/// One category-to-category move. Moves conserve money: the amount leaves one
/// category and arrives in the other in the same month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetMove {
    pub id: i64,
    pub month: BudgetMonth,
    pub from_category_id: CategoryId,
    pub to_category_id: CategoryId,
    pub amount: Amount,
    pub revision: i64,
}

/// A savings target attached to a category. Targets never assign money; they
/// only report progress and a suggested contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryTarget {
    /// Reach `amount` available in `month`.
    BalanceByMonth { amount: Amount, month: BudgetMonth },
    /// Assign `amount` in every month.
    MonthlyContribution { amount: Amount },
}

impl CategoryTarget {
    pub const fn amount(self) -> Amount {
        match self {
            Self::BalanceByMonth { amount, .. } | Self::MonthlyContribution { amount } => amount,
        }
    }
}

/// A stored target with its optimistic-concurrency revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategoryTargetRecord {
    pub category_id: CategoryId,
    pub target: CategoryTarget,
    pub revision: i64,
}

/// Which measure a target's progress uses. The two variants are deliberately
/// distinct so a balance target is never confused with contribution progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetProgressMeasure {
    /// Dated balance target: progress is the category's available balance.
    Balance {
        available: Amount,
        remaining: Amount,
        months_remaining: i64,
    },
    /// Monthly contribution target: progress is this month's assignments.
    Contribution { assigned: Amount, remaining: Amount },
}

/// Progress for a category target in the viewed month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetProgress {
    pub target: CategoryTarget,
    pub measure: TargetProgressMeasure,
    /// Cent-exact amount the owner could assign per remaining month. Advisory
    /// only; no assignment is created.
    pub suggested_contribution: Amount,
}

/// A category row in a monthly budget view, including the system Uncategorized
/// bucket (`category_id: None`), which is never income and receives no
/// assignments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetCategoryView {
    pub category_id: Option<CategoryId>,
    pub group_id: Option<CategoryGroupId>,
    pub name: String,
    pub position: i64,
    pub archived: bool,
    /// Positive availability carried in from the prior month.
    pub carried_in: Amount,
    /// Net money assigned this month (assignment plus incoming moves minus
    /// outgoing moves).
    pub assigned: Amount,
    /// Signed activity this month: outflows negative, refunds positive.
    pub activity: Amount,
    /// End-of-month availability. Negative amounts are current-month deficits.
    pub available: Amount,
    /// Uncovered cash spending this month. Rolls into next month's Ready to
    /// Assign exactly once.
    pub cash_overspending: Amount,
    /// Uncovered card spending this month. Becomes card debt and never reduces
    /// cash again.
    pub credit_overspending: Amount,
    pub target: Option<TargetProgress>,
}

/// A category group with its ordered categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetGroupView {
    pub group: crate::ledger::CategoryGroup,
    pub rows: Vec<BudgetCategoryView>,
}

/// One credit card's payment category and reserve position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardPaymentView {
    pub account_id: AccountId,
    pub account_name: String,
    pub category_id: CategoryId,
    pub carried_in: Amount,
    /// Explicit assignments to the payment category this month.
    pub assigned: Amount,
    /// Automatic reserve added by funded card purchases, net of refund
    /// reversals, this month.
    pub reserved: Amount,
    /// Card payments made from budget accounts this month.
    pub paid: Amount,
    /// Signed payment availability. Negative values are shortfalls that carry
    /// forward until covered.
    pub available: Amount,
    /// The exact uncovered portion of a negative payment availability.
    pub shortfall: Amount,
    pub card_balance: Amount,
}

/// Whether the viewed month is described by actual activity, past activity, or
/// planned assignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetBasis {
    /// The month containing the recognition date.
    Current { as_of: BudgetDate },
    /// A month before the recognition date.
    Historical { as_of: BudgetDate },
    /// A month after the recognition date; activity is not yet actual.
    Forecast { as_of: BudgetDate },
}

impl BudgetBasis {
    pub const fn as_of(self) -> BudgetDate {
        match self {
            Self::Current { as_of } | Self::Historical { as_of } | Self::Forecast { as_of } => {
                as_of
            }
        }
    }
}

/// The design's current-month conservation identity.
///
/// `accounted` sums every budget bucket; `recognized` sums the money those
/// buckets claim. They must be equal when no card cash advance has introduced
/// borrowed cash into a budget account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetConservation {
    pub ready_to_assign: Amount,
    pub ordinary_available: Amount,
    pub card_payment_available: Amount,
    pub uncovered_credit_spending: Amount,
    pub reserved_future_assignments: Amount,
    pub recognized_cash: Amount,
    pub positive_card_assets: Amount,
}

impl BudgetConservation {
    /// `Ready to Assign + ordinary availability + card-payment availability +
    /// uncovered current credit spending + reserved future assignments`.
    pub fn accounted(self) -> Result<Amount, DomainError> {
        let mut total = self.ready_to_assign;
        for part in [
            self.ordinary_available,
            self.card_payment_available,
            self.uncovered_credit_spending,
            self.reserved_future_assignments,
        ] {
            total = total.checked_add(part)?;
        }
        Ok(total)
    }

    /// `recognized budget cash + positive card assets`.
    pub fn recognized(self) -> Result<Amount, DomainError> {
        self.recognized_cash
            .checked_add(self.positive_card_assets)
    }

    pub fn is_balanced(self) -> Result<bool, DomainError> {
        Ok(self.accounted()? == self.recognized()?)
    }
}

/// A complete monthly budget projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetMonthView {
    pub month: BudgetMonth,
    pub basis: BudgetBasis,
    /// Money still unassigned after reserving future months.
    pub ready_to_assign: Amount,
    /// Money already assigned to months after this one.
    pub reserved_future_assignments: Amount,
    /// Sum of every ordinary category's availability, including negative and
    /// Uncategorized amounts.
    pub ordinary_available: Amount,
    /// Uncovered card spending in this month.
    pub uncovered_credit_spending: Amount,
    pub groups: Vec<BudgetGroupView>,
    pub uncategorized: BudgetCategoryView,
    pub card_payments: Vec<CardPaymentView>,
    /// Present only for the current month, where every component refers to the
    /// same recognition date.
    pub conservation: Option<BudgetConservation>,
}

impl BudgetMonthView {
    /// Finds a viewing row by category, including the Uncategorized bucket.
    pub fn category(&self, category_id: Option<CategoryId>) -> Option<&BudgetCategoryView> {
        self.groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .find(|row| row.category_id == category_id)
    }

    pub fn card(&self, account_id: AccountId) -> Option<&CardPaymentView> {
        self.card_payments
            .iter()
            .find(|card| card.account_id == account_id)
    }
}

/// Smaller of two signed amounts, used by funding allocation.
pub(super) fn min_amount(left: Amount, right: Amount) -> Amount {
    if left.cents() <= right.cents() {
        left
    } else {
        right
    }
}
