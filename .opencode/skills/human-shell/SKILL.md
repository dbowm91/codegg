---
name: human-shell
description: Human shell execution with promotion model, safety policy, and bounded output storage
version: 1.0.0
tags:
  - shell
  - human
  - execution
  - safety
---

# Human Shell Guide

This skill covers the human shell module for running shell commands outside the agent context.

## Overview

The human shell lets users run shell commands from the TUI prompt without the model seeing the output — unless explicitly promoted. This preserves context window space while giving users direct shell access.

**Location**: `src/shell/`

**Central invariant**: A human `!` command is not model context unless the user explicitly promotes it.

This is one-shot command execution through `$SHELL -lc` with managed stdout and
stderr capture, timeout, and cancellation. The request uses the managed
processes' default null stdin policy; this feature does not provide a
persistent interactive shell or PTY. Do not describe those terminal-session
capabilities as available unless a separate implementation lands.

## Syntax

| Input | Meaning |
|-------|---------|
| `!command` | Run command, store output ephemerally (model never sees it) |
| `!!command` | Run command, auto-promote output into conversation |
| `\!command` | Escape hatch — becomes literal chat text starting with `!`, never runs (`src/shell/types.rs:127`) |
| `/shell-list` | Show recent shell commands with status |
| `/shell-show <id\|last>` | Show stored output for a command |
| `/shell-include <id\|last> [--tail N\|--stdout\|--stderr\|--summary\|all]` | Promote a stored command's output into context |
| `/shell-expand <id\|last> stdout\|stderr [start..end]` | Expand a byte/line region of a stored output |
| `/shell-rerun <id\|last>` | Re-execute a previous command |
| `/shell-kill <id\|last>` | Abort a running command |

## How It Works

1. User types `!cargo test` in the prompt
2. `classify_prompt_submission()` detects the `!` prefix, returns `PromptSubmissionKind::HumanShell { command: "cargo test", promote_after: false }`
3. TUI dispatches `TuiCommand::RunHumanShell`
4. `ShellRuntime::spawn()` creates a child process via `$SHELL -lc "cargo test"`
5. stdout/stderr chunks stream as `ShellEvent::Stdout`/`ShellEvent::Stderr`
6. `ShellOutputStore` captures output in `BoundedOutput` (head 256KB + tail 256KB)
7. `ShellCell` renders in the message timeline with status, elapsed, exit code
8. `/shell-list` displays the last 10 commands as `[id] <status> $ command`,
   where `<status>` is `done exit=N X.Xs`, `running X.Xs`, `timeout Xs`,
   `killed X.Xs`, or `failed`, plus a ` [promoted]` marker once the entry has
   been promoted (`src/tui/commands/shell.rs:567-574`)
9. Output is NOT added to the model's context

## Promotion Model

- `!cmd` → `ShellCapturePolicy::StoreEphemeral`
- `!!cmd` → `ShellCapturePolicy::StoreAndPromote`
- `/shell-include <id>` → Promotes an existing ephemeral entry into context

**Both keep `origin: ShellOrigin::HumanEphemeral`**
(`src/tui/commands/shell.rs:66`). `promote_after` only selects the capture
policy — do not branch on origin to detect promotion. The
`ShellOrigin::HumanPromoted` variant is declared (`src/shell/types.rs:7`)
but is **never constructed** anywhere in `src/`, `crates/`, or `tests/`;
`ShellOutputEntry::promoted` is the field that records the actual promotion
(`src/shell/store.rs:90`).

## Safety Policy

`evaluate_command()` in `policy.rs` blocks or warns on dangerous patterns:

**Blocked** (refused before execution):
- `rm -rf /` and variants
- `mkfs.*`, `dd if=/dev/*`
- Fork bombs
- `shutdown`, `reboot`, `halt`, `poweroff`

**Warned** (confirmation dialog if `confirm_dangerous` is enabled —
gated at `src/tui/commands/shell.rs:28`):
- `rm -rf .` (current directory), `rm -rf ~` / `$HOME`
- `git clean -f...`
- `sudo`
- `curl|sh`, `curl|bash`, `wget|sh`, `wget|bash` (optional `sudo`)
- `sh <(curl ...)`, `bash <(wget ...)` — process substitution
- `chmod` with a numeric 000/4xx/777 mode, or `[ugoa]+rwx` adds
- `chown -R`
- `find ... -delete`, `find ... -exec rm`

Regexes live in `src/shell/policy.rs:12-56` and are matched against a
*normalized* command (`src/shell/policy.rs:72`): trimmed, lowercased, with
`--recursive`→`-r`, `--force`→`-f`, and quotes stripped. Long-form flags are
only caught because of that normalization.

## Bounded Storage

`ShellOutputStore` enforces limits:
- **Per command**: `DEFAULT_MAX_BYTES_PER_COMMAND = 1_000_000`
  (`src/shell/types.rs:105`)
- **Total**: `DEFAULT_MAX_TOTAL_BYTES = 8_000_000` across all commands
  (`src/shell/types.rs:106`)
- **History**: `DEFAULT_MAX_HISTORY_ENTRIES = 100` (`src/shell/types.rs:107`)
- Eviction: oldest entries removed first

`BoundedOutput` (`src/shell/store.rs:14`) keeps at most **512 KiB** of each
command's output regardless of that 1 MB budget: `HEAD_CAP = 256 * 1024` and
`TAIL_CAP = 256 * 1024` (`src/shell/store.rs:10-11`). Everything in between is
dropped and counted in `omitted_bytes`; `total_bytes` still reports the full
streamed length. The head/tail split is not a 3-way split of the budget.

