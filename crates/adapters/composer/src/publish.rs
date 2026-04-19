//! Publish helpers for the Composer adapter.
//!
//! Composer does not define a standard registry push protocol, so Publaryn's
//! native endpoint accepts a `composer.json` manifest plus a `dist.zip` archive.

use bytes::Bytes;
use serde_json::{Map, Value};
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

use crate::name::validate_composer_package_name;

pub const MAX_COMPOSER_JSON_BYTES: usize = 1024 * 1024;
pub const MAX_DIST_ZIP_BYTES: usize = 256 * 1024 * 1024;

const AUTO_CREATE_ALLOWED_REPOSITORY_KINDS: &[&str] = &["public", "private", "staging", "release"];
const ORG_REPOSITORY_WRITE_ROLES: &[&str] = &["owner", "admin"];
const TEAM_REPOSITORY_CREATE_PERMISSIONS: &[&str] = &["admin", "publish", "write_metadata"];

#[derive(Debug, Clone)]
pub struct ParsedComposerPublish {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub licenses: Vec<String>,
    pub keywords: Vec<String>,
    pub manifest: Value,
    pub zip_bytes: Bytes,
    pub sha256: String,
    pub sha512: String,
    pub size_bytes: i64,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub id: Uuid,
    pub visibility: String,
    pub owner_user_id: Option<Uuid>,
    pub owner_org_id: Option<Uuid>,
}

pub fn parse_composer_publish(
    expected_name: &str,
    composer_json_bytes: Bytes,
    dist_zip_bytes: Bytes,
) -> Result<ParsedComposerPublish> {
    if composer_json_bytes.is_empty() {
        return Err(Error::Validation(
            "The multipart field 'composer.json' is required".into(),
        ));
    }
    if composer_json_bytes.len() > MAX_COMPOSER_JSON_BYTES {
        return Err(Error::Validation(format!(
            "composer.json exceeds the maximum supported size of {} KiB",
            MAX_COMPOSER_JSON_BYTES / 1024
        )));
    }
    if dist_zip_bytes.is_empty() {
        return Err(Error::Validation(
            "The multipart field 'dist.zip' is required".into(),
        ));
    }
    if dist_zip_bytes.len() > MAX_DIST_ZIP_BYTES {
        return Err(Error::Validation(format!(
            "dist.zip exceeds the maximum supported size of {} MiB",
            MAX_DIST_ZIP_BYTES / (1024 * 1024)
        )));
    }

    let manifest: Value = serde_json::from_slice(&composer_json_bytes)
        .map_err(|err| Error::Validation(format!("Invalid composer.json payload: {err}")))?;
    let object = manifest
        .as_object()
        .ok_or_else(|| Error::Validation("composer.json must be a JSON object".into()))?;

    let name = string_field(object, "name").ok_or_else(|| {
        Error::Validation("composer.json must define a string 'name' field".into())
    })?;
    validate_composer_package_name(&name)?;
    if name != expected_name {
        return Err(Error::Validation(format!(
            "composer.json name '{name}' does not match the requested package '{expected_name}'"
        )));
    }

    let version = string_field(object, "version").ok_or_else(|| {
        Error::Validation("composer.json must define a string 'version' field".into())
    })?;

    let sha256 = hex::encode(Sha256::digest(&dist_zip_bytes));
    let sha512 = hex::encode(Sha512::digest(&dist_zip_bytes));
    let size_bytes = i64::try_from(dist_zip_bytes.len())
        .map_err(|_| Error::Validation("dist.zip exceeds the maximum supported size".into()))?;

    Ok(ParsedComposerPublish {
        filename: distribution_filename(&name, &version),
        description: string_field(object, "description"),
        homepage: string_field(object, "homepage"),
        repository_url: repository_url(&manifest),
        licenses: string_list_field(object.get("license")),
        keywords: string_list_field(object.get("keywords")),
        manifest,
        name,
        version,
        zip_bytes: dist_zip_bytes,
        sha256,
        sha512,
        size_bytes,
    })
}

