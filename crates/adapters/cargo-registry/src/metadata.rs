//! NDJSON index entry builder for the Cargo sparse index.
//!
//! Each crate in the sparse index is represented as a file containing one JSON
//! object per line (NDJSON). Each line represents a single published version.

use serde::{Deserialize, Serialize};

use crate::publish::CargoIndexDep;

/// A single version entry in the Cargo sparse index.
///
/// One of these is serialized per line in the index file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub vers: String,
    pub deps: Vec<CargoIndexDep>,
    pub cksum: String,
    pub features: serde_json::Map<String, serde_json::Value>,
    pub yanked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
    /// Schema version — always 2 for modern registries.
    pub v: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features2: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_version: Option<String>,
}

/// Input data for building an index entry, collected from the database.
#[derive(Debug, Clone)]
pub struct VersionIndexInput {
    pub name: String,
    pub version: String,
    pub deps: Vec<CargoIndexDep>,
    pub features: serde_json::Map<String, serde_json::Value>,
    pub features2: Option<serde_json::Map<String, serde_json::Value>>,
    pub cksum: String,
    pub yanked: bool,
    pub links: Option<String>,
    pub rust_version: Option<String>,
}

/// Build the NDJSON index content for a crate from a list of version inputs.
///
/// Returns the full text content (one JSON object per line, no trailing newline
/// on the last line) and a deterministic ETag (SHA-256 of the content).
pub fn build_index_content(versions: &[VersionIndexInput]) -> (String, String) {
    let mut lines = Vec::with_capacity(versions.len());

    for v in versions {
        let entry = IndexEntry {
            name: v.name.clone(),
            vers: v.version.clone(),
            deps: v.deps.clone(),
            cksum: v.cksum.clone(),
            features: v.features.clone(),
            yanked: v.yanked,
            links: v.links.clone(),
            v: 2,
            features2: v.features2.clone(),
            rust_version: v.rust_version.clone(),
        };

        // Each line is compact JSON (no pretty-printing)
        if let Ok(json) = serde_json::to_string(&entry) {
            lines.push(json);
        }
    }

    let content = lines.join("\n");
    let etag = sha2_hex(&content);
    (content, etag)
}

fn sha2_hex(data: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_version(name: &str, vers: &str, yanked: bool) -> VersionIndexInput {
        VersionIndexInput {
            name: name.into(),
            version: vers.into(),
            deps: vec![],
            features: serde_json::Map::new(),
            features2: None,
            cksum: "abc123".into(),
            yanked,
            links: None,
            rust_version: None,
        }
    }

    #[test]
    fn single_version_entry() {
        let versions = vec![make_version("my-crate", "0.1.0", false)];
        let (content, etag) = build_index_content(&versions);

        assert!(!content.is_empty());
        assert!(!etag.is_empty());
        assert!(
            !content.contains('\n'),
            "single entry should not contain newlines"
        );

        let entry: IndexEntry = serde_json::from_str(&content).unwrap();
        assert_eq!(entry.name, "my-crate");
        assert_eq!(entry.vers, "0.1.0");
        assert_eq!(entry.v, 2);
        assert!(!entry.yanked);
    }

    #[test]
    fn multiple_versions() {
        let versions = vec![
            make_version("my-crate", "0.1.0", false),
            make_version("my-crate", "0.2.0", false),
            make_version("my-crate", "0.3.0", true),
        ];
        let (content, _) = build_index_content(&versions);

        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);

        let v1: IndexEntry = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v1.vers, "0.1.0");

        let v3: IndexEntry = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(v3.vers, "0.3.0");
        assert!(v3.yanked);
    }

    #[test]
    fn etag_is_deterministic() {
        let versions = vec![make_version("foo", "1.0.0", false)];
        let (_, etag1) = build_index_content(&versions);
        let (_, etag2) = build_index_content(&versions);
        assert_eq!(etag1, etag2);
    }

    #[test]
    fn etag_changes_on_different_content() {
        let v1 = vec![make_version("foo", "1.0.0", false)];
        let v2 = vec![make_version("foo", "1.0.0", true)];
        let (_, etag1) = build_index_content(&v1);
        let (_, etag2) = build_index_content(&v2);
        assert_ne!(etag1, etag2);
    }

    #[test]
    fn with_dependencies() {
        let versions = vec![VersionIndexInput {
            name: "my-crate".into(),
            version: "0.1.0".into(),
            deps: vec![CargoIndexDep {
                name: "serde".into(),
                req: "^1.0".into(),
                features: vec!["derive".into()],
                optional: false,
                default_features: true,
                target: None,
                kind: "normal".into(),
                registry: None,
                package: None,
            }],
            features: {
                let mut m = serde_json::Map::new();
                m.insert("serde".into(), serde_json::json!(["dep:serde"]));
                m
            },
            features2: None,
            cksum: "deadbeef".into(),
            yanked: false,
            links: None,
            rust_version: Some("1.60".into()),
        }];

        let (content, _) = build_index_content(&versions);
        let entry: IndexEntry = serde_json::from_str(&content).unwrap();

        assert_eq!(entry.deps.len(), 1);
        assert_eq!(entry.deps[0].name, "serde");
        assert_eq!(entry.rust_version.as_deref(), Some("1.60"));
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let (content, _) = build_index_content(&[]);
        assert!(content.is_empty());
    }
}
