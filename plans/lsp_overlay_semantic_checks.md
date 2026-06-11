# LSP Overlay-Backed Semantic Checks Plan

## Purpose

Add an overlay-backed semantic check layer so Codegg can ask LSP for diagnostics and lightweight semantic facts against proposed file content before writing it to disk.

The current LSP edit-preview path is now safe and useful:

```text
renamePreview / formatPreview / sourceActionPreview
  -> WorkspaceEditPreview
  -> unified diff patch preview
  -> apply_patch for actual mutation
```

The next limitation is that semantic feedback is still disk-backed. Codegg can preview patches, but it cannot reliably ask the language server, "what diagnostics/symbols would this proposed file have if applied?" without writing the file.

This pass should introduce temporary in-memory overlays that sync proposed content to the language server using `textDocument/didChange`, gather diagnostics/symbols/code actions, then restore the server view back to the real disk content.

## Target Feature

Add a preview-only LSP operation:

```json
{
  "operation": "semanticCheckPreview",
  "file_path": "src/main.rs",
  "content": "proposed full file content..."
}
```

Optional second input form:

```json
{
  "operation": "semanticCheckPreview",
  "file_path": "src/main.rs",
  "patch": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ ..."
}
```

Recommended first pass: support full `content` first. Add `patch` only if it can reuse existing `patch_util::apply_unified_diff` cleanly without dependency coupling problems.

Output should summarize:

```json
{
  "operation": "semanticCheckPreview",
  "file_path": "src/main.rs",
  "result_count": 3,
  "truncated": false,
  "results": {
    "diagnostics_may_still_be_warming": false,
    "diagnostics": [...],
    "symbols": [...],
    "restored_disk_view": true
  }
}
```

## Non-Goals

Do not write proposed content to disk.

Do not make overlays persistent across user turns in this pass.

Do not support multi-file overlays in this first pass.

Do not add agent-managed patch queues.

Do not execute commands returned by LSP.

Do not expose completions.

Do not replace `apply_patch`.

Do not rely on a real language server in default tests.

## Current State Summary

Relevant files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/service.rs
crates/egglsp/src/diagnostics.rs
crates/egglsp/src/operations.rs
crates/egglsp/src/edit.rs
src/tool/lsp.rs
src/tool/patch_util.rs
tests/lsp.rs
architecture/lsp.md
architecture/tool.md
```

Current useful foundation:

- `LspService::ensure_file_open_from_disk` can open/sync a real file before operations.
- `LspClient::update_file` can send `textDocument/didChange` and update the open-file version.
- diagnostics are notification-driven and can report warm-up state.
- `documentSymbol` and diagnostics already have compact model-facing summaries.
- `apply_patch` mutation remains separate from LSP preview operations.
- edit previews already produce `original_hash` and patch metadata.

## Design Rule

Overlay semantic checks must be temporary, scoped, and restorative.

The flow should be:

```text
1. resolve + validate file path
2. read current disk content
3. ensure file is open in LSP from disk
4. send didChange with proposed content
5. wait/debounce briefly for diagnostics
6. collect diagnostics and optional symbols against proposed content
7. send didChange restoring disk content
8. return preview result
```

The file on disk must never change.

The language server view must be restored even if diagnostics/symbol retrieval fails.

## Phase 1 — Add Overlay Session Guard in `egglsp`

Add a scoped helper to `crates/egglsp/src/service.rs` or a new module:

```text
crates/egglsp/src/overlay.rs
```

Suggested type:

```rust
pub struct OverlayRestoreGuard {
    service: Arc<LspService>,
    file_path: PathBuf,
    original_text: String,
    restored: bool,
}
```

But because async cleanup in `Drop` is awkward, prefer an explicit API:

```rust
pub async fn with_temporary_overlay<F, Fut, T>(
    service: Arc<LspService>,
    file_path: &Path,
    proposed_text: String,
    f: F,
) -> Result<T, LspError>
where
    F: FnOnce(String, String) -> Fut,
    Fut: Future<Output = Result<T, LspError>>;
```

Simpler implementation acceptable:

```rust
pub async fn apply_temporary_overlay(
    &self,
    file_path: &Path,
    proposed_text: &str,
) -> Result<OverlayRestoreToken, LspError>;

