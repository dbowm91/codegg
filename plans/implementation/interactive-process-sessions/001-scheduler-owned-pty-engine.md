# Interactive Process Sessions Milestone 001 — Scheduler-Owned PTY Engine

Status: implemented

Closure record: `plans/closure/interactive-process-sessions/001-status.md`

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/interactive-process-sessions-roadmap.md#M001--scheduler-owned-PTY-engine`

Long-term requirements: `plans/000-long-term-specification.md#45-locality-by-default`, `#17-job-scheduling-and-execution-backends`, `#29-system-invariants`.

Applicable ADRs: none. Primary class: infrastructure.

## 1. Objective

Implement a local execution-node PTY-backed interactive-process service that is admitted by the existing scheduler, bound to an immutable workspace/execution context, owns its child process group, supports input/resize/termination, and retains only bounded sequence-numbered scrollback.

## 2. Why this milestone is ready

Global scheduler, workspace execution contexts, managed non-interactive process supervision and daemon lifecycle are established. No team authorization dependency is required to build the local owner engine; M002 adds transport authority.

## 3. Current implementation evidence

The existing deferred `terminal` tool executes `sh -c` via `ManagedProcessService` and is single-shot. `shell_session` stores metadata only. There is no real PTY service at baseline. Managed process and Bash already define environment/output/cancellation/process-tree expectations that the interactive service must reuse where applicable rather than duplicate.

## 4. Invariants that must not regress

Scheduler permit before spawn; explicit WorkspaceId/root/execution target; no process-global cwd authority; process group cleanup on terminate/shutdown; bounded output and input; PTY lifecycle distinct from human Session; no model-facing tool registration; no second general process supervisor.

## 5. Scope

In: PTY backend abstraction only as needed for supported local OSes, process state/handle, scheduler admission/resource request, environment/cwd policy, input/write, resize, output sequence/ring buffer, exit status, terminate/kill escalation, daemon shutdown cleanup. Out: public attach protocol (M002), TUI (M003), remote PTY, persistent host-reboot recovery, new sandbox.

## 6. Required production changes

Core/runtime: `InteractiveProcessService` or equivalent owned by daemon/execution-node services. Scheduler: dedicated job/admission kind or documented permit contract; no bypass. Process: PTY child spawned in canonical workspace with process-group identity and cancellation. Storage: none required beyond optional run metadata; scrollback is bounded memory unless existing artifact policy is explicitly reused. Security: sanitized environment/workspace path policy; no secret logging. Docs: process ownership map.

## 7. Ordered work packages

A — define state machine/handle/resource limits and platform capability contract.

B — implement PTY spawn/read/write/resize/exit and bounded sequence ring over the smallest appropriate platform layer/dependency.

C — integrate scheduler admission, workspace/environment policy and process-group cancellation/shutdown.

D — add deterministic fake/backend seams only if needed for unit tests plus real supported-host integration fixture.

E — document limits/platform support and failure semantics.

## 8. Failure, cancellation, restart, and contention semantics

Spawn failure releases permit and no handle becomes live. Reader/writer failure produces terminal state and cleans child. Graceful terminate escalates after bounded timeout. Daemon shutdown cancels then joins/cleans. Daemon crash cannot guarantee PTY persistence; restart reports prior ephemeral handles gone. Scheduler contention queues/refuses according to existing policy rather than spawning anyway.

## 9. Compatibility and migration

Additive internal service. Do not change `terminal` tool yet. No schema migration expected. If a durable run record is reused, keep compatibility fields optional/additive.

## 10. Required tests

Interactive prompt/input round-trip; resize signal/size observation where portable; output sequence/ring truncation; large output bounds; scheduler contention/no-spawn-before-permit; cwd/environment policy; process-group child cleanup; timeout/escalation; shutdown; spawn/read failure; supported platform fixture.

## 11. Required verification commands

```bash
cargo test --workspace interactive_process --no-fail-fast
cargo test --workspace managed_process --no-fail-fast
cargo test --workspace scheduler --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Core/process/scheduler architecture; platform support/limitations; clarify `Session` vs interactive process.

## 13. Acceptance criteria

A local scheduled PTY can run a genuinely interactive command, accept input and resize, stream bounded ordered output, terminate its process tree and release scheduler resources; no public/model tool is falsely called interactive.

## 14. Stop conditions

A PTY library/platform choice would replace canonical process/scheduler ownership; required supported platform cannot provide safe process-group cleanup; implementation needs remote/node protocol semantics.

## 15. Closure evidence required

Architecture/state map, scheduler admission proof, real interactive fixture, output/resource bounds, cancellation/shutdown/process-tree tests, platform matrix, dependency rationale, exact verification results.

## 16. Handoff notes

Prefer a thin PTY primitive underneath CodeGG lifecycle ownership. Do not import a terminal multiplexer/session framework that becomes a parallel runtime.
