---
name: publaryn-contract-and-release-alignment
description: 'Keep Publaryn README, 1.0 contract, concept, ADR index, release checklist, and release-facing docs aligned with shipped behavior. Use when resolving docs/code drift, auditing release blockers, updating release notes, or deciding whether to change code or contract docs.'
argument-hint: 'Describe the drift, release concern, or contract area you are aligning.'
user-invocable: true
disable-model-invocation: false
---

# Publaryn Contract and Release Alignment

## Outcome

Keep Publaryn's advertised 1.0 surface truthful by aligning repository docs, release-facing expectations, and implementation evidence.

## When to Use

Use this skill when working on:

- docs and code drift
- 1.0 scope or release-gate checks
- release checklist gaps
- route-matrix mismatches
- release notes or support-compatibility updates
- deciding whether the right fix is code, docs, tests, or all three

## Primary Sources

Read these documents first:

- [README](../../../README.md)
- [docs/1.0.md](../../../docs/1.0.md)
- [docs/concept.md](../../../docs/concept.md)
- [docs/api-routes.md](../../../docs/api-routes.md)
- [docs/release-checklist.md](../../../docs/release-checklist.md)
- [docs/adr/README.md](../../../docs/adr/README.md)
- relevant ADRs under [docs/adr](../../../docs/adr/)
- [docs/releases/README.md](../../../docs/releases/README.md)
- release-facing pages under [docs/releases](../../../docs/releases)
- [CI workflow](../../../.github/workflows/ci.yml)
- [CONTRIBUTING.md](../../../CONTRIBUTING.md)

## Core Rules

- Do not let the public contract over-promise undocumented or unimplemented behavior.
- Do not silently downgrade the documented contract without strong evidence that the shipped behavior intentionally changed.
- Prefer contract alignment to the published 1.0 baseline when the implementation gap is small and the docs are clearly intentional.
- Prefer doc alignment to shipped behavior when the implementation is already established, tested, and intentionally broader or narrower than the stale docs.
- When changing release-facing behavior, update the matching docs and checklist references in the same slice when practical.

## Drift Categories to Check

For any release-facing slice, verify whether it changes or reveals drift in:

- supported ecosystems and baseline adapter scope
- route availability or documented URL prefixes
- visibility and search semantics
- auth, scope, and ownership rules
- release lifecycle and artifact behavior
- operator runbooks and async recovery expectations
- validation commands, CI gates, and release criteria

## Procedure

1. Identify the concrete drift or release-facing claim.
2. Gather evidence from code, tests, docs, and CI configuration.
3. Decide which source is stale: implementation, docs, or both.
4. Make the smallest truthful alignment change that restores consistency.
5. If multiple docs reference the same contract area, update all relevant surfaces in the same slice where practical.
6. Re-check the release checklist and route matrix after the change.
7. Call out any intentionally deferred follow-up work instead of implying hidden completeness.

## Decision Hints

- If `README.md`, `docs/1.0.md`, `docs/concept.md`, and `docs/api-routes.md` disagree, treat that as a release blocker until the inconsistency is resolved.
- If CI or the release checklist names validations that no longer match the repository, update the docs or workflow references so operators are not misled.
- If a prompt, instruction, or skill references stale code anchors, update those developer-facing assets too so future autonomous work starts from the right files.
- If a route is documented but missing, prefer a code fix only when the missing surface is small, already adjacent, and testable; otherwise reduce the contract claim explicitly.

## Completion Checks

Before finishing, verify that:

- the affected contract docs and code now tell the same story
- any release-facing validation or support claims still match CI and repo reality
- follow-up gaps are explicitly documented rather than hidden
- the user can understand whether the slice changed implementation, documentation, or both
