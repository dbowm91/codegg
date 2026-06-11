# LSP Workspace Edit Preview Cleanup Plan

## Purpose

Tighten the first LSP edit-preview feature pass before moving on to `sourceActionPreview`, overlay sync, call hierarchy, or semantic context packets.

The previous pass successfully added:

- `crates/egglsp/src/edit.rs`;
- `WorkspaceEditPreview`, `FileEditPreview`, and `TextEditPreview`;
- UTF-16-aware `TextEdit` application;
- unified-diff preview generation;
- `renamePreview`;
- `formatPreview`;
- model-facing schema updates;
- basic tests.

This cleanup pass should address the remaining correctness and documentation gaps:

1. propagate `allowed_root` consistently through format-preview conversion;
2. re-export preview DTOs from `egglsp`;
3. finish stale LSP/tool docs cleanup;
4. add a compatibility test proving generated patches can be consumed by Codegg's `apply_patch` logic or a shared equivalent;
5. make patch truncation semantics safer and less ambiguous.

## Non-Goals

Do not add `sourceActionPreview` in this pass.

Do not implement overlay sync.

Do not implement call hierarchy.

Do not expose completion or code lens model-facing.

Do not make LSP apply edits directly.

Do not redesign `apply_patch`.

## Phase 1 — Propagate `allowed_root` Through Format Preview

Current issue:

- `LspOperations::format_preview` accepts `allowed_root: Option<&Path>` but ignores it.
- `preview_text_edits_for_file` does not accept `allowed_root`, so it validates with `None` and generates relative paths without the same root context.
- The model-facing wrapper resolves the file path before calling this, so ordinary tool use is probably safe, but the crate API should enforce the same boundary internally.

Required changes:

1. Change the signature in `crates/egglsp/src/edit.rs`:

```rust
pub fn preview_text_edits_for_file(
    title: impl Into<String>,
    file_path: &Path,
    edits: Vec<TextEdit>,
    allowed_root: Option<&Path>,
) -> Result<WorkspaceEditPreview, LspError>
```

2. Replace internal `validate_path_against_root(file_path, None)` with `validate_path_against_root(file_path, allowed_root)`.
3. Pass `allowed_root` to `build_file_preview`.
4. Update all call sites.
5. In `LspOperations::format_preview`, pass the supplied `allowed_root` into `preview_text_edits_for_file`.
6. Update tests for the new signature.

Acceptance criteria:

- `format_preview` enforces `allowed_root` at the crate layer.
- generated format patches use paths relative to `allowed_root` when possible.
- attempts to preview format edits for a path outside `allowed_root` fail with `LspError::PathOutsideRoot`.

## Phase 2 — Re-Export Preview DTOs

Current issue:

- `crates/egglsp/src/lib.rs` declares `pub mod edit`, but does not re-export the preview DTOs.
- Downstream code can still use `egglsp::edit::WorkspaceEditPreview`, but the public crate API should expose the key feature types directly.

Required change:

```rust
pub use edit::{FileEditPreview, TextEditPreview, WorkspaceEditPreview};
```

Optional: also re-export conversion helpers if desired:

```rust
pub use edit::{preview_text_edits_for_file, preview_workspace_edit};
```

Only re-export helpers if they are intended as supported public API. DTO re-export is enough for this cleanup pass.

Acceptance criteria:

- callers can import `egglsp::WorkspaceEditPreview` directly.
- no unnecessary broad public API surface is exposed.

## Phase 3 — Make Patch Truncation Semantics Safe

Current behavior:

- large per-file patches are replaced with the string `"(patch omitted due to size)"`;
- `truncated = true` is inferred by checking whether `fp.patch.contains("omitted")`.

This is workable but brittle.

Required changes:

1. Add a field to `FileEditPreview`:

```rust
pub patch_omitted: bool,
```

2. Set `patch_omitted = true` when the patch exceeds `MAX_PATCH_CHARS_PER_FILE`.
3. Set `WorkspaceEditPreview.truncated = true` from explicit state, not string matching.
4. Keep `patch` as either:
   - empty string when omitted, or
   - `"(patch omitted due to size)\n"` for human readability.

Preferred:

```rust
patch: String::new(),
patch_omitted: true,
```

But if preserving human readability is preferred, keep the message and rely on `patch_omitted` for logic.

5. Update serialization tests and any schema/output assumptions.

Acceptance criteria:

- no truncation logic relies on `patch.contains("omitted")`.
- omitted patches are clearly marked in structured output.
- the model cannot accidentally treat a truncated placeholder as an applicable patch.

## Phase 4 — Add Apply-Patch Compatibility Test

Current issue:

- `egglsp::edit` patch tests check headers and hunks, but do not prove generated patches can actually be consumed by Codegg's `apply_patch` semantics.
- `src/tool/apply_patch.rs::apply_unified_diff_result` is private, so direct cross-module reuse is not currently available.

Preferred implementation:

