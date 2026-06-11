# LSP Source Action Preview Plan

## Purpose

Add the next LSP feature layer: preview-only, allowlisted source actions.

The previous LSP feature pass established a safe edit-preview boundary:

```text
LSP request -> WorkspaceEditPreview -> unified diff patch preview -> apply_patch for mutation
```

This pass should reuse that boundary for source actions, starting with organize imports. The `lsp` tool must remain read-only. It should never execute language-server commands and never apply edits directly.

## Target Feature

Add a model-facing LSP operation:

```json
{
  "operation": "sourceActionPreview",
  "file_path": "src/main.rs",
  "action": "source.organizeImports"
}
```

Initial supported action:

```text
source.organizeImports
```

Optional aliases may be accepted by the wrapper:

```text
organizeImports -> source.organizeImports
organize_imports -> source.organizeImports
```

Output should be the same `WorkspaceEditPreview` shape used by `renamePreview` and `formatPreview`.

## Non-Goals

Do not expose arbitrary code actions.

Do not execute `Command` objects returned by the language server.

Do not support command-only code actions.

Do not support `quickfix`, `refactor`, or `source.fixAll` in this pass.

Do not apply edits directly.

Do not add overlay sync.

Do not add call hierarchy or semantic context packets.

## Current State Summary

Relevant files:

```text
crates/egglsp/src/edit.rs
crates/egglsp/src/operations.rs
src/tool/lsp.rs
tests/lsp.rs
architecture/lsp.md
architecture/tool.md
```

Current useful foundation:

- `WorkspaceEditPreview`, `FileEditPreview`, and `TextEditPreview` already exist.
- `preview_workspace_edit` converts LSP `WorkspaceEdit` into bounded patch previews.
- `renamePreview` and `formatPreview` already use the preview boundary.
- `lsp` remains `ToolCategory::ReadOnly`.
- actual mutation remains in `apply_patch`.
- internal `LspOperations::code_actions` already sends `textDocument/codeAction` and returns `Vec<CodeActionOrCommand>`.

## Design Rule

`sourceActionPreview` may only return concrete edit previews.

Allowed:

```text
CodeAction { edit: Some(WorkspaceEdit), command: None or ignored }
```

Rejected:

```text
Command-only actions
CodeAction with no edit
actions outside the allowlist
actions requiring command execution
multiple ambiguous matching actions
```

If a language server returns several organize-import actions, choose deterministically only when they are equivalent enough or clearly the same kind. Otherwise return an explicit ambiguity error with compact candidate metadata.

## Phase 1 — Add Action Normalization and Allowlist

In `crates/egglsp/src/operations.rs` or a small helper module, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceActionPreviewKind {
    OrganizeImports,
}

