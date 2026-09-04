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
