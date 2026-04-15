//! Parse a NuGet package push request.
//!
//! NuGet `push` sends a `PUT` request with `Content-Type: multipart/form-data`.
//! The first (and usually only) file part is the raw `.nupkg` bytes. This
//! module extracts the `.nupkg`, opens the ZIP, parses the `.nuspec`, and
//! computes integrity digests.

use bytes::Bytes;
use sha2::{Digest, Sha256, Sha512};

use publaryn_core::error::{Error, Result};

use crate::nuspec::{self, NuspecMetadata};

/// Maximum allowed `.nupkg` size (256 MiB).
const MAX_NUPKG_SIZE: usize = 256 * 1024 * 1024;

/// Result of parsing a NuGet push request.
#[derive(Debug, Clone)]
pub struct ParsedNuGetPublish {
    /// Parsed nuspec metadata.
    pub metadata: NuspecMetadata,
    /// Raw `.nuspec` XML bytes (stored separately for efficient serving).
    pub nuspec_bytes: Vec<u8>,
    /// Raw `.nupkg` file bytes.
    pub nupkg_bytes: Bytes,
    /// SHA-256 hex digest of the `.nupkg`.
    pub sha256: String,
    /// SHA-512 hex digest of the `.nupkg`.
    pub sha512: String,
    /// Size of the `.nupkg` in bytes.
    pub size_bytes: i64,
}

/// Parse a raw `.nupkg` payload.
///
/// Validates the archive, extracts the `.nuspec`, and computes hashes.
pub fn parse_nupkg(nupkg_bytes: Bytes) -> Result<ParsedNuGetPublish> {
    if nupkg_bytes.is_empty() {
        return Err(Error::Validation(
            "Empty .nupkg payload".into(),
        ));
    }

    if nupkg_bytes.len() > MAX_NUPKG_SIZE {
        return Err(Error::Validation(format!(
            ".nupkg exceeds maximum allowed size of {} MiB",
            MAX_NUPKG_SIZE / (1024 * 1024)
        )));
    }

    let (metadata, nuspec_bytes) = nuspec::parse_nuspec_from_nupkg(&nupkg_bytes)?;

    let sha256 = hex::encode(Sha256::digest(&nupkg_bytes));
    let sha512 = hex::encode(Sha512::digest(&nupkg_bytes));
    let size_bytes = nupkg_bytes.len() as i64;

    Ok(ParsedNuGetPublish {
        metadata,
        nuspec_bytes,
        nupkg_bytes,
        sha256,
        sha512,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal valid .nupkg (ZIP with a .nuspec) for testing.
    fn make_test_nupkg(id: &str, version: &str) -> Vec<u8> {
        let nuspec = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd">
  <metadata>
    <id>{id}</id>
    <version>{version}</version>
    <description>Test package</description>
    <authors>Test</authors>
  </metadata>
</package>"#
        );

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file(format!("{id}.nuspec"), options).unwrap();
            zip.write_all(nuspec.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn parse_valid_nupkg() {
        let nupkg = make_test_nupkg("TestPkg", "1.0.0");
        let result = parse_nupkg(Bytes::from(nupkg));
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.metadata.id, "TestPkg");
        assert_eq!(parsed.metadata.version, "1.0.0");
        assert!(!parsed.sha256.is_empty());
        assert!(!parsed.sha512.is_empty());
        assert!(parsed.size_bytes > 0);
    }

    #[test]
    fn reject_empty_payload() {
        assert!(parse_nupkg(Bytes::new()).is_err());
    }

    #[test]
    fn reject_invalid_zip() {
        assert!(parse_nupkg(Bytes::from_static(b"not a zip")).is_err());
    }
}
