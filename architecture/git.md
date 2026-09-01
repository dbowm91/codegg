# Git Subsystem — Typed Operations, Structured Execution, and Recovery

The Git subsystem spans three crates and a dozen root modules, providing a
typed vocabulary for Git commands (codegg-git), read-only structured parsing
(egggit), and a mutation/recovery/network execution framework in the root
crate. It serves the command-intent classifier, command planner, BashTool
dispatch, the native Git tool, provenance tracking, TUI sidebar, and
RunStore persistence.

## Purpose

Provide a single source of truth for Git argv parsing, risk classification,
structured read execution, typed mutation execution with snapshot/delta
capture, network/destructive operation support, and operation-aware
conflict recovery — all without exposing credentials through display,
logging, or persistence surfaces.

## Where It Lives

| Layer | Crate / Module | Role |
|-------|----------------|------|
| Data model | `crates/codegg-git/` | `GitOperation` enum (54 variants), `GitRiskClass` (11 variants), `parse_git_argv`, `render_argv`, path/ref safety types, `RedactedUrl`, `AuditSafeArgv`, canonical env-policy tables |
| Structured reads | `crates/egggit/` | `status_v2` (rich status), `diff`, `log`, `blame`, `refs`, `worktree`, `conflict`, `operation_state` — all read-only, async, subprocess-based |
| Execution | `src/git_service.rs` | `GitExecutionService` — unified executor delegating reads to egggit, mutations to subprocess fallback |
| Mutations | `src/git_mutations.rs` | `GitEnvPolicy`, `GitMutationExecutor`, `RepoSnapshot`, `StateDelta`, `MutationOutcome`, `MutationResult` |
| Typed ops | `src/git_mutations_ops.rs` | Stage, commit, branch, restore, stash, merge, rebase, cherry-pick, revert, tag-delete |
| Network | `src/git_network_ops.rs` | Fetch, pull, push, remote add/remove/set-url/rename, config get/set/unset, reset variants, clean |
| Network policy | `src/git_network_policy.rs` | `NetworkEnvPolicy`, `NETWORK_ALLOWED_ENV_VARS`, `NetworkFailureKind`, URL credential redaction, `sanitize_argv_for_run_store` |
| Recovery | `src/git_recovery.rs` | `continue_in_progress`, `abort_in_progress_typed`, `skip_in_progress`, `assert_action_matches` |
| Projector | `src/git_mutation_projector.rs` | `project_mutation`, `project_network_mutation`, `project_destructive_mutation`, `project_recovery` |
| RunStore | `src/git_run_store.rs` | `persist_mutation`, `persist_recovery` |
| Tool surface | `src/tool/git.rs` | `GitTool` with `subcommand`, `mutation` (40 actions), `recover`, `operation_state` |
| Canonical env policy | `crates/codegg-git/src/process_policy.rs` | `ALLOWED_ENV_VARS` (21), `ALWAYS_STRIPPED_ENV_VARS` (28) — single source of truth |
| Managed worktree lifecycle | `crates/codegg-core/src/worktree_service.rs` | Durable worktree identity/lease state over hardened add/remove helpers |

## How It Works

### Read path

`GitExecutionService::execute` accepts a typed `GitOperation` and repository
root. Read-only operations (`Status`, `Diff`, `DiffStaged`, `Log`, `Blame`,
`ChangedFiles`, `BranchList`, `TagList`, `RemoteList`, `RemoteGetUrl`,
`WorktreeList`, `StashList`, `Show`) delegate to egggit functions for
structured parsing, falling back to raw subprocess if egggit fails.
All other operations fall through to `execute_raw` (subprocess via
`render_argv`).

Every `run_git_raw` call flows through `GitEnvPolicy::apply()` (env-clear,
ALLOWED_ENV_VARS restore, ALWAYS_STRIPPED_ENV_VARS strip, editor/pager
pinning, kill_on_drop). stdout/stderr pass through
`redact_url_credentials_in_text` before reaching any downstream consumer.

