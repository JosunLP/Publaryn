//! Publish helpers for the Maven adapter.
//!
//! Publaryn accepts Maven deploy uploads over HTTP `PUT`, using the standard
//! repository layout path as the request URL and the uploaded file bytes as the
//! body.

use bytes::Bytes;
use quick_xml::{events::Event, Reader};
use sha2::{Digest, Sha256, Sha512};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use publaryn_core::{
    domain::{
        artifact::{Artifact, ArtifactKind},
        release::Release,
    },
    error::{Error, Result},
    validation,
};

use crate::name::package_name;

pub const MAX_MAVEN_POM_BYTES: usize = 1024 * 1024;

const AUTO_CREATE_ALLOWED_REPOSITORY_KINDS: &[&str] = &["public", "private", "staging", "release"];
const ORG_REPOSITORY_WRITE_ROLES: &[&str] = &["owner", "admin"];
const TEAM_REPOSITORY_CREATE_PERMISSIONS: &[&str] = &["admin", "publish", "write_metadata"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

#[derive(Debug, Clone)]
pub enum MavenUploadRole {
    Primary,
    Pom,
    Signature {
        target_filename: String,
    },
    Checksum {
        target_filename: String,
        algorithm: ChecksumAlgorithm,
    },
}

#[derive(Debug, Clone)]
pub struct ParsedPom {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub packaging: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub licenses: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedMavenUpload {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub package_name: String,
    pub filename: String,
    pub content_type: String,
    pub artifact_kind: ArtifactKind,
    pub bytes: Bytes,
    pub sha256: String,
    pub sha512: String,
    pub size_bytes: i64,
    pub role: MavenUploadRole,
    pub pom: Option<ParsedPom>,
}

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub id: Uuid,
    pub visibility: String,
    pub owner_user_id: Option<Uuid>,
    pub owner_org_id: Option<Uuid>,
}

impl ParsedMavenUpload {
    pub fn can_start_release(&self) -> bool {
        matches!(self.role, MavenUploadRole::Primary | MavenUploadRole::Pom)
    }

    pub fn triggers_publication(&self) -> bool {
        matches!(self.role, MavenUploadRole::Pom)
    }

    pub fn target_filename(&self) -> Option<&str> {
        match &self.role {
            MavenUploadRole::Signature { target_filename }
            | MavenUploadRole::Checksum {
                target_filename, ..
            } => Some(target_filename),
            MavenUploadRole::Primary | MavenUploadRole::Pom => None,
        }
    }

    pub fn pom_description(&self) -> Option<String> {
        self.pom
            .as_ref()
            .and_then(|pom| {
                pom.description
                    .as_ref()
                    .map(|value| value.trim().to_owned())
            })
            .filter(|value| !value.is_empty())
    }

    pub fn pom_display_name(&self) -> Option<String> {
        self.pom
            .as_ref()
            .and_then(|pom| {
                pom.display_name
                    .as_ref()
                    .map(|value| value.trim().to_owned())
            })
            .filter(|value| !value.is_empty())
    }

    pub fn pom_homepage(&self) -> Option<String> {
        self.pom
            .as_ref()
            .and_then(|pom| pom.homepage.as_ref().map(|value| value.trim().to_owned()))
            .filter(|value| !value.is_empty())
    }

    pub fn pom_repository_url(&self) -> Option<String> {
        self.pom
            .as_ref()
            .and_then(|pom| {
                pom.repository_url
                    .as_ref()
                    .map(|value| value.trim().to_owned())
            })
            .filter(|value| !value.is_empty())
    }

    pub fn pom_primary_license(&self) -> Option<String> {
        self.pom
            .as_ref()
            .and_then(|pom| pom.licenses.first().cloned())
            .filter(|value| !value.trim().is_empty())
    }

    pub fn pom_provenance(&self) -> Option<serde_json::Value> {
        self.pom.as_ref().map(build_provenance)
    }
}

pub fn parse_maven_upload(
    group_id: &str,
    artifact_id: &str,
    version: &str,
    filename: &str,
    bytes: Bytes,
) -> Result<ParsedMavenUpload> {
    validation::validate_version(version)?;
    if version.to_ascii_uppercase().ends_with("-SNAPSHOT") {
        return Err(Error::Validation(
            "Snapshots are not supported by Publaryn's immutable Maven repository. Publish a non-SNAPSHOT version instead.".into(),
        ));
    }

    if bytes.is_empty() {
        return Err(Error::Validation(
            "Maven deploy payloads must not be empty".into(),
        ));
    }

    let package_name = package_name(group_id, artifact_id)?;
    let (role, artifact_kind, content_type) =
        classify_upload_filename(artifact_id, version, filename)?;

    let pom = if matches!(role, MavenUploadRole::Pom) {
        if bytes.len() > MAX_MAVEN_POM_BYTES {
            return Err(Error::Validation(format!(
                "Maven POM files must not exceed {} KiB",
                MAX_MAVEN_POM_BYTES / 1024
            )));
        }

        let pom = parse_pom_xml(&bytes)?;
        if pom.group_id != group_id || pom.artifact_id != artifact_id || pom.version != version {
            return Err(Error::Validation(format!(
                "The uploaded POM coordinates '{}:{}:{}' do not match the request path '{}:{}:{}'",
                pom.group_id, pom.artifact_id, pom.version, group_id, artifact_id, version,
            )));
        }
        if pom.version.to_ascii_uppercase().ends_with("-SNAPSHOT") {
            return Err(Error::Validation(
                "Snapshots are not supported by Publaryn's immutable Maven repository. Publish a non-SNAPSHOT version instead.".into(),
            ));
        }
        Some(pom)
    } else {
        None
    };

    let sha256 = hex::encode(Sha256::digest(&bytes));
    let sha512 = hex::encode(Sha512::digest(&bytes));
    let size_bytes = i64::try_from(bytes.len()).map_err(|_| {
        Error::Validation("Uploaded Maven artifacts exceed supported limits".into())
    })?;

    Ok(ParsedMavenUpload {
        group_id: group_id.to_owned(),
        artifact_id: artifact_id.to_owned(),
        version: version.to_owned(),
        package_name,
        filename: filename.to_owned(),
        content_type,
        artifact_kind,
        bytes,
        sha256,
        sha512,
        size_bytes,
        role,
        pom,
    })
}

pub async fn select_default_repository(db: &PgPool, actor_user_id: Uuid) -> Result<RepoInfo> {
    let row = sqlx::query(
        "SELECT id, visibility::text AS visibility, owner_user_id, owner_org_id \
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
            "You have no repository suitable for Maven deploys. Create one via the Publaryn API or ask an administrator to delegate access first.".into(),
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

pub fn make_release(package_id: Uuid, upload: &ParsedMavenUpload, published_by: Uuid) -> Release {
    let mut release = Release::new(package_id, upload.version.clone(), published_by);
    release.description = upload.pom_description();
    release.is_prerelease = looks_prerelease(&upload.version);
    release.provenance = upload.pom_provenance();
    release
}

pub fn make_artifact(release_id: Uuid, upload: &ParsedMavenUpload) -> Artifact {
    let storage_key = format!(
        "releases/{}/artifacts/{}/{}",
        release_id, upload.sha256, upload.filename
    );
    let mut artifact = Artifact::new(
        release_id,
        upload.artifact_kind.clone(),
        upload.filename.clone(),
        storage_key,
        upload.content_type.clone(),
        upload.size_bytes,
        upload.sha256.clone(),
    );
    artifact.sha512 = Some(upload.sha512.clone());
    artifact
}

pub fn parse_pom_xml(xml_bytes: &[u8]) -> Result<ParsedPom> {
    let xml_str = std::str::from_utf8(xml_bytes)
        .map_err(|error| Error::Validation(format!("Invalid UTF-8 in Maven POM: {error}")))?;

    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);

    let mut stack: Vec<String> = Vec::new();
    let mut group_id: Option<String> = None;
    let mut artifact_id: Option<String> = None;
    let mut version: Option<String> = None;
    let mut parent_group_id: Option<String> = None;
    let mut parent_version: Option<String> = None;
    let mut packaging: Option<String> = None;
    let mut display_name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut homepage: Option<String> = None;
    let mut repository_url: Option<String> = None;
    let mut licenses: Vec<String> = Vec::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                stack.push(local_name_str(e.name().as_ref()));
            }
            Ok(Event::Empty(ref e)) => {
                stack.push(local_name_str(e.name().as_ref()));
                stack.pop();
            }
            Ok(Event::Text(ref e)) => {
                let text = e.xml_content().unwrap_or_default().trim().to_owned();
                if text.is_empty() {
                    buf.clear();
                    continue;
                }

                match stack.join("/").as_str() {
                    "project/groupId" => group_id = Some(text),
                    "project/artifactId" => artifact_id = Some(text),
                    "project/version" => version = Some(text),
                    "project/packaging" => packaging = Some(text),
                    "project/name" => display_name = Some(text),
                    "project/description" => description = Some(text),
                    "project/url" => homepage = Some(text),
                    "project/scm/url" => repository_url = Some(text),
                    "project/licenses/license/name" => licenses.push(text),
                    "project/parent/groupId" => parent_group_id = Some(text),
                    "project/parent/version" => parent_version = Some(text),
                    _ => {}
                }
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(Error::Validation(format!("Invalid Maven POM XML: {error}")));
            }
            _ => {}
        }

        buf.clear();
    }

    let artifact_id = artifact_id
        .ok_or_else(|| Error::Validation("Missing <artifactId> element in Maven POM".into()))?;
    let group_id = group_id.or(parent_group_id).ok_or_else(|| {
        Error::Validation(
            "Missing <groupId> element in Maven POM (and no <parent><groupId> fallback was provided)".into(),
        )
    })?;
    let version = version.or(parent_version).ok_or_else(|| {
        Error::Validation(
            "Missing <version> element in Maven POM (and no <parent><version> fallback was provided)".into(),
        )
    })?;

    Ok(ParsedPom {
        group_id,
        artifact_id,
        version,
        packaging: normalize_optional_field(packaging),
        display_name: normalize_optional_field(display_name),
        description: normalize_optional_field(description),
        homepage: normalize_optional_field(homepage),
        repository_url: normalize_optional_field(repository_url),
        licenses: normalize_string_list(licenses),
    })
}

