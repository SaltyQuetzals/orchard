use orchard::budget::{self, BudgetCategoryView, BudgetMonthView, CategoryTarget, TargetProgressMeasure};
use orchard::db::{Db, Store, StorePaths};
use orchard::domain::{
    AccountId, AccountKind, Amount, BudgetAccountKind, BudgetDate, BudgetMonth, CategoryId,
    TransactionKind,
};
use orchard::ledger::{
    self, EditTransaction, NewAccount, NewAllocation, NewTransaction, NewTransfer,
};
use orchard::secrets::SecretKey;
use tempfile::TempDir;

fn key() -> SecretKey {
    SecretKey::from_bytes([29; 32])
}

fn open_store(directory: &TempDir) -> Store {
    Store::open(StorePaths::new(directory.path()), &key()).unwrap()
}

fn date(input: &str) -> BudgetDate {
    input.parse().unwrap()
}

fn month(input: &str) -> BudgetMonth {
    input.parse().unwrap()
}

fn cents(value: i64) -> Amount {
    Amount::from_cents(value)
}

async fn view(db: &Db, month: &str, as_of: &str) -> BudgetMonthView {
    budget::budget_month(db, self::month(month), date(as_of))
        .await
        .unwrap()
}

async fn open_checking(db: &Db, name: &str, opening: Amount) -> AccountId {
    ledger::create_account(
        db,
        NewAccount {
            name,
            kind: AccountKind::Budget(BudgetAccountKind::Checking),
            opened_on: date("2026-01-01"),
            opening_balance: opening,
        },
    )
    .await
    .unwrap()
    .id
}

async fn open_card(db: &Db, name: &str, opening: Amount) -> AccountId {
    ledger::create_account(
        db,
        NewAccount {
            name,
            kind: AccountKind::Budget(BudgetAccountKind::CreditCard),
            opened_on: date("2026-01-01"),
            opening_balance: opening,
        },
    )
    .await
    .unwrap()
    .id
}

async fn open_tracking(db: &Db, name: &str, opening: Amount) -> AccountId {
    ledger::create_account(
        db,
        NewAccount {
            name,
            kind: AccountKind::Tracking,
            opened_on: date("2026-01-01"),
            opening_balance: opening,
        },
    )
    .await
    .unwrap()
    .id
}

async fn open_category(db: &Db, group: &str, name: &str) -> CategoryId {
    let groups = ledger::category_groups(db).await.unwrap();
    let group = match groups.into_iter().find(|candidate| candidate.name == group) {
        Some(group) => group,
        None => ledger::create_category_group(db, group).await.unwrap(),
    };
    ledger::create_category(db, group.id, name).await.unwrap().id
}

async fn entry(
    db: &Db,
    account_id: AccountId,
    kind: TransactionKind,
    amount: Amount,
    day: &str,
    allocations: Vec<NewAllocation>,
) -> ledger::TransactionRecord {
    ledger::create_transaction(
        db,
        NewTransaction {
            account_id,
            kind,
            date: date(day),
            payee: "Test",
            memo: "",
            amount,
            allocations,
        },
    )
    .await
    .unwrap()
}

async fn assign(db: &Db, month: &str, category: CategoryId, amount: Amount) {
    budget::set_assignment(db, self::month(month), category, amount, None)
        .await
        .unwrap();
}

async fn pay_card(db: &Db, from: AccountId, to: AccountId, amount: Amount, day: &str) {
    ledger::create_transfer(
        db,
        NewTransfer {
            from_account_id: from,
            to_account_id: to,
            amount,
            date: date(day),
            payee: "Payment",
            memo: "",
            boundary: None,
        },
    )
    .await
    .unwrap();
}

fn row(view: &BudgetMonthView, id: CategoryId) -> &BudgetCategoryView {
    view.category(Some(id)).expect("category row")
}

fn assert_conserved(view: &BudgetMonthView) {
    let conservation = view.conservation.expect("current-month conservation");
    assert_eq!(
        conservation.accounted().unwrap(),
        conservation.recognized().unwrap(),
        "conservation identity failed: {conservation:?} view={view:?}"
    );
}

