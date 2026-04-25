---
name: 'Autonomous Publaryn Roadmap'
description: 'Continuously advance Publaryn through validated PR-sized slices until blocked or stopped, using the 1.0 contract, ADRs, tests, and current code as the decision framework.'
argument-hint: 'Optional focus, stop condition, or bias (for example: release blockers only, protocol hardening, frontend-first, stop after 2 completed slices)'
agent: 'Autonomous Publaryn'
---

Use the [Autonomous Publaryn](../agents/autonomous-publaryn.agent.md) custom agent as the persistent persona for this run.

Run-specific guidance: ${input:focus:Optional focus area, stop condition, or extra constraints}

This prompt is the lightweight launcher for the autonomous roadmap agent. Prefer the agent's default workflow and only use the run-specific guidance above to bias selection, stopping behavior, or slice priority.

When relevant, also apply:

- [Continuous Autonomous Working](../skills/continuous-autonomous-working/SKILL.md)
- [Publaryn Contract and Release Alignment](../skills/publaryn-contract-and-release-alignment/SKILL.md)
- [Publaryn Control-plane Auth and Ownership](../skills/publaryn-control-plane-auth-ownership/SKILL.md)
- [Publaryn Release and Artifact Lifecycle](../skills/publaryn-release-artifact-lifecycle/SKILL.md)
- [Continue Publaryn Slice instruction](../instructions/continue-publaryn-slice.instructions.md) for general product, API, frontend, governance, or test slices
- [Implement Publaryn Protocol Adapter instruction](../instructions/implement-publaryn-protocol-adapter.instructions.md) for native adapter slices

Treat these repository sources as primary context:

- [README](../../README.md)
- [Publaryn 1.0 release contract](../../docs/1.0.md)
- [Concept](../../docs/concept.md)
- [API and adapter route reference](../../docs/api-routes.md)
- [Release checklist](../../docs/release-checklist.md)
- [ADR index](../../docs/adr/README.md)
- relevant ADRs under [docs/adr](../../docs/adr/)
- [Contributing guide](../../CONTRIBUTING.md)
- [CI workflow](../../.github/workflows/ci.yml)

Mission for this run:

- continue through successive PR-sized slices without waiting for manual restarts
- prioritize release blockers, docs/code drift, existing-surface completion, protocol correctness, and security hardening
- stop only for a real blocker, an explicit stop condition, or a safety boundary
