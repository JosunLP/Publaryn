//! NuGet V3 JSON response builders.
//!
//! Builds the JSON structures required by the NuGet V3 protocol:
//! - Service index (`/v3/index.json`)
//! - Registration index (package metadata for all versions)
//! - Flat container version listing
//! - Search result format

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::name::normalize_nuget_version;

// ─── Service Index ───────────────────────────────────────────────────────────

/// Build the NuGet V3 service index JSON.
///
/// The service index is the entry point that NuGet clients use to discover
/// available resources. All resource URLs are based on `base_url`.
pub fn build_service_index(base_url: &str) -> Value {
    let base = base_url.trim_end_matches('/');

    json!({
        "version": "3.0.0",
        "resources": [
            {
                "@id": format!("{base}/nuget/v3-flatcontainer/"),
                "@type": "PackageBaseAddress/3.0.0",
                "comment": "Base URL for package content (.nupkg and .nuspec downloads)"
            },
            {
                "@id": format!("{base}/nuget/v2/package"),
                "@type": "PackagePublish/2.0.0",
                "comment": "Package publish endpoint"
            },
            {
                "@id": format!("{base}/nuget/v3/registration/"),
                "@type": "RegistrationsBaseUrl/3.6.0",
                "comment": "Package metadata registration base URL (includes SemVer 2.0.0)"
            },
            {
                "@id": format!("{base}/nuget/v3/search"),
                "@type": "SearchQueryService/3.5.0",
                "comment": "Package search endpoint"
            }
        ]
    })
}

// ─── Registration Index ──────────────────────────────────────────────────────

/// Input for building a registration index.
#[derive(Debug, Clone)]
pub struct RegistrationInput {
    pub package_id: String,
    pub versions: Vec<RegistrationVersionInput>,
}

/// Per-version data for the registration index.
#[derive(Debug, Clone)]
pub struct RegistrationVersionInput {
    pub version: String,
    pub description: Option<String>,
    pub authors: Option<String>,
    pub tags: Vec<String>,
    pub license_url: Option<String>,
    pub license_expression: Option<String>,
    pub project_url: Option<String>,
    pub icon_url: Option<String>,
    pub require_license_acceptance: bool,
    pub summary: Option<String>,
    pub title: Option<String>,
    pub dependency_groups: Value,
    pub is_listed: bool,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub published_at: DateTime<Utc>,
    pub package_types: Value,
}

/// Build a NuGet registration index (package metadata for all versions).
///
/// For simplicity we always inline all leaves (no pagination to separate
/// page documents). This is fine for self-hosted registries.
pub fn build_registration_index(input: &RegistrationInput, base_url: &str) -> Value {
    let base = base_url.trim_end_matches('/');
    let lower_id = input.package_id.to_lowercase();
    let reg_base = format!("{base}/nuget/v3/registration/{lower_id}");
    let flat_base = format!("{base}/nuget/v3-flatcontainer/{lower_id}");

    let leaves: Vec<Value> = input
        .versions
        .iter()
        .map(|v| {
            let lower_version = normalize_nuget_version(&v.version);
            let leaf_url = format!("{reg_base}/{lower_version}.json");
            let nupkg_url = format!("{flat_base}/{lower_version}/{lower_id}.{lower_version}.nupkg");

            let published = if v.is_listed {
                v.published_at.to_rfc3339()
            } else {
                // NuGet convention: unlisted packages have published = year 1900
                "1900-01-01T00:00:00+00:00".to_string()
            };

            let mut catalog_entry = json!({
                "@id": leaf_url,
                "id": input.package_id,
                "version": v.version,
                "published": published,
                "listed": v.is_listed,
                "packageContent": nupkg_url,
            });

            let entry = catalog_entry.as_object_mut().unwrap();

            if let Some(desc) = &v.description {
                entry.insert("description".into(), json!(desc));
            }
            if let Some(authors) = &v.authors {
                entry.insert("authors".into(), json!(authors));
            }
            if let Some(summary) = &v.summary {
                entry.insert("summary".into(), json!(summary));
            }
            if let Some(title) = &v.title {
                entry.insert("title".into(), json!(title));
            }
            if let Some(license_url) = &v.license_url {
                entry.insert("licenseUrl".into(), json!(license_url));
            }
            if let Some(license_expression) = &v.license_expression {
                entry.insert("licenseExpression".into(), json!(license_expression));
            }
            if let Some(project_url) = &v.project_url {
                entry.insert("projectUrl".into(), json!(project_url));
            }
            if let Some(icon_url) = &v.icon_url {
                entry.insert("iconUrl".into(), json!(icon_url));
            }
            if !v.tags.is_empty() {
                entry.insert("tags".into(), json!(v.tags));
            }
            entry.insert(
                "requireLicenseAcceptance".into(),
                json!(v.require_license_acceptance),
            );

            // Dependency groups
            if let Value::Array(ref groups) = v.dependency_groups {
                if !groups.is_empty() {
                    let formatted = format_dependency_groups(groups);
                    entry.insert("dependencyGroups".into(), json!(formatted));
                }
            }

            // Deprecation
            if v.is_deprecated {
                let mut deprecation = json!({
                    "reasons": ["Other"]
                });
                if let Some(msg) = &v.deprecation_message {
                    deprecation
                        .as_object_mut()
                        .unwrap()
                        .insert("message".into(), json!(msg));
                }
                entry.insert("deprecation".into(), deprecation);
            }

            json!({
                "@id": leaf_url,
                "catalogEntry": catalog_entry,
                "packageContent": nupkg_url,
                "registration": format!("{reg_base}/index.json")
            })
        })
        .collect();

    // Determine lower/upper bounds
    let (lower, upper) = if input.versions.is_empty() {
        ("0.0.0".to_string(), "0.0.0".to_string())
    } else {
        (
            input.versions.first().unwrap().version.clone(),
            input.versions.last().unwrap().version.clone(),
        )
    };

    json!({
        "count": 1,
        "items": [
            {
                "@id": format!("{reg_base}/index.json#page/{lower}/{upper}"),
                "count": leaves.len(),
                "items": leaves,
                "lower": lower,
                "upper": upper,
            }
        ]
    })
}

