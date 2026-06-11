# LSP Overlay Semantic Checks Hardening Plan

## Purpose

Harden the first `semanticCheckPreview` implementation before building multi-file overlays or semantic context packets on top of it.

The first pass landed the broad shape:

- `crates/egglsp/src/overlay.rs` exists;
- overlay DTOs are exported from `egglsp`;
- `semanticCheckPreview` is exposed through the model-facing `lsp` tool;
- proposed content is pushed to the LSP with `didChange`;
- diagnostics/symbols are collected;
- the disk-backed view is restored afterward;
- the tool remains read-only from the filesystem perspective.

This pass should make the overlay layer reliable enough to treat as infrastructure.

## Current Issues

1. `OverlaySession` exists but `LspOperations::semantic_check_preview` hand-rolls overlay/restore instead of using it.
2. Restore failures are only logged and returned as `restored_disk_view=false`; this may be too soft for a stale LSP state.
3. `semantic_check_preview` does not accept or enforce `allowed_root` at the `egglsp` operation layer.
4. Diagnostics/symbol errors are swallowed as empty lists, making failure hard to distinguish from clean output.
5. Test coverage is mostly DTO serialization/token tests, not restore/no-disk-write behavior.
6. Diagnostic freshness is still underspecified; stale diagnostics can be confused with clean overlay diagnostics.

## Non-Goals

Do not add multi-file overlays in this pass.

Do not add patch input unless it falls out trivially from wrapper validation.

Do not add semantic context packets.

Do not add call hierarchy or type hierarchy.

Do not execute LSP commands.

Do not make overlays persistent across turns.

Do not write proposed overlay content to disk.

## Phase 1 — Use One Overlay Abstraction

Current implementation has two overlay paths:

```text
crates/egglsp/src/overlay.rs::OverlaySession
crates/egglsp/src/operations.rs::semantic_check_preview hand-rolled update/restore
```

Unify this.

Recommended API in `overlay.rs`:

```rust
pub struct OverlaySession {
    service: Arc<LspService>,
}

pub struct OverlayRestoreToken {
    pub(crate) file_path: PathBuf,
    pub(crate) original_text: String,
    pub(crate) key: String,
    pub(crate) uri: String,
}

impl OverlaySession {
    pub async fn apply_overlay(
        &self,
        file_path: &Path,
        proposed_text: &str,
    ) -> Result<OverlayRestoreToken, LspError>;

    pub async fn restore(&self, token: &OverlayRestoreToken) -> Result<(), LspError>;
}
```

Required changes:

1. `apply_overlay` should read original disk content.
2. `apply_overlay` should call `ensure_file_open_from_disk` and retain `(key, uri)` in the token.
3. `apply_overlay` should call `update_file(file_path, proposed_text)`.
4. `restore` should call `update_file(file_path, original_text)`.
5. `semantic_check_preview` should use `OverlaySession` rather than duplicating the flow.

Acceptance criteria:

- one production overlay apply/restore path exists;
- token carries enough data for diagnostics collection without recomputing URI/key;
- no proposed content is written to disk.

## Phase 2 — Add Operation-Level Root Enforcement

Current wrapper-level `resolve_file` validates model-facing use, but `egglsp` operation API should also enforce boundaries like the edit-preview operations do.

Change signature:

```rust
pub async fn semantic_check_preview(
    &self,
    file_path: &Path,
    proposed_text: String,
    allowed_root: Option<&Path>,
) -> Result<SemanticCheckPreview, LspError>;
```

Add a small helper, preferably shared with `edit.rs` if not too invasive:

```rust
fn validate_path_against_root(path: &Path, allowed_root: Option<&Path>) -> Result<PathBuf, LspError>
```

Implementation requirements:

1. If `allowed_root` is `Some(root)`, canonicalize root.
2. Canonicalize file path.
3. Reject paths outside root with `LspError::PathOutsideRoot`.
4. Use the canonical/validated path for overlay operations.
5. Update `src/tool/lsp.rs` to pass `Some(&self.allowed_root)`.

