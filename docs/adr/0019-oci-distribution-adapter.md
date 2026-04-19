# ADR 0019: OCI Distribution Protocol Adapter

**Status:** Accepted
**Date:** 2026-04-19
**Decision Makers:** Architecture Team

## Context

Publaryn targets multi-ecosystem package distribution. Container images and
OCI artifacts (Helm charts, WASM modules, SBOMs, cosign signatures) are
distributed via the OCI Distribution Specification v1.1
(<https://github.com/opencontainers/distribution-spec>). Clients (`docker`,
`podman`, `buildah`, `oras`, `crane`, `helm`, `cosign`) all speak the `/v2/`
HTTP API surface.

The existing `crates/adapters/oci` crate is a stub with only a `lib.rs`
doc comment. To complete the adapter matrix we need a functional OCI
Distribution read and write surface that reuses Publaryn's quarantine-first
release pipeline (ADR 0009) and auth model (ADRs 0001 / 0002 / 0008).

## Decision

### Protocol surface

Mount under `/oci`, so routes observe the `/oci/v2/…` prefix expected by
clients when they're configured with `--registry publaryn.example.com/oci`
(or similar). The adapter implements the following subset of the spec,
which is sufficient for `docker pull`, `docker push`, `oras push`,
`helm push`, and `cosign` flows with monolithic blob uploads:

| Method        | Path                               | Purpose                                           |
| ------------- | ---------------------------------- | ------------------------------------------------- |
| `GET`         | `/v2/`                             | API version probe / auth challenge                |
| `GET`         | `/v2/_catalog`                     | Repository catalog (paginated, visibility-scoped) |
| `GET`         | `/v2/{name}/tags/list`             | Tag listing                                       |
| `GET`         | `/v2/{name}/referrers/{digest}`    | Subject-linked manifest discovery                 |
| `HEAD`, `GET` | `/v2/{name}/manifests/{reference}` | Fetch manifest by tag or digest                   |
| `PUT`         | `/v2/{name}/manifests/{reference}` | Upload manifest                                   |
| `DELETE`      | `/v2/{name}/manifests/{reference}` | Delete manifest / untag                           |
| `HEAD`, `GET` | `/v2/{name}/blobs/{digest}`        | Blob existence / download                         |
| `POST`        | `/v2/{name}/blobs/uploads/`        | Begin upload (monolithic or session)              |
| `PATCH`       | `/v2/{name}/blobs/uploads/{uuid}`  | Append chunk                                      |
| `PUT`         | `/v2/{name}/blobs/uploads/{uuid}`  | Finalize upload (requires `?digest=…`)            |
| `DELETE`      | `/v2/{name}/blobs/{digest}`        | Delete blob (admin-only)                          |

**Non-goals (MVP):**

- Cross-repo blob mount (`?mount=&from=`) — returns 202 with a normal upload
  session, forcing the client to re-upload. A follow-up ADR can optimize.
- Signed URLs / redirect responses for blob GET — blobs are streamed
  through the registry from object storage.

### Content addressing and storage

All blobs are content-addressable under storage key
`oci/blobs/sha256/<digest>`, shared across all repositories to support
efficient layer reuse within a single organization. Manifests are stored
separately under `oci/manifests/<release_id>/<digest>` because the same
manifest bytes can legitimately be tagged differently in different
repositories.

### Database model

A new table `oci_manifest_references` (migration 017) links an OCI
release's manifest to the blob digests it references, so we can validate
push integrity and enforce referential visibility on GC:

```sql
CREATE TABLE oci_manifest_references (
    release_id   UUID NOT NULL REFERENCES releases(id) ON DELETE CASCADE,
    ref_digest   TEXT NOT NULL,
    ref_kind     TEXT NOT NULL,  -- 'config' | 'layer' | 'subject'
    PRIMARY KEY (release_id, ref_digest, ref_kind)
);
```

Each OCI tag is modelled as a `Release` whose `version` is the tag (or
the digest for untagged references). Artifact kinds `oci_manifest` and
`oci_layer` already exist in the `artifact_kind` enum (migration 001).

### Authentication

The `/v2/` probe returns `401` with a
`WWW-Authenticate: Bearer realm="…",service="publaryn",scope="…"` header
when unauthenticated, as required by Docker clients. Token acquisition is
out of scope for the spec's token-auth flow — clients are expected to
provide a Publaryn Bearer token (`pub_…`) or JWT directly. The adapter
does not implement a separate `/oci/token` endpoint; clients use the
platform's `/v1/auth/login` or personal-access-token flow.

Scopes required:

- `packages:read` for anonymous pulls of non-public repositories.
- `packages:write` for PUT manifest and PUT blob.

### Publish flow

1. Client pushes blobs (config + layers) one by one via
   `POST → PATCH* → PUT` or monolithic `POST?digest=…`.
2. Each blob is stored content-addressable and a placeholder `Release`
   (status `quarantine`) is created on first blob push for the
   repository/reference pair if none exists.
3. Client pushes the manifest. The adapter validates that every digest
   referenced by the manifest is present in storage. Missing references
   return `400 Bad Request` (`MANIFEST_BLOB_UNKNOWN`).
4. On manifest acceptance the release is transitioned to `published`
   (ADR 0009 quarantine-first). Async scanning runs against the manifest
   blob via the existing `ScanArtifact` worker.
5. When the manifest includes a `subject`, the registry stores the subject
  reference, acknowledges it through the `OCI-Subject` response header, and
  exposes the published manifest through `/v2/{name}/referrers/{digest}`.

## Consequences

- **Positive:** Publaryn gains first-class container / OCI artifact
  support, unlocking the largest remaining ecosystem.
- **Positive:** Content-addressable dedupe keeps storage costs in check.
- **Negative:** OCI spec still has many optional features (chunk size
  negotiation, Warnings header, blob mount). MVP excludes the
  optimizations but stays spec-compliant for required behavior.
- **Negative:** Blob GC is manual for now; a follow-up ADR will add a
  background reaper that removes blobs unreferenced by any manifest.

## References

- OCI Distribution Spec v1.1: <https://github.com/opencontainers/distribution-spec/blob/main/spec.md>
- ADR 0009 (quarantine-first publication)
- ADR 0008 (control-plane package creation derives ownership from repositories)
