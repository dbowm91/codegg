# Human Shell Module

The `shell` module provides human-initiated shell command execution with bounded output storage, safety policy enforcement, and a promotion model that keeps ephemeral commands out of the model context.

## Overview

**Location**: `src/shell/`

**Central invariant**: A human `!` command is not model context unless the user explicitly promotes it.

## Syntax

| Syntax | Behavior |
|--------|----------|
| `!command` | Runs shell command; output stored but not sent to the model (ephemeral) |
| `!!command` | Runs shell command; output auto-promoted into the conversation context |
| `/shell-list` | Lists recent shell commands with status |
| `/shell-include <id> [stdout\|stderr\|all]` | Promotes a specific command's output into context |
| `/shell-rerun <id>` | Re-executes a previous command |
| `/shell-kill <id>` | Aborts a running command |

## Module Structure

| File | Contents |
|------|----------|
| `types.rs` | Core types: `ShellOrigin`, `ShellCapturePolicy`, `ShellCommandId`, `ShellRequest`, `ShellEvent`, `ShellStatus`, `PromptSubmissionKind`, `classify_prompt_submission()` |
| `runtime.rs` | `ShellRuntime` (spawns shell child processes via `$SHELL -lc`), `ShellHandle` (abort handle) |
| `store.rs` | `ShellOutputStore` (bounded VecDeque), `BoundedOutput` (head/tail split), `ShellOutputEntry` |
| `policy.rs` | `HumanShellPolicyDecision` (Allow/Warn/Block), `evaluate_command()` |
| `digest.rs` | `ShellDigest` (structured failure extraction), `ShellFailure`, `TruncationReport` |

## Key Types

### ShellOrigin

```rust
pub enum ShellOrigin {
    HumanEphemeral,  // User typed !command
    HumanPromoted,   // User typed !!command or /shell-include
    AgentTool,       // Agent-initiated via bash tool
}
```

### ShellCapturePolicy

```rust
pub enum ShellCapturePolicy {
    DisplayOnly,      // No storage at all
    StoreEphemeral,   // Stored in history but not in model context
    StoreAndPromote,  // Stored and promoted into model context
}
```

### ShellCommandId

Newtype wrapper around `u64`. Monotonically increasing, allocated by `ShellOutputStore::alloc_id()`.

### ShellRequest

```rust
pub struct ShellRequest {
    pub id: ShellCommandId,
    pub origin: ShellOrigin,
    pub command: String,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub capture_policy: ShellCapturePolicy,
}
```

### ShellEvent

Event stream emitted during command execution:

- `Started { id, command, cwd }` — command spawned
- `Stdout { id, bytes }` — stdout data chunk
- `Stderr { id, bytes }` — stderr data chunk
- `Exited { id, status, elapsed }` — process exited
- `TimedOut { id, elapsed }` — timeout killed the process
- `FailedToStart { id, error }` — spawn failed

### PromptSubmissionKind

```rust
pub enum PromptSubmissionKind {
    Chat(String),
    Slash(String),
    HumanShell { command: String, promote_after: bool },
}
```

`classify_prompt_submission()` parses user input: `!!cmd` → promote_after=true, `!cmd` → promote_after=false, `/cmd` → Slash, everything else → Chat.

## ShellRuntime

Spawns commands via the user's `$SHELL` (fallback `sh`) with `-lc` argument. Sends stdout/stderr as byte chunks over an `mpsc::Sender<ShellEvent>`. Returns a `ShellHandle` with an abort handle for killing.

Key behaviors:
- `kill_on_drop(true)` on the child process
- Timeout enforced via `tokio::time::timeout` (default 300s)
- stdout/stderr readers run as separate tokio tasks
- Exit task collects both reader completions before emitting `Exited`

## ShellOutputStore

Bounded in-memory store using `VecDeque<ShellOutputEntry>`.

### Storage Limits

| Default | Value |
|---------|-------|
| `max_entries` | 100 |
| `max_bytes_per_command` | 1 MB (head 256KB + tail 256KB) |
| `max_total_bytes` | 8 MB |

### Eviction

- By count: oldest entries removed when `len > max_entries`
- By total bytes: oldest entries removed when total > `max_total_bytes` (keeps at least 1)

### BoundedOutput

Each command's stdout/stderr is stored as a `BoundedOutput`:
- `head`: first 256KB of output
- `tail`: last 256KB of output
- `omitted_bytes`: bytes dropped from the middle
- `total_bytes`, `total_lines`: full counts

## Policy Evaluation

`evaluate_command()` inspects normalized command text and returns:

- **Block**: rm -rf /, mkfs, dd to device, fork bombs, shutdown/reboot/halt/poweroff
- **Warn**: rm -rf ., git clean -f, sudo, curl|sh, chmod 777, recursive chown
- **Allow**: everything else

Blocked commands are refused before execution. Warned commands show a confirmation dialog when `confirm_dangerous` is enabled.

## Digest Extraction

`ShellDigest::build()` extracts structured failure information from stdout/stderr:

- Rust compiler errors (`error[E\d+]`)
- Rust compiler warnings (`warning:`)
- Test failures (`test result: FAILED`, `failures:` blocks)
- Panics (`thread '...' panicked at '...'`)
- Generic non-zero exit codes

Used by the TUI to render concise failure summaries in the `ShellCell`.

## TUI Integration

### MsgPart::ShellCell

Renders shell output as a collapsible cell with:
- id, command, cwd
- stdout/stderr preview (head text)
- status (running/done/timeout/failed)
- elapsed time, exit code
- truncation flag, promoted flag
- expanded/collapsed state

### TuiCommand Variants

| Variant | Trigger | Behavior |
|---------|---------|----------|
| `RunHumanShell { command, promote_after }` | `!cmd` or `!!cmd` | Spawns via ShellRuntime |
| `ShellEvent(ShellEvent)` | Runtime events | Updates ShellOutputStore, renders ShellCell |
| `ShellInclude { id, mode, question }` | `/shell-include` | Promotes output into context |
| `ShellRerun { id }` | `/shell-rerun` | Re-executes command |
| `ShellKill { id }` | `/shell-kill` | Aborts running command |
| `ShellList` | `/shell-list` | Shows recent commands |

## Configuration

```json
{
  "human_shell": {
    "enabled": true,
    "default_timeout_secs": 300,
    "max_history_entries": 100,
    "max_bytes_per_command": 1000000,
    "max_total_bytes": 8000000,
    "ansi": "stripped",
    "confirm_dangerous": true,
    "auto_promote_bangbang": true
  }
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Enable/disable human shell |
| `default_timeout_secs` | `300` | Default command timeout |
| `max_history_entries` | `100` | Max stored command entries |
| `max_bytes_per_command` | `1000000` | Max bytes stored per command |
| `max_total_bytes` | `8000000` | Max total bytes across all commands |
| `ansi` | `"stripped"` | ANSI handling mode |
| `confirm_dangerous` | `true` | Confirm before executing warned commands |
| `auto_promote_bangbang` | `true` | Auto-promote `!!` output into context |

## See Also

- [tool.md](tool.md) — Agent bash tool (uses `ShellOrigin::AgentTool`)
- [.opencode/skills/human-shell/SKILL.md](../.opencode/skills/human-shell/SKILL.md) — Human shell skill guide