#[tokio::test]
async fn assignments_and_moves_conserve_money_and_keep_history() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let checking = open_checking(db, "Checking", cents(50_000)).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    let transport = open_category(db, "Everyday", "Transport").await;

    assign(db, "2026-01", groceries, cents(20_000)).await;
    let mut january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(january.month, month("2026-01"));
    assert_eq!(january.ready_to_assign, cents(30_000));
    assert_eq!(row(&january, groceries).assigned, cents(20_000));
    assert_eq!(row(&january, groceries).available, cents(20_000));
    assert_eq!(row(&january, transport).available, Amount::ZERO);
    assert_conserved(&january);

    let movement = budget::move_budget(
        db,
        self::month("2026-01"),
        groceries,
        transport,
        cents(5_000),
    )
    .await
    .unwrap();
    assert_eq!(movement.amount, cents(5_000));

    january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(january.ready_to_assign, cents(30_000));
    assert_eq!(row(&january, groceries).assigned, cents(15_000));
    assert_eq!(row(&january, groceries).available, cents(15_000));
    assert_eq!(row(&january, transport).assigned, cents(5_000));
    assert_eq!(row(&january, transport).available, cents(5_000));
    assert_eq!(
        january.ordinary_available,
        row(&january, groceries)
            .available
            .checked_add(row(&january, transport).available)
            .unwrap()
    );
    assert_conserved(&january);

    // Assigning more than remains unallocated is allowed and visible.
    assign(db, "2026-01", transport, cents(32_000)).await;
    january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(january.ready_to_assign, cents(-2_000));
    assert_eq!(row(&january, groceries).available, cents(15_000));
    assert_eq!(row(&january, transport).available, cents(37_000));
    assert_conserved(&january);

    // Historical activity keeps its category after archiving; nothing reopens.
    let purchase = entry(
        db,
        checking,
        TransactionKind::Outflow,
        cents(-1_000),
        "2026-01-20",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-1_000),
        }],
    )
    .await;
    assert_eq!(purchase.amount, cents(-1_000));
    ledger::archive_category(db, groceries).await.unwrap();

    january = view(db, "2026-01", "2026-01-31").await;
    assert!(row(&january, groceries).archived);
    assert_eq!(row(&january, groceries).activity, cents(-1_000));
    assert_eq!(row(&january, groceries).available, cents(14_000));
    assert_eq!(
        ledger::outstanding_category_work(db, date("2026-01-31"))
            .await
            .unwrap(),
        Vec::new()
    );
    assert_eq!(january.uncategorized.activity, Amount::ZERO);
    assert_conserved(&january);
}

