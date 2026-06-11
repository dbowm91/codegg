# LSP Semantic Context Packets Plan

## Purpose

Add compact LSP-backed semantic context packets for edit, review, security, and explain workflows.

The recent LSP work now provides:

```text
renamePreview / formatPreview / sourceActionPreview
  -> WorkspaceEditPreview patches

semanticCheckPreview(content | patch)
  -> temporary overlay diagnostics/symbols without disk writes
```

The next step is to package this information into a small, stable, model-facing context bundle that agents can request before editing or reviewing code.

The goal is not to expose more raw LSP surface. The goal is to produce high-signal context packets that combine:

- current diagnostics;
- overlay diagnostics if proposed content or patch is supplied;
- document symbols near the target area;
- definitions/references for a target position when provided;
- nearby source excerpt;
- optional semantic-check summary;
- bounded metadata and truncation flags.

## Target Feature

Add a model-facing operation:

```json
{
  "operation": "semanticContext",
  "file_path": "src/main.rs",
  "line": 42,
  "column": 17,
  "radius": 40
}
```

Optional proposed-change form:

```json
{
  "operation": "semanticContext",
  "file_path": "src/main.rs",
  "line": 42,
  "column": 17,
  "patch": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ ...",
  "radius": 40
}
```

Recommended output shape:

```json
{
  "operation": "semanticContext",
  "file_path": "src/main.rs",
  "result_count": 12,
  "truncated": false,
  "results": {
    "file": "src/main.rs",
    "target": { "line": 42, "column": 17 },
    "excerpt": {
      "start_line": 2,
      "end_line": 82,
      "text": "..."
    },
    "diagnostics": [...],
    "overlay": {
      "used": true,
      "diagnostics_may_still_be_warming": false,
      "diagnostics": [...],
      "diagnostics_error": null,
      "symbols_error": null,
      "restore_error": null
    },
    "symbols": [...],
    "definitions": [...],
    "references": [...],
    "limits": {
      "diagnostics_truncated": false,
      "symbols_truncated": false,
      "references_truncated": false,
      "excerpt_truncated": false
    }
  }
}
```

## Non-Goals

Do not expose raw LSP JSON.

Do not add completion or code lens to the model-facing API.

Do not execute LSP commands.

Do not write proposed content to disk.

Do not add persistent overlays.

Do not add multi-file overlay support in this pass.

Do not build a vector index.

Do not make semantic packets huge by default.

## Current State Summary

Relevant files:

```text
src/tool/lsp.rs
src/tool/patch_util.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/overlay.rs
crates/egglsp/src/diagnostics.rs
architecture/lsp.md
architecture/tool.md
tests/lsp.rs
```

Useful existing pieces:

- `diagnostics` operation returns compact diagnostics.
- `documentSymbol` returns bounded symbol summaries.
- `goToDefinition` and `findReferences` already summarize locations.
- `semanticCheckPreview` supports `content` or single-file `patch` and preserves no-disk-write semantics.
- `resolve_semantic_check_content` can apply a patch in memory at the wrapper layer.
- `lsp` remains `ToolCategory::ReadOnly`.

## Design Rule

`semanticContext` should be a compact synthesis operation, not a new editing primitive.

It should:

1. gather existing read-only semantic facts;
2. optionally run overlay semantic check against proposed content/patch;
3. include a bounded source excerpt;
4. return a stable JSON DTO;
5. preserve read-only and no-disk-write boundaries.

## Phase 1 — Add DTOs in `src/tool/lsp.rs`

Keep the first pass at the tool-wrapper layer unless shared crate reuse becomes obviously beneficial.

Add DTOs near existing summaries:

```rust
#[derive(Serialize)]
struct SemanticContextPacket {
    file: String,
    target: Option<SemanticContextTarget>,
    excerpt: SourceExcerpt,
    diagnostics: Vec<DiagnosticSummary>,
    overlay: Option<SemanticOverlaySummary>,
    symbols: Vec<SymbolSummary>,
    definitions: Vec<LocationSummary>,
    references: Vec<LocationSummary>,
    limits: SemanticContextLimits,
}

#[derive(Serialize)]
struct SemanticContextTarget {
    line: u32,
    column: u32,
}

#[derive(Serialize)]
struct SourceExcerpt {
    start_line: u32,
    end_line: u32,
    text: String,
}

#[derive(Serialize)]
struct SemanticOverlaySummary {
    used: bool,
    diagnostics_may_still_be_warming: bool,
    diagnostics: Vec<DiagnosticSummary>,
    diagnostics_error: Option<String>,
    symbols: Vec<crate::lsp::overlay::SemanticSymbolSummary>,
    symbols_error: Option<String>,
    restored_disk_view: bool,
    restore_error: Option<String>,
}

#[derive(Serialize)]
struct SemanticContextLimits {
    diagnostics_truncated: bool,
    overlay_diagnostics_truncated: bool,
    symbols_truncated: bool,
    references_truncated: bool,
    excerpt_truncated: bool,
}
```

