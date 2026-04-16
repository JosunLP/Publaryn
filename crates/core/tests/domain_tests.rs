use chrono::{Duration, Utc};
use publaryn_core::{
    domain::{
        namespace::Ecosystem,
        organization::OrgRole,
        organization_invitation::{OrganizationInvitation, OrganizationInvitationStatus},
        package::normalize_package_name,
        user::User,
    },
    error::Error,
    policy::{
        check_name_policy, is_reserved_name, max_artifact_size, name_similarity, PolicyViolation,
    },
    validation::{
        validate_email, validate_package_name, validate_slug, validate_username, validate_version,
    },
};
use std::str::FromStr;
use uuid::Uuid;

// ══════════════════════════════════════════════════════════════════════════════
// normalize_package_name
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_normalize_npm_name() {
    assert_eq!(
        normalize_package_name("MyPackage", &Ecosystem::Npm),
        "mypackage"
    );
    assert_eq!(
        normalize_package_name("@acme/foo", &Ecosystem::Npm),
        "@acme/foo"
    );
}

#[test]
fn test_normalize_bun_uses_npm_rules() {
    assert_eq!(normalize_package_name("MyPkg", &Ecosystem::Bun), "mypkg");
}

#[test]
fn test_normalize_pypi_name() {
    assert_eq!(
        normalize_package_name("My-Package", &Ecosystem::Pypi),
        "my-package"
    );
    assert_eq!(
        normalize_package_name("my.package", &Ecosystem::Pypi),
        "my-package"
    );
    assert_eq!(
        normalize_package_name("my___package", &Ecosystem::Pypi),
        "my-package"
    );
}

#[test]
fn test_normalize_pypi_mixed_separators() {
    // PEP 503: runs of `[-_.]` collapse to single `-`
    assert_eq!(normalize_package_name("a-._b", &Ecosystem::Pypi), "a-b");
}

#[test]
fn test_normalize_cargo_name() {
    assert_eq!(
        normalize_package_name("my-crate", &Ecosystem::Cargo),
        "my_crate"
    );
}

#[test]
fn test_normalize_nuget_name() {
    assert_eq!(
        normalize_package_name("Newtonsoft.Json", &Ecosystem::Nuget),
        "newtonsoft.json"
    );
}

#[test]
fn test_normalize_rubygems_name() {
    assert_eq!(
        normalize_package_name("My-Gem", &Ecosystem::Rubygems),
        "my_gem"
    );
}

#[test]
fn test_normalize_composer_name() {
    assert_eq!(
        normalize_package_name("Vendor/Pkg", &Ecosystem::Composer),
        "vendor/pkg"
    );
}

#[test]
fn test_normalize_maven_name() {
    assert_eq!(
        normalize_package_name("Com.Acme:Foo", &Ecosystem::Maven),
        "com.acme:foo"
    );
}

