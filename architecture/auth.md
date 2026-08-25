# Auth Module Architecture

The `auth` module is the central seam where providers obtain
credentials. It owns the typed `AuthConfig`, the resolved `Credential`,
the `AuthResolver` (priority-based env → config → store lookup), a
user-level encrypted `CredentialStore`, disabled scaffolding for
`ExternalCommandProvider` and OAuth device flow, and a `cli` sub-module
that wires the credential store into `codegg auth status | set-key |
logout`.

## Purpose

Resolve provider credentials through a priority-based lookup chain
(env vars → config inline → encrypted config → user store → legacy
fields). Store and retrieve encrypted API keys via a master-key-backed
credential store. Provide CLI commands for credential management.

## Where It Lives

| Artifact | Location |
|----------|----------|
| Core auth types (`AuthConfig`, `Credential`, `CredentialKind`, `CredentialStore`, `AuthResolver`, `AuthError`) | `crates/codegg-providers/src/auth_types.rs` |
| Re-export for backward compat | `src/auth/mod.rs` |
| CLI (`AuthCli`, `read_key_from_stdin`) | `src/auth/cli.rs` |
| `ExternalCommandProvider`, `ExternalCredential` | `crates/codegg-providers/src/auth_types.rs` |
| OAuth scaffolding (disabled) | `src/auth/oauth.rs` |
| Test support (`env_lock`, `lock_env`) | `src/auth/mod.rs` (test_support) and `crates/codegg-providers/src/auth_types.rs` (test_support) |
| Crypto primitives | `crates/codegg-providers/src/crypto.rs` |
| Master key retrieval | `codegg_config::encryption::get_master_key()` |

> **Note:** Core auth types now live in `codegg-providers`. Root
> `src/auth/mod.rs` re-exports them from
> `codegg_providers::auth_types`.

## How It Works

### Resolution Priority Order

For `AuthConfig::ApiKey { env, value, encrypted_value }`, the
resolver tries each step and returns the first hit:

1. `ctx.env_override` (test-only) or `AuthConfig::ApiKey.env` env var
   → `EnvExplicit`
2. Conventional env var `{PROVIDER}_API_KEY` (provider id uppercased,
   `-` → `_`) → `EnvConventional`
3. `AuthConfig::ApiKey.value` (inline, non-empty) → `InlineValue`
4. `AuthConfig::ApiKey.encrypted_value`, decrypted with master key
   → `EncryptedConfig` (returns `MasterKeyMissing` if no key)
5. User-level `CredentialStore` lookup (provider id + optional account
   id), filtered to `kind == ApiKey` → `UserStore`
6. Legacy `ProviderConfig::api_key` (post-decryption) → `LegacyApiKey`
7. Legacy `ProviderConfig::encrypted_api_key` already decrypted
   → `LegacyDecrypted`

For **no auth** (no `AuthConfig` or `AuthConfig::None`):
1. `ctx.env_override` → `EnvExplicit`
2. Conventional env var → `EnvConventional`
3. `ctx.legacy_api_key` → `LegacyApiKey`
4. `ctx.legacy_decrypted` → `LegacyDecrypted`
5. User-level `CredentialStore` → `UserStore`

`AuthConfig::Stored { account_id }` skips straight to `UserStore`.

`AuthConfig::ExternalCommand` → `AuthError::Unsupported`
`AuthConfig::OAuthDevice` → `AuthError::Unsupported`
`AuthConfig::None` → `Ok(None)`

> **Stored bearer tokens are not yet supported.** Both the
> `AuthConfig::Stored` arm and the no-auth fallback's store lookup
> filter to `CredentialKind::ApiKey`. A stored `BearerToken` record is
> treated as a miss.

### Provider Registration

Three helpers in `crates/codegg-providers/src/provider/mod.rs`:

- **`register_credential_provider`** — factories accepting a full
  `Credential` envelope. Used for OpenAI-compatible providers (mistral,
  groq, deepinfra, cerebras, cohere, together, perplexity, xai,
  venice, opencode_go, generalcompute).
- **`register_api_key_provider`** — factories taking only the secret
  string. Used for opencode_zen and minimax (Anthropic-compatible).
  Rejects `CredentialKind::BearerToken`.
- **`register_config_provider`** — base-URL-aware variant for
  anthropic, openai (native), google, openrouter. Threads resolved
  secret + `cfg.base_url` to factory closure.

All three call `resolve_provider_credential(provider_id, cfg, env_var,
store)` which builds a `ResolverContext` and returns `ResolvedAuth`.
This is the **single resolution path** for provider registration.

`register_builtin` (env-var-only, no config) wraps each key in
`Credential::api_key(...)`. Used as last-resort fallback when
config-aware path registers zero providers.

### Security Rules

- **Never log secret prefix/suffix.** `mask_secret()` returns a fixed
  16-bullet mask (`••••••••••••••••`) regardless of input length.
  Empty secrets return empty string.
- **Master key required to store.** `CredentialStore::put` and
  `AuthResolver` decryption both return `MasterKeyMissing` if no key
  is configured.
