# LSP Phase 6-8 Closeout Corrective Plan

Status date: 2026-06-25
Scope: targeted corrective pass after implementation work following `plans/lsp_phase_6_12_roadmap.md`, `plans/lsp_phase_6_polish_docs_status_plan.md`, `plans/lsp_phase_7_semantic_workflow_recipes_plan.md`, and `plans/lsp_phase_8_preview_artifact_ux_plan.md`.

## Purpose

Phase 6 and Phase 7 appear substantially complete. Phase 8 is also mostly implemented, but a few correctness and UX concerns should be fixed before marking Phases 6, 7, and 8 closed.

This plan is intentionally narrow. Do not broaden it into Phase 9 lifecycle/server-health work or Phase 10 broader semantic-packet expansion. The goal is to close the already-started Phase 6-8 work with precise fixes, regression tests, and documentation updates.

## Current repo shape

The repository now includes:

- Phase 6 status/docs work: `/lsp-status`, `LspTuiSummary::counts_from_packet`, support-tier docs, troubleshooting docs, and regression tests.
- Phase 7 workflow recipes: `crates/egglsp/src/workflow_recipes.rs` with named recipes for repair, review, security enrichment, hunk navigation, and preview suggestions.
- Phase 8 preview UX: expanded `PreviewArtifactRegistry`, preview list/detail rendering, stale-base refresh, preview clear/list/show commands, preview apply-candidate export, and preview patch storage in `LspPreviewArtifact` variants.

The remaining closeout concerns are:

1. `PreviewArtifactRegistry::refresh_staleness()` appears to use `DefaultHasher`, while preview creation stores SHA-256 hashes. That can mark unchanged files stale incorrectly.
2. Preview edit counts are inconsistent across artifact variants. Rename carries `edit_count`, while formatting and code-action variants currently report `0` through `preview_edit_count()`.
3. `/lsp-preview-apply` appears to export/show an apply candidate rather than fully integrating with the mutating apply path. That is safe, but closure docs and UX should make the boundary exact.
4. CI/check status was not independently visible through GitHub status APIs. The closeout should leave explicit local/CI evidence in docs or commit messages.

## Non-goals

Do not make `LspTool` mutate files.

Do not add `workspace/executeCommand` or `workspace/applyEdit` execution.

Do not add persistent semantic cache/memory.

Do not add new broad LSP protocol operations.

Do not rewrite the recipe system. Keep Phase 7 changes limited to regression coverage or small correctness fixes discovered while closing Phase 8.

Do not design full Phase 9 server lifecycle commands here. `/lsp-status` can remain the minimal Phase 6 status surface.

## Workstream 1: centralize preview hash computation

### Problem

`FileEditPreview.original_hash` is produced with SHA-256 during preview construction, while `PreviewArtifactRegistry::refresh_staleness()` currently re-hashes disk content using `std::collections::hash_map::DefaultHasher`. These hashes are not comparable. If registry `original_hashes` are populated from `FileEditPreview.original_hash`, an unchanged file will still look stale.

### Required behavior

Preview staleness must compare like with like. The hash used when generating the preview must be the same hash used when refreshing stale-base state.

The preferred canonical hash is SHA-256 hex because `FileEditPreview.original_hash` already uses SHA-256 and is more stable and explicit than `DefaultHasher`.

### Target files

- `crates/egglsp/src/edit.rs`
- `crates/egglsp/src/preview_registry.rs`
- Possibly a new small helper module if reuse warrants it, but prefer the smallest change.

### Implementation steps

1. Add a public or crate-public helper for SHA-256 text/bytes hashing. Suggested minimal API:

```rust
pub(crate) fn sha256_hex_bytes(bytes: &[u8]) -> String
```

If it belongs in `edit.rs`, make it visible to `preview_registry.rs` only as needed. If `edit.rs` should not expose it, create a small `hash.rs` or `content_hash.rs` module inside `crates/egglsp/src/` and use it from both places.

2. Replace the inline SHA-256 computation in `build_file_preview()` with the helper.
3. Replace `DefaultHasher` in `PreviewArtifactRegistry::refresh_staleness()` with the same helper.
4. Preserve the existing missing-file behavior, but ensure the actual hash string for missing files is clearly non-SHA text such as `missing` or `missing:<io error>`.
5. Add a code comment documenting that preview original hashes and stale refresh hashes must remain algorithm-identical.

### Tests

Add or update tests in `crates/egglsp/src/preview_registry.rs` or a focused integration test:

- `refresh_staleness_unchanged_file_remains_fresh`: create a temp file, compute the same SHA-256 original hash, register a preview, refresh, assert `stale == false` and `stale_files.is_empty()`.
- `refresh_staleness_changed_file_becomes_stale`: modify the temp file after registration, refresh, assert `stale == true` and stale file details include expected and actual SHA-256 strings.
- `refresh_staleness_missing_file_becomes_stale`: remove the temp file after registration, refresh, assert stale with `actual_hash` indicating missing.
- Optional: test that `FileEditPreview.original_hash` equals the shared helper output for a known input.

