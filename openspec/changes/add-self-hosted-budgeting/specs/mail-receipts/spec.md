# Spec Delta

## Purpose

Collect receipt messages from a deliberately selected mailbox scope and preserve original evidence through a provider-independent receipt workflow, starting with Gmail.

## ADDED Requirements

### Requirement: Owner-authorized Gmail connection
The system SHALL allow the authenticated owner to connect Gmail and select one receipt label. Authorization SHALL request read-only mailbox access, disclose that provider permission is mailbox-wide rather than label-restricted, and encrypt stored credentials. A failed or mismatched authorization callback SHALL not replace the active connection.

#### Scenario: Consent and label selection
- **WHEN** the owner authorizes Gmail and selects the Receipts label
- **THEN** the connection screen identifies the mailbox and label and explains the actual read-only permission scope

### Requirement: Label-scoped receipt discovery
The Gmail connector SHALL collect only messages carrying the selected label, traverse all result pages, and discover older messages labeled later. Collection SHALL not depend on message unread state or the transaction-chasing start date. It SHALL not send, delete, relabel, or mark mailbox messages read.

#### Scenario: Label applied to an old message
- **WHEN** an email from six months ago receives the selected label after an earlier collection run
- **THEN** a later completed scan collects it even though its message date predates the chasing start date

#### Scenario: Message outside selected label
- **WHEN** an unrelated message exists in the authorized mailbox without the selected label
- **THEN** it is not fetched into the receipt inbox merely because the OAuth credential can read it

### Requirement: Original message preservation
Collected evidence SHALL preserve the original message with its headers, text/HTML bodies, and attachments. A message with no attachment SHALL remain usable evidence. Decoded previews and extracted attachments SHALL retain their connection to that original message.

#### Scenario: HTML-only receipt
- **WHEN** a labeled message contains an HTML receipt body and no PDF
- **THEN** its original message is retained and a safe preview can be linked as purchase evidence without conversion

### Requirement: Idempotent and resumable collection
Repeated scans SHALL create at most one receipt-inbox item per provider mailbox and message identity. Collection progress SHALL not skip a message whose original content was not durably saved. A failure for one message SHALL be visible without discarding other successfully collected messages.

#### Scenario: Restart during collection
- **WHEN** the application stops after storing a message but before completing its scan
- **THEN** resuming collection reuses that evidence and still attempts any unstored messages

### Requirement: Provider-independent evidence behavior
Collected mail SHALL enter the same receipt inbox and use the same explicit purchase-linking and completion rules as other evidence channels. Mail-provider-specific identifiers SHALL not be required by purchase-facing receipt behavior. Only Gmail SHALL be a required initial provider; no additional mail provider implementation is required by this change.

#### Scenario: Link collected mail to a purchase
- **WHEN** the owner confirms a Gmail receipt's purchase in the receipt inbox
- **THEN** the purchase gains attached evidence through the shared receipt workflow without changing its category or requiring a Gmail-specific purchase field

### Requirement: Connection status and disconnect
The system SHALL expose last collection attempt, last completed scan, message-level errors, and required reauthorization. Disconnecting SHALL remove local mail credentials and stop collection while preserving stored evidence and links. Removing a label or deleting the source email SHALL not delete already collected evidence.

#### Scenario: Gmail authorization revoked
- **WHEN** Gmail rejects a refresh credential
- **THEN** collection pauses with a reconnect action while previously collected receipts remain accessible

#### Scenario: Source email removed
- **WHEN** a merchant receipt is deleted from Gmail after collection
- **THEN** its stored original and purchase linkage remain intact in Orchard
