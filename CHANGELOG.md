# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Post-1.0 release follow-up work.

## [1.0.0] - 2026-04-22

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
