# Crypto Module Architecture Review

**Date**: 2026-05-27
**Reviewer**: Code review agent
**Module**: `src/crypto/`
**Documentation**: `architecture/crypto.md`, `.opencode/skills/crypto/SKILL.md`

## Summary

The crypto module provides AES-256-GCM encryption with Argon2id key derivation (v2 format) and HMAC-SHA256 (legacy format). All claims in the architecture document and skill were verified against the actual implementation. **No bugs found.** The implementation is correct and well-documented.

---

## Verified Items

### 1. Core Implementation (`src/crypto/mod.rs`)

| Item | Status | Details |
|------|--------|---------|
| `encrypt()` function | Verified | Lines 60-78, returns `EncryptedData` with salt/nonce/ciphertext |
| `decrypt()` function | Verified | Lines 80-100, validates format and decrypts |
| `encrypt_to_string()` | Verified | Lines 102-109, returns `v2:` prefix + hex encoded data |
| `decrypt_from_string()` | Verified | Lines 111-120, handles both v2 and legacy formats |
| `FORMAT_V2_PREFIX` | Verified | Line 10: `pub const FORMAT_V2_PREFIX: &str = "v2:"` |
| `EncryptedData` struct | Verified | Lines 28-32, has `salt`, `nonce`, `ciphertext` fields |
| `CryptoError` enum | Verified | Lines 16-26, 4 variants matching docs |

### 2. Key Derivation

| Item | Status | Details |
|------|--------|---------|
| Argon2id params | Verified | Line 35: `Params::new(19_456, 2, 1, Some(KEY_LEN))` - m=19,456 KiB, t=2, p=1 |
| Legacy key derivation | Verified | Lines 45-58, uses HMAC-SHA256 for backward compatibility |
| Compile-time assertions | Verified | Lines 12-14, validates KEY_LEN=32, NONCE_LEN=12, SALT_LEN=32 |

### 3. Format

| Item | Status | Details |
|------|--------|---------|
| v2 format | Verified | `v2:` + hex(salt[32] \|\| nonce[12] \|\| ciphertext) at line 108 |
| Legacy format | Verified | Raw hex without prefix, auto-detected in `decrypt_from_string()` |
| Nonce size | Verified | 12 bytes (96-bit) at line 8 |
| Salt size | Verified | 32 bytes at line 9 |
| Key size | Verified | 32 bytes (256-bit) at line 7 |

### 4. Tests

| Test | Status | Result |
|------|--------|--------|
| `test_encrypt_decrypt` | Verified | Passes |
| `test_encrypt_decrypt_string_format` | Verified | Passes |
| `test_legacy_format_still_decrypts` | Verified | Passes |
| `test_wrong_password` | Verified | Passes |

---

## Integration Points Verified

### Config Encryption (`src/config/encryption.rs`)

| Function | Status | Details |
|----------|--------|---------|
| `decrypt_provider_keys()` | Verified | Lines 12-42, called in `Config::load()` (schema.rs:542) |
| `encrypt_provider_keys()` | Verified | Lines 44-107, called in `Config::save()` (schema.rs:569) |
| Legacy migration | Verified | Lines 68-82, migrates pre-v2 ciphertexts to v2 on save |
| Master key env vars | Verified | Lines 5-10, checks CODEGG_MASTER_KEY, CODEGG_ENCRYPTION_KEY, OPENCODE_ENCRYPTION_KEY |

### Schema Integration (`src/config/schema.rs`)

| Function | Status | Details |
|----------|--------|---------|
| `api_key()` method | Verified | Lines 183-205, decrypts on-demand via `decrypt_from_string()` |
| `decrypt_provider_keys()` call | Verified | Line 542 in `load()` |
| `encrypt_provider_keys()` call | Verified | Line 569 in `save()` |

### ConfigWatcher (`src/config/watcher.rs`)

| Function | Status | Details |
|----------|--------|---------|
| `decrypt_provider_keys()` call | Verified | Line 157 in `reload_config()` |

---

## Dependencies (from Cargo.toml)

All dependencies documented and verified:

| Dependency | Purpose | Verified |
|------------|---------|----------|
| `aes-gcm` | AES-256-GCM authenticated encryption | Line 104 |
| `argon2` | Argon2id key derivation | Line 105 |
| `hmac`, `sha2` | Legacy key derivation | Lines 101-102 |
| `rand` | Random salt/nonce generation | Line 100 |
| `hex` | Hex encoding for string format | Line 103 |

---

## Discrepancies Found

**None.** The architecture document and skill accurately reflect the actual implementation.

---

## Bugs Found

**None.** The implementation is correct.

---

## Documentation Quality

### Architecture (`architecture/crypto.md`)
- Accurate and complete
- Correct function signatures
- Correct format description
- Correct error types
- Correct integration points

### Skill (`.opencode/skills/crypto/SKILL.md`)
- Accurate and complete
- Version 2.0.0 matches implementation
- Correct API reference
- Correct security considerations
- Correct example code

---

## Recommendations

### For Documentation
1. **No changes needed** - Both documents are accurate and well-maintained.

### For Code
1. **No changes needed** - Implementation is correct and properly tested.

---

## File Reference Summary

| File | Lines | Description |
|------|-------|-------------|
| `src/crypto/mod.rs` | 1-236 | Complete crypto implementation |
| `src/crypto/mod.rs:10` | 10 | `FORMAT_V2_PREFIX` constant |
| `src/crypto/mod.rs:28-32` | 28-32 | `EncryptedData` struct |
| `src/crypto/mod.rs:34-43` | 34-43 | `derive_key_argon2id()` |
| `src/crypto/mod.rs:45-58` | 45-58 | `derive_key_legacy()` |
| `src/crypto/mod.rs:60-78` | 60-78 | `encrypt()` |
| `src/crypto/mod.rs:80-100` | 80-100 | `decrypt()` |
| `src/crypto/mod.rs:102-109` | 102-109 | `encrypt_to_string()` |
| `src/crypto/mod.rs:111-120` | 111-120 | `decrypt_from_string()` |
| `src/crypto/mod.rs:179-236` | 179-236 | Unit tests |
| `src/config/encryption.rs` | 1-187 | Config encryption integration |
| `src/config/schema.rs:183-205` | 183-205 | `ProviderConfig::api_key()` |
| `src/config/schema.rs:542` | 542 | `decrypt_provider_keys()` call in `load()` |
| `src/config/schema.rs:569` | 569 | `encrypt_provider_keys()` call in `save()` |
| `src/config/watcher.rs:157` | 157 | `decrypt_provider_keys()` call in hot-reload |

---

## Conclusion

The crypto module is **verified correct**. All documentation claims match the implementation, all tests pass, and the integration with the config system is properly wired. The module correctly implements AES-256-GCM with Argon2id key derivation (v2 format) while maintaining backward compatibility with legacy HMAC-SHA256 ciphertexts.

**No bugs, no inconsistencies, no missing documentation.**
