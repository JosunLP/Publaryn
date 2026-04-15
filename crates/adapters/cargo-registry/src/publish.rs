//! Parser for the Cargo publish binary wire format.
//!
//! When `cargo publish` sends a crate to a registry, the request body is:
//!
//! ```text
//! [u32 LE: json_len][json_bytes][u32 LE: crate_len][crate_bytes]
//! ```
//!
//! This module parses that format and deserializes the JSON metadata.

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use publaryn_core::error::{Error, Result};

/// Maximum JSON metadata size (1 MiB, generous but bounded).
const MAX_JSON_SIZE: u32 = 1_048_576;

/// Maximum .crate file size (512 MiB).
const MAX_CRATE_SIZE: u32 = 536_870_912;

// ─── Wire format types ───────────────────────────────────────────────────────

/// JSON metadata sent by `cargo publish` inside the binary payload.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CargoPublishMetadata {
    pub name: String,
    pub vers: String,
    #[serde(default)]
    pub deps: Vec<CargoPublishDep>,
    #[serde(default)]
    pub features: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub documentation: Option<String>,
    pub homepage: Option<String>,
    pub readme: Option<String>,
    pub readme_file: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub repository: Option<String>,
    #[serde(default)]
    pub badges: serde_json::Map<String, serde_json::Value>,
    pub links: Option<String>,
    pub rust_version: Option<String>,
}

/// A dependency as sent in the publish metadata.
///
/// Note: the publish format uses `version_req` and `explicit_name_in_toml`
/// while the index format uses `req` and `package`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CargoPublishDep {
    pub name: String,
    pub version_req: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default = "default_true")]
    pub default_features: bool,
    pub target: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub registry: Option<String>,
    /// If the dependency is renamed, this is the original crate name.
    pub explicit_name_in_toml: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_kind() -> String {
    "normal".into()
}

// ─── Index-format dependency (for storage and index serving) ─────────────────

/// A dependency in the Cargo index format (differs from publish format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoIndexDep {
    pub name: String,
    pub req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    pub kind: String,
    pub registry: Option<String>,
    /// Original crate name if renamed, else `null`.
    pub package: Option<String>,
}

impl From<&CargoPublishDep> for CargoIndexDep {
    fn from(dep: &CargoPublishDep) -> Self {
        Self {
            // If the dep was renamed, the `name` field in the publish payload is the
            // *renamed* name (as used in Cargo.toml [dependencies]), while
            // `explicit_name_in_toml` holds the rename and the true crate name goes
            // into `name`. In the index format, `name` is always the true crate name
            // and `package` is the rename alias.
            name: dep
                .explicit_name_in_toml
                .as_ref()
                .map(|_| dep.name.clone())
                .unwrap_or_else(|| dep.name.clone()),
            req: dep.version_req.clone(),
            features: dep.features.clone(),
            optional: dep.optional,
            default_features: dep.default_features,
            target: dep.target.clone(),
            kind: dep.kind.clone(),
            registry: dep.registry.clone(),
            package: dep.explicit_name_in_toml.clone(),
        }
    }
}

// ─── Parsed publish result ───────────────────────────────────────────────────

/// Fully parsed and validated result of a Cargo publish request.
#[derive(Debug)]
pub struct ParsedCargoPublish {
    pub metadata: CargoPublishMetadata,
    /// Dependencies converted to index format for storage.
    pub index_deps: Vec<CargoIndexDep>,
    /// Raw `.crate` file bytes.
    pub crate_bytes: Bytes,
    /// SHA-256 hex digest of the `.crate` file.
    pub sha256: String,
}

// ─── Parser ──────────────────────────────────────────────────────────────────