### Acceptance criteria

- `DefaultHasher` is no longer used for preview stale-base file-content hashing.
- Unchanged files do not become stale after `refresh_staleness()`.
- Changed files become stale with precise expected/actual evidence.
- Missing files become stale with a clear actual-hash marker.

## Workstream 2: make preview edit counts consistent across artifact kinds

### Problem

`PreviewArtifactRegistry::preview_edit_count()` currently reports real edit counts for rename artifacts but returns `0` for formatting and code-action artifacts. This weakens preview list/detail UX and apply-candidate metadata.

### Required behavior

Every preview-producing operation should expose a meaningful edit count in the registry/detail/apply-candidate path.

### Target files

- `crates/egglsp/src/context.rs`
- `crates/egglsp/src/preview_registry.rs`
- `src/tool/lsp.rs`
- Tests in `crates/egglsp/src/tui_summary.rs` and/or preview registry tests.

### Design options

Preferred: add `edit_count: usize` to every `LspPreviewArtifact` variant.

Current shape likely resembles:

```rust
pub enum LspPreviewArtifact {
    Rename { description, edit_count, patches },
    Formatting { description, content_hash, patches },
    CodeAction { description, kind, patches },
}
```

Recommended shape:

```rust
pub enum LspPreviewArtifact {
    Rename { description, edit_count, patches },
    Formatting { description, content_hash, edit_count, patches },
    CodeAction { description, kind, edit_count, patches },
}
```

Alternative: derive edit count from `patches.len()` or from patch contents. This is inferior because a patch count is file count, not edit count, and parsing unified diff hunks to reconstruct edit count is brittle.

### Implementation steps

1. Add `edit_count` to `Formatting` and `CodeAction` variants.
2. Update all constructors in production code and tests.
3. Populate formatting edit count from the typed formatting preview's `edit_count` or `WorkspaceEditPreview.total_edits`, whichever is closer to the operation path.
4. Populate code-action edit count from `CodeActionPreview.edit_count` or `WorkspaceEditPreview.total_edits`.
5. Update `PreviewArtifactRegistry::preview_edit_count()` to return real counts for all variants.
6. Update preview list/detail/apply-candidate tests to assert nonzero formatting/code-action counts where appropriate.
7. Update docs if they currently imply only rename has edit counts.

### Acceptance criteria

- `preview_edit_count()` no longer hardcodes formatting/code-action counts to zero.
- Preview list/detail output shows correct edit counts for rename, formatting, source-action, and code-action previews.
- `PreviewApplyCandidate.edit_count` is meaningful for all preview kinds.
- Tests cover at least one formatting preview and one code-action/source-action preview with a nonzero edit count.

## Workstream 3: clarify and harden preview apply handoff

### Problem

Phase 8 correctly preserves the read-only LSP boundary. `/lsp-preview-apply` appears to export an apply candidate and display patches/instructions. That is safe, but it should be described and tested as an export/handoff unless it actually invokes the mutating apply path.

### Required behavior

The code, command description, docs, and tests must agree on the exact semantics:

- If `/lsp-preview-apply` only exports/show patches, call it an export/handoff preview and do not imply it writes files.
- If it invokes the mutating apply path, it must go through normal approval and revalidate hashes first.

For this closeout, the safer target is: keep it as an export/apply-candidate command and document that no file writes occur.

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/tui_summary.rs`
- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- README command table if applicable.

### Implementation steps

1. Rename descriptions if needed so they say `Export LSP preview apply candidate` rather than `Apply preview`.
2. Ensure the command output explicitly says:
   - no files were changed,
   - preview is not applied,
   - use the separate mutating apply path with user approval,
   - stale previews should be refreshed or regenerated before apply.
3. Ensure `PreviewApplyCandidate` includes:
   - `preview_id`,
   - kind,
   - title,
   - affected files,
   - original hashes,
   - edit count,
   - stale flag,
   - provenance,
   - applied flag,
   - patches.
4. Add or update tests for export output:
   - fresh preview export includes patch text/metadata and says no files changed,
   - stale preview export includes a warning,
   - missing preview ID returns a clear error,
   - already-applied preview, if represented, warns or blocks export.
5. Do not mark a preview as applied merely because it was exported.
6. If there is already an actual apply integration, verify it rechecks hashes and routes through the existing approval path. If not, explicitly document actual apply integration as deferred to Phase 9 or a later mutating-tool UX pass.

### Acceptance criteria

- Command behavior and docs cannot be misread as silent application.
- Exporting an apply candidate never mutates files and never sets `applied = true`.
- Stale preview export displays a strong warning.
- Tests prove export is read-only at the registry state level.

## Workstream 4: complete Phase 6 status/doc closeout checks

### Problem

Phase 6 appears implemented, but the closeout should leave a concise verification trail and ensure no doc-count drift remains.

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `README.md`
- `AGENTS.md`
- `CHANGELOG.md`
- `architecture/overview.md`
- `crates/egglsp/tests/phase6_regression.rs`

### Implementation steps

1. Verify the actual server definition count in `crates/egglsp/src/server.rs` and ensure docs consistently state the same number.
2. Verify README command table includes `/lsp-status` and the Phase 8 preview commands if that table lists system commands.
3. Verify docs distinguish:
   - pinned CI-verified servers,
   - compatibility-profile-supported servers,
   - best-effort server definitions.
4. Verify `LspTuiSummary::counts_from_packet` behavior is covered by tests.
5. Add a short closeout section to `architecture/lsp.md` or `CHANGELOG.md` only if needed; avoid large doc churn.

### Acceptance criteria

- No inconsistent server counts remain in the main docs.
- `/lsp-status` and preview commands are documented where appropriate.
- Status-only summaries do not present placeholder zeros as real counts.

## Workstream 5: complete Phase 7 recipe closeout checks

### Problem

Phase 7 has a strong new recipe module and tests. The closeout should avoid expanding it but should verify the recipes are exported, documented, and safe under fallback conditions.

### Target files

- `crates/egglsp/src/lib.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- Tests touching recipe execution.

