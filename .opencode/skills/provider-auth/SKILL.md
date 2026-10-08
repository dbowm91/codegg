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
| Backends | `anthropic.rs`, `openai.rs`, `google.rs`, `openrouter.rs`, `opencode_zen.rs`, `opencode_go.rs`, `provider_profile.rs`, `additional.rs`, `openai_compatible.rs`, `azure.rs`, `vertex.rs`, `bedrock.rs`, `copilot.rs`, `cloudflare.rs`, `gitlab.rs`, `eggpool.rs` | Per-provider request/stream/models mapping; `provider_profile.rs` owns the shared-profile adapter and `opencode_go.rs` the multi-surface dispatch |
| Auth types | `crates/codegg-providers/src/auth_types.rs` | `AuthConfig`, `Credential`, `CredentialKind`, `CredentialCapability`, `CredentialStore`, `AuthResolver`, `AuthError`; `ExternalCommand` unsupported |
| Auth CLI | `src/auth/cli.rs`, `src/auth/mod.rs` | `codegg auth set-key/status/logout`; the clap `AuthSubcommand` enum lives in `src/main.rs:508`, the handlers are `AuthCli` (`status`/`set_key`/`logout`, `cli.rs:93`,`:124`,`:161`) in `src/auth/cli.rs`; `src/auth` re-exports providers types |
| Crypto | `crates/codegg-providers/src/crypto.rs`, `codegg_config::encryption` | AES-256-GCM + Argon2id; master key via `get_master_key()` (`CODEGG_MASTER_KEY`) |
| Resilience | `circuit.rs`, `fallback.rs`, `cache.rs`, `catalog.rs`, `discovery.rs`, `models.rs` | `CircuitBreaker`, `FallbackProvider`, response cache, live catalog + SQLite discovery cache, embedded free-tier defs |
| Streaming | `wire.rs`, `responses_api.rs`, `text_tool_parser.rs` | Shared `eggpool-wire` kernel bridge (canonical request encode + stream decode), Responses API adapter, bounded textual tool-call repair |
| Connection qualification | `crates/codegg-providers/src/qualification.rs`, `src/core/provider_qualification.rs`, `src/agent/provider_qualification.rs` | `CatalogOutcome` (transport/catalog axis) vs `CredentialVerification` (credential axis); `SetupProbeStrategy::credential_evidence()`; revision-scoped inference-feedback writer |

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
7. **Catalog discovery is never credential verification.** A successful
   `Provider::models()` call proves only that the catalog is usable — several
   built-ins return local/static arrays with no network I/O, generic
   compatible discovery falls back rather than proving auth, and some
   provider `/models` endpoints are publicly readable. Only
   `SetupProbeStrategy::AuthenticatedCompatibleCatalog` (a genuinely
   authenticated, non-billable metadata request) may yield
   `CredentialVerification::Verified`, and only
   `CredentialVerification::from_probe` may derive a verdict from a probe.
   Never derive one from a probe result, a model count, or a `healthy` health
   row. `scripts/check_provider_qualification.py` enforces the naming and the
   typed write path.
8. **Credential verdicts are revision-scoped and monotonic in safety.** The
   only durable credential evidence is an authenticated request on the exact
   connection revision; transient failures write nothing, and a verdict for a
   rotated-away revision must be discarded rather than applied to the new
   credential.
9. **An unresolved wire mapping is not an authentication failure.** When a
   model has no reviewed model-to-wire hint in the shared provider profile,
   resolution fails closed with a local, zero-network error. Never default such
   a model to Chat Completions, never infer a surface from a model prefix or
   family, and never label the failure `auth` — that would corrupt the M010
   credential axis, which real 401/403 inference feedback owns.
10. **Per-surface credential headers are owned by the resolved surface.**
    Chat and Responses send `Authorization: Bearer …`; Messages sends
    `x-api-key`. A surface never receives the other's credential header, and
    profile-supplied static headers may not collide with the transport-owned
    credential, session, or content-type headers.

## Static Guards

```bash
bash scripts/check_provider_connections_m4_coverage.sh      # connection lifecycle coverage
bash scripts/check_provider_connections_tombstone_compat.sh  # tombstone compat
python3 scripts/check_provider_qualification.py             # catalog != credential verification
python3 scripts/check_provider_wire_boundary.py             # only neutral eggpool contracts
python3 scripts/check_provider_multi_surface_dispatch.py    # one surface per request, fails closed
```

## Testing

```bash
cargo test -p codegg-providers
cargo test -p codegg --lib core::eggpool::tests   # qualification semantics
cargo test -p codegg --test opencode_go_connection_trajectory  # durable -> wire end to end
codegg providers          # against real config, not a static list
codegg models -p openai
```

The cross-layer trajectory redirects the OpenCode Go **origin** onto a loopback
capture server so a real inference request can be observed. That redirect lives
behind the `codegg-providers` `capture-test-support` feature, which the root
crate enables through a **dev-dependency only**. If you touch endpoint ownership,
keep it that way: path, per-surface auth shape, and the surface decision must
still come from the shared provider profile. Never add a config- or
env-reachable endpoint override to make a test pass.

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