Returns `GitExecutionResult` with `GitPayload` (status, diff, log, branches,
tags, remotes, worktrees, stashes, show, etc.), raw stdout/stderr,
exit code, and `ProjectionHints`.

### Mutation path

`GitMutationExecutor::execute` runs the full 8-step pipeline:

1. Resolve and policy-check repository root (`resolve_repo_root`).
2. Capture pre-operation `RepoSnapshot` via `git status --porcelain=v2 -z
   --branch`.
3. Render argv via `codegg_git::render_argv` (no shell).
4. Execute with timeout and `GitEnvPolicy` hardening.
5. Capture post-operation snapshot (even on nonzero exit where safe).
6. Classify outcome (`Completed`, `NoOp`, `FastForward`, `Conflict`,
   `Rejected`) from before/after snapshots and exit code.
7. Compute `StateDelta` (commits created, refs created/deleted, paths
   staged/unstaged, conflicts).
8. Sanitize and truncate stdout/stderr (64 KiB max, URL credentials
   redacted) into `MutationResult`.

Typed helpers in `git_mutations_ops.rs` wrap the executor with
operation-specific validation (path validation as `RepoPath`, branch/tag
names as `BranchName`/`RefName`). Merge strategies are gated by
`ALLOWED_MERGE_STRATEGIES`. Rebase refuses interactive mode.

`run_raw_mutation` handles variants the typed parser cannot model (e.g.
`git add -A`, `git reset HEAD`) — same snapshot/policy pipeline.

### Network path

Network operations use `NetworkEnvPolicy::apply_to_command`, which extends
the base `GitEnvPolicy` by restoring `NETWORK_ALLOWED_ENV_VARS` (credential
helpers, SSH agent, proxy, git config, author/committer, trace vars) on top
of `ALLOWED_ENV_VARS`. The `ALWAYS_STRIPPED_ENV_VARS` set is then re-applied
as defense-in-depth — any variable in both the network allowlist and the
hard-deny set (e.g. `GIT_ASKPASS`) is always stripped.

`PushForce` enum: `Normal`, `ForceWithLease { expected_sha }`, `Force`.
`PushRequest.is_destructive()` is true for `Force`, `ForceWithLease`, and
`delete`. `PushForce::Force` is rejected at the tool-side policy level.
`push_permission_hint` produces human-readable scope descriptions.

`PullStrategy` enum: `Merge`, `Rebase`, `FastForwardOnly`.

### Managed worktrees

The M003 `WorktreeService` owns only CodeGG-managed local worktree
lifecycles. It resolves the repository root and base HEAD with structured
read APIs, acquires the shared repository/worktree mutation lock, and invokes
the hardened `codegg-core::worktree` add/remove helpers through
`spawn_blocking`. It does not clone repositories, perform network Git
operations, or replace `GitMutationExecutor` for ordinary Git mutations.

Its deterministic branch/path names are validated by `BranchName` and
`ObjectId`; cleanup refreshes structured status and operation state before
removing and never force-removes dirty, conflicted, unknown, or unmanaged
worktrees. Scheduler jobs that perform the lifecycle reserve
`exclusive:worktree-mutation`, matching the existing Git mutation resource
profile.
Fetch with `--prune` uses `ManagedGitArgv` fallback (not modeled by typed
parser).

Network failures are classified by `classify_network_failure` into
`Dns`, `Connect`, `Authentication`, `Authorization`, `RefRejected`,
`Timeout`, `Transport`.

Config operations validate keys against `CONFIG_KEY_ALLOWLIST` (safe
local-scope prefixes: `branch.`, `pull.rebase`, `rebase.autosquash`,
`core.autocrlf`, etc.) and `CONFIG_DENIED_KEY_PATTERNS` (`credential.*`,
`http.*`, `url.*`, `core.gitProxy`, `core.sshCommand`, `core.sshVariant`).
Global-only keys (`user.*`, `gpg.format`) are rejected when
`allow_local_only=true`.

