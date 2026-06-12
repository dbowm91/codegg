# LSP Semantic Context DTO Consolidation Plan

## Purpose

Diagnostic freshness is now stable enough to build richer LSP context on top of it. Before implementing hunk/source navigation, consolidate semantic context assembly so Codegg has one internal semantic read model instead of several tool-local packet builders.

The primary goal is to make `egglsp::semantic_context::SemanticContextResponse` the internal source of truth for semantic context, while preserving the current public `semanticContext` and `securityContext` JSON shapes for compatibility.

This pass should reduce the amount of domain assembly inside `src/tool/lsp.rs` and create a cleaner foundation for hunk-aware source navigation.

## Current State

Current LSP state after diagnostic freshness hardening:

- Diagnostics now flow through `LspDiagnosticSnapshot` with `freshness`, `source`, `age_ms`, and `usable_evidence` semantics.
- `semanticContext` and `securityContext` already include diagnostic evidence metadata.
- `egglsp::semantic_context` defines shared DTOs, but they are not the runtime source of truth.
- `src/tool/lsp.rs` still owns most semantic assembly directly:
  - source excerpt construction;
  - diagnostic snapshot mapping into presentation diagnostics;
  - document symbol flattening;
  - definition/reference gathering;
  - optional call/type hierarchy construction;
  - overlay preview mapping;
  - security context evidence filtering;
  - string notes for unsupported/unavailable operations.
- Security context duplicates some semantic-context collection work instead of consuming a shared semantic response.
- Hunk/source navigation would currently need to bolt onto this large tool path, which would make the tool layer harder to unwind later.

## Non-Goals

Do not change public JSON output shapes for `semanticContext` or `securityContext` unless strictly additive and backward compatible.

Do not implement hunk/source navigation in this pass.

Do not add mutating edit/apply behavior.

Do not require live language servers in new tests.

Do not rewrite every LSP operation.

Do not remove existing tool presentation DTOs yet; reclassify them as presentation/adaptor types.

Do not turn security context into a generic semantic context alias. Security context remains a security-biased consumer of semantic evidence.

## Target Architecture

Move toward this shape:

```text
LSP client/service/operations
        │
        ▼
SemanticContextCollector
        │
        ▼
egglsp::semantic_context::SemanticContextResponse
        │
        ├── semanticContext presentation adapter
        │       └── current SemanticContextPacket JSON shape
        │
        └── SecurityContextBuilder
                └── current SecurityContextPacket JSON shape
```

The shared semantic response should carry domain evidence. Tool packet structs should only adapt/present that evidence.

## Phase 1 — Inventory and Extend Shared Semantic DTOs

Review `crates/egglsp/src/semantic_context.rs` and compare it against the evidence currently emitted by `src/tool/lsp.rs`.

The shared response should be able to represent, at minimum:

- request metadata:
  - file path;
  - optional target line/column;
  - intent;
  - caps/limits used;
- source excerpt:
  - start/end lines;
  - text;
  - truncation flag;
- diagnostic evidence:
  - diagnostics;
  - `LspDiagnosticSnapshot` metadata or equivalent `DiagnosticEvidenceMeta`;
  - diagnostics truncation flag;
  - diagnostics error/unavailable state;
- document symbols:
  - compact symbol summaries;
  - truncation flag;
  - error/unavailable state;
- definitions and references:
  - normalized locations;
  - truncation flags;
  - error/unavailable state;
- optional hierarchy evidence:
  - call hierarchy summary or a compact shared equivalent;
  - type hierarchy summary or a compact shared equivalent;
  - truncation/error metadata;
- overlay evidence:
  - proposed-content diagnostics/symbols where available;
  - restored-disk-view metadata;
  - overlay errors;
- structured unavailable evidence:
  - operation;
  - reason;
  - server/language metadata where known;
  - human-readable detail.

Implementation guidance:

- Prefer adding small DTOs rather than importing large tool presentation structs into `egglsp`.
- Keep internal positions/ranges carefully documented. The shared DTO should state whether it uses LSP-native 0-indexed or presentation 1-indexed line/column values.
- Prefer `PathBuf`/normalized path strings consistently. Avoid mixing raw URI strings and path strings in the same field family.

Acceptance criteria:

- Shared DTOs can represent the evidence currently needed by `semanticContext` without lossy conversion.
- DTO docs specify indexing and path semantics.
- No public tool output changes yet.

## Phase 2 — Add Structured Unavailable and Truncation Metadata

String-only notes are too lossy for future hunk/source navigation. Add structured metadata to the shared semantic response.

