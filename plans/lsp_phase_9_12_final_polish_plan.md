# LSP Phase 9-12 Final Polish Plan

Status date: 2026-06-26
Scope: final closure polish for the remaining post-hardening caveats.

## Purpose

The Phase 9-12 hardening pass has moved the LSP line into closeable shape. The remaining issues are narrow and should be resolved before treating the LSP roadmap through Phase 12 as fully closed.

This plan targets only two final caveats:

1. The production agent-context cache path currently uses an empty file-hash map. That is safe only when cache is disabled by default or when docs clearly state that this entry point is generation/request-keyed rather than file-hash-invalidated. Prefer adding real file hashes for the relevant request inputs.
2. The preview-apply validation boundary is strong, but the write-side TUI handler needs an explicit audit and tests proving no partial-write/mark-applied bug exists.

Do not add new LSP features while executing this plan.

## Current baseline

The repo now includes:

- `validate_preview_apply()` in `crates/egglsp/src/tui_summary.rs`, which validates preview application without mutating files or registry state.
- `PreviewApplyError`, `PreviewApplyPlan`, and `PreviewApplyFilePlan` for typed pre-apply validation.
- `/lsp-preview-apply` wired as an apply command rather than a read-only export-only command.
- A `collect_context_cached()` helper that supports file hashes, cache keys, cache hits, misses, and insertion.
- A production `LspTool::lsp_context_for_agent_with_input()` path that uses the semantic cache when enabled, but currently documents that the file-hash map is empty for that entry point.
- A large dispatch test suite for LSP TUI commands.

## Non-goals

Do not add new semantic operation variants.

Do not add disk cache.

Do not change cache default from disabled unless the repo already intends that and tests cover it.

Do not execute LSP `workspace/applyEdit` or `workspace/executeCommand`.

Do not make `egglsp` itself write files.

Do not redesign the TUI patch/apply subsystem.

## Workstream 1: add request-scoped file hashing to production cache keys

### Problem

The production `LspTool::lsp_context_for_agent_with_input()` path currently uses an empty file-hash map when consulting the semantic cache. This means a cache hit may survive on-disk file changes as long as the request, budget, root, server ID, and server generation remain the same.

The cache itself supports file-hash invalidation. The production path should use that support for files known from the `LspContextRequest` and `LspAgentContextInput`.

### Target files

- `src/tool/lsp.rs`
- `crates/egglsp/src/context.rs`
- `crates/egglsp/src/cache.rs`
- `crates/egglsp/src/evidence_collector.rs`
- tests in `src/tool/lsp.rs` or `crates/egglsp` as appropriate
- docs in `architecture/lsp.md` and `.opencode/skills/lsp/SKILL.md`

### Design target

Add a small helper that extracts candidate file paths from the request/input and hashes the existing files before cache lookup.

Suggested helper shape in `src/tool/lsp.rs`:

```rust
fn collect_cache_file_hashes_for_request(
    request: &egglsp::LspContextRequest,
    allowed_root: &std::path::Path,
) -> std::collections::BTreeMap<std::path::PathBuf, String>
```

The helper should:

- collect only explicit file paths already present in the request,
- validate/normalize each path against `allowed_root`,
- skip missing/unreadable files with a debug note rather than panicking,
- use the same SHA-256 helper semantics as preview hashing, if accessible,
- avoid broad workspace discovery,
- cap the number of hashed files to avoid expensive hashing on large requests.

### File extraction rules

For each `LspContextRequest` variant:

- `File`: hash `file`.
- `Hunk`: hash `file`.
- `Symbol`: hash `file`.
- `Review`: hash `changed_files`; optionally also hash hunk files if separate.
- `ImpactAnalysis`: hash `symbol.file` and `changed_files`, capped.
- `TestFailureRepair`: hash `test_file` and explicit `related_files`, capped.
- `InterfaceBoundary`: hash `file`.
- `CrossFileRepair`: hash `primary_file` and explicit `related_files`, capped.
- `CallNeighborhood`: hash `file`.

Do not infer additional files from LSP results for the cache key in this final polish pass. Request-input hashes are enough to eliminate the obvious stale-hit concern.

### Cap and failure behavior

Use a small conservative cap, such as 16 files. If more files are present:

- hash the first 16 in deterministic order,
- add a debug log or packet note such as `cache file-hash inputs capped at 16`,
- ensure this cap is documented.

If a file cannot be read:

- omit it from file hashes,
- optionally include a synthetic value such as `missing` only if it will not cause accidental reuse,
- prefer omitting plus disabling cache for that request if the primary file is missing/unreadable.

Recommended conservative rule:

- if the primary file for the request cannot be read, bypass cache for that request.
- if a related file cannot be read, omit it and continue with a debug note.

### Implementation steps

1. Add request path extraction helper.
2. Add SHA-256 file hashing helper in `src/tool/lsp.rs` or reuse an existing helper if cleanly exported.
3. Replace the empty `BTreeMap` in `LspTool::lsp_context_for_agent_with_input()` with request-scoped hashes.
4. If no files can be hashed for a request that should have a primary file, bypass cache and use direct collection.
5. Include file hashes in both lookup and insert key paths.
6. Add debug-level logs for hash cap, missing files, and cache bypass.
7. Update docs to remove or narrow the statement that the production path omits file hashes.

### Required tests

Add tests for:

- `File` request hashes exactly the file.
- `Review` request hashes changed files deterministically.
- `CrossFileRepair` request hashes primary and related files up to the cap.
- missing primary file bypasses cache.
- changed file content causes cache miss on the second request.
- unchanged file content causes cache hit when cache is enabled.
- disabled cache still bypasses all hash/cache behavior.

### Acceptance criteria

