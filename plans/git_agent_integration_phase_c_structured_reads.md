# Git Agent Integration Phase C — Structured Read Operations

## Objective

Move common Git inspection onto the unified Git service and return structured repository facts rather than relying primarily on human-oriented stdout. Expand `egggit` only where needed for reusable read-only primitives, then migrate Bash routing, native tools, projectors, commit/review preparation, prompt context, and TUI consumers to those results.

## Dependencies

Phases A and B must be complete. The Git execution request, route, repository resolution, risk model, and RunStore provenance must already be stable.

## Required deliverables

### 1. Rich structured status

Implement a status parser based on `git status --porcelain=v2 -z --branch` or an equivalently robust machine-readable form.

Return:

- canonical repository root;
- branch or detached/unborn HEAD state;
- HEAD object id when available;
- upstream ref;
- ahead/behind counts;
- staged entries;
- unstaged entries;
- untracked entries;
- ignored entries only when requested;
- conflict entries with index stages;
- rename/copy source and destination where available;
- submodule state;
- active merge/rebase/cherry-pick/revert/bisect state where discoverable read-only.

Preserve the existing high-level `RepoStatus` API through an adapter or deprecation path.

### 2. Structured diff requests and results

Support:

- working tree, staged, HEAD, commit/range, and merge-base modes;
- path-scoped diffs;
- name-only/name-status/stat/numstat summaries;
- per-file patch retrieval;
- context-line selection;
- binary file metadata;
- rename/copy metadata;
- no-index mode only if policy can safely contain both paths;
- patch validation using existing `egggit` facilities.

A structured result should retain raw patch text and parsed file/hunk metadata. Do not attempt to create a complete semantic AST for arbitrary patch content.

### 3. Log/show/ref inspection

Add typed results for common agent needs:

- recent commits with oid, parents, author/committer metadata, timestamp, subject, decorations;
- show commit metadata and optional patch/stat;
- branch list with current/upstream/ahead-behind where available;
- tag list;
- remote list and sanitized URLs;
- changed-files queries between refs;
- blame where requested;
- worktree list through existing `egggit` support.

Use NUL-delimited or explicit record separators for machine parsing. Avoid locale-sensitive parsing.

### 4. Unified read execution

Implement typed read variants in the Git execution service by delegating to `egggit`. Return `GitExecutionResult` with:

- operation kind;
- structured payload enum;
- raw stdout/stderr where applicable;
- repository snapshot;
- elapsed time and exit status/provenance;
- projection hints.

Read failures must not mutate repository state and must retain diagnostic output.

### 5. Projection migration

Update Git status, diff, and log projectors to accept structured results.

Requirements:

- concise default status with staged/unstaged/untracked/conflicted separation;
- preserve exact paths, refs, object ids, hunk headers, and line coordinates;
- include truncation metadata;
- allow raw fallback when structured parse fails;
- RTK compression must preserve diff hunks and exact spans required for edits;
- add projection tests for large diffs, binary files, renames, conflicts, detached HEAD, and unborn repositories.

### 6. Native Git tool read actions

Expose action-oriented schemas for common reads:

- status;
- diff;
- log;
- show;
- changed files;
- file diff;
- branches;
- tags;
- remotes;
- worktrees;
- blame;
- patch validation.

The old generic `subcommand + args` path may remain as an explicitly labeled advanced compatibility action.

### 7. TUI integration

Migrate the Git sidebar probe to the unified structured status path while preserving:

- background execution;
- timeout;
- generation-based stale completion rejection;
- render purity;
- graceful behavior outside a Git repository.

Consider surfacing staged/unstaged/conflict counts and ahead/behind state without blocking the initial migration.

### 8. Prompt/context integration

Replace ad hoc Git context formatting with structured status-derived context. Keep prompt injection bounded and deterministic.

Recommended context:

- root identity;
- branch/detached state;
- HEAD short oid;
- dirty summary counts;
- conflict/active-operation warning;
- upstream/ahead-behind when known.

Do not inject full status or diff automatically unless context policy requests it.

### 9. Review tool correction

Refactor `ReviewTool` to use the unified diff request and reclassify it as read-only or read-only-with-model-inference. Ensure permission handling reflects repository reads rather than mutation.

### 10. Compatibility and fallback

Keep raw output available for commands or Git versions that cannot be fully parsed. Structured parse failure must be explicit in provenance and must not silently return incomplete facts as complete.

## Likely files

- `crates/egggit/src/status.rs`, `diff.rs`, `worktree.rs`, new log/ref modules;
- Git orchestration read executor and result types;
- shell projection modules and golden fixtures;
- `src/tool/git.rs`;
- `src/tool/review.rs`;
- TUI Git sidebar command/state/render code;
- prompt/context assembly;
- architecture documentation.

## Test fixtures

Create temporary repository fixtures for:

- clean repository;
- unborn repository;
- detached HEAD;
- staged/unstaged/untracked combinations;
- spaces and Unicode paths;
- rename and copy detection;
- binary files;
- submodules where practical;
- merge conflicts;
- active rebase/cherry-pick/revert states;
- branches with upstream ahead/behind;
- multiple worktrees;
- large diffs requiring projection truncation.

## Validation

Run `egggit`, Git service, projection, TUI state, review, and command-routing tests. Validate against the minimum supported Git version and at least one current Git release in CI where feasible.

## Exit criteria

Phase C is complete when:

- status, diff, log, show, changed files, refs, remotes, and worktrees use structured read paths;
- Bash and native reads receive equivalent structured results;
- TUI and prompt context consume the unified status model;
- review is correctly treated as read-only;
- projectors preserve exact Git semantics and raw fallback;
- legacy high-level APIs remain compatible or have a documented migration path;
- all read operations remain mutation-free and permission-light.

## Handoff to Phase D

Phase D should build local state transitions on these snapshots and result types rather than introducing separate status or diff probes.