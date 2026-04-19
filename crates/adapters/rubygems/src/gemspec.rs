//! Minimal `.gem` parser.
//!
//! A `.gem` file is an uncompressed POSIX tar archive containing:
//!
//! - `metadata.gz` — gzipped YAML gemspec (we parse this).
//! - `data.tar.gz` — the gem's source tree (kept opaque).
//! - `checksums.yaml.gz` — optional.
//!
//! We extract only the metadata we need for registry display and
//! search: name, version, platform, summary, description, authors,
//! licenses, homepage, and dependency requirements.

use std::io::Read;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_yaml_ng as serde_yaml;

use publaryn_core::error::{Error, Result};

/// Maximum allowed `.gem` size (128 MiB).
pub const MAX_GEM_SIZE: usize = 128 * 1024 * 1024;

/// Maximum size of the decompressed metadata YAML (8 MiB).
const MAX_METADATA_SIZE: usize = 8 * 1024 * 1024;

/// Parsed gemspec metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemspecMetadata {
    pub name: String,
    pub version: String,
    /// Platform qualifier (`ruby`, `x86_64-linux`, `arm64-darwin`, …).
    pub platform: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub licenses: Vec<String>,
    pub homepage: Option<String>,
    pub required_ruby_version: Option<String>,
    pub required_rubygems_version: Option<String>,
    /// Runtime dependencies: `[ { name, requirement } ]`.
    pub runtime_dependencies: Vec<GemspecDependency>,
    /// Development dependencies.
    pub development_dependencies: Vec<GemspecDependency>,
    /// Free-form metadata key-value (gemspec `metadata` field).
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

/// A gemspec dependency declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemspecDependency {
    pub name: String,
    /// Concatenated requirement list (`>= 1.0, < 2.0`).
    pub requirement: String,
}

