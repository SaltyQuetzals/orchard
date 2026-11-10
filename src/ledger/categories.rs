use rusqlite::{Connection, OptionalExtension, params};

use crate::db::Db;
use crate::domain::{CategoryGroupId, CategoryId};

use super::rows::{load_category, load_category_group, now_unix};
use super::{Category, CategoryGroup, LedgerError};

const MAX_NAME_CHARS: usize = 120;

pub async fn create_category_group(
    db: &Db,
    name: &str,
) -> Result<CategoryGroup, LedgerError> {
    let name = validate_name(name, LedgerError::InvalidCategoryGroupName)?;
    db.run_transaction(move |tx| {
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM category_groups WHERE name = ?1",
            [&name],
            |row| row.get(0),
        )?;
        if exists > 0 {
            return Err(LedgerError::DuplicateCategoryGroupName { name });
        }
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM category_groups",
            [],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO category_groups (name, position, archived_at, created_at)
             VALUES (?1, ?2, NULL, ?3)",
            params![name, position, now_unix()],
        )?;
        load_category_group(tx, CategoryGroupId::new(tx.last_insert_rowid()))
    })
    .await
}

pub async fn create_category(
    db: &Db,
    group_id: CategoryGroupId,
    name: &str,
) -> Result<Category, LedgerError> {
    let name = validate_name(name, LedgerError::InvalidCategoryName)?;
    db.run_transaction(move |tx| {
        let group = load_category_group(tx, group_id)?;
        if group.archived {
            return Err(LedgerError::ArchivedCategoryGroup { id: group_id });
        }
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM categories WHERE group_id = ?1 AND name = ?2",
            params![group_id.value(), name],
            |row| row.get(0),
        )?;
        if exists > 0 {
            return Err(LedgerError::DuplicateCategoryName { name });
        }
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM categories WHERE group_id = ?1",
            [group_id.value()],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO categories (group_id, name, position, archived_at, created_at)
             VALUES (?1, ?2, ?3, NULL, ?4)",
            params![group_id.value(), name, position, now_unix()],
        )?;
        load_category(tx, CategoryId::new(tx.last_insert_rowid()))
    })
    .await
}

pub async fn archive_category(db: &Db, id: CategoryId) -> Result<Category, LedgerError> {
    db.run_transaction(move |tx| {
        let category = load_category(tx, id)?;
        if !category.archived {
            tx.execute(
                "UPDATE categories SET archived_at = ?1 WHERE id = ?2",
                params![now_unix(), id.value()],
            )?;
        }
        load_category(tx, id)
    })
    .await
}

pub async fn category(db: &Db, id: CategoryId) -> Result<Category, LedgerError> {
    db.run_read(move |conn| load_category(conn, id)).await
}

pub async fn categories(db: &Db) -> Result<Vec<Category>, LedgerError> {
    db.run_read(|conn| {
        let ids: Vec<i64> = {
            let mut statement = conn.prepare(
                "SELECT id FROM categories
                 ORDER BY group_id, position, id",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        ids.into_iter()
            .map(|id| load_category(conn, CategoryId::new(id)))
            .collect()
    })
    .await
}

pub async fn category_groups(db: &Db) -> Result<Vec<CategoryGroup>, LedgerError> {
    db.run_read(|conn| {
        let ids: Vec<i64> = {
            let mut statement =
                conn.prepare("SELECT id FROM category_groups ORDER BY position, id")?;
            let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        ids.into_iter()
            .map(|id| load_category_group(conn, CategoryGroupId::new(id)))
            .collect()
    })
    .await
}

/// Creates or reuses the system group and a uniquely named per-card payment
/// category. Called while opening a credit-card account.
pub(super) fn ensure_payment_category(
    conn: &Connection,
    account_name: &str,
) -> Result<CategoryId, LedgerError> {
    let group_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM category_groups WHERE name = 'Credit Card Payments'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let group_id = match group_id {
        Some(id) => id,
        None => {
            let position: i64 = conn.query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM category_groups",
                [],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO category_groups (name, position, archived_at, created_at)
                 VALUES ('Credit Card Payments', ?1, NULL, ?2)",
                params![position, now_unix()],
            )?;
            conn.last_insert_rowid()
        }
    };

    let base = format!("{account_name} payment");
    let mut candidate = base.clone();
    let mut suffix = 2;
    loop {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM categories WHERE group_id = ?1 AND name = ?2",
            params![group_id, candidate],
            |row| row.get(0),
        )?;
        if exists == 0 {
            break;
        }
        candidate = format!("{base} ({suffix})");
        suffix += 1;
    }

    let position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM categories WHERE group_id = ?1",
        [group_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO categories (group_id, name, position, archived_at, created_at)
         VALUES (?1, ?2, ?3, NULL, ?4)",
        params![group_id, candidate, position, now_unix()],
    )?;
    Ok(CategoryId::new(conn.last_insert_rowid()))
}

fn validate_name(name: &str, invalid: LedgerError) -> Result<String, LedgerError> {
    let trimmed = name.trim();
    let chars = trimmed.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS {
        return Err(invalid);
    }
    Ok(trimmed.to_owned())
}
