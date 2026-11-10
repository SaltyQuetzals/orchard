# Orchard operations

Current scope: the runtime foundation (HTTP server, store, migrations, exact types, and
credential-key handling), single-owner access (host initialization, login, server sessions,
password recovery, and browser request protection), and the account ledger (accounts, opening
balances, manual entries, splits, linked transfers, reconciliation, import review, and tracking
valuations). Financial pages, automatic imports, and provider integrations arrive in later
implementation sections. This document covers everything needed to build, initialize, launch,
sign in to, maintain, and shut down the current application safely; ledger rules and worked
examples live in [`ledger.md`](ledger.md).

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

## Owner initialization (host access only)

There is no public registration and no interface for creating a second owner. Initialize the
single owner, USD budget, and budgeting timezone with a maintenance command while the server is
stopped:

```sh
cargo run --locked --release -- init \
  --data-dir /srv/orchard/data \
  --key-file /srv/orchard/secrets/credentials.key \
  --timezone America/Chicago \
  --budget-name "Home"
```

The command prompts twice for a new password without echoing it. Passwords must contain at
least 12 characters. For supervised automation, `--password-stdin` reads the password and its
confirmation as the first two lines of standard input; never put a password in an argument.

The command opens the store under the same exclusive lock as the server, applies pending
migrations, and creates:

- exactly one owner credential record (Argon2id password hash),
- exactly one USD budget with the supplied display name,
- the IANA budgeting timezone.

Running `init` again is refused with a message that the installation is already initialized; it
does not replace the owner, password, budget, or financial data. `--timezone` must be a
recognized IANA name such as `America/Chicago`, `Europe/Berlin`, or `UTC`.

## Sign in and sessions

Start the server, then open `/login` through the public HTTPS reverse proxy:

```sh
cargo run --locked --release -- serve \
  --bind 127.0.0.1:3000 \
  --data-dir /srv/orchard/data \
  --key-file /srv/orchard/secrets/credentials.key \
  --public-url https://budget.example.com \
  --trusted-proxy 172.20.0.2
```

Login and session behavior:

- Passwords are verified with Argon2id. The plaintext password is never stored or logged.
- A successful login creates a server-side session with a 30-day absolute lifetime. The browser
  receives a 256-bit random token; SQLite stores only its SHA-256 hash.
- Session cookies use the `__Host-orchard_session` name with `Secure`, `HttpOnly`,
  `SameSite=Lax`, `Path=/`, and no `Domain`. Without TLS termination in front of the
  application, the owner cannot use these cookies.
- Each session has its own CSRF token. Every state-changing form, including sign-out, must
  submit it, and unsafe requests must come from the configured application origin.
- Unauthenticated and expired sessions cannot read protected pages or run mutations. Navigation
  is redirected to `/login` and returned to a validated local path after sign-in.
- Sign-out deletes the session row and clears the cookie.
- Failed logins are limited per client and installation-wide: five failures per client or
  twenty-five overall within fifteen minutes block further attempts for fifteen minutes. The
  client key is the direct TCP peer unless that peer is a configured trusted proxy.
- Responses default to `Cache-Control: no-store` and carry restrictive browser security
  headers, so financial pages are not cached or framed.

For loopback HTTP verification only, `--insecure-cookies` omits the `Secure` attribute and uses
the unprefixed `orchard_session`/`orchard_login_csrf` cookie names so browsers accept them over
plain HTTP. Never use this flag for the public deployment.

When JavaScript is disabled, the login form and protected pages continue to work.

## Password recovery (host access only)

Recovery uses host access and an offline maintenance command; there is no email reset flow.
Stop the server first (Ctrl-C or `SIGTERM`), then run:

```sh
cargo run --locked --release -- recover \
  --data-dir /srv/orchard/data \
  --key-file /srv/orchard/secrets/credentials.key
```

The command prompts twice for the replacement password. It updates only the owner credential
and revocation state: the budget, financial data, provider connections, encrypted secrets, and
evidence are preserved. It also:

- increments the owner session-revocation generation,
- deletes every stored session, invalidating all browsers immediately,
- clears failed-login throttling so the owner can sign in with the replacement password.

Restart the server and sign in with the new password. The old password fails. Running recovery
against an uninitialized store is refused. `--password-stdin` behaves as for `init`.