Keep field names stable and boring. Avoid nested raw LSP types.

Acceptance criteria:

- DTOs serialize cleanly;
- no raw `lsp_types::*` leaks into final JSON except through already compact summaries;
- output shape is documented by tests.

## Phase 2 — Add Input Fields

Extend `LspInput`:

```rust
#[serde(default)]
radius: Option<u32>,

#[serde(default)]
include_references: Option<bool>,

#[serde(default)]
include_definitions: Option<bool>,

#[serde(default)]
include_overlay: Option<bool>,
```

Reuse existing fields:

```text
file_path
line
column
content
patch
```

Defaults:

```text
radius = 40 lines
include_references = true only when line+column are provided
include_definitions = true only when line+column are provided
include_overlay = true when content or patch is provided
```

Caps:

```rust
const MAX_SEMANTIC_CONTEXT_RADIUS: u32 = 120;
const DEFAULT_SEMANTIC_CONTEXT_RADIUS: u32 = 40;
const MAX_CONTEXT_DIAGNOSTICS: usize = 100;
const MAX_CONTEXT_SYMBOLS: usize = 120;
const MAX_CONTEXT_REFERENCES: usize = 80;
const MAX_CONTEXT_EXCERPT_BYTES: usize = 32_000;
```

Acceptance criteria:

- omitted radius is deterministic;
- excessive radius is capped;
- overlay is only run when content/patch is supplied unless explicitly requested and possible.

## Phase 3 — Source Excerpt Helper

Add helper:

```rust
fn build_source_excerpt(
    file: &Path,
    target_line: Option<u32>,
    radius: u32,
) -> Result<(SourceExcerpt, bool), ToolError>
```

Behavior:

1. Read current disk file.
2. Treat `target_line` as 1-indexed.
3. If no target line, excerpt from the top of the file with bounded radius.
4. Start at `max(1, target_line - radius)`.
5. End at `min(total_lines, target_line + radius)`.
6. Preserve line numbering in DTO fields.
7. Truncate text if `MAX_CONTEXT_EXCERPT_BYTES` is exceeded.
8. Do not include entire giant files by accident.

Recommended text format:

```text
<raw source lines only>
```

Do not prefix every line with line numbers in this first pass unless existing UI conventions prefer it. Keep DTO start/end lines for line mapping.

Acceptance criteria:

- excerpt is bounded by radius and byte cap;
- binary/unreadable files return a clear error;
- tests cover top-of-file, middle, end-of-file, and large excerpt truncation.

## Phase 4 — Gather Current Diagnostics and Symbols

In `semanticContext` branch:

1. Resolve/validate `file_path`.
2. Build source excerpt.
3. Call existing diagnostics path:
   - either reuse `DiagnosticsCollector`/service methods directly if ergonomic;
   - or call `ops` equivalent if already available.
4. Cap diagnostics to `MAX_CONTEXT_DIAGNOSTICS`.
5. Call `document_symbols(file)` and flatten/cap to `MAX_CONTEXT_SYMBOLS`.

Important: diagnostics/symbol failure should not prevent a context packet if excerpt succeeds. Preserve errors if adding error fields, or return empty lists with conservative `*_truncated=false`. Prefer error fields if easy:

```rust
current_diagnostics_error: Option<String>
current_symbols_error: Option<String>
```

If adding fields, update DTO and docs.

Acceptance criteria:

- current file diagnostics are included when available;
- current document symbols are included when available;
- failures do not destroy the whole packet unless basic file reading fails.

## Phase 5 — Gather Position Context

When `line` and `column` are provided:

1. Convert with `to_lsp_position`.
2. Call `go_to_definition` if enabled.
3. Call `find_references` if enabled.
4. Cap references to `MAX_CONTEXT_REFERENCES`.
5. Keep definition/reference output as `LocationSummary`.

When only one of `line` or `column` is provided:

- return a validation error requiring both, or ignore both.
- Prefer validation error:

```text
semanticContext requires both line and column when either is supplied
```

Acceptance criteria:

- no position-based LSP calls are made without both line and column;
- definition/reference failures do not prevent excerpt/diagnostic/symbol context if feasible;
- result flags indicate references truncation.

## Phase 6 — Optional Overlay Context

If `content` or `patch` is supplied:

1. Use existing `resolve_semantic_check_content`.
2. Call `semantic_check_preview(&file, proposed, Some(&self.allowed_root))`.
3. Convert overlay diagnostics to `DiagnosticSummary`.
4. Include overlay fields and errors.
5. Preserve `restore_error` visibly.