- **Resolver `tracing::debug!` lines** use `source.as_str()` (a stable
  label like `"env(explicit)"`, `"config(inline)"`) and never the
  secret.
- **On-disk file permissions** are `0o600` on Unix. Atomic
  write-then-rename via `.tmp-{uuid}` intermediate file.
- **External command** is disabled. Both `AuthResolver::resolve` and
  `ExternalCommandProvider::fetch` return `AuthError::Unsupported` for
  any non-empty command. No synchronous shell-out path is reachable.
- **`codegg auth` CLI validation.** Provider and account ids validated
  to `[A-Za-z0-9_-]`. `*` accepted only by `logout`. `set-key` never
  echoes key material.

## Key Types & APIs

### AuthConfig (`auth_types.rs:121`)

```rust
pub enum AuthConfig {
    ApiKey { env: Option<String>, value: Option<String>, encrypted_value: Option<String> },
    Stored { account_id: Option<String> },
    ExternalCommand { command: String, args: Vec<String>, timeout_ms: Option<u64> },
    OAuthDevice { client_id: String, scopes: Vec<String>, auth_url: String, token_url: String },
    None,
}
```

### Credential (`auth_types.rs:61`)

```rust
pub struct Credential {
    pub kind: CredentialKind,           // ApiKey | BearerToken
    pub secret: String,
    pub expires_at: Option<DateTime<Utc>>,
}
```

`Debug` impl masks the secret via `mask_secret()`.
`authorization_header_value()` returns `Bearer {secret}` for both kinds.

### CredentialKind (`auth_types.rs:52`)

```rust
pub enum CredentialKind { ApiKey, BearerToken }
```

### AuthError (`auth_types.rs:14`)

```rust
pub enum AuthError {
    NotFound(String),
    Expired(String),
    MasterKeyMissing,
    Crypto(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Unsupported(String),
    Invalid(String),
    ExternalCommand { command: String, message: String },
}
```

### AuthResolver (`auth_types.rs:238`)

```rust
pub struct AuthResolver { external: ExternalCommandProvider }

impl AuthResolver {
    pub fn resolve(
        &self,
        auth: Option<&AuthConfig>,
        ctx: &ResolverContext,
    ) -> Result<Option<ResolvedAuth>, AuthError>;
}
```

### ResolverContext (`auth_types.rs:195`)

```rust
pub struct ResolverContext {
    pub provider_id: String,
    pub account_id: Option<String>,
    pub legacy_api_key: Option<String>,
    pub legacy_decrypted: Option<String>,
    pub store: Option<Arc<CredentialStore>>,
    pub env_override: Option<String>,  // test-only
}
```

### ResolvedAuth (`auth_types.rs:205`)

```rust
pub struct ResolvedAuth {
    pub credential: Credential,
    pub source: ResolvedAuthSource,
}
```

### ResolvedAuthSource (`auth_types.rs:211`)

```rust
pub enum ResolvedAuthSource {
    EnvExplicit,      // "env(explicit)"
    EnvConventional,  // "env(conventional)"
    InlineValue,      // "config(inline)"
    EncryptedConfig,  // "config(encrypted)"
    UserStore,        // "user_store"
    LegacyApiKey,     // "legacy(api_key)"
    LegacyDecrypted,  // "legacy(decrypted)"
    ExternalCommand,  // "external_command"
}
```

### CredentialStore (`auth_types.rs:437`)

```rust
pub struct CredentialStore {
    path: PathBuf,
    records: Mutex<Vec<StoredCredentialRecord>>,
}
```

Key methods:
- `at_default_location()` — opens `~/.config/codegg/credentials.json`
- `put(provider_id, account_id, kind, secret, expires_at, scopes)` —
  encrypts with master key, requires `CODEGG_MASTER_KEY`
- `get_plaintext(provider_id, account_id, predicate)` — decrypts on
  demand; returns `Ok(None)` without master key
- `remove(provider_id, account_id)` — `Some("*")` removes all accounts
- `list()` — returns all records (metadata only)

### StoredCredentialRecord (`auth_types.rs:417`)

```rust
pub struct StoredCredentialRecord {
    pub provider_id: String,
    pub account_id: Option<String>,
    pub kind: CredentialKind,
    pub encrypted_secret: String,  // v2:-prefixed ciphertext
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### ExternalCommandProvider (`auth_types.rs:176`)

```rust
pub struct ExternalCommandProvider;

impl ExternalCommandProvider {
    pub fn fetch(&self, cred: &ExternalCredential) -> Result<Credential, AuthError> {
        // Returns AuthError::Unsupported for any non-empty command
    }
}
```

### Test Support

```rust
// src/auth/mod.rs
pub mod test_support {
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    pub fn lock_env() -> MutexGuard<'static, ()>;
}
```

Cross-module mutex that serializes tests mutating `CODEGG_MASTER_KEY`,
`CODEGG_ENCRYPTION_KEY`, `OPENCODE_ENCRYPTION_KEY`, `OPENAI_API_KEY`.

## Configuration Surface

### Provider Auth Config

```toml
[provider.openai.auth]
type = "api_key"
env = "OPENAI_API_KEY"          # explicit env var name
value = "sk-..."                 # inline (non-empty)
encrypted_value = "v2:..."       # encrypted with master key