### Implementation steps

1. Verify all recipe types/functions intended for external crate use are re-exported from `crates/egglsp/src/lib.rs`.
2. Verify recipe docs state that these are thin helpers over existing packet collection, not a parallel framework.
3. Verify `repair_hunk` and `hunk_source_navigation` preserve `AgentContextSource::Hunk` after dedup/ranking/budget enforcement.
4. Verify `preview_suggestion` does not apply previews or imply workspace mutation.
5. Verify required/opportunistic/disabled mode behavior is tested.
6. Verify stale-evidence summary is tested with at least one stale item.

### Acceptance criteria

- Recipes are discoverable from the crate root if intended.
- Recipe outputs use canonical `LspContextPacket`.
- Hunk source tags survive the recipe path.
- Fallback/stale/disabled behavior has tests.

## Workstream 6: closeout test matrix

Run the narrowest useful test matrix first, then broader checks if feasible.

### Required focused checks

```bash
cargo fmt --check
cargo test -p egglsp preview_registry
cargo test -p egglsp tui_summary
cargo test -p egglsp workflow_recipes
cargo test -p egglsp phase6_regression
```

Adjust exact test filters to match actual test binary/module names.

### Recommended broader checks

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If the full workspace tests or clippy are too slow for the implementing model, at minimum run focused tests plus `cargo test -p egglsp` and document any skipped checks.

### Required new regression tests

- unchanged file remains fresh after preview stale refresh,
- changed file becomes stale after preview stale refresh,
- missing file becomes stale after preview stale refresh,
- formatting preview reports nonzero edit count,
- code-action/source-action preview reports nonzero edit count,
- preview apply-candidate export is read-only and does not mark applied,
- stale preview apply-candidate export includes warning.

## Documentation closeout

After implementation, update only the docs needed to record the corrected behavior:

- `architecture/lsp.md`: Phase 8 closeout note for hash consistency, edit-count consistency, and apply-candidate semantics.
- `.opencode/skills/lsp/SKILL.md`: concise operational note for preview commands and stale-base refresh.
- `CHANGELOG.md`: one unreleased entry if the repo uses it.
- Existing phase plan docs may remain historical; do not rewrite them unless they now contain actively misleading instructions.

## Final acceptance criteria for closing Phases 6-8

Phase 6 can be marked closed when:

- docs have consistent LSP counts and support tiers,
- `/lsp-status` is documented and wired,
- status-only summaries show unavailable counts as unavailable rather than real zero,
- regression tests cover status summary behavior.

Phase 7 can be marked closed when:

- named workflow recipes exist and are exported/documented,
- recipe outputs use canonical `LspContextPacket`,
- hunk/security/preview safety semantics are tested,
- fallback and stale-evidence behavior are tested.

Phase 8 can be marked closed when:

- preview stale refresh uses the same SHA-256 hash algorithm as preview creation,
- preview edit counts are meaningful for rename, formatting, source-action, and code-action previews,
- preview list/detail/clear/refresh/export commands are documented and tested,
- preview apply-candidate export is explicitly read-only,
- stale previews warn users and agents before any apply handoff,
- `LspTool` remains read-only.

## Suggested implementation order

1. Fix hash consistency first. This is the only clear correctness bug.
2. Fix edit-count consistency across preview artifact variants.
3. Tighten `/lsp-preview-apply` wording and tests around read-only export semantics.
4. Add focused regression tests.
5. Update concise docs/changelog closeout notes.
6. Run focused tests and, if feasible, full egglsp/workspace checks.

## Handoff notes for smaller models

Keep this corrective pass surgical. The likely files are `crates/egglsp/src/edit.rs`, `crates/egglsp/src/preview_registry.rs`, `crates/egglsp/src/context.rs`, `crates/egglsp/src/tui_summary.rs`, `src/tool/lsp.rs`, `src/tui/command.rs`, and docs.

Do not change LSP process lifecycle, restart behavior, workspace detection, real-server CI, or model routing as part of this pass.

The most important regression test is the unchanged-file stale-refresh test. If that fails before the hash fix and passes after, the core correctness issue is closed.
