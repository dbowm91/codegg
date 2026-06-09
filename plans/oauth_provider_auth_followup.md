# OAuth Provider Auth Follow-up Plan

## Goal

Finish the second pass of the provider auth work. The first pass added the auth module, typed config, an encrypted credential store, OpenAI-compatible `Credential` plumbing, xAI/Grok Build metadata, secret masking, and a TUI auth-mode scaffold. This follow-up should make the new substrate actually usable end-to-end while preserving current API-key behavior.

The main targets are:

- wire the user credential store into provider registration;
- stop collapsing resolved credentials back into raw strings where credential kind matters;
- make OpenAI-compatible factories accept `Credential` directly;
- keep legacy `api_key` and env-var flows working;
- make `ExternalCommand` safe or explicitly unsupported until safe;
- add tests that exercise stored credentials and credential-kind preservation;
- update docs/examples so users can configure `auth` without guessing.

Do not implement consumer-session SuperGrok, Claude, ChatGPT, Copilot, or other app-token reuse. OAuth/device-code should remain typed scaffolding unless a provider has a documented public contract.

## Current State Summary

`src/auth/` exists and is exported from `src/lib.rs`. It includes `AuthConfig`, `Credential`, `CredentialKind`, `AuthResolver`, `CredentialStore`, `ExternalCommandProvider`, and OAuth scaffolding.

`ProviderConfig` has `auth: Option<AuthConfig>` and `account_id: Option<String>` and merges those fields correctly.

`OpenAiCompatibleConfig` now has `credential: Credential` rather than `api_key: String`, and request/model-list calls use `credential.authorization_header_value()`.

`register_config_provider` and `register_env_fallback_provider` resolve credentials through `AuthResolver`, but then pass only `resolved.credential.secret` into legacy factory closures. This loses `CredentialKind`, `expires_at`, and future refresh metadata.

`ResolverContext` has `store: Option<Arc<CredentialStore>>`, but provider registration currently constructs contexts without a store. As a result, `AuthConfig::Stored` is parsed but not practically usable through provider registration.

`ExternalCommandProvider` accepts `timeout_ms` but uses blocking `std::process::Command` and does not enforce the timeout. This can hang registration if a command blocks.

The TUI connect dialog masks API keys and has `ProviderAuthMode`, but current flow remains API-key-only, which is acceptable for this pass.

## Phase 1: Centralize Credential Resolution for Provider Registration

Create a single helper in `src/provider/mod.rs` or a small helper module such as `src/provider/auth.rs`.

Suggested shape:

```rust
fn resolve_provider_credential(
    provider_id: &str,
    env_var: Option<&str>,
    cfg: Option<&crate::config::schema::ProviderConfig>,
    store: Option<Arc<crate::auth::CredentialStore>>,
) -> Result<Option<crate::auth::ResolvedAuth>, crate::auth::AuthError>
```

Behavior:

1. Build `ResolverContext` with `provider_id`, `account_id`, legacy `api_key`, and the credential store.
2. If `env_var` is provided, set `env_override` to preserve provider-specific env names such as `TOGETHERAI_API_KEY`.
3. Call `AuthResolver::resolve(cfg.and_then(|c| c.auth.as_ref()), &ctx)`.
4. Return the full `ResolvedAuth`, not just the secret string.

Use this helper from both `register_config_provider` and `register_env_fallback_provider`.

Important: do not swallow all resolver errors silently. For unsupported OAuth, log a clear warning and skip the provider. For malformed encrypted credentials or external-command failures, log the provider id and source but never log secret material.

## Phase 2: Wire the User Credential Store into Registration

In `register_builtin_with_config`, create a credential store once near the top:

```rust
let credential_store = crate::auth::CredentialStore::at_default_location()
    .map(Arc::new)
    .map_err(|e| { tracing::warn!(...); e })
    .ok();
```

Pass `credential_store.clone()` into every provider registration helper.

Do not fail all provider registration if the credential store cannot be created. Env/config API keys should still work. If the store is unavailable, only `AuthConfig::Stored` should fail with a clear warning.

Add a test that constructs a temp `CredentialStore`, writes an API key to it, and verifies `resolve_provider_credential` can resolve `AuthConfig::Stored`. If direct testing of a private helper is awkward, expose the helper as `pub(crate)` and test inside `provider::tests`.

## Phase 3: Preserve CredentialKind Through Provider Factories

Change helper signatures from `String` to `Credential` for OpenAI-compatible providers.

Current pattern:

```rust
F: FnOnce(String) -> Box<dyn Provider>
```

Target pattern for OpenAI-compatible providers:

```rust
F: FnOnce(crate::auth::Credential) -> Box<dyn Provider>
```

For providers that truly require a raw API key because they use custom headers or SDK-specific fields, keep a separate legacy helper or convert explicitly after validating `credential.kind`.

Recommended split:

- `register_credential_provider` for providers that can accept a full `Credential`;
- `register_api_key_provider` for providers that only accept static API-key secrets.

For `register_api_key_provider`, reject `CredentialKind::BearerToken` unless the provider explicitly accepts bearer tokens as static API keys. Log:

```text
provider '<id>' resolved a bearer token but this provider path only supports API-key credentials; skipping
```

For OpenAI-compatible providers, accept both `ApiKey` and `BearerToken` because both are represented as `Authorization: Bearer ...`.

## Phase 4: Add Credential-based OpenAI-compatible Constructors

In `src/provider/openai_compatible.rs`, add a constructor that takes `Credential`:

```rust
pub fn simple_with_credential(
    id: &str,
    name: &str,
    credential: Credential,
    base_url: &str,
) -> Self
```

Keep existing `simple(id, name, api_key, base_url)` as a backwards-compatible wrapper:

