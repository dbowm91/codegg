# LSP Workspace Edit Preview Feature Plan

## Purpose

Add the first post-hardening LSP feature layer: preview-only LSP edits.

This pass should make Codegg able to ask language servers for semantic edits such as rename and formatting, convert the returned LSP `WorkspaceEdit` / `TextEdit` data into bounded unified-diff previews, and return those previews through the existing read-only `lsp` tool. Actual file mutation must continue to go through Codegg's existing mutating tools, especially `apply_patch`.

The goal is not to make LSP edit files. The goal is to let LSP produce safe, reviewable patches.

## Current State Summary

Relevant files:

```text
crates/egglsp/src/lib.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/service.rs
crates/egglsp/src/client.rs
src/tool/lsp.rs
src/tool/apply_patch.rs
src/tool/mod.rs
architecture/lsp.md
tests/lsp.rs
```

Current useful foundation:

- `egglsp` owns the native LSP implementation.
- `LspClient` has a background stdout dispatcher and pending request map.
- diagnostics are notification-driven and synchronized from disk before cache reads.
- `src/tool/lsp.rs` is read-only and returns compact DTOs.
- `apply_patch` is already the mutating unified-diff application path.
- `ToolCategory::ReadOnly` vs `ToolCategory::Mutating` already gives the correct permission boundary.
- `LspOperations` already has internal request helpers for several LSP operations.

Current limitation:

- LSP cannot yet expose rename, formatting, or source actions safely because there is no shared conversion layer from `WorkspaceEdit` / `TextEdit` into a Codegg-native patch preview.

## Design Rule

LSP edit features are preview-only.

The flow should be:

```text
model asks lsp renamePreview / formatPreview
  -> egglsp requests semantic edit from language server
  -> egglsp converts WorkspaceEdit/TextEdit into unified diff preview(s)
  -> lsp tool returns JSON preview with patch text
  -> model/user reviews
  -> actual mutation, if desired, uses apply_patch or another existing mutating tool
```

Do not write files from `egglsp` edit-preview operations.

Do not bypass permission handling by applying LSP edits inside the read-only `lsp` tool.

## Non-Goals

Do not implement overlay sync in this pass.

Do not implement call hierarchy in this pass.

Do not expose arbitrary code actions in this pass.

Do not expose completion.

Do not apply rename or format edits automatically.

Do not redesign the tool registry.

Do not change `apply_patch` semantics unless a small helper extraction is clearly necessary.

## Phase 1 — Add `egglsp::edit` Preview Module

Create:

```text
crates/egglsp/src/edit.rs
```