Acceptance criteria:

- direct `egglsp` calls cannot bypass root validation when root is supplied;
- wrapper and crate layer are consistent;
- tests cover outside-root rejection if feasible without an LSP server.

## Phase 3 — Restore Must Be Treated as Critical

Current behavior logs restore failure and returns `restored_disk_view=false` while otherwise returning success.

That is visible but too easy for callers to ignore.

Recommended policy:

- Diagnostics/symbol failures can be partial-output warnings.
- Restore failure should return an error, or return a structured preview with a high-severity `restore_error` field and `restored_disk_view=false`.

Preferred DTO change:

```rust
pub struct SemanticCheckPreview {
    pub file: String,
    pub diagnostics_may_still_be_warming: bool,
    pub diagnostics: Vec<FileDiagnostic>,
    pub diagnostics_error: Option<String>,
    pub symbols: Vec<SemanticSymbolSummary>,
    pub symbols_error: Option<String>,
    pub restored_disk_view: bool,
    pub restore_error: Option<String>,
}
```

If preserving the current DTO is preferred, make restore failure return `Err(LspError::RequestFailed(...))` after logging.

Recommended for agent UX: structured fields are better than hard failure because they can report diagnostics/symbols and still warn that LSP state is stale. However, the wrapper should set `success=false` in structured tool output if `restore_error.is_some()`.

Acceptance criteria:

- restore failure cannot look like a normal successful semantic check;
- output or error clearly says LSP view may be stale;
- model-facing wrapper preserves this signal.

## Phase 4 — Do Not Swallow Diagnostics/Symbol Errors Silently

Current code turns diagnostics/symbol errors into empty vectors.

Change behavior to preserve error metadata:

```rust
let diagnostics_error: Option<String>;
let symbols_error: Option<String>;
```

Rules:

- If diagnostics collection fails, return `diagnostics=[]` and `diagnostics_error=Some(...)`.
- If symbol request fails, return `symbols=[]` and `symbols_error=Some(...)`.
- If both succeed and return no items, errors stay `None`.
- If restore fails, preserve restore signal separately.

Acceptance criteria:

- clean/no diagnostics is distinguishable from diagnostics request failure;
- no symbols is distinguishable from symbol request failure;
- wrapper serializes these fields.

## Phase 5 — Clarify Diagnostic Freshness Semantics

Current approach waits 250 ms, reads diagnostics, and reports `diagnostics_may_still_be_warming`.

Improve the output contract without overbuilding.

Suggested additions:

```rust
pub diagnostics_wait_ms: u64,
pub overlay_version_checked: Option<i32>, // if easy from opened_files/version tracking
```

If version tracking is not easy yet, keep it simple:

- keep fixed wait;
- document that diagnostics may be stale;
- set `diagnostics_may_still_be_warming=true` when no diagnostic notification arrived after overlay update or when cache lacks this URI.

Possible implementation:

1. Before overlay, record whether diagnostics cache has an entry for this URI.
2. After overlay wait, check whether diagnostics cache changed or warming flag says pending.
3. If no overlay-era evidence is available, mark warming true.

Do not introduce indefinite waits.

Acceptance criteria:

- output does not overclaim clean diagnostics when no overlay diagnostic notification has arrived;
- docs explain stale/warming behavior clearly.

## Phase 6 — Wrapper Updates

Update `src/tool/lsp.rs` for the DTO changes and root enforcement.

Required:

1. Pass `Some(&self.allowed_root)` to `semantic_check_preview`.
2. Include `diagnostics_error`, `symbols_error`, and `restore_error` if added.
3. Ensure `semanticCheckPreview` requires `file_path` and `content`.
4. Keep `ToolCategory::ReadOnly`.
5. If `restore_error` is structured instead of hard error, consider `execute_structured` success semantics:
   - plain `execute` can still serialize output;
   - `execute_structured` should ideally set success false when restore failed.

Acceptance criteria:

- model-facing output cannot hide restore failure;
- schema remains clear that `content` is full proposed file content;
- no disk writes are introduced.

