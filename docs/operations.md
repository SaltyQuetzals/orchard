# Orchard operations

Current scope: the runtime foundation (HTTP server, store, migrations, exact types, and
credential-key handling). Owner initialization, authentication, backup/restore commands, and
provider integrations arrive in later implementation sections. This document covers everything
needed to build, launch, and shut down the current application safely.

## Build

```sh
cargo build --locked --release
```

The toolchain is pinned in `rust-toolchain.toml`; `rusqlite` compiles SQLite from source. Run
the test suite with `cargo test --locked`.

## Credential key

Provider secrets (SimpleFIN, Gmail, Telegram) are encrypted at rest with AES-256-GCM. The
32-byte key is read from a mounted secret file that must contain exactly 64 hexadecimal
characters:

```sh
mkdir -p /srv/orchard/secrets
openssl rand -hex 32 > /srv/orchard/secrets/credentials.key
chmod 600 /srv/orchard/secrets/credentials.key
chown <service-user> /srv/orchard/secrets/credentials.key
```

Requirements and failure behavior:

- The key file stays outside the data directory and outside database/evidence backups. Back it
  up separately; losing it makes encrypted provider credentials unrecoverable.
- Existing installations record a key check on first successful open. Starting with a missing
  key file, a malformed key file, or a different key fails before serving and does not replace
  credentials or initialize a new installation.
- The key is loaded before the store is opened, so a missing key never creates a fresh store.
- Never place the key in arguments where it could be logged; pass the file path only.

The path can be supplied as `--key-file` or `ORCHARD_SECRET_KEY_FILE`.

## First launch (fresh store)

```sh
mkdir -p /srv/orchard/data
cargo run --locked --release -- serve \
  --bind 127.0.0.1:3000 \
  --data-dir /srv/orchard/data \
  --key-file /srv/orchard/secrets/credentials.key
```

On a fresh data directory the application:

1. Creates the data root, `evidence/objects/`, and `evidence/tmp/`.
2. Acquires `orchard.lock` exclusively.
3. Creates `orchard.sqlite3` and applies pending migrations in order.
4. Initializes the credential key check.
5. Starts the HTTP server and logs `Orchard listening on http://...` plus the store path.

Verify readiness with the non-sensitive health endpoint:

```sh
curl -fsS http://127.0.0.1:3000/health
# ok
```

The health endpoint returns `200`, `text/plain; charset=utf-8`, `Cache-Control: no-store`, and
the body `ok`. It exposes no financial data and does not contact providers.

Example from an isolated verification directory:

```sh
work="$(mktemp -d)"
openssl rand -hex 32 > "$work/credentials.key"
chmod 600 "$work/credentials.key"
cargo run --locked -- serve \
  --bind 127.0.0.1:32118 \
  --data-dir "$work/data" \
  --key-file "$work/credentials.key"
```

## Shutdown

Stop with Ctrl-C or `SIGTERM` (`kill -TERM <pid>`). The server stops accepting requests,
finishes in-flight handling, drops the store connection, and releases `orchard.lock`. A second
process can then open the same data directory. Avoid `SIGKILL`: SQLite's WAL recovery is safe,
but shutdown is not graceful and the instance is not drained deliberately.

## Data layout

```text
<data-dir>/
  orchard.sqlite3        authoritative financial database
  orchard.sqlite3-wal    WAL journal while running
  orchard.sqlite3-shm    shared-memory index while running
  orchard.lock           exclusive ownership lock (safe to keep; never copy as data)
  evidence/
    objects/             immutable originals, addressed by content hash
    tmp/                 intake staging; may contain incomplete uploads
```

Durable application state is the database plus referenced evidence objects. The lock file and
incomplete `evidence/tmp/` content are not durable data. Do not copy a live database file
outside SQLite-aware tooling; offline backup/restore commands are added by a later section.

## Single-process model

- One server (or maintenance command) owns a store at a time. `orchard.lock` is held with an
  exclusive OS lock for the process lifetime; a second open fails immediately instead of
  waiting or writing.
- One SQLite connection is shared through a mutex. Reads and transactions run on Tokio's
  blocking pool behind a semaphore (four concurrent operations), so SQL never executes on
  async executor threads and queued work is bounded. Mutations run inside a single transaction
  with no network calls.
- Connections use WAL, `synchronous=FULL`, `foreign_keys=ON`, and a five-second busy timeout.
- Migrations are embedded in the binary, ordered by version, and each runs in one transaction
  with its tracking row. The store records applied versions in `schema_migrations`.
- Startup refuses a schema version newer than the binary supports. Restore the matching
  Orchard version instead of letting an older binary write.
- A failed migration rolls back its partial changes and aborts startup; the previous schema
  version remains and no requests or jobs are admitted.

## Troubleshooting

| Symptom | Meaning and action |
| --- | --- |
| `store lock ... is held by another Orchard process or maintenance command` | Another process owns the data directory. Stop it before starting another; do not delete the lock file while a process runs. |
| `store schema version N is newer than this build supports (M)` | The data was written by a newer Orchard. Run the matching version. |
| `migration N (...) failed: ...` | The upgrade did not apply; startup is refused. Inspect the SQLite error, restore the pre-upgrade backup, and run the matching version. |
| `cannot read credential key file ...` | The mounted key path is wrong or unreadable. Fix the mount; do not create a replacement for an existing store. |
| `credential key file ... must contain exactly 64 hexadecimal characters` | Regenerate the secret as shown above; the file is not a valid 32-byte key. |
| `the mounted credential key does not match this store; refusing to replace credentials` | The wrong key is mounted. Restore the key used for this installation; the store was left unchanged. |
| `cannot prepare store directory ...` | The data directory is not writable by the service user. |

Logs intentionally contain no access URLs, tokens, receipt bodies, or financial payloads.
