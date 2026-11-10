use rusqlite::{OptionalExtension, params};

use crate::db::Db;
use crate::domain::{
    Amount, ClearedState, ConnectionId, SourceId, TransactionId, TransactionKind,
    TransactionOrigin,
};

use super::entries::validate_kind_and_sign;
use super::rows::{
    date_window, insert_allocations, insert_entry, invalidate_reconciliations, load_account,
    load_correction, load_source, load_source_by_identity, load_transaction, now_unix,
    NewEntry,
};
use super::{
    CorrectionStatus, LedgerError, MatchCandidate, NewAllocation, NewSource, SourceCorrection,
    SourceIngest, SourceRecord, SourceStatus,
};

pub async fn create_provider_connection(
    db: &Db,
    provider: &str,
    label: &str,
) -> Result<ConnectionId, LedgerError> {
    let provider = provider.trim().to_owned();
    if provider.is_empty() || provider.chars().count() > 40 {
        return Err(LedgerError::InvalidExternalIdentity);
    }
    if label.chars().count() > 120 {
        return Err(LedgerError::InvalidExternalIdentity);
    }
    let label = label.to_owned();
    db.run_transaction(move |tx| {
        tx.execute(
            "INSERT INTO provider_connections (provider, label, created_at)
             VALUES (?1, ?2, ?3)",
            params![provider, label, now_unix()],
        )?;
        Ok(ConnectionId::new(tx.last_insert_rowid()))
    })
    .await
}

