# ADR 0002: Control-plane scope taxonomy and enforcement

- Status: Accepted
- Date: 2026-04-15

## Context

ADR 0001 established stateless bearer-based request authentication for the Publaryn control plane.
That work intentionally carried scopes in the authenticated identity without enforcing them yet.
Leaving scopes unenforced would weaken token governance, make audit access difficult to secure, and blur the boundary between interactive sessions and automation credentials.

Publaryn needs a scope model that is:

- explicit and reviewable
- compatible with stateless horizontal scaling
- simple enough for the current modular monolith
- strict enough to support automation and future policy expansion

## Decision

Publaryn will use a small explicit scope taxonomy for the management API.
The initial supported scopes are:

- `profile:write`
- `tokens:read`
- `tokens:write`
- `orgs:write`
- `orgs:join`
- `orgs:transfer`
- `namespaces:write`
- `namespaces:transfer`
- `repositories:write`
- `packages:write`
- `packages:transfer`
- `audit:read`

JWT login sessions receive a default interactive scope set that covers standard self-service control-plane actions.
Platform administrators additionally receive `audit:read` in their default session scopes.

Opaque API tokens created through `POST /v1/tokens` must request one or more supported scopes.
Requested scopes are normalized, deduplicated, and validated against the supported scope list.

`audit:read` is treated as an administrator-only scope.
Audit log access also requires the authenticated user to be a platform administrator.
This keeps full-system audit visibility from becoming a multi-tenant data leak.

`orgs:transfer` is reserved for dedicated organization ownership transfer flows.
Separating it from `orgs:write` allows automation and personal API tokens to omit top-level governance handoff privileges unless they are explicitly needed.

`packages:transfer` is reserved for dedicated package ownership transfer flows.
Separating it from `packages:write` keeps day-to-day release and metadata maintenance distinct from durable ownership handoff.

`namespaces:transfer` is reserved for dedicated namespace-claim ownership transfer flows.
Separating it from `namespaces:write` keeps day-to-day namespace creation and cleanup distinct from durable governance handoff into organizations.

The first enforcement pass applies scopes only to sensitive control-plane operations.
Public read endpoints remain unchanged unless they are explicitly security-sensitive.

## Consequences

### Positive

- token scopes now have real operational meaning
- unsupported or misspelled scopes are rejected early
- audit access is no longer public and is constrained by both scope and administrator status
- JWT sessions and opaque API tokens share the same scope vocabulary
- the design remains stateless and replica-safe

### Trade-offs

- route handlers now have additional authorization checks
- default interactive sessions still receive a broad write-oriented scope set for maintainability and backward compatibility
- scope enforcement is coarse-grained for now and should evolve as package, repository, and organization policies become richer

## Follow-up work

- add finer-grained scopes for read-private, membership administration, security review, and trusted publishing
- introduce organization- or repository-scoped tokens once the domain model is ready
- expose scope metadata in OpenAPI documentation
- add integration tests that exercise denied paths, especially for audit access and token scope grants
