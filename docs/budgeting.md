# Envelope budgeting

Current scope: the budget commands and monthly projection in `src/budget/`.
They cover ordered category groups/categories, monthly assignments and moves,
cash and credit overspending, per-card payment categories, and savings
targets. Server pages for these workflows arrive in the web-delivery section;
this document describes the financial rules those pages will call.

All amounts are signed integer cents. Every monthly figure is derived by
replaying dated ledger activity and the stored budget records forward from the
earliest affected month. No projected balance is stored as a second source of
truth, so a corrected historical transaction or assignment recomputes later
months from scratch. Every mutation runs in one SQL transaction.

## Categories, assignments, and moves

Categories belong to ordered groups. Archiving a category keeps its history:
existing allocations still count, outstanding-work queries still see an
uncategorized purchase, and new ordinary allocations or new budget money for
that category are rejected.

A month holds at most one assignment per category:

- a positive assignment moves money from Ready to Assign into the category;
- a negative assignment returns money to Ready to Assign;
- a category-to-category move conserves money and never touches Ready to
  Assign.

Assigning more than remains unallocated is allowed. Ready to Assign becomes
visibly negative rather than silently creating money.

```text
Checking opening balance:            $500.00

January assign groceries:            $200.00
  Ready to Assign                    $300.00
  groceries available                $200.00
  transport available                  $0.00

January move groceries to transport:  $50.00
  Ready to Assign                    $300.00
  groceries available                $150.00
  transport available                 $50.00
```

Assigning another `$320.00` to transport with only `$300.00` left:

```text
Ready to Assign:                      -$20.00
groceries available:                 $150.00
transport available:                 $370.00
ordinary availability total:         $520.00
Ready to Assign + availability:      $500.00 = checking balance
```

## Recognized resources

Only dated budget-account resources fund the budget:

- income entries dated on or before the recognition date;
- cash-account opening balances (signed);
- positive credit-card opening balances;
- budget/tracking transfers into a Ready to Assign target.

Future-dated income, investment valuations, and borrowed card money never
increase Ready to Assign. Tracking accounts contribute only to net worth, not
to spendable budget money.

Assigning money in a later month reserves it immediately from already
recognized resources:

```text
Checking opening balance:            $1,000.00
February assignment to groceries:      $300.00
January Ready to Assign:               $700.00
January reserved future assignments:   $300.00
```

A future assignment does not have to be covered by future income. If the owner
over-reserves, Ready to Assign is negative until money arrives.

## Uncategorized activity

A posted outflow or refund with no selected category keeps one uncategorized
allocation. The system Uncategorized bucket is visible, carries no assignment,
and participates in every availability and conservation calculation. Its cash
and card effects exist before the owner categorizes the purchase:

```text
Posted $30.00 checking purchase, no category:
  Uncategorized activity              -$30.00
  Uncategorized available             -$30.00
  Uncategorized cash overspending      $30.00

After selecting groceries:
  Uncategorized activity                $0.00
  groceries activity                  -$30.00
  checking balance                    $970.00 (unchanged)
  Ready to Assign                     unchanged
```

Categorizing moves existing activity; it never posts the purchase again.

## Monthly rollover and deficits

Positive category availability carries into the next month. Negative ordinary
availability resets at the month boundary, but the two components roll over
differently:

- **cash overspending** reduces the next month's Ready to Assign exactly once;
- **credit overspending** becomes card debt and must not reduce cash a second
  time.

Funding is allocated cash first. Within a category-month, assigned money plus
positive carryover first covers cash-like spending, then card spending. Card
purchases are funded in account order (then date and transaction order), so
synchronization arrival order cannot decide whether overspending was cash or
credit.

```text
January: no funding
  cash spending                        $20.00
  card spending                        $30.00
  groceries available                 -$50.00
  cash overspending                    $20.00
  credit overspending                  $30.00

February:
  Ready to Assign reduced by           $20.00 (not $50.00)
  groceries availability                $0.00 (reset)
  card debt remains                    $30.00
```

Mixed payment priority:

```text
January funding (carryover + assigned): $50.00
  cash spending                        $40.00  -> fully funded
  card spending                        $40.00  -> $10.00 funded
  groceries available                  -$30.00
  cash overspending                     $0.00
  credit overspending                  $30.00
  card payment reserved                $10.00
```

Changing the order in which the two purchases are imported changes neither
row.

## Credit-card payment categories

Each budget credit card gets one payment category. Money moves into that
category in three ways:

1. explicit assignments (used to fund pre-existing or unfunded debt);
2. automatic reserve from funded debt-generating purchases;
3. refund reversals returning reserve to the spending category.

Payments consume reserved money without recording spending again. A payment
larger than the reserve leaves a negative payment availability, shown as a
**payment shortfall**. Unlike ordinary category deficits, payment availability
does not reset: the shortfall carries forward until the owner covers it.

```text
January: groceries assigned          $100.00
         card purchases groceries      $60.00
  groceries available                 $40.00
  card payment reserved               $60.00
  Ready to Assign                    $400.00
  conservation: $400 + $40 + $60 = $500 checking balance

January payment from checking         $60.00
  card payment available               $0.00
  groceries available                 $40.00
  checking balance                   $440.00
  conservation: $400 + $40 = $440
```

