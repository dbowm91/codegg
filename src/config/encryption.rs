use crate::config::schema::Config;
use crate::error::{AppError, ConfigError};

const CRYPTO_V2_PREFIX: &str = "v2:";

pub fn get_master_key() -> Option<String> {
    std::env::var("CODEGG_MASTER_KEY")
        .ok()
        .or_else(|| std::env::var("CODEGG_ENCRYPTION_KEY").ok())
        .or_else(|| std::env::var("OPENCODE_ENCRYPTION_KEY").ok())
}

pub fn decrypt_provider_keys(config: &mut Config) -> Result<(), AppError> {
    let Some(master_key) = get_master_key() else {
        return Ok(());
    };

    if let Some(providers) = config.provider.as_mut() {
        let mut failures = Vec::new();
        for (name, provider) in providers.iter_mut() {
            if provider.encrypted == Some(true) {
                if let Some(ref encrypted_key) = provider.encrypted_api_key {
                    match crate::crypto::decrypt_from_string(encrypted_key, &master_key) {
                        Ok(decrypted) => {
                            provider.api_key = Some(decrypted);
                        }
                        Err(e) => {
                            failures.push(format!("{name}: {e}"));
                        }
                    }
                }
            }
        }
        if !failures.is_empty() {
            return Err(AppError::Config(ConfigError::Invalid(format!(
                "failed to decrypt {} provider API key(s): {}",
                failures.len(),
                failures.join("; ")
            ))));
        }
    }
    Ok(())
}

pub fn encrypt_provider_keys(config: &mut Config) -> Result<(), AppError> {
    let Some(master_key) = get_master_key() else {
        return Ok(());
    };

    if let Some(providers) = config.provider.as_mut() {
        let mut encryptions: Vec<(String, String)> = Vec::new();
        let mut migrations: Vec<(String, String)> = Vec::new();
        let mut failures = Vec::new();

        for (name, provider) in providers.iter_mut() {
            if let Some(ref api_key) = provider.api_key {
                if provider.encrypted != Some(true) {
                    match crate::crypto::encrypt_to_string(api_key, &master_key) {
                        Ok(encrypted) => {
                            encryptions.push((name.clone(), encrypted));
                        }
                        Err(e) => {
                            failures.push(format!("{name}: {e}"));
                        }
                    }
                }
            }

            if provider.encrypted == Some(true) {
                if let Some(ref encrypted_key) = provider.encrypted_api_key {
                    if !encrypted_key.starts_with(CRYPTO_V2_PREFIX) {
                        match crate::crypto::decrypt_from_string(encrypted_key, &master_key) {
                            Ok(decrypted) => match crate::crypto::encrypt_to_string(&decrypted, &master_key) {
                                Ok(migrated) => {
                                    migrations.push((name.clone(), migrated));
                                }
                                Err(e) => failures.push(format!("{name}: {e}")),
                            },
                            Err(e) => failures.push(format!("{name}: {e}")),
                        }
                    }
                }
            }
        }

        if !failures.is_empty() {
            return Err(AppError::Config(ConfigError::Invalid(format!(
                "failed to encrypt provider API key(s): {}",
                failures.join("; ")
            ))));
        }

        for (name, encrypted) in encryptions {
            if let Some(provider) = providers.get_mut(&name) {
                provider.encrypted_api_key = Some(encrypted);
                provider.encrypted = Some(true);
                provider.api_key = None;
            }
        }

        for (name, migrated) in migrations {
            if let Some(provider) = providers.get_mut(&name) {
                provider.encrypted_api_key = Some(migrated);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::collections::HashMap;

    type HmacSha256 = Hmac<Sha256>;

    const LEGACY_SALT_LEN: usize = 32;
    const LEGACY_NONCE_LEN: usize = 12;
    const LEGACY_KEY_LEN: usize = 32;

    fn encrypt_legacy_for_test(plaintext: &str, password: &str) -> String {
        let salt: [u8; LEGACY_SALT_LEN] = rand::random();
        let mut hmac = <HmacSha256 as Mac>::new_from_slice(&salt).expect("salt size is fixed");
        hmac.update(password.as_bytes());
        let digest = hmac.finalize().into_bytes();
        let mut key = [0u8; LEGACY_KEY_LEN];
        key.copy_from_slice(&digest[..LEGACY_KEY_LEN]);

        let cipher = Aes256Gcm::new_from_slice(&key).expect("fixed key size");
        let nonce_bytes: [u8; LEGACY_NONCE_LEN] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("legacy encryption should work");

        let mut out = Vec::with_capacity(LEGACY_SALT_LEN + LEGACY_NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&salt);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        hex::encode(out)
    }

    #[test]
    fn test_encrypt_provider_keys_migrates_legacy_ciphertext_to_v2() {
        std::env::set_var("CODEGG_MASTER_KEY", "migration-test-master-key");
        std::env::remove_var("CODEGG_ENCRYPTION_KEY");
        std::env::remove_var("OPENCODE_ENCRYPTION_KEY");

        let legacy = encrypt_legacy_for_test("legacy-provider-secret", "migration-test-master-key");
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            crate::config::schema::ProviderConfig {
                encrypted: Some(true),
                encrypted_api_key: Some(legacy),
                ..Default::default()
            },
        );
        let mut config = Config {
            provider: Some(providers),
            ..Default::default()
        };

        encrypt_provider_keys(&mut config).expect("migration should succeed");

        let provider = config
            .provider
            .as_ref()
            .and_then(|p| p.get("openai"))
            .expect("provider should exist");
        let migrated = provider
            .encrypted_api_key
            .as_ref()
            .expect("encrypted key should still be present");
        assert!(migrated.starts_with(CRYPTO_V2_PREFIX));
        let decrypted = crate::crypto::decrypt_from_string(migrated, "migration-test-master-key")
            .expect("migrated ciphertext should decrypt");
        assert_eq!(decrypted, "legacy-provider-secret");

        std::env::remove_var("CODEGG_MASTER_KEY");
    }
}
