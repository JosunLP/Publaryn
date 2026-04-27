---
name: publaryn-control-plane-auth-ownership
description: 'Apply secure identity derivation, authorization, ownership-sensitive mutation rules, and delegated team access in Publaryn. Use when working on bearer-authenticated control-plane writes, adapter publish or private-read auth, package/repository/namespace ownership derivation, organization-admin checks, delegated team package/repository/namespace access, or token-surface restrictions.'
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
- [docs/1.0.md](../../../docs/1.0.md)
- [docs/api-routes.md](../../../docs/api-routes.md)
- [ADR 0001: control-plane request authentication and ownership derivation](../../../docs/adr/0001-control-plane-request-authentication.md)
- [ADR 0008: package creation derives ownership from repositories](../../../docs/adr/0008-control-plane-package-creation.md)
- [ADR 0012: team-based delegated package governance](../../../docs/adr/0012-team-package-governance.md)
- [ADR 0007: read visibility semantics](../../../docs/adr/0007-package-and-repository-read-visibility.md) when the change affects private, internal, unlisted, or quarantined reads

## Code Anchors

Inspect these files early:

- [crates/api/src/request_auth.rs](../../../crates/api/src/request_auth.rs) — `AuthenticatedIdentity`, credential parsing, scope handling, and `ensure_*` authorization helpers
- [crates/api/src/routes/packages.rs](../../../crates/api/src/routes/packages.rs) — repository-derived package ownership, publish/admin/transfer checks, and adapter-adjacent package mutation paths
- [crates/api/src/routes/orgs.rs](../../../crates/api/src/routes/orgs.rs) — organization governance, team membership, delegated package/repository/namespace access, and org-admin checks
- [crates/api/src/routes/repositories.rs](../../../crates/api/src/routes/repositories.rs) — repository ownership, transfer rules, and delegated repository-sensitive reads and writes
- [crates/api/src/routes/search.rs](../../../crates/api/src/routes/search.rs) — actor-aware private and organization-internal visibility filtering for search
- [crates/api/src/scopes.rs](../../../crates/api/src/scopes.rs) — supported control-plane scope taxonomy
- [crates/auth/src/token.rs](../../../crates/auth/src/token.rs) — token validation and claims model
- [crates/auth/src/oidc.rs](../../../crates/auth/src/oidc.rs) — trusted-publishing claim validation when OIDC or package-bound credentials are involved

## Core Invariants

- Derive actor identity from the presented credential, never from request payload ownership fields.
- Keep API replicas stateless; authorization decisions must rely on shared durable state such as PostgreSQL-backed users, memberships, and tokens.
- Treat scopes as part of the authorization contract, not decorative metadata.
- Package ownership is derived from repository governance and current resource state.
- Organization-sensitive writes require owner or admin membership unless an endpoint is explicitly broader.
- Team grants extend package, repository, and namespace access only where the documented route surface explicitly supports them, but they do not transfer ownership.
- OIDC-derived tokens stay confined to their documented protocol surfaces and package or repository bindings.
- Unauthorized reads of non-public resources should preserve not-found semantics where the existing policy already does so.
- Actor-aware search, listings, and direct reads must follow the shared visibility model rather than inventing route-local shortcuts.

## Procedure

1. Identify which credential types the route or protocol surface accepts.
2. Find or add the narrowest existing request-bound identity extractor or `ensure_*` helper instead of inlining new ad hoc auth logic.
3. Resolve ownership and membership from existing resources, repositories, and organization memberships rather than from caller-supplied IDs.
4. Apply the action-specific permission check that matches the behavior: metadata updates, publish/release work, administration, transfer, team-access management, or non-public reads.
5. Emit or preserve audit coverage for governance-critical mutations.
6. Add or extend targeted tests for the exact authorization path you changed, including negative cases where feasible.

## Decision Hints

- Profile and token management should resolve to the authenticated user only.
- Organization management should gate on owner or admin membership in the target organization.
- Package metadata, publish, admin, and ownership-transfer checks should remain action-specific rather than collapsing back to one broad package-write gate.
- Delegated team access should be evaluated at the narrowest supported scope — package, repository, or namespace — and should be cleared when the underlying ownership move invalidates the old organization context.
- Private and `internal_org` reads should follow the shared visibility policy instead of protocol-specific shortcuts.
- Package-bound or OIDC-derived publish tokens must remain confined to their documented surfaces and should be rejected on unrelated control-plane paths.

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
- delegated permissions, if relevant, only apply where the docs say they do and at the correct scope boundary
- tests or integration checks cover the changed authorization behavior
