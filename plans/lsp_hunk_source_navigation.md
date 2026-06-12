# LSP Hunk and Source Navigation Plan

## Purpose

The LSP semantic-context path is now stable enough to build hunk/source navigation on top of it. This plan adds hunk-aware evidence to Codegg without creating another parallel LSP collection path.

The goal is to let agents ask: "what changed, what symbols do those changes belong to, what diagnostics touch those hunks, what nearby source should I inspect, and what definitions/references/call relationships matter for the changed code?"

This pass should consume the existing `SemanticContextResponse`/`SemanticContextCollector` foundation and extend it with hunk-aware evidence.

## Current State

Current LSP architecture after semantic-context consolidation and boundary clarification:

- `SemanticContextResponse` is the internal semantic read model.
- `SemanticContextCollector` owns generic semantic evidence:
  - source excerpt;
  - diagnostic snapshots and freshness metadata;
  - document symbols;
  - definitions and references;
  - call/type hierarchy summaries when requested;
  - section truncation and unavailable metadata.
- `semanticContext` adapts `SemanticContextResponse` into the tool-local `SemanticContextPacket`.
- `securityContext` consumes shared semantic evidence for generic LSP facts and then applies security filtering.
- Overlay translation remains handler-local by design because patch/content expansion is tool-specific.
- Source-action hints remain handler-local by design because they produce preview-rich `WorkspaceEditPreview` payloads.
- Security call expansion remains handler-local because it is recursive BFS expansion, not the compact immediate call hierarchy in the shared semantic model.

This plan should preserve those boundaries.

## Non-Goals

Do not implement automatic edits or patch application.

Do not change the preview-only contract for LSP edits.

Do not move overlay or source-action preview ownership unless strictly necessary.

Do not require live language servers in core unit tests.

Do not add whole-program call graph analysis.

Do not make diagnostic absence a proof of correctness or safety.

Do not replace existing direct LSP operations such as `goToDefinition`, `findReferences`, `callHierarchy`, or `typeHierarchy`.

## Target Architecture

Add a hunk-aware layer above semantic context:

```text
Diff / patch / changed-file input
        │
        ▼
Hunk parser and line-map builder
        │
        ▼
SemanticContextCollector
        │
        ▼
SemanticContextResponse
        │
        ▼
HunkSourceNavigator
        │
        ▼
HunkSourceNavigationResponse
        │
        ├── semanticContext optional section/adaptor
        ├── securityContext optional enrichment
        └── future review-agent routing evidence
```

The hunk navigator should not call low-level LSP operations directly except through `SemanticContextCollector` or narrowly shared helper methods that already belong to the semantic-context layer.

## Terminology

- **Hunk**: one contiguous changed block from a unified diff.
- **Old range**: the line range in the base/original file.
- **New range**: the line range in the current/proposed file.
- **Focus range**: the range the agent should inspect. Usually the hunk's new range expanded to enclosing symbol and bounded context.
- **Enclosing symbol**: the smallest symbol range that fully contains or intersects the hunk's changed lines.
- **Related symbol**: a symbol near, containing, referenced by, or referencing the changed code.
- **Hunk evidence**: diagnostics, symbols, references, definitions, excerpts, and freshness/unavailable metadata associated with one hunk.

## Phase 1 — Add Shared Hunk DTOs

Add DTOs in `egglsp::semantic_context` or a sibling `egglsp::hunk_context` module. Prefer a new module if the shapes get large.

Suggested types:

```rust
pub struct HunkLineRange {
    pub start_line: u32,
    pub end_line: u32,
}

pub struct HunkDescriptor {
    pub id: String,
    pub file_path: String,
    pub old_range: Option<HunkLineRange>,
    pub new_range: Option<HunkLineRange>,
    pub header: Option<String>,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub context_lines: usize,
}

pub struct HunkSourceNavigationRequest {
    pub file_path: String,
    pub hunks: Vec<HunkDescriptor>,
    pub intent: SemanticContextIntent,
    pub include_definitions: bool,
    pub include_references: bool,
    pub include_call_hierarchy: bool,
    pub include_type_hierarchy: bool,
    pub excerpt_radius: u32,
    pub max_hunks: usize,
    pub max_symbols_per_hunk: usize,
    pub max_diagnostics_per_hunk: usize,
    pub max_references_per_hunk: usize,
}

pub struct HunkSourceNavigationResponse {
    pub file_path: String,
    pub semantic: SemanticContextResponse,
    pub hunks: Vec<HunkEvidence>,
    pub limits: HunkSourceNavigationLimits,
    pub notes: Vec<String>,
    pub truncated: bool,
}
```

