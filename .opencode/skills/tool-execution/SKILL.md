---
name: tool-execution
description: Preserve CodeGG's broker and scheduler ownership when changing model-facing tools or subprocess work.
---

# Tool execution ownership

`ToolBroker` is the production boundary for model tool calls. Keep tool registration, authorization, permission checks, audit events, and dispatch on that path.

Heavy or durable work such as tests, managed processes, subagents, and tool programs goes through `JobSubmissionService` into `JobScheduler`. Do not call an executor directly from a tool, command handler, or UI callback. Human `!cmd` input is hidden from the model; `!!cmd` promotes bounded and redacted output.

When changing a tool, verify its schema and dispatch together, preserve cancellation and stale-completion handling, and cover denied or malformed requests. TUI handlers that take time use `spawn_tui_task` with `finish(request_id)` or `fail(request_id, error)` so stale completions cannot overwrite newer state.

Read `architecture/agent.md`, `architecture/jobs.md`, `architecture/scheduler.md`, and `architecture/human_shell.md` before changing those boundaries. This guide describes current ownership; it does not authorize a new executor or permission path.

For test selection and qualification commands, see `.opencode/skills/testing-ci/SKILL.md`.
