# Declarative Model Adapters

## Purpose

Model behavior that varies by model family is described by versioned TOML
files embedded at build time. The adapter system provides a pure, data-only
resolution path for model profiles, tool aliases, request transforms,
recovery hints, and serving diagnostics. It replaces ad-hoc model-name
branching with a single, auditable, fingerprinted resolution surface.

## Where It Lives

| File | Role |
|------|------|
| `crates/codegg-core/assets/model-adapters/*.toml` | 7 adapter definitions (build-time source) |
| `crates/codegg-core/src/model_profile/adapter.rs` | `AdapterDefinition`, `ResolvedModelAdapter`, `resolve_adapter()` |
| `crates/codegg-core/src/model_profile/capability.rs` | Reasoning-capability record resolution, merge rule, local admission policy |
| `crates/codegg-core/src/model_profile/resolve.rs` | `ModelProfileResolver`, config override application |
| `crates/codegg-core/src/model_profile/types.rs` | `ResolvedModelProfile`, `TaskStatePolicy` |
| `crates/codegg-core/src/model_profile/policy.rs` | `push_control_instruction()` — provider-aware system message placement |
| `crates/codegg-core/src/model_profile/mod.rs` | Re-exports |
| `crates/codegg-core/build.rs` | Parses TOML files, validates, embeds source text into `$OUT_DIR` |
| `crates/codegg-config/src/schema.rs` | `ReasoningCapabilityRecord`, `ModelProfileConfig.reasoning_capability` |
| `crates/codegg-providers/src/capability.rs` | The kernel's capability machinery, reached without a second `eggpool-wire` declaration |

### Adapter TOML Files

| File | Adapter ID | Models | Profile | Capability record |
|------|-----------|--------|---------|-------------------|
| `generic.toml` | `generic` | Fallback for unknown models | `default` | none |
| `anthropic.toml` | `anthropic-frontier` | Claude/Sonnet/Opus/Haiku | `frontier_reasoning` | budget-only |
| `openai.toml` | `openai-frontier` | GPT/o1/o3/o4/Codex | `frontier_reasoning` | effort ladder |
| `google.toml` | `google-long-context` | Gemini models | `long_context_planner` | none |
| `minimax.toml` | `minimax-fast-executor` | MiniMax models | `fast_executor` | always-reasons |
| `local.toml` | `local-strict` | Ollama/Qwen/DeepSeek/Kimi | `local_strict` | none |
| `laguna.toml` | `poolside-laguna-agentic` | Poolside Laguna M/XS/S | `local_strict` | none |

"none" means the adapter declares **no** capability record, so every
dimension of that family is `Unknown` — not `Unsupported`.

## How It Works

### Build Time

`codegg-core/build.rs` parses each TOML file with `deny_unknown_fields`,
validates bounds, regular expressions, transforms, and reversible aliases.
It sorts them by adapter ID and embeds the source text as
`BUILTIN_ADAPTER_SOURCES: &[(&str, &str)]` in `$OUT_DIR/model_adapters.rs`.
Runtime operation therefore does not depend on Python or the source checkout.

### Resolution

`resolve_adapter(provider, model)` is pure and returns an immutable
`ResolvedModelAdapter`. Resolution steps:

1. Infer provider from model string if not explicit (e.g., "claude" → "anthropic")
2. Score each adapter `[[match]]` clause against the provider+model:
   - Exact model match: 400 points
   - Provider match: 200 points
   - Prefix/suffix match: 100 points
   - Regex match: 50 points
3. Sort by (score, adapter priority, adapter id), take highest
4. Merge selected adapter over `generic` adapter (inheritance)
5. Compute `effective_profile()` merging adapter profile over conservative defaults
6. Compute SHA-256 fingerprint of serialized adapter

Unknown models always select the conservative `generic` adapter.

### Adapter Inheritance

All adapters merge over `generic`. The `merge_adapter()` function
inherits profile fields, tool settings, prompt fragments, recovery policy,
server requirements, and transforms from the base. Overlay fields take
precedence; base fields fill in missing values.

### Config Override Layer

`ModelProfileResolver::resolve_adapter()` applies user config overrides
(`[model_profile.<model>]`) on top of the declarative adapter. This
enables per-model tweaks (context window, disabled tools, text tool
repair, and the conservative `orchestration_tier`) without editing adapter
TOML. The tier defaults to `SoloPreferred`; unknown models and adapters are
never promoted to `ConvergenceCapable` by inference. A project or user
profile may explicitly override the tier.

