---
name: auth
description: Typed AuthConfig, Credential, AuthResolver, user-level credential store, external-command provider, and OAuth scaffolding
version: 1.1.0
tags:
  - auth
  - credentials
  - api-key
  - resolver
  - security
  - oauth
  - credential-store
---

# Auth Module Guide

This skill covers the `src/auth/` module — the central seam where
providers obtain credentials. It owns the typed `AuthConfig` (the
configuration shape that lives in `provider.<id>.auth`), the resolved
`Credential` (a `kind`/`secret`/`expires_at` triple used to build an
`Authorization` header), the `AuthResolver` (which performs the env →
config → store priority lookup), a user-level encrypted
`CredentialStore`, an `ExternalCommandProvider` for shelling out to an
officially-supported CLI (currently disabled in the synchronous resolver
because the underlying `std::process::Command` does not enforce its
timeout), OAuth device-flow scaffolding (typed but unimplemented in this
pass), and a `cli` sub-module that wires the credential store into
`codegg auth status | set-key | logout`.

A detailed architecture document at `architecture/auth.md` is forthcoming.

## What `auth/` owns

| Submodule | Public types | Purpose |
|-----------|--------------|---------|
| `auth::credential` | `Credential`, `CredentialKind`, `mask_secret` | Resolved secret + metadata; fixed-length `mask_secret` helper. |
| `auth::resolver` | `AuthResolver`, `ResolverContext`, `ResolvedAuth`, `ResolvedAuthSource`, `conventional_env_map` | Priority-based lookup. |
| `auth::store` | `CredentialStore`, `StoredCredentialRecord` | User-level encrypted store at `~/.config/codegg/credentials.json`. |
| `auth::external` | `ExternalCommandProvider`, `ExternalCredential` | Shells out to an external CLI for short-lived creds. Synchronous execution does NOT enforce its timeout, so the resolver currently returns `Unsupported` for `ExternalCommand`. |
| `auth::oauth` | `OAuthDeviceProvider`, `OAuthDeviceSpec`, `DeviceCode` | Typed scaffolding; entry points return `AuthError::Unsupported`. |
| `auth::cli` | `AuthCli`, `read_key_from_stdin` | Minimal CLI for `codegg auth status | set-key | logout`. |
| `auth::error` | `AuthError` | `NotFound`, `Expired`, `MasterKeyMissing`, `Crypto`, `Io`, `Json`, `Unsupported`, `Invalid`, `ExternalCommand { command, message }`. |
| `auth::test_support` | `env_lock`, `lock_env` | Cross-module mutex that serializes tests mutating `CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY` / `OPENCODE_ENCRYPTION_KEY` / `OPENAI_API_KEY`. Production code should never observe this module. |

The `Credential` type is what providers consume at request time:

```rust
pub struct Credential {
    pub kind: CredentialKind,           // ApiKey | BearerToken
    pub secret: String,
    pub expires_at: Option<DateTime<Utc>>,
}
```

`Credential::authorization_header_value()` returns `Bearer {secret}` for
both variants. `Credential::api_key(secret)` and `Credential::bearer(secret, expires_at)`
are the construction helpers.

## Resolution priority order

For an `AuthConfig::ApiKey { env, value, encrypted_value }` provider, the
resolver tries each step in order and returns the first hit:

1. `ctx.env_override` (test-only) or `AuthConfig::ApiKey.env` env var, if set → `ResolvedAuthSource::EnvExplicit`
2. Conventional env var `{PROVIDER}_API_KEY` (provider id uppercased, `-` → `_`) → `ResolvedAuthSource::EnvConventional`
3. `AuthConfig::ApiKey.value` (inline, non-empty) → `ResolvedAuthSource::InlineValue`
4. `AuthConfig::ApiKey.encrypted_value`, decrypted with the master key → `ResolvedAuthSource::EncryptedConfig` (returns `AuthError::MasterKeyMissing` if no key)
5. User-level `CredentialStore` lookup for the provider id (+ optional account id), filtered to `kind == ApiKey` → `ResolvedAuthSource::UserStore`
6. Legacy `ProviderConfig::api_key` (post-decryption) → `ResolvedAuthSource::LegacyApiKey`
7. Legacy `ProviderConfig::encrypted_api_key` already decrypted → `ResolvedAuthSource::LegacyDecrypted`

