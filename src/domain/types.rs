/// Whether an account participates in the envelope budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountKind {
    Budget(BudgetAccountKind),
    /// Off-budget asset tracked through dated valuations.
    Tracking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudgetAccountKind {
    Checking,
    Savings,
    Cash,
    CreditCard,
}

impl AccountKind {
    pub const fn is_budget(self) -> bool {
        matches!(self, Self::Budget(_))
    }

    pub const fn is_credit_card(self) -> bool {
        matches!(self, Self::Budget(BudgetAccountKind::CreditCard))
    }
}

/// The accounting meaning of a ledger entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionKind {
    Income,
    Outflow,
    Refund,
    Transfer,
    OpeningBalance,
    Adjustment,
}

impl TransactionKind {
    /// Transfers and opening balances never count as ordinary income or spending.
    pub const fn is_ordinary_activity(self) -> bool {
        matches!(self, Self::Income | Self::Outflow | Self::Refund)
    }

    pub const fn is_transfer(self) -> bool {
        matches!(self, Self::Transfer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClearedState {
    Uncleared,
    Cleared,
    Reconciled,
}

/// How a ledger entry came to exist. Imported entries are posted immediately;
/// opening and reconciliation-adjustment entries are explicit system origins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionOrigin {
    Manual,
    Import,
    Opening,
    Adjustment,
}
