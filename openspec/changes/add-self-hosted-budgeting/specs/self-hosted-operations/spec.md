# Spec Delta

## Purpose

Operate Orchard reliably on the owner's homeserver with durable financial and receipt storage, recoverable credentials, and documented deployment, upgrade, and restore procedures.

## ADDED Requirements

### Requirement: Homeserver container deployment
The system SHALL provide a reproducible container image and Docker Compose deployment for the existing homeserver behind a public HTTPS reverse proxy. Application and worker execution SHALL not require a hosted database or application subscription. Required external SimpleFIN service costs and optional Gmail/Telegram configuration SHALL be documented separately.

#### Scenario: Fresh deployment
- **WHEN** the operator follows the documented Compose and reverse-proxy procedure with persistent storage and required secrets
- **THEN** the application starts, exposes a non-sensitive health endpoint, and supports host-access owner initialization

### Requirement: Persistent local application data
Database state, original receipts, emails, and receipt links SHALL survive container recreation. The system SHALL refuse to treat an unreadable or incompatible existing store as a new empty installation. Storage failures SHALL be visible without acknowledging undurable financial or receipt mutations.

#### Scenario: Replace the application container
- **WHEN** the operator recreates the container using the same persistent data and secrets
- **THEN** the owner, financial history, integrations, schedules, and original evidence remain available

### Requirement: Secrets and operational privacy
Provider credentials SHALL be encrypted at rest with an operator-controlled key kept outside the application data backup. Deployment SHALL protect that key and session secrets. Logs, diagnostics, and health output SHALL not expose access URLs, authorization tokens, receipt bodies, or full financial payloads.

#### Scenario: Missing credential key
- **WHEN** an existing installation starts without its required credential-encryption key
- **THEN** it reports the configuration problem without overwriting credentials or silently initializing a different installation

### Requirement: Controlled schema migrations and rollback
Upgrades SHALL apply ordered migrations under exclusive coordination before serving incompatible traffic or running workers. A failed migration SHALL prevent normal startup and preserve diagnostic information. Documentation SHALL require a compatible backup before migration and restore-based rollback when an earlier image cannot read the migrated schema.

#### Scenario: Migration fails
- **WHEN** an upgrade migration fails
- **THEN** normal financial writes and worker jobs do not start against a partially migrated schema

### Requirement: Consistent database and evidence backup
The operator SHALL be able to create a consistent backup containing the database, all referenced original evidence, and a manifest identifying format/schema and integrity checks. Backup creation SHALL coordinate with writes so no completed receipt points to a missing file. Required secrets SHALL be separately recoverable, and encrypted off-host storage SHALL be documented.

#### Scenario: Receipt arrives during backup
- **WHEN** a receipt upload overlaps backup creation
- **THEN** the backup contains either the committed receipt and its original file or neither, never a completed receipt with missing evidence

### Requirement: Verified isolated restore
Restore SHALL validate backup compatibility and evidence integrity before activating recovered data and SHALL refuse to overwrite an active store implicitly. A successful isolated restore SHALL recover financial totals, receipt originals and links, schedules, and decryptable provider settings. Restored sessions SHALL be invalidated and external jobs SHALL remain paused until explicit operator activation.

#### Scenario: Corrupt receipt archive
- **WHEN** backup evidence does not match its integrity manifest
- **THEN** restore fails before replacing live data and reports which integrity check failed

#### Scenario: Restore without external side effects
- **WHEN** the operator restores a valid backup into an isolated location with the matching encryption key
- **THEN** the owner can inspect recovered balances and receipts without sending Telegram notices or polling providers until jobs are explicitly enabled

### Requirement: Observable worker operation and documented recovery
The application SHALL expose authenticated job and integration status sufficient to distinguish a healthy web server from stalled financial sync, mail collection, schedule processing, or Telegram delivery. Operations documentation SHALL cover bootstrap/recovery, provider reconnect/revocation, persistent storage, backups, restore, upgrades, and external service outages.

#### Scenario: Worker stops progressing
- **WHEN** web requests still work but an integration job has stopped completing
- **THEN** its last attempt, success, and failure state are visible without exposing sensitive payloads
