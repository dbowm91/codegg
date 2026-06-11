# LSP Semantic Check Patch Input Plan

## Purpose

Make `semanticCheckPreview` directly useful with existing patch-producing workflows.

The overlay-backed semantic check path now supports full proposed file content:

```json
{
  "operation": "semanticCheckPreview",
  "file_path": "src/main.rs",
  "content": "full proposed file content"
}
```

This pass should add a second input mode:

```json
{
  "operation": "semanticCheckPreview",
  "file_path": "src/main.rs",
  "patch": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ ..."
}
```

The wrapper should read the current file, apply the unified diff in memory, then call the existing overlay-backed `semantic_check_preview` with the resulting full proposed content. The file on disk must remain unchanged.

This makes the flow natural for LSP edit previews:

```text
renamePreview / formatPreview / sourceActionPreview
  -> WorkspaceEditPreview.patch
  -> semanticCheckPreview(patch=...)
  -> apply_patch only if semantic check is acceptable
```

## Scope

Implement patch input at the Codegg tool-wrapper layer first.

Do not move `patch_util` into `egglsp` unless the dependency boundary becomes cleaner later. `egglsp` should remain unaware of Codegg's patch parser for this pass.

## Non-Goals

Do not write proposed content to disk.

Do not add multi-file patch support in this pass.

Do not add persistent overlays.

Do not execute LSP commands.

Do not change the existing `content` input behavior.

Do not expose completions or generic code actions.

Do not redesign `apply_patch`.

## Current State Summary

Relevant files:

```text
src/tool/lsp.rs
src/tool/patch_util.rs
src/tool/apply_patch.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/overlay.rs
tests/lsp.rs
architecture/lsp.md
architecture/tool.md
```

Current useful foundation:

- `semanticCheckPreview` already accepts full proposed content through `content`.
- `src/tool/patch_util.rs` exposes `apply_unified_diff(original, patch)`.
- `apply_patch` already uses the shared patch utility.
- LSP edit previews emit unified diff patches.
- `semantic_check_preview` enforces root boundaries and uses temporary LSP overlays.
- semantic-check output has `diagnostics_error`, `symbols_error`, and `restore_error` fields.

## Phase 0 — Close Small Wrapper Test Gap

Before adding patch input, add the missing wrapper tests from the previous pass.

Required tests in `tests/lsp.rs` or `src/tool/lsp.rs` tests:

```text
semanticCheckPreview_requires_file_path
semanticCheckPreview_requires_content_or_patch
semanticCheckPreview_is_read_only
```

Expected behavior before patch support:

- missing `file_path` should fail with `file_path required`;
- missing both `content` and `patch` should fail with a message naming both accepted inputs;
- tool category remains `ToolCategory::ReadOnly`.

After patch support, the missing-content test must be updated to require either `content` or `patch`.

Acceptance criteria:

- missing input validation is covered before/alongside patch implementation;
- read-only boundary is explicitly tested.

## Phase 1 — Extend Tool Input Schema

Update `src/tool/lsp.rs`.

Add to `LspInput`:

```rust
#[serde(default)]
patch: Option<String>,
```

Update JSON schema property:

```json
"patch": {
  "type": "string",
  "description": "Unified diff patch to apply in memory for semanticCheckPreview. Mutually exclusive with content."
}
```

Update `content` description:

```text
Proposed full file content for semanticCheckPreview. Mutually exclusive with patch.
```

Update operation description to mention:

```text
semanticCheckPreview accepts either full proposed content or a single-file unified diff patch.
```

Acceptance criteria:

- schema advertises `patch`;
- docs/descriptions state content and patch are mutually exclusive;
- `lsp` remains read-only.

## Phase 2 — Add Input Resolution Helper

Add a small helper in `src/tool/lsp.rs`:

```rust
fn resolve_semantic_check_content(
    &self,
    file: &Path,
    content: Option<&String>,
    patch: Option<&String>,
) -> Result<String, ToolError>
```

Behavior:

1. If both `content` and `patch` are `Some`, return an error:

```text
semanticCheckPreview accepts either content or patch, not both
```

2. If both are `None`, return an error:

```text
content or patch required for semanticCheckPreview
```

3. If `content` is `Some`, return it unchanged.

4. If `patch` is `Some`:
   - read current disk content from `file`;
   - call `crate::tool::patch_util::apply_unified_diff(&original, patch)`;
   - map patch errors to `ToolError::Execution("semanticCheckPreview patch failed: ...")`;
   - return the proposed content.

Important: this helper must not write to disk.

Acceptance criteria:

- patch application is in-memory only;
- content input behavior remains unchanged;
- invalid patch errors are clear;
- both-input ambiguity is rejected.

## Phase 3 — Wire Patch Input into `semanticCheckPreview`

Update the existing branch:

```rust
"semanticCheckPreview" => {
    let file = self.resolve_file(&parsed.file_path)?;
    let proposed = self.resolve_semantic_check_content(
        &file,
        parsed.content.as_ref(),
        parsed.patch.as_ref(),
    )?;
    let preview = ops
        .semantic_check_preview(&file, proposed, Some(&self.allowed_root))
        .await?;
    ...
}
```

If `semantic_check_preview` currently takes `String`, pass `proposed`. If it takes `&str`, pass `&proposed` and keep the owned string alive through the await.

Acceptance criteria:

- content path still works;
- patch path produces the same output shape;
- no disk write occurs before calling LSP overlay;
- root validation still happens before reading the file.

## Phase 4 — Single-File Patch Policy

This pass should support single-file patches only.

The current patch parser likely applies a patch to one original string without independently validating header paths. That is acceptable if the wrapper semantics are clear: `file_path` determines the file whose current content is patched.

Add guardrails if easy:

- If patch appears to contain multiple `diff --git` sections, reject it.
- If patch contains multiple `---` / `+++` file headers for different paths, reject it.
- Do not attempt to resolve patch header paths to additional files.

Suggested helper:

```rust
fn reject_probable_multi_file_patch(patch: &str) -> Result<(), ToolError>
```

Simple policy:

```text
allow at most one `diff --git` line
allow at most one `--- ` header and one `+++ ` header
```

Do not overbuild full patch parsing here.

Acceptance criteria:

- multi-file patches are rejected or clearly unsupported;
- patch input cannot silently check only one file from a multi-file diff.

## Phase 5 — Tests

Add wrapper-level tests. These should not require a real LSP server except where noted.

Pure helper tests:

```text
semantic_check_content_accepts_content
semantic_check_content_rejects_content_and_patch
semantic_check_content_rejects_missing_content_and_patch
semantic_check_patch_applies_in_memory
semantic_check_patch_rejects_invalid_patch
semantic_check_patch_rejects_probable_multi_file_patch
semantic_check_patch_does_not_write_disk
```

Schema tests:

```text
lsp_schema_includes_semanticCheckPreview
lsp_schema_includes_patch_property
semanticCheckPreview_is_read_only
```

Execute-branch validation tests:

```text
semanticCheckPreview_requires_file_path
semanticCheckPreview_requires_content_or_patch
semanticCheckPreview_rejects_content_and_patch
```

Patch parser compatibility test:

Use a simple original file:

```rust
fn main() {
    println!("old");
}
```

Patch:

```diff
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!("old");
+    println!("new");
 }
```

Assert returned proposed content contains `"new"` and the actual temp file still contains `"old"`.

Optional integration test with real LSP:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test --test lsp semanticCheckPreview_patch_real_lsp -- --nocapture
```

It should skip if no suitable server is available.

Acceptance criteria:

- all default tests are hermetic;
- invalid patch failures are covered;
- disk non-mutation is covered;
- patch input and content input are mutually exclusive.

## Phase 6 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
```

Document:

- `semanticCheckPreview` accepts either `content` or `patch`;
- `patch` is a single-file unified diff applied in memory to `file_path`;
- multi-file patches are not supported in this pass;
- proposed content is never written to disk;
- LSP overlay view is restored after the check;
- restore/diagnostics/symbol errors are surfaced in output;
- mutation still requires `apply_patch`.

Recommended wording:

```markdown
`semanticCheckPreview` can validate either full proposed file content or a single-file unified diff patch. For patch input, Codegg reads the current `file_path`, applies the patch in memory, sends the resulting content to the LSP overlay, then restores the LSP view. It does not write the patched content to disk.
```

## Phase 7 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test --test lsp semanticCheckPreview
cargo test --test lsp semantic_check_patch
cargo test -p codegg patch_util
rg "semanticCheckPreview|semantic_check_preview|patch" src/tool/lsp.rs tests/lsp.rs architecture/lsp.md architecture/tool.md
rg "std::fs::write|tokio::fs::write" src/tool/lsp.rs crates/egglsp/src/operations.rs crates/egglsp/src/overlay.rs
```

Manual smoke:

```text
1. Create a small patch that introduces a type error.
2. Run lsp semanticCheckPreview with file_path + patch.
3. Confirm output has diagnostics or diagnostics_may_still_be_warming=true.
4. Confirm the file on disk remains unchanged.
5. Apply the same patch with apply_patch only after accepting the semantic result.
```

## Done Criteria

This pass is complete when:

- `semanticCheckPreview` accepts exactly one of `content` or `patch`;
- patch input is applied in memory using shared patch parsing;
- invalid/multi-file patches are rejected clearly;
- disk content is never modified by semantic checking;
- output shape remains unchanged apart from accepting patch input;
- wrapper validation tests cover missing/both/invalid inputs;
- docs explain patch-input semantics and limitations.

## Next Pass After This

After patch input lands, move to semantic context packets.

That pass should package diagnostics, symbols, nearby definitions/references, and optional source-action previews into compact context bundles for edit/review/security workflows.
