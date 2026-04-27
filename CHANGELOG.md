# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### 1.1.0 added

- completed `1.1.0` roadmap in `docs/product/1.1.0-roadmap.md`
- heuristic risk posture on package and release detail surfaces derived from bundle analysis and unresolved security findings
- search discovery hints for risk level, unresolved security findings, trusted publishing, latest release state, and freshness signals
- operator job retry and stale-lock recovery endpoints with retry eligibility, stale state, and recovery hints
- normalized dependency overview on release detail pages for ecosystems with stored structured metadata
- organization delegated-access history API, CSV export, and workspace panel for package, repository, and namespace grants

### 1.1.0 documentation

- release-facing `1.1.0` notes in `docs/releases/1.1.0.md`
- API route reference entries for organization access-history list and export endpoints

## [1.0.0] - 2026-04-23

### Added

- stable 1.0 contract documentation in `docs/1.0.md`
- VitePress-based documentation site rooted in `docs/`
- release notes and release checklist documentation
- unified route reference for control-plane and native adapter mounts

### Stable baseline

- native adapters for npm/Bun, PyPI, Cargo, NuGet, Maven, RubyGems, Composer,
  and OCI
- quarantine-first release lifecycle with immutable artifact storage
- organization governance, delegated team access, trusted publishing, and
  security findings

### Deferred beyond 1.0

- proxy, mirror, and virtual repositories
- enterprise SSO, billing, federation, and air-gapped synchronization
- full attestation and signature workflows

See `docs/releases/1.0.0.md` for the human-readable release notes prepared for
the first stable release.
