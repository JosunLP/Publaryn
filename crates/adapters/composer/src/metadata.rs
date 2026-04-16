use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};

/// Input for the Composer root `packages.json` document.
#[derive(Debug, Clone)]
pub struct PackagesIndexInput {
    pub package_names: Vec<String>,
}

/// Build the Composer root index document.
pub fn build_packages_index(input: &PackagesIndexInput, base_url: &str) -> Value {
    let base = base_url.trim_end_matches('/');
    json!({
        "packages": {},
        "metadata-url": format!("{base}/composer/p/%package%.json"),
        "available-packages": input.package_names,
    })
}

/// Input for one Composer package metadata response.
#[derive(Debug, Clone)]
pub struct ComposerPackageInput {
    pub name: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub licenses: Vec<String>,
    pub keywords: Vec<String>,
    pub versions: Vec<ComposerVersionInput>,
}

/// Input for one version entry inside Composer package metadata.
#[derive(Debug, Clone)]
pub struct ComposerVersionInput {
    pub version: String,
    pub version_normalized: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub licenses: Vec<String>,
    pub keywords: Vec<String>,
    pub dist_url: Option<String>,
    pub dist_reference: Option<String>,
    pub published_at: DateTime<Utc>,
    pub extra_metadata: Option<Value>,
}

/// Build the Composer package metadata document returned by `/p/{vendor}/{name}.json`.
pub fn build_package_metadata(input: &ComposerPackageInput) -> Value {
    let versions = input
        .versions
        .iter()
        .map(|version| build_version_document(&input.name, version))
        .collect::<Vec<_>>();

    json!({
        "packages": {
            input.name.clone(): versions,
        }
    })
}

fn build_version_document(package_name: &str, input: &ComposerVersionInput) -> Value {
    let mut doc = match &input.extra_metadata {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };

    doc.insert("name".into(), Value::String(package_name.to_owned()));
    doc.insert("version".into(), Value::String(input.version.clone()));
    doc.insert(
        "version_normalized".into(),
        Value::String(input.version_normalized.clone()),
    );
    doc.entry("type")
        .or_insert_with(|| Value::String("library".into()));
    doc.insert("time".into(), Value::String(input.published_at.to_rfc3339()));

    if let Some(description) = &input.description {
        doc.entry("description")
            .or_insert_with(|| Value::String(description.clone()));
    }

    if let Some(homepage) = &input.homepage {
        doc.entry("homepage")
            .or_insert_with(|| Value::String(homepage.clone()));
    }

    if !input.licenses.is_empty() {
        doc.entry("license")
            .or_insert_with(|| json!(input.licenses));
    }

    if !input.keywords.is_empty() {
        doc.entry("keywords")
            .or_insert_with(|| json!(input.keywords));
    }

    if let Some(repository_url) = &input.repository_url {
        doc.entry("support").or_insert_with(|| {
            json!({
                "source": repository_url,
            })
        });
    }

    if let Some(dist_url) = &input.dist_url {
        let mut dist = serde_json::Map::new();
        dist.insert("type".into(), Value::String("zip".into()));
        dist.insert("url".into(), Value::String(dist_url.clone()));
        if let Some(reference) = &input.dist_reference {
            dist.insert("reference".into(), Value::String(reference.clone()));
            dist.insert("shasum".into(), Value::String(reference.clone()));
        }
        doc.insert("dist".into(), Value::Object(dist));
    }

    Value::Object(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_packages_index() {
        let doc = build_packages_index(
            &PackagesIndexInput {
                package_names: vec!["acme/demo".into()],
            },
            "http://localhost:3000",
        );
        assert_eq!(doc["metadata-url"], "http://localhost:3000/composer/p/%package%.json");
        assert_eq!(doc["available-packages"][0], "acme/demo");
    }

    #[test]
    fn builds_package_document() {
        let doc = build_package_metadata(&ComposerPackageInput {
            name: "acme/demo".into(),
            description: Some("Demo package".into()),
            homepage: Some("https://example.com".into()),
            repository_url: Some("https://github.com/acme/demo".into()),
            licenses: vec!["MIT".into()],
            keywords: vec!["demo".into()],
            versions: vec![ComposerVersionInput {
                version: "1.0.0".into(),
                version_normalized: "1.0.0.0".into(),
                description: Some("Demo package".into()),
                homepage: Some("https://example.com".into()),
                repository_url: Some("https://github.com/acme/demo".into()),
                licenses: vec!["MIT".into()],
                keywords: vec!["demo".into()],
                dist_url: Some("https://example.com/demo.zip".into()),
                dist_reference: Some("abc123".into()),
                published_at: Utc::now(),
                extra_metadata: None,
            }],
        });

        assert_eq!(doc["packages"]["acme/demo"][0]["name"], "acme/demo");
        assert_eq!(doc["packages"]["acme/demo"][0]["dist"]["type"], "zip");
    }
}
