# Review: batch4 git-persistence
**Reviewed**: 2026-09-11
**Files**: architecture/git.md, architecture/git_phase_f_handoff.md, architecture/git_polish_verification_handoff.md, architecture/session.md, architecture/storage.md, architecture/snapshot.md, architecture/worktree.md, architecture/run_store.md

## Summary

Reviewed 8 architecture docs against source code. Found 27 documentation issues total: 7 HIGH (wrong counts, wrong file paths, phantom line numbers), 13 MEDIUM (stale state in handoff docs, outdated variant counts, inaccurate column counts), 7 LOW (nit line-number drift, minor inaccuracies). No genuine code bugs found.

## Documentation Issues

| # | File | Line | Issue | Severity | Suggested fix |
|---|------|------|-------|----------|---------------|
| 1 | git.md | 39 | `GitPayload (12 variants)` — actual count is 13 (includes `None` variant) | HIGH | Change "12" to "13" |
| 2 | git.md | 204 | `render_argv` file listed as `render_argv.rs` — actual file is `render.rs` | HIGH | Change to `render.rs` |
| 3 | git.md | 229 | `GitExecutionService` at `git_service.rs:229` — actual is `:232` | LOW | Update to `:232` |
| 4 | git.md | 231 | `GitMutationExecutor` at `git_mutations.rs:703` — actual is `:471` | HIGH | Update to `:471` |
| 5 | git.md | 233 | `RepoSnapshot` at `git_mutations.rs:246` — actual is `codegg-git/src/workflow.rs:15` | HIGH | Correct file and line |
| 6 | git.md | 234 | `StateDelta` at `git_mutations.rs:278` — actual is `codegg-git/src/workflow.rs:30` | HIGH | Correct file and line |
| 7 | git.md | 235 | `MutationOutcome` at `git_mutations.rs:324` — actual is `codegg-git/src/workflow.rs:64` | HIGH | Correct file and line; also note the doc says "5 variants" which is correct |
| 8 | git.md | 236 | `MutationResult` at `git_mutations.rs:352` — actual is `codegg-git/src/workflow.rs:86` | MEDIUM | Correct file and line |
| 9 | git.md | 272 | "Canonical source of truth: `crates/codegg-git/src/process_policy.rs`" — that file re-exports from `egggit::process`; canonical source is `crates/egggit/src/process.rs` | MEDIUM | Clarify that `process_policy.rs` is a re-export shim; canonical data lives in `egggit::process` |
| 10 | git.md | 32 | Table row says `ALLOWED_ENV_VARS (21 entries)` at `process_policy.rs` — the re-export is correct but the file description should say "re-exports from egggit::process" | LOW | Add "(re-export)" or note the delegation |
| 11 | git.md | 237 | `GitMutationError (7 variants)` — verified correct (7 variants) | OK | No change needed |
| 12 | session.md | 53 | "52+ CREATE TABLE statements" — actual count is 71 | HIGH | Update to "71" |
| 13 | session.md | 109 | "22 session columns" — SESSION_COLUMNS constant has 27 columns | HIGH | Update to "27" |
| 14 | session.md | 150 | `TuiSessionState` at `session/state.rs:111` — actual is `:112` | LOW | Update to `:112` |
| 15 | worktree.md | 142 | `Worktree (codegg-core:16)` — actual struct is at `:15` | LOW | Update to `:15` |
| 16 | worktree.md | 155 | `WorktreeInfo (egggit:5)` — actual is at `:6` | LOW | Update to `:6` |
| 17 | worktree.md | 163 | `list_worktrees` at `:33` — actual is at `:31` | LOW | Update to `:31` |
| 18 | worktree.md | 164 | `create_worktree` at `:40` — actual is at `:38` | LOW | Update to `:38` |
| 19 | worktree.md | 166 | `remove_worktree` at `:68` — actual is at `:83` | MEDIUM | Update to `:83` |
| 20 | worktree.md | 170 | `hardened_git_command` at `:98` — actual is at `:113` | MEDIUM | Update to `:113` |
| 21 | storage.md | 39 | `STORAGE_LAYOUT_VERSION` at `storage/mod.rs:39` — verified correct at `:39` | OK | No change needed |
| 22 | run_store.md | 87 | `ActualBackend` has "(9)" variants — actual count is 8 (no `Unrouted` variant) | MEDIUM | Update to "8" |
| 23 | run_store.md | 86 | `PlannedBackend` at `:111` — actual is at `:113` | LOW | Update to `:113` |
| 24 | git_phase_f_handoff.md | 111 | `--all-features` in test command — contradicts AGENTS.md guidance to never use `--all-features` for workspace sweeps | MEDIUM | Change to `--features server,plugins,lsp-test-support` per AGENTS.md |
| 25 | git_polish_verification_handoff.md | 54-57 | egggit description says "no subprocess mutations" — egggit does spawn `Command::new("git")` for reads (in `process.rs` and test code), but correctly does not perform mutations | MEDIUM | Clarify "no subprocess mutations" vs "spawns git for reads" to avoid confusion |
| 26 | git.md | 341 | Duplicate numbering: two items numbered "12" (lines 333 and 341) | MEDIUM | Renumber items 13 and 14 |
| 27 | snapshot.md | 146 | `SnapshotManager (:52)` — actual struct is at `:56` | MEDIUM | Update line reference |

## Code Issues Found

No genuine code bugs found during this review.

## Improvement Opportunities

1. **git.md**: The "Key Types & APIs" table at line 198 conflates types defined in `codegg-git` with types re-exported from `codegg-git` into root crate modules. Consider splitting into "defined in" vs "re-exported from" columns, or moving the root-crate types to a separate table, to avoid the current confusion where `RepoSnapshot`/`StateDelta`/`MutationResult`/`MutationOutcome` appear to be defined in `git_mutations.rs` but are actually from `codegg-git::workflow`.

2. **session.md**: The "52+ CREATE TABLE" claim has drifted significantly (actual: 71). Consider adding a migration count summary that tracks the highest migration number and a table count, so the doc stays accurate as new migrations are added. The doc could reference `STORAGE_LAYOUT_VERSION` and the `check_project_catalog_invariants.py` script for automated verification.

3. **worktree.md**: The line-number references for public API functions drift by 2-15 lines. Since these are frequently-changing files, consider replacing precise line numbers with just file paths (e.g., `create_worktree` in `crates/codegg-core/src/worktree.rs`) to reduce maintenance burden.

## Stale Content to Prune

| # | File | Line(s) | Stale content | Notes |
|---|------|---------|---------------|-------|
| 1 | git_phase_f_handoff.md | 5 | References commit `08709d3` and branch `main` | Historical record — acceptable as-is but could note it's a snapshot |
| 2 | git_phase_f_handoff.md | 111 | `--all-features` test command | Should be updated to match AGENTS.md convention (`--features server,plugins,lsp-test-support`) |
| 3 | git_polish_verification_handoff.md | 252-257 | Repeated "Execution-origin matrix" and "Drift guards" sections (duplicated from lines 235-249) | Remove duplicate sections |
| 4 | git_polish_verification_handoff.md | 168 | TUI `RunRerun` placeholder at `src/tui/app/mod.rs:3615` — line number may have shifted | Verify current line number or remove line-specific reference |
| 5 | git.md | 333-344 | Duplicate numbering (two items numbered "12") | Renumber to 13 and 14 |
