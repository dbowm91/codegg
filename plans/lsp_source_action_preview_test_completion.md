# LSP Source Action Preview Test Completion Plan

## Purpose

Close the remaining test gap in the `sourceActionPreview` feature before moving on to overlay-backed semantic checks.

The implementation now has the important production behavior:

- `source_action_preview` computes a real UTF-16 full-document range instead of using `u32::MAX`.
- `SourceActionPreviewKind` only allows `source.organizeImports` plus aliases.
- `select_source_action_edit` rejects command-only and no-edit actions.
- `lsp` exposes `sourceActionPreview` as a read-only preview operation.

The missing piece is test coverage. This pass should add hermetic unit tests and wrapper validation tests only. Avoid feature work.

## Non-Goals

Do not change the `sourceActionPreview` public API unless tests expose a bug.

Do not add new source actions.

Do not add `source.fixAll`, `quickfix`, or `refactor`.

Do not execute LSP commands.

Do not launch a real language server in default tests.

Do not implement overlay sync.

## Phase 1 — Add Pure Tests for `SourceActionPreviewKind::parse`

Add tests in `crates/egglsp/src/operations.rs` under `#[cfg(test)]`.

Required tests:

```text
source_action_kind_accepts_source_organize_imports
source_action_kind_accepts_camel_alias
source_action_kind_accepts_snake_alias
source_action_kind_rejects_fix_all
source_action_kind_rejects_quickfix
source_action_kind_rejects_unknown
```

Expected assertions:

```rust
assert_eq!(
    SourceActionPreviewKind::parse("source.organizeImports").unwrap(),
    SourceActionPreviewKind::OrganizeImports,
);

assert!(matches!(
    SourceActionPreviewKind::parse("source.fixAll"),
    Err(LspError::UnsupportedSourceAction(_)),
));
```

Acceptance criteria:

- allowlist behavior is proven without a server;
- unsupported actions fail before any LSP request path.

## Phase 2 — Add Pure Tests for `document_end_position_utf16`

Add tests next to the parse tests.

Required tests:

```text
document_end_position_empty_file
document_end_position_ascii_single_line
document_end_position_trailing_newline
document_end_position_multiline_no_trailing_newline
document_end_position_unicode_utf16_units
document_end_position_crlf_reasonable_behavior
```

Expected examples:

```rust
assert_eq!(document_end_position_utf16(""), Position { line: 0, character: 0 });
assert_eq!(document_end_position_utf16("abc"), Position { line: 0, character: 3 });
assert_eq!(document_end_position_utf16("abc\n"), Position { line: 1, character: 0 });
assert_eq!(document_end_position_utf16("a\nβ😀"), Position { line: 1, character: 3 });
```

For CRLF, choose and document the behavior. Current helper counts `\r` as a character before `\n`. That is acceptable if intentional, but a test should lock it down or the helper should normalize CRLF if preferred.

Preferred CRLF behavior:

```text
"a\r\n" -> line 1, character 0
```

If the helper currently returns that because `\r` is reset on newline, test it explicitly.

Acceptance criteria:

- UTF-16 behavior is documented by tests;
- no future regression silently reintroduces byte-counting.

## Phase 3 — Add Pure Tests for `select_source_action_edit`

Add helper constructors in the test module to keep tests readable:

```rust
fn empty_workspace_edit() -> WorkspaceEdit;
fn code_action_with_edit(title: &str, kind: CodeActionKind) -> CodeActionOrCommand;
fn code_action_command_only(title: &str, kind: CodeActionKind) -> CodeActionOrCommand;
fn code_action_no_edit_no_command(title: &str, kind: CodeActionKind) -> CodeActionOrCommand;
fn raw_command() -> CodeActionOrCommand;
```

Required tests:

```text
select_source_actions_rejects_raw_command_only
select_source_actions_rejects_code_action_command_only
select_source_actions_rejects_no_edit
select_source_actions_selects_single_edit_bearing_organize_imports
select_source_actions_rejects_ambiguous_multiple_edits
select_source_actions_ignores_nonmatching_actions
select_source_actions_accepts_child_kind_if_policy_keeps_child_kind_support
```

