# Git Agent Integration Phase D — Controlled Local Mutations and Workflow Refactors

## Objective

Implement structured local Git mutations with precise permissions, repository preconditions, before/after snapshots, typed outcomes, and recovery-safe execution. Refactor the existing commit and review workflows to consume the unified Git service rather than duplicating subprocess logic.

## Dependencies

Phases A-C must be complete. Structured status and diff results must be stable enough to serve as mutation preconditions and postconditions.

## Required deliverables

### 1. Mutation execution framework

Add a reusable executor for local Git mutations.

Each operation must:

1. resolve and policy-check the repository root;
2. capture a pre-operation snapshot;
3. validate operation-specific preconditions;
4. request/verify permission before spawning;
5. render argv without a shell;
6. execute with timeout and noninteractive controls;
7. capture raw stdout/stderr and exit status;
8. capture a post-operation snapshot even on nonzero exit where safe;
9. classify conflicts or partial state;
10. return a typed state delta and provenance.

Fallback to shell is not permitted after the mutation subprocess has started.

### 2. Process environment policy

Define and test the explicit environment allowlist required for local Git operations.

Address:

- `PATH`, locale, HOME/XDG behavior where necessary;
- author/committer identity discovery;
- hooks;
- signing configuration and pinentry/editor behavior;
- `GIT_TERMINAL_PROMPT=0`;
- `GIT_EDITOR`, `GIT_SEQUENCE_EDITOR`, and merge editors;
- SSH command/config behavior for operations that unexpectedly contact remotes;
- credential redaction.

Local operations must not hang waiting for an editor or terminal prompt.

### 3. Stage and unstage

Implement:

- stage named literal paths;
- stage all tracked/untracked changes explicitly;
- update tracked files only;
- unstage named paths;
- unstage all;
- intent-to-add only if clearly modeled;
- patch/hunk staging deferred unless a deterministic patch-selection interface exists.

Return index changes and post-operation status. Use `--` before literal paths.

### 4. Commit workflow

Refactor `CommitTool` around explicit selection:

```rust
pub enum CommitSelection {
    AlreadyStaged,
    StagePaths(Vec<RepoRelativePath>),
    StageAll,
}
```

Workflow:

- inspect status and active operation state;
- apply selection;
- obtain exact staged diff;
- fail if staged diff is empty unless explicitly allowing an empty commit;
- optionally generate a message through the configured provider;
- validate/sanitize message and trailers;
- recheck HEAD and staged state before commit;
- execute normal commit or amend;
- require explicit amend acknowledgement;
- verify created/replaced commit oid;
- return final dirty state and remaining unstaged changes.

Do not let message generation own repository mutation. Keep provider errors separate from Git errors.

### 5. Branch create and switch

Implement:

- create branch from validated start point;
- switch existing branch;
- create-and-switch;
- detach at revision only with explicit request;
- orphan branch only as advanced/high-risk operation;
- reject or ask when local changes may be overwritten;
- report upstream and final HEAD state.

Avoid ambiguous use of legacy checkout when switch semantics are sufficient. Retain managed fallback for advanced checkout forms.

### 6. Restore and checkout paths

Implement explicit worktree/index restoration modes:

- restore worktree from index/HEAD/source;
- restore staged state;
- restore both only with clear request;
- named literal paths required by default;
- detect likely data loss and classify accordingly;
- forced overwrites must not be auto-allowed.

### 7. Stash operations

Implement structured:

- list/show as reads;
- push with message, include-untracked, and keep-index options;
- apply/pop by validated stash reference;
- drop/clear with elevated policy;
- branch-from-stash as advanced managed fallback initially.

Capture conflict state and distinguish apply success from pop drop behavior.

### 8. History integration operations

Implement merge, rebase, cherry-pick, and revert with initial scope:

- noninteractive ordinary forms;
- validated refs/commits;
- explicit strategy/options allowlist;
- no arbitrary strategy command execution;
- precondition checks for dirty state and active operation;
- typed result distinguishing completed, no-op, fast-forward, conflict, and rejected;
- leave repository in recoverable Git-native state on conflict;
- no automatic conflict resolution in this phase.

Interactive rebase remains managed fallback or unsupported until an explicit sequence-editor design exists.

### 9. Local ref deletion

Implement safe branch/tag deletion distinctions:

- merged branch deletion versus forced deletion;
- current branch cannot be deleted;
- force deletion is destructive and denied by default;
- tag deletion is ref mutation and asks;
- report deleted oid/ref.

### 10. Permission messages

Generate state-aware permission descriptions. Examples should indicate:

- path count and scope for stage/restore;
- current and target branch for switch;
- staged file count and amend status for commit;
- stash options;
- source and target refs for integration operations;
- possible worktree overwrite or history rewrite.

### 11. Mutation projections

Add a Git mutation projector that returns concise, structured summaries:

- operation performed;
- before/after HEAD and branch;
- created commit/ref;
- affected paths;
- conflicts;
- remaining dirty state;
- recovery instructions where relevant;
- raw diagnostics reference when truncated.

### 12. Native Git tool mutation actions

Expose structured schemas for stage, unstage, commit, branch create/delete, switch, restore, stash, merge, rebase, cherry-pick, and revert. The model-facing tool should prefer these actions over raw argv.

## Likely files

- Git orchestration mutation executor;
- operation-specific modules;
- `src/tool/git.rs`;
- `src/tool/commit.rs`;
- `src/tool/review.rs` cleanup if not completed in C;
- permission generation and descriptions;
- projectors and fixtures;
- RunStore mutation metadata;
- agent prompt/tool guidance;
- integration tests using temporary repositories.

## Test matrix

Test successful and failure paths for:

- named staging/unstaging and stage-all;
- empty commit, normal commit, amend, hooks, signing/editor failure;
- concurrent HEAD/index change detected between preparation and commit;
- branch create/switch with clean and dirty worktrees;
- restore staged/worktree and data-loss policy;
- stash push/apply/pop conflicts;
- merge fast-forward, merge commit, no-op, conflict;
- rebase success/conflict;
- cherry-pick success/conflict;
- revert success/conflict;
- branch/tag deletion policy;
- timeout and killed subprocess;
- post-failure snapshot capture;
- Bash versus native equivalence;
- no-double-execution on nonzero exit.

## Validation

Run local Git mutation integration tests against real temporary repositories, then the command-routing, permission, projection, commit, review, and RunStore suites. Keep tests serialized or resource-bounded where required.

## Exit criteria

Phase D is complete when:

- common local mutations are typed and share one executor;
- every mutation captures pre/post state and returns a typed delta;
- commit no longer owns duplicate Git subprocess logic;
- review is read-only and shares diff infrastructure;
- permissions describe actual state transitions;
- conflict outcomes are recognized without unsafe automatic recovery;
- native and Bash-origin operations are behaviorally equivalent;
- unsupported advanced forms fall back conservatively.

## Handoff to Phase E

Phase E should extend the same execution and policy model to remote/network, repository configuration, and explicitly destructive operations.