# Git Agent Integration Phase F — Conflicts, Recovery, Ergonomics, and Closure

## Objective

Complete the Git integration by making in-progress operations and conflicts first-class, improving agent and TUI ergonomics, removing obsolete duplicate paths, documenting the final architecture, and running a systematic closure pass across routing, policy, execution, projection, provenance, and compatibility.

## Dependencies

Phases A-E must be complete. Typed reads, local mutations, network operations, destructive policy, structured snapshots, and RunStore provenance must all be available.

## Required deliverables

### 1. Repository operation-state discovery

Add a structured read-only model for active repository operations:

```rust
pub enum RepositoryOperationState {
    None,
    Merge(MergeState),
    Rebase(RebaseState),
    CherryPick(SequenceState),
    Revert(SequenceState),
    Bisect(BisectState),
    ApplyMailbox(ApplyState),
    Sequencer(SequencerState),
    Unknown(UnknownOperationState),
}
```

Expose:

- operation type;
- original/current HEAD where available;
- target/base/upstream refs where available;
- current step and total steps where safely discoverable;
- conflicted paths;
- staged resolutions still pending;
- whether continue/abort/skip is available;
- sanitized state-path diagnostics for unsupported states.

Use Git plumbing/commands where possible rather than depending only on undocumented internal files. When filesystem state is required, isolate version-sensitive logic and test it.

### 2. Conflict model

Define typed conflict entries including:

- repository-relative path;
- conflict kind/status code;
- base/ours/theirs object ids and modes where available;
- rename/delete/add combinations;
- whether the worktree file contains conflict markers;
- whether the path is staged as resolved;
- submodule conflicts;
- recommended next legal operations.

Do not automatically select ours/theirs or edit conflict markers in this phase. Existing editing agents may resolve files through normal file tools, after which Git status should verify resolution.

### 3. Continue, abort, and skip operations

Implement explicit operations for:

- merge continue/abort;
- rebase continue/abort/skip;
- cherry-pick continue/abort/skip where Git supports it;
- revert continue/abort/skip where supported;
- sequencer continuation only when operation identity is known.

Requirements:

- validate active state matches requested recovery action;
- prevent cross-operation misuse;
- capture before/after snapshots;
- require permission for state-changing recovery;
- disable editor prompts or provide an explicit message path;
- return completed/conflicted/still-in-progress/aborted/no-op outcomes;
- preserve raw diagnostics.

### 4. Agent workflow ergonomics

Update built-in agent prompts/tool descriptions so agents:

- prefer structured Git actions over Bash for supported operations;
- understand that simple Bash Git commands are translated automatically;
- inspect status before potentially destructive state transitions;
- stage explicit paths rather than stage-all unless intended;
- use the commit workflow for message generation;
- respond to conflict results by editing files, staging resolutions, then continuing;
- never attempt destructive cleanup or force push without explicit authorization;
- use managed/raw Git only for unsupported advanced plumbing.

Keep prompt guidance concise and avoid duplicating the complete Git manual.

### 5. Native tool schema polish

Review the final model-facing schema for:

- discoverable action names;
- consistent path/ref fields;
- mutually exclusive inputs encoded clearly;
- defaults that do not imply broad mutation;
- explicit advanced/raw action;
- structured errors with remediation hints;
- bounded output controls;
- backward compatibility for existing tool calls if required.

Add schema snapshot tests so accidental breaking changes are visible.

### 6. TUI integration

Extend TUI Git presentation without introducing synchronous rendering work.

Candidate capabilities:

- branch/detached state;
- staged/unstaged/untracked/conflict counts;
- ahead/behind upstream state;
- active merge/rebase/cherry-pick/revert indicator;
- last Git operation result;
- permission prompt summaries;
- conflict/recovery action availability;
- refresh after successful Git mutation.

All probing remains background, timeout-bounded, generation-safe, and cached.

### 7. Projection closure

Finalize structured projectors for:

- status;
- diff/log/show;
- local mutation;
- network mutation;
- destructive preview/result;
- conflict state;
- recovery result;
- managed Git argv fallback.

Audit truncation and RTK behavior to ensure preservation of:

- refs and object ids;
- paths;
- hunk headers and line numbers;
- conflict stages;
- rejected ref reasons;
- force modes;
- recovery commands;
- parse/fallback warnings.

Add golden fixtures for every operation family.

### 8. RunStore and observability closure

Audit planned and actual backend data for all origins and tiers. Ensure:

- no Git operation appears as generic managed process unless explicitly retained for compatibility;
- origin is accurate;
- operation/risk/tier are populated;
- before/after HEAD and branch are recorded when appropriate;
- fallback and structured-parse failures are visible;
- credentials and sensitive URLs never persist;
- conflict and recovery outcomes are queryable;
- metrics do not have unbounded cardinality from paths, refs, or URLs.

Add migration/default handling for older records and documentation for schema fields.