Expected assertions:

```rust
assert!(matches!(
    select_source_action_edit(SourceActionPreviewKind::OrganizeImports, vec![raw_command()]),
    Err(LspError::CommandOnlySourceAction(_)),
));

assert!(matches!(
    select_source_action_edit(
        SourceActionPreviewKind::OrganizeImports,
        vec![code_action_command_only("Organize Imports", CodeActionKind::SOURCE_ORGANIZE_IMPORTS)]
    ),
    Err(LspError::CommandOnlySourceAction(_)),
));

let edit = select_source_action_edit(
    SourceActionPreviewKind::OrganizeImports,
    vec![code_action_with_edit("Organize Imports", CodeActionKind::SOURCE_ORGANIZE_IMPORTS)]
).unwrap();
```

For child-kind support, current implementation accepts kinds prefixed by `source.organizeImports.`. Add a test for that behavior or remove child-kind support if it is not desired. Prefer keeping and testing it.

Acceptance criteria:

- command execution rejection is proven;
- ambiguous multiple edit-bearing actions are rejected;
- nonmatching actions do not produce false positives;
- valid edit-bearing organize-import action returns a `WorkspaceEdit`.

## Phase 4 — Add Wrapper Validation Tests

Update `tests/lsp.rs` or `src/tool/lsp.rs` tests.

Required tests:

```text
sourceActionPreview_requires_file_path
sourceActionPreview_requires_action
sourceActionPreview_rejects_unsupported_action_without_lsp_request
```

Suggested assertions:

```rust
let err = tool.execute(json!({
    "operation": "sourceActionPreview",
    "action": "source.organizeImports"
})).await.unwrap_err();
assert!(matches!(err, ToolError::Execution(ref msg) if msg.contains("file_path")));
```

For unsupported action, provide a real temporary file so path resolution succeeds, then pass an unsupported action:

```rust
let (_dir, path) = temp_rs_file("fn main() {}\n");
let tool = make_tool_with_root(_dir.path());
let err = tool.execute(json!({
    "operation": "sourceActionPreview",
    "file_path": path.to_string_lossy(),
    "action": "source.fixAll"
})).await.unwrap_err();
assert!(matches!(err, ToolError::Execution(ref msg) if msg.contains("unsupported source action")));
```

This should fail before any LSP request because parsing happens before `ops.source_action_preview`.

Acceptance criteria:

- missing file path is caught;
- missing action is caught;
- unsupported action is rejected without needing a language server;
- read-only category remains covered by existing tests.

## Phase 5 — Optional Small Doc Tightening

Only update docs if the current docs are still ambiguous.

Check:

```text
architecture/lsp.md
architecture/tool.md
```

Ensure they say:

- `sourceActionPreview` currently supports only `source.organizeImports`;
- command-only actions are rejected;
- generic `code_actions`, `completion`, and `code_lens` remain internal/non-model-facing;
- mutation still requires `apply_patch`.

This phase is optional if docs already clearly say this.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted commands:

```bash
cargo test -p egglsp source_action
cargo test -p egglsp document_end_position
cargo test --test lsp sourceActionPreview
rg "source_action_kind_accepts|select_source_actions|document_end_position" crates/egglsp/src/operations.rs
rg "sourceActionPreview_requires_action|unsupported source action" tests/lsp.rs src/tool/lsp.rs
```

## Done Criteria

This pass is complete when:

- parse allowlist tests exist;
- document-end UTF-16 tests exist;
- source action selection tests exist;
- wrapper validation tests exist for missing/unsupported `action`;
- all tests are hermetic and require no external LSP server;
- no feature expansion or command execution path is introduced.

## Next Pass After This

After this test-completion pass lands, proceed to overlay-backed semantic checks.

That next pass should introduce a document overlay/store layer so Codegg can ask LSP for diagnostics and symbols against proposed patch content before writing it to disk.