impl SourceActionPreviewKind {
    pub fn parse(input: &str) -> Result<Self, LspError>;
    pub fn lsp_kind(self) -> CodeActionKind;
    pub fn title(self) -> &'static str;
}
```

Accepted inputs:

```text
source.organizeImports
organizeImports
organize_imports
```

Rejected inputs should produce a clear error:

```text
unsupported source action '<input>'; supported actions: source.organizeImports
```

Acceptance criteria:

- only organize imports is accepted;
- aliases normalize to `CodeActionKind::SOURCE_ORGANIZE_IMPORTS`;
- `source.fixAll`, `quickfix`, and arbitrary strings are rejected.

## Phase 2 — Implement `source_action_preview` in `LspOperations`

Add to `crates/egglsp/src/operations.rs`:

```rust
pub async fn source_action_preview(
    &self,
    file_path: &Path,
    action: SourceActionPreviewKind,
    allowed_root: Option<&Path>,
) -> Result<WorkspaceEditPreview, LspError>;
```

Implementation details:

1. Call `service.ensure_file_open_from_disk(file_path).await?` before requesting actions.
2. Build a full-file range for `CodeActionParams`.
   - Read the file from disk or reuse the content returned by sync helpers if available.
   - Use start `(0, 0)` and end at the last line/UTF-16 column.
   - Avoid using a fake tiny range; organize imports often expects the whole document.
3. Use `CodeActionContext` with:

```rust
only: Some(vec![CodeActionKind::SOURCE_ORGANIZE_IMPORTS])
diagnostics: vec![]
trigger_kind: Some(CodeActionTriggerKind::INVOKED)
```

If `CodeActionTriggerKind` is unavailable or mismatched in `lsp-types`, omit it rather than blocking the feature.

4. Send `textDocument/codeAction`.
5. Parse `Vec<CodeActionOrCommand>`.
6. Filter to edit-bearing `CodeAction`s whose kind is exactly or hierarchically compatible with `source.organizeImports`.
7. Reject `Command` variants.
8. Reject `CodeAction` values without `edit`.
9. If no edit-bearing allowlisted action is found, return a clear no-edits error.
10. If multiple edit-bearing matches are found:
    - if exactly one has a non-empty `WorkspaceEdit`, use it;
    - otherwise return an ambiguity error with action titles/kinds.
11. Convert the selected edit with `preview_workspace_edit(action.title(), edit, allowed_root)`.
12. Do not apply anything.

Kind matching rule:

```text
accept kind == source.organizeImports
optionally accept child kinds prefixed by source.organizeImports.
reject source, source.fixAll, quickfix, refactor, etc.
```

Acceptance criteria:

- source action preview requests only organize-import actions;
- command-only responses are rejected;
- no direct file mutation occurs;
- returned preview shape is identical to rename/format preview style.

## Phase 3 — Add Model-Facing Tool Schema

Update `src/tool/lsp.rs`.

Changes:

1. Add `action: Option<String>` to `LspInput`.
2. Add `sourceActionPreview` to the operation enum.
3. Add `action` schema property:

```json
{
  "type": "string",
  "description": "Allowlisted source action for sourceActionPreview. Initially supports source.organizeImports."
}
```

4. Update description:

```text
Operations: ..., renamePreview, formatPreview, sourceActionPreview. Edit operations are previews only.
```

5. Add match branch:

```rust
"sourceActionPreview" => {
    let file = self.resolve_file(&parsed.file_path)?;
    let action = parsed.action.as_deref().ok_or_else(...)?;
    let kind = SourceActionPreviewKind::parse(action)?;
    let preview = ops.source_action_preview(&file, kind, Some(&self.allowed_root)).await?;
    serialize LspToolOutput { operation, file_path, result_count: preview.total_edits, truncated: preview.truncated, results: preview }
}
```

6. Keep `category()` as `ToolCategory::ReadOnly`.

Acceptance criteria:

- schema exposes `sourceActionPreview`;
- `sourceActionPreview` requires `file_path` and `action`;
- `lsp` remains read-only;
- `codeLens` remains hidden.

## Phase 4 — Error and Output Hygiene

Add or reuse error variants if helpful:

```rust
UnsupportedSourceAction(String)
NoEditForSourceAction(String)
AmbiguousSourceAction(String)
CommandOnlySourceAction(String)
```

Do not over-expand the error enum if simple `LspError::UnsupportedEdit` / `RequestFailed` messages are sufficient.

Recommended error messages:

```text
unsupported source action 'source.fixAll'; supported actions: source.organizeImports
source action 'source.organizeImports' returned only command actions; command execution is disabled
source action 'source.organizeImports' returned no edit-bearing actions
source action 'source.organizeImports' returned multiple edit-bearing actions: <titles>
```

Acceptance criteria:

- unsafe command execution is obviously rejected;
- unsupported actions fail before making an LSP request;
- errors are actionable for the model/user.

## Phase 5 — Tests

Add hermetic tests. Do not require a real language server by default.

Tests in `crates/egglsp/src/operations.rs` or a helper test module:

```text
source_action_kind_accepts_source_organize_imports
source_action_kind_accepts_aliases
source_action_kind_rejects_fix_all
source_action_kind_rejects_quickfix
source_action_kind_rejects_unknown
filter_source_actions_rejects_command_only
filter_source_actions_rejects_no_edit
filter_source_actions_selects_single_edit_bearing_organize_imports
filter_source_actions_rejects_ambiguous_multiple_edits
```

To make this testable, extract pure helpers:

```rust
fn select_source_action_edit(
    requested: SourceActionPreviewKind,
    actions: Vec<CodeActionOrCommand>,
) -> Result<WorkspaceEdit, LspError>;
```

Tests in `tests/lsp.rs` or `src/tool/lsp.rs` tests:

```text
lsp_schema_includes_sourceActionPreview
sourceActionPreview_requires_file_path
sourceActionPreview_requires_action
sourceActionPreview_rejects_unsupported_action_without_lsp_request_if_testable
lsp_tool_remains_read_only
codeLens_still_not_exposed
```

Optional integration test:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp source_action_preview_real_lsp -- --nocapture
```

The integration test should skip if `rust-analyzer` or another suitable server is unavailable.

## Phase 6 — Documentation Updates

Update:

```text
architecture/lsp.md
architecture/tool.md
```

`architecture/lsp.md` should include:

- `sourceActionPreview` in preview-only semantic edits;
- only `source.organizeImports` is supported initially;
- arbitrary code actions and command execution are intentionally rejected;
- mutation still requires `apply_patch`.

Also fix the existing small stale doc item:

- replace `get_or_create_client_for_root_hint` with the current service method name if the current code uses `find_existing_client_for_root_hint`.

`architecture/tool.md` should include `sourceActionPreview` in the LSP tool row and retain the read-only/mutation-boundary language.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted checks:

```bash
cargo test -p egglsp
cargo test --test lsp
rg "sourceActionPreview|source_action_preview|source.organizeImports|SourceActionPreviewKind" crates/egglsp src tests architecture
rg "codeLens|quickfix|source.fixAll|CommandOnly" src/tool/lsp.rs crates/egglsp/src/operations.rs tests/lsp.rs architecture/lsp.md
```

Manual smoke check with a real server:

```text
1. open a file with disordered imports
2. run lsp sourceActionPreview with action=source.organizeImports
3. confirm returned WorkspaceEditPreview has patch_omitted=false for small changes
4. confirm the file is unchanged
5. apply the returned patch through apply_patch
6. confirm the file changes only after apply_patch
```

## Done Criteria

This pass is complete when:

- `sourceActionPreview` is exposed model-facing;
- only `source.organizeImports` is accepted;
- command-only and arbitrary code actions are rejected;
- returned edits use `WorkspaceEditPreview`;
- `lsp` remains `ToolCategory::ReadOnly`;
- actual mutation still requires `apply_patch`;
- tests cover allowlist parsing and action selection;
- docs reflect the new operation and safety policy.

## Next Pass After This

After this lands, the next meaningful feature pass should be overlay-backed semantic checks for proposed patches.

That pass should introduce a document overlay/store layer so Codegg can ask LSP for diagnostics/outline against proposed text before committing it to disk.
