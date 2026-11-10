# Spec Delta

## Purpose

Help the owner finish categorization and receipt work through Telegram while keeping one authoritative outstanding-work state shared with the web application.

## ADDED Requirements

### Requirement: Owner-only Telegram pairing
The system SHALL pair one private Telegram chat to the authenticated owner through an expiring, single-use pairing challenge. Only the paired sender and chat SHALL read financial details or perform bot actions. Group chats and unauthorized senders SHALL not gain access by knowing a transaction or callback identifier.

#### Scenario: Unauthorized callback
- **WHEN** an unpaired sender submits a copied category-selection callback
- **THEN** no financial details are returned and no transaction changes

#### Scenario: Reuse pairing challenge
- **WHEN** a pairing challenge has expired or has already established the owner's chat
- **THEN** it cannot establish another pairing

### Requirement: Inclusive chasing start date
The owner SHALL select a local transaction start date before chasing begins. Only dated transactions on or after that date with required missing category or receipt work SHALL be chased. Older history SHALL remain available in registers and reports. Transfers and non-purchases SHALL not acquire receipt requirements merely to support notifications.

#### Scenario: Import older history
- **WHEN** a sync imports purchases before and after the selected start date
- **THEN** only eligible on-or-after purchases enter chasing and all imports remain available for bookkeeping

### Requirement: Discovery and daily follow-up
The system SHALL notify the paired chat when newly eligible unresolved transactions are discovered and send one consolidated follow-up for remaining work each local day at an owner-selected time. Notices SHALL distinguish missing categories from missing receipts and link to the transaction. No reminder SHALL be sent when nothing remains outstanding.

#### Scenario: Two independent missing items
- **WHEN** a newly discovered purchase lacks both category and receipt
- **THEN** the discovery notice identifies both needs and one transaction link rather than treating one resolution as completing both

#### Scenario: Daily outstanding summary
- **WHEN** the daily reminder time arrives with unresolved work
- **THEN** a consolidated summary lists or pages the remaining work without generating a separate daily notification per missing field

### Requirement: Shared resolution across channels
Outstanding work SHALL derive from current ledger allocations and receipt status, not a separate Telegram completion flag. Categorization, evidence attachment, and unavailable-receipt explanations performed through any supported channel SHALL update future reminders. Changing the start date or disabling chasing SHALL not delete financial records.

#### Scenario: Resolve receipt in the web application
- **WHEN** the owner marks a purchase's receipt unavailable with an explanation in Orchard
- **THEN** later reminders stop asking for that receipt while continuing to ask for any missing category

### Requirement: Direct category selection for simple entries
The paired owner SHALL be able to select an active spending category in chat for a simple unsplit outflow. Complex splits and other edits SHALL open the authenticated Orchard transaction page. Stale actions SHALL not overwrite newer financial edits or select archived categories.

#### Scenario: Categorize in chat
- **WHEN** the paired owner chooses groceries for an uncategorized simple purchase
- **THEN** the same ledger entry is categorized, budget totals update, and only unresolved receipt work remains

#### Scenario: Split purchase needs editing
- **WHEN** the owner requests categorization for a split purchase in chat
- **THEN** the bot links to that purchase's full web editor without replacing the split with one category

### Requirement: Explicit Telegram receipt submission
The paired owner SHALL be able to submit an image or PDF against an explicitly identified purchase, such as by replying to its bot message. Unassociated submissions SHALL enter the evidence inbox or request a purchase selection. Unsupported or oversized files SHALL leave receipt work unresolved with a visible explanation.

#### Scenario: Reply with receipt
- **WHEN** the owner replies to a purchase notice with a supported receipt file
- **THEN** the original submitted file is durably stored and linked to that purchase through the shared receipt workflow

### Requirement: Duplicate-safe update handling and visible delivery state
Repeated Telegram updates and repeated application jobs SHALL not duplicate financial mutations, evidence links, or normally acknowledged discovery/daily notices. The owner SHALL see bot disconnection and delivery failures in Orchard. Stopping or re-pairing Telegram SHALL preserve outstanding work and invalidate old chat actions.

#### Scenario: Update redelivered after restart
- **WHEN** a previously committed category action or receipt update is received again
- **THEN** it does not mutate the financial record or attach evidence a second time

#### Scenario: Notifications unavailable
- **WHEN** Telegram rejects notification delivery
- **THEN** unresolved work remains accessible in Orchard and notification status shows the failure rather than claiming the work was completed
