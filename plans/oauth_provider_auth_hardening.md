# OAuth Provider Auth Cleanup and Hardening Plan

## Goal

Complete the cleanup pass for the provider auth work. The auth architecture is now mostly in place: typed `AuthConfig`, encrypted user credential storage, config-aware provider registration, credential-preserving OpenAI-compatible providers, and basic `codegg auth` CLI commands exist. This plan focuses on removing the remaining sharp edges, improving invariants, and adding regression tests so future OAuth/device-code work has a stable base.

This is not a new feature pass. Do not add consumer-session auth, SuperGrok subscription auth, Claude app auth, ChatGPT app auth, Copilot token reuse, or undocumented OAuth flows. Keep OAuth/device-code as typed scaffolding until a provider documents a stable public contract.

## Current State to Preserve

The following behavior should continue to work after this pass:

- legacy env-var registration such as `OPENAI_API_KEY`, `XAI_API_KEY`, `ANTHROPIC_API_KEY`, `MINIMAX_API_KEY`;
- legacy config `provider.<id>.api_key` and `encrypted_api_key` fallback;
- new config `provider.<id>.auth = { type = "api_key", env = "..." }`;
- new config `provider.<id>.auth = { type = "stored", account_id = "..." }` when a matching credential exists;
- OpenAI-compatible providers accepting full `Credential` values;
- API-key-only providers rejecting bearer tokens;
- xAI/Grok Build registration through the existing xAI API-key path;
- `codegg auth status`, `codegg auth set-key`, and `codegg auth logout`.

## Phase 1: Remove Redundant Legacy Bypass in Provider Registration

`register_config_provider()` currently calls `resolve_provider_credential()`, then has a fallback branch that directly reads `cfg.api_key` if the resolver returns `Ok(None)`. Since the resolver already handles legacy `api_key`, this weakens the single-resolution-path invariant.

Update `register_config_provider()` so every credential lookup goes through `resolve_provider_credential()`.

Desired behavior:

```rust
match resolve_provider_credential(name, cfg_opt, None, store) {
    Ok(Some(resolved)) => { ... register ... }
    Ok(None) => { trace/debug no credential configured; do not directly read cfg.api_key }
    Err(e) => { warn and skip }
}
```

After this change, the only place legacy `api_key` should be read for registration is inside `AuthResolver` via `ResolverContext.legacy_api_key`.

Add a test proving that a provider with legacy `api_key` still registers through the resolver path after the fallback branch is removed.

## Phase 2: Align `codegg providers` with Config-aware Registration

`cmd_models()` already loads `Config` and calls `register_builtin_with_config()`, but `cmd_providers()` still calls `register_builtin()`. This means `codegg providers` may omit config-only/stored-auth providers while `codegg models` sees them.

Update `cmd_providers()` to:

1. load config with `Config::load().unwrap_or_default()` or propagate errors consistently with neighboring commands;
2. create `ProviderRegistry`;
3. call `provider::register_builtin_with_config(&mut registry, &config)`;
4. print providers as before.

Keep a fallback message such as:

```text
No providers configured. Set API keys, configure provider auth, or store a key with `codegg auth set-key <provider>`.
```

Add a test only if the CLI command layer has existing command tests. Otherwise, document this as manually verified in the implementation summary.

## Phase 3: Make ExternalCommand Impossible to Use Unsafely

`AuthResolver` now returns `Unsupported("ExternalCommand")`, which is good. However, `ExternalCommandProvider::fetch()` remains public and still uses blocking `std::process::Command` without enforcing timeout. This is a future footgun.

Choose one of these options.

### Option A: Disable the unsafe implementation completely, preferred for this cleanup pass

Change `ExternalCommandProvider::fetch()` to return `AuthError::Unsupported("ExternalCommand")` unconditionally after validating the command is non-empty, or remove the method if it has no callers.

If keeping the method for future shape:

```rust
pub fn fetch(&self, cred: &ExternalCredential) -> Result<Credential, AuthError> {
    if cred.command.trim().is_empty() {
        return Err(AuthError::Invalid("external command is empty".to_string()));
    }
    Err(AuthError::Unsupported(
        "ExternalCommand requires async timeout plumbing".to_string(),
    ))
}
```

Update tests accordingly:

- empty command returns `Invalid`;
- non-empty command returns `Unsupported`;
- remove or ignore tests using `printf` and `false`.

### Option B: Implement real async timeout plumbing

Only choose this if it is straightforward within current architecture.

- Use `tokio::process::Command`.
- Use `tokio::time::timeout` around `wait_with_output()`.
- Ensure timed-out children are killed or otherwise cannot remain indefinitely.
- Make resolver async or add a separate explicit async resolver path.
- Do not route provider registration through blocking external commands.

For this pass, Option A is likely better. Real external-command auth can be a future feature.

## Phase 4: Add Stored Credential Registration Tests

Add direct unit tests around `resolve_provider_credential()` and provider registration with a temp credential store.

Suggested tests in `src/provider/mod.rs` tests or a dedicated provider auth test module:

1. `resolve_provider_credential_resolves_stored_api_key`
   - create temp `CredentialStore`;
   - set `CODEGG_MASTER_KEY` under the existing env-lock test guard;
   - store provider `xai`, default account, `CredentialKind::ApiKey`, secret `stored-xai-key`;
   - create `ProviderConfig { auth: Some(AuthConfig::Stored { account_id: None }), ..Default::default() }`;
   - call `resolve_provider_credential("xai", Some(&cfg), Some("XAI_API_KEY"), Some(&Arc::new(store)))`;
   - assert source is `ResolvedAuthSource::UserStore` and kind is `ApiKey`.

