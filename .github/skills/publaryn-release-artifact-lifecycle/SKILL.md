---
name: publaryn-release-artifact-lifecycle
description: 'Implement or review Publaryn release creation, artifact handling, publication, pre-publication visibility, and derived follow-up work. Use when working on quarantine-first publish flows, artifact upload/download, native adapter publish logic, release-state transitions, search reindexing, scanning dispatch, or release-facing operator behavior.'
argument-hint: 'Describe the release, artifact, or publication workflow you are changing.'
user-invocable: true
disable-model-invocation: false
---

# Publaryn Release and Artifact Lifecycle

## Outcome

Implement or review release and artifact behavior that stays consistent with Publaryn's quarantine-first publication model, immutability guarantees, and horizontally scalable storage architecture.

## When to Use

Use this skill when the task involves:

- creating releases or changing release-state transitions
- uploading, storing, serving, or validating release artifacts
- native adapter publish flows that map onto the shared release and artifact model
- public versus privileged visibility for pre-publication releases and files
- search reindexing, scan dispatch, or other async follow-up work tied to release publication

## Primary Sources

Read these documents first:

- [README](../../../README.md)
- [docs/1.0.md](../../../docs/1.0.md)
- [docs/release-checklist.md](../../../docs/release-checklist.md)
- [ADR 0009: quarantine-first release publication and artifact storage](../../../docs/adr/0009-control-plane-release-publication-and-artifact-storage.md)
- [ADR 0018: rate limiting and background job queue](../../../docs/adr/0018-rate-limiting-and-background-job-queue.md)
- [ADR 0007: read visibility semantics](../../../docs/adr/0007-package-and-repository-read-visibility.md) when the change affects direct release reads or artifact downloads
- [docs/operator/job-queue-recovery.md](../../../docs/operator/job-queue-recovery.md) when the change affects async follow-up work, queue visibility, or recovery guidance
- the ecosystem ADR for the adapter you are touching, such as [ADR 0010](../../../docs/adr/0010-npm-registry-protocol-adapter.md), [ADR 0014](../../../docs/adr/0014-pypi-legacy-upload-bridge.md), [ADR 0016](../../../docs/adr/0016-cargo-alternative-registry-adapter.md), or [ADR 0017](../../../docs/adr/0017-nuget-v3-protocol-adapter.md)

## Code Anchors

Inspect these files early:

- [crates/core/src/domain/release.rs](../../../crates/core/src/domain/release.rs) — release status model and allowed state meanings
- [crates/core/src/domain/artifact.rs](../../../crates/core/src/domain/artifact.rs) — artifact metadata, checksums, and storage-facing identity
- [crates/api/src/storage.rs](../../../crates/api/src/storage.rs) — artifact store abstraction and durable object-storage behavior
- [crates/api/src/routes/packages.rs](../../../crates/api/src/routes/packages.rs) — shared release creation, artifact upload, publication, and download visibility checks
- [crates/api/src/routes/search.rs](../../../crates/api/src/routes/search.rs) — derived search-document refresh and latest-version visibility after publication
- [crates/workers/src/queue.rs](../../../crates/workers/src/queue.rs) — background job model for reindexing, scanning, and other async follow-up work
- [frontend/src/utils/releases.ts](../../../frontend/src/utils/releases.ts) — optional client-side release-readiness and action-availability logic when UI behavior must stay aligned

## Core Invariants

- Releases begin in `quarantine` unless an ecosystem ADR explicitly documents a narrower compatibility exception.
- Artifact bytes must be durably stored and metadata-recorded before a release becomes publicly visible.
- Artifact uploads are immutable and retry-safe; existing files are never overwritten.
- Repeated upload attempts for the same logical file should be idempotent where the shared model or adapter ADR says they are.
- Public direct reads are limited to allowed published-state releases; `quarantine` and `scanning` remain privileged-only.
- Search and other derived views may lag, but metadata durability and publication correctness must not depend on search success.
- Long-running follow-up work should use the existing PostgreSQL-backed job queue and replica-safe shared state rather than process-local background state.
- Release-facing code changes must keep the 1.0 contract, release checklist, and operator guidance truthful when visible behavior changes.

## Ecosystem-Specific Nuance

- npm, Cargo, and generic control-plane flows follow an explicit create → upload → publish shape over the shared model.
- PyPI legacy upload is intentionally narrower: the first uploaded file may create and publish the release immediately after durability is established, and additional immutable files may be appended to an already published version as documented in [ADR 0014](../../../docs/adr/0014-pypi-legacy-upload-bridge.md).
- NuGet unlist/relist behavior changes listing visibility without redefining the overall artifact immutability model.
- OCI manifest, blob, tag, referrer, and cleanup flows must still preserve artifact durability, pre-publication visibility boundaries, and queue-backed cleanup semantics rather than inventing adapter-local shortcuts.

## Procedure

1. Decide whether the change belongs in the shared control-plane workflow, an ecosystem adapter, or both.
2. Confirm the expected release states and visibility rules before editing code.
3. Store artifact bytes through the existing storage abstraction or adapter app-state bridge rather than bypassing it.
4. Preserve checksum computation, filename uniqueness, and retry-safe behavior.
5. Keep pre-publication artifacts and metadata behind the correct privileged read checks.
6. Reuse or enqueue derived follow-up work such as search reindexing or scanning instead of coupling it directly to request success when the existing design treats it as asynchronous.
7. Add or extend tests for the concrete lifecycle behavior you changed.
8. Update release-facing docs or operator guidance when the externally visible workflow changed.

## Out of Scope

This skill does not cover:

- bypassing quarantine or artifact durability requirements for convenience
- mutable artifact replacement or silent overwrite semantics
- replacing the shared storage model with adapter-local persistence
- inventing a new queue or rate-limit architecture without an explicit design decision

## Completion Checks

Before finishing, verify that:

- release visibility matches the documented status rules
- artifact uploads remain immutable and retry-safe
- any async follow-up work still uses the documented shared infrastructure
- release-facing docs, checklist references, or runbooks were updated if the workflow surface changed
- tests or focused checks cover the changed lifecycle behavior
