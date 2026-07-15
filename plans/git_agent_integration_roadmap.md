# Git Agent Integration Roadmap

## Status

Proposed implementation roadmap for consolidating Codegg's Git support into a single typed, policy-aware execution path shared by native agent tools, Bash command translation, TUI consumers, commit/review workflows, projection, and run provenance.

## Problem statement

Codegg already contains several useful Git capabilities, but they are distributed across multiple paths:

- `crates/egggit` provides read-only repository facts and diff primitives.
- `src/tool/git.rs` exposes a generic model-facing Git command wrapper.
- `src/tool/commit.rs` implements staging, message generation, commit, and amend behavior.
- `src/tool/review.rs` reads diffs and invokes a model reviewer.
- `src/tool/bash.rs`, `src/command_intent`, and `src/command_routing.rs` classify and route some Git commands issued through Bash.
- Git status is surfaced in the TUI through an asynchronous sidebar refresh.
- shell projection already has Git-specific routes for status, diff, and log.

The current architecture understands Git semantics during classification and planning, but frequently discards them at execution time. Read-only Git commands route toward a native backend, while many mutations degrade into generic managed processes or raw shell execution. Native tool calls and Bash-translated commands therefore do not consistently share parsing, permissions, preconditions, postconditions, result schemas, provenance, or output projection.

The goal of this roadmap is to make Git a coherent subsystem from the agent perspective without reimplementing Git itself.

## Goals

1. Provide one typed `GitOperation` vocabulary for common repository reads and mutations.
2. Preserve the read-only scope and reusability of `egggit`.
3. Add a Codegg-owned Git orchestration layer for parsing, policy, mutation execution, snapshots, and structured outcomes.
4. Route native Git tool calls and eligible Bash Git commands through the same executor.
5. Preserve Git identity through planning, routing, execution, RunStore provenance, and projection.
6. Replace prefix-based risk detection with operation-aware classification.
7. Add precise permission prompts for index, worktree, ref, history, network, configuration, and destructive operations.
8. Return structured repository state and mutation deltas so agents do not need to infer outcomes from human-oriented stdout.
9. Keep compatibility fallbacks for unsupported Git plumbing and complex shell composition.
10. Improve agent ergonomics for staging, committing, branch operations, remotes, conflicts, and recovery.

## Non-goals

- Reimplement Git object storage, transport, merge algorithms, credential helpers, or hooks.
- Remove access to the system Git executable.
- Model every Git subcommand and option in the first release.
- Automatically permit remote writes or destructive operations.
- Restore automatic mutating worktree management in `codegg-core`.
- Couple `egggit` to agent permissions, TUI state, or provider/model infrastructure.

## Architectural direction

The target execution path is:

```text
Native Git tool request ----\
                            -> Git request normalization
Simple Bash `git ...` -----/          |
                                       v
                              typed GitOperation
                                       |
                         policy + repository resolution
                                       |
                              Git execution service
                                       |
                     structured result + raw diagnostics
                                       |
                 projection + RunStore + agent/TUI consumers
```

Complex shell commands remain shell commands. A Bash command is promoted only when the shell parser proves it is a simple argv command and the Git parser can represent the operation without losing semantics.

## Proposed crate and module boundaries

### `crates/egggit`

Retain as the read-only Git facts crate. Expand only where the new orchestration layer needs richer structured inspection primitives.

Expected responsibilities:

- Git root and repository validation helpers where appropriate.
- structured status parsing;
- diff text and structured diff metadata;
- changed files and per-file diff;
- log/show/read-only ref inspection;
- patch validation;
- worktree listing;
- operation-state inspection when it can remain read-only.

It must not own agent permissions, mutation execution, RunStore integration, or provider calls.

### Codegg Git orchestration layer

Create a dedicated crate or clearly isolated module, preferably `crates/codegg-git` if dependency direction remains clean.

Expected responsibilities:

- `GitOperation` and request/result types;
- argv-to-operation parser;
- risk and capability classification;
- repository/cwd/pathspec resolution;
- mutation execution via system Git;
- precondition and postcondition snapshots;
- conflict and in-progress-operation state;
- timeout and noninteractive process configuration;
- structured diagnostics and raw stdout/stderr retention;
- integration adapters for Codegg tools, command routing, and RunStore.

The implementation phase must verify the dependency graph before choosing a crate over an internal module. Avoid cycles between the main `codegg` crate, `codegg-core`, `codegg-config`, and `egggit`.

## Core data model

The exact enum should be refined during Phase A, but the architecture should support at least these operation families:

```rust
pub enum GitOperation {
    Status(StatusRequest),
    Diff(DiffRequest),
    Show(ShowRequest),
    Log(LogRequest),
    ChangedFiles(ChangedFilesRequest),
    Blame(BlameRequest),
    BranchList(BranchListRequest),
    RemoteList(RemoteListRequest),
    TagList(TagListRequest),
    WorktreeList,

    Stage(StageRequest),
    Unstage(UnstageRequest),
    Commit(CommitRequest),
    Stash(StashRequest),
    Switch(SwitchRequest),
    Restore(RestoreRequest),
    BranchCreate(BranchCreateRequest),
    BranchDelete(BranchDeleteRequest),
    TagCreate(TagCreateRequest),
    TagDelete(TagDeleteRequest),
    Merge(MergeRequest),
    Rebase(RebaseRequest),
    CherryPick(CherryPickRequest),
    Revert(RevertRequest),

    Fetch(FetchRequest),
    Pull(PullRequest),
    Push(PushRequest),

    Reset(ResetRequest),
    Clean(CleanRequest),
    RemoteMutation(RemoteMutationRequest),
    ConfigMutation(ConfigMutationRequest),

    Continue(OperationContinueRequest),
    Abort(OperationAbortRequest),
    Skip(OperationSkipRequest),

    ManagedArgv(ManagedGitArgv),
}
```

The typed model must preserve literal repository-relative paths separately from raw Git pathspecs. Safe native operations should use literal paths and insert `--` before path arguments. Raw pathspec syntax should require an explicit advanced representation.

## Execution tiers

The finished subsystem should support three tiers:

1. **Typed native Git operation**: complete parse, precise policy, structured result.
2. **Managed Git argv**: simple argv and repository-scoped execution with Git-aware policy and provenance, but raw output when semantics are not fully modeled.
3. **Raw shell**: complex shell syntax, composition, substitution, environment prefixes, pipelines, or unsupported shell semantics.

Promotion must be monotonic and lossless. Failure to parse must never cause Codegg to reinterpret a command approximately.

## Risk model

Replace the binary read/mutate split with a Git-specific risk taxonomy:

- read-only repository inspection;
- index mutation;
- worktree mutation;
- ref mutation;
- history integration;
- network read;
- network write;
- repository configuration mutation;
- destructive worktree mutation;
- destructive history/ref mutation;
- outside-project repository access.

This taxonomy must map into existing execution capabilities and permission infrastructure rather than creating a parallel authorization system.

## Permission defaults

Initial policy target:

- read-only operations: allow;
- explicitly scoped staging/unstaging: allow or configurable;
- stage all: ask;
- commit/amend: ask, with explicit amend acknowledgement;
- branch create/switch/stash/restore/merge/rebase/cherry-pick/revert: ask;
- fetch: network permission, configurable default;
- pull/push: ask;
- force-with-lease: strong confirmation;
- plain force push: deny by default;
- hard reset, clean, forced branch deletion: deny by default;
- global/system Git configuration mutation: deny;
- access to a different repository outside the active project: deny or explicit outside-workspace permission.

Permission messages must explain the actual state transition and relevant preconditions.

## Repository state model

Add a richer status representation, preferably using `git status --porcelain=v2 -z`, including:

- branch or detached HEAD;
- HEAD object id;
- upstream ref;
- ahead/behind counts;
- staged entries;
- unstaged entries;
- untracked entries;
- conflicts and conflict stages;
- rename/copy metadata where available;
- active merge/rebase/cherry-pick/revert state.

Mutation execution should capture before and after snapshots and return a typed delta containing created commits, changed refs, changed paths, conflicts, remote updates, and final dirty state.

## Bash translation requirements

The Bash layer must:

- require `SimpleArgv` shape for native Git promotion;
- use the parsed argv without whitespace-splitting fallback;
- parse Git global options before the subcommand, including supported `-C` handling only when repository policy can validate it;
- distinguish read-only and mutating forms of overloaded commands such as `branch`, `tag`, `remote`, `stash`, `reset`, and `restore`;
- preserve unsupported simple commands as `ManagedGitArgv` rather than generic managed process where possible;
- leave pipelines, conditionals, substitutions, redirects, shell environment prefixes, and compound commands in the shell path;
- retain Git-aware risk metadata even for raw-shell fallback;
- never execute both translated and fallback forms of the same command.

## Native tool requirements

Replace the generic `subcommand + args` model as the preferred agent interface with action-oriented structured input. Retain an explicit advanced/raw argv escape hatch.

