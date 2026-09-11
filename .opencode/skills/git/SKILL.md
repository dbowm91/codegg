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
   `DestructiveHistory` (command-intent capability `DestructiveFileMutation`,
   default `Deny`); broad `git clean` is rejected at dispatch; dangerous
   `git config` keys (`credential.*`, `http.*`, `url.*`, proxy/ssh vectors)
   are always denied; merge strategies are allowlisted.
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
cargo test --test git_mutations_integration
cargo test --test git_network_integration
cargo test --test git_recovery_integration
```

## See Also

- [architecture/git.md](../../architecture/git.md) — authoritative contract
- `.skills/scheduler/SKILL.md` — git domains are deferred-domain executors
  in `docs/execution-ownership.toml`, not scheduler executors yet
- `.skills/human-shell/SKILL.md` — `!`/`!!` shell boundary (raw shell is
  not the typed git path)
