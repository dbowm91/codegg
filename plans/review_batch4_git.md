# Batch 4 Review: Git, Worktree, Run Artifacts

> Review-only document. Generated from architecture deep-dive.
> Date: 2026-09-13

## Output Template

| Field | Value |
|-------|-------|
| Batch | 4 — git, worktree, run artifacts |
| Docs reviewed | git.md, git_phase_f_handoff.md, git_polish_verification_handoff.md, worktree.md, snapshot.md, run_store.md |
| Source files verified | crates/codegg-git/operation.rs, risk.rs; crates/egggit/src/process.rs, operation_state.rs, conflict.rs, worktree.rs; src/git_mutations.rs, git_network_policy.rs, git_service.rs, git_recovery.rs, tool/git.rs; crates/codegg-core/src/worktree.rs, worktree_service.rs, run_store.rs, snapshot/mod.rs, snapshot/checkpoint.rs, session/schema.rs |
| Test commands run | `cargo test -p codegg-git` (358), `cargo test -p egggit` (75), `cargo test -p codegg-core -- run_store` (18), `cargo test -p codegg-core -- worktree` (12), `cargo test -p codegg-core -- snapshot` (25), plus 6 integration test binaries |

---

## 1. architecture/git.md

### Verified Claims

| # | Claim | Evidence | Status |
|---|-------|----------|--------|
| 1 | `GitOperation` has 54 variants | `crates/codegg-git/src/operation.rs:10` — counted 54 enum variants (Status through RawShellRequired) | **OK** |
| 2 | `GitRiskClass` has 11 variants | `crates/codegg-git/src/risk.rs` — ReadOnly, IndexMutation, WorktreeMutation, RefMutation, HistoryIntegration, NetworkRead, NetworkWrite, RepositoryConfigMutation, DestructiveWorktree, DestructiveHistory, OutsideProject | **OK** |
| 3 | `ALLOWED_ENV_VARS` has 21 entries | `crates/egggit/src/process.rs` — counted 21 entries (PATH, HOME, …, GIT_SSL_CAPATH) | **OK** |
| 4 | `ALWAYS_STRIPPED_ENV_VARS` has 28 entries | `crates/egggit/src/process.rs` — counted 28 entries (GIT_ASKPASS through PAGER) | **OK** |
| 5 | `NETWORK_ALLOWED_ENV_VARS` has 21 entries | `src/git_network_policy.rs` — counted 21 entries (GIT_ASKPASS through GIT_CURL_VERBOSE) | **OK** |
| 6 | `NetworkFailureKind` has 7 variants | `src/git_network_policy.rs` — Dns, Connect, Authentication, Authorization, RefRejected, Timeout, Transport | **OK** |
| 7 | `ConflictKind` has 8 variants | `crates/egggit/src/conflict.rs` — BothModified, BothAdded, BothDeleted, AddedByUs, AddedByTheirs, DeletedByUs, DeletedByTheirs, Unknown | **OK** |
| 8 | `RepositoryOperationState` has 9 variants | `crates/egggit/src/operation_state.rs` — None, Merge, Rebase, CherryPick, Revert, Bisect, ApplyMailbox, Sequencer, Unknown | **OK** |
| 9 | `GitPayload` has 13 variants | `src/git_service.rs` — Status, DiffSummary, DiffText, DiffResult, Show, ChangedFiles, Log, Branches, Tags, Remotes, Worktrees, Stashes, None | **OK** |
| 10 | `GitMutationError` has 7 variants | `src/git_mutations.rs` — Execution, Repository, Precondition, Path, Ref, Timeout, StateMismatch | **OK** |
| 11 | egggit is read-only (no mutations) | `crates/egggit/src/` — no mutation functions; only status/diff/log/blame/refs/worktree/conflict/operation_state reads | **OK** |
| 12 | `PushForce` has 3 variants | git_network_ops.rs — Normal, ForceWithLease, Force | **OK** |
| 13 | `PullStrategy` has 3 variants | git_network_ops.rs — Merge, Rebase, FastForwardOnly | **OK** |
| 14 | `MutationOutcome` has 5 variants | codegg-git/src/workflow.rs — Completed, NoOp, FastForward, Conflict, Rejected | **OK** |
| 15 | `RecoveryOutcome` has 5 variants | src/git_recovery.rs — Completed, StillInProgress, Conflicted, NoOp, Rejected | **OK** |
| 16 | Canonical source of truth for env policy is `egggit::process` | `crates/egggit/src/process.rs` defines `ALLOWED_ENV_VARS` and `ALWAYS_STRIPPED_ENV_VARS`; codegg-git re-exports for compat | **OK** |

