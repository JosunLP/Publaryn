# Product Guide

This section explains what Publaryn is, who it is for, and what the 1.0 release
actually promises.

## Positioning

Publaryn is a self-hostable, security-first package registry platform that
speaks the native protocols of multiple ecosystems while providing one unified
management surface for governance, release handling, search, and security.

The 1.0 baseline is intentionally opinionated:

- native client compatibility is mandatory
- organization governance is a first-class concern
- publish flows are quarantine-first and artifact-immutable
- visibility rules must be consistent across the API, search, and adapters

## Supported ecosystems in 1.0

| Ecosystem  | Mount path                      | Baseline                                                                   |
| ---------- | ------------------------------- | -------------------------------------------------------------------------- |
| npm / Bun  | `/npm`                          | packument reads, tarball download, search, publish, dist-tags              |
| PyPI / pip | `/pypi` plus `/_/oidc/*`        | Simple API, file download, legacy upload, trusted publishing               |
| Cargo      | `/cargo/index`, `/cargo/api/v1` | sparse index, publish, search, yank, unyank, download                      |
| NuGet      | `/nuget`                        | service index, push, flat container, registration, search                  |
| Maven      | `/maven`                        | repository reads, metadata generation, checksum reads, deploy-style upload |
| RubyGems   | `/rubygems`                     | metadata reads, version listing, gem download, push, yank                  |
| Composer   | `/composer`                     | packages index, metadata, dist download, publish, yank                     |
| OCI        | `/oci`                          | catalog, manifests, blobs, uploads, tags, referrers, deletes               |

## What 1.0 includes

- multi-ecosystem hosted package management
- organization workspaces, invitations, teams, and delegated access
- trusted publishing for PyPI and scoped token-based publishing elsewhere
- security findings, background jobs, and operator queue visibility
- a web portal for discovery, settings, MFA, tokens, and organization workflows

## What 1.0 intentionally does not include

- proxy, mirror, and virtual repositories
- Maven snapshot repositories and generic promotion pipelines
- SSO, SAML, and SCIM
- billing and commercial tiering workflows
- federation, regional replication, and air-gapped sync
- deep attestation, signature UX, and broad Sigstore workflows

## Where to go next

- [Read the 1.0 release contract](/1.0)
- [Open the API and adapter route reference](/api-routes)
- [Architecture overview](/architecture/README)
- [Read the full concept document](/concept)
