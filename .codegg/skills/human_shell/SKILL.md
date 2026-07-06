---
name: human-shell
description: Human-initiated shell command execution, ephemeral transcript storage, projection pipeline (Phases 1-10), policy evaluation, and structured failure digest extraction
version: 1.6.0
tags:
  - shell
  - bash
  - command
  - ephemeral
  - projection
  - projector
  - command-event
  - output-handles
---

# Human Shell Module Guide

This skill covers `src/shell/` — codegg's human-initiated shell execution
path. It owns the human-shell ephemeral transcript, the policy
gatekeeper that blocks destructive commands, the structured failure
digest, and the Phase 1+2 command-event projection pipeline that
becomes the substrate for later expansion, redaction, and TUI
features. Phase 8 adds the deterministic redaction pipeline. Phase 9 adds the evaluation harness. Phase 10 integrates projection with context budgeting and compaction.

A detailed architecture document lives at `architecture/human_shell.md`.
The roadmap is `plans/shell_output_projection_rtk_roadmap.md`.

## Current Projection Pipeline Status

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 1 | **Landed** | `CommandOutputStore`, `ShellCommandRunBridge`, stable handles, bounded retention |
| Phase 2 | **Landed** | `CommandOutputProjector` trait, `RawProjector`/`TruncatedProjector`/`ErrorRetentionProjector`, `ProjectionSelector`, redaction hook placeholder |
| Phase 3 | **Landed** | Native structured projectors: `GitStatusProjector`, `GitDiffProjector`, `GitLogProjector`, `CargoCheckProjector`, `CargoTestProjector` |
| Phase 4 | **Partial** | Config schema and `ProjectionSelector::with_config()` present; per-command rules and escape hatches deferred |
| Phase 5 | **Landed** | RTK discovery, eligibility classification, `RtkCapabilities`, `RtkProjector` skeleton |
| Phase 6 | **Landed** | Real RTK invocation: `RtkInvocationMode` (PostProcess/Wrapper/Disabled), capability-driven dispatch, input capping, timeout enforcement, projection metadata |
| Phase 7 | **Landed** | Expansion API (`CommandOutputExpansion`, `ExpansionExactness`, `ExpansionRequest`), `/shell-expand` command, TUI detail panel with projection metadata |
| Phase 8 | **Landed** | Redaction pipeline: `Redactor` with six `RedactRule` implementations, `apply_redaction_hook` entry point, `RedactionState::Applied { replacements }` |
| Phase 9 | **Landed** | Evaluation harness: `tests/shell_projection_harness.rs` (11 invariant tests), fixture corpus in `tests/fixtures/shell_projection/` |
| Phase 10 | **Landed** | Context budget and compaction integration: `ProjectionContextMetadata`, `ProjectionFact`, `ModelTier`, `ContextAwareBudget`, double-compression prevention, critical fact extraction |

## Current Behavior

The projection pipeline is fully landed (Phases 1–10). Key operational facts:

- **Selector chain**: `Raw → Native (GitStatus, GitDiff, GitLog, CargoCheck, CargoTest) → RTK (if enabled) → ErrorRetention → Truncated`. Each projector implements `CommandOutputProjector::supports()`.
- **RTK wrapper mode strict grammar**: When `argv` is not available, wrapper mode uses `parse_simple_wrapper_command()` which rejects shell metacharacters, quotes, pipes, redirects, env assignments, and command substitution. Complex commands without `argv` return `ProjectionError::BackendUnavailable` and the selector falls back to safe projection.
- **Structured raw semantics**: `ProjectionRawSemantics` on every `ProjectionResult` distinguishes `OriginalCommandRaw` (native/post-process), `WrappedCommandRaw` (RTK wrapper, non-partial), `OriginalRawUnavailable` (RTK wrapper, partial), and `Unknown`. This replaces warning-string-based raw-handle truthfulness.
- **Redaction**: Six deterministic rules in `src/shell/redactor.rs`, called inside `ProjectionSelector::project`.
- **Expansion**: `/shell-expand` resolves raw output by handle. Shell detail dialog shows projection metadata and `e` keybinding.
- **RTK integration tests**: Env-gated via `CODEGG_RTK_INTEGRATION=1`; not part of standard CI.

## Central Invariant