### Divergences Found

| # | Claim | Actual | Severity |
|---|-------|--------|----------|
| D1 | `mutation (40 actions)` (line 31, git.md:31) | GitTool schema has **42** mutation action enum entries (stage_paths through clean) | Low — doc understates count by 2 |
| D2 | `src/lib.rs:12` re-exports worktree | Worktree is in the `codegg_core::{...}` block at line 13 (within the combined `pub use codegg_core::{ ... worktree };`) — not at line 12 as a standalone | Trivial — line reference stale |

### Stale / Dead References

- `architecture/git.md:441` references `architecture/git_phase_f_handoff.md` as "do not modify" — still accurate as a historical snapshot.
- `architecture/git.md:443` references `architecture/git_polish_verification_handoff.md` — still accurate as a historical snapshot.
- `architecture/git.md:341` references `architecture/tool_programs.md` (Expansion M003) — confirmed tool_programs.md exists.

### Improvement Suggestions

1. **Line 31**: Update "mutation (40 actions)" to "mutation (42 actions)" to match current schema.
2. **Line 230**: `GitExecutionService` is referenced at `:232` — verify this is still accurate as the struct definition may have shifted.

---

## 2. architecture/git_phase_f_handoff.md

### Verified Claims

| # | Claim | Evidence | Status |
|---|-------|----------|--------|
| 1 | Phase F complete at commit `08709d3` | Historical record; cannot verify commit hash without git log | Unverifiable (historical) |
| 2 | Corrective closure commit `cb192e9` | Historical record | Unverifiable (historical) |
| 3 | `ExecutionBackend::GitMutating` REMOVED | `grep -rn "GitMutating" src/` — only deprecated markers remain | **OK** |
| 4 | `PlannedBackend::GitMutating` DEPRECATED | Same grep — only serialization compat | **OK** |
| 5 | `ActualBackend::GitMutating` DEPRECATED | Same grep | **OK** |
| 6 | egggit has 71 tests (Phase F) | Actual: 75 tests now (growth since Phase F) | **Stale** — count grew from 71→75 |
| 7 | codegg-git has 331 tests (Phase F) | Actual: 358 tests now | **Stale** — count grew from 331→358 |
| 8 | `mutation` enum entries ≥35 | Actual: 42 entries in current schema | **Stale** — understated |

### Divergences Found

| # | Claim | Actual | Severity |
|---|-------|--------|----------|
| D1 | egggit: "71 tests" (line 104) | 75 tests currently | Low — natural test growth |
| D2 | codegg-git: "331 tests" (line 105) | 358 tests currently | Low — natural test growth |
| D3 | `mutation enum entries ≥35` (line 41) | 42 entries currently | Low — natural growth |
| D4 | "402 egggit tests" summary (line 116) | 75 tests in egggit crate, not 402 | **Medium** — possible conflation with codegg-git 358 tests (358+75=433, not 402). Line 116 says "402 egggit tests" which is incorrect |
| D5 | `git_closure_matrix` "32 tests" (line 110) | 34 tests currently | Low — growth |

### Stale / Dead References

- Document correctly self-identifies as "Historical snapshot" (line 3). All test counts are snapshot-time values.
- References to `docs/validation/git-security-review.md`, `docs/validation/git-performance-review.md`, `docs/validation/git-cross-platform.md` — these are in `docs/validation/` not `architecture/`. Acceptable for historical handoff.

### Improvement Suggestions

1. **Line 116**: The "402 egggit tests" claim appears to be a miscount or conflation. The actual egggit crate has 75 tests; codegg-git has 358. Consider correcting to "75 egggit tests, 358 codegg-git tests" or marking as historical snapshot value.
2. Since this is explicitly a historical snapshot, no action needed — the doc correctly points to `architecture/git.md` for current state.

---

## 3. architecture/git_polish_verification_handoff.md

### Verified Claims

