# LSP Semantic Context Hardening Plan

## Purpose

Tighten the first `semanticContext` implementation before adding source-action hints, call/type hierarchy, or security-specific context packets.

The first pass landed the broad feature:

- `semanticContext` is exposed as a read-only LSP operation.
- It returns a compact DTO with source excerpt, current diagnostics, symbols, definitions, references, optional overlay diagnostics, and limits.
- It supports `content`/`patch` overlay input through the existing no-disk-write semantic-check path.
- It has initial helper tests for source excerpts and patch non-mutation.

This hardening pass should fix contract correctness issues and improve test coverage without expanding the semantic surface.

## Current Issues

1. `diagnostics_truncated` is always `false` even though diagnostics are capped.
2. `symbols_truncated` is always `false` even though symbols are capped.
3. Definition/reference errors are swallowed silently.
4. `result_count` excludes overlay diagnostics/symbols.
5. `execute_structured` only checks `/results/restore_error`, but nested overlay restore errors live at `/results/overlay/restore_error`.
6. `semantic_context_requires_file_path` is misnamed/incomplete; it checks schema rather than executing missing `file_path`.
7. `SemanticContextLimits` lacks `overlay_diagnostics_truncated` even though overlay diagnostics may grow.
8. `build_source_excerpt` truncates bytes with `String::from_utf8_lossy(&text.as_bytes()[..MAX])`; this can split UTF-8 and insert replacement characters. It is acceptable but should be deliberate and tested, or replaced with char-boundary truncation.

## Non-Goals

Do not add source-action hints.

Do not add call hierarchy or type hierarchy.

Do not add multi-file overlays.

Do not add persistent overlays.

Do not expose completions or code lens.

Do not execute LSP commands.

Do not write proposed content to disk.

## Phase 1 — Fix Truncation Accounting

### Current diagnostics

Current code caps diagnostics with:

```rust
.iter().take(MAX_CONTEXT_DIAGNOSTICS)
```

but emits:

```rust
diagnostics_truncated: false
```

Change diagnostic collection to track truncation:

```rust
let raw_len = diag_output.diagnostics.len();
let diagnostics_truncated = raw_len > MAX_CONTEXT_DIAGNOSTICS;
let diags = diag_output
    .diagnostics
    .iter()
    .take(MAX_CONTEXT_DIAGNOSTICS)
    ...;
```

### Current symbols

Current symbol flattening uses `remaining`, but truncation is not surfaced.

Use:

```rust
let mut remaining = MAX_CONTEXT_SYMBOLS;
let mut summaries = Vec::new();
Self::flatten_symbols(&syms, &file_str, &mut summaries, &mut remaining);
let symbols_truncated = remaining == 0;
```

This is slightly conservative because exactly hitting the cap may be indistinguishable from truncation without knowing total flattened count. Acceptable first pass, or add `flatten_symbols` return metadata if desired.

### Overlay diagnostics

Add `overlay_diagnostics_truncated` to `SemanticContextLimits` and cap overlay diagnostics in `semanticContext`:

```rust
let overlay_diag_truncated = preview.diagnostics.len() > MAX_CONTEXT_DIAGNOSTICS;
let diag_summaries = preview.diagnostics.iter().take(MAX_CONTEXT_DIAGNOSTICS) ...;
```

Acceptance criteria:

- current diagnostics truncation flag is accurate;
- current symbols truncation flag is conservative and documented by tests;
- overlay diagnostics truncation is represented;
- tests cover at least diagnostics and symbol limit flags via DTO/helper-level paths where feasible.

## Phase 2 — Preserve Definition and Reference Errors

Definition/reference failures currently produce empty vectors with no error field.

Extend DTO:

```rust
struct SemanticContextPacket {
    ...
    definitions: Vec<LocationSummary>,
    definitions_error: Option<String>,
    references: Vec<LocationSummary>,
    references_error: Option<String>,
    ...
}
```

Collection policy:

```rust
let (definitions, definitions_error) = if has_position && want_defs {
    match ops.go_to_definition(...).await {
        Ok(defs) => (..., None),
        Err(e) => (Vec::new(), Some(format!("goToDefinition: {e}"))),
    }
} else {
    (Vec::new(), None)
};
```

Same for references:

```rust
Err(e) => (Vec::new(), Some(format!("findReferences: {e}")))
```

Do not fail the whole packet for these errors. `semanticContext` is a best-effort synthesis operation.

Acceptance criteria:

- no definitions/references is distinguishable from request failure;
- output remains useful when one LSP subrequest fails;
- tests cover serialization of these error fields.

## Phase 3 — Fix `result_count`

Current `result_count` excludes overlay diagnostics and overlay symbols.

Change count to include all returned list-like sections:

```rust
let overlay_count = packet.overlay.as_ref().map(|o| {
    o.diagnostics.len() + o.symbols.len()
}).unwrap_or(0);

let result_count = packet.diagnostics.len()
    + packet.symbols.len()
    + packet.definitions.len()
    + packet.references.len()
    + overlay_count;
```

If computing before packet construction, compute from local variables including overlay.

Acceptance criteria:

- `result_count` reflects visible packet list items;
- tests cover an overlay-bearing packet serialization or helper-level count if feasible.

