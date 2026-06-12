# LSP Semantic Context Consolidation Cleanup Plan

## Purpose

The first semantic-context consolidation pass made real progress: `egglsp::semantic_context::SemanticContextResponse` is now a runtime read model, `SemanticContextCollector` exists, and the public `semanticContext` handler adapts from the shared response. Before starting hunk/source navigation, this cleanup pass should close the remaining coupling and correctness gaps.

The goal is to make the semantic context boundary reliable enough that hunk/source navigation can consume `SemanticContextResponse` directly without bolting more behavior onto `src/tool/lsp.rs`.

## Current State

As of `780450f0a970ee99eb17ae250c8e795c27c38694`:

- `crates/egglsp/src/semantic_context.rs` defines a stronger `SemanticContextRequest` and `SemanticContextResponse`.
- Shared DTOs document a 1-indexed line/column convention.
- `src/lsp/semantic_context.rs` defines `SemanticContextCollector`.
- `SemanticContextCollector::collect()` assembles source excerpts, diagnostic evidence, symbols, definitions, references, overlay, source-action hints, and call/type hierarchy summaries.
- `semanticContext` now builds a `SemanticContextRequest`, calls the collector, then adapts the `SemanticContextResponse` into the existing `SemanticContextPacket` JSON shape.
- Overlay patch/content resolution remains handler-local.
- `securityContext` still largely follows the pre-consolidation path.

Known cleanup issues:

- The collector builds call and type hierarchy whenever a position exists, instead of only when requested.
- The public `semanticContext` handler separately rebuilds call/type hierarchy after the collector, so hierarchy work can be duplicated.
- The public `semanticContext` adapter does not preserve `response.limits`, `response.truncated`, or `response.section_truncations` correctly.
- Source-action hints are collected both by the collector and again by the handler.
- Overlay has two paths: collector-owned disk-content overlay and handler-owned patch/content overlay. The runtime `semanticContext` path currently uses only the handler path.
- `securityContext` still recollects generic semantic evidence rather than consuming `SemanticContextResponse`.
- `SemanticDiagnosticEvidence` stores `freshness` and `source` as strings instead of typed diagnostic enums.
- Some helper/conversion logic remains duplicated between `src/tool/lsp.rs` and `src/lsp/semantic_context.rs`.

## Non-Goals

Do not implement hunk/source navigation in this pass.

Do not change public `semanticContext` or `securityContext` JSON shapes except for strictly additive optional metadata if needed.

Do not remove direct operations such as `goToDefinition`, `findReferences`, `diagnostics`, `callHierarchy`, or `typeHierarchy`.

Do not require live language servers in tests.

Do not attempt a full rewrite of `src/tool/lsp.rs`.

Do not change security-context risk marker semantics unless required to consume shared semantic evidence safely.

## Phase 1 — Add Explicit Hierarchy Request Flags

Problem:

`SemanticContextCollector` currently builds call and type hierarchy whenever a request has a position and the server supports the operation. This can make ordinary semantic context requests unexpectedly expensive, and `semanticContext` still separately builds hierarchy summaries for presentation output.

Implementation:

- Extend `SemanticContextRequest` with explicit flags:

```rust
pub include_call_hierarchy: bool,
pub include_type_hierarchy: bool,
```

- Default both to `false` in `SemanticContextRequest::new()`.
- Add builder methods:

```rust
pub fn with_call_hierarchy(mut self, include: bool) -> Self;
pub fn with_type_hierarchy(mut self, include: bool) -> Self;
```

- In `SemanticContextCollector::collect()`, only collect call hierarchy when `request.include_call_hierarchy && has_position`.
- Only collect type hierarchy when `request.include_type_hierarchy && has_position`.
- If a hierarchy flag is true but no position is present, add structured unavailable metadata or a note rather than silently doing nothing.
- If the server lacks the capability, add structured unavailable metadata via `LspCapabilitySnapshot::unavailable(...)`.

Acceptance criteria:

