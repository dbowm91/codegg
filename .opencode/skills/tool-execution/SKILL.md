---
name: tool-execution
description: Preserve CodeGG tool authority and scheduler ownership when changing execution paths
version: 1.0.0
tags: [tools, scheduler, execution]
---

# Tool execution ownership

Production model tool calls enter through `ToolBroker`. Do not add a second
direct execution path for a tool that the broker already owns.

A process-capable tool is refused **before** it runs, at two boundaries that
must both hold: the tool's own `execute_structured` and the broker's final
dispatch. The broker is the single production tool-call boundary, so a tool
that forgets to consult the gate is still refused — but a new process-capable
tool must consult it too, because the broker's classification is by
**effective runtime effect** (`src/tool/process_capability.rs`), not by
whatever the tool advertises. A plugin that describes itself as read-only and
is process-backed is still a process surface, and the plugin adapter
classifies it by its runtime kind. When you add a tool that starts a process,
add it to the capability table and to `docs/writer-disposition.toml`; the
`scripts/check_process_denial.py` guard fails when either is missing.

Heavy work such as tests, managed processes, subagent dispatch, and tool
programs is submitted through `JobSubmissionService` to `JobScheduler`.
Preserve the typed job request, permission decision, cancellation, and durable
result path when changing those capabilities. Human shell commands are a
separate TUI feature and do not become model tools.

A native writer on a checkout shared by more than one run is serialized by
the checkout's durable writer permit. The fence that takes it resolves the
run's scope from the run's **own durable record**, never from a
model-supplied path, and a run that holds no managed checkout has no fence at
all. When you change how a delegated run is placed on a checkout, keep that
attachment: a run whose record does not name its checkout holds no authority
to write it. See `architecture/writer_permit.md` §"The shared checkout profile
(C006)" and `scripts/check_shared_checkout.py`.

The broker derives every retry's `ToolExecutionContext` from the same
`BrokerInvocationContext`. When an agent call includes
`ExecutionAuthorizationContext`, preserve its immutable snapshot, receipt, and
preflight summary through the canonical executor. Do not repeat command
preflight after approval or let an executor use a broader sandbox profile than
the accepted snapshot; unavailable required containment is a typed refusal.

Repository retrieval must carry workspace scope from that accepted broker
context through the canonical repository tool adapter. Derive the canonical
root only from an allowed execution decision and its workspace path policy;
never accept a model argument or ambient process cwd as authority. For the
CodeGG-managed Eggsearch stdio child, send this scope as authenticated private
MCP request metadata, with an explicit null root when the accepted context
cannot establish a root. Keep it out of public tool schemas and ordinary MCP
tool arguments. Preserve local-required failure semantics so missing or
unmatched local scope cannot silently route to remote providers.

`ManagedProcessService` remains the sole finite-process owner on every host.
For model-owned work, a host-stamped session identity may carry the process
through a persistent private session scratch root. The service injects its
path after environment sanitization and adds it to Required sandbox grants;
never derive it from command input or human-shell context. Structured `read`
and `write` only accept absolute paths beneath that session root. Cleanup is
lease-aware: a terminal delegated run reclaims its own private root, a revoked
shared resource is reclaimed after the last attempt lease releases, and a
soft-deleted session's whole tree is reclaimed once its durable runs and jobs
are terminal. The daemon resumes that cleanup after a restart. A bounded
retention sweep reclaims a session's tree once the session has been idle past
the configured retention window, and an orphan tree past its grace period —
both only while the lease is clear, where the lease is an accepted turn
admission receipt, a non-terminal run, or a non-terminal job. The sweep never
runs on the agent hot path and never blocks a tool call. A tree the automatic
path refuses is disposed of only by the explicit, audited
`codegg daemon scratch-reclaim` action, which relaxes the entry-content
refusals only and never the ownership or containment checks. Scratch contents
remain disposable cache data that host cache maintenance may purge.
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
shells. Windows raw-shell calls get a unique temporary directory through
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
