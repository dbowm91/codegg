# Agent Run, Async Delegation, and Worktree Concurrency Milestone 003 — Durable Worktree Service and Leases

Status: blocked

Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

Source roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md#m003--durable-daemon-worktree-service-and-leases`

Long-term requirements:

- `plans/000-long-term-specification.md#18-worktree-native-concurrency`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — worktree, worktree owner, workspace, repository
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`

Applicable ADRs:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: infrastructure/invariant

Hard blocker: M001 must close with durable `AgentRunId` ownership.

## 1. Objective

Promote CodeGG’s low-level Git worktree helpers into a daemon-owned durable worktree service with typed `WorktreeId`, owner/lease identity, base commit, lifecycle/health state, restart reconciliation, and safe cleanup.

This milestone establishes worktree ownership but does not yet automatically allocate a worktree to every mutating child. M004 consumes the service and changes delegated-run policy.

## 2. Why this milestone becomes ready after M001

M001 provides the durable run owner needed for leases. Existing Git foundations are already strong:

- typed project/repository/workspace/worktree identities;
- `crates/egggit` read-only worktree discovery;
- hardened `codegg-core::worktree` create/remove helpers;
- typed Git operation parsing/rendering, safe paths/refs, mutation snapshots, conflict state, recovery, and credential policy;
- scheduler resource/exclusivity mechanisms;
- project/workspace catalog and explicit workspace roots.

The missing pieces are durable ownership, lifecycle, recovery, and service-level safety policy.

## 3. Current implementation evidence

Reconfirm before editing:

- `crates/codegg-core/src/worktree.rs` exposes `list_worktrees`, synchronous create/remove, root detection, and worktree-pointer helpers;
- `crates/egggit/src/worktree.rs` provides async read-only worktree facts;
- current `Worktree` DTO contains path/branch/current/detached fields but no stable owner/lease/base commit/health/generation;
- worktree add/remove use hardened Git environment policy but are not permission-gated as a daemon domain service;
- TUI/sidebar/tool consumers treat worktrees mostly as repository facts/manual commands;
- scheduler has an `exclusive:worktree-mutation` style resource class/seam for Git mutation but no durable lease ownership;
- typed Git mutation and conflict state should be reused instead of shelling out through new ad-hoc paths.

## 4. Invariants that must not regress

- A durable worktree record is tied to one repository identity and explicit workspace/path locator.
- One active CodeGG lease has one owner/generation; stale owners cannot mutate or remove a re-leased worktree.
- Worktree path and branch/ref creation use typed validation and hardened Git environment policy.
- Automatic cleanup never force-removes a dirty, conflicted, unknown-health, or externally modified worktree.
- Existing user/manual worktrees are discoverable but are not silently claimed or deleted by CodeGG.
- Worktree isolation does not bypass scheduler exclusivity for shared repository/build/cache/database resources.
- Restart recovery is conservative: uncertainty produces `orphaned`/`needs_attention`, not destructive cleanup.
- Repository relocation or symlink differences do not silently create duplicate durable owners when the underlying repository/worktree is the same.
- No network Git operation is required to create/reconcile a local worktree.

## 5. Scope

### In scope

- durable `WorktreeRecord`, `WorktreeLease`, state/health enums, owner relation, generation, base commit, path, branch/ref strategy, timestamps;
- daemon `WorktreeService` using existing Git/worktree primitives;
- operations: reserve/create, inspect/refresh, acquire/renew/release lease, mark health, archive/remove when safe, reconcile after restart;
- deterministic CodeGG-owned path and branch naming based on typed run/worktree IDs rather than free-form descriptions;
- collision handling for branch names, paths, existing Git worktree registration, and stale durable records;
- distinction between CodeGG-managed worktrees and externally/manual-created worktrees;
- dirty/conflict checks before cleanup/remove;
- scheduler exclusivity around create/remove/repository metadata mutations;
- minimal protocol/service operations needed by later run construction and operator inspection;
- tests across Linux/macOS path semantics where existing test infrastructure supports them.

### Explicitly out of scope

- automatic child allocation policy (M004);
- allowing child commits (M004);
- automatic merge/rebase/cherry-pick into parent;
- remote/SSH worktrees;
- cloning repositories per run;
- managing submodule/LFS policy beyond existing Git behavior;
- deleting arbitrary user worktrees;
- broad TUI workflow redesign beyond minimal inspection hooks.

## 6. Required production changes

### Core/domain

Define durable worktree state with only states used by real lifecycle behavior. A representative shape:

```rust
pub enum ManagedWorktreeState {
    Reserved,
    Preparing,
    Ready,
    InUse,
    Releasing,
    Archived,
    Orphaned,
    Removed,
}

pub enum WorktreeHealth {
    Clean,
    Dirty,
    Conflicted,
    Missing,
    GitError,
    Unknown,
}

