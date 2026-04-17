---
name: continuous-autonomous-working
description: 'Complete multi-step coding and repo tasks continuously without waiting for human restarts. Use when the user wants autonomous execution, end-to-end delivery, proactive next steps, iterative debugging, testing, validation, and minimal check-ins. Trigger phrases: continue autonomously, keep going until done, no restarts, do the whole task, finish end-to-end, do not stop between steps.'
argument-hint: 'Describe the task to complete autonomously, plus any boundaries, approvals, or stop conditions.'
user-invocable: true
disable-model-invocation: false
---

# Continuous Autonomous Working

## Outcome

Drive a task from understanding to implementation, validation, and wrap-up without making the user manually restart the workflow after each intermediate step.

## When to Use

Use this skill when the user:

- explicitly asks for autonomous, continuous, end-to-end execution
- wants a bug fix, feature, refactor, migration, or review completed in one pass
- does not want repeated "what next?" prompts between obvious steps
- expects the agent to plan, act, test, debug, and continue until done or genuinely blocked

## Operating Rules

- Prefer action over unnecessary confirmation.
- Continue to the next obvious step after each successful investigation, edit, or test.
- Use small, verifiable changes instead of large speculative rewrites.
- Keep the user informed with brief progress updates, but do not stop for permission unless required.
- Respect safety, policy, destructive-operation, credential, and approval boundaries.
- Default autonomy boundary: code, configuration, analysis, and validation are in scope; commits, pushes, pull requests, deployments, and other external publication steps are out of scope unless the user explicitly expands the boundary.

## Procedure

1. **Lock the objective**
   - Restate the requested outcome internally.
   - Note constraints, acceptance criteria, repo conventions, and any explicit do-not-touch areas.
   - If a reasonable default exists for an unspecified detail, choose it and proceed.

2. **Gather the minimum useful context**
   - Inspect relevant files, symbols, errors, tests, docs, or configuration.
   - Identify the likely root cause, implementation surface, and verification path.
   - Avoid broad exploration unless the task is genuinely ambiguous.

3. **Create and maintain a lightweight todo list**
   - Break the work into concrete, testable steps.
   - Keep exactly one step in progress.
   - Update the list as new subproblems or follow-up checks appear.

4. **Execute the smallest meaningful next step**
   - Make focused code or config changes.
   - Preserve existing style and public behavior unless the task requires otherwise.
   - Prefer low-risk fixes before broader refactors.

5. **Validate immediately**
   - Run the most relevant checks available: tests, type checks, linting, builds, or targeted execution.
   - If validation fails, diagnose the failure and continue iterating.
   - Distinguish between issues caused by the change and unrelated pre-existing failures.

6. **Continue automatically**
   - After each successful step, move straight to the next planned step.
   - Do not ask the user to continue, confirm, or restart the workflow when the next action is clear.
   - If the best next step is another investigation or test, do it.

7. **Close the loop**
   - Ensure the original request is fully addressed.
   - Update the todo list so nothing is left ambiguous.
   - Summarize what changed, how it was verified, and any remaining risks or optional follow-ups.

## Decision Points

### Proceed without asking when

- the next step is obvious and low risk
- a sensible default can be chosen safely
- additional context can be gathered directly
- validation can confirm the decision quickly

### Ask the user only when

- multiple materially different options would change product or architecture direction
- required credentials, secrets, approvals, or external access are missing
- the action is destructive, irreversible, or affects production-facing state
- the next step would create commits, push branches, open pull requests, deploy changes, or publish artifacts
- the task is blocked by ambiguity that cannot be resolved from the repo or available tools

### If something fails

- inspect the failure instead of stopping immediately
- retry with a narrower or safer approach when appropriate
- update the todo list to reflect the new subtask
- continue until the root problem is solved or a real blocker remains

## Progress Update Style

Provide short progress updates after several meaningful actions or a burst of edits. Each update should briefly cover:

- what was learned or changed
- what is being verified now
- what the next likely step is

Avoid repetitive status chatter.

## Completion Checks

Before stopping, verify that:

- the requested outcome is actually complete
- relevant code paths were tested or otherwise validated
- changed files are internally consistent
- any newly introduced errors were resolved
- the todo list is fully updated with completed or blocked status
- the final message includes results, verification, and any necessary follow-up

## Stop Conditions

Stop only when one of these is true:

- the task is complete and verified
- the remaining blocker genuinely requires user input or approval
- further action would be unsafe or impossible with the available access

## Example Prompts

- `/continuous-autonomous-working Fix the failing auth tests and keep going until everything relevant passes.`
- `/continuous-autonomous-working Implement the NuGet search pagination fix end-to-end without stopping between steps.`
- `/continuous-autonomous-working Review this repo for release blockers, apply the safe fixes, and only interrupt me if you hit a real decision.`
