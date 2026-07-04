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
| `projection.rs` | Phase 1 of the shell-output projection roadmap. Durable command event model: `CommandRun`, `CommandExit`, `CommandOutputStore`, `OutputHandle`, `default_command_projection` placeholder |
| `projection_bridge.rs` | `ShellCommandRunBridge` — sidecar accumulator that mirrors `ShellEvent`s into the `CommandOutputStore` |

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

### Key Methods

- `insert_started(req)` — Creates entry with `exit_code: None`, status `Running`
- `mark_exited(id, status, elapsed)` — Sets status to `Exited`, stores `exit_code: Option<i32>`, records elapsed time
- `mark_timeout(id, elapsed)` — Sets status to `TimedOut`
- `mark_failed_to_start(id)` — Sets status to `FailedToStart`
- `mark_killed(id, elapsed)` — Sets status to `Killed` with elapsed time. Late `Exited` events no longer overwrite `Killed` status.

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

### ShellOutputEntry

Each stored entry includes:
- `id`, `command`, `cwd`, `started_at`, `finished_at`
- `status`: `ShellStatus` (Running, Exited, TimedOut, FailedToStart, Killed)
- `exit_code: Option<i32>` — process exit code (None if killed or not yet exited)
- `stdout`, `stderr`: `BoundedOutput`
- `elapsed: Option<Duration>`, `promoted: bool`, `capture_policy`

## Policy Evaluation

`evaluate_command()` inspects normalized command text and returns:

- **Block**: rm -rf /, mkfs, dd to device, fork bombs, shutdown/reboot/halt/poweroff
- **Warn**: rm -rf ., git clean -f, sudo, curl|sh, chmod 777, recursive chown
- **Allow**: everything else

Blocked commands are refused before execution. Warned commands show a confirmation dialog when `confirm_dangerous` is enabled.

## Digest Extraction

`ShellDigest::build(status, ...)` extracts structured failure information from stdout/stderr:

- Rust compiler errors (`error[E\d+]`)
- Rust compiler warnings (`warning:`)
- Test failures (`test result: FAILED`, `failures:` blocks)
- Panics (`thread '...' panicked at '...'`)
- Generic non-zero exit codes
- Generates failures for `Killed`, `TimedOut`, and `FailedToStart` statuses

`ShellDigest::build_from_entry()` is a convenience constructor that takes a `&ShellOutputEntry` directly, extracting command, cwd, exit_code, elapsed, stdout, stderr, and status from the entry.

Used by the TUI to render concise failure summaries in the `ShellCell`.

## TUI Integration

### MsgPart::ShellCell

Renders shell output as a collapsible cell with:
- id, command, cwd
- stdout/stderr preview (head text)
- status (running/done/timeout/failed/killed)
- elapsed time, exit code
- truncation flag, promoted flag
- expanded/collapsed state

### `/shell-list` Display Format

The `/shell-list` command displays recent commands in a compact format:
```
[id] <status> $ <command>
```

Status labels vary by state:
- `running X.Xs` — command still in progress
- `done exit=N X.Xs` — exited with code N and elapsed time
- `done` — exited with no recorded exit code
- `timeout X.Xs` — killed by timeout
- `failed` — failed to start
- `killed X.Xs` — aborted by user via `/shell-kill`

The promoted state of each entry is visible in the detail view (`/shell-show`), where the `Promoted:` field shows `yes` or `no`.

Example: `[1] done exit=0 1.2s $ cargo test`

### `/shell-show` Display Format

The `/shell-show <id>` command opens a scrollable `InfoDialog` with full command details:

```
ID:       1
Command:  cargo test
CWD:      /path/to/project
Started:  1719650000
Finished: 1719650001
Elapsed:  1.2s
Status:   done
Exit:     0
Promoted: no
Capture:  StoreEphemeral

── stdout ──
  test result: ok. 5 passed; 0 failed

── stderr ──
  (empty)
```

### Shell Status Colors

Shell status labels use theme-aware colors for visual distinction:

| Status | Color |
|--------|-------|
| Running | Primary (active/highlighted) |
| Exited | Muted (secondary/gray) |
| Failed | Error (red) |
| Killed | Warning (yellow/orange) |
| TimedOut | Warning (yellow/orange) |
| FailedToStart | Error (red) |

### Shell Commands Reference

| Command | Description |
|---------|-------------|
| `!command` | Run shell command (ephemeral, hidden from model) |
| `!!command` | Run shell command and auto-promote output into context |
| `/shell-list` | List recent shell commands with status |
| `/shell-show <id>` | Show full details of a shell command in a scrollable dialog |
| `/shell-include <id> [stdout\|stderr\|all]` | Promote a specific command's output into context |
| `/shell-ask <id>` | Ask the agent about a command's output |
| `/shell-rerun <id>` | Re-execute a previous command |
| `/shell-kill <id>` | Abort a running command |

### `/shell-kill` Behavior

`/shell-kill <id>` aborts a running command and marks the store entry as `Killed` (not `Exited`) with proper elapsed time calculation. Late `Exited` events from the runtime no longer overwrite the `Killed` status.

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
- [.codegg/skills/human_shell/SKILL.md](../.codegg/skills/human_shell/SKILL.md) — Human shell skill guide
- [shell_output_projection_phase_01_command_event_model.md](../plans/shell_output_projection_phase_01_command_event_model.md) — Phase 1 plan this section implements
- [shell_output_projection_rtk_roadmap.md](../plans/shell_output_projection_rtk_roadmap.md) — Full roadmap

## Command Output Projection (Phase 1)

Phase 1 of the [shell output projection roadmap](../plans/shell_output_projection_rtk_roadmap.md) introduces a structured command event that becomes the durable substrate for projection, expansion, redaction, and TUI expansion. It is implemented in `src/shell/projection.rs` and `src/shell/projection_bridge.rs`. The system runs **alongside** the existing `ShellOutputStore` — it does not replace it.