The tool should expose stable operations such as status, diff, stage, unstage, commit, switch, branch creation, stash, merge, rebase, fetch, pull, push, and recovery commands. Unsupported operations should produce a deterministic unsupported result or use the explicit managed argv tier.

## Commit and review integration

Refactor `CommitTool` into a workflow client of the Git service. Explicitly model commit selection:

- already staged;
- stage named paths;
- stage all.

The commit workflow should inspect state, select/stage content, obtain the exact staged diff, optionally generate a message, revalidate HEAD/index state, execute the commit, and verify the resulting commit.

Reclassify `ReviewTool` as read-only and make it consume the same structured diff request path. Provider/model inference is not a repository mutation.

## Projection and context integration

Retain dedicated status, diff, and log projectors but allow them to consume structured results. Add Git mutation, conflict, and network projection routes.

Projection must preserve:

- file paths;
- ref names;
- commit ids;
- hunk headers;
- line numbers;
- conflict markers and stages;
- rejected remote refs;
- truncation metadata;
- exact spans needed for follow-up edits.

RTK compression may reduce redundant context but must not alter patch or conflict semantics.

## RunStore and observability

Record Git as a first-class planned and actual backend. Preserve:

- command origin: native tool, Bash translation, TUI/workflow;
- operation kind;
- risk class;
- repository root identity;
- planned tier and actual tier;
- permission decisions;
- fallback reason;
- before/after HEAD where applicable;
- exit status and timeout;
- projection route;
- structured result availability.

Do not store secrets, credential output, full environment values, or sensitive remote URLs containing credentials.

## Process execution constraints

All Git subprocesses must:

- clear the environment and restore only an explicit allowlist;
- preserve required Git identity and credential-helper behavior deliberately rather than accidentally;
- set noninteractive controls such as `GIT_TERMINAL_PROMPT=0` where appropriate;
- disable editor spawning unless an operation explicitly supports an editor workflow;
- use bounded timeouts and `kill_on_drop`;
- retain raw stdout/stderr for diagnostics;
- redact credentials and sensitive URL components;
- avoid shell invocation for argv-based execution.

The implementation must determine which environment variables are needed for signing, hooks, SSH, credential helpers, locale, and user identity. This is a security-sensitive compatibility decision and requires dedicated tests.

## Testing strategy

Testing must include:

- pure parser unit tests for every supported command shape;
- table-driven risk classification tests;
- adversarial quoting and pathspec tests;
- overloaded-subcommand mutation tests;
- simple argv versus complex shell promotion tests;
- temporary real Git repository integration tests;
- before/after snapshot tests;
- commit hook and noninteractive behavior tests;
- merge/rebase/cherry-pick conflict fixtures;
- remote tests using local bare repositories;
- force/lease and rejected push tests;
- RunStore provenance tests;
- projection golden tests;
- no-double-execution regression tests;
- cross-platform tests where behavior differs.

Tests should remain compatible with the repository's constrained test-thread policy and avoid unnecessary process/memory fan-out.

## Migration strategy

Adopt an additive migration:

1. Introduce types and parser without changing execution.
2. Preserve Git identity through planning/routing.
3. Route read-only commands through the unified service.
4. Route controlled local mutations.
5. Add network and destructive policies.
6. Add conflict/recovery ergonomics and remove obsolete duplicate paths.

Keep compatibility fallbacks until equivalent coverage and telemetry demonstrate that removal is safe.

## Phase summary

### Phase A — typed operation model and parser

Define the operation vocabulary, parser, risk classes, pathspec/ref types, structured errors, and exhaustive tests. No broad execution behavior change.

### Phase B — unified planning, routing, and provenance

Introduce a Git-specific execution backend and routing decision. Make native and Bash-origin Git requests converge and preserve Git identity in RunStore.

### Phase C — structured read operations

Expand `egggit` where necessary, route status/diff/log/show and related reads through the unified service, and migrate projectors/TUI consumers.

### Phase D — controlled local mutations and workflow refactors

Implement staging, commit, branch, switch, restore, stash, merge, rebase, cherry-pick, and revert with snapshots and precise permissions. Refactor commit/review tools.

### Phase E — network, configuration, and destructive operations

Add fetch/pull/push, remotes, selected configuration changes, reset, clean, forced deletion, and force-push policy with local bare-remote tests.

### Phase F — conflicts, recovery, ergonomics, and closure ✅

Add operation-state discovery, continue/abort/skip flows, conflict projections, TUI/prompt integration, compatibility cleanup, docs, and closure validation.