- Ordinary `semanticContext` requests with a position do not trigger hierarchy LSP calls unless requested.
- `semanticContext` sets the request hierarchy flags from `include_call_hierarchy` and `include_type_hierarchy`.
- No hierarchy work is duplicated between collector and handler.
- Tests cover hierarchy flag false/true behavior with fake/static inputs where possible.

## Phase 2 — Move Hierarchy Presentation Adaptation Behind the Shared Response

Problem:

The shared response contains compact `SemanticCallGraphSummary` / `SemanticTypeGraphSummary`, while the public packet still expects detailed `CallHierarchySummary` / `TypeHierarchySummary`. The handler currently rebuilds detailed hierarchy after the collector, defeating part of the consolidation.

Choose one of two paths.

Option A, preferred for hunk/source nav foundation:

- Expand shared hierarchy DTOs enough to carry the detailed presentation data currently needed by `SemanticContextPacket`:
  - prepared items;
  - incoming/outgoing call summaries;
  - type items/supertypes/subtypes;
  - ranges;
  - truncation flags;
  - per-section errors.
- Have the collector build the detailed shared hierarchy DTO once.
- Adapt from shared hierarchy DTO into the existing presentation packet.

Option B, smaller transitional step:

- Keep shared hierarchy DTOs compact.
- Do not collect hierarchy in the collector yet.
- Leave hierarchy explicitly handler-local and document it as deferred.
- Remove call/type hierarchy fields from collector-owned response assembly until the DTO can represent detailed hierarchy cleanly.

Recommendation: choose Option A if implementation remains bounded. If not, choose Option B to prevent hidden double work.

Acceptance criteria:

- There is exactly one hierarchy collection path for `semanticContext`.
- Public JSON remains compatible.
- The chosen ownership boundary is documented in `architecture/lsp.md`.

## Phase 3 — Preserve Limits, Truncation, and Section Truncations in the Adapter

Problem:

The current `semanticContext` handler constructs presentation `SemanticContextLimits` with several fields hardcoded to `false` and returns outer `truncated: false`, even though the collector may have set `response.limits`, `response.section_truncations`, and `response.truncated`.

Implementation:

- Update `SemanticContextPacket::from_semantic_response()` or its call site to derive presentation limits from `response.limits`:

```rust
SemanticContextLimits {
    diagnostics_truncated: response.limits.diagnostics_truncated,
    symbols_truncated: response.limits.symbols_truncated,
    references_truncated: response.limits.references_truncated,
    overlay_diagnostics_truncated: response.limits.overlay_diagnostics_truncated || overlay_diagnostics_truncated,
    excerpt_truncated: response.limits.excerpt_truncated,
}
```

- Set the outer `LspToolOutput.truncated` from the packet limits or `response.truncated`, not a hardcoded `false`.
- Decide whether `section_truncations` should be exposed in public JSON now:
  - If yes, add an optional field to `SemanticContextPacket`.
  - If no, preserve it internally and add a TODO, but do not drop the boolean limits.

Acceptance criteria:

- If diagnostics/symbols/references/excerpt are truncated by the collector, public `semanticContext` output reflects that.
- `result_count` remains computed from emitted items, not original counts.
- Tests verify adapter propagation for diagnostics, symbols, references, overlay diagnostics, and excerpt truncation.

## Phase 4 — Choose One Source-Action Ownership Path

Problem:

`SemanticContextCollector` can collect source-action hints, but the `semanticContext` handler also recollects them and passes handler-local hints into the adapter.

Preferred implementation:

- Let the collector own source-action hint collection.
- Adapt `response.source_actions` into the existing presentation `SemanticSourceActionHint` shape.
- If the existing public shape requires `preview: Option<WorkspaceEditPreview>` and the shared response only carries action/available/error, decide whether to:
  - add a compact preview field to shared `SemanticSourceActionHint`; or
  - intentionally keep source actions handler-local and remove them from collector responsibility.

Recommendation:

- For now, keep the public preview-rich source-action shape handler-local only if preserving `WorkspaceEditPreview` in `egglsp::semantic_context` would make the shared DTO too presentation-heavy.
- If choosing handler-local, set `request.include_source_actions = false` and document source actions as deferred from collector ownership.
- If choosing collector-owned, remove `self.collect_source_action_hints()` from the handler.

