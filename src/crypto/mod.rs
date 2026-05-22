use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 32;
const FORMAT_V2_PREFIX: &str = "v2:";

const _: () = assert!(KEY_LEN == 32, "KEY_LEN must be 32 for AES-256");
const _: () = assert!(NONCE_LEN == 12, "NONCE_LEN must be 12 for AES-GCM");
const _: () = assert!(SALT_LEN == 32, "SALT_LEN must be 32 for Argon2id");

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("invalid data format")]
    InvalidFormat,
    #[error("key derivation failed")]
    KeyDerivationFailed,
}

pub struct EncryptedData {
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

fn derive_key_argon2id(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    let params = Params::new(19_456, 2, 1, Some(KEY_LEN))
        .map_err(|_| CryptoError::KeyDerivationFailed)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| CryptoError::KeyDerivationFailed)?;
    Ok(key)
}

fn derive_key_legacy(password: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    // Backward compatibility for pre-v2 ciphertexts.
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let mut hmac = <HmacSha256 as Mac>::new_from_slice(salt).expect("fixed-size salt is valid");
    hmac.update(password.as_bytes());
    let result = hmac.finalize();
    let bytes = result.into_bytes();
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes[..KEY_LEN]);
    key
}

pub fn encrypt(plaintext: &str, password: &str) -> Result<EncryptedData, CryptoError> {
    let salt: [u8; SALT_LEN] = rand::random();
    let key = derive_key_argon2id(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

    let nonce_bytes: [u8; NONCE_LEN] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

    Ok(EncryptedData {
        salt: salt.to_vec(),
        nonce: nonce_bytes.to_vec(),
        ciphertext,
    })
}

pub fn decrypt(encrypted: &EncryptedData, password: &str) -> Result<String, CryptoError> {
    if encrypted.salt.len() != SALT_LEN
        || encrypted.nonce.len() != NONCE_LEN
        || encrypted.ciphertext.is_empty()
    {
        return Err(CryptoError::InvalidFormat);
    }

    let key = derive_key_argon2id(password, &encrypted.salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

    let nonce = Nonce::from_slice(&encrypted.nonce);

    let plaintext = cipher
        .decrypt(nonce, encrypted.ciphertext.as_ref())
        .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

    String::from_utf8(plaintext)
        .map_err(|_| CryptoError::DecryptionFailed("invalid utf-8".to_string()))
}

pub fn encrypt_to_string(plaintext: &str, password: &str) -> Result<String, CryptoError> {
    let encrypted = encrypt(plaintext, password)?;
    let mut result = Vec::with_capacity(SALT_LEN + NONCE_LEN + encrypted.ciphertext.len());
    result.extend_from_slice(&encrypted.salt);
    result.extend_from_slice(&encrypted.nonce);
    result.extend_from_slice(&encrypted.ciphertext);
    Ok(format!("{}{}", FORMAT_V2_PREFIX, hex::encode(result)))
}

pub fn decrypt_from_string(encrypted_str: &str, password: &str) -> Result<String, CryptoError> {
    if let Some(v2_hex) = encrypted_str.strip_prefix(FORMAT_V2_PREFIX) {
        let bytes = hex::decode(v2_hex).map_err(|_| CryptoError::InvalidFormat)?;
        return decrypt_from_bytes_argon2id(&bytes, password);
    }

    // Legacy format (pre-v2): hex(salt || nonce || ciphertext) with HMAC-based key derivation.
    let bytes = hex::decode(encrypted_str).map_err(|_| CryptoError::InvalidFormat)?;
    decrypt_from_bytes_legacy(&bytes, password)
}

fn decrypt_from_bytes_argon2id(bytes: &[u8], password: &str) -> Result<String, CryptoError> {
    if bytes.len() < SALT_LEN + NONCE_LEN {
        return Err(CryptoError::InvalidFormat);
    }

    let salt = bytes[..SALT_LEN].to_vec();
    let nonce = bytes[SALT_LEN..SALT_LEN + NONCE_LEN].to_vec();
    let ciphertext = bytes[SALT_LEN + NONCE_LEN..].to_vec();

    let encrypted = EncryptedData {
        salt,
        nonce,
        ciphertext,
    };

    decrypt(&encrypted, password)
}

fn decrypt_from_bytes_legacy(bytes: &[u8], password: &str) -> Result<String, CryptoError> {
    if bytes.len() < SALT_LEN + NONCE_LEN {
        return Err(CryptoError::InvalidFormat);
    }

    let salt = bytes[..SALT_LEN].to_vec();
    let nonce = bytes[SALT_LEN..SALT_LEN + NONCE_LEN].to_vec();
    let ciphertext = bytes[SALT_LEN + NONCE_LEN..].to_vec();

    let encrypted = EncryptedData {
        salt,
        nonce,
        ciphertext,
    };

    decrypt_legacy(&encrypted, password)
}

fn decrypt_legacy(encrypted: &EncryptedData, password: &str) -> Result<String, CryptoError> {
    if encrypted.salt.len() != SALT_LEN
        || encrypted.nonce.len() != NONCE_LEN
        || encrypted.ciphertext.is_empty()
    {
        return Err(CryptoError::InvalidFormat);
    }

    let key = derive_key_legacy(password, &encrypted.salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

    let nonce = Nonce::from_slice(&encrypted.nonce);
    let plaintext = cipher
        .decrypt(nonce, encrypted.ciphertext.as_ref())
        .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

    String::from_utf8(plaintext)
        .map_err(|_| CryptoError::DecryptionFailed("invalid utf-8".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let plaintext = "test-api-key-12345";
        let password = "my-secret-password";

        let encrypted = encrypt(plaintext, password).unwrap();
        let decrypted = decrypt(&encrypted, password).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_string_format() {
        let plaintext = "sk-ant-api03-test-key";
        let password = "another-password";

        let encrypted = encrypt_to_string(plaintext, password).unwrap();
        assert!(encrypted.starts_with(FORMAT_V2_PREFIX));
        let decrypted = decrypt_from_string(&encrypted, password).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_legacy_format_still_decrypts() {
        let plaintext = "legacy-secret";
        let password = "legacy-password";

        let salt: [u8; SALT_LEN] = rand::random();
        let key = derive_key_legacy(password, &salt);
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce_bytes: [u8; NONCE_LEN] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).unwrap();

        let mut legacy_bytes = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
        legacy_bytes.extend_from_slice(&salt);
        legacy_bytes.extend_from_slice(&nonce_bytes);
        legacy_bytes.extend_from_slice(&ciphertext);
        let legacy_hex = hex::encode(legacy_bytes);

        let decrypted = decrypt_from_string(&legacy_hex, password).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_password() {
        let plaintext = "secret-key";
        let password = "correct-password";
        let wrong_password = "wrong-password";

        let encrypted = encrypt(plaintext, password).unwrap();
        let result = decrypt(&encrypted, wrong_password);
        assert!(result.is_err());
    }
}
