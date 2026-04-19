# ADR 0020: Maven Deploy Adapter

**Status:** Accepted
**Date:** 2026-04-19
**Decision Makers:** Architecture Team

## Context

The Maven ecosystem (Java, Kotlin, Scala, Groovy) uses Maven-style HTTP
repositories as the canonical distribution format. Publaryn already
exposes a read-only Maven surface (`GET /maven/{*path}`) that serves
`maven-metadata.xml`, POMs, and artifact binaries. The remaining gap is
publish: developers need to `mvn deploy`, `gradle publish`, or
`sbt publish` into Publaryn.

## Decision

### Protocol surface

Maven's deploy protocol is just HTTP `PUT` into the repository layout.
We extend the existing router under `/maven` with:

```
PUT /maven/{groupPath}/{artifactId}/{version}/{filename}
```

`filename` accepts the standard Maven conventions:

- `{artifactId}-{version}.jar` (primary binary)
- `{artifactId}-{version}.pom` (required — defines coordinates)
- `{artifactId}-{version}.war`, `.aar`, `.ear`
- `{artifactId}-{version}-{classifier}.{ext}` (e.g. `-sources.jar`, `-javadoc.jar`)
- `{artifactId}-{version}.module` (Gradle metadata)
- `{artifactId}-{version}.pom.asc` (PGP signature)
- `{filename}.md5`, `.sha1`, `.sha256`, `.sha512` (checksum files)

### Auth

`PUT` requires Bearer auth with `packages:write` scope. Checksum files
reuse the same scope but require the primary artifact already exists
(avoids stray checksum writes).

### Coordinate derivation

The POM file is the source of truth. On receipt:

1. If filename is `{artifactId}-{version}.pom`: parse XML, extract
   `<groupId>`, `<artifactId>`, `<version>`, assert they match the URL
   path. Use as the authoritative coordinates for the release.
2. If a non-POM artifact arrives before its POM: accept it, park under
   a pending upload keyed by `(groupId, artifactId, version)`, and
   finalize once the matching POM lands.

### Release lifecycle

Each `(groupId, artifactId, version)` maps to a Publaryn `Release`.
Publish flow:

1. First file of a version: create `Release` in `quarantine`, create
   `Package` if missing using the pusher's first repository (ADR 0008
   auto-assignment, same as npm/NuGet).
2. Each subsequent file in the same version appends to the release
   as a new `Artifact` (kind: `jar`, `pom`, `source_zip`, `checksum`,
   `signature`, or `sbom`).
3. When a POM arrives for a release still in `quarantine`, the release
   transitions to `published` (scanners fire async). Additional files
   arriving after publish are accepted only while the release is within
   the configured grace window (default: 1 hour; see further work).

### Snapshots

Mutable `-SNAPSHOT` versions conflict with Publaryn's
immutable-artifact model (ADR 0009). **Snapshots are explicitly
out-of-scope for this ADR.** A future ADR can introduce a
`repository_kind = 'snapshot'` with mutable artifacts scoped to
development flows. Snapshot deploys are rejected with `400 Bad Request`
and a human-readable message.

### Metadata regeneration

On publish we enqueue a `RegenerateMavenMetadata` background job that
rewrites `maven-metadata.xml` for the package, serializing concurrent
deploys. The existing on-the-fly metadata generator remains as a
fallback when no cached copy exists.

## Consequences

- **Positive:** Maven, Gradle, and sbt publish flows work end-to-end.
- **Positive:** Immutability guarantees and visibility rules apply
  uniformly with the other adapters.
- **Negative:** No snapshot support in MVP — enterprise Java teams that
  rely on `-SNAPSHOT` workflows must use a separate snapshot repo until
  a follow-up ADR lands.

## References

- Maven Deploy Plugin docs: <https://maven.apache.org/plugins/maven-deploy-plugin/>
- ADR 0009, ADR 0008
