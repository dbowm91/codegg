---
name: crypto
description: AES-256-GCM encryption for API keys and sensitive data
version: 1.0.0
tags:
  - encryption
  - crypto
  - security
  - api-keys
  - aes-gcm
---

# Crypto Module Guide

This skill covers the AES-256-GCM encryption system in opencode-rs for securing sensitive data.

## Overview

The `src/crypto/mod.rs` module provides AES-256-GCM encryption for storing sensitive data like API keys. It uses:
- **Key derivation**: HMAC-SHA256 from password and salt
- **Encryption**: AES-256-GCM with random nonce
- **Format**: Salt + Nonce + Ciphertext (hex-encoded)

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

**Format**: `salt$nonce$ciphertext` (all hex-encoded)

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

1. **Password Strength**: The password should be strong. For API key encryption, consider using a password derived from user's master password or a separate encryption key.

2. **Key Derivation**: Uses HMAC-SHA256 with random salt. Each encryption uses a different salt, making rainbow table attacks infeasible.

3. **No PBKDF2**: Current key derivation is simple HMAC-SHA256. For higher security, consider Argon2 or scrypt for password-based key derivation.

4. **In-Memory Decryption**: Decrypted data is held in memory. Clear sensitive strings after use if possible.

## Example: Encrypting Provider API Keys

```rust
use opencode_rs::crypto::encrypt_to_string;

// When storing API key in config
let api_key = "sk-ant-api03-xxx";
let encryption_password = derive_password_from_user_password()?;

let encrypted = encrypt_to_string(api_key, &encryption_password)?;
// Store encrypted in config file

// When reading back
let decrypted = decrypt_from_string(&encrypted, &encryption_password)?;
```

## Testing

Run crypto tests:
```bash
cargo test --lib -- crypto
```

Tests include:
- `test_encrypt_decrypt`: Basic encryption/decryption
- `test_encrypt_decrypt_string_format`: String format round-trip
- `test_wrong_password`: Verify wrong password fails decryption

## Dependencies

Uses existing dependencies:
- `aes-gcm` (already in Cargo.toml)
- `hmac` (already in Cargo.toml)
- `sha2` (already in Cargo.toml)
- `rand` (already in Cargo.toml)
- `hex` (already in Cargo.toml)

No new dependencies required.