#[tokio::test]
async fn recognized_resources_reserve_future_months_and_keep_uncategorized_visible() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let checking = open_checking(db, "Checking", cents(100_000)).await;
    let _brokerage = open_tracking(db, "Brokerage", cents(500_000)).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;

    // Future income is dated after the recognition date.
    entry(
        db,
        checking,
        TransactionKind::Income,
        cents(20_000),
        "2026-02-10",
        Vec::new(),
    )
    .await;

    let january = view(db, "2026-01", "2026-01-31").await;
    // Tracking values and future income never fund today.
    assert_eq!(january.ready_to_assign, cents(100_000));
    assert_conserved(&january);

    // An assignment in a later month reserves money immediately.
    assign(db, "2026-02", groceries, cents(30_000)).await;
    let january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(january.reserved_future_assignments, cents(30_000));
    assert_eq!(january.ready_to_assign, cents(70_000));
    assert_conserved(&january);

    // An uncategorized cash purchase is visible and included in the budget.
    let purchase = entry(
        db,
        checking,
        TransactionKind::Outflow,
        cents(-3_000),
        "2026-01-20",
        Vec::new(),
    )
    .await;
    let mut january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(january.uncategorized.activity, cents(-3_000));
    assert_eq!(january.uncategorized.available, cents(-3_000));
    assert_eq!(january.uncategorized.cash_overspending, cents(3_000));
    assert_eq!(january.uncategorized.credit_overspending, Amount::ZERO);
    assert_eq!(january.ready_to_assign, cents(70_000));
    assert_conserved(&january);

    // Selecting a category moves the existing activity without re-posting.
    ledger::edit_transaction(
        db,
        purchase.id,
        purchase.revision,
        EditTransaction {
            date: purchase.date,
            payee: &purchase.payee,
            memo: &purchase.memo,
            amount: purchase.amount,
            allocations: Some(vec![NewAllocation {
                category_id: Some(groceries),
                amount: cents(-3_000),
            }]),
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap();

    january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(january.uncategorized.activity, Amount::ZERO);
    assert_eq!(row(&january, groceries).activity, cents(-3_000));
    assert_eq!(row(&january, groceries).available, cents(-3_000));
    assert_eq!(january.ready_to_assign, cents(70_000));
    assert_eq!(
        ledger::account_balance(db, checking, Some(date("2026-01-31")))
            .await
            .unwrap(),
        cents(97_000)
    );
    assert_conserved(&january);
}

async fn mixed_payment_scenario(reverse: bool) -> (Amount, Amount, Amount, Amount, Amount) {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let checking = open_checking(db, "Checking", cents(20_000)).await;
    let card = open_card(db, "Visa", Amount::ZERO).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    assign(db, "2026-01", groceries, cents(5_000)).await;

    let cash_purchase = (
        checking,
        TransactionKind::Outflow,
        cents(-4_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-4_000),
        }],
    );
    let card_purchase = (
        card,
        TransactionKind::Outflow,
        cents(-4_000),
        "2026-01-20",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-4_000),
        }],
    );
    let purchases = if reverse {
        vec![card_purchase, cash_purchase]
    } else {
        vec![cash_purchase, card_purchase]
    };
    for (account, kind, amount, day, allocations) in purchases {
        entry(db, account, kind, amount, day, allocations).await;
    }

    let january = view(db, "2026-01", "2026-01-31").await;
    let groceries_row = row(&january, groceries);
    (
        january.ready_to_assign,
        groceries_row.available,
        groceries_row.cash_overspending,
        groceries_row.credit_overspending,
        january.card(card).unwrap().available,
    )
}

#[tokio::test]
async fn carryover_and_cash_first_funding_are_deterministic() {
    // Positive availability carries forward.
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let _checking = open_checking(db, "Checking", cents(4_500)).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    assign(db, "2026-01", groceries, cents(4_500)).await;
    assign(db, "2026-02", groceries, cents(2_000)).await;
    let february = view(db, "2026-02", "2026-02-28").await;
    assert_eq!(row(&february, groceries).carried_in, cents(4_500));
    assert_eq!(row(&february, groceries).assigned, cents(2_000));
    assert_eq!(row(&february, groceries).available, cents(6_500));
    assert_conserved(&february);

    // Cash and credit overspending take different rollover paths.
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let checking = open_checking(db, "Checking", cents(10_000)).await;
    let card = open_card(db, "Visa", Amount::ZERO).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    entry(
        db,
        checking,
        TransactionKind::Outflow,
        cents(-2_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-2_000),
        }],
    )
    .await;
    entry(
        db,
        card,
        TransactionKind::Outflow,
        cents(-3_000),
        "2026-01-15",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-3_000),
        }],
    )
    .await;

    let january = view(db, "2026-01", "2026-01-31").await;
    let groceries_row = row(&january, groceries);
    assert_eq!(groceries_row.available, cents(-5_000));
    assert_eq!(groceries_row.cash_overspending, cents(2_000));
    assert_eq!(groceries_row.credit_overspending, cents(3_000));
    assert_eq!(january.ready_to_assign, cents(10_000));
    assert_eq!(january.card(card).unwrap().available, Amount::ZERO);
    assert_conserved(&january);

    let february = view(db, "2026-02", "2026-02-28").await;
    assert_eq!(february.ready_to_assign, cents(8_000));
    assert_eq!(row(&february, groceries).available, Amount::ZERO);
    assert_eq!(row(&february, groceries).carried_in, Amount::ZERO);
    assert_eq!(row(&february, groceries).cash_overspending, Amount::ZERO);
    assert_eq!(row(&february, groceries).credit_overspending, Amount::ZERO);
    assert_eq!(february.card(card).unwrap().available, Amount::ZERO);
    assert_conserved(&february);

    // Cash-first funding makes mixed cash/card results independent of arrival.
    let forward = mixed_payment_scenario(false).await;
    let reverse = mixed_payment_scenario(true).await;
    assert_eq!(forward, reverse);
    let (rta, available, cash_overspending, credit_overspending, payment) = forward;
    assert_eq!(rta, cents(15_000));
    assert_eq!(available, cents(-3_000));
    assert_eq!(cash_overspending, Amount::ZERO);
    assert_eq!(credit_overspending, cents(3_000));
    assert_eq!(payment, cents(1_000));
}

