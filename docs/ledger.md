# Account ledger

Current scope: the account ledger commands in `src/ledger/`. They are the
single mutation path for accounts, manual entries, splits, linked transfers,
statement reconciliation, import review, and tracking valuations. Server
pages for these workflows arrive in the web-delivery section; this document
describes the financial rules those pages will call.

All amounts are signed integer cents. Construction rejects fractional cents
and overflow. Dates are budget-local `YYYY-MM-DD` values. Every mutation runs
in one SQL transaction, so a rejected command leaves no partial entry.

## Accounts and opening balances

Accounts are either budget accounts (checking, savings, cash, credit card) or
off-budget tracking accounts (investments). A credit-card account automatically
gets a per-card payment category in the `Credit Card Payments` group.

A budget account's opening balance is recorded as an explicit
`opening_balance` entry. It establishes starting assets or debt and is never
income:

```text
Open Visa with opening balance -$300.00
  ledger balance: -$300.00
  recognized income: $0.00
  payment category: created
  automatic payment funding: none
```

For a tracking account, a nonzero opening amount becomes its first manual
valuation instead of an opening register entry; the register holds transfer
references while net worth uses the valuation.

A budget account can close only when its ledger balance is exactly zero.
Closing preserves every historical entry and rejects new ordinary entries.
Tracking accounts may close without a zero ledger balance because their value
comes from dated valuations, not the register.

## Dated manual entries

Manual entries are one of:

| Kind | Sign | Allocations |
| --- | --- | --- |
| `income` | positive | none |
| `outflow` | negative | required (empty means one uncategorized allocation) |
| `refund` | positive | required (empty means one uncategorized allocation) |

A future-dated entry is stored but remains a forecast. `account_balance` with
an `as_of` date and `recognized_income` exclude entries after that date:

```text
Income +$1,000 on 2026-01-10
Income +$500 on 2026-02-10

Balance as of 2026-01-31:       $1,000.00
Recognized income at 2026-01-31: $1,000.00
Recognized income at 2026-02-28: $1,500.00
```

Every entry carries a revision. Edits and deletes require the expected
revision; a stale form receives a conflict instead of overwriting a newer
change. Deleting a transaction linked to an import source also marks that
source deleted, so the provider row cannot recreate the entry.

## Splits and category completion

A split's signed allocations must sum exactly to the parent amount. An
allocation with no category keeps category work outstanding. The register
balance always reflects the whole parent amount, including the uncategorized
part:

```text
Outflow -$100.00 on 2026-01-15
  groceries:    -$60.00
  uncategorized: -$40.00

Account balance: $100.00 lower
Outstanding category work: this transaction
```

Editing the same entry to allocate the remaining $40 to groceries completes
category work without changing the account balance or charging the purchase
again. A split totaling only $90 against a $100 parent is rejected.

Refunds are positive allocations that reverse net category spending. A
refund into groceries does not become income.

Archiving a category prevents new ordinary allocations to it. Existing
allocations remain valid, keep their historical totals, and continue to count
as completed category work. Archived categories are therefore safe to use on
historical edits; new entries using them are rejected.

## Linked transfers

A transfer writes two equal-and-opposite `transfer` entries in one
transaction. Budget-internal transfers, including credit-card payments, have
no allocations and never count as income or spending:

```text
Transfer $80.00 from Checking to Visa
  Checking: -$80.00
  Visa:     +$80.00
  Spending/income: unchanged
```

Editing the transfer updates both legs and their revisions together; deleting
it removes both legs. A transfer leg cannot be edited or deleted through the
ordinary entry commands.

A transfer that crosses the budget/tracking boundary requires an explicit
budget-side target: Ready to Assign or an active category. The tracking leg is
a reference row for the movement, not a valuation. Net worth therefore uses
the tracking account's latest valuation, never the reference amount:

```text
Transfer $200.00 from Checking to Brokerage (Ready to Assign)
Brokerage provider valuation: $5,000.00

Tracking valuation component: $5,000.00
Not $5,200.00
```

Transfers between two tracking accounts are not supported.

## Cleared, reconciled, and statements

Entries are `uncleared`, `cleared`, or `reconciled`. Reconciliation compares
the cleared balance through a statement date with an owner-supplied statement
balance:

```text
Opening balance:        $1,000.00 (cleared)
Outflow:                   -$10.00 (cleared)
Cleared activity:          $990.00
Statement balance:       $1,000.00
Difference:                 +$10.00
```

A nonzero difference is reported. Completing the reconciliation is rejected
while the difference remains, and no balancing entry is created silently.
The owner may record an explicit adjustment:

```text
Adjustment +$10.00, identified as kind `adjustment`
```

Completing the reconciliation then marks cleared entries through the
statement date as reconciled. Editing the amount or date of a reconciled
entry requires explicit confirmation. A confirmed edit clears that entry's
reconciled state and invalidates completed reconciliations for the account
from the affected date onward. Allocation-only edits do not invalidate a
reconciliation because they do not change the account balance.

## Import review and matching

An imported posted row is identified by connection, institution connection,
external account, and external transaction identity. The first ingest creates
a `pending_review` source and posts its ledger entry immediately; approval is
review state only and never posts the money again. Repeated ingests of the
same identity reuse the source.

Only posted activity enters the ledger. Pending provider rows must be
filtered before calling `ingest_source`.

Matching is explicit:

- Candidates require the same account and exact signed amount, within a
  narrow date window.
- Transfer legs and already-linked entries are not suggested.
- Two same-amount manual purchases remain two candidates; neither is selected
  automatically.
- Confirming a match binds the source to the chosen entry and removes the
  duplicate imported entry. The chosen entry keeps its payee, memo, splits,
  category, and evidence.

A deleted or dismissed source keeps its external identity as a tombstone.
Re-ingesting that identity does not create a new entry or notification.

Provider amount or date changes become pending correction proposals; they do
not overwrite the ledger. Applying a correction requires the expected
transaction revision. A single-allocation entry can absorb the new amount
while keeping its category. An entry with multiple allocations requires a
manual split edit first, preserving the owner's splits. Corrections to a
reconciled entry require confirmation and invalidate the affected
reconciliation.

## Tracking valuations

Tracking accounts store dated valuations separately from transfer references.
The latest valuation is the account's contribution to net worth. A tracking
account with no valuation is reported as missing, not as zero. Bank/card
balance snapshots and provider valuation ingestion are added by the
financial-sync section; the ledger command accepts the stored valuation and
its source.

## Verification

`tests/ledger.rs` contains the domain-command smoke scenarios for every
example above, including exact resulting balances:

- account lifecycle, card payment category, and closure rules;
- split allocation sums, forecast exclusion, stale revisions, and archived
  category history;
- linked transfer edit/delete and the valuation-reference exclusion;
- statement mismatch, explicit adjustment, and reconciliation invalidation;
- import approval, ambiguous matching, tombstones, and correction proposals;
- the documented mixed ledger example with exact balances.