Acceptance criteria:

- Source actions are not collected twice.
- Public JSON remains compatible.
- Ownership is documented clearly.

## Phase 5 — Choose One Overlay Ownership Path

Problem:

The collector has an overlay path that reads the file from disk when `request.include_overlay` is true, but the actual `semanticContext` runtime path sets `request.include_overlay = false` and handles overlay in the tool handler because patch/content resolution is tool-specific.

Preferred design:

- Keep patch/content resolution in the handler, but pass resolved overlay content into the collector request.
- Extend `SemanticContextRequest` with:

```rust
pub overlay_content: Option<String>,
```

- The handler resolves `content` or `patch` into full proposed content and sets `request.overlay_content = Some(content)`.
- The collector performs the overlay semantic check if `overlay_content.is_some()` or `include_overlay` is true.
- Remove the collector's disk-read overlay fallback unless there is a clear use case.

Alternative:

- Keep overlay entirely handler-local and remove/defer collector overlay handling.

Recommendation: prefer resolved-content handoff to the collector. This gives hunk/source nav one place to look for overlay evidence while preserving patch validation at the tool boundary.

Acceptance criteria:

- Overlay semantic check happens in one place.
- Patch/content validation remains at the tool boundary.
- Public overlay JSON remains compatible.
- Tests cover content overlay and patch-resolved overlay behavior without writing proposed content to disk.

## Phase 6 — Type Diagnostic Evidence in the Shared DTO

Problem:

`SemanticDiagnosticEvidence` currently stores `freshness` and `source` as strings. That is less useful for downstream code such as hunk/source navigation, which may need to branch on freshness.

Implementation:

- Change `SemanticDiagnosticEvidence` to use typed enums:

```rust
pub freshness: LspDiagnosticFreshness,
pub source: LspDiagnosticSource,
```

- Ensure serialization remains compatible with the current JSON representation, or document the exact casing change if any.
- Update adapters that convert shared evidence into presentation `DiagnosticEvidenceMeta`.

Acceptance criteria:

- Rust consumers can branch on diagnostic freshness/source without string matching.
- Public JSON stays stable or intentionally documented.
- Tests verify serialization shape for evidence metadata.

## Phase 7 — Make `securityContext` Consume Shared Semantic Evidence

Problem:

`securityContext` still builds source excerpts, diagnostics, symbols, definitions, and references mostly through its legacy path. This keeps generic semantic evidence duplicated.

Implementation approach:

- Build a `SemanticContextRequest` for security context using `SemanticContextIntent::SecurityReview`.
- Set request fields from security settings:
  - `file_path`;
  - optional target position;
  - `excerpt_radius = settings.radius`;
  - `max_symbols = MAX_SECURITY_SYMBOLS` or larger if filtering after collection needs headroom;
  - `max_diagnostics = MAX_SECURITY_DIAGNOSTICS` or larger if filtering after collection needs headroom;
  - definitions/references included when position exists;
  - call/type hierarchy only if needed by security settings and DTO ownership has been resolved.
- Call `SemanticContextCollector::collect()`.
- Build `SecurityContextPacket` from the semantic response plus security-specific filtering:
  - risk marker scanning over `response.source_excerpt`;
  - security-relevant diagnostics from `response.diagnostics`;
  - security-relevant symbols from `response.all_symbols`;
  - diagnostic evidence from `response.diagnostic_evidence`;
  - definitions/references from `response.definitions` / `response.references`;
  - stale/unavailable diagnostic notes preserved;
  - preset notes preserved;
  - call expansion remains security-specific if needed.

Rules:

- Do not treat unavailable diagnostics as clean.
- Do not drop stale diagnostics silently.
- Security-specific call expansion can remain handler-local if not yet represented in shared DTOs.

Acceptance criteria:

- `securityContext` no longer independently recollects diagnostics/symbols/definitions/references when those are available from `SemanticContextResponse`.
- Existing security output shape remains compatible.
- Tests verify stale/unavailable diagnostic evidence propagates into security notes.
- Tests verify risk marker scanning still uses the same excerpt window semantics.