A human `!` command is **not** model context unless the user explicitly
promotes it. `!command` runs and hides the output (ephemeral); `!!command`
runs and auto-promotes. `/shell-include` and `/shell-ask` promote a
prior command's output on demand.

## Submodules

| Submodule | Public types | Purpose |
|-----------|--------------|---------|
| `shell::types` | `ShellOrigin`, `ShellCapturePolicy`, `ShellCommandId`, `ShellRequest`, `ShellEvent`, `ShellStatus`, `ShellPromotionMode`, `ShellEnvPolicy`, `PromptSubmissionKind`, `classify_prompt_submission()` | Core data model for shell requests, events, and prompt classification |
| `shell::runtime` | `ShellRuntime`, `ShellHandle` | Async process spawner; uses `$SHELL -lc`; sends `ShellEvent`s over `mpsc` |
| `shell::store` | `ShellOutputStore`, `BoundedOutput`, `ShellOutputEntry` | Ephemeral TUI transcript store with bounded head/tail per stream |
| `shell::policy` | `HumanShellPolicyDecision`, `evaluate_command()` | Block / warn / allow for destructive or risky commands |
| `shell::digest` | `ShellDigest`, `ShellFailure`, `ShellFailureKind`, `TruncationReport` | Structured failure extraction from captured output |
| `shell::projection` (Phase 1) | `CommandRun`, `CommandRunId`, `CommandExit`, `CommandOutputStore`, `OutputHandle`, `OutputStreamKind`, `default_command_projection` | Durable command-event model; raw stdout/stderr retention out-of-band from model context |
| `shell::projection_bridge` (Phase 1) | `ShellCommandRunBridge` | Sidecar that mirrors `ShellEvent`s into `CommandOutputStore` |
| `shell::projector` (Phase 2) | `CommandOutputProjector`, `ProjectionRequest`, `ProjectionResult`, `ProjectionKind`, `ProjectionExactness`, `OmittedRange`, `ExpansionHandle`, `ProjectionSupport`, `ProjectionTarget`, `ProjectionBudget`, `ProjectionPolicy`, `ProjectionError`, `ProjectionSelector`, `RawProjector`, `TruncatedProjector`, `ErrorRetentionProjector`, `apply_redaction_hook` | Projector trait + built-in projectors + selector + redaction hook site |
| `shell::projector` (Phase 3) | `GitStatusProjector`, `GitDiffProjector`, `GitLogProjector`, `CargoCheckProjector`, `CargoTestProjector` | Native structured projectors for Git and Rust toolchains |
| `shell::rtk` | `RtkDiscovery`, `RtkAvailability`, `RtkState`, `RtkCapabilities`, `CapabilityState`, `CompressionEligibility`, `RtkProjector`, `classify_command()` | RTK discovery, capability probing, eligibility classification, and RTK projector skeleton |

## What Phase 1 Adds

Phase 1 introduces a parallel command-event system that runs alongside
the existing `ShellOutputStore`. Both stores are populated by the same
`ShellEvent` stream — the legacy store keeps lossy head/tail previews
for the TUI; the new `CommandOutputStore` keeps raw stdout/stderr for
the projection pipeline.

| Store | Purpose | Retention |
|-------|---------|-----------|
| `ShellOutputStore` (existing) | TUI transcript, digests, `/shell-include` promotion | 1 MB per command, 8 MB total, head + tail only |
| `CommandOutputStore` (Phase 1) | Projection pipeline substrate, expansion handles | 32 MiB per stream, 64 MiB total, full prefix |

### Phase 1 Boundaries

- **Stable command IDs**: `CommandRunId` allocated via `CommandOutputStore::alloc_id()`; matches `ShellCommandId` by `.0` value.
- **Stable handle URLs**: `cmd://<id>/<stream>` for `stdout` / `stderr` / `combined`.
- **Bounded retention**: per-stream and total caps prevent unbounded memory growth.
- **Single projection seam**: `default_command_projection(run, store)` is the only path that produces model-visible command text. Phase 1 made it the seam; Phase 2 routed it through the projector trait while keeping the function signature stable.

### Phase 1 Non-Goals

- Real projector selection (Phase 2) — landed
- Native structured projectors (Phase 3) — landed
- Projection policy config (Phase 4)
- RTK backend (Phase 5) — landed
- TUI expansion UI (Phase 7)
- Redaction pipeline (Phase 8) — redaction hook site present