## Phase 7 — Tests

Add hermetic tests. Avoid requiring a real language server by default.

Required wrapper tests in `tests/lsp.rs` or `src/tool/lsp.rs`:

```text
schema_includes_semanticCheckPreview
semanticCheckPreview_requires_file_path
semanticCheckPreview_requires_content
semanticCheckPreview_is_read_only
```

Required DTO/helper tests in `overlay.rs`:

```text
semantic_check_preview_serializes_error_fields
overlay_restore_token_carries_key_uri_file_original
symbol_summary_bounding_still_works
```

Required restore/no-disk-write tests if feasible:

```text
overlay_does_not_write_disk
overlay_restore_runs_after_success
overlay_restore_failure_is_visible
```

If current `LspService` is too concrete for hermetic restore tests, add a small trait seam only for overlay behavior:

```rust
#[async_trait]
trait OverlayService {
    async fn ensure_file_open_from_disk(&self, file_path: &Path) -> Result<(String, String), LspError>;
    async fn update_file(&self, file_path: &Path, text: &str) -> Result<(), LspError>;
    async fn diagnostics_may_still_be_warming(&self, key: &str, uri: &str) -> bool;
    async fn get_diagnostics_for_key(&self, key: &str, uri: &str) -> Result<Vec<Diagnostic>, LspError>;
}
```

Keep the seam small and local. Do not redesign `LspService` broadly.

Optional integration test:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test -p egglsp semantic_check_preview_real_lsp -- --nocapture
```

Acceptance criteria:

- default tests prove wrapper validation and DTO behavior;
- restore/no-disk-write behavior is either hermetically tested or explicitly deferred with a clear reason;
- no default test launches an external LSP server.

## Phase 8 — Docs

Update:

```text
architecture/lsp.md
architecture/tool.md
```

Document:

- `semanticCheckPreview` uses a temporary in-memory LSP overlay;
- it never writes proposed content to disk;
- it restores the LSP view to current disk content afterward;
- restore failure is surfaced as an error or structured warning;
- diagnostics may be warming/stale because publishDiagnostics is async;
- first pass is single-file only;
- mutation still requires `apply_patch`.

Acceptance criteria:

- docs do not imply diagnostics are definitive when warming is true;
- docs clearly describe restore failure behavior;
- docs keep the read-only/mutation boundary explicit.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p egglsp overlay
cargo test -p egglsp semantic_check
cargo test --test lsp semanticCheckPreview
rg "semanticCheckPreview|semantic_check_preview|OverlaySession|OverlayRestoreToken|restore_error|diagnostics_error|symbols_error" crates/egglsp src tests architecture
rg "std::fs::write|tokio::fs::write" crates/egglsp/src/overlay.rs crates/egglsp/src/operations.rs src/tool/lsp.rs
```

Manual smoke:

```text
1. Choose a file with clean diagnostics.
2. Call semanticCheckPreview with proposed content that introduces a clear type/syntax error.
3. Confirm disk file is unchanged.
4. Confirm output reports diagnostics or diagnostics_may_still_be_warming=true.
5. Confirm normal diagnostics afterward reflect disk content, not overlay content.
6. Force a restore failure if practical and confirm it is visible as error/restore_error.
```

## Done Criteria

This hardening pass is complete when:

- `semantic_check_preview` uses the shared overlay abstraction;
- operation-level root enforcement exists;
- restore failure is impossible to miss;
- diagnostics/symbol request failures are distinguishable from clean empty output;
- diagnostic warming/staleness semantics are documented and represented;
- wrapper tests cover required validation;
- no disk-write path for proposed content exists;
- docs explain limitations and safety boundaries.

## Next Pass After Hardening

After this hardening lands, move to one of:

1. patch-input support for `semanticCheckPreview`;
2. multi-file overlay sessions for rename/source-action previews;
3. semantic context packets for edit/review/security/explain workflows.

Recommended next pass: patch-input support, because it makes the new semantic check immediately useful with existing `WorkspaceEditPreview` patches while keeping scope smaller than multi-file overlays.
