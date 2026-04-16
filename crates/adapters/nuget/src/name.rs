//! NuGet package ID validation and normalization.
//!
//! NuGet package IDs are case-insensitive and limited to a subset of
//! characters.  The canonical lowercase form is used for URL paths and
//! deduplication.

use publaryn_core::error::{Error, Result};

/// Maximum length of a NuGet package ID.
const MAX_NUGET_ID_LENGTH: usize = 128;

/// Validate a NuGet package ID.
///
/// Rules:
/// - Must not be empty.
/// - Maximum 128 characters.
/// - Allowed characters: `a-z`, `A-Z`, `0-9`, `.`, `-`, `_`.
/// - Must not start or end with `.`.
/// - Must not contain consecutive `.` characters.
pub fn validate_nuget_package_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(Error::Validation(
            "NuGet package ID must not be empty".into(),
        ));
    }

    if id.len() > MAX_NUGET_ID_LENGTH {
        return Err(Error::Validation(format!(
            "NuGet package ID exceeds maximum length of {MAX_NUGET_ID_LENGTH} characters"
        )));
    }

    if id.starts_with('.') {
        return Err(Error::Validation(
            "NuGet package ID must not start with '.'".into(),
        ));
    }

    if id.ends_with('.') {
        return Err(Error::Validation(
            "NuGet package ID must not end with '.'".into(),
        ));
    }

    if id.contains("..") {
        return Err(Error::Validation(
            "NuGet package ID must not contain consecutive '.' characters".into(),
        ));
    }

    for ch in id.chars() {
        if !matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_') {
            return Err(Error::Validation(format!(
                "NuGet package ID contains invalid character: '{ch}'"
            )));
        }
    }

    Ok(())
}

/// Normalize a NuGet package ID for deduplication and URL construction.
///
/// NuGet IDs are case-insensitive; the normalized form is always lowercase.
pub fn normalize_nuget_id(id: &str) -> String {
    id.to_lowercase()
}

/// Normalize a NuGet version string for URL construction.
///
/// Rules applied:
/// - Strip leading zeros from each numeric segment.
/// - Remove a trailing fourth segment if it is `0` (e.g. `1.0.0.0` → `1.0.0`).
/// - Strip build metadata (`+...`).
/// - Lowercase the entire string.
pub fn normalize_nuget_version(version: &str) -> String {
    // Strip build metadata
    let version = version.split('+').next().unwrap_or(version);

    // Split pre-release suffix
    let (main, pre) = if let Some(idx) = version.find('-') {
        (&version[..idx], Some(&version[idx..]))
    } else {
        (version, None)
    };

    let mut parts: Vec<String> = main
        .split('.')
        .map(|p| {
            // Strip leading zeros from numeric parts
            if let Ok(n) = p.parse::<u64>() {
                n.to_string()
            } else {
                p.to_owned()
            }
        })
        .collect();

    // Remove trailing fourth segment if it is "0"
    if parts.len() == 4 && parts[3] == "0" {
        parts.pop();
    }

    let mut normalized = parts.join(".");
    if let Some(pre) = pre {
        normalized.push_str(pre);
    }

    normalized.to_lowercase()
}

/// Build the canonical `.nupkg` filename.
pub fn nupkg_filename(id: &str, version: &str) -> String {
    let lower_id = id.to_lowercase();
    let lower_version = normalize_nuget_version(version);
    format!("{lower_id}.{lower_version}.nupkg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_simple_id() {
        assert!(validate_nuget_package_id("Newtonsoft.Json").is_ok());
    }

    #[test]
    fn valid_with_hyphens() {
        assert!(validate_nuget_package_id("My-Package-Name").is_ok());
    }

    #[test]
    fn valid_with_underscores() {
        assert!(validate_nuget_package_id("My_Package").is_ok());
    }

    #[test]
    fn reject_empty() {
        assert!(validate_nuget_package_id("").is_err());
    }

    #[test]
    fn reject_starts_with_dot() {
        assert!(validate_nuget_package_id(".Hidden").is_err());
    }

    #[test]
    fn reject_ends_with_dot() {
        assert!(validate_nuget_package_id("Trailing.").is_err());
    }

    #[test]
    fn reject_consecutive_dots() {
        assert!(validate_nuget_package_id("Double..Dot").is_err());
    }

    #[test]
    fn reject_invalid_char() {
        assert!(validate_nuget_package_id("no spaces").is_err());
    }

    #[test]
    fn reject_at_sign() {
        assert!(validate_nuget_package_id("@scope/pkg").is_err());
    }

    #[test]
    fn reject_too_long() {
        let name = "a".repeat(129);
        assert!(validate_nuget_package_id(&name).is_err());
    }

    #[test]
    fn normalize_case() {
        assert_eq!(normalize_nuget_id("Newtonsoft.Json"), "newtonsoft.json");
    }

    #[test]
    fn normalize_version_strips_leading_zeros() {
        assert_eq!(normalize_nuget_version("01.02.03"), "1.2.3");
    }

    #[test]
    fn normalize_version_strips_fourth_zero() {
        assert_eq!(normalize_nuget_version("1.0.0.0"), "1.0.0");
    }

    #[test]
    fn normalize_version_keeps_fourth_nonzero() {
        assert_eq!(normalize_nuget_version("1.0.0.1"), "1.0.0.1");
    }

    #[test]
    fn normalize_version_preserves_prerelease() {
        assert_eq!(normalize_nuget_version("1.0.0-beta.1"), "1.0.0-beta.1");
    }

    #[test]
    fn normalize_version_strips_build_metadata() {
        assert_eq!(normalize_nuget_version("1.0.0+build.123"), "1.0.0");
    }

    #[test]
    fn nupkg_filename_correct() {
        assert_eq!(
            nupkg_filename("Newtonsoft.Json", "13.0.1"),
            "newtonsoft.json.13.0.1.nupkg"
        );
    }
}
