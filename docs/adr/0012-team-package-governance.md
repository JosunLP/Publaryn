# ADR 0012: Team-based delegated package governance for organization-owned packages

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn already models organizations, teams, team memberships, and package-scoped team permissions in PostgreSQL.
However, the initial control-plane slices only allowed package management through direct user ownership or broad organization roles.

That left a practical governance gap:

- organizations could create teams, but teams could not actually manage packages
- package responsibilities could not be delegated cleanly without promoting users to broader org roles
- private package access for specialist groups (for example security or release teams) required broader organization membership powers than necessary

Because Publaryn must remain safe for horizontal scaling, delegated package access also needs to live entirely in shared durable state instead of process-local caches or single-node coordinators.

## Decision

Publaryn will support team-based delegated package governance for organization-owned packages.

### Team management surface

Organization administrators can now:

- create, update, and delete teams
- add and remove team members
- list team members
- manage package-scoped team access grants

Team membership is restricted to users who are already members of the parent organization.

### Package-scoped team permissions

The current delegated permissions are:

- `admin`
- `publish`
- `write_metadata`
- `read_private`
- `security_review`
- `transfer_ownership`

These permissions are stored in the existing `team_package_access` table and are always evaluated against the package's current owning organization.

### Authorization model

Team grants extend package access without changing package ownership:

- `write_metadata` allows package metadata changes
- `publish` allows release and artifact lifecycle operations
- `admin` allows package-administration actions such as archival and trusted publisher changes
- `transfer_ownership` allows package ownership transfer from the current owning organization
- any package grant allows non-public package reads for that package

Direct user ownership and organization roles remain authoritative and continue to work alongside team grants.

### Ownership-transfer safety

When a package moves to a different organization, all existing team-package grants for that package are deleted in the same transaction.
This prevents stale permissions from the previous organization from surviving the ownership change.

## Consequences

### Positive

- organizations can delegate package work without over-granting organization-wide roles
- private package access is more least-privilege and audit-friendly
- the model stays horizontally scalable because team grants live in PostgreSQL and are evaluated per request
- package ownership transfers now explicitly clean up stale delegated access

### Trade-offs

- delegation is currently package-scoped only; repository- and namespace-level delegation remain future work
- team management remains an organization-admin capability in this slice rather than introducing team-admin subroles
- `security_review` is reserved for future deeper security workflows beyond the current management API surface

## Follow-up work

- extend delegation to repository- and namespace-scoped governance where appropriate
- surface team package access clearly in the SvelteKit frontend
- add PostgreSQL-backed integration tests for delegated package authorization paths
- expand security workflows that make active use of `security_review`
