---
name: crypto
description: AES-256-GCM encryption with Argon2id key derivation for API keys and sensitive data
version: 2.0.0
tags:
  - encryption
  - crypto
  - security
  - api-keys
  - aes-gcm
  - argon2
---

# Crypto Module Guide

This skill covers the AES-256-GCM encryption system in opencode-rs for securing sensitive data.

## Overview

The `src/crypto/mod.rs` module provides AES-256-GCM encryption for storing sensitive data like API keys. It uses:
- **Key derivation**: Argon2id (v2 format) for strong memory-hard key derivation
- **Legacy key derivation**: HMAC-SHA256 (pre-v2 format) for backward compatibility
- **Encryption**: AES-256-GCM with random 12-byte nonce
- **Format**: `v2:` prefix + hex(salt[32] || nonce[12] || ciphertext)

## Usage

### Basic Encryption/Decryption

```rust
use opencode_rs::crypto::{encrypt_to_string, decrypt_from_string};

let plaintext = "sk-ant-api03-xxx";
let password = "user-provided-password";

// Encrypt
let encrypted = encrypt_to_string(plaintext, password)?;

// Decrypt
let decrypted = decrypt_from_string(&encrypted, password)?;
assert_eq!(decrypted, plaintext);
```

### Using EncryptedData Struct

```rust
use opencode_rs::crypto::{encrypt, decrypt, EncryptedData};

let encrypted = encrypt(plaintext, password)?;
let decrypted = decrypt(&encrypted, password)?;
```

## API Reference

### `encrypt(plaintext: &str, password: &str) -> Result<EncryptedData, CryptoError>`

Encrypts plaintext using password-derived key.

**Returns**: `EncryptedData` containing:
- `salt`: 32-byte random salt
- `nonce`: 12-byte random nonce
- `ciphertext`: Encrypted data

### `decrypt(encrypted: &EncryptedData, password: &str) -> Result<String, CryptoError>`

Decrypts encrypted data using password-derived key.

**Returns**: Decrypted plaintext string.

### `encrypt_to_string(plaintext: &str, password: &str) -> Result<String, CryptoError>`

Same as `encrypt()` but returns hex-encoded string for easy storage.

**Format**: `v2:` prefix + hex(salt || nonce || ciphertext)

### `decrypt_from_string(encrypted_str: &str, password: &str) -> Result<String, CryptoError>`

Decrypts from hex-encoded string produced by `encrypt_to_string()`.

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

## Security Considerations

1. **Password Strength**: The password should be strong. For API key encryption, use the master key from environment variables.

2. **Key Derivation (v2)**: Uses Argon2id with params m=19,456 KB, t=2, p=1. Memory-hard design resists GPU/ASIC attacks.

3. **Backward Compatibility**: Legacy ciphertexts (without `v2:` prefix) use HMAC-SHA256 key derivation and are automatically migrated to v2 format on save.

4. **Compile-Time Validation**: Constants are validated at compile time:
   ```rust
   const _: () = assert!(KEY_LEN == 32, "KEY_LEN must be 32 for AES-256");
   const _: () = assert!(NONCE_LEN == 12, "NONCE_LEN must be 12 for AES-GCM");
   const _: () = assert!(SALT_LEN == 32, "SALT_LEN must be 32 for Argon2id");
   ```

5. **In-Memory Decryption**: Decrypted data is held in memory. Clear sensitive strings after use if possible.

## Example: Encrypting Provider API Keys

The encryption is integrated via `src/config/encryption.rs`:

```rust
use opencode_rs::crypto::{encrypt_to_string, decrypt_from_string};

// 1. Get master key from environment
//    CODEGG_MASTER_KEY, CODEGG_ENCRYPTION_KEY, or OPENCODE_ENCRYPTION_KEY

// 2. Encrypt API key (done automatically in Config::save())
let api_key = "sk-ant-api03-xxx";
let encrypted = encrypt_to_string(api_key, &master_key)?;
// Store encrypted in config as encrypted_api_key

// 3. Decrypt API key (done automatically in Config::load())
let decrypted = decrypt_from_string(&encrypted, &master_key)?;
```

### Config Integration

- `decrypt_provider_keys()` called in `Config::load()` - decrypts `encrypted_api_key` fields
- `encrypt_provider_keys()` called in `Config::save()` - encrypts `api_key` fields and migrates legacy ciphertexts

## Testing

Run crypto tests:
```bash
cargo test --lib -- crypto
```

Tests include:
- `test_encrypt_decrypt`: Basic encryption/decryption with Argon2id
- `test_encrypt_decrypt_string_format`: String format round-trip (v2 prefix)
- `test_legacy_format_still_decrypts`: Backward compatibility with HMAC-SHA256
- `test_wrong_password`: Verify wrong password fails decryption

## Dependencies

Uses existing dependencies:
- `aes-gcm` (AES-256-GCM authenticated encryption)
- `argon2` (Argon2id key derivation)
- `hmac`, `sha2` (legacy key derivation)
- `rand` (random salt/nonce generation)
- `hex` (hex encoding for string format)
