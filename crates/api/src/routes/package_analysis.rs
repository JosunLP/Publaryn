use std::{
    collections::BTreeSet,
    io::{Cursor, Read},
};

use flate2::read::GzDecoder;
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use tar::Archive;
use uuid::Uuid;
use zip::ZipArchive;

use publaryn_core::{domain::namespace::Ecosystem, error::Error};

use crate::{
    error::{ApiError, ApiResult},
    storage::ArtifactStore,
};

// Cap archive probing to 64 MiB so bundle-style analysis remains responsive and
// avoids fully scanning very large release artifacts on detail page requests.
const MAX_ARCHIVE_ANALYSIS_BYTES: i64 = 64 * 1024 * 1024;
const NPM_INSTALL_LIFECYCLE_SCRIPTS: &[&str] = &[
    "preinstall",
    "install",
    "postinstall",
    "prepare",
    "prepublish",
    "prepublishOnly",
    "prepack",
    "postpack",
];

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub(crate) struct BundleAnalysisSummary {
    pub source_version: Option<String>,
    pub artifact_count: usize,
    pub total_artifact_size_bytes: Option<i64>,
    pub compressed_size_bytes: Option<i64>,
    pub install_size_bytes: Option<i64>,
    pub file_count: Option<usize>,
    pub direct_dependency_count: Option<usize>,
    pub runtime_dependency_count: Option<usize>,
    pub development_dependency_count: Option<usize>,
    pub peer_dependency_count: Option<usize>,
    pub optional_dependency_count: Option<usize>,
    pub bundled_dependency_count: Option<usize>,
    pub dependency_group_count: Option<usize>,
    pub extra_count: Option<usize>,
    pub package_type_count: Option<usize>,
    pub layer_count: Option<usize>,
    pub install_script_count: Option<usize>,
    pub has_cli_entrypoints: Option<bool>,
    pub has_tree_shaking_hints: Option<bool>,
    pub has_native_code: Option<bool>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct ArtifactRecord {
    kind: Option<String>,
    filename: String,
    size_bytes: Option<i64>,
    storage_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ArchiveProbeSummary {
    install_size_bytes: i64,
    file_count: usize,
}

pub(crate) async fn load_release_bundle_analysis(
    db: &sqlx::PgPool,
    artifact_store: &dyn ArtifactStore,
    ecosystem: &Ecosystem,
    release_id: Uuid,
    version: &str,
    provenance: Option<&Value>,
    allow_archive_probe: bool,
) -> ApiResult<BundleAnalysisSummary> {
    let artifacts = load_release_artifacts(db, release_id).await?;
    let mut notes = BTreeSet::new();
    let total_artifact_size_bytes = if artifacts.is_empty() {
        Some(0)
    } else {
        sum_known_sizes(&artifacts)
    };

    let mut summary = BundleAnalysisSummary {
        source_version: Some(version.to_owned()),
        artifact_count: artifacts.len(),
        total_artifact_size_bytes,
        compressed_size_bytes: None,
        install_size_bytes: None,
        file_count: None,
        direct_dependency_count: None,
        runtime_dependency_count: None,
        development_dependency_count: None,
        peer_dependency_count: None,
        optional_dependency_count: None,
        bundled_dependency_count: None,
        dependency_group_count: None,
        extra_count: None,
        package_type_count: None,
        layer_count: None,
        install_script_count: None,
        has_cli_entrypoints: None,
        has_tree_shaking_hints: None,
        has_native_code: None,
        notes: Vec::new(),
    };

    let primary_artifact = select_primary_artifact(ecosystem, &artifacts);
    if let Some(artifact) = primary_artifact {
        summary.compressed_size_bytes = artifact.size_bytes;
    }

    match ecosystem {
        Ecosystem::Npm | Ecosystem::Bun => {
            apply_npm_analysis_fields(&mut summary, provenance, &mut notes);
        }
        Ecosystem::Pypi => {
            apply_pypi_analysis_fields(db, &mut summary, release_id, &mut notes).await?;
        }
        Ecosystem::Cargo => {
            apply_cargo_analysis_fields(db, &mut summary, release_id, &mut notes).await?;
        }
        Ecosystem::Nuget => {
            apply_nuget_analysis_fields(db, &mut summary, release_id, &mut notes).await?;
        }
        Ecosystem::Rubygems => {
            apply_rubygems_analysis_fields(db, &mut summary, release_id, &mut notes).await?;
        }
        Ecosystem::Composer => {
            apply_composer_analysis_fields(&mut summary, provenance, &mut notes);
        }
        Ecosystem::Maven => {
            apply_maven_analysis_fields(&mut summary, provenance, &mut notes);
        }
        Ecosystem::Oci => {
            apply_oci_analysis_fields(&mut summary, provenance, &mut notes);
        }
    }

    if allow_archive_probe {
        if let Some(primary_artifact) = primary_artifact {
            if let Some(probe) =
                probe_primary_artifact(artifact_store, primary_artifact, &mut notes).await
            {
                summary.install_size_bytes = Some(probe.install_size_bytes);
                summary.file_count = Some(probe.file_count);
            }
        }
    }

    summary.notes = notes.into_iter().collect();
    Ok(summary)
}

async fn load_release_artifacts(
    db: &sqlx::PgPool,
    release_id: Uuid,
) -> ApiResult<Vec<ArtifactRecord>> {
    sqlx::query(
        "SELECT kind::text AS kind, filename, size_bytes, storage_key \
         FROM artifacts \
         WHERE release_id = $1 \
         ORDER BY uploaded_at ASC, filename ASC",
    )
    .bind(release_id)
    .fetch_all(db)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|row| ArtifactRecord {
                kind: row.try_get::<String, _>("kind").ok(),
                filename: row
                    .try_get::<String, _>("filename")
                    .unwrap_or_else(|_| "artifact".to_owned()),
                size_bytes: row.try_get::<i64, _>("size_bytes").ok(),
                storage_key: row.try_get::<String, _>("storage_key").ok(),
            })
            .collect()
    })
    .map_err(|error| ApiError(Error::Database(error)))
}

