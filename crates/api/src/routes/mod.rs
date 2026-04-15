use publaryn_core::domain::namespace::Ecosystem;

use crate::error::ApiError;
use publaryn_core::error::Error;

pub mod audit;
pub mod auth;
pub mod health;
pub mod namespaces;
pub mod openapi;
pub mod orgs;
pub mod org_invitations;
pub mod packages;
pub mod pypi_oidc;
pub mod repositories;
pub mod search;
pub mod security;
pub mod tokens;
pub mod trusted_publishers;
pub mod users;

pub(crate) fn parse_ecosystem(s: &str) -> Result<Ecosystem, ApiError> {
    match s.to_lowercase().as_str() {
        "npm" | "bun" => Ok(Ecosystem::Npm),
        "pypi" => Ok(Ecosystem::Pypi),
        "cargo" => Ok(Ecosystem::Cargo),
        "nuget" => Ok(Ecosystem::Nuget),
        "rubygems" => Ok(Ecosystem::Rubygems),
        "maven" => Ok(Ecosystem::Maven),
        "composer" => Ok(Ecosystem::Composer),
        "oci" => Ok(Ecosystem::Oci),
        other => Err(ApiError(Error::Validation(format!("Unknown ecosystem: {other}")))),
    }
}