fn classify_upload_filename(
    artifact_id: &str,
    version: &str,
    filename: &str,
) -> Result<(MavenUploadRole, ArtifactKind, String)> {
    validate_filename(filename)?;

    if let Some((target_filename, algorithm)) = strip_checksum_suffix(filename) {
        validate_primary_filename(artifact_id, version, target_filename)?;
        return Ok((
            MavenUploadRole::Checksum {
                target_filename: target_filename.to_owned(),
                algorithm,
            },
            ArtifactKind::Checksum,
            "text/plain; charset=utf-8".into(),
        ));
    }

    if let Some(target_filename) = filename.strip_suffix(".asc") {
        validate_primary_filename(artifact_id, version, target_filename)?;
        return Ok((
            MavenUploadRole::Signature {
                target_filename: target_filename.to_owned(),
            },
            ArtifactKind::Signature,
            "application/pgp-signature".into(),
        ));
    }

    validate_primary_filename(artifact_id, version, filename)?;

    if filename.ends_with(".pom") {
        return Ok((
            MavenUploadRole::Pom,
            ArtifactKind::Pom,
            "application/xml; charset=utf-8".into(),
        ));
    }

    Ok((
        MavenUploadRole::Primary,
        ArtifactKind::Jar,
        content_type_for_primary_filename(filename),
    ))
}

