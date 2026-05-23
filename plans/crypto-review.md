# Crypto Module Architecture Review

## Verified Claims

### Function Signatures (lines 20-42 in arch, lines 60-120 in src/crypto/mod.rs)
- `encrypt()`, `decrypt()`, `encrypt_to_string()`, `decrypt_from_string()` all match
- Return types correct: `EncryptedData` struct for `encrypt()`, `Result<String, CryptoError>` for decrypt variants

### EncryptedData Struct (lines 47-55 in arch, lines 28-32 in src/crypto/mod.rs)
- Struct definition matches exactly: `salt`, `nonce`, `ciphertext` as `Vec<u8>`
- Salt: 32 bytes (line 9: `SALT_LEN = 32`)
- Nonce: 12 bytes (line 8: `NONCE_LEN = 12`)
- Key: 32 bytes (line 7: `KEY_LEN = 32`)

### Key Derivation (v2 format)
- Argon2id params match: `Params::new(19_456, 2, 1, Some(32))` at mod.rs:35
- Algorithm: `Argon2id`, Version: `V0x13` (line 37)
- HMAC-SHA256 legacy derivation correctly documented (lines 74-81)

### AES-256-GCM
- Key size: 256-bit (32 bytes)
- Nonce: 96-bit (12 bytes)
- Authentication tag included via AES-GCM (lines 63-70)

### Format
- `v2:` prefix constant exported: `pub const FORMAT_V2_PREFIX: &str = "v2:"` (mod.rs:10)
- Format string at arch line 92 matches implementation

### Error Types (lines 97-110 in arch, lines 16-26 in src/crypto/mod.rs)
- `CryptoError` enum variants match exactly: `EncryptionFailed`, `DecryptionFailed`, `InvalidFormat`, `KeyDerivationFailed`
- Error message formats match

### Dependencies Listed
- `aes-gcm`, `argon2`, `hmac`, `sha2`, `rand`, `hex` all used

### Legacy Format
- Legacy decryption via HMAC-SHA256 implemented (mod.rs:45-58, 140-177)
- Legacy format auto-detection works via lack of `v2:` prefix (mod.rs:111-120)
- Migration on save: non-v2 prefixed ciphertexts are re-encrypted to v2 (encryption.rs:68-82)

## Bugs/Discrepancies Found

### Discrepancy 1: `api_key()` method documentation
**Priority: Medium**

The architecture doc at line 143 states:
> `config/schema.rs` - `ProviderConfig::api_key()` method for on-demand decryption

The documentation example shows `decrypt_from_string(decrypted, &master_key)` being called from `api_key()` but the actual implementation at schema.rs:183-205 shows `api_key(self, prefix: &str)` takes a `prefix` parameter for environment variable lookup (`{PREFIX}_API_KEY`). The method does use `decrypt_from_string` but with `get_master_key()` not a passed master key.

The doc at lines 125-128 shows an incorrect example:
```rust
use crate::crypto::{decrypt_from_string, decrypt_from_string};
```

This duplicates `decrypt_from_string` and doesn't show the actual `ProviderConfig::api_key()` usage pattern.

### Discrepancy 2: Legacy migration documentation
**Priority: Low**

Arch doc line 138 states:
> Legacy ciphertexts automatically migrated to v2 on save

This is partially correct but the implementation in `encryption.rs:encrypt_provider_keys()` only migrates when `encrypt_provider_keys()` is explicitly called, not on every save. The migration logic exists (lines 68-82) but is gated on calling that function.

## Improvement Suggestions

### Priority: Medium

1. **Fix `decrypt_from_string` import typo** (architecture/crypto.md:125-126)
   - Shows `use crate::crypto::{decrypt_from_string, decrypt_from_string};`
   - Should demonstrate `ProviderConfig::api_key()` method usage or remove duplicate import

2. **Clarify migration behavior** (architecture/crypto.md:138)
   - "automatically migrated to v2 on save" is misleading
   - Should say "migrated when `encrypt_provider_keys()` is called (e.g., on config save)"

### Priority: Low

3. **Add missing security note about environment variables**
   - The `get_master_key()` function checks `CODEGG_MASTER_KEY`, `CODEGG_ENCRYPTION_KEY`, and `OPENCODE_ENCRYPTION_KEY` env vars (encryption.rs:5-10)
   - Should document which env var takes precedence

4. **Document that `decrypt_provider_keys()` and `encrypt_provider_keys()` are idempotent**
   - `decrypt_provider_keys()` skips providers where `encrypted != Some(true)`
   - `encrypt_provider_keys()` only encrypts providers where `encrypted != Some(true)`
   - Safe to call multiple times

### Priority: Informational

5. **Consider adding test coverage for edge cases**
   - No test for corrupted hex input
   - No test for truncated ciphertext
   - No test for wrong-length salt/nonce

## Summary

The crypto module architecture documentation is **largely accurate**. All function signatures, type definitions, key derivation parameters, and error types match the implementation. The main issues are:

1. A typo in the usage example (duplicate import)
2. Slightly misleading wording about "automatic" migration
3. Incomplete documentation of `ProviderConfig::api_key()` signature

These are documentation quality issues rather than implementation bugs. The crypto implementation itself appears correct and well-tested with legacy format compatibility working as documented.