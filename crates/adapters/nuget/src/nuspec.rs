//! `.nuspec` XML parser.
//!
//! A `.nupkg` is a ZIP archive that always contains a `.nuspec` file at the
//! root. This module extracts and parses that XML file into a strongly-typed
//! `NuspecMetadata` struct.

use std::io::{Cursor, Read};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use publaryn_core::error::{Error, Result};

/// Parsed contents of a `.nuspec` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NuspecMetadata {
    pub id: String,
    pub version: String,
    pub authors: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub license_url: Option<String>,
    pub license_expression: Option<String>,
    pub project_url: Option<String>,
    pub icon_url: Option<String>,
    pub readme: Option<String>,
    pub tags: Vec<String>,
    pub require_license_acceptance: bool,
    pub min_client_version: Option<String>,
    pub dependency_groups: Vec<DependencyGroup>,
    pub package_types: Vec<PackageType>,
}

/// A set of dependencies scoped to a target framework.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGroup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_framework: Option<String>,
    pub dependencies: Vec<NuspecDependency>,
}

/// A single package dependency as declared in the nuspec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NuspecDependency {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
}

/// A package type (e.g. `Dependency`, `DotnetTool`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageType {
    pub name: String,
}

/// Extract and parse the `.nuspec` from the raw bytes of a `.nupkg` archive.
pub fn parse_nuspec_from_nupkg(nupkg_bytes: &[u8]) -> Result<(NuspecMetadata, Vec<u8>)> {
    let reader = Cursor::new(nupkg_bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| Error::Validation(format!("Invalid .nupkg archive: {e}")))?;

    // Find the .nuspec file (there should be exactly one at the root level)
    let nuspec_name = (0..archive.len())
        .find_map(|i| {
            let file = archive.by_index(i).ok()?;
            let name = file.name().to_owned();
            if name.ends_with(".nuspec") && !name.contains('/') {
                Some(name)
            } else {
                None
            }
        })
        .ok_or_else(|| Error::Validation("No .nuspec file found in .nupkg archive".into()))?;

    let mut nuspec_file = archive
        .by_name(&nuspec_name)
        .map_err(|e| Error::Validation(format!("Failed to read .nuspec from archive: {e}")))?;

    let mut nuspec_bytes = Vec::new();
    nuspec_file
        .read_to_end(&mut nuspec_bytes)
        .map_err(|e| Error::Validation(format!("Failed to read .nuspec contents: {e}")))?;

    let metadata = parse_nuspec_xml(&nuspec_bytes)?;
    Ok((metadata, nuspec_bytes))
}

