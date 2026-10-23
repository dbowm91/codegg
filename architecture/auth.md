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

MCP OAuth is intentionally separate from `CredentialStore`: an MCP
`TokenSet` is a server-scoped multi-field lifecycle (access token, optional
refresh token, expiry, type, and scope), whereas `CredentialStore` stores one
encrypted secret per provider/account binding. MCP uses the canonical master
key and crypto implementation for its own versioned whole-store envelope and
retains a decrypt-only reader for historical `CODEGG_ENC_v1` files.

## Where It Lives

| Artifact | Location |
|----------|----------|
| Core auth types (`AuthConfig`, `Credential`, `CredentialKind`, `CredentialStore`, `AuthResolver`, `AuthError`) | `crates/codegg-providers/src/auth_types.rs` |
| Re-export for backward compat | `src/auth/mod.rs` |
| CLI (`AuthCli`, `read_key_from_stdin`) | `src/auth/cli.rs` |
| `ExternalCommandProvider`, `ExternalCredential` | `crates/codegg-providers/src/auth_types.rs` (live) and `src/auth/external.rs` (root-crate copy) |
| OAuth scaffolding (disabled) | `src/auth/oauth.rs` |
| Test support (`env_lock`, `lock_env`) | `src/auth/mod.rs` (test_support) and `crates/codegg-providers/src/auth_types.rs` (test_support) |
| Crypto primitives | `crates/codegg-providers/src/crypto.rs` |
| Master key retrieval | `codegg_config::encryption::get_master_key()` (read-only) |
| First-run key bootstrap | `codegg_config::encryption::get_or_create_master_key*` (writes only) |
| Resolver-bypass guard | `scripts/check_master_key_resolver.py` |
| Capability matrix (single source of truth) | `crates/codegg-providers/src/setup_catalog.rs` (`provider_setup_catalog()`) |

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

When none of the four hits, the resolver falls through to the **shared
no-auth tail** below — which is where the legacy and store sources live.

For **no auth** (no `AuthConfig` or `AuthConfig::None`):
1. `ctx.env_override` → `EnvExplicit`
2. Conventional env var → `EnvConventional`
3. `ctx.legacy_api_key` → `LegacyApiKey`
4. `ctx.legacy_decrypted` → `LegacyDecrypted`
5. User-level `CredentialStore`, filtered by `ctx.capability` →
   `UserStore`

`ctx.capability` comes from `credential_capability_for(provider_id)` in
the centralized `resolve_provider_credential` path. Env/config/legacy
steps always produce `CredentialKind::ApiKey`; only the store steps can
yield `BearerToken`, and only when the target capability accepts it.

`AuthConfig::Stored { account_id }` skips straight to `UserStore`: it
requires a store (no store → `NotFound`), uses `account_id` or falls
back to `ctx.account_id`, and returns `NotFound` when the record or its
decryption is unavailable.

`AuthConfig::ExternalCommand` → `AuthError::Unsupported`
`AuthConfig::OAuthDevice` → `AuthError::Unsupported`
`AuthConfig::None` → `Ok(None)`

> **The connection binding outranks everything.** Above the resolver
> itself, `resolve_provider_credential`
> (`crates/codegg-providers/src/provider_core.rs`) checks the turn's
> `TurnCredentialPin` **first** and returns
> `ResolvedAuthSource::ConnectionBinding`. Rotation exists precisely so
> a connection's key changes while the rest of the machine (config, env,
> other stored records) still holds the old one — any later source could
> silently undo the rotation the user just performed.
>
> **Stored bearer tokens are capability-scoped.** Both the
> `AuthConfig::Stored` arm and the no-auth fallback's store lookup
> select records according to the target provider's
> `CredentialCapability` (`ApiKeyOnly` vs `ApiKeyOrBearer`, see
> `crates/codegg-providers/src/auth_types.rs` and the matrix in
> `credential_capability_for`). A stored `BearerToken` resolves for
> `ApiKeyOrBearer` providers, is rejected with typed
> `AuthError::Unsupported` for `ApiKeyOnly` providers, and expired
> records of either kind fail with `AuthError::Expired` before network
> access. The store enforces one record per `(provider_id, account_id)`
> binding, so selection is deterministic.
>
> **Ambiguity is refused, not guessed.** `AuthError::AmbiguousCredential`
> exists because the store keeps every record a rotation or `/connect`
> ever wrote: a provider can hold an orphaned key beside the live one.
> Picking the first record would inherit on-disk insertion order and
> silently authenticate with a key the user replaced. The refusal names
> every candidate account id (never a secret) and tells the operator to
> bind one with `/connect` or remove the unused accounts.

