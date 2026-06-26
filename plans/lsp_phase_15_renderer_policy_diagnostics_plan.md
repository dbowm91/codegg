# LSP Phase 15 Plan: Renderer-Policy Unification and Context Diagnostics

Status date: 2026-06-26
Phase type: context policy / renderer correctness / diagnostics UX
Prerequisites: Phase 14 workflow UX substantially complete.

## Purpose

Phase 15 should close the remaining gap between policy, recipes, rendering, and diagnostics. The repo already has `LspContextPolicy`, `RecipeSettings`, `LspContextRenderConfig`, workflow recipes, cache-hit notes, stale evidence, and truncation metadata. The remaining work is to make policy effects testable and inspectable without bloating normal agent prompts.

This phase should also fix the known Phase 10 notes-text bug around impact-analysis reference-cap messaging.

## Current baseline

Known baseline:

- `LspContextPolicy` centralizes model tier, workflow, task risk, lifecycle, token budget, stale policy, unavailable policy, and feature flags.
- `RecipeSettings` carries workflow-oriented feature flags such as cross-file and hierarchy inclusion.
- `LspContextRenderConfig` controls rendering shape but currently does not expose every policy feature flag.
- `policy_summary()` exists and is appended in some agent-facing context paths.
- `LspContextPacket` includes item counts, freshness, notes, truncation metadata, cache-hit notes, and provenance.

## Non-goals

Do not add new recipes or semantic operations.

Do not add disk cache.

Do not significantly increase normal prompt size.

Do not make policy diagnostics mandatory in every agent prompt.

Do not add another context packet model.

## Workstream 1: fix known Phase 10 cap-note bug

### Problem

The roadmap documents an inverted comparison in impact-analysis note text: it emits a `references capped` note when references are not actually capped. The underlying reference count and budget enforcement are documented as correct, but the note is misleading.

### Target files

- `crates/egglsp/src/evidence_collector.rs`
- tests in `crates/egglsp` for impact analysis
- docs/changelog

### Implementation steps

1. Locate the impact-analysis reference cap logic.
2. Compare original reference count to the effective cap, not capped count to unrelated budget.
3. Emit cap note only when original count exceeds cap.
4. Include both included and total counts when known.
5. Add tests for capped and uncapped cases.

### Acceptance criteria

- No cap note appears when references are not capped.
- Cap note appears when references are capped.
- Existing budget enforcement remains unchanged.

## Workstream 2: decide renderer feature-flag ownership

### Problem

`LspContextPolicy` includes `include_cross_file` and `include_hierarchy`, and `RecipeSettings` propagates them. `LspContextRenderConfig` does not currently expose those fields. This may be fine, but the boundary must be deliberate.

### Options

Option A: extend `LspContextRenderConfig`.

Add:

- `include_cross_file: bool`,
- `include_hierarchy: bool`,
- `include_policy_diagnostics: bool`,
- possibly `include_cache_notes: bool`.

Option B: document renderer config as output-shape only.

Keep cross-file/hierarchy feature flags in `RecipeSettings`; renderer only renders whatever packet contains. Add docs/tests proving this is intentional.

### Recommendation

Prefer Option A only if renderer actively filters/sections evidence based on these flags. Prefer Option B if recipe collection already controls inclusion and the renderer should remain a pure packet formatter.

### Target files

- `crates/egglsp/src/context_policy.rs`
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `architecture/lsp.md`
- tests

### Acceptance criteria

- The repo has one documented ownership model for feature flags.
- Tests prove policy-to-render or policy-to-recipe behavior.
- Known limitation is either fixed or reclassified as intentional design.

## Workstream 3: context diagnostics DTO

### Purpose

Provide structured diagnostics explaining why LSP context was shaped as it was.

### Proposed DTO

Add a diagnostics struct near renderer or policy code:

