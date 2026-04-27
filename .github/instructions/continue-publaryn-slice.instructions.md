---
name: 'Continue Publaryn Slice'
description: 'Use when the task is to continue Publaryn by selecting and delivering one roadmap-aligned, PR-sized implementation slice.'
---

# Continue Publaryn with one roadmap-aligned slice

Use this instruction when the user asks to continue implementation, choose the next slice, or work autonomously on roadmap delivery in this repository.

- Treat [README](../../README.md), [Concept](../../docs/concept.md), and the relevant ADRs under [docs/adr](../../docs/adr/) as the source of truth for product intent and architectural direction.
- Default to exactly one coherent, PR-sized vertical slice per run.
- Prefer end-to-end completion of already-exposed surfaces over speculative new subsystems or broad refactors.
- Keep source code, docs, tests, commit-ready summaries, and user-facing copy in English unless the user explicitly asks to localize content.
- Before editing, inspect the current implementation to determine what is already shipped, what is partially implemented, and what is still missing.
- Choose the highest-value next slice that is clearly aligned with the docs, already adjacent to existing code or APIs, small enough to complete safely in one pass, and testable end-to-end.
- Prioritize in this order:
  1. Frontend or API completion for already-landed backend capabilities.
  2. Protocol completeness for ecosystem adapters that already exist.
  3. Targeted test or documentation hardening for recently implemented slices.
- When multiple slices are viable, prefer the smallest high-value extension of the organization workspace that reuses existing organization and team APIs.
- For organization governance work, inspect [ADR 0012](../../docs/adr/0012-team-package-governance.md), [frontend/src/routes/orgs/[slug]/+page.svelte](../../frontend/src/routes/orgs/[slug]/+page.svelte), [frontend/src/routes/orgs/[slug]/teams/[team_slug]/+page.svelte](../../frontend/src/routes/orgs/[slug]/teams/[team_slug]/+page.svelte), [frontend/src/api/orgs.ts](../../frontend/src/api/orgs.ts), and [crates/api/src/routes/orgs.rs](../../crates/api/src/routes/orgs.rs) early.
- Likely adjacent slices include team CRUD, team member management, delegated package-access management, delegated repository-access or namespace-access management, organization ownership transfer, or narrowly scoped delegated-authorization tests when UI scope would be too large.
- Before making changes, show a concise checkbox todo list.
- Implement incrementally, add or extend targeted tests, run the relevant checks, and fix issues before finishing.
- Update docs only when the visible behavior or implementation surface materially changes.
- End with:
  - the updated todo list
  - what changed and why
  - files changed with one-line purpose
  - tests/checks run and outcomes
  - the best next follow-up slice
- Stop after one slice. Recommend the next slice instead of starting a second implementation slice in the same run.

This instruction is intentionally task-scoped instead of automatically applying to every file, because the workflow is specific to “continue implementation” requests rather than general coding standards.
