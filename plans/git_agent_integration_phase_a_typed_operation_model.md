# Git Agent Integration Phase A — Typed Operation Model and Parser

## Objective

Establish the typed Git vocabulary and parser contracts that all later native-tool, Bash-routing, policy, execution, projection, and provenance work will consume. This phase is intentionally architecture-first: it must not broadly change runtime behavior.

## Scope

Create the operation/request/result foundations; parse eligible simple argv Git commands; classify risk and capabilities from complete semantics; define repository-relative path, ref, pathspec, and parser error types; add exhaustive table-driven tests.

## Required deliverables

### 1. Resolve module/crate placement

Inspect the workspace dependency graph and select either:

- a new `crates/codegg-git` orchestration crate depending on `egggit`; or
- a temporary isolated module under the main crate if a new crate would introduce dependency cycles.

Document the decision. The public model must not depend on TUI, provider, or Bash implementation types.

### 2. Define source metadata

Add an origin type such as:

```rust
pub enum GitCommandOrigin {
    NativeTool,
    BashTranslation,
    Workflow,
    Tui,
}
```

The origin must be metadata only and must not change operation semantics.

### 3. Define operation families

Implement typed variants for the roadmap's read, local mutation, network, destructive, configuration, and recovery families. Requests should be explicit rather than boolean-heavy.

At minimum model:

- status, diff, show, log, changed files, blame;
- branch/remote/tag/worktree listing;
- stage and unstage;
- commit selection and amend acknowledgement;
- stash list/show/push/apply/pop/drop;
- switch/checkout branch versus checkout paths;
- restore staged versus worktree;
- branch/tag create/delete;
- merge, rebase, cherry-pick, revert;
- fetch, pull, push and force mode;
- reset modes and clean modes;
- remote/config mutation scope;
- continue, abort, and skip of in-progress operations;
- managed Git argv fallback.

Avoid one catch-all mutation variant for supported operations.

### 4. Define risk taxonomy

Add `GitRiskClass` and map each parsed operation to existing execution capabilities. Required classes:

- ReadOnly;
- IndexMutation;
- WorktreeMutation;
- RefMutation;
- HistoryIntegration;
- NetworkRead;
- NetworkWrite;
- RepositoryConfigMutation;
- DestructiveWorktree;
- DestructiveHistory;
- OutsideProject.

Allow operations to carry multiple capabilities where necessary.

### 5. Define path and ref safety types

Create explicit wrappers for:

- canonical repository root;
- repository-relative literal path;
- raw advanced pathspec;
- branch/ref name;
- revision expression where raw revision syntax is allowed;
- remote name;
- object id where validated.

Literal file operations must be renderable after `--`. Reject NUL bytes, absolute paths, parent traversal after normalization, and paths resolving outside the canonical repository.

Do not over-validate Git revision expressions with fragile custom grammar; distinguish untrusted raw expressions from validated object ids.

### 6. Implement simple argv parser

Implement `parse_git_argv(argv, context) -> ParsedGitCommand`.

Requirements:

- input is already tokenized argv;
- reject empty argv and non-`git` executable forms unless explicitly normalized by caller;
- support recognized Git global options before the subcommand;
- handle `--` pathspec boundaries;
- distinguish overloaded read and mutation forms for branch, tag, remote, stash, reset, restore, checkout, switch, and config;
- detect force variants separately (`--force`, `-f`, `--force-with-lease`);
- detect destructive flag combinations independent of flag order;
- represent unsupported but simple forms as `ManagedGitArgv` with conservative risk;
- never use whitespace splitting;
- never execute commands in this parser.

Treat `git -C` conservatively. Parse it only if the target can later be canonicalized and policy-checked; otherwise mark the operation as requiring repository-resolution validation.

### 7. Structured parser errors

Errors should distinguish:

- malformed argv;
- unsupported global option;
- unsupported subcommand;
- ambiguous syntax;
- unsafe path/pathspec;
- missing required argument;
- contradictory flags;
- operation requires managed fallback;
- operation must remain raw shell because shell semantics are required.

Errors must be suitable for routing telemetry without exposing secrets.

### 8. Rendering contract

For each typed operation define deterministic argv rendering for later execution. Parsing followed by rendering should preserve semantics for supported inputs, though not necessarily original flag ordering.

Rendering requirements:

- no shell strings;
- literal paths after `--`;
- deterministic option order;
- explicit noninteractive flags only at execution layer, not parser layer;
- raw/managed argv preserved exactly except approved executable normalization.

## Likely files

- new `crates/codegg-git/Cargo.toml` and `src/lib.rs`, or isolated equivalent;
- operation/request modules;
- parser module;
- risk module;
- path/ref modules;
- workspace `Cargo.toml`;
- architecture note for the chosen boundary;
- parser unit tests and fixtures.

## Test matrix

Add table-driven coverage for:

- all supported read commands;
- staged/unstaged/name-only/stat diff permutations;
- branch list versus create/delete/force-delete;
- tag list versus create/delete;
- remote list/get-url versus add/remove/set-url;
- stash list/show versus push/apply/pop/drop;
- checkout branch versus `checkout -- path`;
- restore staged/worktree/source combinations;
- reset soft/mixed/keep/merge/hard;
- clean dry-run versus `-f`, `-d`, `-x` combinations;
- push normal, set-upstream, tags, force-with-lease, force;
- Git global options and `-C`;
- filenames beginning with `-` after `--`;
- spaces, Unicode, quotes already resolved by shell parser, and invalid NUL input;
- unsupported plumbing commands falling to managed argv;
- malformed commands returning stable errors.

Add property-style tests where practical:

- parser never panics for arbitrary argv vectors;
- rendering a supported parsed operation reparses to an equivalent operation;
- destructive flags never classify below destructive risk;
- unsupported forms never classify more permissively than their command family.

## Compatibility requirements

- No removal of existing `GitTool`, Bash routing, commit, review, or `egggit` APIs in this phase.
- Existing runtime behavior should remain unchanged except optional shadow parsing/telemetry behind a disabled-by-default or observe-only integration.
- Do not introduce provider/model dependencies.

## Validation

Run targeted parser tests, workspace formatting, clippy for touched crates, and existing command-intent adversarial tests. Keep test concurrency compatible with repository policy.

Suggested commands:

```bash
cargo fmt --all --check
cargo test -p codegg-git
cargo test -p codegg --lib command_intent
cargo test -p codegg --test command_routing_adversarial
cargo clippy -p codegg-git --all-targets -- -D warnings
```

Adjust package names to the final placement.

## Exit criteria

Phase A is complete when:

- a stable typed operation vocabulary exists;
- every currently recognized Git command has an explicit parse result or conservative managed fallback;
- risk classification is derived from parsed semantics rather than prefixes;
- path/ref/pathspec boundaries are explicit;
- parser and renderer are side-effect free and extensively tested;
- no existing Git execution path has been prematurely removed;
- architecture documentation records the crate/module boundary and invariants.

## Handoff to Phase B

Phase B should consume these types directly. Do not duplicate parser logic in `command_intent`, `command_routing`, `BashTool`, or the native Git tool.