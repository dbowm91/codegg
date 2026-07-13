# Git Agent Integration Phase B — Unified Planning, Routing, and Provenance

## Objective

Preserve Git semantics end to end through command classification, planning, permission generation, routing, execution dispatch, fallback, and RunStore provenance. Native Git tool calls and Bash-origin simple Git commands must converge on one Git request path without changing unsupported complex-shell behavior.

## Dependencies

Phase A must be complete and provide stable `GitOperation`, parser, risk, path/ref, and managed fallback types.

## Required deliverables

### 1. Add a Git-specific execution backend

Replace the split between `NativeTool { tool_name: "egggit" }` for reads and `GitMutating { tool_name, argv }` for selected mutations with a backend that carries a typed request:

```rust
ExecutionBackend::Git {
    request: GitExecutionRequest,
}
```

Retain legacy variants only during migration if required for compatibility. Mark them deprecated internally and add tests proving new Git planning no longer selects them.

### 2. Add a Git-specific routing decision

Add:

```rust
RoutingDecision::RouteToGit {
    request: GitExecutionRequest,
    timeout_secs: Option<u64>,
}
```

Do not convert typed Git mutations into `RouteToManagedProcess`. Managed unsupported Git argv must still route through `RouteToGit` so Git-specific policy and provenance remain intact.

### 3. Integrate command intent classification

Update command-intent classification to delegate Git argv semantics to the Phase A parser.

Requirements:

- shell-shape parsing remains authoritative for deciding whether native promotion is possible;
- `SimpleArgv` plus successful Git parse produces a typed Git intent;
- no whitespace-splitting fallback for active Git routing;
- complex shell commands remain shell intents;
- parser failure uses conservative fallback and records a reason;
- prefix heuristics remain only as conservative fallback for raw-shell risk labeling, not as primary classification.

Consider whether `CommandIntentKind::GitReadOnly` and `GitMutating` should be replaced by a single Git kind carrying risk, or retained as compatibility labels while detailed semantics live in the request.

### 4. Integrate Bash translation

Modify `BashTool` dispatch so simple Git commands invoke `RouteToGit`.

Critical invariants:

- execute exactly once;
- never attempt typed execution and then rerun through shell after a non-routing execution error;
- fallback is permitted only before execution begins;
- preserve raw command text for audit and display, but execute argv without a shell;
- retain existing preflight and permission checks;
- do not promote commands with pipes, redirects, substitutions, assignments, conditionals, sequencing, or backgrounding.

### 5. Add initial Git execution service shell

Create the service interface used by routing even if Phase B initially delegates reads to `egggit` and mutations/managed argv to a controlled process adapter.

Required interface characteristics:

- canonical repository root in request;
- typed operation and origin;
- timeout;
- explicit execution state marker so fallback cannot double-run;
- structured outcome envelope with raw output;
- planned tier and actual tier;
- stable errors for pre-execution rejection, spawn failure, timeout, exit failure, and structured-result failure.

### 6. Repository resolution

Centralize cwd/repository resolution:

- canonicalize requested cwd;
- discover containing Git root;
- compare against active Codegg project root;
- reject or request outside-project capability where applicable;
- resolve supported `git -C` targets before policy;
- identify nested repositories/submodules explicitly;
- include selected root in the request and result.

Do not let separate callers resolve different roots for the same command.

### 7. Permission generation

Generate permissions from `GitRiskClass` and capabilities produced by the parser.

At minimum:

- read-only: allow;
- named path stage/unstage: configured safe default;
- stage all and local mutations: ask;
- network access: ask unless configured otherwise;
- destructive worktree/history: deny by default;
- outside project: deny by default.

Permission prompts should include operation, repository, branch/ref where known, path scope, remote where known, and destructive/force mode.

### 8. RunStore provenance

Add or extend backend and metadata types so records distinguish:

- planned Git backend;
- actual Git backend;
- typed versus managed-argv execution tier;
- origin: native/Bash/workflow/TUI;
- operation kind;
- risk class/capabilities;
- repository root identifier;
- fallback reason;
- permission decision;
- timeout and exit status.

Maintain schema compatibility or implement migration/default handling for persisted records.

### 9. Routing metadata and metrics

Replace string-prefix Git mutation detection in `BashTool` metadata with parsed operation metadata whenever available. Raw-shell fallback may use conservative heuristics, but must never understate destructive risk.

Add metrics for:

- typed promotion success;
- managed Git argv fallback;
- raw-shell fallback;
- parser rejection reason;
- permission denial;
- execution timeout;
- planned/actual tier mismatch.

### 10. Configuration

Extend command-intent configuration only as needed. Prefer one `Git` family with per-risk controls over scattered read/mutation switches, but maintain backward-compatible interpretation of existing `GitRead` settings.

Document default modes and migration behavior.

## Likely files

- `src/command_intent/mod.rs` and planner modules;
- `src/command_routing.rs`;
- `src/tool/bash.rs`;
- new Git execution service adapter;
- `src/command_outcome/*`;
- RunStore types in `codegg-core`;
- configuration schema/defaults/documentation;
- routing and adversarial integration tests.

## Test matrix

Add tests proving:

- `git status`, diff, log, show route to Git backend;
- add/commit/switch/stash route to Git backend;
- unsupported simple plumbing routes to managed Git argv, not generic process;
- push/reset hard/clean remain Git backend with high-risk policy rather than losing identity;
- pipelines and compound commands remain shell;
- quoted argv from shell parser is preserved;
- no active Git route uses whitespace splitting;
- repository root resolution is consistent;
- outside-project `-C` is rejected;
- permission denial prevents spawn;
- an execution error does not trigger shell rerun;
- RunStore planned and actual backends match;
- fallback reasons are persisted;
- existing non-Git routing remains unchanged.

## Validation

Run targeted command-intent, Bash, routing, RunStore, configuration, and adversarial suites, followed by formatting and clippy for touched packages.

## Exit criteria

Phase B is complete when:

- all eligible native and Bash Git commands produce the same `GitExecutionRequest`;
- Git identity survives through actual dispatch;
- managed unsupported Git commands remain Git-specific;
- complex shell commands remain shell-owned;
- permissions are operation-aware;
- RunStore accurately captures origin, risk, tier, fallback, and executor;
- no-double-execution tests pass;
- legacy Git routing variants are removed or isolated behind migration compatibility.

## Handoff to Phase C

Phase C should replace provisional read execution and raw-output interpretation with structured `egggit` operations and typed projectors while retaining the routing contracts established here.