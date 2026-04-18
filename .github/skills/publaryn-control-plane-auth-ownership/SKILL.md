---
name: publaryn-control-plane-auth-ownership
description: 'Apply secure identity derivation, authorization, and ownership-sensitive mutation rules in Publaryn. Use when working on bearer-authenticated control-plane writes, adapter publish auth, package ownership derivation, organization-admin checks, delegated team package access, or token-surface restrictions.'
argument-hint: 'Describe the endpoint, protocol surface, or authorization decision you are changing.'
user-invocable: true
disable-model-invocation: false
---

# Publaryn Control-plane Auth and Ownership

## Outcome

Implement or review security-sensitive behavior without trusting caller-supplied identity data, while preserving the repository's stateless multi-replica architecture.

## When to Use

Use this skill when the task involves:

- mutable `/v1/*` control-plane endpoints authenticated by Bearer credentials
- native adapter publish or private-read authorization
- package, repository, namespace, or organization ownership derivation
- delegated package permissions for organization teams
- scope-aware token handling, token-surface confinement, or OIDC-derived token restrictions

## Primary Sources

Read these documents first:

- [README](../../../README.md)
- [ADR 0001: control-plane request authentication and ownership derivation](../../../docs/adr/0001-control-plane-request-authentication.md)
- [ADR 0008: package creation derives ownership from repositories](../../../docs/adr/0008-control-plane-package-creation.md)
- [ADR 0012: team-based delegated package governance](../../../docs/adr/0012-team-package-governance.md)
- [ADR 0007: read visibility semantics](../../../docs/adr/0007-package-and-repository-read-visibility.md) when the change affects private, internal, unlisted, or quarantined reads

## Code Anchors

Inspect these files early:

- [crates/api/src/request_auth.rs](../../../crates/api/src/request_auth.rs) — `AuthenticatedIdentity`, credential parsing, scope handling, and `ensure_*` authorization helpers
- [crates/api/src/routes/packages.rs](../../../crates/api/src/routes/packages.rs) — repository-derived package ownership, publish/admin/transfer checks, and adapter-adjacent package mutation paths
- [crates/api/src/scopes.rs](../../../crates/api/src/scopes.rs) — supported control-plane scope taxonomy
- [crates/auth/src/token.rs](../../../crates/auth/src/token.rs) — token validation and claims model
- [crates/auth/src/oidc.rs](../../../crates/auth/src/oidc.rs) — trusted-publishing claim validation when OIDC or package-bound credentials are involved

## Core Invariants

- Derive actor identity from the presented credential, never from request payload ownership fields.
- Keep API replicas stateless; authorization decisions must rely on shared durable state such as PostgreSQL-backed users, memberships, and tokens.
- Treat scopes as part of the authorization contract, not decorative metadata.
- Package ownership is derived from repository governance and current resource state.
- Organization-sensitive writes require owner or admin membership unless an endpoint is explicitly broader.
- Team grants extend package access for organization-owned packages, but they do not transfer package ownership.
- OIDC-derived tokens stay confined to their documented protocol surfaces and package or repository bindings.
- Unauthorized reads of non-public resources should preserve not-found semantics where the existing policy already does so.

## Procedure

1. Identify which credential types the route or protocol surface accepts.
2. Find or add the narrowest existing request-bound identity extractor or `ensure_*` helper instead of inlining new ad hoc auth logic.
3. Resolve ownership and membership from existing resources, repositories, and organization memberships rather than from caller-supplied IDs.
4. Apply the action-specific permission check that matches the behavior: metadata updates, publish/release work, administration, transfer, or non-public reads.
5. Emit or preserve audit coverage for governance-critical mutations.
6. Add or extend targeted tests for the exact authorization path you changed, including negative cases where feasible.

## Decision Hints

- Profile and token management should resolve to the authenticated user only.
- Organization management should gate on owner or admin membership in the target organization.
- Package metadata, publish, admin, and ownership-transfer checks should remain action-specific rather than collapsing back to one broad package-write gate.
- Delegated team access matters only for organization-owned packages and should be cleared during ownership transfer according to [ADR 0012](../../../docs/adr/0012-team-package-governance.md).
- Private and `internal_org` reads should follow the shared visibility policy instead of protocol-specific shortcuts.

## Out of Scope

This skill does not cover:

- inventing a new authentication system or local session cache
- weakening ownership derivation to trust caller-provided owner IDs
- broad policy-engine redesign unrelated to the concrete task
- replacing route-level checks with speculative cross-cutting abstractions without a demonstrated need

## Completion Checks

Before finishing, verify that:

- ownership-sensitive fields are derived from credentials and existing resources
- the changed path still works across multiple replicas with no process-local state
- delegated permissions, if relevant, only apply where the docs say they do
- tests or integration checks cover the changed authorization behavior