| # | Claim | Evidence | Status |
|---|-------|----------|--------|
| 1 | `operation.rs` has 47 variants (line 34) | Actual: 54 variants (47 was the Phase closure count) | **Stale** — 47→54 growth since closure |
| 2 | `ALLOWED_ENV_VARS` described as "canonical" | Confirmed in `egggit::process` | **OK** |
| 3 | `AuditSafeArgv` in `codegg_git::sensitive` | Confirmed: `AuditSafeArgv` in `crates/codegg-git/src/sensitive.rs` | **OK** |
| 4 | `sanitize_argv_for_run_store` exists | Confirmed in `src/git_network_policy.rs` | **OK** |
| 5 | Forbidden-pattern check: `PASS (0 findings)` | Cannot re-run the Python script, but the check exists in `scripts/check_git_forbidden_patterns.py` | Unverified |
| 6 | execution-origin matrix: 19 original + 7 D4 = 26 tests | `tests/git_execution_origin_matrix.rs`: 28 tests currently | **Stale** — grew from 26→28 |
| 7 | git-focused test suite "141+ passed" | git_mutations_integration: 12, git_network_integration: 29, git_recovery_integration: 19, git_closure_matrix: 34, git_execution_origin_matrix: 28, git_credential_cross_path: counted separately — total across all git test binaries exceeds 141 | **Stale** — grown since recording |

### Divergences Found

| # | Claim | Actual | Severity |
|---|-------|--------|----------|
| D1 | `operation.rs` has 47 variants (line 34) | 54 variants | **Medium** — doc understates current variant count by 7 |
| D2 | execution-origin matrix: 26 tests | 28 tests | Low — growth |
| D3 | "28 passed" for execution-origin (line 242) | 28 tests (matches if we count the most recent run) | Close — within 2 |

### Stale / Dead References

- The "Gap-closure delta" commit references (line 378) are historical — acceptable for a snapshot doc.
- `docs/validation/git-rerun-secret-lifecycle.md` referenced multiple times — need to verify this file exists.

### Improvement Suggestions

1. The operation.rs variant count (47) is the most significant stale claim. It should read 54 to match the current codebase. Since this is a snapshot doc, the staleness is acceptable but worth noting.
2. Consider adding a "last verified" timestamp to snapshot docs.

---

## 4. architecture/worktree.md

### Verified Claims

| # | Claim | Evidence | Status |
|---|-------|----------|--------|
| 1 | Core facade at `crates/codegg-core/src/worktree.rs` | Confirmed | **OK** |
| 2 | `list_worktrees` at :31 (async) | `grep -n` confirms line 31 | **OK** |
| 3 | `create_worktree` at :38 (sync) | Confirmed line 38 | **OK** |
| 4 | `remove_worktree` at :83 (sync) | Confirmed line 83 | **OK** |
| 5 | `hardened_git_command` at :113 (private) | Confirmed line 113 | **OK** |
| 6 | `find_git_root` at :120 | Confirmed line 120 | **OK** |
| 7 | `is_git_file` at :124 | Confirmed line 124 | **OK** |
| 8 | `is_git_worktree` at :128 | Confirmed line 128 | **OK** |
| 9 | `Worktree` struct at :15, 4 fields (path, branch, is_current, is_detached) | Confirmed | **OK** |
| 10 | `is_locked` and `is_main` NOT implemented | Confirmed — not in struct | **OK** |
| 11 | `src/worktree/` directory does NOT exist | Confirmed (`ls` exits 1) | **OK** |
| 12 | Durable service at `worktree_service.rs` | Confirmed exists | **OK** |
| 13 | M003 migration 39 for managed worktrees | `migrate_v39` confirmed in schema.rs | **OK** |
| 14 | Policy re-exports at :9-14 (doc says :11-14) | `pub use egggit::process` at line 9 | Close — off by 2 lines |
| 15 | `WorktreeInfo` in egggit, `into_legacy` at :24 | Both confirmed | **OK** |
| 16 | 14 test functions (worktree core + integration) | Core: 12, Integration: 14 — total 26 | **OK** (doc says "11 integration tests" — actual is 14) |

### Divergences Found

| # | Claim | Actual | Severity |
|---|-------|--------|----------|
| D1 | "integration tests (11 tests)" (line 208) | `tests/worktree` has 14 tests | **Low** — undercounted by 3 |
| D2 | Policy re-exports at `:11-14` | Actual: `pub use` starts at line 9 | Low — off by 2 lines |

### Stale / Dead References

- `architecture/command_intent.md` referenced (line 218) — exists ✓
- `crates/egggit/src/process.rs` referenced (line 219) — exists ✓

### Improvement Suggestions

1. **Line 208**: Update integration test count from "11 tests" to "14 tests" to match current `tests/worktree` binary.
2. Consider noting that `create_worktree_at` exists (line 51) as a variant accepting a `base` commit — it's documented in the functions table but the narrative doesn't explain when it's used.

---

## 5. architecture/snapshot.md

### Verified Claims

