use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use blake2::{
    digest::{Update, VariableOutput},
    Blake2bVar,
};
use bytes::Bytes;
use md5::{Digest as Md5Digest, Md5};
use publaryn_core::domain::artifact::ArtifactKind;
use serde_json::{json, Value};
use sha2::{Sha256, Sha512};
use std::collections::{BTreeMap, BTreeSet};

const LEGACY_UPLOAD_ACTION: &str = "file_upload";
const LEGACY_UPLOAD_PROTOCOL_VERSION: &str = "1";
const UPLOAD_ONLY_FIELDS: &[&str] = &[
    ":action",
    "protocol_version",
    "md5_digest",
    "sha256_digest",
    "blake2_256_digest",
    "filetype",
    "pyversion",
    "comment",
    "attestations",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyUploadRequest {
    pub package_name: String,
    pub version: String,
    pub metadata_version: String,
    pub artifact_kind: ArtifactKind,
    pub filetype: String,
    pub pyversion: String,
    pub filename: String,
    pub content_type: String,
    pub bytes: Bytes,
    pub comment: Option<String>,
    pub metadata_fields: BTreeMap<String, Vec<String>>,
    pub digests: ValidatedArtifactDigests,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyPackageMetadata {
    pub description: Option<String>,
    pub readme: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub license: Option<String>,
    pub keywords: Vec<String>,
    pub requires_python: Option<String>,
    pub requires_dist: Vec<String>,
    pub requires_external: Vec<String>,
    pub provides_extra: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedArtifactDigests {
    pub sha256_hex: String,
    pub sha512_hex: String,
    pub md5_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UploadedFile {
    filename: String,
    content_type: String,
    bytes: Bytes,
}

#[derive(Debug, Default)]
pub struct LegacyUploadBuilder {
    fields: BTreeMap<String, Vec<String>>,
    file: Option<UploadedFile>,
}

impl LegacyUploadBuilder {
    pub fn add_text_field(&mut self, name: &str, value: String) {
        self.fields
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(value);
    }

    pub fn add_file_field(
        &mut self,
        name: &str,
        filename: Option<&str>,
        content_type: Option<&str>,
        bytes: Bytes,
    ) -> Result<(), String> {
        let normalized_name = name.to_ascii_lowercase();
        if normalized_name != "content" {
            return Err(format!(
                "The multipart field '{normalized_name}' is not supported yet"
            ));
        }

        if self.file.is_some() {
            return Err("The upload payload must include exactly one distribution file".into());
        }

        let filename = filename
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "The upload payload must include a distribution filename".to_owned())?;

        if filename.contains('/') || filename.contains('\\') {
            return Err("The distribution filename must not contain path separators".into());
        }

        self.file = Some(UploadedFile {
            filename: filename.to_owned(),
            content_type: content_type
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("application/octet-stream")
                .to_owned(),
            bytes,
        });

        Ok(())
    }

    pub fn build(self) -> Result<LegacyUploadRequest, String> {
        let action = required_single_field(&self.fields, ":action")?;
        if action != LEGACY_UPLOAD_ACTION {
            return Err("The legacy upload action must be 'file_upload'".into());
        }

        let protocol_version = required_single_field(&self.fields, "protocol_version")?;
        if protocol_version != LEGACY_UPLOAD_PROTOCOL_VERSION {
            return Err("Only legacy upload protocol version '1' is supported".into());
        }

        if self.fields.contains_key("attestations") {
            return Err("PyPI upload attestations are not supported yet".into());
        }

        let metadata_version = required_single_field(&self.fields, "metadata_version")?;
        validate_metadata_version(&metadata_version)?;
        let package_name = required_single_field(&self.fields, "name")?;
        let version = required_single_field(&self.fields, "version")?;
        let filetype = required_single_field(&self.fields, "filetype")?;
        let pyversion = required_single_field(&self.fields, "pyversion")?;
        let artifact_kind = parse_artifact_kind(&filetype, &pyversion)?;

        let file = self.file.ok_or_else(|| {
            "The upload payload must include a distribution file in the 'content' field".to_owned()
        })?;

        validate_filename_for_artifact_kind(&file.filename, artifact_kind.clone())?;

        let digests = compute_artifact_digests(&file.bytes);
        validate_supplied_digests(&self.fields, &digests, &file.bytes)?;

        Ok(LegacyUploadRequest {
            package_name,
            version,
            metadata_version,
            artifact_kind,
            filetype,
            pyversion,
            filename: file.filename,
            content_type: file.content_type,
            bytes: file.bytes,
            comment: optional_single_field(&self.fields, "comment"),
            metadata_fields: filter_metadata_fields(self.fields),
            digests,
        })
    }
}

impl LegacyUploadRequest {
    pub fn package_metadata(&self) -> LegacyPackageMetadata {
        let project_urls = collect_metadata_values(&self.metadata_fields, &["project_urls"]);

        LegacyPackageMetadata {
            description: self.first_metadata_value("summary"),
            readme: self.first_metadata_value("description"),
            homepage: self
                .first_metadata_value("home_page")
                .or_else(|| project_url_for_labels(&project_urls, &["homepage", "home"])),
            repository_url: project_url_for_labels(
                &project_urls,
                &["source", "repository", "code", "source code"],
            ),
            license: self
                .first_metadata_value("license_expression")
                .or_else(|| self.first_metadata_value("license")),
            keywords: parse_keywords(self.metadata_values("keywords")),
            requires_python: first_non_empty_value(collect_metadata_values(
                &self.metadata_fields,
                &["requires_python", "requires-python"],
            )),
            requires_dist: parse_multi_value_fields(collect_metadata_values(
                &self.metadata_fields,
                &["requires_dist", "requires-dist"],
            )),
            requires_external: parse_multi_value_fields(collect_metadata_values(
                &self.metadata_fields,
                &["requires_external", "requires-external"],
            )),
            provides_extra: parse_multi_value_fields(collect_metadata_values(
                &self.metadata_fields,
                &["provides_extra", "provides-extra"],
            )),
        }
    }

    pub fn release_description(&self) -> Option<String> {
        self.first_metadata_value("summary")
            .or_else(|| self.first_metadata_value("description"))
    }

    pub fn is_prerelease(&self) -> bool {
        self.version
            .chars()
            .any(|character| character.is_ascii_alphabetic())
            || self.version.contains('-')
    }

    pub fn provenance_json(&self) -> Value {
        json!({
            "source": "pypi_legacy_upload",
            "metadata_version": self.metadata_version.as_str(),
            "filetype": self.filetype.as_str(),
            "pyversion": self.pyversion.as_str(),
            "comment": self.comment.as_deref(),
            "filename": self.filename.as_str(),
            "content_type": self.content_type.as_str(),
            "digests": {
                "sha256": self.digests.sha256_hex.as_str(),
                "sha512": self.digests.sha512_hex.as_str(),
                "md5": self.digests.md5_hex.as_str(),
            },
            "core_metadata": &self.metadata_fields,
        })
    }

    pub fn first_metadata_value(&self, key: &str) -> Option<String> {
        self.metadata_fields
            .get(key)
            .and_then(|values| values.first())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    pub fn metadata_values(&self, key: &str) -> &[String] {
        self.metadata_fields
            .get(key)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

fn required_single_field(
    fields: &BTreeMap<String, Vec<String>>,
    key: &str,
) -> Result<String, String> {
    fields
        .get(key)
        .and_then(|values| values.first())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("The multipart field '{key}' is required"))
}

fn optional_single_field(fields: &BTreeMap<String, Vec<String>>, key: &str) -> Option<String> {
    fields
        .get(key)
        .and_then(|values| values.first())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn filter_metadata_fields(fields: BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
    fields
        .into_iter()
        .filter(|(key, _)| !UPLOAD_ONLY_FIELDS.contains(&key.as_str()))
        .collect()
}

fn validate_metadata_version(metadata_version: &str) -> Result<(), String> {
    let major = metadata_version
        .split('.')
        .next()
        .ok_or_else(|| "The metadata_version field is malformed".to_owned())?
        .parse::<u32>()
        .map_err(|_| "The metadata_version field is malformed".to_owned())?;

    if !(1..=2).contains(&major) {
        return Err(format!(
            "Unsupported PyPI metadata version '{metadata_version}'"
        ));
    }

    Ok(())
}

fn parse_artifact_kind(filetype: &str, pyversion: &str) -> Result<ArtifactKind, String> {
    match filetype {
        "bdist_wheel" => {
            if pyversion.eq_ignore_ascii_case("source") {
                return Err("Wheel uploads must provide a Python tag instead of 'source'".into());
            }

            Ok(ArtifactKind::Wheel)
        }
        "sdist" => {
            if !pyversion.eq_ignore_ascii_case("source") {
                return Err("Source distribution uploads must use pyversion='source'".into());
            }

            Ok(ArtifactKind::Sdist)
        }
        other => Err(format!("Unsupported PyPI distribution filetype '{other}'")),
    }
}

fn validate_filename_for_artifact_kind(
    filename: &str,
    artifact_kind: ArtifactKind,
) -> Result<(), String> {
    if filename.chars().any(|character| character.is_control()) {
        return Err("The distribution filename must not contain control characters".into());
    }

    if matches!(artifact_kind, ArtifactKind::Wheel) && !filename.ends_with(".whl") {
        return Err("Wheel uploads must use a '.whl' filename".into());
    }

    Ok(())
}

fn compute_artifact_digests(bytes: &Bytes) -> ValidatedArtifactDigests {
    let sha256_hex = hex::encode(Sha256::digest(bytes));
    let sha512_hex = hex::encode(Sha512::digest(bytes));

    let mut md5 = Md5::new();
    Md5Digest::update(&mut md5, bytes);
    let md5_hex = hex::encode(Md5Digest::finalize(md5));

    ValidatedArtifactDigests {
        sha256_hex,
        sha512_hex,
        md5_hex,
    }
}

fn validate_supplied_digests(
    fields: &BTreeMap<String, Vec<String>>,
    digests: &ValidatedArtifactDigests,
    bytes: &Bytes,
) -> Result<(), String> {
    let mut digest_count = 0_u8;

    if let Some(sha256_digest) = optional_single_field(fields, "sha256_digest") {
        digest_count += 1;
        validate_hex_digest("sha256_digest", &sha256_digest)?;
        if sha256_digest.to_ascii_lowercase() != digests.sha256_hex {
            return Err("The supplied sha256_digest does not match the uploaded file".into());
        }
    }

    if let Some(md5_digest) = optional_single_field(fields, "md5_digest") {
        digest_count += 1;

        let mut md5 = Md5::new();
        Md5Digest::update(&mut md5, bytes);
        let encoded = URL_SAFE_NO_PAD.encode(Md5Digest::finalize(md5));
        if md5_digest != encoded {
            return Err("The supplied md5_digest does not match the uploaded file".into());
        }
    }

    if let Some(blake2_digest) = optional_single_field(fields, "blake2_256_digest") {
        digest_count += 1;
        validate_hex_digest("blake2_256_digest", &blake2_digest)?;

        let mut hasher = Blake2bVar::new(32)
            .map_err(|_| "The server failed to initialize Blake2 digest validation".to_owned())?;
        hasher.update(bytes);
        let mut output = [0_u8; 32];
        hasher
            .finalize_variable(&mut output)
            .map_err(|_| "The server failed to finalize Blake2 digest validation".to_owned())?;

        if blake2_digest.to_ascii_lowercase() != hex::encode(output) {
            return Err("The supplied blake2_256_digest does not match the uploaded file".into());
        }
    }

    if digest_count == 0 {
        return Err(
            "The upload payload must include one of md5_digest, sha256_digest, or blake2_256_digest"
                .into(),
        );
    }

    Ok(())
}

fn validate_hex_digest(field_name: &str, value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!(
            "The multipart field '{field_name}' must be a 64-character hexadecimal digest"
        ));
    }

    Ok(())
}

fn parse_keywords(values: &[String]) -> Vec<String> {
    let mut deduplicated = BTreeSet::new();

    for value in values {
        for keyword in value.split(',') {
            let keyword = keyword.trim();
            if !keyword.is_empty() {
                deduplicated.insert(keyword.to_owned());
            }
        }
    }

    deduplicated.into_iter().collect()
}

fn collect_metadata_values(fields: &BTreeMap<String, Vec<String>>, keys: &[&str]) -> Vec<String> {
    let mut values = Vec::new();

    for key in keys {
        if let Some(field_values) = fields.get(*key) {
            values.extend(field_values.iter().cloned());
        }
    }

    values
}

fn first_non_empty_value(values: Vec<String>) -> Option<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

fn parse_multi_value_fields(values: Vec<String>) -> Vec<String> {
    let mut deduplicated = BTreeSet::new();

    for value in values {
        let normalized = value.trim();
        if !normalized.is_empty() {
            deduplicated.insert(normalized.to_owned());
        }
    }

    deduplicated.into_iter().collect()
}

fn project_url_for_labels(values: &[String], labels: &[&str]) -> Option<String> {
    let labels = labels
        .iter()
        .map(|label| label.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut fallback = None;

    for value in values {
        let Some((label, url)) = value.split_once(',') else {
            continue;
        };

        let normalized_label = label.trim().to_ascii_lowercase();
        let url = url.trim();
        if url.is_empty() {
            continue;
        }

        if labels.iter().any(|allowed| allowed == &normalized_label) {
            return Some(url.to_owned());
        }

        if fallback.is_none() {
            fallback = Some(url.to_owned());
        }
    }

    fallback
}

#[cfg(test)]
mod tests {
    use super::{LegacyUploadBuilder, LegacyUploadRequest};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use blake2::{
        digest::{Update, VariableOutput},
        Blake2bVar,
    };
    use bytes::Bytes;
    use md5::{Digest as Md5Digest, Md5};
    use sha2::Sha256;

    fn build_minimal_request() -> LegacyUploadRequest {
        let content = Bytes::from_static(b"demo wheel bytes");
        let mut builder = LegacyUploadBuilder::default();
        builder.add_text_field(":action", "file_upload".into());
        builder.add_text_field("protocol_version", "1".into());
        builder.add_text_field("metadata_version", "2.4".into());
        builder.add_text_field("name", "Demo-Package".into());
        builder.add_text_field("version", "1.2.3".into());
        builder.add_text_field("filetype", "bdist_wheel".into());
        builder.add_text_field("pyversion", "py3".into());
        builder.add_text_field("summary", "A demo package".into());
        builder.add_text_field("description", "Long description".into());
        builder.add_text_field("keywords", "python, packages, demo".into());
        builder.add_text_field("requires_python", ">=3.10".into());
        builder.add_text_field("requires_dist", "requests>=2.31".into());
        builder.add_text_field("requires_dist", "urllib3>=2".into());
        builder.add_text_field("requires_external", "libssl".into());
        builder.add_text_field("provides_extra", "s3".into());
        builder.add_text_field("project_urls", "Source, https://example.test/src".into());
        builder.add_text_field("project_urls", "Homepage, https://example.test".into());
        builder.add_text_field("license_expression", "MIT".into());
        builder.add_text_field("sha256_digest", hex::encode(Sha256::digest(&content)));
        builder
            .add_file_field(
                "content",
                Some("demo_package-1.2.3-py3-none-any.whl"),
                Some("application/octet-stream"),
                content,
            )
            .expect("file should be accepted");

        builder.build().expect("request should be valid")
    }

    #[test]
    fn build_accepts_sha256_validated_upload() {
        let request = build_minimal_request();
        let metadata = request.package_metadata();

        assert_eq!(request.package_name, "Demo-Package");
        assert_eq!(request.version, "1.2.3");
        assert_eq!(metadata.description.as_deref(), Some("A demo package"));
        assert_eq!(metadata.readme.as_deref(), Some("Long description"));
        assert_eq!(metadata.homepage.as_deref(), Some("https://example.test"));
        assert_eq!(
            metadata.repository_url.as_deref(),
            Some("https://example.test/src")
        );
        assert_eq!(metadata.license.as_deref(), Some("MIT"));
        assert_eq!(
            metadata.keywords,
            vec![
                "demo".to_owned(),
                "packages".to_owned(),
                "python".to_owned()
            ]
        );
        assert_eq!(metadata.requires_python.as_deref(), Some(">=3.10"));
        assert_eq!(
            metadata.requires_dist,
            vec!["requests>=2.31".to_owned(), "urllib3>=2".to_owned()]
        );
        assert_eq!(metadata.requires_external, vec!["libssl".to_owned()]);
        assert_eq!(metadata.provides_extra, vec!["s3".to_owned()]);
    }

    #[test]
    fn build_rejects_missing_digest() {
        let mut builder = LegacyUploadBuilder::default();
        builder.add_text_field(":action", "file_upload".into());
        builder.add_text_field("protocol_version", "1".into());
        builder.add_text_field("metadata_version", "2.4".into());
        builder.add_text_field("name", "demo".into());
        builder.add_text_field("version", "1.0.0".into());
        builder.add_text_field("filetype", "sdist".into());
        builder.add_text_field("pyversion", "source".into());
        builder
            .add_file_field(
                "content",
                Some("demo-1.0.0.tar.gz"),
                Some("application/gzip"),
                Bytes::from_static(b"sdist"),
            )
            .expect("file should be accepted");

        let error = builder.build().expect_err("digest should be required");
        assert!(
            error.contains("must include one of md5_digest, sha256_digest, or blake2_256_digest")
        );
    }

    #[test]
    fn build_accepts_md5_and_blake2_digests() {
        let content = Bytes::from_static(b"signed artifact");

        let mut md5 = Md5::new();
        Md5Digest::update(&mut md5, &content);
        let md5_digest = URL_SAFE_NO_PAD.encode(Md5Digest::finalize(md5));

        let mut blake2 = Blake2bVar::new(32).expect("blake2 init should work");
        blake2.update(&content);
        let mut output = [0_u8; 32];
        blake2
            .finalize_variable(&mut output)
            .expect("blake2 finalize should work");

        let mut builder = LegacyUploadBuilder::default();
        builder.add_text_field(":action", "file_upload".into());
        builder.add_text_field("protocol_version", "1".into());
        builder.add_text_field("metadata_version", "2.4".into());
        builder.add_text_field("name", "demo".into());
        builder.add_text_field("version", "1.0.0".into());
        builder.add_text_field("filetype", "sdist".into());
        builder.add_text_field("pyversion", "source".into());
        builder.add_text_field("md5_digest", md5_digest);
        builder.add_text_field("blake2_256_digest", hex::encode(output));
        builder
            .add_file_field(
                "content",
                Some("demo-1.0.0.tar.gz"),
                Some("application/gzip"),
                content,
            )
            .expect("file should be accepted");

        let request = builder.build().expect("request should validate");
        assert_eq!(request.filename, "demo-1.0.0.tar.gz");
    }

    #[test]
    fn build_rejects_attestations_until_supported() {
        let mut builder = LegacyUploadBuilder::default();
        builder.add_text_field(":action", "file_upload".into());
        builder.add_text_field("protocol_version", "1".into());
        builder.add_text_field("metadata_version", "2.4".into());
        builder.add_text_field("name", "demo".into());
        builder.add_text_field("version", "1.0.0".into());
        builder.add_text_field("filetype", "sdist".into());
        builder.add_text_field("pyversion", "source".into());
        builder.add_text_field("sha256_digest", hex::encode(Sha256::digest(b"demo")));
        builder.add_text_field("attestations", "[]".into());
        builder
            .add_file_field(
                "content",
                Some("demo-1.0.0.tar.gz"),
                Some("application/gzip"),
                Bytes::from_static(b"demo"),
            )
            .expect("file should be accepted");

        let error = builder.build().expect_err("attestations should fail");
        assert!(error.contains("attestations are not supported yet"));
    }

    #[test]
    fn build_rejects_wheel_without_python_tag() {
        let mut builder = LegacyUploadBuilder::default();
        builder.add_text_field(":action", "file_upload".into());
        builder.add_text_field("protocol_version", "1".into());
        builder.add_text_field("metadata_version", "2.4".into());
        builder.add_text_field("name", "demo".into());
        builder.add_text_field("version", "1.0.0".into());
        builder.add_text_field("filetype", "bdist_wheel".into());
        builder.add_text_field("pyversion", "source".into());
        builder.add_text_field("sha256_digest", hex::encode(Sha256::digest(b"wheel")));
        builder
            .add_file_field(
                "content",
                Some("demo-1.0.0-py3-none-any.whl"),
                Some("application/octet-stream"),
                Bytes::from_static(b"wheel"),
            )
            .expect("file should be accepted");

        let error = builder.build().expect_err("wheel pyversion should fail");
        assert!(error.contains("Wheel uploads must provide a Python tag"));
    }
}