fn validate_primary_filename(artifact_id: &str, version: &str, filename: &str) -> Result<()> {
    let expected_prefix = format!("{artifact_id}-{version}");
    let Some(suffix) = filename.strip_prefix(&expected_prefix) else {
        return Err(Error::Validation(format!(
            "The uploaded filename '{filename}' does not match the requested Maven coordinates"
        )));
    };

    if suffix.is_empty() {
        return Err(Error::Validation(format!(
            "The uploaded filename '{filename}' is missing a file extension"
        )));
    }

    if let Some(extension) = suffix.strip_prefix('.') {
        if extension.is_empty() || extension.starts_with('.') {
            return Err(Error::Validation(format!(
                "The uploaded filename '{filename}' is not a valid Maven artifact filename"
            )));
        }
        return Ok(());
    }

    if let Some(classified) = suffix.strip_prefix('-') {
        if classified.is_empty() || !classified.contains('.') || classified.starts_with('.') {
            return Err(Error::Validation(format!(
                "The uploaded filename '{filename}' is not a valid classified Maven artifact filename"
            )));
        }
        return Ok(());
    }

    Err(Error::Validation(format!(
        "The uploaded filename '{filename}' is not a valid Maven artifact filename"
    )))
}

fn validate_filename(filename: &str) -> Result<()> {
    if filename.trim().is_empty() {
        return Err(Error::Validation(
            "Artifact filename must not be empty".into(),
        ));
    }

    if filename.contains('/') || filename.contains('\\') {
        return Err(Error::Validation(
            "Artifact filename must not contain path separators".into(),
        ));
    }

    if filename.chars().any(|character| character.is_control()) {
        return Err(Error::Validation(
            "Artifact filename must not contain control characters".into(),
        ));
    }

    Ok(())
}

