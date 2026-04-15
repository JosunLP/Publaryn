use publaryn_core::error::Error;

use crate::{
    error::{ApiError, ApiResult},
    request_auth::AuthenticatedIdentity,
};

pub const SCOPE_PROFILE_WRITE: &str = "profile:write";
pub const SCOPE_TOKENS_READ: &str = "tokens:read";
pub const SCOPE_TOKENS_WRITE: &str = "tokens:write";
pub const SCOPE_ORGS_WRITE: &str = "orgs:write";
pub const SCOPE_NAMESPACES_WRITE: &str = "namespaces:write";
pub const SCOPE_REPOSITORIES_WRITE: &str = "repositories:write";
pub const SCOPE_PACKAGES_WRITE: &str = "packages:write";
pub const SCOPE_AUDIT_READ: &str = "audit:read";

const DEFAULT_SESSION_SCOPES: &[&str] = &[
    SCOPE_PROFILE_WRITE,
    SCOPE_TOKENS_READ,
    SCOPE_TOKENS_WRITE,
    SCOPE_ORGS_WRITE,
    SCOPE_NAMESPACES_WRITE,
    SCOPE_REPOSITORIES_WRITE,
    SCOPE_PACKAGES_WRITE,
];

const GRANTABLE_SCOPES: &[&str] = &[
    SCOPE_PROFILE_WRITE,
    SCOPE_TOKENS_READ,
    SCOPE_TOKENS_WRITE,
    SCOPE_ORGS_WRITE,
    SCOPE_NAMESPACES_WRITE,
    SCOPE_REPOSITORIES_WRITE,
    SCOPE_PACKAGES_WRITE,
    SCOPE_AUDIT_READ,
];

const ADMIN_ONLY_SCOPES: &[&str] = &[SCOPE_AUDIT_READ];

pub fn default_session_scopes(is_platform_admin: bool) -> Vec<String> {
    let mut scopes = DEFAULT_SESSION_SCOPES
        .iter()
        .map(|scope| (*scope).to_owned())
        .collect::<Vec<_>>();

    if is_platform_admin {
        scopes.push(SCOPE_AUDIT_READ.to_owned());
    }

    scopes
}

pub fn supported_scopes() -> &'static [&'static str] {
    GRANTABLE_SCOPES
}

pub fn normalize_requested_scopes(scopes: &[String]) -> Result<Vec<String>, Error> {
    if scopes.is_empty() {
        return Err(Error::Validation(
            "At least one scope is required when creating an API token".into(),
        ));
    }

    let mut normalized = scopes
        .iter()
        .map(|scope| scope.trim())
        .map(|scope| {
            if scope.is_empty() {
                return Err(Error::Validation(
                    "Token scopes must not contain empty values".into(),
                ));
            }

            if !supported_scopes().contains(&scope) {
                return Err(Error::Validation(format!("Unknown token scope: {scope}")));
            }

            Ok(scope.to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;

    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

pub fn ensure_scope_grant_allowed(
    requested_scopes: &[String],
    is_platform_admin: bool,
) -> ApiResult<()> {
    if !is_platform_admin {
        if let Some(scope) = requested_scopes
            .iter()
            .find(|scope| ADMIN_ONLY_SCOPES.contains(&scope.as_str()))
        {
            return Err(ApiError(Error::Forbidden(format!(
                "Only platform administrators can grant the '{scope}' scope"
            ))));
        }
    }

    Ok(())
}

pub fn ensure_scope(identity: &AuthenticatedIdentity, scope: &str) -> ApiResult<()> {
    if identity.scopes().iter().any(|candidate| candidate == scope) {
        return Ok(());
    }

    Err(ApiError(Error::Forbidden(format!(
        "This operation requires the '{scope}' scope"
    ))))
}

#[cfg(test)]
mod tests {
    use super::{
        default_session_scopes, ensure_scope_grant_allowed, normalize_requested_scopes,
        supported_scopes, SCOPE_AUDIT_READ, SCOPE_PACKAGES_WRITE, SCOPE_PROFILE_WRITE,
    };

    #[test]
    fn default_session_scopes_for_standard_users_exclude_admin_scope() {
        let scopes = default_session_scopes(false);

        assert!(scopes.contains(&SCOPE_PROFILE_WRITE.to_owned()));
        assert!(!scopes.contains(&SCOPE_AUDIT_READ.to_owned()));
    }

    #[test]
    fn default_session_scopes_for_admins_include_admin_scope() {
        let scopes = default_session_scopes(true);

        assert!(scopes.contains(&SCOPE_AUDIT_READ.to_owned()));
    }

    #[test]
    fn normalize_requested_scopes_sorts_and_deduplicates() {
        let scopes = vec![
            SCOPE_PACKAGES_WRITE.to_owned(),
            SCOPE_PROFILE_WRITE.to_owned(),
            SCOPE_PACKAGES_WRITE.to_owned(),
        ];

        let normalized = normalize_requested_scopes(&scopes).expect("scopes should normalize");

        assert_eq!(
            normalized,
            vec![SCOPE_PACKAGES_WRITE.to_owned(), SCOPE_PROFILE_WRITE.to_owned()]
        );
    }

    #[test]
    fn normalize_requested_scopes_rejects_unknown_scope() {
        let error = normalize_requested_scopes(&["definitely:not-valid".to_owned()])
            .expect_err("unknown scopes must be rejected");

        assert_eq!(
            error.to_string(),
            "Validation error: Unknown token scope: definitely:not-valid"
        );
    }

    #[test]
    fn non_admins_cannot_grant_admin_only_scope() {
        let error = ensure_scope_grant_allowed(&[SCOPE_AUDIT_READ.to_owned()], false)
            .expect_err("standard users must not grant admin-only scopes");

        assert_eq!(
            error.0.to_string(),
            "Forbidden: Only platform administrators can grant the 'audit:read' scope"
        );
    }

    #[test]
    fn supported_scope_list_contains_expected_package_scope() {
        assert!(supported_scopes().contains(&SCOPE_PACKAGES_WRITE));
    }
}
