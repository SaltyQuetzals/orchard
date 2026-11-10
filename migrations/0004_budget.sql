-- Envelope budget: monthly assignments, category-to-category moves, and
-- savings targets. Monthly availability, activity, card reserves, and
-- Ready to Assign are derived from these records plus ledger activity; no
-- projected balance is persisted as a second authoritative table.

CREATE TABLE budget_assignments (
    id           INTEGER PRIMARY KEY,
    month        TEXT NOT NULL CHECK (month GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'),
    category_id  INTEGER NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
    amount_cents INTEGER NOT NULL CHECK (amount_cents <> 0),
    revision     INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE (month, category_id)
) STRICT;

CREATE TABLE budget_moves (
    id               INTEGER PRIMARY KEY,
    month            TEXT NOT NULL CHECK (month GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'),
    from_category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
    to_category_id   INTEGER NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
    amount_cents     INTEGER NOT NULL CHECK (amount_cents > 0),
    revision         INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    CHECK (from_category_id <> to_category_id)
) STRICT;

CREATE INDEX budget_moves_month ON budget_moves(month);

CREATE TABLE category_targets (
    category_id  INTEGER PRIMARY KEY REFERENCES categories(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('balance', 'contribution')),
    amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
    target_month TEXT CHECK (target_month IS NULL OR target_month GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'),
    revision     INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    updated_at   INTEGER NOT NULL,
    CHECK ((kind = 'balance') = (target_month IS NOT NULL))
) STRICT;
