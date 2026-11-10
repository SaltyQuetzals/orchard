use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::{
    AccountId, AllocationId, Amount, BudgetDate, CategoryGroupId, CategoryId, ClearedState,
    ConnectionId, SourceId, TransactionId, TransactionKind, TransactionOrigin,
};

use super::{
    Account, Allocation, Category, CategoryGroup, LedgerError, NewAllocation,
    ReconciliationRecord, ReconciliationStatus, SourceCorrection, SourceRecord, SourceStatus,
    TransactionRecord, TransferBoundary, TransferRecord,
};

pub(super) fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

pub(super) fn parse_date(table: &'static str, text: &str) -> Result<BudgetDate, LedgerError> {
    text.parse().map_err(|source| LedgerError::CorruptRow {
        table,
        detail: format!("invalid date `{text}`: {source}"),
    })
}

fn parse_optional_date(
    table: &'static str,
    text: Option<String>,
) -> Result<Option<BudgetDate>, LedgerError> {
    text.map(|text| parse_date(table, &text)).transpose()
}

pub(super) fn account_balance_in(
    conn: &Connection,
    account_id: AccountId,
    as_of: Option<BudgetDate>,
) -> Result<Amount, LedgerError> {
    let as_of = as_of.map(|date| date.to_string());
    let mut statement = conn.prepare(
        "SELECT amount_cents FROM transactions
         WHERE account_id = ?1 AND (?2 IS NULL OR transaction_date <= ?2)",
    )?;
    let rows = statement.query_map(params![account_id.value(), as_of], |row| {
        row.get::<_, i64>(0)
    })?;
    let mut total = Amount::ZERO;
    for row in rows {
        total = total.checked_add(Amount::from_cents(row?))?;
    }
    Ok(total)
}

pub(super) fn cleared_balance_in(
    conn: &Connection,
    account_id: AccountId,
    statement_date: BudgetDate,
) -> Result<Amount, LedgerError> {
    let mut statement = conn.prepare(
        "SELECT amount_cents FROM transactions
         WHERE account_id = ?1
           AND transaction_date <= ?2
           AND cleared_state IN ('cleared', 'reconciled')",
    )?;
    let rows = statement.query_map(
        params![account_id.value(), statement_date.to_string()],
        |row| row.get::<_, i64>(0),
    )?;
    let mut total = Amount::ZERO;
    for row in rows {
        total = total.checked_add(Amount::from_cents(row?))?;
    }
    Ok(total)
}

pub(super) fn invalidate_reconciliations(
    conn: &Connection,
    account_id: AccountId,
    from_date: BudgetDate,
) -> Result<(), LedgerError> {
    conn.execute(
        "UPDATE reconciliations
         SET status = 'invalidated', invalidated_at = ?1, revision = revision + 1
         WHERE account_id = ?2 AND status = 'completed' AND statement_date >= ?3",
        params![now_unix(), account_id.value(), from_date.to_string()],
    )?;
    Ok(())
}

pub(super) fn mark_linked_source_deleted(
    conn: &Connection,
    transaction_id: TransactionId,
) -> Result<(), LedgerError> {
    let now = now_unix();
    conn.execute(
        "UPDATE source_corrections
         SET status = 'dismissed', resolved_at = ?1
         WHERE status = 'pending'
           AND source_id IN (
               SELECT id FROM source_records WHERE transaction_id = ?2
           )",
        params![now, transaction_id.value()],
    )?;
    conn.execute(
        "UPDATE source_records
         SET status = 'deleted', transaction_id = NULL,
             source_revision = source_revision + 1, last_seen_at = ?1
         WHERE transaction_id = ?2 AND status <> 'deleted'",
        params![now, transaction_id.value()],
    )?;
    Ok(())
}

