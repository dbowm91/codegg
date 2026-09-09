# Interactive Process Sessions Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#18-remote-projects-and-execution-targets`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#3-deployment-terms`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related ADRs:

- None required for the local execution-node implementation. The canonical specification already assigns PTYs/local process trees to execution nodes and scheduler admission to the global scheduler. A future cross-node interactive-terminal protocol belongs to remote/node roadmaps and may require a separate decision.

## 1. Purpose and ownership boundary

This roadmap fills the gap between non-interactive managed processes and a real attachable interactive terminal. It owns a daemon/execution-node interactive-process service, PTY lifecycle, scheduler admission, bounded input/output/resize, attach/detach/resume and TUI integration.

It explicitly does not treat a CodeGG `Session` as a terminal. It does not revive `shell_session` as the execution owner. `ManagedProcessService` remains the non-interactive execution path; Bash remains the ordinary model-facing shell tool.

## 2. Work classification

### Invariants

- Interactive processes execute on the node that owns the workspace.
- Scheduler admission/resource policy precedes interactive process creation.
- PTY child process groups are cancellable and cleaned up on terminal completion/daemon shutdown.
- Attach/detach never transfers authorization implicitly.
- Output/scrollback/input queues are bounded.
- Human `Session`, scheduler `Job`, `Run`, and interactive-process handle remain distinct identities.

### Capabilities

Humans can create, attach to, resize, detach from and terminate a real interactive workspace terminal from the TUI.

### Infrastructure

PTY process service with lifecycle state, bounded scrollback and subscriber ownership.

### Polish

Truthful disposition of the existing deferred `terminal` model tool and obsolete shell-session naming.

## 3. Non-goals

Remote terminal federation, terminal screen sharing as session observation, tmux replacement, shell history synchronization, persistent PTY survival across host reboot, model-visible interactive shell by default, new sandbox/namespace framework.

## 4. Current state

`ManagedProcessService` owns supervised non-interactive execution. The deferred `terminal` tool is not actually interactive: it executes one `sh -c` request through `ManagedProcessService`, captures bounded stdout/stderr and returns after termination. `src/shell_session/` contains metadata CRUD only and no PTY/process. No production PTY/openpty/forkpty/attach implementation was found at the audit baseline.

## 5. Target architecture

An execution-node `InteractiveProcessService` receives an immutable workspace/execution context and scheduler admission, creates a PTY-backed process group, owns input/resize/termination, captures bounded sequence-numbered output/scrollback and exposes daemon-issued handles. Attachments are ephemeral client-owned subscriptions authorized separately from process ownership. Detaching does not necessarily kill the process; explicit owner policy and idle timeout define lifecycle.

The TUI consumes a versioned protocol/controller. Model tools continue to use Bash/managed non-interactive execution unless a later explicit use case justifies a separately constrained interactive tool.

## 6. Dependency graph

```text
M001 scheduler-owned PTY engine
          |
          v
M002 bounded attach/resume protocol
          |
          v
M003 TUI terminal integration + legacy disposition
```

M001 is dependency-ready on existing scheduler/workspace/managed-process foundations. M002 hard-depends on M001; M003 hard-depends on M002.

## 7. Milestones

### M001 — Scheduler-owned PTY engine
Class: infrastructure. Objective: create/cancel/resize/write/read a local PTY process under workspace and scheduler ownership with bounded scrollback and cleanup. Exit: real interactive behavior demonstrated; process group termination and resource release pass; no `shell_session` authority.

### M002 — Bounded attach/resume protocol and authority
Class: capability. Objective: versioned daemon operations for create/list/attach/detach/input/resize/terminate/resume with client ownership, sequence/resync and authorization seams. Exit: multiple attachments policy explicit; lag/resync bounded; disconnect/reconnect deterministic; no request payload grants ownership.

### M003 — TUI terminal integration and legacy disposition
Class: capability. Objective: reference TUI interactive terminal UX over M002 and disposition the misleading deferred `terminal` tool/legacy metadata surface. Exit: human interactive terminal works end-to-end; model tool surface remains unambiguous; old one-shot behavior is removed/renamed/delegated only with consumer evidence.

## 8. Cross-cutting requirements

Use minimal portable PTY dependencies/platform code consistent with supported OS targets; dependency choice must not create a second process supervisor. Scheduler/resource/sandbox/environment policy is reused. Output and subscribers are bounded. Protocol unknown versions degrade safely. Secrets/environment content are not persisted into terminal metadata.

## 9. Verification strategy

Focused PTY/process-group/resize/input/lag/disconnect tests on supported hosts plus existing managed-process/scheduler/TUI suites and normal repository verification. Platform tests may be conditionally closed only with exact named operational evidence, not by assumption.

## 10. Risks and decision points

PTY portability differs across Unix/Windows; do not claim unsupported platforms. Interactive workloads can outlive clients and leak processes; ownership/idle/shutdown policies must be explicit. Do not duplicate Bash command safety parsing into the PTY engine; interactive user terminals have separate authorization/sandbox context.

## 11. Completion definition

M001-M003 accepted: CodeGG has a real bounded scheduler-owned interactive workspace terminal in the TUI, lifecycle cleanup is deterministic, and obsolete/misleading terminal metadata/tool surfaces are truthfully dispositioned.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/interactive-process-sessions/001-scheduler-owned-pty-engine.md` | `plans/closure/interactive-process-sessions/001-status.md` | — |
| M002 | closed | `plans/implementation/interactive-process-sessions/002-bounded-attach-resume-protocol.md` | `plans/closure/interactive-process-sessions/002-status.md` | — |
| M003 | ready | `plans/implementation/interactive-process-sessions/003-tui-terminal-integration-and-legacy-disposition.md` | — | — |
