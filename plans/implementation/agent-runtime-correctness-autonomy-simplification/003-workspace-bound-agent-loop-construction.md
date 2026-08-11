# Agent Runtime Correctness, Autonomy, and Simplification M003 — Workspace-Bound AgentLoop Construction

Status: ready

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M003

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: correctness/ownership invariant

Dependencies:

- hard: none
- interface: `ExecutionContext`, `TurnRunInput`, snapshot manager, tool execution context, permission/path helpers
- soft: M005 benefits from the reduced loop state but does not require it for semantic design

Relevant references:

- `plans/000-long-term-specification.md` — singleton daemon and explicit project/workspace ownership
- `plans/001-terminology-and-domain-model.md` — workspace and execution context
- `plans/003-planning-process.md` — explicit typed workspace/session context
- `architecture/core.md`
- `architecture/agent.md`
- `architecture/storage.md`

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/003-status.md`

## 1. Objective

Make explicit execution/workspace identity mandatory at production `AgentLoop` construction time and ensure every workspace-sensitive helper derives from that identity rather than process-global current working directory or setters applied after partial construction.

This milestone fixes the concrete snapshot-root defect and removes the transitional constructor/factory structure that permits invalid intermediate state.

## 2. Explicit non-goals

Do not:

- redesign workspace identity, project catalog, daemon attachment, or scheduler ownership;
- change public project/session protocol messages;
- split daemon and TUI processes;
- remove standalone/test execution surfaces that are intentionally outside daemon singleton guarantees;
- ban all uses of `std::env::current_dir()` repository-wide when they are legitimate CLI/bootstrap/UI convenience reads;
- add a global workspace singleton;
- change snapshot storage semantics beyond using the correct root;
- create another builder/factory layer solely to hide constructor arguments;
- preserve a legacy factory merely because it already exists if it has no independent consumer/invariant.

## 3. Current implementation evidence

Inspect at minimum:

- `src/agent/turn_runtime.rs` and `TurnRunInput.execution`;
- `src/agent/agent_loop_factory.rs`;
- `src/agent/runtime_factory.rs`;
- `src/agent/loop.rs::new`, `set_workspace_root`, snapshot creation, local path/mutation helpers, `build_tool_execution_context()`;
- `src/snapshot/` ownership and constructors;
- `src/tool/factory.rs` session tool construction with `ExecutionContext`;
- daemon/project CWD static guards and existing workspace-isolation tests;
- standalone/stdio/exec tests that build `AgentLoop` directly.

Known defects/duplication:

- `TurnRunInput` already declares `execution: Arc<ExecutionContext>` as the single source of truth for filesystem/process execution;
- `AgentLoop::new()` does not receive this required identity and initializes `workspace_root` as `None`;
- snapshot manager construction happens inside `AgentLoop::new()` using `std::env::current_dir()` when snapshots are enabled;
- `runtime_factory::build_agent_loop()` later calls `set_workspace_root(workspace_root)`, which only sets the field and does not rebuild the snapshot manager;
- `AgentLoopFactory` is documented as transitional and merely repacks `AgentLoopBuildInput` into the older many-argument factory;
- production correctness therefore depends on post-construction setters and has at least one subsystem (snapshots) initialized before the authoritative root is supplied.

## 4. Invariants that cannot regress

- every production turn has one explicit `ExecutionContext` or equivalent required typed workspace identity before `AgentLoop` initialization;
- snapshot capture/restore uses the same workspace root as tool execution for that turn;
- local mutation/path auto-approval logic, if retained after M001, evaluates paths relative to the explicit workspace root;
- tool execution context CWD derives from explicit turn identity;
- subprocess, Git, test, LSP, and other daemon-owned execution remain scoped by existing scheduler/tool factories and are not redirected to process CWD by this cleanup;
- active turns remain isolated when multiple workspaces are served by one daemon;
- changing process CWD after daemon startup cannot change the workspace authority of an already constructed turn;
- test/standalone fixtures must opt into an explicit fixture root rather than silently inherit production behavior from ambient CWD;
- no new global mutable workspace state is introduced.

## 5. Target construction model

Prefer one complete typed construction input. Reuse `AgentLoopBuildInput` or replace it with a similarly explicit structure rather than adding more positional arguments.

A representative shape:

```text
AgentLoopBuildInput {
    execution: Arc<ExecutionContext>,
    session_id,
    agents,
    provider,
    config,
    tool_registry,
    permission_checker or inputs needed to construct it,
    persistence/services,
    subagent/scheduler services,
    runtime assets,
}
```

Required turn identity must not be patched by setters after creation.

If `AgentLoopFactory` has only the default implementation and no test injection value beyond repacking fields, delete it and let `DefaultTurnRuntime` call one constructor/build function directly. If tests genuinely mock the factory, retain the trait only if it provides real seam value; still make the typed build input the sole source of required state.

## 6. Snapshot requirements

- construct `SnapshotManager` from `execution.workspace_root` or an explicit root passed from it;
- snapshot manager must not call process-global CWD to infer the project root;
- snapshot capture and restore tests must prove workspace A cannot capture/restore workspace B when daemon/process CWD points elsewhere;
- if snapshot storage uses session IDs, preserve that behavior; root correctness is the scope here;
- no schema migration should be required.

## 7. Path and mutation helper requirements

Audit helpers such as `is_path_within_working_directory` and any workspace mutation detection used by permission logic.

Requirements:

- helper accepts explicit workspace root/context;
- resolve relative paths against the explicit root;
- canonicalize safely when the path exists;
- for not-yet-created mutation targets, evaluate the canonical/validated parent plus lexical final component under existing path-security rules rather than falling back to ambient CWD;
- path absence must not imply workspace safety;
- preserve symlink/traversal protections already present in permission/tool layers;
- M003 must not reintroduce blanket approval removed by M001.

## 8. Ordered work packages

### Work package A — Inventory construction paths

1. find all production and test/standalone `AgentLoop::new`/factory callers;
2. classify each as daemon production, standalone compatibility, or test fixture;
3. identify which required fields are currently set after construction;
4. identify components initialized before those setters (snapshot manager is known);
5. map all remaining process-CWD reads in agent-loop/workspace-sensitive code.

### Work package B — Consolidate construction

1. make workspace/execution identity part of the initial build input;
2. construct permission checker/services with explicit identity where needed;
3. initialize snapshot manager only after the authoritative root is available;
4. remove `set_workspace_root()` from production construction;
5. remove or narrow setters for other truly required identity fields when safe (`session_id` may also belong in build input);
6. collapse `AgentLoopFactory` + `runtime_factory::build_agent_loop` indirection if no distinct seam remains.

Avoid a broad dependency-injection rewrite. One typed constructor is sufficient.

### Work package C — Remove ambient-CWD authority

1. change workspace containment/mutation helpers to accept explicit root/context;
2. remove production `std::env::current_dir()` fallback from snapshot/permission/tool context;
3. retain explicit `PathBuf::from(".")` only in isolated test/legacy helper constructors where the caller deliberately chooses it and comments make non-authoritative semantics clear;
4. ensure static CWD guards are updated only if their allowlist/reference paths change.

### Work package D — Multi-workspace regressions

Add focused tests proving:

- daemon/process CWD can point to workspace A while a turn for workspace B snapshots B;
- two simultaneously constructed loops with roots A/B keep independent snapshot/path behavior;
- relative mutation path resolves under the owning workspace;
- changing process CWD after construction does not change the loop's execution root;
- direct test/standalone construction must supply a root explicitly.

Use temporary directories; do not depend on the developer machine's actual CWD layout.

### Work package E — Documentation

Update only ownership docs affected by the new constructor:

- `architecture/agent.md`;
- `architecture/core.md` if construction ownership is described there;
- snapshot/storage docs if they currently imply ambient project root discovery;
- code comments marking transitional factory layers after deletion/consolidation.

## 9. Storage, protocol, migration, and compatibility effects

Storage:

- no schema migration;
- snapshots may now be captured from the correct workspace where previous behavior used daemon launch CWD. Existing snapshot records remain readable under their current identifiers.

Protocol:

- none expected.

Compatibility:

- normal daemon users should see only correctness improvement;
- standalone/test code that directly constructs `AgentLoop` may need an explicit workspace path. Treat this as an internal API correction unless a public library API is documented;
- do not silently restore ambient CWD as a compatibility fallback in production.

## 10. Concurrency, cancellation, and restart semantics

- workspace identity is immutable for the lifetime of a turn;
- concurrent turns for different workspaces must not share mutable root state;
- snapshot capture retains existing async/cancellation behavior;
- daemon restart reconstructs turns/services from durable project/session identity as before;
- no `set_current_dir` synchronization/locking should be introduced. Process CWD is not a concurrency primitive.

## 11. Focused verification

Run focused tests for:

```text
snapshot root follows ExecutionContext
concurrent/multiple workspace isolation
relative path resolution under explicit root
ambient CWD change does not alter turn root
standalone/test explicit-root construction
```

Also run the existing relevant CWD/workspace guards and tests, for example the repository's current daemon-CWD/project-agent-PWD guards if still applicable.

Then run:

```bash
scripts/verify.sh quick
```

Do not require full workspace tests unless constructor signature changes affect a broad number of crates and quick verification is insufficient to compile all consumers.

## 12. Static guards

Do not add another CWD guard if existing `check_daemon_cwd_usage.py` / project-agent inference guards cover the relevant source boundary.

Prefer deleting allowlist exceptions made obsolete by this milestone. If the existing guard cannot distinguish bootstrap CWD use from production authority, narrow the existing guard rather than creating a second script.

## 13. Acceptance criteria

M003 closes only when:

- production `AgentLoop` construction receives explicit execution/workspace identity before initializing workspace-sensitive components;
- snapshot manager uses that explicit root;
- `set_workspace_root()` is absent from the production construction path;
- workspace containment/mutation helpers no longer call process-global CWD for production decisions;
- transitional `AgentLoopFactory`/legacy factory layers are deleted or justified by a real remaining seam;
- no invalid partially initialized production loop state remains for required workspace identity;
- multi-workspace regression tests pass;
- relevant existing CWD guards and `scripts/verify.sh quick` pass;
- no new global workspace singleton, protocol change, or storage migration is introduced.

## 14. Stop conditions

Stop and create a narrower follow-up/ADR if:

- a documented public library API promises ambient-CWD `AgentLoop` construction;
- snapshot records encode an implicit root requiring a persistent migration rather than a constructor correction;
- removing the transitional factory would require redesigning an external plugin/ACP API rather than internal call sites.

Do not preserve the known production bug merely to avoid updating internal tests/callers.

## 15. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/003-status.md` must include:

- implementation commit/PR;
- before/after constructor ownership diagram or concise path description;
- list of removed production ambient-CWD reads/setter dependencies;
- snapshot multi-workspace test evidence;
- disposition of `AgentLoopFactory`, `runtime_factory`, and required setters;
- focused guard/test and quick-verification results;
- compatibility notes for standalone/internal callers;
- unresolved findings classified by severity.