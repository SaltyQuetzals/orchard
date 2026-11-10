use rusqlite::{params, Connection};

use crate::db::Db;
use crate::domain::{Amount, BudgetMonth, CategoryId};

use super::rows::{
    ensure_available_category, load_assignment, load_move, load_target, now_unix, parse_month,
};
use super::{
    BudgetAssignment, BudgetError, BudgetMove, CategoryTarget, CategoryTargetRecord,
};

/// Creates or replaces the single assignment for one category in one month.
///
/// `expected_revision` is `None` when creating a new assignment and `Some`
/// when changing an existing one. Assignments may be negative, which returns
/// money to Ready to Assign. Assigning more than is unallocated is allowed and
/// makes Ready to Assign visibly negative.
pub async fn set_assignment(
    db: &Db,
    month: BudgetMonth,
    category_id: CategoryId,
    amount: Amount,
    expected_revision: Option<i64>,
) -> Result<BudgetAssignment, BudgetError> {
    if amount.is_zero() {
        return Err(BudgetError::ZeroAssignment);
    }
    db.run_transaction(move |tx| {
        ensure_available_category(tx, category_id)?;
        match (load_assignment(tx, month, category_id)?, expected_revision) {
            (None, None) => {
                tx.execute(
                    "INSERT INTO budget_assignments (
                        month, category_id, amount_cents, revision, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, 1, ?4, ?4)",
                    params![month.to_string(), category_id.value(), amount.cents(), now_unix()],
                )?;
            }
            (None, Some(_)) => {
                return Err(BudgetError::AssignmentNotFound {
                    month,
                    category: category_id,
                });
            }
            (Some(_), None) => {
                return Err(BudgetError::DuplicateAssignment {
                    month,
                    category: category_id,
                });
            }
            (Some(existing), Some(expected)) => {
                if existing.revision != expected {
                    return Err(BudgetError::StaleRevision {
                        entity: "assignment",
                        id: existing.id,
                        expected,
                        actual: existing.revision,
                    });
                }
                tx.execute(
                    "UPDATE budget_assignments
                     SET amount_cents = ?1, revision = revision + 1, updated_at = ?2
                     WHERE id = ?3",
                    params![amount.cents(), now_unix(), existing.id],
                )?;
            }
        }
        load_assignment(tx, month, category_id)?.ok_or(BudgetError::CorruptRow {
            table: "budget_assignments",
            detail: "assignment was not found after writing".to_owned(),
        })
    })
    .await
}

pub async fn clear_assignment(
    db: &Db,
    month: BudgetMonth,
    category_id: CategoryId,
    expected_revision: i64,
) -> Result<(), BudgetError> {
    db.run_transaction(move |tx| {
        let existing = load_assignment(tx, month, category_id)?.ok_or(
            BudgetError::AssignmentNotFound {
                month,
                category: category_id,
            },
        )?;
        if existing.revision != expected_revision {
            return Err(BudgetError::StaleRevision {
                entity: "assignment",
                id: existing.id,
                expected: expected_revision,
                actual: existing.revision,
            });
        }
        tx.execute(
            "DELETE FROM budget_assignments WHERE id = ?1",
            [existing.id],
        )?;
        Ok(())
    })
    .await
}

pub async fn assignments(
    db: &Db,
    month: Option<BudgetMonth>,
) -> Result<Vec<BudgetAssignment>, BudgetError> {
    db.run_read(move |conn| {
        let sql = match month {
            Some(_) => {
                "SELECT id, month, category_id, amount_cents, revision
                 FROM budget_assignments WHERE month = ?1
                 ORDER BY month, category_id"
            }
            None => {
                "SELECT id, month, category_id, amount_cents, revision
                 FROM budget_assignments ORDER BY month, category_id"
            }
        };
        let mut statement = conn.prepare(sql)?;
        let mut records = Vec::new();
        if let Some(month) = month {
            for row in statement.query_map([month.to_string()], assignment_row)? {
                records.push(assignment_from_row(row?)?);
            }
        } else {
            for row in statement.query_map([], assignment_row)? {
                records.push(assignment_from_row(row?)?);
            }
        }
        Ok(records)
    })
    .await
}

fn assignment_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(i64, String, i64, i64, i64)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn assignment_from_row(
    row: (i64, String, i64, i64, i64),
) -> Result<BudgetAssignment, BudgetError> {
    Ok(BudgetAssignment {
        id: row.0,
        month: parse_month("budget_assignments", &row.1)?,
        category_id: CategoryId::new(row.2),
        amount: Amount::from_cents(row.3),
        revision: row.4,
    })
}

/// Moves money between two categories in the same month without touching Ready
/// to Assign.
pub async fn move_budget(
    db: &Db,
    month: BudgetMonth,
    from_category_id: CategoryId,
    to_category_id: CategoryId,
    amount: Amount,
) -> Result<BudgetMove, BudgetError> {
    if !amount.is_positive() {
        return Err(BudgetError::InvalidMoveAmount);
    }
    if from_category_id == to_category_id {
        return Err(BudgetError::SameCategoryMove);
    }
    db.run_transaction(move |tx| {
        ensure_available_category(tx, from_category_id)?;
        ensure_available_category(tx, to_category_id)?;
        tx.execute(
            "INSERT INTO budget_moves (
                month, from_category_id, to_category_id, amount_cents,
                revision, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
            params![
                month.to_string(),
                from_category_id.value(),
                to_category_id.value(),
                amount.cents(),
                now_unix()
            ],
        )?;
        load_move(tx, tx.last_insert_rowid())
    })
    .await
}

