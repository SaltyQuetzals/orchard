# Spec Delta

## Purpose

Allocate actual budget money to monthly envelopes while exposing overspending, card-payment funding, and savings progress without inventing spendable cash.

## ADDED Requirements

### Requirement: Monthly categories and allocations
The system SHALL provide ordered category groups and categories with monthly assigned, activity, and available amounts. Assignments SHALL move money between Ready to Assign and a category; category-to-category moves SHALL conserve money. Archiving a category SHALL retain history and prevent new ordinary allocations to it.

#### Scenario: Assign and move cash
- **WHEN** Ready to Assign is $500, the owner assigns $200 to groceries, then moves $50 from groceries to transport
- **THEN** Ready to Assign is $300, groceries has $150 available, and transport has $50 available before activity

#### Scenario: Overassignment
- **WHEN** the owner assigns $120 while only $100 remains unallocated
- **THEN** Ready to Assign is visibly -$20 rather than silently creating money

### Requirement: Recognized resources and uncategorized activity
Only dated budget-account resources SHALL fund the budget. Future income, investment valuations, and borrowed card money SHALL not increase Ready to Assign. Uncategorized spending SHALL remain visibly unresolved and included in current budget activity rather than being omitted from available-money calculations.

#### Scenario: Income and tracking balances
- **WHEN** $1,000 arrives in checking while an investment valuation rises by $300
- **THEN** only the $1,000 can increase assignable money

#### Scenario: Uncategorized cash purchase
- **WHEN** a posted $30 checking purchase has no category
- **THEN** the budget exposes $30 of uncategorized cash spending and includes its cash effect
- **AND** selecting groceries moves the activity to groceries without deducting another $30

### Requirement: Month rollover and overspending
Positive category availability SHALL carry forward. Negative spending-category availability SHALL reset at rollover: uncovered cash spending SHALL reduce the next month's Ready to Assign, while uncovered credit spending SHALL remain card debt without a second cash deduction. Where cash and credit share insufficient category funding, cash spending SHALL have funding priority.

#### Scenario: Positive carryover
- **WHEN** a category ends January with $45 available and receives $20 in February
- **THEN** February has $65 available before February activity

#### Scenario: Cash and credit deficits
- **WHEN** January ends with $20 uncovered cash spending and $30 uncovered card spending
- **THEN** February's Ready to Assign is reduced by $20, not $50
- **AND** the $30 remains unfunded card debt and both category deficits reset

#### Scenario: Mixed-payment funding priority
- **WHEN** a category has $50 of funding, $40 cash spending, and $40 card spending in one month
- **THEN** $40 funds the cash purchase, $10 funds card repayment, and $30 is credit overspending regardless of import arrival order

### Requirement: Credit-card payment categories
Each budget credit card SHALL have a payment category. Funded debt-generating purchases SHALL move funded spending money into that card's payment availability. Existing and unfunded debt SHALL require explicit payment-category assignment. Payments SHALL consume reserved money without recording spending again. Negative payment availability SHALL carry forward until covered, rather than resetting as ordinary category overspending.

#### Scenario: Funded purchase and payment
- **WHEN** groceries has $100 available, a card with no positive balance buys $60 of groceries, and checking later pays that card $60
- **THEN** the purchase leaves $40 in groceries and reserves $60 for payment
- **AND** the payment consumes the $60 reserve without adding another $60 of grocery spending

#### Scenario: Pay more than reserved
- **WHEN** a $100 payment is made with only $70 reserved
- **THEN** the payment category exposes a $30 shortfall requiring owner action rather than silently funding it from another category

### Requirement: Refunds and positive card balances
Categorized refunds SHALL reverse net category spending and the associated card-payment funding without creating duplicate income or cash. Card purchases covered by a positive card balance SHALL consume that asset before generating debt-payment reserves. Payment availability changes SHALL remain visible when a refund arrives after repayment.

#### Scenario: Refund before repayment
- **WHEN** a fully funded $60 grocery card purchase receives a $20 categorized refund before payment
- **THEN** groceries regains $20, card debt decreases by $20, and its reserved payment money decreases from $60 to $40

#### Scenario: Spend a positive card balance
- **WHEN** a card has a positive $25 balance and makes a funded $40 purchase
- **THEN** the positive balance covers $25 and only the remaining $15 can generate debt-payment funding

### Requirement: Recomputable historical budget effects
Editing a prior transaction, split, transfer, or assignment SHALL update affected monthly balances and later carryovers consistently. The same financial history SHALL produce the same totals regardless of synchronization order. A changed prior month SHALL expose its downstream effect on current availability.

#### Scenario: Correct prior cash spending
- **WHEN** a January cash purchase is corrected upward by $10 after January closed
- **THEN** January activity and any resulting February cash-shortfall deduction are recomputed without duplicate deductions

### Requirement: Savings targets without implicit assignment
The system SHALL support a target category balance by a selected month and a recurring monthly contribution target. Targets SHALL display remaining need, monthly suggested contribution, and progress without moving money automatically. Target changes SHALL not rewrite actual assignments or spending.

#### Scenario: Dated savings goal
- **WHEN** a category holds $100 toward a $400 target with three contribution months remaining
- **THEN** the suggested contribution is $100 per remaining month and no money is assigned until the owner acts

#### Scenario: Monthly contribution progress
- **WHEN** the owner assigns $30 toward a $50 monthly contribution target
- **THEN** the target displays $20 still needed even if older positive carryover exists

### Requirement: Required target-progress visualizations
The budget SHALL render release-required target-progress graphics and supporting tables with exact amounts and labeled periods. Dated-balance progress SHALL measure available category balance; monthly-contribution progress SHALL measure the selected month's assignments. Carryover SHALL not count as new contributions, and spending SHALL not reverse assigned-contribution progress.

#### Scenario: Carryover and spending do not inflate or erase contributions
- **WHEN** a category carries $100 forward, receives a $30 assignment toward a $50 monthly contribution target, and then incurs $20 of spending
- **THEN** its contribution indicator and supporting table show $30 assigned of $50 and $20 still needed, while its available balance is $110
- **AND** neither the carryover nor the spending changes the month's $30 assigned-contribution progress

#### Scenario: Spending changes balance-target progress
- **WHEN** a category holds $100 toward a $400 dated-balance target and incurs $20 of spending
- **THEN** its balance-target indicator and supporting table show $80 available toward $400 and $320 remaining
- **AND** the display does not confuse that available balance with this month's assigned contribution or perform an automatic assignment

### Requirement: Exact and distinct budget decision indicators
The monthly budget SHALL keep exact Ready to Assign and category availability primary, including negative amounts, rather than replace them with percentage gauges. Cash overspending, credit overspending, and card-payment shortfalls SHALL have distinct text labels and exact amounts, not color-only meanings. These decision indicators SHALL be required for release.

#### Scenario: Different deficits need different actions
- **WHEN** the budget contains cash overspending, credit overspending, and a card-payment funding shortfall
- **THEN** the owner can distinguish all three by their exact amounts and text labels without relying on color
- **AND** the display preserves their different rollover and payment-funding consequences rather than collapsing them into one generic progress score
- **AND** target-progress graphics do not obscure deficits or turn investment valuations or future income into spendable resources
