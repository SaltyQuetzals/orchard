# Spec Delta

## Purpose

Explain spending, income, balances, and net worth from the same dated financial records used by the budget, with explicit valuation freshness and coverage limits.

## ADDED Requirements

### Requirement: Spending and income reports
The system SHALL report posted budget activity over an owner-selected local date range, grouped by spending category and category group. Splits SHALL contribute their allocations, categorized refunds SHALL reduce net spending, and uncategorized spending SHALL remain identifiable. Import approval SHALL not change reported financial totals.

#### Scenario: Split purchase and refund
- **WHEN** a $100 purchase is split $60 groceries and $40 household and groceries later receives a $15 refund within the selected range
- **THEN** the report shows $45 groceries and $40 household net spending

### Requirement: Transfers and non-income exclusions
Reports SHALL exclude budget-internal transfers, card repayments, opening balances, and reconciliation adjustments from ordinary income and spending. Budget-to-tracking transfers SHALL show the budget allocation separately from consumption spending and earned income, so investment contributions are not reported as purchases.

#### Scenario: Salary, card payment, and investment contribution
- **WHEN** checking receives $1,000 salary, pays a card $200, and transfers $100 to an investment account
- **THEN** earned income is $1,000, the card payment adds no spending, and the $100 contribution is identified as a budget-boundary transfer rather than consumption

### Requirement: Account balance and net-worth reporting
The system SHALL show dated ledger balance histories for budget accounts and dated valuation histories for investment tracking accounts. Net worth SHALL sum budget assets and signed card liabilities with one valuation per tracking account. Investment transfer records SHALL not be added on top of snapshot valuations.

#### Scenario: Net worth with card debt
- **WHEN** checking holds $1,000, card debt is $200, and the latest investment valuation is $5,000
- **THEN** net worth is $5,800 and Ready to Assign is not increased by the investment valuation

#### Scenario: Contribution already included in valuation
- **WHEN** a $100 checking-to-investment transfer is followed by a provider valuation including that contribution
- **THEN** net worth uses the decreased checking balance and that single investment valuation, without adding the transfer a second time

### Requirement: Freshness and historical coverage
Reports SHALL disclose stale investment values, missing historical coverage, and the as-of time of provider valuations. Historical points SHALL use only valuations known as of that point; later snapshots SHALL not be presented as earlier observed values. Future forecasts SHALL be excluded from actual reports.

#### Scenario: Missing historical investment value
- **WHEN** a report requests a date before an investment account's earliest recorded valuation
- **THEN** the investment component is marked unavailable and the total is labeled incomplete rather than represented as a complete historical net worth

#### Scenario: Stale valuation
- **WHEN** an investment provider has not refreshed successfully for several days
- **THEN** the report retains the last known valuation with its actual date and a freshness warning

### Requirement: Drill-down consistency
The owner SHALL be able to inspect ledger entries behind spending and income report totals and account-specific data behind net-worth totals. Correcting dated ledger data SHALL update the corresponding reports using the same accounting rules as the register and budget.

#### Scenario: Reclassify a reported purchase
- **WHEN** a $25 purchase moves from groceries to household
- **THEN** the report moves $25 between those categories without changing total spending or the account balance

### Requirement: Required signed spending visualization
The system SHALL render release-required ranked horizontal net-spending bars by category/group for the selected local date range, including Uncategorized, with a common scale, explicit zero baseline, and exact signed labels. Charts, supporting tables, totals, and drill-downs SHALL share financial inclusion rules and underlying entries. Tables alone SHALL not satisfy visualization acceptance.

#### Scenario: Visualize split spending and refunds
- **WHEN** a $100 purchase is split $60 groceries and $40 household and groceries receives a $15 refund within the selected range
- **THEN** the bars, exact labels, supporting table, and drill-down totals show $45 groceries and $40 household
- **AND** card payments, openings, adjustments, and future forecasts do not increase the displayed ordinary spending

#### Scenario: Refunds exceed purchases
- **WHEN** a category has $10 of purchases and $15 of categorized refunds in the selected range
- **THEN** its net-spending bar extends to the negative side of the zero baseline and its label, table, and drill-down show -$5
- **AND** the category is not omitted or converted into an unsigned share of a pie chart

#### Scenario: No spending in the selected range
- **WHEN** the selected range contains no qualifying spending or refunds
- **THEN** the report displays an explicit no-activity state rather than fabricated bars

### Requirement: Required observation-aware net-worth visualization
The system SHALL render a release-required dated net-worth chart from signed budget balances and tracking valuations, with a supporting table of totals, components, and valuation as-of dates. It SHALL distinguish observed from carried-forward values and stale from missing coverage, without inventing observations, zero-filling unknown values, or presenting incomplete totals as complete. Tables alone SHALL not satisfy visualization acceptance.

#### Scenario: Visualized net worth agrees with account data
- **WHEN** checking holds $1,000, card debt is $200, and an observed investment valuation is $5,000 at a displayed historical point
- **THEN** the chart and supporting table show $5,800 with the signed account components and the valuation's actual as-of date
- **AND** investment-transfer references are not added on top of the valuation

#### Scenario: Stale and missing investment history remain distinct
- **WHEN** the selected history includes dates before the first investment observation and dates carrying forward an old last-known valuation
- **THEN** missing coverage creates a gap in the complete-total series with an incomplete-history label, not a zero investment value or a backfilled later snapshot
- **AND** last-known values are carried forward stepwise, remain distinguishable from fresh observations, retain their original as-of dates and freshness warnings, and are not smoothed or interpolated into invented investment observations

#### Scenario: No historical observations
- **WHEN** no qualifying balance history or valuation observations exist for the selected range
- **THEN** the report displays an explicit no-history state rather than fabricated points
