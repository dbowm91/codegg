# Crypto Module

The `crypto` module provides AES-256-GCM encryption for sensitive data
using Argon2id key derivation. It is the cryptographic backbone for the
user-level credential store, the typed `provider.<id>.auth.encrypted_value`
config field, and the MCP OAuth token store.

It is **not** the config-file provider-key pipeline. `codegg_config`'s
`encrypt_provider_keys()` / `decrypt_provider_keys()` are deliberate no-ops
(`crates/codegg-config/src/encryption.rs:544-552`): the config crate does not
own the encryption primitives, so the legacy `provider.<id>.api_key` /
`provider.<id>.encrypted_api_key` fields are surfaced as-is and the encrypted
one is decrypted later, during credential resolution in the providers crate.

## Purpose

Encrypt and decrypt secrets (provider API keys, stored credentials)
using memory-hard key derivation and authenticated encryption. Support
backward-compatible decryption of legacy HMAC-SHA256-derived ciphertexts.

## Where It Lives

| Artifact | Location |
|----------|----------|
| Encryption/decryption functions | `crates/codegg-providers/src/crypto.rs` |
| Re-exported via | `codegg_providers::crypto` (and, from the root crate, `codegg::provider::crypto`) |
| `CryptoError` | `crates/codegg-providers/src/crypto.rs:13` |
| `EncryptedData` | `crates/codegg-providers/src/crypto.rs:24` |
| Master key retrieval | `codegg_config::encryption::get_master_key()` |

> **Note:** This module was previously at `src/crypto/` and has been
> moved into the `codegg-providers` crate. The root crate re-exports the
> whole crate as `codegg::provider` (`src/lib.rs:16`), so
> `codegg::provider::crypto` reaches it from the root package.

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

The split happens in the private helpers
`decrypt_from_bytes_argon2id` / `decrypt_from_bytes_legacy`, each of which
slices the blob into `salt[32] || nonce[12] || ciphertext` and rejects a
blob shorter than `SALT_LEN + NONCE_LEN` with `CryptoError::InvalidFormat`.
The public `decrypt()` is **always** Argon2id: it takes an `EncryptedData`
and has no version field, so the legacy KDF is reachable only through
`decrypt_from_string()`. Both paths then run the same AES-GCM open and
report a non-UTF-8 plaintext as `CryptoError::DecryptionFailed`.

## Key Types & APIs

### CryptoError (`crypto.rs:13`)

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

- `encrypt(plaintext, password) -> Result<EncryptedData, CryptoError>` (:56)
- `decrypt(encrypted, password) -> Result<String, CryptoError>` (:76)
- `encrypt_to_string(plaintext, password) -> Result<String, CryptoError>` (:98)
- `decrypt_from_string(encrypted_str, password) -> Result<String, CryptoError>` (:107)

### Constants

- `KEY_LEN = 32` — AES-256 key length (private)
- `NONCE_LEN = 12` — AES-GCM nonce length (96 bits) (private)
- `SALT_LEN = 32` — Argon2id salt length (private)
- `FORMAT_V2_PREFIX = "v2:"` — v2 format identifier (**public** — the only
  exported constant)

### Key Derivation

**v2 (Argon2id):**
```rust
fn derive_key_argon2id(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    // m=19,456 KiB (~19 MiB), t=2 iterations, p=1 parallelism
    let params = Params::new(19_456, 2, 1, Some(KEY_LEN))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    // ...
}
```

**Legacy (HMAC-SHA256):**
```rust
fn derive_key_legacy(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    // HMAC-SHA256(salt, password) → first KEY_LEN bytes
}
```

### AES-256-GCM Parameters

- 256-bit key size
- 96-bit (12-byte) nonce (never reused — random per encryption)
- 128-bit authentication tag

## Configuration Surface

The master key is resolved by `codegg_config::encryption` (read path,
side-effect free, checked in order):

1. `CODEGG_MASTER_KEY`
2. `CODEGG_ENCRYPTION_KEY`
3. `OPENCODE_ENCRYPTION_KEY`
4. the CodeGG-managed user-local key at `<config_dir>/codegg/master.key`
   (`~/.config/codegg/master.key` on Linux,
   `~/Library/Application Support/codegg/master.key` on macOS,
   `%APPDATA%\codegg\master.key` on Windows) when one exists.
   `CODEGG_MASTER_KEY_FILE` overrides the path (test/isolation support).

```bash
# Explicit deployment-managed key (retains precedence; never auto-created)
export CODEGG_MASTER_KEY="your-master-key-here"
# Fresh personal installs need no variable: the first protected
# credential/MCP-token write atomically creates the managed key above.
```