pub struct WorktreeRecord {
    pub worktree_id: WorktreeId,
    pub project_id: ProjectId,
    pub repository_id: RepositoryId,
    pub workspace_id: WorkspaceId,
    pub node_id: Option<NodeId>,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub base_commit: String,
    pub managed: bool,
    pub state: ManagedWorktreeState,
    pub health: WorktreeHealth,
    pub lease_generation: u64,
    pub owner_run_id: Option<AgentRunId>,
    pub created_at: i64,
    pub updated_at: i64,
}
```

Exact types should reuse existing `ObjectId`/safe ref/path types where crate boundaries allow.

### Storage and migrations

Add durable records and lease generation/owner fields. Unique constraints should prevent duplicate active managed records for the same canonical Git worktree path and prevent a single managed worktree from being concurrently leased to multiple owners.

Store canonicalized/normalized locator information carefully; do not make path text the repository identity.

### Worktree service

Implement one daemon domain service that:

1. resolves repository root/identity and current HEAD/base commit;
2. reserves durable identity/path/branch name;
3. acquires scheduler/repository mutation exclusivity;
4. creates the worktree through existing hardened Git primitives;
5. verifies Git registration and actual `.git` worktree pointer;
6. marks `Ready` and issues a lease generation to the owning run;
7. refreshes dirty/conflict/operation state using structured Git reads;
8. releases ownership only when the run is terminal or explicitly hands off;
9. removes only when safe cleanup policy permits;
10. reconciles durable records with `git worktree list --porcelain` after daemon restart.

Prefer evolving `codegg-core::worktree` into a non-blocking service boundary rather than adding new synchronous subprocess calls. If low-level create/remove remain synchronous, execute them through `spawn_blocking`/the scheduler-owned domain path so the Tokio runtime is not blocked.

### Naming and layout

Use a configured/default CodeGG worktree root outside the main working tree, scoped by repository identity. Names should contain a short safe run/worktree ID for operator recognition but not depend solely on display descriptions. Avoid branch names that can collide across daemon restarts; include durable run/worktree identity or generation.

Do not assume the repository default branch is `main`.

### Cleanup and orphan policy

Automatic cleanup may remove only CodeGG-managed worktrees whose:

- lease is released/terminal;
- Git state is clean;
- no in-progress merge/rebase/cherry-pick/revert/etc. exists;
- durable generation/owner still matches;
- path resolves to the expected managed worktree;
- removal passes existing Git safety policy.

Otherwise mark `Orphaned`/`NeedsAttention` (or equivalent) and surface operator guidance.

### Protocol/operator surface

Expose bounded operations/DTOs sufficient to:

- inspect managed worktree by ID/run;
- list active/orphaned managed worktrees;
- request safe cleanup/archive;
- view branch/base/health/owner summary.

Manual existing `/worktree` listing remains; managed ownership metadata can be additive.

### Security

- validate all branch/ref/path components through existing typed Git safety utilities;
- never accept an arbitrary caller-supplied absolute path for automatic managed worktree creation without explicit trusted/user action;
- use existing Git env/credential policy;
- prevent stale lease generation from removing/releasing a newly re-owned worktree;
- do not follow symlinks outside the managed root during cleanup.

## 7. Ordered work packages

### A — Domain/store and managed-vs-external distinction

Implement records, states, generation ownership, migrations, and discovery classification.

Acceptance evidence:

- managed record round-trip;
- manual worktrees show as external/unmanaged;
- duplicate active lease constraints hold.

### B — Reserve/create/verify

Implement deterministic path/ref selection, scheduler exclusivity, hardened create, and post-create verification.

Acceptance evidence:

- two concurrent create requests for distinct runs succeed with distinct worktrees;
- branch/path collision returns typed retryable/conflict result rather than overwriting state;
- failed Git creation leaves no false `Ready` record.

### C — Lease acquire/release/generation

Implement owner checks, lease generation, stale-operation rejection, and handoff/release seam.

Acceptance evidence:

- stale owner/generation cannot release a re-leased worktree;
- one active owner invariant holds under concurrency.

### D — Health, dirty/conflict, and cleanup policy

Reuse structured status/operation-state/conflict reads and add safe archive/remove behavior.

Acceptance evidence:

- dirty/conflicted worktree is retained and marked attention-required;
- clean terminal managed worktree can be safely removed;
- manual/unmanaged worktree is never removed automatically.

### E — Restart reconciliation

Reconcile durable records against Git’s actual worktree list and filesystem state.

Acceptance evidence:

- missing directory, stale Git registration, existing clean worktree, and dirty orphan each produce deterministic health/state;
- restart does not reclaim an active matching lease until owning run state is reconciled.

### F — Protocol/docs

Add bounded inspection/cleanup DTOs and update architecture docs.

## 8. Failure, cancellation, restart, and contention semantics

- Reserve succeeds but Git create fails: mark failed/orphan-cleanup-needed and remove only verified empty/safe artifacts.
- Process crashes after Git create but before durable `Ready`: startup reconciliation discovers the worktree and either completes adoption for the same reserved ID/generation or marks orphaned; never create a second overlapping worktree blindly.
- Release races run completion/retry: generation check prevents stale release.
- Cleanup races external user edits: refresh status immediately before remove and fail safe on any change/unknown state.
- Repository-level Git lock contention: return/requeue according to scheduler/exclusivity semantics, not spin or force-delete locks.
- Daemon restart: no destructive automatic cleanup until durable run owner state and actual Git state are both known.
- Cancellation during preparation: cancel where safe, then reconcile actual Git state; do not assume no worktree was created merely because cancellation arrived.

## 9. Compatibility and migration

- Existing worktree list/create/remove APIs remain available for explicit user actions.
- Managed worktree records are additive; existing manual worktrees need no migration.
- Existing `Worktree` DTO can be extended or adapted, but durable `WorktreeId`/owner state must not be faked from path/branch text.
- Keep low-level `egggit` read-only.
- Do not move all Git mutation logic into the worktree service; it owns worktree lifecycle and delegates Git operations to existing typed services.

## 10. Required tests

### Focused unit tests

- state transition table;
- deterministic/safe path and branch naming;
- lease-generation checks;
- managed-vs-external classification;
- cleanup eligibility matrix.

### Integration tests

- create/inspect/release/remove round trip;
- two concurrent worktrees from the same repository/base;
- branch/path collision handling;
- detached/head/base commit behavior;
- dirty/conflicted retention;
- manual worktree coexistence.

### Restart and recovery tests

- crash after reserve;
- crash after Git create before ready;
- restart with active owner;
- missing managed path;
- stale Git registration;
- dirty orphan.

### Contention and cancellation tests

- repository/worktree Git lock contention;
- concurrent release/remove;
- cancellation during create/remove;
- stale generation attempting mutation.

### Security and negative tests

- path traversal/symlink escape;
- malicious branch/ref inputs;
- cleanup target outside managed root;
- unmanaged worktree auto-remove refusal.

## 11. Required verification commands

Expected focused shape after M001 closes:

```bash
cargo test -p codegg-core worktree
cargo test --test worktree
cargo test --lib git
cargo test --test scheduler_contention
cargo fmt --all -- --check
```

Run existing Git/execution ownership guards if touched. One current quick verification pass at closure is sufficient; do not add new CI machinery.

## 12. Documentation updates

- `architecture/worktree.md` — durable managed service/lease lifecycle.
- `architecture/git.md` — service ownership relationship and shared env policy.
- `architecture/scheduler.md` — worktree/repository exclusivity.
- workspace/project docs if managed worktree locator semantics change.
- source roadmap status after closure.

## 13. Acceptance criteria

1. CodeGG-managed worktrees have stable `WorktreeId`, repository/workspace relation, base commit, owner, generation, lifecycle, and health state.
2. One active managed worktree lease has one owner/generation.
3. Concurrent distinct run reservations produce distinct worktrees or typed collision results without corrupting Git metadata.
4. Restart reconciles durable records with actual Git worktrees conservatively.
5. Dirty/conflicted/unknown worktrees are never force-removed automatically.
6. Manual/unmanaged worktrees are not silently claimed/deleted.
7. Safe cleanup validates owner generation and current Git state immediately before removal.
8. Existing hardened typed Git/worktree primitives remain the execution boundary.
9. Existing manual worktree UX remains compatible.
10. Focused concurrency/restart/security tests pass.

## 14. Stop conditions

Stop if:

- M001 does not provide durable run ownership;
- worktree service implementation requires a second Git parser/executor instead of existing typed services;
- safe cleanup cannot distinguish managed from external worktrees;
- repository identity cannot be established without path-derived durable identity;
- implementation expands into automatic child allocation/commit policy owned by M004;
- platform-specific Git behavior requires silently dropping a supported platform rather than documenting/handling it.

## 15. Closure evidence required

- implementation/review commits;
- schema/lease lifecycle matrix;
- concurrent creation/lease tests;
- dirty/conflict/manual-worktree cleanup evidence;
- restart reconciliation fixtures;
- path/ref/symlink negative tests;
- exact verification commands/outcomes;
- unresolved findings and closure recommendation.

## 16. Handoff notes

Keep this service thin over existing Git authority. The value is durable ownership and conservative lifecycle management, not a second Git abstraction. Avoid premature automatic cleanup sophistication; retaining a failed worktree for inspection is preferable to destructive guessing.