### Provider Registration

Three helpers in `crates/codegg-providers/src/provider_core.rs`:

- **`register_credential_provider`** (`ApiKeyOrBearer`) — factories
  accepting a full `Credential` envelope. Used for OpenAI-compatible
  providers (mistral, groq, deepinfra, cerebras, cohere, together,
  perplexity, xai, venice, opencode_go, generalcompute). Preserves
  `CredentialKind` and `expires_at`; transport sends
  `Authorization: Bearer …` for either kind.
- **`register_api_key_provider`** (`ApiKeyOnly`) — factories taking
  only the secret string. Used for opencode_zen and minimax
  (Anthropic-compatible). Stored bearer records are rejected with typed
  `AuthError::Unsupported` before transport.
- **`register_config_provider`** (`ApiKeyOnly`) — base-URL-aware
  variant for anthropic, openai (native), google, openrouter. Threads
  resolved secret + `cfg.base_url` to factory closure. OpenAI-native,
  OpenRouter, and Zen wire `Authorization: Bearer …` for their
  long-lived API keys, but the factory contract remains a static API-key
  string; short-lived bearer lifecycle is not claimed for these paths.

All three call `resolve_provider_credential(provider_id, cfg, env_var,
store, pin)` which builds a `ResolverContext` with
`capability = credential_capability_for(provider_id)` and returns
`ResolvedAuth`. `pin` is the turn's connection-bound credential, checked
ahead of every other source; `architecture/provider.md` "Turn Credential
Authority" owns that contract. This is the **single resolution path** for provider
registration. The executable matrix lives in
`credential_capability_for` plus `builtin_registration_order()`; the
`capability_matrix_covers_every_registration_branch` test fails if a new
branch lands without a classification.

`credential_capability_for` reads the **setup catalog**
(`crates/codegg-providers/src/setup_catalog.rs`), which is the single
source of truth for every built-in's capability; an unknown provider id
falls back to the conservative `ApiKeyOnly`. The catalog also covers
non-registered ids (`eggpool`, `custom-compatible`, `azure`) that have
their own capabilities but are not part of `builtin_registration_order()`.

| Provider | Helper | Capability |
|---|---|---|
| anthropic, openai, google, openrouter | `register_config_provider` | `ApiKeyOnly` |
| opencode_zen, minimax | `register_api_key_provider` | `ApiKeyOnly` |
| mistral, groq, deepinfra, cerebras, cohere, together, perplexity, xai, venice, opencode_go, generalcompute | `register_credential_provider` | `ApiKeyOrBearer` |

`register_builtin` (env-var-only, no config) wraps each key in
`Credential::api_key(...)`. It is **not** the normal path: the
config-aware path above registers every built-in unconditionally, and
`register_builtin` only runs as the last-resort fallback when the
config-aware pass registered zero providers
(`crates/codegg-providers/src/provider_core.rs:1317-1319`).

> **Defining a provider in config does NOT disable env-var
> auto-registration for the others.** The config-first path
> (`register_builtins_with_store`) explicitly registers all 17 built-ins,
> and each one resolves **its own** config and then **its own** conventional
> env var. So `[provider.openai]` in config affects only `openai`; the
> other 16 still auto-register from their env vars. The
> `if registry.list().is_empty() { register_builtin(registry) }` fallback
> is a redundant safety net that only fires when config-based registration
> produced zero results.