```rust
pub struct LspContextDiagnostics {
    pub model_tier: ModelTier,
    pub tier_source: TierSource,
    pub workflow: LspWorkflowRecipe,
    pub task_risk: LspTaskRisk,
    pub stale_policy: StaleEvidencePolicy,
    pub unavailable_policy: LspUnavailablePolicy,
    pub max_context_bytes: usize,
    pub included_items: usize,
    pub omitted_items: usize,
    pub stale_items: usize,
    pub truncated_sections: Vec<String>,
    pub cache_hit: bool,
    pub notes: Vec<String>,
}
```

Use existing types and names where possible.

### Implementation steps

1. Build diagnostics from `LspContextPacket`, `LspContextPolicy`, and render result metadata.
2. Do not require packet mutation to compute diagnostics.
3. Include cache-hit detection from packet notes.
4. Include stale/fresh item counts.
5. Include truncation notes and section byte limits.
6. Add stable human-readable rendering for diagnostics.

### Acceptance criteria

- Diagnostics can be generated deterministically from packet + policy.
- Diagnostics explain included/omitted/truncated/stale/cache decisions.
- Diagnostics rendering is tested.

## Workstream 4: TUI/debug surfaces

### Problem

Context diagnostics are useful but should not bloat standard prompts.

### Candidate surfaces

- `/lsp-context-diagnostics` shows last context diagnostic snapshot.
- `/lsp-status --detail` includes a compact policy/context summary.
- debug logs include structured context diagnostics.
- agent prompt includes compact policy line only, unless a debug mode is enabled.

### Target files

- `src/tool/lsp.rs`
- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/tui_summary.rs`

### Implementation steps

1. Decide where last-context diagnostics are stored, if at all.
2. Add a compact render helper.
3. Wire a TUI command only if state storage is straightforward.
4. Keep normal agent context compact.
5. Add tests for no-last-context, with-last-context, and unavailable LSP.

### Acceptance criteria

- Users can inspect context shaping decisions on demand.
- Normal prompts are not bloated by diagnostics.

## Workstream 5: stale/unavailable policy application audit

### Problem

Policy fields are only valuable if they are actually applied. Phase 15 should audit and test all policy paths.

### Checks

- `StaleEvidencePolicy::IncludeWithWarning` includes stale items with warnings.
- `StaleEvidencePolicy::OmitStale` omits stale items and records omission.
- `StaleEvidencePolicy::RequireFresh` fails or returns note-only when stale evidence would be used.
- `LspUnavailablePolicy::NoteOnly` renders a note.
- `LspUnavailablePolicy::Omit` omits LSP context.
- `LspUnavailablePolicy::FailWhenRequired` returns a structured failure in required workflows.

### Target files

- `crates/egglsp/src/context_policy.rs`
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `src/tool/lsp.rs`

### Acceptance criteria

- Stale/unavailable policy variants have tests proving real behavior.
- No policy field is dead configuration unless documented as reserved.

## Workstream 6: docs and examples

Update:

- `architecture/lsp.md`,
- `.opencode/skills/lsp/SKILL.md`,
- `AGENTS.md`,
- `CHANGELOG.md`.

Document:

- renderer/policy ownership,
- context diagnostics fields,
- how to inspect diagnostics,
- prompt bloat controls,
- stale/unavailable behavior,
- cache-hit visibility,
- fixed impact-analysis cap note.

## Test matrix

Focused:

```bash
cargo fmt --check
cargo test -p egglsp context_policy
cargo test -p egglsp context_renderer
cargo test -p egglsp evidence_collector
cargo test -p egglsp workflow_recipes
```

Broader:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Final acceptance criteria

Phase 15 is complete when:

- the impact-analysis cap-note bug is fixed,
- policy-to-render vs policy-to-recipe ownership is explicit and tested,
- context diagnostics can explain tier/workflow/risk/stale/unavailable/truncation/cache decisions,
- diagnostics are available on demand without bloating normal prompts,
- stale/unavailable policy variants have behavior tests,
- docs classify previous limitations as fixed or intentional design.