2. `register_credential_provider_accepts_stored_key`
   - if testing private helper is practical, register a test OpenAI-compatible provider using `register_credential_provider` and verify registry contains it.
   - If private helper visibility is awkward, test through `resolve_provider_credential()` only and keep registration behavior covered by smaller helper tests.

3. `register_api_key_provider_rejects_bearer_token`
   - construct a `ResolvedAuth` with `CredentialKind::BearerToken` and call `ensure_api_key_credential()`;
   - assert `None`.

4. `openai_compatible_accepts_bearer_token`
   - construct provider with `Credential::bearer("tok", None)` via `simple_with_credential()`;
   - assert provider config credential kind remains `BearerToken` if accessible from unit tests in the same module.

These tests should not touch the real user credential store.

## Phase 5: Clarify Stored Bearer-token Semantics

Currently `AuthConfig::Stored` resolves only stored credentials with `CredentialKind::ApiKey`. That is correct for the current CLI because `codegg auth set-key` only stores API keys, but it will be insufficient for future OAuth/bearer-token storage.

For this cleanup pass, do not implement OAuth storage. Instead:

- rename or document the current behavior as stored API-key resolution;
- add a comment near the `CredentialKind::ApiKey` predicate explaining that OAuth/bearer-token store lookup will need a future policy;
- optionally add a future enum field to `AuthConfig::Stored` only if backward-compatible and simple, e.g. `kind: Option<CredentialKind>`, but do not require it now.

If adding `kind` is too disruptive, leave the schema alone and document the limitation in `architecture/config.md` and `codegg.example.jsonc`.

## Phase 6: Tighten `codegg auth` UX and Safety

The CLI reads `set-key` from stdin. That is acceptable, but make the user-facing behavior explicit and safe.

Recommended changes:

- Update `codegg auth set-key --help` text to say: `Reads the key from stdin; example: printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai`.
- Avoid echoing any key material in success/error messages.
- Validate provider ids are non-empty and contain only conservative characters: `[A-Za-z0-9_-]`.
- Validate account ids similarly, allowing `*` only for logout.
- `logout --account '*'` should remove all records for that provider; document that behavior in help text.
- `status` should never print encrypted ciphertext, raw secrets, or secret-derived fingerprints.

Add tests for provider/account validation if implemented.

## Phase 7: Repair Documentation Accuracy

Update docs and examples after the cleanup changes.

Files to inspect/update:

- `architecture/config.md`
- `architecture/provider.md`
- `codegg.example.jsonc`
- `README.md`, if it documents provider credentials
- `plans/oauth_provider_auth.md` and `plans/oauth_provider_auth_followup.md` only if they contain now-misleading implementation status claims; otherwise leave historical plan files alone.

Documentation should state:

- `external_command` is parsed but unsupported until async timeout plumbing exists;
- `stored` currently resolves stored API keys, not OAuth bearer token refresh sets;
- `codegg providers` and `codegg models` both use config-aware provider registration;
- stored credentials require a master key, currently `CODEGG_MASTER_KEY`, `CODEGG_ENCRYPTION_KEY`, or `OPENCODE_ENCRYPTION_KEY`;
- consumer app subscription auth is intentionally not implemented.

Fix any stale text that says `codegg auth` exists if the command behavior differs from docs.

## Phase 8: Secret Logging Audit

Run a text audit for obvious secret leakage patterns:

```text
key_prefix
key_suffix
key_len
api_key.*debug
secret.*debug
credential.*debug
Authorization
Bearer {}
```

Remove or harden any remaining logs that include secret material, secret lengths, prefixes, suffixes, ciphertext, or full command stdout.

Allowed diagnostics:

- provider id;
- credential source such as `env(conventional)` or `user_store`;
- credential kind (`api_key` vs `bearer`), if needed;
- whether a credential exists, without length or fingerprint.

Do not log external command stdout, even when external commands are disabled.

## Phase 9: Validation Commands

Run the standard checks:

```text
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

If clippy fails because of pre-existing unrelated warnings, record exact file paths and messages in the implementation summary. Do not hide unrelated failures by broad `allow` annotations.

Also manually verify these flows if possible:

```text
# env var path still works
XAI_API_KEY=test cargo run -- providers

# stored key path works
export CODEGG_MASTER_KEY=dev-test-key
printf '%s' 'stored-test-key' | cargo run -- auth set-key xai
cargo run -- auth status
# configure provider.xai.auth = { type = "stored" } in a temp config if possible
cargo run -- providers
cargo run -- models -p xai
cargo run -- auth logout xai
```

## Acceptance Criteria

This cleanup pass is complete when:

- provider registration has a single credential-resolution path and no direct `cfg.api_key` bypass after resolver returns `None`;
- `codegg providers` uses config-aware registration consistently with `codegg models`;
- `ExternalCommandProvider::fetch()` cannot execute an unbounded blocking command from any public safe path, or it returns `Unsupported` until async timeout plumbing exists;
- stored API-key credentials are tested through `resolve_provider_credential()` with a temp store;
- API-key-only provider helpers reject bearer credentials in tests;
- OpenAI-compatible constructors preserve bearer-token credential kind in tests;
- docs accurately describe supported, unsupported, and intentionally omitted auth modes;
- secret logging audit finds no secret prefixes, suffixes, lengths, ciphertexts, raw stdout tokens, or raw authorization headers;
- legacy env/config API-key flows still work;
- `cargo fmt` and `cargo test` pass, and clippy status is documented.

## Non-goals

Do not implement provider-specific OAuth flows in this pass.

Do not add OS keychain support in this pass.

Do not remove legacy `api_key` / `encrypted_api_key` fields.

Do not migrate existing config automatically.

Do not implement undocumented consumer subscription auth.

Do not add browser-based auth UX to the TUI yet.