### Security Rules

- **Never log secret prefix/suffix.** `mask_secret()` returns a fixed
  16-bullet mask (`••••••••••••••••`) for any non-empty secret; an
  empty secret returns an empty string. The length never leaks.
- **Master key required to store.** `CredentialStore::put` uses the
  canonical create-on-write resolver: an explicit environment key wins,
  an existing managed key (`<config_dir>/codegg/master.key`, `0o600` on
  Unix) is reused, and a genuinely fresh store atomically bootstraps one
  (concurrent first writes converge). `AuthResolver` decryption still
  returns `MasterKeyMissing` if no key is available. A store that already
  holds encrypted material without a usable key keeps `MasterKeyMissing`
  with recovery guidance — no replacement key is generated over existing
  ciphertext. Reads never create a key.
- **MCP token-store key lifecycle.** New MCP OAuth token writes use the
  same managed-key lifecycle as provider credentials (explicit env key,
  else existing managed key, else bootstrap for a fresh token store).
  The canonical master-key lookup (`CODEGG_MASTER_KEY`, then the existing
  compatibility aliases) remains the read path. `CODEGG_TOKEN_KEY` is deprecated and only reads
  historical MCP v1 stores during safe migration; it is never used for new
  ciphertext. Legacy migration itself never bootstraps: it requires an
  already-resolvable key, so load/startup paths stay side-effect free.
- **Resolver `tracing::debug!` lines** use `source.as_str()` (a stable
  label like `"env(explicit)"`, `"config(inline)"`) and never the
  secret.
- **On-disk file permissions** are `0o600` on Unix. Atomic
  write-then-rename via a `.{filename}.tmp-{uuid}` intermediate file
  (opened `create_new`, `fsync`ed, then `rename`d; the parent directory
  is `fsync`ed too on Unix).
- **External command** is disabled. Both `AuthResolver::resolve` and
  `ExternalCommandProvider::fetch` return `AuthError::Unsupported` for
  any non-empty command. No synchronous shell-out path is reachable.
- **`codegg auth` CLI validation.** Provider and account ids validated
  to `[A-Za-z0-9_-]`. `*` accepted only by `logout`. `set-key` never
  echoes key material.

## Key Types & APIs

### AuthConfig (`auth_types.rs:243`)

```rust
pub enum AuthConfig {
    ApiKey { env: Option<String>, value: Option<String>, encrypted_value: Option<String> },
    Stored { account_id: Option<String> },
    ExternalCommand { command: String, args: Vec<String>, timeout_ms: Option<u64> },
    OAuthDevice { client_id: String, scopes: Vec<String>, auth_url: String, token_url: String },
    None,
}
```

`Default` is `ApiKey { env: None, value: None, encrypted_value: None }`.
`is_api_key()` and `is_supported()` are the two predicates;
`is_supported()` is true for `ApiKey` and `Stored` only.

### Credential (`auth_types.rs:149`)

```rust
pub struct Credential {
    pub kind: CredentialKind,           // ApiKey | BearerToken
    pub secret: String,
    pub expires_at: Option<DateTime<Utc>>,
}
```

`Debug` impl masks the secret via `mask_secret()`.
`authorization_header_value()` returns `Bearer {secret}` for both kinds.

### CredentialKind (`auth_types.rs:88`)

```rust
pub enum CredentialKind { ApiKey, BearerToken }
```

### AuthError (`auth_types.rs:15`) — 10 variants

```rust
pub enum AuthError {
    NotFound(String),
    Expired(String),
    AmbiguousCredential { provider_id: String, candidates: String },
    MasterKeyMissing,
    Crypto(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Unsupported(String),
    Invalid(String),
    ExternalCommand { command: String, message: String },
}
```

`AmbiguousCredential` names the candidate **account ids** (comma-separated,
or the literal `no-account` for a legacy account-less record) — never a
secret.

### AuthResolver (`auth_types.rs:371`)

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

### ResolverContext (`auth_types.rs:318`)

