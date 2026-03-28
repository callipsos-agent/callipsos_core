use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

use crate::error::AppError;

// ── Encryption key from environment ─────────────────────────
// The server operator sets ENCRYPTION_KEY as a 64-char hex string
// (32 bytes = 256 bits) in the environment. This key never touches
// the database. Without it, ciphertext in the llm_api_key column
// is unrecoverable.
//
// Only the Telegram bot binary needs this. The core API server
// does not handle LLM API keys and should not call this function.

#[derive(Debug, thiserror::Error)]
pub enum EncryptionKeyError {
    #[error("ENCRYPTION_KEY env var not set")]
    Missing,
    #[error("ENCRYPTION_KEY is not valid hex: {0}")]
    InvalidHex(String),
    #[error("ENCRYPTION_KEY must be exactly 32 bytes (64 hex chars), got {0} bytes")]
    WrongLength(usize),
}

/// Loads the 32-byte AES-256 key from the ENCRYPTION_KEY env var.
/// Returns an error if missing or malformed. The caller decides
/// whether that's fatal (bot binary) or ignorable (API server).
pub fn load_encryption_key() -> Result<Key<Aes256Gcm>, EncryptionKeyError> {
    let hex_key = std::env::var("ENCRYPTION_KEY")
        .map_err(|_| EncryptionKeyError::Missing)?;

    let bytes = hex::decode(&hex_key)
        .map_err(|e| EncryptionKeyError::InvalidHex(e.to_string()))?;

    if bytes.len() != 32 {
        return Err(EncryptionKeyError::WrongLength(bytes.len()));
    }

    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&bytes);
    Ok(Key::<Aes256Gcm>::from(key_bytes))
}

// ── Encrypt / Decrypt ───────────────────────────────────────
// Format: base64(nonce || ciphertext)
// The 12-byte nonce is generated randomly per encryption and
// prepended to the ciphertext. On decrypt, the first 12 bytes
// are split off as the nonce.

/// Encrypts a plaintext string and returns a base64-encoded string
/// containing the nonce and ciphertext.
pub fn encrypt(key: &Key<Aes256Gcm>, plaintext: &str) -> Result<String, AppError> {
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| AppError::Internal(format!("Encryption failed: {e}")))?;

    // Prepend nonce to ciphertext, then base64 the whole thing
    let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypts a base64-encoded string (nonce || ciphertext) back to plaintext.
pub fn decrypt(key: &Key<Aes256Gcm>, encoded: &str) -> Result<String, AppError> {
    let combined = BASE64
        .decode(encoded)
        .map_err(|e| AppError::Internal(format!("Base64 decode failed: {e}")))?;

    if combined.len() < 12 {
        return Err(AppError::Internal(
            "Encrypted data too short (missing nonce)".into(),
        ));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(key);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Internal("Decryption failed (wrong key or corrupted data)".into()))?;

    String::from_utf8(plaintext)
        .map_err(|e| AppError::Internal(format!("Decrypted data is not valid UTF-8: {e}")))
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::KeyInit;

    fn test_key() -> Key<Aes256Gcm> {
        // Deterministic test key (not for production)
        let bytes = [0x42u8; 32];
        Key::<Aes256Gcm>::from(bytes)
    }

    #[test]
    fn round_trip() {
        let key = test_key();
        let original = "sk-ant-api03-test-key-1234567890";

        let encrypted = encrypt(&key, original).unwrap();
        assert_ne!(encrypted, original);

        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn different_nonces_produce_different_ciphertext() {
        let key = test_key();
        let original = "sk-ant-api03-same-plaintext";

        let encrypted1 = encrypt(&key, original).unwrap();
        let encrypted2 = encrypt(&key, original).unwrap();
        assert_ne!(encrypted1, encrypted2);

        // Both decrypt to the same plaintext
        assert_eq!(decrypt(&key, &encrypted1).unwrap(), original);
        assert_eq!(decrypt(&key, &encrypted2).unwrap(), original);
    }

    #[test]
    fn wrong_key_fails_decrypt() {
        let key1 = test_key();
        let key2 = {
            let bytes = [0x99u8; 32];
            Key::<Aes256Gcm>::from(bytes)
        };

        let encrypted = encrypt(&key1, "secret").unwrap();
        let result = decrypt(&key2, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn empty_string_round_trip() {
        let key = test_key();
        let encrypted = encrypt(&key, "").unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn corrupted_ciphertext_fails() {
        let key = test_key();
        let result = decrypt(&key, "not-valid-base64-!@#");
        assert!(result.is_err());
    }

    #[test]
    fn truncated_data_fails() {
        let key = test_key();
        // Base64 of less than 12 bytes
        let short = BASE64.encode(&[0u8; 8]);
        let result = decrypt(&key, &short);
        assert!(result.is_err());
    }
}