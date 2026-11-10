use orchard::db::{Db, Store, StorePaths};
use orchard::domain::{
    AccountId, AccountKind, Amount, BudgetAccountKind, BudgetDate, CategoryId, ClearedState,
    TransactionKind, TransactionOrigin,
};
use orchard::ledger::{
    self, BoundaryTarget, CorrectionStatus, EditTransaction, EditTransfer, LedgerError,
    NewAccount, NewAllocation, NewSource, NewTransaction, NewTransfer, ReconciliationStatus,
    SourceStatus, ValuationSource,
};
use orchard::secrets::SecretKey;
use tempfile::TempDir;

fn key() -> SecretKey {
    SecretKey::from_bytes([13; 32])
}

fn open_store(directory: &TempDir) -> Store {
    Store::open(StorePaths::new(directory.path()), &key()).unwrap()
}

fn date(input: &str) -> BudgetDate {
    input.parse().unwrap()
}

fn cents(value: i64) -> Amount {
    Amount::from_cents(value)
}

async fn checking(db: &Db, name: &str, opening: Amount) -> AccountId {
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

async fn card(db: &Db, name: &str, opening: Amount) -> AccountId {
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

async fn tracking(db: &Db, name: &str, opening: Amount) -> AccountId {
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

async fn category(db: &Db, group: &str, name: &str) -> CategoryId {
    let group = ledger::create_category_group(db, group).await.unwrap();
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

#[tokio::test]
async fn account_lifecycle_creates_card_payment_category_and_closure_rules() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let card = card(db, "Visa", cents(-30_000)).await;
    let card_account = ledger::account(db, card).await.unwrap();
    assert!(card_account.kind.is_credit_card());
    assert!(card_account.payment_category_id.is_some());
    assert_eq!(
        ledger::account_balance(db, card, None).await.unwrap(),
        cents(-30_000)
    );
    assert_eq!(
        ledger::recognized_income(db, date("2026-12-31"))
            .await
            .unwrap(),
        Amount::ZERO
    );
    let opening = ledger::transactions_for_account(db, card).await.unwrap();
    assert_eq!(opening.len(), 1);
    assert_eq!(opening[0].kind, TransactionKind::OpeningBalance);
    assert_eq!(opening[0].origin, TransactionOrigin::Opening);
    assert!(opening[0].allocations.is_empty());

    let nonzero = checking(db, "Nonzero", cents(1_000)).await;
    let account = ledger::account(db, nonzero).await.unwrap();
    let error = ledger::close_account(db, nonzero, account.revision, date("2026-02-01"))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        LedgerError::NonzeroAccountBalance { id, .. } if id == nonzero
    ));

    let history = checking(db, "History", Amount::ZERO).await;
    let income = entry(
        db,
        history,
        TransactionKind::Income,
        cents(1_000),
        "2026-01-10",
        Vec::new(),
    )
    .await;
    entry(
        db,
        history,
        TransactionKind::Outflow,
        cents(-1_000),
        "2026-01-11",
        Vec::new(),
    )
    .await;
    let account = ledger::account(db, history).await.unwrap();
    let closed = ledger::close_account(db, history, account.revision, date("2026-02-01"))
        .await
        .unwrap();
    assert_eq!(closed.closed_on, Some(date("2026-02-01")));
    let error = ledger::create_transaction(
        db,
        NewTransaction {
            account_id: history,
            kind: TransactionKind::Income,
            date: date("2026-02-02"),
            payee: "Late",
            memo: "",
            amount: cents(10),
            allocations: Vec::new(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, LedgerError::AccountClosed { id } if id == history));
    let preserved = ledger::transactions_for_account(db, history).await.unwrap();
    assert_eq!(preserved.len(), 2);
    assert_eq!(preserved[0].id, income.id);

    let investment = tracking(db, "Brokerage", cents(100_000)).await;
    let valuation = ledger::latest_valuation(db, investment)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(valuation.value, cents(100_000));
    assert_eq!(
        ledger::net_worth_tracking_total(db).await.unwrap().total,
        cents(100_000)
    );
    let account = ledger::account(db, investment).await.unwrap();
    let closed = ledger::close_account(db, investment, account.revision, date("2026-02-01"))
        .await
        .unwrap();
    assert_eq!(closed.closed_on, Some(date("2026-02-01")));
}

#[tokio::test]
async fn manual_entries_splits_forecasts_revisions_and_archived_categories() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let account = checking(db, "Checking", Amount::ZERO).await;
    entry(
        db,
        account,
        TransactionKind::Income,
        cents(100_000),
        "2026-01-10",
        Vec::new(),
    )
    .await;
    entry(
        db,
        account,
        TransactionKind::Income,
        cents(50_000),
        "2026-02-10",
        Vec::new(),
    )
    .await;

    assert_eq!(
        ledger::account_balance(db, account, Some(date("2026-01-31")))
            .await
            .unwrap(),
        cents(100_000)
    );
    assert_eq!(
        ledger::recognized_income(db, date("2026-01-31"))
            .await
            .unwrap(),
        cents(100_000)
    );
    assert_eq!(
        ledger::recognized_income(db, date("2026-02-28"))
            .await
            .unwrap(),
        cents(150_000)
    );

    let groceries = category(db, "Everyday", "Groceries").await;
    let split = entry(
        db,
        account,
        TransactionKind::Outflow,
        cents(-10_000),
        "2026-01-15",
        vec![
            NewAllocation {
                category_id: Some(groceries),
                amount: cents(-6_000),
            },
            NewAllocation {
                category_id: None,
                amount: cents(-4_000),
            },
        ],
    )
    .await;
    assert_eq!(split.allocations.len(), 2);
    assert_eq!(
        split
            .allocations
            .iter()
            .map(|allocation| allocation.amount)
            .fold(Amount::ZERO, |total, amount| total.checked_add(amount).unwrap()),
        cents(-10_000)
    );
    let work = ledger::outstanding_category_work(db, date("2026-01-31"))
        .await
        .unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].transaction_id, split.id);

    let before = ledger::transactions_for_account(db, account).await.unwrap().len();
    let error = ledger::create_transaction(
        db,
        NewTransaction {
            account_id: account,
            kind: TransactionKind::Outflow,
            date: date("2026-01-16"),
            payee: "Test",
            memo: "",
            amount: cents(-10_000),
            allocations: vec![
                NewAllocation {
                    category_id: Some(groceries),
                    amount: cents(-6_000),
                },
                NewAllocation {
                    category_id: Some(groceries),
                    amount: cents(-3_000),
                },
            ],
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        LedgerError::AllocationSumMismatch { .. }
    ));
    assert_eq!(
        ledger::transactions_for_account(db, account).await.unwrap().len(),
        before
    );

    let refund = entry(
        db,
        account,
        TransactionKind::Refund,
        cents(1_500),
        "2026-01-20",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(1_500),
        }],
    )
    .await;
    assert_eq!(refund.kind, TransactionKind::Refund);
    assert_eq!(
        ledger::account_balance(db, account, Some(date("2026-01-31")))
            .await
            .unwrap(),
        cents(91_500)
    );

    let error = ledger::edit_transaction(
        db,
        split.id,
        99,
        EditTransaction {
            date: split.date,
            payee: "Changed",
            memo: "",
            amount: split.amount,
            allocations: None,
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, LedgerError::StaleRevision { .. }));
    assert_eq!(
        ledger::transaction(db, split.id).await.unwrap().payee,
        "Test"
    );

    let edited = ledger::edit_transaction(
        db,
        split.id,
        split.revision,
        EditTransaction {
            date: split.date,
            payee: "Changed",
            memo: "owner edit",
            amount: split.amount,
            allocations: Some(vec![
                NewAllocation {
                    category_id: Some(groceries),
                    amount: cents(-6_000),
                },
                NewAllocation {
                    category_id: Some(groceries),
                    amount: cents(-4_000),
                },
            ]),
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(edited.revision, split.revision + 1);
    assert_eq!(
        ledger::outstanding_category_work(db, date("2026-01-31"))
            .await
            .unwrap()
            .len(),
        0
    );

    let archived = ledger::archive_category(db, groceries).await.unwrap();
    assert!(archived.archived);
    let edited = ledger::edit_transaction(
        db,
        split.id,
        edited.revision,
        EditTransaction {
            date: edited.date,
            payee: "Changed again",
            memo: "historical archived category",
            amount: edited.amount,
            allocations: None,
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(edited.payee, "Changed again");
    assert_eq!(
        ledger::outstanding_category_work(db, date("2026-01-31"))
            .await
            .unwrap()
            .len(),
        0
    );
    let error = ledger::create_transaction(
        db,
        NewTransaction {
            account_id: account,
            kind: TransactionKind::Outflow,
            date: date("2026-01-21"),
            payee: "Test",
            memo: "",
            amount: cents(-1_000),
            allocations: vec![NewAllocation {
                category_id: Some(groceries),
                amount: cents(-1_000),
            }],
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, LedgerError::ArchivedCategory { id } if id == groceries));

    let removable = entry(
        db,
        account,
        TransactionKind::Outflow,
        cents(-1_000),
        "2026-01-22",
        Vec::new(),
    )
    .await;
    let error = ledger::delete_transaction(db, removable.id, removable.revision + 1, false)
        .await
        .unwrap_err();
    assert!(matches!(error, LedgerError::StaleRevision { .. }));
    assert!(ledger::transaction(db, removable.id).await.is_ok());
    ledger::delete_transaction(db, removable.id, removable.revision, false)
        .await
        .unwrap();
    assert!(matches!(
        ledger::transaction(db, removable.id).await.unwrap_err(),
        LedgerError::TransactionNotFound { .. }
    ));
}

#[tokio::test]
async fn linked_transfers_update_both_legs_and_ignore_valuation_references() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let account = checking(db, "Checking", cents(100_000)).await;
    let card = card(db, "Visa", Amount::ZERO).await;

    let transfer = ledger::create_transfer(
        db,
        NewTransfer {
            from_account_id: account,
            to_account_id: card,
            amount: cents(8_000),
            date: date("2026-01-15"),
            payee: "Card payment",
            memo: "",
            boundary: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(transfer.from.amount, cents(-8_000));
    assert_eq!(transfer.to.amount, cents(8_000));
    assert_eq!(transfer.boundary, ledger::TransferBoundary::Budget);
    assert_eq!(transfer.boundary_target, None);
    assert_eq!(
        ledger::account_balance(db, account, None).await.unwrap(),
        cents(92_000)
    );
    assert_eq!(
        ledger::account_balance(db, card, None).await.unwrap(),
        cents(8_000)
    );
    assert_eq!(
        ledger::recognized_income(db, date("2026-12-31"))
            .await
            .unwrap(),
        Amount::ZERO
    );
    assert!(
        ledger::outstanding_category_work(db, date("2026-12-31"))
            .await
            .unwrap()
            .is_empty()
    );

    let edited = ledger::edit_transfer(
        db,
        transfer.id,
        EditTransfer {
            amount: cents(9_000),
            date: date("2026-01-16"),
            payee: "Card payment",
            memo: "revised",
            boundary: None,
            expected_from_revision: transfer.from.revision,
            expected_to_revision: transfer.to.revision,
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(edited.from.amount, cents(-9_000));
    assert_eq!(edited.to.amount, cents(9_000));
    assert_eq!(
        ledger::account_balance(db, account, None).await.unwrap(),
        cents(91_000)
    );
    let checking_entries = ledger::transactions_for_account(db, account).await.unwrap();
    let card_entries = ledger::transactions_for_account(db, card).await.unwrap();
    assert_eq!(
        checking_entries
            .iter()
            .filter(|entry| entry.kind == TransactionKind::Transfer)
            .count(),
        1
    );
    assert_eq!(
        card_entries
            .iter()
            .filter(|entry| entry.kind == TransactionKind::Transfer)
            .count(),
        1
    );

    ledger::delete_transfer(
        db,
        edited.id,
        edited.from.revision,
        edited.to.revision,
        false,
    )
    .await
    .unwrap();
    assert!(matches!(
        ledger::transfer(db, edited.id).await.unwrap_err(),
        LedgerError::TransferNotFound { .. }
    ));
    assert_eq!(
        ledger::account_balance(db, account, None).await.unwrap(),
        cents(100_000)
    );

    let investment = tracking(db, "Brokerage", Amount::ZERO).await;
    let contribution = ledger::create_transfer(
        db,
        NewTransfer {
            from_account_id: account,
            to_account_id: investment,
            amount: cents(20_000),
            date: date("2026-01-20"),
            payee: "Investment",
            memo: "",
            boundary: Some(BoundaryTarget::ReadyToAssign),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        contribution.boundary,
        ledger::TransferBoundary::BudgetTracking
    );
    assert_eq!(
        contribution.boundary_target,
        Some(BoundaryTarget::ReadyToAssign)
    );
    assert_eq!(
        ledger::account_balance(db, investment, None).await.unwrap(),
        cents(20_000)
    );
    ledger::record_valuation(
        db,
        investment,
        date("2026-01-31"),
        cents(500_000),
        ValuationSource::Provider,
    )
    .await
    .unwrap();
    let latest = ledger::latest_valuation(db, investment)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.value, cents(500_000));
    let total = ledger::net_worth_tracking_total(db).await.unwrap();
    assert_eq!(total.total, cents(500_000));
    assert!(total.missing.is_empty());
    assert_ne!(total.total, cents(520_000));

    let error = ledger::create_transfer(
        db,
        NewTransfer {
            from_account_id: account,
            to_account_id: investment,
            amount: cents(100),
            date: date("2026-01-21"),
            payee: "",
            memo: "",
            boundary: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, LedgerError::BoundaryTargetRequired));

    let second_tracking = tracking(db, "Other brokerage", Amount::ZERO).await;
    let error = ledger::create_transfer(
        db,
        NewTransfer {
            from_account_id: investment,
            to_account_id: second_tracking,
            amount: cents(100),
            date: date("2026-01-21"),
            payee: "",
            memo: "",
            boundary: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, LedgerError::TrackingToTrackingTransfer));

    let stale = ledger::create_transfer(
        db,
        NewTransfer {
            from_account_id: account,
            to_account_id: investment,
            amount: cents(500),
            date: date("2026-01-22"),
            payee: "",
            memo: "",
            boundary: Some(BoundaryTarget::ReadyToAssign),
        },
    )
    .await
    .unwrap();
    let error = ledger::edit_transfer(
        db,
        stale.id,
        EditTransfer {
            amount: cents(600),
            date: stale.from.date,
            payee: "",
            memo: "",
            boundary: Some(BoundaryTarget::ReadyToAssign),
            expected_from_revision: stale.from.revision,
            expected_to_revision: stale.to.revision + 1,
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, LedgerError::StaleRevision { .. }));
    let unchanged = ledger::transfer(db, stale.id).await.unwrap();
    assert_eq!(unchanged.from.amount, cents(-500));
    assert_eq!(unchanged.to.amount, cents(500));
}

#[tokio::test]
async fn reconciliation_reports_mismatch_and_requires_explicit_adjustment() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let account = checking(db, "Checking", cents(100_000)).await;
    let outflow = entry(
        db,
        account,
        TransactionKind::Outflow,
        cents(-1_000),
        "2026-01-15",
        Vec::new(),
    )
    .await;
    let outflow = ledger::set_cleared_state(
        db,
        outflow.id,
        outflow.revision,
        ClearedState::Cleared,
        false,
    )
    .await
    .unwrap();

    let reconciliation = ledger::compare_statement(
        db,
        account,
        date("2026-01-31"),
        cents(100_000),
    )
    .await
    .unwrap();
    assert_eq!(reconciliation.cleared_balance, cents(99_000));
    assert_eq!(reconciliation.difference, cents(1_000));
    assert_eq!(reconciliation.status, ReconciliationStatus::Open);

    let error = ledger::complete_reconciliation(db, reconciliation.id, reconciliation.revision)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        LedgerError::ReconciliationMismatch { difference } if difference == cents(1_000)
    ));
    let adjustments_before = ledger::transactions_for_account(db, account)
        .await
        .unwrap()
        .into_iter()
        .filter(|entry| entry.kind == TransactionKind::Adjustment)
        .count();
    assert_eq!(adjustments_before, 0);

    let adjustment = ledger::create_reconciliation_adjustment(
        db,
        reconciliation.id,
        reconciliation.revision,
        "Bank fee recorded explicitly",
    )
    .await
    .unwrap();
    assert_eq!(adjustment.kind, TransactionKind::Adjustment);
    assert_eq!(adjustment.origin, TransactionOrigin::Adjustment);
    assert_eq!(adjustment.amount, cents(1_000));
    let reconciliation = ledger::reconciliation(db, reconciliation.id)
        .await
        .unwrap();
    assert_eq!(reconciliation.difference, Amount::ZERO);

    let completed =
        ledger::complete_reconciliation(db, reconciliation.id, reconciliation.revision)
            .await
            .unwrap();
    assert_eq!(completed.status, ReconciliationStatus::Completed);
    let entries = ledger::transactions_for_account(db, account).await.unwrap();
    assert!(
        entries
            .iter()
            .all(|entry| entry.cleared == ClearedState::Reconciled)
    );

    let current = ledger::transaction(db, outflow.id).await.unwrap();
    let error = ledger::edit_transaction(
        db,
        current.id,
        current.revision,
        EditTransaction {
            date: current.date,
            payee: &current.payee,
            memo: &current.memo,
            amount: cents(-2_000),
            allocations: Some(vec![NewAllocation {
                category_id: None,
                amount: cents(-2_000),
            }]),
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        LedgerError::ReconciledEditRequiresConfirmation { id } if id == current.id
    ));

    let edited = ledger::edit_transaction(
        db,
        current.id,
        current.revision,
        EditTransaction {
            date: current.date,
            payee: &current.payee,
            memo: "confirmed correction",
            amount: cents(-2_000),
            allocations: Some(vec![NewAllocation {
                category_id: None,
                amount: cents(-2_000),
            }]),
            confirm_reconciled: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(edited.cleared, ClearedState::Cleared);
    let reconciliations = ledger::reconciliations_for_account(db, account)
        .await
        .unwrap();
    assert_eq!(
        reconciliations.last().unwrap().status,
        ReconciliationStatus::Invalidated
    );
}

#[tokio::test]
async fn import_review_matching_corrections_and_tombstones() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let account = checking(db, "Checking", Amount::ZERO).await;
    let connection = ledger::create_provider_connection(db, "simplefin", "Test bank")
        .await
        .unwrap();

    let first = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst-1",
            external_account_id: "acct-1",
            external_transaction_id: "txn-1",
            account_id: account,
            date: date("2026-01-10"),
            amount: cents(-2_500),
            payee: "Coffee",
            memo: "posted",
        },
    )
    .await
    .unwrap();
    assert!(first.created_entry);
    assert_eq!(first.source.status, SourceStatus::PendingReview);
    assert!(first.source.transaction_id.is_some());
    let imported = ledger::transaction(db, first.source.transaction_id.unwrap())
        .await
        .unwrap();
    assert_eq!(imported.origin, TransactionOrigin::Import);
    assert_eq!(imported.kind, TransactionKind::Outflow);
    assert_eq!(imported.allocations.len(), 1);
    assert_eq!(imported.allocations[0].category_id, None);
    assert_eq!(
        ledger::account_balance(db, account, None).await.unwrap(),
        cents(-2_500)
    );

    let approved = ledger::approve_source(db, first.source.id, first.source.revision)
        .await
        .unwrap();
    assert_eq!(approved.status, SourceStatus::Approved);
    assert_eq!(
        ledger::transactions_for_account(db, account).await.unwrap().len(),
        1
    );

    let repeat = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst-1",
            external_account_id: "acct-1",
            external_transaction_id: "txn-1",
            account_id: account,
            date: date("2026-01-10"),
            amount: cents(-2_500),
            payee: "Coffee",
            memo: "posted",
        },
    )
    .await
    .unwrap();
    assert!(!repeat.created_entry);
    assert_eq!(
        ledger::transactions_for_account(db, account).await.unwrap().len(),
        1
    );

    let manual_a = entry(
        db,
        account,
        TransactionKind::Outflow,
        cents(-1_200),
        "2026-01-12",
        Vec::new(),
    )
    .await;
    let manual_b = entry(
        db,
        account,
        TransactionKind::Outflow,
        cents(-1_200),
        "2026-01-13",
        Vec::new(),
    )
    .await;
    let ambiguous = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst-1",
            external_account_id: "acct-1",
            external_transaction_id: "txn-2",
            account_id: account,
            date: date("2026-01-12"),
            amount: cents(-1_200),
            payee: "Ambiguous",
            memo: "",
        },
    )
    .await
    .unwrap();
    assert!(ambiguous.created_entry);
    let candidates = ledger::match_candidates(db, ambiguous.source.id)
        .await
        .unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].transaction_id, manual_a.id);
    assert_eq!(candidates[1].transaction_id, manual_b.id);

    let matched = ledger::match_source(
        db,
        ambiguous.source.id,
        manual_a.id,
        ambiguous.source.revision,
        false,
    )
    .await
    .unwrap();
    assert_eq!(matched.status, SourceStatus::Approved);
    assert_eq!(matched.transaction_id, Some(manual_a.id));
    let preserved = ledger::transaction(db, manual_a.id).await.unwrap();
    assert_eq!(preserved.origin, TransactionOrigin::Manual);
    assert_eq!(preserved.payee, "Test");
    assert_eq!(preserved.allocations.len(), 1);
    assert_eq!(
        ledger::transactions_for_account(db, account).await.unwrap().len(),
        3
    );

    let deleted = ledger::delete_source(db, matched.id, matched.revision, false)
        .await
        .unwrap();
    assert_eq!(deleted.status, SourceStatus::Deleted);
    let preserved = ledger::transaction(db, manual_a.id).await.unwrap();
    assert_eq!(preserved.origin, TransactionOrigin::Manual);
    let after_delete = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst-1",
            external_account_id: "acct-1",
            external_transaction_id: "txn-2",
            account_id: account,
            date: date("2026-01-12"),
            amount: cents(-1_200),
            payee: "Ambiguous",
            memo: "",
        },
    )
    .await
    .unwrap();
    assert!(!after_delete.created_entry);
    assert_eq!(after_delete.source.status, SourceStatus::Deleted);
    assert_eq!(
        ledger::transactions_for_account(db, account).await.unwrap().len(),
        3
    );

    let correction_source = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst-1",
            external_account_id: "acct-1",
            external_transaction_id: "txn-3",
            account_id: account,
            date: date("2026-01-15"),
            amount: cents(-2_500),
            payee: "Correction",
            memo: "",
        },
    )
    .await
    .unwrap();
    let changed = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst-1",
            external_account_id: "acct-1",
            external_transaction_id: "txn-3",
            account_id: account,
            date: date("2026-01-16"),
            amount: cents(-3_000),
            payee: "Correction",
            memo: "",
        },
    )
    .await
    .unwrap();
    assert!(!changed.created_entry);
    let correction = changed.correction.unwrap();
    assert_eq!(correction.status, CorrectionStatus::Pending);
    assert_eq!(correction.previous_amount, cents(-2_500));
    assert_eq!(correction.proposed_amount, cents(-3_000));
    let unchanged = ledger::transaction(db, correction_source.source.transaction_id.unwrap())
        .await
        .unwrap();
    assert_eq!(unchanged.amount, cents(-2_500));

    let corrected = ledger::apply_source_correction(
        db,
        correction.id,
        unchanged.revision,
        false,
    )
    .await
    .unwrap();
    assert_eq!(corrected.amount, cents(-3_000));
    assert_eq!(corrected.date, date("2026-01-16"));
    assert_eq!(corrected.allocations.len(), 1);
    assert_eq!(corrected.allocations[0].amount, cents(-3_000));
    let applied = ledger::source_corrections(db, correction_source.source.id)
        .await
        .unwrap();
    assert_eq!(applied[0].status, CorrectionStatus::Applied);

    let split = entry(
        db,
        account,
        TransactionKind::Outflow,
        cents(-10_000),
        "2026-01-20",
        vec![
            NewAllocation {
                category_id: None,
                amount: cents(-5_000),
            },
            NewAllocation {
                category_id: None,
                amount: cents(-5_000),
            },
        ],
    )
    .await;
    let split_source = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst-1",
            external_account_id: "acct-1",
            external_transaction_id: "txn-4",
            account_id: account,
            date: date("2026-01-20"),
            amount: cents(-10_000),
            payee: "Split",
            memo: "",
        },
    )
    .await
    .unwrap();
    let split_matched = ledger::match_source(
        db,
        split_source.source.id,
        split.id,
        split_source.source.revision,
        false,
    )
    .await
    .unwrap();
    let split_changed = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst-1",
            external_account_id: "acct-1",
            external_transaction_id: "txn-4",
            account_id: account,
            date: date("2026-01-20"),
            amount: cents(-12_000),
            payee: "Split",
            memo: "",
        },
    )
    .await
    .unwrap();
    let split_correction = split_changed.correction.unwrap();
    let error = ledger::apply_source_correction(
        db,
        split_correction.id,
        split.revision,
        false,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        LedgerError::CorrectionRequiresManualEdit { .. }
    ));
    let still = ledger::transaction(db, split.id).await.unwrap();
    assert_eq!(still.amount, cents(-10_000));
    let dismissed =
        ledger::dismiss_source_correction(db, split_correction.id).await.unwrap();
    assert_eq!(dismissed.status, CorrectionStatus::Dismissed);
    assert_eq!(
        ledger::transaction(db, split.id).await.unwrap().amount,
        cents(-10_000)
    );
    assert_eq!(split_matched.status, SourceStatus::Approved);
}