/// Format dependency groups from stored JSON to NuGet registration format.
fn format_dependency_groups(groups: &[Value]) -> Vec<Value> {
    groups
        .iter()
        .map(|g| {
            let mut obj = json!({});
            let m = obj.as_object_mut().unwrap();

            if let Some(tf) = g.get("target_framework").and_then(|v| v.as_str()) {
                m.insert("targetFramework".into(), json!(tf));
            }

            if let Some(deps) = g.get("dependencies").and_then(|v| v.as_array()) {
                let formatted_deps: Vec<Value> = deps
                    .iter()
                    .map(|d| {
                        let mut dep = json!({});
                        let dm = dep.as_object_mut().unwrap();
                        if let Some(id) = d.get("id").and_then(|v| v.as_str()) {
                            dm.insert("id".into(), json!(id));
                        }
                        if let Some(range) = d.get("version_range").and_then(|v| v.as_str()) {
                            dm.insert("range".into(), json!(range));
                        }
                        dep
                    })
                    .collect();
                m.insert("dependencies".into(), json!(formatted_deps));
            }

            obj
        })
        .collect()
}

// ─── Flat Container ──────────────────────────────────────────────────────────

/// Build the flat container version listing.
pub fn build_version_listing(versions: &[String]) -> Value {
    let normalized: Vec<String> = versions
        .iter()
        .map(|v| normalize_nuget_version(v))
        .collect();

    json!({ "versions": normalized })
}

// ─── Search ──────────────────────────────────────────────────────────────────

/// Input for building search results.
#[derive(Debug, Clone)]
pub struct SearchResultInput {
    pub package_id: String,
    pub latest_version: String,
    pub description: Option<String>,
    pub authors: Option<String>,
    pub tags: Vec<String>,
    pub total_downloads: i64,
    pub verified: bool,
    pub versions: Vec<SearchVersionInput>,
    pub package_types: Value,
}

/// Per-version in search results.
#[derive(Debug, Clone)]
pub struct SearchVersionInput {
    pub version: String,
    pub downloads: i64,
}

