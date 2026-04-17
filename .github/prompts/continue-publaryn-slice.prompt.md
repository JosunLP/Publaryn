---
name: 'Continue Publaryn Slice'
description: 'Choose and deliver the next roadmap-aligned, PR-sized implementation slice for Publaryn. Use when extending existing surfaces safely without starting a broad refactor.'
argument-hint: 'Optional focus, layer, or constraint (for example: org workspace frontend, delegated package access UI, API-only, tests-first)'
agent: 'agent'
---

You are continuing implementation in the Publaryn repository.

Run-specific guidance: ${input:focus:Optional focus area, priority bias, or extra constraints}

Also apply the reusable [Continue Publaryn Slice instruction](../instructions/continue-publaryn-slice.instructions.md) for the shared workflow and selection rules.

Treat the following as the source of truth for product intent and architectural direction:

- [README](../../README.md)
- [Concept](../../docs/concept.md)
- relevant ADRs under [docs/adr](../../docs/adr/)
- when working on organization governance, especially [ADR 0012: Team-based delegated package governance for organization-owned packages](../../docs/adr/0012-team-package-governance.md)

Operating defaults:

- Priority bias: auto
- Preferred layer: end-to-end
- Max scope: exactly one coherent vertical slice
- Language, copy, docs, tests, and commit-ready output: English only

Your goal is to deliver exactly one coherent, PR-sized slice that is already supported by the documented roadmap and adjacent to the current codebase.

## Workflow

1. Read [README](../../README.md), [Concept](../../docs/concept.md), and the ADRs relevant to the area you choose.
2. Inspect the current implementation to determine what is already shipped, what is partially implemented, and what is still missing.
3. Choose the highest-value next slice that is:
   - clearly aligned with the docs
   - already partially enabled by existing code or APIs
   - small enough to complete safely in one pass
   - testable end-to-end
4. Before making changes, show a concise checkbox todo list.
5. Implement incrementally.
6. Add or extend targeted tests for the changed behavior.
7. Run the relevant tests/checks and fix issues before finishing.
8. Update docs only where the implementation surface or visible behavior materially changes.
9. Stop after one slice. Do not begin a second implementation slice in the same run; recommend the best next slice instead.

## Selection heuristics

- Prefer completing existing surfaces over inventing brand-new ones.
- Prefer end-to-end value over broad refactors.
- Prefer features explicitly called out as “next” in docs, ADR follow-up sections, or nearby code comments.
- Avoid speculative abstractions unless they are required to finish the chosen slice.

## Prioritization order

1. Frontend or API completion for already-landed backend capabilities.
2. Protocol completeness for ecosystem adapters that already exist.
3. Test and documentation hardening for recently implemented slices.

## Repository-specific hints to verify before choosing

- [Concept](../../docs/concept.md) says the immediate frontend goal is to expand the governance baseline into dedicated organization workspaces, then into teams, package access, audit, and security surfaces.
- [ADR 0012](../../docs/adr/0012-team-package-governance.md) explicitly lists surfacing team package access in the frontend as follow-up work.
- [Organization workspace page](../../frontend/src/pages/org-detail.ts) is the current frontend hub for org governance.
- [Organization API client](../../frontend/src/api/orgs.ts) is the frontend wrapper layer to inspect first.
- [Organization routes](../../crates/api/src/routes/orgs.rs) already expose organization ownership transfer, team CRUD, team membership, and team package-access endpoints.

## Default decision rule

If multiple slices are viable, choose the smallest high-value slice that extends the organization workspace using already-existing organization and team APIs.

## Good candidate slices if the evidence still supports them

- add frontend support for team CRUD in the organization workspace
- add team member management UI and any missing client wrappers
- add delegated package-access management UI for org-owned packages
- add organization ownership-transfer UI
- add targeted tests for delegated authorization paths if the UI scope is too large for one safe pass

## Implementation constraints

- Reuse established patterns in [frontend/src/pages](../../frontend/src/pages), [frontend/src/api](../../frontend/src/api), and [crates/api/src/routes](../../crates/api/src/routes).
- Keep changes narrow and reviewable.
- Preserve existing behavior unless the selected slice intentionally extends it.
- Keep user-facing copy concise and consistent with the existing UI tone.
- If docs and code disagree, explain which one you followed and why.

## Acceptance bar

- the chosen slice is visibly usable or meaningfully complete
- tests cover the new behavior
- docs remain aligned
- the result clearly moves the repo forward according to the source documents

## Required ending format

End with:

- updated todo list
- what you changed and why
- files changed with one-line purpose
- tests/checks run and outcomes
- the best next follow-up slice
