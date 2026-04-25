---
name: publaryn-autonomous-roadmap-delivery
description: 'Choose, implement, validate, and chain PR-sized Publaryn roadmap slices autonomously toward the 1.0 contract. Use when continuing Publaryn, clearing release blockers, aligning docs and code, or advancing the next highest-value slice without waiting for manual restarts.'
argument-hint: 'Describe any focus, stop condition, or constraints for the autonomous run.'
user-invocable: true
disable-model-invocation: false
---

# Publaryn Autonomous Roadmap Delivery

## Outcome

Advance Publaryn through a sequence of validated, reviewable slices that move the repository closer to the published 1.0 contract without waiting for human restarts between obvious next steps.

## When to Use

Use this skill when the user asks to:

- continue Publaryn autonomously
- keep going until done or blocked
- deliver the next roadmap slice repeatedly
- clear release blockers or 1.0 gaps
- align docs, code, tests, and release expectations while continuing implementation

## Primary Sources

Read these sources first and re-check the relevant ones before each new slice:

- [README](../../../README.md)
- [docs/1.0.md](../../../docs/1.0.md)
- [docs/concept.md](../../../docs/concept.md)
- [docs/api-routes.md](../../../docs/api-routes.md)
- [docs/release-checklist.md](../../../docs/release-checklist.md)
- [docs/adr/README.md](../../../docs/adr/README.md)
- relevant ADRs under [docs/adr](../../../docs/adr/)
- [CONTRIBUTING.md](../../../CONTRIBUTING.md)
- [CI workflow](../../../.github/workflows/ci.yml)

## Decision Framework

Treat the repository in this order of truth:

1. ADRs for hard architectural and security invariants
2. current code and tests for shipped behavior
3. README, `docs/1.0.md`, `docs/concept.md`, `docs/api-routes.md`, and the release checklist for product direction and 1.0 expectations

If those sources disagree, explicitly decide whether the correct fix is:

- implementation alignment to the documented contract, or
- contract and docs alignment to shipped behavior

Do not leave the mismatch implicit.

## Prioritization Order

Choose work in this order unless the user overrides it:

1. failing tests, broken builds, or CI-gate regressions
2. code and docs drift that affects the advertised 1.0 contract
3. frontend or API completion for already-landed backend capabilities
4. protocol completeness or correctness for already-mounted adapters
5. security, ownership, visibility, audit, or release-lifecycle hardening
6. missing regression coverage, docs, or runbook updates required by the release checklist
7. only then: broader refactors needed to unlock one of the above

## Slice Rules

Every slice must be:

- coherent and PR-sized
- clearly adjacent to existing code
- aligned with the roadmap, docs, or ADR follow-up direction
- safely completable in one pass
- testable with focused validation

Prefer end-to-end completion of already-exposed surfaces over speculative new subsystems.

## Non-negotiable Publaryn Invariants

- Never trust caller-supplied identity or ownership fields on mutable paths.
- Keep API and worker behavior stateless and horizontally safe.
- Preserve quarantine-first publication and artifact immutability.
- Do not expose `quarantine` or `scanning` releases publicly unless an accepted ADR explicitly documents a narrow exception.
- Keep package and repository visibility semantics consistent across API, search, frontend, and native adapters.
- Preserve the thin adapter-crate plus API-bridge architecture for protocol work.
- Reuse existing shared domain behavior before inventing parallel abstractions.

## Required Workflow for Each Slice

1. Re-read the highest-value docs and ADRs for the candidate area.
2. Inspect the current implementation and nearby tests before editing.
3. Show a concise checkbox todo list.
4. Implement incrementally.
5. Add or extend targeted tests.
6. Run the narrowest relevant validation first, then expand to broader checks when the slice warrants it.
7. Fix follow-up issues before declaring the slice complete.
8. Update docs, release notes, or runbooks when visible behavior or release-facing expectations changed.
9. Summarize evidence that the slice is complete.
10. Immediately continue with the next best slice unless a real blocker applies.

## Repo-specific Anchors to Verify Early

- [crates/api/src/router.rs](../../../crates/api/src/router.rs)
- [crates/api/src/request_auth.rs](../../../crates/api/src/request_auth.rs)
- [crates/api/src/routes/orgs.rs](../../../crates/api/src/routes/orgs.rs)
- [crates/api/src/routes/packages.rs](../../../crates/api/src/routes/packages.rs)
- [crates/api/src/routes/search.rs](../../../crates/api/src/routes/search.rs)
- [frontend/src/routes/orgs/[slug]/+page.svelte](../../../frontend/src/routes/orgs/[slug]/+page.svelte)
- [frontend/src/routes/orgs/[slug]/teams/[team_slug]/+page.svelte](../../../frontend/src/routes/orgs/[slug]/teams/[team_slug]/+page.svelte)
- [frontend/src/routes/packages/[ecosystem]/[name]/+page.svelte](../../../frontend/src/routes/packages/[ecosystem]/[name]/+page.svelte)
- [frontend/src/routes/packages/[ecosystem]/[name]/versions/[version]/+page.svelte](../../../frontend/src/routes/packages/[ecosystem]/[name]/versions/[version]/+page.svelte)
- [frontend/src/api/orgs.ts](../../../frontend/src/api/orgs.ts)
- [frontend/src/api/packages.ts](../../../frontend/src/api/packages.ts)
- [crates/api/tests/integration_tests.rs](../../../crates/api/tests/integration_tests.rs)
- [crates/auth/tests/auth_tests.rs](../../../crates/auth/tests/auth_tests.rs)
- [frontend/tests](../../../frontend/tests)

## Validation Baseline

Use the relevant subset based on the touched surface, and escalate when justified.

Rust:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test -p publaryn-core`
- `cargo test -p publaryn-auth --lib`
- `cargo test -p publaryn-api --lib`
- `cargo test -p publaryn-api --test integration_tests`
- `cargo test -p publaryn-auth --test auth_tests`

Frontend:

- `bun install --frozen-lockfile`
- `bun run typecheck`
- `bun test`
- `bun run build`

Release-facing changes:

- build the docs site when docs changed
- use the Docker smoke build when runtime packaging or release-facing behavior changed

## Web Research Rule

If repository docs, ADRs, code, and tests do not fully answer a protocol, framework, or toolchain question, consult authoritative ecosystem or vendor documentation before deciding behavior. Prefer official specs and docs over blogs. Briefly cite the source that resolved the ambiguity.

## Stop Conditions

Stop only when one of these is true:

- a real product or architecture decision is required
- credentials, services, permissions, or external dependencies are missing
- further work would become speculative or unsafe
- the user-provided stop condition has been reached
- context saturation makes additional autonomous work low-confidence

## Completion Checks

Before finishing a run or pausing for a blocker, verify that:

- the current slice is actually complete and validated
- the todo list is fully updated
- any docs and code drift discovered during the slice was resolved or explicitly documented
- the best next slice is identified
- the user receives a clear summary of evidence, not just a claim of completion