fn strip_checksum_suffix(filename: &str) -> Option<(&str, ChecksumAlgorithm)> {
    filename
        .strip_suffix(".md5")
        .map(|target| (target, ChecksumAlgorithm::Md5))
        .or_else(|| {
            filename
                .strip_suffix(".sha1")
                .map(|target| (target, ChecksumAlgorithm::Sha1))
        })
        .or_else(|| {
            filename
                .strip_suffix(".sha256")
                .map(|target| (target, ChecksumAlgorithm::Sha256))
        })
        .or_else(|| {
            filename
                .strip_suffix(".sha512")
                .map(|target| (target, ChecksumAlgorithm::Sha512))
        })
}

fn content_type_for_primary_filename(filename: &str) -> String {
    if filename.ends_with(".module") {
        "application/json".into()
    } else if filename.ends_with(".pom") {
        "application/xml; charset=utf-8".into()
    } else if filename.ends_with(".jar")
        || filename.ends_with(".war")
        || filename.ends_with(".ear")
        || filename.ends_with(".aar")
    {
        "application/java-archive".into()
    } else {
        "application/octet-stream".into()
    }
}

fn build_provenance(pom: &ParsedPom) -> serde_json::Value {
    serde_json::json!({
        "source": "maven_deploy",
        "group_id": pom.group_id,
        "artifact_id": pom.artifact_id,
        "version": pom.version,
        "packaging": pom.packaging,
        "display_name": pom.display_name,
        "description": pom.description,
        "homepage": pom.homepage,
        "repository_url": pom.repository_url,
        "licenses": pom.licenses,
    })
}

fn looks_prerelease(version: &str) -> bool {
    version.contains('-')
        || version
            .chars()
            .any(|character| character.is_ascii_alphabetic())
}

fn local_name_str(name: &[u8]) -> String {
    let name = std::str::from_utf8(name).unwrap_or_default();
    if let Some(index) = name.rfind(':') {
        name[index + 1..].to_owned()
    } else {
        name.to_owned()
    }
}

