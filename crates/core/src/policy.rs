use once_cell::sync::Lazy;
use std::collections::HashSet;

use crate::domain::namespace::Ecosystem;
use crate::error::{Error, Result};

/// Well-known reserved package names that cannot be registered.
static RESERVED_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "admin",
        "administrator",
        "root",
        "system",
        "publaryn",
        "registry",
        "api",
        "www",
        "mail",
        "smtp",
        "ftp",
        "ssh",
        "npm",
        "pypi",
        "cargo",
        "nuget",
        "rubygems",
        "maven",
        "docker",
        "oci",
        "composer",
        "security",
        "internal",
        "private",
        "public",
        "official",
    ]
    .into_iter()
    .collect()
});

/// Check whether a package name is reserved.
pub fn is_reserved_name(name: &str) -> bool {
    RESERVED_NAMES.contains(name.to_lowercase().as_str())
}

/// Compute the Levenshtein similarity ratio between two strings.
///
/// Returns a value in [0, 1] where 1.0 = identical.
pub fn name_similarity(a: &str, b: &str) -> f64 {
    let dist = strsim::levenshtein(a, b);
    let max_len = a.len().max(b.len());
    if max_len == 0 {
        return 1.0;
    }
    1.0 - (dist as f64 / max_len as f64)
}

/// Policy error kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyViolation {
    ReservedName(String),
    SimilarNameExists { existing: String, similarity: f64 },
    NamespaceMismatch,
    PackageTooLarge { size_bytes: u64, limit_bytes: u64 },
    MalwareDetected,
    SecretsDetected,
}

impl std::fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyViolation::ReservedName(n) => write!(f, "Package name '{n}' is reserved"),
            PolicyViolation::SimilarNameExists {
                existing,
                similarity,
            } => {
                write!(
                    f,
                    "Package name is {:.0}% similar to existing package '{existing}'",
                    similarity * 100.0
                )
            }
            PolicyViolation::NamespaceMismatch => {
                write!(f, "Package name does not match the claimed namespace")
            }
            PolicyViolation::PackageTooLarge {
                size_bytes,
                limit_bytes,
            } => {
                write!(
                    f,
                    "Package size {size_bytes} bytes exceeds limit {limit_bytes} bytes"
                )
            }
            PolicyViolation::MalwareDetected => write!(f, "Malware detected in package"),
            PolicyViolation::SecretsDetected => {
                write!(f, "Secrets or credentials detected in package")
            }
        }
    }
}

impl From<PolicyViolation> for Error {
    fn from(v: PolicyViolation) -> Self {
        Error::PolicyViolation(v.to_string())
    }
}

/// Maximum allowed artifact sizes per ecosystem (in bytes).
pub fn max_artifact_size(ecosystem: &Ecosystem) -> u64 {
    match ecosystem {
        Ecosystem::Oci => 10 * 1024 * 1024 * 1024, // 10 GiB
        Ecosystem::Maven => 512 * 1024 * 1024,     // 512 MiB
        _ => 256 * 1024 * 1024,                    // 256 MiB default
    }
}

/// Validate a package name against platform-wide name policies.
pub fn check_name_policy(
    name: &str,
    existing_names: &[String],
    _ecosystem: &Ecosystem,
) -> Result<Vec<PolicyViolation>> {
    let mut violations = vec![];
    let normalized = name.to_lowercase();

    if is_reserved_name(&normalized) {
        violations.push(PolicyViolation::ReservedName(name.to_owned()));
    }

    // Similarity check against existing packages
    let threshold = 0.85_f64;
    for existing in existing_names {
        let sim = name_similarity(&normalized, &existing.to_lowercase());
        if sim >= threshold && normalized != existing.to_lowercase() {
            violations.push(PolicyViolation::SimilarNameExists {
                existing: existing.clone(),
                similarity: sim,
            });
        }
    }

    Ok(violations)
}
