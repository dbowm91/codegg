# Worktree Module

Git worktree management — listing, creating, and removing worktrees,
plus hardening for subprocess environments.

## Purpose

Provides a thin API over `git worktree` operations. Read-only listing
delegates to `egggit`; mutating operations (add/remove) use hardened
`std::process::Command` subprocesses with a security-enforced
environment policy shared with the root crate's `GitMutationExecutor`.

The durable M003 service in `crates/codegg-core/src/worktree_service.rs`
adds ownership and lifecycle state around these primitives. It is a daemon
domain service, not a replacement Git executor.

## Where It Lives

| Layer | Path | Role |
|-------|------|------|
| Core facade | `crates/codegg-core/src/worktree.rs` | Public API: `list_worktrees`, `create_worktree`, `remove_worktree`, `find_git_root`, `is_git_worktree`, `is_git_file` |
| Read-only engine | `crates/egggit/src/worktree.rs` | Async `list_worktrees` (porcelain parser), `find_git_root`, `is_git_file`, `is_git_worktree` |
| Root re-export | `src/lib.rs:23` | `pub use codegg_core::worktree;` |
| Mutation executor | `src/git_mutations.rs` | All other git mutations (worktree add/remove are in codegg-core) |
| Durable service | `crates/codegg-core/src/worktree_service.rs` | SQLite records, leases, reconciliation, health, and safe cleanup |
| Durable schema | `crates/codegg-core/src/session/schema.rs` | M003 migration 39 (`migrate_v39`, `:2113`): managed worktrees and lease history |

The `src/worktree/` directory referenced in older docs **does not exist**.
All worktree code lives in codegg-core, re-exported for root-crate callers.

## How It Works

### Read-only path

`list_worktrees` is async. It spawns `git worktree list --porcelain` via
`egggit::worktree::list_worktrees`, parses the porcelain output into
`WorktreeInfo` structs, and wraps them in the legacy `Worktree` shape.

Current-worktree detection uses canonicalized path comparison against
the provided `git_root`, including symlink-safe resolution.

### Mutating path

`create_worktree` and `remove_worktree` are synchronous, building a
`std::process::Command` via `hardened_git_command`. This helper:

1. Clears the process environment (`env_clear()`)
2. Restores only variables in the canonical allowlist
   (`egggit::process::ALLOWED_ENV_VARS`)
3. Strips command-bearing variables
   (`egggit::process::ALWAYS_STRIPPED_ENV_VARS`)
4. Pins `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat`, `PAGER=cat`
5. Pins `GIT_EDITOR=true`, `GIT_SEQUENCE_EDITOR=true`
6. Strips `EDITOR`, `VISUAL`
7. Sets `GPG_TTY=""`

The canonical policy and command builder live in `egggit::process` and are
re-exported from codegg-git/codegg-core for compatibility as
`POLICY_ALLOWED_ENV_VARS` / `POLICY_ALWAYS_STRIPPED_ENV_VARS`.

**The mutating path is policy-gated, not merely hardened.** `hardened_git_command`
does not return a `Command`: it returns
`Result<GitSyncCommand, GitExecutionPolicyOutcome>`, because the canonical
policy classifies the argv *before* the subprocess is built.
`git worktree add` and `git worktree remove` both require the
`CleanSmudgeFilter` capability (`crates/egggit/src/process.rs:641`-`:664`),
because a linked worktree writes the branch's files through the repository's
smudge filter and a filter *command* is arbitrary code. Without a contained
backend the call is **refused** — `AppError::Worktree("Git backend unavailable
for [CleanSmudgeFilter]")` — and no subprocess is spawned. The durable
contained route is `WorktreeService::create_reserved_contained`
(`crates/codegg-core/src/worktree_service.rs:1109`); see
`architecture/git.md` "Contained managed-worktree creation" and ADR-0048.

### Durable managed lifecycle

`WorktreeService` owns CodeGG-created worktrees through a durable
`WorktreeRecord` and a generation-fenced `WorktreeLease`:

```text
reserve -> preparing -> ready -> in_use -> ready
                              \\-> releasing -> removed
                              \\-> archived -> removed
                              \\-> orphaned (uncertain/unsafe state)
```

Records carry typed project, repository, workspace, worktree, and run
identities; the canonical repository root, managed path, branch, base commit,
health, lifecycle state, and lease generation are persisted. A partial unique
index permits at most one active lease per worktree, while the record owner and
generation are checked for every renew, release, archive, and cleanup action.

Managed paths are deterministic (`<daemon-data>/worktrees/<repository-id>/wt-<short-worktree-id>`)
and outside the repository root. Branch names are derived from the durable
worktree identity and validated with `BranchName`; base commits are validated
with `ObjectId`. Manual worktrees are only returned as external discoveries
and are never claimed or automatically removed.

