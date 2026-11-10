-- Single-owner installation: credentials, USD budget settings, server sessions,
-- and durable login throttling.
CREATE TABLE owners (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    password_hash       TEXT NOT NULL,
    password_changed_at INTEGER NOT NULL,
    session_generation  INTEGER NOT NULL DEFAULT 0 CHECK (session_generation >= 0),
    created_at          INTEGER NOT NULL
) STRICT;

CREATE TABLE budgets (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    name       TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    currency   TEXT NOT NULL CHECK (currency = 'USD'),
    timezone   TEXT NOT NULL CHECK (length(timezone) BETWEEN 1 AND 64),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    owner_id   INTEGER NOT NULL REFERENCES owners(id) ON DELETE CASCADE,
    csrf_token TEXT NOT NULL,
    generation INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX sessions_expires_at ON sessions(expires_at);

CREATE TABLE login_throttle (
    client_key       TEXT PRIMARY KEY NOT NULL,
    failures         INTEGER NOT NULL CHECK (failures >= 0),
    window_started_at INTEGER NOT NULL,
    blocked_until    INTEGER
) STRICT;
