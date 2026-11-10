# Spec Delta

## Purpose

Deliver an authenticated, responsive server-rendered budgeting application with convenient online receipt capture and installation without promising offline bookkeeping.

## ADDED Requirements

### Requirement: Authenticated server-rendered application
The system SHALL deliver server-rendered owner pages for account registers, monthly budgets, transaction editing/import review, schedules, targets, reports, receipt inbox, outstanding work, and settings. Core read and ordinary form workflows SHALL remain usable without client-side JavaScript. Financial data SHALL not be present in unauthenticated page responses.

#### Scenario: Open a register directly
- **WHEN** an authenticated owner opens an account URL with client-side JavaScript disabled
- **THEN** the server returns the register's dated entries and functional ordinary entry forms

#### Scenario: Unauthenticated deep link
- **WHEN** an unauthenticated browser opens a Telegram transaction link
- **THEN** it must log in before seeing details and can return to the intended local transaction afterward

### Requirement: Responsive budgeting and receipt capture
The interface SHALL support phone-sized and desktop viewports for budget review, simple transaction editing, outstanding-work resolution, and receipt upload. Supported mobile browsers SHALL offer camera/file selection through ordinary upload controls. Tables and editors SHALL remain navigable without losing critical actions or values.

#### Scenario: Capture a receipt on a phone
- **WHEN** the owner opens a missing-receipt purchase on a phone and submits a supported image
- **THEN** upload progress or result is visible and successful persistence changes the shared receipt status

### Requirement: Online-first installation and offline behavior
The system SHALL provide installation metadata and icons for browsers supporting PWA installation. Offline operation SHALL show a non-sensitive connectivity message, not accept queued financial mutations or receipt uploads. Installation SHALL not imply offline transaction entry, and the interface SHALL state that a connection is required.

#### Scenario: Lose connectivity before submission
- **WHEN** an installed application is offline and the owner attempts to submit a transaction or receipt
- **THEN** it reports the connectivity failure without claiming success or silently queueing the mutation

### Requirement: Private caching and logout
Financial pages, API responses, and receipt evidence SHALL be excluded from service-worker and shared caches. Logout SHALL invalidate the session and clear application-managed private client state. A later visit or offline fallback SHALL not reveal a former session's financial content.

#### Scenario: Reopen after logout
- **WHEN** the owner logs out and reopens the installed application without connectivity
- **THEN** only a non-sensitive offline state is available, not cached registers, receipt images, or provider details

### Requirement: Accessible and conflict-aware forms
Ordinary workflows SHALL provide labeled keyboard-operable controls, visible validation errors, and clear mutation results. Expired sessions and stale concurrent edits SHALL not silently discard or overwrite financial changes. Receipt and category completion SHALL remain visually distinguishable in web lists and transaction detail.

#### Scenario: Stale edit after Telegram categorization
- **WHEN** a web form opened before a Telegram category change submits the older transaction revision
- **THEN** the stale submission reports a conflict and offers current data instead of overwriting the chat change

#### Scenario: Receipt explicitly unavailable
- **WHEN** the owner views a purchase marked unavailable with a retained explanation
- **THEN** the interface labels it unavailable, exposes the explanation, and does not use the attached-evidence status

### Requirement: Required accessible server-rendered financial visualizations
The application SHALL deliver release-required spending bars, net-worth history charts, and both target-progress variants as authenticated server-rendered graphics with accessible titles/descriptions and matching exact-value tables. Graphics and tables SHALL remain inspectable on phone and desktop with JavaScript disabled; filters and drill-downs SHALL work by keyboard without hover or color-only meaning. Tables alone SHALL not substitute for required graphics.

#### Scenario: Inspect charts without JavaScript or a mouse
- **WHEN** the authenticated owner opens the budget and reports with JavaScript disabled and uses only the keyboard
- **THEN** spending bars, net-worth history, and target-progress graphics render with accessible descriptions and exact supporting table values
- **AND** native date filters and drill-down links work without hover, a mouse, or a client-side charting runtime

#### Scenario: Inspect visualizations on a phone
- **WHEN** the owner opens budget target progress and spending/net-worth reports at a phone-sized viewport
- **THEN** chart labels, exact amounts, signed values, supporting tables, filters, and drill-down actions remain readable and navigable without losing critical values or controls
- **AND** cash/credit/payment-shortfall distinctions and stale/missing-history states remain understandable without color alone

#### Scenario: Private chart data after logout
- **WHEN** the owner logs out and a client tries to reopen a financial chart or its supporting table without a valid session or without connectivity
- **THEN** no former-session chart data, table values, or private chart descriptions are disclosed through unauthenticated responses or offline caches
- **AND** financial chart responses use private no-store delivery and are not exposed as public static assets or offline-cached financial data