#[tokio::test]
async fn card_payment_categories_fund_debt_and_expose_shortfalls() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let checking = open_checking(db, "Checking", cents(50_000)).await;
    let visa = open_card(db, "Visa", Amount::ZERO).await;
    let mastercard = open_card(db, "Mastercard", Amount::ZERO).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;

    let visa_account = ledger::account(db, visa).await.unwrap();
    let mastercard_account = ledger::account(db, mastercard).await.unwrap();
    assert!(visa_account.payment_category_id.is_some());
    assert!(mastercard_account.payment_category_id.is_some());
    assert_ne!(
        visa_account.payment_category_id,
        mastercard_account.payment_category_id
    );

    assign(db, "2026-01", groceries, cents(10_000)).await;
    entry(
        db,
        visa,
        TransactionKind::Outflow,
        cents(-6_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-6_000),
        }],
    )
    .await;

    let before_payment = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(row(&before_payment, groceries).available, cents(4_000));
    assert_eq!(row(&before_payment, groceries).activity, cents(-6_000));
    assert_eq!(before_payment.card(visa).unwrap().reserved, cents(6_000));
    assert_eq!(before_payment.card(visa).unwrap().available, cents(6_000));
    assert_eq!(before_payment.ready_to_assign, cents(40_000));
    assert_conserved(&before_payment);

    pay_card(db, checking, visa, cents(6_000), "2026-01-15").await;
    let after_payment = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(row(&after_payment, groceries).available, cents(4_000));
    assert_eq!(row(&after_payment, groceries).activity, cents(-6_000));
    assert_eq!(after_payment.card(visa).unwrap().paid, cents(6_000));
    assert_eq!(after_payment.card(visa).unwrap().available, Amount::ZERO);
    assert_eq!(after_payment.card(visa).unwrap().card_balance, Amount::ZERO);
    assert_eq!(after_payment.ready_to_assign, cents(40_000));
    assert_conserved(&after_payment);

    // Explicit funding of pre-existing debt, then a payment beyond the reserve.
    let debt_card = open_card(db, "Store card", cents(-10_000)).await;
    let payment_category = ledger::account(db, debt_card)
        .await
        .unwrap()
        .payment_category_id
        .unwrap();
    assign(db, "2026-01", payment_category, cents(7_000)).await;
    pay_card(db, checking, debt_card, cents(10_000), "2026-01-20").await;

    let january = view(db, "2026-01", "2026-01-31").await;
    let payment = january.card(debt_card).unwrap();
    assert_eq!(payment.assigned, cents(7_000));
    assert_eq!(payment.available, cents(-3_000));
    assert_eq!(payment.shortfall, cents(3_000));
    assert_eq!(payment.card_balance, Amount::ZERO);
    assert_conserved(&january);

    // Payment shortfalls do not reset at a month boundary.
    let february = view(db, "2026-02", "2026-02-28").await;
    let payment = february.card(debt_card).unwrap();
    assert_eq!(payment.carried_in, cents(-3_000));
    assert_eq!(payment.available, cents(-3_000));
    assert_eq!(payment.shortfall, cents(3_000));
    assert_conserved(&february);
}