## Digest Extraction

`ShellDigest::build()` extracts structured failure info:
- Rust compiler errors (`error[E0308]`)
- Warnings (`warning: unused variable`)
- Test failures (`test result: FAILED`)
- Panics (`thread 'main' panicked`)
- Non-zero exit codes

`ShellDigest::build_from_entry()` is a convenience constructor that takes a `&ShellOutputEntry` directly, extracting command, cwd, exit_code, elapsed, stdout, and stderr from the entry.

## Configuration

```json
{
  "human_shell": {
    "enabled": true,
    "default_timeout_secs": 300,
    "max_history_entries": 100,
    "max_bytes_per_command": 1000000,
    "max_total_bytes": 8000000,
    "ansi": "sgr-only",
    "confirm_dangerous": true,
    "auto_promote_bangbang": true
  }
}
```

`ansi` controls ANSI escape handling in captured output (`AnsiMode`: `sgr-only`
(default), `strip`, `raw` — `crates/codegg-config/src/schema.rs:2936`).
`default_timeout_secs` defaults to 300 (`DEFAULT_TIMEOUT_SECS`,
`src/shell/types.rs:104`) and is capped at 1 hour by config validation.

`auto_promote_bangbang` is currently **inert**: it is parsed and defaults to
`true` (`crates/codegg-config/src/schema.rs:2995`), but nothing reads it.
`!!` promotion is unconditional — `promote_after` is fixed by
`classify_prompt_submission` at parse time and threaded straight to
`spawn_human_shell`. Do not assume setting it to `false` disables `!!`
promotion; wire it up in `src/tui/app/prompt_turn.rs` first.

## Command Routing (agent bash, not human shell)

Separate from the `!`/`!!` path, the *agent's* bash tool classifies commands
by family. `RouteLevel` defaults to `Observe`
(`crates/codegg-config/src/schema.rs:3483`) — classification and metadata
only, with every command still executing via raw shell. Raising a family to
`Active` is what routes execution to structured backends. Setting
`CODEGG_ROUTING_DISABLE=1` is the emergency kill switch that disables routing
for all families (`src/tool/bash/policy.rs:322`); per-family `Off` config
disables one family.

## Key Types

- `ShellOrigin` — Declared: `HumanEphemeral`, `HumanPromoted`, `AgentTool`.
  Only `HumanEphemeral` is ever constructed
- `ShellCapturePolicy` — What to store: `DisplayOnly`, `StoreEphemeral`,
  `StoreAndPromote`. Only the latter two are ever constructed
- `ShellCommandId` — Newtype `u64`, monotonically allocated (`src/shell/types.rs:52`)
- `ShellEvent` — Stream events: `Started`, `Stdout`, `Stderr`, `Exited`, `TimedOut`, `FailedToStart`
- `ShellRuntime` — Spawns child processes via `$SHELL -lc` (falls back to `sh`; `src/shell/runtime.rs:16,108`)
- `ShellHandle` — Abort handle for killing running commands (`CancellationToken` + task `AbortHandle`)
- `BoundedOutput` — Head/tail split storage with omitted byte tracking
- `ShellOutputEntry` — Stores `exit_code: Option<i32>` alongside status, stdout, stderr, elapsed time, plus `promoted`/`promote_after`/`capture_policy` (`src/shell/store.rs:79`)
- `ShellDigest` — Structured failure extraction from output

## Relationship to Other Modules

- **tool::bash** — Fully separate: the agent bash tool (`src/tool/bash.rs`)
  never references `crate::shell` types. `ShellOrigin::AgentTool` is
  declared (`src/shell/types.rs:8`) but never constructed
- **tui** — Renders `MsgPart::ShellCell`, handles `/shell-*` commands via `TuiCommand` variants

## See Also

- `architecture/human_shell.md` — full module contract (10-phase projection pipeline: `projection.rs`, `projector.rs`, `redactor.rs`, `rtk.rs`, `projection_bridge.rs`)
- `.opencode/skills/tui/SKILL.md` — TUI command registration and async dispatch rules

## Source verification

Re-verified 2026-10-06 against `src/shell/{store,types,policy,runtime,digest}.rs`,
`src/tui/commands/shell.rs`, `src/tui/app/mod.rs`, `crates/codegg-config/src/schema.rs`,
and `src/tool/bash/policy.rs`. Corrected the promotion model — `!!` keeps
`ShellOrigin::HumanEphemeral` and only switches `capture_policy`;
`HumanPromoted`, `AgentTool`, and `DisplayOnly` are declared but never
constructed. Replaced the `/shell-include` flag list with the real usage
string, added the undocumented `/shell-expand` command and the `\!` escape
hatch, completed the warn-pattern list, and flagged `auto_promote_bangbang`
as parsed-but-unread. Added the `RouteLevel::Observe` default and the
`CODEGG_ROUTING_DISABLE=1` kill switch. Claims without a traceable source
were removed rather than guessed. The earlier bounded-storage correction
stands: `BoundedOutput` retains 512 KiB of head+tail, not the full 1 MB
budget; the middle is dropped and counted in `omitted_bytes`, and eviction
is oldest-first via `VecDeque::pop_front` (`src/shell/store.rs:253-255`).