```rust
pub struct ResolverContext {
    pub provider_id: String,
    pub account_id: Option<String>,
    pub legacy_api_key: Option<String>,
    pub legacy_decrypted: Option<String>,
    pub store: Option<Arc<CredentialStore>>,
    pub env_override: Option<String>,  // test-only
    pub capability: CredentialCapability,  // accepted stored kinds
}
```

`capability` defaults to `ApiKeyOnly`. Production registration always
sets it from `credential_capability_for`; direct resolver callers must
set it explicitly when they intend bearer support.

### ResolvedAuth (`auth_types.rs:334`)

```rust
pub struct ResolvedAuth {
    pub credential: Credential,
    pub source: ResolvedAuthSource,
}
```

### ResolvedAuthSource (`auth_types.rs:340`) — 9 variants

```rust
pub enum ResolvedAuthSource {
    EnvExplicit,      // "env(explicit)"
    EnvConventional,  // "env(conventional)"
    InlineValue,      // "config(inline)"
    EncryptedConfig,  // "config(encrypted)"
    UserStore,        // "user_store"
    ConnectionBinding,// "connection_binding"
    LegacyApiKey,     // "legacy(api_key)"
    LegacyDecrypted,  // "legacy(decrypted)"
    ExternalCommand,  // "external_command"
}
```

`ConnectionBinding` is the turn's connection-bound credential; it
outranks every other source (see *Resolution Priority Order*).

### CredentialCapability (`auth_types.rs:102`)

```rust
pub enum CredentialCapability { ApiKeyOnly, ApiKeyOrBearer }
```

`accepts(kind)` is the single predicate for stored-credential selection.
`incompatible_credential_message()` builds the typed
`Unsupported` diagnostic without secret material.

### CredentialStore (`auth_types.rs:610`)

```rust
pub struct CredentialStore {
    path: PathBuf,
    records: Mutex<Vec<StoredCredentialRecord>>,
}
```

Key methods:
- `at_default_location()` — opens `<config_dir>/codegg/credentials.json`
  (`~/.config/codegg/credentials.json` on Linux,
  `~/Library/Application Support/codegg/credentials.json` on macOS,
  `%APPDATA%\codegg\credentials.json` on Windows)
- `at_path(path)` — opens an explicit store path (used by the CLI's
  `with_store_path` and by tests)
- `path()` — the store's on-disk path
- `put(provider_id, account_id, kind, secret, expires_at, scopes)` —
  encrypts through the canonical master-key resolver: an explicit
  environment key wins, an existing managed key is reused, and a genuinely
  fresh protected store bootstraps the managed key automatically. One record
  per `(provider_id, account_id)`; `put` replaces in place (including kind
  changes).
- `get_plaintext(provider_id, account_id, predicate)` — decrypts on
  demand; returns `Ok(None)` when no resolvable master key is available
  (legacy seam, retained for non-provider callers)
- `get_credential(provider_id, account_id)` — decrypts to a full
  `Credential` preserving `kind` + `expires_at`; `Ok(None)` when no
  resolvable master key is available
- `find_record(provider_id, account_id)` — exact-match metadata lookup
  used for expiry/capability checks before decryption
- `records_for_provider(provider_id)` — every record for one provider, in
  on-disk order; the caller's tool for proving a choice is unambiguous
- `remove(provider_id, account_id)` — `Some("*")` removes all accounts
- `list()` — returns all records (metadata only, deliberately unsorted)

### StoredCredentialRecord (`auth_types.rs:590`)

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

### ExternalCommandProvider (`auth_types.rs:298`)

```rust
pub struct ExternalCommandProvider;

impl ExternalCommandProvider {
    pub fn fetch(&self, cred: &ExternalCredential) -> Result<Credential, AuthError> {
        // Returns AuthError::Unsupported for any non-empty command
    }
}
```

`ExternalCredential` is at `auth_types.rs:291`. A **second**, root-crate
copy of both types lives in `src/auth/external.rs`; `src/auth/mod.rs`
re-exports the providers-crate versions, so the root `crate::auth::external`
module is the legacy home and the re-exported ones are the live types.

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