#[tokio::test]
async fn card_refunds_and_positive_balances_never_invent_income() {
    // Refund before repayment reverses part of the reserve.
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let _checking = open_checking(db, "Checking", cents(50_000)).await;
    let visa = open_card(db, "Visa", Amount::ZERO).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    assign(db, "2026-01", groceries, cents(10_000)).await;
    entry(
        db,
        visa,
        TransactionKind::Outflow,
        cents(-6_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-6_000),
        }],
    )
    .await;
    entry(
        db,
        visa,
        TransactionKind::Refund,
        cents(2_000),
        "2026-01-12",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(2_000),
        }],
    )
    .await;

    let january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(row(&january, groceries).available, cents(6_000));
    assert_eq!(january.card(visa).unwrap().reserved, cents(4_000));
    assert_eq!(january.card(visa).unwrap().available, cents(4_000));
    assert_eq!(january.ready_to_assign, cents(40_000));
    assert_conserved(&january);

    // A refund after repayment leaves the card in credit without new money.
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let checking = open_checking(db, "Checking", cents(50_000)).await;
    let visa = open_card(db, "Visa", Amount::ZERO).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    assign(db, "2026-01", groceries, cents(6_000)).await;
    entry(
        db,
        visa,
        TransactionKind::Outflow,
        cents(-6_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-6_000),
        }],
    )
    .await;
    pay_card(db, checking, visa, cents(6_000), "2026-01-15").await;
    entry(
        db,
        visa,
        TransactionKind::Refund,
        cents(2_000),
        "2026-02-05",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(2_000),
        }],
    )
    .await;

    let february = view(db, "2026-02", "2026-02-28").await;
    assert_eq!(row(&february, groceries).activity, cents(2_000));
    assert_eq!(row(&february, groceries).available, cents(2_000));
    assert_eq!(february.card(visa).unwrap().available, Amount::ZERO);
    assert_eq!(february.card(visa).unwrap().card_balance, cents(2_000));
    assert_eq!(february.ready_to_assign, cents(44_000));
    assert_conserved(&february);

    // A positive $25 card balance covers $25 of a funded $40 purchase.
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let _checking = open_checking(db, "Checking", cents(50_000)).await;
    let visa = open_card(db, "Visa", cents(2_500)).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    assign(db, "2026-01", groceries, cents(4_000)).await;
    entry(
        db,
        visa,
        TransactionKind::Outflow,
        cents(-4_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-4_000),
        }],
    )
    .await;

    let january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(row(&january, groceries).available, Amount::ZERO);
    assert_eq!(january.card(visa).unwrap().reserved, cents(1_500));
    assert_eq!(january.card(visa).unwrap().available, cents(1_500));
    assert_eq!(january.card(visa).unwrap().card_balance, cents(-1_500));
    assert_eq!(january.ready_to_assign, cents(48_500));
    assert_conserved(&january);

    // Overpayment creates a positive asset and a matching visible shortfall.
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let checking = open_checking(db, "Checking", cents(50_000)).await;
    let visa = open_card(db, "Visa", Amount::ZERO).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    assign(db, "2026-01", groceries, cents(7_000)).await;
    entry(
        db,
        visa,
        TransactionKind::Outflow,
        cents(-7_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-7_000),
        }],
    )
    .await;
    pay_card(db, checking, visa, cents(10_000), "2026-01-20").await;

    let january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(january.card(visa).unwrap().card_balance, cents(3_000));
    assert_eq!(january.card(visa).unwrap().available, cents(-3_000));
    assert_eq!(january.card(visa).unwrap().shortfall, cents(3_000));
    assert_conserved(&january);
}