Creation and removal run through `spawn_blocking` under the repository lock.
Creation verifies both Git registration and the worktree `.git` pointer before
marking a record ready. Refresh and restart reconciliation use structured
`egggit` status and operation-state reads. Missing, dirty, conflicted,
unknown, or unregistered worktrees become attention/orphan states. Cleanup
refreshes immediately before removal, checks the expected generation and
managed-root/symlink boundary, and never uses `--force`.

SQLite-backed daemons start a bounded reconciliation task. An active durable
run remains leased until its run record is terminal; a terminal run is
released but not destructively cleaned up during reconciliation.

### Agent-run mutation isolation (M004)

The scheduler classifies delegated capability before child execution. A
mutation-capable durable child receives a distinct managed worktree lease in
the run's `Preparing` phase and all child repository tools are rooted there.
The lease is refreshed before release; dirty or conflicted results remain
retained and are not eligible for automatic cleanup. Read-only children avoid
allocation and are filtered away from write, terminal, Bash, Git, and commit
authority.

The durable run result is stored in `agent_run_result` and is bounded before
serialization. Parent integration is a separate operation: it requires the
child's recorded base/result identity, the same repository, and a clean parent
still at that base, then uses the typed Git mutation executor. Conflict
outcomes are returned as structured recoverable results; successful child
completion alone never changes the parent.

Nested allocation keeps the durable `RepositoryId` and resolves linked
worktrees back to the repository's common root for lifecycle identity. The
effective base is read from the owning checkout (or an explicitly supplied
base commit), so a grandchild continues from its parent's current checkout
without sharing its write path. Convergence repair/replan children always
supply the host-selected immutable base commit and therefore receive a new
managed worktree; the prior child worktree is never reused or copied.

Durable delegated-run projections expose only bounded worktree identity, owner
run, branch, base commit, lifecycle/health, dirty/conflicted state, and the
retained-for-attention flag. The projection is derived from `WorktreeService`
and can be replayed after reconnect or restart; it does not grant ownership or
perform cleanup. Full paths and Git details remain behind the existing
authorized worktree inspection APIs.

`RunPlacement::ExistingManagedReadOnly { worktree_id }` is a separate delegated
placement for inspecting an already-created managed checkout. The ID is only a
lookup key: the daemon checks project, repository, source workspace, canonical
checkout binding, session/job ownership, `Ready` state and clean health before
acquiring a run-owned exclusive lease. The child can use only `read`, `list`,
and `glob`; process, plugin, delegation and mutation tools are removed from its
effective registry. When the attempt ends, the exact lease generation is
released and the checkout remains in place. A busy checkout is refused. This
mode does not permit concurrent inspectors, shared writes, checkout cleanup,
or guarantees against external processes running as the same OS user.

### WorkOrder workspace placement corrective (M001)

WorkOrders currently materialize the mutation-capable `build` agent. Its
mutation classification is independent of the requested placement: `shared`
and `serialized` never make that job read-only. Auto-isolated Git work uses a
managed worktree; read-only work may share the project checkout. Mutation in a
shared or serialized checkout is still **refused** and recorded as
`NeedsAttention`. The older global scheduler key is not evidence of
serialization and is not attached to WorkOrder AgentTurns. Select
`auto_isolated`, or place the run explicitly with the shared placement on a
qualified platform.

The refusal is not the absence of a reservation — the checkout-scoped writer
permit and the checkout reservation authority **do** exist (corrective
C002–C004) and native writers are routed through them. What is still missing
is a production path that places a WorkOrder on a group's shared checkout, and
the shared profile itself is opt-in and platform-gated (corrective C006:
`codegg_core::run_placement::RunPlacement`, default `Auto` = isolated). The
daemon states exactly this in its own diagnostic
(`crates/codegg-core/src/work_order/coordinator.rs:304`):
`shared_mutation_unavailable`.

### Utility functions

- `find_git_root(start)` — walks up the directory tree looking for `.git`
  directory or `.git` file (worktree pointer). Returns `None` if no git
  root found.
- `is_git_worktree(dir)` — returns `true` only when `dir/.git` exists
  AND is a file starting with `gitdir:`. Regular repos with `.git`
  directories return `false`.
- `is_git_file(git_path)` — checks if a `.git` path is a worktree
  pointer file.

## Key Types & APIs