- Production agent-context cache keys include request-scoped file hashes when files are available.
- A file edit invalidates the relevant cache entry.
- The cache remains disabled by default unless explicitly configured.
- The implementation does not scan the workspace broadly.

## Workstream 2: audit and test the preview-apply write-side handler

### Problem

`validate_preview_apply()` is a good pre-mutation boundary. The remaining risk is the caller that writes `PreviewApplyPlan.files[*].new_content` to disk and marks the preview applied.

### Required invariant

The preview must be marked applied only after every planned file write succeeds. Partial write failures must be reported clearly and must not mark the preview applied.

### Target files

- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/tui_summary.rs`
- preview apply command tests
- any existing patch/apply utility file

### Audit checklist

Verify the `/lsp-preview-apply` handler does all of the following:

1. Parses exactly one preview ID.
2. Returns usage text when ID is missing.
3. Requires an available `LspTool`.
4. Calls `refresh_preview_staleness(id)` or equivalent immediately before validation, or relies on `validate_preview_apply()` checking hashes directly.
5. Calls `validate_preview_apply()` before any write.
6. Performs writes only from the returned `PreviewApplyPlan`.
7. Writes all files successfully before calling `mark_preview_applied(id)`.
8. If any write fails, reports the failed path and does not call `mark_preview_applied(id)`.
9. If multiple files are written and a later write fails, reports that partial writes may have occurred and does not mark applied.
10. Does not call LSP `workspace/applyEdit` or `workspace/executeCommand`.
11. Does not silently overwrite files whose hashes changed after validation.
12. Ideally rechecks the validated hash immediately before each write, especially if validation and writing are separated.

### Recommended implementation refinement

If the current handler validates and then writes without rechecking, add a small helper:

```rust
fn write_preview_apply_plan_atomically_enough(
    plan: &PreviewApplyPlan,
) -> Result<PreviewApplyWriteReport, PreviewApplyWriteError>
```

This helper should not promise true filesystem atomicity unless implemented. It should instead provide deterministic best-effort behavior:

- before each write, re-read the file and confirm its SHA-256 equals `file.validated_hash`,
- write `file.new_content`,
- track successful writes,
- if any write fails, return a report containing completed paths and failed path,
- the caller marks applied only when all writes succeeded.

Do not introduce complex rollback in this final polish pass unless there is already a robust rollback utility. If partial writes can occur, make the error explicit.

### Required tests

Add handler/helper tests for:

- successful single-file apply marks applied.
- successful multi-file apply marks applied only after all writes succeed.
- validation failure does not write.
- stale/hash mismatch does not write.
- write failure on first file does not mark applied.
- write failure on second file does not mark applied and reports partial write state.
- file changed between validation and write is blocked by second hash check.
- already-applied preview is blocked.
- no-patch preview is blocked.

### Acceptance criteria

- Preview apply has a proven pre-write validation gate.
- Preview apply has a proven write-side success/failure gate.
- Registry `applied` state cannot be set after partial failure.
- User-facing errors distinguish validation failure, write failure, and partial write failure.

## Workstream 3: final safety sweep

### Purpose

Confirm the final polish did not weaken the long-standing LSP mutation boundary.

### Static searches

Run and inspect:

```bash
rg "workspace/applyEdit|workspace/executeCommand|executeCommand|applyEdit" src crates/egglsp
rg "std::fs::write|write_all|File::create" src crates/egglsp
rg "mark_preview_applied|mark_applied" src crates/egglsp
```

Expected allowed matches:

- docs/comments/tests explaining unsupported or rejected LSP mutations,
- preview apply write-side handler in root TUI/app code,
- registry state updates after successful non-LSP writes.

Disallowed matches:

- `egglsp` writing source files,
- LSP operation code executing server commands,
- mark-applied before all writes complete,
- file writes in validation-only code.

### Acceptance criteria

- Static sweep findings are summarized in commit message or docs.
- Any suspicious match is removed or backed by tests/comments.

## Workstream 4: documentation polish

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `CHANGELOG.md`
- `plans/lsp_phase_6_12_roadmap.md`

### Required doc updates

1. State that production semantic-cache keys include request-scoped file hashes where available.
2. State exact fallback behavior when primary file hashes cannot be computed.
3. State the cache remains disabled by default unless user config enables memory mode.
4. State that preview apply validates first, writes only from a validated plan, and marks applied only after full success.
5. State that partial write failures do not mark previews applied and are reported to the user.
6. Update roadmap labels from `hardened` to `closed` only after this polish pass and tests pass.

### Acceptance criteria

- Docs no longer contain the caveat that production cache keys omit file hashes, unless a narrowed exception remains true.
- Preview apply docs match the actual write-side behavior.
- Roadmap status is precise: `closed` only if all acceptance criteria pass.

## Workstream 5: final validation commands

Run focused checks first:

```bash
cargo fmt --check
cargo test -p egglsp cache
cargo test -p egglsp tui_summary
cargo test -p egglsp evidence_collector
cargo test --test phase5_context_integration lsp
```

Then run broader checks if feasible:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If root workspace tests or clippy fail for unrelated reasons, document the exact failure and prove the focused LSP test set passes.

## Final closure criteria

This polish pass is complete when:

- production agent-context cache keys include request-scoped file hashes where available,
- changing a request input file invalidates cache hits in tests,
- cache remains disabled by default unless configured,
- preview apply write-side behavior is tested for success, validation failure, write failure, partial failure, and race-after-validation,
- applied state is set only after full success,
- static sweep confirms no LSP mutation boundary regression,
- docs and roadmap reflect actual final behavior,
- focused LSP tests and formatting pass.

After this, the LSP roadmap through Phase 12 can be treated as closed unless real-world usage exposes bugs.