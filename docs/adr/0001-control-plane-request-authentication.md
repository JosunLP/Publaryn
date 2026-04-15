# ADR 0001: Control-plane request authentication and ownership derivation

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn is designed as a horizontally scalable modular monolith with stateless API replicas.
Before this change, multiple mutable control-plane endpoints accepted caller-supplied identity data or used placeholder identities such as `Uuid::nil()`.
That created clear authorization gaps for token management, package mutations, namespace claims, repository changes, and organization administration.

The platform mission requires secure defaults, strong governance, immutable artifacts, and clean horizontal scaling.
Any write-path identity model must therefore:

- avoid local in-memory session affinity
- work safely across multiple API replicas
- derive the acting principal from verifiable credentials
- keep ownership checks at clear module boundaries
- support both browser/API session flows and automation credentials

## Decision

Publaryn control-plane write endpoints will authenticate requests through bearer credentials carried in the `Authorization` header.

Two credential types are supported:

1. JWT access tokens issued by `/v1/auth/login`
2. Opaque API tokens created by `/v1/tokens` and stored hashed in PostgreSQL

The API layer resolves the authenticated actor into a request-scoped identity object.
That identity contains:

- authenticated user ID
- optional token ID
- credential kind
- granted scopes for future authorization expansion

The API must derive actor identity from the credential, not from request payload fields.
Caller-supplied fields such as `created_by` or foreign `owner_user_id` values must not be trusted as the source of truth.

For the current increment, mutable control-plane routes enforce minimum viable authorization as follows:

- user profile updates require the authenticated user to match the target account
- token management is restricted to the authenticated user
- package mutations require package ownership or qualifying organization membership
- organization administration requires owner or admin membership
- organization-owned namespace or repository mutations require owner or admin membership in the owning organization
- user-owned namespace or repository creation is limited to the authenticated user

Selected security-sensitive write actions also emit audit events.

## Consequences

### Positive

- API replicas remain stateless and scale horizontally without sticky sessions
- identity is derived consistently across write paths
- caller-controlled identity spoofing is reduced substantially
- opaque API tokens are usable across replicas because validation relies on shared PostgreSQL state
- authorization logic is centralized in the API boundary instead of scattered ad hoc inside handlers

### Trade-offs

- authorization checks currently perform database lookups on write paths
- scope strings are carried in the identity object but are not yet fully enforced per route
- JWT access tokens are self-contained and currently not revocation-backed through shared token persistence
- package and organization authorization is intentionally minimal for the first secure increment and should evolve into richer policy enforcement

## Follow-up work

- add route-level scope enforcement with a documented scope taxonomy
- persist or introspect JWT session identifiers if revocation is required for interactive sessions
- expand audit coverage for all governance-critical mutations
- move repeated authorization patterns toward domain services as policy complexity grows
- add integration tests with PostgreSQL-backed fixtures for organization and package authorization paths