`AuthConfig::Stored { account_id }` skips straight to step 5
(`ResolvedAuthSource::UserStore`). `AuthConfig::ExternalCommand` is
recognized but the synchronous resolver returns
`AuthError::Unsupported("ExternalCommand")` because the underlying
`std::process::Command`-based provider does not enforce its timeout
and could otherwise hang provider registration indefinitely. Async
timeout plumbing is a follow-up — see
`plans/oauth_provider_auth_followup.md`.
`AuthConfig::OAuthDevice` is recognized but returns
`AuthError::Unsupported("OAuthDevice")`. `AuthConfig::None` returns
`Ok(None)` regardless of legacy fields.

`ResolvedAuthSource::as_str()` returns a stable, secret-free label
(`env(explicit)`, `config(inline)`, `user_store`, `external_command`, ...)
suitable for `tracing::debug!` lines.

## When to use each `AuthConfig` variant

| Variant | Use when |
|---------|----------|
| `ApiKey { env, value, encrypted_value }` | The provider takes a static API key. `env` overrides the conventional `{PROVIDER}_API_KEY` name. `value` is an explicit inline string (avoid in committed configs). `encrypted_value` is a `v2:`-prefixed ciphertext (encrypted with the master key). All three are optional and the resolver falls through if they're empty. |
| `Stored { account_id }` | The credential should live in the user-level encrypted store and be looked up by provider id (+ optional account id). Best fit for multi-account providers. |
| `ExternalCommand { command, args, timeout_ms }` | The provider documents an officially-supported CLI that issues short-lived credentials. Currently returns `AuthError::Unsupported("ExternalCommand")` from the synchronous resolver; the underlying `ExternalCommandProvider::fetch` uses `std::process::Command` and does not enforce its timeout, which would otherwise let a hanging command stall provider registration. Async plumbing is a follow-up. |
| `OAuthDevice { client_id, scopes, auth_url, token_url }` | Reserved for providers that publish a stable, public device-code/PKCE contract. Not implemented in this pass; the resolver returns `AuthError::Unsupported`. |
| `None` | Explicit "no auth" marker. The resolver returns `Ok(None)` and does not consult env, store, or legacy fields. |

The default is `AuthConfig::ApiKey { env: None, value: None, encrypted_value: None }`,
which behaves identically to "no auth descriptor present" — the resolver
falls through to the conventional env var, then legacy fields, then the
store.

## How to add a new auth mode