[provider.openai.auth]
type = "stored"
account_id = "work"              # optional account selector

[provider.openai.auth]
type = "external_command"
command = "some-cli"
args = ["get-api-key"]
timeout_ms = 5000                # not enforced (disabled)

[provider.openai.auth]
type = "oauth_device"            # not implemented
client_id = "..."
auth_url = "..."
token_url = "..."

[provider.openai.auth]
type = "none"                    # no auth needed
```

### Master Key

```bash
# Required for storing new credentials and decrypting encrypted_value
export CODEGG_MASTER_KEY="your-master-key"
# Also checked (legacy):
export CODEGG_ENCRYPTION_KEY="..."
export OPENCODE_ENCRYPTION_KEY="..."
```

### CLI Usage

```text
codegg auth status                    # list stored credentials (metadata only)
codegg auth set-key openai            # read key from stdin, store under default account
codegg auth set-key openai --account work
codegg auth logout openai             # remove default-account record
codegg auth logout openai --account '*'    # remove all accounts for the provider
```

### Provider Registration

```toml
[provider.anthropic]
base_url = "https://api.anthropic.com"
# auth resolved via Anthropic auth config or env var

[provider.openai]
base_url = "https://api.openai.com/v1"
# auth resolved via OpenAI auth config or env var
```

Adding ANY provider via config disables all env-var auto-registration.
Env vars are: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY`,
`OPENROUTER_API_KEY`, `MISTRAL_API_KEY`, `GROQ_API_KEY`,
`DEEPINFRA_API_KEY`, `CEREBRAS_API_KEY`, `COHERE_API_KEY`,
`TOGETHERAI_API_KEY`, `PERPLEXITY_API_KEY`, `XAI_API_KEY`,
`VENICE_API_KEY`, `MINIMAX_API_KEY`, `OPENCODE_GO_API_KEY`,
`GENERALCOMPUTE_API_KEY`, `OPENCODE_ZEN_API_KEY`.

## Invariants & Gotchas

1. **ExternalCommand is disabled.** Both `AuthResolver::resolve` and
   `ExternalCommandProvider::fetch` return `AuthError::Unsupported`
   until async timeout plumbing exists.
2. **OAuth device flow is disabled.** Returns
   `AuthError::Unsupported`.
3. **Stored bearer tokens not supported.** Store lookups filter to
   `CredentialKind::ApiKey`. A stored `BearerToken` is a miss.
4. **Master key required to store.** Reading plaintext without a master
   key returns `Ok(None)` (no decryption), so env/config paths still
   work.
5. **Provider registration via config disables env-var auto-registration.**
   This is an all-or-nothing toggle.
6. **Conventional env var** is `{PROVIDER_UPPER}_API_KEY` with `-`
   replaced by `_`. E.g., `opencode_go` → `OPENCODE_GO_API_KEY`.
7. **`codegg auth` CLI** validates ids up-front to prevent log injection
   and store corruption. Empty ids, whitespace, punctuation, and
   non-ASCII are rejected.
8. **On-disk atomicity.** Write to `.tmp-{uuid}`, `fsync`, then
   `rename`. On Unix, parent directory is also synced. Mode `0o600`.

## Durable Connection Secret References

Durable provider connections do not embed `Credential` values or
plaintext secrets. Their persisted `SecretRef` is an opaque
identifier. The connection store retains the non-secret
provider/account locator; the resolver decrypts only at lazy runtime
construction. A missing master key, missing account, expired record,
or invalid binding is an explicit resolution failure.

## Provider-Connection Rotation Input

The local TUI may place a typed secret into a bounded daemon-owned
input buffer. `ConnectionRotateBegin` carries only a redacted
`SecretInputRef` handle. Remote WebSocket requests carrying the
variant are rejected with `secret_operation_remote_denied`. Rotation
allocates a new credential account reference and removes the previous
exact binding only after commit.

## Intentionally Not Implemented

- **SuperGrok, Claude, ChatGPT, Copilot, other consumer-session /
  app-token flows.** They require account-token reuse not part of any
  provider's documented public API.
- **OAuth device-code / PKCE** (`AuthConfig::OAuthDevice`). Reserved
  for providers that publish a stable, public contract.
- **External command** (`AuthConfig::ExternalCommand`). Async timeout
  plumbing is a follow-up.

## Testing

```bash
# Auth types tests (CredentialStore, AuthResolver, Credential)
cargo test -p codegg-providers --lib auth_types

# CLI tests (set-key, status, logout)
cargo test -p codegg --lib auth::cli

# Full auth module tests
cargo test -p codegg --lib auth
```

Key test patterns:
- `credential_debug_masks_secret` — Debug never leaks plaintext
- `set_key_without_master_key_returns_error` — MasterKeyMissing enforced
- `set_key_rejects_invalid_provider_id` — id validation
- `logout_wildcard` — `*` removes all accounts
- Resolution priority ordering tests

## Related Docs

- [crypto.md](crypto.md) — Encryption primitives
- [provider.md](provider.md) — Provider registration
- [config.md](config.md) — Configuration schema