/// Parse the Cargo publish binary wire format.
///
/// Returns the parsed metadata, index-format dependencies, raw `.crate` bytes,
/// and the SHA-256 checksum.
pub fn parse_cargo_publish(body: &[u8]) -> Result<ParsedCargoPublish> {
    if body.len() < 4 {
        return Err(Error::Validation(
            "Publish payload too short: missing JSON length prefix".into(),
        ));
    }

    // Read JSON length (u32 LE)
    let json_len = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);

    if json_len > MAX_JSON_SIZE {
        return Err(Error::Validation(format!(
            "JSON metadata too large: {json_len} bytes (max {MAX_JSON_SIZE})"
        )));
    }

    let json_start = 4usize;
    let json_end = json_start
        .checked_add(json_len as usize)
        .ok_or_else(|| Error::Validation("JSON length overflow".into()))?;

    if body.len() < json_end + 4 {
        return Err(Error::Validation(
            "Publish payload truncated: missing crate data length prefix".into(),
        ));
    }

    let json_bytes = &body[json_start..json_end];

    // Read crate length (u32 LE)
    let crate_len_start = json_end;
    let crate_len = u32::from_le_bytes([
        body[crate_len_start],
        body[crate_len_start + 1],
        body[crate_len_start + 2],
        body[crate_len_start + 3],
    ]);

    if crate_len > MAX_CRATE_SIZE {
        return Err(Error::Validation(format!(
            ".crate file too large: {crate_len} bytes (max {MAX_CRATE_SIZE})"
        )));
    }

    let crate_start = crate_len_start + 4;
    let crate_end = crate_start
        .checked_add(crate_len as usize)
        .ok_or_else(|| Error::Validation("Crate length overflow".into()))?;

    if body.len() < crate_end {
        return Err(Error::Validation(
            "Publish payload truncated: .crate data shorter than declared".into(),
        ));
    }

    if crate_len == 0 {
        return Err(Error::Validation(
            ".crate file must not be empty".into(),
        ));
    }

    let crate_bytes = Bytes::copy_from_slice(&body[crate_start..crate_end]);

    // Deserialize JSON metadata
    let metadata: CargoPublishMetadata = serde_json::from_slice(json_bytes)
        .map_err(|e| Error::Validation(format!("Invalid publish metadata JSON: {e}")))?;

    if metadata.name.is_empty() {
        return Err(Error::Validation(
            "Crate name in metadata must not be empty".into(),
        ));
    }

    if metadata.vers.is_empty() {
        return Err(Error::Validation(
            "Version in metadata must not be empty".into(),
        ));
    }

    // Convert dependencies to index format
    let index_deps: Vec<CargoIndexDep> = metadata.deps.iter().map(CargoIndexDep::from).collect();

    // Compute SHA-256
    let sha256 = hex::encode(Sha256::digest(&crate_bytes));

    Ok(ParsedCargoPublish {
        metadata,
        index_deps,
        crate_bytes,
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a valid wire-format payload from JSON metadata and fake .crate bytes.
    fn build_payload(json: &[u8], crate_data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
        buf.extend_from_slice(json);
        buf.extend_from_slice(&(crate_data.len() as u32).to_le_bytes());
        buf.extend_from_slice(crate_data);
        buf
    }

    fn minimal_json() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "name": "my-crate",
            "vers": "0.1.0",
            "deps": [],
            "features": {},
            "authors": ["Test <test@example.com>"],
            "description": "A test crate",
            "license": "MIT"
        }))
        .unwrap()
    }

    #[test]
    fn parse_valid_payload() {
        let json = minimal_json();
        let crate_data = b"fake-crate-tarball-bytes";
        let payload = build_payload(&json, crate_data);

        let result = parse_cargo_publish(&payload).unwrap();
        assert_eq!(result.metadata.name, "my-crate");
        assert_eq!(result.metadata.vers, "0.1.0");
        assert_eq!(result.crate_bytes.as_ref(), crate_data);
        assert!(!result.sha256.is_empty());
    }

    #[test]
    fn parse_with_dependencies() {
        let json = serde_json::to_vec(&serde_json::json!({
            "name": "my-crate",
            "vers": "0.1.0",
            "deps": [{
                "name": "serde",
                "version_req": "^1.0",
                "features": ["derive"],
                "optional": false,
                "default_features": true,
                "target": null,
                "kind": "normal",
                "registry": null,
                "explicit_name_in_toml": null
            }],
            "features": {
                "serde": ["dep:serde"]
            },
            "authors": [],
            "description": "test"
        }))
        .unwrap();
        let payload = build_payload(&json, b"crate-data");

        let result = parse_cargo_publish(&payload).unwrap();
        assert_eq!(result.index_deps.len(), 1);
        assert_eq!(result.index_deps[0].name, "serde");
        assert_eq!(result.index_deps[0].req, "^1.0");
        assert!(result.index_deps[0].features.contains(&"derive".to_owned()));
    }

    #[test]
    fn parse_renamed_dependency() {
        let json = serde_json::to_vec(&serde_json::json!({
            "name": "my-crate",
            "vers": "0.1.0",
            "deps": [{
                "name": "custom_serde",
                "version_req": "^1.0",
                "features": [],
                "optional": false,
                "default_features": true,
                "target": null,
                "kind": "normal",
                "registry": null,
                "explicit_name_in_toml": "serde"
            }],
            "features": {},
            "authors": []
        }))
        .unwrap();
        let payload = build_payload(&json, b"crate-data");

        let result = parse_cargo_publish(&payload).unwrap();
        assert_eq!(result.index_deps[0].name, "custom_serde");
        assert_eq!(result.index_deps[0].package.as_deref(), Some("serde"));
    }

    #[test]
    fn reject_too_short() {
        assert!(parse_cargo_publish(b"abc").is_err());
    }

    #[test]
    fn reject_truncated_json() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&100u32.to_le_bytes());
        buf.extend_from_slice(b"short");
        assert!(parse_cargo_publish(&buf).is_err());
    }

    #[test]
    fn reject_empty_crate() {
        let json = minimal_json();
        let payload = build_payload(&json, b"");
        assert!(parse_cargo_publish(&payload).is_err());
    }

    #[test]
    fn reject_empty_name() {
        let json = serde_json::to_vec(&serde_json::json!({
            "name": "",
            "vers": "0.1.0"
        }))
        .unwrap();
        let payload = build_payload(&json, b"data");
        assert!(parse_cargo_publish(&payload).is_err());
    }

    #[test]
    fn reject_empty_version() {
        let json = serde_json::to_vec(&serde_json::json!({
            "name": "my-crate",
            "vers": ""
        }))
        .unwrap();
        let payload = build_payload(&json, b"data");
        assert!(parse_cargo_publish(&payload).is_err());
    }

    #[test]
    fn sha256_is_computed() {
        let json = minimal_json();
        let crate_data = b"deterministic-content";
        let payload = build_payload(&json, crate_data);

        let result = parse_cargo_publish(&payload).unwrap();
        let expected = hex::encode(sha2::Sha256::digest(crate_data));
        assert_eq!(result.sha256, expected);
    }
}
