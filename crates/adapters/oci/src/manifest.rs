use bytes::Bytes;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256, Sha512};

use publaryn_core::error::{Error, Result};

use crate::name::validate_digest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ManifestReferenceKind {
    Config,
    Layer,
    Subject,
}

impl ManifestReferenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Layer => "layer",
            Self::Subject => "subject",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestReference {
    pub digest: String,
    pub kind: ManifestReferenceKind,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ParsedManifest {
    pub bytes: Bytes,
    pub digest: String,
    pub sha256: String,
    pub sha512: String,
    pub size_bytes: i64,
    pub content_type: String,
    pub media_type: Option<String>,
    pub references: Vec<ManifestReference>,
    pub raw: Value,
}

pub fn parse_manifest(bytes: Bytes, content_type: Option<&str>) -> Result<ParsedManifest> {
    if bytes.is_empty() {
        return Err(Error::Validation(
            "OCI manifests must not be empty".into(),
        ));
    }

    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Validation(format!("Invalid OCI manifest JSON: {error}")))?;
    let object = raw
        .as_object()
        .ok_or_else(|| Error::Validation("OCI manifests must be JSON objects".into()))?;

    let mut references = Vec::new();
    if let Some(config) = object.get("config") {
        if !config.is_null() {
            references.push(parse_descriptor(config, ManifestReferenceKind::Config)?);
        }
    }

    if let Some(layers) = object.get("layers").and_then(Value::as_array) {
        for layer in layers {
            references.push(parse_descriptor(layer, ManifestReferenceKind::Layer)?);
        }
    }

    if let Some(blobs) = object.get("blobs").and_then(Value::as_array) {
        for blob in blobs {
            references.push(parse_descriptor(blob, ManifestReferenceKind::Layer)?);
        }
    }

    if let Some(manifests) = object.get("manifests").and_then(Value::as_array) {
        for manifest in manifests {
            references.push(parse_descriptor(manifest, ManifestReferenceKind::Layer)?);
        }
    }

    if let Some(subject) = object.get("subject") {
        if !subject.is_null() {
            references.push(parse_descriptor(subject, ManifestReferenceKind::Subject)?);
        }
    }

    let looks_like_manifest = object.contains_key("config")
        || object.contains_key("layers")
        || object.contains_key("blobs")
        || object.contains_key("manifests")
        || object.contains_key("subject");
    if !looks_like_manifest {
        return Err(Error::Validation(
            "The uploaded JSON does not look like a supported OCI manifest".into(),
        ));
    }

    let sha256 = hex::encode(Sha256::digest(&bytes));
    let sha512 = hex::encode(Sha512::digest(&bytes));
    let digest = format!("sha256:{sha256}");
    let size_bytes = i64::try_from(bytes.len()).map_err(|_| {
        Error::Validation("OCI manifests exceed supported size limits".into())
    })?;

    let media_type = object
        .get("mediaType")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let content_type = content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| media_type.clone())
        .unwrap_or_else(|| "application/vnd.oci.image.manifest.v1+json".into());

    Ok(ParsedManifest {
        bytes,
        digest,
        sha256,
        sha512,
        size_bytes,
        content_type,
        media_type,
        references,
        raw,
    })
}

fn parse_descriptor(value: &Value, kind: ManifestReferenceKind) -> Result<ManifestReference> {
    let object = value.as_object().ok_or_else(|| {
        Error::Validation(format!(
            "OCI {} descriptors must be JSON objects",
            kind.as_str()
        ))
    })?;

    let digest = object
        .get("digest")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Validation(format!(
                "OCI {} descriptors must define a digest",
                kind.as_str()
            ))
        })?;
    let digest = validate_digest(digest)?;

    let size_bytes = object
        .get("size")
        .and_then(Value::as_i64)
        .or_else(|| {
            object
                .get("size")
                .and_then(Value::as_u64)
                .and_then(|value| i64::try_from(value).ok())
        });

    Ok(ManifestReference {
        digest,
        kind,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_image_manifest_references() {
        let parsed = parse_manifest(
            Bytes::from_static(
                br#"{
                    "schemaVersion": 2,
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "config": {
                        "mediaType": "application/vnd.oci.image.config.v1+json",
                        "digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                        "size": 42
                    },
                    "layers": [
                        {
                            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                            "digest": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
                            "size": 99
                        }
                    ]
                }"#,
            ),
            Some("application/vnd.oci.image.manifest.v1+json"),
        )
        .expect("manifest should parse");

        assert_eq!(parsed.references.len(), 2);
        assert_eq!(parsed.references[0].kind, ManifestReferenceKind::Config);
        assert_eq!(parsed.references[1].kind, ManifestReferenceKind::Layer);
        assert_eq!(parsed.content_type, "application/vnd.oci.image.manifest.v1+json");
    }

    #[test]
    fn parses_artifact_manifest_references() {
        let parsed = parse_manifest(
            Bytes::from_static(
                br#"{
                    "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
                    "artifactType": "application/vnd.cncf.helm.chart.v1.tar+gzip",
                    "blobs": [
                        {
                            "mediaType": "application/octet-stream",
                            "digest": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
                            "size": 123
                        }
                    ]
                }"#,
            ),
            Some("application/vnd.oci.artifact.manifest.v1+json"),
        )
        .expect("artifact manifest should parse");

        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].kind, ManifestReferenceKind::Layer);
    }
}
