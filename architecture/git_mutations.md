# Git Mutations

Typed Git mutation operations with state-delta semantics.

## Purpose

`src/git_mutations.rs` and `src/git_mutations_ops.rs` provide the canonical entry points for all native-tool Git mutations. The execution model is: resolve repo → pre-snapshot → validate → render argv → execute → post-snapshot → classify → typed `StateDelta`.

## Module Structure

| File | Purpose |
|------|---------|
| `git_mutations.rs` | Core executor, env policy, snapshot/delta model |
| `git_mutations_ops.rs` | Operation-specific wrappers (stage, unstage, commit, etc.) |

## Key Types

### GitMutationExecutor

Core executor that orchestrates the mutation pipeline. Resolves the repo, captures before/after snapshots, executes the git operation, and produces a `StateDelta`.

### GitEnvPolicy

Environment variable hardening for git subprocess calls:

| Method | Purpose |
|--------|---------|
| `apply(cmd)` | Apply env policy to async `Command` |
| `apply_sync(cmd)` | Apply env policy to `std::process::Command` |

Controls:
- `GIT_TERMINAL_PROMPT=0` — Prevents interactive prompts
- Editor pinning — Prevents interactive editor launches
- Command-bearer stripping — Removes sensitive env vars

### StateDelta

Typed representation of what changed after a mutation:

| Field | Description |
|-------|-------------|
| Pre-snapshot | Repository state before mutation |
| Post-snapshot | Repository state after mutation |
| Delta | Classification of what changed |

### RepoSnapshot

Capture of repository state at a point in time (staged files, branch, HEAD, etc.).

## Mutation Operations

Available through `git_mutations_ops.rs`:

| Function | Description |
|----------|-------------|
| `stage_paths(exec, repo_root, paths)` | Stage named literal paths |
| `stage_all(exec, repo_root)` | Stage all (tracked + untracked) |
| `stage_tracked(exec, repo_root)` | Stage only tracked files |
| `unstage_paths(exec, repo_root, paths)` | Unstage named paths |
| `unstage_all(exec, repo_root)` | Unstage all staged changes |
| `run_raw_mutation(exec, repo_root, op, argv)` | Internal: raw mutation execution |

## Execution Pipeline

```
Git Mutation Request
    │
    ▼
Resolve repo root
    │
    ▼
Capture pre-snapshot (RepoSnapshot)
    │
    ▼
Validate operation (codegg-git risk classification)
    │
    ▼
Render argv (codegg-git render_argv)
    │
    ▼
Apply GitEnvPolicy (env hardening)
    │
    ▼
Execute git subprocess
    │
    ▼
Capture post-snapshot (RepoSnapshot)
    │
    ▼
Classify changes → StateDelta
    │
    ▼
Persist to RunStore (lineage tracking)
    │
    ▼
Return MutationResult
```

## Risk Classification

All mutations go through `codegg_git::parse_git_argv()` for risk assessment before execution. High-risk operations (e.g., `git reset --hard`, `git clean -f`) may be blocked or require confirmation depending on policy.

## RunStore Integration

Each mutation records its lineage in the `RunStore` via `git_run_store.rs`, linking the operation to the job/attempt that triggered it.

## See Also

- [Git](git.md) — Full Git module overview
- [Git Network](git_network.md) — Network operations (push/pull/fetch)
- [Git Recovery](git_recovery.md) — In-progress operation recovery
- [Tool](tool.md) — Git tool wrapper