pub async fn restore_overlay(
    &self,
    token: OverlayRestoreToken,
) -> Result<(), LspError>;
```

Required behavior:

1. Read disk content before overlay.
2. Call `ensure_file_open_from_disk(file_path)`.
3. Call `update_file(file_path, proposed_text)` to push the overlay.
4. Return enough data to restore the original disk content.
5. Ensure restore uses the same file path and current client key resolution.
6. If overlay application fails, do not attempt restore.
7. If restore fails, return or attach a clear error so the caller can warn that LSP state may be stale.

Acceptance criteria:

- overlay changes only LSP in-memory state;
- disk is not written;
- restore path is explicit and testable;
- version counters continue to advance normally via existing `update_file`.

## Phase 2 — Add `semantic_check_preview` Operation

Add to `crates/egglsp/src/operations.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticCheckPreview {
    pub file: PathBuf,
    pub diagnostics_may_still_be_warming: bool,
    pub diagnostics: Vec<FileDiagnosticLike>,
    pub symbols: Vec<SemanticSymbolSummary>,
    pub restored_disk_view: bool,
}

pub async fn semantic_check_preview(
    &self,
    file_path: &Path,
    proposed_text: String,
    allowed_root: Option<&Path>,
) -> Result<SemanticCheckPreview, LspError>;
```

Use existing diagnostics DTOs if possible rather than inventing duplicate types.

Implementation outline:

```rust
let original = tokio::fs::read_to_string(file_path).await?;
validate allowed_root;
ensure file open from disk;
update file to proposed_text;
wait diagnostics debounce interval;
collect diagnostics for file URI/key;
request document symbols;
restore original text;
return SemanticCheckPreview { restored_disk_view: true, ... };
```

Important: restore must run even if diagnostics or symbols fail. Use a pattern like:

```rust
let result = async {
    // collect diagnostics/symbols
}.await;
let restore_result = self.service.update_file(file_path, &original).await;
match (result, restore_result) { ... }
```

Acceptance criteria:

- proposed text is checked through LSP without disk mutation;
- restore happens after the preview operation;
- restore failure is surfaced;
- output includes enough data for the agent to decide whether applying the patch is safe.

## Phase 3 — Diagnostics Collection Against Overlay

Diagnostics are notification-driven, so this needs careful semantics.

Required behavior:

1. After applying proposed text with `didChange`, wait for a bounded debounce interval.
2. Reuse existing diagnostics cache for the file URI.
3. Report `diagnostics_may_still_be_warming` if diagnostics have not arrived yet.
4. Do not block indefinitely waiting for language-server diagnostics.

Suggested caps:

```rust
const OVERLAY_DIAGNOSTIC_WAIT_MS: u64 = 250;
const OVERLAY_DIAGNOSTIC_MAX_WAIT_MS: u64 = 1500; // optional future retry loop
const MAX_OVERLAY_DIAGNOSTICS: usize = 100;
const MAX_OVERLAY_SYMBOLS: usize = 200;
```

Recommended first pass:

- sleep once for 250–500ms after overlay update;
- read diagnostics cache;
- set warming flag if no diagnostic notification has arrived for this version/window.

Do not overbuild retry loops yet.

Acceptance criteria:

- no indefinite wait;
- diagnostics output distinguishes clean file from still-warming state where possible;
- output is bounded.

## Phase 4 — Symbol Summary Against Overlay

After applying the overlay, request document symbols for the same file.

Implementation choices:

- Reuse `document_symbols(file_path)` after overlay update.
- Convert symbols to compact summaries similar to `src/tool/lsp.rs`.
- Keep nesting flattening bounded.

Output fields for each symbol:

```rust
pub struct SemanticSymbolSummary {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}
```

Acceptance criteria:

- symbols reflect proposed text, not disk content;
- output is bounded;
- symbol failures should not prevent diagnostics from being returned unless the whole request fails before restore.

## Phase 5 — Model-Facing Tool Wrapper

Update `src/tool/lsp.rs`.

Input additions:

```rust
content: Option<String>,
patch: Option<String>, // optional, maybe defer
```

Add operation enum:

```text
semanticCheckPreview
```

Schema description:

```text
Run diagnostics/symbol preview against proposed full file content without writing it to disk. The LSP view is restored to disk content afterward.
```

Execution branch:

```rust
"semanticCheckPreview" => {
    let file = self.resolve_file(&parsed.file_path)?;
    let content = parsed.content.as_ref().ok_or_else(...)?;
    let preview = ops.semantic_check_preview(&file, content.clone(), Some(&self.allowed_root)).await?;
    serialize LspToolOutput { ... }
}
```

If `patch` support is added:

1. read original file;
2. apply patch using shared `patch_util::apply_unified_diff`;
3. pass resulting content to `semantic_check_preview`.

Recommendation: defer `patch` support unless it is trivial to avoid coupling `egglsp` to Codegg's tool module.

Acceptance criteria:

- `semanticCheckPreview` requires `file_path` and either `content` or supported `patch`;
- `lsp` remains `ToolCategory::ReadOnly`;
- operation does not write disk;
- schema clearly states preview-only behavior.

## Phase 6 — Tests

Default tests must be hermetic and require no external LSP server.

Add pure/unit tests for overlay helper behavior using fake service/client seams if available. If no fake seam exists, focus tests on helpers and wrapper validation, then add optional integration tests.

Required tests:

```text
semanticCheckPreview_requires_file_path
semanticCheckPreview_requires_content
semanticCheckPreview_is_read_only
schema_includes_semanticCheckPreview
semantic_check_output_serializes
symbol_summary_bounding
```

Overlay behavior tests if feasible:

```text
overlay_restore_runs_after_success
overlay_restore_runs_after_diagnostic_failure
overlay_restore_error_is_reported
overlay_does_not_write_disk
```

If current architecture makes these hard without a real LSP server, introduce a small trait seam around the subset of service methods used by overlay operations:

```rust
trait OverlayLspSession {
    async fn ensure_file_open_from_disk(...);
    async fn update_file(...);
    async fn get_diagnostics_for_file_or_uri(...);
    async fn document_symbols(...);
}
```

Do not over-abstract the production path. The seam should exist primarily to make restore behavior testable.

Optional integration test:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp semantic_check_preview_real_lsp -- --nocapture
```

