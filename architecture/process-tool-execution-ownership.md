# Process and Tool Execution Ownership

## Contract

`ManagedProcessService` in `src/managed_process.rs` is the canonical owner for
finite local process lifecycle. Callers provide typed argv, an explicit
working directory, environment policy, provenance, timeout/cancellation,
stdin, output, and sandbox policy. The service owns process-group/session
setup, bounded concurrent stdout/stderr draining, timeout and cancellation
termination, sandbox-helper coordination, exit classification, cleanup, and
reaping. Callers only map the typed result into their domain or protocol.

```text
authorization / schema / scheduler admission
                  |
                  v
       ManagedProcessRequest (typed argv)
                  |
                  v
       ManagedProcessService::run
       /                         \
  captured one-shot        streaming foreground
  or run_blocking          (bounded chunks + final result)
                  |
                  v
       ManagedProcessResult / domain projection
```

`run_blocking` is a bounded adapter for synchronous callers; it does not
admit durable work. `run_streaming` forwards bounded output chunks while
retaining a bounded final result. `EnvironmentPolicy::sanitized()` is the
default for machine/tool execution; `inherited()` is reserved for human shell
semantics and still strips known unsafe Git environment variables.

Durable or background work is admitted by `JobSubmissionService` and runs
through a scheduler executor. The process service executes an accepted
attempt; it is not a second scheduler or job authority.

## Disposition of production process sites

The machine-readable inventory in `docs/execution-ownership.toml` is the
complete guard input. Its production dispositions are:

| Surface | Disposition | Boundary |
|---|---|---|
| `src/managed_process.rs` | Canonical | Only finite-process direct-spawn owner; owns lifecycle and safety primitives. |
| `src/interactive_process.rs` | Canonical interactive | Only PTY direct-spawn owner (M001). Every spawn holds a scheduler `AdmissionController` permit acquired before `openpty` (ManagedProcess resource class, no exclusivity key); cwd/env derive from the immutable `ExecutionContext`; bounded sequence-numbered scrollback; input/resize/terminate over the master fd; child process-group (setsid leader) cleanup with SIGTERM-then-SIGKILL escalation on terminate/shutdown. Handles are ephemeral UUIDs and do not survive daemon restart. No model-facing tool is registered here. |
| `src/interactive_process_attach.rs` | Interactive protocol adapter (M002) | No direct-spawn owner: the bounded attach/resume family (`create/list/attach/detach/input/resize/terminate/remove/resume`) delegates every lifecycle call to the M001 service. Attachment ownership is keyed by transport `client_id`; mutating payloads name an `attachment_id`, never a handle. Output reads are on-demand from the shared M001 ring (no per-attachment queue or task); lag answers typed resync. The daemon shares the scheduler's admission controller with the M001 engine so process-slot accounting stays single. |
| `src/tool/bash.rs`, `src/scheduler/`, `src/python_script/` | Scheduler/adapter | Job admission and domain output remain local; accepted finite execution uses the canonical service. |
| `src/shell/runtime.rs` | Interactive adapter | Human `$SHELL -lc` semantics and shell events remain local; streaming lifecycle uses the canonical service. |
| `src/shell/rtk.rs`, `src/tool/formatter.rs`, `src/tool/terminal.rs`, `src/ide/` | Blocking adapters | Authorization, parsing, and presentation remain local; timeout, bounded capture, cwd, env, and cleanup use the canonical service. |
| `src/hooks/` | Configured hook adapter | Hook-specific context remains local; finite command lifecycle uses the canonical service. |
| `src/plugin/runtime/process.rs` | Deferred domain adapter | Plugin protocol/result mapping remains local; finite child lifecycle uses the canonical service. Plugin admission/lifecycle integration remains future domain work. |
| `crates/egglsp/src/launch.rs` | Protocol-specialized | LSP owns long-lived JSON-RPC stdin/stdout framing and restart state. It cannot depend on the root service without a dependency cycle; its explicit environment and child cleanup are documented in `architecture/lsp.md`. |
| `src/mcp/local.rs` | Protocol-specialized | MCP owns JSON-RPC framing and persistent connection state; it is not a finite captured process. |
| `src/core/transport/stdio.rs` | Standalone compatibility | Deprecated stdio core transport owns protocol framing and child connection state. |
| `src/core/instance.rs`, `src/tui/app/`, `src/tts/`, `src/core/notification.rs`, `src/upgrade/` | Standalone/interactive exceptions | Daemon bootstrap, external editor, speech, and self-upgrade are explicit administrative or user-controlled surfaces. |
| `src/bin/codegg-sandbox-helper.rs` | Service adapter | Installation-owned helper applies Landlock and replaces itself with the already-validated target; it is launched only by `ManagedProcessService`. |
| `src/git_*.rs`, `crates/egggit/`, `crates/codegg-core/src/{worktree.rs,worktree_service.rs,repository_lineage.rs}` | Deferred domain | Typed Git/worktree/read probes retain domain semantics and are tracked for M003 Git ownership convergence. Test-only fixtures are annotated separately. |

Every exception is represented in the manifest with an owner and reason.
New direct process or dispatch sites fail
`scripts/check_execution_ownership.py` unless they are classified or
line-annotated.

## Safety and compatibility

The migration keeps shell-session IDs, Tool Program run IDs, projection
events, and user-visible command categories unchanged. It preserves explicit
cwd and environment construction, Unix process-tree termination with bounded
grace and reaping, cancellation and timeout classification, bounded
head-plus-tail capture and streaming, sandbox-helper status isolation and
Landlock behavior, secret-safe diagnostics, and existing authorization
boundaries.

Protocol children remain direct owners only where framing, persistence, or
crate dependency direction makes finite-process capture the wrong abstraction.
Their ownership is explicit rather than hidden behind a generic shell helper.

## Session vs interactive process (M001)

A human `Session` is a conversation/turn identity; a scheduler `Job` is
durable admitted work; a `Run` is an execution record. An interactive-process
handle (`src/interactive_process.rs`) is none of these: it names one
ephemeral PTY-backed process group on the node that owns the workspace.
Attach/detach/resume protocol (M002) and TUI views (M003) consume handles;
they never reinterpret a `Session` id as a terminal, and terminating a
process never closes a conversation. The legacy metadata-only
`src/shell_session/` store and the one-shot deferred `terminal` model tool
are not execution owners and remain unchanged by M001.

## Attach ownership and resync (M002)

Attachment is a transport-bound subscription, not authorization transfer:
`InteractiveAttachmentRegistry` maps `attachment_id -> (handle, client_id)`
and every lookup checks the caller's transport `client_id`, so unknown and
foreign attachment IDs answer the same `interactive_attachment_gone` code.
`create` returns a bare handle; only `attach` mints an attachment, and
re-attach is idempotent per (client, handle). `detach` and connection close
(`handle_disconnect`) release attachments without touching processes;
explicit `terminate` (bounded escalation, `InteractiveProcessExited` event)
or `remove` are the only kill paths, and both require attachment ownership
plus terminate authority (local-owner transport, or a plugged semantic
capability through the same `InteractiveAuthority` context — no wire
change). Resume from a sequence cursor returns the next bounded chunk or a
typed `InteractiveResync` (`HistoryExpired` with both cursors, `CursorAhead`,
`HandleGone` after remove/restart); history is never silently shifted.
