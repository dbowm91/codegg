# Worktree Module

Git worktree management — listing, creating, and removing worktrees,
plus hardening for subprocess environments.

## Purpose

Provides a thin API over `git worktree` operations. Read-only listing
delegates to `egggit`; mutating operations (add/remove) use hardened
`std::process::Command` subprocesses with a security-enforced
environment policy shared with the root crate's `GitMutationExecutor`.

## Where It Lives

| Layer | Path | Role |
|-------|------|------|
| Core facade | `crates/codegg-core/src/worktree.rs` | Public API: `list_worktrees`, `create_worktree`, `remove_worktree`, `find_git_root`, `is_git_worktree`, `is_git_file` |
| Read-only engine | `crates/egggit/src/worktree.rs` | Async `list_worktrees` (porcelain parser), `find_git_root`, `is_git_file`, `is_git_worktree` |
| Root re-export | `src/lib.rs:12` | `pub use codegg_core::worktree;` |
| Mutation executor | `src/git_mutations.rs` | All other git mutations (worktree add/remove are in codegg-core) |

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