## Phase 8 — Reduce Duplicate Helpers and Clarify Boundaries

Problem:

Several helpers still exist in both `src/tool/lsp.rs` and `src/lsp/semantic_context.rs`:

- URI-to-path conversion;
- symbol kind string conversion;
- range conversion;
- source excerpt construction;
- source-action hint conversion;
- hierarchy conversion.

Implementation:

- Move shared conversion helpers into one module, likely `src/lsp/semantic_context.rs` for now or a new `src/lsp/convert.rs` if broader reuse is needed.
- Keep presentation-only helpers in `src/tool/lsp.rs`.
- Add comments indicating which structs are domain DTOs and which are presentation DTOs.

Acceptance criteria:

- No unnecessary duplicate conversion code remains for semantic-context paths.
- `src/tool/lsp.rs` has a clearer adapter/presentation role.
- Direct operations can keep their local summaries if they are outside semantic context.

## Phase 9 — Tests

Add focused non-live-LSP tests.

Suggested tests:

- `semantic_request_hierarchy_flags_default_false`
- `collector_does_not_collect_hierarchy_when_flags_false`
- `collector_records_unavailable_when_requested_hierarchy_unsupported`
- `semantic_packet_adapter_preserves_limits_from_response`
- `semantic_packet_adapter_sets_outer_truncated_from_response`
- `source_actions_are_collected_once`
- `overlay_content_is_resolved_by_handler_and_checked_by_collector`
- `semantic_diagnostic_evidence_serializes_with_stable_shape`
- `security_context_consumes_semantic_diagnostic_evidence`
- `security_context_preserves_unavailable_diagnostics_warning`
- `security_context_filters_symbols_from_semantic_response`

Use fake/static responses for adapter tests. Avoid requiring `rust-analyzer`, `typescript-language-server`, or any live LSP process.

Acceptance criteria:

- The cleanup behavior is covered without live LSP servers.
- Adapter tests verify public-shape compatibility where practical.
- Tests make duplicate collection regressions difficult to reintroduce.

## Phase 10 — Documentation Updates

Update docs after code lands.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`, only for verified facts

Docs should clarify:

- `SemanticContextCollector` only collects optional hierarchy when requested.
- `semanticContext` output truncation reflects shared response limits.
- Source-action ownership decision.
- Overlay ownership decision.
- `securityContext` consumes shared semantic evidence for generic LSP facts.
- Remaining handler-local logic, if any, and why it remains outside the collector.

Acceptance criteria:

- Docs do not overclaim that all overlay/source-action/security behavior is collector-owned if any remains handler-local.
- Facts in `AGENTS.md` are verified after implementation.
- Handoff notes identify any intentionally deferred items before hunk/source navigation begins.

## Suggested Verification Commands

Run:

```bash
cargo fmt --all
cargo test -p egglsp
cargo test --lib lsp
```

Then, if feasible:

```bash
cargo test --all --workspace
```

If the full workspace test is skipped, record the reason in the implementation summary.

## Review Checklist

Before considering this cleanup complete:

- Hierarchy is request-gated and not duplicated.
- Public `semanticContext` truncation accurately reflects collector truncation.
- Source actions have one owner.
- Overlay has one owner.
- Diagnostic evidence is typed internally.
- `securityContext` consumes shared semantic evidence for generic LSP data.
- Public JSON compatibility is preserved.
- No new tests require live language servers.
- The next hunk/source nav pass can consume `SemanticContextResponse` rather than calling low-level LSP operations directly.

## Expected Follow-Up

After this cleanup pass, begin hunk/source navigation. That phase should extend semantic context with hunk-aware evidence rather than adding another parallel collection path:

- hunk-to-symbol mapping;
- hunk-to-diagnostic mapping;
- nearest enclosing symbol for each changed range;
- focused excerpts around hunks;
- definitions/references for changed symbols;
- optional call graph expansion around changed functions;
- freshness/unavailable metadata per hunk.
