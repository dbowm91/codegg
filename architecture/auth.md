# Auth Module Architecture

The `src/auth/` module is the central seam where providers obtain
credentials. It owns the typed `AuthConfig` (the configuration shape
that lives in `provider.<id>.auth`), the resolved `Credential` (a
`kind` / `secret` / `expires_at` triple used to build an
`Authorization` header), the `AuthResolver` (which performs the env →
config → store priority lookup), a user-level encrypted
`CredentialStore`, an `ExternalCommandProvider` (typed but disabled —
both the resolver and the provider's own `fetch` entry point return
`AuthError::Unsupported` until async timeout plumbing exists), OAuth
device-flow scaffolding (typed but unimplemented in this pass), and a
`cli` sub-module that wires the credential store into
`codegg auth status | set-key | logout`.

> **Note:** Auth types (`AuthConfig`, `Credential`, `AuthResolver`,
> `CredentialStore`, `AuthError`, etc.) now live in the `codegg-providers`
> crate (`crates/codegg-providers/`). The root `src/auth/mod.rs` re-exports
> them from `codegg_providers::auth_types` for backwards compatibility.

## Sub-modules

| Sub-module | Public types | Purpose |
|------------|--------------|---------|
| `auth::credential` | `Credential`, `CredentialKind`, `mask_secret` | Resolved secret + metadata; fixed-length `mask_secret` helper. |
| `auth::resolver` | `AuthResolver`, `ResolverContext`, `ResolvedAuth`, `ResolvedAuthSource`, `conventional_env_map` | Priority-based lookup. The single resolution path used by every provider registration helper in `src/provider/mod.rs`. |
| `auth::store` | `CredentialStore`, `StoredCredentialRecord` | User-level encrypted store at `~/.config/codegg/credentials.json`. |
| `auth::external` | `ExternalCommandProvider`, `ExternalCredential` | Typed home for future short-lived creds. Both `AuthResolver::resolve` and `ExternalCommandProvider::fetch` return `AuthError::Unsupported("ExternalCommand")` for a non-empty command; an empty command yields `AuthError::Invalid`. |
| `auth::oauth` | `OAuthDeviceProvider`, `OAuthDeviceSpec`, `DeviceCode` | Typed scaffolding; entry points return `AuthError::Unsupported`. |
| `auth::cli` | `AuthCli`, `read_key_from_stdin` | Minimal CLI for `codegg auth status | set-key | logout`. Validates provider and account ids (`[A-Za-z0-9_-]`, with `*` allowed for `logout` only); never echoes key material; never prints ciphertext or secret-derived fingerprints. |
| `auth::error` | `AuthError` | `NotFound`, `Expired`, `MasterKeyMissing`, `Crypto`, `Io`, `Json`, `Unsupported`, `Invalid`, `ExternalCommand { command, message }`. |
| `auth::test_support` | `env_lock`, `lock_env` | Cross-module mutex that serializes tests mutating master-key and API-key env vars. |

## `Credential` envelope

The `Credential` type is what providers consume at request time:

```rust
pub struct Credential {
    pub kind: CredentialKind,           // ApiKey | BearerToken
    pub secret: String,
    pub expires_at: Option<DateTime<Utc>>,
}

pub enum CredentialKind {
    ApiKey,        // Bearer {secret} for OpenAI-compatible
                   // x-api-key: {secret} for Anthropic-style
    BearerToken,   // short-lived OAuth/bearer access token
}
```

`Credential::authorization_header_value()` returns `Bearer {secret}` for
both variants. `Credential::api_key(secret)` and
`Credential::bearer(secret, expires_at)` are the construction helpers.
The full `Credential` (not just the secret) is what flows from
`AuthResolver::resolve` through `register_credential_provider` /
`register_api_key_provider` / `register_config_provider` and into the
per-provider factory closures in `src/provider/mod.rs`. This preserves
`CredentialKind` and `expires_at` for OpenAI-compatible providers that
need to distinguish a static API key from a short-lived bearer token.

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
(`ResolvedAuthSource::UserStore`).