#[test]
fn test_normalize_oci_name() {
    assert_eq!(
        normalize_package_name("My/Image", &Ecosystem::Oci),
        "my/image"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// validate_package_name
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_npm_name_valid() {
    assert!(validate_package_name("my-package", &Ecosystem::Npm).is_ok());
    assert!(validate_package_name("@acme/foo", &Ecosystem::Npm).is_ok());
}

#[test]
fn test_npm_name_invalid() {
    assert!(validate_package_name("", &Ecosystem::Npm).is_err());
    assert!(validate_package_name("UPPERCASE", &Ecosystem::Npm).is_err());
}

#[test]
fn test_pypi_name_valid() {
    assert!(validate_package_name("my-package", &Ecosystem::Pypi).is_ok());
    assert!(validate_package_name("MyPackage", &Ecosystem::Pypi).is_ok());
}

#[test]
fn test_pypi_name_empty_is_invalid() {
    assert!(validate_package_name("", &Ecosystem::Pypi).is_err());
}

#[test]
fn test_cargo_name_valid() {
    assert!(validate_package_name("my_crate", &Ecosystem::Cargo).is_ok());
    assert!(validate_package_name("MyAwesomeCrate", &Ecosystem::Cargo).is_ok());
}

#[test]
fn test_cargo_name_invalid_starts_with_digit() {
    assert!(validate_package_name("1bad", &Ecosystem::Cargo).is_err());
}

#[test]
fn test_nuget_name_valid() {
    assert!(validate_package_name("Newtonsoft.Json", &Ecosystem::Nuget).is_ok());
}

#[test]
fn test_nuget_name_too_long() {
    let long = "a".repeat(101);
    assert!(validate_package_name(&long, &Ecosystem::Nuget).is_err());
}

#[test]
fn test_rubygems_name_valid() {
    assert!(validate_package_name("rails", &Ecosystem::Rubygems).is_ok());
    assert!(validate_package_name("my_gem-1.0", &Ecosystem::Rubygems).is_ok());
}

#[test]
fn test_rubygems_name_special_chars_invalid() {
    assert!(validate_package_name("gem with spaces", &Ecosystem::Rubygems).is_err());
}

#[test]
fn test_composer_name_valid() {
    assert!(validate_package_name("vendor/package", &Ecosystem::Composer).is_ok());
}

#[test]
fn test_composer_name_invalid_no_slash() {
    assert!(validate_package_name("novendor", &Ecosystem::Composer).is_err());
}

#[test]
fn test_maven_name_valid() {
    assert!(validate_package_name("com.example:my-artifact", &Ecosystem::Maven).is_ok());
}

#[test]
fn test_maven_name_invalid_no_colon() {
    assert!(validate_package_name("noartifact", &Ecosystem::Maven).is_err());
}

#[test]
fn test_oci_name_valid() {
    assert!(validate_package_name("my/image", &Ecosystem::Oci).is_ok());
    assert!(validate_package_name("library/nginx", &Ecosystem::Oci).is_ok());
}

#[test]
fn test_oci_name_invalid_uppercase() {
    assert!(validate_package_name("MY/IMAGE", &Ecosystem::Oci).is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// validate_username
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_username_valid() {
    assert!(validate_username("alice").is_ok());
    assert!(validate_username("alice_b").is_ok());
    assert!(validate_username("alice-b").is_ok());
    assert!(validate_username("Alice123").is_ok());
}

#[test]
fn test_username_too_short() {
    assert!(validate_username("a").is_err());
}

#[test]
fn test_username_invalid_chars() {
    assert!(validate_username("alice@b").is_err());
    assert!(validate_username("alice.b").is_err());
}

#[test]
fn test_username_max_length_boundary() {
    let name_39 = "a".repeat(39);
    assert!(validate_username(&name_39).is_ok());
    let name_40 = "a".repeat(40);
    assert!(validate_username(&name_40).is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// validate_email
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_email_valid() {
    assert!(validate_email("alice@example.com").is_ok());
    assert!(validate_email("alice+tag@example.co.uk").is_ok());
}

#[test]
fn test_email_invalid() {
    assert!(validate_email("not-an-email").is_err());
    assert!(validate_email("@example.com").is_err());
}

#[test]
fn test_email_missing_tld() {
    assert!(validate_email("user@localhost").is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// validate_slug
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_slug_valid() {
    assert!(validate_slug("my-org").is_ok());
    assert!(validate_slug("acme123").is_ok());
}

#[test]
fn test_slug_invalid_uppercase() {
    assert!(validate_slug("MyOrg").is_err());
}

#[test]
fn test_slug_invalid_leading_hyphen() {
    assert!(validate_slug("-bad").is_err());
}

#[test]
fn test_slug_max_length() {
    let slug_64 = "a".repeat(64);
    assert!(validate_slug(&slug_64).is_ok());
    let slug_65 = "a".repeat(65);
    assert!(validate_slug(&slug_65).is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// validate_version
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_version_valid() {
    assert!(validate_version("1.0.0").is_ok());
    assert!(validate_version("0.1.0-beta.1").is_ok());
}

#[test]
fn test_version_empty() {
    assert!(validate_version("").is_err());
}

#[test]
fn test_version_too_long() {
    let long = "1.".repeat(33);
    assert!(validate_version(&long).is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// policy: reserved names
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_reserved_names() {
    assert!(is_reserved_name("admin"));
    assert!(is_reserved_name("npm"));
    assert!(is_reserved_name("ADMIN")); // case-insensitive
    assert!(!is_reserved_name("mypackage"));
}

#[test]
fn test_reserved_names_full_list() {
    for name in &[
        "root", "system", "publaryn", "registry", "api", "docker", "oci", "security",
    ] {
        assert!(is_reserved_name(name), "Expected '{name}' to be reserved");
    }
}

#[test]
fn test_name_similarity_identical() {
    assert!((name_similarity("foo", "foo") - 1.0).abs() < 1e-9);
}

#[test]
fn test_name_similarity_different() {
    let sim = name_similarity("foo", "bar");
    assert!(sim < 1.0);
    assert!(sim >= 0.0);
}

#[test]
fn test_name_similarity_empty_strings() {
    assert!((name_similarity("", "") - 1.0).abs() < 1e-9);
}

#[test]
fn test_policy_violation_reserved() {
    let violations = check_name_policy("admin", &[], &Ecosystem::Npm).unwrap();
    assert!(violations
        .iter()
        .any(|v| matches!(v, PolicyViolation::ReservedName(_))));
}

#[test]
fn test_policy_no_violation_unique_name() {
    let existing = vec![
        "completely-different".to_owned(),
        "other-package".to_owned(),
    ];
    let violations = check_name_policy("my-new-package", &existing, &Ecosystem::Npm).unwrap();
    assert!(violations.is_empty());
}

#[test]
fn test_policy_similar_name_flagged() {
    let existing = vec!["my-package".to_owned()];
    let violations = check_name_policy("my-packag3", &existing, &Ecosystem::Npm).unwrap();
    let has_similar = violations
        .iter()
        .any(|v| matches!(v, PolicyViolation::SimilarNameExists { .. }));
    assert!(
        has_similar,
        "Expected a similarity violation, got: {violations:?}"
    );
}

#[test]
fn test_policy_exact_duplicate_not_flagged_as_similar() {
    // If the name is identical (after lowercasing), similarity should not trigger
    // because the condition requires sim >= threshold AND not identical.
    let existing = vec!["mypackage".to_owned()];
    let violations = check_name_policy("mypackage", &existing, &Ecosystem::Npm).unwrap();
    let has_similar = violations
        .iter()
        .any(|v| matches!(v, PolicyViolation::SimilarNameExists { .. }));
    assert!(
        !has_similar,
        "Exact match should not trigger similarity warning"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// policy: max artifact sizes
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_oci_max_artifact_size() {
    assert_eq!(max_artifact_size(&Ecosystem::Oci), 10 * 1024 * 1024 * 1024);
}

#[test]
fn test_maven_max_artifact_size() {
    assert_eq!(max_artifact_size(&Ecosystem::Maven), 512 * 1024 * 1024);
}

#[test]
fn test_default_max_artifact_size() {
    assert_eq!(max_artifact_size(&Ecosystem::Npm), 256 * 1024 * 1024);
    assert_eq!(max_artifact_size(&Ecosystem::Pypi), 256 * 1024 * 1024);
    assert_eq!(max_artifact_size(&Ecosystem::Cargo), 256 * 1024 * 1024);
}

// ══════════════════════════════════════════════════════════════════════════════
// policy: PolicyViolation Display
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_policy_violation_display_reserved() {
    let v = PolicyViolation::ReservedName("admin".into());
    assert_eq!(v.to_string(), "Package name 'admin' is reserved");
}

#[test]
fn test_policy_violation_display_similar() {
    let v = PolicyViolation::SimilarNameExists {
        existing: "foo".into(),
        similarity: 0.9,
    };
    assert!(v.to_string().contains("90%"));
    assert!(v.to_string().contains("foo"));
}

#[test]
fn test_policy_violation_display_too_large() {
    let v = PolicyViolation::PackageTooLarge {
        size_bytes: 1000,
        limit_bytes: 500,
    };
    let s = v.to_string();
    assert!(s.contains("1000"));
    assert!(s.contains("500"));
}

#[test]
fn test_policy_violation_converts_to_error() {
    let v = PolicyViolation::MalwareDetected;
    let err: Error = v.into();
    assert!(matches!(err, Error::PolicyViolation(_)));
}

// ══════════════════════════════════════════════════════════════════════════════
// error: Error type
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_error_display() {
    let err = Error::NotFound("package foo".into());
    assert_eq!(err.to_string(), "Not found: package foo");

    let err = Error::Forbidden("not allowed".into());
    assert_eq!(err.to_string(), "Forbidden: not allowed");
}

#[test]
fn test_error_variants_distinct() {
    let e1 = Error::NotFound("a".into());
    let e2 = Error::AlreadyExists("a".into());
    assert_ne!(e1.to_string(), e2.to_string());
}

// ══════════════════════════════════════════════════════════════════════════════
// Token helpers
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_generate_random_token() {
    let tok = publaryn_core::security::generate_random_token(32);
    assert_eq!(tok.len(), 64); // hex-encoded 32 bytes = 64 chars
}

#[test]
fn test_generate_random_token_uniqueness() {
    let t1 = publaryn_core::security::generate_random_token(32);
    let t2 = publaryn_core::security::generate_random_token(32);
    assert_ne!(t1, t2);
}

#[test]
fn test_hash_token_deterministic() {
    let raw = "my_secret_token";
    let h1 = publaryn_core::security::hash_token(raw);
    let h2 = publaryn_core::security::hash_token(raw);
    assert_eq!(h1, h2);
}

#[test]
fn test_hash_token_different_inputs() {
    let h1 = publaryn_core::security::hash_token("token_a");
    let h2 = publaryn_core::security::hash_token("token_b");
    assert_ne!(h1, h2);
}

#[test]
fn test_verify_sha256() {
    let data = b"hello world";
    let hex = publaryn_core::security::sha256_hex(data);
    assert!(publaryn_core::security::verify_sha256(data, &hex));
    assert!(!publaryn_core::security::verify_sha256(b"other", &hex));
}

#[test]
fn test_sha256_hex_known_value() {
    // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let hex = publaryn_core::security::sha256_hex(b"");
    assert_eq!(
        hex,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Domain model: User construction
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_user_new_defaults() {
    let user = User::new("alice".into(), "alice@test.dev".into(), Some("hash".into()));
    assert!(!user.is_admin);
    assert!(user.is_active);
    assert!(!user.email_verified);
    assert!(!user.mfa_enabled);
    assert!(user.display_name.is_none());
}

// ══════════════════════════════════════════════════════════════════════════════
// Domain model: OrgRole
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_org_role_as_str() {
    assert_eq!(OrgRole::Owner.as_str(), "owner");
    assert_eq!(OrgRole::SecurityManager.as_str(), "security_manager");
}

#[test]
fn test_org_role_from_str() {
    assert_eq!(OrgRole::from_str("owner").unwrap(), OrgRole::Owner);
    assert_eq!(
        OrgRole::from_str("security-manager").unwrap(),
        OrgRole::SecurityManager
    );
    assert!(OrgRole::from_str("nonexistent").is_err());
}

#[test]
fn test_org_role_is_owner() {
    assert!(OrgRole::Owner.is_owner());
    assert!(!OrgRole::Admin.is_owner());
}

// ══════════════════════════════════════════════════════════════════════════════
// Domain model: Ecosystem
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_ecosystem_as_str_round_trip() {
    let ecosystems = [
        Ecosystem::Npm,
        Ecosystem::Bun,
        Ecosystem::Pypi,
        Ecosystem::Composer,
        Ecosystem::Nuget,
        Ecosystem::Rubygems,
        Ecosystem::Maven,
        Ecosystem::Oci,
        Ecosystem::Cargo,
    ];
    for eco in &ecosystems {
        let s = eco.as_str();
        assert!(!s.is_empty(), "Ecosystem {:?} returned empty string", eco);
    }
}

#[test]
fn test_ecosystem_bun_protocol_family_is_npm() {
    assert_eq!(Ecosystem::Bun.protocol_family(), "npm");
}

#[test]
fn test_ecosystem_display() {
    assert_eq!(format!("{}", Ecosystem::Cargo), "cargo");
}

// ══════════════════════════════════════════════════════════════════════════════
// Organization invitations
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_org_role_parses_from_supported_strings() {
    assert_eq!(OrgRole::from_str("admin").unwrap(), OrgRole::Admin);
    assert_eq!(
        OrgRole::from_str("security-manager").unwrap(),
        OrgRole::SecurityManager
    );
    assert_eq!(
        OrgRole::from_str("billing_manager").unwrap(),
        OrgRole::BillingManager
    );
}

#[test]
fn test_org_role_unknown_value_is_rejected() {
    assert!(OrgRole::from_str("supreme-overlord").is_err());
}

#[test]
fn test_org_invitation_new_requires_future_expiry() {
    let result = OrganizationInvitation::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        OrgRole::Viewer,
        Uuid::new_v4(),
        Utc::now() - Duration::minutes(1),
    );

    assert!(result.is_err());
}

#[test]
fn test_org_invitation_status_pending() {
    let invitation = OrganizationInvitation::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        OrgRole::Viewer,
        Uuid::new_v4(),
        Utc::now() + Duration::days(3),
    )
    .unwrap();

    assert_eq!(
        invitation.status_at(Utc::now()),
        OrganizationInvitationStatus::Pending
    );
    assert!(invitation.is_actionable_at(Utc::now()));
}

#[test]
fn test_org_invitation_status_expired() {
    let mut invitation = OrganizationInvitation::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        OrgRole::Viewer,
        Uuid::new_v4(),
        Utc::now() + Duration::minutes(5),
    )
    .unwrap();
    invitation.expires_at = Utc::now() - Duration::minutes(1);

    assert_eq!(
        invitation.status_at(Utc::now()),
        OrganizationInvitationStatus::Expired
    );
    assert!(!invitation.is_actionable_at(Utc::now()));
}

#[test]
fn test_org_invitation_status_accepted() {
    let mut invitation = OrganizationInvitation::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        OrgRole::Viewer,
        Uuid::new_v4(),
        Utc::now() + Duration::days(1),
    )
    .unwrap();
    invitation.accepted_at = Some(Utc::now());
    invitation.accepted_by = Some(Uuid::new_v4());

    assert_eq!(
        invitation.status_at(Utc::now()),
        OrganizationInvitationStatus::Accepted
    );
    assert!(!invitation.is_actionable_at(Utc::now()));
}
