# Phase F Handoff: Git Agent Integration Closure

## Status

Phase F complete. Commit `08709d3` — `feat: Phase F — conflicts, recovery, ergonomics, and closure`. Branch: `main`.

### Corrective security closure (post-merge)

Two Phase F security-review findings were resolved post-merge in commit `cb192e9` — `feat: Phase F corrective security closure — RedactedUrl + hardened env policy`:

1. **`remote_set_url` credential leakage** — `GitOperation::RemoteAdd.url` and `GitOperation::RemoteSetUrl.url` are now typed as `codegg_git::RedactedUrl` (a newtype carrying both raw and redacted forms). `Debug`/`Display`/`Serialize` see only the redacted form; raw is reachable exclusively via `RedactedUrl::expose_secret()` consumed at `render_argv`. Defense-in-depth sanitizers (`redact_url_credentials_in_text`, `sanitize_argv_for_run_store`, `sanitize_truncate_for_result`) keep credential leaks out of `MutationResult.stdout/stderr` and RunStore artifacts.

2. **Raw fallback missing hardened env policy** — Every Codegg-owned `git` subprocess now flows through `GitEnvPolicy::apply()` (tokio async) or the new `GitEnvPolicy::apply_sync()` (synchronous TUI probes). The policy's default includes `strip_command_bearers = true`, which removes 27 command-bearing vars (`GIT_ASKPASS`, `GIT_SSH_COMMAND`, `GIT_PROXY_COMMAND`, all `GIT_CONFIG_*` injection vectors, `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_PAGER`, `PAGER`, etc.). Affected callers: `src/tool/git.rs::run_raw_subcommand`, `src/git_service.rs::run_git_raw`, `src/tool/commit.rs::fetch_head_message`, `src/core/daemon.rs::SnapshotWorkspace`, the TUI `handle_diff_command` / `handle_revert_command`, and `crates/codegg-core/src/worktree.rs` (local mirror, since `codegg-core` cannot depend on root-crate helpers).

See `architecture/git.md` "Phase F corrective security closure" subsection for the full fix description, and `docs/validation/git-security-review.md` "Resolutions (Phase F Closure)" section for the resolution notes per finding.

## Final Architecture and Crate Boundaries

```
crates/egggit          read-only git facts (status_v2, diff, log, blame, refs,
                       operation_state, conflict) — no subprocess mutations
crates/codegg-git      typed Git operation model (GitOperation, parse_git_argv,
                       render_argv), risk classification (GitRiskClass, RiskSet),
                       path/ref safety types — pure data, no TUI/provider/Bash deps

src/git_mutations.rs       mutation executor: snapshots, env hardening, RunStore
src/git_mutations_ops.rs   typed helpers (stage_paths, commit, branch_create, …)
src/git_network_policy.rs  env clearing for network ops, failure classification, URL redaction
src/git_network_ops.rs     fetch/pull/push/remote/config/clean typed helpers
src/git_recovery.rs        operation-aware continue/abort/skip with cross-op guards
src/git_mutation_projector.rs  structured projection of mutation/recovery outcomes
src/git_run_store.rs       RunStore persistence with backend.detail provenance
src/git_service.rs         GitExecutionService: unified executor, delegates reads to egggit
src/tool/git.rs            model-facing git tool with mutation/recover/operation_state schema
```

## Supported Typed Operation Matrix

`mutation` enum entries from `src/tool/git.rs` (≥35 entries):

| Family | Actions | Risk |
|--------|---------|------|
| Staging | `stage_paths`, `stage_all`, `stage_tracked`, `unstage_paths`, `unstage_all` | IndexMutation |
| Commit | `commit`, `commit_amend` | IndexMutation |
| Branch | `branch_create`, `branch_switch`, `branch_create_and_switch`, `branch_delete`, `detach` | RefMutation |
| Restore | `restore_worktree`, `restore_staged`, `restore_both` | WorktreeMutation |
| Stash | `stash_push`, `stash_apply`, `stash_pop`, `stash_drop` | IndexMutation / WorktreeMutation |
| Merge/Rebase | `merge`, `rebase`, `cherry_pick`, `revert`, `abort` | HistoryIntegration |
| Network | `fetch`, `pull`, `push` | NetworkRead / NetworkWrite |
| Remote | `remote_add`, `remote_remove`, `remote_set_url`, `remote_rename` | RepositoryConfigMutation |
| Config | `config_get`, `config_set`, `config_unset` | RepositoryConfigMutation |
| Reset | `reset_soft`, `reset_mixed`, `reset_hard`, `reset_merge`, `reset_keep`, `reset_paths` | IndexMutation / DestructiveWorktree / DestructiveHistory |
| Clean | `clean_preview`, `clean` | DestructiveWorktree |

`recover` enum: `continue`, `abort`, `skip` — operation-aware, refuses cross-operation misuse.

`operation_state`: boolean probe returning typed active operation + conflicted paths + available actions.

## Permission Defaults

