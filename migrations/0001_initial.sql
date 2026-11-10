-- Initial Orchard store: installation metadata and encrypted provider secrets.
CREATE TABLE app_meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value BLOB NOT NULL
) STRICT;

CREATE TABLE provider_secrets (
    provider   TEXT PRIMARY KEY NOT NULL,
    envelope   BLOB NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
