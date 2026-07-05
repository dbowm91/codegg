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
| `projection.rs` | Phase 1 of the shell-output projection roadmap. Durable command event model: `CommandRun`, `CommandExit`, `CommandOutputStore`, `OutputHandle`, `default_command_projection` seam |
| `projector.rs` | Phase 2+3 of the shell-output projection roadmap. `CommandOutputProjector` trait, `ProjectionRequest`/`ProjectionResult`, `RawProjector` / `TruncatedProjector` / `ErrorRetentionProjector` + Phase 3 native projectors (`GitStatusProjector`, `GitDiffProjector`, `GitLogProjector`, `CargoCheckProjector`, `CargoTestProjector`), `ProjectionSelector`, redaction hook |
| `rtk.rs` | Phase 5: `RtkDiscovery`, `RtkAvailability`, `RtkState`, `RtkCapabilities`, `CapabilityState`, `CompressionEligibility`, `classify_command()`, `RtkProjector` |
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
- [shell_output_projection_phase_02_projection_trait.md](../plans/shell_output_projection_phase_02_projection_trait.md) — Phase 2 plan (projector trait + built-ins)
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

In Phase 2 this function delegates to the [`ProjectionSelector`](#command-output-projection-phase-2); the string it returns is the `text` field of the resulting `ProjectionResult`. Phase 1's job was to make every model-facing command output flow through one seam; Phase 2 keeps that seam and layers a real projector trait on top of it.

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

- Real projector trait (Phase 2) — landed in Phase 2
- Native structured projectors (Phase 3) — landed
- Configuration schema for projection policy (Phase 4)
- RTK backend (Phase 5) — landed
- TUI expansion panel (Phase 7)
- Redaction pipeline (Phase 8) — redaction hook placeholder exists in Phase 2

## Command Output Projection (Phase 2)

Phase 2 of the [shell output projection roadmap](../plans/shell_output_projection_rtk_roadmap.md) introduces the projector trait, three conservative built-in projectors, a centralised selector, and a redaction hook placeholder. It is implemented in `src/shell/projector.rs` and re-exported from `src/shell/mod.rs`. Phase 1's domain types (`CommandRun`, `CommandOutputStore`, `OutputHandle`, `RedactionState`) are unchanged.

### Why a Trait

Projecting raw command output is intrinsically plural: small successes want exact text, long successes want bounded head/tail, failures want error-line retention, and command-specific projectors (Phase 3) want shape-based parsing. Wrapping these behind a single trait lets the selector pick the right view per request without callers having to inspect command shape themselves.

```rust
pub trait CommandOutputProjector: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports(&self, request: &ProjectionRequest<'_>) -> ProjectionSupport;
    fn project(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
    ) -> Result<ProjectionResult, ProjectionError>;
}
```

The trait is intentionally backend-agnostic. RTK (Phase 5) and model-generated summaries are later implementations of the same trait, not a parallel pipeline.

### Request, Result, and Metadata

Every projector receives a [`ProjectionRequest`] and returns a [`ProjectionResult`]. The result carries more than text — it carries the provenance and risk metadata the model needs to know whether it is seeing an exact view:

- `projector` — stable name of the projector that produced the result (e.g. `"raw"`, `"truncated"`, `"error-retention"`)
- `kind` — [`ProjectionKind`] (Raw / Truncated / ErrorRetention / Structured / ExternalCompressed / Summary)
- `exactness` — [`ProjectionExactness`] (Exact / ExactRange / Truncated / Lossy / Parsed / PartialRawArtifact)
- `redaction` — [`RedactionState`] (whether the redaction hook fired)
- `omitted` — every [`OmittedRange`] (stream, byte range, line range, total retained bytes, note)
- `expansion_handles` — [`ExpansionHandle`] values the consumer can use to fetch the omitted bytes (`cmd://<id>/<stream>` or `cmd://<id>/<stream>#<start>-<end>`)
- `input_bytes` / `output_bytes` / `estimated_input_tokens` / `estimated_output_tokens` / `warnings`

`ProjectionResult::banner(run)` renders a compact metadata line that prefixes the text and tells the model the projector, exactness, and redaction state without requiring it to parse free-form projection output.

### Built-in Projectors

| Projector | Selects when | Output shape |
|-----------|--------------|--------------|
| `RawProjector` | Total retained output ≤ budget, or caller asked for exact | Command header + raw stdout/stderr text + raw handles. Marks `PartialRawArtifact` when the underlying store is itself partial. |
| `TruncatedProjector` | Long successful output | Command header + bounded head + explicit omission marker + bounded tail. Stderr is always shown in full when it fits; otherwise it is also head/tail-bounded but stderr is never silently dropped. |
| `ErrorRetentionProjector` | Command failed (non-zero exit / timeout / cancellation / spawn failure) | Command header + only lines matching Rust/Python/JS/generic error patterns + bounded context around them. Falls back to head/tail when no patterns match. Marks `Lossy` exactness. |

All three are conservative: no RTK, no command-shape inspection, no model-generated summaries. They are designed to be a reliable internal boundary that the Phase 3 native projectors and the Phase 5 RTK backend will sit on top of.

### Selector and Policy

[`ProjectionSelector::with_defaults`] returns a selector that tries projectors in priority order (`RawProjector` → `GitStatusProjector` → `GitDiffProjector` → `GitLogProjector` → `CargoCheckProjector` → `CargoTestProjector` → `ErrorRetentionProjector` → `TruncatedProjector`) and picks the first one whose `supports()` returns `Preferred` or, failing that, `Supported` or `Fallback`. The selector's `project(request, store)` method:

1. Looks up the matching projector.
2. Invokes the projector with the request.
3. Applies the redaction hook if the request target requires redaction (`ModelContext` / `ToolExpansion`) and the policy allows it.
4. Returns a `ProjectionResult`. On projector error it returns a result with the error text and a warning so callers can still surface raw handles.

[`ProjectionPolicy`] is constructed once and threaded into every request. The conservative default enables the redaction hook for model-facing targets and disables external backends (RTK, etc.).

[`ProjectionBudget`] carries a byte cap plus optional token hints. Phase 2 uses a rough `bytes / 4` token estimate; the goal is to establish the budget plumbing, not to ship a perfect estimator.

### Redaction Hook Placeholder

Phase 8 will implement full redaction. Phase 2 already includes a hook at the model-facing boundary:

```rust
pub fn apply_redaction_hook(result: &mut ProjectionResult, target: ProjectionTarget)
```

The current implementation is a no-op that flips `RedactionState` to `Applied` so the metadata banner reflects that the hook fired. Crucially, the call site exists in `ProjectionSelector::project`, so future redaction implementations cannot be bypassed by RTK or native projectors.

### Stability Guarantees (Phase 2)

- `default_command_projection(run, store)` and `default_command_projection_with_budget(run, store, budget)` keep the Phase 1 signatures.
- `ExpansionHandle::as_url` extends the existing `cmd://<id>/<stream>` URL form with an optional `#<start>-<end>` byte range fragment.
- The selector is `Debug` and constructed from `Box<dyn CommandOutputProjector>` so later phases can append native projectors and RTK-backed projectors without changing the public API.

### What's NOT in Phase 2

- Native structured projectors (Phase 3: Git, Rust, ...) — landed
- Configuration schema for projection policy (Phase 4)
- RTK backend (Phase 5) — landed
- TUI expansion panel (Phase 7)
- Full redaction pipeline (Phase 8) — the hook site is in place, but the redaction rules are not implemented yet
- Per-run `ProjectionHandle` carrying the resolved `ProjectionResult` (deferred; today the result lives in selector return values and any caller that wants to keep it can stash it on the run manually)

## Command Output Projection (Phase 3)

Phase 3 adds native structured projectors that parse command-specific output into semantically meaningful, low-token summaries. These projectors are registered in `ProjectionSelector::with_defaults()` after `RawProjector` and before the generic fallback projectors.

### Native Projectors

| Projector | `name()` | Selects when | Output shape |
|-----------|----------|--------------|--------------|
| `GitStatusProjector` | `native-git-status` | `git status` with allowed flags (`--porcelain`, `--short`, `--branch`, etc.) | Structured summary: branch info, staged/unstaged/untracked/conflicted file counts with filenames |
| `GitDiffProjector` | `native-git-diff` | `git diff`, `git diff --cached/--staged`, `git show` | File stats with hunk previews (≤5 files, ≤3 hunks each) |
| `GitLogProjector` | `native-git-log` | `git log` with any flags | Compact commit list capped at 20 entries (hash, subject, author) |
| `CargoCheckProjector` | `native-cargo-diagnostics` | `cargo check`, `cargo build`, `cargo clippy` | Parsed Rust diagnostics: error codes, file locations, notes/help |
| `CargoTestProjector` | `native-cargo-test` | `cargo test` | Test result summary with failure details and panic output |

### Selector Priority

The updated selector order is:
```
RawProjector → GitStatus → GitDiff → GitLog → CargoCheck → CargoTest → ErrorRetention → Truncated
```

Native projectors return `ProjectionSupport::Preferred` when their command matches, and `Unsupported` otherwise. The selector picks the first `Preferred` projector, falling through to generic projectors for unrecognized commands.

All native projectors produce `ProjectionKind::Structured` with `ProjectionExactness::Parsed` and include raw expansion handles for full output access.

### Helper Functions

- `base_command_name(run)` — extracts the base command name from argv or command string
- `command_args(run)` — extracts the argument list from argv or command string
- `make_native_result(name, text, run, expansion_handles, omitted, warnings)` — builds a `ProjectionResult` with `Structured` kind and `Parsed` exactness

### What's NOT in Phase 3

- Configuration schema for projection policy (Phase 4)
- RTK backend (Phase 5) — landed
- TUI expansion panel (Phase 7)
- Full redaction pipeline (Phase 8) — the hook site is in place, but the redaction rules are not implemented yet

## Command Output Projection (Phase 4 — partial)

Phase 4 provides configuration-driven projection policy and TUI metadata display. The config schema and selector integration are landed; per-command rules and full escape hatches are deferred.

### Config Schema

`ShellOutputConfig` in `crates/codegg-config/src/schema.rs` defines:

```toml
[shell.output]
projection = "safe"           # off | safe | rtk | aggressive (default: safe)
retain_raw = true             # default: true
redact_model_visible_output = "model_only"  # off | model_only | all (default: model_only)
max_model_output_tokens = 4000              # default: 4000
max_tui_output_bytes = 200000               # default: 200000
show_projection_metadata = true             # default: true
prefer_native_projectors = true             # default: true

[shell.output.rtk]
enabled = false               # default: false
path = "rtk"                  # optional explicit path
eligible_only = true          # default: true
timeout_ms = 5000             # default: 5000
allow_side_effecting_commands = false
```

`ProjectionSelector::with_config()` builds the appropriate selector from this config, including RTK when enabled.

### What's Landed

- Config schema (`ShellOutputConfig`, `ProjectionPolicyKind`, `ProjectionRedactPolicy`, `ShellOutputRtkConfig`, `ShellOutputRuleConfig`)
- `ProjectionPolicy::from_config()`, `ProjectionBudget::from_config()`, `ProjectionSelector::with_config()`
- TUI metadata display via `ProjectionResult::banner()`

### What's Deferred

- Per-command rules (parsed but not consumed by projection pipeline)
- Escape hatches and rule-based projector selection
- Full TUI metadata panel (Phase 7)

## Command Output Projection (Phase 5 + Phase 6)

Phase 5 adds RTK as an optional, detected command-output compressor backend behind the projection abstraction. It is implemented in `src/shell/rtk.rs` and integrated into the selector via `ProjectionSelector::with_rtk()` and `ProjectionSelector::with_config()`.

### RTK Discovery

`RtkDiscovery` handles lazy detection of the RTK binary:

- Probes on first use (not at startup)
- Resolves configured path or searches `$PATH`
- Runs `rtk --version` with configurable timeout
- Caches availability state

`RtkAvailability` carries the probe result with a `RtkState` enum:

| State | Meaning |
|-------|---------|
| `Disabled` | Config has RTK disabled |
| `Available` | RTK found and working |
| `NotFound` | Binary not on PATH |
| `Broken` | Found but version probe failed |
| `TimedOut` | Version probe exceeded timeout |
| `UnsupportedVersion` | Incompatible version |

`RtkDiscovery::probe_capabilities()` probes the available RTK for specific behavior, returning `RtkCapabilities` where each capability is `CapabilityState::Yes`, `No`, or `Unknown`.

### Eligibility Classification

`classify_command()` inspects command text and returns a `CompressionEligibility`:

| Category | Meaning | Example commands |
|----------|---------|-----------------|
| `EligibleReadOnly` | Safe to compress; no side effects | `git status`, `git diff`, `git log`, `rg`, `ls`, `find`, `cat` |
| `EligibleWithRawCapture` | Compressible but needs raw capture (reserved) | (future use) |
| `IneligibleSideEffecting` | Has side effects; must not compress | `cargo build`, `git commit`, `npm install`, `rm` |
| `IneligibleSecuritySensitive` | Network/security boundary; must not compress | `curl`, `ssh`, `sudo`, `wget` |
| `Unknown` | Unrecognized command | — |

### RtkProjector (Phase 6 — Real Invocation)

`RtkProjector` implements the `CommandOutputProjector` trait with real RTK invocation:

- Returns `Unsupported` when RTK is disabled, unavailable, or command is ineligible
- Returns `Fallback` support level when RTK is available and command is eligible
- Selects invocation mode via `RtkCapabilities::invocation_mode()`: prefers `PostProcess`, falls back to `Wrapper`, defaults to `Disabled`
- Returns `ProjectionError::BackendUnavailable` when invocation mode is disabled or RTK fails
- The selector falls back to safe projection on error and records a warning
- Raw expansion handles are included for stdout/stderr

#### `RtkInvocationMode`

| Mode | Behavior |
|------|----------|
| `PostProcess` | Pipes captured stdout/stderr to RTK via stdin. 1 MiB input cap, configurable timeout. Returns `ExternalCompressed` / `Lossy`. |
| `Wrapper` | Runs `rtk <command>` for eligible read-only commands only. Same timeout/error handling. |
| `Disabled` | No invocation; returns `BackendUnavailable`. |

#### Projection Metadata

Projection results include:
- RTK version and binary path
- Invocation mode used
- Input/output byte counts
- Timeout configured
- Raw expansion handles for stdout/stderr
- Warnings when streams were merged, RTK failed, or mode was unsupported

### Selector Integration

`ProjectionSelector::with_rtk()` conditionally includes the RTK projector in the chain:

```
Raw → Native → RTK (if enabled) → ErrorRetention → Truncated
```

`ProjectionSelector::with_config()` reads `ShellOutputConfig` to build the appropriate selector, including RTK when enabled and available.

`ProjectionError::BackendUnavailable` is returned when a caller requests an external backend (like RTK) but discovery has not yet been probed.

### What's NOT in Phase 5/6

- TUI expansion panel (Phase 7)
- Full redaction pipeline (Phase 8) — the hook site is in place, but the redaction rules are not implemented yet
- Broad RTK coverage — Phase 6 is intentionally conservative, covering low-risk read-only commands only

## Current Projection Pipeline Status

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 1 | **Landed** | `CommandOutputStore`, `ShellCommandRunBridge`, stable handles, bounded retention |
| Phase 2 | **Landed** | `CommandOutputProjector` trait, `RawProjector`/`TruncatedProjector`/`ErrorRetentionProjector`, `ProjectionSelector`, redaction hook placeholder |
| Phase 3 | **Landed** | Native structured projectors: `GitStatusProjector`, `GitDiffProjector`, `GitLogProjector`, `CargoCheckProjector`, `CargoTestProjector` |
| Phase 4 | **Partial** | Config schema and `ProjectionSelector::with_config()` present; per-command rules and escape hatches deferred |
| Phase 5 | **Landed** | RTK discovery, eligibility classification, `RtkCapabilities`, `RtkProjector` skeleton |
| Phase 6 | **Landed** | Real RTK invocation: `RtkInvocationMode` (PostProcess/Wrapper/Disabled), capability-driven dispatch, input capping, timeout enforcement, projection metadata |
| Phase 7+ | **Pending** | TUI expansion panel, full redaction pipeline (Phase 8) |
