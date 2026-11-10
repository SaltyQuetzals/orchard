use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::db::Db;
use crate::domain::{
    AccountId, AccountKind, Amount, BudgetAccountKind, BudgetDate, BudgetMonth, CategoryId,
    TransactionKind,
};
use crate::ledger::{self, Account, BoundaryTarget, Category, CategoryGroup, TransferBoundary};

use super::rows::{load_target, parse_month};
use super::types::min_amount;
use super::{
    BudgetAssignment, BudgetBasis, BudgetCategoryView, BudgetConservation, BudgetError,
    BudgetGroupView, BudgetMonthView, BudgetMove, CardPaymentView, CategoryTarget,
    CategoryTargetRecord, TargetProgress, TargetProgressMeasure,
};

/// Loads the ledger and budget records a projection needs. Transactions after
/// the recognition date are excluded so future dates never fund today.
pub(super) struct ProjectionData {
    transactions: Vec<RowTxn>,
    allocations: HashMap<i64, Vec<RowAlloc>>,
    transfers: HashMap<i64, TransferMeta>,
    assignments: Vec<BudgetAssignment>,
    moves: Vec<BudgetMove>,
    targets: Vec<CategoryTargetRecord>,
}

#[derive(Debug, Clone, Copy)]
struct RowTxn {
    id: i64,
    account_id: AccountId,
    kind: TransactionKind,
    date: BudgetDate,
    amount: Amount,
}

#[derive(Debug, Clone, Copy)]
struct RowAlloc {
    category_id: Option<CategoryId>,
    amount: Amount,
}

#[derive(Debug, Clone, Copy)]
struct TransferMeta {
    boundary: TransferBoundary,
    target: Option<BoundaryTarget>,
}

pub async fn budget_month(
    db: &Db,
    month: BudgetMonth,
    as_of: BudgetDate,
) -> Result<BudgetMonthView, BudgetError> {
    let groups = ledger::category_groups(db).await?;
    let categories = ledger::categories(db).await?;
    let accounts = ledger::accounts(db, true).await?;
    db.run_read(move |conn| {
        let data = load_projection_data(conn, as_of)?;
        project(&groups, &categories, &accounts, &data, month, as_of)
    })
    .await
}

fn load_projection_data(
    conn: &Connection,
    as_of: BudgetDate,
) -> Result<ProjectionData, BudgetError> {
    let transactions = load_transactions(conn, as_of)?;
    let allocations = load_allocations(conn, as_of)?;
    let transfers = load_transfers(conn)?;
    let assignments = load_assignments(conn)?;
    let moves = load_moves(conn)?;
    let targets = load_targets(conn)?;
    Ok(ProjectionData {
        transactions,
        allocations,
        transfers,
        assignments,
        moves,
        targets,
    })
}

