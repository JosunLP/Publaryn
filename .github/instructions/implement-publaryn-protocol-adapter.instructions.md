---
name: 'Implement Publaryn Protocol Adapter'
description: 'Use when implementing, extending, or debugging one native protocol-adapter slice in the Publaryn repository.'
---

# Implement one native protocol-adapter slice for Publaryn

Use this instruction when the user asks to implement, extend, harden, or debug one ecosystem adapter in this repository.

- Treat [README](../../README.md), [Concept](../../docs/concept.md), and the relevant ecosystem ADRs under [docs/adr](../../docs/adr/) as the source of truth for product intent and protocol direction.
- Keep source code, docs, tests, UI copy, and commit-ready output in English unless the user explicitly asks to localize content.
- Default to exactly one coherent adapter slice per run.
- Preserve native client compatibility and the existing trait-bridge architecture before considering broader refactors.
- Before editing, inspect the active adapter crate, its bridge module in `crates/api/src`, and its router mount in `crates/api/src/router.rs`.

## Adapter architecture rules

- Follow the pattern established by [ADR 0010](../../docs/adr/0010-npm-registry-protocol-adapter.md): each adapter crate defines an `XxxAppState` trait, the API crate implements that trait in a thin bridge module, and the main router mounts the adapter under a distinct prefix.
- Keep bridge modules thin and stateless. Business rules belong in shared domain or adapter code, not inside glue code.
- Reuse shared package, release, artifact, visibility, audit, and token infrastructure instead of introducing ecosystem-specific parallel models unless an ADR explicitly requires dedicated metadata storage.
- Preserve horizontal-scaling safety: avoid process-local mutable state, sticky sessions, or single-node assumptions in publish and private-read flows.

## Ecosystem-specific source map

Read the docs that match the adapter surface you touch:

- npm / Bun: [ADR 0010](../../docs/adr/0010-npm-registry-protocol-adapter.md)
- PyPI read surface: [ADR 0013](../../docs/adr/0013-pypi-simple-api-read-surface.md)
- PyPI uploads: [ADR 0014](../../docs/adr/0014-pypi-legacy-upload-bridge.md)
- PyPI trusted publishing: [ADR 0015](../../docs/adr/0015-pypi-trusted-publishing.md)
- Cargo sparse index and publish: [ADR 0016](../../docs/adr/0016-cargo-alternative-registry-adapter.md)
- NuGet V3: [ADR 0017](../../docs/adr/0017-nuget-v3-protocol-adapter.md)
- Shared read visibility: [ADR 0007](../../docs/adr/0007-package-and-repository-read-visibility.md)
- Shared identity and ownership rules: [ADR 0001](../../docs/adr/0001-control-plane-request-authentication.md), [ADR 0008](../../docs/adr/0008-control-plane-package-creation.md), and [ADR 0012](../../docs/adr/0012-team-package-governance.md)
- Shared release/artifact workflow and async infrastructure: [ADR 0009](../../docs/adr/0009-control-plane-release-publication-and-artifact-storage.md) and [ADR 0018](../../docs/adr/0018-rate-limiting-and-background-job-queue.md)

## Concrete code anchors to inspect early

- [crates/core/src/domain/namespace.rs](../../crates/core/src/domain/namespace.rs) for ecosystem identity and normalization hooks
- [crates/api/src/router.rs](../../crates/api/src/router.rs) for mount-point conventions
- [crates/api/src/request_auth.rs](../../crates/api/src/request_auth.rs) for bearer auth, ownership checks, and delegated package permissions
- [crates/api/src/routes/packages.rs](../../crates/api/src/routes/packages.rs) for shared package, release, artifact, and publication helpers
- representative adapter trait/router modules such as:
  - [crates/adapters/npm/src/routes.rs](../../crates/adapters/npm/src/routes.rs)
  - [crates/api/src/npm_bridge.rs](../../crates/api/src/npm_bridge.rs)
  - [crates/api/src/pypi_bridge.rs](../../crates/api/src/pypi_bridge.rs)
  - [crates/api/src/cargo_bridge.rs](../../crates/api/src/cargo_bridge.rs)
  - [crates/api/src/nuget_bridge.rs](../../crates/api/src/nuget_bridge.rs)

## Workflow

1. Read [README](../../README.md), [Concept](../../docs/concept.md), and the relevant adapter ADRs before changing code.
2. Inspect the current implementation to determine which endpoints, wire formats, auth behaviors, or metadata paths already exist.
3. Choose exactly one adapter slice that is:
   - explicitly aligned with the docs
   - adjacent to existing code
   - small enough to complete safely in one pass
   - testable with targeted protocol checks
4. Before making changes, show a concise checkbox todo list.
5. Implement incrementally and keep the adapter-specific code, bridge code, and shared-domain changes narrowly scoped.
6. Add or extend targeted tests in the touched adapter crate or API integration harness.
7. Run the most relevant tests/checks and fix issues before finishing.
8. Update docs only when the visible protocol surface or operator guidance materially changes.
9. Stop after one adapter slice and recommend the best next protocol follow-up.

## Selection heuristics

- Prefer already-mounted ecosystems over speculative new ones.
- Prefer real client-facing protocol completeness over broad internal refactors.
- Prefer documented follow-up gaps over aesthetic cleanup.
- If multiple slices are viable, choose the smallest one that unlocks or hardens a real native workflow.

## Common guardrails

- Do not trust caller-supplied owner identifiers or package ownership fields.
- Do not expose `quarantine` or `scanning` releases publicly unless the relevant ADR explicitly says otherwise.
- Do not invent adapter-local authentication schemes when shared JWT/API-token mechanisms already exist.
- Preserve ecosystem-specific error formats, normalization rules, and path semantics.
- Keep protocol behavior replica-safe and retry-safe.

## Good candidate slices if the evidence still supports them

- complete a missing adapter endpoint that is already documented
- align adapter visibility behavior with [ADR 0007](../../docs/adr/0007-package-and-repository-read-visibility.md)
- harden protocol publish or private-read authorization against the newer delegated-team model
- add protocol-specific regression tests for native publish, install, search, yank, unyank, or download paths
- close a documented metadata or serialization gap that blocks common client tooling

## Required ending format

End with:

- updated todo list
- selected adapter slice and why
- what changed and why
- files changed with one-line purpose
- tests/checks run and outcomes
- the best next adapter follow-up slice

This instruction is intentionally adapter-scoped. Use the broader repository workflow only when the user asks to choose among unrelated roadmap slices.
