# LSP Phase 7 Plan: Semantic Workflow Recipes for Repair, Review, Security, and Hunks

Status date: 2026-06-25
Phase type: workflow orchestration / agent-context policy
Primary goal: turn the existing LSP primitives into named, repeatable, bounded workflows that agents can invoke consistently.

## Current baseline

The repo already has the major semantic primitives needed for this phase:

- `semanticContext` for source excerpt, diagnostics, symbols, definitions, references, hierarchy summaries, overlay summaries, and source-action hints.
- `securityContext` for deterministic risk markers, security-relevant diagnostics/symbols, optional definitions/references/call hierarchy, optional overlay, presets, and shallow call expansion.
- `hunkSourceContext` for diff/hunk-local semantic evidence.
- `egglsp::LspContextPacket` as the canonical agent/review packet model.
- `ServiceLspEvidenceProvider` and bridge functions for folding collected evidence into canonical packet consumers.
- `render_lsp_context_for_agent` and `ModelTier` for model-tier-sensitive rendering.
- Security-review workflow code that can perform deterministic review first and optionally enrich with LSP context.

Phase 7 should not invent new raw tool calls. It should define recipes that choose existing operations, budgets, fallback behavior, and render policy for common tasks.

## Non-goals

Do not add persistent semantic memory.

Do not add unbounded cross-file analysis.

Do not execute LSP mutation-producing operations directly. Preview-producing operations may be included only as hints or preview artifacts.

Do not make LSP required for baseline security review or repair workflows. Missing or degraded LSP should produce notes and fallbacks, not total workflow failure, unless a user explicitly requests required LSP mode.

Do not introduce a second canonical packet model.

## Recipe taxonomy

Implement or document these named workflow recipes:

1. `repair_local`
2. `repair_hunk`
3. `review_file`
4. `review_diff`
5. `security_review_enriched`
6. `hunk_source_navigation`
7. `preview_suggestion`

The recipe names can be internal if the public UI uses different command names, but they should exist as conceptual units in docs and code comments so contributors can reason about behavior.

## Workstream 1: define recipe request/response contract

### Problem

Current LSP calls are powerful but still low-level. Agents need predictable recipes: given a task type, gather a bounded set of evidence, render it for the model tier, preserve freshness notes, and degrade gracefully.

### Target files

- `crates/egglsp/src/context.rs`
- `crates/egglsp/src/context_renderer.rs`
- `crates/egglsp/src/evidence_collector.rs`
- `crates/egglsp/src/evidence_adapter.rs`
- `src/lsp/semantic_context.rs`
- `src/tool/lsp.rs`
- New module if appropriate: `crates/egglsp/src/workflow_recipes.rs` or `src/lsp/workflow_recipes.rs`
- `architecture/lsp.md`

### Implementation steps

1. Decide where recipe orchestration belongs.
   - If recipes are pure packet construction/rendering, prefer `crates/egglsp`.
   - If recipes depend on Codegg-specific tool wiring, allowed roots, security workflow, or TUI concepts, keep orchestration in `src/lsp` or `src/security`.
2. Define a small typed enum or struct set for recipe requests. Suggested shape:

```rust
pub enum LspWorkflowRecipe {
    RepairLocal,
    RepairHunk,
    ReviewFile,
    ReviewDiff,
    SecurityReviewEnriched,
    HunkSourceNavigation,
    PreviewSuggestion,
}
```

3. Define shared settings:
   - model tier,
   - required/opportunistic mode,
   - risk mode,
   - max files,
   - max ranges per file,
   - max diagnostics/references/symbols,
   - include definitions/references/hierarchy flags,
   - include preview hints flag,
   - freshness tolerance.
4. Define a recipe outcome:
   - canonical `LspContextPacket`, or existing workflow-specific response plus bridge into packet,
   - rendered context string,
   - notes,
   - unavailable/fallback reasons,
   - preview IDs if generated,
   - stale/freshness summary.
5. Keep initial implementation minimal: recipes can be helper functions that assemble existing `LspContextRequest` values and render them. Avoid over-engineering a new framework.

### Acceptance criteria

- Each recipe has an explicit request shape and documented behavior.
- Recipes return bounded context and notes rather than raw LSP JSON.
- Missing/degraded LSP is represented as structured notes/fallback state.
- Recipe outputs can be rendered differently by model tier.

## Workstream 2: repair recipes

### `repair_local`

Purpose: repair a localized issue in one file around a target line/column or diagnostic.

Expected evidence:

- Source excerpt around the target line.
- Current diagnostics for the file with freshness metadata.
- Enclosing document symbols.
- Definition at target if positioned.
- References only if explicitly requested or model tier is workhorse/frontier.
- Optional source-action hints for organize imports or quick fixes, preview-only.

Default budget:

- Small model: same-file excerpt + diagnostics + enclosing symbol only.
- Workhorse: add definitions and limited references.
- Frontier: add limited hierarchy or semantic token summary if useful.

