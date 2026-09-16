# Execution Reliability, Approval, and Autonomy M005 — Production Sandbox Policy Wiring

Status: active

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4.7-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: security invariant / capability

Hard dependency: M003 closure.

Foundation consumed, not reopened:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- existing `src/security/sandbox.rs` child helper/Landlock contract.

## 1. Objective

Make the resolved `SandboxProfile` an actual production execution property: thread it through agent/tool construction, enable the existing supported-Linux filesystem sandbox for constrained Bash/process execution, distinguish WorkspaceWrite from true FullHost, and report filesystem/network enforcement truthfully without letting ApprovalMode alter containment.

## 2. Why this milestone is blocked

M003 must first provide one immutable execution-policy snapshot and ApprovalRouter so sandbox policy can be threaded through the same canonical runtime construction rather than another independent config branch.

## 3. Current implementation evidence

- `src/security/sandbox.rs` defines `SandboxConfig`/`SandboxMode`, trusted helper resolution, bounded status frames, Landlock application, and fail-closed required sandbox launch behavior.
- `BashTool` exposes `with_landlock_sandbox`, `with_landlock_sandbox_custom`, and `with_sandbox_mode`.
- `src/tool/bash/process.rs` uses a required `SandboxRequest` when an enabled sandbox config is present.
- `ToolRegistryOptions` does not currently carry a sandbox policy.
- normal `ToolRegistry::with_options()` constructs `BashTool::default()`, applies workspace allowed-path/cwd constraints and runtime services, but never enables Landlock.
- workspace `allowed_paths` validates working directory; it is not OS enforcement against commands referencing arbitrary absolute paths.
- Landlock is filesystem containment. Python sandbox code explicitly notes it does not enforce network.
- `SandboxMode::DangerFullAccess` currently only makes configured roots writable, so its name does not faithfully mean “unsandboxed host authority.”

## 4. Invariants that must not regress

- explicit constrained sandbox request fails closed if required enforcement cannot be established;
- ApprovalMode cannot modify SandboxProfile, including Automatic/Yolo;
- filesystem sandbox result does not imply network sandbox;
- parent/subagent sandbox ceiling cannot be broadened by the child;
- all process launches continue through canonical managed-process/scheduler execution paths;
- workspace path canonicalization/symlink protections remain intact;
- FullHost is explicit and auditable, never an accidental fallback from WorkspaceWrite;
- unsupported hosts report actual enforcement rather than claiming sandbox success.

## 5. Scope

### In scope

- `SandboxProfile::{ReadOnly, WorkspaceWrite, FullHost}` resolved runtime representation;
- threading profile through ExecutionPolicySnapshot/AgentLoopBuildInput/ToolRegistryOptions/child construction;
- Bash/compatible process-tool Landlock configuration from authoritative workspace root;
- truthful enforcement result for filesystem and network dimensions;
- cleanup/rename of ambiguous internal DangerFullAccess semantics if needed;
- policy interaction with Python/scheduler routes where they already consume sandbox contract;
- tests/docs.

### Explicitly out of scope

- new container/VM/seccomp/eBPF framework;
- strong network sandbox backend;
- automatic approval reviewer;
- changing PermissionChecker rule language;
- cross-platform parity claims beyond actual supported mechanisms;
- bypassing managed process service.

## 6. Required production changes

### Core/domain

Add or finalize `SandboxProfile` in the M003 execution-policy domain. Separately represent obtained enforcement, for example:

```text
SandboxEnforcement {
  filesystem: Enforced(backend/abi) | Unavailable(reason) | FullHost,
  network: Enforced(backend) | Unrestricted | Unavailable(reason),
}
```

Do not overload one boolean.

### Tool registry/runtime wiring

Add sandbox policy/profile to `ToolRegistryOptions` and all production builders. Normal daemon `TurnRuntime`/AgentLoopFactory must pass the resolved profile based on authoritative workspace root and policy snapshot.

For WorkspaceWrite on supported Linux:

- construct `SandboxConfig` before child spawn;
- allowed read/write roots are the exact workspace/effective approved roots plus minimum runtime libraries/executable requirements already handled by sandbox helper;
- use `SandboxMode::WorkspaceWrite` semantics;
- helper setup failure becomes typed failure, not unsandboxed execution.

ReadOnly should deny writes at OS policy where the path is routed through a sandbox-capable process.

FullHost intentionally skips CodeGG filesystem containment and records FullHost. Rename/deprecate internal `DangerFullAccess` if its current meaning cannot truthfully map to this profile; do not silently reinterpret old config without compatibility handling.

### Network truthfulness

Until a network backend exists, record `network=Unrestricted` for normal shell under WorkspaceWrite/ReadOnly. Permission/security policy may still classify network commands, but that is not OS network isolation. UI/help must state this distinction.

### Sandbox escalation

If a command genuinely needs a path outside WorkspaceWrite, deterministic policy may return an escalation request describing the specific capability/path. M005 need not implement a sophisticated temporary expansion store; it must avoid the crude behavior of silently switching the entire turn to FullHost.

