use publaryn_core::error::{Error, Result};

/// Maximum length of a Cargo crate name.
const MAX_CARGO_NAME_LENGTH: usize = 64;

/// Windows reserved device names that must be rejected as crate names.
const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul",
    "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Validate a Cargo crate name.
///
/// Rules (matching crates.io):
/// - ASCII only
/// - First character must be alphabetic
/// - Remaining characters: alphanumeric, `-`, `_`
/// - 1–64 characters
/// - Rejects Windows reserved device names
pub fn validate_crate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Validation("Crate name must not be empty".into()));
    }

    if name.len() > MAX_CARGO_NAME_LENGTH {
        return Err(Error::Validation(format!(
            "Crate name exceeds maximum length of {MAX_CARGO_NAME_LENGTH} characters"
        )));
    }

    let mut chars = name.chars();

    let first = chars.next().unwrap(); // safe: name is non-empty
    if !first.is_ascii_alphabetic() {
        return Err(Error::Validation(
            "Crate name must start with an ASCII letter".into(),
        ));
    }

    for ch in chars {
        if !matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_') {
            return Err(Error::Validation(format!(
                "Crate name contains invalid character: '{ch}'"
            )));
        }
    }

    // Reject Windows reserved names (case-insensitive)
    let lower = name.to_ascii_lowercase();
    if WINDOWS_RESERVED.contains(&lower.as_str()) {
        return Err(Error::Validation(format!(
            "Crate name '{name}' is a reserved name"
        )));
    }

    Ok(())
}

/// Normalize a Cargo crate name for deduplication.
///
/// Cargo treats hyphens and underscores as equivalent and the comparison
/// is case-insensitive: `My-Crate` and `my_crate` are the same crate.
pub fn normalize_crate_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

/// Compute the sparse index path for a crate name.
///
/// The path follows the tiered directory structure used by crates.io:
/// - 1-char names: `1/{name}`
/// - 2-char names: `2/{name}`
/// - 3-char names: `3/{first_char}/{name}`
/// - 4+ char names: `{first_two}/{next_two}/{name}`
///
/// The name is lowercased for the path.
pub fn index_path(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.len() {
        0 => String::new(),
        1 => format!("1/{lower}"),
        2 => format!("2/{lower}"),
        3 => {
            let first = &lower[..1];
            format!("3/{first}/{lower}")
        }
        _ => {
            let ab = &lower[..2];
            let cd = &lower[2..4];
            format!("{ab}/{cd}/{lower}")
        }
    }
}

/// Strip SemVer build metadata (everything after `+`) for version uniqueness.
///
/// Cargo treats `1.0.0+build1` and `1.0.0+build2` as the same version.
pub fn strip_build_metadata(version: &str) -> &str {
    version.split_once('+').map_or(version, |(base, _)| base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_simple_name() {
        assert!(validate_crate_name("serde").is_ok());
    }

    #[test]
    fn valid_name_with_hyphen() {
        assert!(validate_crate_name("my-crate").is_ok());
    }

    #[test]
    fn valid_name_with_underscore() {
        assert!(validate_crate_name("my_crate").is_ok());
    }

    #[test]
    fn valid_name_with_digits() {
        assert!(validate_crate_name("tokio1").is_ok());
    }

    #[test]
    fn reject_empty() {
        assert!(validate_crate_name("").is_err());
    }

    #[test]
    fn reject_starts_with_digit() {
        assert!(validate_crate_name("1crate").is_err());
    }

    #[test]
    fn reject_starts_with_hyphen() {
        assert!(validate_crate_name("-crate").is_err());
    }

    #[test]
    fn reject_too_long() {
        let name = format!("a{}", "b".repeat(64));
        assert!(validate_crate_name(&name).is_err());
    }

    #[test]
    fn reject_invalid_char() {
        assert!(validate_crate_name("my.crate").is_err());
    }

    #[test]
    fn reject_windows_reserved_con() {
        assert!(validate_crate_name("CON").is_err());
    }

    #[test]
    fn reject_windows_reserved_nul() {
        assert!(validate_crate_name("nul").is_err());
    }

    #[test]
    fn reject_windows_reserved_com1() {
        assert!(validate_crate_name("COM1").is_err());
    }

    #[test]
    fn normalize_hyphen_to_underscore() {
        assert_eq!(normalize_crate_name("My-Crate"), "my_crate");
    }

    #[test]
    fn normalize_already_canonical() {
        assert_eq!(normalize_crate_name("serde"), "serde");
    }

    #[test]
    fn normalize_mixed_case() {
        assert_eq!(normalize_crate_name("Tokio"), "tokio");
    }

    #[test]
    fn index_path_one_char() {
        assert_eq!(index_path("a"), "1/a");
    }

    #[test]
    fn index_path_two_chars() {
        assert_eq!(index_path("ab"), "2/ab");
    }

    #[test]
    fn index_path_three_chars() {
        assert_eq!(index_path("foo"), "3/f/foo");
    }

    #[test]
    fn index_path_four_chars() {
        assert_eq!(index_path("rand"), "ra/nd/rand");
    }

    #[test]
    fn index_path_long_name() {
        assert_eq!(index_path("cargo"), "ca/rg/cargo");
    }

    #[test]
    fn index_path_uppercased() {
        assert_eq!(index_path("Serde"), "se/rd/serde");
    }

    #[test]
    fn strip_build_metadata_with_plus() {
        assert_eq!(strip_build_metadata("1.0.0+build1"), "1.0.0");
    }

    #[test]
    fn strip_build_metadata_without_plus() {
        assert_eq!(strip_build_metadata("1.0.0-rc.1"), "1.0.0-rc.1");
    }

    #[test]
    fn strip_build_metadata_plain() {
        assert_eq!(strip_build_metadata("2.3.4"), "2.3.4");
    }
}