Suggested DTO:

```rust
pub struct SemanticUnavailable {
    pub operation: LspSemanticOperation,
    pub reason: SemanticUnavailableReason,
    pub server_name: Option<String>,
    pub language_id: Option<String>,
    pub detail: Option<String>,
}

pub enum SemanticUnavailableReason {
    Unsupported,
    ServerUnavailable,
    RequestFailed,
    MissingTarget,
    TimedOut,
    InvalidInput,
}
```

Suggested truncation DTO:

```rust
pub struct SemanticTruncation {
    pub section: String,
    pub original_count: Option<usize>,
    pub emitted_count: usize,
    pub limit: usize,
}
```

Rules:

- Keep human-readable notes for public compatibility, but derive them from structured metadata where possible.
- Unsupported optional enrichments should populate structured unavailable metadata rather than only setting `definitions_error` or appending a note.
- Truncation should attach to the section that was truncated.

Acceptance criteria:

- Shared response can distinguish unsupported, unavailable, failed, timed out, and missing target cases.
- Tool adapters can still render current note/error strings.
- Tests verify structured metadata is emitted for unsupported definition/reference/hierarchy cases.

## Phase 3 — Introduce `SemanticContextCollector`

Create a collector/builder that owns domain assembly.

Possible location:

- `crates/egglsp/src/semantic_context_collector.rs`, if it can depend only on `egglsp` service/client primitives; or
- `src/lsp/semantic_context.rs`, if it must depend on Codegg-specific operations, path policy, overlay preview, or source excerpt helpers.

Prefer the narrowest dependency boundary. Do not force `egglsp` to depend on tool-layer concepts.

Suggested API:

```rust
pub struct SemanticContextCollector {
    service: Arc<LspService>,
    operations: LspOperations,
    limits: SemanticContextLimits,
}

impl SemanticContextCollector {
    pub async fn collect(
        &self,
        request: SemanticContextRequest,
    ) -> Result<SemanticContextResponse, LspError>;
}
```

Request inputs should include:

- file path;
- optional target position;
- radius/source excerpt settings;
- include definitions/references flags;
- include call/type hierarchy flags;
- include overlay/source-action flags if supported in this pass;
- optional proposed content/patch reference if overlay is included;
- allowed root/path policy if needed.

Collector responsibilities:

- ensure the file is open where required;
- fetch diagnostic snapshot;
- collect document symbols;
- collect definition/reference data with capability gating;
- collect hierarchy data with capability gating;
- build source excerpt;
- apply limits/truncation;
- return structured unavailable/truncation metadata;
- avoid presentation-specific JSON field names.

Acceptance criteria:

- At least `semanticContext` can be assembled into `SemanticContextResponse` through the collector.
- Existing public `semanticContext` output remains compatible through an adapter.
- The collector has unit tests with fake/static inputs where possible and does not require live LSP servers.

## Phase 4 — Adapt Existing `semanticContext` Output From Shared Response

Keep current user-facing JSON stable.

Refactor the `semanticContext` operation in `src/tool/lsp.rs` so it does roughly:

```rust
let request = SemanticContextRequest::from_tool_input(...)?;
let response = collector.collect(request).await?;
let packet = SemanticContextPacket::from_semantic_response(response);
serialize(packet)
```

Rules:

- Keep field names currently emitted by `SemanticContextPacket` unless adding optional fields.
- Preserve current limits and truncation behavior.
- Preserve existing diagnostic evidence fields.
- Preserve current behavior for missing line/column and hierarchy flags.
- Do not regress source-action hints or overlay behavior; if a piece cannot move safely, isolate it behind a clear adapter boundary and document it as remaining tool-local.

Acceptance criteria:

- `semanticContext` no longer directly owns diagnostics/symbols/definition/reference assembly.
- Existing tests for semantic context still pass.
- New adapter tests verify response-to-packet conversion.

## Phase 5 — Make `securityContext` Consume Semantic Context

Refactor security context to consume the shared semantic response for generic LSP evidence, then apply security-specific filtering.

Target flow:

```rust
SemanticContextResponse
    -> SecurityContextBuilder
        -> risk markers from excerpt
        -> security-relevant diagnostics
        -> security-relevant symbols
        -> security definitions/references notes
        -> call expansion / preset-specific behavior
        -> SecurityContextPacket
```

Security-specific responsibilities that should remain outside generic semantic context:

- risk marker scanning;
- category filtering;
- security preset handling;
- selecting security-relevant diagnostics and symbols;
- call expansion policy/depth defaults;
- security-specific note wording.

