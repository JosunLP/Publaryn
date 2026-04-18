---
name: 'Implement Publaryn Protocol Adapter'
description: 'Choose and deliver one native protocol-adapter slice for Publaryn using the existing trait-bridge pattern, ecosystem ADRs, and shared package domain.'
argument-hint: 'Optional ecosystem, protocol surface, or constraint (for example: PyPI legacy upload hardening, Cargo sparse index caching, NuGet relist flow, npm dist-tags tests)'
agent: 'agent'
---

You are implementing one native protocol-adapter slice in the Publaryn repository.

Run-specific guidance: ${input:focus:Optional ecosystem, protocol surface, priority bias, or extra constraints}

Also apply the reusable [Implement Publaryn Protocol Adapter instruction](../instructions/implement-publaryn-protocol-adapter.instructions.md) for the shared workflow and guardrails.

Treat the following as the source of truth for product intent and adapter architecture:

- [README](../../README.md)
- [Concept](../../docs/concept.md)
- relevant adapter ADRs under [docs/adr](../../docs/adr/), especially:
  - [ADR 0010: npm registry protocol adapter](../../docs/adr/0010-npm-registry-protocol-adapter.md)
  - [ADR 0013: read-only PyPI Simple API](../../docs/adr/0013-pypi-simple-api-read-surface.md)
  - [ADR 0014: PyPI legacy upload bridge](../../docs/adr/0014-pypi-legacy-upload-bridge.md)
  - [ADR 0015: PyPI trusted publishing](../../docs/adr/0015-pypi-trusted-publishing.md)
  - [ADR 0016: Cargo alternative registry adapter](../../docs/adr/0016-cargo-alternative-registry-adapter.md)
  - [ADR 0017: NuGet V3 protocol adapter](../../docs/adr/0017-nuget-v3-protocol-adapter.md)
- when your slice touches identity, ownership derivation, scopes, or delegated package permissions, also apply [Publaryn Control-plane Auth and Ownership](../skills/publaryn-control-plane-auth-ownership/SKILL.md)
- when your slice touches quarantine-first publication, artifact upload/download behavior, or release visibility, also apply [Publaryn Release and Artifact Lifecycle](../skills/publaryn-release-artifact-lifecycle/SKILL.md)

Operating defaults:

- Priority bias: native client compatibility first
- Preferred layer: end-to-end through the adapter boundary
- Max scope: exactly one coherent adapter slice
- Language, copy, docs, tests, and commit-ready output: English only

Your goal is to deliver exactly one protocol-adapter slice that is already supported by the documented roadmap, aligned with the existing adapter architecture, and testable end to end.

## Workflow

1. Read [README](../../README.md), [Concept](../../docs/concept.md), and the ADRs relevant to the adapter you choose.
2. Inspect the current adapter crate under [crates/adapters](../../crates/adapters/) together with its API bridge and router mount.
3. Choose one adapter slice that improves protocol completeness, correctness, or native-client interoperability without starting a broad refactor.
4. Before making changes, show a concise checkbox todo list.
5. Implement incrementally and preserve the established `XxxAppState` trait + API bridge pattern.
6. Add or extend targeted tests for the changed protocol behavior.
7. Run the most relevant checks for the touched adapter crate, API crate, or frontend utility code.
8. Update docs only if the visible protocol surface or usage guidance materially changes.
9. Stop after one adapter slice. Recommend the best next adapter follow-up instead of starting a second slice.

## Selection heuristics

- Prefer extending already-mounted adapter crates over inventing a brand-new ecosystem.
- Prefer protocol correctness and native-client compatibility over internal abstraction work.
- Prefer documented follow-up work or incomplete endpoints over broad cleanup.
- If several slices are viable, choose the smallest one that materially improves a real native workflow such as publish, install, search, yank, unyank, download, or ownership management.

## Repository-specific hints to verify before choosing

- [ADR 0010](../../docs/adr/0010-npm-registry-protocol-adapter.md) establishes the thin-bridge pattern via `XxxAppState` traits and `Router::nest` mount points.
- [ADR 0013](../../docs/adr/0013-pypi-simple-api-read-surface.md), [ADR 0014](../../docs/adr/0014-pypi-legacy-upload-bridge.md), and [ADR 0015](../../docs/adr/0015-pypi-trusted-publishing.md) split PyPI work into read, upload, and trusted-publishing slices.
- [ADR 0016](../../docs/adr/0016-cargo-alternative-registry-adapter.md) shows how sparse index serving and write APIs still reuse the shared release/artifact model.
- [ADR 0017](../../docs/adr/0017-nuget-v3-protocol-adapter.md) keeps NuGet-specific metadata in adapter-adjacent schema while preserving the same bridge structure.
- The representative adapter anchors are [crates/adapters/npm/src/routes.rs](../../crates/adapters/npm/src/routes.rs), [crates/api/src/npm_bridge.rs](../../crates/api/src/npm_bridge.rs), and [crates/api/src/router.rs](../../crates/api/src/router.rs).

## Good candidate slices if the evidence still supports them

- extend an existing adapter with a missing publish, yank, unyank, or relist endpoint
- align adapter-private reads with the shared visibility model
- harden protocol-specific authentication or token-surface restrictions
- add targeted adapter integration tests for a documented happy path or regression
- complete a small metadata or error-format gap that blocks standard client tooling

## Implementation constraints

- Reuse the shared package, release, artifact, visibility, and audit models whenever possible.
- Keep adapter crates independently testable by preserving the trait-bridge split.
- Do not introduce process-local session or cache state into publish-critical flows.
- Respect ecosystem-specific normalization and wire-format rules instead of forcing a fake universal protocol shape.
- If the docs and code disagree, explain which source you followed and why.

## Acceptance bar

- the chosen slice meaningfully improves a real native client or protocol workflow
- the adapter still follows the documented trait-bridge architecture
- tests cover the new behavior or regression
- docs remain aligned with the actual protocol surface

## Required ending format

End with:

- updated todo list
- selected adapter slice and why
- what changed and why
- files changed with one-line purpose
- tests/checks run and outcomes
- the best next adapter follow-up slice