#### Phase F completion summary

- **Completion date:** 2026-07-14
- **Key deliverables:**
  - `RepositoryOperationState` with eight operation families (merge, rebase, cherry-pick, revert, bisect, apply-mailbox, sequencer, unknown) and `RecoveryAction` enum
  - `ConflictEntry`, `ConflictKind`, `ConflictShape`, `ConflictReport` typed conflict model
  - `continue_in_progress`, `abort_in_progress_typed`, `skip_in_progress` recovery functions
  - `git` tool exposes `operation_state` and `recover` parameters
  - TUI sidebar caches operation state, available actions, and conflicted paths
  - Agent prompts updated with Phase F git workflow guidance
  - RunStore persistence for recovery actions (`RunKind::GitMutation`)
  - Schema snapshot tests pinning mutation and recover enums
- **Handoff artifact:** [`architecture/git_phase_f_handoff.md`](../architecture/git_phase_f_handoff.md)

## Cross-phase invariants

Every phase must preserve these invariants:

1. No Git command is executed twice because routing failed after partial execution.
2. Native promotion occurs only for proven simple argv commands.
3. Parsed argv is never reconstructed by whitespace splitting for active Git routing.
4. Unsupported syntax falls back conservatively without changing semantics.
5. Read-only operations do not require mutation permission.
6. Destructive operations are never downgraded to a safer risk class by parser failure.
7. The active repository root is canonicalized and policy-checked.
8. Literal paths are passed after `--` and cannot escape the repository.
9. Raw stdout/stderr remain available even when a structured result is produced.
10. RunStore records planned versus actual execution accurately.
11. Complex shell behavior remains owned by the shell executor.
12. `egggit` remains free of agent- and UI-specific mutation policy.

## Definition of done

The roadmap is complete when:

- common agent Git workflows use structured native operations by default;
- simple Bash Git commands route through the same Git service;
- unsupported simple Git commands retain a Git-specific managed argv path;
- complex shell Git commands remain safely in the shell path;
- permissions distinguish operation risk precisely;
- status and mutation outcomes are structured and verifiable;
- commit and review no longer duplicate Git execution logic;
- [x] conflicts and active operations have first-class recovery commands;
- projection and RunStore retain Git semantics end to end;
- destructive and remote operations have adversarial integration coverage;
- obsolete generic paths are removed or explicitly retained as documented compatibility escapes;
- architecture and agent documentation describe the final model accurately.

## Completion status

**Phases A–F complete** (as of 2026-07-14). All six phases of the Git agent integration roadmap have been implemented.

## Post-closure pass

After the corrective security closure (`cb192e9`, `c2e806f`, `53b2beb`) closed the two Phase F findings, a polish / maintainability / verification pass was executed per [`plans/git_agent_integration_polish_maintainability_verification.md`](git_agent_integration_polish_maintainability_verification.md). The pass did not add new Git capability; it tightened three invariants:

1. **Canonical subprocess policy** — `ALLOWED_ENV_VARS` and `ALWAYS_STRIPPED_ENV_VARS` now live in `codegg_git::process_policy` and are re-exported by both `src/git_mutations.rs` and `crates/codegg-core/src/worktree.rs`. Drift is caught by `cargo test -p codegg-core` (`worktree_uses_canonical_policy`, `canonical_includes_locally_drifted_entries`) and `src/git_mutations.rs::policy_drift_tests`.
2. **Audit-safe rerun argv** — `RerunDescriptor.argv` is `Option<AuditSafeArgv>` (newtype in `codegg_git::sensitive`). The only construction path (`AuditSafeArgv::from_argv`) runs the URL sanitizer; durable RunStore records are credential-free. See [`docs/validation/git-rerun-secret-lifecycle.md`](../docs/validation/git-rerun-secret-lifecycle.md).
3. **Forbidden-pattern static checks** — `scripts/check_git_forbidden_patterns.py` enforces `expose_secret()` only at `render_argv`, no hand-maintained env-policy tables, `RerunDescriptor.argv` is always `AuditSafeArgv`, git argv in `RunInvocation` is sanitized.

Verified state and remaining limitations are recorded in [`architecture/git_polish_verification_handoff.md`](../architecture/git_polish_verification_handoff.md).

## Handoff notes

Implement phases in order. Phase A and B establish contracts used by all later work. Do not expand mutation coverage before routing and provenance preserve Git identity. Each phase plan in this series contains scoped deliverables, file-level guidance, validation requirements, and exit criteria.