fn sum_known_sizes(artifacts: &[ArtifactRecord]) -> Option<i64> {
    let mut saw_known_size = false;
    let mut total = 0_i64;

    for artifact in artifacts {
        if let Some(size_bytes) = artifact.size_bytes {
            saw_known_size = true;
            total = total.saturating_add(size_bytes);
        }
    }

    saw_known_size.then_some(total)
}

fn select_primary_artifact<'a>(
    ecosystem: &Ecosystem,
    artifacts: &'a [ArtifactRecord],
) -> Option<&'a ArtifactRecord> {
    let preferred_kind = match ecosystem {
        Ecosystem::Npm | Ecosystem::Bun => Some("tarball"),
        Ecosystem::Pypi => Some("wheel"),
        Ecosystem::Cargo => Some("crate"),
        Ecosystem::Nuget => Some("nupkg"),
        Ecosystem::Rubygems => Some("gem"),
        Ecosystem::Composer => Some("composer_zip"),
        Ecosystem::Maven => Some("jar"),
        Ecosystem::Oci => Some("oci_manifest"),
    };

    preferred_kind
        .and_then(|kind| {
            artifacts.iter().find(|artifact| {
                artifact
                    .kind
                    .as_deref()
                    .is_some_and(|artifact_kind| artifact_kind.eq_ignore_ascii_case(kind))
            })
        })
        .or_else(|| {
            artifacts
                .iter()
                .find(|artifact| filename_matches_primary_artifact(ecosystem, &artifact.filename))
        })
        .or_else(|| artifacts.first())
}