### Worktree (codegg-core:15)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_detached: bool,
}
```

Note: `is_locked` and `is_main` are **not implemented**.

### WorktreeInfo (egggit:8)
Internal type in egggit (`crates/egggit/src/worktree.rs:8`). Converted to
legacy `Worktree` via `into_legacy()` at codegg-core:22.

### Public functions (codegg-core)

| Function | Line | Signature |
|----------|------|-----------|
| `list_worktrees` | :31 | `async fn(git_root: &Path) -> Result<Vec<Worktree>, AppError>` |
| `create_worktree` | :39 | `fn(git_root, path, branch, create_branch) -> Result<(), AppError>` |
| `create_worktree_at` | :52 | `fn(git_root, path, branch, create_branch, base) -> Result<(), AppError>` |
| `remove_worktree` | :85 | `fn(git_root, path, force) -> Result<(), AppError>` |
| `hardened_git_command` | :116 | `fn(args, git_root) -> Result<GitSyncCommand, GitExecutionPolicyOutcome>` (private) |
| `find_git_root` | :126 | `fn(start: &Path) -> Option<PathBuf>` |
| `is_git_file` | :130 | `fn(git_path: &Path) -> bool` |
| `is_git_worktree` | :134 | `fn(dir: &Path) -> bool` |
| `git_dir_for` | :152 | `fn(root: &Path) -> Option<PathBuf>` — the Git **directory** (objects, refs, index, `worktrees/` admin dir), resolved through a `.git` pointer file; `None` for a non-repository or a malformed pointer |

### Policy re-exports (codegg-core:9-12)
```rust
pub use egggit::process::{
    ALLOWED_ENV_VARS as POLICY_ALLOWED_ENV_VARS,
    ALWAYS_STRIPPED_ENV_VARS as POLICY_ALWAYS_STRIPPED_ENV_VARS,
};
```

## Consumers

| Location | How Used |
|----------|----------|
| `src/core/daemon_projects.rs:71` (`handle_projects_request`) | `find_git_root` + `list_worktrees` for workspace/project discovery |
| `src/tui/app/mod.rs:6612` | `/worktree` command handler (`B::Worktree`) |
| `src/tui/commands/git_sidebar.rs:143` | Git sidebar worktree listing |
| `src/tui/commands/tasks.rs:550` | `start_worktree_list` async task |
| `src/git_service.rs:1124` (`execute_worktrees`) | Git tool `worktree` subcommand (read-only dispatch; rendered at `src/tool/git_read.rs:336`) |
| `src/agent/turn_runtime.rs:1272` | `egggit::worktree::find_git_root` for workspace root resolution |

## Invariants & Gotchas

- **Shared env policy**: `hardened_git_command` and `GitEnvPolicy::apply`
  both consume `egggit::process` constants. A drift-guard
  unit test (`worktree_uses_canonical_policy`,
  `crates/codegg-core/src/worktree.rs:188`) ensures codegg-core stays
  synchronized with the canonical source.
- **Sync vs async**: `create_worktree`/`remove_worktree` are synchronous
  (blocking `std::process::Command`); `list_worktrees` is async
  (`tokio::task::spawn_blocking`).
- **No permission flow**: Worktree add/remove are not gated by the
  permission system. They are only called from TUI command handlers
  where the user has already initiated the action.

## Testing

```bash
cargo test -p codegg-core worktree         # core unit tests
cargo test --test worktree                  # integration tests (14 tests)
```

The 14 integration tests in `tests/command_git/worktree.inc` cover: struct creation,
detached state, `find_git_root` with a `.git` dir and with a `.git` file,
`list_worktrees` on a non-git dir, porcelain parsing with current/detached
detection, the **contained-backend refusal** for `create_worktree` /
`remove_worktree` (both are refused with `Git backend unavailable` /
`CleanSmudgeFilter` and leave no worktree behind),
`is_git_worktree` with a `.git` dir / `.git` file / non-git dir,
`is_git_file` with and without the `gitdir:` prefix, and two
symlink-detection cases (`list_worktrees_symlink_detection`,
`list_worktrees_symlink_worktree_path`).

## Related Docs

- [git.md](git.md) — Git execution architecture
- `architecture/command_intent.md` — Git command classification
- `crates/egggit/src/process.rs` — Canonical env policy and process builder

## Source verification

### Pass of 2026-10-23

Re-verified against `crates/codegg-core/src/worktree.rs`,
`crates/codegg-core/src/worktree_service.rs`, `crates/egggit/src/worktree.rs`,
`crates/egggit/src/process.rs`, `crates/codegg-core/src/session/schema.rs`,
`tests/command_git/worktree.inc`, and `src/lib.rs`. No build or test run was
performed; every count is a source-level measurement.

- **Filled a real gap.** The "Mutating path" section described
  `create_worktree` / `remove_worktree` as plain hardened subprocess builders.
  They are in fact **policy-gated**: `hardened_git_command` returns
  `Result<GitSyncCommand, GitExecutionPolicyOutcome>`, `worktree add` and
  `worktree remove` require the `CleanSmudgeFilter` capability, and the call
  is refused with `Git backend unavailable for [CleanSmudgeFilter]` when no
  contained backend is available. The section now says so, names the
  contained route (`WorktreeService::create_reserved_contained`), and the
  Testing section's "create+remove round-trip" entry is corrected to the
  refusal it actually asserts.
- **Corrected a stale "not yet" claim.** The M001 section said the daemon
  "does not yet have one checkout-scoped writer reservation". The writer
  permit and the checkout reservation authority do exist (C002–C004) and
  native writers are routed through them; what is missing is a production path
  that places a WorkOrder on a group's shared checkout, and the shared profile
  is opt-in and platform-gated (C006). The section now matches the daemon's
  own `shared_mutation_unavailable` diagnostic
  (`crates/codegg-core/src/work_order/coordinator.rs:304`).
- **Re-anchored every stale ref**: `src/lib.rs:12` → `:23`;
  `create_worktree` `:38` → `:39`; `create_worktree_at` `:51` → `:52`;
  `remove_worktree` `:83` → `:85`; `hardened_git_command` `:113` → `:116`;
  `find_git_root` `:120` → `:126`; `is_git_file` `:124` → `:130`;
  `is_git_worktree` `:128` → `:134`; `WorktreeInfo`
  `crates/egggit/src/worktree.rs:6` → `:8`; policy re-exports `:9-11` → `:9-12`;
  `worktree_uses_canonical_policy` `worktree.rs:142` → `:188`;
  `migrate_v39` `schema.rs:2036` → `:2113`.
- **Re-anchored every consumer ref**: `src/core/daemon_projects.rs:13` → `:71`;
  `src/tui/app/mod.rs:4282` → `:6612`;
  `src/tui/commands/git_sidebar.rs:113` → `:143`;
  `src/git_service.rs:721` → `:1124` (`execute_worktrees`; the read-only
  dispatch is `GitOperation::WorktreeList` at `:346`);
  `src/tool/git_read.rs:335` → `:336`;
  `src/agent/turn_runtime.rs:984` → `:1272`.
  `src/tui/commands/tasks.rs:550` was already accurate.
- **Added a missing public API.** `git_dir_for` (`:152`) resolves the Git
  *directory* (objects, refs, index, and the `worktrees/` administration
  directory) through a `.git` pointer file, and is what a contained child
  needs instead of the source working tree. It was absent from the public
  function table.
- **Confirmed correct as written**: `Worktree` (`:15`), `into_legacy()`
  (`codegg-core:22`), `list_worktrees` (`:31`), the seven-step hardened
  environment, the durable lifecycle state machine, the managed-path
  determinism claim, `tests/command_git/worktree.inc` = **14** test functions,
  and the M003 durable schema claim (`migrate_v39`).

### Pass of 2026-10-06

Verified against `crates/codegg-core/src/worktree.rs`,
`crates/codegg-core/src/worktree_service.rs`, `crates/egggit/src/worktree.rs`,
`tests/command_git/worktree.inc`, and `src/lib.rs`. Confirmed the prior review's test-count
claim: `tests/command_git/worktree.inc` holds **14** `#[test]`/`#[tokio::test]` functions,
not 11, and the coverage list is now enumerated by name. Confirmed the prior
review's function-line corrections as already applied and accurate
(`list_worktrees` `:31`, `create_worktree` `:38`, `remove_worktree` `:83`).
Further corrections made here: `into_legacy()` `codegg-core:24` → `:22`, the
policy re-export range `codegg-core:11-14` → `:9-11`, the previously
line-less `create_worktree_at` filled in at `:51`, and four consumer refs
(`src/core/daemon_projects.rs` → `:13`, `src/tui/app/mod.rs:5880` → `:4282`,
`src/tool/git.rs:964` → `src/git_service.rs:721` because the read-only
`worktree` subcommand dispatches through `git_service`, and
`src/agent/turn_runtime.rs:782` → `:984`, which resolves through
`egggit::worktree`, not the codegg-core facade). Added the drift-guard test
location (`worktree_uses_canonical_policy`, `worktree.rs:142`).
Confirmed correct as written: `Worktree` (`:15`), `WorktreeInfo`
(`crates/egggit/src/worktree.rs:6`), `find_git_root`/`is_git_file`/
`is_git_worktree`/`hardened_git_command` lines, the `src/lib.rs:12`
re-export, `worktree_service.rs` existence, and the M003 durable schema claim
(`migrate_v39`, `crates/codegg-core/src/session/schema.rs:2036`).

Every line ref in the 2026-10-06 list has since moved again; the 2026-10-23
pass above carries the current anchors.
