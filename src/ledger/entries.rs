use rusqlite::params;

use crate::db::Db;
use crate::domain::{
    AccountId, ClearedState, TransactionId, TransactionKind,
};

use super::rows::{
    insert_allocations, insert_entry, invalidate_reconciliations, load_account,
    load_transaction, mark_linked_source_deleted, now_unix, validate_existing_allocations,
    validate_new_allocations, NewEntry,
};
use super::{
    EditTransaction, LedgerError, NewTransaction, TransactionRecord,
};

pub async fn create_transaction(
    db: &Db,
    new: NewTransaction<'_>,
) -> Result<TransactionRecord, LedgerError> {
    validate_text(new.payee, 200, LedgerError::InvalidPayee)?;
    validate_text(new.memo, 1000, LedgerError::InvalidMemo)?;
    let payee = new.payee.to_owned();
    let memo = new.memo.to_owned();
    let NewTransaction {
        account_id,
        kind,
        date,
        amount,
        allocations,
        ..
    } = new;
    db.run_transaction(move |tx| {
        let account = load_account(tx, account_id)?;
        if account.closed_on.is_some() {
            return Err(LedgerError::AccountClosed { id: account_id });
        }
        if !account.kind.is_budget() {
            return Err(LedgerError::TrackingAccountOrdinaryActivity { id: account_id });
        }
        validate_kind_and_sign(kind, amount)?;
        let allocations = validate_new_allocations(tx, kind, amount, &allocations)?;
        let id = insert_entry(
            tx,
            NewEntry {
                account_id,
                kind,
                origin: crate::domain::TransactionOrigin::Manual,
                date,
                payee: &payee,
                memo: &memo,
                amount,
                cleared: ClearedState::Uncleared,
            },
        )?;
        insert_allocations(tx, id, &allocations)?;
        load_transaction(tx, id)
    })
    .await
}

pub async fn edit_transaction(
    db: &Db,
    id: TransactionId,
    expected_revision: i64,
    edit: EditTransaction<'_>,
) -> Result<TransactionRecord, LedgerError> {
    validate_text(edit.payee, 200, LedgerError::InvalidPayee)?;
    validate_text(edit.memo, 1000, LedgerError::InvalidMemo)?;
    let payee = edit.payee.to_owned();
    let memo = edit.memo.to_owned();
    let EditTransaction {
        date,
        amount,
        allocations,
        confirm_reconciled,
        ..
    } = edit;
    db.run_transaction(move |tx| {
        let existing = load_transaction(tx, id)?;
        if existing.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "transaction",
                id: id.value(),
                expected: expected_revision,
                actual: existing.revision,
            });
        }
        if existing.kind == TransactionKind::Transfer {
            return Err(LedgerError::UseTransferCommands);
        }
        validate_kind_and_sign(existing.kind, amount)?;
        let allocations = match &allocations {
            Some(allocations) => {
                validate_new_allocations(tx, existing.kind, amount, allocations)?
            }
            None => {
                validate_existing_allocations(existing.kind, amount, &existing.allocations)?
            }
        };

        let reconciled_change = existing.cleared == ClearedState::Reconciled
            && (existing.amount != amount || existing.date != date);
        if reconciled_change && !confirm_reconciled {
            return Err(LedgerError::ReconciledEditRequiresConfirmation { id });
        }
        let cleared = if reconciled_change {
            ClearedState::Cleared
        } else {
            existing.cleared
        };

        tx.execute(
            "UPDATE transactions
             SET transaction_date = ?1, payee = ?2, memo = ?3, amount_cents = ?4,
                 cleared_state = ?5, revision = revision + 1, updated_at = ?6
             WHERE id = ?7",
            params![
                date.to_string(),
                payee,
                memo,
                amount.cents(),
                cleared.as_str(),
                now_unix(),
                id.value()
            ],
        )?;
        tx.execute(
            "DELETE FROM allocations WHERE transaction_id = ?1",
            [id.value()],
        )?;
        insert_allocations(tx, id, &allocations)?;

        if reconciled_change {
            let from_date = if existing.date < date {
                existing.date
            } else {
                date
            };
            invalidate_reconciliations(tx, existing.account_id, from_date)?;
        }

        load_transaction(tx, id)
    })
    .await
}

