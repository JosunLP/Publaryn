# Security Policy

Thanks for helping keep `Publaryn` secure.

## Reporting a vulnerability

Please do **not** report security vulnerabilities in public GitHub issues, pull requests, or discussions.

Instead, use GitHub Security Advisories for private disclosure:

- Open a private report at: <https://github.com/JosunLP/Publaryn/security/advisories/new>

Include as much detail as you can:

- affected area or component (for example API, auth, frontend, npm adapter, PyPI upload flow, Cargo registry adapter, organization governance, or release publication)
- impact and severity as you understand it
- reproduction steps, proof of concept, or request traces
- version, branch, commit, or deployment details if known
- suggested mitigations or context that may help triage

## Supported versions

Publaryn is currently pre-1.0. Security fixes are best-effort and are expected to land on the active default branch.

At this stage, the project only guarantees evaluation of vulnerabilities reported against the current codebase in this repository. Older forks, stale deployments, and heavily modified downstream builds may require separate validation by the operator.

## Disclosure process

When a report is received, the maintainer will aim to:

1. acknowledge receipt
2. validate impact and affected surfaces
3. prepare a fix or mitigation
4. coordinate disclosure timing when appropriate

Please avoid public disclosure until a fix or mitigation plan is available, unless disclosure is legally required.

## Security boundaries to keep in mind

Publaryn includes several security-sensitive areas where careful reports are especially helpful:

- authentication, sessions, tokens, and MFA
- organization governance, ownership transfer, and team delegation
- package publication, artifact storage, and release visibility
- trusted publishing and OIDC token exchange
- private package and artifact read authorization
- audit logging, policy enforcement, and supply-chain protections

## Non-security issues

For general bugs, feature requests, and usage questions, please use the standard GitHub issue flow described in [SUPPORT.md](SUPPORT.md).
