# API and Adapter Route Reference

This page is the documentation-oriented route map for Publaryn 1.0. It is not
an OpenAPI replacement; it exists to show the mounted surface area and the
baseline responsibilities of each route group.

## Control-plane surface

The management API lives under `/v1/*` and is paired with public liveness,
readiness, and protocol mounts.

| Prefix or route         | Purpose                                                                              |
| ----------------------- | ------------------------------------------------------------------------------------ |
| `/v1/auth/*`            | Registration, login, logout, MFA, and account security flows                         |
| `/v1/users/*`           | User profile and user package views                                                  |
| `/v1/orgs/*`            | Organization profile, membership, teams, audit, security, repositories, and packages |
| `/v1/org-invitations/*` | Invitation inbox plus accept/decline actions                                         |
| `/v1/namespaces/*`      | Namespace claim creation, transfer, deletion, and lookup                             |
| `/v1/repositories/*`    | Repository creation, reads, updates, transfer, and package listings                  |
| `/v1/packages/*`        | Package, release, artifact, tag, security-finding, and trusted-publisher workflows   |
| `GET /v1/search`        | Visibility-aware package search                                                      |
| `/v1/tokens*`           | Scoped API token issuance, listing, and revocation                                   |
| `GET /v1/audit`         | Platform audit log for platform administrators                                       |
| `GET /v1/admin/jobs`    | Filtered operator queue visibility for recovery and triage                           |
| `GET /v1/stats`         | Public top-level platform statistics                                                 |
| `GET /health`           | Liveness probe                                                                       |
| `GET /readiness`        | Readiness probe backed by PostgreSQL and optional Redis connectivity                 |
| `GET /swagger-ui`       | Interactive OpenAPI/Swagger UI for the management API                                |

## Control-plane workflow hotspots

The following endpoint groups define the main 1.0 user journeys:

- **Authentication and account security**: register, login, logout, TOTP MFA,
  recovery codes, and scoped token management.
- **Governance**: organizations, invitations, teams, delegated package access,
  repository access, namespace access, and ownership transfer flows.
- **Package lifecycle**: package creation, release creation, artifact upload,
  publish, yank, unyank, deprecate, tags, security-finding triage, and trusted
  publisher configuration.
- **Operations**: platform statistics, operator queue visibility, audit export,
  security export, and health probes.

## Native protocol adapter mounts

Each adapter is mounted in the main API router under a fixed prefix.

### npm / Bun — `/npm`

| Route                                             | Purpose                    |
| ------------------------------------------------- | -------------------------- |
| `GET /npm/-/v1/search`                            | npm-compatible search      |
| `GET /npm/-/package/{package}/dist-tags`          | List dist-tags             |
| `PUT /npm/-/package/{package}/dist-tags/{tag}`    | Set dist-tag               |
| `DELETE /npm/-/package/{package}/dist-tags/{tag}` | Delete dist-tag            |
| `GET /npm/{scope}/{name}`                         | Scoped package packument   |
| `PUT /npm/{scope}/{name}`                         | Scoped package publish     |
| `GET /npm/{scope}/{name}/-/{filename}`            | Scoped tarball download    |
| `GET /npm/{package}`                              | Unscoped package packument |
| `PUT /npm/{package}`                              | Unscoped package publish   |
| `GET /npm/{package}/-/{filename}`                 | Unscoped tarball download  |

### PyPI / pip — `/pypi` plus `/_/oidc/*`

| Route                                                            | Purpose                                                        |
| ---------------------------------------------------------------- | -------------------------------------------------------------- |
| `GET /_/oidc/audience`                                           | Return the audience string for trusted publishing              |
| `POST /_/oidc/mint-token`                                        | Exchange an external OIDC JWT for a short-lived Publaryn token |
| `GET /pypi/simple` and `GET /pypi/simple/`                       | Simple API root                                                |
| `GET /pypi/simple/{project}` and `GET /pypi/simple/{project}/`   | Project detail in the Simple API                               |
| `GET /pypi/files/{artifact_id}/{filename}`                       | Distribution download                                          |
| `POST /pypi/legacy` and `POST /pypi/legacy/`                     | Default legacy upload endpoint                                 |
| `POST /pypi/legacy/{repository_slug}` and trailing-slash variant | Repository-targeted legacy upload                              |

### Cargo — `/cargo/index` and `/cargo/api/v1`

