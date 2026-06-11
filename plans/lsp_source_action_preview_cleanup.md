# LSP Source Action Preview Cleanup Plan

## Purpose

Tighten the first `sourceActionPreview` implementation before moving on to overlay-backed semantic checks.

The current pass appears to have landed the broad feature:

- `SourceActionPreviewKind` exists.
- `source.organizeImports` plus aliases are accepted.
- unsupported actions are rejected.
- `select_source_action_edit` filters LSP code action responses to edit-bearing actions.
- model-facing `lsp` exposes `sourceActionPreview` and remains read-only.
- docs mention preview-only source actions.

This cleanup pass should address the remaining correctness and test gaps:

1. compute a real full-document range instead of using `u32::MAX` as the range end;
2. add hermetic tests for action parsing and edit selection;
3. classify `CodeAction { command: Some(_), edit: None }` as command-only/command-disabled when appropriate;
4. add wrapper tests for missing and unsupported `action`;
5. tighten docs to say only `source.organizeImports` is currently supported.

## Non-Goals

Do not add new source actions in this pass.

Do not add `source.fixAll`, `quickfix`, or `refactor`.

Do not execute returned LSP commands.

Do not apply edits directly.

Do not implement overlay sync.

Do not implement call hierarchy or semantic context packets.

## Phase 1 — Compute a Real Full-Document Range

Current issue:

`LspOperations::source_action_preview` builds a code-action range with:

```rust
end: Position {
    line: u32::MAX,
    character: u32::MAX,
}
```

Some language servers tolerate this, but it is not protocol-clean and may be rejected by stricter servers.

Required changes:

1. Read the file text from disk in `source_action_preview` after `ensure_file_open_from_disk`, or modify `ensure_file_open_from_disk` to optionally return the synced text if that is cleaner.
2. Add a helper to compute the final LSP position:

```rust
fn document_end_position_utf16(text: &str) -> Position
```

Expected behavior:

- empty string -> `(0, 0)`;
- one-line text with ASCII -> `(0, len)`;
- text ending in newline -> final line is the empty line after the newline, character `0`;
- unicode text counts UTF-16 code units, not bytes/chars;
- CRLF should not over-count `\r` as part of the line content if existing text-position helpers already normalize this elsewhere.

3. Use:

```rust
range: Range {
    start: Position { line: 0, character: 0 },
    end: document_end_position_utf16(&text),
}
```

4. Keep `only: Some(vec![CodeActionKind::SOURCE_ORGANIZE_IMPORTS])`.
5. Keep `trigger_kind: Some(CodeActionTriggerKind::INVOKED)` if supported by current `lsp-types`.

Acceptance criteria:

- no `u32::MAX` range remains in `source_action_preview`;
- full-document range is computed from actual synced file contents;
- UTF-16 position tests cover ASCII, Unicode, final newline, and empty file cases.

## Phase 2 — Improve Command-Only Classification

Current behavior:

`select_source_action_edit` returns `CommandOnlySourceAction` only when all returned actions are raw `CodeActionOrCommand::Command` variants.

This misses the common LSP shape:

```rust
CodeAction {
    kind: Some(source.organizeImports),
    edit: None,
    command: Some(Command { ... }),
    ...
}
```

That is still command-only from Codegg's safety perspective and should produce the command-disabled error, not a generic no-edit error.

Required changes:

1. Track whether any matching allowlisted action had `command: Some(_)` and `edit: None`.
2. Track whether any matching allowlisted action existed at all.
3. Return:
   - `UnsupportedSourceAction` before request for unsupported input;
   - `CommandOnlySourceAction` when all matching allowlisted actions require command execution;
   - `NoEditForSourceAction` when matching actions exist but have neither edit nor command, or no matching action exists;
   - `AmbiguousSourceAction` when multiple edit-bearing matches exist.
4. Continue rejecting raw `Command` variants.

Suggested internal counters:

```rust
let mut matching_command_only = 0usize;
let mut matching_no_edit_no_command = 0usize;
let mut matching_non_edit_total = 0usize;
let mut edit_bearing = Vec::new();
```

Acceptance criteria:

- raw `Command` variants are rejected;
- matching `CodeAction` with command-only shape yields `CommandOnlySourceAction`;
- matching `CodeAction` with neither edit nor command yields `NoEditForSourceAction`;
- nonmatching actions do not trigger command-only errors.

## Phase 3 — Add Hermetic Operation Helper Tests

Add tests near `SourceActionPreviewKind` / `select_source_action_edit`, ideally in `crates/egglsp/src/operations.rs` under `#[cfg(test)]`.

