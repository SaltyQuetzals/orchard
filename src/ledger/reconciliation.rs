use rusqlite::params;

use crate::db::Db;
use crate::domain::{
    AccountId, BudgetDate, ClearedState, TransactionKind, TransactionOrigin,
};

use super::rows::{
    cleared_balance_in, insert_entry, load_account, load_reconciliation, load_transaction,
    now_unix, NewEntry,
};
use super::{
    LedgerError, ReconciliationRecord, ReconciliationStatus,
};

pub async fn compare_statement(
    db: &Db,
    account_id: AccountId,
    statement_date: BudgetDate,
    statement_balance: crate::domain::Amount,
) -> Result<ReconciliationRecord, LedgerError> {
    db.run_transaction(move |tx| {
        let account = load_account(tx, account_id)?;
        if !account.kind.is_budget() {
            return Err(LedgerError::ReconciliationRequiresBudgetAccount { id: account_id });
        }
        let cleared_balance = cleared_balance_in(tx, account_id, statement_date)?;
        let difference = statement_balance.checked_sub(cleared_balance)?;
        tx.execute(
            "INSERT INTO reconciliations (
                account_id, statement_date, statement_balance_cents,
                cleared_balance_cents, difference_cents, adjustment_transaction_id,
                status, revision, created_at, completed_at, invalidated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'open', 1, ?6, NULL, NULL)",
            params![
                account_id.value(),
                statement_date.to_string(),
                statement_balance.cents(),
                cleared_balance.cents(),
                difference.cents(),
                now_unix()
            ],
        )?;
        load_reconciliation(tx, tx.last_insert_rowid())
    })
    .await
}

pub async fn create_reconciliation_adjustment(
    db: &Db,
    id: i64,
    expected_revision: i64,
    memo: &str,
) -> Result<super::TransactionRecord, LedgerError> {
    let memo = memo.to_owned();
    if memo.chars().count() > 1000 {
        return Err(LedgerError::InvalidMemo);
    }
    db.run_transaction(move |tx| {
        let reconciliation = load_reconciliation(tx, id)?;
        if reconciliation.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "reconciliation",
                id,
                expected: expected_revision,
                actual: reconciliation.revision,
            });
        }
        if reconciliation.status != ReconciliationStatus::Open {
            return Err(LedgerError::ReconciliationNotOpen { id });
        }
        let cleared_balance =
            cleared_balance_in(tx, reconciliation.account_id, reconciliation.statement_date)?;
        let difference = reconciliation.statement_balance.checked_sub(cleared_balance)?;
        if difference.is_zero() {
            return Err(LedgerError::NoAdjustmentNeeded);
        }
        let adjustment = insert_entry(
            tx,
            NewEntry {
                account_id: reconciliation.account_id,
                kind: TransactionKind::Adjustment,
                origin: TransactionOrigin::Adjustment,
                date: reconciliation.statement_date,
                payee: "Reconciliation adjustment",
                memo: &memo,
                amount: difference,
                cleared: ClearedState::Cleared,
            },
        )?;
        tx.execute(
            "UPDATE reconciliations
             SET cleared_balance_cents = ?1, difference_cents = 0,
                 adjustment_transaction_id = ?2, revision = revision + 1
             WHERE id = ?3",
            params![
                reconciliation.statement_balance.cents(),
                adjustment.value(),
                id
            ],
        )?;
        load_transaction(tx, adjustment)
    })
    .await
}

pub async fn complete_reconciliation(
    db: &Db,
    id: i64,
    expected_revision: i64,
) -> Result<ReconciliationRecord, LedgerError> {
    db.run_transaction(move |tx| {
        let reconciliation = load_reconciliation(tx, id)?;
        if reconciliation.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "reconciliation",
                id,
                expected: expected_revision,
                actual: reconciliation.revision,
            });
        }
        if reconciliation.status != ReconciliationStatus::Open {
            return Err(LedgerError::ReconciliationNotOpen { id });
        }
        let cleared_balance =
            cleared_balance_in(tx, reconciliation.account_id, reconciliation.statement_date)?;
        let difference = reconciliation.statement_balance.checked_sub(cleared_balance)?;
        if !difference.is_zero() {
            return Err(LedgerError::ReconciliationMismatch { difference });
        }
        let now = now_unix();
        tx.execute(
            "UPDATE transactions
             SET cleared_state = 'reconciled', revision = revision + 1, updated_at = ?1
             WHERE account_id = ?2
               AND transaction_date <= ?3
               AND cleared_state = 'cleared'",
            params![
                now,
                reconciliation.account_id.value(),
                reconciliation.statement_date.to_string()
            ],
        )?;
        tx.execute(
            "UPDATE reconciliations
             SET status = 'completed', completed_at = ?1,
                 cleared_balance_cents = ?2, difference_cents = 0,
                 revision = revision + 1
             WHERE id = ?3",
            params![now, reconciliation.statement_balance.cents(), id],
        )?;
        load_reconciliation(tx, id)
    })
    .await
}

pub async fn reconciliation(db: &Db, id: i64) -> Result<ReconciliationRecord, LedgerError> {
    db.run_read(move |conn| load_reconciliation(conn, id)).await
}

pub async fn reconciliations_for_account(
    db: &Db,
    account_id: AccountId,
) -> Result<Vec<ReconciliationRecord>, LedgerError> {
    db.run_read(move |conn| {
        let ids: Vec<i64> = {
            let mut statement = conn.prepare(
                "SELECT id FROM reconciliations
                 WHERE account_id = ?1
                 ORDER BY statement_date, id",
            )?;
            let rows = statement.query_map([account_id.value()], |row| {
                row.get::<_, i64>(0)
            })?;
            rows.collect::<Result<_, _>>()?
        };
        ids.into_iter()
            .map(|id| load_reconciliation(conn, id))
            .collect()
    })
    .await
}