#[tokio::test]
async fn historical_edits_recompute_downstream_and_multiple_cards_are_stable() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let checking = open_checking(db, "Checking", cents(10_000)).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    assign(db, "2026-01", groceries, cents(3_000)).await;
    let purchase = entry(
        db,
        checking,
        TransactionKind::Outflow,
        cents(-4_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-4_000),
        }],
    )
    .await;

    let february = view(db, "2026-02", "2026-02-28").await;
    assert_eq!(february.ready_to_assign, cents(6_000));
    assert_conserved(&february);

    ledger::edit_transaction(
        db,
        purchase.id,
        purchase.revision,
        EditTransaction {
            date: date("2026-01-10"),
            payee: "Test",
            memo: "",
            amount: cents(-5_000),
            allocations: Some(vec![NewAllocation {
                category_id: Some(groceries),
                amount: cents(-5_000),
            }]),
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap();

    // The January correction flows into February exactly once.
    let february = view(db, "2026-02", "2026-02-28").await;
    assert_eq!(february.ready_to_assign, cents(5_000));
    assert_eq!(
        ledger::account_balance(db, checking, None).await.unwrap(),
        cents(5_000)
    );
    assert_conserved(&february);

    // Seeded multi-month history conserves with both cash and card spending.
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let checking = open_checking(db, "Checking", cents(100_000)).await;
    let visa = open_card(db, "Visa", Amount::ZERO).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    let transport = open_category(db, "Everyday", "Transport").await;

    assign(db, "2026-01", groceries, cents(20_000)).await;
    assign(db, "2026-01", transport, cents(10_000)).await;
    entry(
        db,
        checking,
        TransactionKind::Outflow,
        cents(-15_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-15_000),
        }],
    )
    .await;
    entry(
        db,
        visa,
        TransactionKind::Outflow,
        cents(-6_000),
        "2026-01-12",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-6_000),
        }],
    )
    .await;
    pay_card(db, checking, visa, cents(6_000), "2026-01-15").await;

    entry(
        db,
        checking,
        TransactionKind::Income,
        cents(50_000),
        "2026-02-01",
        Vec::new(),
    )
    .await;
    assign(db, "2026-02", groceries, cents(10_000)).await;
    entry(
        db,
        checking,
        TransactionKind::Outflow,
        cents(-8_000),
        "2026-02-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-8_000),
        }],
    )
    .await;

    let february = view(db, "2026-02", "2026-02-28").await;
    assert_eq!(february.ready_to_assign, cents(110_000));
    assert_eq!(row(&february, groceries).available, cents(2_000));
    assert_eq!(row(&february, transport).available, cents(10_000));
    let payment = february.card(visa).unwrap();
    assert_eq!(payment.available, cents(-1_000));
    assert_eq!(payment.shortfall, cents(1_000));
    assert_eq!(payment.card_balance, Amount::ZERO);
    assert_conserved(&february);

    // Multiple cards receive funding in account order regardless of insert order.
    async fn two_card_view(reverse: bool) -> (Amount, Amount, Amount) {
        let directory = TempDir::new().unwrap();
        let store = open_store(&directory);
        let db = store.db();
        let _checking = open_checking(db, "Checking", cents(100_000)).await;
        let first = open_card(db, "First", Amount::ZERO).await;
        let second = open_card(db, "Second", Amount::ZERO).await;
        let groceries = open_category(db, "Everyday", "Groceries").await;
        assign(db, "2026-01", groceries, cents(5_000)).await;

        let first_purchase = (
            first,
            TransactionKind::Outflow,
            cents(-4_000),
            vec![NewAllocation {
                category_id: Some(groceries),
                amount: cents(-4_000),
            }],
        );
        let second_purchase = (
            second,
            TransactionKind::Outflow,
            cents(-4_000),
            vec![NewAllocation {
                category_id: Some(groceries),
                amount: cents(-4_000),
            }],
        );
        let purchases = if reverse {
            vec![second_purchase, first_purchase]
        } else {
            vec![first_purchase, second_purchase]
        };
        for (account, kind, amount, allocations) in purchases {
            entry(db, account, kind, amount, "2026-01-10", allocations).await;
        }

        let january = view(db, "2026-01", "2026-01-31").await;
        (
            january.card(first).unwrap().reserved,
            january.card(second).unwrap().reserved,
            row(&january, groceries).credit_overspending,
        )
    }

    let forward = two_card_view(false).await;
    let reverse = two_card_view(true).await;
    assert_eq!(forward, reverse);
    assert_eq!(
        forward,
        (cents(4_000), cents(1_000), cents(3_000))
    );
}

