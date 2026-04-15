use publaryn_core::domain::{namespace::Ecosystem, package::normalize_package_name};

/// Canonicalize a PyPI project name according to the PEP 503 normalization
/// rules used by pip and the Simple Repository API.
pub fn canonicalize_project_name(name: &str) -> String {
    normalize_package_name(name, &Ecosystem::Pypi)
}

/// Returns `true` when the provided project name is already in canonical
/// PEP 503 form.
pub fn is_canonical_project_name(name: &str) -> bool {
    canonicalize_project_name(name) == name
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_project_name, is_canonical_project_name};

    #[test]
    fn canonicalization_collapses_separator_runs() {
        assert_eq!(canonicalize_project_name("My..Package__Name---Core"), "my-package-name-core");
    }

    #[test]
    fn canonicalization_preserves_alphanumeric_segments() {
        assert_eq!(canonicalize_project_name("Requests2"), "requests2");
    }

    #[test]
    fn canonical_detection_requires_pep503_form() {
        assert!(is_canonical_project_name("simple-package"));
        assert!(!is_canonical_project_name("Simple_Package"));
    }
}
