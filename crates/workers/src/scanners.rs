//! Built-in artifact scanners for the background scanning pipeline.
//!
//! Scanners implement the [`ArtifactScanner`] trait and return a list of
//! [`ScanFinding`]s.  The [`ScanArtifactHandler`] orchestrates scanning by
//! running all registered scanners against an artifact and persisting results.

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::handler::JobHandler;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single finding produced by a scanner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanFinding {
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub description: Option<String>,
}

/// Payload for the `ScanArtifact` background job.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScanArtifactPayload {
    pub release_id: Uuid,
    pub artifact_id: Uuid,
    pub storage_key: String,
    pub filename: String,
    pub ecosystem: String,
}

// ── Scanner trait ─────────────────────────────────────────────────────────────

/// Trait for pluggable artifact scanners.
#[async_trait]
pub trait ArtifactScanner: Send + Sync {
    async fn scan(
        &self,
        artifact_id: Uuid,
        filename: &str,
        bytes: &[u8],
    ) -> Result<Vec<ScanFinding>, String>;
}

// ── Storage abstraction ──────────────────────────────────────────────────────

/// Read-only access to the artifact object store.
///
/// Implemented in the API crate and passed into [`ScanArtifactHandler`] at
/// startup, so that the workers crate stays decoupled from S3 specifics.
#[async_trait]
pub trait ArtifactStoreReader: Send + Sync {
    async fn get_object_bytes(&self, storage_key: &str) -> Result<Option<Bytes>, String>;
}

// ── Policy scanner ───────────────────────────────────────────────────────────

/// Checks artifact size limits and detects suspicious archive patterns.
pub struct PolicyScanner {
    pub max_artifact_bytes: u64,
}

#[async_trait]
impl ArtifactScanner for PolicyScanner {
    async fn scan(
        &self,
        _artifact_id: Uuid,
        filename: &str,
        bytes: &[u8],
    ) -> Result<Vec<ScanFinding>, String> {
        let mut findings = Vec::new();

        if bytes.len() as u64 > self.max_artifact_bytes {
            findings.push(ScanFinding {
                kind: "policy_violation".into(),
                severity: "high".into(),
                title: format!(
                    "Artifact exceeds size limit ({} bytes > {} bytes)",
                    bytes.len(),
                    self.max_artifact_bytes
                ),
                description: Some(format!("File: {filename}")),
            });
        }

        // Heuristic: very small compressed archives *may* be archive bombs.
        let is_compressed = filename.ends_with(".tar.gz")
            || filename.ends_with(".tgz")
            || filename.ends_with(".zip");
        if is_compressed && bytes.len() < 1024 {
            findings.push(ScanFinding {
                kind: "archive_bomb".into(),
                severity: "medium".into(),
                title: "Suspiciously small compressed archive".into(),
                description: Some(format!(
                    "Archive {filename} is only {} bytes — may indicate an archive bomb",
                    bytes.len()
                )),
            });
        }

        Ok(findings)
    }
}

// ── Secrets scanner ──────────────────────────────────────────────────────────

/// Detects common credential patterns via regex.
pub struct SecretsScanner {
    patterns: Vec<(&'static str, regex::Regex)>,
}

impl SecretsScanner {
    pub fn new() -> Self {
        let patterns = vec![
            (
                "AWS Access Key",
                regex::Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
            ),
            (
                "AWS Secret Key",
                regex::Regex::new(
                    r#"(?i)aws.{0,20}secret.{0,20}['"][0-9a-zA-Z/+]{40}['"]"#,
                )
                .unwrap(),
            ),
            (
                "GitHub Token",
                regex::Regex::new(r"gh[ps]_[A-Za-z0-9_]{36,}").unwrap(),
            ),
            (
                "Private Key",
                regex::Regex::new(
                    r"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
                )
                .unwrap(),
            ),
            (
                "Generic API Key",
                regex::Regex::new(
                    r#"(?i)(api[_-]?key|apikey|secret[_-]?key)\s*[=:]\s*['"][A-Za-z0-9/+=]{20,}['"]"#,
                )
                .unwrap(),
            ),
            (
                "Database Connection String",
                regex::Regex::new(
                    r#"(?i)(postgres|mysql|mongodb)://[^\s'"]{10,}"#,
                )
                .unwrap(),
            ),
        ];
        Self { patterns }
    }
}

impl Default for SecretsScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ArtifactScanner for SecretsScanner {
    async fn scan(
        &self,
        _artifact_id: Uuid,
        filename: &str,
        bytes: &[u8],
    ) -> Result<Vec<ScanFinding>, String> {
        // Skip binary files and very large files for performance.
        if bytes.len() > 10 * 1024 * 1024 {
            return Ok(vec![]);
        }

        let content = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return Ok(vec![]),
        };

        let mut findings = Vec::new();

        for (name, pattern) in &self.patterns {
            if pattern.is_match(content) {
                findings.push(ScanFinding {
                    kind: "secrets_exposed".into(),
                    severity: "critical".into(),
                    title: format!("{name} detected in {filename}"),
                    description: Some(format!(
                        "A pattern matching '{name}' was found. \
                         This may indicate exposed credentials."
                    )),
                });
            }
        }