#[tokio::test]
async fn documented_ledger_examples_have_exact_balances() {
    let directory = TempDir::new().unwrap();
    let store = open_store(&directory);
    let db = store.db();

    let account = checking(db, "Checking", cents(100_000)).await;
    let groceries = category(db, "Everyday", "Groceries").await;
    let split = entry(
        db,
        account,
        TransactionKind::Outflow,
        cents(-10_000),
        "2026-01-15",
        vec![
            NewAllocation {
                category_id: Some(groceries),
                amount: cents(-6_000),
            },
            NewAllocation {
                category_id: None,
                amount: cents(-4_000),
            },
        ],
    )
    .await;
    assert_eq!(
        ledger::account_balance(db, account, None).await.unwrap(),
        cents(90_000)
    );
    assert_eq!(
        ledger::outstanding_category_work(db, date("2026-01-31"))
            .await
            .unwrap()
            .len(),
        1
    );

    let split = ledger::edit_transaction(
        db,
        split.id,
        split.revision,
        EditTransaction {
            date: split.date,
            payee: &split.payee,
            memo: &split.memo,
            amount: split.amount,
            allocations: Some(vec![
                NewAllocation {
                    category_id: Some(groceries),
                    amount: cents(-6_000),
                },
                NewAllocation {
                    category_id: Some(groceries),
                    amount: cents(-4_000),
                },
            ]),
            confirm_reconciled: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(split.amount, cents(-10_000));
    assert_eq!(
        ledger::outstanding_category_work(db, date("2026-01-31"))
            .await
            .unwrap()
            .len(),
        0
    );

    entry(
        db,
        account,
        TransactionKind::Refund,
        cents(1_500),
        "2026-01-20",
        vec![NewAllocation {
            category_id: Some(groceries),
            amount: cents(1_500),
        }],
    )
    .await;
    assert_eq!(
        ledger::account_balance(db, account, None).await.unwrap(),
        cents(91_500)
    );

    let card = card(db, "Visa", cents(-30_000)).await;
    assert!(ledger::account(db, card)
        .await
        .unwrap()
        .payment_category_id
        .is_some());
    ledger::create_transfer(
        db,
        NewTransfer {
            from_account_id: account,
            to_account_id: card,
            amount: cents(8_000),
            date: date("2026-01-25"),
            payee: "Card payment",
            memo: "",
            boundary: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        ledger::account_balance(db, account, None).await.unwrap(),
        cents(83_500)
    );
    assert_eq!(
        ledger::account_balance(db, card, None).await.unwrap(),
        cents(-22_000)
    );
    assert_eq!(
        ledger::recognized_income(db, date("2026-01-31"))
            .await
            .unwrap(),
        Amount::ZERO
    );
    assert!(
        ledger::outstanding_category_work(db, date("2026-01-31"))
            .await
            .unwrap()
            .is_empty()
    );

    let connection = ledger::create_provider_connection(db, "simplefin", "Bank")
        .await
        .unwrap();
    let imported = ledger::ingest_source(
        db,
        NewSource {
            connection_id: connection,
            institution_connection_id: "inst",
            external_account_id: "acct",
            external_transaction_id: "txn",
            account_id: account,
            date: date("2026-01-26"),
            amount: cents(-2_500),
            payee: "Imported purchase",
            memo: "",
        },
    )
    .await
    .unwrap();
    assert!(imported.created_entry);
    assert_eq!(
        ledger::account_balance(db, account, None).await.unwrap(),
        cents(81_000)
    );
    assert_eq!(
        ledger::outstanding_category_work(db, date("2026-01-31"))
            .await
            .unwrap()
            .len(),
        1
    );
}
