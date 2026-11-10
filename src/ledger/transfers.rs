use rusqlite::{Connection, params};

use crate::db::Db;
use crate::domain::{
    AccountKind, ClearedState, TransactionId, TransactionKind, TransactionOrigin,
};

use super::rows::{
    insert_entry, invalidate_reconciliations, load_account, load_category, load_transfer,
    mark_linked_source_deleted, now_unix, NewEntry,
};
use super::{
    BoundaryTarget, EditTransfer, LedgerError, NewTransfer, TransferBoundary, TransferRecord,
};

pub async fn create_transfer(
    db: &Db,
    new: NewTransfer<'_>,
) -> Result<TransferRecord, LedgerError> {
    if !new.amount.is_positive() {
        return Err(LedgerError::InvalidTransferAmount);
    }
    validate_text(new.payee, 200, LedgerError::InvalidPayee)?;
    validate_text(new.memo, 1000, LedgerError::InvalidMemo)?;
    let payee = new.payee.to_owned();
    let memo = new.memo.to_owned();
    let NewTransfer {
        from_account_id,
        to_account_id,
        amount,
        date,
        boundary: requested_boundary,
        ..
    } = new;
    db.run_transaction(move |tx| {
        if from_account_id == to_account_id {
            return Err(LedgerError::TransferAccountsMustDiffer);
        }
        let from = load_account(tx, from_account_id)?;
        let to = load_account(tx, to_account_id)?;
        if from.closed_on.is_some() {
            return Err(LedgerError::AccountClosed { id: from.id });
        }
        if to.closed_on.is_some() {
            return Err(LedgerError::AccountClosed { id: to.id });
        }
        let (boundary, target, category_id) =
            resolve_boundary(tx, from.kind, to.kind, requested_boundary)?;

        let from_entry = insert_entry(
            tx,
            NewEntry {
                account_id: from.id,
                kind: TransactionKind::Transfer,
                origin: TransactionOrigin::Manual,
                date,
                payee: &payee,
                memo: &memo,
                amount: amount.checked_neg()?,
                cleared: ClearedState::Uncleared,
            },
        )?;
        let to_entry = insert_entry(
            tx,
            NewEntry {
                account_id: to.id,
                kind: TransactionKind::Transfer,
                origin: TransactionOrigin::Manual,
                date,
                payee: &payee,
                memo: &memo,
                amount,
                cleared: ClearedState::Uncleared,
            },
        )?;

        tx.execute(
            "INSERT INTO transfers (
                from_transaction_id, to_transaction_id, boundary,
                boundary_target, boundary_category_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                from_entry.value(),
                to_entry.value(),
                boundary.as_str(),
                target,
                category_id,
                now_unix()
            ],
        )?;
        let transfer_id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE transactions SET transfer_id = ?1 WHERE id IN (?2, ?3)",
            params![transfer_id, from_entry.value(), to_entry.value()],
        )?;
        load_transfer(tx, transfer_id)
    })
    .await
}

pub async fn edit_transfer(
    db: &Db,
    id: i64,
    edit: EditTransfer<'_>,
) -> Result<TransferRecord, LedgerError> {
    if !edit.amount.is_positive() {
        return Err(LedgerError::InvalidTransferAmount);
    }
    validate_text(edit.payee, 200, LedgerError::InvalidPayee)?;
    validate_text(edit.memo, 1000, LedgerError::InvalidMemo)?;
    let payee = edit.payee.to_owned();
    let memo = edit.memo.to_owned();
    let EditTransfer {
        amount,
        date,
        boundary: requested_boundary,
        expected_from_revision,
        expected_to_revision,
        confirm_reconciled,
        ..
    } = edit;
    db.run_transaction(move |tx| {
        let existing = load_transfer(tx, id)?;
        if existing.from.revision != expected_from_revision {
            return Err(LedgerError::StaleRevision {
                entity: "transfer leg",
                id: existing.from.id.value(),
                expected: expected_from_revision,
                actual: existing.from.revision,
            });
        }
        if existing.to.revision != expected_to_revision {
            return Err(LedgerError::StaleRevision {
                entity: "transfer leg",
                id: existing.to.id.value(),
                expected: expected_to_revision,
                actual: existing.to.revision,
            });
        }

        let from_account = load_account(tx, existing.from.account_id)?;
        let to_account = load_account(tx, existing.to.account_id)?;
        let (boundary, target, category_id) =
            resolve_boundary(tx, from_account.kind, to_account.kind, requested_boundary)?;
        if boundary != existing.boundary {
            return Err(LedgerError::CorruptRow {
                table: "transfers",
                detail: "account kinds no longer match the stored transfer boundary".to_owned(),
            });
        }

        let new_from_amount = amount.checked_neg()?;
        let reconciled_change = (existing.from.cleared == ClearedState::Reconciled
            || existing.to.cleared == ClearedState::Reconciled)
            && (existing.from.amount != new_from_amount
                || existing.to.amount != amount
                || existing.from.date != date
                || existing.to.date != date);
        if reconciled_change && !confirm_reconciled {
            return Err(LedgerError::TransferReconciledEditRequiresConfirmation { id });
        }

        let from_cleared = if reconciled_change {
            ClearedState::Cleared
        } else {
            existing.from.cleared
        };
        let to_cleared = if reconciled_change {
            ClearedState::Cleared
        } else {
            existing.to.cleared
        };
        update_leg(
            tx,
            existing.from.id,
            new_from_amount,
            date,
            &payee,
            &memo,
            from_cleared,
        )?;
        update_leg(
            tx,
            existing.to.id,
            amount,
            date,
            &payee,
            &memo,
            to_cleared,
        )?;
        tx.execute(
            "UPDATE transfers
             SET boundary = ?1, boundary_target = ?2, boundary_category_id = ?3
             WHERE id = ?4",
            params![boundary.as_str(), target, category_id, id],
        )?;

        if reconciled_change {
            let from_date = if existing.from.date < date {
                existing.from.date
            } else {
                date
            };
            invalidate_reconciliations(tx, existing.from.account_id, from_date)?;
            invalidate_reconciliations(tx, existing.to.account_id, from_date)?;
        }

        load_transfer(tx, id)
    })
    .await
}

