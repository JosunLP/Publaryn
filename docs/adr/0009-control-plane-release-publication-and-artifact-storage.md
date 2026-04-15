# ADR 0009: Control-plane release publication uses a quarantine-first artifact workflow

- Status: Accepted
- Date: 2026-04-15

## Context

Publaryn already supported package creation, release state mutation, and tag management, but it still lacked a control-plane workflow that could create a release, attach immutable artifacts, and then publish the release consistently.

Without that workflow:

- releases could not be created explicitly through the management API
- artifact storage configuration existed but was not used
- there was no idempotent path for uploading release files
- quarantined or scanning releases risked being exposed through direct release reads
- search documents could not be updated with a meaningful latest version after publication

Publaryn needs a publish flow that keeps PostgreSQL as the metadata source of truth, object storage as the artifact source of truth, and public visibility gated on explicit publication.

## Decision

Publaryn adds an explicit control-plane release lifecycle built around quarantine-first publication.

### Lifecycle

A release now moves through these control-plane steps:

1. `POST /v1/packages/:ecosystem/:name/releases`
   - creates the release in `quarantine`
   - stores release metadata without making the release publicly readable
2. `PUT /v1/packages/:ecosystem/:name/releases/:version/artifacts/:filename?kind=...`
   - uploads an immutable artifact into shared object storage
   - computes checksums server-side
   - treats repeated uploads of the same filename and content as idempotent
3. `POST /v1/packages/:ecosystem/:name/releases/:version/publish`
   - verifies that at least one artifact exists
   - marks the release as `published` unless existing flags require `deprecated` or `yanked`
   - writes the existing `release_publish` audit event
   - refreshes the package search document

### Artifact immutability and idempotency

Artifact uploads are allowed only while the release is in `quarantine` or `scanning`.
Once a release has been published or otherwise finalized, artifact uploads are rejected.

Artifacts are keyed in object storage using the release identifier, content checksum, and filename.
At the database layer, `(release_id, filename)` is unique so retries can be handled safely and multiple API replicas cannot create duplicate artifact rows for the same logical file.

### Read visibility

Direct release reads and artifact downloads follow package visibility rules first and release-status visibility second.

Public direct reads are allowed only for releases whose status is one of:

- `published`
- `deprecated`
- `yanked`

`quarantine` and `scanning` releases remain visible only to actors who already have package write access.
This keeps pre-publication artifacts and metadata from leaking through public package paths.

### Search consistency

Search remains a derived view.
After publication, Publaryn reindexes the package document so search can reflect the latest published release version.
If search indexing fails, the publish transaction remains successful because search is not the source of truth.

## Consequences

### Positive

- Publaryn now has a concrete control-plane release publishing workflow
- artifact storage is externalized and replica-safe
- release publication is explicit and auditable
- idempotent artifact uploads work cleanly with horizontal scaling
- pre-publication releases are not exposed through public read paths
- search can reflect the latest published version after release publication

### Trade-offs

- artifact upload and download are currently proxied through the API, which is simple and stateless but may later be optimized with streaming or presigned URLs for very high-throughput protocol paths
- orphaned objects are still possible in rare concurrent conflict races because object storage is written before metadata insertion; background garbage collection is a follow-up concern
- the current control-plane flow is generic and does not yet model ecosystem-specific publish semantics such as npm dist-tag defaults or OCI manifest assembly

## Follow-up work

- add ecosystem-specific publish adapters that map native client workflows onto the shared release/artifact model
- introduce scan workers and an explicit transition into and out of `scanning`
- consider presigned or streamed artifact delivery for hot protocol download paths
- add background garbage collection for orphaned objects after failed metadata writes
- extend API and UI surfaces to show artifact lists, checksums, and publication readiness more prominently
