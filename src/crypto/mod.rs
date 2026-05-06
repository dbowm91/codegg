use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 32;

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

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    let input_key = password.as_bytes();

    let mut hmac =
        <HmacSha256 as Mac>::new_from_slice(salt).map_err(|_| CryptoError::KeyDerivationFailed)?;
    hmac.update(input_key);
    let result = hmac.finalize();

    let bytes = result.into_bytes();
    if bytes.len() < KEY_LEN {
        return Err(CryptoError::KeyDerivationFailed);
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes[..KEY_LEN]);
    Ok(key)
}

pub fn encrypt(plaintext: &str, password: &str) -> Result<EncryptedData, CryptoError> {
    let salt: [u8; SALT_LEN] = rand::random();
    let key = derive_key(password, &salt)?;
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

    let key = derive_key(password, &encrypted.salt)?;
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
    Ok(hex::encode(result))
}

pub fn decrypt_from_string(encrypted_str: &str, password: &str) -> Result<String, CryptoError> {
    let bytes = hex::decode(encrypted_str).map_err(|_| CryptoError::InvalidFormat)?;

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
        let decrypted = decrypt_from_string(&encrypted, password).unwrap();
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
