use chrono::{DateTime, Utc};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct GemMetadataInput {
    pub name: String,
    pub version: String,
    pub version_downloads: i64,
    pub total_downloads: i64,
    pub platform: String,
    pub authors: Vec<String>,
    pub info: Option<String>,
    pub licenses: Vec<String>,
    pub project_uri: Option<String>,
    pub homepage_uri: Option<String>,
    pub source_code_uri: Option<String>,
    pub bug_tracker_uri: Option<String>,
    pub documentation_uri: Option<String>,
    pub sha: Option<String>,
    pub gem_uri: Option<String>,
    pub version_created_at: DateTime<Utc>,
    pub prerelease: bool,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct GemVersionListItem {
    pub number: String,
    pub prerelease: bool,
    pub created_at: DateTime<Utc>,
    pub platform: String,
    pub sha: Option<String>,
    pub gem_uri: Option<String>,
}

pub fn build_gem_metadata(input: &GemMetadataInput) -> Value {
    let mut object = json!({
        "name": input.name,
        "version": input.version,
        "platform": input.platform,
        "authors": input.authors.join(", "),
        "info": input.info,
        "licenses": input.licenses,
        "version_downloads": input.version_downloads,
        "downloads": input.total_downloads,
        "prerelease": input.prerelease,
        "sha": input.sha,
        "gem_uri": input.gem_uri,
        "project_uri": input.project_uri,
        "homepage_uri": input.homepage_uri,
        "source_code_uri": input.source_code_uri,
        "bug_tracker_uri": input.bug_tracker_uri,
        "documentation_uri": input.documentation_uri,
        "version_created_at": input.version_created_at.to_rfc3339(),
    });

    if let Some(metadata) = &input.metadata {
        object["metadata"] = metadata.clone();
    }

    object
}

pub fn build_versions_list(items: &[GemVersionListItem]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| {
                json!({
                    "number": item.number,
                    "prerelease": item.prerelease,
                    "created_at": item.created_at.to_rfc3339(),
                    "platform": item.platform,
                    "sha": item.sha,
                    "gem_uri": item.gem_uri,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_package_metadata() {
        let doc = build_gem_metadata(&GemMetadataInput {
            name: "rails".into(),
            version: "7.1.0".into(),
            version_downloads: 10,
            total_downloads: 100,
            platform: "ruby".into(),
            authors: vec!["DHH".into()],
            info: Some("A web framework".into()),
            licenses: vec!["MIT".into()],
            project_uri: Some("https://rubyonrails.org".into()),
            homepage_uri: None,
            source_code_uri: None,
            bug_tracker_uri: None,
            documentation_uri: None,
            sha: Some("abc123".into()),
            gem_uri: Some("https://example.com/gems/rails-7.1.0.gem".into()),
            version_created_at: Utc::now(),
            prerelease: false,
            metadata: None,
        });
        assert_eq!(doc["name"], "rails");
        assert_eq!(doc["version"], "7.1.0");
    }

    #[test]
    fn builds_versions_list() {
        let doc = build_versions_list(&[GemVersionListItem {
            number: "1.0.0".into(),
            prerelease: false,
            created_at: Utc::now(),
            platform: "ruby".into(),
            sha: Some("abc".into()),
            gem_uri: Some("https://example.com/gems/demo-1.0.0.gem".into()),
        }]);
        assert_eq!(doc[0]["number"], "1.0.0");
    }
}
