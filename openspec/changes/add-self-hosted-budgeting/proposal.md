# Proposal

## Why

Build Orchard as a self-hosted, single-user alternative to hosted budgeting software, preserving a familiar YNAB-inspired envelope-budgeting experience while reducing recurring costs and tailoring bookkeeping to the owner's life. Combine manual budgeting, automatic US financial-account synchronization, and receipt collection through the web, email, and Telegram so the owner controls their application data and can keep purchases evidenced and categorized.

## What Changes

- Introduce an authenticated, server-rendered, responsive web application for one owner and one USD budget, installable as an online-first PWA. Deploy with persistent local storage using Docker Compose on the owner's existing homeserver behind a public HTTPS reverse proxy. No public registration or household sharing.
- Provide checking, savings, cash, and credit-card registers with opening balances, split transactions, transfers, cleared/reconciled states, import review, and duplicate-safe manual/import matching.
- Implement monthly category groups, money assignment and movement, Ready to Assign, positive carryover, explicit cash overspending, credit overspending becoming debt, and credit-card payment categories. Keep exact availability, cash/credit deficits, and card-payment shortfalls primary and visibly distinct in the budget decision surface.
- Add savings targets, scheduled recurring transactions, category spending reports, and account-balance/net-worth reports. Ranked signed category/group spending bars, dated net-worth history charts, and distinct balance-target versus monthly-contribution progress indicators are required release acceptance criteria, not optional decoration. Require matching accessible tables, exact values and drill-downs without hover or color alone, and responsive keyboard-operable rendering with JavaScript disabled.
- Synchronize bank/card balances and posted transactions through SimpleFIN Bridge; synchronize investment account balance snapshots as off-budget tracking assets. Required institutions: Frost Bank, CIT Bank, Chase, Capital One, Vanguard, Charles Schwab, and Fidelity.
- Make sync status, provider errors, freshness, and reconnect/disconnect visible; keep manual budgeting usable during provider outages. Daily automatic refresh is the commitment, not real-time data.
- Include owner bootstrap/recovery, encrypted provider credentials, database migrations, backup/restore covering both the database and receipt evidence, and deployment/operation documentation.
- Attach receipt evidence to every purchase through web/PWA uploads, Telegram, or email. Keep categorization and receipt completion independent; allow an explicit "receipt unavailable" resolution with a retained explanation, visibly distinct from attached evidence, that stops receipt chasing without resolving an outstanding category.
- Collect receipt messages from a designated Gmail label through a provider-agnostic mail connector using the strategy pattern. Preserve original emails, including body-only receipts and attachments, as evidence rather than requiring a PDF; link that evidence to purchases through the shared receipt workflow.
- Chase missing categories and receipts through Telegram starting from an owner-selected transaction date. Notify when new unresolved transactions are discovered and send a daily consolidated follow-up for outstanding work; older imported history does not trigger chasing. Let the owner select categories directly in chat and submit receipts, with links to the full transaction in Orchard for splits and more involved edits. Resolution through any channel updates the same outstanding work.

Scope boundaries: no CSV import, YNAB data migration, native mobile app, offline transaction entry or queued receipt uploads, multiple users/budgets/currencies, automatic transaction classification, direct bank credential collection, transfers initiated at banks, investment trading, holdings/investment-activity/tax-lot analytics, or exact YNAB feature/UI parity. Categorization remains manual, including category selection in Telegram. Investment balance snapshots contribute to net worth, never spendable budget cash. Scheduled future income is not assignable before its transaction date. The chasing start date controls notifications, not the availability of imported history.

## Capabilities

### New Capabilities

- `owner-access`: Single-owner initialization, authentication, session protection, and operator recovery.
- `account-ledger`: Account lifecycle, exact-money transaction registers, splits, transfers, reconciliation, and import approval/matching.
- `envelope-budgeting`: Monthly category allocation, money conservation, carryover/overspending, credit-card payment budgeting, and savings targets.
- `financial-sync`: Secure SimpleFIN connection management, institution/account mapping, daily idempotent bank/card imports, investment balance snapshots, and freshness/error visibility.
- `scheduled-transactions`: Calendar-based recurring entries, upcoming previews, and duplicate-safe due-date materialization.
- `financial-reporting`: Spending/income and net-worth reporting with required signed spending bars and dated net-worth charts, consistent drill-downs, and explicit stale/missing valuation coverage, using budget ledger activity and tracking valuations without double-counting transfers.
- `receipt-evidence`: Purchase-linked receipt uploads, original email evidence, independent receipt completion, and explicit unavailable-receipt explanations.
- `mail-receipts`: Provider-agnostic receipt collection through mail strategies, initially reading a designated Gmail label and preserving message bodies and attachments.
- `transaction-chasing`: Telegram discovery notifications and daily follow-ups for missing categories and receipts from a selected start date, direct category selection and receipt submission, and links to complex web edits.
- `web-delivery`: Authenticated server-rendered pages, required responsive and accessible financial visualizations with supporting tables and no-JavaScript operation, receipt capture and budgeting interactions, and online-first PWA installation.
- `self-hosted-operations`: Homeserver container deployment behind a public HTTPS reverse proxy, persistent database and receipt storage, schema migration, backup/restore, and operational guidance.

### Modified Capabilities

None. The capability inventory is empty and the project root has no application implementation.

## Impact

This is a greenfield application in `/Users/gabrielbritain/Documents/Programming/orchard/`. Inspection found only the existing `openspec/` planning root and `.omp/` workflow configuration: there is no source, test suite, package manifest, deployment configuration, or application documentation to reuse. Planning and future application work share this project root; sibling projects are unrelated and must not be modified. New server-rendered application/UI, database, background workers, receipt persistence, mail strategies, Telegram integration, containers, tests, and operational documentation will be needed; no existing APIs or persisted application data are being replaced.

Owner-approved technology constraints: a high-performance language with algebraic data types and exhaustive pattern matching; server-side templates and modern CSS; minimal JavaScript only for interactivity; no React or frontend frameworks; SQLite and directory-based evidence storage. The selected design uses Rust, Axum, and Askama with a database-backed worker in one deployable application image. These are proposed additions, not existing repository conventions. Automatic synchronization depends on an externally operated SimpleFIN Bridge subscription and the owner's institution authorization; self-hosting does not make the bank-data provider self-hosted. Reuse the existing homeserver rather than require a separate hosting subscription; operating costs also include backups and receipt storage.

Gmail and Telegram introduce additional provider authorization and data-handling boundaries. Gmail supports [label-filtered message retrieval](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/list), but its [read-only OAuth scope](https://developers.google.com/workspace/gmail/api/auth/scopes) grants mailbox-wide read access rather than label-restricted permission. Transaction details and receipts sent through Telegram also reside in its [cloud-chat infrastructure](https://telegram.org/privacy#3-3-1-cloud-chats); Orchard remains the authoritative application store, not the only service that processes those copies.

The provider's public [institution search](https://beta-bridge.simplefin.org/search-institutions) listed all seven requested institutions during planning, including Frost National Bank, CIT Bank, Chase Bank, Capital One, Vanguard, Charles Schwab US, and Fidelity Investments. Listings are not proof of successful owner-account connectivity. Live balance/transaction checks for every requested institution are release acceptance, not an optional replacement with manual entry. The [developer guide](https://beta-bridge.simplefin.org/info/developers) documents daily-oriented updates and request/history limits. The [subscription page](https://beta-bridge.simplefin.org/) lists $1.50/month or $15/year plus tax at planning time; pricing and availability can change.
