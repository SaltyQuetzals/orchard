# Spec Delta

## Purpose

Protect one owner's financial information through authenticated access, exclusive account initialization, and recoverable operator-controlled credentials.

## ADDED Requirements

### Requirement: Exclusive owner initialization
The system SHALL allow an operator with host access to initialize exactly one owner and one USD budget, including a budgeting timezone. Public registration and creation of additional owners SHALL be unavailable.

#### Scenario: First initialization
- **WHEN** the operator initializes an unused installation
- **THEN** one owner and one empty USD budget are created
- **AND** no financial information is accessible without authentication

#### Scenario: Repeated initialization
- **WHEN** an operator attempts to initialize an installation that already has an owner
- **THEN** initialization is rejected without replacing the owner or financial data

### Requirement: Authenticated financial access
The system SHALL require a valid owner session to read or change financial data, provider connections, schedules, reports, and application settings. Health information exposed without authentication SHALL contain no financial data or secrets.

#### Scenario: Unauthenticated financial request
- **WHEN** a client without a valid session requests a financial resource or mutation
- **THEN** access is denied and the response discloses no financial data

### Requirement: Secure login and session lifecycle
The system SHALL verify a securely hashed password, limit repeated failed login attempts, and issue expiring server-controlled sessions. Logout and password recovery SHALL invalidate the affected sessions. Browser sessions SHALL resist script access, cross-site mutation, and insecure transport in production.

#### Scenario: Logout invalidates credentials
- **WHEN** the owner logs out and the former session is reused
- **THEN** the former session cannot access financial resources

#### Scenario: Login abuse and cross-site requests
- **WHEN** a client repeatedly submits invalid passwords or submits a cross-site authenticated mutation
- **THEN** repeated login attempts are throttled and the cross-site mutation is rejected

#### Scenario: Expired session
- **WHEN** an owner attempts a mutation with an expired session
- **THEN** the mutation is rejected without changing financial data
- **AND** the interface requests a new login

### Requirement: Operator password recovery
The system SHALL provide host-access password recovery without an email service. Recovery SHALL preserve all financial data and invalidate all existing owner sessions without revealing passwords or provider credentials.

#### Scenario: Recover owner access
- **WHEN** the operator successfully resets the owner password through the host-access recovery procedure
- **THEN** the new password grants access, the old password and sessions do not, and the budget and bank connections are preserved