> **Stored bearer tokens are not yet supported.** Both the
> `AuthConfig::Stored` arm and the no-auth fallback's store lookup
> filter to `CredentialKind::ApiKey`. A future OAuth/bearer-token
> refresh flow will need either an explicit `kind` selector on
> `AuthConfig::Stored` or a separate policy module; until then, a
> stored `BearerToken` record is treated as a miss for
> `AuthConfig::Stored`. `codegg auth set-key` only writes `ApiKey`
> records today, so this is consistent with the CLI's surface.

`AuthConfig::ExternalCommand` is recognized but the synchronous resolver
returns `AuthError::Unsupported("ExternalCommand")` because the
underlying shell-out path does not enforce its timeout and could
otherwise hang provider registration indefinitely. The same
`Unsupported` result is returned by
`ExternalCommandProvider::fetch` for any non-empty command so that no
safe code path can accidentally shell out. Async timeout plumbing is
a follow-up — async timeout plumbing deferred.
`AuthConfig::OAuthDevice` is recognized but returns
`AuthError::Unsupported("OAuthDevice")`. `AuthConfig::None` returns
`Ok(None)` regardless of legacy fields.

`ResolvedAuthSource::as_str()` returns a stable, secret-free label
(`env(explicit)`, `config(inline)`, `user_store`, ...) suitable for
`tracing::debug!` lines.

## Credential store

`CredentialStore` is the user-level encrypted credential store. It
lives at `<config_dir>/codegg/credentials.json` (or the platform
equivalent: `~/.config/codegg/credentials.json` on Linux,
`~/Library/Application Support/codegg/credentials.json` on macOS). The
file is a JSON object whose value is a list of `StoredCredentialRecord`s.
Each `encrypted_secret` is encrypted with the existing
`CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY` master key using
`crypto::encrypt_to_string`. Reading plaintext still works without a
master key for env / config-backed paths; **storing** a new credential
requires a master key and returns `AuthError::MasterKeyMissing` if
none is configured.

The on-disk file is mode `0o600` on Unix, written atomically via
write-then-rename.

The store is exposed to the CLI through `auth::cli::AuthCli` and to
provider registration through `Arc<CredentialStore>` that
`register_builtin_with_config` builds at the top of the function and
threads into every per-provider helper via the `store` argument.

## Security rules

- **Never log secret prefix/suffix.** Use `auth::mask_secret` for any
  user-facing render of a secret. The helper returns a fixed 16-bullet
  mask regardless of input length and never contains a substring of the
  input.
- **Master key is required to store.** `CredentialStore::put` and
  `AuthResolver` decryption of `encrypted_value` both return
  `AuthError::MasterKeyMissing` if `CODEGG_MASTER_KEY` /
  `CODEGG_ENCRYPTION_KEY` / `OPENCODE_ENCRYPTION_KEY` is not set. Reading
  plaintext from the store without a master key returns `Ok(None)` (no
  decryption) so env / config paths still work.
- **Resolver `tracing::debug!` lines** use `source.as_str()` (a stable
  label) and never the secret. Do not introduce log lines that include
  the resolved `Credential.secret` field. The three
  `register_*_provider` helpers in `src/provider/mod.rs` follow the same
  policy.
- **On-disk file permissions** are set to `0o600` on Unix. The file uses
  atomic write-then-rename semantics.
- **External command** output is treated as a bearer token. Do not log
  stdout / stderr. Treat the command itself as a privilege boundary —
  only point `AuthConfig::ExternalCommand.command` at officially
  supported CLIs. Both `AuthResolver::resolve` and
  `ExternalCommandProvider::fetch` return
  `AuthError::Unsupported("ExternalCommand")` for any non-empty command
  until async timeout plumbing lands. No synchronous shell-out path is
  reachable from the public safe API today.
- **`codegg auth` CLI validation.** Provider and account ids are
  validated up-front to contain only `[A-Za-z0-9_-]`. The wildcard
  `*` is accepted **only** by `logout` and is documented in
  `codegg auth logout --help`. `set-key` does not echo the key, key
  length, or any prefix/suffix; the only output is a generic
  "Stored API key for provider ..." line. The `status` command never
  prints ciphertext, plaintext, or secret-derived fingerprints.

## CLI surface

