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

Before editing, read:

- `architecture/agent.md` for tool dispatch and broker ownership.
- `architecture/jobs.md` and `architecture/scheduler.md` for durable admission
  and execution.
- `docs/execution-ownership.md` and `docs/execution-ownership.toml` for the
  process-spawning inventory and guard.

Verify process-spawning changes with `python3 scripts/check_execution_ownership.py`; scheduler changes also require
`python3 scripts/check_scheduler_bypass.py`. Keep the ownership manifest in
sync. Skills describe contracts; they do not grant execution permission.
