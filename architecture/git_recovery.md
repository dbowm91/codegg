# Git Recovery

In-progress operation detection and recovery (continue/abort/skip).

## Purpose

`src/git_recovery.rs` implements Phase F of the Git architecture. It inspects `RepositoryOperationState` from `egggit` and dispatches the correct recovery subcommand (merge, rebase, cherry-pick, revert, sequencer), preventing cross-operation misuse.

## Key Types

### RecoveryOutcome

| Variant | Meaning |
|---------|---------|
| `Completed` | Operation finished successfully |
| `StillInProgress` | Operation still in progress after action |
| `Conflicted` | Operation has unresolved conflicts |
| `NoOp` | No operation in progress |
| `Rejected(String)` | Action rejected with reason |

## Operations

### continue_in_progress(exec, repo_root)

Dispatches `--continue` for the detected operation family:

| Detected Operation | Command |
|-------------------|---------|
| Merge | `git merge --continue` |
| Rebase | `git rebase --continue` |
| Cherry-pick | `git cherry-pick --continue` |
| Revert | `git revert --continue` |

### abort_in_progress_typed(exec, repo_root)

Dispatches `--abort` for the detected operation family.

### skip_in_progress(exec, repo_root)

Dispatches `--skip` for the detected operation family.

## Cross-Operation Protection

The recovery system prevents misuse by detecting the current operation state before dispatching. If no operation is in progress, or if the requested action doesn't match the detected operation, it returns `RecoveryOutcome::NoOp` or `RecoveryOutcome::Rejected`.

## Operation State Detection

Delegates to `egggit::RepositoryOperationState` which inspects:
- `.git/MERGE_HEAD` — merge in progress
- `.git/rebase-merge/` or `.git/rebase-apply/` — rebase in progress
- `.git/CHERRY_PICK_HEAD` — cherry-pick in progress
- `.git/REVERT_HEAD` — revert in progress
- `.git/sequencer/` — sequencer in progress

## See Also

- [Git](git.md) — Full Git module overview
- [Git Mutations](git_mutations.md) — Local mutation operations
- [Git Network](git_network.md) — Network operations
- [egggit](https://github.com/...) — Read-only git facts crate
