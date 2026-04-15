# ADR 0008: Control-plane package creation derives ownership from repositories

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn already supported package metadata updates, release state changes, tag mutation, trusted publisher configuration, and ownership transfer.
However, the management API still lacked an explicit package creation endpoint.

That gap made the control plane inconsistent and left important policy questions unanswered:

- who is allowed to create a package in a repository
- whether package ownership should be caller-supplied or repository-derived
- how package visibility interacts with repository visibility
- how namespace claims constrain new package names
- how to avoid ambiguous control-plane paths while package reads are still addressed as `/v1/packages/:ecosystem/:name`

Publaryn's current control-plane read model assumes a package name is unique within an ecosystem.
The database schema allows uniqueness per repository, but the public management path does not include repository identity yet.
Until that path model evolves, package creation needs a conservative rule that preserves route stability.

## Decision

Publaryn adds `POST /v1/packages` as the initial control-plane package creation endpoint.

### Ownership

Package creation requires `packages:write` and write access to the target repository.
The package owner is derived from the repository owner.
Caller-supplied owner IDs are intentionally not accepted.

This keeps package ownership aligned with repository governance and follows the broader control-plane rule that ownership-sensitive fields are derived from authenticated state and existing resources.

### Repository eligibility

Packages can currently be created only in repository kinds intended for hosted content:

- `public`
- `private`
- `staging`
- `release`

`proxy` and `virtual` repositories are excluded from direct package creation.

### Visibility

Package visibility defaults to repository visibility unless explicitly requested.
Requested package visibility must not be broader than repository visibility.

Additional guardrails:

- `internal_org` requires an organization-owned repository
- packages inside a `quarantined` repository must remain `quarantined`

### Namespace claim enforcement

If a matching namespace claim exists for a namespace that can be extracted unambiguously from the package name, that claim must belong to the same owner as the repository.

The first extraction set is intentionally narrow:

- npm/Bun scope, such as `@acme/pkg` → `@acme`
- Composer vendor, such as `acme/pkg` → `acme`
- Maven group ID, such as `com.acme:artifact` → `com.acme`

Other ecosystems are left for follow-up work because their namespace models are either optional, prefix-based, or otherwise less exact in the current schema.

### Path stability

For the current slice, package names are enforced as globally unique within an ecosystem.
This is stricter than the database uniqueness constraint, but it preserves unambiguous control-plane routing while package reads remain addressed by ecosystem and name only.

### Search indexing

After the database transaction commits, package creation attempts to index the package in search.
Search indexing failure does not fail package creation, because search is a derived, eventually consistent view rather than the source of truth.

## Consequences

### Positive

- the control plane now supports explicit package creation
- ownership is derived safely from repository governance
- repository and package visibility semantics stay aligned
- namespace claims can protect scoped ecosystems from unauthorized package creation
- search availability is decoupled from package metadata durability
- the current control-plane route shape remains unambiguous

### Trade-offs

- ecosystem-global uniqueness is stricter than the current database schema and may limit future multi-repository naming flexibility until package paths evolve
- namespace claim enforcement is intentionally partial and currently covers only ecosystems with exact namespace extraction
- package creation still performs a full ecosystem name scan for similarity checks, which is acceptable for the current slice but should be optimized later

## Follow-up work

- move package identity toward repository-aware or namespace-aware routing if multi-repository duplicate names become a requirement
- expand namespace extraction and ownership checks for additional ecosystems
- push similarity checks toward indexed or optimized lookup paths as the registry grows
- add publish-time workflows that create-and-publish atomically for ecosystems that expect implicit package creation on first publish
