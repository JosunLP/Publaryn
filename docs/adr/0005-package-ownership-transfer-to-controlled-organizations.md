# ADR 0005: Package ownership transfer to controlled organizations

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn already models package ownership explicitly through `owner_user_id` and `owner_org_id`.
That makes package transfer a natural governance capability, especially for the common case where a personal package matures into an organization-managed package.

At the same time, direct transfer to another user account has sharper security and trust implications than organization-targeted transfer.
Blind user-to-user transfer can surprise the target account, create reputation abuse opportunities, and likely needs an acceptance or confirmation workflow.

## Decision

The first package ownership-transfer slice will support transfer only into organizations that the authenticated actor already administers.

The first slice has these rules:

- the caller must hold the explicit `packages:transfer` scope
- the package must currently be owned either by the authenticated user or by an organization where the actor is an owner or admin
- the target organization must already exist
- the actor must also be an owner or admin in the target organization
- the transfer is applied in a single database transaction
- package ownership is updated by clearing `owner_user_id` and setting `owner_org_id`
- a `package_transfer` audit event is recorded with previous and new ownership metadata

This design deliberately supports the highest-value secure handoff cases first:

- personal package to organization package
- organization package to another organization package when the actor controls both organizations

Direct transfer to another user account is intentionally deferred.
That future slice should add an acceptance-based workflow or equivalent confirmation step.

## Consequences

### Positive

- delivers a real ownership-transfer capability without introducing silent cross-user handoff risk
- supports common team-maturation and consolidation workflows immediately
- remains stateless and horizontally scalable because the full handoff state lives in PostgreSQL
- preserves least-privilege token design through a dedicated scope

### Trade-offs

- the first slice does not support user-target transfers
- package ownership can still diverge from repository ownership, which is allowed today but should be reviewed as repository policy becomes richer
- package transfer currently relies on application logic rather than a dedicated pending-transfer domain model

## Follow-up work

- add PostgreSQL-backed integration tests for successful and denied package-transfer API paths
- design an acceptance-based user-target package transfer workflow
- evaluate whether repository policies should constrain cross-owner package transfer more tightly
- implement namespace and repository ownership transfer flows with similar governance rules
