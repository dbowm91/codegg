# Agent Runtime, Model Adaptation, and ACP Milestone 007 — Declarative Model-Adapter Registry

Status: blocked — requires Milestones 001 and 002 closure

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-007--declarative-model-adapter-registry-and-build-generation`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Primary class: infrastructure

## 1. Objective

Replace hard-coded model-name substring adaptation with a versioned declarative model-adapter registry. Built-in adapter TOML files must describe matching, prompt/control behavior, capability assumptions, canonical-to-wire tool/argument aliases, request-field transforms, recovery preferences, and serving requirements. Cargo build infrastructure must validate and compile the TOMLs into Rust so adapters are distributed in the binary without requiring runtime source files or Python.

This milestone defines the registry and generic typed transform surface. Milestone 008 adds provider-neutral reasoning preservation and the complete Poolside Laguna vertical slice.

## 2. Dependencies

Hard dependencies:

- M001 canonical prompt compiler and deterministic agent registry;
- M002 canonical/wire tool-surface seam and stable surface fingerprint.

Interface dependencies:

- current model profiles and resolver;
- provider registry and request DTOs;
- prompt profile kinds and task-state policy;
- tool definitions and provider-specific serialization;
- recovery-controller generic policy seam from M006 may be consumed if available, but M007 can define adapter fields before M006 closure.

No external model endpoint is required.

## 3. Current implementation evidence

Re-audit:

- `ModelProfileResolver` selects built-in profiles using hard-coded model ID substring checks;
- profile fields cover context/output limits, reliability, late-system support, control-message preference, patch size, explicit tool contract, continue nudge, parallel tools, preferred/disabled tools, and task-state policy;
- `select_provider_prompt` independently uses hard-coded model/family matching;
- provider capabilities are also hard-coded by provider ID;
- config overrides can modify model profiles but cannot express model matching precedence, tool/argument aliases, message/request transforms, serving requirements, or adapter identity/version;
- built-in agent TOMLs already demonstrate a generated-asset precedent, but generation currently uses Python and checked-in Rust;
- the user requires simple maintainable TOMLs compiled into Rust during build.

## 4. Invariants

- Canonical internal model/tool semantics remain stable.
- Adapters only transform provider/model-facing representation and policy; they cannot execute code or grant authority.
- Adapter TOML is parsed with strict unknown-key rejection and bounded values.
- Matching/layering is deterministic and inspectable.
- Exact-model and user overrides may narrow/change adaptation but cannot bypass hard provider/runtime constraints.
- Tool aliases are reversible and collision-free within a resolved surface.
- Built-in adapters are compiled into the binary through Cargo-owned Rust generation.
- Runtime startup does not require Python or the source TOML directory.
- Unknown models receive a conservative generic adapter.
- Adapter ID/version/fingerprint participate in diagnostics, request/cache identity, and reproducibility.
- Adapter changes do not silently alter existing sessions mid-turn; active turns pin a resolved adapter.

## 5. Scope

### In scope

- Define a versioned adapter schema and typed Rust representation.
- Create `assets/model-adapters/` for built-in TOMLs.
- Support deterministic matching fields such as:
  - provider IDs/API families;
  - exact model IDs;
  - model prefixes/suffixes or validated regexes;
  - exclusion patterns;
  - priority/specificity;
  - optional endpoint/serving-family hints supplied by configuration.
- Support layered resolution:
  - generic API adapter;
  - provider adapter;
  - model-family adapter;
  - exact-model adapter;
  - bounded user override.
- Support typed sections for:
  - model capabilities/limits and reliability hints;
  - prompt profile/fragments;
  - system/control-message placement;
  - tool format, tool choice, parallelism, required tools;
  - canonical tool aliases and argument aliases;
  - request parameter insertion/rename/omission using a fixed typed transform enum;
  - thinking/reasoning controls without implementing reasoning preservation yet;
  - recovery thresholds/preferences;
  - serving requirements/diagnostics;
  - cache identity behavior.
- Add Rust build-time validation/generation through `build.rs` or a small internal codegen crate.
- Generate static Rust data into `$OUT_DIR` and include it from the model-profile/adapter module.
- Add `cargo xtask adapters check` only if an existing xtask pattern exists or a small check command materially improves local diagnostics; do not create a new task framework solely for this.
- Migrate existing hard-coded profile/prompt selection into equivalent built-in adapter assets where feasible, retaining minimal generic provider code.
- Add inspection/diagnostic API or command showing matched layers, source, version, and effective fields.

### Out of scope

- Arbitrary scripting, WASM, expressions, or runtime code loading in adapters.
- Automatic benchmark-driven adapter learning.
- Downloading adapters from remote registries.
- Complete reasoning-message preservation; M008 owns it.
- Live validation of every model/provider.
- Replacing provider transport implementations.
- Exposing adapter configuration as a stable public ecosystem package format in this milestone.

## 6. Required production changes

### Schema

A representative bounded schema may resemble:

```toml
schema_version = 1

