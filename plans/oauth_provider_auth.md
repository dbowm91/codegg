# OAuth and Account Credential Provider Plan

## Goal

Add a conservative, provider-agnostic credential layer to codegg so providers can use API keys today and short-lived bearer/OAuth-style credentials later without rewriting every provider. This first pass should not attempt to reverse-engineer SuperGrok, Claude, ChatGPT, Copilot, or any other consumer-app session. It should build the local auth substrate, preserve the current API-key flow, add better model metadata for xAI/Grok Build through the existing API-key provider path, and make future official OAuth/device-code integrations straightforward.

The intended outcome is that codegg can represent these auth modes cleanly:

- static API key from environment/config, preserving current behavior;
- encrypted user-level API key storage, avoiding project-local secrets where possible;
- bearer token material resolved at request time, so expiring credentials can be refreshed later;
- explicitly supported device-code/PKCE providers when there is a stable public contract;
- external-command credential providers for official CLIs if a provider documents that flow.

Out of scope for this pass: scraping app cookies, reusing opaque consumer subscription tokens, reverse-engineering private OAuth clients, bypassing provider API billing, or shipping provider-specific auth that depends on undocumented endpoints.

## Current Repo Observations

The provider layer is already centralized around `src/provider/mod.rs`. The `Provider` trait exposes `stream`, `models`, `discover_models`, and `ping`, and `ProviderRegistry` owns provider registration. This is a good seam for introducing auth without changing the agent loop.

Provider registration is currently API-key centric. `register_builtin` reads environment variables such as `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `XAI_API_KEY`, and `MINIMAX_API_KEY`. `register_builtin_with_config` similarly reads `ProviderConfig.api_key` or falls back to env vars through `register_env_fallback_provider`.

`src/config/schema.rs` currently models provider secrets as `api_key`, `encrypted_api_key`, and `encrypted`, plus provider transport/model options such as `base_url`, `timeout`, `models`, and `options`. This means there is no typed way to represent `auth.type`, `access_token`, `refresh_token`, `expires_at`, `scopes`, `token_url`, or an external command credential source.

`src/config/encryption.rs` already encrypts and decrypts provider API keys using the existing master key path. This can be reused for a first token store, but long-term OAuth refresh tokens should not live in project config files. The first implementation should add a user-level credential store under the codegg config directory and keep project config limited to provider selection and non-secret auth hints.

`src/provider/openai_compatible.rs` currently stores `api_key: String` directly in `OpenAiCompatibleConfig` and sends `Authorization: Bearer {api_key}` in both chat and model-list calls. This is the highest-leverage refactor point because it covers xAI, Groq, DeepInfra, Cerebras, Cohere, Together, Perplexity, Venice, GeneralCompute, OpenCode Go, and several config-only OpenAI-compatible endpoints.

`src/provider/additional.rs` already has `create_xai(api_key)` using `https://api.x.ai/v1`. That is the right integration path for Grok Build as currently exposed by xAI API docs. Do not add `supergrok_oauth` unless xAI documents a third-party account-auth flow. For this pass, add or verify model metadata for Grok Build under the xAI provider while preserving `XAI_API_KEY` as the credential path.

`src/tui/components/dialogs/connect.rs` has a provider connect dialog, but it is API-key-only. `ProviderInfo` has `requires_api_key` and the dialog only has `SelectProvider` and `EnterApiKey` steps. This should eventually become an auth-method-aware connect flow, but do not couple the first core auth refactor to a large TUI rewrite.

There is some partial secret logging in provider registration and MiniMax/OpenAI-compatible request paths: key length plus visible prefix/suffix. That should be removed or replaced before adding refresh-token support. OAuth tokens and refresh tokens must never have prefix/suffix logged.

## Design Direction

Introduce a new `src/auth/` module that owns credential representation and resolution. Providers should request a header value or credential from this module at request time rather than storing static secret strings forever.

Suggested file layout:

```text
src/auth/
  mod.rs
  credential.rs
  resolver.rs
  store.rs
  error.rs
  external.rs
  oauth.rs        # scaffolding only in first pass
```

Suggested core types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    ApiKey {
        env: Option<String>,
        value: Option<String>,
        encrypted_value: Option<String>,
    },
    Stored {
        account_id: Option<String>,
    },
    ExternalCommand {
        command: String,
        args: Vec<String>,
        timeout_ms: Option<u64>,
    },
    OAuthDevice {
        client_id: String,
        scopes: Vec<String>,
        auth_url: String,
        token_url: String,
    },
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CredentialKind {
    ApiKey,
    BearerToken,
}

