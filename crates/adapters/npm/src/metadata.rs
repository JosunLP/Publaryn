use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};

use crate::name::tarball_filename;

/// Information about a single release needed to build the packument.
#[derive(Debug, Clone)]
pub struct VersionRecord {
    pub version: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub keywords: Vec<String>,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub is_yanked: bool,
    pub tarball_sha256: Option<String>,
    pub tarball_sha512: Option<String>,
    pub tarball_size: Option<i64>,
    pub published_at: DateTime<Utc>,
    /// Full version-specific metadata stored as JSONB (from release provenance
    /// or stored package.json subset). May be `None` if we don't store it yet.
    pub extra_metadata: Option<Value>,
}

/// Package-level information for the packument.
#[derive(Debug, Clone)]
pub struct PackumentInput {
    pub name: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub keywords: Vec<String>,
    pub readme: Option<String>,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub versions: Vec<VersionRecord>,
    pub dist_tags: Vec<(String, String)>,
}

/// Build a full npm packument document.
///
/// The packument is the JSON document returned by `GET /:package` that
/// npm/yarn/pnpm use to resolve versions, discover tarballs, and display
/// metadata.
pub fn build_packument(input: &PackumentInput, tarball_base_url: &str) -> Value {
    let mut versions_map = Map::new();
    let mut time_map = Map::new();

    time_map.insert(
        "created".to_owned(),
        Value::String(input.created_at.to_rfc3339()),
    );
    time_map.insert(
        "modified".to_owned(),
        Value::String(input.updated_at.to_rfc3339()),
    );

    for v in &input.versions {
        time_map.insert(
            v.version.clone(),
            Value::String(v.published_at.to_rfc3339()),
        );

        let filename = tarball_filename(&input.name, &v.version);
        let tarball_url = format!("{tarball_base_url}/{filename}");

        let mut dist = Map::new();
        dist.insert("tarball".to_owned(), Value::String(tarball_url));
        if let Some(sha256) = &v.tarball_sha256 {
            dist.insert("shasum".to_owned(), Value::String(sha256.clone()));
        }
        if let Some(sha512) = &v.tarball_sha512 {
            let integrity = format!("sha512-{sha512}");
            dist.insert("integrity".to_owned(), Value::String(integrity));
        }
        if let Some(size) = v.tarball_size {
            dist.insert(
                "unpackedSize".to_owned(),
                Value::Number(serde_json::Number::from(size)),
            );
        }

        let mut version_doc = if let Some(extra) = &v.extra_metadata {
            match extra {
                Value::Object(m) => m.clone(),
                _ => Map::new(),
            }
        } else {
            Map::new()
        };

        version_doc.insert("name".to_owned(), Value::String(input.name.clone()));
        version_doc.insert("version".to_owned(), Value::String(v.version.clone()));

        if let Some(desc) = &v.description {
            version_doc
                .entry("description")
                .or_insert_with(|| Value::String(desc.clone()));
        }

        if let Some(lic) = &v.license {
            version_doc
                .entry("license")
                .or_insert_with(|| Value::String(lic.clone()));
        }

        if let Some(hp) = &v.homepage {
            version_doc
                .entry("homepage")
                .or_insert_with(|| Value::String(hp.clone()));
        }

        if v.is_deprecated {
            let msg = v
                .deprecation_message
                .clone()
                .unwrap_or_else(|| "This version has been deprecated".to_owned());
            version_doc.insert("deprecated".to_owned(), Value::String(msg));
        }

        version_doc.insert("dist".to_owned(), Value::Object(dist));

        versions_map.insert(v.version.clone(), Value::Object(version_doc));
    }

    // Build dist-tags
    let mut dist_tags = Map::new();
    for (tag, version) in &input.dist_tags {
        dist_tags.insert(tag.clone(), Value::String(version.clone()));
    }

    let mut packument = json!({
        "name": input.name,
        "versions": versions_map,
        "dist-tags": dist_tags,
        "time": time_map,
    });

    let obj = packument.as_object_mut().unwrap();

    if let Some(desc) = &input.description {
        obj.insert("description".to_owned(), Value::String(desc.clone()));
    }
    if let Some(lic) = &input.license {
        obj.insert("license".to_owned(), Value::String(lic.clone()));
    }
    if let Some(hp) = &input.homepage {
        obj.insert("homepage".to_owned(), Value::String(hp.clone()));
    }
    if let Some(repo) = &input.repository_url {
        obj.insert(
            "repository".to_owned(),
            json!({ "type": "git", "url": repo }),
        );
    }
    if !input.keywords.is_empty() {
        obj.insert("keywords".to_owned(), json!(input.keywords));
    }
    if let Some(readme) = &input.readme {
        obj.insert("readme".to_owned(), Value::String(readme.clone()));
    }

    packument
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_packument_basic() {
        let input = PackumentInput {
            name: "my-pkg".to_owned(),
            description: Some("A test package".to_owned()),
            license: Some("MIT".to_owned()),
            homepage: None,
            repository_url: None,
            keywords: vec![],
            readme: None,
            is_deprecated: false,
            deprecation_message: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            versions: vec![VersionRecord {
                version: "1.0.0".to_owned(),
                description: Some("A test package".to_owned()),
                license: Some("MIT".to_owned()),
                homepage: None,
                repository_url: None,
                keywords: vec![],
                is_deprecated: false,
                deprecation_message: None,
                is_yanked: false,
                tarball_sha256: Some("abc123".to_owned()),
                tarball_sha512: None,
                tarball_size: Some(1024),
                published_at: Utc::now(),
                extra_metadata: None,
            }],
            dist_tags: vec![("latest".to_owned(), "1.0.0".to_owned())],
        };

        let doc = build_packument(&input, "https://registry.example.com/npm/my-pkg/-");
        assert_eq!(doc["name"], "my-pkg");
        assert_eq!(doc["dist-tags"]["latest"], "1.0.0");
        assert!(doc["versions"]["1.0.0"]["dist"]["tarball"]
            .as_str()
            .unwrap()
            .contains("my-pkg-1.0.0.tgz"));
    }

    #[test]
    fn deprecated_version_has_deprecated_field() {
        let input = PackumentInput {
            name: "old-pkg".to_owned(),
            description: None,
            license: None,
            homepage: None,
            repository_url: None,
            keywords: vec![],
            readme: None,
            is_deprecated: false,
            deprecation_message: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            versions: vec![VersionRecord {
                version: "0.1.0".to_owned(),
                description: None,
                license: None,
                homepage: None,
                repository_url: None,
                keywords: vec![],
                is_deprecated: true,
                deprecation_message: Some("Use new-pkg instead".to_owned()),
                is_yanked: false,
                tarball_sha256: None,
                tarball_sha512: None,
                tarball_size: None,
                published_at: Utc::now(),
                extra_metadata: None,
            }],
            dist_tags: vec![],
        };

        let doc = build_packument(&input, "https://r.example.com/npm/old-pkg/-");
        assert_eq!(
            doc["versions"]["0.1.0"]["deprecated"],
            "Use new-pkg instead"
        );
    }
}
