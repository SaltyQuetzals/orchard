# Spec Delta

## Purpose

Keep purchase evidence durable and accessible while tracking receipt completion independently from category completion across web, mail, and Telegram channels.

## ADDED Requirements

### Requirement: Purchase-level receipt work
Every dated non-transfer purchase outflow SHALL have a receipt status of missing, attached, or unavailable. Opening balances, reconciliation adjustments, forecasts, and transfer-only entries SHALL not require purchase evidence. A split purchase SHALL share parent-level evidence while each spending allocation retains its own category completion.

#### Scenario: Receipt and category are independent
- **WHEN** an uncategorized grocery purchase receives a receipt
- **THEN** its receipt status becomes attached and its missing-category work remains outstanding

#### Scenario: Categorize without a receipt
- **WHEN** the owner categorizes an otherwise undocumented purchase
- **THEN** category work completes and receipt work remains missing

### Requirement: Shared multi-channel evidence intake
The system SHALL accept web/PWA uploads, authorized Telegram submissions, and collected original emails into one evidence inbox. The owner SHALL explicitly select or confirm the purchase linkage; unlinked evidence SHALL remain reviewable without falsely resolving a purchase. Evidence SHALL support multiple files per purchase and explicit linkage to more than one purchase.

#### Scenario: Ambiguous receipt
- **WHEN** a receipt could belong to either of two same-amount purchases
- **THEN** neither purchase is silently marked complete and the owner can choose its linkage

#### Scenario: Repeated delivery
- **WHEN** the same channel item is delivered again or the same evidence is linked twice to one purchase
- **THEN** the existing evidence and link are reused without duplicate receipt rows

### Requirement: Original email as evidence
The system SHALL retain the original email content, headers, bodies, and attachments as evidence, including messages with no attachment. An email SHALL not need conversion to PDF to resolve receipt work. Safe previews SHALL not execute active content or fetch remote resources.

#### Scenario: Body-only receipt email
- **WHEN** the owner links a merchant's body-only email to a purchase
- **THEN** the original email is retrievable, its safely rendered body is viewable, and the purchase has attached evidence

### Requirement: Explicit unavailable-receipt resolution
The owner SHALL be able to mark a purchase's receipt unavailable only with a nonblank retained explanation. This SHALL resolve receipt chasing without claiming evidence is attached or resolving category work. The owner SHALL be able to reopen receipt work or replace the resolution with attached evidence.

#### Scenario: Lost receipt and missing category
- **WHEN** the owner marks an uncategorized purchase unavailable with the explanation "Paper receipt lost"
- **THEN** the explanation is retained, the status is visibly unavailable rather than attached, and only the category remains outstanding

### Requirement: Bounded and authenticated evidence access
Uploads SHALL enforce disclosed size and type limits before becoming available as evidence. Web and Telegram file intake SHALL support images and PDFs; original email intake SHALL preserve mail content. Downloads and previews SHALL require owner authorization, resist filename/path traversal, and never serve untrusted active content as application HTML.

#### Scenario: Oversized or unsafe upload
- **WHEN** a submitted file exceeds its channel's limit or is an unsupported executable
- **THEN** the file is rejected with a visible reason and the purchase remains missing a receipt

#### Scenario: Unauthorized evidence request
- **WHEN** a client without an owner session requests a known receipt URL
- **THEN** neither the file nor its private metadata is disclosed

### Requirement: Durable linkage and removal
A purchase SHALL be marked attached only after its original evidence is durably stored and linked. Interrupted uploads SHALL not create completed receipt work. Unlinking the last evidence SHALL reopen receipt work unless an explicit unavailable resolution remains. Removing one link SHALL not delete evidence still linked elsewhere.

#### Scenario: Interrupted upload
- **WHEN** file persistence fails before the receipt link commits
- **THEN** the purchase remains unresolved and the owner can retry without a dangling completed receipt

#### Scenario: Remove shared evidence link
- **WHEN** evidence linked to two purchases is unlinked from one
- **THEN** the other purchase retains accessible evidence and the first becomes missing unless separately resolved as unavailable