Required parsing tests:

```text
source_action_kind_accepts_source_organize_imports
source_action_kind_accepts_camel_alias
source_action_kind_accepts_snake_alias
source_action_kind_rejects_fix_all
source_action_kind_rejects_quickfix
source_action_kind_rejects_unknown
```

Required selection tests:

```text
select_source_actions_rejects_raw_command_only
select_source_actions_rejects_code_action_command_only
select_source_actions_rejects_no_edit
select_source_actions_selects_single_edit_bearing_organize_imports
select_source_actions_rejects_ambiguous_multiple_edits
select_source_actions_ignores_nonmatching_actions
select_source_actions_accepts_child_kind_if_policy_keeps_child_kind_support
```

Keep these tests pure. Do not start a language server.

Construct minimal `WorkspaceEdit` values such as:

```rust
WorkspaceEdit {
    changes: Some(HashMap::new()),
    document_changes: None,
    change_annotations: None,
}
```

or a minimal single-file edit if the type requires it.

Acceptance criteria:

- `cargo test -p egglsp` proves allowlist and selection policy without external servers;
- command execution remains impossible through this path.

## Phase 4 — Add Model-Facing Wrapper Tests

Update `tests/lsp.rs` and/or `src/tool/lsp.rs` tests.

Required tests:

```text
lsp_schema_includes_sourceActionPreview
sourceActionPreview_requires_file_path
sourceActionPreview_requires_action
sourceActionPreview_rejects_unsupported_action_without_lsp_request_if_testable
lsp_tool_remains_read_only
codeLens_still_not_exposed
```

Notes:

- Unsupported action should fail before any LSP request because `SourceActionPreviewKind::parse` happens before `ops.source_action_preview`.
- A valid action with a fake/missing file may still fail at file resolution; that is fine.
- Existing schema snapshot should include the `action` property and `sourceActionPreview` enum value.

Acceptance criteria:

- wrapper-level validation catches missing action;
- unsupported action returns a useful error without requiring a real server;
- read-only category is preserved.

## Phase 5 — Tighten Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
```

Required doc corrections:

1. State that `sourceActionPreview` currently supports only `source.organizeImports`.
2. State that aliases may be accepted by the wrapper if implemented.
3. State that command-only source actions are rejected because command execution is disabled.
4. State that arbitrary code actions are internal-only and not model-facing.
5. Ensure `code_lens`, `completion`, and generic `code_actions` are described as internal/non-model-facing where mentioned.
6. Preserve the mutation boundary:

```text
LSP preview operations never write files. Applying any preview requires apply_patch or another mutating tool.
```

Acceptance criteria:

- docs do not imply arbitrary source actions are supported;
- docs do not imply command execution is allowed;
- docs clearly preserve the read-only preview boundary.

## Phase 6 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted checks:

```bash
cargo test -p egglsp source_action
cargo test -p egglsp document_end_position
cargo test --test lsp sourceActionPreview
rg "u32::MAX" crates/egglsp/src/operations.rs
rg "sourceActionPreview|source.organizeImports|SourceActionPreviewKind|CommandOnlySourceAction" crates/egglsp src tests architecture
rg "source.fixAll|quickfix|refactor" crates/egglsp/src/operations.rs src/tool/lsp.rs architecture/lsp.md tests/lsp.rs
```

Optional real-server smoke test:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp source_action_preview_real_lsp -- --nocapture
```

Manual smoke:

```text
1. Use a file with disordered imports.
2. Run lsp sourceActionPreview with action=source.organizeImports.
3. Confirm a WorkspaceEditPreview is returned.
4. Confirm patch_omitted=false for small patches.
5. Confirm the file is unchanged.
6. Apply the patch through apply_patch.
7. Confirm the file changes only after apply_patch.
```

## Done Criteria

This cleanup pass is complete when:

- `source_action_preview` computes a real UTF-16 full-document range;
- `u32::MAX` is gone from the code-action range;
- action parsing and selection policy are covered by hermetic tests;
- command-only `CodeAction` values are classified as command-disabled;
- wrapper tests cover missing/unsupported `action`;
- docs clearly state only `source.organizeImports` is supported;
- `lsp` remains read-only and mutation remains in `apply_patch`.

## Next Pass After Cleanup

After this cleanup lands, move to overlay-backed semantic checks for proposed patches.

That pass should introduce a document overlay/store layer so Codegg can ask LSP for diagnostics, symbols, and possibly format/organize-import previews against proposed content before writing it to disk.