Fresh local credential/token stores do not require a preconfigured key.
On the first protected write, CodeGG creates a managed 256-bit key under
the user configuration directory (for example
`~/.config/codegg/master.key` on Linux) and reuses it on later reads and
writes.

Deployments may provide an explicit key instead; explicit environment keys
take precedence over the managed key:

```bash
export CODEGG_MASTER_KEY="your-master-key"
# Also checked for compatibility:
export CODEGG_ENCRYPTION_KEY="..."
export OPENCODE_ENCRYPTION_KEY="..."
```

If protected material already exists but its historical key is unavailable,
CodeGG fails closed with `MasterKeyMissing`; it does not create a replacement
key over existing ciphertext.

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

Adding a provider via config does **not** disable env-var
auto-registration for the other providers — the config-first path
registers all 17 built-ins explicitly, each resolving its own config and
then its own conventional env var. The env vars are:
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY`,
`OPENROUTER_API_KEY`, `MISTRAL_API_KEY`, `GROQ_API_KEY`,
`DEEPINFRA_API_KEY`, `CEREBRAS_API_KEY`, `COHERE_API_KEY`,
`TOGETHERAI_API_KEY`, `PERPLEXITY_API_KEY`, `XAI_API_KEY`,
`VENICE_API_KEY`, `MINIMAX_API_KEY`, `OPENCODE_GO_API_KEY`,
`GENERALCOMPUTE_API_KEY`, `OPENCODE_ZEN_API_KEY`.

To *disable* a provider, use the config's `disabled_providers` list — not
the presence of another provider's config entry.

## Invariants & Gotchas

1. **ExternalCommand is disabled.** Both `AuthResolver::resolve` and
   `ExternalCommandProvider::fetch` return `AuthError::Unsupported`
   until async timeout plumbing exists.
2. **OAuth device flow is disabled.** Returns
   `AuthError::Unsupported`.
3. **Stored bearer tokens are capability-scoped.** Store lookups
   filter by the target `CredentialCapability`. A stored `BearerToken`
   resolves for `ApiKeyOrBearer` providers and is a typed
   `Unsupported` error (not a silent miss) for `ApiKeyOnly` providers.
   Expired records of either kind fail with `Expired` before transport.
4. **Master key required to store.** Reading plaintext without a master
   key returns `Ok(None)` (no decryption), so env/config paths still
   work.
5. **Config does not disable env-var auto-registration.** Defining one
   provider in config leaves the other 16 built-ins free to auto-register
   from their env vars. The `registry.list().is_empty()` fallback to
   `register_builtin` is a redundant safety net, not the mechanism.
   `disabled_providers` is the opt-out.
6. **Conventional env var** is `{PROVIDER_UPPER}_API_KEY` with `-`
   replaced by `_`. E.g., `opencode_go` → `OPENCODE_GO_API_KEY`.
7. **`codegg auth` CLI** validates ids up-front to prevent log injection
   and store corruption. Empty ids, whitespace, punctuation, and
   non-ASCII are rejected.
8. **On-disk atomicity.** Write to `.{filename}.tmp-{uuid}`, `fsync`,
   then `rename`. On Unix, parent directory is also synced. Mode `0o600`.
9. **A rotation leaves an orphan beside the live key.** The store keeps
   every record a rotation or `/connect` ever wrote, so "first match"
   would be on-disk insertion order — a coin flip that can authenticate
   with a superseded key. Selection therefore refuses with
   `AuthError::AmbiguousCredential` and names the candidate accounts.
   `CredentialStore::list()` is deliberately unsorted; use
   `records_for_provider()` when a caller must reason about one
   provider's records.

## Durable Connection Secret References

Durable provider connections do not embed `Credential` values or
plaintext secrets. Their persisted `SecretRef` is an opaque
identifier. The connection store retains the non-secret
provider/account locator; the resolver decrypts only at lazy runtime
construction. A missing master key, missing account, expired record,
or invalid binding is an explicit resolution failure.

`capability_for_provider_kind()` mirrors the registration matrix:
`OpenAiCompatible` connections are `ApiKeyOrBearer`; native
`OpenAi`/`Anthropic`/`Google`/`AzureOpenAi` are `ApiKeyOnly`.
`validate_rotation_kind()` must run before overwriting a store binding
so incompatible rotations fail before commit. `ProviderConnectionFactory`
re-validates at build time (before network) and in-flight callers retain
their captured `Arc` via the daemon `ConnectionManager` revision cache.

## Provider-Connection Rotation Input

The local TUI may place a typed secret into a bounded daemon-owned
input buffer. `ConnectionRotateBegin` carries only a redacted
`SecretInputRef` handle. Remote WebSocket requests carrying the
variant are rejected with `secret_operation_remote_denied`. Rotation
allocates a new credential account reference and removes the previous
exact binding only after commit, and only when no other connection still
binds it (`prune_superseded_credential` in `src/core/eggpool.rs`).

## Provider-Neutral `/connect` Secret Handling

`/connect` collects credentials through the same masked `SecretInput`
path: masked rendering with paste support, no insertion into prompt
history, command text, toasts, generic TUI snapshots, or debug output,
and buffer clearing on submit, cancellation, error, and dialog close.
Only the trusted local core request carries the plaintext secret, and
completion events/results are secret-free. A clean local profile needs
no preconfigured encryption environment variable: the first protected
write self-initializes the managed master key (M001).

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
- `set_key_without_master_key_bootstraps_fresh_store` — a fresh store
  self-initializes its managed key
- `set_key_with_orphaned_material_refuses_new_key` — existing ciphertext
  is never orphaned by a replacement key
- `set_key_rejects_invalid_provider_id` — id validation
- `logout_wildcard_is_accepted` — `*` removes all accounts
- Resolution priority ordering tests

## Related Docs

- [crypto.md](crypto.md) — Encryption primitives
- [provider.md](provider.md) — Provider registration
- [config.md](config.md) — Configuration schema
- [connection.md](../crates/codegg-providers/src/connection.rs) — durable
  connection secret references and rotation guards

## Source Verification

Verified 2026-10-23 against `crates/codegg-providers/src/auth_types.rs`,
`crates/codegg-providers/src/provider_core.rs`,
`crates/codegg-providers/src/setup_catalog.rs`,
`crates/codegg-providers/src/connection.rs`,
`src/auth/{mod,cli,external,oauth}.rs`, and
`scripts/check_master_key_resolver.py`.

Corrections made in this pass:
- **Corrected a false claim that contradicted the code and AGENTS.md.**
  The doc stated "Adding ANY provider via config disables all env-var
  auto-registration. This is an all-or-nothing toggle." and repeated it
  as invariant #5. The config-first path
  (`register_builtins_with_store`,
  `crates/codegg-providers/src/provider_core.rs:1109`) explicitly
  registers **all 17** built-ins, each resolving its own config and then
  its own conventional env var — so defining one provider in config
  affects only that provider. The `registry.list().is_empty()` fallback
  to `register_builtin` (`provider_core.rs:1317-1319`) is a redundant
  safety net that fires only when config-based registration produced
  zero results. The doc now states the correct behaviour in both places
  and names `disabled_providers` as the actual opt-out.
- **Corrected nine stale `auth_types.rs:line` refs**, all of which had
  drifted further since 2026-10-06: `AuthConfig` `190`→`243`,
  `Credential` `131`→`149`, `CredentialKind` `70`→`88`,
  `AuthResolver` `314`→`371`, `ResolverContext` `265`→`318`,
  `ResolvedAuth` `281`→`334`, `ResolvedAuthSource` `287`→`340`,
  `CredentialStore` `553`→`610`, `StoredCredentialRecord` `533`→`590`,
  `ExternalCommandProvider` `245`→`298`. Added
  `CredentialCapability` `102` and `ExternalCredential` `291`.
- **Corrected `AuthError`'s variant count** (9 → **10**). The
  `AmbiguousCredential { provider_id, candidates }` variant was missing
  entirely. It is load-bearing: the store keeps every record a rotation
  or `/connect` ever wrote, so "first match" would be on-disk insertion
  order and could authenticate with a superseded key.
- **Corrected `ResolvedAuthSource`'s variant count** (8 → **9**).
  `ConnectionBinding` was missing — it is the source the turn's
  connection-bound credential resolves under, and it outranks every other
  source in `resolve_provider_credential`.
- **Corrected the `AuthConfig` resolution order.** The doc listed seven
  steps for the `ApiKey` arm, but the `ApiKey` arm only tries four
  (env override → conventional env → inline value → encrypted value);
  the store and legacy sources are reached by **falling through** to the
  shared no-auth tail. The doc now shows the four steps and then the
  tail explicitly, so the priority order is not read as if the store
  preceded the legacy fields.
- **Added the connection-binding precedence** to the resolution section
  — it was only mentioned in passing under *Provider Registration*.
- **Documented the `AuthConfig::Stored` arm's failure modes** (no store →
  `NotFound`; missing record → `NotFound`; undecryptable → `NotFound`),
  which the doc summarised as "skips straight to `UserStore`".
- **Added `CredentialStore::at_path`, `path()`, and
  `records_for_provider()`** to the store's method list, and the note
  that `list()` is deliberately unsorted.
- **Corrected the credential store's default path.** The doc gave only
  the Linux form; the store resolves
  `codegg_config::paths::global_config_path()`'s parent and falls back to
  `dirs::config_dir()/codegg`, so the macOS and Windows forms differ.
- **Corrected the on-disk temp-file name** to
  `.{filename}.tmp-{uuid}` (the doc wrote `.tmp-{uuid}`).
- **Corrected `mask_secret()`'s description** to "16-bullet mask for any
  non-empty secret; empty secret returns an empty string" (the old
  "regardless of input length" contradicted the very next sentence).
- **Added the setup catalog** as the single source of truth for the
  capability matrix, and the note that it also covers `eggpool`,
  `custom-compatible`, and `azure` — ids that are *not* in
  `builtin_registration_order()`.
- **Corrected the test-name list.** `set_key_without_master_key_returns_error`
  does not exist; the real test is
  `set_key_without_master_key_bootstraps_fresh_store`, and
  `logout_wildcard` is `logout_wildcard_is_accepted`. Added
  `set_key_with_orphaned_material_refuses_new_key`.
- **Added the `ExternalCommandProvider` duplication note**: the types
  exist in both `crates/codegg-providers/src/auth_types.rs` (live) and
  `src/auth/external.rs` (root copy), and `src/auth/mod.rs` re-exports
  the providers-crate ones.

Confirmed correct as written: `AuthError` at `auth_types.rs:15`;
`CredentialKind`'s exactly `ApiKey, BearerToken`; the module boundary
(`src/auth/mod.rs` re-exports `codegg_providers::auth_types::{…}` and
declares only `cli`, `external`, and `oauth` submodules, and does **not**
re-export crypto — the crypto surface lives at `codegg_providers::crypto`
(`crates/codegg-providers/src/crypto.rs`, `pub mod crypto` at
`crates/codegg-providers/src/lib.rs:18`)); the three registration helpers
and their provider lists; the 17-provider capability matrix (verified
against `setup_catalog.rs`); `builtin_registration_order()`'s 17 ids;
`conventional_env_for`'s `{PROVIDER_UPPER}_API_KEY` rule; the
`ExternalCommand` / `OAuthDevice` disablement; the MCP token-store key
lifecycle; the master-key precedence chain and
`CODEGG_MASTER_KEY_FILE` override; the CLI's `[A-Za-z0-9_-]` id
validation with `*` allowed only for `logout`; and
`capability_for_provider_kind()` / `validate_rotation_kind()`.