[adapter]
id = "poolside-laguna-2"
version = 1
priority = 100
description = "Poolside Laguna agentic coding models"

[[match]]
provider = ["poolside", "openai-compatible", "vllm", "sglang"]
model_regex = "(?i)(^|/)(laguna-(xs|s)-2\\.1|laguna-m\\.1)(-|$)"
exclude_regex = "(?i)-base($|/)"

[capabilities]
tools = true
reasoning = true
interleaved_reasoning = true
parallel_tool_calls = false
late_system_messages = false

[tools]
format = "openai_function"
tool_choice = "auto"
max_parallel = 1
require_structured_calls = true

[tools.rename]
bash = "shell"

[tools.arguments.shell]
command = "cmd"

[prompt]
profile = "local_strict"
fragments = ["Use structured tool calls for actions."]

[recovery]
malformed_tool_retry = 1
no_action_turn_limit = 1
restore_full_palette_on_missing_tool = true

[server_requirements]
tool_call_parser = "poolside_v1"
reasoning_parser = "poolside_v1"
auto_tool_choice = true
```

The implementation may refine field names, but schema meaning must remain fixed, documented, and strictly validated.

### Typed transforms

Do not permit generic JSONPath or arbitrary templating. Define a small enum of reviewed operations, such as:

- set/remove top-level request field;
- set nested known request field;
- rename canonical tool for wire schema;
- rename known tool arguments;
- choose system/control role;
- choose tool-choice mode;
- set max parallel tools;
- set thinking parameter through an approved provider path;
- require/forbid late system messages;
- require post-tool continuation nudge.

Provider implementations remain responsible for serializing only supported transforms.

### Matching and precedence

Resolution must be deterministic. Compute specificity from exact provider/model match before regex/prefix and use explicit priority only as a tie-breaker or documented override. Reject ambiguous equal-precedence conflicting adapters.

User overrides should reference adapter IDs or explicit model match keys and remain bounded. Do not allow project content to inject executable transforms.

### Build generation

Preferred structure:

```text
assets/model-adapters/*.toml
crates/model-adapter-schema/      # only if dependency isolation warrants it
crates/model-adapter-codegen/     # only if build.rs would otherwise duplicate logic
build.rs
src/model_adapter/generated.rs or include!($OUT_DIR/...)
```

`build.rs` must:

- emit `cargo:rerun-if-changed` for adapter assets/codegen;
- enumerate files in sorted order;
- parse/validate strict schema;
- validate unique IDs, versions, regexes, transforms, aliases, and precedence;
- canonicalize definitions;
- generate deterministic Rust into `$OUT_DIR`;
- fail with source file and field diagnostics.

Do not check in generated Rust unless repository packaging constraints demonstrably require it. The crate package must include the TOML inputs needed at build time.

### Runtime integration

Create `ResolvedModelAdapter` carrying effective model profile, prompt/control policy, tool mapping rules, request transforms, recovery policy, source layers, and fingerprint. Root and child turns resolve/pin it before prompt/tool/context construction.

M001 prompt compiler and M002 tool surface consume it; provider adapters receive only validated applicable transforms.

## 7. Ordered work packages

### A — Inventory and schema contract

- inventory hard-coded model/profile/prompt/provider capability branches;
- classify which fields belong to adapter data versus provider transport code;
- define schema, precedence, transform enum, bounds, and diagnostics;
- create invalid/ambiguous fixture TOMLs.

### B — Rust parser/validator/codegen

- implement strict shared schema parsing;
- compile regex/match definitions safely;
- validate alias reversibility and transform support;
- generate deterministic static Rust through Cargo;
- add package/build inclusion checks.

### C — Resolver and pinning

- implement layered matching and effective merge;
- provide source/field provenance;
- produce adapter fingerprint;
- pin resolved adapter per turn/child;
- expose inspection diagnostics.

### D — Migrate generic existing behavior

- represent current OpenAI/Anthropic/Gemini/MiniMax/Kimi/local profile behavior as built-in adapters where appropriate;
- retain transport-specific capabilities in provider code when they depend on actual API implementation rather than model behavior;
- remove duplicated hard-coded prompt/profile inference branches or make them fallback-only.

### E — Integration and documentation

- feed prompt compiler, tool surface, recovery seam, and request builder;
- document adding/updating an adapter;
- document serving requirements as diagnostics, not guarantees;
- add narrow drift/validation checks without expanding routine CI substantially.

## 8. Failure, cancellation, restart, and contention semantics

- Invalid built-in adapter data fails compilation.
- Invalid user override fails config load with source-aware diagnostics; it does not silently downgrade.
- Unknown model falls back to a conservative generic adapter with warning/diagnostic, not failure.
- Adapter resolution is pure/bounded and immutable per turn.
- Concurrent turns share immutable compiled definitions safely.
- Runtime adapter refresh, if user overrides are reloadable, applies only to subsequent turns and follows asset/config generation semantics.
- Provider rejection of an optional transform produces a typed compatibility error; it must not retry indefinitely with the same unsupported payload.

## 9. Compatibility

- Existing `model_profile` config remains supported through a compatibility mapping or documented migration.
- Existing provider transports remain authoritative for authentication, endpoint, and wire protocol.
- Existing tool names remain canonical internally.
- Existing sessions continue with the adapter pinned at turn start.
- Adapter schema version changes require explicit migration/compatibility handling.
- Crates.io/package builds include all adapter source assets required by `build.rs`.

## 10. Required tests

Focused:

- schema serde/unknown-key rejection;
- bounds and enum validation;
- regex/exclusion validation;
- unique ID/version rules;
- deterministic file ordering/code generation;
- exact/family/provider/generic precedence;
- ambiguity rejection;
- user override layering;
- canonical/wire and argument alias reversibility;
- unsupported transform rejection;
- unknown-model fallback;
- fingerprint stability/change behavior;
- package/build asset inclusion.

Production-shaped:

- existing MiniMax profile behavior reproduced from adapter data;
- generic OpenAI/Anthropic/local model resolution;
- prompt compiler receives adapter fragments/profile;
- tool surface receives aliases/parallelism;
- provider request receives one approved typed transform;
- config reload affects only the next turn.

Negative/security:

- adapter cannot grant a denied canonical tool;
- no arbitrary file read, environment interpolation, script, or executable expression;
- pathological regex/oversized fields rejected;
- project adapter cannot shadow built-in with ambiguous ID silently;
- diagnostics do not expose provider secrets.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test model_profile::
cargo test model_adapter::
cargo test tool::contract
cargo check --workspace
cargo package --allow-dirty --no-verify
```

The package command verifies asset inclusion only; do not publish. Run one broad local library suite. Do not add a model/provider CI matrix.

## 12. Acceptance criteria

- Built-in adapter TOMLs compile into deterministic Rust during Cargo build.
- Runtime/install does not require Python or source-tree access.
- Strict schema, matching, precedence, typed transforms, aliases, diagnostics, and fingerprints exist.
- Existing profile/prompt behavior is migrated or retained only as documented conservative fallback.
- Prompt, tool surface, recovery, and provider request paths consume one pinned resolved adapter.
- Unknown models use a conservative generic adapter.
- Adapter data cannot bypass authority or execute arbitrary code.
- M008 can implement reasoning preservation and Laguna without adding a second adaptation mechanism.

## 13. Stop conditions

Stop if:

- adapter behavior requires arbitrary scripting or provider-specific untyped JSON mutation;
- Cargo packaging cannot include build-time assets without checking in generated code and no bounded packaging solution exists;
- existing model-profile config cannot be migrated compatibly;
- matching ambiguity requires an ecosystem/public-schema decision beyond this plan;
- provider transport capabilities and model behavior cannot be separated without a broader provider ADR;
- reasoning preservation must be implemented to validate the registry; defer that vertical slice to M008.

## 14. Closure evidence

Include:

- adapter schema/reference examples;
- inventory of removed/retained hard-coded branches;
- deterministic generation hashes across two builds;
- precedence/ambiguity/alias fixture results;
- package asset inclusion evidence;
- prompt/tool/provider integration evidence;
- focused and broad local verification results;
- known fallback/unsupported transform limitations;
- closure recommendation.
