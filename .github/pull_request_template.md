# Pull request

## Summary

Describe what changed and why.

## Related context

- Issue / discussion:
- ADR / design note:

## Type of change

- [ ] Bug fix
- [ ] Feature or slice completion
- [ ] Refactor with no intended behavior change
- [ ] Documentation or repository tooling update
- [ ] Security-sensitive change

## Validation

List the checks you ran locally.

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test -p publaryn-core`
- [ ] `cargo test -p publaryn-auth --lib`
- [ ] `cargo test -p publaryn-api --lib`
- [ ] `cargo test -p publaryn-api --test integration_tests`
- [ ] `cargo test -p publaryn-auth --test auth_tests`
- [ ] `bun install --frozen-lockfile`
- [ ] `bun run typecheck`
- [ ] `bun test`
- [ ] `bun run build`
- [ ] Not all checks were relevant; I explained the subset I ran

## Checklist

- [ ] I kept the change focused and reviewable
- [ ] I updated docs when visible behavior or setup changed
- [ ] I added or updated tests when behavior changed
- [ ] I considered security, ownership, and authorization impacts where relevant
- [ ] I noted any follow-up work that remains out of scope

## Screenshots, logs, or API examples

Add UI screenshots, request/response examples, or other evidence when helpful.
