use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::{namespace::Ecosystem, repository::Visibility};

/// Visibility of a package (mirrors Repository visibility but scoped to the package).
pub type PackageVisibility = Visibility;

/// An ecosystem-specific package identity.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Package {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub ecosystem: Ecosystem,
    /// Canonical name as provided by the publisher.
    pub name: String,
    /// Normalized name for de-duplication (e.g. lowercase, hyphens).
    pub normalized_name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub readme: Option<String>,
    pub homepage: Option<String>,
    pub repository_url: Option<String>,
    pub license: Option<String>,
    pub keywords: Vec<String>,
    pub visibility: PackageVisibility,
    /// Owner user (personal package).
    pub owner_user_id: Option<Uuid>,
    /// Owner organization.
    pub owner_org_id: Option<Uuid>,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub is_archived: bool,
    pub download_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Package {
    pub fn new(
        repository_id: Uuid,
        ecosystem: Ecosystem,
        name: String,
        visibility: PackageVisibility,
    ) -> Self {
        let normalized_name = normalize_package_name(&name, &ecosystem);
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            repository_id,
            ecosystem,
            normalized_name,
            name,
            display_name: None,
            description: None,
            readme: None,
            homepage: None,
            repository_url: None,
            license: None,
            keywords: vec![],
            visibility,
            owner_user_id: None,
            owner_org_id: None,
            is_deprecated: false,
            deprecation_message: None,
            is_archived: false,
            download_count: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Normalizes a package name according to ecosystem conventions.
pub fn normalize_package_name(name: &str, ecosystem: &Ecosystem) -> String {
    match ecosystem {
        Ecosystem::Npm | Ecosystem::Bun => name.to_lowercase(),
        Ecosystem::Pypi => normalize_pypi_name(name),
        Ecosystem::Cargo => name.to_lowercase().replace('-', "_"),
        Ecosystem::Nuget => name.to_lowercase(),
        Ecosystem::Rubygems => name.to_lowercase().replace('-', "_"),
        Ecosystem::Composer => name.to_lowercase(),
        Ecosystem::Maven => name.to_lowercase(),
        Ecosystem::Oci => name.to_lowercase(),
    }
}

fn normalize_pypi_name(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    let mut previous_was_separator = false;

    for character in name.chars() {
        match character {
            '-' | '_' | '.' => {
                if !previous_was_separator {
                    normalized.push('-');
                    previous_was_separator = true;
                }
            }
            other => {
                normalized.push(other.to_ascii_lowercase());
                previous_was_separator = false;
            }
        }
    }

    normalized
}
