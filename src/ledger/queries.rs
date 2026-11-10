use rusqlite::{OptionalExtension, params};

use crate::db::Db;
use crate::domain::{AccountId, Amount, BudgetDate, TransactionId};

use super::rows::{load_account, parse_date};
use super::{
    CategoryWork, LedgerError, TrackingTotal, Valuation, ValuationSource,
};

pub async fn outstanding_category_work(
    db: &Db,
    as_of: BudgetDate,
) -> Result<Vec<CategoryWork>, LedgerError> {
    db.run_read(move |conn| {
        let mut statement = conn.prepare(
            "SELECT t.id, t.account_id, t.transaction_date, t.amount_cents
             FROM transactions t
             WHERE t.kind IN ('outflow', 'refund')
               AND t.transaction_date <= ?1
               AND EXISTS (
                   SELECT 1 FROM allocations a
                   WHERE a.transaction_id = t.id AND a.category_id IS NULL
               )
             ORDER BY t.transaction_date, t.id",
        )?;
        let rows = statement.query_map([as_of.to_string()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut work = Vec::new();
        for row in rows {
            let row = row?;
            work.push(CategoryWork {
                transaction_id: TransactionId::new(row.0),
                account_id: AccountId::new(row.1),
                date: parse_date("transactions", &row.2)?,
                amount: Amount::from_cents(row.3),
            });
        }
        Ok(work)
    })
    .await
}

/// Income recognized on or before `as_of`. Future-dated income is excluded
/// from current assignable money.
pub async fn recognized_income(
    db: &Db,
    as_of: BudgetDate,
) -> Result<Amount, LedgerError> {
    db.run_read(move |conn| {
        let mut statement = conn.prepare(
            "SELECT amount_cents FROM transactions
             WHERE kind = 'income' AND transaction_date <= ?1",
        )?;
        let rows = statement.query_map([as_of.to_string()], |row| {
            row.get::<_, i64>(0)
        })?;
        let mut total = Amount::ZERO;
        for row in rows {
            total = total.checked_add(Amount::from_cents(row?))?;
        }
        Ok(total)
    })
    .await
}

pub async fn record_valuation(
    db: &Db,
    account_id: AccountId,
    as_of_date: BudgetDate,
    value: Amount,
    source: ValuationSource,
) -> Result<Valuation, LedgerError> {
    db.run_transaction(move |tx| {
        let account = load_account(tx, account_id)?;
        if !matches!(account.kind, crate::domain::AccountKind::Tracking) {
            return Err(LedgerError::ValuationRequiresTrackingAccount { id: account_id });
        }
        tx.execute(
            "INSERT INTO account_valuations (
                account_id, as_of_date, value_cents, observed_at, source
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(account_id, as_of_date, source)
             DO UPDATE SET value_cents = excluded.value_cents,
                           observed_at = excluded.observed_at",
            params![
                account_id.value(),
                as_of_date.to_string(),
                value.cents(),
                super::rows::now_unix(),
                source.as_str()
            ],
        )?;
        load_valuation(tx, account_id, as_of_date, source)?.ok_or(LedgerError::CorruptRow {
            table: "account_valuations",
            detail: "valuation was not found after insert".to_owned(),
        })
    })
    .await
}

pub async fn latest_valuation(
    db: &Db,
    account_id: AccountId,
) -> Result<Option<Valuation>, LedgerError> {
    db.run_read(move |conn| latest_valuation_in(conn, account_id)).await
}

/// Sums the latest valuation for each tracking account. Transfer reference
/// entries are deliberately ignored, and accounts without a valuation are
/// reported as missing rather than zero.
pub async fn net_worth_tracking_total(db: &Db) -> Result<TrackingTotal, LedgerError> {
    db.run_read(|conn| {
        let account_ids: Vec<i64> = {
            let mut statement = conn.prepare(
                "SELECT id FROM accounts
                 WHERE kind = 'tracking' AND closed_on IS NULL
                 ORDER BY id",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut total = Amount::ZERO;
        let mut missing = Vec::new();
        for id in account_ids {
            let account_id = AccountId::new(id);
            match latest_valuation_in(conn, account_id)? {
                Some(valuation) => {
                    total = total.checked_add(valuation.value)?;
                }
                None => missing.push(account_id),
            }
        }
        Ok(TrackingTotal { total, missing })
    })
    .await
}

fn latest_valuation_in(
    conn: &rusqlite::Connection,
    account_id: AccountId,
) -> Result<Option<Valuation>, LedgerError> {
    let row = conn
        .query_row(
            "SELECT as_of_date, value_cents, observed_at, source
             FROM account_valuations
             WHERE account_id = ?1
             ORDER BY as_of_date DESC, observed_at DESC
             LIMIT 1",
            [account_id.value()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(Valuation {
        account_id,
        as_of_date: parse_date("account_valuations", &row.0)?,
        value: Amount::from_cents(row.1),
        observed_at: row.2,
        source: ValuationSource::parse(&row.3)?,
    }))
}

fn load_valuation(
    conn: &rusqlite::Connection,
    account_id: AccountId,
    as_of_date: BudgetDate,
    source: ValuationSource,
) -> Result<Option<Valuation>, LedgerError> {
    let row = conn
        .query_row(
            "SELECT value_cents, observed_at
             FROM account_valuations
             WHERE account_id = ?1 AND as_of_date = ?2 AND source = ?3",
            params![account_id.value(), as_of_date.to_string(), source.as_str()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(Valuation {
        account_id,
        as_of_date,
        value: Amount::from_cents(row.0),
        observed_at: row.1,
        source,
    }))
}