/// Parse a `.nuspec` XML document into `NuspecMetadata`.
pub fn parse_nuspec_xml(xml_bytes: &[u8]) -> Result<NuspecMetadata> {
    let xml_str = std::str::from_utf8(xml_bytes)
        .map_err(|e| Error::Validation(format!("Invalid UTF-8 in .nuspec: {e}")))?;

    // We use a simple XML parsing approach using quick-xml events.
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);

    let mut id = String::new();
    let mut version = String::new();
    let mut authors: Option<String> = None;
    let mut title: Option<String> = None;
    let mut description: Option<String> = None;
    let mut summary: Option<String> = None;
    let mut license_url: Option<String> = None;
    let mut license_expression: Option<String> = None;
    let mut project_url: Option<String> = None;
    let mut icon_url: Option<String> = None;
    let mut readme: Option<String> = None;
    let mut tags_str: Option<String> = None;
    let mut require_license_acceptance = false;
    let mut min_client_version: Option<String> = None;
    let mut dependency_groups: Vec<DependencyGroup> = Vec::new();
    let mut package_types: Vec<PackageType> = Vec::new();

    // Track current element path for context
    let mut current_element = String::new();
    let mut in_metadata = false;
    let mut in_dependencies = false;
    let mut in_group = false;
    let mut current_group: Option<DependencyGroup> = None;
    let mut in_package_types = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local_name = local_name_str(e.name().as_ref());
                let tag = local_name.clone();

                match tag.as_str() {
                    "metadata" => {
                        in_metadata = true;
                        // Check for minClientVersion attribute
                        for attr in e.attributes().flatten() {
                            if local_name_str(attr.key.as_ref()) == "minClientVersion" {
                                min_client_version =
                                    attr.unescape_value().ok().map(|v| v.to_string());
                            }
                        }
                    }
                    "dependencies" if in_metadata => {
                        in_dependencies = true;
                    }
                    "group" if in_dependencies => {
                        in_group = true;
                        let tf = e
                            .attributes()
                            .flatten()
                            .find(|a| local_name_str(a.key.as_ref()) == "targetFramework")
                            .and_then(|a| a.unescape_value().ok().map(|v| v.to_string()));
                        current_group = Some(DependencyGroup {
                            target_framework: tf,
                            dependencies: Vec::new(),
                        });
                    }
                    "dependency" if in_dependencies => {
                        let mut dep_id = String::new();
                        let mut dep_version = None;
                        let mut dep_exclude = None;
                        for attr in e.attributes().flatten() {
                            match local_name_str(attr.key.as_ref()).as_str() {
                                "id" => {
                                    dep_id = attr
                                        .unescape_value()
                                        .map(|v| v.to_string())
                                        .unwrap_or_default();
                                }
                                "version" => {
                                    dep_version = attr.unescape_value().ok().map(|v| v.to_string());
                                }
                                "exclude" => {
                                    dep_exclude = attr.unescape_value().ok().map(|v| v.to_string());
                                }
                                _ => {}
                            }
                        }
                        let dep = NuspecDependency {
                            id: dep_id,
                            version_range: dep_version,
                            exclude: dep_exclude,
                        };
                        if let Some(ref mut group) = current_group {
                            group.dependencies.push(dep);
                        } else {
                            // Flat dependency (no group) — treat as no-TFM group
                            if dependency_groups.is_empty()
                                || dependency_groups.last().unwrap().target_framework.is_some()
                            {
                                dependency_groups.push(DependencyGroup {
                                    target_framework: None,
                                    dependencies: vec![dep],
                                });
                            } else {
                                dependency_groups.last_mut().unwrap().dependencies.push(dep);
                            }
                        }
                    }
                    "packageTypes" if in_metadata => {
                        in_package_types = true;
                    }
                    "packageType" if in_package_types => {
                        if let Some(attr) = e
                            .attributes()
                            .flatten()
                            .find(|a| local_name_str(a.key.as_ref()) == "name")
                        {
                            if let Ok(val) = attr.unescape_value() {
                                package_types.push(PackageType {
                                    name: val.to_string(),
                                });
                            }
                        }
                    }
                    "license" if in_metadata => {
                        // Check for type="expression" attribute
                        let is_expression = e.attributes().flatten().any(|a| {
                            local_name_str(a.key.as_ref()) == "type"
                                && a.unescape_value()
                                    .map(|v| v == "expression")
                                    .unwrap_or(false)
                        });
                        if is_expression {
                            current_element = "license_expression".into();
                        } else {
                            current_element = "license_file".into();
                        }
                        continue;
                    }
                    _ if in_metadata && !in_dependencies && !in_package_types => {
                        current_element = tag;
                        continue;
                    }
                    _ => {}
                }
                current_element.clear();
            }
            Ok(Event::Text(ref e)) if !current_element.is_empty() && in_metadata => {
                let text = e.xml_content().unwrap_or_default().to_string();
                match current_element.as_str() {
                    "id" => id = text,
                    "version" => version = text,
                    "authors" => authors = Some(text),
                    "title" => title = Some(text),
                    "description" => description = Some(text),
                    "summary" => summary = Some(text),
                    "licenseUrl" => license_url = Some(text),
                    "license_expression" => license_expression = Some(text),
                    "projectUrl" => project_url = Some(text),
                    "iconUrl" => icon_url = Some(text),
                    "readme" => readme = Some(text),
                    "tags" => tags_str = Some(text),
                    "requireLicenseAcceptance" => {
                        require_license_acceptance = text.eq_ignore_ascii_case("true");
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name_str(e.name().as_ref());
                match local.as_str() {
                    "metadata" => in_metadata = false,
                    "dependencies" => in_dependencies = false,
                    "group" if in_group => {
                        in_group = false;
                        if let Some(group) = current_group.take() {
                            dependency_groups.push(group);
                        }
                    }
                    "packageTypes" => in_package_types = false,
                    _ => {}
                }
                current_element.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::Validation(format!("Invalid .nuspec XML: {e}")));
            }
            _ => {}
        }
        buf.clear();
    }

    if id.is_empty() {
        return Err(Error::Validation(
            "Missing <id> element in .nuspec metadata".into(),
        ));
    }
    if version.is_empty() {
        return Err(Error::Validation(
            "Missing <version> element in .nuspec metadata".into(),
        ));
    }

    // Parse tags — NuGet uses space-separated tags
    let tags: Vec<String> = tags_str
        .map(|s| s.split_whitespace().map(|t| t.to_owned()).collect())
        .unwrap_or_default();

    if package_types.is_empty() {
        package_types.push(PackageType {
            name: "Dependency".into(),
        });
    }

    Ok(NuspecMetadata {
        id,
        version,
        authors,
        title,
        description,
        summary,
        license_url,
        license_expression,
        project_url,
        icon_url,
        readme,
        tags,
        require_license_acceptance,
        min_client_version,
        dependency_groups,
        package_types,
    })
}

