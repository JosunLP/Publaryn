use publaryn_core::error::{Error, Result};

pub const OCI_DIGEST_PREFIX: &str = "sha256:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OciReference {
    Tag(String),
    Digest(String),
}

pub fn normalize_repository_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

pub fn validate_repository_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::Validation(
            "OCI repository names must not be empty".into(),
        ));
    }

    if trimmed.len() > 255 {
        return Err(Error::Validation(
            "OCI repository names must not exceed 255 characters".into(),
        ));
    }

    if trimmed.starts_with('/') || trimmed.ends_with('/') || trimmed.contains("//") {
        return Err(Error::Validation(
            "OCI repository names must not start, end, or contain empty path segments".into(),
        ));
    }

    for segment in trimmed.split('/') {
        validate_repository_segment(segment)?;
    }

    Ok(())
}

pub fn parse_reference(reference: &str) -> Result<OciReference> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return Err(Error::Validation(
            "OCI references must not be empty".into(),
        ));
    }

    if trimmed.starts_with(OCI_DIGEST_PREFIX) {
        return Ok(OciReference::Digest(validate_digest(trimmed)?));
    }

    validate_tag(trimmed)?;
    Ok(OciReference::Tag(trimmed.to_owned()))
}

pub fn validate_digest(digest: &str) -> Result<String> {
    let canonical = digest.trim().to_ascii_lowercase();
    let Some(hex) = canonical.strip_prefix(OCI_DIGEST_PREFIX) else {
        return Err(Error::Validation(format!(
            "Unsupported OCI digest '{digest}'; only sha256 digests are supported"
        )));
    };

    if hex.len() != 64 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(Error::Validation(format!(
            "OCI digest '{digest}' is not a valid sha256 digest"
        )));
    }

    Ok(format!("{OCI_DIGEST_PREFIX}{hex}"))
}

pub fn digest_hex(digest: &str) -> Result<&str> {
    digest
        .strip_prefix(OCI_DIGEST_PREFIX)
        .ok_or_else(|| Error::Validation(format!("Unsupported OCI digest '{digest}'")))
}

fn validate_repository_segment(segment: &str) -> Result<()> {
    if segment.is_empty() {
        return Err(Error::Validation(
            "OCI repository path segments must not be empty".into(),
        ));
    }

    if segment.starts_with(['.', '-', '_']) || segment.ends_with(['.', '-', '_']) {
        return Err(Error::Validation(format!(
            "OCI repository segment '{segment}' must start and end with an ASCII letter or digit"
        )));
    }

    let mut previous_was_separator = false;
    for character in segment.chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            previous_was_separator = false;
            continue;
        }

        if matches!(character, '.' | '_' | '-') {
            if previous_was_separator {
                return Err(Error::Validation(format!(
                    "OCI repository segment '{segment}' must not contain consecutive separators"
                )));
            }
            previous_was_separator = true;
            continue;
        }

        return Err(Error::Validation(format!(
            "OCI repository segment '{segment}' contains unsupported character '{character}'"
        )));
    }

    Ok(())
}

fn validate_tag(tag: &str) -> Result<()> {
    if tag.len() > 128 {
        return Err(Error::Validation(
            "OCI tags must not exceed 128 characters".into(),
        ));
    }

    let mut characters = tag.chars();
    let Some(first) = characters.next() else {
        return Err(Error::Validation("OCI tags must not be empty".into()));
    };

    if !(first.is_ascii_alphanumeric() || first == '_') {
        return Err(Error::Validation(format!(
            "OCI tag '{tag}' must start with an ASCII letter, digit, or underscore"
        )));
    }

    if !characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
    }) {
        return Err(Error::Validation(format!(
            "OCI tag '{tag}' contains unsupported characters"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_repository_names() {
        validate_repository_name("acme/widget").expect("repository name should validate");
        validate_repository_name("acme/tools.widget_v2").expect("repository name should validate");
    }

    #[test]
    fn rejects_invalid_repository_names() {
        let error = validate_repository_name("Acme/widget").expect_err("uppercase must fail");
        assert!(error.to_string().contains("unsupported character"));
    }

    #[test]
    fn parses_digest_references() {
        let reference = parse_reference(
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("digest reference should parse");

        assert_eq!(
            reference,
            OciReference::Digest(
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .into()
            )
        );
    }

    #[test]
    fn parses_tag_references() {
        let reference = parse_reference("latest").expect("tag should parse");
        assert_eq!(reference, OciReference::Tag("latest".into()));
    }
}