## Reasoning Capability Record

An adapter may declare what the model family accepts on the reasoning
dimensions: the accepted effort labels, whether reasoning can be turned
off, whether a fixed thinking budget is accepted, and whether the model
always reasons. The record is compiled into the binary and resolved per
turn, so an unsupported selection is refused locally instead of being
forwarded for the provider to reject.

### The Record

`ReasoningCapabilityRecord` (`crates/codegg-config/src/schema.rs`) carries
four **independent** dimensions. Each has its own status in the kernel's
vocabulary — `Supported | Unsupported | Unknown | Mixed | Conflicting` —
so an **undeclared** dimension is `Unknown`, never `Unsupported`:

| Dimension | Fields | Meaning |
|---|---|---|
| effort | `effort_status`, `accepted_effort_labels` | The accepted reasoning-effort labels — an **open, provider-scoped** string set |
| disable | `disable_status` | May reasoning be turned off at all? |
| budget | `budget_status`, `min_thinking_budget`, `max_thinking_budget` | Is a fixed token budget accepted, and within which bounds? |
| always-reasons | `always_reasons_status`, `always_reasons` | Does the model always reason? |

`Unknown` is a claim about *knowledge*, not about the model: an undeclared
dimension means "nobody has published a contract for this", which is not
the same as "the model refuses this". The kernel's fail-closed policy is
what turns that distinction into a refusal at admission time; the record
never collapses the two states into each other.

The accepted-label set is never a fixed enumeration and never fabricated.
A label is carried verbatim; a depth label is never converted into a
token budget; a bound is never invented for a model whose maximum nobody
has published.

### The Merge Rule

Adapters merge over `generic`, and the capability record merges by
**narrowing**, not by "overlay wins":

- the merged status of a dimension is the more restrictive of the two
  declared statuses;
- the merged accepted-label set is the **intersection** of the two
  declared sets, so a merge can never widen a dimension's accepted set
  beyond the baseline;
- the merged thinking-budget range is the intersection of the two
  declared ranges;
- the always-reasons property is the narrower of the two claims.

A dimension a side does not declare imposes no constraint, so the generic
baseline — which declares nothing — never erases what a specific adapter
declares.

### Resolution Order

`resolve_reasoning_capability(baseline, operator_override, overlay)`
resolves the effective capability for a model:

1. the **operator override** (`[model_profile.<model>].reasoning_capability`)
   is authoritative for the dimensions it declares, and may only narrow
   the compiled baseline;
2. the **adapter baseline** is the compiled, deterministic record;
3. the **discovery overlay** may only narrow — an overlay that declares a
   label, a bound, or an always-reasons value the effective record does
   not already carry is **rejected in full** and the rejection is
   reported, never silent;
4. **unknown** — the absence of a record.

The resolution is **read-only**: it never mutates the baseline, the
override, the overlay, or any in-flight turn state.

### The Local Admission Policy

`evaluate_reasoning_selection(...)` applies the effective capability to a
resolved selection through the kernel's `reasoning_capability_notices_neutral`
and `ReasoningCapabilityPolicy`, producing exactly one of:

| Outcome | Meaning |
|---|---|
| `Reject` | The selection is refused locally with a typed refusal naming the model and the dimension |
| `WarnDrop` | The unsupported dimension is dropped and a notice records **what was dropped** |
| `AllowWithWarning` | The selection proceeds with a recorded warning |
| `Allow` | The selection proceeds unchanged |

The kernel's default policy is **fail-closed**: `unsupported → Reject`,
`unknown → Reject`, `mixed → WarnDrop`. This milestone relies on that
default. Its consequence is deliberate: a selection on a dimension the
effective capability does not declare is refused locally rather than
forwarded optimistically.

The kernel's neutral capability type is **status-only** — it carries no
accepted-label list and never validates a label — so CodeGG owns the
accepted-label set and the budget bounds, and the kernel supplies the
per-dimension decision.

### The Shipped Baseline

Only families with a published contract carry a record:

| Family | Record | Published contract relied on |
|---|---|---|
| `minimax` | effort `supported` = `low, medium, high, xhigh, max`; disable `unsupported`; budget `unknown`; always-reasons `true` | MiniMax-M3.1 accepts those `reasoning_effort` values, rejects `none` and rejects `thinking.type = "disabled"` with "requires adaptive thinking" |
| `anthropic` | effort `unsupported`; disable `supported`; budget `supported`, min `1024`; always-reasons `false` | Claude's extended thinking is a fixed token budget (documented minimum 1024 tokens); the Messages API has no `reasoning_effort` parameter |
| `openai` | effort `supported` = `minimal, low, medium, high`; disable `supported`; budget `unsupported`; always-reasons `false` | OpenAI's reasoning control is `reasoning_effort`; the ladder varies per model inside the family, so the family contract is the union of the published values |

`google`, `local`, `laguna`, and `generic` carry **no** record. Their
published contracts vary materially across the versions inside one
family, and a family-scoped record that guessed at them would be a
fabrication. An undeclared family is `Unknown`, not `Unsupported`.

## Key Types & APIs

### AdapterDefinition (`adapter.rs:14`)

```rust
pub struct AdapterDefinition {
    pub schema_version: u32,           // Must be 1
    pub adapter: AdapterMetadata,      // id, version, priority, description
    pub r#match: Vec<AdapterMatch>,    // Provider/model matching rules
    pub profile: AdapterProfile,       // Model capability profile
    pub tools: AdapterTools,           // Tool format, aliases, arguments
    pub prompt: AdapterPrompt,         // Prompt fragments, system/control roles
    pub recovery: RecoveryPolicy,      // Malformed tool retry, no-action limits
    pub server_requirements: ServerRequirements, // Parser requirements
    pub transforms: Vec<RequestTransform>,       // Bounded request mutations
}
```

### AdapterMatch (`adapter.rs:42`)

```rust
pub struct AdapterMatch {
    pub provider: Vec<String>,
    pub exact_model: Vec<String>,
    pub model_prefix: Option<String>,
    pub model_suffix: Option<String>,
    pub model_regex: Option<String>,
    pub exclude_regex: Option<String>,
}
```

### ResolvedModelAdapter (`adapter.rs:187`)

Immutable, fully-resolved adapter returned by `resolve_adapter()`.
Contains: `profile`, `adapter_id`, `adapter_version`, `fingerprint`,
`tool_format`, `tool_choice`, `max_parallel_tools`, `require_structured_calls`,
`text_tool_repair`, `tool_aliases`, `argument_aliases`, `prompt_fragments`,
`prompt_system_role`, `prompt_control_role`, `recovery`, `server_requirements`,
`transforms`.

### ResolvedModelProfile (`types.rs:9`)

Core model capability profile:
- `prompt_profile: PromptProfileKind` — selects execution policy defaults
- `context_window`, `max_output_tokens`
- Reliability tiers: `tool_call_reliability`, `instruction_adherence`, `patch_reliability`
- Behavioral flags: `supports_late_system_messages`, `prefers_user_control_messages`,
  `prefers_small_patches`, `requires_explicit_tool_contract`, `requires_post_tool_continue_nudge`
- `max_parallel_tools`, `preferred_tools`, `disabled_tools`
- `task_state_policy: TaskStatePolicy` — todo injection behavior

### RequestTransform (`adapter.rs:150`)

Closed, typed set of request mutations:

```rust
pub enum RequestTransform {
    SetRequestField { field, value },
    RemoveRequestField { field },
    RenameToolArgument { field, value },
    SetSystemRole { field, value },
    SetToolChoice { field, value },
    SetMaxParallelTools { field, value },
    SetThinkingParameter { field, value },
    RequireLateSystemMessages { field },
    RequireContinueNudge { field },
}
```

Built-in adapter TOML is parsed at build time; unknown operations,
conflicting targets, nested paths, and authority fields are rejected.

### Text Repair Profiles

The adapter's `tools.text_tool_repair` field selects a bounded textual
repair grammar for models that emit tool calls as prose:

| Profile | Description |
|---------|-------------|
| `hermes_xml` | Hermes XML-style tool calls |
| `invoke_json` | JSON envelope invocation |
| `raw_json_envelope` | Raw JSON with envelope wrapping |

Repair is opt-in per adapter. Structured provider calls remain canonical.
The repair function is bounded: max 64KB input, max 8 repaired calls.

### Serving Diagnostics