        Ok(findings)
    }
}

// ── Scan artifact handler ────────────────────────────────────────────────────

/// Background job handler that runs all registered scanners against an artifact,
/// persists findings, and transitions the release status.
pub struct ScanArtifactHandler {
    pub db: PgPool,
    pub artifact_store: std::sync::Arc<dyn ArtifactStoreReader>,
    pub scanners: Vec<Box<dyn ArtifactScanner>>,
}

#[async_trait]
impl JobHandler for ScanArtifactHandler {
    async fn handle(&self, payload: serde_json::Value) -> Result<(), String> {
        let payload: ScanArtifactPayload =
            serde_json::from_value(payload).map_err(|e| format!("Invalid scan payload: {e}"))?;

        tracing::info!(
            artifact_id = %payload.artifact_id,
            release_id = %payload.release_id,
            filename = %payload.filename,
            "Starting artifact scan"
        );

        // Fetch artifact from object storage.
        let bytes = self
            .artifact_store
            .get_object_bytes(&payload.storage_key)
            .await?
            .ok_or_else(|| format!("Artifact not found in storage: {}", payload.storage_key))?;

        // Run every scanner.
        let mut all_findings = Vec::new();
        for scanner in &self.scanners {
            match scanner
                .scan(payload.artifact_id, &payload.filename, &bytes)
                .await
            {
                Ok(findings) => all_findings.extend(findings),
                Err(e) => {
                    tracing::error!(
                        artifact_id = %payload.artifact_id,
                        error = %e,
                        "Scanner failed"
                    );
                }
            }
        }

        // Persist findings.
        for finding in &all_findings {
            let finding_id = Uuid::new_v4();
            let result = sqlx::query(
                "INSERT INTO security_findings \
                 (id, release_id, artifact_id, kind, severity, title, description) \
                 VALUES ($1, $2, $3, $4::finding_kind, $5::security_severity, $6, $7)",
            )
            .bind(finding_id)
            .bind(payload.release_id)
            .bind(payload.artifact_id)
            .bind(&finding.kind)
            .bind(&finding.severity)
            .bind(&finding.title)
            .bind(&finding.description)
            .execute(&self.db)
            .await;

            if let Err(e) = result {
                tracing::error!(
                    finding_id = %finding_id,
                    error = %e,
                    "Failed to persist security finding"
                );
            }
        }

        // Transition the release status based on findings.
        let has_blocking = all_findings
            .iter()
            .any(|f| f.severity == "critical" || f.kind == "malware");

        let new_status = if has_blocking {
            "quarantine"
        } else {
            "published"
        };

        sqlx::query(
            "UPDATE releases SET status = $2::release_status, updated_at = NOW() \
             WHERE id = $1 AND status = 'scanning'",
        )
        .bind(payload.release_id)
        .bind(new_status)
        .execute(&self.db)
        .await
        .map_err(|e| format!("Failed to transition release status: {e}"))?;

        tracing::info!(
            artifact_id = %payload.artifact_id,
            findings = all_findings.len(),
            blocked = has_blocking,
            new_status,
            "Artifact scan completed"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn policy_scanner_detects_oversized_artifact() {
        let scanner = PolicyScanner {
            max_artifact_bytes: 100,
        };
        let bytes = vec![0u8; 200];
        let findings = scanner
            .scan(Uuid::new_v4(), "package.tar.gz", &bytes)
            .await
            .unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "policy_violation");
    }

    #[tokio::test]
    async fn policy_scanner_flags_tiny_archive() {
        let scanner = PolicyScanner {
            max_artifact_bytes: 1_000_000,
        };
        let bytes = vec![0u8; 100];
        let findings = scanner
            .scan(Uuid::new_v4(), "bomb.tar.gz", &bytes)
            .await
            .unwrap();
        assert!(findings.iter().any(|f| f.kind == "archive_bomb"));
    }

    #[tokio::test]
    async fn secrets_scanner_detects_aws_key() {
        let scanner = SecretsScanner::new();
        let content = b"config_key = AKIAIOSFODNN7EXAMPLE";
        let findings = scanner
            .scan(Uuid::new_v4(), "config.py", content)
            .await
            .unwrap();
        assert!(!findings.is_empty());
        assert_eq!(findings[0].kind, "secrets_exposed");
    }

    #[tokio::test]
    async fn secrets_scanner_detects_private_key() {
        let scanner = SecretsScanner::new();
        let content = b"-----BEGIN RSA PRIVATE KEY-----\nMIIEow...";
        let findings = scanner
            .scan(Uuid::new_v4(), "key.pem", content)
            .await
            .unwrap();
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn secrets_scanner_skips_binary() {
        let scanner = SecretsScanner::new();
        let content = vec![0u8, 0xFF, 0xFE, 0x00, 0xAB];
        let findings = scanner
            .scan(Uuid::new_v4(), "binary.so", &content)
            .await
            .unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn secrets_scanner_clean_file() {
        let scanner = SecretsScanner::new();
        let content = b"fn main() { println!(\"hello\"); }";
        let findings = scanner
            .scan(Uuid::new_v4(), "main.rs", content)
            .await
            .unwrap();
        assert!(findings.is_empty());
    }
}