Managed-key file contract: 32 CSPRNG bytes (256-bit entropy),
hex-encoded (64 chars); the Unix parent is created `0o700` when new. The key
is written to a **private `0o600` sibling** (opened `create_new`, i.e.
`O_EXCL`), `fsync`ed, and only then published into place with a
**non-overwriting hard link** (plus a parent-dir `fsync`) — so a reader never
observes an empty target between creation and payload write, and two
concurrent first writes converge on one key (the loser of the link race
re-reads the winner's complete file). Reads reject symlinks, non-regular
files, and any group/other permission bits, failing closed instead of
repairing an attacker-controlled path. On Windows the file is written to a
private sibling and published with a same-directory hard link inside the user
profile (`%APPDATA%\codegg`), so it inherits the profile's user-private ACL;
no external command is used on any platform. The key value is never logged,
serialized into normal config, exported with sessions, or included in
diagnostics (`ManagedMasterKey` has a redacted `Debug`).

Secret-store writes use the create-on-write resolvers
(`get_or_create_master_key` for the default locations,
`get_or_create_master_key_for_store` for a caller-supplied freshness bit, and
`get_or_create_master_key_at` for an explicit path): an explicit env key
wins, an existing managed key is reused, and a genuinely fresh store
bootstraps one. When encrypted credential/OAuth material already exists
without a usable key, writes keep `MasterKeyMissing` with recovery guidance
instead of generating a replacement key that would orphan the old ciphertext.
Reads stay side-effect free; no key is created at process startup.

## Invariants & Gotchas

1. **Never log plaintext.** Always handle encrypted data securely in
   logs and error messages.
2. **Nonce uniqueness.** Each `encrypt()` call generates a fresh random
   nonce via `rand::random()`. AES-GCM nonce reuse is catastrophic.
3. **Memory-hard derivation.** Argon2id with 19 MiB memory cost
   resists GPU/ASIC attacks. Legacy HMAC-SHA256 does not.
4. **Legacy migration.** Legacy ciphertexts are decrypted transparently by
   `decrypt_from_string`. A legacy ciphertext stays legacy until it is
   rewritten: **nothing re-encrypts it on config save.**
   `codegg_config::encryption::encrypt_provider_keys()` is an intentional
   no-op, and `Config::save_to` writes the legacy `api_key` /
   `encrypted_api_key` fields through as-is. The only writer of canonical
   `v2:` ciphertext is the credential store
   (`CredentialStore::put`) and the MCP token store, both of which encrypt
   fresh material with `encrypt_to_string`.
5. **Master key bootstrap.** The first protected write on a fresh
   local profile creates the managed key automatically
   (`CredentialStore::put`, MCP OAuth token writes, Eggpool
   provisioning). `AuthError::MasterKeyMissing` now only means a store
   already holds encrypted material without a usable key — restore the
   historical environment key rather than expecting a new one.
   Reading plaintext from the store without a master key returns
   `Ok(None)`.
6. **Auth logging never reveals secrets.** `mask_secret()`
   (`crates/codegg-providers/src/auth_types.rs`) returns a fixed 16-bullet
   mask for any non-empty secret, and an empty string for an empty one —
   never a length-dependent rendering of the secret itself.

## Dependencies

- `aes-gcm` — AES-256-GCM authenticated encryption
- `argon2` — Argon2id key derivation (v0x13)
- `hmac`, `sha2` — Legacy key derivation
- `rand` — Random salt/nonce generation
- `hex` — Hex encoding for string format

## Used By

- `codegg_config::encryption` — owns **master-key resolution** only
  (`get_master_key`, `get_or_create_master_key*`), not the primitives. It
  depends on `codegg-providers` for the crypto itself.
- `codegg_config::schema` — `ProviderConfig::api_key(prefix)` resolves an
  env var or a plaintext `api_key`; encrypted values are **not** decrypted
  there. The legacy `provider.<id>.encrypted_api_key` field is decrypted in
  the providers crate by `resolve_provider_credential`
  (`crates/codegg-providers/src/provider_core.rs`), and the typed
  `provider.<id>.auth.encrypted_value` field by `AuthResolver`
  (`crates/codegg-providers/src/auth_types.rs`).
- `codegg_providers::auth_types::CredentialStore` — user-level encrypted
  credential store at `<config_dir>/codegg/credentials.json`
  (`~/.config/codegg/credentials.json` on Linux,
  `~/Library/Application Support/codegg/credentials.json` on macOS,
  `%APPDATA%\codegg\credentials.json` on Windows). Each
  `StoredCredentialRecord`'s `encrypted_secret` is a `v2:`-prefixed
  ciphertext under the same master key.
- `mcp::OAuthManager` (`src/mcp/auth.rs`) — MCP-owned multi-server OAuth
  token-set persistence. New whole-store writes use the same master-key
  lookup and canonical `v2:` ciphertext inside the explicit
  `CODEGG_MCP_ENC_v2:` envelope. Its `CODEGG_ENC_v1` / `CODEGG_TOKEN_KEY`
  path is a decrypt-only migration reader for historical files and is not a
  second new-write crypto policy.

## Testing

There is **no `#[cfg(test)]` module inside `crypto.rs`** — the primitive
itself is covered indirectly, from the crates that own the encrypted
stores:

```bash
# Master-key resolution, the managed-key file contract, and the
# material detectors (codegg-config):
cargo test -p codegg-config --lib encryption

# v2 encrypt/decrypt round-trip through the credential store, the
# managed-key bootstrap, and legacy `encrypted_api_key` decryption
# (codegg-providers):
cargo test -p codegg-providers --lib auth_types
cargo test -p codegg-providers --lib provider_core
```

Tests cover:
- v2 encrypt → decrypt round-trip (via `CredentialStore::put` /
  `get_plaintext`, and `encrypt_to_string` / `decrypt_from_string`)
- legacy `provider.<id>.encrypted_api_key` decryption and its
  "no master key → skip" branch
- `MasterKeyMissing` when orphaned material exists, and the refusal to
  generate a replacement key
- the managed-key file's symlink / permission / corruption rejection
  (fail-closed, never repaired)
- `CryptoError::InvalidFormat` on malformed input (surfaced through the
  store and resolver error paths)

## Related Docs

- [config.md](config.md) — Config encryption integration
- [auth.md](auth.md) — Credential store and resolution
- [security.md](security.md) — Additional security measures

## Source Verification

Verified 2026-10-23 against `crates/codegg-providers/src/crypto.rs`
(the only `crypto.rs` in the workspace; exported as `pub mod crypto` from
`crates/codegg-providers/src/lib.rs`), plus
`crates/codegg-config/src/encryption.rs`,
`crates/codegg-providers/src/auth_types.rs`,
`crates/codegg-providers/src/provider_core.rs`,
`crates/codegg-config/src/schema.rs`, and `src/mcp/auth.rs`.

Corrections made in this pass:
- **Removed the false "encrypts API keys in config files" claim.** The
  config crate's `encrypt_provider_keys()` / `decrypt_provider_keys()`
  are explicit no-ops (`crates/codegg-config/src/encryption.rs:544-552`),
  and `Config::save_to` writes the legacy `api_key` /
  `encrypted_api_key` fields through as-is
  (`crates/codegg-config/src/schema.rs`). The doc's *Legacy migration*
  invariant claimed "re-encryption to v2 happens when
  `encrypt_provider_keys()` is called during config save" — that call is a
  no-op, so no such migration occurs.
- **Corrected the `Used By` list.** `ProviderConfig::api_key(prefix)`
  (`crates/codegg-config/src/schema.rs`) resolves only an env var or a
  plaintext `api_key`; it never decrypts. Decryption of
  `provider.<id>.encrypted_api_key` happens in the providers crate's
  `resolve_provider_credential`, and of
  `provider.<id>.auth.encrypted_value` in `AuthResolver`. The doc also
  named `codegg_config::encryption` as an *encryptor*; it owns master-key
  **resolution** only.
- **Corrected the managed-key file contract.** The file is not created
  in place with `O_EXCL`: the key is written to a private `0o600` sibling
  (opened `create_new`), `fsync`ed, and published with a non-overwriting
  **hard link** plus a parent-dir `fsync`. The doc's "concurrent first
  writes converge via `O_EXCL`" was the wrong mechanism — convergence is
  the hard-link race, and the loser re-reads the winner's complete file.
- **Corrected the `Testing` section.** `crypto.rs` has no `#[cfg(test)]`
  module, so `cargo test -p codegg-providers --lib crypto` selects zero
  tests. The coverage is in `codegg-config`'s `encryption` module and in
  `codegg-providers`' `auth_types` / `provider_core` modules; the command
  list and the covered behaviours now name those.
- **Corrected `mask_secret()`'s description.** It returns a 16-bullet mask
  for a non-empty secret and an empty string for an empty one — "a fixed
  16-bullet mask regardless of input length" was wrong for the empty case.
- **Added the credential-store's real path** (`<config_dir>/codegg/credentials.json`
  with the per-OS expansion) — the doc had only the Linux form, while the
  master-key section already listed all three.

Confirmed correct as written: the module location table; the 4
`CryptoError` variants; `EncryptedData`'s three fields; the 4 public
functions and their signatures; the 4 constants (`KEY_LEN=32`,
`NONCE_LEN=12`, `SALT_LEN=32`, `FORMAT_V2_PREFIX="v2:"` at `:7`-`:10`);
the Argon2id params `m=19_456, t=2, p=1, V0x13`; the v2 wire format
`v2:<hex(salt[32] || nonce[12] || ciphertext)>`; the legacy HMAC-SHA256
KDF and the legacy `decrypt_from_bytes_legacy` path; the `derive_key_legacy`
signature `Result<[u8; KEY_LEN], CryptoError>`; the master-key
precedence chain and the `CODEGG_MASTER_KEY_FILE` override; the
`get_or_create_master_key*` family and the `MasterKeyMissing`
recovery contract; and that `src/auth/mod.rs` re-exports `auth_types` only
and does **not** contain crypto.
