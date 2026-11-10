//! Single-owner password, session, and login-throttle policy.
//!
//! Initialization and recovery are host-access operations performed by CLI
//! commands under the exclusive store lock. The web layer only verifies
//! credentials and manages server-side sessions.

mod error;
mod password;
mod session;
mod store;

pub use error::AuthError;
pub use session::{
    SESSION_TTL_SECONDS, Session, generate_token, hash_token, is_token_shape, tokens_match,
};
pub use store::{
    BudgetSummary, CLIENT_FAILURE_LIMIT, GLOBAL_FAILURE_LIMIT, LOGIN_BLOCK_SECONDS,
    LOGIN_WINDOW_SECONDS, OwnerRecord, budget_summary, clear_login_failures,
    create_session, delete_session, initialize, load_session, new_session, now_unix, owner_record,
    record_login_failure, recover, retry_after, verify_off_thread,
};
