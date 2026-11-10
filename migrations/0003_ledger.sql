-- Account ledger: accounts, entries, splits, linked transfers, reconciliation,
-- category references, import review, and tracking valuations.

CREATE TABLE category_groups (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    position    INTEGER NOT NULL CHECK (position >= 0),
    archived_at INTEGER,
    created_at  INTEGER NOT NULL,
    UNIQUE (name)
) STRICT;

CREATE TABLE categories (
    id          INTEGER PRIMARY KEY,
    group_id    INTEGER NOT NULL REFERENCES category_groups(id) ON DELETE RESTRICT,
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    position    INTEGER NOT NULL CHECK (position >= 0),
    archived_at INTEGER,
    created_at  INTEGER NOT NULL,
    UNIQUE (group_id, name)
) STRICT;

CREATE TABLE accounts (
    id                  INTEGER PRIMARY KEY,
    name                TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    kind                TEXT NOT NULL CHECK (kind IN ('checking','savings','cash','credit_card','tracking')),
    payment_category_id INTEGER REFERENCES categories(id) ON DELETE RESTRICT,
    closed_on           TEXT CHECK (closed_on IS NULL OR closed_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    revision            INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at          INTEGER NOT NULL,
    CHECK ((kind = 'credit_card') = (payment_category_id IS NOT NULL))
) STRICT;

CREATE TABLE transactions (
    id               INTEGER PRIMARY KEY,
    account_id       INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    kind             TEXT NOT NULL CHECK (kind IN ('income','outflow','refund','transfer','opening_balance','adjustment')),
    origin           TEXT NOT NULL CHECK (origin IN ('manual','import','opening','adjustment')),
    transaction_date TEXT NOT NULL CHECK (transaction_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    payee            TEXT NOT NULL DEFAULT '' CHECK (length(payee) <= 200),
    memo             TEXT NOT NULL DEFAULT '' CHECK (length(memo) <= 1000),
    amount_cents     INTEGER NOT NULL,
    cleared_state    TEXT NOT NULL DEFAULT 'uncleared' CHECK (cleared_state IN ('uncleared','cleared','reconciled')),
    revision         INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    CHECK (kind <> 'opening_balance' OR origin = 'opening'),
    CHECK (kind <> 'adjustment' OR origin = 'adjustment')
) STRICT;

CREATE INDEX transactions_account_date ON transactions(account_id, transaction_date);

CREATE TABLE allocations (
    id             INTEGER PRIMARY KEY,
    transaction_id INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    category_id    INTEGER REFERENCES categories(id) ON DELETE RESTRICT,
    amount_cents   INTEGER NOT NULL CHECK (amount_cents <> 0),
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
) STRICT;

CREATE INDEX allocations_transaction ON allocations(transaction_id);
CREATE INDEX allocations_category ON allocations(category_id);

CREATE TABLE transfers (
    id                   INTEGER PRIMARY KEY,
    from_transaction_id  INTEGER NOT NULL UNIQUE REFERENCES transactions(id) ON DELETE RESTRICT,
    to_transaction_id    INTEGER NOT NULL UNIQUE REFERENCES transactions(id) ON DELETE RESTRICT,
    boundary             TEXT NOT NULL CHECK (boundary IN ('budget','budget_tracking')),
    boundary_target      TEXT CHECK (boundary_target IN ('category','ready_to_assign')),
    boundary_category_id INTEGER REFERENCES categories(id) ON DELETE RESTRICT,
    created_at           INTEGER NOT NULL,
    CHECK (from_transaction_id <> to_transaction_id),
    CHECK (
        (boundary = 'budget' AND boundary_target IS NULL AND boundary_category_id IS NULL)
        OR (boundary = 'budget_tracking' AND boundary_target IS NOT NULL)
    ),
    CHECK (
        (boundary_target = 'category' AND boundary_category_id IS NOT NULL)
        OR (boundary_target = 'ready_to_assign' AND boundary_category_id IS NULL)
        OR (boundary_target IS NULL AND boundary_category_id IS NULL)
    )
) STRICT;

ALTER TABLE transactions
    ADD COLUMN transfer_id INTEGER REFERENCES transfers(id) ON DELETE SET NULL;

CREATE INDEX transactions_transfer ON transactions(transfer_id);

CREATE TABLE reconciliations (
    id                        INTEGER PRIMARY KEY,
    account_id                INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    statement_date            TEXT NOT NULL CHECK (statement_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    statement_balance_cents   INTEGER NOT NULL,
    cleared_balance_cents     INTEGER NOT NULL,
    difference_cents          INTEGER NOT NULL,
    adjustment_transaction_id INTEGER REFERENCES transactions(id) ON DELETE RESTRICT,
    status                    TEXT NOT NULL CHECK (status IN ('open','completed','invalidated')),
    revision                  INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at                INTEGER NOT NULL,
    completed_at              INTEGER,
    invalidated_at            INTEGER
) STRICT;

CREATE INDEX reconciliations_account ON reconciliations(account_id, statement_date);

CREATE TABLE provider_connections (
    id         INTEGER PRIMARY KEY,
    provider   TEXT NOT NULL CHECK (length(provider) BETWEEN 1 AND 40),
    label      TEXT NOT NULL DEFAULT '' CHECK (length(label) <= 120),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE source_records (
    id                        INTEGER PRIMARY KEY,
    connection_id             INTEGER NOT NULL REFERENCES provider_connections(id) ON DELETE RESTRICT,
    institution_connection_id TEXT NOT NULL CHECK (length(institution_connection_id) > 0),
    external_account_id       TEXT NOT NULL CHECK (length(external_account_id) > 0),
    external_transaction_id   TEXT NOT NULL CHECK (length(external_transaction_id) > 0),
    account_id                INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    source_date               TEXT NOT NULL CHECK (source_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    amount_cents              INTEGER NOT NULL CHECK (amount_cents <> 0),
    payee                     TEXT NOT NULL DEFAULT '' CHECK (length(payee) <= 200),
    memo                      TEXT NOT NULL DEFAULT '' CHECK (length(memo) <= 1000),
    status                    TEXT NOT NULL CHECK (status IN ('pending_review','approved','dismissed','deleted')),
    transaction_id            INTEGER UNIQUE REFERENCES transactions(id) ON DELETE SET NULL,
    source_revision           INTEGER NOT NULL DEFAULT 1 CHECK (source_revision >= 1),
    first_seen_at             INTEGER NOT NULL,
    last_seen_at              INTEGER NOT NULL,
    UNIQUE (connection_id, institution_connection_id, external_account_id, external_transaction_id)
) STRICT;

CREATE INDEX source_records_account_status ON source_records(account_id, status);

CREATE TABLE source_corrections (
    id                    INTEGER PRIMARY KEY,
    source_id             INTEGER NOT NULL REFERENCES source_records(id) ON DELETE CASCADE,
    previous_amount_cents INTEGER NOT NULL,
    previous_date         TEXT NOT NULL CHECK (previous_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    proposed_amount_cents INTEGER NOT NULL,
    proposed_date         TEXT NOT NULL CHECK (proposed_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    status                TEXT NOT NULL CHECK (status IN ('pending','applied','dismissed')),
    created_at            INTEGER NOT NULL,
    resolved_at           INTEGER
) STRICT;

CREATE UNIQUE INDEX source_corrections_pending
    ON source_corrections(source_id) WHERE status = 'pending';

CREATE TABLE account_valuations (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    as_of_date  TEXT NOT NULL CHECK (as_of_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    value_cents INTEGER NOT NULL,
    observed_at INTEGER NOT NULL,
    source      TEXT NOT NULL CHECK (source IN ('provider','manual')),
    UNIQUE (account_id, as_of_date, source)
) STRICT;

CREATE INDEX account_valuations_account_date ON account_valuations(account_id, as_of_date);