Each DTO should explicitly document 1-indexed line/column conventions, matching the current semantic-context DTOs.

Acceptance criteria:

- Hunk DTOs are serializable.
- Hunk DTOs are independent of tool-layer presentation packets.
- DTOs carry enough metadata to map hunk lines to symbols/diagnostics/excerpts.

## Phase 2 — Parse Unified Diffs Into Hunk Descriptors

Add a parser that converts unified diff text into `HunkDescriptor` values.

Possible location:

- `src/lsp/hunk_nav.rs`, if this stays Codegg-specific; or
- `crates/egggit` / existing diff utilities, if there is already a reusable git/diff layer.

The parser should support:

- single-file unified diff;
- multi-file unified diff, at least by grouping hunks per file;
- standard hunk headers: `@@ -old_start,old_len +new_start,new_len @@ optional context`;
- hunks with omitted length fields, e.g. `@@ -10 +10 @@`;
- additions-only and deletions-only hunks;
- new file/deleted file markers where feasible.

Output rules:

- `old_range` is `None` for new-file additions with no meaningful old span.
- `new_range` is `None` for deleted-file hunks with no meaningful current-file span.
- Hunk ids should be deterministic: e.g. `file_path:hunk_index:new_start-new_end`.
- Keep raw hunk header text for diagnostics/debugging.

Acceptance criteria:

- Parser handles representative single-file and multi-file diffs.
- Parser is UTF-8 safe and does not attempt byte offsets for now.
- Parser tests are pure unit tests.

## Phase 3 — Build Line/Range Matching Primitives

Add pure helper functions for range overlap and containment.

Required helpers:

```rust
fn ranges_overlap(a: HunkLineRange, b: HunkLineRange) -> bool;
fn range_contains(container: HunkLineRange, inner: HunkLineRange) -> bool;
fn distance_between_ranges(a: HunkLineRange, b: HunkLineRange) -> u32;
fn expand_range(range: HunkLineRange, radius: u32, file_line_count: u32) -> HunkLineRange;
```

Add mapping helpers for existing semantic DTOs:

- symbol range to `HunkLineRange`;
- diagnostic line to single-line range;
- reference/definition range to `HunkLineRange`;
- source excerpt range to `HunkLineRange`.

Ranking rules for symbol matching:

1. Prefer symbols that fully contain the hunk new range.
2. If none contain it, prefer symbols that overlap it.
3. If none overlap, prefer nearest preceding/following symbols within a bounded distance.
4. Prefer the smallest containing symbol over a larger parent symbol.
5. Preserve parent/outer candidates as context if available.

Acceptance criteria:

- Helpers are deterministic and tested.
- No LSP server is required for these tests.
- Symbol ranking tests cover nested functions/classes/modules where symbol ranges overlap.

## Phase 4 — Add `HunkSourceNavigator`

Introduce a hunk-aware builder that consumes a semantic response and hunk descriptors.

Possible location:

- `src/lsp/hunk_nav.rs`

Suggested API:

```rust
pub struct HunkSourceNavigator {
    limits: HunkSourceNavigationLimits,
}

impl HunkSourceNavigator {
    pub fn build(
        &self,
        semantic: SemanticContextResponse,
        hunks: Vec<HunkDescriptor>,
    ) -> HunkSourceNavigationResponse;
}
```

The navigator should be pure where possible. It should not directly call LSP. It should consume a `SemanticContextResponse` already collected for the target file.

Each `HunkEvidence` should include:

- hunk descriptor;
- primary/enclosing symbol;
- related symbols;
- diagnostics intersecting the hunk;
- diagnostics near the hunk;
- definitions intersecting or near the hunk;
- references intersecting or near the hunk;
- optional compact call/type hierarchy evidence if the semantic response contains it;
- focused source excerpt metadata;
- diagnostic evidence freshness copied from `semantic.diagnostic_evidence`;
- unavailable/truncation metadata relevant to the hunk.

Suggested DTO:

```rust
pub struct HunkEvidence {
    pub hunk: HunkDescriptor,
    pub focus_range: Option<HunkLineRange>,
    pub enclosing_symbol: Option<SemanticSymbolSummary>,
    pub related_symbols: Vec<SemanticSymbolSummary>,
    pub diagnostics: Vec<FileDiagnostic>,
    pub nearby_diagnostics: Vec<FileDiagnostic>,
    pub definitions: Vec<SemanticLocation>,
    pub references: Vec<SemanticLocation>,
    pub call_hierarchy: Option<SemanticCallGraphSummary>,
    pub type_hierarchy: Option<SemanticTypeGraphSummary>,
    pub source_excerpt: Option<SemanticSourceExcerpt>,
    pub diagnostic_evidence: Option<SemanticDiagnosticEvidence>,
    pub section_truncations: Vec<SemanticSectionTruncation>,
    pub unavailable: Vec<LspUnavailable>,
    pub notes: Vec<String>,
}
```

Acceptance criteria:

- Hunk evidence is derived from `SemanticContextResponse` and hunks only.
- No direct LSP calls exist inside `HunkSourceNavigator`.
- Hunk evidence preserves diagnostic freshness metadata.
- Tests cover symbol/diagnostic/reference matching.

## Phase 5 — Add Collection Entry Point

Add a high-level collector that coordinates diff parsing and semantic collection.

Suggested API:

```rust
pub struct HunkSourceNavigationCollector {
    semantic_collector: SemanticContextCollector,
    navigator: HunkSourceNavigator,
}

impl HunkSourceNavigationCollector {
    pub async fn collect(
        &self,
        request: HunkSourceNavigationRequest,
    ) -> Result<HunkSourceNavigationResponse, String>;
}
```

Flow:

1. Parse/accept hunk descriptors.
2. Determine the target file and current new ranges.
3. Build a `SemanticContextRequest` for the file.
4. Set `SemanticContextRequest::intent = Navigation` or `Review` depending on caller.
5. Set definitions/references/hierarchy flags from request.
6. Use a source excerpt radius large enough to cover all hunks or let the navigator compute per-hunk excerpts from the file.
7. Call `SemanticContextCollector::collect()` once per file.
8. Feed `SemanticContextResponse` plus hunks into `HunkSourceNavigator`.

For multi-file diffs:

- Group by file.
- Collect semantic response once per changed file.
- Return per-file hunk evidence.
- Apply global caps to avoid huge diffs exhausting context.

Acceptance criteria:

- One semantic collection per file, not per hunk.
- Large diffs are capped with explicit truncation metadata.
- Multi-file diffs fail soft per file where possible.

## Phase 6 — Add Tool Operation

Add a new read-only LSP tool operation.

Suggested operation name:

- `hunkSourceContext`

Inputs:

```json
{
  "operation": "hunkSourceContext",
  "file_path": "src/foo.rs",
  "patch": "...optional unified diff...",
  "hunks": "...optional pre-parsed hunk list...",
  "include_definitions": true,
  "include_references": true,
  "include_call_hierarchy": false,
  "include_type_hierarchy": false,
  "radius": 40,
  "max_hunks": 20
}
```

Input rules:

- Accept either `patch` or pre-parsed hunks, not both.
- If `patch` is supplied, parse hunks from it.
- If `file_path` is supplied with a multi-file patch, filter to that file unless explicit multi-file mode is added.
- If no patch/hunks are supplied, optionally derive hunks from current git diff in a later pass; do not do implicit git reads in the first pass unless existing tool conventions allow it.

Output:

- `LspToolOutput<HunkSourceNavigationResponse>` or a presentation adapter preserving existing tool output conventions.
- `operation = "hunkSourceContext"`.
- `result_count = hunk count + evidence count`.
- `truncated = response.truncated`.

Acceptance criteria:

- Operation is read-only.
- Operation enforces allowed root/path validation.
- Operation preserves preview-only boundaries.
- Operation does not apply patches.

## Phase 7 — Integrate With `semanticContext` Optional Output

After `hunkSourceContext` works, consider adding optional hunk evidence to `semanticContext` itself.

Do this only if it does not bloat default responses.

Potential field:

```rust
hunks: Option<Vec<HunkEvidence>>
```

Guarded by input:

```json
"include_hunk_context": true
```

Recommendation:

- Keep `hunkSourceContext` as the primary operation first.
- Avoid making ordinary `semanticContext` heavier.
- Add integration only after agents prove they need a single merged packet.

Acceptance criteria:

- Default `semanticContext` remains unchanged.
- Hunk evidence is opt-in.
- Existing tests remain stable.

## Phase 8 — Security Context Optional Hunk Enrichment

After the base hunk navigator exists, add optional security review use.

Security-specific behavior:

- Use hunk evidence to prioritize risk markers inside changed ranges.
- Treat diagnostics outside hunks as lower-priority context.
- Highlight changed symbols with security-relevant names or categories.
- Preserve stale/unavailable diagnostic warnings.
- Never claim the hunk is safe because no diagnostic intersects it.

Potential security output additions:

- `hunk_risk_markers`;
- `hunk_relevant_diagnostics`;
- `hunk_enclosing_symbols`;
- `changed_security_surface_notes`.

Recommendation:

- Do not include this in the first hunk/source navigation implementation.
- Add after `hunkSourceContext` is stable.

Acceptance criteria:

- Security hunk enrichment is opt-in.
- Existing `securityContext` output remains compatible.
- Security notes explicitly distinguish changed-range evidence from whole-file evidence.

## Phase 9 — Tests

Add tests in layers.

### Pure parser tests

- single hunk parse;
- multiple hunks same file;
- multi-file diff grouping;
- omitted hunk lengths;
- additions-only hunk;
- deletions-only hunk;
- malformed hunk header returns structured error.

### Range matching tests

- diagnostic inside hunk;
- diagnostic adjacent to hunk;
- symbol fully contains hunk;
- nested symbols choose smallest containing symbol;
- hunk overlaps two symbols;
- no symbol near hunk yields no enclosing symbol but a note.

### Navigator tests

Use static `SemanticContextResponse` fixtures:

- hunk maps to enclosing function;
- hunk maps to diagnostics and references;
- hunk preserves diagnostic freshness;
- hunk preserves unavailable metadata;
- hunk truncates related symbols per cap;
- multi-hunk response preserves deterministic order.

### Tool-level tests

Avoid live LSP servers. If full `LspTool::execute` requires a live server, test:

- input validation;
- patch-vs-hunks mutual exclusion;
- parser integration;
- adapter serialization.

Acceptance criteria:

- Core hunk/source-nav behavior is covered without live LSP.
- Tests would fail if another parallel low-level LSP collection path was introduced.
- Tests would fail if diagnostic freshness is dropped.

## Phase 10 — Documentation

Update docs only after implementation.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`, only for verified facts

Docs should state:

- hunk/source navigation consumes `SemanticContextResponse`;
- hunk parsing is read-only and does not apply patches;
- evidence is best-effort and bounded;
- diagnostic freshness is preserved per hunk;
- overlay/source-action/security call expansion boundaries remain unchanged;
- hunk context is not whole-program analysis.

Acceptance criteria:

- Docs do not imply hunk context can prove correctness or security.
- Docs describe caps/truncation behavior.
- Docs identify the public operation name and input/output shape.

## Suggested Verification Commands

Run:

```bash
cargo fmt --all
cargo test -p egglsp
cargo test --lib lsp
```

Then, if touched modules are broad enough:

```bash
cargo test --all --workspace
```

If full workspace tests are skipped, record the reason in the implementation summary.

## Review Checklist

Before considering this phase complete:

- Hunk DTOs are serializable and documented.
- Unified diff parsing handles common hunk headers.
- Range overlap/containment helpers are tested.
- `HunkSourceNavigator` consumes `SemanticContextResponse` and does not call low-level LSP directly.
- Diagnostic freshness and unavailable metadata survive into hunk evidence.
- The tool operation is read-only and bounded.
- Multi-file or large diff behavior is capped and explicit.
- No overlay/source-action/security-call-expansion ownership boundaries are regressed.

## Expected Follow-Up

After this pass:

1. Add hunk-aware prompt routing for code review/edit-planning agents.
2. Use hunk evidence in security review enrichment.
3. Add optional git-diff discovery mode if it fits existing tool conventions.
4. Consider richer per-hunk call graph expansion only after the basic hunk/source mapping proves useful.
