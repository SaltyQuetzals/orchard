use rusqlite::{Connection, OptionalExtension};

use crate::domain::{Amount, BudgetMonth, CategoryId};

use super::{
    BudgetAssignment, BudgetError, BudgetMove, CategoryTarget, CategoryTargetRecord,
};

pub(super) fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

pub(super) fn parse_month(
    table: &'static str,
    text: &str,
) -> Result<BudgetMonth, BudgetError> {
    text.parse().map_err(|source| BudgetError::CorruptRow {
        table,
        detail: format!("invalid month `{text}`: {source}"),
    })
}

/// Rejects missing categories and refuses new budget money for archived ones.
pub(super) fn ensure_available_category(
    conn: &Connection,
    id: CategoryId,
) -> Result<(), BudgetError> {
    let archived: Option<Option<i64>> = conn
        .query_row(
            "SELECT archived_at FROM categories WHERE id = ?1",
            [id.value()],
            |row| row.get(0),
        )
        .optional()?;
    match archived {
        None => Err(BudgetError::CategoryNotFound { id }),
        Some(Some(_)) => Err(BudgetError::ArchivedCategory { id }),
        Some(None) => Ok(()),
    }
}

pub(super) fn load_assignment(
    conn: &Connection,
    month: BudgetMonth,
    category: CategoryId,
) -> Result<Option<BudgetAssignment>, BudgetError> {
    let row = conn
        .query_row(
            "SELECT id, amount_cents, revision
             FROM budget_assignments
             WHERE month = ?1 AND category_id = ?2",
            rusqlite::params![month.to_string(), category.value()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(row.map(|(id, amount, revision)| BudgetAssignment {
        id,
        month,
        category_id: category,
        amount: Amount::from_cents(amount),
        revision,
    }))
}

pub(super) fn load_move(conn: &Connection, id: i64) -> Result<BudgetMove, BudgetError> {
    let row = conn
        .query_row(
            "SELECT id, month, from_category_id, to_category_id, amount_cents, revision
             FROM budget_moves WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(BudgetError::MoveNotFound { id })?;
    Ok(BudgetMove {
        id: row.0,
        month: parse_month("budget_moves", &row.1)?,
        from_category_id: CategoryId::new(row.2),
        to_category_id: CategoryId::new(row.3),
        amount: Amount::from_cents(row.4),
        revision: row.5,
    })
}

pub(super) fn load_target(
    conn: &Connection,
    category: CategoryId,
) -> Result<Option<CategoryTargetRecord>, BudgetError> {
    let row = conn
        .query_row(
            "SELECT kind, amount_cents, target_month, revision
             FROM category_targets WHERE category_id = ?1",
            [category.value()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((kind, amount, target_month, revision)) = row else {
        return Ok(None);
    };
    let amount = Amount::from_cents(amount);
    let target = match (kind.as_str(), target_month) {
        ("balance", Some(month)) => CategoryTarget::BalanceByMonth {
            amount,
            month: parse_month("category_targets", &month)?,
        },
        ("contribution", None) => CategoryTarget::MonthlyContribution { amount },
        (kind, month) => {
            return Err(BudgetError::CorruptRow {
                table: "category_targets",
                detail: format!("kind `{kind}` does not match target month {month:?}"),
            });
        }
    };
    Ok(Some(CategoryTargetRecord {
        category_id: category,
        target,
        revision,
    }))
}
