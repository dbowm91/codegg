# LSP Phase 11 Plan: Routing and Model-Tier-Aware LSP Context Policy

Status date: 2026-06-26
Phase type: context policy / model routing / budget governance
Prerequisites: Phase 10 bounded semantic operations implemented or sufficiently stable.

## Purpose

Phase 11 should turn the current model-tier-aware rendering foundation into a deliberate LSP context policy layer. The repo already has `ModelTier`, `LspContextRenderConfig`, `render_lsp_context_for_agent`, production agent-path context assembly, and Phase 7 recipe defaults. Phase 11 should make LSP context selection, compression, fallback, and routing model-aware, workflow-aware, and budget-aware.

The goal is not to make the model router dependent on LSP. The goal is to ensure LSP context injected into agent prompts is proportionate to the model tier, task risk, token budget, lifecycle state, and semantic operation type.

## Current baseline

Existing pieces likely include:

- `egglsp::ModelTier::{Small, Workhorse, Frontier}`.
- `LspContextRenderConfig` with per-section byte limits and preview inclusion flag.
- `render_lsp_context_for_agent()`.
- `RecipeSettings::for_tier()` and `default_settings_for_recipe()`.
- Agent turn runtime input capable of carrying workflow metadata and inferred/explicit model tier.
- Production path that can assemble LSP context for an agent turn when metadata is available.

Phase 11 should centralize policy so future operations do not hardcode ad-hoc tier logic.

## Non-goals

Do not add new LSP protocol operations. That belongs to Phase 10.

Do not add persistent semantic cache. That belongs to Phase 12.

Do not make routing pick models solely from LSP availability.

Do not inject large LSP context by default for small/cheap models.

Do not silently drop all LSP context without a note when a task requested it.

Do not create a separate context packet model.

## Workstream 1: define the LSP context policy object

### Problem

Tier-aware behavior is currently split across render configs, recipe settings, and production agent assembly. This can drift as new operations arrive.

### Target files

- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `src/agent/turn_runtime.rs`
- `src/tool/lsp.rs`
- config schema if policy is user-configurable
- docs in `architecture/lsp.md`

### Proposed types

Create a single policy struct, ideally in `egglsp` if independent of Codegg routing, or in `src/agent` if it depends on provider/model metadata.

Candidate shape:

```rust
pub struct LspContextPolicy {
    pub model_tier: ModelTier,
    pub workflow: LspWorkflowRecipe,
    pub task_risk: LspTaskRisk,
    pub lifecycle_state: Option<LspOperationalState>,
    pub token_budget_hint: Option<usize>,
    pub max_context_bytes: usize,
    pub include_cross_file: bool,
    pub include_hierarchy: bool,
    pub include_previews: bool,
    pub stale_evidence_policy: StaleEvidencePolicy,
    pub unavailable_policy: LspUnavailablePolicy,
}
```

Avoid overfitting names; the important point is to centralize policy decisions.

### Implementation steps

1. Inventory all model-tier branching in LSP context code.
2. Define the minimal policy struct and enums:
   - stale evidence: include-with-warning, omit, require-fresh,
   - unavailable: omit, note-only, fail-required,
   - risk: low, normal, high, security-sensitive.
3. Add conversion from policy to:
   - `RecipeSettings`,
   - `LspContextBudget`,
   - `LspContextRenderConfig`.
4. Keep existing defaults behavior-compatible initially.
5. Add tests that old tier defaults map to equivalent new policies.

### Acceptance criteria

- Tier, workflow, risk, stale, and budget decisions have one policy entry point.
- Existing recipe/render behavior is preserved unless explicitly changed.
- Tests cover small/workhorse/frontier policy derivation.

## Workstream 2: model classification and override rules

### Problem

Production context policy depends on knowing whether a model is small, workhorse, or frontier. This should be deterministic, overridable, and inspectable.

### Target files

- `src/agent/turn_runtime.rs`
- model/provider metadata code
- config schema for model tier overrides if present
- docs

### Rules

Suggested precedence:

1. Explicit per-request tier override.
2. User config per model ID or model family.
3. Provider/model metadata from router/provider registry.
4. Heuristic family matching.
5. Default to `Workhorse`.

### Implementation steps

1. Locate current model-tier inference.
2. Add a small policy resolver that returns:
   - model tier,
   - source of decision,
   - confidence/notes.
3. Add config override support only if it fits existing config patterns. Do not create heavy config migration.
4. Add debug/status output such as `LSP policy: tier=workhorse source=model-family`.
5. Add tests for explicit override, config override, known family, unknown model fallback.

### Acceptance criteria

- Model tier classification is deterministic and explainable.
- Unknown models fall back safely.
- Users can override misclassified models if config support is added.

## Workstream 3: workflow-specific policy defaults

### Problem

Different workflows need different semantic breadth. Repair, review, security, hunk navigation, preview suggestion, and Phase 10 packets should not all use the same render config.

### Target files

- `crates/egglsp/src/workflow_recipes.rs`
- `crates/egglsp/src/context_renderer.rs`
- `src/tool/lsp.rs`
- tests

### Default policy matrix

Recommended defaults:

| Workflow | Small | Workhorse | Frontier |
|---|---|---|---|
| repair_local | same-file diagnostics, symbol, definition | add limited refs | add hover/signature and preview hints |
| repair_hunk | hunk-local only | hunk + limited cross-file refs | hunk + bounded hierarchy |
| review_diff | diagnostics + changed symbols | add refs/definitions | add impact/call/interface packets |
| security_review_enriched | deterministic review + concise LSP notes | add risk-linked refs/defs | add shallow call expansion |
| preview_suggestion | summaries only | previews only if cheap | previews with metadata and warnings |
| impact_analysis | minimal same-file | bounded cross-file | richer refs/hierarchy under caps |
| test_failure_repair | failure-local diagnostics | add related files | add interface/call-neighborhood if relevant |

