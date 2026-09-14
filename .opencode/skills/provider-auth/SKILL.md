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
| Trait + registry | `crates/codegg-providers/src/provider_core.rs` | `Provider` trait, `ProviderRegistry`, `register_builtin`, `register_builtin_with_config`, `CredentialCapability`; derive current provider totals from the registry rather than pinning them here |
| Backends | `anthropic.rs`, `openai.rs`, `google.rs`, `openrouter.rs`, `opencode_zen.rs`, `additional.rs`, `openai_compatible.rs`, `azure.rs`, `vertex.rs`, `bedrock.rs`, `copilot.rs`, `cloudflare.rs`, `gitlab.rs`, `eggpool.rs` | Per-provider request/stream/models mapping |
| Auth types | `crates/codegg-providers/src/auth_types.rs` | `AuthConfig`, `Credential`, `CredentialKind`, `CredentialStore`, `AuthResolver`, `AuthError`; `ExternalCommand` unsupported |
| Auth CLI | `src/auth/cli.rs`, `src/auth/mod.rs` | `codegg auth set-key/status/logout`; `src/auth` re-exports providers types |
| Crypto | `crates/codegg-providers/src/crypto.rs`, `codegg_config::encryption` | AES-256-GCM + Argon2id; master key via `get_master_key()` (`CODEGG_MASTER_KEY`) |
| Resilience | `circuit.rs`, `fallback.rs`, `cache.rs`, `catalog.rs`, `discovery.rs`, `models.rs` | `CircuitBreaker`, `FallbackProvider`, response cache, live catalog + SQLite discovery cache, embedded free-tier defs |
| Streaming | `sse_parser.rs`, `responses_api.rs`, `text_tool_parser.rs` | SSE parsing, Responses API adapter, bounded textual tool-call repair |

## Hard Rules

1. **Two registration paths only.** `register_builtin` is the env-var path and
   `register_builtin_with_config` is the config-first path. Adding any
   config-defined provider disables all env-var auto-registration. Current
   provider totals are registry state, not part of this contract.
2. **`ExternalCommand` auth is unsupported.** Never add it as a production
   credential source.
3. **Never log secrets.** `codegg auth status` never prints stored secrets;
   redaction is load-bearing. API-key and stored-key flows are the
   documented production paths; other typed modes are schema-only until
   runtime-complete.
4. **Single credential resolution path.** Env → config inline → encrypted
   config → user store → legacy fields via `AuthResolver`. No parallel
   lookup chains.
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