fn filename_matches_primary_artifact(ecosystem: &Ecosystem, filename: &str) -> bool {
    let normalized = filename.to_ascii_lowercase();

    match ecosystem {
        Ecosystem::Npm | Ecosystem::Bun | Ecosystem::Cargo => {
            normalized.ends_with(".tgz")
                || normalized.ends_with(".tar.gz")
                || normalized.ends_with(".crate")
        }
        Ecosystem::Pypi => {
            normalized.ends_with(".whl")
                || normalized.ends_with(".zip")
                || normalized.ends_with(".tar.gz")
        }
        Ecosystem::Nuget => normalized.ends_with(".nupkg"),
        Ecosystem::Rubygems => normalized.ends_with(".gem"),
        Ecosystem::Composer => normalized.ends_with(".zip"),
        Ecosystem::Maven => {
            normalized.ends_with(".jar")
                || normalized.ends_with(".aar")
                || normalized.ends_with(".war")
                || normalized.ends_with(".ear")
        }
        Ecosystem::Oci => normalized.ends_with(".json"),
    }
}

fn apply_npm_analysis_fields(
    summary: &mut BundleAnalysisSummary,
    provenance: Option<&Value>,
    notes: &mut BTreeSet<String>,
) {
    let Some(object) = provenance.and_then(Value::as_object) else {
        return;
    };

    let runtime = object_key_count(object.get("dependencies"));
    let development = object_key_count(object.get("devDependencies"));
    let peer = object_key_count(object.get("peerDependencies"));
    let optional = object_key_count(object.get("optionalDependencies"));
    let bundled = bundled_dependency_count(object.get("bundleDependencies"))
        .or_else(|| bundled_dependency_count(object.get("bundledDependencies")));
    let direct_total = unique_dependency_count([
        object_keys(object.get("dependencies")),
        object_keys(object.get("peerDependencies")),
        object_keys(object.get("optionalDependencies")),
        array_string_values(object.get("bundleDependencies")),
        array_string_values(object.get("bundledDependencies")),
    ]);

    summary.runtime_dependency_count = runtime;
    summary.development_dependency_count = development;
    summary.peer_dependency_count = peer;
    summary.optional_dependency_count = optional;
    summary.bundled_dependency_count = bundled;
    summary.direct_dependency_count = direct_total;
    summary.install_script_count = install_lifecycle_script_count(object.get("scripts"));
    summary.has_cli_entrypoints = Some(has_non_empty_entrypoint(object.get("bin")));
    summary.has_tree_shaking_hints = Some(
        object.contains_key("sideEffects")
            || object.contains_key("module")
            || object.contains_key("exports"),
    );
    summary.has_native_code = Some(has_npm_native_build_hints(object));

    if let Some(license) = object.get("license").and_then(Value::as_str) {
        if !license.trim().is_empty() {
            notes.insert(format!("License: {}", license.trim()));
        }
    }
}