The `auth::cli::AuthCli` struct backs the `codegg auth` subcommand. The
CLI is intentionally minimal — TUI flows can build on the same
`CredentialStore` API for richer behavior.

```text
codegg auth status                    # list stored credentials (metadata only)
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

### Eggpool `/connect` credentials

The Eggpool `/connect` workflow writes its API key only through the existing
protected `CredentialStore`, using an operation-owned account locator. A
master key is required before a new connection can be committed. The TUI
keeps the key in a masked field until submission, and the protocol request's
`SecretInput` has redacted `Debug` and `Display` implementations.

Provider-connection, provisioning, health, and model tables store only opaque
secret/provider/account references. They never store the key, ciphertext, or
authorization header. Probe failure and cancellation remove only the
credential binding owned by that operation. Secret-bearing create requests
are accepted over local authenticated core IPC; the remote core WebSocket
rejects them.

Both `set-key` and `logout` validate the provider and account ids
up-front. Empty ids, ids containing whitespace, punctuation, or
non-ASCII characters are rejected with a `Config(Invalid(...))` error
before the store is touched. This keeps error messages and any future
log lines secret-free and prevents store-file corruption from
arbitrary user input.

## Provider integration

Provider registration goes through `crates/codegg-providers/src/provider/mod.rs`. The three
helpers in that file are:

- `register_credential_provider` — factories that accept a full
  `Credential` envelope. Used for all OpenAI-compatible providers
  (mistral, groq, deepinfra, cerebras, cohere, together, perplexity,
  xai, venice, opencode_go, generalcompute).
- `register_api_key_provider` — factories that take only the secret
  string. Used for `opencode_zen` and `minimax` (Anthropic-compatible,
  uses a different auth header). Rejects `CredentialKind::BearerToken`
  with a `tracing::warn!` and skips registration.
- `register_config_provider` — base-URL-aware variant for `anthropic`,
  `openai` (native), `google`, and `openrouter`. Threads the resolved
  secret through to the factory closure along with `cfg.base_url`.

All three call the centralized
`resolve_provider_credential(provider_id, cfg, env_var, store)` helper
which builds a `ResolverContext` (carrying the legacy
`api_key`/`account_id` fields and the shared `Arc<CredentialStore>`)
and returns the full `ResolvedAuth`. This is the **single resolution
path** for provider registration. `register_config_provider` does
**not** read `cfg.api_key` directly anymore; the resolver honors
`ctx.legacy_api_key` so legacy `provider.<id>.api_key` fields still
work.

`register_builtin` (env-var-only registration, no config) wraps each
env-var key in `Credential::api_key(...)` so the OpenAI-compatible
factories (which now accept a `Credential`) get a uniform envelope. It
is still used as a last-resort fallback by
`register_builtin_with_config` when the config-aware path registers
zero providers, so the TUI continues to work on hosts that rely
purely on env vars.

## Durable connection secret references

Durable provider connections do not embed `Credential` values or plaintext
secrets. Their persisted `SecretRef` is an opaque identifier. The connection
store separately retains the non-secret provider/account locator used by the
existing encrypted `CredentialStore`; the resolver decrypts only at lazy
runtime construction. A missing master key, missing account, expired record,
or invalid binding is an explicit resolution failure and never falls back to
another account.

This adapter preserves the existing environment/config registration path.
Connections are a daemon-owned runtime seam, not a replacement credential
store or an authorization decision. Secret values remain absent from
connection rows, summaries, debug output, and protocol-facing redacted
metadata.

## Intentionally not implemented

The following are parsed by the typed config but never resolved
successfully:

- **SuperGrok, Claude, ChatGPT, Copilot, other consumer-session /
  app-token flows.** They require account-token reuse that is not part
  of any provider's documented public third-party API surface. The
  CLI / TUI refuse to model them.
- **OAuth device-code / PKCE** (`AuthConfig::OAuthDevice`). Reserved
  for providers that publish a stable, public contract. Not implemented
  in this pass; the resolver returns `AuthError::Unsupported`.
- **External command** (`AuthConfig::ExternalCommand`). Currently
  returns `AuthError::Unsupported` from the synchronous resolver
  because the underlying `std::process::Command` does not enforce its
  timeout. Async plumbing is a follow-up.