## What Phase 2 Adds

Phase 2 introduces the projection abstraction that converts raw command artifacts into explicit model-facing and TUI-facing views. Every model-visible command output now flows through a single selector that picks the right projector for the request.

### Built-in Projectors

| Projector | `name()` | Selects when | Output shape |
|-----------|----------|--------------|--------------|
| `RawProjector` | `raw` | Total retained output ≤ budget, or caller asked for exact | Command header + raw stdout/stderr text + raw handles. Marks `PartialRawArtifact` when the underlying store is itself partial. |
| `ErrorRetentionProjector` | `error-retention` | Command failed (non-zero exit / timeout / cancellation / spawn failure) | Command header + only lines matching Rust/Python/JS/generic error patterns + bounded context. Falls back to head/tail when no patterns match. Marks `Lossy` exactness. |
| `TruncatedProjector` | `truncated` | Long successful output, or the previous two declined | Command header + bounded head + explicit omission marker + bounded tail. Stderr is always shown in full when it fits. |

The selector (`ProjectionSelector::with_defaults()`) tries projectors in priority order `raw → error-retention → truncated` and picks the first one whose `supports()` returns `Preferred` (or, failing that, `Supported` or `Fallback`).

### Result Metadata

`ProjectionResult` is more than text. It carries:

- `projector` — stable projector name (e.g. `"raw"`, `"error-retention"`, `"truncated"`)
- `kind` — `ProjectionKind` enum (Raw / Truncated / ErrorRetention / Structured / ExternalCompressed / Summary)
- `exactness` — `ProjectionExactness` enum (Exact / ExactRange / Truncated / Lossy / Parsed / PartialRawArtifact)
- `redaction` — `RedactionState` (NotApplied / HookAppliedNoRules / Applied / Skipped)
- `omitted` — every `OmittedRange` (stream, byte range, line range, total retained bytes, note)
- `expansion_handles` — `ExpansionHandle` values the consumer can use to fetch the omitted bytes
- `input_bytes` / `output_bytes` / token estimates / warnings

`ProjectionResult::banner(run)` renders a compact metadata line that prefixes the text and tells the model the projector, exactness, duration, and redaction state.

### Redaction Hook

`apply_redaction_hook(result, target)` is invoked for `ModelContext` and `ToolExpansion` targets when the policy allows it. Phase 2 ships a no-op placeholder that sets `RedactionState::HookAppliedNoRules` (not `Applied`) to make it clear no actual secret filtering occurs; Phase 8 will replace the body with a real implementation. The call site lives in `ProjectionSelector::project` so future redaction cannot be bypassed by RTK or native projectors. `RedactionState` has four variants: `NotApplied`, `HookAppliedNoRules`, `Applied` (Phase 8+), `Skipped`.

Test coverage includes false-positive checks (compiler diagnostics, prose with sensitive words), long-line and multiple-credentials-per-line edge cases, PEM certificate vs private key disambiguation, and embedded credential URLs with ports.

### Expansion Handles

`ExpansionHandle::as_url()` extends the existing `cmd://<id>/<stream>` URL form with an optional byte range fragment:

```text
cmd://42/stdout               # full stdout
cmd://42/stderr#0-1024        # first KiB of stderr
```

These are exactly the handles surfaced in the projection text and embedded in `ProjectionResult::expansion_handles`.

Round-trip tests verify that `OutputHandle::as_url()` URLs parse back to identical handles, and that expansion via `expand_stream` returns correct bytes for full streams, bounded ranges, and out-of-range clamping.

### Phase 2 Non-Goals

- Native structured projectors (Phase 3) — landed
- Configuration schema for projection policy (Phase 4)
- RTK backend (Phase 5) — landed
- TUI expansion panel (Phase 7)
- Full redaction pipeline (Phase 8) — only the call site is in place

## What Phase 3 Adds

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

## What Phase 5 + 6 Adds

Phase 5 adds RTK as an optional, detected command-output compressor backend
without making it a hard dependency or default execution path. Phase 6
replaces the skeleton with real invocation logic.

### RTK Discovery

`RtkDiscovery` handles lazy detection of the RTK binary:

- Probes on first use (not at startup)
- Resolves configured path or searches `$PATH`
- Runs `rtk --version` with configurable timeout
- Caches availability state

| State | Meaning |
|-------|---------|
| `Disabled` | Config has RTK disabled |
| `Available` | RTK found and working |
| `NotFound` | Binary not on PATH |
| `Broken` | Found but version probe failed |
| `TimedOut` | Version probe exceeded timeout |
| `UnsupportedVersion` | Incompatible version |

### Capability Probing

`RtkCapabilities` tracks confirmed behavior:

| Capability | States |
|------------|--------|
| `preserves_exit_code` | Yes / No / Unknown |
| `preserves_stderr` | Yes / No / Unknown |
| `supports_post_process` | Yes / No / Unknown |
| `supports_wrapper_mode` | Yes / No / Unknown |
| `utf8_output` | Yes / No / Unknown |

`probe_capabilities()` probes both PostProcess mode (piping data via stdin) and Wrapper mode (`rtk echo hello`) to determine which invocation modes are supported. Results include structured `RtkCapabilityDiagnostics` per probe.

### Structured Probe Diagnostics

| Type | Purpose |
|------|---------|
| `ProbeOutcome` | `Confirmed`, `Denied`, `Failed`, `Skipped` — per-probe result |
| `ProbeDiagnostic` | `name`, `command_shape`, `timeout_ms`, `outcome`, `output_bytes`, `reason` |
| `RtkCapabilityDiagnostics` | 4 probe diagnostic fields (exit_code, post_process, wrapper, stderr) |

The PostProcess probe includes a **help-text detection heuristic** that catches RTK versions lacking stdin support (e.g., RTK v0.43.0 prints help + exit 2).

### User-Facing RTK Status

`RtkStatusSummary` provides a multi-line status display via `RtkDiscovery::status_summary()` or `RtkProjector::status_summary()`. Shows config state, binary path, version, selected invocation mode, and per-capability results.

### Invocation Mode (Phase 6)

`RtkInvocationMode` enum selects how RTK processes output:

| Mode | Behavior |
|------|----------|
| `PostProcess` | Pipes captured stdout/stderr to RTK via stdin. 1 MiB input cap, configurable timeout. |
| `Wrapper` | Runs `rtk <command>` for eligible read-only commands only. Uses `argv` when available to avoid shell re-parsing. |
| `Disabled` | No invocation; returns `BackendUnavailable`. |

`RtkCapabilities::invocation_mode()` prefers PostProcess, falls back to Wrapper, defaults to Disabled.

### Eligibility Classification

`classify_command()` classifies commands into:

| Category | Example commands |
|----------|-----------------|
| `EligibleReadOnly` | `git status`, `git diff`, `git log`, `rg`, `ls`, `find`, `cat` |
| `EligibleWithRawCapture` | (reserved for future use) |
| `IneligibleSideEffecting` | `cargo build`, `git commit`, `npm install`, `rm` |
| `IneligibleSecuritySensitive` | `curl`, `ssh`, `sudo`, `wget` |
| `Unknown` | Unrecognized commands |

### RtkProjector (Phase 6)

`RtkProjector` implements the `CommandOutputProjector` trait with real invocation:

- Returns `Unsupported` when RTK is disabled, unavailable, or command is ineligible
- Returns `Fallback` support level when RTK is available and command is eligible
- Dispatches to `project_post_process()` or `project_wrapper()` based on `invocation_mode()`
- Returns `ProjectionError::BackendUnavailable` when invocation fails or mode is disabled
- The selector falls back to safe projection on error and records a warning
- Raw expansion handles are included for stdout/stderr

### Selector Integration

`ProjectionSelector::with_rtk()` conditionally includes the RTK projector:

```
Raw → Native → RTK (if enabled) → ErrorRetention → Truncated
```

`ProjectionSelector::with_config()` reads `ShellOutputConfig` to build the appropriate selector.

### Strict Wrapper Grammar (WS3 polish)

