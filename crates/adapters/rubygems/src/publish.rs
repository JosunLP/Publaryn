//! Publish flow for RubyGems: parse `.gem`, create/update package +
//! release + artifact records, and finalize publication.

use bytes::Bytes;
use sha2::{Digest, Sha256, Sha512};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{
    domain::{
        artifact::{Artifact, ArtifactKind},
        release::Release,
    },
    error::{Error, Result},
};

use crate::gemspec::{self, gem_filename, release_version, GemspecMetadata};

/// Result of parsing a pushed gem.
#[derive(Debug, Clone)]
pub struct ParsedGemPush {
    pub metadata: GemspecMetadata,
    pub bytes: Bytes,
    pub sha256: String,
    pub sha512: String,
    pub size_bytes: i64,
    pub filename: String,
    pub release_version: String,
}

/// Parse raw `.gem` bytes, extracting metadata and computing digests.
pub fn parse_gem_push(bytes: Bytes) -> Result<ParsedGemPush> {
    if bytes.is_empty() {
        return Err(Error::Validation("Empty .gem payload".into()));
    }
    if bytes.len() > gemspec::MAX_GEM_SIZE {
        return Err(Error::Validation(format!(
            ".gem exceeds maximum size of {} MiB",
            gemspec::MAX_GEM_SIZE / (1024 * 1024)
        )));
    }

    let metadata = gemspec::parse_gem(&bytes)?;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let sha512 = hex::encode(Sha512::digest(&bytes));
    let size_bytes = bytes.len() as i64;
    let filename = gem_filename(&metadata);
    let release_version = release_version(&metadata);

    Ok(ParsedGemPush {
        metadata,
        bytes,
        sha256,
        sha512,
        size_bytes,
        filename,
        release_version,
    })
}

/// Select the repository to auto-create a package in for a first-time
/// pusher. Mirrors NuGet/npm behavior.
pub async fn select_default_repository(db: &PgPool, user_id: Uuid) -> Result<RepoInfo> {
    let row = sqlx::query(
        "SELECT id, visibility::text AS visibility \
         FROM repositories \
         WHERE owner_user_id = $1 \
           AND kind IN ('public', 'private', 'staging', 'release') \
         ORDER BY created_at ASC \
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
    .map_err(Error::Database)?;

    let row = row.ok_or_else(|| {
        Error::Forbidden(
            "You have no repository to publish into. Create a repository first.".into(),
        )
    })?;

    Ok(RepoInfo {
        id: row.try_get("id").unwrap_or_default(),
        visibility: row.try_get("visibility").unwrap_or_else(|_| "public".into()),
    })
}

pub struct RepoInfo {
    pub id: Uuid,
    pub visibility: String,
}

/// Create an artifact record for a pushed `.gem`.
pub fn make_artifact(release_id: Uuid, parsed: &ParsedGemPush) -> Artifact {
    let storage_key = format!(
        "releases/{}/artifacts/{}/{}",
        release_id, parsed.sha256, parsed.filename
    );
    Artifact::new(
        release_id,
        ArtifactKind::Gem,
        parsed.filename.clone(),
        storage_key,
        "application/octet-stream".into(),
        parsed.size_bytes,
        parsed.sha256.clone(),
    )
}

/// Build a fresh release in `quarantine`.
pub fn make_release(package_id: Uuid, parsed: &ParsedGemPush, published_by: Uuid) -> Release {
    let mut release = Release::new(package_id, parsed.release_version.clone(), published_by);
    release.description = parsed.metadata.description.clone();
    release.is_prerelease = looks_prerelease(&parsed.metadata.version);
    release
}

fn looks_prerelease(version: &str) -> bool {
    // RubyGems treats a version as pre-release if any segment contains a letter.
    version.split('.').any(|segment| {
        segment
            .chars()
            .any(|c| c.is_ascii_alphabetic() && c.is_ascii_lowercase())
    })
}

/// Provenance blob we persist on the release.
pub fn build_provenance(metadata: &GemspecMetadata) -> serde_json::Value {
    serde_json::json!({
        "source": "rubygems_push",
        "platform": metadata.platform,
        "authors": metadata.authors,
        "licenses": metadata.licenses,
        "summary": metadata.summary,
        "required_ruby_version": metadata.required_ruby_version,
        "required_rubygems_version": metadata.required_rubygems_version,
        "runtime_dependencies": metadata.runtime_dependencies,
        "development_dependencies": metadata.development_dependencies,
        "metadata": metadata.metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prerelease_detection() {
        assert!(!looks_prerelease("1.2.3"));
        assert!(looks_prerelease("1.2.3.beta1"));
        assert!(looks_prerelease("2.0.0.rc1"));
        assert!(!looks_prerelease("0.0.0"));
    }

    #[test]
    fn empty_payload_rejected() {
        assert!(parse_gem_push(Bytes::new()).is_err());
    }
}
