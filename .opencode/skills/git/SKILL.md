---
name: git
description: Typed Git operations, risk classification, guarded mutations, and recovery in codegg
version: 1.0.0
tags:
  - git
  - mutations
  - risk
  - recovery
---

# Git Subsystem Guide

Operational guide for changing Git code safely. The full module contract
lives in `architecture/git.md`; this skill covers the ownership boundaries
and invariants that are easy to violate.

## Ownership Map

| Layer | Location | Role |
|-------|----------|------|
| Typed workflow model | `crates/codegg-git/` | `GitOperation` (54 variants), `GitRiskClass` (11 classes), argv parser/renderer, `RepoPath`/`RefName` safety types, credential redaction |
| Read-only git facts | `crates/egggit/` | Structured status/diff/log/blame/refs/worktree/conflict/operation-state reads; canonical subprocess env policy |
| Read executor | `src/git_service.rs` | `GitExecutionService` — maps typed ops to egggit reads and the process boundary |
| Mutations | `src/git_mutations.rs`, `src/git_mutations_ops.rs` | `GitMutationExecutor`, snapshot/delta capture, stage/commit/branch/restore/stash/merge/rebase/cherry-pick/revert/tag ops |
| Contained worktree creation | `src/git_contained_worktree.rs` | The ADR-0048 route: `git worktree add --detach <path> <commit>` followed by `git symbolic-ref HEAD refs/heads/<branch>`, both inside an OS-contained child whose writable grant is the **shared** administration root `<common-gitdir>/worktrees/` (plus the new checkout and the daemon scratch). The child's argv pins `core.hooksPath`, so the repository's own hooks cannot run inside the containment — a defence in depth that does not narrow the grant |
| Network | `src/git_network_ops.rs`, `src/git_network_policy.rs` | Fetch/pull/push/remote/config/reset/clean; env hardening; URL credential redaction |
| Recovery | `src/git_recovery.rs` | `continue_in_progress`, `abort_in_progress_typed`, `skip_in_progress`, `assert_action_matches` |
| Projection | `src/git_mutation_projector.rs` | `project_mutation`, `project_network_mutation`, `project_destructive_mutation`, `project_recovery` |
| Persistence | `src/git_run_store.rs` | `persist_mutation`, `persist_recovery` (argv always sanitized) |

## Hard Rules

1. **`egggit` never mutates.** All mutations live in `src/git_mutations*.rs`
   (+ network/config policy in `src/git_network_policy.rs`). New write paths
   must go through `GitMutationExecutor` with snapshot/delta capture, never
   direct `egggit` or raw `git` spawns.
2. **Single parser truth.** `parse_git_argv` / `render_argv` in `codegg-git`
   are the only argv parser/renderer. No duplicate parsing downstream.
3. **Credentials never leak.** Raw URLs cross into a child only via
   `expose_secret()` at the `render_argv` boundary. `Debug`/`Serialize`/
   `Display` on URL types stay redacted (`RedactedUrl`); persisted argv goes
   through `sanitize_argv_for_run_store`; result text through
   `sanitize_truncate_for_result`.
4. **Destructive ops stay denied by default.** Force-push classifies
   `DestructiveHistory` (`operation.rs:441`), which the command-intent
   classifier turns into capability `DestructiveFileMutation` with default
   `Deny` (`src/command_intent/mod.rs:581`); `clean -f` classifies
   `DestructiveWorktree` the same way. Only a *broad* clean is refused
   outright — `CleanRequest::is_broad()` (`src/git_network_ops.rs:731`) is
   `ignored && paths.is_empty()`, rejected at the tool dispatch layer
   (`src/tool/git.rs:978`). Dangerous `git config` keys are denied by
   `CONFIG_DENIED_KEY_PATTERNS` (`src/git_network_ops.rs:467`):
   `credential.*`, `http.*`, `url.*`, `core.gitProxy`, `core.sshCommand`,
   `core.sshVariant`. Merge strategies are allowlisted
   (`src/git_mutations_ops.rs:421`).
5. **Recovery re-reads state.** `assert_action_matches` re-reads operation
   state from disk immediately before acting (TOCTOU defense); mismatch is
   `GitMutationError::StateMismatch`. Recovery is never auto-resolved:
   conflicts surface as typed data, the agent edits + stages, then
   `recover: continue`.
6. **Shell stays shell.** Argv with pipes, redirects, substitution,
   semicolons, or env assignments is NOT rewritten to the Git backend
   (`ActualBackend::RawShell`).
7. **Tool Programs get reads only.** Programs cannot call the multiplexed
   `git` tool (`DirectOnly`); the hidden `ProgrammaticOnly` `git_read`
   adapter exposes `status`/`diff`/`log`/`branches` with a 64 KiB cap.
