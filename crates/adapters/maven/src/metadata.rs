use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct MavenMetadataInput {
    pub group_id: String,
    pub artifact_id: String,
    pub latest: Option<String>,
    pub release: Option<String>,
    pub versions: Vec<String>,
    pub last_updated: DateTime<Utc>,
}

pub fn build_maven_metadata_xml(input: &MavenMetadataInput) -> String {
    let versions = input
        .versions
        .iter()
        .map(|version| format!("      <version>{}</version>", xml_escape(version)))
        .collect::<Vec<_>>()
        .join("\n");

    let latest = input
        .latest
        .as_ref()
        .map(|latest| format!("    <latest>{}</latest>\n", xml_escape(latest)))
        .unwrap_or_default();
    let release = input
        .release
        .as_ref()
        .map(|release| format!("    <release>{}</release>\n", xml_escape(release)))
        .unwrap_or_default();

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<metadata>\n  <groupId>{}</groupId>\n  <artifactId>{}</artifactId>\n  <versioning>\n{}{}    <versions>\n{}\n    </versions>\n    <lastUpdated>{}</lastUpdated>\n  </versioning>\n</metadata>\n",
        xml_escape(&input.group_id),
        xml_escape(&input.artifact_id),
        latest,
        release,
        versions,
        input.last_updated.format("%Y%m%d%H%M%S")
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_metadata_xml() {
        let xml = build_maven_metadata_xml(&MavenMetadataInput {
            group_id: "com.example".into(),
            artifact_id: "demo".into(),
            latest: Some("1.2.0".into()),
            release: Some("1.2.0".into()),
            versions: vec!["1.0.0".into(), "1.2.0".into()],
            last_updated: Utc::now(),
        });

        assert!(xml.contains("<groupId>com.example</groupId>"));
        assert!(xml.contains("<artifactId>demo</artifactId>"));
        assert!(xml.contains("<version>1.0.0</version>"));
    }
}