/// Parse a `.gem` tarball and extract gemspec metadata.
pub fn parse_gem(bytes: &[u8]) -> Result<GemspecMetadata> {
    if bytes.is_empty() {
        return Err(Error::Validation("Empty .gem payload".into()));
    }
    if bytes.len() > MAX_GEM_SIZE {
        return Err(Error::Validation(format!(
            ".gem exceeds maximum allowed size of {} MiB",
            MAX_GEM_SIZE / (1024 * 1024)
        )));
    }

    let mut archive = tar::Archive::new(std::io::Cursor::new(bytes));
    let entries = archive
        .entries()
        .map_err(|e| Error::Validation(format!("Invalid .gem archive: {e}")))?;

    for entry in entries {
        let mut entry = entry.map_err(|e| Error::Validation(format!("Invalid .gem entry: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| Error::Validation(format!("Invalid .gem entry path: {e}")))?
            .to_path_buf();

        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name != "metadata.gz" {
            continue;
        }

        let mut gz_bytes = Vec::new();
        entry
            .read_to_end(&mut gz_bytes)
            .map_err(|e| Error::Validation(format!("Failed reading metadata.gz: {e}")))?;

        let mut decoder = GzDecoder::new(std::io::Cursor::new(gz_bytes));
        let mut yaml_bytes = Vec::new();
        let mut limited = std::io::Read::take(&mut decoder, MAX_METADATA_SIZE as u64);
        limited
            .read_to_end(&mut yaml_bytes)
            .map_err(|e| Error::Validation(format!("Corrupt metadata.gz: {e}")))?;

        return parse_gemspec_yaml(&yaml_bytes);
    }

    Err(Error::Validation(
        "metadata.gz missing from .gem archive".into(),
    ))
}

/// Parse raw gemspec YAML bytes.
///
/// Ruby serializes gemspecs as YAML with `!ruby/object:…` tags. We
/// deserialize permissively into `serde_yaml_ng::Value` and extract the
/// fields we care about; unknown fields are ignored.
pub fn parse_gemspec_yaml(yaml: &[u8]) -> Result<GemspecMetadata> {
    let value: serde_yaml::Value = serde_yaml::from_slice(yaml)
        .map_err(|e| Error::Validation(format!("Invalid gemspec YAML: {e}")))?;

    let name = get_string(&value, "name")
        .ok_or_else(|| Error::Validation("gemspec is missing `name`".into()))?;

    // Version can be a string or a `!ruby/object:Gem::Version` with `version` field.
    let version = get_string(&value, "version")
        .or_else(|| {
            value
                .get("version")
                .and_then(|v| v.get("version"))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .ok_or_else(|| Error::Validation("gemspec is missing `version`".into()))?;

    let platform = match value.get("platform") {
        Some(serde_yaml::Value::String(s)) => s.clone(),
        Some(other) => other
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| "ruby".into()),
        None => "ruby".into(),
    };

    let summary = get_string(&value, "summary");
    let description = get_string(&value, "description");
    let homepage = get_string(&value, "homepage");

    let authors = yaml_string_list(&value, &["authors", "author"]);
    let licenses = yaml_string_list(&value, &["licenses", "license"]);

    let required_ruby_version = get_requirement_string(&value, "required_ruby_version");
    let required_rubygems_version = get_requirement_string(&value, "required_rubygems_version");

    let (runtime_dependencies, development_dependencies) = extract_dependencies(&value);

    let metadata = value
        .get("metadata")
        .and_then(yaml_value_to_json_map)
        .unwrap_or_default();

    Ok(GemspecMetadata {
        name,
        version,
        platform,
        summary,
        description,
        authors,
        licenses,
        homepage,
        required_ruby_version,
        required_rubygems_version,
        runtime_dependencies,
        development_dependencies,
        metadata,
    })
}

fn get_string(value: &serde_yaml::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}

fn yaml_string_list(value: &serde_yaml::Value, keys: &[&str]) -> Vec<String> {
    for key in keys {
        if let Some(seq) = value.get(*key).and_then(|v| v.as_sequence()) {
            let items: Vec<String> = seq
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .filter(|s| !s.is_empty())
                .collect();
            if !items.is_empty() {
                return items;
            }
        }
        if let Some(s) = value.get(*key).and_then(|v| v.as_str()) {
            return s
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
        }
    }
    Vec::new()
}

/// Parse `Gem::Requirement` YAML into a human-readable string such as
/// `>= 2.7, < 4`.
fn get_requirement_string(value: &serde_yaml::Value, key: &str) -> Option<String> {
    let req = value.get(key)?;
    // If it's a plain string, use it.
    if let Some(s) = req.as_str() {
        return Some(s.to_owned());
    }
    // Otherwise look for requirements: [ [op, {version: v}] , ... ]
    let seq = req
        .get("requirements")
        .and_then(|v| v.as_sequence())
        .or_else(|| req.as_sequence())?;

    let parts: Vec<String> = seq
        .iter()
        .filter_map(|item| {
            let arr = item.as_sequence()?;
            let op = arr.first()?.as_str()?;
            let ver = arr
                .get(1)
                .and_then(|v| v.get("version").and_then(|x| x.as_str()).or_else(|| v.as_str()))?;
            Some(format!("{op} {ver}"))
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn extract_dependencies(
    value: &serde_yaml::Value,
) -> (Vec<GemspecDependency>, Vec<GemspecDependency>) {
    let mut runtime = Vec::new();
    let mut development = Vec::new();

    let Some(deps) = value.get("dependencies").and_then(|v| v.as_sequence()) else {
        return (runtime, development);
    };

    for dep in deps {
        let Some(name) = dep.get("name").and_then(|v| v.as_str()) else {
            continue;
        };

        let requirement = dep
            .get("requirement")
            .or_else(|| dep.get("version_requirements"))
            .and_then(|req| {
                if let Some(s) = req.as_str() {
                    return Some(s.to_owned());
                }
                let seq = req
                    .get("requirements")
                    .and_then(|v| v.as_sequence())
                    .or_else(|| req.as_sequence())?;
                let parts: Vec<String> = seq
                    .iter()
                    .filter_map(|item| {
                        let arr = item.as_sequence()?;
                        let op = arr.first()?.as_str()?;
                        let ver = arr
                            .get(1)
                            .and_then(|v| {
                                v.get("version")
                                    .and_then(|x| x.as_str())
                                    .or_else(|| v.as_str())
                            })?;
                        Some(format!("{op} {ver}"))
                    })
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(", "))
                }
            })
            .unwrap_or_default();

        let kind = dep.get("type").and_then(|v| v.as_str()).unwrap_or(":runtime");
        let entry = GemspecDependency {
            name: name.to_owned(),
            requirement,
        };
        if kind.contains("development") {
            development.push(entry);
        } else {
            runtime.push(entry);
        }
    }

    (runtime, development)
}

fn yaml_value_to_json_map(
    value: &serde_yaml::Value,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let map = value.as_mapping()?;
    let mut out = serde_json::Map::new();
    for (k, v) in map {
        let key = k.as_str()?.to_owned();
        if let Some(json) = yaml_to_json(v) {
            out.insert(key, json);
        }
    }
    Some(out)
}

fn yaml_to_json(value: &serde_yaml::Value) -> Option<serde_json::Value> {
    Some(match value {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f).map_or(serde_json::Value::Null, Into::into)
            } else {
                return None;
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().filter_map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                if let (Some(k), Some(v)) = (k.as_str(), yaml_to_json(v)) {
                    obj.insert(k.to_owned(), v);
                }
            }
            serde_json::Value::Object(obj)
        }
        _ => return None,
    })
}

/// Compose the storage filename for a gem based on its gemspec metadata.
///
/// Convention: `{name}-{version}.gem` for ruby-platform gems;
/// `{name}-{version}-{platform}.gem` for native gems.
pub fn gem_filename(metadata: &GemspecMetadata) -> String {
    if metadata.platform == "ruby" || metadata.platform.is_empty() {
        format!("{}-{}.gem", metadata.name, metadata.version)
    } else {
        format!(
            "{}-{}-{}.gem",
            metadata.name, metadata.version, metadata.platform
        )
    }
}

/// Derive the `releases.version` string. For non-ruby platforms the
/// platform suffix is appended so the `(package_id, version)`
/// uniqueness constraint can distinguish platform variants.
pub fn release_version(metadata: &GemspecMetadata) -> String {
    if metadata.platform == "ruby" || metadata.platform.is_empty() {
        metadata.version.clone()
    } else {
        format!("{}-{}", metadata.version, metadata.platform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_GEMSPEC: &str = r#"--- !ruby/object:Gem::Specification
name: demo
version: !ruby/object:Gem::Version
  version: 1.2.3
platform: ruby
authors:
  - Alice
  - Bob
summary: A sample gem
description: Longer description here.
licenses:
  - MIT
homepage: https://example.com
required_ruby_version: !ruby/object:Gem::Requirement
  requirements:
    - - ">="
      - !ruby/object:Gem::Version
        version: '2.7'
dependencies:
  - !ruby/object:Gem::Dependency
    name: rake
    requirement: !ruby/object:Gem::Requirement
      requirements:
        - - "~>"
          - !ruby/object:Gem::Version
            version: '13.0'
    type: :development
  - !ruby/object:Gem::Dependency
    name: json
    requirement: !ruby/object:Gem::Requirement
      requirements:
        - - ">="
          - !ruby/object:Gem::Version
            version: '2.0'
    type: :runtime
metadata:
  homepage_uri: https://example.com
"#;

    #[test]
    fn parses_sample_gemspec() {
        let meta = parse_gemspec_yaml(SAMPLE_GEMSPEC.as_bytes()).expect("should parse");
        assert_eq!(meta.name, "demo");
        assert_eq!(meta.version, "1.2.3");
        assert_eq!(meta.platform, "ruby");
        assert_eq!(meta.authors, vec!["Alice", "Bob"]);
        assert_eq!(meta.licenses, vec!["MIT"]);
        assert_eq!(meta.homepage.as_deref(), Some("https://example.com"));
        assert_eq!(meta.required_ruby_version.as_deref(), Some(">= 2.7"));
        assert_eq!(meta.runtime_dependencies.len(), 1);
        assert_eq!(meta.runtime_dependencies[0].name, "json");
        assert_eq!(meta.runtime_dependencies[0].requirement, ">= 2.0");
        assert_eq!(meta.development_dependencies.len(), 1);
        assert_eq!(meta.development_dependencies[0].name, "rake");
    }

    #[test]
    fn release_version_appends_platform() {
        let mut meta = parse_gemspec_yaml(SAMPLE_GEMSPEC.as_bytes()).unwrap();
        assert_eq!(release_version(&meta), "1.2.3");
        meta.platform = "x86_64-linux".into();
        assert_eq!(release_version(&meta), "1.2.3-x86_64-linux");
        assert_eq!(gem_filename(&meta), "demo-1.2.3-x86_64-linux.gem");
    }

    #[test]
    fn rejects_empty_gem() {
        assert!(parse_gem(&[]).is_err());
    }

    #[test]
    fn rejects_missing_version() {
        let yaml = b"--- !ruby/object:Gem::Specification\nname: demo\n";
        assert!(parse_gemspec_yaml(yaml).is_err());
    }
}