## Phase 4 — Fix Structured Success for Nested Restore Errors

Current `execute_structured` only checks:

```text
/results/restore_error
```

For `semanticContext`, restore error is nested:

```text
/results/overlay/restore_error
```

Change success detection to check both paths:

```rust
let success = match serde_json::from_str::<serde_json::Value>(&output) {
    Ok(v) => {
        let top_restore_error = v.pointer("/results/restore_error")
            .and_then(|e| e.as_str())
            .is_some();
        let overlay_restore_error = v.pointer("/results/overlay/restore_error")
            .and_then(|e| e.as_str())
            .is_some();
        !(top_restore_error || overlay_restore_error)
    }
    Err(_) => true,
};
```

Optional: also treat `diagnostics_error`/`symbols_error` as success=true because packet still provides partial information. Restore failure is the critical stale-LSP-state signal.

Acceptance criteria:

- `semanticContext` with nested overlay restore failure yields structured success=false;
- `semanticCheckPreview` behavior remains unchanged;
- test covers nested path logic, preferably by extracting helper:

```rust
fn structured_lsp_success(output: &str) -> bool
```

## Phase 5 — Correct Wrapper Validation Tests

Fix/replace the misleading test:

```text
semantic_context_requires_file_path
```

It currently checks schema only. It should execute without `file_path`:

```rust
let err = tool.execute(json!({ "operation": "semanticContext" })).await.unwrap_err();
assert!(matches!(err, ToolError::Execution(ref m) if m.contains("file_path")));
```

Add or keep explicit schema test separately:

```text
lsp_schema_includes_semanticContext
lsp_schema_includes_radius_and_include_flags
semanticContext_is_read_only
```

Add validation tests:

```text
semantic_context_requires_line_and_column_together
semantic_context_rejects_content_and_patch
semantic_context_patch_does_not_write_disk
```

These mostly exist; ensure names match behavior.

Acceptance criteria:

- no misleading test names;
- missing `file_path` behavior is tested;
- schema inclusion is tested separately;
- read-only category remains tested.

## Phase 6 — Harden Source Excerpt Truncation

Current byte truncation may split UTF-8. Either make that deliberate and test it, or switch to char-boundary truncation.

Preferred helper:

```rust
fn truncate_to_byte_limit_on_char_boundary(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}
```

Then use it in `build_source_excerpt`.

Add tests:

```text
semantic_context_excerpt_truncates_on_utf8_boundary
semantic_context_excerpt_truncates_large_text
```

Acceptance criteria:

- excerpt truncation never emits replacement characters from split UTF-8;
- byte cap remains enforced;
- tests cover Unicode near the boundary.

## Phase 7 — Optional Refactor: Extract Semantic Context Builder

The `semanticContext` branch is large. If this pass touches the branch substantially, consider extracting:

```rust
async fn build_semantic_context_packet(
    &self,
    ops: &crate::lsp::operations::LspOperations,
    file: &Path,
    parsed: &LspInput,
) -> Result<SemanticContextPacket, ToolError>
```

Keep this optional. Do not refactor if it risks expanding the patch too much.

Acceptance criteria if done:

- `execute` branch becomes smaller;
- tests remain unchanged;
- no behavior change beyond the hardening items.

## Phase 8 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
```

Document:

- `semanticContext` is best-effort and may include per-section errors;
- definition/reference failures are represented as error fields, not fatal packet failures;
- truncation flags are per section;
- overlay restore failure is treated as a structured failure signal;
- source excerpt is byte-bounded and UTF-8-safe;
- mutation still requires `apply_patch`.

Acceptance criteria:

- docs match DTO fields;
- docs do not imply every section is always available;
- read-only/no-disk-write semantics remain explicit.

## Phase 9 — Validation Commands

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
cargo test -p codegg lsp_parameters_schema_snapshot
rg "semanticContext|SemanticContextPacket|definitions_error|references_error|overlay_diagnostics_truncated|structured_lsp_success" src/tool/lsp.rs tests/lsp.rs architecture/lsp.md architecture/tool.md
rg "from_utf8_lossy|MAX_CONTEXT_EXCERPT_BYTES" src/tool/lsp.rs
```

Manual smoke:

```text
1. Run semanticContext on a Rust file with line+column.
2. Confirm diagnostics/symbol/reference limits are accurate.
3. Run semanticContext with a patch overlay.
4. Confirm overlay diagnostics/symbols contribute to result_count.
5. Confirm disk file remains unchanged.
6. Simulate or inspect restore_error handling and confirm structured success would be false.
```

## Done Criteria

This pass is complete when:

- diagnostic, symbol, reference, overlay diagnostic, and excerpt truncation flags are truthful;
- definition/reference errors are visible in the packet;
- `result_count` includes overlay list items;
- structured success detects nested overlay restore errors;
- semanticContext wrapper validation tests are correctly named and meaningful;
- excerpt truncation is UTF-8-safe;
- docs match the hardened contract.

## Next Pass After This

After this cleanup lands, move to source-action hints inside `semanticContext`.

That pass should add optional allowlisted source-action suggestions, starting with organize imports, without executing commands or applying edits.