### 9. Compatibility cleanup

Inventory and remove or narrow obsolete paths:

- duplicated Git subprocess helpers;
- prefix-based risk detection used where typed parsing is available;
- legacy `GitMutating` routing variants;
- read-only commands executed through the generic Git wrapper by default;
- direct commit subprocess logic replaced by workflow service;
- stale architecture documentation describing removed session/worktree mutation behavior;
- redundant status probes;
- generic whitespace-splitting fallback for Git.

Retain only documented compatibility escapes. Mark them clearly in code and architecture docs.

### 10. Architecture and user documentation

Update:

- Git architecture document;
- command-intent/planner/routing documents;
- tool architecture;
- permission model;
- RunStore/provenance docs;
- Bash translation behavior;
- TUI Git state behavior;
- agent instructions;
- configuration reference;
- troubleshooting guide for credentials, hooks, signing, conflicts, detached HEAD, and unsupported Git commands.

Documentation must distinguish `egggit` read-only primitives from Codegg Git orchestration.

### 11. Security review

Perform a focused review of:

- path traversal and pathspec injection;
- revision names beginning with `-`;
- option smuggling around `--`;
- repository root escape via `-C`, symlinks, submodules, and worktrees;
- hostile Git config and aliases;
- external diff/textconv/filter/credential/helper/hook execution;
- pager/editor/sequence-editor spawning;
- SSH command/config injection;
- credential leakage;
- malicious repository filenames and output control sequences;
- race conditions between precondition snapshot and mutation;
- raw/managed fallback bypass;
- force/destructive misclassification;
- no-double-execution guarantees.

Decide explicitly whether Git aliases are ignored, resolved, or managed-fallback only. Prefer invoking built-in subcommands in a way that avoids alias ambiguity for typed operations.

### 12. Performance and resource review

Measure:

- status/sidebar refresh latency;
- process count per operation;
- large repository status/diff behavior;
- RunStore overhead;
- projection memory usage;
- timeout behavior;
- test suite wall-clock and memory.

Consolidate redundant probes where safe, but do not combine operations in ways that obscure failure or weaken correctness.

### 13. Cross-platform review

Validate supported behavior on primary platforms, especially:

- path encoding and separators;
- executable discovery;
- process termination;
- HOME/XDG and credential-helper behavior;
- SSH agent handling;
- temporary repository fixtures;
- file permission and symlink behavior;
- newline and NUL-delimited parsing.

Document unsupported or degraded behavior rather than silently diverging.

## Systematic closure test matrix

Run end-to-end scenarios from both native tool calls and Bash translation:

1. inspect clean/dirty/unborn/detached repositories;
2. inspect large staged and unstaged diffs;
3. stage named paths, unstage, commit, amend;
4. create/switch/delete branches;
5. stash push/apply/pop conflicts;
6. merge, rebase, cherry-pick, and revert success/conflict/recovery;
7. fetch/pull/push normal and rejected cases;
8. force-with-lease success and stale lease;
9. destructive operations denied by default;
10. reset/clean preview and authorized execution;
11. remote/config mutation policy;
12. nested repository/submodule/worktree resolution;
13. hostile filenames, refs, config, hooks, aliases, and output;
14. timeout and interrupted subprocess;
15. permission denial before spawn;
16. structured parser failure and managed fallback;
17. complex shell command remains shell-owned;
18. no duplicate execution after any failure;
19. RunStore/projection/TUI state agree with actual repository state;
20. restart/reload behavior with persisted records and configuration.

## Validation commands

Use the repository's actual package names and constrained test policy. The closure pass should include:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p egggit
cargo test -p codegg-git
cargo test -p codegg --lib command_intent
cargo test -p codegg --test command_routing_adversarial
cargo test --workspace
```

Split or serialize expensive integration suites as necessary to avoid the repository's known process/thread and memory pressure.

## Exit criteria

Phase F and the roadmap are complete when:

- conflicts and in-progress operations are structured and recoverable;
- continue/abort/skip actions are operation-aware and safe;
- native and Bash Git workflows are equivalent for supported simple commands;
- agent prompts and tool schemas guide structured use by default;
- TUI state refreshes after operations and surfaces conflicts without blocking render;
- projections and RunStore preserve Git semantics across every operation family;
- obsolete duplicate execution and prefix-risk paths are removed;
- security, performance, cross-platform, and adversarial closure tests pass;
- documentation reflects the final architecture;
- remaining managed/raw fallbacks are explicit, conservative, tested, and documented.

## Final handoff artifact

At completion, add a concise implementation report describing:

- final architecture and crate boundaries;
- supported typed operation matrix;
- permission defaults;
- managed/raw fallback matrix;
- known limitations;
- test coverage and commands run;
- migration/deprecation status;
- recommended future work such as deterministic hunk staging, PR hosting-provider integration, or optional isolated worktree workflows.