From `src/command_intent/plan.rs::generate_permission_requests`:

| Capability | Default | Risk | Notes |
|-----------|---------|------|-------|
| `GitMutation` | **Allow** | Medium | `git add` only (`is_safe_git_subcommand`) |
| `GitMutation` | **Ask** | Medium | All other subcommands |
| `DestructiveFileMutation` | **Deny** | High | `reset --hard`, `clean -f` etc. |
| `Network` | **Ask** | Medium | fetch/pull/push |
| `OutsideWorkspace` | **Deny** | High | — |
| `ReadWorkspace` | **Allow** | Safe | — |
| `WriteWorkspace` | **Ask** | Medium | Writing formatters |
| `DependencyInstall` | **Deny** | Medium | — |
| `Subprocess` | **Allow** | Low | — |

Tool-level typed mutations bypass intent routing and go directly through `GitMutationExecutor` with RunStore persistence (`RunKind::GitMutation`, `PlannedBackend::Git`, `ActualBackend::Git`, `RunOwnership::DelegatedBackend`).

## Managed/Raw Fallback Matrix

| Operation | Typed Path | Raw Fallback |
|-----------|-----------|--------------|
| All typed mutations | `GitMutationExecutor` via `git_mutations_ops` | n/a |
| Read-only (status, diff, log, blame, refs) | `egggit` structured parsing | `GitExecutionService` subprocess |
| BashTool `git *` | `classify_git()` → `ExecutionBackend::Git` | Raw shell when validation fails |
| Unknown subcommands | `ManagedGitArgv` fallback with conservative risk | `RawShellRequired` for shell syntax |

## Known Limitations

- **Bisect recovery**: not supported — `git bisect` has no `--continue`/`--abort`/`--skip`; agent must drive manually
- **Apply-mailbox recovery**: not supported — `git am` recovery is caller-driven
- **Sequencer**: detected but limited to state read; no typed continue/abort via Codegg
- **Submodule mutations**: not delegated to submodule repos
- **Worktree ergonomics**: `worktree create`/`move`/`remove` are read-only listings; full management deferred
- **Force-with-lease**: requires explicit `force_with_lease = <old_sha>` parameter; no auto-detection
- **Wide ref patterns**: `git push origin '*'` may bypass per-ref validation
- **Config scope**: `global` scope writes rejected at tool layer — those belong outside the repo boundary
- **Rename remote**: falls back to `ManagedGitArgv` (not in typed parser)

## Test Coverage and Commands

```bash
# Phase F focused
cargo test -p egggit                                          # 71 tests
cargo test -p codegg-git                                      # 331 tests
cargo test -p codegg --lib git_mutation_projector             # 10 tests
cargo test -p codegg --lib git_recovery                       # 4 tests
cargo test -p codegg --lib tool::git                          # 6 schema tests
cargo test --test git_recovery_integration                    # 19 tests
cargo test --test git_closure_matrix                          # 32 tests

# Full suite (capped)
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=8
```

**Summary**: 402 egggit tests, 331 codegg-git tests, 19+32 integration/closure tests, 10 projector tests, 6 schema tests. 9 golden fixture files for projection regression.

## Migration/Deprecation Status

| Item | Status |
|------|--------|
| `ExecutionBackend::GitMutating` | **REMOVED** (Phase F closure) |
| `PlannedBackend::GitMutating` | **DEPRECATED** — serialization compat only; use `PlannedBackend::Git` |
| `ActualBackend::GitMutating` | **DEPRECATED** — serialization compat only; use `ActualBackend::Git` |
| `CommandIntentKind::GitMutating` | **RETAINED** for classification output; routing uses `Git` |
| `git_mutations_ops::abort_in_progress` | **DEPRECATED** shim — delegates to `git_recovery::abort_in_progress_typed` |
| Legacy `OperationState` | **RE-EXPORTED** for back-compat; `RepositoryOperationState` is canonical |

`grep -n "GitMutating" src/` returns only the deprecated markers. No active code paths use the old variants.

## Recommended Future Work

1. **PR hosting-provider integration** (GitHub, GitLab, Bitbucket) — out of scope for Phase F
2. **Isolated worktree workflows** for agent-driven changes — defer
3. **Deterministic hunk staging** — defer
4. **Submodule-aware operations** (cross-submodule mutation, `git submodule update --remote`) — defer
5. **Bisect automation hooks** — defer
6. **Worktree create/move/remove** as typed mutations — defer
7. **Interactive rebase** support — defer (currently non-interactive only)

## References

- `architecture/git.md` — Phase A–F sections (authoritative)
- `architecture/command_intent.md` — classification and routing
- `architecture/command_planner.md` — backend routing, permission generation
- `architecture/command_routing.md` — routing resolution
- `docs/validation/git-security-review.md`
- `docs/validation/git-performance-review.md`
- `docs/validation/git-cross-platform.md`
- Git agent integration Phase F conflicts and closure (plan pruned post-completion)
