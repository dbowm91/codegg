# LSP Semantic Context Source Action Hints Plan

## Purpose

Add safe source-action hints to `semanticContext`, starting with `source.organizeImports`.

The current LSP stack now has:

```text
sourceActionPreview(action=source.organizeImports)
  -> WorkspaceEditPreview
  -> patch preview only
  -> apply_patch for mutation

semanticContext
  -> bounded source excerpt
  -> current diagnostics/symbols
  -> optional definitions/references
  -> optional overlay diagnostics from content/patch
```

This pass should let `semanticContext` include a small optional section describing whether a safe allowlisted source action is available and previewable. It must not execute commands, apply edits, or broaden model-facing code actions.

## Target Feature

Add optional source-action hints to `semanticContext` output:

```json
{
  "operation": "semanticContext",
  "file_path": "src/main.rs",
  "line": 1,
  "column": 1,
  "include_source_actions": true
}
```

Recommended output addition:

```json
"source_actions": [
  {
    "action": "source.organizeImports",
    "available": true,
    "preview": {
      "title": "organize imports",
      "total_files": 1,
      "total_edits": 3,
      "truncated": false,
      "files": [...]
    },
    "error": null
  }
]
```

If no edit-bearing organize-import action exists:

```json
"source_actions": [
  {
    "action": "source.organizeImports",
    "available": false,
    "preview": null,
    "error": "No edit-bearing source action available"
  }
]
```

## Non-Goals

Do not add arbitrary `codeAction` exposure.

Do not add `quickfix`, `refactor`, or `source.fixAll`.

Do not execute returned LSP commands.

Do not apply the preview patch.

Do not mutate files.

Do not run source actions automatically by default.

Do not support multi-file overlays for source action hints in this pass.

## Current State Summary

Relevant files:

```text
src/tool/lsp.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/edit.rs
crates/egglsp/src/overlay.rs
tests/lsp.rs
architecture/lsp.md
architecture/tool.md
```

Useful existing pieces:

- `SourceActionPreviewKind` only supports organize imports and aliases.
- `source_action_preview` returns `WorkspaceEditPreview` and reuses the preview-only mutation boundary.
- `select_source_action_edit` rejects raw commands, command-only actions, no-edit actions, nonmatching actions, and ambiguous edit-bearing actions.
- `semanticContext` already has a stable compact packet DTO.
- `lsp` remains `ToolCategory::ReadOnly`.

## Design Rule

Source-action hints are advisory preview metadata.

The model-facing invariant remains:

```text
LSP source actions may produce preview patches.
Only apply_patch or another explicit mutating tool may write files.
```

`semanticContext` should never execute LSP commands or directly apply preview patches.

## Phase 1 — Add Input Flag

Extend `LspInput` in `src/tool/lsp.rs`:

```rust
#[serde(default)]
include_source_actions: Option<bool>,
```

Default:

```text
include_source_actions = false
```

Rationale: source-action preview may require an LSP request and may be noisier than regular context. Keep it opt-in initially.

Update schema:

```json
"include_source_actions": {
  "type": "boolean",
  "description": "Include safe allowlisted source-action preview hints in semanticContext. Initially only source.organizeImports. Default false."
}
```

Acceptance criteria:

- schema documents the flag;
- default false is preserved;
- no source-action LSP request is made unless true.

## Phase 2 — Add DTOs

Add to `src/tool/lsp.rs`:

```rust
#[derive(Serialize)]
struct SemanticSourceActionHint {
    action: String,
    available: bool,
    preview: Option<crate::lsp::edit::WorkspaceEditPreview>,
    error: Option<String>,
}
```

Then add to `SemanticContextPacket`:

```rust
source_actions: Vec<SemanticSourceActionHint>,
```

Alternatively, if importing `WorkspaceEditPreview` directly is awkward, define a smaller summary:

```rust
#[derive(Serialize)]
struct SemanticSourceActionPreviewSummary {
    title: String,
    total_files: usize,
    total_edits: usize,
    truncated: bool,
    files: Vec<crate::lsp::edit::FileEditPreview>,
}
```

Preferred first pass: reuse `WorkspaceEditPreview` to avoid lossy conversion and preserve existing patch preview shape.

Acceptance criteria:

- output can represent available preview, unavailable action, and errors;
- preview remains the same shape as `sourceActionPreview`;
- packet serialization remains stable.

## Phase 3 — Implement Source-Action Hint Collection

Add helper on `LspTool`:

```rust
async fn collect_source_action_hints(
    &self,
    ops: &crate::lsp::operations::LspOperations,
    file: &Path,
) -> Vec<SemanticSourceActionHint>
```

For this pass, the list is hardcoded:

```rust
let actions = [SourceActionPreviewKind::OrganizeImports];
```

Behavior per action:

```rust
match ops.source_action_preview(file, action, Some(&self.allowed_root)).await {
    Ok(preview) if preview.total_edits > 0 => SemanticSourceActionHint {
        action: "source.organizeImports".to_string(),
        available: true,
        preview: Some(preview),
        error: None,
    },
    Ok(preview) => SemanticSourceActionHint {
        action: "source.organizeImports".to_string(),
        available: false,
        preview: Some(preview),
        error: Some("source action produced no edits".to_string()),
    },
    Err(e) => SemanticSourceActionHint {
        action: "source.organizeImports".to_string(),
        available: false,
        preview: None,
        error: Some(e.to_string()),
    },
}
```