async fn apply_pypi_analysis_fields(
    db: &sqlx::PgPool,
    summary: &mut BundleAnalysisSummary,
    release_id: Uuid,
    notes: &mut BTreeSet<String>,
) -> ApiResult<()> {
    let row = sqlx::query(
        "SELECT requires_python, requires_dist, provides_extra \
         FROM pypi_release_metadata \
         WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_optional(db)
    .await
    .map_err(|error| ApiError(Error::Database(error)))?;

    let Some(row) = row else {
        return Ok(());
    };

    let requires_python = row
        .try_get::<Option<String>, _>("requires_python")
        .ok()
        .flatten();
    let requires_dist = row
        .try_get::<Vec<String>, _>("requires_dist")
        .unwrap_or_default();
    let provides_extra = row
        .try_get::<Vec<String>, _>("provides_extra")
        .unwrap_or_default();

    summary.direct_dependency_count = Some(requires_dist.len());
    summary.runtime_dependency_count = Some(requires_dist.len());
    summary.extra_count = Some(provides_extra.len());

    if let Some(requires_python) = requires_python.filter(|value| !value.trim().is_empty()) {
        notes.insert(format!("Requires Python {}", requires_python.trim()));
    }
    if !provides_extra.is_empty() {
        notes.insert(format!(
            "{} optional extra{} declared",
            provides_extra.len(),
            plural_suffix(provides_extra.len())
        ));
    }

    Ok(())
}

async fn apply_cargo_analysis_fields(
    db: &sqlx::PgPool,
    summary: &mut BundleAnalysisSummary,
    release_id: Uuid,
    notes: &mut BTreeSet<String>,
) -> ApiResult<()> {
    let row = sqlx::query(
        "SELECT deps, features, features2, links, rust_version \
         FROM cargo_release_metadata \
         WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_optional(db)
    .await
    .map_err(|error| ApiError(Error::Database(error)))?;

    let Some(row) = row else {
        return Ok(());
    };

    let deps = row
        .try_get::<Value, _>("deps")
        .unwrap_or_else(|_| Value::Array(Vec::new()));
    let features = row
        .try_get::<Value, _>("features")
        .unwrap_or_else(|_| Value::Object(Default::default()));
    let features2 = row.try_get::<Option<Value>, _>("features2").ok().flatten();
    let links = row.try_get::<Option<String>, _>("links").ok().flatten();
    let rust_version = row
        .try_get::<Option<String>, _>("rust_version")
        .ok()
        .flatten();

    let direct_dependency_count = deps.as_array().map_or(0, Vec::len);
    let feature_count = object_key_count(Some(&features)).unwrap_or(0)
        + features2
            .as_ref()
            .and_then(|value| object_key_count(Some(value)))
            .unwrap_or(0);

    summary.direct_dependency_count = Some(direct_dependency_count);
    summary.runtime_dependency_count = Some(direct_dependency_count);
    summary.extra_count = Some(feature_count);
    summary.has_native_code = Some(links.is_some());

    if let Some(rust_version) = rust_version.filter(|value| !value.trim().is_empty()) {
        notes.insert(format!("Requires Rust {}", rust_version.trim()));
    }
    if feature_count > 0 {
        notes.insert(format!(
            "{} feature flag{} available",
            feature_count,
            plural_suffix(feature_count)
        ));
    }

    Ok(())
}

async fn apply_nuget_analysis_fields(
    db: &sqlx::PgPool,
    summary: &mut BundleAnalysisSummary,
    release_id: Uuid,
    notes: &mut BTreeSet<String>,
) -> ApiResult<()> {
    let row = sqlx::query(
        "SELECT min_client_version, dependency_groups, package_types \
         FROM nuget_release_metadata \
         WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_optional(db)
    .await
    .map_err(|error| ApiError(Error::Database(error)))?;

    let Some(row) = row else {
        return Ok(());
    };

    let min_client_version = row
        .try_get::<Option<String>, _>("min_client_version")
        .ok()
        .flatten();
    let dependency_groups = row
        .try_get::<Value, _>("dependency_groups")
        .unwrap_or_else(|_| Value::Array(Vec::new()));
    let package_types = row
        .try_get::<Value, _>("package_types")
        .unwrap_or_else(|_| Value::Array(Vec::new()));

    let (dependency_group_count, direct_dependency_count) =
        nuget_dependency_group_metrics(&dependency_groups);

    summary.dependency_group_count = Some(dependency_group_count);
    summary.direct_dependency_count = Some(direct_dependency_count);
    summary.runtime_dependency_count = Some(direct_dependency_count);
    summary.package_type_count = Some(json_array_count(&package_types));

    if let Some(min_client_version) = min_client_version.filter(|value| !value.trim().is_empty()) {
        notes.insert(format!(
            "Minimum NuGet client {}",
            min_client_version.trim()
        ));
    }

    Ok(())
}

async fn apply_rubygems_analysis_fields(
    db: &sqlx::PgPool,
    summary: &mut BundleAnalysisSummary,
    release_id: Uuid,
    notes: &mut BTreeSet<String>,
) -> ApiResult<()> {
    let row = sqlx::query(
        "SELECT required_ruby_version, required_rubygems_version, runtime_dependencies, development_dependencies \
         FROM rubygems_release_metadata \
         WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_optional(db)
    .await
    .map_err(|error| ApiError(Error::Database(error)))?;

    let Some(row) = row else {
        return Ok(());
    };

    let required_ruby_version = row
        .try_get::<Option<String>, _>("required_ruby_version")
        .ok()
        .flatten();
    let required_rubygems_version = row
        .try_get::<Option<String>, _>("required_rubygems_version")
        .ok()
        .flatten();
    let runtime_dependencies = row
        .try_get::<Value, _>("runtime_dependencies")
        .unwrap_or_else(|_| Value::Array(Vec::new()));
    let development_dependencies = row
        .try_get::<Value, _>("development_dependencies")
        .unwrap_or_else(|_| Value::Array(Vec::new()));

    summary.runtime_dependency_count = Some(json_array_count(&runtime_dependencies));
    summary.development_dependency_count = Some(json_array_count(&development_dependencies));
    summary.direct_dependency_count = summary.runtime_dependency_count;

    if let Some(required_ruby_version) =
        required_ruby_version.filter(|value| !value.trim().is_empty())
    {
        notes.insert(format!("Requires Ruby {}", required_ruby_version.trim()));
    }
    if let Some(required_rubygems_version) =
        required_rubygems_version.filter(|value| !value.trim().is_empty())
    {
        notes.insert(format!(
            "Requires RubyGems {}",
            required_rubygems_version.trim()
        ));
    }

    Ok(())
}

fn apply_composer_analysis_fields(
    summary: &mut BundleAnalysisSummary,
    provenance: Option<&Value>,
    notes: &mut BTreeSet<String>,
) {
    let Some(object) = provenance.and_then(Value::as_object) else {
        return;
    };

    let runtime = object_key_count(object.get("require"));
    let development = object_key_count(object.get("require-dev"));
    let optional = object_key_count(object.get("suggest"));

    summary.runtime_dependency_count = runtime;
    summary.development_dependency_count = development;
    summary.optional_dependency_count = optional;
    summary.direct_dependency_count = runtime;
    summary.has_cli_entrypoints = Some(has_non_empty_entrypoint(object.get("bin")));

    if let Some(autoload) = object.get("autoload") {
        if !autoload.is_null() {
            notes.insert("Autoload rules declared".to_owned());
        }
    }
}

fn apply_maven_analysis_fields(
    summary: &mut BundleAnalysisSummary,
    provenance: Option<&Value>,
    notes: &mut BTreeSet<String>,
) {
    let Some(object) = provenance.and_then(Value::as_object) else {
        return;
    };

    if let Some(packaging) = object.get("packaging").and_then(Value::as_str) {
        if !packaging.trim().is_empty() {
            notes.insert(format!("Packaging: {}", packaging.trim()));
        }
    }
    if let Some(licenses) = object.get("licenses").and_then(Value::as_array) {
        if !licenses.is_empty() {
            notes.insert(format!(
                "{} license declaration{}",
                licenses.len(),
                plural_suffix(licenses.len())
            ));
        }
    }

    summary.has_cli_entrypoints = Some(false);
}

fn apply_oci_analysis_fields(
    summary: &mut BundleAnalysisSummary,
    provenance: Option<&Value>,
    notes: &mut BTreeSet<String>,
) {
    let Some(object) = provenance.and_then(Value::as_object) else {
        return;
    };

    let config_size = object
        .get("config")
        .and_then(Value::as_object)
        .and_then(|config| config.get("size"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let layers = object
        .get("layers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let layer_count = layers.len();
    let layer_size_bytes = layers
        .iter()
        .filter_map(|layer| layer.get("size").and_then(Value::as_i64))
        .fold(0_i64, |total, size| total.saturating_add(size));

    summary.layer_count = Some(layer_count);
    if layer_count > 0 || config_size > 0 {
        summary.compressed_size_bytes = Some(layer_size_bytes.saturating_add(config_size));
    }
    if object.get("subject").and_then(Value::as_object).is_some() {
        notes.insert("Subject-linked manifest".to_owned());
    }
}

fn object_key_count(value: Option<&Value>) -> Option<usize> {
    value
        .and_then(Value::as_object)
        .map(|object| object.keys().count())
}

fn json_array_count(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn object_keys(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_object)
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn array_string_values(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn unique_dependency_count(groups: [Vec<String>; 5]) -> Option<usize> {
    let mut values = BTreeSet::new();

    for group in groups {
        for value in group {
            if !value.trim().is_empty() {
                values.insert(value);
            }
        }
    }

    (!values.is_empty()).then_some(values.len())
}

fn bundled_dependency_count(value: Option<&Value>) -> Option<usize> {
    match value {
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|entry| !entry.trim().is_empty())
                .count(),
        ),
        Some(Value::Object(object)) => Some(object.keys().count()),
        _ => None,
    }
}

fn install_lifecycle_script_count(value: Option<&Value>) -> Option<usize> {
    let Some(object) = value.and_then(Value::as_object) else {
        return None;
    };

    Some(
        NPM_INSTALL_LIFECYCLE_SCRIPTS
            .iter()
            .filter(|name| {
                object
                    .get(**name)
                    .and_then(Value::as_str)
                    .is_some_and(|script| !script.trim().is_empty())
            })
            .count(),
    )
}

fn has_non_empty_entrypoint(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(entry)) => !entry.trim().is_empty(),
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(Value::as_str)
            .any(|entry| !entry.trim().is_empty()),
        Some(Value::Object(entries)) => entries
            .values()
            .filter_map(Value::as_str)
            .any(|entry| !entry.trim().is_empty()),
        _ => false,
    }
}

fn has_npm_native_build_hints(object: &serde_json::Map<String, Value>) -> bool {
    if object.get("gypfile").and_then(Value::as_bool) == Some(true) {
        return true;
    }

    let Some(scripts) = object.get("scripts").and_then(Value::as_object) else {
        return false;
    };

    scripts.values().filter_map(Value::as_str).any(|script| {
        let normalized = script.to_ascii_lowercase();
        normalized.contains("node-gyp")
            || normalized.contains("node-pre-gyp")
            || normalized.contains("prebuild-install")
            || normalized.contains("cmake-js")
    })
}

fn nuget_dependency_group_metrics(value: &Value) -> (usize, usize) {
    let Some(groups) = value.as_array() else {
        return (0, 0);
    };

    let dependency_count = groups
        .iter()
        .filter_map(Value::as_object)
        .map(|group| {
            group
                .get("dependencies")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        })
        .sum();

    (groups.len(), dependency_count)
}

async fn probe_primary_artifact(
    artifact_store: &dyn ArtifactStore,
    artifact: &ArtifactRecord,
    notes: &mut BTreeSet<String>,
) -> Option<ArchiveProbeSummary> {
    let storage_key = artifact.storage_key.as_deref()?;
    let size_bytes = artifact.size_bytes.unwrap_or_default();

    if size_bytes > MAX_ARCHIVE_ANALYSIS_BYTES {
        notes.insert(format!(
            "Install size skipped for {} because the primary artifact exceeds {} MiB",
            artifact.filename,
            MAX_ARCHIVE_ANALYSIS_BYTES / (1024 * 1024)
        ));
        return None;
    }

    let bytes = match artifact_store.get_object(storage_key).await {
        Ok(Some(object)) => object.bytes,
        Ok(None) => {
            notes.insert(format!(
                "Install size unavailable because {} is missing from artifact storage",
                artifact.filename
            ));
            return None;
        }
        Err(error) => {
            notes.insert(format!(
                "Install size unavailable because {} could not be read ({error})",
                artifact.filename
            ));
            return None;
        }
    };

    match inspect_archive_bytes(&artifact.filename, bytes.as_ref()) {
        Some(summary) => Some(summary),
        None => {
            notes.insert(format!(
                "Install size unavailable because {} is not a supported archive format",
                artifact.filename
            ));
            None
        }
    }
}

fn inspect_archive_bytes(filename: &str, bytes: &[u8]) -> Option<ArchiveProbeSummary> {
    let normalized = filename.to_ascii_lowercase();

    if normalized.ends_with(".tgz")
        || normalized.ends_with(".tar.gz")
        || normalized.ends_with(".crate")
    {
        return inspect_tar_gz_bytes(bytes);
    }
    if normalized.ends_with(".zip")
        || normalized.ends_with(".whl")
        || normalized.ends_with(".nupkg")
        || normalized.ends_with(".jar")
        || normalized.ends_with(".war")
        || normalized.ends_with(".ear")
        || normalized.ends_with(".aar")
    {
        return inspect_zip_bytes(bytes);
    }
    if normalized.ends_with(".gem") {
        return inspect_gem_bytes(bytes);
    }

    None
}

fn inspect_tar_gz_bytes(bytes: &[u8]) -> Option<ArchiveProbeSummary> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(decoder);
    let mut summary = ArchiveProbeSummary::default();
    let entries = archive.entries().ok()?;

    for entry in entries {
        let entry = entry.ok()?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        summary.file_count = summary.file_count.saturating_add(1);
        let entry_size = i64::try_from(entry.size()).ok()?;
        summary.install_size_bytes = summary.install_size_bytes.saturating_add(entry_size);
    }

    Some(summary)
}

fn inspect_zip_bytes(bytes: &[u8]) -> Option<ArchiveProbeSummary> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).ok()?;
    let mut summary = ArchiveProbeSummary::default();

    for index in 0..archive.len() {
        let file = archive.by_index(index).ok()?;
        if file.is_dir() {
            continue;
        }
        summary.file_count = summary.file_count.saturating_add(1);
        let file_size = i64::try_from(file.size()).ok()?;
        summary.install_size_bytes = summary.install_size_bytes.saturating_add(file_size);
    }

    Some(summary)
}

fn inspect_gem_bytes(bytes: &[u8]) -> Option<ArchiveProbeSummary> {
    let mut archive = Archive::new(Cursor::new(bytes));
    let entries = archive.entries().ok()?;

    for entry in entries {
        let mut entry = entry.ok()?;
        let path = entry.path().ok()?.to_string_lossy().to_string();
        if !path.ends_with("data.tar.gz") {
            continue;
        }

        let mut nested_bytes = Vec::new();
        entry.read_to_end(&mut nested_bytes).ok()?;
        return inspect_tar_gz_bytes(&nested_bytes);
    }

    None
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use flate2::{write::GzEncoder, Compression};
    use serde_json::json;
    use std::io::{Cursor, Write};
    use tar::{Builder, Header};
    use zip::write::SimpleFileOptions;

    use super::{
        apply_npm_analysis_fields, inspect_gem_bytes, inspect_tar_gz_bytes, inspect_zip_bytes,
        BundleAnalysisSummary,
    };

    fn make_tgz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let tar_bytes = {
            let mut builder = Builder::new(Vec::new());
            for (name, contents) in entries {
                let mut header = Header::new_gnu();
                header.set_mode(0o644);
                header.set_size(contents.len() as u64);
                header.set_cksum();
                builder
                    .append_data(&mut header, *name, *contents)
                    .expect("tar entry should append");
            }
            builder.into_inner().expect("tar builder should finish")
        };

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&tar_bytes)
            .expect("gzip writer should accept tar bytes");
        encoder.finish().expect("gzip writer should finish")
    }

    #[test]
    fn npm_analysis_counts_runtime_scripts_and_hints() {
        let mut summary = BundleAnalysisSummary::default();
        let mut notes = std::collections::BTreeSet::new();

        apply_npm_analysis_fields(
            &mut summary,
            Some(&json!({
                "dependencies": { "react": "^19.0.0", "scheduler": "^0.26.0" },
                "devDependencies": { "vitest": "^3.0.0" },
                "peerDependencies": { "react-dom": "^19.0.0" },
                "optionalDependencies": { "fsevents": "^2.3.0" },
                "bundleDependencies": ["kleur"],
                "scripts": {
                    "prepare": "node-gyp rebuild",
                    "postinstall": "node scripts/postinstall.js"
                },
                "bin": { "demo": "./cli.js" },
                "sideEffects": false,
                "license": "MIT"
            })),
            &mut notes,
        );

        assert_eq!(summary.direct_dependency_count, Some(5));
        assert_eq!(summary.runtime_dependency_count, Some(2));
        assert_eq!(summary.development_dependency_count, Some(1));
        assert_eq!(summary.peer_dependency_count, Some(1));
        assert_eq!(summary.optional_dependency_count, Some(1));
        assert_eq!(summary.bundled_dependency_count, Some(1));
        assert_eq!(summary.install_script_count, Some(2));
        assert_eq!(summary.has_cli_entrypoints, Some(true));
        assert_eq!(summary.has_tree_shaking_hints, Some(true));
        assert_eq!(summary.has_native_code, Some(true));
        assert!(notes.contains("License: MIT"));
    }

    #[test]
    fn tar_gz_probe_reports_install_size_and_file_count() {
        let archive = make_tgz(&[
            ("package/index.js", b"console.log('hi');"),
            ("package/package.json", br#"{"name":"demo"}"#),
        ]);

        let summary = inspect_tar_gz_bytes(&archive).expect("tgz should be parsed");
        assert_eq!(summary.file_count, 2);
        assert_eq!(
            summary.install_size_bytes,
            i64::try_from("console.log('hi');".len() + br#"{"name":"demo"}"#.len())
                .expect("sizes should fit"),
        );
    }

    #[test]
    fn zip_probe_reports_install_size_and_file_count() {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            writer
                .start_file("dist/index.js", SimpleFileOptions::default())
                .expect("zip file should start");
            writer
                .write_all(b"export const value = 1;")
                .expect("zip entry should write");
            writer
                .start_file("README.md", SimpleFileOptions::default())
                .expect("zip file should start");
            writer.write_all(b"# demo").expect("zip entry should write");
            writer.finish().expect("zip writer should finish");
        }

        let summary = inspect_zip_bytes(cursor.get_ref()).expect("zip should be parsed");
        assert_eq!(summary.file_count, 2);
        assert_eq!(
            summary.install_size_bytes,
            i64::try_from("export const value = 1;".len() + "# demo".len())
                .expect("sizes should fit"),
        );
    }

    #[test]
    fn gem_probe_reads_nested_data_archive() {
        let data_archive = make_tgz(&[("lib/demo.rb", b"puts 'demo'")]);
        let gem_bytes = {
            let mut builder = Builder::new(Vec::new());
            let mut header = Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(data_archive.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, "data.tar.gz", data_archive.as_slice())
                .expect("gem data archive should append");
            builder.into_inner().expect("gem tar should finish")
        };

        let summary = inspect_gem_bytes(&gem_bytes).expect("gem should be parsed");
        assert_eq!(summary.file_count, 1);
        assert_eq!(
            summary.install_size_bytes,
            i64::try_from("puts 'demo'".len()).expect("sizes should fit"),
        );
    }
}