Add public DTOs:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkspaceEditPreview {
    pub title: String,
    pub files: Vec<FileEditPreview>,
    pub total_files: usize,
    pub total_edits: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEditPreview {
    pub file: PathBuf,
    pub original_hash: String,
    pub edits: Vec<TextEditPreview>,
    pub patch: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TextEditPreview {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub replacement_preview: String,
}
```

Add conversion functions:

```rust
pub fn preview_workspace_edit(
    title: impl Into<String>,
    edit: lsp_types::WorkspaceEdit,
    allowed_root: Option<&std::path::Path>,
) -> Result<WorkspaceEditPreview, LspError>;

pub fn preview_text_edits_for_file(
    title: impl Into<String>,
    file_path: &std::path::Path,
    edits: Vec<lsp_types::TextEdit>,
) -> Result<WorkspaceEditPreview, LspError>;
```

Implementation requirements:

1. Support `WorkspaceEdit.changes`.
2. Support `WorkspaceEdit.document_changes` for `TextDocumentEdit`.
3. Ignore or reject resource operations (`CreateFile`, `RenameFile`, `DeleteFile`) for this pass with a clear error.
4. Decode file URIs with `Url::to_file_path()`.
5. Validate decoded paths against `allowed_root` when supplied.
6. Read original file text from disk.
7. Apply LSP text edits to an in-memory string.
8. Generate a real unified diff patch for each touched file.
9. Include an `original_hash` so later tooling can detect stale previews.
10. Cap replacement preview strings and total file/edit counts.

Preferred caps:

```rust
const MAX_EDIT_PREVIEW_FILES: usize = 20;
const MAX_EDIT_PREVIEW_EDITS: usize = 1000;
const MAX_REPLACEMENT_PREVIEW_CHARS: usize = 500;
const MAX_PATCH_CHARS_PER_FILE: usize = 50_000;
```

Use `sha2` if already available. If not, use a stable simple hash helper already present in the repo, or add `sha2` to `crates/egglsp/Cargo.toml` only if acceptable. Avoid cryptographic overreach; this is a stale-preview guard, not a security boundary.

## Phase 2 — Implement TextEdit Application Correctly

LSP ranges are UTF-16 positions. Rust strings are UTF-8. Do not naïvely index bytes by `character`.

Add helper functions in `edit.rs`:

```rust
fn line_start_offsets(text: &str) -> Vec<usize>;
fn utf16_position_to_byte_offset(text: &str, line: u32, character: u32) -> Result<usize, LspError>;
fn apply_text_edits(text: &str, edits: &[lsp_types::TextEdit]) -> Result<String, LspError>;
```

Implementation requirements:

1. Convert LSP line + UTF-16 character offsets to byte offsets.
2. Reject out-of-bounds ranges with clear errors.
3. Reject overlapping edits.
4. Apply edits in reverse byte-offset order.
5. Preserve final newline behavior as faithfully as possible.
6. Support multi-line edits.
7. Support unicode text, including non-ASCII and multi-byte characters.

Tests required:

```text
apply_single_line_edit
apply_multiline_edit
apply_insert_at_start
apply_insert_at_end
apply_unicode_utf16_position
reject_out_of_bounds_edit
reject_overlapping_edits
apply_multiple_edits_reverse_order
```

## Phase 3 — Generate Unified Diffs

Do not reuse `src/tool/apply_patch.rs::generate_diff_preview` directly; it is private and emits a readable preview, not necessarily the patch format the model can pass to `apply_patch`.

Add a small unified-diff generator in `egglsp::edit`, using `similar` if available from workspace dependencies. The patch should be compatible with the existing `apply_patch` parser, which expects hunks and accepts `---` / `+++` lines.

Patch shape:

```diff
--- a/path/to/file.rs
+++ b/path/to/file.rs
@@ -10,7 +10,7 @@
 old line
-new symbol
+new symbol
 next line
```

Implementation requirements:

1. Generate hunks with enough context for `apply_patch` to verify safely.
2. Use repo-relative paths when possible; otherwise use the supplied path string.
3. Do not include absolute paths in patches if a relative path can be computed from `allowed_root` or current dir.
4. If generated patch exceeds cap, truncate display fields but do not emit a partially-applicable patch as though it is complete. Either set `truncated = true` and omit the patch or return an error instructing the caller to narrow the operation.

Tests required:

```text
unified_diff_contains_hunk
unified_diff_can_be_applied_by_apply_patch_parser_if_helper_is_public_or_tested_indirectly
large_patch_truncates_or_errors_without_partial_patch
```

If `apply_patch` internals are private and not easily testable cross-module, keep tests local to `egglsp` for patch shape and add an integration-style test later.

## Phase 4 — Add Rename Preview Operation

In `crates/egglsp/src/operations.rs`, add:

```rust
pub async fn prepare_rename(
    &self,
    file_path: &Path,
    line: u32,
    column: u32,
) -> Result<Option<PrepareRenameResponse>, LspError>;

pub async fn rename_preview(
    &self,
    file_path: &Path,
    line: u32,
    column: u32,
    new_name: &str,
    allowed_root: Option<&Path>,
) -> Result<WorkspaceEditPreview, LspError>;
```

Implementation requirements:

1. Resolve/get the client by file path.
2. Ensure the file is open/synced from disk before rename. Reuse `ensure_file_open_from_disk` if appropriate.
3. Optionally call `textDocument/prepareRename` first. If unsupported, continue to `textDocument/rename` only if the server indicates rename capability or if unsupported detection is clean.
4. Send `textDocument/rename` with `RenameParams`.
5. Convert returned `WorkspaceEdit` through `edit::preview_workspace_edit`.
6. Return a clear error if the server returns `null` or no edits.
7. Do not apply edits.

Model-facing operation in `src/tool/lsp.rs`:

```json
{
  "operation": "renamePreview",
  "file_path": "src/foo.rs",
  "line": 42,
  "column": 13,
  "new_name": "new_symbol_name"
}
```

Add `new_name: Option<String>` to `LspInput`.

Output should be the serialized `WorkspaceEditPreview` inside the existing `LspToolOutput` shape or a close equivalent:

```json
{
  "operation": "renamePreview",
  "file_path": "src/foo.rs",
  "result_count": 2,
  "truncated": false,
  "results": {
    "title": "rename symbol",
    "files": [ ... ],
    "total_files": 2,
    "total_edits": 5,
    "truncated": false
  }
}
```

Tests required:

```text
lsp_schema_includes_renamePreview
renamePreview_requires_file_path_line_column_new_name
renamePreview_is_read_only_tool_category
workspace_edit_preview_serializes_expected_shape
```

Do not require a real language server in default tests. Unit-test conversion helpers separately; operation integration can be optional.

## Phase 5 — Add Format Preview Operation

In `crates/egglsp/src/operations.rs`, add:

```rust
pub async fn format_preview(
    &self,
    file_path: &Path,
    allowed_root: Option<&Path>,
) -> Result<WorkspaceEditPreview, LspError>;
```

Implementation requirements:

1. Resolve/get the client by file path.
2. Ensure the file is open/synced from disk.
3. Send `textDocument/formatting` with conservative defaults:

```rust
FormattingOptions {
    tab_size: 4,
    insert_spaces: true,
    properties: Default::default(),
    trim_trailing_whitespace: Some(true),
    insert_final_newline: Some(true),
    trim_final_newlines: Some(true),
}
```

Use server/language config later if needed; hardcoded defaults are acceptable for this first pass.

4. Convert returned `Vec<TextEdit>` through `preview_text_edits_for_file`.
5. If no edits are returned, return a preview with no files/edits and a clear `(no changes)` result rather than an error.
6. Do not apply edits.

Model-facing operation:

```json
{
  "operation": "formatPreview",
  "file_path": "src/foo.rs"
}
```

Tests required:

```text
lsp_schema_includes_formatPreview
formatPreview_requires_file_path
format_preview_empty_edits_is_no_changes
format_preview_output_contains_patch_when_edits_exist
```

## Phase 6 — Keep Source Actions for a Follow-Up

Do not implement `sourceActionPreview` in this first pass unless `WorkspaceEditPreview` and both rename/format previews are already complete and clean.

If the implementer has extra time, add only a stub plan comment, not a half-implemented public operation.

Reason: arbitrary code actions can return commands without edits. This needs a separate allowlist and command-rejection policy.

## Phase 7 — Update `src/tool/lsp.rs`

Changes:

1. Add `new_name` to `LspInput`.
2. Add `renamePreview` and `formatPreview` to the schema enum.
3. Update description to mention preview-only edits.
4. Add match branches for `renamePreview` and `formatPreview`.
5. Pass `Some(&self.allowed_root)` to `egglsp` preview conversion.
6. Keep `category()` as `ToolCategory::ReadOnly`.
7. Keep provenance as native / `egglsp`.
8. Ensure outputs are capped and serialized compactly.

Important: Do not let these operations call `apply_patch` internally.

## Phase 8 — Update Exports and Docs

Update:

```text
crates/egglsp/src/lib.rs
architecture/lsp.md
architecture/tool.md
```

`crates/egglsp/src/lib.rs` should export the new edit preview module/types.

`architecture/lsp.md` is currently stale. Fix at least:

- remove old `notif_tx` / `notif_rx` client fields;
- describe the background stdout reader and pending request map;
- remove model-facing code lens language;
- replace `get_or_create_client_for_root_hint` with `find_existing_client_for_root_hint` if documented;
- add preview-only `renamePreview` and `formatPreview`;
- state that LSP edit features never write files and produce patches for `apply_patch`.

`architecture/tool.md` should update the LSP tool row to include the new preview-only operations and reiterate that mutation stays in `apply_patch`.

## Phase 9 — Tests

Required default tests must be hermetic.

Add tests in `crates/egglsp` for:

```text
TextEdit application:
  apply_single_line_edit
  apply_multiline_edit
  apply_insert_at_start
  apply_insert_at_end
  apply_unicode_utf16_position
  reject_out_of_bounds_edit
  reject_overlapping_edits
  apply_multiple_edits_reverse_order

WorkspaceEdit conversion:
  workspace_edit_changes_to_preview
  workspace_edit_document_changes_to_preview
  workspace_edit_rejects_resource_operations
  workspace_edit_rejects_outside_allowed_root
  workspace_edit_multi_file_preview

Patch generation:
  patch_contains_file_headers
  patch_contains_hunk
  patch_omitted_or_errors_when_over_cap
```

Add tests in `tests/lsp.rs` or `src/tool/lsp.rs` tests for:

```text
lsp_schema_includes_renamePreview_and_formatPreview
renamePreview_requires_new_name
renamePreview_requires_file_path_line_column
formatPreview_requires_file_path
lsp_tool_remains_read_only
codeLens_still_not_exposed
```

Optional integration test:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp rename_preview_real_lsp -- --nocapture
```

This may require `rust-analyzer`; it must skip cleanly if unavailable.

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
rg "renamePreview|formatPreview|WorkspaceEditPreview|TextEditPreview" crates/egglsp src tests architecture
rg "codeLens" src/tool/lsp.rs tests/lsp.rs architecture/lsp.md
```

Manual behavior check with a real Rust project and rust-analyzer available:

```text
1. call lsp diagnostics on a Rust file to warm/open it
2. call lsp renamePreview at a known symbol with new_name
3. inspect returned patch only; ensure the file is unchanged
4. apply patch manually through apply_patch if desired
5. call lsp diagnostics again
```

## Done Criteria

This pass is complete when:

- `egglsp::edit` can convert LSP edits into bounded unified-diff previews.
- `renamePreview` is model-facing and preview-only.
- `formatPreview` is model-facing and preview-only.
- actual mutation still requires `apply_patch` or another existing mutating tool.
- `lsp` remains `ToolCategory::ReadOnly`.
- code lens remains hidden from the model-facing LSP schema.
- default tests do not require external LSP servers.
- stale `architecture/lsp.md` content is corrected.

## Suggested Follow-Up Passes

After this pass:

1. Add allowlisted `sourceActionPreview` for `source.organizeImports` only.
2. Add document overlay sync for proposed patches before file commit.
3. Add semantic context packets for edit/review/security/explain intents.
4. Add real call hierarchy using prepare/incoming/outgoing flow.