Fallback behavior:

- If LSP unavailable, return source excerpt plus note.
- If diagnostics stale, include stale warning and do not state that the file is clean.

Implementation steps:

1. Add helper to build a `SemanticContextRequest` for local repair.
2. Map it into `LspContextPacket` through existing adapter/bridge where possible.
3. Render with tier-aware policy.
4. Add tests with fake server diagnostics and definition/reference responses.

### `repair_hunk`

Purpose: repair code around changed diff hunks.

Expected evidence:

- Hunk-local source excerpts.
- Diagnostics intersecting or near hunks.
- Enclosing symbols.
- Definitions/references only near changed lines.
- Source tags set to `AgentContextSource::Hunk`.

Default budget:

- Max hunks: low default, with cap.
- Prefer hunk-local same-file evidence before cross-file evidence.
- Any truncation must be explicit.

Implementation steps:

1. Reuse `hunkSourceContext` and `collect_hunk_context` rather than rebuilding hunk parsing.
2. Ensure hunk evidence is bridged into canonical `LspContextPacket` with `AgentContextSource::Hunk`.
3. Add renderer section headers that distinguish hunk-local evidence from general context.
4. Add tests for multiple hunks with truncation and stale diagnostics.

## Workstream 3: review recipes

### `review_file`

Purpose: semantic review of a single file without a diff.

Expected evidence:

- Source excerpt or selected ranges.
- File diagnostics.
- Document symbols.
- Optional hierarchy around requested positions.
- Capability/unavailable notes.

Default behavior:

- Do not scan entire large files into model context.
- Require explicit ranges or choose bounded symbol/diagnostic summaries.
- Render limitations clearly.

### `review_diff`

Purpose: semantic review of changed files/hunks.

Expected evidence:

- Changed file list and hunk descriptors from diff APIs.
- Diagnostics near changes.
- Definitions/references for changed symbols.
- Optional implementations and semantic tokens only under higher risk or frontier tier.
- Preview hints for safe source actions if enabled.

Implementation steps:

1. Add a recipe helper that converts changed files and hunk descriptors into `LspContextRequest::Review` or hunk requests.
2. Preserve per-hunk provenance.
3. Render findings prompts separately from evidence notes.
4. Add tests for changed-file caps and hunk truncation.

### Acceptance criteria for review recipes

- Review recipes prefer changed/hunk-local evidence over global evidence.
- Generated context includes enough provenance to audit why evidence was included.
- Unsupported operations do not fail the whole recipe unless required mode is enabled.
- Tests cover no-LSP, degraded-LSP, and truncated-hunk cases.

## Workstream 4: security recipe integration

### Problem

Security review already has a deterministic stage and optional LSP enrichment. Phase 7 should make that enrichment a first-class recipe rather than a special one-off path.

### Target files

- `src/security/workflow/context.rs`
- `src/security/workflow/enrichment.rs`
- `src/security/workflow/evidence.rs`
- `src/security/workflow/report.rs`
- `src/security/lsp_executor.rs`
- `src/tool/lsp_security.rs`
- `architecture/lsp.md`
- `.opencode/skills/security/SKILL.md`

### Recipe: `security_review_enriched`

Purpose: enrich deterministic defensive review with LSP-backed evidence while preserving the rule that risk markers are prompts, not findings.

Expected evidence:

- Deterministic risk markers by preset.
- Security-relevant diagnostics and symbols.
- Definitions/references around high-risk changed hunks.
- Shallow call hierarchy by default only when positioned and preset permits it.
- Bounded call expansion only when escalation policy says so.
- Truncation and stale-evidence notes.

Implementation steps:

1. Document the security enrichment recipe as the canonical recipe for `/security-review --enrich`.
2. Ensure enrichment path returns recipe-like notes:
   - LSP unavailable,
   - no eligible targets,
   - request timeout,
   - truncated evidence,
   - stale diagnostics,
   - degraded server.
3. Confirm marker-only evidence cannot synthesize findings.
4. Make call expansion explicitly opt-in or escalation-policy-driven, never preset-default.
5. Add focused tests for:
   - stale diagnostic note propagation,
   - timeout note propagation,
   - marker-only evidence not producing findings,
   - high-risk target escalation to depth 1 when policy allows,
   - no executor fallback in remote/socket mode.

### Acceptance criteria

- `/security-review --enrich` behavior is described in terms of the recipe.
- LSP enrichment remains optional and nonfatal.
- Security findings remain evidence-gated.
- Tests prove LSP failures do not fail deterministic review.

## Workstream 5: hunk-source workflow recipe

### Problem

Hunk navigation exists, but agents need predictable hunk-focused context for repair/review. The recipe should make hunk-local evidence easy to request and render.

### Target files

- `crates/egglsp/src/hunk_context.rs`
- `crates/egglsp/src/evidence_collector.rs`
- `crates/egglsp/src/bridges.rs`
- `src/lsp/hunk_nav*.rs` if present
- `src/tool/lsp.rs`
- `tests/lsp_composite_stdio.rs`