pub async fn delete_transaction(
    db: &Db,
    id: TransactionId,
    expected_revision: i64,
    confirm_reconciled: bool,
) -> Result<(), LedgerError> {
    db.run_transaction(move |tx| {
        let existing = load_transaction(tx, id)?;
        if existing.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "transaction",
                id: id.value(),
                expected: expected_revision,
                actual: existing.revision,
            });
        }
        if existing.kind == TransactionKind::Transfer || existing.transfer_id.is_some() {
            return Err(LedgerError::UseTransferCommands);
        }
        if existing.cleared == ClearedState::Reconciled && !confirm_reconciled {
            return Err(LedgerError::ReconciledEditRequiresConfirmation { id });
        }
        if existing.cleared == ClearedState::Reconciled {
            invalidate_reconciliations(tx, existing.account_id, existing.date)?;
        }
        mark_linked_source_deleted(tx, id)?;
        tx.execute("DELETE FROM transactions WHERE id = ?1", [id.value()])?;
        Ok(())
    })
    .await
}

pub async fn set_cleared_state(
    db: &Db,
    id: TransactionId,
    expected_revision: i64,
    state: ClearedState,
    confirm_reconciled: bool,
) -> Result<TransactionRecord, LedgerError> {
    if state == ClearedState::Reconciled {
        return Err(LedgerError::ReconciliationRequired);
    }
    db.run_transaction(move |tx| {
        let existing = load_transaction(tx, id)?;
        if existing.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "transaction",
                id: id.value(),
                expected: expected_revision,
                actual: existing.revision,
            });
        }
        if existing.cleared == state {
            return Ok(existing);
        }
        if existing.cleared == ClearedState::Reconciled && !confirm_reconciled {
            return Err(LedgerError::ReconciledEditRequiresConfirmation { id });
        }
        if existing.cleared == ClearedState::Reconciled {
            invalidate_reconciliations(tx, existing.account_id, existing.date)?;
        }
        tx.execute(
            "UPDATE transactions
             SET cleared_state = ?1, revision = revision + 1, updated_at = ?2
             WHERE id = ?3",
            params![state.as_str(), now_unix(), id.value()],
        )?;
        load_transaction(tx, id)
    })
    .await
}

pub async fn transaction(
    db: &Db,
    id: TransactionId,
) -> Result<TransactionRecord, LedgerError> {
    db.run_read(move |conn| load_transaction(conn, id)).await
}

pub async fn transactions_for_account(
    db: &Db,
    account_id: AccountId,
) -> Result<Vec<TransactionRecord>, LedgerError> {
    db.run_read(move |conn| {
        let ids: Vec<i64> = {
            let mut statement = conn.prepare(
                "SELECT id FROM transactions
                 WHERE account_id = ?1
                 ORDER BY transaction_date, id",
            )?;
            let rows = statement.query_map([account_id.value()], |row| {
                row.get::<_, i64>(0)
            })?;
            rows.collect::<Result<_, _>>()?
        };
        ids.into_iter()
            .map(|id| load_transaction(conn, TransactionId::new(id)))
            .collect()
    })
    .await
}

pub(super) fn validate_kind_and_sign(
    kind: TransactionKind,
    amount: crate::domain::Amount,
) -> Result<(), LedgerError> {
    match kind {
        TransactionKind::Income if amount.is_positive() => Ok(()),
        TransactionKind::Outflow if amount.is_negative() => Ok(()),
        TransactionKind::Refund if amount.is_positive() => Ok(()),
        TransactionKind::Income | TransactionKind::Outflow | TransactionKind::Refund => {
            Err(LedgerError::InvalidAmountSign { kind, amount })
        }
        TransactionKind::Transfer => Err(LedgerError::UseTransferCommands),
        TransactionKind::OpeningBalance | TransactionKind::Adjustment => {
            Err(LedgerError::ReservedTransactionKind { kind })
        }
    }
}

fn validate_text(
    value: &str,
    max_chars: usize,
    invalid: LedgerError,
) -> Result<(), LedgerError> {
    if value.chars().count() > max_chars {
        Err(invalid)
    } else {
        Ok(())
    }
}
