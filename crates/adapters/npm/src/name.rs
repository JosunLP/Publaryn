use publaryn_core::error::{Error, Result};

/// Maximum length of an npm package name (including scope).
const MAX_NPM_NAME_LENGTH: usize = 214;

/// Validate an npm package name according to the npm naming rules.
///
/// Accepts both scoped (`@scope/name`) and unscoped names.
pub fn validate_npm_package_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Validation(
            "npm package name must not be empty".into(),
        ));
    }

    if name.len() > MAX_NPM_NAME_LENGTH {
        return Err(Error::Validation(format!(
            "npm package name exceeds maximum length of {MAX_NPM_NAME_LENGTH} characters"
        )));
    }

    if name.starts_with('.') || name.starts_with('_') {
        return Err(Error::Validation(
            "npm package name must not start with '.' or '_'".into(),
        ));
    }

    if name != name.to_lowercase() {
        return Err(Error::Validation(
            "npm package name must be lowercase".into(),
        ));
    }

    if let Some(scoped) = name.strip_prefix('@') {
        let Some((scope, local)) = scoped.split_once('/') else {
            return Err(Error::Validation(
                "Scoped npm package name must contain exactly one '/' after '@'".into(),
            ));
        };
        validate_npm_name_segment(scope, "scope")?;
        validate_npm_name_segment(local, "name")?;
    } else {
        validate_npm_name_segment(name, "name")?;
    }

    Ok(())
}

fn validate_npm_name_segment(segment: &str, label: &str) -> Result<()> {
    if segment.is_empty() {
        return Err(Error::Validation(format!("npm package {label} is empty")));
    }

    for ch in segment.chars() {
        if !matches!(ch, 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~') {
            return Err(Error::Validation(format!(
                "npm package {label} contains invalid character: '{ch}'"
            )));
        }
    }

    Ok(())
}

/// Normalize an npm package name for deduplication.
///
/// npm names are already required to be lowercase, so normalization is
/// the identity function.
pub fn normalize_npm_name(name: &str) -> String {
    name.to_lowercase()
}

/// Extract the scope from a scoped npm name, e.g. `@scope/pkg` → `@scope`.
/// Returns `None` for unscoped packages.
pub fn extract_scope(name: &str) -> Option<&str> {
    if name.starts_with('@') {
        name.split_once('/').map(|(scope, _)| scope)
    } else {
        None
    }
}

/// Build the conventional tarball filename for a given package name and version.
///
/// For scoped packages, the scope prefix is stripped:
/// `@scope/pkg` version `1.0.0` → `pkg-1.0.0.tgz`
pub fn tarball_filename(package_name: &str, version: &str) -> String {
    let local_name = if let Some((_scope, local)) = package_name.split_once('/') {
        local
    } else {
        package_name
    };
    format!("{local_name}-{version}.tgz")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_unscoped_name() {
        assert!(validate_npm_package_name("my-package").is_ok());
    }

    #[test]
    fn valid_scoped_name() {
        assert!(validate_npm_package_name("@myorg/my-package").is_ok());
    }

    #[test]
    fn reject_uppercase() {
        assert!(validate_npm_package_name("MyPackage").is_err());
    }

    #[test]
    fn reject_empty() {
        assert!(validate_npm_package_name("").is_err());
    }

    #[test]
    fn reject_starts_with_dot() {
        assert!(validate_npm_package_name(".hidden").is_err());
    }

    #[test]
    fn reject_starts_with_underscore() {
        assert!(validate_npm_package_name("_private").is_err());
    }

    #[test]
    fn reject_too_long() {
        let name = "a".repeat(215);
        assert!(validate_npm_package_name(&name).is_err());
    }

    #[test]
    fn reject_scoped_without_slash() {
        assert!(validate_npm_package_name("@noslash").is_err());
    }

    #[test]
    fn extract_scope_scoped() {
        assert_eq!(extract_scope("@myorg/pkg"), Some("@myorg"));
    }

    #[test]
    fn extract_scope_unscoped() {
        assert_eq!(extract_scope("pkg"), None);
    }

    #[test]
    fn tarball_filename_scoped() {
        assert_eq!(tarball_filename("@scope/pkg", "1.0.0"), "pkg-1.0.0.tgz");
    }

    #[test]
    fn tarball_filename_unscoped() {
        assert_eq!(tarball_filename("my-lib", "2.3.4"), "my-lib-2.3.4.tgz");
    }
}
