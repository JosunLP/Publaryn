# ADR 0003: Registered-user organization invitations with in-product acceptance

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn needs organization invitations to support governed multi-user collaboration.
The broader product direction includes email delivery, verified email addresses, and stronger trust signals, but the current platform does not yet implement end-to-end email verification or invitation email delivery.

Introducing tokenized email invitations before email verification exists would create unnecessary trust ambiguity.
At the same time, waiting for the full email subsystem would leave organization onboarding incomplete.

## Decision

The first production-meaningful invitation slice will support invitations for existing active user accounts only.

Organization administrators can invite a user by username or email.
The system resolves that input to an existing active account, stores an invitation record in PostgreSQL, and exposes the pending invitation to the invited user through authenticated control-plane endpoints.

The invited user can:

- list their pending organization invitations
- accept an invitation
- decline an invitation

Organization administrators can:

- create invitations
- list invitations for an organization
- revoke invitations

The invitation workflow is modeled entirely in shared durable state and does not depend on local files, local memory, or single-instance coordination.

Owner-role invitations are explicitly not supported in this slice.
Ownership transfer is intentionally handled by a separate dedicated workflow. See ADR 0004.

## Consequences

### Positive

- delivers usable invitation capability without depending on unfinished email infrastructure
- keeps the workflow safe for horizontal scaling and multi-instance API deployment
- avoids trusting unverified email ownership for organization membership changes
- gives the future web UI a clean in-product invitation inbox to build on
- preserves room for a later email-delivery layer without changing the core invitation state model

### Trade-offs

- users who do not yet have an account cannot be invited in this first slice
- invitations are discoverable through authenticated API access rather than external email links
- owner transfer is intentionally deferred to a future dedicated flow

## Follow-up work

- add verified-email enforcement and out-of-band invitation delivery
- add invitation reminders and expiry notifications
- support explicit invitation decline reasons if needed for enterprise workflows
- continue refining ownership-transfer and last-owner safety rules across the broader org lifecycle after the dedicated handoff flow in ADR 0004
- surface invitation state in the frontend once the bQuery web application is introduced
