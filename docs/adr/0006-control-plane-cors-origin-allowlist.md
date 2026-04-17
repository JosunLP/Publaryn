# ADR 0006: Control-plane CORS uses explicit origin allowlists

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn's control plane carries bearer credentials for package maintenance, organization administration, token management, and other governance-sensitive actions.
The API is designed to run as a stateless, horizontally scalable service behind proxies, ingress controllers, and CDN layers where appropriate.

Before this change, the API used permissive CORS behavior for every route.
That was convenient for ad hoc browser clients, but it was not aligned with Publaryn's security-first posture.
Permissive cross-origin access broadens the blast radius of frontend deployment mistakes, makes browser-based token use easier to misuse, and weakens the default boundary around administrative surfaces.

Publaryn also needs a development path for the SvelteKit frontend when it runs on a different origin from the API.
That path should remain explicit, environment-specific, and compatible with multi-replica deployments.

## Decision

Publaryn will use an explicit CORS origin allowlist for browser-based cross-origin API access.

The server configuration adds `SERVER__CORS_ALLOWED_ORIGINS`, parsed as a comma-separated list of allowed origins.
If the list is empty, the API does not emit cross-origin allow-origin headers.
This keeps same-origin deployments working while denying cross-origin browser access by default.

Configured origins must:

- use `http` or `https`
- include a host
- omit paths, queries, fragments, and embedded credentials
- not use the wildcard `*`

Invalid origin values cause startup to fail fast.
That prevents accidentally deploying an overly broad or malformed browser access policy across all replicas.

The API exposes the `x-request-id` response header to allowed browser clients so frontend debugging and incident correlation remain practical without weakening origin restrictions.

## Consequences

### Positive

- control-plane browser access is deny-by-default instead of permissive-by-default
- split-origin frontend deployments remain supported through explicit configuration
- invalid CORS settings fail at startup and are consistent across all replicas
- the approach preserves stateless API scaling because policy is driven entirely by shared configuration
- request correlation remains usable for browser clients through the exposed request ID header

### Trade-offs

- local frontend development on a separate origin now requires explicit configuration
- browser integrations cannot rely on wildcard origins for convenience
- if future cookie-based cross-origin authentication is introduced, the policy will need a dedicated follow-up decision instead of reusing the current bearer-token assumptions

## Follow-up work

- add environment-specific deployment guidance for API and frontend origin layouts
- keep public package pages preferably same-origin or reverse-proxied where possible
- revisit CORS behavior separately if a browser-first public API surface is introduced