### Recovery

`src/git_recovery.rs` provides `continue_in_progress`,
`abort_in_progress_typed`, and `skip_in_progress`. Each:

1. Calls `detect_operation_state_for_root(repo_root)` to identify the
   active operation via `.git/` plumbing file inspection.
2. Builds the matching typed `GitOperation` variant.
3. Re-checks `assert_action_matches` (TOCTOU defense — re-reads state
   from disk, refuses if state changed or action is illegal).
4. Runs `GitMutationExecutor::execute`.
5. Classifies outcome with recovery-aware labels.

Family-specific guards:
- `Bisect`, `ApplyMailbox`, `Unknown` refuse automatic recovery.
- `Merge` only supports `Continue`/`Abort` (no `--skip` for merge).
- `Sequencer`-driven operations (cherry-pick/revert ≥ Git 2.25) funnel
  through typed codegg-git variants.

`risk_classes_for_recovery`: `Abort` is tagged `HistoryIntegration +
DestructiveHistory`; `Continue` and `Skip` are `HistoryIntegration` only.

The legacy `git_mutations_ops::abort_in_progress` remains as a deprecated
shim for backward compat.

### Projection

`project_mutation(&MutationResult) -> String` formats: operation label,
before/after HEAD/branch, commits/refs created, paths affected, conflicts,
recovery hints, exit code, duration.

`project_network_mutation` appends network output (git fetch/push
summary lines, with byte-count fallback for large output).

`project_destructive_mutation` appends recovery hint (`git reflog` +
`git reset --hard <sha>`).