/// Build a NuGet search query response.
pub fn build_search_response(hits: &[SearchResultInput], total_hits: i64, base_url: &str) -> Value {
    let base = base_url.trim_end_matches('/');

    let data: Vec<Value> = hits
        .iter()
        .map(|hit| {
            let lower_id = hit.package_id.to_lowercase();
            let reg_url = format!("{base}/nuget/v3/registration/{lower_id}/index.json");

            let versions: Vec<Value> = hit
                .versions
                .iter()
                .map(|v| {
                    let lower_version = normalize_nuget_version(&v.version);
                    json!({
                        "version": v.version,
                        "downloads": v.downloads,
                        "@id": format!("{base}/nuget/v3/registration/{lower_id}/{lower_version}.json")
                    })
                })
                .collect();

            let mut result = json!({
                "id": hit.package_id,
                "version": hit.latest_version,
                "registration": reg_url,
                "totalDownloads": hit.total_downloads,
                "verified": hit.verified,
                "versions": versions,
                "packageTypes": hit.package_types,
            });

            let obj = result.as_object_mut().unwrap();
            if let Some(desc) = &hit.description {
                obj.insert("description".into(), json!(desc));
            }
            if let Some(authors) = &hit.authors {
                obj.insert("authors".into(), json!(authors));
            }
            if !hit.tags.is_empty() {
                obj.insert("tags".into(), json!(hit.tags));
            }

            result
        })
        .collect();

    json!({
        "totalHits": total_hits,
        "data": data
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn service_index_has_required_resources() {
        let idx = build_service_index("http://localhost:3000");
        let resources = idx["resources"].as_array().unwrap();
        assert!(resources.len() >= 4);

        let types: Vec<&str> = resources
            .iter()
            .filter_map(|r| r["@type"].as_str())
            .collect();
        assert!(types.contains(&"PackageBaseAddress/3.0.0"));
        assert!(types.contains(&"PackagePublish/2.0.0"));
        assert!(types.contains(&"RegistrationsBaseUrl/3.6.0"));
        assert!(types.contains(&"SearchQueryService/3.5.0"));
    }

    #[test]
    fn registration_index_structure() {
        let input = RegistrationInput {
            package_id: "TestPkg".into(),
            versions: vec![RegistrationVersionInput {
                version: "1.0.0".into(),
                description: Some("A test".into()),
                authors: Some("Test Author".into()),
                tags: vec!["test".into()],
                license_url: None,
                license_expression: Some("MIT".into()),
                project_url: None,
                icon_url: None,
                require_license_acceptance: false,
                summary: None,
                title: None,
                dependency_groups: json!([]),
                is_listed: true,
                is_deprecated: false,
                deprecation_message: None,
                published_at: Utc::now(),
                package_types: json!([{"name": "Dependency"}]),
            }],
        };

        let idx = build_registration_index(&input, "http://localhost:3000");
        assert_eq!(idx["count"], 1);
        let page = &idx["items"][0];
        assert_eq!(page["count"], 1);
        let leaf = &page["items"][0];
        assert!(leaf["catalogEntry"]["id"].as_str().unwrap() == "TestPkg");
        assert!(leaf["packageContent"].as_str().unwrap().contains(".nupkg"));
    }

    #[test]
    fn version_listing_normalizes() {
        let versions = vec!["1.0.0".to_string(), "2.0.0-beta.1".to_string()];
        let listing = build_version_listing(&versions);
        let arr = listing["versions"].as_array().unwrap();
        assert_eq!(arr[0], "1.0.0");
        assert_eq!(arr[1], "2.0.0-beta.1");
    }

    #[test]
    fn search_response_structure() {
        let hits = vec![SearchResultInput {
            package_id: "TestPkg".into(),
            latest_version: "1.0.0".into(),
            description: Some("A test".into()),
            authors: Some("Test".into()),
            tags: vec!["test".into()],
            total_downloads: 42,
            verified: false,
            versions: vec![SearchVersionInput {
                version: "1.0.0".into(),
                downloads: 42,
            }],
            package_types: json!([{"name": "Dependency"}]),
        }];

        let response = build_search_response(&hits, 1, "http://localhost:3000");
        assert_eq!(response["totalHits"], 1);
        assert_eq!(response["data"][0]["id"], "TestPkg");
        assert_eq!(response["data"][0]["version"], "1.0.0");
    }

    #[test]
    fn unlisted_version_uses_1900_date() {
        let input = RegistrationInput {
            package_id: "Unlisted".into(),
            versions: vec![RegistrationVersionInput {
                version: "1.0.0".into(),
                description: None,
                authors: None,
                tags: vec![],
                license_url: None,
                license_expression: None,
                project_url: None,
                icon_url: None,
                require_license_acceptance: false,
                summary: None,
                title: None,
                dependency_groups: json!([]),
                is_listed: false,
                is_deprecated: false,
                deprecation_message: None,
                published_at: Utc::now(),
                package_types: json!([{"name": "Dependency"}]),
            }],
        };

        let idx = build_registration_index(&input, "http://localhost:3000");
        let published = idx["items"][0]["items"][0]["catalogEntry"]["published"]
            .as_str()
            .unwrap();
        assert!(published.starts_with("1900"));
    }
}
