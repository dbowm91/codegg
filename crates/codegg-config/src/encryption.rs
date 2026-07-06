// The encryption helpers here are intentionally no-ops. The real encryption
// pipeline lives in `codegg-providers` (root crate's `crypto` module) and is
// driven by the credential store and typed `AuthConfig::ApiKey.encrypted_value`
// fields during credential resolution. The legacy
// `ProviderConfig::encrypted_api_key` field is decrypted inside
// `resolve_provider_credential`, never here.
//
// Call sites that previously invoked `encrypt_provider_keys` /
// `decrypt_provider_keys` to migrate plaintext keys before persisting config
// have been removed; callers should use the credential store or typed auth
// instead.

use crate::error::AppError;
use crate::schema::Config;

pub fn get_master_key() -> Option<String> {
    std::env::var("CODEGG_MASTER_KEY")
        .ok()
        .or_else(|| std::env::var("CODEGG_ENCRYPTION_KEY").ok())
        .or_else(|| std::env::var("OPENCODE_ENCRYPTION_KEY").ok())
}

pub fn decrypt_provider_keys(_config: &mut Config) -> Result<(), AppError> {
    // Intentionally a no-op: see the file header.
    Ok(())
}

pub fn encrypt_provider_keys(_config: &mut Config) -> Result<(), AppError> {
    // Intentionally a no-op: see the file header.
    Ok(())
}
