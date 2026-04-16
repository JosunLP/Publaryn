use base64::engine::{general_purpose::STANDARD as BASE64, Engine};
use bytes::Bytes;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use publaryn_core::error::{Error, Result};

/// The top-level JSON document that `npm publish` sends via `PUT /:package`.
///
/// This is sometimes called the "fat manifest" — it embeds the tarball as a
/// base64-encoded attachment alongside the version metadata.
#[derive(Debug, Deserialize)]
pub struct NpmPublishPayload {
    /// Package name (must match the URL path).
    pub name: String,

    /// Versions being published. In practice this map contains exactly one
    /// entry for a normal `npm publish`.
    #[serde(default)]
    pub versions: HashMap<String, Value>,

    /// Dist-tags to set (e.g. `{"latest": "1.0.0"}`).
    #[serde(default, rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,

    /// Attached tarballs keyed by filename (e.g.
    /// `"my-pkg-1.0.0.tgz": { "content_type": "...", "data": "<base64>" }`).
    #[serde(default, rename = "_attachments")]
    pub attachments: HashMap<String, NpmAttachment>,

    /// Description from the `npm publish` CLI.
    pub description: Option<String>,

    /// README content.
    pub readme: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NpmAttachment {
    pub content_type: Option<String>,
    pub data: String,
    pub length: Option<u64>,
}

/// Parsed and validated result of an npm publish request.
#[derive(Debug)]
pub struct ParsedPublish {
    pub package_name: String,
    pub version: String,
    pub version_metadata: Value,
    pub dist_tags: HashMap<String, String>,
    pub tarball_filename: String,
    pub tarball_bytes: Bytes,
    pub tarball_content_type: String,
    pub description: Option<String>,
    pub readme: Option<String>,
}

/// Parse and validate the npm publish payload.
///
/// Returns the extracted version, tarball bytes, and metadata needed to
/// create a release and upload the artifact through the domain model.
pub fn parse_publish_payload(payload: NpmPublishPayload) -> Result<ParsedPublish> {
    if payload.versions.is_empty() {
        return Err(Error::Validation(
            "Publish payload must contain at least one version".into(),
        ));
    }

    if payload.versions.len() > 1 {
        return Err(Error::Validation(
            "Publishing multiple versions in a single request is not supported".into(),
        ));
    }

    let (version, version_metadata) = payload.versions.into_iter().next().unwrap();

    if version.is_empty() {
        return Err(Error::Validation("Version string must not be empty".into()));
    }

    if payload.attachments.is_empty() {
        return Err(Error::Validation(
            "Publish payload must contain a tarball attachment".into(),
        ));
    }

    let (tarball_filename, attachment) = payload.attachments.into_iter().next().unwrap();

    let tarball_bytes = BASE64
        .decode(attachment.data.as_bytes())
        .map_err(|e| Error::Validation(format!("Invalid base64 in tarball attachment: {e}")))?;

    if tarball_bytes.is_empty() {
        return Err(Error::Validation(
            "Tarball attachment must not be empty".into(),
        ));
    }

    let tarball_content_type = attachment
        .content_type
        .unwrap_or_else(|| "application/octet-stream".to_owned());

    Ok(ParsedPublish {
        package_name: payload.name,
        version,
        version_metadata,
        dist_tags: payload.dist_tags,
        tarball_filename,
        tarball_bytes: Bytes::from(tarball_bytes),
        tarball_content_type,
        description: payload.description,
        readme: payload.readme,
    })
}

/// Extract commonly useful fields from the version metadata object.
pub struct VersionFields {
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub keywords: Vec<String>,
}

pub fn extract_version_fields(metadata: &Value) -> VersionFields {
    let description = metadata
        .get("description")
        .and_then(Value::as_str)
        .map(String::from);
    let license = metadata
        .get("license")
        .and_then(Value::as_str)
        .map(String::from);
    let homepage = metadata
        .get("homepage")
        .and_then(Value::as_str)
        .map(String::from);
    let repository_url = metadata.get("repository").and_then(|v| {
        if let Some(s) = v.as_str() {
            Some(s.to_owned())
        } else {
            v.get("url").and_then(Value::as_str).map(String::from)
        }
    });
    let keywords = metadata
        .get("keywords")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    VersionFields {
        description,
        license,
        homepage,
        repository_url,
        keywords,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_payload(name: &str, version: &str, tarball_data: &[u8]) -> NpmPublishPayload {
        let b64 = BASE64.encode(tarball_data);
        let filename = format!("{name}-{version}.tgz");
        NpmPublishPayload {
            name: name.to_owned(),
            versions: HashMap::from([(
                version.to_owned(),
                json!({"name": name, "version": version}),
            )]),
            dist_tags: HashMap::from([("latest".to_owned(), version.to_owned())]),
            attachments: HashMap::from([(
                filename,
                NpmAttachment {
                    content_type: Some("application/gzip".to_owned()),
                    data: b64,
                    length: Some(tarball_data.len() as u64),
                },
            )]),
            description: Some("Test package".to_owned()),
            readme: None,
        }
    }

    #[test]
    fn parse_valid_publish() {
        let payload = make_payload("test-pkg", "1.0.0", b"fake-tarball-bytes");
        let result = parse_publish_payload(payload).unwrap();
        assert_eq!(result.package_name, "test-pkg");
        assert_eq!(result.version, "1.0.0");
        assert_eq!(result.tarball_bytes.as_ref(), b"fake-tarball-bytes");
        assert_eq!(*result.dist_tags.get("latest").unwrap(), "1.0.0");
    }

    #[test]
    fn reject_empty_versions() {
        let payload = NpmPublishPayload {
            name: "test".to_owned(),
            versions: HashMap::new(),
            dist_tags: HashMap::new(),
            attachments: HashMap::new(),
            description: None,
            readme: None,
        };
        assert!(parse_publish_payload(payload).is_err());
    }

    #[test]
    fn reject_empty_tarball() {
        let payload = NpmPublishPayload {
            name: "test".to_owned(),
            versions: HashMap::from([("1.0.0".to_owned(), json!({}))]),
            dist_tags: HashMap::new(),
            attachments: HashMap::from([(
                "test-1.0.0.tgz".to_owned(),
                NpmAttachment {
                    content_type: None,
                    data: BASE64.encode(b""),
                    length: None,
                },
            )]),
            description: None,
            readme: None,
        };
        assert!(parse_publish_payload(payload).is_err());
    }

    #[test]
    fn extract_fields_from_version_metadata() {
        let meta = json!({
            "description": "A cool package",
            "license": "MIT",
            "homepage": "https://example.com",
            "repository": { "url": "https://github.com/user/repo" },
            "keywords": ["cool", "package"]
        });
        let fields = extract_version_fields(&meta);
        assert_eq!(fields.description.as_deref(), Some("A cool package"));
        assert_eq!(fields.license.as_deref(), Some("MIT"));
        assert_eq!(fields.homepage.as_deref(), Some("https://example.com"));
        assert_eq!(
            fields.repository_url.as_deref(),
            Some("https://github.com/user/repo")
        );
        assert_eq!(fields.keywords, vec!["cool", "package"]);
    }
}