fn normalize_optional_field(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();

    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }

        if normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }

        normalized.push(trimmed.to_owned());
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_root_coordinates_from_pom() {
        let parsed = parse_pom_xml(
            br#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
            <project xmlns=\"http://maven.apache.org/POM/4.0.0\">
              <modelVersion>4.0.0</modelVersion>
              <groupId>com.example</groupId>
              <artifactId>demo</artifactId>
              <version>1.2.3</version>
              <name>Demo</name>
              <description>Test package</description>
              <url>https://packages.example.test/demo</url>
              <licenses>
                <license><name>Apache-2.0</name></license>
                <license><name>MIT</name></license>
              </licenses>
              <scm><url>https://git.example.test/demo</url></scm>
            </project>"#,
        )
        .expect("pom should parse");

        assert_eq!(parsed.group_id, "com.example");
        assert_eq!(parsed.artifact_id, "demo");
        assert_eq!(parsed.version, "1.2.3");
        assert_eq!(parsed.display_name.as_deref(), Some("Demo"));
        assert_eq!(parsed.description.as_deref(), Some("Test package"));
        assert_eq!(
            parsed.homepage.as_deref(),
            Some("https://packages.example.test/demo")
        );
        assert_eq!(
            parsed.repository_url.as_deref(),
            Some("https://git.example.test/demo")
        );
        assert_eq!(parsed.licenses, vec!["Apache-2.0", "MIT"]);
    }

    #[test]
    fn parses_parent_fallback_coordinates_from_pom() {
        let parsed = parse_pom_xml(
            br#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
            <project xmlns=\"http://maven.apache.org/POM/4.0.0\">
              <modelVersion>4.0.0</modelVersion>
              <parent>
                <groupId>com.example</groupId>
                <artifactId>parent</artifactId>
                <version>9.9.9</version>
              </parent>
              <artifactId>demo</artifactId>
            </project>"#,
        )
        .expect("pom should parse with parent fallback");

        assert_eq!(parsed.group_id, "com.example");
        assert_eq!(parsed.artifact_id, "demo");
        assert_eq!(parsed.version, "9.9.9");
    }

    #[test]
    fn rejects_mismatched_pom_coordinates() {
        let error = parse_maven_upload(
            "com.example",
            "demo",
            "1.2.3",
            "demo-1.2.3.pom",
            Bytes::from_static(
                br#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
                <project xmlns=\"http://maven.apache.org/POM/4.0.0\">
                  <modelVersion>4.0.0</modelVersion>
                  <groupId>com.example</groupId>
                  <artifactId>other</artifactId>
                  <version>1.2.3</version>
                </project>"#,
            ),
        )
        .expect_err("mismatched pom should fail");

        assert!(error.to_string().contains("do not match the request path"));
    }

    #[test]
    fn rejects_snapshot_versions() {
        let error = parse_maven_upload(
            "com.example",
            "demo",
            "1.2.3-SNAPSHOT",
            "demo-1.2.3-SNAPSHOT.jar",
            Bytes::from_static(b"jar"),
        )
        .expect_err("snapshots should be rejected");

        assert!(error.to_string().contains("Snapshots are not supported"));
    }

    #[test]
    fn classifies_checksum_and_signature_filenames() {
        let checksum = parse_maven_upload(
            "com.example",
            "demo",
            "1.2.3",
            "demo-1.2.3.jar.sha256",
            Bytes::from_static(b"deadbeef"),
        )
        .expect("checksum upload should parse");
        assert!(matches!(
            checksum.role,
            MavenUploadRole::Checksum {
                target_filename,
                algorithm: ChecksumAlgorithm::Sha256,
            } if target_filename == "demo-1.2.3.jar"
        ));

        let signature = parse_maven_upload(
            "com.example",
            "demo",
            "1.2.3",
            "demo-1.2.3.jar.asc",
            Bytes::from_static(b"signature"),
        )
        .expect("signature upload should parse");
        assert!(matches!(
            signature.role,
            MavenUploadRole::Signature { target_filename } if target_filename == "demo-1.2.3.jar"
        ));
    }

    #[test]
    fn accepts_classified_artifacts() {
        let parsed = parse_maven_upload(
            "com.example",
            "demo",
            "1.2.3",
            "demo-1.2.3-sources.jar",
            Bytes::from_static(b"sources"),
        )
        .expect("classified artifact should parse");

        assert!(matches!(parsed.role, MavenUploadRole::Primary));
        assert_eq!(parsed.filename, "demo-1.2.3-sources.jar");
    }
}