Paying more than reserved:

```text
Pre-existing card debt:              $100.00
January payment-category assignment:  $70.00
January checking payment:            $100.00
  card payment available              -$30.00
  payment shortfall                   $30.00
  card balance                         $0.00

February: payment available           -$30.00 (carried, still visible)
```

The grocery purchase is counted once. The payment only consumes the reserve.

## Card refunds and positive balances

A categorized card refund increases the spending category's availability and
reverses automatic payment funding that remains reserved:

```text
Fully funded $60.00 card grocery purchase
$20.00 refund before repayment:
  groceries available                 $60.00
  card payment reserved               $40.00
  card debt                           $40.00
```

A refund after the card was already repaid has no reserve left to reverse. It
creates a positive card asset instead and must not invent income or a new
reserve:

```text
January: $60.00 purchase, then $60.00 payment
February: $20.00 refund
  groceries activity                  +$20.00
  card payment available                $0.00
  card balance                        +$20.00
  Ready to Assign                     unchanged
```

A positive card balance is budget money. Purchases consume it before creating
debt, so only the debt-generating remainder can receive payment funding:

```text
Card opening balance:                 +$25.00
Groceries assigned:                   $40.00
Card purchase:                        $40.00
  groceries available                  $0.00
  card payment reserved               $15.00
  card balance                       -$15.00
  conservation: $485 + $15 = $500 checking balance
```

Overpaying a card creates a positive card asset and a matching shortfall. The
two cancel in the conservation identity:

```text
$70.00 funded purchase, then $100.00 payment
  card balance                       +$30.00
  card payment available              -$30.00
  payment shortfall                   $30.00
  Ready to Assign                    $430.00
  conservation: $430 - $30 = $400 checking balance
```

## Historical corrections

Because projections are recomputed, editing a prior amount, date, category, or
assignment updates every affected month once:

```text
January income:                      $100.00
January groceries assignment:         $30.00
January groceries cash purchase:      $40.00
  January cash overspending           $10.00
  February Ready to Assign            $60.00

Correct the purchase to $50.00:
  January cash overspending           $20.00
  February Ready to Assign            $50.00
  checking balance                    $50.00
```

The deduction is not repeated. Multiple cards divide remaining funding
deterministically: cash spending first, then cards by account order. A seeded
multi-month history with both cash and card spending still satisfies the
conservation check in every month.

## Current-month conservation identity

For the current month, the projection exposes every component of the design
identity:

```text
Ready to Assign
+ ordinary availability (negative and Uncategorized included)
+ card-payment availability (negative shortfalls included)
+ uncovered current credit spending
+ reserved future assignments
= recognized budget cash + positive card assets
```

`BudgetConservation::accounted()` and `recognized()` must return the same
cents; `is_balanced()` reports it. Positive card assets are netted against a
matching negative payment availability so an overpayment is never counted
twice. Card debt is excluded from recognized money, and tracking assets are
excluded entirely.

## Savings targets

Two target variants are supported; neither assigns money automatically:

- **dated balance**: reach a target amount by a selected month;
- **monthly contribution**: assign a target amount in each month.

Progress uses different measures on purpose:

| Target | Progress measure | Suggested contribution |
| --- | --- | --- |
| Dated balance | available category balance | remaining need divided across the remaining months, rounded up to the cent |
| Monthly contribution | this month's net assignments | target minus this month's assignments |

```text
Dated target: $400.00 by March, January available $100.00
  remaining                          $300.00
  months remaining (Jan, Feb, Mar)        3
  suggested contribution             $100.00

$100.00 target with three months and $0.00 available
  suggested contribution              $33.34 (deterministic cent remainder)

Monthly contribution target: $50.00
  December carryover                 $100.00
  January assignment                  $30.00
  January spending                    $20.00
  January available                  $110.00
  contribution progress         $30.00 of $50.00
  still needed                        $20.00
```

Carryover is not a new contribution and spending does not reverse the month's
assigned-contribution progress. Changing a target never rewrites assignments.

## Deficits and their different actions

The budget exposes three deficits with separate text labels and exact amounts.
They are never collapsed into one score, and color is supplementary:

| Label | Exact amount | Rollover consequence | Owner action |
| --- | --- | --- | --- |
| Cash overspending | uncovered cash spending | next month's Ready to Assign is reduced once | assign money or correct the purchase |
| Credit overspending | unfunded card spending | becomes card debt; no second cash deduction | fund the card payment category if the debt should be repaid |
| Payment shortfall | negative card-payment availability | carries forward until covered | assign money to that card's payment category |

## Differences from exact YNAB parity

These are explicit design choices, not claims of exact parity:

- Assignments to future months reserve money from today's recognized
  resources immediately; future income is never precredited.
- Cash-first funding within a category-month is deterministic by calendar
  dates and account order, not by import arrival order.
- Card payment availability keeps negative shortfalls visible across month
  boundaries instead of resetting them as ordinary spending deficits do.
- The system Uncategorized bucket participates in overspending and rollover
  accounting while remaining visibly unresolved.
- Savings targets only report progress and a cent-exact suggestion; they never
  move money.
- Projections are recomputed from the ledger on request. There is no
  materialized budget-balance table and no background projection job.