### Why Two Stores

| Store | Purpose | Retention |
|-------|---------|-----------|
| `ShellOutputStore` (existing) | Ephemeral TUI transcript: bounded head/tail for `ShellCell` rendering, digests, `/shell-include` promotion | 1 MB per command, 8 MB total, head + tail only |
| `CommandOutputStore` (new) | Durable raw stdout/stderr for the projection pipeline; resolved by stable `cmd://<id>/<stream>` handles | 32 MiB per stream, 64 MiB total, full prefix retained |

Both stores are populated by the same `ShellEvent` stream. The legacy store keeps lossy head/tail previews for the TUI; the projection store keeps the raw bytes that later projectors, expansion requests, and redaction passes need.

### Core Types

```rust
pub struct CommandRun {
    pub id: CommandRunId,
    pub command: String,
    pub argv: Option<Vec<String>>,
    pub cwd: PathBuf,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub exit: CommandExit,
    pub stdout: RawStream,
    pub stderr: RawStream,
    pub combined: Option<RawStream>,
    pub projection: Option<ProjectionHandle>,
    pub redaction: RedactionState,
}

pub enum CommandExit {
    Code(i32),
    Signal { signal: i32 },
    Timeout,
    Cancelled,
    SpawnFailed { message: String },
    InternalError { message: String },
}

pub struct OutputHandle {
    pub command_id: CommandRunId,
    pub stream: CommandOutputStream, // Stdout | Stderr | Combined
}
```

`OutputHandle` round-trips through the canonical URL form `cmd://<id>/<stream>` (e.g. `cmd://42/stdout`, `cmd://42/stderr`). `CommandOutputStore::parse_handle` resolves URLs back into handles.

### CommandOutputStore API

```rust
impl CommandOutputStore {
    pub fn alloc_id(&self) -> CommandRunId;
    pub fn insert(&mut self, id: CommandRunId, command: String, cwd: PathBuf,
                  started_at: SystemTime, stdout: Vec<u8>, stderr: Vec<u8>) -> CommandRun;
    pub fn record_exit(&mut self, id: CommandRunId, exit: CommandExit, duration: Duration);
    pub fn get_run(&self, id: CommandRunId) -> Option<&CommandRun>;
    pub fn get_stream(&self, handle: OutputHandle) -> Option<&[u8]>;
    pub fn get_range(&self, handle: OutputHandle, range: Range<usize>) -> Option<&[u8]>;
    pub fn byte_len(&self, handle: OutputHandle) -> Option<usize>;
    pub fn parse_handle(&self, url: &str) -> Option<OutputHandle>;
}
```

Per-stream bytes are capped at `COMMAND_OUTPUT_MAX_SINGLE_STREAM_BYTES` (32 MiB). When a stream exceeds the cap, the prefix is retained and `OutputCompleteness::Partial` is set on the corresponding `RawStream` so downstream code can tell the difference between "the command produced small output" and "we only kept the head of large output". Total retention is capped at `COMMAND_OUTPUT_MAX_RETAINED_BYTES` (64 MiB) and history is capped at `COMMAND_OUTPUT_MAX_HISTORY_ENTRIES` (100 commands); eviction is LRU.

### ShellCommandRunBridge

The bridge in `src/shell/projection_bridge.rs` is a sidecar accumulator that mirrors `ShellEvent`s into the `CommandOutputStore`:

- `Started` records the command metadata and reserves an entry.
- `Stdout` / `Stderr` append bytes to the in-flight buffer.
- `Exited` / `TimedOut` / `FailedToStart` finalize the entry into the store with the appropriate `CommandExit` and duration.

`FailedToStart` arriving without a prior `Started` is handled by synthesizing an empty entry so the projection pipeline still has a record.

The bridge is invoked from `src/tui/commands/shell.rs::handle_shell_event` before the legacy store update, so every `ShellEvent` populates both stores.

### Default Projection Boundary

`default_command_projection(run, store)` is the Phase 1 placeholder for the model-visible projection seam. It produces a compact text view containing:

- command ID and command string
- cwd, exit label, duration
- truncated stdout and stderr (bounded by `DEFAULT_PROJECTION_BUDGET_BYTES`, default 8 KiB per stream)
- raw retention handles (`cmd://<id>/stdout`, `cmd://<id>/stderr`)

It does NOT parse command shape, select projectors, or invoke external backends. Phase 2 replaces it with the real `CommandOutputProjector` trait and the `Raw` / `Truncated` / `ErrorRetention` projectors. Phase 1's job is to make every model-facing command output flow through one seam.

### App Integration

`App` carries:

- `command_run_store: CommandOutputStore` — durable raw output for projection
- `command_run_bridge: ShellCommandRunBridge` — in-flight accumulator

Both are constructed in the App's `new` and the test constructor. They are owned by `App` and do not yet feed any UI surface — the projection seam is wired but not yet exposed in the TUI. Phase 4 will add TUI metadata display and Phase 7 will add expansion UI.

### Stability Guarantees

- Command IDs are stable for the lifetime of the store.
- `cmd://<id>/<stream>` URLs resolve to the same bytes until the run is evicted by retention limits.
- Combined stream is **not** synthesized in Phase 1 — `get_stream` returns `None` for `CommandOutputStream::Combined` unless the execution layer supplies it explicitly.

### What's NOT in Phase 1

- Real projector trait (Phase 2)
- Native structured projectors (Phase 3)
- Configuration schema for projection policy (Phase 4)
- RTK backend (Phase 5)
- TUI expansion panel (Phase 7)
- Redaction pipeline (Phase 8)