pub async fn ingest_source(db: &Db, new: NewSource<'_>) -> Result<SourceIngest, LedgerError> {
    if new.institution_connection_id.trim().is_empty()
        || new.external_account_id.trim().is_empty()
        || new.external_transaction_id.trim().is_empty()
    {
        return Err(LedgerError::InvalidExternalIdentity);
    }
    if new.amount.is_zero() {
        return Err(LedgerError::ZeroAmount);
    }
    if new.payee.chars().count() > 200 {
        return Err(LedgerError::InvalidPayee);
    }
    if new.memo.chars().count() > 1000 {
        return Err(LedgerError::InvalidMemo);
    }

    let institution = new.institution_connection_id.to_owned();
    let external_account = new.external_account_id.to_owned();
    let external_transaction = new.external_transaction_id.to_owned();
    let payee = new.payee.to_owned();
    let memo = new.memo.to_owned();

    db.run_transaction(move |tx| {
        let account = load_account(tx, new.account_id)?;
        if account.closed_on.is_some() {
            return Err(LedgerError::AccountClosed { id: new.account_id });
        }
        if !account.kind.is_budget() {
            return Err(LedgerError::TrackingAccountOrdinaryActivity {
                id: new.account_id,
            });
        }

        if let Some(source_id) = load_source_by_identity(
            tx,
            new.connection_id,
            &institution,
            &external_account,
            &external_transaction,
        )? {
            let existing = load_source(tx, source_id)?;
            if matches!(
                existing.status,
                SourceStatus::Deleted | SourceStatus::Dismissed
            ) {
                tx.execute(
                    "UPDATE source_records SET last_seen_at = ?1 WHERE id = ?2",
                    params![now_unix(), source_id.value()],
                )?;
                return Ok(SourceIngest {
                    source: load_source(tx, source_id)?,
                    created_entry: false,
                    correction: None,
                });
            }

            let changed = existing.amount != new.amount || existing.date != new.date;
            let text_changed = existing.payee != payee || existing.memo != memo;
            let revision = if changed || text_changed {
                existing.revision + 1
            } else {
                existing.revision
            };
            tx.execute(
                "UPDATE source_records
                 SET source_date = ?1, amount_cents = ?2, payee = ?3, memo = ?4,
                     source_revision = ?5, last_seen_at = ?6
                 WHERE id = ?7",
                params![
                    new.date.to_string(),
                    new.amount.cents(),
                    payee,
                    memo,
                    revision,
                    now_unix(),
                    source_id.value()
                ],
            )?;

            let correction = if changed {
                let updated = tx.execute(
                    "UPDATE source_corrections
                     SET proposed_amount_cents = ?1, proposed_date = ?2
                     WHERE source_id = ?3 AND status = 'pending'",
                    params![
                        new.amount.cents(),
                        new.date.to_string(),
                        source_id.value()
                    ],
                )?;
                if updated == 0 {
                    tx.execute(
                        "INSERT INTO source_corrections (
                            source_id, previous_amount_cents, previous_date,
                            proposed_amount_cents, proposed_date, status, created_at, resolved_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, NULL)",
                        params![
                            source_id.value(),
                            existing.amount.cents(),
                            existing.date.to_string(),
                            new.amount.cents(),
                            new.date.to_string(),
                            now_unix()
                        ],
                    )?;
                }
                load_pending_correction(tx, source_id)?
            } else {
                None
            };

            return Ok(SourceIngest {
                source: load_source(tx, source_id)?,
                created_entry: false,
                correction,
            });
        }

        let now = now_unix();
        tx.execute(
            "INSERT INTO source_records (
                connection_id, institution_connection_id, external_account_id,
                external_transaction_id, account_id, source_date, amount_cents,
                payee, memo, status, transaction_id, source_revision,
                first_seen_at, last_seen_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                       'pending_review', NULL, 1, ?10, ?10)",
            params![
                new.connection_id.value(),
                institution,
                external_account,
                external_transaction,
                new.account_id.value(),
                new.date.to_string(),
                new.amount.cents(),
                payee,
                memo,
                now
            ],
        )?;
        let source_id = SourceId::new(tx.last_insert_rowid());

        let entry_id = insert_entry(
            tx,
            NewEntry {
                account_id: new.account_id,
                kind: if new.amount.is_negative() {
                    TransactionKind::Outflow
                } else {
                    TransactionKind::Income
                },
                origin: TransactionOrigin::Import,
                date: new.date,
                payee: &payee,
                memo: &memo,
                amount: new.amount,
                cleared: ClearedState::Cleared,
            },
        )?;
        if new.amount.is_negative() {
            insert_allocations(
                tx,
                entry_id,
                &[NewAllocation {
                    category_id: None,
                    amount: new.amount,
                }],
            )?;
        }
        tx.execute(
            "UPDATE source_records SET transaction_id = ?1 WHERE id = ?2",
            params![entry_id.value(), source_id.value()],
        )?;

        Ok(SourceIngest {
            source: load_source(tx, source_id)?,
            created_entry: true,
            correction: None,
        })
    })
    .await
}

pub async fn approve_source(
    db: &Db,
    id: SourceId,
    expected_revision: i64,
) -> Result<SourceRecord, LedgerError> {
    db.run_transaction(move |tx| {
        let source = load_source(tx, id)?;
        if source.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "import source",
                id: id.value(),
                expected: expected_revision,
                actual: source.revision,
            });
        }
        match source.status {
            SourceStatus::Deleted => return Err(LedgerError::SourceDeleted { id }),
            SourceStatus::Dismissed => return Err(LedgerError::SourceDismissed { id }),
            SourceStatus::Approved => return Ok(source),
            SourceStatus::PendingReview => {}
        }
        if source.transaction_id.is_none() {
            return Err(LedgerError::SourceHasNoEntry { id });
        }
        tx.execute(
            "UPDATE source_records
             SET status = 'approved', source_revision = source_revision + 1, last_seen_at = ?1
             WHERE id = ?2",
            params![now_unix(), id.value()],
        )?;
        load_source(tx, id)
    })
    .await
}

pub async fn dismiss_source(
    db: &Db,
    id: SourceId,
    expected_revision: i64,
    confirm_reconciled: bool,
) -> Result<SourceRecord, LedgerError> {
    set_source_terminal(
        db,
        id,
        expected_revision,
        SourceStatus::Dismissed,
        confirm_reconciled,
    )
    .await
}

