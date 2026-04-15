# ADR 0004: Dedicated organization ownership transfer flow

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn already treats organization ownership as a high-sensitivity governance capability.
Generic member management and invitation flows explicitly reject owner-role assignment and owner-role removal because those paths do not provide enough structure for a safe handoff.

At the same time, organizations need a supported path for leadership changes, account handoff, and project maturation.
Without a dedicated transfer flow, ownership becomes sticky and operationally awkward.

## Decision

Publaryn will expose a dedicated ownership-transfer endpoint for organizations.

The first ownership-transfer slice has these rules:

- the caller must be a current organization owner
- the caller must hold the explicit `orgs:transfer` scope
- the target user must already be an active member of the organization
- the target user must not already be an owner
- the transfer is executed in a single database transaction
- the target member is promoted to `owner`
- the initiating owner is demoted to `admin`
- an `org_ownership_transfer` audit event is recorded

This flow is intentionally scoped to transfers between existing members only.
It does not yet introduce approval chains, pending transfer state, email confirmation, or a database-enforced single-owner invariant.

## Consequences

### Positive

- closes the governance gap left by blocking owner changes in generic member endpoints
- keeps the handoff replica-safe and horizontally scalable because all state lives in PostgreSQL
- makes ownership transfer auditable with a dedicated event type
- allows least-privilege API tokens to exclude ownership-transfer authority unless explicitly granted

### Trade-offs

- the first slice supports only direct owner-to-member handoff, not multi-step approval workflows
- the former owner is always demoted to `admin`, which is simple but not yet customizable
- application logic, not database constraints, still carries the last-owner safety rule for this workflow

## Follow-up work

- add integration tests that exercise transfer success and denied paths against PostgreSQL-backed API routes
- evaluate whether a stronger database-side invariant is needed for at-least-one-owner enforcement
- support richer handoff workflows such as confirmation windows, approval requirements, or owner exit flows
- consider namespace and repository ownership transfer flows that mirror the same governance principles
