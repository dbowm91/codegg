# Agent Run, Async Delegation, and Worktree Concurrency Milestone 003 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-run-worktree-concurrency/003-durable-worktree-service-and-leases.md`
Source subsystem roadmap: `plans/subsystems/agent-run-worktree-concurrency-roadmap.md#m003--durable-daemon-worktree-service-and-leases`
Repository baseline reviewed: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`
Implementation commits: `0f3d75bf07f93cc375fed966cf324dff837187ad` — durable managed worktree service, leases, protocol, migration, and documentation

## 1. Executive finding

M003 is fully implemented and strictly closed. CodeGG now has a daemon-owned
durable managed-worktree service with typed repository/workspace/worktree/run
relations, deterministic paths and branches, generation-fenced leases,
structured health, restart reconciliation, and conservative cleanup. Existing
manual worktree APIs remain intact and unmanaged worktrees are never claimed
or automatically removed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Durable record with typed identity, owner, base, lifecycle, and health | `WorktreeRecord`, `WorktreeLease`, SQLite migration 39, and protocol DTO in `crates/codegg-core/src/worktree_service.rs`, `crates/codegg-core/src/session/schema.rs`, and `crates/codegg-protocol/src/dto.rs`. |
| One active owner/generation | Atomic store lease acquisition, partial unique active-lease index, generation checks on renew/release/archive/cleanup, and `stale_generation_cannot_release_new_owner`. |
| Safe deterministic create/verify | `reserve` validates `BranchName`/`ObjectId`, scopes paths by repository identity, uses the hardened create helper under `spawn_blocking`, and verifies Git registration plus `.git` pointer. |
| Contention and distinct concurrent creation | `concurrent_distinct_runs_get_distinct_worktrees`; repository lock and scheduler exclusivity contract documented and wired to the existing Git mutation resource profile. |
| Health and cleanup safety | Structured `egggit` status/operation reads, `dirty_worktree_is_not_cleanup_safe`, immediate refresh before remove, managed-root/symlink checks, and non-force removal. |
| Manual/external coexistence | `external_worktree_is_discovered_but_not_managed`; discovery returns external worktrees without durable ownership, and cleanup rejects unmanaged records. |
| Restart reconciliation | `reconcile_repository`/`reconcile_all`, terminal-owner resolver seam, startup reconciliation task, and durable records for reserved/preparing/active/orphaned states. |
| Protocol/operator surface | Additive `ManagedWorktreeGet`, `ManagedWorktreeList`, `ManagedWorktreeArchive`, and `ManagedWorktreeCleanup` requests with bounded record responses. |
| Compatibility and documentation | Existing `WorktreeList` and low-level APIs remain; `architecture/worktree.md`, `architecture/git.md`, and `architecture/scheduler.md` document ownership and boundaries. |

## 3. Production implementation evidence

- Added `codegg-core::worktree_service` with SQLite and in-memory stores.
- Added durable `managed_worktree` and `worktree_lease` tables and indexes in
  schema migration 39; bumped `STORAGE_LAYOUT_VERSION` to 39.
- Extended low-level worktree creation with an explicit base-commit helper
  while retaining the existing API and hardened environment policy.
- Wired the service through `CoreRuntimeDeps`, `CoreDaemon`, production SQLite
  construction, and bounded startup reconciliation.
- Added additive protocol requests/responses and core-to-wire conversion.
- Kept the core boundary intact by using a narrow owner-status resolver rather
  than importing the root agent runtime into `codegg-core`.

## 4. Verification executed (commands + results; label local vs CI truthfully)

All results below are local verification; no hosted CI claim is made.

- `rtk cargo check -p codegg-core` — passed.
- `rtk cargo check -p codegg` — passed.
- `rtk cargo test -p codegg-core worktree_service` — 8 passed.
- `rtk cargo test -p codegg-protocol` — 162 passed.
- `rtk cargo test --test worktree` — passed.
- `rtk cargo test --lib git` — passed.
- `rtk cargo test --test scheduler_contention` — passed.
- `rtk scripts/verify.sh quick` — passed, including formatting, generated
  agent assets, core boundary, sandbox contract, execution ownership, and
  workspace all-target checks.
- `rtk python3 scripts/check_daemon_cwd_usage.py` — passed.
- `rtk python3 scripts/check_execution_ownership.py` — passed.
- `rtk python3 scripts/check_git_forbidden_patterns.py` — passed.
- `rtk git diff --check` — passed.

## 5. Invariant review

- Worktree identity is opaque and supplied through the repository relation;
  paths are locators, not identity keys.
- A lease has one durable owner and monotonically increasing generation.
- Creation/removal is serialized by the workspace repository lock and uses the
  existing hardened Git subprocess boundary.
- Dirty, conflicted, unknown, missing, or unregistered worktrees cannot pass
  cleanup eligibility.
- Manual worktrees are external discoveries only.
- The service does not perform network Git operations or child-run allocation;
  those remain outside M003 and are owned by later milestones.

## 6. Failure and recovery review

Git creation failure records an orphaned/Git-error state rather than claiming
`Ready`. Post-create verification failure follows the same conservative path.
Cancellation or process failure can therefore be reconciled from durable
reservation state and Git's actual registration. Startup reconciliation retains
non-terminal owners, releases only owners proven terminal by the daemon's
resolver, and never performs destructive cleanup. Repository contention is
serialized instead of spin-waiting or deleting lock state.

## 7. Migration and compatibility review

Migration 39 is additive and idempotent. Existing manual worktrees and legacy
worktree/session records are not backfilled because they lack authoritative
ownership provenance. Existing list/create/remove functions and the existing
`Worktree` DTO remain available for explicit user actions.

## 8. Security review

Automatic paths are generated beneath daemon data and scoped by typed
`RepositoryId`; repository roots are canonicalized. Branches and base commits
are validated by `BranchName` and `ObjectId`. Cleanup verifies the managed-root
boundary, rejects symlink components, refreshes Git state immediately before
removal, checks owner generation, and never passes `--force`. No credentials or
network operation is introduced.

## 9. Documentation and operations

Architecture documentation now describes the managed lifecycle, scheduler
exclusivity relationship, restart behavior, health model, cleanup safety, and
the distinction from manual worktree commands. Operator inspection and bounded
cleanup/archive operations are available through the additive core protocol.

## 10. Unresolved findings (severity: critical/high/medium/low)

None.

## 11. Roadmap disposition

M003 is closed. M004 is dependency-ready because M002 and M003 are now closed;
its child allocation/mutation policy remains entirely deferred to M004. M005
remains blocked on M004, and M006 remains blocked on M001–M005.

## 12. Registry updates

- Marked the implementation plan `implemented` and the roadmap M003 row
  `closed`.
- Added this closure record and recorded implementation commit
  `0f3d75bf07f93cc375fed966cf324dff837187ad`.
- Moved M004 from `blocked` to `ready` in both the roadmap and registry after
  auditing all of its declared dependencies.
- Left M005 and M006 blocked with their remaining dependencies explicitly
  recorded.
