# Publaryn ADR Index

This index maps each accepted architecture decision record to the part of the
product surface it governs.

| ADR                                                                    | Title                                                                 | Primary surface                                   |
| ---------------------------------------------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------- |
| [0001](0001-control-plane-request-authentication.md)                   | Control-plane request authentication and ownership derivation         | bearer auth, ownership-sensitive writes           |
| [0002](0002-control-plane-scope-taxonomy.md)                           | Control-plane scope taxonomy                                          | token scopes and route authorization              |
| [0003](0003-registered-user-organization-invitations.md)               | Registered-user organization invitations                              | invitation model and membership onboarding        |
| [0004](0004-organization-ownership-transfer.md)                        | Organization ownership transfer                                       | organization governance                           |
| [0005](0005-package-ownership-transfer-to-controlled-organizations.md) | Package ownership transfer to controlled organizations                | package governance                                |
| [0006](0006-control-plane-cors-origin-allowlist.md)                    | Control-plane CORS origin allowlist                                   | browser/API boundary                              |
| [0007](0007-package-and-repository-read-visibility.md)                 | Package and repository read visibility                                | visibility and private-read rules                 |
| [0008](0008-control-plane-package-creation.md)                         | Control-plane package creation                                        | package identity and repository-derived ownership |
| [0009](0009-control-plane-release-publication-and-artifact-storage.md) | Control-plane release publication and artifact storage                | release lifecycle and artifact storage            |
| [0010](0010-npm-registry-protocol-adapter.md)                          | npm registry protocol adapter                                         | npm/Bun adapter architecture                      |
| [0011](0011-stateless-api-runtime-and-graceful-lifecycle.md)           | Stateless API runtime and graceful lifecycle                          | API process model and shutdown                    |
| [0012](0012-team-package-governance.md)                                | Team delegated governance                                             | package, repository, and namespace team access    |
| [0013](0013-pypi-simple-api-read-surface.md)                           | PyPI Simple API read surface                                          | PyPI read adapter                                 |
| [0014](0014-pypi-legacy-upload-bridge.md)                              | PyPI legacy upload bridge                                             | PyPI publish adapter                              |
| [0015](0015-pypi-trusted-publishing.md)                                | PyPI trusted publishing                                               | OIDC trusted publishing                           |
| [0016](0016-cargo-alternative-registry-adapter.md)                     | Cargo alternative registry adapter                                    | Cargo sparse index and publish                    |
| [0017](0017-nuget-v3-protocol-adapter.md)                              | NuGet V3 protocol adapter                                             | NuGet read/write surface                          |
| [0018](0018-rate-limiting-and-background-job-queue.md)                 | Redis-backed rate limiting and PostgreSQL-backed background job queue | abuse protection and async work                   |
| [0019](0019-oci-distribution-adapter.md)                               | OCI Distribution Protocol Adapter                                     | OCI read/write surface                            |
| [0020](0020-maven-deploy-adapter.md)                                   | Maven Deploy Adapter                                                  | Maven publish surface                             |
| [0021](0021-composer-publish-adapter.md)                               | Composer Publish Adapter                                              | Composer publish surface                          |
| [0022](0022-rubygems-push-adapter.md)                                  | RubyGems Push Adapter                                                 | RubyGems publish surface                          |

## How to use this index

- Start with the [repository README](https://github.com/JosunLP/Publaryn/blob/main/README.md) for the repository baseline.
- Use [docs/1.0.md](../1.0.md) for the current release contract.
- Read the relevant ADRs before changing authentication, visibility, release,
  governance, or protocol behavior.

## 1.0 contract map

- **Scope, support posture, and release criteria:** [docs/1.0.md](../1.0.md)
  and the [repository README](https://github.com/JosunLP/Publaryn/blob/main/README.md)
- **Repository lifecycle baseline:** hosted `public`, `private`, `staging`, and `release`
  repositories in 1.0; proxy and virtual repositories remain post-1.0 lifecycle work
- **Visibility and actor-aware search:** [0007](0007-package-and-repository-read-visibility.md)
- **Control-plane authentication and scoped writes:** [0001](0001-control-plane-request-authentication.md)
  and [0002](0002-control-plane-scope-taxonomy.md)
- **Release lifecycle, quarantine, async work, and operator queue visibility:** [0009](0009-control-plane-release-publication-and-artifact-storage.md)
  and [0018](0018-rate-limiting-and-background-job-queue.md); see
  [docs/operator/job-queue-recovery.md](../operator/job-queue-recovery.md) for
  the `/v1/admin/jobs` operator surface and queue recovery baseline
- **Organization governance and delegated access:** [0003](0003-registered-user-organization-invitations.md),
  [0004](0004-organization-ownership-transfer.md), [0005](0005-package-ownership-transfer-to-controlled-organizations.md),
  and [0012](0012-team-package-governance.md)
- **Mounted adapter surfaces:** [0010](0010-npm-registry-protocol-adapter.md),
  [0013](0013-pypi-simple-api-read-surface.md), [0014](0014-pypi-legacy-upload-bridge.md),
  [0015](0015-pypi-trusted-publishing.md), [0016](0016-cargo-alternative-registry-adapter.md),
  [0017](0017-nuget-v3-protocol-adapter.md), [0019](0019-oci-distribution-adapter.md),
  [0020](0020-maven-deploy-adapter.md), [0021](0021-composer-publish-adapter.md),
  and [0022](0022-rubygems-push-adapter.md)