#### Sparse index mount: `/cargo/index`

| Route                                | Purpose                        |
| ------------------------------------ | ------------------------------ |
| `GET /cargo/index/config.json`       | Cargo registry config          |
| `GET /cargo/index/1/{name}`          | 1-character crate index entry  |
| `GET /cargo/index/2/{name}`          | 2-character crate index entry  |
| `GET /cargo/index/3/{prefix}/{name}` | 3-character crate index entry  |
| `GET /cargo/index/{ab}/{cd}/{name}`  | 4+ character crate index entry |

#### Web API mount: `/cargo/api/v1`

| Route                                                | Purpose                  |
| ---------------------------------------------------- | ------------------------ |
| `PUT /cargo/api/v1/crates/new`                       | Publish a crate          |
| `DELETE /cargo/api/v1/crates/{name}/{version}/yank`  | Yank a version           |
| `PUT /cargo/api/v1/crates/{name}/{version}/unyank`   | Restore a yanked version |
| `GET /cargo/api/v1/crates/{name}/owners`             | List owners              |
| `PUT /cargo/api/v1/crates/{name}/owners`             | Add owners               |
| `DELETE /cargo/api/v1/crates/{name}/owners`          | Remove owners            |
| `GET /cargo/api/v1/crates`                           | Search crates            |
| `GET /cargo/api/v1/crates/{name}/{version}/download` | Download crate archive   |

### NuGet — `/nuget`

| Route                                                   | Purpose             |
| ------------------------------------------------------- | ------------------- |
| `GET /nuget/v3/index.json`                              | NuGet service index |
| `PUT /nuget/v2/package`                                 | Push package        |
| `DELETE /nuget/v2/package/{id}/{version}`               | Unlist package      |
| `POST /nuget/v2/package/{id}/{version}`                 | Relist package      |
| `GET /nuget/v3-flatcontainer/{id}/index.json`           | Version listing     |
| `GET /nuget/v3-flatcontainer/{id}/{version}/{filename}` | Package download    |
| `GET /nuget/v3/registration/{id}/index.json`            | Registration index  |
| `GET /nuget/v3/search`                                  | Search              |

### Maven — `/maven`

| Route                | Purpose                                                           |
| -------------------- | ----------------------------------------------------------------- |
| `GET /maven/{*path}` | Repository reads, including metadata and checksum materialization |
| `PUT /maven/{*path}` | Deploy-style artifact and metadata upload                         |

The Maven adapter uses path-aware handling for `maven-metadata.xml`, checksum
reads, and deploy-compatible uploads behind the shared catch-all route.

### RubyGems — `/rubygems`

| Route                                  | Purpose                 |
| -------------------------------------- | ----------------------- |
| `GET /rubygems/api/v1/gems/{name}`     | Gem metadata            |
| `GET /rubygems/api/v1/versions/{name}` | Version listing         |
| `GET /rubygems/gems/{filename}`        | Gem download            |
| `POST /rubygems/api/v1/gems`           | Push gem                |
| `DELETE /rubygems/api/v1/gems/yank`    | Yank gem                |
| `POST /rubygems/api/v1/api_key`        | API key echo/validation |

### Composer — `/composer`

| Route                                                             | Purpose                          |
| ----------------------------------------------------------------- | -------------------------------- |
| `GET /composer/packages.json`                                     | Composer packages index          |
| `GET /composer/p/{vendor}/{package}`                              | Package metadata                 |
| `GET /composer/files/{artifact_id}/{filename}`                    | Distribution download            |
| `PUT /composer/packages/{vendor}/{package}`                       | Publish package metadata/version |
| `DELETE /composer/packages/{vendor}/{package}/versions/{version}` | Yank a package version           |

### OCI — `/oci`

| Route                                                 | Purpose                                                                       |
| ----------------------------------------------------- | ----------------------------------------------------------------------------- |
| `GET /oci/v2/`                                        | OCI API probe                                                                 |
| `GET /oci/v2/_catalog`                                | Repository catalog                                                            |
| `GET, HEAD, PUT, POST, PATCH, DELETE /oci/v2/{*path}` | Distribution-spec dispatch for manifests, blobs, uploads, tags, and referrers |

## Related references

- [Publaryn 1.0 release contract](/1.0)
- [Publaryn release checklist](/release-checklist)
- [Publaryn ADR index](/adr/README)
