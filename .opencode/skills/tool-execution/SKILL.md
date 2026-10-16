---
name: tool-execution
description: Preserve CodeGG tool authority and scheduler ownership when changing execution paths
version: 1.0.0
tags: [tools, scheduler, execution]
---

# Tool execution ownership

Production model tool calls enter through `ToolBroker`. Do not add a second
direct execution path for a tool that the broker already owns.

Heavy work such as tests, managed processes, subagent dispatch, and tool
programs is submitted through `JobSubmissionService` to `JobScheduler`.
Preserve the typed job request, permission decision, cancellation, and durable
result path when changing those capabilities. Human shell commands are a
separate TUI feature and do not become model tools.

The broker derives every retry's `ToolExecutionContext` from the same
`BrokerInvocationContext`. When an agent call includes
`ExecutionAuthorizationContext`, preserve its immutable snapshot, receipt, and
preflight summary through the canonical executor. Do not repeat command
preflight after approval or let an executor use a broader sandbox profile than
the accepted snapshot; unavailable required containment is a typed refusal.

`ManagedProcessService` remains the sole finite-process owner on every host.
Its Windows adapter starts unconstrained children suspended, assigns a
kill-on-close Job Object before resume, and owns timeout/cancellation cleanup
for the whole process tree. Job ownership is process supervision only; Windows
constrained requests remain unavailable until a separate backend proves
filesystem enforcement and installs any required token/status controls. The
test-only AppContainer path is integrated through this same service, verifies
the package SID and Job before resume, and uses package-local `LOCALAPPDATA`,
`TEMP`, and `TMP`; it does not claim filesystem enforcement.
The C010 test fixture can attach journaled read, read/write, and deny ACL leases
to fresh disposable roots and exercises restoration after exit, cancellation,
and recovery. It remains test-only; the production daemon-owned journal and
Windows constrained backend are not activated by this fixture.

Raw model-shell text is compiled to platform-specific execution inputs (`sh -c`
on Unix; an absolute Windows PowerShell, `cmd.exe`, Git Bash, or WSL executable
on Windows). `cmd.exe` uses a dedicated raw-command field because its command
line parser differs from standard Windows argv parsing; that field is accepted
only with the selected absolute `cmd.exe`. Structured argv continues to bypass
shells. The legacy one-shot `terminal` model tool must carry the same accepted
execution-authorization snapshot and workspace sandbox request as `bash`; its
managed-process adapter must refuse constrained execution when a workspace
root or enforcing backend is unavailable. Windows raw-shell calls get a unique temporary directory through
`TEMP`/`TMP`, cleaned after completion; that does not restrict filesystem
access. Shell selection and policy path checks must not be described as Windows
enforcement.

Raw model-shell text is compiled to platform-specific execution inputs (`sh -c`
on Unix; an absolute Windows PowerShell, `cmd.exe`, Git Bash, or WSL executable
on Windows). `cmd.exe` uses a dedicated raw-command field because its command
line parser differs from standard Windows argv parsing; that field is accepted
only with the selected absolute `cmd.exe`. Structured argv continues to bypass
shells. Windows raw-shell calls get a unique temporary directory through
`TEMP`/`TMP`, cleaned after completion; that does not restrict filesystem
access. Shell selection and policy path checks must not be described as Windows
enforcement.

Before editing, read:

- `architecture/agent.md` for tool dispatch and broker ownership.
- `architecture/jobs.md` and `architecture/scheduler.md` for durable admission
  and execution.
- `docs/execution-ownership.md` and `docs/execution-ownership.toml` for the
  process-spawning inventory and guard.
- `architecture/process-tool-execution-ownership.md` for the managed-process
  lifecycle, including the Windows Job Object boundary.

Verify process-spawning changes with `python3 scripts/check_execution_ownership.py`; scheduler changes also require
`python3 scripts/check_scheduler_bypass.py`. Keep the ownership manifest in
sync. Skills describe contracts; they do not grant execution permission.