`serving_requirement_diagnostics()` compares adapter requirements against
actual serving metadata and returns diagnostic strings for mismatched
tool-call parsers, reasoning parsers, or auto-tool-choice settings.

## Configuration Surface

| Config Key | Effect |
|------------|--------|
| `[model_profile.<model>]` | Override any `ResolvedModelProfile` field |
| `model_profile.<model>.text_tool_repair` | Enable textual tool-call repair |
| `model_profile.<model>.disabled_tools` | Remove specific tools for a model |
| `model_profile.<model>.preferred_tools` | Preferred tool ordering |
| `model_profile.<model>.orchestration_tier` | `solo_preferred`, `delegation_capable`, or `convergence_capable` |
| `model_profile.<model>.reasoning_capability` | Override the reasoning-capability record for a model (narrowing only) |

## Invariants & Gotchas

- **Adapter data is policy only**: It cannot execute code, grant
  permissions, or replace provider authentication/transport.
- **Generic fallback is required**: Unknown models always select the
  conservative `generic` adapter.
- **Priority is tie-breaker**: Match score determines primary ranking;
  adapter priority breaks ties. Adapter ID is the final tie-breaker.
- **Fingerprint is deterministic**: SHA-256 over serialized adapter;
  stable across unchanged builds, independent of wall-clock time.
- **Transforms are a closed set**: Only the enum variants above are
  accepted. Unknown operations are rejected at parse time.
- **Tool aliases are bidirectional**: The resolver keeps both directions
  so provider wire calls can be normalized to canonical names before
  permission and broker execution.
- **Provider wire policy is projected once**: The owning CodeGG layer copies
  aliases, argument aliases, and explicitly declared private-reasoning
  permission into `ProviderRequestContext.wire_policy`. Provider modules
  consume this bounded value and do not independently match model names.
  Standard OpenAI Chat, Anthropic Messages, and Gemini GenerateContent
  encoding/stream decoding use `codegg-providers::wire`. OpenAI-compatible
  aliases are restored before tool authorization, and private reasoning
  round-trip/output remains gated by the resolved policy.
- **Text tool repair is model-specific**: Controlled by adapter; never
  a generic text-to-action parser.
- **Unknown is not Unsupported**: An undeclared reasoning dimension means
  "nobody published a contract for this", not "the model refuses it".
  The kernel's fail-closed policy refuses it at admission; the record
  never collapses the two.
- **The capability record only narrows**: A merge, an operator override,
  and a discovery overlay may all remove from a dimension's accepted set
  and none of them may widen it. A discovery overlay that tries is
  rejected in full and the rejection is reported.
- **The build script mirrors the profile shape**: `build.rs` declares its
  own copy of `AdapterProfile` and of `ReasoningCapabilityRecord`. A
  one-sided change between them surfaces as a `deny_unknown_fields` error
  against an adapter asset, which reads like a data problem.
  `scripts/check_model_profile_capability_mirror.py` asserts that the two
  declare the same field set, and the `codegg-core` test suite asserts it
  again from the runtime side.
- **The kernel is reached through one declaration**: `eggpool-wire` is
  declared once, in `crates/codegg-providers/Cargo.toml`. Cargo resolves a
  git dependency's path dependencies from the first declaration it
  encounters, so a second declaration would give the workspace two copies
  of the crate and two `WireSurface` types. `codegg-core` therefore
  reaches the kernel through `codegg_providers::capability`.

## Testing

- Unit tests in `crates/codegg-core/src/model_profile/adapter.rs::tests`
- Capability tests in `crates/codegg-core/src/model_profile/capability.rs::tests`
- Narrowest run:
  ```bash
  cargo test -p codegg-core model_profile::
  ```

## Related Docs

- [agent-tool-surface.md](agent-tool-surface.md) — resolved tool surface
- [agent.md](agent.md) — agent loop, execution policy
- [provider.md](provider.md) — provider trait and registry

## Source Verification

Verified 2026-10-06 against `crates/codegg-core/src/model_profile/{adapter.rs,types.rs}`.
- Corrected 5 stale line refs: `AdapterDefinition` `10`->`14`, `AdapterMatch`
  `38`->`42`, `RequestTransform` `146`->`150`, `ResolvedModelAdapter`
  `183`->`187`, `ResolvedModelProfile` `8`->`9`. All five were off by 2-4
  lines; the relative ordering in this document was already correct.