`project_recovery` formats action, family, outcome, and next-step hint
tailored to the outcome (abort-completed → "repository back to clean
state"; still-conflicted continue → "resolve conflict markers, git add,
then re-run recover: continue").

## Key Types & APIs

### codegg-git (`crates/codegg-git/`)

| Type | File | Purpose |
|------|------|---------|
| `GitOperation` (54 variants) | `operation.rs` | Typed vocabulary for all Git commands |
| `GitRiskClass` (11 variants) | `risk.rs` | Risk classification per operation |
| `RiskSet` | `risk.rs` | `is_destructive()`, `requires_network()` |
| `parse_git_argv` | `parser.rs` | Pre-tokenized argv → `GitOperation` |
| `render_argv` | `render_argv.rs` | `GitOperation` → `Vec<String>` (canonical) |
| `RepoRoot`, `RepoPath`, `Pathspec` | `path.rs` | Path safety types |
| `BranchName`, `RefName`, `RemoteName`, `ObjectId`, `RevisionExpr` | `ref_name.rs` | Ref safety types |
| `RedactedUrl` | `sensitive.rs` | Credential-hiding URL wrapper; `expose_secret()` only at `render_argv` boundary |
| `AuditSafeArgv` | `sensitive.rs` | `RerunDescriptor.argv` type — always sanitized |
| `ALLOWED_ENV_VARS` (21 entries) | `process_policy.rs` | Canonical allowlist for local git subprocesses |
| `ALWAYS_STRIPPED_ENV_VARS` (28 entries) | `process_policy.rs` | Hard-deny set — always removed before launch |

### egggit (`crates/egggit/`)

| Type | File | Purpose |
|------|------|---------|
| `RichRepoStatus` | `status_v2.rs` | Structured status with branch, dirty state, conflict entries |
| `ConflictEntry`, `ConflictKind` (8 variants), `ConflictShape` | `conflict.rs` | Typed conflict model — does NOT auto-resolve |
| `ConflictReport` | `conflict.rs` | Aggregated summary from entries |
| `RepositoryOperationState` (9 variants) | `operation_state.rs` | In-progress operation detection from `.git/` plumbing |
| `OperationFamily` (9 variants) | `operation_state.rs` | Routing/UI labels |
| `RecoveryAction` (`Continue`, `Abort`, `Skip`) | `operation_state.rs` | Legal recovery actions |
| `detect_operation_state_for_root` | `operation_state.rs` | Canonical state discovery |
| `CommitInfo`, `BlameResult`, `BranchInfo`, `TagInfo`, `RemoteInfo` | various | Typed read results |

### Root crate

| Type | File | Purpose |
|------|------|---------|
| `GitExecutionService` | `git_service.rs:229` | Unified read+raw executor |
| `GitPayload` (12 variants) | `git_service.rs:39` | Structured read payloads |
| `GitMutationExecutor` | `git_mutations.rs:703` | Mutation executor with snapshot/delta |
| `GitEnvPolicy` | `git_mutations.rs:51` | `apply()` (async) / `apply_sync()` (sync) |
| `RepoSnapshot` | `git_mutations.rs:246` | Pre/post state capture |
| `StateDelta` | `git_mutations.rs:278` | Diff between snapshots |
| `MutationOutcome` (5 variants) | `git_mutations.rs:324` | Completed/NoOp/FastForward/Conflict/Rejected |
| `MutationResult` | `git_mutations.rs:352` | Full mutation record |
| `GitMutationError` (7 variants) | `git_mutations.rs:452` | Detailed error with `ExecutionContext` |
| `NetworkEnvPolicy` | `git_network_ops.rs:44` | Extends base env with network vars |
| `PushForce` (3 variants) | `git_network_ops.rs:207` | Normal/ForceWithLease/Force |
| `PushRequest` | `git_network_ops.rs:248` | Push parameters |
| `PullStrategy` (3 variants) | `git_network_ops.rs:155` | Merge/Rebase/FastForwardOnly |
| `CleanRequest`, `CleanPreview` | `git_network_ops.rs:733,714` | Clean operation types |
| `NetworkFailureKind` (7 variants) | `git_network_policy.rs:71` | Failure classification |
| `RecoveryOutcome` (5 variants) | `git_recovery.rs:27` | Recovery result classification |
| `GitTool` | `tool/git.rs:14` | Model-facing tool with 40 mutation actions |

## Configuration Surface

| Config field | Default | Effect |
|-------------|---------|--------|
| `command_intent.route_git_local_mutation` | `off` | Gates Bash→Git routing of local mutations |
| `command_intent.route_git_network` | `off` | Gates command-intent routing of network ops |
| `command_intent.route_git_destructive` | `off` | Gates command-intent routing of reset/clean |

Tool-level typed actions (the model-facing `git` tool `mutation` parameter)
are unaffected by these flags — they route through the dedicated mutation
action regardless of routing mode.

### Env policy layers

1. `GitEnvPolicy::apply()` / `apply_sync()`: clears env, restores
   `ALLOWED_ENV_VARS` (21 vars), strips `ALWAYS_STRIPPED_ENV_VARS` (28
   vars), pins `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`,
   `GIT_SEQUENCE_EDITOR=true`, strips `EDITOR`/`VISUAL`, sets
   `GIT_PAGER=cat`, `PAGER=cat`, `GPG_TTY=""`.
2. `NetworkEnvPolicy::apply_to_command()`: same as base, plus restores
   `NETWORK_ALLOWED_ENV_VARS` (21 vars: `GIT_ASKPASS`, `GIT_SSH_COMMAND`,
   `GIT_SSH_VARIANT`, `GIT_CONFIG_COUNT/GLOBAL/SYSTEM`, `GIT_AUTHOR_*`,
   `GIT_COMMITTER_*`, `HTTP(S)_PROXY`, `http(s)_proxy`, `NO_PROXY`,
   `no_proxy`, `GIT_TRACE`, `GIT_TRACE_PACKET`, `GIT_CURL_VERBOSE`).
   Re-strips `ALWAYS_STRIPPED_ENV_VARS` as defense-in-depth.
3. Canonical source of truth: `crates/codegg-git/src/process_policy.rs`.
   Both root crate and `codegg-core::worktree` consume the same lists.

## Invariants & Gotchas

1. **egggit never mutates.** All egggit modules are read-only. Mutations
   stay in the root crate (`git_mutations`, `git_mutations_ops`,
   `git_network_ops`, `git_recovery`).

2. **Single parsing/rendering truth.** `parse_git_argv` and `render_argv`
   in `codegg-git` are the only Git argv parsers. No duplicate parser
   logic in downstream crates.

3. **Path/ref safety at parse time.** `RepoPath` rejects NUL bytes,
   absolute paths, parent traversal (`..`), paths resolving outside the
   repository root. `BranchName`/`RefName` reject empty, leading `-`,
   `..`, `.lock` suffix, special chars.

4. **Credential leakage prevention.** `RedactedUrl` wraps raw URLs so
   `Debug`/`Serialize`/`Display` never expose credentials. Raw value
   reaches Git child only via `expose_secret()` at the `render_argv`
   boundary. `sanitize_truncate_for_result` redacts stdout/stderr.
   `sanitize_argv_for_run_store` redacts persisted argv.
   `redact_url_credentials_in_text` redacts URL-shaped tokens in
   arbitrary text.

5. **Force-push rejection.** `PushForce::Force` is classified
   `DestructiveHistory` by the parser, so command-intent routing carries
   `DestructiveFileMutation` capability (default: `Deny`). The tool-side
   policy also rejects it.

6. **Broad clean rejection.** `CleanRequest::is_broad()` (ignored=true,
   no paths) is rejected at the tool dispatch layer.

7. **Config key gating.** `credential.*`, `http.*`, `url.*`,
   `core.gitProxy`, `core.sshCommand`, `core.sshVariant` are always
   denied. Global-only keys (`user.*`, `gpg.format`) rejected when
   `allow_local_only=true`.

8. **Merge strategy allowlist.** Only `recursive`, `resolve`, `octopus`,
   `ours`, `subtree`, `ort` are permitted. Arbitrary strategy strings
   are rejected.

9. **Recovery TOCTOU defense.** `assert_action_matches` re-reads
   operation state from disk immediately before executing the recovery
   action. State mismatch → `GitMutationError::StateMismatch`.

10. **Shell boundary.** Commands with pipes, redirects, command
    substitution, semicolons, env assignments, or quoted glob patterns
    are NOT rewritten as `ActualBackend::Git`. They remain
    `ActualBackend::RawShell`. The classification boundary is
    `shell_shape::parse_shell_words` + `has_shell_operators()`.

11. **Canonical subprocess policy.** `ALLOWED_ENV_VARS` and
    `ALWAYS_STRIPPED_ENV_VARS` live in `codegg_git::process_policy`.
    Both root crate and `codegg-core::worktree` consume the same lists.
    Drift caught by `policy_drift_tests` and
    `worktree_uses_canonical_policy`.

12. **RunStore audit-safe rerun argv.** `RerunDescriptor.argv` is
    `Option<AuditSafeArgv>` — always sanitized via URL sanitizer. Raw URL
    reaches Git only ephemerally during execution.

13. **No editor/spawn injection.** `GIT_EDITOR=true`,
    `GIT_SEQUENCE_EDITOR=true`, `EDITOR`/`VISUAL` stripped,
    `GIT_ASKPASS`, `GIT_SSH_COMMAND`, `GIT_PROXY_COMMAND`, all
    `GIT_CONFIG_*` injection vectors stripped.

14. **Recovery not auto-resolved.** Conflicts are presented as typed
    data. The agent must edit files, `git add`, then `recover: continue`.

## Testing

### Narrowest commands

```bash
# codegg-git crate (parser, operation, risk, path, ref_name, render)
cargo test -p codegg-git

# egggit structured reads + operation state + conflicts
cargo test -p egggit

# Root git modules (mutations, network, recovery)
cargo test -p codegg git_mutations
cargo test -p codegg git_network
cargo test -p codegg git_recovery
cargo test -p codegg git_service

# Integration tests (skip when git unavailable)
cargo test --test git_mutations_integration
cargo test --test git_network_integration
cargo test --test git_recovery_integration
cargo test --test git_execution_origin_matrix
```

### Isolated delegated-run commits and integration

Mutation-capable delegated runs are assigned a managed worktree before their
agent loop is constructed. The child tool registry receives that worktree as
its workspace root and applies `ChildGitPolicy::LocalCommitOnly` when the
resolved parent authority includes `GitWrite`. This permits stage/unstage and
local commits in the owned worktree while continuing to reject push, remote and
credential configuration, history rewrites, broad clean/reset, and other
network/destructive operations through their independent policy gates.

The child result is persisted as a bounded, machine-readable
`codegg_core::run_result::AgentRunResult`. Its base/result commits, changed
paths, repository state, and findings are collected from Git facts rather than
treated as claims in final model prose. A completed child does not mutate the
parent repository. `AgentRunIntegrationService` performs an explicit,
lineage-checked merge, cherry-pick, or rebase through the typed Git mutation
executor and returns a structured success or conflict outcome; failed or dirty
worktrees remain available for inspection under the worktree service's cleanup
policy.

The session projection carries only bounded worktree/run facts needed for
inspection: typed ownership IDs, branch, base/result commit, health,
dirty/conflicted state, validation summary, and retention attention. It does
not expose credentials, full diffs, or authority to integrate; integration
remains an explicit parent-side typed Git operation.

### Test counts (verified today)

| Module / file | #[test] + #[tokio::test] |
|---------------|--------------------------|
| `codegg-git` (parser, operation, risk, path, ref_name, render, process_policy) | 342 |
| `egggit/operation_state.rs` | 8 |
| `egggit/conflict.rs` | 7 |
| `tests/git_mutations_integration.rs` | 12 (tokio::test) |
| `tests/git_network_integration.rs` | 13 |
| `tests/git_recovery_integration.rs` | 13 |

### Key test categories

- **codegg-git**: Property-based testing (proptest) for parser. Risk
  classification tests per variant. Path/ref rejection tests.
- **egggit**: Operation state detection for all 9 families. Conflict
  classification, marker detection, binary detection, report aggregation.
- **git_mutations_integration**: Stage/unstage, commit (normal + amend +
  empty), branch create/switch/delete (refuse-current), stash push/apply,
  merge (fast-forward + conflict), rebase, cherry-pick, revert, restore,
  env-policy, projector.
- **git_network_integration**: URL redaction, failure classification,
  remote management, config allowlist, network round-trips, destructive
  reset/clean.
- **git_recovery_integration**: End-to-end on tempdir fixtures — continue
  with conflicts, continue after resolution, abort without state, abort in
  progress, skip during rebase.
- **git_execution_origin_matrix**: Track U routing — verifies
  `route_git_local_mutation` gate behavior.
- **policy_drift_tests** (root): Ensures canonical lists match Phase F
  entries. `root_and_core_share_canonical_lists` verifies root and
  codegg-core read from same source.

## Related Docs

- `architecture/git_phase_f_handoff.md` — Phase F review context
  (do not modify).
- `architecture/git_polish_verification_handoff.md` — Verified state,
  execution-origin matrix, remaining limitations.
- `docs/validation/git-security-review.md` — Phase F security closure.
- `docs/validation/git-rerun-secret-lifecycle.md` — Rerun argv credential
  lifecycle.
- `scripts/check_git_forbidden_patterns.py` — Enforces `expose_secret()`
  boundary, no hand-maintained env-policy tables, `AuditSafeArgv` usage.
- `architecture/command_intent.md` — Command intent classification and
  routing.
- `architecture/command_routing.md` — Active routing mode.
