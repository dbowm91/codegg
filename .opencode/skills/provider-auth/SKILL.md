---
name: provider-auth
description: LLM provider backends, credential resolution, encrypted store, and resilience in codegg
version: 1.0.0
tags:
  - provider
  - auth
  - credentials
  - resilience
---

# Provider and Auth Guide

Operational guide for changing provider and credential code. The full
contracts live in `architecture/provider.md`, `architecture/auth.md`,
and `architecture/crypto.md`; this skill covers the seams that are easy
to violate.

## Ownership Map

| Layer | Location | Role |
|-------|----------|------|
| Trait + registry | `crates/codegg-providers/src/provider_core.rs` | `Provider` trait, `ProviderRegistry`, `register_builtin`, `register_builtin_with_config`, `credential_capability_for`; derive current provider totals from the registry rather than pinning them here |
| Setup catalog + durable builders | `crates/codegg-providers/src/setup_catalog.rs` | Pre-credential `ProviderDefinition` catalog (endpoint policy, capability, construction/probe strategy); `build_durable_provider` shared by `ProviderConnectionFactory` and the generic provisioner; Eggpool is a `ProxyPreset`, `custom` the generic compatible entry |
| Backends | `anthropic.rs`, `openai.rs`, `google.rs`, `openrouter.rs`, `opencode_zen.rs`, `additional.rs`, `openai_compatible.rs`, `azure.rs`, `vertex.rs`, `bedrock.rs`, `copilot.rs`, `cloudflare.rs`, `gitlab.rs`, `eggpool.rs` | Per-provider request/stream/models mapping |
| Auth types | `crates/codegg-providers/src/auth_types.rs` | `AuthConfig`, `Credential`, `CredentialKind`, `CredentialCapability`, `CredentialStore`, `AuthResolver`, `AuthError`; `ExternalCommand` unsupported |
| Auth CLI | `src/auth/cli.rs`, `src/auth/mod.rs` | `codegg auth set-key/status/logout`; the clap `AuthSubcommand` enum lives in `src/main.rs:508`, the handlers are `AuthCli` (`status`/`set_key`/`logout`, `cli.rs:93`,`:124`,`:161`) in `src/auth/cli.rs`; `src/auth` re-exports providers types |
| Crypto | `crates/codegg-providers/src/crypto.rs`, `codegg_config::encryption` | AES-256-GCM + Argon2id; master key via `get_master_key()` (`CODEGG_MASTER_KEY`) |
| Resilience | `circuit.rs`, `fallback.rs`, `cache.rs`, `catalog.rs`, `discovery.rs`, `models.rs` | `CircuitBreaker`, `FallbackProvider`, response cache, live catalog + SQLite discovery cache, embedded free-tier defs |
| Streaming | `wire.rs`, `responses_api.rs`, `text_tool_parser.rs` | Shared `eggpool-wire` kernel bridge (canonical request encode + stream decode), Responses API adapter, bounded textual tool-call repair |

## Hard Rules

1. **Two registration paths only.** `register_builtin`
   (`provider_core.rs:498`) is the pure env-var sweep over 15 providers
   (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); `register_builtin_with_config`
   (`provider_core.rs:890`) is the production path and explicitly calls
   `register_config_provider`/`register_api_key_provider`/`register_credential_provider`
   for all 17 built-ins
   (`builtin_registration_order()`, `provider_core.rs:592`). Each of those
   helpers resolves that provider's own config entry first, then its
   conventional env var. **Declaring one provider in config does NOT disable
   env-var auto-registration for the others** — resolution is per-provider
   and independent. The trailing
   `if registry.list().is_empty() { register_builtin(registry) }`
   (`provider_core.rs:1088`) is a redundant safety net that only fires when
   config-based registration produced zero results; do not treat it as the
   mechanism that supplies the other providers. Do not "fix" registration by
   registering providers ad hoc.
   Exact totals are `builtin_registration_order()` / the catalog, not this
   contract.
