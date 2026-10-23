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

| File | Adapter ID | Priority | Models | Profile | Capability record |
|------|-----------|----------|--------|---------|-------------------|
| `generic.toml` | `generic` | -1000 | Fallback for unknown models | `default` | none |
| `anthropic.toml` | `anthropic-frontier` | 10 | Claude/Sonnet/Opus/Haiku | `frontier_reasoning` | budget-only |
| `openai.toml` | `openai-frontier` | 10 | GPT/o1/o3/o4/Codex | `frontier_reasoning` | effort ladder |
| `google.toml` | `google-long-context` | 10 | Gemini models | `long_context_planner` | none |
| `minimax.toml` | `minimax-fast-executor` | 20 | MiniMax models | `fast_executor` | always-reasons |
| `local.toml` | `local-strict` | 5 | Ollama/Qwen/DeepSeek/Kimi | `local_strict` | none |
| `laguna.toml` | `poolside-laguna-agentic` | 40 | Poolside Laguna M/XS/S | `local_strict` | none |

`generic` carries `priority = -1000` — every other adapter outranks it,
and it is the merge base for all of them. `poolside-laguna-agentic`
carries the highest priority (40).

"none" means the adapter declares **no** capability record, so every
dimension of that family is `Unknown` — not `Unsupported`.

## How It Works

### Build Time

`codegg-core/build.rs` parses each TOML file with `deny_unknown_fields`,
validates bounds, regular expressions, transforms, and reversible aliases.
It sorts the files by path, then embeds the source text as
`BUILTIN_ADAPTER_SOURCES: &[(&str, &str)]` in `$OUT_DIR/model_adapters.rs`.
Runtime operation therefore does not depend on Python or the source checkout.

What the build rejects (a build failure, not a runtime skip):

- `schema_version != 1`; empty or over-long (`> 96`) `adapter.id`;
  duplicate adapter ids; `adapter.version` outside `1..=1000`;
  `description` over 512 chars.
- An adapter with no `[[match]]` clause; a `[[match]]` clause with more
  than 32 total provider/exact-model keys.
- A `model_regex` / `exclude_regex` that does not compile, or is over
  512 chars.
- Duplicate match clauses (ambiguous matches are rejected by a
  `(provider, exact_model, prefix, suffix, regex, priority)` key set).
- `context_window > 10_000_000`; `max_output_tokens > 1_000_000`;
  `tools.max_parallel` outside `1..=64`.
- A transform with an empty or over-long (`> 128`) field, a field
  containing `.`/`[`/`]` (nested paths), a field named
  `authorization` / `headers` / `endpoint` / `url` / `tools` /
  `permissions` (authority and transport are not adapter-adjustable),
  or a duplicate/conflicting transform.
- `SetRequestField` on a field other than `reasoning_content`;
  `SetThinkingParameter` on a field other than `enable_thinking`.
- Tool or argument aliases whose wire names collide (the aliases must
  stay **reversible**).

### Resolution

`resolve_adapter(provider, model)` is pure and returns an immutable
`ResolvedModelAdapter`. Resolution steps:

1. Infer provider from the model string if not explicit. A `provider/model`
   form is used verbatim; otherwise the model id is matched by family
   substring (`gpt`/`o1`/`o3`/`o4`/`codex` → `openai`;
   `claude`/`sonnet`/`opus`/`haiku` → `anthropic`; `gemini` → `google`;
   `minimax` → `minimax`; `qwen`/`qwq`/`deepseek`/`kimi` → `local`).
   An unmatched model falls back to the model string itself as the
   provider key.
2. Score each adapter `[[match]]` clause against the provider+model:
   - Exact model match: 400 points
   - Provider match: 200 points
   - Prefix/suffix match: 100 points
   - Regex match: 50 points
   A clause with **no** match key scores 0 — the score is a sum of the
   keys the clause *carries*, not a rank of the clause's specificity.
   `exclude_regex` vetoes a clause outright; an invalid regex is treated
   as a non-match (and logged), never as a match.