pub async fn delete_source(
    db: &Db,
    id: SourceId,
    expected_revision: i64,
    confirm_reconciled: bool,
) -> Result<SourceRecord, LedgerError> {
    set_source_terminal(
        db,
        id,
        expected_revision,
        SourceStatus::Deleted,
        confirm_reconciled,
    )
    .await
}

async fn set_source_terminal(
    db: &Db,
    id: SourceId,
    expected_revision: i64,
    status: SourceStatus,
    confirm_reconciled: bool,
) -> Result<SourceRecord, LedgerError> {
    db.run_transaction(move |tx| {
        let source = load_source(tx, id)?;
        if source.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "import source",
                id: id.value(),
                expected: expected_revision,
                actual: source.revision,
            });
        }
        if source.status == status {
            return Ok(source);
        }
        if source.status == SourceStatus::Deleted {
            return Err(LedgerError::SourceDeleted { id });
        }
        if source.status == SourceStatus::Dismissed {
            return Err(LedgerError::SourceDismissed { id });
        }

        if let Some(transaction_id) = source.transaction_id {
            let entry = load_transaction(tx, transaction_id)?;
            if entry.origin == TransactionOrigin::Import {
                if entry.cleared == ClearedState::Reconciled && !confirm_reconciled {
                    return Err(LedgerError::ReconciledEditRequiresConfirmation {
                        id: transaction_id,
                    });
                }
                if entry.cleared == ClearedState::Reconciled {
                    invalidate_reconciliations(tx, entry.account_id, entry.date)?;
                }
                tx.execute(
                    "DELETE FROM transactions WHERE id = ?1",
                    [transaction_id.value()],
                )?;
            }
        }

        let now = now_unix();
        tx.execute(
            "UPDATE source_records
             SET status = ?1, source_revision = source_revision + 1, last_seen_at = ?2
             WHERE id = ?3",
            params![status.as_str(), now, id.value()],
        )?;
        tx.execute(
            "UPDATE source_corrections
             SET status = 'dismissed', resolved_at = ?1
             WHERE source_id = ?2 AND status = 'pending'",
            params![now, id.value()],
        )?;
        load_source(tx, id)
    })
    .await
}