pub(super) fn load_account(
    conn: &Connection,
    id: AccountId,
) -> Result<Account, LedgerError> {
    let row = conn
        .query_row(
            "SELECT id, name, kind, payment_category_id, closed_on, revision
             FROM accounts WHERE id = ?1",
            [id.value()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(LedgerError::AccountNotFound { id })?;

    Ok(Account {
        id: AccountId::new(row.0),
        name: row.1,
        kind: crate::domain::AccountKind::parse(&row.2)?,
        payment_category_id: row.3.map(CategoryId::new),
        closed_on: parse_optional_date("accounts", row.4)?,
        revision: row.5,
    })
}

pub(super) fn load_category_group(
    conn: &Connection,
    id: CategoryGroupId,
) -> Result<CategoryGroup, LedgerError> {
    let row = conn
        .query_row(
            "SELECT id, name, position, archived_at FROM category_groups WHERE id = ?1",
            [id.value()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(LedgerError::CategoryGroupNotFound { id })?;

    Ok(CategoryGroup {
        id: CategoryGroupId::new(row.0),
        name: row.1,
        position: row.2,
        archived: row.3.is_some(),
    })
}

pub(super) fn load_category(
    conn: &Connection,
    id: CategoryId,
) -> Result<Category, LedgerError> {
    let row = conn
        .query_row(
            "SELECT id, group_id, name, position, archived_at FROM categories WHERE id = ?1",
            [id.value()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(LedgerError::CategoryNotFound { id })?;

    Ok(Category {
        id: CategoryId::new(row.0),
        group_id: CategoryGroupId::new(row.1),
        name: row.2,
        position: row.3,
        archived: row.4.is_some(),
    })
}

pub(super) fn load_transaction(
    conn: &Connection,
    id: TransactionId,
) -> Result<TransactionRecord, LedgerError> {
    let row = conn
        .query_row(
            "SELECT id, account_id, kind, origin, transaction_date, payee, memo,
                    amount_cents, cleared_state, revision, transfer_id
             FROM transactions WHERE id = ?1",
            [id.value()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                ))
            },
        )
        .optional()?
        .ok_or(LedgerError::TransactionNotFound { id })?;

    let mut statement = conn.prepare(
        "SELECT id, category_id, amount_cents FROM allocations
         WHERE transaction_id = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map([id.value()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut allocations = Vec::new();
    for row in rows {
        let row = row?;
        allocations.push(Allocation {
            id: AllocationId::new(row.0),
            category_id: row.1.map(CategoryId::new),
            amount: Amount::from_cents(row.2),
        });
    }

    Ok(TransactionRecord {
        id: TransactionId::new(row.0),
        account_id: AccountId::new(row.1),
        kind: TransactionKind::parse(&row.2)?,
        origin: TransactionOrigin::parse(&row.3)?,
        date: parse_date("transactions", &row.4)?,
        payee: row.5,
        memo: row.6,
        amount: Amount::from_cents(row.7),
        cleared: ClearedState::parse(&row.8)?,
        revision: row.9,
        transfer_id: row.10,
        allocations,
    })
}

pub(super) fn load_transfer(conn: &Connection, id: i64) -> Result<TransferRecord, LedgerError> {
    let row = conn
        .query_row(
            "SELECT id, from_transaction_id, to_transaction_id, boundary,
                    boundary_target, boundary_category_id, created_at
             FROM transfers WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or(LedgerError::TransferNotFound { id })?;

    let boundary_target = match row.4.as_deref() {
        None => None,
        Some("ready_to_assign") => Some(super::BoundaryTarget::ReadyToAssign),
        Some("category") => {
            let category_id = row
                .5
                .ok_or_else(|| LedgerError::CorruptRow {
                    table: "transfers",
                    detail: "category boundary has no category identifier".to_owned(),
                })?;
            Some(super::BoundaryTarget::Category(CategoryId::new(category_id)))
        }
        Some(other) => {
            return Err(LedgerError::CorruptRow {
                table: "transfers",
                detail: format!("unknown boundary target `{other}`"),
            });
        }
    };

    Ok(TransferRecord {
        id: row.0,
        from: load_transaction(conn, TransactionId::new(row.1))?,
        to: load_transaction(conn, TransactionId::new(row.2))?,
        boundary: TransferBoundary::parse(&row.3)?,
        boundary_target,
        created_at: row.6,
    })
}

pub(super) fn load_reconciliation(
    conn: &Connection,
    id: i64,
) -> Result<ReconciliationRecord, LedgerError> {
    let row = conn
        .query_row(
            "SELECT id, account_id, statement_date, statement_balance_cents,
                    cleared_balance_cents, difference_cents, adjustment_transaction_id,
                    status, revision
             FROM reconciliations WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or(LedgerError::ReconciliationNotFound { id })?;

    Ok(ReconciliationRecord {
        id: row.0,
        account_id: AccountId::new(row.1),
        statement_date: parse_date("reconciliations", &row.2)?,
        statement_balance: Amount::from_cents(row.3),
        cleared_balance: Amount::from_cents(row.4),
        difference: Amount::from_cents(row.5),
        adjustment_transaction_id: row.6.map(TransactionId::new),
        status: ReconciliationStatus::parse(&row.7)?,
        revision: row.8,
    })
}

pub(super) fn load_source(
    conn: &Connection,
    id: SourceId,
) -> Result<SourceRecord, LedgerError> {
    let row = conn
        .query_row(
            "SELECT id, connection_id, institution_connection_id, external_account_id,
                    external_transaction_id, account_id, source_date, amount_cents,
                    payee, memo, status, transaction_id, source_revision
             FROM source_records WHERE id = ?1",
            [id.value()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, i64>(12)?,
                ))
            },
        )
        .optional()?
        .ok_or(LedgerError::SourceNotFound { id })?;

    Ok(SourceRecord {
        id: SourceId::new(row.0),
        connection_id: ConnectionId::new(row.1),
        institution_connection_id: row.2,
        external_account_id: row.3,
        external_transaction_id: row.4,
        account_id: AccountId::new(row.5),
        date: parse_date("source_records", &row.6)?,
        amount: Amount::from_cents(row.7),
        payee: row.8,
        memo: row.9,
        status: SourceStatus::parse(&row.10)?,
        transaction_id: row.11.map(TransactionId::new),
        revision: row.12,
    })
}

pub(super) fn load_source_by_identity(
    conn: &Connection,
    connection_id: ConnectionId,
    institution_connection_id: &str,
    external_account_id: &str,
    external_transaction_id: &str,
) -> Result<Option<SourceId>, LedgerError> {
    let id = conn
        .query_row(
            "SELECT id FROM source_records
             WHERE connection_id = ?1
               AND institution_connection_id = ?2
               AND external_account_id = ?3
               AND external_transaction_id = ?4",
            params![
                connection_id.value(),
                institution_connection_id,
                external_account_id,
                external_transaction_id
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(id.map(SourceId::new))
}

pub(super) fn load_correction(
    conn: &Connection,
    id: i64,
) -> Result<SourceCorrection, LedgerError> {
    let row = conn
        .query_row(
            "SELECT id, source_id, previous_amount_cents, previous_date,
                    proposed_amount_cents, proposed_date, status
             FROM source_corrections WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or(LedgerError::CorrectionNotFound { id })?;

    Ok(SourceCorrection {
        id: row.0,
        source_id: SourceId::new(row.1),
        previous_amount: Amount::from_cents(row.2),
        previous_date: parse_date("source_corrections", &row.3)?,
        proposed_amount: Amount::from_cents(row.4),
        proposed_date: parse_date("source_corrections", &row.5)?,
        status: super::CorrectionStatus::parse(&row.6)?,
    })
}

pub(super) struct NewEntry<'a> {
    pub account_id: AccountId,
    pub kind: TransactionKind,
    pub origin: TransactionOrigin,
    pub date: BudgetDate,
    pub payee: &'a str,
    pub memo: &'a str,
    pub amount: Amount,
    pub cleared: ClearedState,
}

pub(super) fn insert_entry(
    conn: &Connection,
    entry: NewEntry<'_>,
) -> Result<TransactionId, LedgerError> {
    let now = now_unix();
    conn.execute(
        "INSERT INTO transactions (
            account_id, kind, origin, transaction_date, payee, memo,
            amount_cents, cleared_state, revision, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)",
        params![
            entry.account_id.value(),
            entry.kind.as_str(),
            entry.origin.as_str(),
            entry.date.to_string(),
            entry.payee,
            entry.memo,
            entry.amount.cents(),
            entry.cleared.as_str(),
            now
        ],
    )?;
    Ok(TransactionId::new(conn.last_insert_rowid()))
}

pub(super) fn insert_allocations(
    conn: &Connection,
    transaction_id: TransactionId,
    allocations: &[NewAllocation],
) -> Result<Vec<Allocation>, LedgerError> {
    let now = now_unix();
    let mut stored = Vec::with_capacity(allocations.len());
    for allocation in allocations {
        conn.execute(
            "INSERT INTO allocations (
                transaction_id, category_id, amount_cents, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![
                transaction_id.value(),
                allocation.category_id.map(CategoryId::value),
                allocation.amount.cents(),
                now
            ],
        )?;
        stored.push(Allocation {
            id: AllocationId::new(conn.last_insert_rowid()),
            category_id: allocation.category_id,
            amount: allocation.amount,
        });
    }
    Ok(stored)
}

pub(super) fn validate_new_allocations(
    conn: &Connection,
    kind: TransactionKind,
    amount: Amount,
    allocations: &[NewAllocation],
) -> Result<Vec<NewAllocation>, LedgerError> {
    match kind {
        TransactionKind::Income => {
            if allocations.is_empty() {
                Ok(Vec::new())
            } else {
                Err(LedgerError::IncomeCannotHaveAllocations)
            }
        }
        TransactionKind::Outflow | TransactionKind::Refund => {
            if allocations.is_empty() {
                return Ok(vec![NewAllocation {
                    category_id: None,
                    amount,
                }]);
            }
            let mut total = Amount::ZERO;
            for allocation in allocations {
                if allocation.amount.is_zero() {
                    return Err(LedgerError::ZeroAllocation);
                }
                let sign_ok = match kind {
                    TransactionKind::Outflow => allocation.amount.is_negative(),
                    TransactionKind::Refund => allocation.amount.is_positive(),
                    _ => false,
                };
                if !sign_ok {
                    return Err(LedgerError::AllocationSignMismatch { kind });
                }
                if let Some(category_id) = allocation.category_id {
                    let category = load_category(conn, category_id)?;
                    if category.archived {
                        return Err(LedgerError::ArchivedCategory { id: category_id });
                    }
                }
                total = total.checked_add(allocation.amount)?;
            }
            if total != amount {
                return Err(LedgerError::AllocationSumMismatch {
                    amount,
                    allocations: total,
                });
            }
            Ok(allocations.to_vec())
        }
        TransactionKind::Transfer
        | TransactionKind::OpeningBalance
        | TransactionKind::Adjustment => Err(LedgerError::ReservedTransactionKind { kind }),
    }
}

pub(super) fn validate_existing_allocations(
    kind: TransactionKind,
    amount: Amount,
    allocations: &[Allocation],
) -> Result<Vec<NewAllocation>, LedgerError> {
    match kind {
        TransactionKind::Income => {
            if allocations.is_empty() {
                Ok(Vec::new())
            } else {
                Err(LedgerError::IncomeCannotHaveAllocations)
            }
        }
        TransactionKind::Outflow | TransactionKind::Refund => {
            if allocations.is_empty() {
                return Ok(vec![NewAllocation {
                    category_id: None,
                    amount,
                }]);
            }
            let mut total = Amount::ZERO;
            for allocation in allocations {
                if allocation.amount.is_zero() {
                    return Err(LedgerError::ZeroAllocation);
                }
                let sign_ok = match kind {
                    TransactionKind::Outflow => allocation.amount.is_negative(),
                    TransactionKind::Refund => allocation.amount.is_positive(),
                    _ => false,
                };
                if !sign_ok {
                    return Err(LedgerError::AllocationSignMismatch { kind });
                }
                total = total.checked_add(allocation.amount)?;
            }
            if total != amount {
                return Err(LedgerError::AllocationSumMismatch {
                    amount,
                    allocations: total,
                });
            }
            Ok(allocations
                .iter()
                .map(|allocation| NewAllocation {
                    category_id: allocation.category_id,
                    amount: allocation.amount,
                })
                .collect())
        }
        TransactionKind::Transfer
        | TransactionKind::OpeningBalance
        | TransactionKind::Adjustment => Err(LedgerError::ReservedTransactionKind { kind }),
    }
}

pub(super) fn date_window(date: BudgetDate, days: i32) -> (BudgetDate, BudgetDate) {
    let julian = date.date().to_julian_day();
    let start = time::Date::from_julian_day(julian - days).unwrap_or(date.date());
    let end = time::Date::from_julian_day(julian + days).unwrap_or(date.date());
    (BudgetDate::from_date(start), BudgetDate::from_date(end))
}
