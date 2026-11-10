//! Persistence for the singleton owner, budget settings, sessions, and login
//! throttling.

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::{Db, DbError};

use super::password;
use super::session::Session;
use super::{AuthError, generate_token};

/// Failed-login accounting window and block duration.
pub const LOGIN_WINDOW_SECONDS: i64 = 15 * 60;
/// Failed attempts allowed per client within the window.
pub const CLIENT_FAILURE_LIMIT: i64 = 5;
/// Failed attempts allowed installation-wide within the window.
pub const GLOBAL_FAILURE_LIMIT: i64 = 25;
/// How long a limit violation blocks further attempts.
pub const LOGIN_BLOCK_SECONDS: i64 = 15 * 60;
/// Throttle row shared by all clients, including direct and proxied requests.
pub const GLOBAL_THROTTLE_KEY: &str = "__all_clients__";

const BUDGET_NAME_MAX_CHARS: usize = 120;
const TIMEZONE_MAX_BYTES: usize = 64;

/// Stored owner credential and revocation generation.
#[derive(Debug, Clone)]
pub struct OwnerRecord {
    pub password_hash: String,
    pub generation: i64,
}

/// Non-financial budget settings shown after login.
#[derive(Debug, Clone)]
pub struct BudgetSummary {
    pub name: String,
    pub currency: String,
    pub timezone: String,
}

/// Current time as Unix seconds, used for expiry and throttling.
pub fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

