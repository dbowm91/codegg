# Managed Process

Canonical execution service for scheduler-owned non-shell argv processes.

## Purpose

`ManagedProcessService` (`src/managed_process.rs`) provides a stateless async entry point for executing OS processes with environment sanitization, bounded output capture, cancellation, timeout, and process-group cleanup. It is the execution backend for scheduler-owned jobs that need argv-based process spawning.

## Key Types

### ManagedProcessRequest

Full request struct for process execution:

| Field | Type | Description |
|-------|------|-------------|
| `argv` | `Vec<String>` | Executable + arguments |
| `cwd` | `PathBuf` | Working directory |
| `env_policy` | `EnvironmentPolicy` | Env var allowlist/denylist |
| `stdin` | `StdinPolicy` | Null or bytes |
| `timeout` | `Option<Duration>` | Kill after timeout |
| `cancellation` | `CancellationToken` | Cooperative cancellation |
| `output_policy` | `OutputPolicy` | Bounded stdout/stderr limits |
| `sandbox` | `SandboxRequest` | Landlock sandbox config |
| `provenance` | `ProcessProvenance` | Job/attempt IDs for audit |

### EnvironmentPolicy

Allowlist-based environment control:

| Method | Purpose |
|--------|---------|
| `sanitized()` | Default: strips to `codegg-git` allowlist |
| `allow_inherited_var(name)` | Whitelist an inherited env var |
| `deny_var(name)` | Explicitly deny a var |
| `with_var(k, v)` | Set a specific var |

### OutputPolicy

Bounded stdout/stderr capture:

- `new(max_bytes)` — single limit for both streams
- `.with_limits(stdout, stderr)` — separate limits
- `.terminate_on_overflow()` — kill process on limit exceeded

### BoundedOutput

Head/tail capture ring that never exceeds the configured cap:

- `is_truncated()` — true if output was clipped
- `retained_bytes()` — bytes actually kept
- `omitted_bytes` — bytes dropped

### ManagedProcessResult

| Field | Type | Description |
|-------|------|-------------|
| `exit_status` | `ExitStatus` | Process exit code |
| `stdout` | `BoundedOutput` | Bounded stdout |
| `stderr` | `BoundedOutput` | Bounded stderr |
| `duration` | `Duration` | Wall-clock time |
| `termination_reason` | `TerminationReason` | Why the process stopped |
| `cleanup_diagnostics` | `CleanupDiagnostics` | Process-group cleanup info |

### TerminationReason

| Variant | Meaning |
|---------|---------|
| `Exited` | Normal exit |
| `TimedOut` | Killed by timeout |
| `Cancelled` | Killed by cancellation token |
| `OutputLimitExceeded` | Killed because output exceeded bounds |

## Execution Flow

```
ManagedProcessRequest
    │
    ▼
Validate argv (non-empty, no invalid args)
    │
    ▼
Apply EnvironmentPolicy (allowlist/denylist)
    │
    ▼
Setup process group (setsid on Unix)
    │
    ▼
Optional: Apply Landlock sandbox
    │
    ▼
Spawn process with bounded output readers
    │
    ▼
Wait for: exit / timeout / cancellation / output limit
    │
    ▼
Graceful termination (250ms SIGTERM → SIGKILL)
    │
    ▼
Cleanup process group
    │
    ▼
ManagedProcessResult
```

## Process Group Management

On Unix, processes are spawned in a new session (`setsid`) to ensure clean cleanup of the entire process tree. On termination, the service sends SIGTERM, waits 250ms, then SIGKILL if needed.

## Sandbox Integration

When `SandboxRequest::Required` is specified, the process is launched through `codegg-sandbox-helper` which applies Landlock filesystem restrictions before exec'ing the target process.

## Usage

```rust
let request = ManagedProcessRequest::new(
    vec!["cargo".into(), "test".into()],
    workspace_root.into(),
    ProcessProvenance::new(job_id, attempt_id),
)
.with_timeout(Duration::from_secs(300))
.with_output_policy(OutputPolicy::new(1024 * 1024)); // 1MB

let result = ManagedProcessService::run(request).await?;
```

## See Also

- [Scheduler](scheduler.md) — JobScheduler dispatches to ManagedProcessService
- [Jobs](jobs.md) — Durable job lifecycle
- [Tool Broker](tool_broker.md) — Tool execution boundary
- [Tool Programs](tool_programs.md) — Programs that may spawn managed processes
