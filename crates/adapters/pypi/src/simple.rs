use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const PYPI_SIMPLE_API_VERSION: &str = "1.1";
pub const PYPI_SIMPLE_JSON_CONTENT_TYPE: &str = "application/vnd.pypi.simple.v1+json";
pub const PYPI_SIMPLE_HTML_CONTENT_TYPE: &str = "application/vnd.pypi.simple.v1+html";
pub const PYPI_SIMPLE_TEXT_HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseFormat {
    Html,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedFormat {
    pub format: ResponseFormat,
    pub content_type: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLink {
    pub name: String,
    pub normalized_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFile {
    pub filename: String,
    pub url: String,
    pub hashes: BTreeMap<String, String>,
    pub size_bytes: i64,
    pub upload_time: Option<DateTime<Utc>>,
    pub is_yanked: bool,
    pub yanked_reason: Option<String>,
}

pub fn select_response_format(accept_header: Option<&str>) -> Result<SelectedFormat, ()> {
    let Some(accept_header) = accept_header
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(SelectedFormat {
            format: ResponseFormat::Html,
            content_type: PYPI_SIMPLE_TEXT_HTML_CONTENT_TYPE,
        });
    };

    let mut best_match: Option<(i32, i32, SelectedFormat)> = None;

    for raw_entry in accept_header.split(',') {
        let raw_entry = raw_entry.trim();
        if raw_entry.is_empty() {
            continue;
        }

        let mut segments = raw_entry.split(';').map(str::trim);
        let media_type = segments.next().unwrap_or_default();
        let mut quality = 1000_i32;

        for parameter in segments {
            let Some((name, value)) = parameter.split_once('=') else {
                continue;
            };
            if !name.trim().eq_ignore_ascii_case("q") {
                continue;
            }

            quality = parse_quality(value.trim()).unwrap_or(0);
        }

        if quality <= 0 {
            continue;
        }

        let candidate = match media_type {
            PYPI_SIMPLE_JSON_CONTENT_TYPE | "application/vnd.pypi.simple.latest+json" => Some((
                3,
                SelectedFormat {
                    format: ResponseFormat::Json,
                    content_type: PYPI_SIMPLE_JSON_CONTENT_TYPE,
                },
            )),
            PYPI_SIMPLE_HTML_CONTENT_TYPE | "application/vnd.pypi.simple.latest+html" => Some((
                2,
                SelectedFormat {
                    format: ResponseFormat::Html,
                    content_type: PYPI_SIMPLE_HTML_CONTENT_TYPE,
                },
            )),
            "text/html" | "text/*" | "*/*" => Some((
                1,
                SelectedFormat {
                    format: ResponseFormat::Html,
                    content_type: PYPI_SIMPLE_TEXT_HTML_CONTENT_TYPE,
                },
            )),
            "application/*" => Some((
                2,
                SelectedFormat {
                    format: ResponseFormat::Json,
                    content_type: PYPI_SIMPLE_JSON_CONTENT_TYPE,
                },
            )),
            _ => None,
        };

        let Some((priority, selected)) = candidate else {
            continue;
        };

        let should_replace = match &best_match {
            Some((best_quality, best_priority, _)) => {
                quality > *best_quality || (quality == *best_quality && priority > *best_priority)
            }
            None => true,
        };

        if should_replace {
            best_match = Some((quality, priority, selected));
        }
    }

    best_match.map(|(_, _, selected)| selected).ok_or(())
}

pub fn render_index_html(projects: &[ProjectLink]) -> String {
    let mut html = String::from(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta name=\"pypi:repository-version\" content=\"1.1\"><title>Publaryn Simple Index</title></head><body>",
    );

    for project in projects {
        html.push_str("<a href=\"./");
        html.push_str(&escape_html_attribute(&project.normalized_name));
        html.push_str("/\">");
        html.push_str(&escape_html_text(&project.name));
        html.push_str("</a>\n");
    }

    html.push_str("</body></html>");
    html
}

pub fn render_project_html(project_name: &str, files: &[ProjectFile]) -> String {
    let mut html = String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta name=\"pypi:repository-version\" content=\"1.1\"><title>");
    html.push_str(&escape_html_text(project_name));
    html.push_str("</title></head><body>");

    for file in files {
        let file_url = file_url_with_hash_fragment(file);
        html.push_str("<a href=\"");
        html.push_str(&escape_html_attribute(&file_url));
        html.push('"');

        if file.is_yanked {
            match file
                .yanked_reason
                .as_deref()
                .filter(|reason| !reason.is_empty())
            {
                Some(reason) => {
                    html.push_str(" data-yanked=\"");
                    html.push_str(&escape_html_attribute(reason));
                    html.push('"');
                }
                None => html.push_str(" data-yanked"),
            }
        }

        html.push('>');
        html.push_str(&escape_html_text(&file.filename));
        html.push_str("</a>\n");
    }

    html.push_str("</body></html>");
    html
}

pub fn build_index_json(projects: &[ProjectLink]) -> Value {
    json!({
        "meta": {
            "api-version": PYPI_SIMPLE_API_VERSION,
        },
        "projects": projects
            .iter()
            .map(|project| json!({ "name": project.name }))
            .collect::<Vec<_>>(),
    })
}

pub fn build_project_json(project_name: &str, versions: &[String], files: &[ProjectFile]) -> Value {
    json!({
        "meta": {
            "api-version": PYPI_SIMPLE_API_VERSION,
        },
        "name": project_name,
        "files": files.iter().map(project_file_to_json).collect::<Vec<_>>(),
        "versions": versions,
    })
}

fn project_file_to_json(file: &ProjectFile) -> Value {
    let mut object = Map::new();
    object.insert("filename".into(), Value::String(file.filename.clone()));
    object.insert("url".into(), Value::String(file.url.clone()));
    object.insert("hashes".into(), json!(file.hashes));
    object.insert("size".into(), Value::Number(file.size_bytes.into()));

    if let Some(upload_time) = file.upload_time {
        object.insert(
            "upload-time".into(),
            Value::String(format_timestamp(upload_time)),
        );
    }

    if file.is_yanked {
        match file
            .yanked_reason
            .as_deref()
            .filter(|reason| !reason.is_empty())
        {
            Some(reason) => {
                object.insert("yanked".into(), Value::String(reason.to_owned()));
            }
            None => {
                object.insert("yanked".into(), Value::Bool(true));
            }
        }
    }

    Value::Object(object)
}

fn file_url_with_hash_fragment(file: &ProjectFile) -> String {
    match file.hashes.get("sha256") {
        Some(sha256) if !sha256.is_empty() => format!("{}#sha256={sha256}", file.url),
        _ => file.url.clone(),
    }
}

fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn parse_quality(input: &str) -> Option<i32> {
    let value = input.parse::<f32>().ok()?;
    if !(0.0..=1.0).contains(&value) {
        return None;
    }

    Some((value * 1000.0).round() as i32)
}

fn escape_html_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_attribute(input: &str) -> String {
    escape_html_text(input).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use std::collections::BTreeMap;

    use super::{
        build_project_json, render_project_html, select_response_format, ProjectFile, ProjectLink,
        ResponseFormat, PYPI_SIMPLE_JSON_CONTENT_TYPE, PYPI_SIMPLE_TEXT_HTML_CONTENT_TYPE,
    };

    #[test]
    fn negotiation_prefers_json_when_available() {
        let selected = select_response_format(Some(
            "application/vnd.pypi.simple.v1+html;q=0.5, application/vnd.pypi.simple.v1+json",
        ))
        .expect("json should be selected");

        assert_eq!(selected.format, ResponseFormat::Json);
        assert_eq!(selected.content_type, PYPI_SIMPLE_JSON_CONTENT_TYPE);
    }

    #[test]
    fn negotiation_falls_back_to_text_html_for_wildcards() {
        let selected =
            select_response_format(Some("text/html, */*;q=0.8")).expect("html should be selected");

        assert_eq!(selected.format, ResponseFormat::Html);
        assert_eq!(selected.content_type, PYPI_SIMPLE_TEXT_HTML_CONTENT_TYPE);
    }

    #[test]
    fn negotiation_rejects_unsupported_accept_headers() {
        let result = select_response_format(Some("application/json"));
        assert!(result.is_err(), "unsupported Accept headers should fail");
    }

    #[test]
    fn project_json_includes_versions_and_yanked_reason() {
        let mut hashes = BTreeMap::new();
        hashes.insert("sha256".into(), "abc123".into());
        let files = vec![ProjectFile {
            filename: "demo-1.0.tar.gz".into(),
            url: "https://example.test/files/demo-1.0.tar.gz".into(),
            hashes,
            size_bytes: 42,
            upload_time: Some(chrono::Utc.with_ymd_and_hms(2026, 4, 15, 9, 30, 0).unwrap()),
            is_yanked: true,
            yanked_reason: Some("Broken build".into()),
        }];

        let document = build_project_json("demo-package", &["1.0".into()], &files);

        assert_eq!(document["name"], "demo-package");
        assert_eq!(document["versions"][0], "1.0");
        assert_eq!(document["files"][0]["size"], 42);
        assert_eq!(document["files"][0]["yanked"], "Broken build");
    }

    #[test]
    fn project_html_includes_hash_fragment_and_yanked_attribute() {
        let mut hashes = BTreeMap::new();
        hashes.insert("sha256".into(), "deadbeef".into());
        let html = render_project_html(
            "demo-package",
            &[ProjectFile {
                filename: "demo-1.0.tar.gz".into(),
                url: "https://example.test/files/demo-1.0.tar.gz".into(),
                hashes,
                size_bytes: 7,
                upload_time: None,
                is_yanked: true,
                yanked_reason: Some("Superseded".into()),
            }],
        );

        assert!(html.contains("#sha256=deadbeef"));
        assert!(html.contains("data-yanked=\"Superseded\""));
    }

    #[test]
    fn index_json_uses_project_names() {
        let document = super::build_index_json(&[ProjectLink {
            name: "Requests".into(),
            normalized_name: "requests".into(),
        }]);

        assert_eq!(document["projects"][0]["name"], "Requests");
    }
}
