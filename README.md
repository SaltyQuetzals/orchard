# Orchard

Self-hosted, single-owner envelope budgeting for one USD budget. The application is a
Rust/Axum server with Askama templates, a single SQLite store, and directory-based receipt
evidence. No frontend framework, Node toolchain, hosted database, or application subscription
is required.

This repository is being implemented from the OpenSpec change
`add-self-hosted-budgeting`. The current build provides the runtime foundation: HTTP delivery,
the store, exact money/date types, migrations, and credential-key handling. Financial pages,
authentication, imports, and integrations are added by later sections.

## Requirements

- Rust 1.93.1, pinned by `rust-toolchain.toml` (rustup installs it automatically).
- SQLite is compiled in through `rusqlite`'s bundled build; no system SQLite package is needed.
- No Node.js or JavaScript build pipeline.

## Build and test

```sh
cargo build --locked
cargo test --locked
```

Use `--release` for a production binary. The committed `Cargo.lock` is required; do not pass
`--locked` only after intentionally updating dependencies.

## Credential key

Provider credentials are encrypted with AES-256-GCM using a 32-byte master key. The key lives
in a mounted secret file outside the data directory:

```sh
mkdir -p "$HOME/.config/orchard"
openssl rand -hex 32 > "$HOME/.config/orchard/credentials.key"
chmod 600 "$HOME/.config/orchard/credentials.key"
```

The file must contain exactly 64 hexadecimal characters (trailing whitespace is ignored). Keep
it out of backups of `/data` and store it separately: losing it makes stored provider secrets
unrecoverable, and a wrong key is refused rather than replacing them.

## Run

```sh
cargo run --locked -- serve \
  --bind 127.0.0.1:3000 \
  --data-dir ./data \
  --key-file "$HOME/.config/orchard/credentials.key"
```

`--key-file` may instead come from `ORCHARD_SECRET_KEY_FILE`. The data directory defaults to
`/data`; the bind address defaults to `127.0.0.1:3000`.

```sh
curl -fsS http://127.0.0.1:3000/health
# ok
```

The health response is intentionally non-sensitive: it confirms the process opened its store,
acquired its lock, applied migrations, and verified the credential key.

Stop with Ctrl-C or `SIGTERM`; shutdown is graceful and releases the store lock. See
[`docs/operations.md`](docs/operations.md) for operational details.

## Data layout

```text
<data-dir>/
  orchard.sqlite3        SQLite database (WAL: -wal and -shm files appear beside it)
  orchard.lock           exclusive single-process lock
  evidence/
    objects/            immutable original evidence (written by later receipt work)
    tmp/                staging for receipt intake
```

The credential key file must not live inside `<data-dir>`.

## Single-process SQLite model

- One process owns one installation; the lock file rejects a second server or maintenance
  process on the same data directory.
- One SQLite connection sits behind a mutex, and database work runs on Tokio's blocking pool
  behind a semaphore, so SQL never blocks async request processing and in-flight work is
  bounded.
- SQLite runs with WAL journalling, `synchronous=FULL`, foreign keys, and a busy timeout.
- Migrations are ordered, embedded in the binary, and transactional per version. A newer
  schema or an unexpected migration history is refused; a failed migration leaves the previous
  version intact and prevents normal startup.

## Documentation

- [`docs/operations.md`](docs/operations.md): build, launch, key handling, data, shutdown, and
  store troubleshooting.
