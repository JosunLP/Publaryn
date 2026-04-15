use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Type of artifact file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "artifact_kind", rename_all = "snake_case")]
pub enum ArtifactKind {
    /// npm tarball (.tgz)
    Tarball,
    /// Python wheel (.whl)
    Wheel,
    /// Python source distribution (.tar.gz)
    Sdist,
    /// Java archive (.jar)
    Jar,
    /// Maven POM file
    Pom,
    /// Ruby gem (.gem)
    Gem,
    /// NuGet package (.nupkg)
    Nupkg,
    /// NuGet symbols package (.snupkg)
    Snupkg,
    /// OCI image manifest (JSON)
    OciManifest,
    /// OCI image layer (blob)
    OciLayer,
    /// Rust crate (.crate)
    Crate,
    /// Composer package zip
    ComposerZip,
    /// Checksum file
    Checksum,
    /// Detached signature
    Signature,
    /// SBOM (SPDX/CycloneDX)
    Sbom,
    /// Source ZIP
    SourceZip,
}

/// An individual file artifact associated with a release.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Artifact {
    pub id: Uuid,
    pub release_id: Uuid,
    pub kind: ArtifactKind,
    pub filename: String,
    /// Storage key (object storage path).
    pub storage_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub sha512: Option<String>,
    pub md5: Option<String>,
    pub is_signed: bool,
    pub signature_key_id: Option<String>,
    pub uploaded_at: DateTime<Utc>,
}

impl Artifact {
    pub fn new(
        release_id: Uuid,
        kind: ArtifactKind,
        filename: String,
        storage_key: String,
        content_type: String,
        size_bytes: i64,
        sha256: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            release_id,
            kind,
            filename,
            storage_key,
            content_type,
            size_bytes,
            sha256,
            sha512: None,
            md5: None,
            is_signed: false,
            signature_key_id: None,
            uploaded_at: Utc::now(),
        }
    }
}