Do not classify no-edit as a fatal `semanticContext` error. It is just a hint section.

Acceptance criteria:

- organize-import preview can appear in semantic context;
- errors are captured per action;
- source-action failures do not fail the whole `semanticContext` packet;
- no source-action command execution occurs.

## Phase 4 — Wire into `semanticContext`

Inside the `semanticContext` branch:

```rust
let include_source_actions = parsed.include_source_actions.unwrap_or(false);
let source_actions = if include_source_actions {
    self.collect_source_action_hints(&ops, &file).await
} else {
    Vec::new()
};
```

Add `source_actions` to `SemanticContextPacket`.

Update `result_count`:

```rust
let source_action_count = source_actions
    .iter()
    .filter(|hint| hint.available)
    .count();
```

Then include it in result count. Do not count unavailable hints as results unless current code convention prefers all returned sections.

Acceptance criteria:

- default semanticContext output remains compact;
- source-action hints are opt-in;
- available hints affect `result_count`;
- unavailable hints are still visible in `source_actions` when requested.

## Phase 5 — Safety Checks

Verify these invariants explicitly:

1. `sourceActionPreview` still only accepts `source.organizeImports` and aliases.
2. `semanticContext` source-action hints do not accept arbitrary action input.
3. `semanticContext` source-action hints do not call `code_actions` directly.
4. No returned LSP `Command` is executed.
5. No patch application happens inside source-action hint collection.
6. The only mutation path remains external `apply_patch`.

Recommended `rg` checks:

```bash
rg "source.fixAll|quickfix|refactor" src/tool/lsp.rs crates/egglsp/src/operations.rs tests/lsp.rs
rg "code_actions\(" src/tool/lsp.rs
rg "apply_patch|apply_unified_diff" src/tool/lsp.rs crates/egglsp/src/operations.rs
```

Acceptance criteria:

- generic code actions remain internal-only;
- semanticContext cannot be used to request unsupported source actions;
- command execution remains impossible.

## Phase 6 — Tests

Default tests must remain hermetic where possible.

Schema/input tests:

```text
lsp_schema_includes_include_source_actions
semantic_context_source_actions_default_false
semantic_context_source_actions_is_read_only
```

DTO serialization tests:

```text
semantic_context_packet_serializes_empty_source_actions
semantic_context_packet_serializes_source_action_error_hint
```

Behavior tests without real LSP are harder because `source_action_preview` requires an LSP server. Do not introduce brittle integration defaults. Instead, test the helper shape if feasible by extracting pure conversion:

```rust
fn source_action_hint_from_preview_result(
    action: SourceActionPreviewKind,
    result: Result<WorkspaceEditPreview, LspError>,
) -> SemanticSourceActionHint
```

Pure tests:

```text
source_action_hint_available_when_preview_has_edits
source_action_hint_unavailable_when_preview_empty
source_action_hint_captures_error
```

Optional real-LSP integration:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test --test lsp semanticContext_source_actions_real_lsp -- --nocapture
```

It should skip cleanly if no suitable server is available.

Acceptance criteria:

- schema flag is tested;
- default false behavior is tested;
- pure result-to-hint conversion is tested;
- no default test requires a real language server.

## Phase 7 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
```

Document:

- `semanticContext` can include source-action hints when `include_source_actions=true`;
- only `source.organizeImports` is supported initially;
- hints are preview-only;
- command-only actions are rejected by existing source-action policy;
- unavailable hints are nonfatal;
- applying an action still requires `apply_patch` using the returned preview patch.

Recommended snippet:

```markdown
`semanticContext` may include safe source-action hints when requested. These hints reuse `sourceActionPreview` and are currently limited to `source.organizeImports`. They never execute commands or apply edits; they only return preview metadata and patches that can be applied later through `apply_patch`.
```

## Phase 8 — Validation Commands

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
cargo test --test lsp source_action_hint
rg "include_source_actions|SemanticSourceActionHint|source_actions|source.organizeImports" src/tool/lsp.rs tests/lsp.rs architecture/lsp.md architecture/tool.md
rg "source.fixAll|quickfix|refactor" src/tool/lsp.rs crates/egglsp/src/operations.rs tests/lsp.rs architecture
rg "code_actions\(" src/tool/lsp.rs
```

Manual smoke:

```text
1. Run semanticContext without include_source_actions; confirm source_actions is empty or omitted per implementation.
2. Run semanticContext with include_source_actions=true on a Rust file with disordered imports.
3. Confirm source.organizeImports hint appears with preview patch if LSP supports it.
4. Confirm disk file remains unchanged.
5. Apply the returned patch manually through apply_patch only if desired.
```

## Done Criteria

This pass is complete when:

- `semanticContext` supports `include_source_actions` flag;
- source-action hints are opt-in and bounded;
- only `source.organizeImports` is included;
- hints reuse preview-only `sourceActionPreview` behavior;
- command execution and mutation remain impossible;
- tests cover schema/default/conversion behavior;
- docs explain preview-only source-action hints.

## Next Pass After This

After this lands, move to call hierarchy and type hierarchy summaries.

That pass should add explicit read-only LSP operations and optional semantic-context sections for call/type relationships without exposing raw LSP response objects.
