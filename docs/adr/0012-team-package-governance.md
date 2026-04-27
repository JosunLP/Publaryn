# ADR 0012: Team-based delegated governance for organization-owned resources

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn already models organizations, teams, team memberships, and delegated access grants in PostgreSQL.
However, the initial control-plane slices only allowed package, repository, and namespace management through direct user ownership or broad organization roles.

That left a practical governance gap:

- organizations could create teams, but teams could not actually manage packages
- package responsibilities could not be delegated cleanly without promoting users to broader org roles
- private package access for specialist groups (for example security or release teams) required broader organization membership powers than necessary
- repository-wide and namespace-level responsibilities could not be delegated without over-granting organization-wide administration

Because Publaryn must remain safe for horizontal scaling, delegated access also needs to live entirely in shared durable state instead of process-local caches or single-node coordinators.

## Decision

Publaryn will support team-based delegated governance for organization-owned packages, repositories, and namespace claims.

### Team management surface

Organization administrators can now:

- create, update, and delete teams
- add and remove team members
- list team members
- manage package-scoped team access grants
- manage repository-scoped team access grants
- manage namespace-scoped team access grants

Team membership is restricted to users who are already members of the parent organization.

### Team permission scopes

Package and repository grants support the package lifecycle permissions:

- `admin`
- `publish`
- `write_metadata`
- `read_private`
- `security_review`
- `transfer_ownership`

Namespace grants support the namespace governance permissions:

- `admin`
- `transfer_ownership`

Package permissions are stored in `team_package_access`, repository permissions are stored in `team_repository_access`, and namespace permissions are stored in `team_namespace_access`.
All grants are always evaluated against the resource's current owning organization.

### Authorization model

Team grants extend access without changing package, repository, or namespace ownership:

- `write_metadata` allows package metadata changes
- `publish` allows release and artifact lifecycle operations
- `admin` allows package-administration actions such as visibility changes, archival, and trusted publisher changes
- `transfer_ownership` allows package ownership transfer from the current owning organization
- any package grant allows non-public package reads for that package
- repository grants apply the same package lifecycle permissions to all current and future packages in the repository
- namespace `admin` allows deletion of organization-owned namespace claims
- namespace `transfer_ownership` allows transfer of organization-owned namespace claims into another controlled organization

Direct user ownership and organization roles remain authoritative and continue to work alongside team grants.

### Ownership-transfer safety

When a package, repository, or namespace claim moves to a different organization, existing team grants for that resource are deleted in the same transaction.
This prevents stale permissions from the previous organization from surviving the ownership change.

## Consequences

### Positive

- organizations can delegate package work without over-granting organization-wide roles
- repository-level grants cover all packages in a repository without duplicating per-package grants
- namespace-level grants make claim deletion and transfer least-privilege workflows
- private package access is more least-privilege and audit-friendly
- the model stays horizontally scalable because team grants live in PostgreSQL and are evaluated per request
- ownership transfers now explicitly clean up stale delegated access

### Trade-offs

- team management remains an organization-admin capability in this slice rather than introducing team-admin subroles
- `security_review` is reserved for future deeper security workflows beyond the current management API surface
- repository-level grants intentionally apply to current and future packages in that repository, so administrators should use package-scoped grants when they need narrower delegation

## Follow-up work

- expand end-to-end coverage for delegated authorization paths across native adapters as they gain private-read checks
- expand security workflows that make active use of `security_review`
