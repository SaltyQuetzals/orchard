# Spec Delta

## Purpose

Bring daily bank and card activity and investment valuations into Orchard through SimpleFIN while preserving accounting identity, provider limitations, and owner control.

## ADDED Requirements

### Requirement: Secure SimpleFIN connection lifecycle
The system SHALL exchange an owner-supplied SimpleFIN setup token for an encrypted credential without collecting bank login credentials. It SHALL use verified HTTPS, protect claim and access requests from arbitrary-network access, and never disclose credentials in pages or logs. Failed token claims SHALL explain potential prior use and provider-side revocation.

#### Scenario: Previously claimed token
- **WHEN** the provider rejects a token claim as already used or invalid
- **THEN** no connection is marked healthy and the owner is told to disable the possibly compromised token and create a new one

#### Scenario: Unsafe token destination
- **WHEN** a setup token points to an unapproved host, insecure URL, or private-network destination
- **THEN** the system rejects it before making the claim request

### Requirement: Explicit account mapping and currency scope
The owner SHALL select which provider accounts map to checking, savings, credit-card, or off-budget investment accounts. Unmapped and non-USD accounts SHALL not silently enter the budget. Reconnection SHALL require explicit mapping to existing accounts when provider identities change.

#### Scenario: Reconnect an existing bank
- **WHEN** a replacement connection exposes an account with a different provider identifier
- **THEN** the owner can link it to the existing register without creating a second account or blindly duplicating its history

### Requirement: Daily synchronization within provider limits
The system SHALL schedule automatic refresh at least daily while running and connected, show the next attempt, and support owner-triggered refresh within shared provider quotas. Requests SHALL respect provider date-window and rate limits and overlap recent history. Provider outages SHALL not prevent manual ledger or budget changes.

#### Scenario: Overlapping refresh
- **WHEN** the same posted purchase appears in consecutive overlapping refresh windows
- **THEN** it produces one ledger purchase and no duplicate discovery notification

#### Scenario: Provider unavailable
- **WHEN** a scheduled refresh fails
- **THEN** existing data remains available, manual bookkeeping still works, and the failure and last successful refresh are visible

### Requirement: Posted transaction identity and correction handling
Only posted provider activity SHALL enter the financial ledger. The system SHALL distinguish transaction identity by provider connection, account, and transaction identifiers, retain source data, and detect revisions. Pending records SHALL not create budget activity. Owner-edited or reconciled revisions SHALL follow the account-ledger review-conflict behavior.

#### Scenario: Pending becomes posted
- **WHEN** a provider first exposes a pending $20 authorization and later a posted $22 purchase
- **THEN** only the posted $22 enters ledger and budget totals

#### Scenario: Interrupted account import
- **WHEN** processing an account fails before its imported batch commits
- **THEN** its progress marker does not advance past uncommitted transactions and repeating the batch does not duplicate committed entries

### Requirement: Initial balance anchoring and bounded history
Initial account setup SHALL distinguish the provider's as-of balance from imported transactions and compute an explicit opening anchor without double-counting history. The selected import range SHALL be independent of the chasing start date. Missing provider history SHALL be disclosed; reconnecting an existing register SHALL not silently replace its opening balance.

#### Scenario: Initial balance and overlapping history
- **WHEN** an account snapshot is $1,000 and selected posted history through that snapshot nets +$200
- **THEN** its opening anchor is $800 and the resulting ledger through the snapshot totals $1,000
- **AND** fetching that history again does not increase the ledger to $1,200

#### Scenario: History is unavailable
- **WHEN** the owner requests history older than the institution supplies
- **THEN** the available range and limitation are shown without fabricated transactions or a claim that the history is complete

### Requirement: Investment balance snapshots
Investment accounts SHALL store dated USD balance snapshots as off-budget tracking assets, not spendable cash, investment trades, or positions. Older snapshots SHALL not overwrite a newer current valuation. Net worth SHALL use each valuation once and identify its as-of time.

#### Scenario: Investment valuation changes
- **WHEN** a Vanguard valuation changes from $10,000 to $10,300
- **THEN** tracking value and net worth increase by $300 without changing Ready to Assign or recording $300 of budget income

### Requirement: Freshness and provider error visibility
The system SHALL distinguish last attempt, last successful fetch, and each provider balance's as-of time. It SHALL show sanitized general, institution, and account errors, including partial responses, authentication requirements, and quota restrictions. A successful HTTP response with account errors SHALL not mark every account current.

#### Scenario: Partial provider success
- **WHEN** a response contains fresh Chase data and a Fidelity authentication error
- **THEN** Chase updates, Fidelity retains its prior valuation with stale/error status, and the connection screen exposes the required owner action

### Requirement: Disconnect without data loss
Disconnecting SHALL stop future refresh jobs and remove locally stored provider credentials while preserving imported history and evidence. The interface SHALL distinguish local disconnection from revoking authorization at SimpleFIN and explain provider-side revocation.

#### Scenario: Disconnect a provider
- **WHEN** the owner disconnects SimpleFIN
- **THEN** subsequent jobs make no authorized provider request and historical ledger data remains editable and reportable

### Requirement: Required institution acceptance
Release acceptance SHALL include owner-authorized live connectivity checks for Frost Bank, CIT Bank, Chase, Capital One, Vanguard, Charles Schwab, and Fidelity. Bank and card checks SHALL demonstrate posted transactions and balances; investment checks SHALL demonstrate dated valuations. Directory listings alone SHALL not count as proof of connectivity.

#### Scenario: Listed institution fails live access
- **WHEN** a required institution is listed by SimpleFIN but cannot supply the owner's required data
- **THEN** the failed institution and provider limitation are recorded as an unresolved release acceptance blocker, not replaced by a manual-entry claim of success