3. Sort by (score, adapter priority, adapter id), take highest
4. Merge selected adapter over `generic` adapter (inheritance)
5. Compute `effective_profile()` merging adapter profile over conservative defaults
6. Compute SHA-256 fingerprint of serialized adapter

Unknown models always select the conservative `generic` adapter. If the
`generic` adapter itself is somehow missing from the embedded set, the
resolver logs an error and returns `fallback_resolved_adapter(model)` —
a synthetic `generic` profile with fingerprint `fallback:generic` and
source layer `fallback:generic`, so resolution still succeeds.

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

The lookup is **suffix-tolerant**: `find_config_override` tries the full
model string first, then the segment after the first `/`, then the segment
after the second `/`. So `[model_profile.gpt-4o]` applies to both
`gpt-4o` and `openai/gpt-4o`.

`apply_config_override` copies each `Option` field only when the config
supplies it, so an unset config key never erases an adapter-declared
value. `text_tool_repair` is the one field that also lives on the
*adapter* rather than the profile: `resolve_adapter` copies
`cfg.text_tool_repair` onto the resolved adapter, so a config override can
turn repair on for a model whose adapter does not declare it.

`resolve()` (profile-only) and `resolve_adapter()` (full adapter) are two
entry points. A caller that needs tool/request adaptation must keep the
returned adapter — resolving the profile a second time would drop the
adapter's aliases, transforms, and recovery policy.

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
`source_layers`, `tool_format`, `tool_choice`, `max_parallel_tools`,
`require_structured_calls`, `text_tool_repair`, `tool_aliases`,
`argument_aliases`, `prompt_fragments`, `prompt_system_role`,
`prompt_control_role`, `recovery`, `server_requirements`, `transforms`.

`source_layers` records which layers produced the resolution — e.g.
`["builtin:generic", "builtin:anthropic-frontier"]` for an inherited
adapter, `["builtin:generic"]` for the generic one itself, and
`["fallback:generic"]` for the synthetic fallback.

### ResolvedModelProfile (`types.rs:9`)

Core model capability profile:
- `model`, `prompt_profile: PromptProfileKind` — selects execution policy defaults
- `family`
- `context_window`, `max_output_tokens`
- Reliability tiers: `tool_call_reliability`, `instruction_adherence`, `patch_reliability`
- `orchestration_tier: OrchestrationTier`
- Behavioral flags: `supports_late_system_messages`, `prefers_user_control_messages`,
  `prefers_small_patches`, `requires_explicit_tool_contract`, `requires_post_tool_continue_nudge`
- `default_reasoning_effort`, `default_thinking_budget`
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

The profile names are parsed by
`codegg_providers::text_tool_parser::TextRepairProfile::parse`; an
unrecognized name is `TextRepairError::UnknownProfile`, not a silent
no-op.

Repair is opt-in per adapter. Structured provider calls remain canonical.
The repair function is bounded: max 64KB input
(`MAX_REPAIR_INPUT_BYTES = 64 * 1024`), max 8 repaired calls
(`MAX_REPAIRED_CALLS = 8`). It only runs when the provider's stop reason
is one of `stop` / `tool_calls` / `length` / `max_tokens`; any other stop
reason returns `Ok(None)` and leaves the text alone.

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
  permissions, or replace provider authentication/transport. The build
  rejects any transform that names an authority or transport field
  (`authorization`, `headers`, `endpoint`, `url`, `tools`, `permissions`).
- **Generic fallback is required**: Unknown models always select the
  conservative `generic` adapter, and `generic` is the merge base for
  every other adapter.
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
- **The config override layer is additive, not replacing**: an unset
  config key leaves the adapter's value in place, and the config lookup
  is suffix-tolerant so a bare `[model_profile.<name>]` also matches
  `provider/<name>`.
- **One resolution, one owner**: a caller that resolves the profile
  separately from the adapter loses the adapter's aliases, transforms,
  and recovery policy — keep the returned `ResolvedModelAdapter`.

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
  and `crates/codegg-core/src/model_profile/resolve.rs::tests`
- Narrowest run:
  ```bash
  cargo test -p codegg-core model_profile::
  ```

## Related Docs