8. **Timeout cleanup is process-group scoped.** Unix children that may be
   killed as a tree must become their own process-group leaders before spawn;
   otherwise `getpgid(child)` can identify the caller's group and cleanup can
   kill the caller.

## Static Guards

```bash
python3 scripts/check_git_forbidden_patterns.py  # secret boundary + policy drift
```

Run after touching any git file. It enforces `expose_secret()` discipline
and rejects duplicate env-policy copies (`ALLOWED_ENV_VARS` /
`GitEnvPolicy` live canonically in `egggit::process`).

## Testing

```bash
cargo test -p codegg-git          # parser, operation, risk, path, render
cargo test -p egggit              # structured reads, operation state, conflicts
cargo test -p codegg git_service  # read executor
cargo test -p codegg git_mutations
cargo test -p codegg git_network
cargo test -p codegg git_recovery
cargo test --test command_git -- git_mutations_integration
cargo test --test command_git -- git_network_integration
cargo test --test command_git -- git_recovery_integration
```

The root package's integration tests are **families** — one cargo executable
per subject area, under `tests/<family>/`. `cargo test --test <member>` names
no target, selects zero tests, and still exits 0, so it is a silent no-op.
The git members live in the `command_git` family: the correct form is
`cargo test --test command_git -- <member>`. A test's full path is
`family::member::test_name`, which is what `--exact` matches against.

## See Also

- `architecture/git.md` — authoritative contract
- `.opencode/skills/scheduler/SKILL.md` — git domains are deferred-domain
  executors in `docs/execution-ownership.toml`, not scheduler executors yet
- `.opencode/skills/human-shell/SKILL.md` — `!`/`!!` shell boundary (raw
  shell is not the typed git path)

## Source verification

Verified 2026-10-23 against `crates/codegg-git/src/{operation,risk,parser,render}.rs`,
`crates/egggit/src/process.rs`, `src/git_service.rs`,
`src/git_mutations.rs`, `src/git_mutations_ops.rs`, `src/git_network_ops.rs`,
`src/git_network_policy.rs`, `src/git_recovery.rs`,
`src/git_mutation_projector.rs`, `src/git_run_store.rs`,
`src/git_contained_worktree.rs`, `src/tool/git.rs`, `src/tool/git_read.rs`,
`src/command_intent/mod.rs`, `docs/execution-ownership.toml`, and
`scripts/check_git_forbidden_patterns.py`.

Confirmed: `GitOperation` / `GitRiskClass` (`crates/codegg-git/src/operation.rs:10`,
`risk.rs:8`) with 54 and 11 variants respectively, matching the authoritative
counts table; `parse_git_argv` / `render_argv` (`parser.rs:11`, `render.rs:9`)
as the only argv parser/renderer; `GitExecutionService` (`src/git_service.rs:282`);
`GitMutationExecutor`; the ADR-0048 route's `git worktree add --detach` plus
`git symbolic-ref HEAD refs/heads/<branch>` (`src/git_contained_worktree.rs:141-145,690-713`,
`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`);
the recovery entry points (`continue_in_progress` `:49`,
`abort_in_progress_typed` `:111`, `skip_in_progress` `:169`,
`assert_action_matches` `:228` in `src/git_recovery.rs`); the projector
functions (`src/git_mutation_projector.rs:27,153,174,189`); `persist_mutation`
/ `persist_recovery` (`src/git_run_store.rs:40,248`); `ALLOWED_ENV_VARS` and
`GitEnvPolicy` living canonically in `egggit::process`
(`crates/egggit/src/process.rs:33,157`); and the `git_read` adapter's 64 KiB
cap (`src/tool/git_read.rs:49`).

Corrected: the broad-clean rejection was cited at `src/tool/git.rs:794` — that
line is a `restore_worktree` dispatch arm; the rejection is at
`src/tool/git.rs:978`. `CleanRequest::is_broad()` is at
`src/git_network_ops.rs:731` (was `:728`); `CONFIG_DENIED_KEY_PATTERNS` at
`:467` (was `:463`); `ALLOWED_MERGE_STRATEGIES` at
`src/git_mutations_ops.rs:421` (was `:419`); the
`DestructiveFileMutation` capability at `src/command_intent/mod.rs:581`
(was `:579`). The three integration-test commands named targets that do not
exist — the members are `git_mutations_integration.inc`,
`git_network_integration.inc`, and `git_recovery_integration.inc` inside the
`command_git` family, so `cargo test --test <member>` selected zero tests and
exited 0; they now use `cargo test --test command_git -- <member>`.