1. Extract the pure patch-application helper from `src/tool/apply_patch.rs` into a testable utility module:

```text
src/tool/patch_util.rs
```

or:

```text
src/tool/apply_patch_util.rs
```

2. Move or expose:

```rust
pub(crate) fn apply_unified_diff_result(original: &str, patch: &str) -> Result<String, String>;
```

3. Update `ApplyPatchTool` to use the shared helper.
4. Add tests that generate an LSP preview patch and apply it through the same helper.

Suggested test shape:

```rust
#[test]
fn generated_lsp_patch_applies_with_codegg_patch_parser() {
    let original = "fn main() {\n    old_name();\n}\n";
    let edits = vec![TextEdit { ... old_name -> new_name ... }];
    let preview = preview_text_edits_for_file("rename", path, edits, Some(root)).unwrap();
    let patch = &preview.files[0].patch;
    let updated = apply_unified_diff_result(original, patch).unwrap();
    assert_eq!(updated, "fn main() {\n    new_name();\n}\n");
}
```

Fallback if extraction is too invasive:

- create a minimal compatibility test inside `src/tool/apply_patch.rs` that calls `egglsp::edit` under `dev-dependencies` only if feasible;
- or add a local parser check that mirrors the existing parser behavior.

Preferred path remains extracting the pure helper to avoid duplication.

Acceptance criteria:

- at least one test proves `egglsp` generated patch text can be consumed by Codegg's patch application logic.
- `ApplyPatchTool` still behaves the same externally.
- no mutation occurs in the LSP preview path.

## Phase 5 — Complete Documentation Cleanup

Current `architecture/lsp.md` has been partially updated, but still has stale content.

Fix at least:

1. Key Responsibilities:
   - remove model-facing `code lens` language;
   - add preview-only `renamePreview` and `formatPreview`.
2. Service API block:
   - replace stale `get_or_create_client_for_root_hint` with current API naming, likely `find_existing_client_for_root_hint` if that is the current method.
3. Operations API block:
   - add `prepare_rename`, `rename_preview`, and `format_preview`;
   - mark `code_lens`, `completion`, and arbitrary `code_actions` as internal/non-model-facing.
4. Add a short section:

```markdown
### Preview-only edits

`renamePreview` and `formatPreview` request semantic edits from the language server, convert them into `WorkspaceEditPreview`, and return unified diff patches. They never write files. Applying a preview requires the existing mutating `apply_patch` tool and therefore follows normal Codegg permission handling.
```

5. Check `architecture/tool.md`:
   - update the LSP row to include `renamePreview` and `formatPreview`;
   - explicitly say these remain read-only previews;
   - preserve `apply_patch` as the mutating path.

Acceptance criteria:

- docs no longer imply code lens is model-facing.
- docs no longer mention old `notif_tx` / `notif_rx` or direct response reads.
- docs clearly describe the mutation boundary.

## Phase 6 — Tests to Add or Update

Required tests:

```text
egglsp::edit:
  format_preview_rejects_path_outside_allowed_root
  format_preview_uses_allowed_root_relative_patch_path
  large_patch_sets_patch_omitted_field
  workspace_truncated_uses_structured_flag_not_patch_string

src/tool/apply_patch or shared patch_util:
  generated_lsp_patch_applies_with_codegg_patch_parser

crate exports:
  workspace_edit_preview_type_is_reexported
```

Model-facing wrapper tests already cover schema exposure and read-only category, but update snapshots if `patch_omitted` changes serialized output.

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
cargo test apply_patch
rg "patch_omitted|WorkspaceEditPreview|renamePreview|formatPreview" crates/egglsp src tests architecture
rg "codeLens|code lens|get_or_create_client_for_root_hint|notif_tx|notif_rx|read_response|read_notification" architecture src crates/egglsp
```

Manual smoke check:

```text
1. Use lsp renamePreview on a known symbol.
2. Confirm the returned patch is complete and patch_omitted=false.
3. Confirm no files changed.
4. Apply the returned patch through apply_patch.
5. Confirm the file changes only after apply_patch.
```

## Done Criteria

This cleanup pass is complete when:

- `format_preview` consistently enforces `allowed_root`.
- preview DTOs are re-exported from `egglsp` as intended.
- patch omission is represented structurally, not by string matching.
- generated preview patches have a compatibility test against Codegg's patch-application logic.
- `architecture/lsp.md` and `architecture/tool.md` accurately describe the current model-facing LSP surface and mutation boundary.
- no new model-facing mutation path is introduced.

## Next Pass After Cleanup

After this cleanup lands, proceed to one of:

1. `sourceActionPreview` for allowlisted edit-bearing actions only, starting with `source.organizeImports`.
2. overlay-backed diagnostics/semantic checks for proposed patches.
3. semantic context packets for edit/review/security/explain intents.

Recommended next pass: `sourceActionPreview`, because it reuses the same `WorkspaceEditPreview` infrastructure and is smaller than overlay sync.