When `argv` is unavailable, wrapper mode uses `parse_simple_wrapper_command()` — a strict grammar that rejects shell metacharacters (`|`, `&&`, `||`, `;`, `` ` ``, `$(…)`, `>`, `<`, `&`), quoted strings, env assignments, and multi-command pipelines. Complex commands without `argv` return `ProjectionError::BackendUnavailable` and the selector falls back to safe projection. This prevents RTK from receiving commands it cannot safely interpret.

### Structured Raw Semantics (WS4 polish)

Every `ProjectionResult` carries a `raw_semantics` field (`ProjectionRawSemantics` enum):

| Variant | Set by |
|---------|--------|
| `OriginalCommandRaw` | PostProcess mode, native projectors |
| `WrappedCommandRaw` | RTK wrapper mode (non-partial) |
| `OriginalRawUnavailable` | RTK wrapper mode (partial) |
| `Unknown` | Default / fallback |

This makes raw-handle truthfulness structured rather than a warning string.

### Tests

62 unit tests covering:
- Discovery: disabled config, not-found state, available/broken/timed-out states, capability probing
- Capability diagnostics: help-text detection, Skipped/Confirmed/Denied outcomes, mode summary
- Status summary: disabled config display, not-found display
- Eligibility: read-only, side-effecting, security-sensitive, and unknown commands
- Projector: rejection when external backend disallowed, when RTK unavailable, when command ineligible
- Projector: acceptance for eligible commands
- Invocation: PostProcess and Wrapper mode dispatch, disabled mode fallback
- Skeleton: returns `BackendUnavailable` when invocation disabled
- Selector: falls back to safe projection on RTK error with warning

## What Phase 7 Adds

Phase 7 adds user-facing and model-tool-facing affordances for expanding
retained raw command output from projection handles.

### Expansion API

`CommandOutputStore::expand()` and `expand_stream()` are core expansion
operations in `src/shell/projection.rs`:

- `ExpansionRequest` — command_id, stream, optional byte_range
- `CommandOutputExpansion` — result with text, exactness, total_stream_bytes, returned_bytes, warnings
- `ExpansionExactness` — `Exact`, `LossyUtf8`, `Unavailable` (with `label()`)

Expansion handles eviction, missing streams, UTF-8 errors, and clamps
out-of-range byte offsets to available data.

### `/shell-expand` Command

New slash command: `/shell-expand <id|last> stdout|stderr [start..end]`

- Resolves shell ID via `resolve_shell_id()` (supports `"last"` or numeric ID)
- Parses optional byte range in `start..end` format
- Calls `app.command_run_store.expand_stream()` for raw expansion
- Displays result in `InfoDialog` with metadata and expanded output (truncated at 8192 chars)

### TUI Shell Detail Enhancement

`handle_shell_show()` dialog now includes projection metadata:

- Raw retention sizes, observed vs retained bytes, partiality
- Projector name, exactness, output size
- Omitted ranges with byte ranges
- Expansion handles as `cmd://` URLs
- Footer includes `e expand` keybinding

### Promotion Semantics

- `!command`: expansion remains local; model cannot read raw output
- `!!command`: projection can enter model context; raw handles accessible
- `/shell-include`: includes projection/expansion into model context
- `/shell-expand`: local TUI operation unless explicitly promoted

## Working Examples

### Allocating a command ID and inserting raw output

```rust
use crate::shell::projection::{CommandOutputStore, CommandRunId};
use std::time::SystemTime;
use std::path::PathBuf;

let mut store = CommandOutputStore::new();
let id = store.alloc_id();
let run = store.insert(
    id,
    "cargo test".to_string(),
    PathBuf::from("/tmp"),
    SystemTime::now(),
    b"test result: ok".to_vec(),
    Vec::new(),
);
assert_eq!(run.command, "cargo test");
assert!(run.stdout_handle().is_some());
```

### Resolving an expansion handle

```rust
use crate::shell::projection::{CommandOutputStream, OutputHandle};

let handle = OutputHandle::new(id, CommandOutputStream::Stdout);
let url = handle.as_url(); // "cmd://1/stdout"
let resolved = store.parse_handle(&url).unwrap();
assert_eq!(store.get_stream(resolved).unwrap(), b"test result: ok");
```

### Producing model-visible projection text

```rust
use crate::shell::projection::default_command_projection;

let run = store.get_run(id).unwrap();
let text = default_command_projection(run, &store);
// Includes command id, command string, cwd, exit label, duration,
// truncated stdout/stderr, and raw retention handles.
```

### Bridging a `ShellEvent` stream into the projection store

```rust
use crate::shell::projection_bridge::ShellCommandRunBridge;
use crate::shell::ShellEvent;

let mut bridge = ShellCommandRunBridge::new();
bridge.observe(&mut store, &ShellEvent::Started { id: shell_id(1), command: "echo hi".into(), cwd: PathBuf::from("/tmp") });
bridge.observe(&mut store, &ShellEvent::Stdout { id: shell_id(1), bytes: b"hi\n".to_vec() });
bridge.observe(&mut store, &ShellEvent::Exited { id: shell_id(1), status: Some(0), elapsed: Duration::from_millis(20) });
// store now has a CommandRun with stdout="hi\n", exit=Code(0), duration=20ms.
```

### Running a projection through the Phase 2 selector

```rust
use crate::shell::projector::{
    ProjectionPolicy, ProjectionRequest, ProjectionSelector, ProjectionTarget,
};

let policy = ProjectionPolicy::conservative();
let run = store.get_run(id).unwrap();
let request = ProjectionRequest::for_target(run, ProjectionTarget::ModelContext, &policy);
let selector = ProjectionSelector::with_defaults();
let result = selector.project(request, &store);
// result.projector, result.kind, result.exactness, result.omitted,
// result.expansion_handles, result.warnings all carry provenance.
let text = result.text; // also obtainable via default_command_projection(run, &store).
```

### Building an `ExpansionHandle` for a byte range

```rust
use crate::shell::projector::ExpansionHandle;
use crate::shell::projection::{CommandOutputStream, CommandRunId};

let handle = ExpansionHandle {
    command_id: CommandRunId(42),
    stream: CommandOutputStream::Stderr,
    byte_range: Some(0..1024),
};
assert_eq!(handle.as_url(), "cmd://42/stderr#0-1024");
```

## Integration Points

- `src/tui/app/mod.rs` carries `shell_store`, `command_run_store`, and `command_run_bridge` as fields on `App`.
- `src/tui/commands/shell.rs::handle_shell_event` mirrors every `ShellEvent` into both stores before performing the legacy store update.
- `classify_prompt_submission` is called from `send_prompt` to dispatch `!cmd` and `!!cmd` to `TuiCommand::RunHumanShell`.

## Boundaries and Caveats

- **Phase 1 does not synthesize combined output.** `get_stream` returns `None` for `CommandOutputStream::Combined` unless the execution layer supplies it explicitly. Downstream code must label any synthesized combined output.
- **Stream caps mark `Partial`.** Code that consumes a `RawStream` MUST check `OutputCompleteness` and surface the partial state to the user/model rather than silently truncating.
- **Bridge is additive.** It does NOT modify the existing `ShellOutputStore`, the `ShellEvent` enum, or `ShellRuntime`. Removing or altering those would break Phase 1.
- **Built-in projectors are conservative.** `RawProjector`, `TruncatedProjector`, and `ErrorRetentionProjector` do not parse command shape, do not invoke RTK, and do not produce model-generated summaries. Native structured projectors (Phase 3) and the RTK backend (Phase 5+6) plug into the same selector without changing the public API.
- **Redaction hook is a placeholder.** `apply_redaction_hook` sets `RedactionState::HookAppliedNoRules` for `ModelContext` and `ToolExpansion` targets but does not actually rewrite any text. Phase 8 will replace the body; the call site in `ProjectionSelector::project` is the contract.
- **`ProjectionResult` owns the metadata.** The model-facing text is the `text` field; consumers MUST also surface `projector`, `kind`, `exactness`, `redaction`, and `omitted` (or the rendered banner) so the model knows what it is looking at.

## Validation

```bash
# Standard validation
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
scripts/check-core-boundary.sh

# Shell projection tests (also run explicitly in CI)
cargo test --test shell_projection_harness         # 11 invariant tests (fixture corpus)
cargo test --test shell_projection_phase10         # 33 context budget/compaction tests
cargo test -p codegg --lib shell::redactor         # 33 redactor unit tests
cargo test -p codegg --lib shell::rtk              # 62 RTK unit tests (no binary required)

# Optional RTK integration (requires rtk installed, env-gated)
CODEGG_RTK_INTEGRATION=1 cargo test --all-features rtk_integration
```