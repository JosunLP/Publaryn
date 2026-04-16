use publaryn_core::{
    domain::namespace::Ecosystem,
    error::{Error, Result},
    validation,
};

/// Validate a Composer package name like `vendor/package`.
pub fn validate_composer_package_name(name: &str) -> Result<()> {
    validation::validate_package_name(name, &Ecosystem::Composer)
}

/// Split a Composer package name into `(vendor, package)`.
pub fn split_composer_name(name: &str) -> Result<(&str, &str)> {
    let (vendor, package) = name.split_once('/').ok_or_else(|| {
        Error::Validation("Composer package names must be in the form vendor/package".into())
    })?;

    if vendor.is_empty() || package.is_empty() {
        return Err(Error::Validation(
            "Composer package names must include both vendor and package segments".into(),
        ));
    }

    Ok((vendor, package))
}

/// Build a Composer package name from path segments.
pub fn build_composer_package_name(vendor: &str, package_segment: &str) -> Result<String> {
    let package = package_segment.strip_suffix(".json").ok_or_else(|| {
        Error::Validation("Composer metadata URLs must end in .json".into())
    })?;

    let name = format!("{vendor}/{package}");
    validate_composer_package_name(&name)?;
    Ok(name)
}

/// Best-effort normalization of Composer versions.
///
/// Composer often uses a four-part normalized version; for our MVP we pad
/// missing numeric parts and preserve pre-release/build suffixes.
pub fn normalize_composer_version(version: &str) -> String {
    let trimmed = version.trim().trim_start_matches('v');
    if trimmed.is_empty() {
        return version.to_owned();
    }

    let (core, suffix) = match trimmed.find(['-', '+']) {
        Some(idx) => (&trimmed[..idx], &trimmed[idx..]),
        None => (trimmed, ""),
    };

    let numeric_parts = core.split('.').collect::<Vec<_>>();
    if numeric_parts.iter().all(|part| part.chars().all(|c| c.is_ascii_digit())) {
        let mut padded = numeric_parts.iter().map(|part| (*part).to_owned()).collect::<Vec<_>>();
        while padded.len() < 4 {
            padded.push("0".into());
        }
        return format!("{}{}", padded.join("."), suffix);
    }

    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_name_from_path_segments() {
        let name = build_composer_package_name("acme", "demo.json").unwrap();
        assert_eq!(name, "acme/demo");
    }

    #[test]
    fn rejects_missing_json_suffix() {
        assert!(build_composer_package_name("acme", "demo").is_err());
    }

    #[test]
    fn normalizes_simple_versions() {
        assert_eq!(normalize_composer_version("1.2.3"), "1.2.3.0");
        assert_eq!(normalize_composer_version("1.2"), "1.2.0.0");
    }

    #[test]
    fn keeps_suffixes() {
        assert_eq!(normalize_composer_version("v1.2.3-beta1"), "1.2.3.0-beta1");
    }
}
