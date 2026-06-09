use crate::error::AppError;
use crate::schema::Config;

pub fn get_master_key() -> Option<String> {
    std::env::var("CODEGG_MASTER_KEY")
        .ok()
        .or_else(|| std::env::var("CODEGG_ENCRYPTION_KEY").ok())
        .or_else(|| std::env::var("OPENCODE_ENCRYPTION_KEY").ok())
}

pub fn decrypt_provider_keys(_config: &mut Config) -> Result<(), AppError> {
    // TODO: Wire up to root crate's crypto module.
    // The actual decryption is done in the root crate's crypto module.
    // For now, this is a no-op stub.
    Ok(())
}

pub fn encrypt_provider_keys(_config: &mut Config) -> Result<(), AppError> {
    // TODO: Wire up to root crate's crypto module.
    // The actual encryption is done in the root crate's crypto module.
    // For now, this is a no-op stub.
    Ok(())
}
