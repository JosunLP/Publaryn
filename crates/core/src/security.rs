use sha2::{Digest, Sha256};

/// Compute SHA-256 hex digest of a byte slice.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Verify a SHA-256 digest against expected hex string.
pub fn verify_sha256(data: &[u8], expected_hex: &str) -> bool {
    sha256_hex(data).eq_ignore_ascii_case(expected_hex)
}

/// Generate a cryptographically-random token string of `n` bytes,
/// encoded as hex. The result length will be `n * 2` characters.
pub fn generate_random_token(n: usize) -> String {
    let mut buf = vec![0u8; n];
    rand::fill(&mut buf[..]);
    hex::encode(buf)
}

/// Hash a raw token value for storage (SHA-256, hex).
pub fn hash_token(raw: &str) -> String {
    sha256_hex(raw.as_bytes())
}
