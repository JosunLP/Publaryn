# ADR 0017: NuGet V3 Protocol Adapter

**Status:** Accepted
**Date:** 2025-07-27
**Decision Makers:** Architecture Team

## Context

Publaryn aims to be a multi-ecosystem package registry that supports NuGet (the .NET package ecosystem). NuGet clients (dotnet CLI, Visual Studio Package Manager, NuGet.exe) communicate with package sources via the NuGet V3 Server API, a RESTful protocol documented by Microsoft.

We need to support:

- Publishing packages via `dotnet nuget push`
- Restoring packages via `dotnet restore` / NuGet.exe
- Searching packages via the NuGet search surface
- Unlisting and relisting versions

## Decision

### Protocol Surface

We implement the following NuGet V3 resources:

| Resource             | Type ID                      | Route                                     |
| -------------------- | ---------------------------- | ----------------------------------------- |
| Service Index        | —                            | `GET /nuget/v3/index.json`                |
| PackagePublish       | `PackagePublish/2.0.0`       | `PUT /nuget/v2/package`                   |
| Unlist               | —                            | `DELETE /nuget/v2/package/{id}/{version}` |
| Relist               | —                            | `POST /nuget/v2/package/{id}/{version}`   |
| PackageBaseAddress   | `PackageBaseAddress/3.0.0`   | `GET /nuget/v3-flatcontainer/…`           |
| RegistrationsBaseUrl | `RegistrationsBaseUrl/3.6.0` | `GET /nuget/v3/registration/…`            |
| SearchQueryService   | `SearchQueryService/3.5.0`   | `GET /nuget/v3/search`                    |

### Package ID and Version Normalization

NuGet package IDs are case-insensitive. We normalize IDs to lowercase for storage and lookup, while preserving the original casing in metadata. Version normalization follows NuGet conventions: strip leading zeros from numeric segments, remove a trailing `.0` fourth segment, strip build metadata, and lowercase pre-release tags.

### Authentication

NuGet clients send API keys via the `X-NuGet-ApiKey` HTTP header. We also accept `Authorization: Bearer` as a fallback. Both use the same token resolution logic as other adapters: `pub_*`-prefixed tokens are looked up by hash in the `tokens` table; other values are validated as JWTs. OIDC-derived tokens are rejected.

### Publish Auto-Creation

First-time pushes auto-create the package record, assigned to the pusher's first available repository. This mirrors the npm adapter behavior and avoids requiring a separate package creation API step.

### Unlist vs. Delete

NuGet convention is that DELETE unlists rather than hard-deletes a package version. Unlisted packages remain downloadable by exact version but are hidden from search results and default version listings. The `is_listed` column in `nuget_release_metadata` controls listing visibility; `is_yanked` on the release is also set for consistency with the cross-ecosystem domain model.

### NuGet-Specific Metadata

A dedicated `nuget_release_metadata` table stores per-release NuGet-specific fields (authors, title, icon URL, license expression, dependency groups, package types, etc.) following the same pattern as `cargo_release_metadata`.

### .nupkg and .nuspec Storage

The `.nupkg` archive is stored as an artifact. The `.nuspec` XML is additionally stored separately for efficient serving of the flat container nuspec endpoint without requiring archive extraction at read time.

### Architecture

The adapter follows the established trait-bridge pattern:

1. **`publaryn-adapter-nuget`** crate defines `NuGetAppState` trait and all handlers
2. **`publaryn-api`** implements `NuGetAppState` for `AppState` in `nuget_bridge.rs`
3. Routes are mounted under `/nuget` via `Router::nest`

## Consequences

### Positive

- .NET developers can use `dotnet nuget push` and `dotnet restore` directly against Publaryn
- Protocol compliance with NuGet V3 service index enables automatic NuGet client discovery
- Trait-bridge pattern keeps the adapter decoupled from the API crate

### Negative

- The NuGet V3 protocol has some legacy quirks (publish endpoint lives under `/v2/`, multipart form upload) that add complexity
- Registration metadata responses are always inline (no pagination) which may become expensive for packages with hundreds of versions

### Not Implemented (Future Work)

- Symbol packages (`.snupkg`) — can be added by extending the publish handler
- Package content resource (README rendering)
- Autocomplete / package versions resource
- Catalog resource (change tracking)
- Signed package verification