/// Convert dependency groups to JSON for storage.
pub fn dependency_groups_to_json(groups: &[DependencyGroup]) -> JsonValue {
    serde_json::to_value(groups).unwrap_or(JsonValue::Array(Vec::new()))
}

/// Convert package types to JSON for storage.
pub fn package_types_to_json(types: &[PackageType]) -> JsonValue {
    serde_json::to_value(types).unwrap_or(JsonValue::Array(Vec::new()))
}

/// Extract the local name from an XML element, stripping namespace prefix.
fn local_name_str(name: &[u8]) -> String {
    let s = std::str::from_utf8(name).unwrap_or("");
    // Strip namespace prefix if present (e.g. "ns:element" → "element")
    if let Some(idx) = s.rfind(':') {
        s[idx + 1..].to_owned()
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_NUSPEC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd">
  <metadata>
    <id>TestPackage</id>
    <version>1.0.0</version>
    <authors>Test Author</authors>
    <description>A test package for unit testing.</description>
    <tags>test unit</tags>
    <requireLicenseAcceptance>false</requireLicenseAcceptance>
    <license type="expression">MIT</license>
    <projectUrl>https://example.com/test</projectUrl>
    <dependencies>
      <group targetFramework=".NETStandard2.0">
        <dependency id="Newtonsoft.Json" version="[13.0.1, )" />
      </group>
    </dependencies>
  </metadata>
</package>"#;

    #[test]
    fn parse_sample_nuspec() {
        let meta = parse_nuspec_xml(SAMPLE_NUSPEC.as_bytes()).unwrap();
        assert_eq!(meta.id, "TestPackage");
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.authors.as_deref(), Some("Test Author"));
        assert_eq!(
            meta.description.as_deref(),
            Some("A test package for unit testing.")
        );
        assert_eq!(meta.tags, vec!["test", "unit"]);
        assert!(!meta.require_license_acceptance);
        assert_eq!(meta.license_expression.as_deref(), Some("MIT"));
        assert_eq!(
            meta.project_url.as_deref(),
            Some("https://example.com/test")
        );
        assert_eq!(meta.dependency_groups.len(), 1);
        assert_eq!(
            meta.dependency_groups[0].target_framework.as_deref(),
            Some(".NETStandard2.0")
        );
        assert_eq!(meta.dependency_groups[0].dependencies.len(), 1);
        assert_eq!(
            meta.dependency_groups[0].dependencies[0].id,
            "Newtonsoft.Json"
        );
    }

    #[test]
    fn reject_missing_id() {
        let xml = r#"<?xml version="1.0"?>
<package><metadata><version>1.0.0</version></metadata></package>"#;
        assert!(parse_nuspec_xml(xml.as_bytes()).is_err());
    }

    #[test]
    fn reject_missing_version() {
        let xml = r#"<?xml version="1.0"?>
<package><metadata><id>Pkg</id></metadata></package>"#;
        assert!(parse_nuspec_xml(xml.as_bytes()).is_err());
    }

    #[test]
    fn default_package_type_dependency() {
        let xml = r#"<?xml version="1.0"?>
<package><metadata><id>Pkg</id><version>1.0.0</version></metadata></package>"#;
        let meta = parse_nuspec_xml(xml.as_bytes()).unwrap();
        assert_eq!(meta.package_types.len(), 1);
        assert_eq!(meta.package_types[0].name, "Dependency");
    }
}
