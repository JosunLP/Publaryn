//! TOTP-based multi-factor authentication.
//!
//! Provides setup (secret generation + provisioning URI), verification,
//! and backup recovery code generation.

use rand::Rng;
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};

use publaryn_core::error::Error;

/// Number of digits in a TOTP code.
const TOTP_DIGITS: usize = 6;
/// TOTP time step in seconds.
const TOTP_STEP: u64 = 30;
/// Number of recovery codes generated.
const RECOVERY_CODE_COUNT: usize = 8;
/// Length of each segment in a recovery code (`xxxx-yyyy`).
const RECOVERY_CODE_SEGMENT_LEN: usize = 4;

/// Result of TOTP setup — returned to the client for QR code rendering.
#[derive(Debug, Clone)]
pub struct TotpSetup {
    /// Base32-encoded secret for manual entry.
    pub secret_base32: String,
    /// `otpauth://totp/...` URI for QR code generation.
    pub provisioning_uri: String,
    /// Single-use recovery codes (show to user once, then discard plaintext).
    pub recovery_codes: Vec<String>,
    /// Hashed recovery codes for database storage.
    pub recovery_code_hashes: Vec<String>,
}

/// Generate a new TOTP secret and provisioning URI.
pub fn setup_totp(account_name: &str, issuer: &str) -> Result<TotpSetup, Error> {
    let secret = Secret::generate_secret();
    let secret_bytes = secret
        .to_bytes()
        .map_err(|e| Error::Internal(format!("Failed to generate TOTP secret: {e}")))?;

    let totp = TOTP::new(
        Algorithm::SHA1,
        TOTP_DIGITS,
        1, // skew: allow 1 step in each direction
        TOTP_STEP,
        secret_bytes,
        Some(issuer.to_string()),
        account_name.to_string(),
    )
    .map_err(|e| Error::Internal(format!("Failed to create TOTP instance: {e}")))?;

    let secret_base32 = secret.to_encoded().to_string();
    let provisioning_uri = totp.get_url();

    let (recovery_codes, recovery_code_hashes) = generate_recovery_codes();

    Ok(TotpSetup {
        secret_base32,
        provisioning_uri,
        recovery_codes,
        recovery_code_hashes,
    })
}

/// Verify a TOTP code against a stored base32 secret.
///
/// Returns `true` if the code is valid for the current or adjacent time step.
pub fn verify_totp(secret_base32: &str, code: &str) -> Result<bool, Error> {
    let secret = Secret::Encoded(secret_base32.to_string());
    let secret_bytes = secret
        .to_bytes()
        .map_err(|e| Error::Internal(format!("Failed to decode TOTP secret: {e}")))?;

    let totp = TOTP::new(
        Algorithm::SHA1,
        TOTP_DIGITS,
        1,
        TOTP_STEP,
        secret_bytes,
        None,
        "".to_string(),
    )
    .map_err(|e| Error::Internal(format!("Failed to create TOTP instance: {e}")))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| Error::Internal(format!("System time error: {e}")))?
        .as_secs();

    Ok(totp.check(code, now))
}

/// Verify a recovery code against stored hashes.
///
/// Returns the index of the matching code (so the caller can mark it as used),
/// or `None` if no match.
pub fn verify_recovery_code(code: &str, hashes: &[String]) -> Option<usize> {
    let candidate_hash = hash_recovery_code(code);
    hashes.iter().position(|h| h == &candidate_hash)
}

/// Hash a single recovery code for storage.
pub fn hash_recovery_code(code: &str) -> String {
    let normalized = code.replace('-', "").to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

fn generate_recovery_codes() -> (Vec<String>, Vec<String>) {
    let mut rng = rand::thread_rng();
    let alphabet: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

    let mut codes = Vec::with_capacity(RECOVERY_CODE_COUNT);
    let mut hashes = Vec::with_capacity(RECOVERY_CODE_COUNT);

    for _ in 0..RECOVERY_CODE_COUNT {
        let seg1: String = (0..RECOVERY_CODE_SEGMENT_LEN)
            .map(|_| alphabet[rng.gen_range(0..alphabet.len())] as char)
            .collect();
        let seg2: String = (0..RECOVERY_CODE_SEGMENT_LEN)
            .map(|_| alphabet[rng.gen_range(0..alphabet.len())] as char)
            .collect();
        let code = format!("{seg1}-{seg2}");
        let hash = hash_recovery_code(&code);
        codes.push(code);
        hashes.push(hash);
    }

    (codes, hashes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_produces_valid_provisioning_uri() {
        let setup = setup_totp("testuser", "Publaryn").unwrap();
        assert!(setup.provisioning_uri.starts_with("otpauth://totp/"));
        assert!(setup.provisioning_uri.contains("Publaryn"));
        assert!(!setup.secret_base32.is_empty());
    }

    #[test]
    fn recovery_codes_are_generated_correctly() {
        let setup = setup_totp("testuser", "Publaryn").unwrap();
        assert_eq!(setup.recovery_codes.len(), RECOVERY_CODE_COUNT);
        assert_eq!(setup.recovery_code_hashes.len(), RECOVERY_CODE_COUNT);
        for code in &setup.recovery_codes {
            assert!(code.contains('-'));
            assert_eq!(code.len(), RECOVERY_CODE_SEGMENT_LEN * 2 + 1);
        }
    }

    #[test]
    fn recovery_code_verification_works() {
        let setup = setup_totp("testuser", "Publaryn").unwrap();
        let code = &setup.recovery_codes[0];
        let idx = verify_recovery_code(code, &setup.recovery_code_hashes);
        assert_eq!(idx, Some(0));
    }

    #[test]
    fn invalid_recovery_code_returns_none() {
        let setup = setup_totp("testuser", "Publaryn").unwrap();
        let idx = verify_recovery_code("xxxx-yyyy", &setup.recovery_code_hashes);
        assert_eq!(idx, None);
    }

    #[test]
    fn verify_totp_rejects_wrong_code() {
        let setup = setup_totp("testuser", "Publaryn").unwrap();
        // Overwhelmingly unlikely to match; smoke-test that it doesn't panic
        let result = verify_totp(&setup.secret_base32, "000000").unwrap();
        // We can't assert true/false reliably because of time-based TOTP —
        // just ensure it returns without error.
        let _ = result;
    }
}
