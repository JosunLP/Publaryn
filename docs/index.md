---
layout: home
title: Publaryn Documentation
titleTemplate: false
hero:
  name: Publaryn
  text: 1.0 release documentation
  tagline: Security-first, self-hostable multi-ecosystem package registry built in Rust.
  actions:
    - theme: brand
      text: Read the 1.0 contract
      link: /1.0
    - theme: alt
      text: Browse API routes
      link: /api-routes
    - theme: alt
      text: Review release notes
      link: /releases/1.0.0
features:
  - title: Clear 1.0 contract
    details: The release contract defines exactly what Publaryn 1.0 supports, what is deferred, and what must pass before the release can ship.
    link: /1.0
  - title: Structured guides
    details: Product, architecture, reference, and operations now have dedicated entry pages instead of living only inside one large concept document.
    link: /product/README
  - title: Native protocol coverage
    details: npm, PyPI, Cargo, NuGet, Maven, RubyGems, Composer, and OCI are documented with their mounted paths and baseline flows.
    link: /api-routes
  - title: Architecture with receipts
    details: The concept document and ADR catalog explain the why behind the product scope, governance model, and adapter design.
    link: /adr/
  - title: Release-ready operations
    details: Use the release checklist, operator runbooks, and release notes to validate and communicate a stable Publaryn release.
    link: /release-checklist
---

## What ships in the 1.0 baseline

Publaryn 1.0 is the first release intended to be treated as a coherent,
production-oriented baseline for self-hosted, multi-ecosystem package hosting.

- unified management APIs for governance, packages, releases, tokens, audit, and search
- native read and publish flows for all mounted adapters
- quarantine-first publication with immutable artifact storage
- delegated organization governance through teams, repository access, and namespace claims
- security findings, background jobs, and visibility-aware search
- a SvelteKit web portal for discovery, package details, settings, and organization workspaces

## Recommended reading order

1. Start with the [1.0 release contract](/1.0) for the supported baseline.
2. Use the [API and adapter route reference](/api-routes) to understand mounted surfaces.
3. Continue with the [product guide](/product/README), [architecture overview](/architecture/README), and [operations guide](/operations/README).
4. Read the [product concept](/concept) for architecture and roadmap context.
5. Use the [ADR index](/adr/README) before changing protocol, governance, or release behavior.
6. Follow the [release checklist](/release-checklist) before cutting a tagged release.

## Release artifacts

- [Release notes index](/releases/README)
- [Product guide](/product/README)
- [Publaryn 1.0.0 release notes](/releases/1.0.0)
- [Operator queue recovery runbook](/operator/job-queue-recovery)
