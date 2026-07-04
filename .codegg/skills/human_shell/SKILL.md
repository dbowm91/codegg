---
name: human-shell
description: Human-initiated shell command execution, ephemeral transcript storage, projection pipeline foundation (Phase 1), policy evaluation, and structured failure digest extraction
version: 1.0.0
tags:
  - shell
  - bash
  - command
  - ephemeral
  - projection
  - command-event
  - output-handles
---

# Human Shell Module Guide

This skill covers `src/shell/` — codegg's human-initiated shell execution
path. It owns the human-shell ephemeral transcript, the policy
gatekeeper that blocks destructive commands, the structured failure
digest, and the Phase 1 command-event projection model that becomes
the substrate for later projection, expansion, redaction, and TUI
features.

A detailed architecture document lives at `architecture/human_shell.md`.
The roadmap is `plans/shell_output_projection_rtk_roadmap.md`.

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
- **Single projection seam**: `default_command_projection(run, store)` is the only path that produces model-visible command text in Phase 1. Phase 2 will replace it with the real projector trait.

### Phase 1 Non-Goals

- Real projector selection (Phase 2)
- Native structured projectors (Phase 3)
- Projection policy config (Phase 4)
- RTK backend (Phase 5)
- TUI expansion UI (Phase 7)
- Redaction pipeline (Phase 8)

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

## Integration Points

- `src/tui/app/mod.rs` carries `shell_store`, `command_run_store`, and `command_run_bridge` as fields on `App`.
- `src/tui/commands/shell.rs::handle_shell_event` mirrors every `ShellEvent` into both stores before performing the legacy store update.
- `classify_prompt_submission` is called from `send_prompt` to dispatch `!cmd` and `!!cmd` to `TuiCommand::RunHumanShell`.

## Boundaries and Caveats

- **Phase 1 does not synthesize combined output.** `get_stream` returns `None` for `CommandOutputStream::Combined` unless the execution layer supplies it explicitly. Downstream code must label any synthesized combined output.
- **Phase 1 placeholder projection is conservative.** It does not inspect command shape, parse structured output, or invoke any external backend. The metadata banner and raw handles are present, but compactness and selection are deferred to Phase 2+.
- **Stream caps mark `Partial`.** Code that consumes a `RawStream` MUST check `OutputCompleteness` and surface the partial state to the user/model rather than silently truncating.
- **Bridge is additive.** It does NOT modify the existing `ShellOutputStore`, the `ShellEvent` enum, or `ShellRuntime`. Removing or altering those would break Phase 1.