```rust
pub fn simple(...) -> Self {
    Self::simple_with_credential(id, name, Credential::api_key(api_key), base_url)
}
```

In `src/provider/additional.rs`, add credential-based constructors for OpenAI-compatible providers where practical:

```rust
pub fn create_xai_with_credential(credential: Credential) -> impl Provider
pub fn create_groq_with_credential(credential: Credential) -> impl Provider
pub fn create_mistral_with_credential(credential: Credential) -> impl Provider
...
```

Keep old `create_xai(api_key: String)` wrappers to preserve existing calls.

Refactor `register_env_fallback_provider` calls for OpenAI-compatible providers to use the credential-based constructors so `ExternalCommand` bearer tokens and stored credential metadata are not immediately flattened.

## Phase 5: Decide ExternalCommand Safety

The current `ExternalCommandProvider::fetch` computes a timeout but does not enforce it. Fix this before considering `ExternalCommand` supported.

Option A, preferred: make external command resolution async.

- Use `tokio::process::Command`.
- Use `tokio::time::timeout` around `wait_with_output()`.
- Return `AuthError::ExternalCommand` on timeout.
- Consider killing the child process on timeout if feasible.
- This may require making `AuthResolver::resolve` async. If that is too invasive, add a separate `resolve_async` and do not route provider registration through external commands until async plumbing is available.

Option B, safer short-term: mark `ExternalCommand` unsupported in `AuthResolver::resolve`.

- Return `AuthError::Unsupported("ExternalCommand requires async timeout plumbing")`.
- Keep the type and docs, but do not execute external commands from provider registration.
- Leave a follow-up plan for CLI-mediated credential import.

Do not keep the current behavior where a blocking command can hang registration indefinitely.

Also make existing external-command tests cross-platform. The current tests use `printf` and `false`, which are Unix-oriented. Either gate them with `#[cfg(unix)]` or rewrite them around a test helper binary.

## Phase 6: Improve Stored Credential UX Without Large TUI Scope

Add minimal CLI or internal functions for credential-store management. If the CLI command layout is easy to extend, implement:

```text
codegg auth status
codegg auth set-key <provider>
codegg auth logout <provider>
```

Minimum acceptable implementation for this pass:

- `auth status` lists providers/accounts from `CredentialStore::list()` without plaintext;
- `auth logout <provider>` removes stored credentials for that provider;
- `auth set-key <provider>` reads a key securely if a hidden-input crate already exists, or accepts a `--env`/stdin path if no hidden-input dependency should be added yet.

If CLI integration is too large, add public crate APIs plus tests and document that CLI/TUI management remains follow-up. Do not block provider registration from reading stored credentials.

## Phase 7: Docs and Examples

Update these files if present:

- `architecture/provider.md`
- `architecture/config.md`
- `codegg.example.jsonc`
- README provider/config section, if it mentions only `api_key`.

Document legacy and new config forms:

```jsonc
{
  "provider": {
    "xai": {
      "auth": { "type": "api_key", "env": "XAI_API_KEY" }
    },
    "openai": {
      "auth": { "type": "stored", "account_id": "default" }
    }
  }
}
```

Document clearly:

- `api_key` is still supported for backward compatibility;
- stored credentials live in the user config directory, not project config;
- storing credentials requires `CODEGG_MASTER_KEY` or another supported master-key env var;
- OAuth/device-code is parsed but unsupported unless a provider-specific official flow is added;
- SuperGrok/app subscription auth is intentionally not implemented.

## Phase 8: Tests

Add or update tests for these exact cases:

1. `ProviderConfig` parses `auth: { type: "api_key", env: "XAI_API_KEY" }`.
2. `ProviderConfig` parses `auth: { type: "stored", account_id: "default" }`.
3. `ProviderConfig::merge()` overrides `auth` and `account_id` while preserving unrelated fields.
4. `resolve_provider_credential` resolves a stored credential when supplied a temp `CredentialStore`.
5. `resolve_provider_credential` preserves `CredentialKind::BearerToken` for `ExternalCommand` if external commands remain enabled.
6. OpenAI-compatible credential-based constructor sends `Authorization: Bearer ...` for both `ApiKey` and `BearerToken` credentials.
7. API-key-only provider registration rejects bearer credentials with a warning or explicit error path.
8. Existing env-var registration still works for `XAI_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and `MINIMAX_API_KEY`.
9. TUI API-key rendering never includes prefix/suffix/plaintext.
10. External command timeout behavior is either enforced or the variant returns `Unsupported`.

Run:

```text
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

If clippy has unrelated pre-existing failures, record them in the implementation summary with file paths and messages.

## Acceptance Criteria

This follow-up is complete when:

- `AuthConfig::Stored` works through provider registration when a user credential exists;
- provider registration passes a real `CredentialStore` into `AuthResolver`;
- OpenAI-compatible provider registration can preserve `CredentialKind::BearerToken` and `expires_at` metadata at construction time;
- raw-string provider factories are limited to providers that truly need static API-key strings;
- external commands cannot hang provider registration indefinitely;
- no provider logs secret prefix/suffix/key length or user-entered key material;
- old env var and legacy config `api_key` flows still work;
- xAI/Grok Build continues to register through `XAI_API_KEY` or equivalent auth config;
- docs show both legacy and new auth config forms;
- tests cover stored credentials, constructor credential-kind preservation, and external-command timeout/unsupported behavior.

## Non-goals

Do not add undocumented account-token flows.

Do not add `supergrok` as a distinct provider unless xAI publishes a stable third-party API/auth surface for SuperGrok account usage.

Do not remove legacy `api_key` config yet.

Do not migrate existing users' config automatically in this pass.

Do not introduce OS keychain support in this pass unless it is trivially isolated behind the same `CredentialStore` trait. File-based encrypted storage is sufficient for now.
