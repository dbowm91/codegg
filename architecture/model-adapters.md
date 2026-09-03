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
| `crates/codegg-core/src/model_profile/resolve.rs` | `ModelProfileResolver`, config override application |
| `crates/codegg-core/src/model_profile/types.rs` | `ResolvedModelProfile`, `TaskStatePolicy` |
| `crates/codegg-core/src/model_profile/policy.rs` | `push_control_instruction()` — provider-aware system message placement |
| `crates/codegg-core/src/model_profile/mod.rs` | Re-exports |
| `crates/codegg-core/build.rs` | Parses TOML files, validates, embeds source text into `$OUT_DIR` |

### Adapter TOML Files

| File | Adapter ID | Models | Profile |
|------|-----------|--------|---------|
| `generic.toml` | `generic` | Fallback for unknown models | `default` |
| `anthropic.toml` | `anthropic-frontier` | Claude/Sonnet/Opus/Haiku | `frontier_reasoning` |
| `openai.toml` | `openai-frontier` | GPT/o1/o3/o4/Codex | `frontier_reasoning` |
| `google.toml` | `google-long-context` | Gemini models | `long_context_planner` |
| `minimax.toml` | `minimax-fast-executor` | MiniMax models | `fast_executor` |
| `local.toml` | `local-strict` | Ollama/Qwen/DeepSeek/Kimi | `local_strict` |
| `laguna.toml` | `poolside-laguna-agentic` | Poolside Laguna M/XS/S | `local_strict` |

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

## Key Types & APIs

### AdapterDefinition (`adapter.rs:10`)

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

### AdapterMatch (`adapter.rs:38`)

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

### ResolvedModelAdapter (`adapter.rs:183`)

Immutable, fully-resolved adapter returned by `resolve_adapter()`.
Contains: `profile`, `adapter_id`, `adapter_version`, `fingerprint`,
`tool_format`, `tool_choice`, `max_parallel_tools`, `require_structured_calls`,
`text_tool_repair`, `tool_aliases`, `argument_aliases`, `prompt_fragments`,
`prompt_system_role`, `prompt_control_role`, `recovery`, `server_requirements`,
`transforms`.

### ResolvedModelProfile (`types.rs:8`)

Core model capability profile:
- `prompt_profile: PromptProfileKind` — selects execution policy defaults
- `context_window`, `max_output_tokens`
- Reliability tiers: `tool_call_reliability`, `instruction_adherence`, `patch_reliability`
- Behavioral flags: `supports_late_system_messages`, `prefers_user_control_messages`,
  `prefers_small_patches`, `requires_explicit_tool_contract`, `requires_post_tool_continue_nudge`
- `max_parallel_tools`, `preferred_tools`, `disabled_tools`
- `task_state_policy: TaskStatePolicy` — todo injection behavior

### RequestTransform (`adapter.rs:146`)

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
- **Text tool repair is model-specific**: Controlled by adapter; never
  a generic text-to-action parser.

## Testing

- Unit tests in `crates/codegg-core/src/model_profile/adapter.rs::tests`
- Narrowest run:
  ```bash
  cargo test -p codegg-core model_profile::
  ```

## Related Docs

- [agent-tool-surface.md](agent-tool-surface.md) — resolved tool surface
- [agent.md](agent.md) — agent loop, execution policy
- [provider.md](provider.md) — provider trait and registry