pub async fn select_default_repository(db: &PgPool, actor_user_id: Uuid) -> Result<RepoInfo> {
    let row = sqlx::query(
        "SELECT id, slug, visibility::text AS visibility, owner_user_id, owner_org_id \
         FROM repositories \
         WHERE kind::text = ANY($1) \
           AND (\
               owner_user_id = $2 \
               OR EXISTS (\
                   SELECT 1 \
                   FROM org_memberships om \
                   WHERE om.org_id = repositories.owner_org_id \
                     AND om.user_id = $2 \
                     AND om.role::text = ANY($3)\
               ) \
               OR EXISTS (\
                   SELECT 1 \
                   FROM team_repository_access tra \
                   JOIN team_memberships tm ON tm.team_id = tra.team_id \
                   JOIN teams t ON t.id = tra.team_id \
                   WHERE tra.repository_id = repositories.id \
                     AND tm.user_id = $2 \
                     AND t.org_id = repositories.owner_org_id \
                     AND tra.permission::text = ANY($4)\
               )\
           ) \
         ORDER BY (owner_user_id = $2) DESC NULLS LAST, created_at ASC \
         LIMIT 1",
    )
    .bind(
        AUTO_CREATE_ALLOWED_REPOSITORY_KINDS
            .iter()
            .map(|kind| (*kind).to_owned())
            .collect::<Vec<_>>(),
    )
    .bind(actor_user_id)
    .bind(
        ORG_REPOSITORY_WRITE_ROLES
            .iter()
            .map(|role| (*role).to_owned())
            .collect::<Vec<_>>(),
    )
    .bind(
        TEAM_REPOSITORY_CREATE_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>(),
    )
    .fetch_optional(db)
    .await
    .map_err(Error::Database)?
    .ok_or_else(|| {
        Error::Forbidden(
            "You have no repository suitable for Composer publishes. Create one via the Publaryn API or ask an administrator to delegate access first.".into(),
        )
    })?;

    Ok(RepoInfo {
        id: row.try_get("id").unwrap_or_default(),
        visibility: row
            .try_get("visibility")
            .unwrap_or_else(|_| "public".into()),
        owner_user_id: row.try_get("owner_user_id").unwrap_or(None),
        owner_org_id: row.try_get("owner_org_id").unwrap_or(None),
    })
}

pub fn make_release(
    package_id: Uuid,
    parsed: &ParsedComposerPublish,
    published_by: Uuid,
) -> Release {
    let mut release = Release::new(package_id, parsed.version.clone(), published_by);
    release.description = parsed.description.clone();
    release.is_prerelease = looks_prerelease(&parsed.version);
    release.provenance = Some(parsed.manifest.clone());
    release
}

pub fn make_artifact(release_id: Uuid, parsed: &ParsedComposerPublish) -> Artifact {
    let storage_key = format!(
        "releases/{}/artifacts/{}/{}",
        release_id, parsed.sha256, parsed.filename
    );
    let mut artifact = Artifact::new(
        release_id,
        ArtifactKind::ComposerZip,
        parsed.filename.clone(),
        storage_key,
        "application/zip".into(),
        parsed.size_bytes,
        parsed.sha256.clone(),
    );
    artifact.sha512 = Some(parsed.sha512.clone());
    artifact
}

fn looks_prerelease(version: &str) -> bool {
    version.contains('-')
        || version
            .chars()
            .any(|character| character.is_ascii_alphabetic())
}

fn distribution_filename(name: &str, version: &str) -> String {
    let normalized_name = name.replace(['/', '\\'], "-");
    let normalized_version = version.replace(['/', '\\'], "-");
    format!("{normalized_name}-{normalized_version}.zip")
}

fn string_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn string_list_field(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_owned()]
            }
        }
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn repository_url(manifest: &Value) -> Option<String> {
    manifest
        .get("support")
        .and_then(|value| value.get("source"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            manifest
                .get("source")
                .and_then(|value| value.get("url"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_and_zip() {
        let parsed = parse_composer_publish(
            "acme/demo",
            Bytes::from_static(
                br#"{
                    "name": "acme/demo",
                    "version": "1.2.3",
                    "description": "Demo package",
                    "homepage": "https://example.test/demo",
                    "license": ["MIT", "Apache-2.0"],
                    "keywords": ["demo", "composer"],
                    "support": { "source": "https://github.com/acme/demo" }
                }"#,
            ),
            Bytes::from_static(b"zip-bytes"),
        )
        .expect("composer publish payload should parse");

        assert_eq!(parsed.name, "acme/demo");
        assert_eq!(parsed.version, "1.2.3");
        assert_eq!(parsed.filename, "acme-demo-1.2.3.zip");
        assert_eq!(parsed.licenses, vec!["MIT", "Apache-2.0"]);
        assert_eq!(parsed.keywords, vec!["demo", "composer"]);
        assert_eq!(
            parsed.repository_url.as_deref(),
            Some("https://github.com/acme/demo")
        );
        assert_eq!(parsed.size_bytes, 9);
    }

    #[test]
    fn rejects_name_mismatch() {
        let err = parse_composer_publish(
            "acme/demo",
            Bytes::from_static(br#"{"name":"acme/other","version":"1.0.0"}"#),
            Bytes::from_static(b"zip-bytes"),
        )
        .expect_err("mismatched name should fail");

        assert!(err
            .to_string()
            .contains("does not match the requested package"));
    }

    #[test]
    fn keeps_source_url_as_repository_url() {
        let parsed = parse_composer_publish(
            "acme/demo",
            Bytes::from_static(
                br#"{
                    "name": "acme/demo",
                    "version": "1.0.0",
                    "source": { "url": "https://git.example.test/acme/demo" }
                }"#,
            ),
            Bytes::from_static(b"zip-bytes"),
        )
        .expect("composer publish payload should parse");

        assert_eq!(
            parsed.repository_url.as_deref(),
            Some("https://git.example.test/acme/demo")
        );
    }
}
