# Spec Delta

## Purpose

Maintain an exact, reviewable account ledger that supports manual bookkeeping and imported financial activity without duplicating money or losing owner edits.

## ADDED Requirements

### Requirement: Account lifecycle and opening balances
The system SHALL support USD checking, savings, cash, credit-card, and off-budget investment tracking accounts. Opening balances SHALL establish starting assets or debt without being reported as earned income. Closing an account SHALL preserve its history and require a zero ledger balance for budget accounts.

#### Scenario: Open a credit card with existing debt
- **WHEN** the owner opens a credit-card account owing $300
- **THEN** its signed ledger balance is -$300 and it has a payment category
- **AND** the opening debt creates neither spendable income nor automatically funded payment money

#### Scenario: Close an account with activity
- **WHEN** the owner closes a zero-balance checking account
- **THEN** historical transactions remain accessible and new ordinary entries into that account are rejected

### Requirement: Exact and dated ledger entries
The system SHALL record signed USD amounts exactly to cents, account, local transaction date, payee, memo, and provenance. The owner SHALL be able to create, edit, and delete manual entries. Invalid precision SHALL be rejected. Future entries SHALL remain forecasts until their date, without affecting current balances or assignable cash.

#### Scenario: Decimal precision and invalid input
- **WHEN** the owner records $0.10 and $0.20 inflows and attempts a $0.001 inflow
- **THEN** the first two total exactly $0.30 and the third is rejected without a partial entry

#### Scenario: Future income
- **WHEN** the owner records income dated next month
- **THEN** it appears as upcoming activity and does not increase today's ledger balance or Ready to Assign

### Requirement: Splits and categorization
The system SHALL allow categorized income, refunds, and outflows, including split entries whose signed allocations equal the parent amount. Every spending allocation SHALL require an explicit category for completion; archived categories SHALL remain valid on existing allocations. Uncategorized activity SHALL remain visible and affect financial totals before import approval.

#### Scenario: Incomplete split
- **WHEN** a $100 outflow has $60 assigned to groceries and $40 left uncategorized
- **THEN** the register balance reflects the entire $100 and category work remains outstanding
- **AND** a split whose allocations total only $90 is rejected

#### Scenario: Categorized refund
- **WHEN** the owner records a $15 refund into a spending category
- **THEN** it reduces that category's net spending rather than being counted as earned income

#### Scenario: Archive a previously used category
- **WHEN** the owner archives a category already assigned to historical purchases
- **THEN** those purchases remain categorized and their historical totals are preserved
- **AND** the archived category is unavailable for new ordinary allocations

### Requirement: Linked transfers
The system SHALL represent transfers as linked equal-and-opposite account entries changed atomically. Transfers between budget accounts, including credit-card payments, SHALL not be income or spending. Crossing the budget/tracking boundary SHALL require a budget-side category or Ready to Assign selection without creating a second tracking valuation.

#### Scenario: Edit a card payment
- **WHEN** the owner changes a checking-to-card payment from $80 to $90
- **THEN** both transfer legs change to $90 together and neither spending nor income increases

#### Scenario: Fund an investment account
- **WHEN** $200 moves from checking to an off-budget investment account
- **THEN** checking decreases by $200 with a budget-side allocation
- **AND** the transfer is retained for reference without adding $200 on top of a provider valuation already containing it

### Requirement: Cleared and reconciled states
The system SHALL distinguish uncleared, cleared, and reconciled entries. Reconciliation SHALL compare cleared ledger activity with an owner-supplied statement balance. A mismatch SHALL require correction or an explicit adjustment; changes to reconciled amounts or dates SHALL require confirmation and invalidate the affected reconciliation.

#### Scenario: Statement mismatch
- **WHEN** cleared activity totals $990 against a $1,000 statement balance
- **THEN** reconciliation reports a $10 difference and does not silently alter transactions
- **AND** an owner-confirmed adjustment is separately identified in the ledger

### Requirement: Import approval independent of accounting
The system SHALL identify imported entries awaiting owner review while including each posted import exactly once in the ledger, budget activity, and outstanding work. Approving an import SHALL not post the money again. Provider balance snapshots SHALL remain distinguishable from calculated ledger balances.

#### Scenario: Approve an imported purchase
- **WHEN** a posted $25 purchase is imported and later approved
- **THEN** the account decreases by $25 only once and approval alone does not resolve missing category or receipt work

### Requirement: Explicit duplicate matching
The system SHALL suggest manual/import matches only within the same account and exact signed amount and SHALL require owner confirmation before merging. Matching SHALL retain the manual category, splits, memo, and evidence while recording provider identity. Ambiguous candidates and transfer legs SHALL remain reviewable without automatic merging.

#### Scenario: Same-amount purchases
- **WHEN** two manual $12 purchases are candidates for one imported $12 purchase
- **THEN** neither candidate is silently selected
- **AND** confirming one match leaves one financial entry for that purchase with its existing evidence and allocations

### Requirement: Import corrections and deletion protection
The system SHALL retain source identity when imported entries are matched, dismissed, or deleted, preventing unchanged reimports from recreating them. Provider changes affecting an owner-edited or reconciled entry SHALL become visible review conflicts rather than silently overwriting owner work.

#### Scenario: Deleted imported transaction returns
- **WHEN** the owner removes an imported transaction and the next synchronization contains the same provider identity
- **THEN** it is not silently recreated or notified as a new purchase

#### Scenario: Provider revises a matched amount
- **WHEN** the provider revises a previously matched and categorized purchase amount
- **THEN** the revision is shown for review without destroying its splits, receipt links, or reconciliation history
