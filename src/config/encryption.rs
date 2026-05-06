use crate::config::schema::Config;
use crate::error::AppError;

pub fn get_master_key() -> Option<String> {
    std::env::var("CODEGG_MASTER_KEY")
        .ok()
        .or_else(|| std::env::var("CODEGG_ENCRYPTION_KEY").ok())
}

pub fn decrypt_provider_keys(config: &mut Config) -> Result<(), AppError> {
    let Some(master_key) = get_master_key() else {
        return Ok(());
    };

    if let Some(providers) = config.provider.as_mut() {
        for (_name, provider) in providers.iter_mut() {
            if provider.encrypted == Some(true) {
                if let Some(ref encrypted_key) = provider.encrypted_api_key {
                    match crate::crypto::decrypt_from_string(encrypted_key, &master_key) {
                        Ok(decrypted) => {
                            provider.api_key = Some(decrypted);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to decrypt API key: {}", e);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn encrypt_provider_keys(config: &mut Config) -> Result<(), AppError> {
    let Some(master_key) = get_master_key() else {
        return Ok(());
    };

    if let Some(providers) = config.provider.as_mut() {
        for (_name, provider) in providers.iter_mut() {
            if let Some(ref api_key) = provider.api_key {
                if provider.encrypted != Some(true) {
                    match crate::crypto::encrypt_to_string(api_key, &master_key) {
                        Ok(encrypted) => {
                            provider.encrypted_api_key = Some(encrypted);
                            provider.encrypted = Some(true);
                            provider.api_key = None;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to encrypt API key: {}", e);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}