#[tokio::test]
async fn targets_report_distinct_progress_without_assigning_money() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let _checking = open_checking(db, "Checking", cents(10_000)).await;
    let savings = open_category(db, "Goals", "Savings").await;

    // $100 available toward a $400 target with three contribution months.
    assign(db, "2025-12", savings, cents(10_000)).await;
    let target = budget::set_category_target(
        db,
        savings,
        CategoryTarget::BalanceByMonth {
            amount: cents(40_000),
            month: month("2026-03"),
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(target.revision, 1);

    let january = view(db, "2026-01", "2026-01-31").await;
    let progress = row(&january, savings).target.expect("target progress");
    assert_eq!(progress.suggested_contribution, cents(10_000));
    match progress.measure {
        TargetProgressMeasure::Balance {
            available,
            remaining,
            months_remaining,
        } => {
            assert_eq!(available, cents(10_000));
            assert_eq!(remaining, cents(30_000));
            assert_eq!(months_remaining, 3);
        }
        TargetProgressMeasure::Contribution { .. } => panic!("expected balance progress"),
    }
    // Setting a target never assigns money.
    assert!(budget::assignments(db, Some(self::month("2026-01")))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(january.ready_to_assign, Amount::ZERO);
    assert_conserved(&january);

    // Cent remainders are deterministic and never auto-assigned.
    let small = open_category(db, "Goals", "Small goal").await;
    budget::set_category_target(
        db,
        small,
        CategoryTarget::BalanceByMonth {
            amount: cents(10_000),
            month: month("2026-03"),
        },
        None,
    )
    .await
    .unwrap();
    let january = view(db, "2026-01", "2026-01-31").await;
    let progress = row(&january, small).target.expect("target progress");
    assert_eq!(progress.suggested_contribution, cents(3_334));

    // Contribution progress counts assignments, not carryover or spending.
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();
    let checking = open_checking(db, "Checking", cents(10_000)).await;
    let groceries = open_category(db, "Everyday", "Groceries").await;
    assign(db, "2025-12", groceries, cents(10_000)).await;
    assign(db, "2026-01", groceries, cents(3_000)).await;
    entry(
        db,
        checking,
        TransactionKind::Outflow,
        cents(-2_000),
        "2026-01-10",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(-2_000),
        }],
    )
    .await;
    budget::set_category_target(
        db,
        groceries,
        CategoryTarget::MonthlyContribution { amount: cents(5_000) },
        None,
    )
    .await
    .unwrap();

    let january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(row(&january, groceries).carried_in, cents(10_000));
    assert_eq!(row(&january, groceries).available, cents(11_000));
    let progress = row(&january, groceries).target.expect("target progress");
    assert_eq!(progress.suggested_contribution, cents(2_000));
    match progress.measure {
        TargetProgressMeasure::Contribution { assigned, remaining } => {
            assert_eq!(assigned, cents(3_000));
            assert_eq!(remaining, cents(2_000));
        }
        TargetProgressMeasure::Balance { .. } => panic!("expected contribution progress"),
    }
    assert_conserved(&january);

    // Changing a target leaves assignments and availability untouched.
    let before = budget::assignments(db, None).await.unwrap();
    budget::set_category_target(
        db,
        groceries,
        CategoryTarget::MonthlyContribution { amount: cents(8_000) },
        Some(1),
    )
    .await
    .unwrap();
    assert_eq!(budget::assignments(db, None).await.unwrap(), before);
    let january = view(db, "2026-01", "2026-01-31").await;
    assert_eq!(row(&january, groceries).available, cents(11_000));
}