pub async fn match_candidates(
    db: &Db,
    source_id: SourceId,
) -> Result<Vec<MatchCandidate>, LedgerError> {
    db.run_read(move |conn| {
        let source = load_source(conn, source_id)?;
        if matches!(
            source.status,
            SourceStatus::Deleted | SourceStatus::Dismissed
        ) {
            return Ok(Vec::new());
        }
        let (start, end) = date_window(source.date, 5);
        let current = source.transaction_id.map(TransactionId::value).unwrap_or(-1);
        let mut statement = conn.prepare(
            "SELECT t.id, t.transaction_date, t.payee, t.amount_cents, t.revision
             FROM transactions t
             WHERE t.account_id = ?1
               AND t.amount_cents = ?2
               AND t.kind <> 'transfer'
               AND t.origin = 'manual'
               AND t.id <> ?3
               AND NOT EXISTS (
                   SELECT 1 FROM source_records s
                   WHERE s.transaction_id = t.id AND s.id <> ?4
               )
               AND t.transaction_date BETWEEN ?5 AND ?6
             ORDER BY t.transaction_date, t.id",
        )?;
        let rows = statement.query_map(
            params![
                source.account_id.value(),
                source.amount.cents(),
                current,
                source_id.value(),
                start.to_string(),
                end.to_string()
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )?;
        let mut candidates = Vec::new();
        for row in rows {
            let row = row?;
            candidates.push(MatchCandidate {
                transaction_id: TransactionId::new(row.0),
                date: super::rows::parse_date("transactions", &row.1)?,
                payee: row.2,
                amount: Amount::from_cents(row.3),
                revision: row.4,
            });
        }
        Ok(candidates)
    })
    .await
}

pub async fn match_source(
    db: &Db,
    source_id: SourceId,
    transaction_id: TransactionId,
    expected_revision: i64,
    confirm_reconciled: bool,
) -> Result<SourceRecord, LedgerError> {
    db.run_transaction(move |tx| {
        let source = load_source(tx, source_id)?;
        if source.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "import source",
                id: source_id.value(),
                expected: expected_revision,
                actual: source.revision,
            });
        }
        match source.status {
            SourceStatus::Deleted => return Err(LedgerError::SourceDeleted { id: source_id }),
            SourceStatus::Dismissed => return Err(LedgerError::SourceDismissed { id: source_id }),
            SourceStatus::PendingReview | SourceStatus::Approved => {}
        }
        if source.transaction_id == Some(transaction_id) {
            return Ok(source);
        }

        let target = load_transaction(tx, transaction_id)?;
        if target.account_id != source.account_id {
            return Err(LedgerError::MatchAccountMismatch);
        }
        if target.amount != source.amount {
            return Err(LedgerError::MatchAmountMismatch {
                source: source.amount,
                transaction: target.amount,
            });
        }
        let already_linked: Option<i64> = tx
            .query_row(
                "SELECT id FROM source_records WHERE transaction_id = ?1 AND id <> ?2",
                params![transaction_id.value(), source_id.value()],
                |row| row.get(0),
            )
            .optional()?;
        if already_linked.is_some() {
            return Err(LedgerError::TransactionAlreadyMatched { id: transaction_id });
        }

        if let Some(current) = source.transaction_id {
            let entry = load_transaction(tx, current)?;
            if entry.origin == TransactionOrigin::Import {
                if entry.cleared == ClearedState::Reconciled && !confirm_reconciled {
                    return Err(LedgerError::ReconciledEditRequiresConfirmation { id: current });
                }
                if entry.cleared == ClearedState::Reconciled {
                    invalidate_reconciliations(tx, entry.account_id, entry.date)?;
                }
                tx.execute("DELETE FROM transactions WHERE id = ?1", [current.value()])?;
            }
        }

        tx.execute(
            "UPDATE source_records
             SET transaction_id = ?1, status = 'approved',
                 source_revision = source_revision + 1, last_seen_at = ?2
             WHERE id = ?3",
            params![transaction_id.value(), now_unix(), source_id.value()],
        )?;
        load_source(tx, source_id)
    })
    .await
}

pub async fn source_corrections(
    db: &Db,
    source_id: SourceId,
) -> Result<Vec<SourceCorrection>, LedgerError> {
    db.run_read(move |conn| {
        let ids: Vec<i64> = {
            let mut statement = conn.prepare(
                "SELECT id FROM source_corrections
                 WHERE source_id = ?1 ORDER BY id",
            )?;
            let rows = statement.query_map([source_id.value()], |row| {
                row.get::<_, i64>(0)
            })?;
            rows.collect::<Result<_, _>>()?
        };
        ids.into_iter()
            .map(|id| load_correction(conn, id))
            .collect()
    })
    .await
}

