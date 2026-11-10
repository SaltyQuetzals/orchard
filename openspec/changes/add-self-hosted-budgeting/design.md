# Design

## Context

See `proposal.md` for motivation and the eleven capability deltas for observable behavior. Planning and future application work share `/Users/gabrielbritain/Documents/Programming/orchard/`. Inspection found only `openspec/` planning artifacts and `.omp/` workflow configuration, with no source, tests, manifest, deployment configuration, or application documentation. The main capability inventory is also empty. There is no application architecture or persisted data to migrate, and sibling projects are not integration targets.

The owner rejected the earlier TypeScript/React/Fastify suggestion. Required technology direction: a high-performance language with sum/product types and exhaustive matching; server-side templates and modern CSS; minimal JavaScript for interactivity, with no frontend framework. SQLite and ordinary directories are sufficient for this single-user workload. Rust is selected from the owner's acceptable language options; the web stack below is a design decision, not an existing implementation.

External feasibility is bounded. SimpleFIN provides institution-dependent history and daily-oriented updates; a directory listing does not prove access to the owner's accounts. Gmail read-only authorization is broader than the selected receipt label. Telegram holds copies of transmitted financial details. None of these services is made self-hosted by deploying Orchard locally.

## Goals / Non-Goals

**Goals:**
- Make invalid financial states difficult to construct, accounting effects deterministic, and every channel use the same domain commands.
- Keep deployment to one Rust application image, one SQLite file, and a receipt directory on the existing homeserver.
- Complete ordinary workflows with server-rendered pages and native forms; add JavaScript only for interactions that need it.
- Preserve financial and evidence identity across imports, restarts, manual matching, and restores.

**Non-Goals:**
- No SPA, hydration runtime, frontend framework, Node toolchain, ORM, distributed queue, separate database server, or object-storage service.
- No general plugin platform: mail has a small strategy boundary because multiple mail providers must be representable, but only Gmail is implemented.
- No full event-sourcing system or permanently materialized copy of every budget projection. The authoritative ledger and source provenance are sufficient.
- No promise of exactly-once external message delivery, full historical bank data, real-time financial refresh, or offline financial writes. Other product exclusions remain in the proposal.

## Decisions

### 1. Rust, Axum, Askama, and framework-free assets

Use stable Rust, Axum on Tokio for HTTP and background I/O, Askama for compiled HTML templates, `rusqlite` with bundled SQLite, and `reqwest` with verified TLS for provider clients. Commit a pinned Rust toolchain and `Cargo.lock` when implementing. Use native Rust enums, structs, newtypes, `Option`, and `Result`; no JavaScript backend or frontend package build is needed.

Axum handlers load typed view models, call `Template::render`, and return `axum::response::Html`. Autoescaping stays enabled; raw mail markup never becomes an unchecked template string. Native GET/POST forms, validation responses, and POST/Redirect/GET with HTTP 303 provide navigation and mutations. Every stale-write form carries an expected record revision. Server redirects accept only application-local destinations.

Use CSS custom properties, Grid/Flexbox, media/container queries, and semantic HTML. Budget tables can scroll with sticky identifying columns on phones; the transaction editor and outstanding-work list use single-column mobile layouts. Ranked signed spending bars, dated net-worth history charts, and target-progress indicators with accessible supporting tables are required release criteria. Render them as inline SVG through Askama, with labeled scales, exact textual values, descriptive chart titles, and native keyboard-operable filters/drill-down links; no charting framework or JavaScript is required to read or navigate them. Small vanilla ES modules handle conditional split-row controls, upload progress where supported, and PWA registration. Every enhanced form retains a normal server submission path. Do not introduce React, HTMX, Alpine, a component framework, a CSS framework, or an npm bundler.