It should skip cleanly if `rust-analyzer` is unavailable.

## Phase 7 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
```

Document:

- overlay-backed semantic previews;
- disk is never written;
- LSP in-memory state is restored after the check;
- diagnostics may be warming/stale depending on language-server notification timing;
- `semanticCheckPreview` is single-file in the first pass;
- mutation still requires `apply_patch`.

Add a short architecture note:

```markdown
### Temporary overlays

`semanticCheckPreview` pushes proposed file content to the language server with `didChange`, gathers diagnostics/symbols, then restores the LSP view back to the current disk content. This allows pre-apply semantic checks without writing files. The operation is read-only from Codegg's filesystem permission perspective.
```

## Phase 8 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted checks:

```bash
cargo test -p egglsp overlay
cargo test -p egglsp semantic_check
cargo test --test lsp semanticCheckPreview
rg "semanticCheckPreview|semantic_check_preview|OverlayRestore|temporary overlay" crates/egglsp src tests architecture
rg "std::fs::write|tokio::fs::write" crates/egglsp/src src/tool/lsp.rs
```

Manual smoke with a real Rust project:

```text
1. Choose a Rust file with no diagnostics.
2. Call semanticCheckPreview with proposed content that introduces a type error.
3. Confirm diagnostics are returned or diagnostics_may_still_be_warming=true.
4. Confirm the file on disk is unchanged.
5. Call normal diagnostics afterward and confirm LSP view was restored to disk content.
```

## Done Criteria

This pass is complete when:

- Codegg exposes `semanticCheckPreview` as a read-only LSP operation;
- proposed full-file content can be semantically checked without disk writes;
- LSP in-memory state is restored to disk content after the operation;
- diagnostics and symbol summaries are bounded;
- restore failures are surfaced clearly;
- default tests are hermetic;
- docs explain temporary overlay semantics and limitations.

## Follow-Up Passes

After this lands:

1. Add patch-input support for `semanticCheckPreview` if deferred.
2. Add multi-file overlay sessions for rename/source-action previews that touch multiple files.
3. Add semantic context packets for edit/review/security/explain workflows.
4. Add call hierarchy and type hierarchy previews.
