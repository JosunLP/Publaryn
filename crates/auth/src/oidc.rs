use publaryn_core::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// OIDC discovery document (subset).
#[derive(Debug, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub jwks_uri: String,
    pub token_endpoint: Option<String>,
    pub userinfo_endpoint: Option<String>,
}

/// Claims extracted from an OIDC ID token for trusted publishing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPublishingClaims {
    /// Issuer URL (e.g. `https://token.actions.githubusercontent.com`).
    pub iss: String,
    /// Subject — CI job identifier.
    pub sub: String,
    /// Repository (e.g. `org/repo`).
    pub repository: Option<String>,
    /// Workflow ref.
    pub workflow_ref: Option<String>,
    /// Environment.
    pub environment: Option<String>,
}

/// Supported OIDC trusted publishing issuers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedIssuer {
    GitHubActions,
    GitLabCi,
    AzureDevOps,
    Custom(String),
}

impl TrustedIssuer {
    pub fn discovery_url(&self) -> String {
        match self {
            TrustedIssuer::GitHubActions => {
                "https://token.actions.githubusercontent.com/.well-known/openid-configuration"
                    .to_owned()
            }
            TrustedIssuer::GitLabCi => {
                "https://gitlab.com/.well-known/openid-configuration".to_owned()
            }
            TrustedIssuer::AzureDevOps => {
                "https://vstoken.dev.azure.com/.well-known/openid-configuration".to_owned()
            }
            TrustedIssuer::Custom(url) => {
                format!("{url}/.well-known/openid-configuration")
            }
        }
    }

    /// Parse an issuer URL into a known variant.
    pub fn from_issuer_url(url: &str) -> Self {
        match url {
            s if s.contains("token.actions.githubusercontent.com") => {
                TrustedIssuer::GitHubActions
            }
            s if s.contains("gitlab.com") => TrustedIssuer::GitLabCi,
            s if s.contains("vstoken.dev.azure.com") => TrustedIssuer::AzureDevOps,
            other => TrustedIssuer::Custom(other.to_owned()),
        }
    }
}

/// Verify that the issuer is in the allow-list.
pub fn assert_trusted_issuer(issuer: &str, allowed: &[TrustedIssuer]) -> Result<()> {
    let parsed = TrustedIssuer::from_issuer_url(issuer);
    if allowed.contains(&parsed) {
        Ok(())
    } else {
        Err(Error::Unauthorized(format!(
            "OIDC issuer '{issuer}' is not trusted"
        )))
    }
}