2. **`ExternalCommand` auth is unsupported.** Never add it as a production
   credential source.
3. **Never log secrets.** `codegg auth status` never prints stored secrets;
   redaction is load-bearing. API-key and stored-key flows are the
   documented production paths; other typed modes are schema-only until
   runtime-complete.
4. **Single credential resolution path.** Env → config inline → encrypted
   config → user store → legacy fields via `AuthResolver`. No parallel
   lookup chains.
4b. **Setup catalog owns onboarding metadata.** New built-in providers need
   a `setup_catalog` definition (explicit connectable disposition) plus a
   durable-builder arm — never a second name list, and never a fake
   API-key coercion for OAuth/signing/multi-field auth. The
   `catalog_covers_builtin_registration_order_with_explicit_disposition`
   test enforces this.
5. **MCP OAuth stays separate from `CredentialStore`.** A `TokenSet` is a
   server-scoped multi-field lifecycle (access/refresh/expiry/type/scope)
   with its own versioned whole-store envelope; it reuses only the
   canonical master key + crypto, plus a decrypt-only `CODEGG_ENC_v1`
   reader.
6. **Semantic routing validates against the selected connection.** See the
   `agent` skill; provider selection itself is durable
   session/connection state, not a routing decision.

## Static Guards

```bash
bash scripts/check_provider_connections_m4_coverage.sh      # connection lifecycle coverage
bash scripts/check_provider_connections_tombstone_compat.sh  # tombstone compat
```

## Testing

```bash
cargo test -p codegg-providers
codegg providers          # against real config, not a static list
codegg models -p openai
```

## See Also

- `architecture/provider.md`, `architecture/auth.md`, `architecture/crypto.md`
- `.skills/agent/SKILL.md` — per-turn provider object + semantic router bounds
- `.skills/core/SKILL.md` — provider/model selection helpers on the core facade

## Source verification

Verified 2026-10-06 against `architecture/provider.md`,
`architecture/auth.md`, `architecture/crypto.md`,
`crates/codegg-providers/src/provider_core.rs`,
`crates/codegg-providers/src/auth_types.rs`,
`crates/codegg-providers/src/setup_catalog.rs`,
`crates/codegg-providers/src/wire.rs`, `crates/codegg-providers/src/circuit.rs`,
`crates/codegg-providers/src/fallback.rs`, `crates/codegg-providers/src/cache.rs`,
`crates/codegg-providers/src/catalog.rs`,
`crates/codegg-providers/src/discovery.rs`,
`crates/codegg-providers/src/models.rs`,
`crates/codegg-config/src/encryption.rs`, `src/auth/cli.rs`, `src/auth/mod.rs`,
`src/main.rs`, and `scripts/`. Corrected the streaming row (`sse_parser.rs`
was retired; the shared `eggpool-wire` kernel bridge `wire.rs` now owns
canonical encode and stream decode), the `CredentialCapability` attribution
(defined in `auth_types.rs:84` and re-exported from `provider_core.rs:17` as
`ProviderCredentialCapability`; `credential_capability_for` is the
provider-side entry point), and the truncated test name
`catalog_covers_builtin_registration_order` →
`catalog_covers_builtin_registration_order_with_explicit_disposition`.

**Corrected a materially wrong invariant.** The skill previously claimed
that one config-declared provider suppresses the whole env-var sweep.
Disproved by `provider_core.rs:890-1091`: the config-first path makes 17
explicit per-provider registration calls (each config-then-env-var), and the
`if registry.list().is_empty()` guard at `provider_core.rs:1088` is a
redundant fallback. `architecture/provider.md:82-86` and `:471-477` already
stated the correct per-provider independence; the skill now matches both the
source and that doc. Counts confirmed empirically: 17 entries in
`builtin_registration_order()` (`provider_core.rs:592`) and 17 registration
calls in `register_builtin_with_config`, versus 15 env vars in
`register_builtin` (`provider_core.rs:498-563`).
Claims without a traceable source were removed rather than guessed.
