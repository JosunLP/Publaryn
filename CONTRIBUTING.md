# Contributing to Publaryn

Thanks for helping improve `Publaryn`.

This repository is building a secure, multi-ecosystem package registry platform with a Rust backend, native protocol adapters, and a Bun-managed SvelteKit frontend. We welcome focused contributions that align with the current product direction and keep native client compatibility, security, and governance rules intact.

## Before you start

- Read the project overview in [README.md](README.md).
- Review [docs/concept.md](docs/concept.md) for product intent and platform goals.
- Check the relevant architecture decision records in [docs/adr/](docs/adr/) when your change touches protocol behavior, authentication, release publication, or organization governance.
- Open an issue first for major changes so the approach can be discussed before implementation starts.

## Development setup

### Prerequisites

- Rust 1.77+
- Docker and Docker Compose
- Bun 1.3+

### Bootstrapping the workspace

From the repository root:

1. Start local infrastructure:
   - `docker compose up -d postgres redis minio meilisearch`
2. Copy the example environment file:
   - `cp .env.example .env`
3. Start the API server:
   - `cargo run --bin publaryn`
4. In a second terminal, start the frontend:
   - `cd frontend`
   - `bun install`
   - `bun run dev`

The API is available at `http://localhost:3000` and the frontend at `http://localhost:5173`.

If you run the frontend on a separate origin, set `SERVER__CORS_ALLOWED_ORIGINS` in `.env` as described in [README.md](README.md) and [`.env.example`](.env.example).

## What to change with your code

Please keep pull requests small, reviewable, and directly related to the problem being solved.

When relevant, include:

- focused tests for the behavior you changed
- documentation updates when visible behavior, setup, or operator guidance changes
- ADR updates or follow-up notes when architectural intent changes materially

Please avoid broad drive-by refactors in the same pull request unless they are required to complete the slice safely.

## Validation before opening a pull request

Run the checks that match the parts of the repo you changed.

### Rust workspace checks

From the repository root:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test -p publaryn-core`
- `cargo test -p publaryn-auth --lib`
- `cargo test -p publaryn-api --lib`
- `cargo test -p publaryn-api --test integration_tests`
- `cargo test -p publaryn-auth --test auth_tests`

### Frontend checks

From `frontend/`:

- `bun install --frozen-lockfile`
- `bun run typecheck`
- `bun test`
- `bun run build`

If your change does not touch both stacks, run the relevant subset and mention what you verified in the pull request description.

## Pull request expectations

Please include:

- a short summary of what changed and why
- links to related issues, discussions, or ADRs
- the checks you ran locally
- screenshots or API examples when UI or external behavior changed
- follow-up work that you intentionally left out of scope

Keep source code, documentation, tests, and pull request text in English unless a change explicitly targets localized end-user content.

## Security-sensitive changes

If your change affects authentication, authorization, package ownership, publication, artifact access, or any other security-sensitive surface:

- verify the behavior against the relevant ADRs
- avoid trusting caller-supplied ownership or authorization data
- preserve auditability for governance-critical mutations
- do not disclose vulnerabilities in a public issue; use [SECURITY.md](SECURITY.md) instead

## Licensing

By submitting a contribution, you agree that your work will be distributed under the licenses that apply to this repository: Apache License 2.0 and MIT.
