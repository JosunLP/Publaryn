# Publaryn Release Checklist

Use this checklist before publishing any stable Publaryn release. For `1.0.0`,
every item below is part of the release gate, not optional ceremony.

## 1. Contract and scope alignment

- [ ] `README.md`, `docs/1.0.md`, `docs/concept.md`, and `docs/adr/README.md` agree on what is in scope for the release.
- [ ] Supported, unsupported, and deferred features are clearly separated.
- [ ] No public route or workflow required for the documented release is missing from the contract.

## 2. Documentation site and release notes

- [ ] The VitePress site builds successfully from `docs/`.
- [ ] The release notes page exists under `docs/releases/`.
- [ ] The release notes call out supported adapters, key governance/security capabilities, and explicit post-1.0 deferrals.
- [ ] Operator runbooks referenced by the release remain accurate.

## 3. Backend validation matrix

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test -p publaryn-core`
- [ ] `cargo test -p publaryn-auth --lib`
- [ ] `cargo test -p publaryn-api --lib`
- [ ] `cargo test -p publaryn-api --test integration_tests`
- [ ] `cargo test -p publaryn-auth --test auth_tests`

## 4. Frontend validation matrix

- [ ] `bun install --frozen-lockfile` in `frontend/`
- [ ] `bun run typecheck`
- [ ] `bun test`
- [ ] `bun run build`

## 5. Container and deployment checks

- [ ] The Docker smoke build completes successfully.
- [ ] The frontend static build is present in the runtime image or release build path.
- [ ] Health and readiness probes remain valid for the documented deployment model.

## 6. Versioning and release automation

- [ ] All release-facing manifests are synchronized to the target version.
- [ ] GitHub release automation validates before publish, syncs the versioned docs release notes into the GitHub release body, and builds release artifacts on publish.
- [ ] Container images are tagged consistently with the release version and `latest` policy, when applicable.
- [ ] The changelog and release notes reference the same release number.

## 7. Final sign-off

- [ ] The release notes clearly separate supported, unsupported, and deferred features.
- [ ] No undocumented route is required for the advertised 1.0 user journeys.
- [ ] The final GitHub release description matches `docs/releases/1.0.0.md`.