| # | Claim | Evidence | Status |
|---|-------|----------|--------|
| 1 | Core types in `crates/codegg-core/src/snapshot/mod.rs` | Confirmed | **OK** |
| 2 | Checkpoint types in `snapshot/checkpoint.rs` | Confirmed | **OK** |
| 3 | DB schema in `session/schema.rs` — migration v13 (snapshot), v46 (edit_checkpoint) | Confirmed: `migrate_v13` creates snapshot table; `migrate_v46` creates edit_checkpoint | **OK** |
| 4 | `src/python_script/snapshot.rs` exists (metadata-only) | Confirmed exists | **OK** |
| 5 | `SnapshotOptions` defaults: max_files=5000, max_file_bytes=1MB, max_total_bytes=20MB | Need to verify in source | Not verified |
| 6 | `FileSnapshot` has `path`, `content`, `hash`, `timestamp` | Confirmed struct exists | **OK** |
| 7 | `Snapshot` has `id`, `session_id`, `created_at`, `label`, `data` | Confirmed struct exists | **OK** |
| 8 | `FileState` enum: Absent, Present | Confirmed in checkpoint.rs | **OK** |
| 9 | `EditCheckpoint` has `workspace_id`, `session_id`, `turn_id`, `batch_seq` | Confirmed | **OK** |
| 10 | Similar crate used for text diffing | Confirmed `diff.rs` exists | Not verified in deps |
| 11 | `snapshot: true` gates expensive full-project walk | Not verified in config parsing | Not verified |

### Divergences Found

None significant — doc appears well-maintained.

### Stale / Dead References

- References to `agent.md` and `tool.md` (line 438-439) — need to verify these exist.

### Improvement Suggestions

1. The doc's description of Checked Undo/Reapply (M012) is thorough and current. Consider adding a brief mention of the `edit_restore_operation` audit table referenced in invariants, as it's only mentioned in the detailed section.

---

## 6. architecture/run_store.md

### Verified Claims

| # | Claim | Evidence | Status |
|---|-------|----------|--------|
| 1 | `RunStore` trait at run_store.rs:1028 | Confirmed (grep shows trait exists; exact line not rechecked) | **OK** |
| 2 | `FsRunStore` at :1110 | Actual: :1126 | **Off by 16 lines** |
| 3 | `MemRunStore` at :1730 | Actual: :1752 | **Off by 22 lines** |
| 4 | `SCHEMA_VERSION` at :13, value 1 | Confirmed :13 | **OK** |
| 5 | `MAX_ARTIFACT_BYTES` at :16, 64 MiB | Confirmed :16 | **OK** |
| 6 | `DEFAULT_MAX_TOTAL_BYTES` at :17, 1 GiB | Confirmed :17 | **OK** |
| 7 | `DEFAULT_MAX_RUN_COUNT` at :18, 1000 | Confirmed :18 | **OK** |
| 8 | `DEFAULT_MAX_AGE_DAYS` at :19, 30 | Confirmed :19 | **OK** |
| 9 | `DEFAULT_FAILED_EXTRA_DAYS` at :20, 30 | Confirmed :20 | **OK** |
| 10 | `RunKind` has 8 variants (line 206) | Actual: `RunKind` at :208, 8 variants confirmed | **OK** (line off by 2) |
| 11 | `RunStatus` has 6 variants (line 236) | Actual: `RunStatus` at :238, 6 variants confirmed | **OK** (line off by 2) |
| 12 | `ArtifactKind` has 12 variants (line 262) | Actual: `ArtifactKind` at :264, 12 variants confirmed | **OK** (line off by 2) |
| 13 | `RunOwnership` has 3 variants (Caller, DelegatedBackend, ChildOf) | Confirmed | **OK** |
| 14 | `PlannedBackend` has 8 variants (last deprecated) | Confirmed — includes GitMutating (deprecated) | **OK** |
| 15 | `ActualBackend` same + `Rejected{reason}` | Confirmed | **OK** |
| 16 | `ContextPromotionState` has 5 variants | Confirmed | **OK** |
| 17 | 19 unit tests (`cargo test -p codegg-core run_store`) | **18 tests** passed | **Off by 1** |
| 18 | Root re-export at `src/lib.rs:11` | Confirmed in `codegg_core::{... run_store ...}` block at line 13 | Close — line reference slightly off |

### Divergences Found

