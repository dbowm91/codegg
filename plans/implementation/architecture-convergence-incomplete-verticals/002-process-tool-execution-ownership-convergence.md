# Architecture Convergence M002 — Process and Tool Execution Ownership Convergence

Status: implemented

Repository baseline: `3c4890035513cd4d74430b6f64523c8be676024e`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.5-locality-by-default`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Relevant dependencies:

- Tool Programs subsystem is closed through M020;
- runtime-safety checked edit history is closed through M013;
- post-audit daemon/process lifecycle corrective work is closed;
- no hard dependency on other milestones in this roadmap.

Primary class: invariant / infrastructure

## 1. Objective

Establish one canonical production process-execution service beneath all CodeGG tools that spawn local processes. Bash, shell sessions, build/test Tool Programs, Git subprocess helpers, LSP process launchers, plugin installers, and other process-running callers may expose different semantics, but they must not independently implement overlapping process-group, cancellation, sandbox, environment, timeout, output-bound, or reap logic.

The target separation is:

```text
tool schema / authorization / dispatch
                 |
                 v
        canonical execution service
        | process lifecycle
        | sandbox/resource policy
        | cancellation/process groups
        | bounded stdout/stderr capture
        | cwd/env construction
        ` typed execution result
                 |
                 v
       tool-specific result mapping
```

## 2. Explicit non-goals

M002 must not:

- replace the tool broker/catalog with a new registry;
- create a second job scheduler or background-task engine;
- merge Tool Programs and ordinary foreground tools into one user-facing command model;
- weaken shell sandboxing, destructive-operation policy, or authorization;
- make shell sessions stateless if they intentionally require durable/interactive process state;
- force LSP stdio protocol handling through a shell-output abstraction;
- redesign remote execution/node protocols;
- add a new CI or chaos framework.

## 3. Current implementation evidence to inspect

The implementation agent must inspect at least:

- `src/exec.rs`;
- `src/tool/bash.rs`;
- `src/tool/backend.rs` and `src/tool/broker.rs`;
- shell-session modules and TUI shell controls;
- Tool Program execution paths;
- process launching in Git/worktree helpers;
- LSP server launch/process management in `egglsp`;
- plugin installation subprocess/archive behavior where relevant;
- sandbox helper and Landlock/process-group code;
- timeout/cancellation wrappers and output truncation helpers;
- environment allowlists and cwd/workspace resolution.

Classify each process spawn site as canonical service, protocol-specialized adapter, or justified exception.

## 4. Required canonical execution contract

The canonical service must own or compose the existing owners for:

- explicit `ExecutionContext`/workspace cwd selection;
- environment construction and allowlisting;
- process-group creation and tree termination;
- cancellation propagation;
- timeout/deadline handling;
- stdin mode;
- bounded stdout/stderr streaming/capture;
- exit/signal classification;
- sandbox/resource-policy application;
- deterministic cleanup/reaping;
- secret-safe diagnostics.

The service may expose specialized modes for:

```text
one-shot captured process
streaming foreground process
persistent interactive shell/process
protocol child process (e.g. LSP stdio)
```

These modes must share lifecycle/safety primitives rather than copy them.

## 5. Ordered work packages

### WP1 — Spawn-site inventory

Enumerate production `Command`/spawn/process sites and identify their lifecycle/safety owner. Include both direct `tokio::process` use and wrappers.

Produce an implementation note/table with disposition for every site. Missing sites are a closure blocker.

### WP2 — Select/extend canonical execution owner

Prefer the existing execution/runtime-safety service rather than adding a new crate unless current dependency cycles make that impossible. If `src/exec.rs` is only a partial abstraction, evolve it into the canonical root adapter over lower-level safety primitives.

Define typed request/result structures sufficient to prevent callers from rebuilding timeout/cancel/output semantics ad hoc.

### WP3 — Migrate Bash and shell-session shared lifecycle

Refactor `src/tool/bash.rs` so schema parsing, shell-specific semantics, and persistent-session behavior remain local while generic process lifecycle moves under the canonical service. Preserve interactive session identity, kill semantics, and output streaming.

### WP4 — Migrate Tool Programs and other one-shot callers

Ensure foreground/background tool execution, test/build jobs, plugin helper processes, and other ordinary subprocesses use canonical lifecycle/safety primitives.

Do not bypass scheduler ownership for background durable work; the execution service runs an accepted attempt, it does not admit one.

### WP5 — Protocol-process exceptions

For LSP and any other protocol child process, share process creation, environment, process-group, cancellation, and cleanup primitives while retaining protocol framing in the specialized owner. Document any remaining direct spawn with a concrete reason.

### WP6 — Root/tool-layer cleanup

Delete duplicate timeout, process-group, kill-tree, env, and output-bound helpers after migration. Keep the broker focused on authorization/dispatch and the backend focused on tool implementations rather than subprocess mechanics.

### WP7 — Documentation

Update execution/sandbox/tool architecture documentation with one process lifecycle diagram and an exception list.

## 6. Storage, protocol, migration, compatibility

No durable schema or external protocol change is expected. Existing shell-session IDs, Tool Program run IDs, process projection events, and command outputs must remain compatible.

Internal error variants may become more typed, but frontend-visible diagnostics should preserve meaningful categories.

## 7. Security and failure semantics

This milestone must not reduce:

- Landlock/sandbox application on supported Linux paths;
- environment filtering;
- destructive-command authorization;
- process-tree termination;
- output bounds;
- cancellation responsiveness;
- secret redaction.

A migration that routes a previously sandboxed path through an unsandboxed canonical helper is a blocker, not acceptable simplification.

## 8. Verification

Focused tests must cover representative one-shot, streaming, persistent, timeout, cancellation, process-tree kill, output-bound, and sandbox-policy paths. Run narrow package/tests during implementation, then:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use existing runtime-safety fixtures where available. Do not add a new chaos or CI lane.

## 9. Static guards

If useful, update an existing architecture guard to prohibit new raw process spawn sites outside the canonical service plus a short explicit allowlist for protocol-specialized owners. Prefer one narrow guard over broad grep ratchets.

## 10. Acceptance criteria

M002 is complete only when:

- every production process spawn site has an explicit ownership disposition;
- common lifecycle/sandbox/cancellation/output semantics have one canonical implementation;
- Bash and Tool Program process execution consume those primitives;
- protocol processes reuse shared lifecycle primitives where technically appropriate;
- duplicate generic process helpers are removed;
- no scheduler/tool-registry/runtime duplication was introduced;
- focused safety/cancellation tests and quick verification pass.

## 11. Stop conditions

Stop if convergence would require redesigning scheduler admission, remote execution/node ownership, LSP protocol semantics, or sandbox authority. Register a separate plan/ADR rather than smuggling those changes into this milestone.

## 12. Closure evidence required

Record:

- implementation commits;
- complete spawn-site disposition table;
- before/after ownership diagram;
- deleted helper list and justified exceptions;
- sandbox/cancel/process-tree regression evidence;
- verification outcomes;
- unresolved findings by severity.