## Reverse proxy, origins, and trusted client addresses

Terminate TLS at the existing reverse proxy and publish only the proxy's connection to
Orchard. The application binds loopback by default.

- `--public-url https://budget.example.com` sets the only accepted browser origin for unsafe
  requests. It must start with `https://` or `http://`.
- `--trusted-proxy <IP>` may be repeated. Forwarded client addresses and
  `X-Forwarded-Proto` are trusted only when the direct TCP peer matches one of these addresses.
  The proxy must overwrite `X-Forwarded-For` with the client address and set
  `X-Forwarded-Proto: https`.
- Do not expose the application port directly to clients. A direct client could otherwise
  present its own origin and forwarded headers.
- Browser POSTs must carry an `Origin` (or `Referer`) matching the public origin or the
  request's Host. Cross-site `Sec-Fetch-Site: cross-site` requests are rejected before any
  handler runs.

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

Initialization is a separate host-access step (see above). Until it runs, `/login` reports that
the installation has no owner, and no financial page exists.

Verify readiness with the non-sensitive health endpoint:

```sh
curl -fsS http://127.0.0.1:3000/health
# ok
```

The health endpoint returns `200`, `text/plain; charset=utf-8`, `Cache-Control: no-store`, and
the body `ok`. It exposes no financial data, authentication state, or secrets, and does not
contact providers.

Example from an isolated verification directory:

```sh
work="$(mktemp -d)"
openssl rand -hex 32 > "$work/credentials.key"
chmod 600 "$work/credentials.key"
cargo run --locked --release -- init \
  --data-dir "$work/data" \
  --key-file "$work/credentials.key" \
  --timezone America/Chicago \
  --budget-name "Home"
cargo run --locked --release -- serve \
  --bind 127.0.0.1:32118 \
  --data-dir "$work/data" \
  --key-file "$work/credentials.key" \
  --insecure-cookies
```

## Shutdown and offline maintenance

Stop with Ctrl-C or `SIGTERM` (`kill -TERM <pid>`). The server stops accepting requests,
finishes in-flight handling, drops the store connection, and releases `orchard.lock`. A second
process can then open the same data directory. Avoid `SIGKILL`: SQLite's WAL recovery is safe,
but shutdown is not graceful and the instance is not drained deliberately.

`serve`, `init`, and `recover` all require the exclusive store lock. They cannot run while
another Orchard process owns the data directory, so initialization and recovery are inherently
offline operations. A refused command with
`store lock ... is held by another Orchard process` means the server is still running; stop it
and retry; do not delete the lock file while a process runs.

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
| `this installation is already initialized; refusing to replace its owner` | `init` ran against an existing installation. Do not delete data; use `recover` to change the password. |
| `this installation has no owner; run the initialize command first` | Run the documented `init` command before signing in or running `recover`. |
| `unknown timezone ...` | Use an IANA name such as `America/Chicago` or `UTC`; names come from the host timezone database. |
| `the two passwords do not match` | Rerun the command and enter the same replacement password twice. |
| `the password must contain at least 12 characters` | Choose a longer password. |
| `Too many failed attempts` / HTTP 429 | Wait for the `Retry-After` interval before retrying. Host recovery also clears the throttle. |
| `the request did not pass the browser security checks` | The form was submitted without a valid same-origin request or session CSRF token. Use the application's own forms, and confirm `--public-url` matches the public origin. |
| `store schema version N is newer than this build supports (M)` | The data was written by a newer Orchard. Run the matching version. |
| `migration N (...) failed: ...` | The upgrade did not apply; startup is refused. Inspect the SQLite error, restore the pre-upgrade backup, and run the matching version. |
| `cannot read credential key file ...` | The mounted key path is wrong or unreadable. Fix the mount; do not create a replacement for an existing store. |
| `credential key file ... must contain exactly 64 hexadecimal characters` | Regenerate the secret as shown above; the file is not a valid 32-byte key. |
| `the mounted credential key does not match this store; refusing to replace credentials` | The wrong key is mounted. Restore the key used for this installation; the store was left unchanged. |
| `cannot prepare store directory ...` | The data directory is not writable by the service user. |

Logs intentionally contain no access URLs, tokens, receipt bodies, or financial payloads.