1. **Extend the enum** at `src/auth/mod.rs`:

   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
   #[serde(tag = "type", rename_all = "snake_case")]
   pub enum AuthConfig {
       // ...existing variants...
       MyNewMode { /* fields */ },
   }
   ```

   Update `AuthConfig::is_api_key` and `AuthConfig::is_supported` as
   appropriate.

2. **Update `AuthResolver::resolve`** at `src/auth/resolver.rs:100` with a
   new `match` arm. If the new mode reads from the env / store / legacy
   fields, add a `ResolvedAuthSource` variant in the same file and a
   string label in `ResolvedAuthSource::as_str()`. Update
   `conventional_env_map()` if the new mode changes env semantics.

3. **Wire the provider**: in `src/provider/mod.rs`,
   `register_credential_provider` (factories that accept a full
   `Credential` envelope — used for all OpenAI-compatible providers) and
   `register_api_key_provider` (factories that take only the secret —
   used for `minimax` and any provider that cannot accept a bearer
   credential) both go through `AuthResolver`. The centralized
   `resolve_provider_credential(provider_id, cfg, env_var, store)` helper
   builds a `ResolverContext` and calls `AuthResolver::resolve`. The
   helper passes the full `ResolvedAuth` back to the registration helper
   so the credential kind can be preserved. No per-mode changes are
   needed in `register_builtin_with_config` itself; it threads an
   `Arc<CredentialStore>` into every helper so the user store is
   available for `AuthConfig::Stored` lookups.

4. **Add a Source variant** if the new mode produces a new resolution
   path:

   ```rust
   pub enum ResolvedAuthSource {
       // ...
       MyNewModeSource,
   }
   ```

5. **Tests**: use `crate::auth::test_support::lock_env()` to serialize
   tests that flip master-key or env-var state (see "Test support" below).
   Add at least one resolver test for the happy path and one for the
   failure mode (`AuthError::MasterKeyMissing`, `NotFound`, `Unsupported`,
   etc.).

6. **UI**: if the TUI surfaces a `provider.<id>.auth` editor, mirror the
   new variant in the connect dialog at
   `src/tui/components/dialogs/connect.rs` and use `auth::mask_secret`
   for any rendered value.

## Security rules

- **Never log secret prefix/suffix.** Use `auth::mask_secret` for any
  user-facing render of a secret. The helper returns a fixed 16-bullet
  mask (`••••••••••••••••`) regardless of input length and never
  contains a substring of the input — verified by
  `src/auth/credential.rs:91-99`.
- **API keys entered in the TUI** (e.g. via `/connect`) are rendered
  through `mask_secret` while typing, with a non-secret length hint
  appended (e.g. `(42 chars)`) so the user can confirm entry.
  See `src/tui/components/dialogs/connect.rs:328` and `:587`.
- **Master key is required to store.** `CredentialStore::put` and
  `AuthResolver` decryption of `encrypted_value` both return
  `AuthError::MasterKeyMissing` if `CODEGG_MASTER_KEY` /
  `CODEGG_ENCRYPTION_KEY` / `OPENCODE_ENCRYPTION_KEY` is not set. Reading
  plaintext from the store without a master key returns `Ok(None)` (no
  decryption) so env / config paths still work.
- **Resolver `tracing::debug!` lines** use `source.as_str()` (a stable
  label) and never the secret. Do not introduce log lines that include
  the resolved `Credential.secret` field.
- **On-disk file permissions** are set to `0o600` on Unix in
  `CredentialStore::write_to_disk`. The file lives at
  `<config_dir>/codegg/credentials.json` and uses atomic
  write-then-rename semantics.
- **External command** output is treated as a bearer token. Do not log
  stdout / stderr. Treat the command itself as a privilege boundary —
  only point `AuthConfig::ExternalCommand.command` at officially
  supported CLIs. The current synchronous implementation is
  intentionally disabled from `AuthResolver::resolve` because the
  `std::process::Command`-based `ExternalCommandProvider::fetch` does
  not enforce its timeout. When async timeout plumbing lands, this
  arm will be re-enabled and the warning will be removed.

## Test support

`crate::auth::test_support::env_lock()` exposes a single, cross-module
mutex that serializes tests mutating master-key and API-key env vars.
Acquire it at the top of any test that flips
`CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY` / `OPENCODE_ENCRYPTION_KEY`
or `OPENAI_API_KEY`:

```rust
#[test]
fn my_resolver_test() {
    let _guard = crate::auth::test_support::lock_env();

    // ...flip env vars...
    // ...run resolver...
    // ...restore env vars (or use a Drop guard)...
}
```

Using a unique provider id (e.g. `"code_legacy_only_provider"`) and
unsetting `OPENAI_API_KEY` in the test avoids cross-test pollution even
without the lock, but the lock is the safest default.

`CredentialStore` tests use `tempfile::tempdir()` and the same
`lock_env()` guard to keep the master-key state serialized.

## CLI surface (`codegg auth ...`)

The `auth::cli::AuthCli` struct backs the `codegg auth` subcommand. The
CLI is intentionally minimal — TUI flows can build on the same
`CredentialStore` API for richer behavior.

```text
codegg auth status                    # list stored credentials (no plaintext)
codegg auth set-key openai            # read key from stdin, store under default account
codegg auth set-key openai --account work
codegg auth logout openai             # remove default-account record
codegg auth logout openai --account '*'    # remove all accounts for the provider
```

`set-key` requires `CODEGG_MASTER_KEY` to be set; if it is missing the
command returns a `Config(Invalid("no master key configured..."))`
error. The default input path is `read_key_from_stdin` (trims trailing
newlines), so a `readline`-style prompt can be wired in later without
changing the public API.

## Architecture reference

A detailed architecture document at `architecture/auth.md` is forthcoming.
For now, the closest existing documents are:

- `architecture/crypto.md` — the underlying `crypto::encrypt_to_string` /
  `decrypt_from_string` primitives that back both
  `auth::store::CredentialStore` and `provider.<id>.auth.encrypted_value`.
- `architecture/provider.md` — how `register_credential_provider` /
  `register_api_key_provider` / `register_config_provider` route
  through `AuthResolver` and the `resolve_provider_credential` helper.
- `plans/oauth_provider_auth_followup.md` — the plan that drove this
  pass (ExternalCommand safety, credential-kind preservation,
  `codegg auth ...` CLI, store-backed registration tests).