If neither `content` nor `patch` is supplied:

```rust
overlay: None
```

If both are supplied, reuse existing error:

```text
semanticCheckPreview accepts either content or patch, not both
```

Do not apply source actions automatically.

Acceptance criteria:

- overlay uses the same no-disk-write path as `semanticCheckPreview`;
- patch input is applied in memory only;
- restore errors are surfaced in the packet;
- no new mutation path exists.

## Phase 7 — Tool Schema and Dispatch

Update schema operation enum:

```text
semanticContext
```

Update description:

```text
semanticContext returns a compact LSP-backed context packet with source excerpt, diagnostics, symbols, and optional definition/reference/overlay information.
```

Add parameter descriptions for:

```text
radius
include_references
include_definitions
include_overlay
```

Add branch:

```rust
"semanticContext" => {
    let file = self.resolve_file(&parsed.file_path)?;
    let packet = self.build_semantic_context_packet(&ops, &file, &parsed).await?;
    serialize LspToolOutput { operation: "semanticContext", ... }
}
```

Prefer implementing `build_semantic_context_packet` as a private async helper on `LspTool` to keep `execute` readable.

Acceptance criteria:

- schema includes operation and parameters;
- `lsp` remains read-only;
- branch is not a large inline blob if avoidable.

## Phase 8 — Tests

Default tests must be hermetic.

Pure/helper tests:

```text
semantic_context_excerpt_top_of_file
semantic_context_excerpt_middle
semantic_context_excerpt_end_of_file
semantic_context_excerpt_caps_radius
semantic_context_excerpt_truncates_large_text
semantic_context_requires_file_path
semantic_context_requires_line_and_column_together
semantic_context_rejects_content_and_patch
semantic_context_patch_does_not_write_disk
```

Schema tests:

```text
lsp_schema_includes_semanticContext
lsp_schema_includes_radius_and_include_flags
semanticContext_is_read_only
```

Serialization tests:

```text
semantic_context_packet_serializes_empty_optional_sections
semantic_context_packet_serializes_overlay_errors
```

Optional integration test with real LSP:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test --test lsp semanticContext_real_lsp -- --nocapture
```

It should skip if no suitable server is available.

Acceptance criteria:

- default tests do not launch LSP servers;
- excerpt behavior is well covered;
- patch no-disk-write behavior is covered;
- schema and validation are covered.

## Phase 9 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
```

Document:

- `semanticContext` is a compact packet operation;
- it includes source excerpt, diagnostics, symbols, definitions/references where requested;
- it can include overlay diagnostics from `content` or single-file `patch`;
- it does not write files;
- all sections are bounded;
- diagnostics may be warming/stale for the same reasons as `semanticCheckPreview`;
- mutation still requires `apply_patch`.

Recommended doc snippet:

```markdown
`semanticContext` is the preferred agent-facing pre-edit/pre-review context operation. It combines a bounded source excerpt with current diagnostics, document symbols, optional definition/reference information, and optional overlay diagnostics for proposed content or a single-file patch. It is read-only and never applies changes.
```

## Phase 10 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test --test lsp semanticContext
cargo test --test lsp semantic_context
rg "semanticContext|SemanticContextPacket|build_semantic_context|SourceExcerpt" src/tool/lsp.rs tests/lsp.rs architecture/lsp.md architecture/tool.md
rg "std::fs::write|tokio::fs::write" src/tool/lsp.rs crates/egglsp/src/operations.rs crates/egglsp/src/overlay.rs
```

Manual smoke:

```text
1. Run semanticContext on a known Rust file with line+column.
2. Confirm excerpt, symbols, diagnostics, definitions, and references are bounded.
3. Run semanticContext with a single-file patch that introduces a syntax/type error.
4. Confirm overlay section appears and disk file is unchanged.
5. Confirm result remains useful if references or symbols fail.
```

## Done Criteria

This pass is complete when:

- `semanticContext` is exposed as a read-only LSP operation;
- it returns a stable compact packet DTO;
- source excerpt is bounded and tested;
- current diagnostics/symbols are included where available;
- definition/reference context is included when line+column are provided;
- overlay diagnostics are included for content/patch input;
- no disk writes or command execution paths are introduced;
- tests and docs cover semantics and limits.

## Follow-Up Passes

After this lands:

1. Add source-action hints inside `semanticContext` for safe allowlisted actions such as organize imports.
2. Add multi-file overlay context for multi-file edit previews.
3. Add call hierarchy/type hierarchy summaries.
4. Add security-review context packets that prioritize taint-like flows, auth boundaries, unsafe code, dependency-sensitive files, and diagnostics severity.