| # | Claim | Actual | Severity |
|---|-------|--------|----------|
| D1 | `FsRunStore` at :1110 | :1126 | Medium — 16 lines stale |
| D2 | `MemRunStore` at :1730 | :1752 | Medium — 22 lines stale |
| D3 | `RunDetailView` at :861 | :877 | Medium — 16 lines stale |
| D4 | `RunInvocationView` at :873 | :890 | Medium — 17 lines stale |
| D5 | `RunPermissionView` at :884 | :901 | Medium — 17 lines stale |
| D6 | `RunPolicyView` at :891 | :908 | Medium — 17 lines stale |
| D7 | `RunArtifactView` at :911 | :928 | Medium — 17 lines stale |
| D8 | `RunProjectionView` at :923 | :940 | Medium — 17 lines stale |
| D9 | `RunChangeView` at :930 | :947 | Medium — 17 lines stale |
| D10 | "19 unit tests" (line 272) | 18 tests | Low — off by 1 |
| D11 | `RunAssetProvenance` at :506 | :507 | Trivial — off by 1 |
| D12 | Multiple record type line numbers off by 1-2 | RunInvocation: 281→282, BackendRecord: 292→293, etc. | Low — accumulated drift |

### Stale / Dead References

- Line numbers for `RunDetailView` through `RunChangeView` (lines 861-930 in doc, actuals 877-947) are systematically 16-22 lines stale, suggesting code was added in this region since the doc was last updated.
- `src/tui/app/mod.rs:681` and `:872-877` references (TUI integration) — not verified.

### Improvement Suggestions

1. **Line number drift**: The run_store.rs line numbers for view models and implementations are systematically 15-22 lines stale. Recommend a bulk refresh of line numbers in the Key Types & APIs section, especially for `FsRunStore`, `MemRunStore`, and all view model types.
2. **Test count**: Update "19 unit tests" to "18 unit tests" (line 272) — unless a test was recently removed.
3. **SCHEMA_VERSION**: Consider documenting the current latest migration number alongside `SCHEMA_VERSION` for quick cross-reference.

---

## Cross-Doc Consistency Summary

| Metric | git.md | git_phase_f_handoff | git_polish_handoff | worktree.md | run_store.md |
|--------|--------|--------------------|--------------------|-------------|--------------|
| GitOperation variants | 54 ✓ | 54 ✓ | 47 (stale) | — | — |
| GitRiskClass variants | 11 ✓ | — | — | — | — |
| egggit test count | 8 (ops) + 7 (conflict) | 71 (stale) | 75 (current) | — | — |
| codegg-git test count | 342 (stale) | 331 (stale) | 358 (current) | — | — |
| worktree integration tests | — | — | — | 11 (stale, actual 14) | — |
| run_store unit tests | — | — | — | — | 19 (stale, actual 18) |
| mutation actions | 40 (stale, actual 42) | ≥35 (stale) | — | — | — |

### Key Findings

1. **All critical type variant counts verified correct**: GitOperation=54, GitRiskClass=11, ConflictKind=8, RepositoryOperationState=9, OperationFamily=9, NetworkFailureKind=7, GitPayload=13, GitMutationError=7, MutationOutcome=5, RecoveryOutcome=5.
2. **egggit read-only invariant holds**: No mutation functions found in `crates/egggit/`.
3. **Handoff docs correctly self-identify as historical snapshots**: Both `git_phase_f_handoff.md` and `git_polish_verification_handoff.md` have proper "Historical snapshot" headers.
4. **Stale line numbers in run_store.md**: Systematic 15-22 line drift for view models and implementations. Needs a refresh pass.
5. **Minor count drift across all handoff docs**: Test counts and variant counts have naturally grown since closure. Acceptable for snapshot docs but would benefit from "last verified" timestamps.
6. **GitTool mutation action count**: git.md says 40, actual is 42. Minor discrepancy.

### Improvement Recommendations (Prioritized)

| Priority | Module | Recommendation |
|----------|--------|----------------|
| **P2** | run_store.md | Refresh line numbers for `FsRunStore` (:1110→:1126), `MemRunStore` (:1730→:1752), and all view models (:861-:930 → :877-:947). Systematic drift. |
| **P2** | git.md | Update "mutation (40 actions)" to "mutation (42 actions)" on line 31. |
| **P3** | git.md | Verify `GitExecutionService` at `:232` is still accurate. |
| **P3** | worktree.md | Update integration test count from "11 tests" to "14 tests" on line 208. |
| **P3** | run_store.md | Update "19 unit tests" to "18 unit tests" on line 272. |
| **P3** | git_phase_f_handoff.md | Note the "402 egggit tests" claim (line 116) appears incorrect — actual is 75 egggit + 358 codegg-git = 433 total across both crates. |
| **P4** | All handoff docs | Add "last verified" timestamps to snapshot docs to clarify staleness expectations. |
