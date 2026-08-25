# Crypto Module

The `crypto` module provides AES-256-GCM encryption for sensitive data
using Argon2id key derivation. It is the cryptographic backbone for
encrypting API keys in config files and the user-level credential store.

## Purpose

Encrypt and decrypt secrets (provider API keys, stored credentials)
using memory-hard key derivation and authenticated encryption. Support
backward-compatible decryption of legacy HMAC-SHA256-derived ciphertexts.

## Where It Lives

| Artifact | Location |
|----------|----------|
| Encryption/decryption functions | `crates/codegg-providers/src/crypto.rs` |
| Re-exported via | `codegg_providers::crypto` |
| `CryptoError` | `crates/codegg-providers/src/crypto.rs:12` |
| Master key retrieval | `codegg_config::encryption::get_master_key()` |

> **Note:** This module was previously at `src/crypto/` and has been
> moved into the `codegg-providers` crate. Root `src/` re-exports are
> available via `codegg_providers::crypto`.

## How It Works

### v2 Format (Current)

1. Generate 32-byte random salt
2. Derive 32-byte key via Argon2id (m=19,456 KiB, t=2, p=1)
3. Generate 12-byte random nonce
4. Encrypt with AES-256-GCM (256-bit key, 96-bit nonce, 128-bit auth tag)
5. Encode as: `v2:<hex(salt[32] || nonce[12] || ciphertext)>`

### Legacy Format (Pre-v2)

Raw hex without prefix, using HMAC-SHA256 key derivation:

```
hex(salt[32] || nonce[12] || ciphertext)
```

Legacy key derivation uses `HMAC-SHA256(salt, password)` as a
one-shot KDF (not memory-hard).

### Decryption

`decrypt_from_string()` accepts both formats:
- `v2:` prefix → Argon2id key derivation
- No prefix → Legacy HMAC-SHA256 key derivation

## Key Types & APIs

### CryptoError (`crypto.rs:12`)

```rust
pub enum CryptoError {
    EncryptionFailed(String),
    DecryptionFailed(String),
    InvalidFormat,
    KeyDerivationFailed,
}
```

### EncryptedData (`crypto.rs:24`)

```rust
pub struct EncryptedData {
    pub salt: Vec<u8>,       // 32 bytes (Argon2id salt)
    pub nonce: Vec<u8>,     // 12 bytes (AES-GCM nonce)
    pub ciphertext: Vec<u8>, // Variable length
}
```

### Public Functions

- `encrypt(plaintext, password) -> Result<EncryptedData, CryptoError>` (:55)
- `decrypt(encrypted, password) -> Result<String, CryptoError>` (:75)
- `encrypt_to_string(plaintext, password) -> Result<String, CryptoError>` (:97)
- `decrypt_from_string(encrypted_str, password) -> Result<String, CryptoError>` (:106)

### Constants

- `KEY_LEN = 32` — AES-256 key length
- `NONCE_LEN = 12` — AES-GCM nonce length (96 bits)
- `SALT_LEN = 32` — Argon2id salt length
- `FORMAT_V2_PREFIX = "v2:"` — v2 format identifier

### Key Derivation

**v2 (Argon2id):**
```rust
fn derive_key_argon2id(password: &str, salt: &[u8]) -> Result<[u8; 32], CryptoError> {
    // m=19,456 KiB (~19 MiB), t=2 iterations, p=1 parallelism
    let params = Params::new(19_456, 2, 1, Some(32))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    // ...
}
```

**Legacy (HMAC-SHA256):**
```rust
fn derive_key_legacy(password: &str, salt: &[u8]) -> [u8; 32] {
    // HMAC-SHA256(salt, password) → first 32 bytes
}
```

### AES-256-GCM Parameters

- 256-bit key size
- 96-bit (12-byte) nonce (never reused — random per encryption)
- 128-bit authentication tag

## Configuration Surface

The master key is retrieved from environment variables (checked in
order):
1. `CODEGG_MASTER_KEY`
2. `CODEGG_ENCRYPTION_KEY`
3. `OPENCODE_ENCRYPTION_KEY`

```bash
# Set the master key for encryption/decryption
export CODEGG_MASTER_KEY="your-master-key-here"
```

## Invariants & Gotchas

1. **Never log plaintext.** Always handle encrypted data securely in
   logs and error messages.
2. **Nonce uniqueness.** Each `encrypt()` call generates a fresh random
   nonce via `rand::random()`. AES-GCM nonce reuse is catastrophic.
3. **Memory-hard derivation.** Argon2id with 19 MiB memory cost
   resists GPU/ASIC attacks. Legacy HMAC-SHA256 does not.
4. **Legacy migration.** Legacy ciphertexts are decrypted transparently
   by `decrypt_from_string`. Re-encryption to v2 happens when
   `encrypt_provider_keys()` is called during config save. Previously
   encrypted data remains in legacy format until explicitly
   re-encrypted.
5. **Master key required to store.** `CredentialStore::put` and
   `AuthResolver` decryption of `encrypted_value` return
   `AuthError::MasterKeyMissing` if no master key is configured.
   Reading plaintext from the store without a master key returns
   `Ok(None)`.
6. **Auth logging never reveals secrets.** `mask_secret()` returns a
   fixed 16-bullet mask regardless of input length.

## Dependencies

- `aes-gcm` — AES-256-GCM authenticated encryption
- `argon2` — Argon2id key derivation (v0x13)
- `hmac`, `sha2` — Legacy key derivation
- `rand` — Random salt/nonce generation
- `hex` — Hex encoding for string format

## Used By

- `codegg_config::encryption` — Encrypting config secrets (provider
  API keys) via `encrypt_provider_keys()`
- `codegg_config::schema` — `ProviderConfig::api_key(prefix)` method
  for on-demand decryption
- `auth::CredentialStore` — User-level encrypted credential store at
  `~/.config/codegg/credentials.json`. Each `StoredCredentialRecord`'s
  `encrypted_secret` is a `v2:`-prefixed ciphertext under the same
  master key.
- `auth::AuthResolver` — Decrypts `AuthConfig::ApiKey.encrypted_value`
  from provider config.

## Testing

```bash
cargo test -p codegg-providers --lib crypto
```

Tests verify:
- v2 encryption/decryption roundtrip
- Legacy decryption compatibility
- `encrypt_to_string` / `decrypt_from_string` roundtrip
- `CryptoError::InvalidFormat` on malformed input

## Related Docs

- [config.md](config.md) — Config encryption integration
- [auth.md](auth.md) — Credential store and resolution
- [security.md](security.md) — Additional security measures