pub async fn edit_budget_move(
    db: &Db,
    id: i64,
    expected_revision: i64,
    amount: Amount,
) -> Result<BudgetMove, BudgetError> {
    if !amount.is_positive() {
        return Err(BudgetError::InvalidMoveAmount);
    }
    db.run_transaction(move |tx| {
        let existing = load_move(tx, id)?;
        if existing.revision != expected_revision {
            return Err(BudgetError::StaleRevision {
                entity: "budget move",
                id,
                expected: expected_revision,
                actual: existing.revision,
            });
        }
        tx.execute(
            "UPDATE budget_moves
             SET amount_cents = ?1, revision = revision + 1, updated_at = ?2
             WHERE id = ?3",
            params![amount.cents(), now_unix(), id],
        )?;
        load_move(tx, id)
    })
    .await
}

pub async fn delete_budget_move(
    db: &Db,
    id: i64,
    expected_revision: i64,
) -> Result<(), BudgetError> {
    db.run_transaction(move |tx| {
        let existing = load_move(tx, id)?;
        if existing.revision != expected_revision {
            return Err(BudgetError::StaleRevision {
                entity: "budget move",
                id,
                expected: expected_revision,
                actual: existing.revision,
            });
        }
        tx.execute("DELETE FROM budget_moves WHERE id = ?1", [id])?;
        Ok(())
    })
    .await
}

pub async fn budget_moves(
    db: &Db,
    month: Option<BudgetMonth>,
) -> Result<Vec<BudgetMove>, BudgetError> {
    db.run_read(move |conn| {
        let ids: Vec<i64> = match month {
            Some(month) => {
                let mut statement = conn.prepare(
                    "SELECT id FROM budget_moves WHERE month = ?1 ORDER BY id",
                )?;
                let rows = statement.query_map([month.to_string()], |row| {
                    row.get::<_, i64>(0)
                })?;
                rows.collect::<Result<_, _>>()?
            }
            None => {
                let mut statement =
                    conn.prepare("SELECT id FROM budget_moves ORDER BY month, id")?;
                let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
                rows.collect::<Result<_, _>>()?
            }
        };
        ids.into_iter().map(|id| load_move(conn, id)).collect()
    })
    .await
}

/// Creates or replaces a category's savings target. Setting a target never
/// creates an assignment.
pub async fn set_category_target(
    db: &Db,
    category_id: CategoryId,
    target: CategoryTarget,
    expected_revision: Option<i64>,
) -> Result<CategoryTargetRecord, BudgetError> {
    if !target.amount().is_positive() {
        return Err(BudgetError::InvalidTargetAmount);
    }
    db.run_transaction(move |tx| {
        ensure_available_category(tx, category_id)?;
        let (kind, amount, target_month) = target_columns(target);
        match (load_target(tx, category_id)?, expected_revision) {
            (None, None) => {
                tx.execute(
                    "INSERT INTO category_targets (
                        category_id, kind, amount_cents, target_month, revision, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                    params![
                        category_id.value(),
                        kind,
                        amount.cents(),
                        target_month,
                        now_unix()
                    ],
                )?;
            }
            (None, Some(_)) => return Err(BudgetError::TargetNotFound { category: category_id }),
            (Some(existing), Some(expected)) => {
                if existing.revision != expected {
                    return Err(BudgetError::StaleRevision {
                        entity: "category target",
                        id: category_id.value(),
                        expected,
                        actual: existing.revision,
                    });
                }
                tx.execute(
                    "UPDATE category_targets
                     SET kind = ?1, amount_cents = ?2, target_month = ?3,
                         revision = revision + 1, updated_at = ?4
                     WHERE category_id = ?5",
                    params![kind, amount.cents(), target_month, now_unix(), category_id.value()],
                )?;
            }
            (Some(_), None) => {
                return Err(BudgetError::DuplicateTarget { category: category_id });
            }
        }
        load_target(tx, category_id)?.ok_or(BudgetError::CorruptRow {
            table: "category_targets",
            detail: "target was not found after writing".to_owned(),
        })
    })
    .await
}

fn target_columns(
    target: CategoryTarget,
) -> (&'static str, Amount, Option<String>) {
    match target {
        CategoryTarget::BalanceByMonth { amount, month } => {
            ("balance", amount, Some(month.to_string()))
        }
        CategoryTarget::MonthlyContribution { amount } => ("contribution", amount, None),
    }
}

pub async fn clear_category_target(
    db: &Db,
    category_id: CategoryId,
    expected_revision: i64,
) -> Result<(), BudgetError> {
    db.run_transaction(move |tx| {
        let existing = load_target(tx, category_id)?
            .ok_or(BudgetError::TargetNotFound { category: category_id })?;
        if existing.revision != expected_revision {
            return Err(BudgetError::StaleRevision {
                entity: "category target",
                id: category_id.value(),
                expected: expected_revision,
                actual: existing.revision,
            });
        }
        tx.execute(
            "DELETE FROM category_targets WHERE category_id = ?1",
            [category_id.value()],
        )?;
        Ok(())
    })
    .await
}

pub async fn category_target(
    db: &Db,
    category_id: CategoryId,
) -> Result<Option<CategoryTargetRecord>, BudgetError> {
    db.run_read(move |conn| load_target(conn, category_id))
        .await
}

pub async fn category_targets(db: &Db) -> Result<Vec<CategoryTargetRecord>, BudgetError> {
    db.run_read(|conn: &Connection| {
        let ids: Vec<i64> = {
            let mut statement = conn.prepare(
                "SELECT category_id FROM category_targets ORDER BY category_id",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut targets = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(target) = load_target(conn, CategoryId::new(id))? {
                targets.push(target);
            }
        }
        Ok(targets)
    })
    .await
}
