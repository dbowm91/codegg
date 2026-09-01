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
| Root re-export | `src/lib.rs:12` | `pub use codegg_core::worktree;` |
| Mutation executor | `src/git_mutations.rs` | All other git mutations (worktree add/remove are in codegg-core) |
| Durable service | `crates/codegg-core/src/worktree_service.rs` | SQLite records, leases, reconciliation, health, and safe cleanup |
| Durable schema | `crates/codegg-core/src/session/schema.rs` | M003 migration 39: managed worktrees and lease history |

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
   (`codegg_git::process_policy::ALLOWED_ENV_VARS`)
3. Strips command-bearing variables
   (`codegg_git::process_policy::ALWAYS_STRIPPED_ENV_VARS`)
4. Pins `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat`, `PAGER=cat`
5. Pins `GIT_EDITOR=true`, `GIT_SEQUENCE_EDITOR=true`
6. Strips `EDITOR`, `VISUAL`
7. Sets `GPG_TTY=""`

The canonical policy lists live in `codegg-git` and are re-exported
from codegg-core as `POLICY_ALLOWED_ENV_VARS` / `POLICY_ALWAYS_STRIPPED_ENV_VARS`.

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
without sharing its write path.

Durable delegated-run projections expose only bounded worktree identity, owner
run, branch, base commit, lifecycle/health, dirty/conflicted state, and the
retained-for-attention flag. The projection is derived from `WorktreeService`
and can be replayed after reconnect or restart; it does not grant ownership or
perform cleanup. Full paths and Git details remain behind the existing
authorized worktree inspection APIs.

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

### Worktree (codegg-core:16)
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

### WorktreeInfo (egggit:5)
Internal type in egggit. Converted to legacy `Worktree` via
`into_legacy()` at codegg-core:24.

### Public functions (codegg-core)

| Function | Line | Signature |
|----------|------|-----------|
| `list_worktrees` | :33 | `async fn(git_root: &Path) -> Result<Vec<Worktree>, AppError>` |
| `create_worktree` | :40 | `fn(git_root, path, branch, create_branch) -> Result<(), AppError>` |
| `create_worktree_at` | — | `fn(git_root, path, branch, create_branch, base) -> Result<(), AppError>` |
| `remove_worktree` | :68 | `fn(git_root, path, force) -> Result<(), AppError>` |
| `find_git_root` | :120 | `fn(start: &Path) -> Option<PathBuf>` |
| `is_git_file` | :124 | `fn(git_path: &Path) -> bool` |
| `is_git_worktree` | :128 | `fn(dir: &Path) -> bool` |
| `hardened_git_command` | :98 | `fn(args, git_root) -> Command` (private) |

### Policy re-exports (codegg-core:11-14)
```rust
pub use codegg_git::process_policy::{
    ALLOWED_ENV_VARS as POLICY_ALLOWED_ENV_VARS,
    ALWAYS_STRIPPED_ENV_VARS as POLICY_ALWAYS_STRIPPED_ENV_VARS,
};
```

## Consumers

| Location | How Used |
|----------|----------|
| `src/core/daemon.rs:3747,4977` | `find_git_root` + `list_worktrees` for workspace/project discovery |
| `src/tui/app/mod.rs:5880` | `/worktree` command handler |
| `src/tui/commands/git_sidebar.rs:113` | Git sidebar worktree listing |
| `src/tui/commands/tasks.rs:550` | `start_worktree_list` async task |
| `src/tool/git.rs:964` | Git tool `worktree` subcommand |
| `src/agent/turn_runtime.rs:782` | `find_git_root` for workspace root resolution |

## Invariants & Gotchas

- **Shared env policy**: `hardened_git_command` and `GitEnvPolicy::apply`
  both consume `codegg_git::process_policy` constants. A drift-guard
  test (`worktree_uses_canonical_policy`) ensures codegg-core stays
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
cargo test --test worktree                  # integration tests (11 tests)
```

Integration tests cover: struct creation, detached state, find_git_root,
list_worktrees (current/detached detection), create+remove round-trip,
is_git_worktree/is_git_file edge cases, and symlink detection.

## Related Docs

- [git.md](git.md) — Git execution architecture
- `architecture/command_intent.md` — Git command classification
- `crates/codegg-git/src/process_policy.rs` — Canonical env policy
