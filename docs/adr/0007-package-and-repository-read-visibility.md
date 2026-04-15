# ADR 0007: Package and repository reads follow explicit visibility semantics

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn supports multiple visibility modes for packages and repositories: `public`, `private`, `internal_org`, `unlisted`, and `quarantined`.
Those states already existed in the schema and API responses, but the read-path behavior was inconsistent.

Before this change, several endpoints either exposed too much data or applied incomplete filtering.
Examples included:

- package detail endpoints hiding only `private` resources while still exposing `internal_org`, `unlisted`, or `quarantined` data publicly
- repository package listings ignoring repository visibility
- search returning index hits without re-checking repository/package discoverability
- trusted publisher and security finding endpoints not reusing package visibility checks

That behavior was not acceptable for a security-first multi-tenant registry.
It also risked making future protocol adapters inconsistent with the management API.

## Decision

Publaryn will apply one explicit visibility model to read endpoints in the management API.

### Discoverability

Only `public` resources are discoverable in search and package listing surfaces by default.

For the current slice, this means:

- search returns only packages whose package visibility and repository visibility are both `public`
- anonymous user, organization, and repository package listings return only packages that are public inside a public repository

### Direct reads

Direct package or repository URLs may expose:

- `public` resources
- `unlisted` resources

`unlisted` is therefore readable when the caller already knows the direct path, but it is not returned in search or listing endpoints.

### Non-public reads

`private`, `internal_org`, and `quarantined` resources require authenticated access tied to ownership or organization membership.

For the current authorization model:

- user-owned resources are readable by the owning user
- organization-owned resources are readable by organization members

This keeps the current slice aligned with the existing membership model while leaving room for finer-grained repository and package permissions later.

### Failure behavior

Unauthorized reads of non-public resources return `not found` semantics rather than explicit authorization failures.
This reduces unnecessary resource enumeration across tenants.

## Consequences

### Positive

- package and repository reads now align with secure multi-tenant expectations
- unlisted resources have a clear meaning: directly readable, not discoverable
- repository visibility now constrains package discovery and detail access
- search no longer trusts the search index alone for discoverability decisions
- the policy remains replica-safe and stateless because access checks use shared PostgreSQL state

### Trade-offs

- search currently filters non-public visibility after querying the search backend, so authenticated private discovery is intentionally deferred
- organization membership is currently the coarse-grained read gate for organization-owned non-public resources
- package routes still assume ecosystem/name uniqueness at the control-plane path level, which should be revisited if multi-repository name collisions become a supported scenario

## Follow-up work

- introduce actor-aware indexing for private and organization-internal package search
- add finer-grained read permissions at repository and package level
- align ecosystem protocol adapters with the same visibility policy
- revisit package path identity if repository-scoped names become externally addressable
