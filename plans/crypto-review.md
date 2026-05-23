# Crypto Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| `encrypt()` returns `EncryptedData` with salt/nonce/ciphertext | VERIFIED | `src/crypto/mod.rs:60-78` - `encrypt` returns struct with all three fields |
| `decrypt()` signature `fn decrypt(encrypted: &EncryptedData, password: &str) -> Result<String, CryptoError>` | VERIFIED | `src/crypto/mod.rs:80-100` - exact match |
| `encrypt_to_string()` returns `"v2:"` prefix + hex(salt\|\|nonce\|\|ciphertext) | VERIFIED | `src/crypto/mod.rs:102-109` - `FORMAT_V2_PREFIX` constant + hex encoding |
| `decrypt_from_string()` accepts v2 and legacy formats | VERIFIED | `src/crypto/mod.rs:111-120` - strips prefix for v2, falls back to legacy |
| `EncryptedData` struct has salt (32 bytes), nonce (12 bytes), ciphertext | VERIFIED | `src/crypto/mod.rs:28-32` - struct definition matches |
| Argon2id params m=19,456 KiB, t=2, p=1 | VERIFIED | `src/crypto/mod.rs:35` - `Params::new(19_456, 2, 1, Some(KEY_LEN))` |
| Legacy HMAC-SHA256 key derivation exists | VERIFIED | `src/crypto/mod.rs:45-58` - `derive_key_legacy()` function implemented |
| AES-256-GCM (256-bit key, 12-byte nonce, 128-bit auth tag) | VERIFIED | `src/crypto/mod.rs:63` - `Aes256Gcm::new_from_slice`, `NONCE_LEN = 12` |
| Error enum variants: EncryptionFailed, DecryptionFailed, InvalidFormat, KeyDerivationFailed | VERIFIED | `src/crypto/mod.rs:16-26` - all four variants present |
| Legacy ciphertexts automatically migrated to v2 on save | VERIFIED | `src/config/encryption.rs:68-81` - checks prefix and re-encrypts |
| `decrypt_provider_keys()` called on config load | VERIFIED | `schema.rs:508`, `watcher.rs:153` - both call decrypt on load |
| `encrypt_provider_keys()` called on config save | VERIFIED | `schema.rs:535` - called before save |
| `ProviderConfig::api_key()` decrypts on-demand | VERIFIED | `schema.rs:161-169` - uses `decrypt_from_string` with master key |
| Dependencies: aes-gcm, argon2, hmac, sha2, rand, hex | VERIFIED | `Cargo.toml` - all present |
| Format: `v2:<hex(salt[32] || nonce[12] || ciphertext)>` | VERIFIED | `src/crypto/mod.rs:102-109` - implementation matches doc |
| Nonce uniqueness (fresh random per encryption) | VERIFIED | `src/crypto/mod.rs:66` - `rand::random()` for nonce |
| Memory-hard Argon2id resists GPU/ASIC | VERIFIED | `argon2` crate used with Argon2id algorithm |
| Used by config/encryption.rs | VERIFIED | `encryption.rs:23,55,71,164` - all crypto functions used |
| Used by config/schema.rs ProviderConfig::api_key() | VERIFIED | `schema.rs:165` - `decrypt_from_string` called |

---

## Bugs Found

### Critical
None identified - crypto implementation is sound.

### High
None identified.

### Medium

1. **Duplicate prefix constants** (`config/encryption.rs:4` vs `src/crypto/mod.rs:10`)
   - `encryption.rs` defines `CRYPTO_V2_PREFIX: &str = "v2:";` locally
   - `crypto/mod.rs` defines `FORMAT_V2_PREFIX: &str = "v2:";`
   - If one is changed without the other, behavior diverges
   - **Fix**: Export `FORMAT_V2_PREFIX` from `crypto/mod.rs` and import in `encryption.rs`

---

## Improvement Suggestions

### Performance
- Consider adding `#[inline]` hints to small functions like `derive_key_legacy` and constant-time comparison helpers if profiling indicates benefit

### Correctness
1. **Export v2 prefix constant** - prevent the duplicate constant issue described above
2. **Add direct test for `decrypt_legacy()`** - currently only tested indirectly via `decrypt_from_string` with legacy hex input

### Maintainability

1. **Re-export format prefix from crypto module**
   ```rust
   // src/crypto/mod.rs
   pub const FORMAT_V2_PREFIX: &str = "v2:";
   
   // src/config/encryption.rs
   use crate::crypto::FORMAT_V2_PREFIX;
   ```
   This ensures the v2 prefix stays synchronized.

2. **Add integration test for legacy round-trip** - `test_legacy_format_still_decrypts` in crypto/mod.rs tests via `decrypt_from_string`; consider also testing `decrypt_legacy` directly for completeness

3. **Document the migration flow in code comments** - the relationship between `encrypt_provider_keys` migration logic and the legacy detection in `decrypt_from_string` is spread across two files; a doc comment would help future maintainers

---

## Priority Actions (top 5 items to fix)

1. **Export `FORMAT_V2_PREFIX` from crypto module** - eliminates duplicate constant risk (Medium priority)
2. **Add `#[inline]` to small hot-path functions if profiling shows benefit** (Low priority)
3. **Add direct unit test for `decrypt_legacy()` function** - improves test coverage (Low priority)
4. **Consider adding `encrypt_to_string` test with empty plaintext edge case** - verifies behavior when ciphertext length is 0 (unlikely but defensive)
5. **Add doc comment to `decrypt_legacy` explaining it's for pre-v2 ciphertexts** - improves code clarity (Low priority)

---

## Summary

The crypto module implementation is **correct and secure**. All claims in the architecture document are verified accurate. The only issue found is a maintainability concern: duplicate `v2:` prefix constants in `crypto/mod.rs` and `config/encryption.rs` that must be kept synchronized manually. No logic errors, edge case failures, or race conditions identified.