Rust is preferred over Gleam here for direct native compilation and established SQLite/HTTP libraries. Clojure does not supply the desired compile-time exhaustive algebraic-data-type checks by default. Askama is preferred over runtime templates because template/view-model mismatches fail at compilation. Axum fits typed extractors and Tokio provider clients without a custom web framework. Relevant APIs: [Askama templates and escaping](https://docs.rs/askama/latest/askama/), [Axum HTML responses](https://docs.rs/axum/latest/axum/response/struct.Html.html).

### 2. One binary with explicit domain boundaries

Create a single Cargo application directly in the project root, organized as follows. These are proposed paths, not files found during inspection; do not create an additional nested `orchard/` application directory.

| Area | Planned location | Responsibility |
| --- | --- | --- |
| Domain | `src/domain/` | Exact money, ledger commands, monthly budget projection, schedules, receipt/work states |
| Persistence | `src/db/`, `migrations/` | Prepared SQL, migrations, transactions, source identity, revisions |
| HTTP delivery | `src/web/`, `templates/`, `static/` | Authentication, forms, typed views, CSS, minimal JS/PWA assets |
| Integrations | `src/integrations/simplefin/`, `src/integrations/mail/`, `src/integrations/telegram/` | Provider transport and normalization, not independent accounting logic |
| Worker | `src/jobs/` | Due occurrences, integration polls, delivery outbox, durable leases/status |
| Operator commands | `src/cli/` | Initialization, recovery, backup, restore, explicit job activation |
| Deployment/docs | `Dockerfile`, `compose.yaml`, `docs/` | Homeserver operations and user/provider setup |
| Verification | `tests/` | Consumer-visible accounting, identity, security, and recovery contracts |

The long-running command starts the HTTP server and worker tasks in one process. CLI maintenance commands use the same application lock and refuse to run against a live instance. This avoids separate-worker coordination and makes offline maintenance straightforward. Integration HTTP calls never hold a database lock. After receiving a bounded response, normalize it, then commit each account/message/occurrence and its progress atomically.

### 3. Algebraic domain model and exact persistence

Use an `Amount` newtype around checked signed `i64` cents, distinct typed identifiers, and a budget-local date type. Parse decimal provider strings and form amounts directly into cents; reject fractional cents and overflow rather than rounding binary floating-point values. Use checked wider intermediate arithmetic for sums and target division. Borrow input data and reuse prepared statements instead of routinely cloning financial records or provider payloads.

Represent mutually exclusive states with enums, for example account kind, transaction kind, cleared state, recurrence rule, import conflict, and delivery outcome. Receipt resolution is a derived `Missing | Attached(evidence summary) | Unavailable(nonempty reason)` enum; category completion is a separate value, never a coupled receipt/category status. Exhaustively match internal enums without catch-all branches. Unknown external protocol codes are explicit boundary values and visible errors, not impossible internal variants. Validate SQL rows on reconstruction and enforce matching database CHECK, UNIQUE, and foreign-key constraints.

A single `rusqlite::Connection` is owned behind a mutex and used only inside bounded `spawn_blocking` operations. A semaphore bounds queued DB operations; lock acquisition and synchronous SQL never block Tokio's async executor. Each domain mutation is one SQL transaction with no async wait inside. This is deliberate serialization for trivial load, not a connection pool abstraction. Enable foreign keys, WAL, a busy timeout, and durable commits; page register/report queries. [The connection API](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html) supports cached prepared statements, transactions, and SQLite backups and is `Send`, not a shared concurrent-query object.

Persist these principal records:
- Singleton owner/budget settings, timezone, password hash, session token hashes, CSRF state, and a session-revocation generation.
- Accounts, transaction headers, allocation rows, linked transfer identifiers, reconciliation records, and explicit opening/adjustment kinds. Tracking accounts hold valuations; transfer-reference rows do not become additive investment balances.
- Category groups/categories, monthly assignment/move records, savings targets, and ordered identifiers for stable projection.
- Provider connections with encrypted secret envelopes; external account bindings; raw normalized transaction source records; immutable source links; correction proposals; and dismissed/deleted source tombstones.
- Dated tracking snapshots and provider balance snapshots distinct from register totals.
- Schedule templates and unique occurrence records, including occurrences whose ledger entry was matched or removed.
- Evidence objects, channel/source inbox items, purchase-evidence links, and unavailable-receipt explanations with change history.
- Job leases/progress, processed Telegram update IDs, pairing generations, callback records, and notification outbox rows.

Record revisions support conditional updates from both web and Telegram. A stale expected revision causes a conflict, not last-write-wins replacement. Removing imported data preserves its external identity tombstone; removing a scheduled entry preserves the occurrence identity. No generic audit/telemetry platform is introduced.

### 4. Ledger, import review, and opening anchors

Checking, savings, cash, and credit cards are budget accounts. Signed assets are positive; card debt is negative. Opening amounts establish assets/debt without income-report activity. Reconciliations use cleared activity and explicit statement differences. Editing a reconciled amount/date requires confirmation and invalidates the affected reconciliation. Imported posted transactions affect the ledger immediately; approval is only review state. Source balances are diagnostic snapshots, never repeated ledger inflows.

Splits store a parent amount and signed allocation rows whose sum must equal it. Transfers link two opposite legs and update atomically. Budget-internal transfers have no category spending or income, including card payments. A budget/tracking transfer needs a budget-side allocation, is separately identified in reports, and is a reference on the tracking side; investment snapshots remain authoritative for tracking value. Ordinary tracking-account trade registers are not implemented.

For a newly mapped provider account, select a history start separately from chasing settings. At an initial snapshot time, set the opening anchor to the snapshot balance minus all selected posted activity through that time, place the anchor before the selected history, and add later activity normally. Disclose the supplied range and any unsupported period. Do not claim all historical snapshots or full statement coverage exist. Mapping into a pre-existing manual register does not recompute its opening balance; discrepancies and candidate matches require owner review.

Suggest matches using the same account, exact signed cents, and a narrow displayed date window; require confirmation, even for one candidate. Preserve manual payee overrides, memo, splits, evidence, and schedule identity. A provider row can bind to only one ledger leg. Matching two imported sides to a transfer, or an import to an existing transfer leg, is explicit and atomic. All source amount/date corrections appear as review proposals; none silently replaces reconciled or owner-edited data. Unknown institution account types and non-USD accounts remain unmapped.

### 5. Deterministic monthly envelope projection

Compute budget views from recognized, dated ledger activity and monthly assignments, not provider balances. For today's view, ignore future transaction dates and schedule forecasts. Use a visible system Uncategorized bucket for incomplete spending: its full cash/card effect exists before category selection. It is not treated as income or silently given an owner category.

Process calendar months forward from the earliest affected month. Positive availability carries forward; ordinary category deficits reset at a month boundary. Allocate a month's category funding to cash-like spending before debt-generating card spending, so synchronization arrival order cannot decide whether overspending is cash or credit. Use stable transaction-date/identifier ordering to divide remaining funding among multiple cards. Opening card debt gets no automatic payment reserve. A funded debt-generating purchase reduces spending-category availability and increases that card's payment reserve by the same funded amount. An unfunded portion creates card debt, not assignable cash.

For each category's deficit, distinguish uncovered cash from uncovered card spending. Next month, only the cash component reduces Ready to Assign; the card component is already debt and must not reduce cash again. Historical category/amount/date/assignment edits recompute all affected later months, including payment reserves. Start with indexed queries and pure projections; avoid re-running a full projection for every rendered row. Cache only within a request/command result, not in a second authoritative balance table.

Refunds reverse net spending in their selected category. A debt-reducing card refund reverses associated automatic payment funding that remains reserved; a refund after repayment can instead create a positive card asset and must not manufacture a new reserve or duplicate income. Compute debt-generating portions from the card's signed balance history: purchases use an existing positive balance before creating debt. Payment/overpayment and refund handling must account explicitly for movement into or out of positive card assets. Preserve visible payment-category shortfalls when actual debt payments exceed funding.

Check the current-month conservation identity:

`Ready to Assign + ordinary availability + card-payment availability + uncovered current credit spending + reserved future assignments = recognized budget cash and positive card assets`.

Ordinary availability includes negative and uncategorized amounts; tracking assets and card debt are excluded from the right side. Current cash overspending is therefore visible in negative availability, and its rollover moves that deficit into Ready to Assign exactly once. Reserve money assigned to future months immediately from already recognized resources, but never precredit future income. Historical and future month screens label their as-of/forecast basis. This future-assignment reservation and cash-first funding policy are explicit design assumptions, not claims of exact YNAB parity.

Support two target variants: desired available balance by a month and recurring monthly assigned contribution. Compute suggestions with cent-exact division across remaining contribution months, including the selected current month; distribute any remainder deterministically. Targets never perform assignments. Recurrence uses the budget timezone and original day anchor, clamping short months without permanent drift.

### 6. Provider-specific financial sync, not speculative universal banking

Implement SimpleFIN Bridge directly using typed protocol DTOs and a narrow normalization boundary. Request protocol v2 explicitly, use its connection/account identity, and handle both structured `errlist` and any accompanying legacy provider messages. The published v2 protocol is marked draft; confirm the live service response before enabling a connection rather than silently interpreting an incompatible shape as success. Only posted, non-pending rows enter the ledger; retain posted timestamps and use the budget timezone for their initial ledger date. Owner date overrides remain separate from source timestamps.

Source uniqueness is connection + institution-connection ID + account ID + transaction ID. Reconnection may change these IDs: the owner explicitly remaps accounts and reviews potential historical matches; a new token does not authorize blind reimport into a second register. A completed account batch and its progress marker commit together. A malformed account batch leaves that account's prior state intact while other valid account batches can succeed. Retain source revisions, not just final displayed entries.

Run one scheduled refresh daily at a stable non-peak minute. Initial/history catch-up and manual refresh share a persisted quota gate, stay within the documented 24-requests-per-day expectation, use windows no larger than 90 days, and overlap recent history by about five days. Long gaps become bounded catch-up windows. Show the owner what is queued or quota-limited; do not compensate for failures with uncontrolled requests. General/account/institution errors and provider balance-date remain visible separately from successful fetch time. Investment ingestion records only USD valuations, never holdings or account activity analytics.

Base64 setup tokens contain URLs and the returned access URL contains a secret. Permit only the configured official Bridge HTTPS origins, disable automatic redirects, verify certificates, and reject private/loopback/link-local destinations at connection resolution. Do not fetch institution logos, custom-currency URLs, or provider-provided arbitrary URLs. Sanitize errors and redact credential-bearing URLs from HTTP diagnostics. A failed claim explains prior-use/compromise risk; disconnect removes local secrets and pending authorization work but retains history and explains provider-side revocation.

Sources: [SimpleFIN protocol](https://www.simplefin.org/protocol.html), [Bridge developer limits and overlap guidance](https://beta-bridge.simplefin.org/info/developers). Directory listings in the proposal remain only a discovery lead. Release must demonstrate the owner's required Frost, CIT, Chase, Capital One, Vanguard, Charles Schwab, and Fidelity data; no alternative provider is silently substituted.

### 7. Directory-based immutable evidence

Store original bytes under `/data/evidence/objects/<hash-prefix>/<sha256>` and temporary uploads under `/data/evidence/tmp/`; never use supplied filenames as paths. The SQLite evidence row stores content hash, byte count, media type, source, and original display filename. Distinct channel items can reference one content object; the same inbox item and purchase link remain unique. An owner can explicitly link one evidence item to several purchases.

Stream web and Telegram file intake with a 20,000,000-byte limit and allow JPEG, PNG, WebP, and PDF. Reject uploaded executable/active formats; inspect content signatures rather than trusting extensions. Gmail original-message intake has a separate disclosed 40,000,000-byte decoded-message limit and keeps all MIME parts in the original `.eml`, including parts not eligible for inline preview. Exceeding a limit leaves an actionable collection/upload error, not a truncated successful receipt. These are initial configurable limits, not arbitrary provider promises; Telegram's cloud download ceiling also applies.

Persist by writing a same-filesystem temporary object, syncing it, renaming it into its hash-addressed location, syncing the containing directory, then committing evidence metadata/linkage and channel identity. A crash can leave an unreferenced object, never an acknowledged link without its durable original. Garbage collection is not required for this change; do not delete referenced originals. Backups include referenced objects. An explicit receipt-unavailable explanation is retained separately from links; current display derives attached/missing/unavailable state and category completion independently.

Only authenticated handlers serve evidence. Image/PDF delivery uses restrictive headers; PDFs and arbitrary attachments download rather than execute in application origin. Safe email previews remove active markup and remote-resource URLs, with a restrictive CSP; original EML is downloadable as an attachment. Use a Rust MIME parser and an allowlist sanitizer, not regex HTML cleaning. Telegram photo mode may compress images before the bot receives them; preserve the bytes delivered by Telegram and document sending as a file when the original image is required.

### 8. Small mail strategy boundary with Gmail as the sole implementation

Define one mail strategy interface for label/folder discovery, paginated receipt-message enumeration, and original-message retrieval. Normalize provider mailbox/message IDs and raw message bytes before invoking the evidence service. Gmail-specific OAuth, labels, page tokens, and errors stay in `src/integrations/mail/gmail.rs`. There is no speculative IMAP implementation, dynamic plugin loading, or provider flag on purchase records.

Use authenticated OAuth authorization-code flow with expiring single-use state and the exact configured HTTPS callback. Request `gmail.readonly` and offline refresh authorization. Store refresh/client secrets encrypted and never request write access to mark messages processed. Show the broad read permission before connecting; the chosen label is an application filter, not a provider authorization boundary. Operator documentation must cover OAuth consent configuration, token expiry/revocation, and Google personal-use/verification policy rather than promising that test-mode tokens remain valid indefinitely.

Every fifteen minutes, walk the selected label using `users.messages.list` with `labelIds`, traverse all pages, and fetch each previously unseen message with `users.messages.get?format=raw`. Decode base64url to original EML, preserve body-only messages, and deduplicate by mailbox/message identity. Repeated complete scans deliberately avoid a date-only watermark: labeling an old email later must still discover it. Persist bounded scan progress and per-message failures; an invalidated page token restarts enumeration with identity deduplication. Mark a scan complete only when required pages/messages are accounted for. A label change starts a new scan without removing existing evidence.

Mail receipt matching is manual confirmation in the shared inbox. No amount extraction or automatic category classification is needed. Source deletion or label removal never deletes stored evidence. Disconnect stops collection and removes local credentials, not purchase links.

Sources: [Gmail label-filtered listing](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/list), [message retrieval](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/get), [raw format](https://developers.google.com/workspace/gmail/api/reference/rest/v1/Format), [OAuth policies](https://developers.google.com/identity/protocols/oauth2/policies).

### 9. Shared outstanding-work query and authorized Telegram commands

A domain query derives outstanding category work from required incomplete allocations and receipt work from missing purchase evidence/resolution. Eligibility additionally requires a recognized transaction date on or after the owner-selected inclusive chasing date. Forecasts, openings, adjustments, and transfer-only receipt work are excluded. The same query feeds the web inbox, discovery messages, and daily reminders. Completing one dimension does not imply the other is complete.

Pair the bot using a short-lived random single-use challenge generated in an authenticated web session and redeemed in a private chat. Store both numeric chat and sender IDs and a pairing generation; validate all three before disclosing details or mutating. Invalidate pending callbacks on disconnect/re-pair. Credentials are encrypted. Use Telegram `getUpdates` long polling with only required update types, not public webhooks. A durable processed-update record and associated mutation commit together; only then advance the next requested offset. Persist failed evidence intake as actionable work without marking the purchase complete.

Create opaque, short callback handles backed by transaction ID, expected revision, intended action, and pairing generation. Do not embed category names, secrets, or authoritative financial data in callback payloads. Paginate category choices. For a simple unsplit outflow, invoke the same domain categorization command as the web form. Complex split edits use an authenticated transaction link. A stale revision or archived category returns current information without overwriting. Reply-to bot message IDs map receipt files to explicit transactions; otherwise save to the unlinked evidence inbox and request owner selection.

Discovery notices are grouped by newly eligible transactions; a start-date change can produce a summary of newly eligible existing work rather than a message storm. One daily consolidated follow-up is keyed by pairing generation and local date at the configured local time. Read current work immediately before sending so resolved items are not deliberately reminded again. If downtime spans days, send the current day's remaining-work summary rather than replaying every missed day.

Use a transactional outbox for discovery/daily intents, with states pending, acknowledged, failed, or delivery-uncertain. Normal repeat jobs and committed updates are deduplicated. Telegram does not offer an idempotency key for sends: a crash/timeout after provider acceptance but before local acknowledgement can cause uncertainty. Do not claim exactly-once delivery; show uncertain sends and do not automatically replay an uncertain discovery. The next daily summary uses current work. Bot outages never block accounting. Long outages may exceed Telegram's retained update history; explain resubmission without claiming lost uploads were stored.

Sources: [Telegram update offsets and long polling](https://core.telegram.org/bots/api/methods/getUpdates), [cloud file downloads](https://core.telegram.org/bots/api/methods/getfile), [Bot API](https://core.telegram.org/bots/api).

### 10. Authentication, secret handling, and PWA privacy

Initialize the singleton owner/budget/timezone only with a host-access CLI command. Hash passwords with Argon2id and generate high-entropy server-controlled session tokens, storing only their hashes in SQLite. Production cookies are Secure, HttpOnly, SameSite, and expire after a bounded server-enforced lifetime. Logout deletes the session; operator password recovery changes the password hash and revocation generation without touching financial data or connections. Throttle failed logins and trust forwarded client/origin information only from the configured reverse proxy.

All browser mutations, including multipart uploads and logout, require session-bound CSRF tokens and allowed-origin checks. All financial pages, API endpoints, receipt downloads, and integration settings require authentication; only non-sensitive health/static assets and narrowly scoped OAuth callback handling are public. Bot authentication is exclusively the validated pairing flow, not a browser-session bypass. Typed domain errors map to clear validation/conflict/authentication responses without sensitive diagnostics.

Encrypt provider credentials with AES-256-GCM using random unique nonces and authenticated connection identity. Read the master key from an operator-protected mounted secret outside `/data`; do not generate a replacement when an established installation cannot decrypt credentials. Separate backup custody of the key from financial backups. TLS terminates at the existing public reverse proxy; upstream exposure is restricted to that proxy. Avoid secrets in process arguments, tracing fields, URLs returned to browsers, or logging middleware.

PWA assets are a manifest, icons, and a small service worker. Cache only versioned public CSS/JS/icons and a non-sensitive offline page. Never cache authenticated HTML, API responses, original evidence, or financial JSON; set private no-store responses. Financial navigation and mutations require network access. No background sync, offline mutation queue, IndexedDB financial store, or service-worker receipt cache. Logout clears application-managed private state; session rejection and non-sensitive offline fallback prevent a new visit revealing cached financial pages. Verify actual browser history/logout behavior, not just headers.

### 11. Reports and date-sensitive jobs

Spending/income reports query signed allocations of recognized entries over local date ranges. Refunds reduce net category spending; budget-internal transfers/card payments, openings, and reconciliation adjustments are not ordinary income/spending. Budget/tracking transfers are a separate report classification. Drills use the same transaction selection as totals. Net worth uses signed budget-account ledger balances and the latest investment valuation known at each historical point. No future snapshot is backdated, missing valuation coverage is labeled incomplete, and old values show freshness.

Keep two financial views distinct: the monthly budget supports present decisions, while date-filtered reports explain past spending and financial position. In the budget, exact Ready to Assign and category availability remain primary rather than becoming bounded percentage gauges. Label cash overspending, credit overspending, and card-payment shortfalls separately; color is supplementary, never their only distinction. Target progress uses available balance for a dated-balance target and the selected month's assignments for a monthly-contribution target. Carryover does not count as a new contribution, and spending can reduce balance-target progress without reversing the month's assigned contribution.

Reports show ranked horizontal bars for category/group net spending on a common scale within the selected view, with an explicit zero baseline and negative bars when refunds exceed purchases. Include Uncategorized activity; exclude internal transfers/card payments, openings, adjustments, and future forecasts from ordinary spending/income, and identify budget/tracking boundary transfers separately. Use the same typed query result for each chart, its accessible table, totals, and drill-down selection rather than recomputing accounting in SVG or JavaScript. Exact signed amounts and links remain available without hover.

The net-worth chart shows dated total-history observations from signed budget balances and tracking valuations. Distinguish actual investment observation dates from stepwise carried-forward last-known values, retain their as-of dates and freshness warnings, and never smooth or interpolate investment changes into invented observations. Missing valuation coverage is a gap with an incomplete-total label, not a zero asset or a complete total; later snapshots never fill earlier unknown history. Supporting tables expose the component values, signed liabilities, observation dates, and coverage behind the displayed history.

All three visualization types must render and remain inspectable on phone and desktop with JavaScript disabled. Native date filters and HTML drill-down links remain keyboard operable; SVG has an accessible title/description and the corresponding table supplies exact values and status text without color-only meaning. Empty data produces an explicit no-activity/no-history state, not fabricated bars or points. Financial SVG and tables inherit authenticated, private no-store delivery and are never public static assets or offline caches. Release acceptance exercises these actual surfaces, numerical agreement, negative spending, stale/missing valuations, and distinct target-progress measures; a table-only implementation does not satisfy the visualization requirement.

Persist schedule occurrence uniqueness by template and original local due date. Daily/weekly/monthly/yearly recurrence uses the original anchor, budget timezone, and explicit month-end clamp; materialize due entries once with a transaction, including after downtime. A materialized future-dated manual entry follows the same recognition boundary. Template edits do not rewrite posted occurrences. The worker reads the persisted budget date and next due jobs after startup/timezone changes, rather than assuming a 24-hour day through DST.

Durable job rows hold lease expiry, due time, last attempt/success, and sanitized errors. Jobs cover schedule materialization, daily financial sync/catch-up, mail scans, Telegram polling/outbox, and daily chasing. Reclaim interrupted leases; use unique occurrence/update/outbox identities for correctness, not lease timing alone. Authenticated status pages distinguish web liveness from stalled jobs and provider freshness.

### 12. Homeserver deployment and consistent offline maintenance

Build a multi-stage Docker image containing the release Rust binary, migrations/templates/static assets as needed, and trusted TLS roots. Run as non-root. Compose mounts `/data` for SQLite/evidence and a separate read-only secret file; publish only the application port to the reverse-proxy network. Support the homeserver's architecture through a reproducible Linux build; no dedicated paid host, remote database, Redis, or S3 is required. Non-sensitive health only confirms local readiness; provider errors are not treated as a reason to restart a usable manual budget.

Acquire an exclusive store lock before migration or normal startup. Maintenance commands require the app/worker stopped and the same lock; they cannot accidentally race a live writer. Run ordered transactional SQL migrations before serving, refusing newer incompatible schemas and reporting failures without initializing a replacement store.

Use offline backup deliberately: stop accepting requests, drain in-flight work, stop the one app/worker process, run the backup command with the store lock, then restart. For one owner this short downtime is preferable to a cross-resource online snapshot protocol. SQLite's backup API creates the database copy; enumerate its referenced evidence objects into a protected backup directory and write a manifest with format/schema version, sizes, and SHA-256 checksums. Completed pre-shutdown uploads are included; uncommitted uploads are not acknowledged. Backup staging contains sensitive plaintext financial data and needs mode 0700; document encrypting and copying it off-host, for example to an existing restic repository. Do not build a new encryption/archive service.

Restore into a new inactive data directory, never by silently overwriting the running store. Reject symlinks/path escapes, incompatible schema versions, invalid SQLite integrity/foreign keys, and missing or mismatched evidence before activation. Require the matching separate credential key. Revoke browser sessions and pairing challenges, leave external jobs disabled persistently, and inspect balances and receipts in the isolated instance. Before re-enabling jobs, rebaseline notification intents against current outstanding work instead of replaying stale historical sends. Operator activation is explicit. Retain the old image and pre-migration backup for rollback; do not promise down-migrations for incompatible schema changes.

## Risks / Trade-offs

- **Required institution connectivity is not proven** → Treat owner-authorized live balances/posted activity or investment valuations for all seven institutions as a release gate. Record failures; do not substitute directory listings or manual-entry demonstrations.
- **Financial projection/card edge cases are easy to get wrong** → Use cent-exact pure projections with conservation, cash-versus-credit rollover, multiple-card funding, refunds after repayment, positive balances, and historical-edit tests before connecting automatic imports.
- **SQLite and evidence files cannot commit in one transaction** → Durable file-before-link ordering, immutable content addressing, offline consistent backup, and integrity-checked restore. Orphan files are safer than missing acknowledged originals.
- **Single database connection serializes access and offline backups interrupt service** → Accept for the explicit trivial-load, single-owner scope; bound queries and keep network calls outside DB operations. No speculative scaling infrastructure.
- **SimpleFIN v2 is documented as draft and service quotas/history can change** → Validate live response shape and show unsupported responses/errors; use documented daily quotas and bounded windows, preserving manual operations throughout.
- **Gmail consent is broader than the receipt label; production OAuth setup may need owner action** → Explicit disclosure, read-only scopes, strict label filtering, encrypted credentials, and setup documentation reflecting Google's actual consent policy.
- **Telegram is a third-party cloud channel with finite update retention and ambiguous send outcomes** → Explicit owner pairing, minimal disclosed details, authoritative web work state, durable update identity/outbox status, and no exactly-once claim.
- **Minimal JS requires intentional UX, especially split editing and phone uploads** → Native forms first, small optional modules, keyboard/mobile browser verification, and no frontend framework as an expedient fallback.
- **Lost credential key or untested backup makes recovery incomplete** → Separate protected key recovery instructions and an isolated restore drill covering encrypted integrations and original receipt/email evidence.

## Migration Plan

1. During the separately authorized apply workflow, create the Rust application directly in `/Users/gabrielbritain/Documents/Programming/orchard/`; preserve its existing `openspec/` planning home and `.omp/` workflow configuration, and leave sibling projects untouched. There is no existing app/data migration.
2. Establish exact-money/domain persistence and authentication before exposing financial pages or enabling provider jobs. Migrate a fresh database under exclusive lock and bootstrap the owner/timezone through the host CLI.
3. Validate ledger/budget/receipt behavior with isolated data; deploy on the existing homeserver through Compose and its HTTPS proxy. Keep external jobs paused until credentials, explicit account mappings, Gmail label, Telegram pairing, and chasing start date are configured.
4. Demonstrate live required-institution data and the Gmail/Telegram workflows with the owner; record provider limitations rather than relaxing acceptance. Check mobile/desktop pages, installed PWA privacy/offline behavior, restart persistence, and provider-outage manual operation.
5. Create an encrypted off-host backup and prove isolated restore of balances, original emails/files, links, schedules, and decryptable settings before normal use. Enable jobs explicitly, then observe one scheduled financial refresh and daily follow-up at the configured local time.
6. For later upgrades, stop/drain, create a compatible backup, migrate, and restart. On incompatible upgrade failure, stop the new image and restore the pre-upgrade database/evidence bundle with its matching key before starting the prior image; never run an old binary blindly against a newer schema.
