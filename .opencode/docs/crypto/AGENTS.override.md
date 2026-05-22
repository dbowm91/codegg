# Crypto Module Override

This file contains crypto-specific guidance and overrides root AGENTS.md.

## API Key Encryption (Updated 2026-05-22)

Provider API keys can be encrypted at rest using master key from environment:
- `CODEGG_MASTER_KEY`
- `CODEGG_ENCRYPTION_KEY`
- `OPENCODE_ENCRYPTION_KEY`

### Encryption Format (v2)

- **Key derivation**: Argon2id (m=19,456 KB, t=2, p=1)
- **Encryption**: AES-256-GCM with random 12-byte nonce
- **Format**: `v2:hex(salt[32] || nonce[12] || ciphertext)`
- **Backward compatible**: Legacy format (HMAC-SHA256) auto-migrated on save

### Config Integration

- `decrypt_provider_keys()` called in `Config::load()` - decrypts `encrypted_api_key` fields
- `encrypt_provider_keys()` called in `Config::save()` - encrypts `api_key` fields and migrates legacy ciphertexts
- `ProviderConfig::api_key()` method: Returns decrypted key if `encrypted == Some(true)`

### Compile-Time Constants

```rust
const KEY_LEN: usize = 32;    // AES-256
const NONCE_LEN: usize = 12;   // AES-GCM standard
const SALT_LEN: usize = 32;    // Argon2id recommended
```

Validated at compile time with `const` assertions.