#[derive(Debug, Clone)]
pub struct Credential {
    pub kind: CredentialKind,
    pub secret: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Credential {
    pub fn authorization_header_value(&self) -> String {
        match self.kind {
            CredentialKind::ApiKey | CredentialKind::BearerToken => format!("Bearer {}", self.secret),
        }
    }
}
```

Keep the exact shape flexible if the repo style suggests better naming, but the important design invariant is that providers should not need to know whether a secret came from env, config, encrypted user storage, a future OAuth refresh, or an official CLI command.

## Implementation Tasks

### 1. Add auth module scaffolding

Create `src/auth/mod.rs`, `credential.rs`, `resolver.rs`, `store.rs`, `external.rs`, and `error.rs`.

The first pass only needs fully working API-key resolution plus structure for stored and external credentials. OAuth/device-code can be a typed config variant with `Unsupported` or `NotConfigured` errors until a provider-specific official flow is added.

`AuthResolver` should support this priority order for API-key-compatible providers:

1. explicit env var named by `AuthConfig::ApiKey.env`;
2. conventional env var `{PROVIDER}_API_KEY` for backward compatibility;
3. explicit `AuthConfig::ApiKey.value`;
4. encrypted value from config;
5. user-level credential store lookup;
6. legacy `ProviderConfig.api_key` fallback.

This keeps current config/env behavior working while introducing the new path.

### 2. Extend config schema without breaking current configs

Update `ProviderConfig` in `src/config/schema.rs` to add:

```rust
pub auth: Option<AuthConfig>,
pub account_id: Option<String>,
```

Import the new type as `crate::auth::AuthConfig` or define a config-layer mirror type if avoiding cross-module coupling is preferred.

Update `ProviderConfig::merge()` so `auth` and `account_id` merge field-by-field using the existing pattern: if the override field is `Some`, replace the base field.

Do not remove `api_key`, `encrypted_api_key`, or `encrypted`. Treat them as legacy aliases until a later migration pass.

Update config docs and `codegg.example.jsonc` with examples:

```jsonc
{
  "provider": {
    "xai": {
      "auth": { "type": "api_key", "env": "XAI_API_KEY" }
    },
    "openai": {
      "auth": { "type": "api_key", "env": "OPENAI_API_KEY" }
    }
  }
}
```

Also include a warning that account/OAuth auth is only for officially supported provider flows.

### 3. Add a user-level credential store

Add `src/auth/store.rs` with a small encrypted JSON store. Suggested location:

```text
~/.config/codegg/credentials.json
```

or the platform equivalent already used by config path helpers. Do not place this under `.codegg/` inside a project.

Suggested stored representation:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredentialRecord {
    pub provider_id: String,
    pub account_id: Option<String>,
    pub kind: CredentialKind,
    pub encrypted_secret: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Use the existing crypto helpers for encryption in the first pass. If no master key is configured, return a clear error for storing new credentials rather than silently writing plaintext. Reading env/config API keys should still work without a master key.

Add tests with temp directories where possible. Do not test against the real user config dir.

### 4. Refactor OpenAI-compatible provider auth

Change `OpenAiCompatibleConfig` in `src/provider/openai_compatible.rs` so it no longer assumes a static `api_key: String` is the only credential source.

Minimal safe first-pass option:

```rust
pub struct OpenAiCompatibleConfig {
    pub credential: Credential,
    pub base_url: String,
    pub auth_header: String,
    pub extra_headers: Vec<(String, String)>,
    pub models: Vec<ModelInfo>,
    pub tool_choice: ToolChoice,
}
```

This still uses a resolved static credential at provider construction time, but it moves the code off a raw `api_key` field and prepares for a future `CredentialProvider`/closure/Arc resolver. If implementing request-time resolution is easy, prefer:

```rust
pub credential_source: Arc<dyn CredentialSource>,
```

where `CredentialSource::credential()` can refresh expiring tokens later.

Update request sending and model discovery to call `credential.authorization_header_value()`.

Make `OpenAiCompatibleProvider::simple(id, name, api_key, base_url)` continue to exist for existing provider factory compatibility.

### 5. Refactor provider registry around auth resolution

Update `register_config_provider` and `register_env_fallback_provider` to use the new resolver while preserving current behavior.

Desired helper shape:

```rust
fn resolve_provider_credential(
    name: &str,
    env_var: &str,
    cfg: Option<&ProviderConfig>,
) -> Option<Credential>
```

Use this helper in `register_builtin_with_config` for OpenAI-compatible providers first.

Keep `register_builtin` as a compatibility path, but consider implementing it in terms of the same helper with `None` config to avoid drift.

### 6. Remove secret prefix/suffix logging

Replace all key prefix/suffix logs with one of these:

- no secret-derived data at all; preferred;
- a short stable fingerprint, e.g. `sha256(secret)[0..8]`, only under an explicit diagnostic environment variable.

Specific places to fix:

- `register_env_fallback_provider` in `src/provider/mod.rs`;
- `OpenAiCompatibleProvider::stream` in `src/provider/openai_compatible.rs`;
- `create_minimax` in `src/provider/additional.rs`.

Do not display entered API keys in plaintext in TUI. The connect dialog currently truncates long keys but still displays secret material. Replace with masked rendering such as `••••••••` plus length or last four only if explicitly needed. For safest behavior, render only a fixed mask while input length is nonzero.

### 7. Add xAI/Grok Build metadata through API-key path

Keep provider id `xai` and env var `XAI_API_KEY`.

Update `create_xai` to use explicit model metadata rather than relying only on dynamic `/models` discovery. Include Grok Build model metadata if the model id is confirmed in the docs at implementation time. At the time this plan was written, the relevant target is `grok-build-0.1`, with xAI OpenAI-compatible base URL `https://api.x.ai/v1`.

Suggested implementation:

```rust
pub fn create_xai(api_key: String) -> impl Provider {
    let models = vec![
        ModelInfo {
            id: "grok-build-0.1".to_string(),
            name: "Grok Build 0.1".to_string(),
            provider: "xai".to_string(),
            context_window: 256_000,
            max_output_tokens: None,
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        // Keep existing general Grok models only if already present in current catalog/docs.
    ];
    OpenAiCompatibleProvider::new(
        "xai",
        "xAI",
        OpenAiCompatibleConfig { models, .. }
    )
}
```

Adjust to the actual constructor after the auth refactor. Do not add `supergrok` as a provider alias unless there is a documented API-compatible endpoint or official account auth flow.

### 8. Add CLI/TUI-facing auth commands or messages lightly

If the CLI command structure is easy to extend, add:

```text
codegg auth status
codegg auth set-key <provider>
codegg auth logout <provider>
```

If CLI routing is not currently clean, skip the command implementation and add only auth module APIs plus a follow-up TODO. Do not block the core refactor on TUI/CLI polish.

For the TUI connect dialog, minimally update `ProviderInfo` to make future auth choices possible:

```rust
pub enum ProviderAuthMode {
    ApiKey,
    OAuthDevice,
    ExternalCommand,
    None,
}
```

Then replace `requires_api_key: bool` with `auth_modes: Vec<ProviderAuthMode>` or add the new field while preserving the old one temporarily. First pass can still route all current providers to API-key entry.

### 9. Tests

Add focused unit tests rather than broad integration tests:

- `ProviderConfig` parses both legacy `api_key` and new `auth` config.
- `ProviderConfig::merge()` preserves base fields and overrides `auth` correctly.
- `AuthResolver` resolves env over config.
- `AuthResolver` falls back to legacy `api_key`.
- encrypted user credential store round-trips when `CODEGG_MASTER_KEY` is set.
- OpenAI-compatible provider builds `Authorization: Bearer ...` from `Credential`.
- secret masking helper never returns the raw key, prefix, or suffix.

Run:

```text
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

If the full clippy suite is currently noisy, document existing unrelated failures in the implementation summary rather than papering over them.

## Acceptance Criteria

The first pass is complete when:

- existing `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `XAI_API_KEY`, and other provider env var flows still work;
- existing config files with `provider.<id>.api_key` still work;
- `provider.<id>.auth.type = "api_key"` works for at least OpenAI-compatible providers;
- OpenAI-compatible providers no longer hard-code an `api_key` field as the only auth primitive;
- secret prefix/suffix logging is removed from provider registration and request paths;
- entered API keys are masked in the TUI connect dialog;
- xAI includes Grok Build metadata if the model id remains current in official docs;
- no undocumented SuperGrok/consumer-session auth is implemented;
- tests cover config parsing, auth resolution, store round-trip, and masking.

## Follow-up Passes

A later pass can add official OAuth/device-code providers once there is a documented public contract. The likely candidates are OpenAI/Codex official auth, GitHub OAuth for repo/account integration, and any xAI account auth flow if xAI publishes one. Claude/Claude Code account auth should remain API-key or official-tool-mediated unless Anthropic documents supported third-party account usage.

A later pass should replace the file-based encrypted credential store with OS keychain support where practical. Evaluate crates such as `keyring` after deciding how much platform-specific behavior codegg is willing to carry.

A later pass should add a richer TUI flow: auth method selection, device-code display, browser-open prompt, polling status, account status, logout, and token-expiry warnings.

A later pass should add request-time credential refresh via `Arc<dyn CredentialSource>` instead of provider-construction-time credential snapshots. This is required before real OAuth refresh tokens are useful.
