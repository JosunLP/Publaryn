use once_cell::sync::Lazy;
use regex::Regex;

use crate::domain::namespace::Ecosystem;
use crate::error::{Error, Result};

static SEMVER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\d+\.\d+\.\d+([.-][0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)*$").unwrap()
});

static SLUG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-z0-9][a-z0-9_-]{0,63}$").unwrap()
});

static USERNAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_-]{1,38}$").unwrap()
});

static EMAIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}$").unwrap()
});

/// Validate a package name for a given ecosystem.
pub fn validate_package_name(name: &str, ecosystem: &Ecosystem) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Validation("Package name must not be empty".into()));
    }
    match ecosystem {
        Ecosystem::Npm | Ecosystem::Bun => validate_npm_name(name),
        Ecosystem::Pypi => validate_pypi_name(name),
        Ecosystem::Cargo => validate_cargo_name(name),
        Ecosystem::Nuget => validate_nuget_name(name),
        Ecosystem::Rubygems => validate_rubygems_name(name),
        Ecosystem::Composer => validate_composer_name(name),
        Ecosystem::Maven => validate_maven_name(name),
        Ecosystem::Oci => validate_oci_name(name),
    }
}

fn validate_npm_name(name: &str) -> Result<()> {
    let re = Regex::new(r"^(@[a-z0-9-~][a-z0-9-._~]*/)?[a-z0-9-~][a-z0-9-._~]*$").unwrap();
    if !re.is_match(name) || name.len() > 214 {
        return Err(Error::Validation(format!("Invalid npm package name: {name}")));
    }
    Ok(())
}

fn validate_pypi_name(name: &str) -> Result<()> {
    let re = Regex::new(r"^([A-Z0-9]|[A-Z0-9][A-Z0-9._-]*[A-Z0-9])$").unwrap();
    if !re.is_match(&name.to_uppercase()) || name.len() > 200 {
        return Err(Error::Validation(format!("Invalid PyPI package name: {name}")));
    }
    Ok(())
}

fn validate_cargo_name(name: &str) -> Result<()> {
    let re = Regex::new(r"^[A-Za-z][A-Za-z0-9_-]{0,63}$").unwrap();
    if !re.is_match(name) {
        return Err(Error::Validation(format!("Invalid Cargo crate name: {name}")));
    }
    Ok(())
}

fn validate_nuget_name(name: &str) -> Result<()> {
    if name.len() > 100 {
        return Err(Error::Validation("NuGet package ID too long".into()));
    }
    Ok(())
}

fn validate_rubygems_name(name: &str) -> Result<()> {
    let re = Regex::new(r"^[A-Za-z0-9._-]+$").unwrap();
    if !re.is_match(name) || name.len() > 200 {
        return Err(Error::Validation(format!("Invalid RubyGems gem name: {name}")));
    }
    Ok(())
}

fn validate_composer_name(name: &str) -> Result<()> {
    let re = Regex::new(r"^[a-z0-9]([_.-]?[a-z0-9]+)*/[a-z0-9]([_.-]?[a-z0-9]+)*$").unwrap();
    if !re.is_match(name) {
        return Err(Error::Validation(format!("Invalid Composer package name: {name}")));
    }
    Ok(())
}

fn validate_maven_name(name: &str) -> Result<()> {
    // groupId:artifactId
    let parts: Vec<&str> = name.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(Error::Validation(
            "Maven package name must be groupId:artifactId".into(),
        ));
    }
    Ok(())
}

fn validate_oci_name(name: &str) -> Result<()> {
    let re = Regex::new(r"^[a-z0-9][a-z0-9._/-]*[a-z0-9]$").unwrap();
    if !re.is_match(name) {
        return Err(Error::Validation(format!("Invalid OCI image name: {name}")));
    }
    Ok(())
}

/// Validate a semantic version string (permissive, not strict SemVer).
pub fn validate_version(version: &str) -> Result<()> {
    if version.is_empty() {
        return Err(Error::Validation("Version must not be empty".into()));
    }
    if version.len() > 64 {
        return Err(Error::Validation("Version string too long".into()));
    }
    Ok(())
}

/// Validate a username.
pub fn validate_username(username: &str) -> Result<()> {
    if !USERNAME_RE.is_match(username) {
        return Err(Error::Validation(format!(
            "Invalid username: '{username}'. Must be 2-39 characters, alphanumeric, hyphens or underscores."
        )));
    }
    Ok(())
}

/// Validate an email address.
pub fn validate_email(email: &str) -> Result<()> {
    if !EMAIL_RE.is_match(email) {
        return Err(Error::Validation(format!("Invalid email address: {email}")));
    }
    Ok(())
}

/// Validate an org/repository slug.
pub fn validate_slug(slug: &str) -> Result<()> {
    if !SLUG_RE.is_match(slug) {
        return Err(Error::Validation(format!(
            "Invalid slug: '{slug}'. Must start with alphanumeric, up to 64 characters."
        )));
    }
    Ok(())
}