Generic responsibilities that should come from semantic context:

- diagnostic snapshot and evidence metadata;
- source excerpt;
- document symbols;
- definitions/references;
- capability unavailable metadata;
- truncation metadata;
- optional hierarchy evidence when requested.

Acceptance criteria:

- `securityContext` no longer recollects diagnostics/symbols/definitions/references independently when semantic context already collected them.
- Security notes for stale/unavailable diagnostics are preserved.
- Security output remains backward compatible.
- Tests cover stale/unavailable diagnostic evidence propagation into security context.

## Phase 6 — Normalize Locations, Ranges, and Indexing

Before hunk/source navigation, location semantics need to be unambiguous.

Define and enforce one internal convention:

- Option A: shared DTOs use LSP-native 0-indexed positions and tool adapters convert to 1-indexed presentation;
- Option B: shared DTOs use presentation 1-indexed positions everywhere.

Recommendation: use LSP-native 0-indexed internally and convert at the tool boundary. If that is too disruptive for this pass, explicitly document any DTOs that remain 1-indexed and add TODOs for later normalization.

Centralize helpers for:

- URI to path conversion;
- path to URI conversion;
- range conversion;
- symbol kind formatting;
- diagnostic conversion;
- location conversion.

Acceptance criteria:

- Conversion helpers are not duplicated across unrelated local functions.
- Tests cover representative URI/path and range conversions.
- Public output remains 1-indexed where it is currently 1-indexed.

## Phase 7 — Add Tests Without Live LSP Servers

Prioritize adapter and assembly tests that can use fake data.

Suggested tests:

- `semantic_response_preserves_diagnostic_evidence_metadata`
- `semantic_response_records_unsupported_definition_as_unavailable`
- `semantic_response_records_reference_truncation`
- `semantic_context_packet_from_response_preserves_current_json_shape`
- `security_context_builder_preserves_stale_diagnostic_warning`
- `security_context_builder_does_not_treat_unavailable_diagnostics_as_clean`
- `location_conversion_keeps_public_output_one_indexed`
- `source_excerpt_truncation_records_structured_metadata`

Avoid live servers:

- Use fake `SemanticContextResponse` instances for adapter tests.
- Use fake diagnostics/symbols/locations for builder tests.
- If a collector test needs service behavior, use an in-memory/fake service boundary rather than launching a language server.

Acceptance criteria:

- Core conversion and propagation behavior is covered.
- Tests are deterministic.
- No test requires a local Rust Analyzer or TypeScript server.

## Phase 8 — Documentation Updates

Update documentation after the code shape is stable.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`, only for verified facts

Docs should explain:

- `SemanticContextResponse` is the internal semantic read model;
- `semanticContext` and `securityContext` are presentation/consumer adapters;
- diagnostic freshness evidence flows through semantic context;
- structured unavailable/truncation metadata exists;
- public output remains backward compatible;
- hunk/source navigation should consume semantic context rather than re-collecting LSP data.

Acceptance criteria:

- No docs imply `src/tool/lsp.rs` is the domain owner for semantic assembly.
- Facts listed in `AGENTS.md` are true after implementation.
- Handoff notes identify any remaining tool-local logic intentionally deferred.

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

If the full workspace test is skipped, record why in the implementation summary.

## Review Checklist

Before considering this pass complete:

- `SemanticContextResponse` is produced by a runtime path, not only test fixtures.
- `semanticContext` adapts from `SemanticContextResponse` and preserves existing JSON shape.
- `securityContext` consumes shared semantic evidence for diagnostics/symbols/definitions/references where practical.
- Diagnostic freshness metadata is preserved end-to-end.
- Unsupported/unavailable sections are represented structurally.
- Truncation metadata is section-specific.
- Location/indexing semantics are documented and tested.
- No live LSP server is required for new tests.
- `src/tool/lsp.rs` is smaller or at least has a clearer boundary between collection and presentation.

## Expected Follow-Up

After this consolidation lands, start the hunk/source navigation phase. That phase should consume `SemanticContextResponse` and add hunk-aware evidence on top:

- hunk-to-symbol mapping;
- hunk-to-diagnostic mapping;
- nearest enclosing symbol;
- focused excerpts around changed ranges;
- definitions/references for changed symbols;
- optional call graph expansion around changed functions;
- freshness/unavailable metadata per hunk.

Do not start hunk/source navigation until this semantic context boundary is in place, or the new hunk features will deepen the current tool-layer coupling.
