# Crypto Module

The `crypto` module provides AES-256-GCM encryption for sensitive data.

## Overview

**Location**: `src/crypto/`

**Key Responsibilities**:
- AES-256-GCM encryption/decryption
- HMAC-SHA256 key derivation
- Secure string handling

## Key Functions

### encrypt()

```rust
pub fn encrypt(plaintext: &[u8], key: &Key) -> Result<Vec<u8>>;
```

### decrypt()

```rust
pub fn decrypt(ciphertext: &[u8], key: &Key) -> Result<Vec<u8>>;
```

### encrypt_to_string()

```rust
pub fn encrypt_to_string(plaintext: &str, key: &Key) -> Result<String>;
// Returns base64-encoded ciphertext
```

### decrypt_from_string()

```rust
pub fn decrypt_from_string(ciphertext: &str, key: &Key) -> Result<String>;
// Expects base64-encoded ciphertext
```

## Implementation Details

### Key Derivation

Uses HMAC-SHA256 for key derivation:

```rust
fn derive_key(master: &Key, purpose: &str) -> [u8; 32] {
    let hmac = HmacSha256::new_from_slice(master.as_bytes())
        .expect("HMAC can take key of any size");
    // Use hmac.tag() for derived key
}
```

### AES-256-GCM

- 256-bit key size
- 96-bit nonce (never reused)
- 128-bit authentication tag

### Key Type

```rust
pub struct Key([u8; 32]);

impl Key {
    pub fn from_password(password: &str) -> Self;
    pub fn from_bytes(bytes: [u8; 32]) -> Self;
}
```

## Usage

### Encrypting API Keys

```rust
use crate::crypto::{encrypt_to_string, Key};

let key = Key::from_password("my-master-password");
let encrypted = encrypt_to_string("sk-ant-api-key...", &key)?;
// Store encrypted string in config
```

### Decrypting

```rust
use crate::crypto::{decrypt_from_string, Key};

let key = Key::from_password("my-master-password");
let api_key = decrypt_from_string(encrypted, &key)?;
```

## Security Notes

1. **Never log plaintext** - Always handle securely
2. **Key management** - Master key should be from secure source
3. **Nonce uniqueness** - Each encryption generates fresh nonce
4. **Authentication tag** - Any tampering is detected

## Used By

- `config/encryption.rs` - Encrypting config secrets
- `permission/` - HMAC signatures on permission decisions

## See Also

- [config.md](config.md) - Config encryption
- [security.md](security.md) - Additional security measures