- [agent-tool-surface.md](agent-tool-surface.md) — resolved tool surface
- [agent.md](agent.md) — agent loop, execution policy
- [provider.md](provider.md) — provider trait and registry
- [config.md](config.md) — the `[model_profile.<model>]` config schema

## Source Verification

Verified 2026-10-23 against
`crates/codegg-core/src/model_profile/{adapter,resolve,types,policy}.rs`,
`crates/codegg-core/build.rs`,
`crates/codegg-core/assets/model-adapters/*.toml` (all 7),
`crates/codegg-providers/src/text_tool_parser.rs`, and
`crates/codegg-config/src/schema.rs`.

Corrections made in this pass:
- **Added the adapter `priority` column.** The doc's TOML table listed the
  IDs and profiles but not the priorities, which are load-bearing:
  `generic` is `-1000` (merge base, lowest), `local-strict` 5,
  `anthropic-frontier`/`openai-frontier`/`google-long-context` 10,
  `minimax-fast-executor` 20, `poolside-laguna-agentic` 40.
- **Corrected the build-time description.** The doc said the build "sorts
  them by adapter ID"; it sorts the **file paths**, and the embedded
  array is in file order. Added the full list of build-time rejections
  (bounds, duplicate ids, ambiguous match clauses, nested/authority
  transform fields, the `reasoning_content` / `enable_thinking`
  field whitelists, and alias reversibility) — these are build
  *failures*, not runtime skips.
- **Corrected the scoring description.** The score is a **sum of the
  keys the clause carries**, not a rank of specificity: a clause with no
  match key scores 0, and a clause with both `exact_model` and `provider`
  scores 600. Added that `exclude_regex` vetoes a clause and that an
  invalid regex is a non-match, not a match.
- **Added the provider-inference table** (`gpt`/`o1`/`o3`/`o4`/`codex` →
  `openai`, `claude`/`sonnet`/`opus`/`haiku` → `anthropic`, `gemini` →
  `google`, `minimax` → `minimax`, `qwen`/`qwq`/`deepseek`/`kimi` →
  `local`), and that an explicit `provider/model` prefix is used verbatim.
- **Added the synthetic fallback** (`fallback_resolved_adapter`) for the
  case where `generic` itself is missing from the embedded set.
- **Added `source_layers`** to the `ResolvedModelAdapter` field list —
  it was missing from a list that otherwise enumerated every field.
- **Added `model`, `family`, `orchestration_tier`,
  `default_reasoning_effort`, and `default_thinking_budget`** to the
  `ResolvedModelProfile` field list; the doc's list omitted five of the
  struct's fields.
- **Added the config-override lookup semantics**: suffix-tolerant
  matching (full string, then after the first `/`, then after the
  second `/`), the additive copy semantics of
  `apply_config_override`, and that `text_tool_repair` is an *adapter*
  field the resolver copies onto the resolved adapter.
- **Added the `resolve()` vs `resolve_adapter()` distinction** and why a
  caller must keep the adapter.
- **Added the text-repair stop-reason gate** and the named bounds
  (`MAX_REPAIR_INPUT_BYTES = 64 * 1024`, `MAX_REPAIRED_CALLS = 8`), and
  that an unrecognized profile name is an error rather than a silent
  no-op.
- **Added the `[model_profile.<model>]` link to `config.md`** in Related
  Docs.

Confirmed correct as written: the 7 adapter files and their IDs,
models, and profiles; `AdapterDefinition` `adapter.rs:14`,
`AdapterMatch` `adapter.rs:42`, `RequestTransform` `adapter.rs:150`,
`ResolvedModelAdapter` `adapter.rs:187`, `ResolvedModelProfile`
`types.rs:9`; the 4 `RequestTransform` variant families and the closed
set; the `merge_adapter()` inheritance direction (overlay over base);
the three repair profile names; the `serving_requirement_diagnostics()`
contract; the `orchestration_tier` default of `SoloPreferred` and the
`SoloPreferred`/`DelegationCapable`/`ConvergenceCapable` vocabulary; the
`push_control_instruction()` role; and the 64KB / 8-call repair bounds.