### Subagents

Derive child sandbox ceiling as intersection/no-broader-than parent plus child agent policy/worktree. Worktree root becomes the child's writable sandbox root when it owns an isolated worktree.

### Protocol/frontends

Expose effective requested profile and obtained enforcement snapshot/reason. Frontends can render warnings/status but do not choose a stronger profile without daemon-authorized mode update.

### Documentation/static guards

- update `architecture/security.md`, `architecture/tool.md`, execution context docs;
- add regression/static test ensuring production ToolRegistry receives sandbox policy and that WorkspaceWrite construction does not use `BashTool::default()` without applying it;
- preserve existing sandbox-contract guard.

## 7. Ordered work packages

### Work package A — Policy/enforcement types

Define requested profile versus obtained filesystem/network enforcement and compatibility mapping from existing SandboxMode.

### Work package B — Production threading

Thread profile through turn/subagent/tool registry construction and configure BashTool/compatible routes using authoritative workspace root.

### Work package C — FullHost and fallback truthfulness

Make FullHost intentional; constrained mode failure cannot fall through. Expose unsupported-host behavior and network-unrestricted state.

### Work package D — Child/worktree matrix

Ensure read-only/shared and mutation/isolation child paths inherit bounded profiles correctly.

## 8. Failure, cancellation, restart, and contention semantics

- sandbox helper unavailable/setup error under explicitly constrained profile fails the command/turn action with typed diagnostic; no unsandboxed retry;
- if host does not support Landlock, the resolved policy must follow configured compatibility rule (fail constrained request or ask user to choose FullHost), never claim Enforced;
- cancellation still terminates managed process tree;
- restart recomputes sandbox enforcement availability from current host and persisted requested preference/policy; it does not persist stale “Enforced” as current fact;
- concurrent turns/worktrees use their own immutable roots/policy snapshots.

## 9. Compatibility and migration

- old config/no SandboxProfile resolves to the documented conservative/default behavior selected by M003/ADR-0004; avoid surprising automatic FullHost.
- existing runtime-safety SandboxConfig remains implementation primitive.
- if `DangerFullAccess` serialized config exists, add explicit compatibility mapping/deprecation with no semantic ambiguity.
- no database migration beyond M003 preference fields should be required.

## 10. Required tests

### Focused unit tests

- SandboxProfile -> SandboxConfig/enforcement mapping;
- FullHost distinct from WorkspaceWrite;
- filesystem/network reporting;
- parent-child ceiling matrix.

### Integration tests

On supported Linux fixture:

- WorkspaceWrite can read/write inside workspace;
- cannot read/write representative outside path;
- ReadOnly cannot modify workspace;
- FullHost is unsandboxed by CodeGG and explicitly reported;
- ApprovalMode changes do not alter SandboxProfile.

### Restart and recovery tests

- persisted WorkspaceWrite preference re-resolves enforcement after daemon restart;
- unavailable helper after restart fails constrained action rather than falling back.

### Contention and cancellation tests

- two worktree children get distinct writable roots;
- cancellation cleans sandboxed process descendants.

### Security and negative tests

- symlink/path escape remains denied;
- Yolo does not cause sandbox disable;
- network is reported unrestricted when not OS-contained;
- child cannot select FullHost under WorkspaceWrite parent.

### Migration and compatibility tests

- legacy sandbox config mapping/deprecation;
- unsupported platform behavior.

## 11. Required verification commands

```bash
cargo test --test tool_execution
cargo test --test command_routing_execution_ownership
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Additionally run the existing supported-Linux Landlock fixture when the host supports it and record unsupported-host reason honestly.

## 12. Documentation updates

- `architecture/security.md`
- `architecture/tool.md`
- workspace/execution context docs;
- CLI/TUI effective policy help text groundwork;
- runtime-safety roadmap cross-reference only if needed, without reopening closed milestones.

## 13. Acceptance criteria

- production constrained Bash actually receives the configured sandbox on supported Linux;
- workspace cwd validation is no longer misrepresented as full filesystem containment;
- WorkspaceWrite/ReadOnly failures cannot silently execute FullHost;
- ApprovalMode and SandboxProfile remain orthogonal;
- FullHost means explicit no CodeGG filesystem containment and is auditable;
- network containment status is truthful;
- child/worktree sandboxes cannot exceed parent ceiling.

## 14. Stop conditions

Stop if correct implementation requires inventing a new network/container sandbox backend, replacing managed-process ownership, or changing approval semantics before M006/M007.

## 15. Closure evidence required

- production ToolRegistry wiring trace;
- SandboxProfile/enforcement matrix;
- supported/unsupported host results;
- escape/fail-closed tests;
- parent-child worktree matrix;
- proof Automatic/Yolo mode field cannot mutate sandbox;
- verification commands and remaining network limitation.

## 16. Handoff notes

The highest-value fix is wiring the sandbox CodeGG already has. Do not expand scope into a new cross-platform sandbox framework unless a later dedicated ADR/workstream justifies it.
