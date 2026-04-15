# ADR 0015: Implement PyPI trusted publishing as a root-level OIDC token exchange

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn already supports package-scoped trusted publisher configuration in the control plane and Twine-compatible legacy uploads on `/pypi/legacy/`.
The next production-meaningful PyPI slice is secretless CI publishing.

Modern PyPI tooling does not send an external OIDC identity token directly to the upload endpoint.
Instead, it expects a two-phase exchange compatible with Warehouse:

1. fetch an audience from `GET /_/oidc/audience`
2. request an external OIDC JWT from the CI provider for that audience
3. exchange the JWT at `POST /_/oidc/mint-token`
4. use the returned short-lived API token for the normal upload flow

Publaryn must implement this flow without weakening its existing architectural constraints:

- API replicas must remain stateless and horizontally scalable
- publish authorization must remain package-aware
- the exchanged credential must not become a general-purpose control-plane token
- replay protection must survive restarts and multi-replica deployments

## Decision

Publaryn now implements PyPI trusted publishing through root-level OIDC exchange endpoints:

- `GET /_/oidc/audience`
- `POST /_/oidc/mint-token`

### Verification model

The mint-token endpoint:

- accepts a JSON payload containing an external OIDC JWT
- determines the issuer from the unverified JWT payload
- allows only explicitly trusted CI issuers
- resolves OIDC discovery and JWKS metadata from the issuer
- validates the JWT signature, issuer, audience, and expiry
- extracts trusted-publishing claims used for publisher matching

### Publisher matching

A mint request succeeds only when exactly one existing, non-archived PyPI package has a matching trusted publisher configuration.
Matching currently uses the existing `trusted_publishers` table fields:

- `issuer`
- `subject`
- optional `repository`
- optional `workflow_ref`
- optional `environment`

If no package matches, or multiple packages match, the exchange is rejected.

### Minted token model

A successful exchange creates a short-lived Publaryn API token with these properties:

- `kind = 'oidc_derived'`
- `packages:write` scope only
- bound to a single `package_id`
- bound to that package's `repository_id`
- 15 minute lifetime

The raw token is returned once and is then used with the existing PyPI legacy upload endpoint.

### Replay protection

Publaryn stores consumed external JWT identifiers in PostgreSQL using `(issuer, jwt_id)` as the replay key.
This keeps replay prevention durable across process restarts and horizontally scaled API replicas.

### Surface confinement

OIDC-derived tokens are intentionally not general-purpose API tokens.
They are:

- accepted for PyPI uploads
- rejected on control-plane `/v1/*` endpoints
- rejected on npm native endpoints
- rejected on PyPI read endpoints

PyPI uploads with an OIDC-derived token are additionally restricted to the single package bound into the minted token.

### Scope of this slice

This slice intentionally supports existing packages only.
Trusted publishing does not yet auto-create new PyPI packages or implement PyPI-style pending publisher flows.

## Consequences

### Positive

- standard PyPI trusted-publishing clients can integrate with Publaryn using the expected root-level exchange contract
- CI pipelines no longer require long-lived Publaryn secrets for PyPI publishing
- authorization remains package-aware and least-privilege
- replay protection remains safe in multi-instance deployments
- the implementation reuses Publaryn's existing token, audit, and upload infrastructure

### Trade-offs

- trusted publishing currently cannot bootstrap a brand-new PyPI package
- ambiguous trusted publisher definitions are rejected instead of minting a broader token
- the initial issuer allowlist is intentionally narrow and may need future expansion
- OIDC-derived tokens are PyPI-specific rather than reusable across other native adapters

## Follow-up work

- add pending-publisher or package-bootstrap flows for first publish without pre-created packages
- persist and validate stronger repository ownership continuity signals such as stable owner IDs where supported by the issuer
- add end-to-end integration coverage for the full audience → mint-token → legacy upload flow
- consider richer observability for OIDC exchange failures, replay rejections, and issuer-specific diagnostics