pub async fn delete_transfer(
    db: &Db,
    id: i64,
    expected_from_revision: i64,
    expected_to_revision: i64,
    confirm_reconciled: bool,
) -> Result<(), LedgerError> {
    db.run_transaction(move |tx| {
        let existing = load_transfer(tx, id)?;
        if existing.from.revision != expected_from_revision {
            return Err(LedgerError::StaleRevision {
                entity: "transfer leg",
                id: existing.from.id.value(),
                expected: expected_from_revision,
                actual: existing.from.revision,
            });
        }
        if existing.to.revision != expected_to_revision {
            return Err(LedgerError::StaleRevision {
                entity: "transfer leg",
                id: existing.to.id.value(),
                expected: expected_to_revision,
                actual: existing.to.revision,
            });
        }
        let reconciled = existing.from.cleared == ClearedState::Reconciled
            || existing.to.cleared == ClearedState::Reconciled;
        if reconciled && !confirm_reconciled {
            return Err(LedgerError::TransferReconciledEditRequiresConfirmation { id });
        }
        if reconciled {
            invalidate_reconciliations(tx, existing.from.account_id, existing.from.date)?;
            invalidate_reconciliations(tx, existing.to.account_id, existing.to.date)?;
        }
        mark_linked_source_deleted(tx, existing.from.id)?;
        mark_linked_source_deleted(tx, existing.to.id)?;
        tx.execute(
            "UPDATE transactions SET transfer_id = NULL WHERE id IN (?1, ?2)",
            params![existing.from.id.value(), existing.to.id.value()],
        )?;
        tx.execute("DELETE FROM transfers WHERE id = ?1", [id])?;
        tx.execute(
            "DELETE FROM transactions WHERE id IN (?1, ?2)",
            params![existing.from.id.value(), existing.to.id.value()],
        )?;
        Ok(())
    })
    .await
}

pub async fn transfer(db: &Db, id: i64) -> Result<TransferRecord, LedgerError> {
    db.run_read(move |conn| load_transfer(conn, id)).await
}

fn resolve_boundary(
    conn: &Connection,
    from: AccountKind,
    to: AccountKind,
    target: Option<BoundaryTarget>,
) -> Result<(TransferBoundary, Option<&'static str>, Option<i64>), LedgerError> {
    match (from.is_budget(), to.is_budget()) {
        (true, true) => {
            if target.is_some() {
                return Err(LedgerError::BoundaryTargetNotAllowed);
            }
            Ok((TransferBoundary::Budget, None, None))
        }
        (false, false) => Err(LedgerError::TrackingToTrackingTransfer),
        _ => {
            let target = target.ok_or(LedgerError::BoundaryTargetRequired)?;
            match target {
                BoundaryTarget::ReadyToAssign => {
                    Ok((TransferBoundary::BudgetTracking, Some("ready_to_assign"), None))
                }
                BoundaryTarget::Category(category_id) => {
                    let category = load_category(conn, category_id)?;
                    if category.archived {
                        return Err(LedgerError::ArchivedCategory { id: category_id });
                    }
                    Ok((
                        TransferBoundary::BudgetTracking,
                        Some("category"),
                        Some(category_id.value()),
                    ))
                }
            }
        }
    }
}

fn update_leg(
    conn: &Connection,
    id: TransactionId,
    amount: crate::domain::Amount,
    date: crate::domain::BudgetDate,
    payee: &str,
    memo: &str,
    cleared: ClearedState,
) -> Result<(), LedgerError> {
    conn.execute(
        "UPDATE transactions
         SET amount_cents = ?1, transaction_date = ?2, payee = ?3, memo = ?4,
             cleared_state = ?5, revision = revision + 1, updated_at = ?6
         WHERE id = ?7",
        params![
            amount.cents(),
            date.to_string(),
            payee,
            memo,
            cleared.as_str(),
            now_unix(),
            id.value()
        ],
    )?;
    Ok(())
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