pub async fn apply_source_correction(
    db: &Db,
    correction_id: i64,
    expected_transaction_revision: i64,
    confirm_reconciled: bool,
) -> Result<super::TransactionRecord, LedgerError> {
    db.run_transaction(move |tx| {
        let correction = load_correction(tx, correction_id)?;
        if correction.status != CorrectionStatus::Pending {
            return Err(LedgerError::CorrectionNotPending { id: correction_id });
        }
        let source = load_source(tx, correction.source_id)?;
        let transaction_id = source
            .transaction_id
            .ok_or(LedgerError::SourceHasNoEntry { id: source.id })?;
        let entry = load_transaction(tx, transaction_id)?;
        if entry.revision != expected_transaction_revision {
            return Err(LedgerError::StaleRevision {
                entity: "transaction",
                id: transaction_id.value(),
                expected: expected_transaction_revision,
                actual: entry.revision,
            });
        }
        if entry.kind == TransactionKind::Transfer {
            return Err(LedgerError::CorrectionRequiresManualEdit { id: transaction_id });
        }
        validate_kind_and_sign(entry.kind, correction.proposed_amount)?;
        let reconciled_change = entry.cleared == ClearedState::Reconciled
            && (entry.amount != correction.proposed_amount || entry.date != correction.proposed_date);
        if reconciled_change && !confirm_reconciled {
            return Err(LedgerError::ReconciledEditRequiresConfirmation { id: transaction_id });
        }

        if entry.amount != correction.proposed_amount {
            if entry.allocations.len() == 1 && entry.allocations[0].amount == entry.amount {
                tx.execute(
                    "UPDATE allocations SET amount_cents = ?1, updated_at = ?2 WHERE id = ?3",
                    params![
                        correction.proposed_amount.cents(),
                        now_unix(),
                        entry.allocations[0].id.value()
                    ],
                )?;
            } else if entry.allocations.is_empty() && entry.kind == TransactionKind::Income {
                // Income carries no allocation; changing the header is enough.
            } else {
                return Err(LedgerError::CorrectionRequiresManualEdit { id: transaction_id });
            }
        }

        let cleared = if reconciled_change {
            ClearedState::Cleared
        } else {
            entry.cleared
        };
        tx.execute(
            "UPDATE transactions
             SET amount_cents = ?1, transaction_date = ?2, cleared_state = ?3,
                 revision = revision + 1, updated_at = ?4
             WHERE id = ?5",
            params![
                correction.proposed_amount.cents(),
                correction.proposed_date.to_string(),
                cleared.as_str(),
                now_unix(),
                transaction_id.value()
            ],
        )?;
        if reconciled_change {
            invalidate_reconciliations(tx, entry.account_id, entry.date)?;
        }
        tx.execute(
            "UPDATE source_corrections
             SET status = 'applied', resolved_at = ?1
             WHERE id = ?2",
            params![now_unix(), correction_id],
        )?;
        tx.execute(
            "UPDATE source_records
             SET source_date = ?1, amount_cents = ?2,
                 source_revision = source_revision + 1, last_seen_at = ?3
             WHERE id = ?4",
            params![
                correction.proposed_date.to_string(),
                correction.proposed_amount.cents(),
                now_unix(),
                source.id.value()
            ],
        )?;
        load_transaction(tx, transaction_id)
    })
    .await
}

pub async fn dismiss_source_correction(
    db: &Db,
    correction_id: i64,
) -> Result<SourceCorrection, LedgerError> {
    db.run_transaction(move |tx| {
        let correction = load_correction(tx, correction_id)?;
        match correction.status {
            CorrectionStatus::Dismissed => return Ok(correction),
            CorrectionStatus::Applied => {
                return Err(LedgerError::CorrectionNotPending { id: correction_id });
            }
            CorrectionStatus::Pending => {}
        }
        tx.execute(
            "UPDATE source_corrections
             SET status = 'dismissed', resolved_at = ?1
             WHERE id = ?2",
            params![now_unix(), correction_id],
        )?;
        load_correction(tx, correction_id)
    })
    .await
}

pub async fn source(db: &Db, id: SourceId) -> Result<SourceRecord, LedgerError> {
    db.run_read(move |conn| load_source(conn, id)).await
}

fn load_pending_correction(
    conn: &rusqlite::Connection,
    source_id: SourceId,
) -> Result<Option<SourceCorrection>, LedgerError> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM source_corrections
             WHERE source_id = ?1 AND status = 'pending'",
            [source_id.value()],
            |row| row.get(0),
        )
        .optional()?;
    id.map(|id| load_correction(conn, id)).transpose()
}
