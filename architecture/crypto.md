# Crypto Module

The `crypto` module provides AES-256-GCM encryption for sensitive data using Argon2id key derivation.

## Overview

**Location**: `src/crypto/`

**Key Responsibilities**:
- AES-256-GCM encryption/decryption
- Argon2id key derivation (v2 format)
- HMAC-SHA256 key derivation (legacy format, pre-v2)
- Secure string handling with hex encoding

## Key Functions

### encrypt()

```rust
pub fn encrypt(plaintext: &str, password: &str) -> Result<EncryptedData, CryptoError>;
```

Returns `EncryptedData` containing `salt`, `nonce`, and `ciphertext`.

### decrypt()

```rust
pub fn decrypt(encrypted: &EncryptedData, password: &str) -> Result<String, CryptoError>;
```

### encrypt_to_string()

```rust
pub fn encrypt_to_string(plaintext: &str, password: &str) -> Result<String, CryptoError>;
// Returns: "v2:" prefix + hex(salt[32] || nonce[12] || ciphertext)
```

### decrypt_from_string()

```rust
pub fn decrypt_from_string(encrypted_str: &str, password: &str) -> Result<String, CryptoError>;
// Accepts both v2 format ("v2:" prefix) and legacy format
```

## Implementation Details

### EncryptedData Struct

The `EncryptedData` struct is `pub` (visible outside the crypto module), and its fields are `pub`:

```rust
pub struct EncryptedData {
    pub salt: Vec<u8>,       // 32 bytes (Argon2id salt)
    pub nonce: Vec<u8>,     // 12 bytes (AES-GCM nonce)
    pub ciphertext: Vec<u8>, // Variable length
}
```

### Key Derivation (v2 format)

Uses Argon2id for strong memory-hard key derivation:

```rust
fn derive_key_argon2id(password: &str, salt: &[u8]) -> Result<[u8; 32], CryptoError> {
    // Params: m=19,456 KiB, t=2 iterations, p=1 degree, output=32 bytes (key length)
    let params = Params::new(19_456, 2, 1, Some(32))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    // ...
}
```

### Key Derivation (Legacy)

Pre-v2 ciphertexts used HMAC-SHA256:

```rust
fn derive_key_legacy(password: &str, salt: &[u8]) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    // ...
}
```

### AES-256-GCM

- 256-bit key size
- 96-bit (12-byte) nonce (never reused)
- 128-bit authentication tag

### Format

The `FORMAT_V2_PREFIX` constant (`"v2:"`) identifies the v2 encryption format:

```
v2:<hex(salt[32] || nonce[12] || ciphertext)>
```

Legacy format (pre-v2) is raw hex without prefix.

## Error Types

```rust
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
```

## Usage

### Encrypting API Keys

```rust
use crate::crypto::{encrypt_to_string, decrypt_from_string};

let encrypted = encrypt_to_string("sk-ant-api-key...", &master_key)?;
// Store encrypted string in config
```

### Decrypting

```rust
use crate::crypto::decrypt_from_string;

let api_key = decrypt_from_string(&encrypted, &master_key)?;
```

## Security Notes

1. **Never log plaintext** - Always handle securely
2. **Key management** - Master key should be from secure source (environment variable)
3. **Nonce uniqueness** - Each encryption generates fresh random nonce
4. **Authentication tag** - Any tampering is detected via AES-GCM authentication
5. **Memory-hard derivation** - Argon2id resists GPU/ASIC attacks
6. **Legacy migration** - Legacy ciphertexts are migrated to v2 when `encrypt_provider_keys()` is called during config save. Previously encrypted data remains in legacy format until explicitly re-encrypted.

## Used By

- `config/encryption.rs` - Encrypting config secrets (provider API keys)
- `config/schema.rs` - `ProviderConfig::api_key(prefix)` method for on-demand decryption
- `auth/store.rs` - `auth::CredentialStore` uses `crypto::encrypt_to_string` /
  `decrypt_from_string` to back the user-level encrypted credential store at
  `~/.config/codegg/credentials.json`. Each `StoredCredentialRecord`'s
  `encrypted_secret` is a `v2:`-prefixed ciphertext under the same master
  key (`CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY` /
  `OPENCODE_ENCRYPTION_KEY`). The same primitives are also used by
  `auth::resolver::AuthResolver` to decrypt
  `AuthConfig::ApiKey.encrypted_value` from the provider config. See
  `.opencode/skills/auth/SKILL.md` for the resolution order and security
  rules around the master key.

## Dependencies

- `aes-gcm` - AES-256-GCM authenticated encryption
- `argon2` - Argon2id key derivation
- `hmac`, `sha2` - Legacy key derivation
- `rand` - Random salt/nonce generation
- `hex` - Hex encoding for string format

## See Also

- [config.md](config.md) - Config encryption integration
- [security.md](security.md) - Additional security measures