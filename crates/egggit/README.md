# egggit

Read-only Git and worktree facts for Rust tools.

`egggit` exposes a small async API for inspecting a Git repository:
branch, status, diff summary, changed files, log, blame, refs, worktree
facts, operation state, and patch validation. It does not mutate the
repository. Commit, worktree create/remove, and other mutating workflows
stay with the host application, which owns its own permission and approval
policy.

## Scope

The supported contract is read-only repository facts plus deterministic
pure helpers (conflict-marker classification, patch validation). The
low-level `process` module is unprivileged plumbing shared by the
structured read operations: it builds shell-free `git` commands with a
hardened environment policy but enforces no permission policy itself.

## Stability

Public types use `serde` with `snake_case` wire strings. Adding new
fact fields or enum variants is minor; renaming or removing existing
fields or changing their meaning is major. No release automation or
publication promises are included in this repository milestone.

## No host dependency

`egggit` depends only on `serde`, `thiserror`, and `tokio` (plus
`tempfile` for tests). It does not depend on any host application.
