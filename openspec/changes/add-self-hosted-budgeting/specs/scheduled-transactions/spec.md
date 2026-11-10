# Spec Delta

## Purpose

Preview and materialize recurring financial entries on budget-local dates without recognizing future money or duplicating entries after downtime and import matching.

## ADDED Requirements

### Requirement: Owner-managed recurring templates
The system SHALL allow the owner to create, edit, pause, resume, and end recurring income, outflow, split, and linked-transfer templates. Templates SHALL include account, amount, payee, allocation, first date, and daily, weekly, monthly, or yearly interval. Invalid splits and invalid account/category references SHALL be rejected.

#### Scenario: Pause a recurring bill
- **WHEN** the owner pauses a monthly bill before its next due date
- **THEN** no later occurrence is materialized while paused and prior posted occurrences remain unchanged

### Requirement: Calendar-based recurrence
Recurrence SHALL use the budget timezone and the template's original calendar anchor. A missing day in a shorter month SHALL use that month's final day without shifting later anchors. Each occurrence SHALL have one local due date independent of daylight-saving clock changes.

#### Scenario: Month-end schedule
- **WHEN** a monthly schedule starts January 31 in a non-leap year
- **THEN** its next occurrences are February 28 and March 31, not March 28

#### Scenario: Leap-year anniversary
- **WHEN** a yearly schedule is anchored to February 29
- **THEN** non-leap years use February 28 and a later leap year returns to February 29

### Requirement: Upcoming previews are not cash
The system SHALL display upcoming occurrences separately from dated ledger activity. Forecast entries SHALL not affect current balances, reports, budget availability, or transaction chasing. A due occurrence SHALL enter the ledger only once its local transaction date arrives.

#### Scenario: Upcoming salary
- **WHEN** a $2,000 salary occurrence is due tomorrow
- **THEN** the preview shows it but today's Ready to Assign and account balance do not include it

### Requirement: Durable due-date materialization
The system SHALL materialize each eligible schedule occurrence once, including overdue occurrences after downtime. Repeated runs and interrupted processing SHALL not create duplicate ledger entries or half a transfer. Materialized entries SHALL retain their schedule and occurrence identity.

#### Scenario: Restart after missed dates
- **WHEN** a daily schedule misses three dates during downtime and the worker runs twice after restart
- **THEN** exactly one dated entry exists for each missed date and no future entry is posted early

### Requirement: Independent posted history and import matching
Editing a template SHALL affect future, unmaterialized occurrences only. Posted occurrences SHALL be editable independently and eligible for explicit provider-import matching. Removing or matching a posted occurrence SHALL not cause its schedule to recreate it on the next worker run.

#### Scenario: Match a scheduled payment
- **WHEN** the owner confirms that an imported payment matches a materialized scheduled entry
- **THEN** one ledger payment remains with both schedule and provider provenance

#### Scenario: Change future rent
- **WHEN** the owner changes a rent template from $1,000 to $1,100 after the current month's occurrence is posted
- **THEN** the posted rent remains $1,000 and the next unmaterialized occurrence uses $1,100
