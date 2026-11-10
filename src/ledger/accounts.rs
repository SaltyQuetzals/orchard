use rusqlite::params;

use crate::db::Db;
use crate::domain::{AccountId, AccountKind, BudgetDate, ClearedState, TransactionKind, TransactionOrigin};

use super::categories::ensure_payment_category;
use super::rows::{account_balance_in, insert_entry, load_account, now_unix, NewEntry};
use super::{Account, LedgerError, NewAccount};

const MAX_NAME_CHARS: usize = 120;

pub async fn create_account(db: &Db, new: NewAccount<'_>) -> Result<Account, LedgerError> {
    let name = new.name.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS {
        return Err(LedgerError::InvalidAccountName);
    }
    let name = name.to_owned();
    db.run_transaction(move |tx| {
        let payment_category_id = if new.kind.is_credit_card() {
            Some(ensure_payment_category(tx, &name)?)
        } else {
            None
        };
        tx.execute(
            "INSERT INTO accounts (
                name, kind, payment_category_id, closed_on, revision, created_at
             ) VALUES (?1, ?2, ?3, NULL, 1, ?4)",
            params![
                name,
                new.kind.as_str(),
                payment_category_id.map(|id| id.value()),
                now_unix()
            ],
        )?;
        let id = AccountId::new(tx.last_insert_rowid());

        if !new.opening_balance.is_zero() {
            if new.kind.is_budget() {
                insert_entry(
                    tx,
                    NewEntry {
                        account_id: id,
                        kind: TransactionKind::OpeningBalance,
                        origin: TransactionOrigin::Opening,
                        date: new.opened_on,
                        payee: "Opening balance",
                        memo: "",
                        amount: new.opening_balance,
                        cleared: ClearedState::Cleared,
                    },
                )?;
            } else {
                // Tracking value is authoritative from valuations; an opening
                // amount is the first manual valuation, not a register entry.
                tx.execute(
                    "INSERT INTO account_valuations (
                        account_id, as_of_date, value_cents, observed_at, source
                     ) VALUES (?1, ?2, ?3, ?4, 'manual')",
                    params![
                        id.value(),
                        new.opened_on.to_string(),
                        new.opening_balance.cents(),
                        now_unix()
                    ],
                )?;
            }
        }

        load_account(tx, id)
    })
    .await
}

pub async fn close_account(
    db: &Db,
    id: AccountId,
    expected_revision: i64,
    closed_on: BudgetDate,
) -> Result<Account, LedgerError> {
    db.run_transaction(move |tx| {
        let account = load_account(tx, id)?;
        if account.revision != expected_revision {
            return Err(LedgerError::StaleRevision {
                entity: "account",
                id: id.value(),
                expected: expected_revision,
                actual: account.revision,
            });
        }
        if account.closed_on.is_some() {
            return Err(LedgerError::AccountAlreadyClosed { id });
        }
        if account.kind.is_budget() {
            let balance = account_balance_in(tx, id, None)?;
            if !balance.is_zero() {
                return Err(LedgerError::NonzeroAccountBalance { id, balance });
            }
        }
        tx.execute(
            "UPDATE accounts SET closed_on = ?1, revision = revision + 1 WHERE id = ?2",
            params![closed_on.to_string(), id.value()],
        )?;
        load_account(tx, id)
    })
    .await
}

pub async fn account(db: &Db, id: AccountId) -> Result<Account, LedgerError> {
    db.run_read(move |conn| load_account(conn, id)).await
}

pub async fn accounts(db: &Db, include_closed: bool) -> Result<Vec<Account>, LedgerError> {
    db.run_read(move |conn| {
        let include_closed = if include_closed { 1_i64 } else { 0_i64 };
        let ids: Vec<i64> = {
            let mut statement = conn.prepare(
                "SELECT id FROM accounts
                 WHERE ?1 = 1 OR closed_on IS NULL
                 ORDER BY id",
            )?;
            let rows = statement.query_map([include_closed], |row| {
                row.get::<_, i64>(0)
            })?;
            rows.collect::<Result<_, _>>()?
        };
        ids.into_iter()
            .map(|id| load_account(conn, AccountId::new(id)))
            .collect()
    })
    .await
}

pub async fn account_balance(
    db: &Db,
    id: AccountId,
    as_of: Option<BudgetDate>,
) -> Result<crate::domain::Amount, LedgerError> {
    db.run_read(move |conn| {
        // Reject unknown accounts rather than returning a plausible zero.
        load_account(conn, id)?;
        account_balance_in(conn, id, as_of)
    })
    .await
}

pub async fn account_kind(db: &Db, id: AccountId) -> Result<AccountKind, LedgerError> {
    Ok(account(db, id).await?.kind)
}