fn load_transactions(
    conn: &Connection,
    as_of: BudgetDate,
) -> Result<Vec<RowTxn>, BudgetError> {
    let mut statement = conn.prepare(
        "SELECT t.id, t.account_id, t.kind, t.transaction_date, t.amount_cents
         FROM transactions t
         JOIN accounts a ON a.id = t.account_id
         WHERE t.transaction_date <= ?1 AND a.kind <> 'tracking'
         ORDER BY t.transaction_date, t.id",
    )?;
    let rows = statement.query_map([as_of.to_string()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    let mut transactions = Vec::new();
    for row in rows {
        let row = row?;
        transactions.push(RowTxn {
            id: row.0,
            account_id: AccountId::new(row.1),
            kind: TransactionKind::parse(&row.2)?,
            date: parse_date("transactions", &row.3)?,
            amount: Amount::from_cents(row.4),
        });
    }
    Ok(transactions)
}

fn load_allocations(
    conn: &Connection,
    as_of: BudgetDate,
) -> Result<HashMap<i64, Vec<RowAlloc>>, BudgetError> {
    let mut statement = conn.prepare(
        "SELECT a.transaction_id, a.category_id, a.amount_cents
         FROM allocations a
         JOIN transactions t ON t.id = a.transaction_id
         JOIN accounts ac ON ac.id = t.account_id
         WHERE t.transaction_date <= ?1 AND ac.kind <> 'tracking'
         ORDER BY a.transaction_id, a.id",
    )?;
    let rows = statement.query_map([as_of.to_string()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut allocations: HashMap<i64, Vec<RowAlloc>> = HashMap::new();
    for row in rows {
        let row = row?;
        allocations.entry(row.0).or_default().push(RowAlloc {
            category_id: row.1.map(CategoryId::new),
            amount: Amount::from_cents(row.2),
        });
    }
    Ok(allocations)
}

fn load_transfers(conn: &Connection) -> Result<HashMap<i64, TransferMeta>, BudgetError> {
    let mut statement = conn.prepare(
        "SELECT from_transaction_id, to_transaction_id, boundary,
                boundary_target, boundary_category_id
         FROM transfers",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<i64>>(4)?,
        ))
    })?;
    let mut transfers = HashMap::new();
    for row in rows {
        let row = row?;
        let target = match row.3.as_deref() {
            None => None,
            Some("ready_to_assign") => Some(BoundaryTarget::ReadyToAssign),
            Some("category") => Some(BoundaryTarget::Category(CategoryId::new(
                row.4.ok_or(BudgetError::CorruptRow {
                    table: "transfers",
                    detail: "category boundary has no category identifier".to_owned(),
                })?,
            ))),
            Some(other) => {
                return Err(BudgetError::CorruptRow {
                    table: "transfers",
                    detail: format!("unknown boundary target `{other}`"),
                });
            }
        };
        let meta = TransferMeta {
            boundary: TransferBoundary::parse(&row.2)?,
            target,
        };
        transfers.insert(row.0, meta);
        transfers.insert(row.1, meta);
    }
    Ok(transfers)
}

fn load_assignments(conn: &Connection) -> Result<Vec<BudgetAssignment>, BudgetError> {
    let mut statement = conn.prepare(
        "SELECT id, month, category_id, amount_cents, revision
         FROM budget_assignments ORDER BY month, category_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    let mut assignments = Vec::new();
    for row in rows {
        let row = row?;
        assignments.push(BudgetAssignment {
            id: row.0,
            month: parse_month("budget_assignments", &row.1)?,
            category_id: CategoryId::new(row.2),
            amount: Amount::from_cents(row.3),
            revision: row.4,
        });
    }
    Ok(assignments)
}

fn load_moves(conn: &Connection) -> Result<Vec<BudgetMove>, BudgetError> {
    let mut statement = conn.prepare(
        "SELECT id, month, from_category_id, to_category_id, amount_cents, revision
         FROM budget_moves ORDER BY month, id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;
    let mut moves = Vec::new();
    for row in rows {
        let row = row?;
        moves.push(BudgetMove {
            id: row.0,
            month: parse_month("budget_moves", &row.1)?,
            from_category_id: CategoryId::new(row.2),
            to_category_id: CategoryId::new(row.3),
            amount: Amount::from_cents(row.4),
            revision: row.5,
        });
    }
    Ok(moves)
}

fn load_targets(conn: &Connection) -> Result<Vec<CategoryTargetRecord>, BudgetError> {
    let ids: Vec<i64> = {
        let mut statement = conn.prepare(
            "SELECT category_id FROM category_targets ORDER BY category_id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut targets = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(target) = load_target(conn, CategoryId::new(id))? {
            targets.push(target);
        }
    }
    Ok(targets)
}

fn parse_date(table: &'static str, text: &str) -> Result<BudgetDate, BudgetError> {
    text.parse().map_err(|source| BudgetError::CorruptRow {
        table,
        detail: format!("invalid date `{text}`: {source}"),
    })
}

fn corrupt(table: &'static str, detail: &str) -> BudgetError {
    BudgetError::CorruptRow {
        table,
        detail: detail.to_owned(),
    }
}

/// Mutable state carried forward between months.
struct State {
    rta: Amount,
    carryover: HashMap<Option<CategoryId>, Amount>,
    payment_available: HashMap<AccountId, Amount>,
    card_balance: HashMap<AccountId, Amount>,
    cash_balance: Amount,
}

/// One month's computed buckets, kept so the viewed month can be rendered.
struct MonthComputation {
    carried_in: HashMap<Option<CategoryId>, Amount>,
    payment_carried_in: HashMap<AccountId, Amount>,
    assigned: HashMap<Option<CategoryId>, Amount>,
    activity: HashMap<Option<CategoryId>, Amount>,
    available: HashMap<Option<CategoryId>, Amount>,
    cash_overspending: HashMap<Option<CategoryId>, Amount>,
    credit_overspending: HashMap<Option<CategoryId>, Amount>,
    payment_assigned: HashMap<AccountId, Amount>,
    payment_reserved: HashMap<AccountId, Amount>,
    payment_paid: HashMap<AccountId, Amount>,
    payment_available: HashMap<AccountId, Amount>,
    card_balance: HashMap<AccountId, Amount>,
    cash_balance: Amount,
    ready_to_assign: Amount,
    ordinary_available: Amount,
    uncovered_credit_spending: Amount,
}

/// Ledger activity gathered for one month before funding is allocated.
#[derive(Default)]
struct MonthInputs {
    net_assigned: HashMap<Option<CategoryId>, Amount>,
    cash_net: HashMap<Option<CategoryId>, Amount>,
    card_net: HashMap<Option<CategoryId>, Amount>,
    income: Amount,
    opening: Amount,
    rta_boundary: Amount,
    cash_month: Amount,
}

/// Card-payment bucket movements produced by chronological card events.
struct CardEvents {
    payment_available: HashMap<AccountId, Amount>,
    payment_reserved: HashMap<AccountId, Amount>,
    payment_paid: HashMap<AccountId, Amount>,
}

/// End-of-month bucket results for one month.
struct Settlement {
    available: HashMap<Option<CategoryId>, Amount>,
    activity: HashMap<Option<CategoryId>, Amount>,
    cash_overspending: HashMap<Option<CategoryId>, Amount>,
    credit_overspending: HashMap<Option<CategoryId>, Amount>,
    carryover: HashMap<Option<CategoryId>, Amount>,
    ordinary_available: Amount,
    uncovered_credit_spending: Amount,
    total_cash_overspend: Amount,
    total_assigned: Amount,
}

struct Projector<'a> {
    transactions: &'a [RowTxn],
    allocations: &'a HashMap<i64, Vec<RowAlloc>>,
    transfers: &'a HashMap<i64, TransferMeta>,
    assignments: &'a [BudgetAssignment],
    moves: &'a [BudgetMove],
    targets: &'a [CategoryTargetRecord],
    categories: &'a [Category],
    groups: &'a [CategoryGroup],
    accounts: &'a [Account],
    by_month: HashMap<BudgetMonth, Vec<&'a RowTxn>>,
    card_ids: HashSet<AccountId>,
    cash_ids: HashSet<AccountId>,
    payment_category_to_card: HashMap<CategoryId, AccountId>,
    payment_category_ids: HashSet<CategoryId>,
}

fn project(
    groups: &[CategoryGroup],
    categories: &[Category],
    accounts: &[Account],
    data: &ProjectionData,
    view_month: BudgetMonth,
    as_of: BudgetDate,
) -> Result<BudgetMonthView, BudgetError> {
    let projector = Projector::new(groups, categories, accounts, data);
    projector.run(view_month, as_of)
}

impl<'a> Projector<'a> {
    fn new(
        groups: &'a [CategoryGroup],
        categories: &'a [Category],
        accounts: &'a [Account],
        data: &'a ProjectionData,
    ) -> Self {
        let card_ids: HashSet<AccountId> = accounts
            .iter()
            .filter(|account| account.kind.is_credit_card())
            .map(|account| account.id)
            .collect();
        let cash_ids: HashSet<AccountId> = accounts
            .iter()
            .filter(|account| {
                matches!(
                    account.kind,
                    AccountKind::Budget(
                        BudgetAccountKind::Checking
                            | BudgetAccountKind::Savings
                            | BudgetAccountKind::Cash
                    )
                )
            })
            .map(|account| account.id)
            .collect();
        let payment_category_to_card: HashMap<CategoryId, AccountId> = accounts
            .iter()
            .filter_map(|account| {
                account
                    .payment_category_id
                    .map(|category| (category, account.id))
            })
            .collect();
        let payment_category_ids = payment_category_to_card.keys().copied().collect();

        let mut by_month: HashMap<BudgetMonth, Vec<&RowTxn>> = HashMap::new();
        for transaction in &data.transactions {
            by_month
                .entry(BudgetMonth::containing(transaction.date))
                .or_default()
                .push(transaction);
        }

        Self {
            transactions: &data.transactions,
            allocations: &data.allocations,
            transfers: &data.transfers,
            assignments: &data.assignments,
            moves: &data.moves,
            targets: &data.targets,
            categories,
            groups,
            accounts,
            by_month,
            card_ids,
            cash_ids,
            payment_category_to_card,
            payment_category_ids,
        }
    }

    fn run(
        &self,
        view_month: BudgetMonth,
        as_of: BudgetDate,
    ) -> Result<BudgetMonthView, BudgetError> {
        let current_month = BudgetMonth::containing(as_of);
        let start = self.start_month(view_month);
        let mut state = State {
            rta: Amount::ZERO,
            carryover: HashMap::new(),
            payment_available: self
                .card_ids
                .iter()
                .map(|card| (*card, Amount::ZERO))
                .collect(),
            card_balance: self
                .card_ids
                .iter()
                .map(|card| (*card, Amount::ZERO))
                .collect(),
            cash_balance: Amount::ZERO,
        };

        let mut cursor = start;
        let computation = loop {
            let computation = self.run_month(&mut state, cursor)?;
            if cursor == view_month {
                break computation;
            }
            cursor = cursor.next().ok_or_else(|| {
                corrupt("budget", "month range exceeded the supported calendar")
            })?;
        };

        self.render(view_month, current_month, as_of, computation)
    }

    fn start_month(&self, view_month: BudgetMonth) -> BudgetMonth {
        let mut start = view_month;
        for transaction in self.transactions {
            let month = BudgetMonth::containing(transaction.date);
            if month < start {
                start = month;
            }
        }
        for assignment in self.assignments {
            if assignment.month < start {
                start = assignment.month;
            }
        }
        for budget_move in self.moves {
            if budget_move.month < start {
                start = budget_move.month;
            }
        }
        start
    }

    fn allocations_for(&self, transaction_id: i64) -> &[RowAlloc] {
        self.allocations
            .get(&transaction_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn is_payment_category(&self, category: Option<CategoryId>) -> bool {
        category.is_some_and(|category| self.payment_category_ids.contains(&category))
    }

    fn run_month(
        &self,
        state: &mut State,
        month: BudgetMonth,
    ) -> Result<MonthComputation, BudgetError> {
        let carried_in = state.carryover.clone();
        let payment_carried_in = state.payment_available.clone();

        let MonthInputs {
            net_assigned,
            cash_net,
            card_net,
            income,
            opening,
            rta_boundary,
            cash_month,
        } = self.harvest_month(month)?;

        let mut payment_assigned: HashMap<AccountId, Amount> = self
            .card_ids
            .iter()
            .map(|card| (*card, Amount::ZERO))
            .collect();
        for (category, card) in &self.payment_category_to_card {
            if let Some(amount) = net_assigned.get(&Some(*category)) {
                add_to(&mut payment_assigned, *card, *amount)?;
            }
        }

        let mut remaining_funding =
            self.allocate_funding(&carried_in, &net_assigned, &cash_net, &card_net)?;
        let mut unfunded: HashMap<Option<CategoryId>, Amount> = HashMap::new();
        let payment_available =
            self.start_payment_availability(&payment_carried_in, &net_assigned)?;

        let CardEvents {
            payment_available,
            payment_reserved,
            payment_paid,
        } = self.apply_card_events(
            state,
            month,
            payment_available,
            &mut remaining_funding,
            &mut unfunded,
        )?;

        let settlement =
            self.settle_month(&carried_in, &net_assigned, &cash_net, &card_net, &unfunded)?;

        let ready_to_assign = state
            .rta
            .checked_add(income)?
            .checked_add(opening)?
            .checked_add(rta_boundary)?
            .checked_sub(settlement.total_assigned)?;

        state.rta = ready_to_assign.checked_sub(settlement.total_cash_overspend)?;
        state.carryover = settlement.carryover;
        state.payment_available = payment_available.clone();
        state.cash_balance = state.cash_balance.checked_add(cash_month)?;

        Ok(MonthComputation {
            carried_in,
            payment_carried_in,
            assigned: net_assigned,
            activity: settlement.activity,
            available: settlement.available,
            cash_overspending: settlement.cash_overspending,
            credit_overspending: settlement.credit_overspending,
            payment_assigned,
            payment_reserved,
            payment_paid,
            payment_available,
            card_balance: state.card_balance.clone(),
            cash_balance: state.cash_balance,
            ready_to_assign,
            ordinary_available: settlement.ordinary_available,
            uncovered_credit_spending: settlement.uncovered_credit_spending,
        })
    }

    fn harvest_month(&self, month: BudgetMonth) -> Result<MonthInputs, BudgetError> {
        let mut inputs = MonthInputs::default();
        for transaction in self
            .by_month
            .get(&month)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            self.harvest_transaction(&mut inputs, transaction)?;
        }
        self.harvest_budget_money(&mut inputs, month)?;
        Ok(inputs)
    }

    fn harvest_transaction(
        &self,
        inputs: &mut MonthInputs,
        transaction: &RowTxn,
    ) -> Result<(), BudgetError> {
        if self.cash_ids.contains(&transaction.account_id) {
            self.harvest_cash(inputs, transaction)?;
        } else if self.card_ids.contains(&transaction.account_id) {
            self.harvest_card(inputs, transaction)?;
        }
        self.harvest_boundary(inputs, transaction)
    }

    fn harvest_cash(
        &self,
        inputs: &mut MonthInputs,
        transaction: &RowTxn,
    ) -> Result<(), BudgetError> {
        inputs.cash_month = inputs.cash_month.checked_add(transaction.amount)?;
        match transaction.kind {
            TransactionKind::Income => {
                inputs.income = inputs.income.checked_add(transaction.amount)?;
            }
            TransactionKind::OpeningBalance => {
                inputs.opening = inputs.opening.checked_add(transaction.amount)?;
            }
            TransactionKind::Outflow
            | TransactionKind::Refund
            | TransactionKind::Transfer
            | TransactionKind::Adjustment => {}
        }
        self.harvest_allocations(&mut inputs.cash_net, transaction)
    }

    fn harvest_card(
        &self,
        inputs: &mut MonthInputs,
        transaction: &RowTxn,
    ) -> Result<(), BudgetError> {
        match transaction.kind {
            TransactionKind::Income => {
                inputs.income = inputs.income.checked_add(transaction.amount)?;
            }
            TransactionKind::OpeningBalance if transaction.amount.is_positive() => {
                inputs.opening = inputs.opening.checked_add(transaction.amount)?;
            }
            TransactionKind::Outflow
            | TransactionKind::Refund
            | TransactionKind::Transfer
            | TransactionKind::OpeningBalance
            | TransactionKind::Adjustment => {}
        }
        self.harvest_allocations(&mut inputs.card_net, transaction)
    }

    fn harvest_allocations(
        &self,
        target: &mut HashMap<Option<CategoryId>, Amount>,
        transaction: &RowTxn,
    ) -> Result<(), BudgetError> {
        for allocation in self.allocations_for(transaction.id) {
            if self.is_payment_category(allocation.category_id) {
                continue;
            }
            add_to(target, allocation.category_id, allocation.amount)?;
        }
        Ok(())
    }

    fn harvest_boundary(
        &self,
        inputs: &mut MonthInputs,
        transaction: &RowTxn,
    ) -> Result<(), BudgetError> {
        let Some(meta) = self.transfers.get(&transaction.id) else {
            return Ok(());
        };
        if meta.boundary != TransferBoundary::BudgetTracking {
            return Ok(());
        }
        match meta.target {
            Some(BoundaryTarget::Category(category)) => add_to(
                &mut inputs.net_assigned,
                Some(category),
                transaction.amount,
            ),
            Some(BoundaryTarget::ReadyToAssign) => {
                inputs.rta_boundary = inputs.rta_boundary.checked_add(transaction.amount)?;
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn harvest_budget_money(
        &self,
        inputs: &mut MonthInputs,
        month: BudgetMonth,
    ) -> Result<(), BudgetError> {
        for assignment in self.assignments {
            if assignment.month == month {
                add_to(
                    &mut inputs.net_assigned,
                    Some(assignment.category_id),
                    assignment.amount,
                )?;
            }
        }
        for budget_move in self.moves {
            if budget_move.month == month {
                add_to(
                    &mut inputs.net_assigned,
                    Some(budget_move.from_category_id),
                    budget_move.amount.checked_neg()?,
                )?;
                add_to(
                    &mut inputs.net_assigned,
                    Some(budget_move.to_category_id),
                    budget_move.amount,
                )?;
            }
        }
        Ok(())
    }

    fn allocate_funding(
        &self,
        carried_in: &HashMap<Option<CategoryId>, Amount>,
        net_assigned: &HashMap<Option<CategoryId>, Amount>,
        cash_net: &HashMap<Option<CategoryId>, Amount>,
        card_net: &HashMap<Option<CategoryId>, Amount>,
    ) -> Result<HashMap<Option<CategoryId>, Amount>, BudgetError> {
        let mut remaining = HashMap::new();
        for key in self.ordinary_keys(net_assigned, cash_net, card_net) {
            let carried = carried_in.get(&key).copied().unwrap_or(Amount::ZERO);
            let assigned = net_assigned.get(&key).copied().unwrap_or(Amount::ZERO);
            let funding = carried.checked_add(assigned)?;
            let cash = cash_net.get(&key).copied().unwrap_or(Amount::ZERO);
            let cash_spending = positive_part(cash.checked_neg()?);
            let funded_cash = min_amount(funding, cash_spending);
            let available_to_cards = if funding.is_negative() {
                // Moving out more than a category holds is itself a cash
                // deficit; no money remains to fund card spending.
                Amount::ZERO
            } else {
                funding.checked_sub(funded_cash)?
            };
            remaining.insert(key, available_to_cards);
        }
        Ok(remaining)
    }

    fn start_payment_availability(
        &self,
        carried: &HashMap<AccountId, Amount>,
        net_assigned: &HashMap<Option<CategoryId>, Amount>,
    ) -> Result<HashMap<AccountId, Amount>, BudgetError> {
        let mut available = carried.clone();
        for (category, card) in &self.payment_category_to_card {
            if let Some(amount) = net_assigned.get(&Some(*category))
                && let Some(slot) = available.get_mut(card)
            {
                *slot = slot.checked_add(*amount)?;
            }
        }
        Ok(available)
    }

    fn apply_card_events(
        &self,
        state: &mut State,
        month: BudgetMonth,
        mut payment_available: HashMap<AccountId, Amount>,
        remaining_funding: &mut HashMap<Option<CategoryId>, Amount>,
        unfunded: &mut HashMap<Option<CategoryId>, Amount>,
    ) -> Result<CardEvents, BudgetError> {
        let mut payment_reserved: HashMap<AccountId, Amount> = self
            .card_ids
            .iter()
            .map(|card| (*card, Amount::ZERO))
            .collect();
        let mut payment_paid: HashMap<AccountId, Amount> = self
            .card_ids
            .iter()
            .map(|card| (*card, Amount::ZERO))
            .collect();
        let mut card_events: Vec<&RowTxn> = self
            .by_month
            .get(&month)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .copied()
            .filter(|transaction| self.card_ids.contains(&transaction.account_id))
            .collect();
        card_events.sort_by_key(|transaction| {
            (
                transaction.date,
                transaction.account_id.value(),
                transaction.id,
            )
        });

        for transaction in card_events {
            let card = transaction.account_id;
            match transaction.kind {
                TransactionKind::Income
                | TransactionKind::OpeningBalance
                | TransactionKind::Adjustment => {
                    self.add_card_balance(state, card, transaction.amount)?;
                }
                TransactionKind::Transfer => {
                    self.add_card_balance(state, card, transaction.amount)?;
                    if let Some(meta) = self.transfers.get(&transaction.id)
                        && meta.boundary == TransferBoundary::Budget
                        && transaction.amount.is_positive()
                    {
                        let available = payment_available.get_mut(&card).ok_or_else(|| {
                            corrupt("accounts", "card payment state is missing")
                        })?;
                        *available = available.checked_sub(transaction.amount)?;
                        add_to(&mut payment_paid, card, transaction.amount)?;
                    }
                }
                TransactionKind::Outflow | TransactionKind::Refund => {
                    for allocation in self.allocations_for(transaction.id) {
                        if self.is_payment_category(allocation.category_id) {
                            continue;
                        }
                        apply_card_allocation(
                            state,
                            &mut payment_available,
                            &mut payment_reserved,
                            unfunded,
                            remaining_funding
                                .get_mut(&allocation.category_id)
                                .ok_or_else(|| {
                                    corrupt("budget_assignments", "funding state is missing")
                                })?,
                            card,
                            allocation,
                        )?;
                    }
                }
            }
        }
        Ok(CardEvents {
            payment_available,
            payment_reserved,
            payment_paid,
        })
    }

    fn add_card_balance(
        &self,
        state: &mut State,
        card: AccountId,
        amount: Amount,
    ) -> Result<(), BudgetError> {
        let balance = state
            .card_balance
            .get_mut(&card)
            .ok_or_else(|| corrupt("accounts", "card balance state is missing"))?;
        *balance = balance.checked_add(amount)?;
        Ok(())
    }

    fn settle_month(
        &self,
        carried_in: &HashMap<Option<CategoryId>, Amount>,
        net_assigned: &HashMap<Option<CategoryId>, Amount>,
        cash_net: &HashMap<Option<CategoryId>, Amount>,
        card_net: &HashMap<Option<CategoryId>, Amount>,
        unfunded: &HashMap<Option<CategoryId>, Amount>,
    ) -> Result<Settlement, BudgetError> {
        let mut settlement = Settlement {
            available: HashMap::new(),
            activity: HashMap::new(),
            cash_overspending: HashMap::new(),
            credit_overspending: HashMap::new(),
            carryover: HashMap::new(),
            ordinary_available: Amount::ZERO,
            uncovered_credit_spending: Amount::ZERO,
            total_cash_overspend: Amount::ZERO,
            total_assigned: Amount::ZERO,
        };
        for key in self.ordinary_keys(net_assigned, cash_net, card_net) {
            let carried = carried_in.get(&key).copied().unwrap_or(Amount::ZERO);
            let assigned = net_assigned.get(&key).copied().unwrap_or(Amount::ZERO);
            let cash = cash_net.get(&key).copied().unwrap_or(Amount::ZERO);
            let card = card_net.get(&key).copied().unwrap_or(Amount::ZERO);
            let end = carried
                .checked_add(assigned)?
                .checked_add(cash)?
                .checked_add(card)?;
            settlement.available.insert(key, end);
            settlement
                .activity
                .insert(key, cash.checked_add(card)?);
            settlement.ordinary_available = settlement.ordinary_available.checked_add(end)?;

            let deficit = positive_part(end.checked_neg()?);
            let pool = unfunded.get(&key).copied().unwrap_or(Amount::ZERO);
            let credit = min_amount(pool, deficit);
            let cash_part = deficit.checked_sub(credit)?;
            settlement.credit_overspending.insert(key, credit);
            settlement.cash_overspending.insert(key, cash_part);
            settlement.uncovered_credit_spending = settlement
                .uncovered_credit_spending
                .checked_add(credit)?;
            settlement.total_cash_overspend =
                settlement.total_cash_overspend.checked_add(cash_part)?;
            if end.is_positive() {
                settlement.carryover.insert(key, end);
            }
        }
        for amount in net_assigned.values() {
            settlement.total_assigned = settlement.total_assigned.checked_add(*amount)?;
        }
        Ok(settlement)
    }

    fn ordinary_keys(
        &self,
        net_assigned: &HashMap<Option<CategoryId>, Amount>,
        cash_net: &HashMap<Option<CategoryId>, Amount>,
        card_net: &HashMap<Option<CategoryId>, Amount>,
    ) -> HashSet<Option<CategoryId>> {
        let mut keys: HashSet<Option<CategoryId>> = HashSet::new();
        keys.insert(None);
        for category in self.categories {
            if !self.payment_category_ids.contains(&category.id) {
                keys.insert(Some(category.id));
            }
        }
        for map in [net_assigned, cash_net, card_net] {
            for key in map.keys() {
                if !self.is_payment_category(*key) {
                    keys.insert(*key);
                }
            }
        }
        keys
    }

    fn render(
        &self,
        view_month: BudgetMonth,
        current_month: BudgetMonth,
        as_of: BudgetDate,
        computation: MonthComputation,
    ) -> Result<BudgetMonthView, BudgetError> {
        let basis = budget_basis(view_month, current_month, as_of);
        let reserved_future = self.reserved_future(view_month, current_month)?;
        let ready_to_assign = computation.ready_to_assign.checked_sub(reserved_future)?;

        let groups = self.group_views(&computation, view_month)?;

        let uncategorized =
            self.bucket_view(None, "Uncategorized", 0, false, None, &computation)?;

        let card_payments = self.card_payment_views(&computation)?;

        let mut card_payment_available = Amount::ZERO;
        let mut positive_card_assets = Amount::ZERO;
        for payment in &card_payments {
            card_payment_available = card_payment_available.checked_add(payment.available)?;
            positive_card_assets = positive_card_assets
                .checked_add(positive_card_asset(payment.card_balance, payment.available)?)?;
        }

        let conservation = if view_month == current_month {
            Some(BudgetConservation {
                ready_to_assign,
                ordinary_available: computation.ordinary_available,
                card_payment_available,
                uncovered_credit_spending: computation.uncovered_credit_spending,
                reserved_future_assignments: reserved_future,
                recognized_cash: computation.cash_balance,
                positive_card_assets,
            })
        } else {
            None
        };

        Ok(BudgetMonthView {
            month: view_month,
            basis,
            ready_to_assign,
            reserved_future_assignments: reserved_future,
            ordinary_available: computation.ordinary_available,
            uncovered_credit_spending: computation.uncovered_credit_spending,
            groups,
            uncategorized,
            card_payments,
            conservation,
        })
    }

    fn reserved_future(
        &self,
        view_month: BudgetMonth,
        current_month: BudgetMonth,
    ) -> Result<Amount, BudgetError> {
        if view_month < current_month {
            return Ok(Amount::ZERO);
        }
        let mut reserved = Amount::ZERO;
        for assignment in self.assignments {
            if assignment.month > view_month {
                reserved = reserved.checked_add(assignment.amount)?;
            }
        }
        Ok(reserved)
    }

    fn group_views(
        &self,
        computation: &MonthComputation,
        view_month: BudgetMonth,
    ) -> Result<Vec<BudgetGroupView>, BudgetError> {
        let mut groups = Vec::with_capacity(self.groups.len());
        for group in self.groups {
            let mut rows = Vec::new();
            for category in self
                .categories
                .iter()
                .filter(|category| category.group_id == group.id)
            {
                if self.payment_category_ids.contains(&category.id) {
                    continue;
                }
                rows.push(self.category_view(category.id, computation, view_month)?);
            }
            rows.sort_by_key(|row| (row.position, row.category_id.map(CategoryId::value)));
            groups.push(BudgetGroupView {
                group: group.clone(),
                rows,
            });
        }
        Ok(groups)
    }

    fn card_payment_views(
        &self,
        computation: &MonthComputation,
    ) -> Result<Vec<CardPaymentView>, BudgetError> {
        let mut cards = Vec::new();
        for card in self.accounts.iter().filter(|card| card.kind.is_credit_card()) {
            let Some(category_id) = card.payment_category_id else {
                return Err(corrupt("accounts", "credit card has no payment category"));
            };
            let available = computation
                .payment_available
                .get(&card.id)
                .copied()
                .unwrap_or(Amount::ZERO);
            cards.push(CardPaymentView {
                account_id: card.id,
                account_name: card.name.clone(),
                category_id,
                carried_in: computation
                    .payment_carried_in
                    .get(&card.id)
                    .copied()
                    .unwrap_or(Amount::ZERO),
                assigned: computation
                    .payment_assigned
                    .get(&card.id)
                    .copied()
                    .unwrap_or(Amount::ZERO),
                reserved: computation
                    .payment_reserved
                    .get(&card.id)
                    .copied()
                    .unwrap_or(Amount::ZERO),
                paid: computation
                    .payment_paid
                    .get(&card.id)
                    .copied()
                    .unwrap_or(Amount::ZERO),
                available,
                shortfall: positive_part(available.checked_neg()?),
                card_balance: computation
                    .card_balance
                    .get(&card.id)
                    .copied()
                    .unwrap_or(Amount::ZERO),
            });
        }
        Ok(cards)
    }

    fn category_view(
        &self,
        category_id: CategoryId,
        computation: &MonthComputation,
        view_month: BudgetMonth,
    ) -> Result<BudgetCategoryView, BudgetError> {
        let category = self
            .categories
            .iter()
            .find(|category| category.id == category_id)
            .ok_or(BudgetError::CategoryNotFound { id: category_id })?;
        let mut view = self.bucket_view(
            Some(category.id),
            &category.name,
            category.position,
            category.archived,
            Some(category.group_id),
            computation,
        )?;
        if let Some(target) = self
            .targets
            .iter()
            .find(|target| target.category_id == category_id)
        {
            view.target = Some(target_progress(
                *target,
                view_month,
                view.available,
                view.assigned,
            )?);
        }
        Ok(view)
    }

    fn bucket_view(
        &self,
        category_id: Option<CategoryId>,
        name: &str,
        position: i64,
        archived: bool,
        group: Option<crate::domain::CategoryGroupId>,
        computation: &MonthComputation,
    ) -> Result<BudgetCategoryView, BudgetError> {
        Ok(BudgetCategoryView {
            category_id,
            group_id: group,
            name: name.to_owned(),
            position,
            archived,
            carried_in: computation
                .carried_in
                .get(&category_id)
                .copied()
                .unwrap_or(Amount::ZERO),
            assigned: computation
                .assigned
                .get(&category_id)
                .copied()
                .unwrap_or(Amount::ZERO),
            activity: computation
                .activity
                .get(&category_id)
                .copied()
                .unwrap_or(Amount::ZERO),
            available: computation
                .available
                .get(&category_id)
                .copied()
                .unwrap_or(Amount::ZERO),
            cash_overspending: computation
                .cash_overspending
                .get(&category_id)
                .copied()
                .unwrap_or(Amount::ZERO),
            credit_overspending: computation
                .credit_overspending
                .get(&category_id)
                .copied()
                .unwrap_or(Amount::ZERO),
            target: None,
        })
    }
}

/// Applies one signed card allocation to a purchase or refund.
fn apply_card_allocation(
    state: &mut State,
    payment_available: &mut HashMap<AccountId, Amount>,
    payment_reserved: &mut HashMap<AccountId, Amount>,
    unfunded: &mut HashMap<Option<CategoryId>, Amount>,
    remaining_funding: &mut Amount,
    card: AccountId,
    allocation: &RowAlloc,
) -> Result<(), BudgetError> {
    let balance = state
        .card_balance
        .get_mut(&card)
        .ok_or_else(|| corrupt("accounts", "card balance state is missing"))?;
    let available = payment_available
        .get_mut(&card)
        .ok_or_else(|| corrupt("accounts", "card payment state is missing"))?;

    if allocation.amount.is_negative() {
        let purchase = allocation.amount.checked_neg()?;
        let asset_before = positive_part(*balance);
        let debt = positive_part(purchase.checked_sub(asset_before)?);
        let funded = min_amount(*remaining_funding, purchase);
        *remaining_funding = remaining_funding.checked_sub(funded)?;
        let funded_asset = min_amount(funded, min_amount(purchase, asset_before));
        let funded_debt = funded.checked_sub(funded_asset)?;
        *available = available.checked_add(funded_debt)?;
        add_to(payment_reserved, card, funded_debt)?;
        add_to(
            unfunded,
            allocation.category_id,
            debt.checked_sub(funded_debt)?,
        )?;
        *balance = balance.checked_sub(purchase)?;
    } else if allocation.amount.is_positive() {
        let refund = allocation.amount;
        let debt_before = positive_part(balance.checked_neg()?);
        let reduces_debt = min_amount(refund, debt_before);
        let reversal = min_amount(reduces_debt, positive_part(*available));
        *available = available.checked_sub(reversal)?;
        add_to(payment_reserved, card, reversal.checked_neg()?)?;
        let pool = unfunded
            .get(&allocation.category_id)
            .copied()
            .unwrap_or(Amount::ZERO);
        let reduction = min_amount(reduces_debt.checked_sub(reversal)?, pool);
        if !reduction.is_zero() {
            add_to(unfunded, allocation.category_id, reduction.checked_neg()?)?;
        }
        *balance = balance.checked_add(refund)?;
    }
    Ok(())
}

/// A card's positive balance is budget money, except for the portion already
/// explained by a negative payment availability (an overpayment shortfall).
fn positive_card_asset(balance: Amount, payment_available: Amount) -> Result<Amount, BudgetError> {
    if !balance.is_positive() {
        return Ok(Amount::ZERO);
    }
    let offset = if payment_available.is_negative() {
        payment_available
    } else {
        Amount::ZERO
    };
    Ok(positive_part(balance.checked_add(offset)?))
}

fn add_to<K: std::hash::Hash + Eq>(
    map: &mut HashMap<K, Amount>,
    key: K,
    delta: Amount,
) -> Result<(), BudgetError> {
    let entry = map.entry(key).or_insert(Amount::ZERO);
    *entry = entry.checked_add(delta)?;
    Ok(())
}

fn budget_basis(
    view_month: BudgetMonth,
    current_month: BudgetMonth,
    as_of: BudgetDate,
) -> BudgetBasis {
    if view_month == current_month {
        BudgetBasis::Current { as_of }
    } else if view_month < current_month {
        BudgetBasis::Historical { as_of }
    } else {
        BudgetBasis::Forecast { as_of }
    }
}

fn positive_part(amount: Amount) -> Amount {
    if amount.is_positive() {
        amount
    } else {
        Amount::ZERO
    }
}

/// Cent-exact ceiling division used by target suggestions.
fn ceil_div(amount: Amount, divisor: i64) -> Amount {
    let divisor = divisor.max(1);
    let cents = amount.cents();
    let quotient = cents.div_euclid(divisor);
    let remainder = cents.rem_euclid(divisor);
    Amount::from_cents(if remainder > 0 {
        quotient + 1
    } else {
        quotient
    })
}

fn target_progress(
    target: CategoryTargetRecord,
    view_month: BudgetMonth,
    available: Amount,
    assigned: Amount,
) -> Result<TargetProgress, BudgetError> {
    match target.target {
        CategoryTarget::BalanceByMonth { amount, month } => {
            let remaining = positive_part(amount.checked_sub(available)?);
            let months_remaining = if month >= view_month {
                view_month.months_until(month) + 1
            } else {
                0
            };
            let suggested = if remaining.is_zero() {
                Amount::ZERO
            } else if months_remaining > 0 {
                ceil_div(remaining, months_remaining)
            } else {
                remaining
            };
            Ok(TargetProgress {
                target: target.target,
                measure: TargetProgressMeasure::Balance {
                    available,
                    remaining,
                    months_remaining,
                },
                suggested_contribution: suggested,
            })
        }
        CategoryTarget::MonthlyContribution { amount } => {
            let remaining = positive_part(amount.checked_sub(assigned)?);
            Ok(TargetProgress {
                target: target.target,
                measure: TargetProgressMeasure::Contribution { assigned, remaining },
                suggested_contribution: remaining,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceiling_division_distributes_remainders() {
        assert_eq!(ceil_div(Amount::from_cents(300), 3), Amount::from_cents(100));
        assert_eq!(
            ceil_div(Amount::from_cents(10_000), 3),
            Amount::from_cents(3_334)
        );
        assert_eq!(ceil_div(Amount::ZERO, 3), Amount::ZERO);
        assert_eq!(ceil_div(Amount::from_cents(5), 0), Amount::from_cents(5));
    }

    #[test]
    fn positive_card_assets_net_overpayment_shortfalls() {
        assert_eq!(
            positive_card_asset(Amount::from_cents(3_000), Amount::from_cents(-3_000)).unwrap(),
            Amount::ZERO
        );
        assert_eq!(
            positive_card_asset(Amount::from_cents(3_000), Amount::from_cents(-1_000)).unwrap(),
            Amount::from_cents(2_000)
        );
        assert_eq!(
            positive_card_asset(Amount::from_cents(-1_500), Amount::from_cents(1_500)).unwrap(),
            Amount::ZERO
        );
    }
}