### Recipe: `hunk_source_navigation`

Purpose: collect semantic context around changed hunks for source navigation and review.

Expected evidence:

- Per-hunk source excerpt.
- Enclosing symbols.
- Hunk-local diagnostics.
- Optional definitions/references.
- Notes for unsupported/missing capabilities.
- Freshness and truncation metadata.

Implementation steps:

1. Ensure the public/typed hunk request path exposes per-hunk caps clearly.
2. Ensure zero limits are coerced or rejected consistently.
3. Ensure hunk-to-context bridge tags every item with `AgentContextSource::Hunk`.
4. Add render format for hunk packets that groups evidence by hunk.
5. Add tests for:
   - one hunk with symbols/diagnostics,
   - multiple hunks with cap truncation,
   - stale diagnostics,
   - LSP unavailable fallback,
   - unsupported references/definitions.

### Acceptance criteria

- Hunk-focused context renders as grouped hunk evidence, not a flat unrelated blob.
- Hunk source tags survive production adapter paths.
- Truncation and stale evidence are visible.

## Workstream 6: preview-suggestion recipe

### Problem

Repair and review workflows may want to suggest semantic previews such as organize imports, formatting, quick fixes, or rename. Phase 7 should only define how recipes mention or gather previews; full preview lifecycle UX belongs to Phase 8.

### Recipe: `preview_suggestion`

Purpose: include safe preview-only semantic edit suggestions in repair/review context.

Expected behavior:

- Only preview-producing operations are allowed.
- Preview IDs and metadata are returned when generated.
- The recipe must state that previews are not applied.
- Stale-base flags must be preserved.
- Command-only actions remain rejected.

Implementation steps:

1. For local repair, optionally collect source-action hints where cheap.
2. For review/security, do not generate many previews by default; prefer summaries or hints.
3. Add recipe note: `preview available: <id>, not applied`.
4. Defer list/detail/apply UX to Phase 8.

### Acceptance criteria

- Recipes can reference preview IDs without applying edits.
- Preview metadata includes affected files, edit count, and stale-base flag.
- No workflow applies a preview directly.

## Workstream 7: tier-aware rendering defaults

### Problem

The repo already has model-tier rendering, but recipes need default policies.

### Target files

- `crates/egglsp/src/context_renderer.rs`
- `src/agent/loop.rs`
- Any config schema around LSP context policy

### Implementation steps

1. Define default render policy by recipe and tier.
2. Suggested defaults:
   - Small: hunk-local diagnostics, same-file excerpt, enclosing symbol; omit references unless directly hunk-local.
   - Workhorse: add definitions, limited references, selected symbols, concise notes.
   - Frontier: add cross-file references, hierarchy summaries, richer diagnostic provenance, and limited preview hints.
3. Ensure renderer emits explicit truncation notes.
4. Add tests that a given packet renders differently for small/workhorse/frontier.
5. Ensure explicit input model tier still overrides inferred family tier.

### Acceptance criteria

- Recipes have deterministic tier defaults.
- Tests prove small tier avoids cross-file bloat.
- Frontier tier can include richer evidence without breaking budgets.

## Suggested implementation order

1. Document recipe taxonomy and request/outcome shapes.
2. Implement local repair and hunk repair recipes first.
3. Add review diff recipe.
4. Fold security enrichment into recipe terminology and notes.
5. Add preview-suggestion hooks without full lifecycle UX.
6. Add tier-aware render defaults and tests.
7. Update architecture and skill docs.

## Completion checklist

- [x] Recipe taxonomy documented.
- [x] Recipe request/outcome types or helper functions added.
- [x] `repair_local` implemented/tested.
- [x] `repair_hunk` implemented/tested.
- [x] `review_file` documented and minimally implemented or deferred explicitly.
- [x] `review_diff` implemented/tested.
- [x] `security_review_enriched` documented as canonical enrichment recipe.
- [x] `hunk_source_navigation` renders grouped hunk evidence.
- [x] `preview_suggestion` can expose preview IDs without applying edits.
- [x] Tier-aware render defaults exist for small/workhorse/frontier.
- [x] Missing/degraded LSP fallback tests pass.
- [x] Stale/truncated evidence notes are visible in rendered output.
- [x] `cargo fmt --check` passes.
- [x] Relevant LSP/security/agent tests pass.

## Handoff notes for smaller models

Start with helper functions and tests. Avoid building a large generic workflow engine unless duplication becomes painful.

Prefer existing types: `SemanticContextRequest`, `HunkSourceNavigationRequest`, `LspContextRequest`, `LspContextPacket`, `LspContextRenderConfig`, and `ModelTier`.

Keep recipes deterministic and inspectable. A recipe should be explainable as: inputs -> evidence requests -> budget -> fallback behavior -> render policy.