### Implementation steps

1. Encode workflow/tier defaults in a table-like function, not scattered match statements.
2. Ensure every workflow has explicit small/workhorse/frontier defaults.
3. Add tests for each row where feasible.
4. Ensure security-sensitive workflows default to fresh/stale warning behavior and do not suppress lifecycle notes.
5. Render truncation and policy decisions in debug/notes when context is omitted due to policy.

### Acceptance criteria

- Workflow defaults are centralized and tested.
- Small-tier outputs cannot accidentally include large cross-file context.
- Frontier-tier outputs remain bounded.

## Workstream 4: token/context budget integration

### Problem

LSP context should adapt to available prompt budget. A small model near context limit should not receive the same LSP packet as a frontier model with ample space.

### Target files

- agent prompt assembly / turn runtime
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/context.rs`
- context budgeting utilities if present

### Implementation steps

1. Identify whether agent prompt assembly already computes remaining context/token budget.
2. Add an optional `token_budget_hint` or byte-budget hint into the LSP context policy resolver.
3. Map token budget to `max_bytes`, section bytes, and item caps.
4. Define hard minimum behavior:
   - if budget is too low, emit a compact status/fallback note instead of full evidence,
   - never exceed hard byte cap,
   - do not drop stale/lifecycle warning if any LSP evidence is included.
5. Add tests for high, normal, low, and near-zero budget behavior.

### Acceptance criteria

- LSP context respects available prompt budget.
- Low-budget mode still communicates critical lifecycle/stale warnings.
- Renderer byte caps are enforced.

## Workstream 5: stale/unavailable policy

### Problem

Different tasks tolerate stale evidence differently. Routine editing can include possibly stale diagnostics with warnings; security review and preview apply should be stricter.

### Target files

- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `src/tool/lsp.rs`
- tests

### Proposed policies

- `IncludeWithWarning`: default for repair/review.
- `OmitStale`: for low-confidence stale evidence in small contexts.
- `RequireFresh`: for preview apply handoff, security-sensitive final claims, and anything that would imply code is clean.
- `NoteOnlyWhenUnavailable`: for opportunistic LSP.
- `FailWhenRequired`: for explicit required LSP operations.

### Implementation steps

1. Add stale-evidence policy enum.
2. Apply policy before rendering or during collection, whichever preserves provenance best.
3. When stale evidence is omitted, include a note saying it was omitted.
4. For required modes, return a structured failure rather than empty context.
5. Add tests for all policies.

### Acceptance criteria

- Stale evidence handling is consistent across workflows.
- Omitted stale evidence leaves an auditable note.
- Security/review workflows do not overstate stale evidence.

## Workstream 6: routing feedback and observability

### Problem

Users need to understand why LSP context was included, compressed, omitted, or expanded. This helps debug model behavior and context bloat.

### Target files

- `src/tool/lsp.rs`
- TUI status/detail areas
- logs/tracing
- docs

### Implementation steps

1. Add concise debug notes to rendered context or tracing:
   - policy name,
   - model tier,
   - workflow,
   - included item counts,
   - omitted item counts,
   - stale policy,
   - budget cap.
2. Add `/lsp-policy` or include policy summary in `/lsp-status --detail` if Phase 9 command suite exists.
3. Avoid noisy logs by default; use debug/trace for detailed decisions.
4. Add tests for policy summary string.

### Acceptance criteria

- Developers can inspect why LSP context was shaped a certain way.
- Users can see broad tier/workflow policy without reading code.

## Workstream 7: configuration surface

### Problem

Users may need to tune LSP context policy for local models, expensive frontier models, or low-context providers.

### Target files

- config schema
- config docs
- `architecture/lsp.md`

### Suggested config keys

Keep config minimal:

```toml
[lsp.context_policy]
default_tier = "workhorse"
max_bytes_small = 4096
max_bytes_workhorse = 12288
max_bytes_frontier = 32768
include_previews_for_frontier = true
stale_evidence = "include_with_warning"

[lsp.model_tiers]
"mini" = "small"
"sonnet" = "workhorse"
"gpt-5.5" = "frontier"
```

Only add config if existing config patterns make this straightforward. Otherwise keep defaults hardcoded and document future config.

### Acceptance criteria

- Either minimal config exists and is tested, or config is explicitly deferred.
- Defaults are safe without user config.

## Documentation

Update:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- config docs if config added.

Document:

- tier classification,
- policy precedence,
- workflow defaults,
- stale/unavailable behavior,
- budget behavior,
- debug/status output.

## Test matrix

Required focused tests:

```bash
cargo fmt --check
cargo test -p egglsp context_renderer
cargo test -p egglsp workflow_recipes
cargo test --test phase5_context_integration lsp
```

Recommended broader checks:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Final acceptance criteria

Phase 11 is complete when:

- LSP context shaping has a central policy resolver,
- model tier classification is deterministic and override-capable or explicitly documented,
- workflow/tier defaults are centralized and tested,
- token/byte budget pressure changes rendered context predictably,
- stale/unavailable policy is explicit and tested,
- users/developers can inspect policy decisions,
- no new packet model or unbounded LSP surface is introduced.