pub async fn owner_record(db: &Db) -> Result<Option<OwnerRecord>, DbError> {
    db.read(|conn| {
        conn.query_row(
            "SELECT password_hash, session_generation FROM owners WHERE id = 1",
            [],
            |row| {
                Ok(OwnerRecord {
                    password_hash: row.get(0)?,
                    generation: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(DbError::from)
    })
    .await
}

pub async fn budget_summary(db: &Db) -> Result<Option<BudgetSummary>, DbError> {
    db.read(|conn| {
        conn.query_row(
            "SELECT name, currency, timezone FROM budgets WHERE id = 1",
            [],
            |row| {
                Ok(BudgetSummary {
                    name: row.get(0)?,
                    currency: row.get(1)?,
                    timezone: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(DbError::from)
    })
    .await
}

/// Creates the singleton owner, USD budget, and timezone exactly once.
pub async fn initialize(
    db: &Db,
    password: &str,
    timezone: &str,
    budget_name: &str,
) -> Result<(), AuthError> {
    password::validate_new_password(password)?;
    let timezone = timezone.trim().to_owned();
    if timezone.is_empty()
        || timezone.len() > TIMEZONE_MAX_BYTES
        || jiff::tz::TimeZone::get(&timezone).is_err()
    {
        return Err(AuthError::InvalidTimezone { timezone });
    }
    let budget_name = budget_name.trim().to_owned();
    let name_chars = budget_name.chars().count();
    if name_chars == 0 || name_chars > BUDGET_NAME_MAX_CHARS {
        return Err(AuthError::InvalidBudgetName);
    }

    let hash = hash_off_thread(password.to_owned()).await?;
    let now = now_unix();
    let created = db
        .transaction(move |tx| {
            let inserted = tx.execute(
                "INSERT INTO owners (id, password_hash, password_changed_at, session_generation, created_at)
                 SELECT 1, ?1, ?2, 0, ?2 WHERE NOT EXISTS (SELECT 1 FROM owners)",
                params![hash, now],
            )?;
            if inserted == 0 {
                return Ok(false);
            }
            tx.execute(
                "INSERT INTO budgets (id, name, currency, timezone, created_at)
                 VALUES (1, ?1, 'USD', ?2, ?3)",
                params![budget_name, timezone, now],
            )?;
            Ok(true)
        })
        .await?;

    if created {
        Ok(())
    } else {
        Err(AuthError::AlreadyInitialized)
    }
}

/// Replaces the owner password and revokes every session without touching
/// financial data, provider secrets, or budget settings.
pub async fn recover(db: &Db, password: &str) -> Result<(), AuthError> {
    password::validate_new_password(password)?;
    let hash = hash_off_thread(password.to_owned()).await?;
    let now = now_unix();
    let updated = db
        .transaction(move |tx| {
            let updated = tx.execute(
                "UPDATE owners
                 SET password_hash = ?1,
                     password_changed_at = ?2,
                     session_generation = session_generation + 1
                 WHERE id = 1",
                params![hash, now],
            )?;
            if updated == 0 {
                return Ok(false);
            }
            tx.execute("DELETE FROM sessions", [])?;
            tx.execute("DELETE FROM login_throttle", [])?;
            Ok(true)
        })
        .await?;

    if updated {
        Ok(())
    } else {
        Err(AuthError::NotInitialized)
    }
}

/// Stores only a token hash and per-session CSRF token.
pub async fn create_session(db: &Db, session: &Session, now: i64) -> Result<(), DbError> {
    let session = session.clone();
    db.transaction(move |tx| {
        tx.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now])?;
        tx.execute(
            "INSERT INTO sessions (token_hash, owner_id, csrf_token, generation, created_at, expires_at)
             VALUES (?1, 1, ?2, ?3, ?4, ?5)",
            params![
                session.token_hash,
                session.csrf_token,
                session.generation,
                now,
                session.expires_at
            ],
        )?;
        Ok(())
    })
    .await
}

/// Loads a live session, enforcing expiry and the owner revocation generation.
pub async fn load_session(db: &Db, token_hash: &str, now: i64) -> Result<Option<Session>, DbError> {
    let token_hash = token_hash.to_owned();
    db.read(move |conn| {
        conn.query_row(
            "SELECT s.token_hash, s.csrf_token, s.expires_at, s.generation
             FROM sessions s
             JOIN owners o ON o.id = s.owner_id
             WHERE s.token_hash = ?1
               AND s.expires_at > ?2
               AND s.generation = o.session_generation",
            params![token_hash, now],
            |row| {
                Ok(Session {
                    token_hash: row.get(0)?,
                    csrf_token: row.get(1)?,
                    expires_at: row.get(2)?,
                    generation: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(DbError::from)
    })
    .await
}

pub async fn delete_session(db: &Db, token_hash: &str) -> Result<(), DbError> {
    let token_hash = token_hash.to_owned();
    db.transaction(move |tx| {
        tx.execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
        Ok(())
    })
    .await
}

/// Seconds until the client may attempt login again, if it is blocked.
pub async fn retry_after(db: &Db, client_key: &str, now: i64) -> Result<Option<i64>, DbError> {
    let client_key = client_key.to_owned();
    db.read(move |conn| retry_after_in(conn, &client_key, now))
        .await
}

fn retry_after_in(conn: &Connection, client_key: &str, now: i64) -> Result<Option<i64>, DbError> {
    let blocked_until: Option<i64> = conn.query_row(
        "SELECT MAX(blocked_until)
         FROM login_throttle
         WHERE client_key IN (?1, ?2) AND blocked_until > ?3",
        params![client_key, GLOBAL_THROTTLE_KEY, now],
        |row| row.get(0),
    )?;
    Ok(blocked_until.map(|until| (until - now).max(1)))
}

/// Records one failed attempt for the client and installation-wide counters.
pub async fn record_login_failure(
    db: &Db,
    client_key: &str,
    now: i64,
) -> Result<(), DbError> {
    let client_key = client_key.to_owned();
    db.transaction(move |tx| {
        record_failure_in(tx, &client_key, CLIENT_FAILURE_LIMIT, now)?;
        record_failure_in(tx, GLOBAL_THROTTLE_KEY, GLOBAL_FAILURE_LIMIT, now)?;
        Ok(())
    })
    .await
}

fn record_failure_in(
    conn: &Connection,
    client_key: &str,
    limit: i64,
    now: i64,
) -> Result<(), DbError> {
    let previous: Option<(i64, i64, Option<i64>)> = conn
        .query_row(
            "SELECT failures, window_started_at, blocked_until
             FROM login_throttle WHERE client_key = ?1",
            [client_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;

    let (mut failures, window_started_at, mut blocked_until) = match previous {
        Some((failures, window, blocked)) if now.saturating_sub(window) < LOGIN_WINDOW_SECONDS => {
            (failures, window, blocked)
        }
        _ => (0, now, None),
    };

    if blocked_until.is_none_or(|until| until <= now) {
        blocked_until = None;
        failures = failures.saturating_add(1);
        if failures >= limit {
            blocked_until = Some(now.saturating_add(LOGIN_BLOCK_SECONDS));
        }
    }

    conn.execute(
        "INSERT INTO login_throttle (client_key, failures, window_started_at, blocked_until)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(client_key) DO UPDATE SET
             failures = excluded.failures,
             window_started_at = excluded.window_started_at,
             blocked_until = excluded.blocked_until",
        params![client_key, failures, window_started_at, blocked_until],
    )?;
    Ok(())
}

/// Clears the successful client's failed-attempt counter.
pub async fn clear_login_failures(db: &Db, client_key: &str) -> Result<(), DbError> {
    let client_key = client_key.to_owned();
    db.transaction(move |tx| {
        tx.execute(
            "DELETE FROM login_throttle WHERE client_key = ?1",
            [client_key],
        )?;
        Ok(())
    })
    .await
}

async fn hash_off_thread(password: String) -> Result<String, AuthError> {
    tokio::task::spawn_blocking(move || password::hash_password(&password))
        .await
        .map_err(|_| AuthError::PasswordHashing)?
}

/// Verifies a password off the async executor. Public for the login handler,
/// which must not run Argon2 on a Tokio worker.
pub async fn verify_off_thread(encoded: String, password: String) -> bool {
    tokio::task::spawn_blocking(move || password::verify_password(&encoded, &password))
        .await
        .unwrap_or(false)
}

/// Generates a raw cookie token alongside a session that stores only its hash.
pub fn new_session(generation: i64, now: i64) -> Result<(String, Session), AuthError> {
    let token = generate_token()?;
    let csrf_token = generate_token()?;
    Ok((
        token.clone(),
        Session {
            token_hash: super::hash_token(&token),
            csrf_token,
            expires_at: now.saturating_add(super::SESSION_TTL_SECONDS),
            generation,
        },
    ))
}
