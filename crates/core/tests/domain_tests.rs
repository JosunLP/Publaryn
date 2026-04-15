use publaryn_core::{
    domain::{
        namespace::Ecosystem,
        organization::OrgRole,
        organization_invitation::{OrganizationInvitation, OrganizationInvitationStatus},
        package::normalize_package_name,
    },
    policy::{check_name_policy, is_reserved_name, name_similarity, PolicyViolation},
    validation::{validate_email, validate_package_name, validate_slug, validate_username},
};
use chrono::{Duration, Utc};
use std::str::FromStr;
use uuid::Uuid;

// ── normalize_package_name ─────────────────────────────────────────────────

#[test]
fn test_normalize_npm_name() {
    assert_eq!(normalize_package_name("MyPackage", &Ecosystem::Npm), "mypackage");
    assert_eq!(normalize_package_name("@acme/foo", &Ecosystem::Npm), "@acme/foo");
}

#[test]
fn test_normalize_pypi_name() {
    assert_eq!(normalize_package_name("My-Package", &Ecosystem::Pypi), "my_package");
    assert_eq!(normalize_package_name("my.package", &Ecosystem::Pypi), "my_package");
}

#[test]
fn test_normalize_cargo_name() {
    assert_eq!(normalize_package_name("my-crate", &Ecosystem::Cargo), "my_crate");
}

// ── validate_package_name ──────────────────────────────────────────────────

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
fn test_cargo_name_valid() {
    assert!(validate_package_name("my_crate", &Ecosystem::Cargo).is_ok());
    assert!(validate_package_name("MyAwesomeCrate", &Ecosystem::Cargo).is_ok());
}

#[test]
fn test_cargo_name_invalid_starts_with_digit() {
    assert!(validate_package_name("1bad", &Ecosystem::Cargo).is_err());
}

#[test]
fn test_composer_name_valid() {
    assert!(validate_package_name("vendor/package", &Ecosystem::Composer).is_ok());
}

#[test]
fn test_composer_name_invalid_no_slash() {
    assert!(validate_package_name("novendor", &Ecosystem::Composer).is_err());
}

// ── validate_username ──────────────────────────────────────────────────────

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

// ── validate_email ─────────────────────────────────────────────────────────

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

// ── validate_slug ──────────────────────────────────────────────────────────

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

// ── policy ─────────────────────────────────────────────────────────────────

#[test]
fn test_reserved_names() {
    assert!(is_reserved_name("admin"));
    assert!(is_reserved_name("npm"));
    assert!(is_reserved_name("ADMIN")); // case-insensitive
    assert!(!is_reserved_name("mypackage"));
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
fn test_policy_violation_reserved() {
    let violations = check_name_policy("admin", &[], &Ecosystem::Npm).unwrap();
    assert!(violations
        .iter()
        .any(|v| matches!(v, PolicyViolation::ReservedName(_))));
}

#[test]
fn test_policy_no_violation_unique_name() {
    let existing = vec!["completely-different".to_owned(), "other-package".to_owned()];
    let violations = check_name_policy("my-new-package", &existing, &Ecosystem::Npm).unwrap();
    assert!(violations.is_empty());
}

#[test]
fn test_policy_similar_name_flagged() {
    let existing = vec!["my-package".to_owned()];
    let violations = check_name_policy("my-packag3", &existing, &Ecosystem::Npm).unwrap();
    // "my-packag3" vs "my-package" is close — should flag similarity
    let has_similar = violations
        .iter()
        .any(|v| matches!(v, PolicyViolation::SimilarNameExists { .. }));
    assert!(has_similar, "Expected a similarity violation, got: {violations:?}");
}

// ── Token helpers ──────────────────────────────────────────────────────────

#[test]
fn test_generate_random_token() {
    let tok = publaryn_core::security::generate_random_token(32);
    assert_eq!(tok.len(), 64); // hex-encoded 32 bytes = 64 chars
}

#[test]
fn test_hash_token_deterministic() {
    let raw = "my_secret_token";
    let h1 = publaryn_core::security::hash_token(raw);
    let h2 = publaryn_core::security::hash_token(raw);
    assert_eq!(h1, h2);
}

#[test]
fn test_verify_sha256() {
    let data = b"hello world";
    let hex = publaryn_core::security::sha256_hex(data);
    assert!(publaryn_core::security::verify_sha256(data, &hex));
    assert!(!publaryn_core::security::verify_sha256(b"other", &hex));
}

// ── Ecosystem display ──────────────────────────────────────────────────────

#[test]
fn test_ecosystem_display() {
    assert_eq!(Ecosystem::Npm.to_string(), "npm");
    assert_eq!(Ecosystem::Pypi.to_string(), "pypi");
    assert_eq!(Ecosystem::Cargo.to_string(), "cargo");
}

#[test]
fn test_bun_uses_npm_protocol() {
    assert_eq!(Ecosystem::Bun.protocol_family(), "npm");
}

// ── Organization roles and invitations ─────────────────────────────────────

#[test]
fn test_org_role_parses_from_supported_strings() {
    assert_eq!(OrgRole::from_str("admin").unwrap(), OrgRole::Admin);
    assert_eq!(OrgRole::from_str("security-manager").unwrap(), OrgRole::SecurityManager);
    assert_eq!(OrgRole::from_str("billing_manager").unwrap(), OrgRole::BillingManager);
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

    assert_eq!(invitation.status_at(Utc::now()), OrganizationInvitationStatus::Pending);
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

    assert_eq!(invitation.status_at(Utc::now()), OrganizationInvitationStatus::Expired);
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

    assert_eq!(invitation.status_at(Utc::now()), OrganizationInvitationStatus::Accepted);
    assert!(!invitation.is_actionable_at(Utc::now()));
}
