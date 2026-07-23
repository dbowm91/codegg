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

## Syntax

| Input | Meaning |
|-------|---------|
| `!command` | Run command, store output ephemerally (model never sees it) |
| `!!command` | Run command, auto-promote output into conversation |
| `/shell-list` | Show recent shell commands with status |
| `/shell-include <id> [stdout\|stderr\|all]` | Promote a stored command's output into context |
| `/shell-rerun <id>` | Re-execute a previous command |
| `/shell-kill <id>` | Abort a running command |

## How It Works

1. User types `!cargo test` in the prompt
2. `classify_prompt_submission()` detects the `!` prefix, returns `PromptSubmissionKind::HumanShell { command: "cargo test", promote_after: false }`
3. TUI dispatches `TuiCommand::RunHumanShell`
4. `ShellRuntime::spawn()` creates a child process via `$SHELL -lc "cargo test"`
5. stdout/stderr chunks stream as `ShellEvent::Stdout`/`ShellEvent::Stderr`
6. `ShellOutputStore` captures output in `BoundedOutput` (head 256KB + tail 256KB)
7. `ShellCell` renders in the message timeline with status, elapsed, exit code
8. `/shell-list` displays recent commands in format: `[id] done exit=N X.Xs $ command`
9. Output is NOT added to the model's context

## Promotion Model

- `!cmd` → `ShellOrigin::HumanEphemeral`, `ShellCapturePolicy::StoreEphemeral`
- `!!cmd` → `ShellOrigin::HumanPromoted`, `ShellCapturePolicy::StoreAndPromote`
- `/shell-include <id>` → Promotes an existing ephemeral entry into context

## Safety Policy

`evaluate_command()` in `policy.rs` blocks or warns on dangerous patterns:

**Blocked** (refused before execution):
- `rm -rf /` and variants
- `mkfs.*`, `dd if=/dev/*`
- Fork bombs
- `shutdown`, `reboot`, `halt`, `poweroff`

**Warned** (confirmation dialog if `confirm_dangerous` is enabled):
- `rm -rf .` (current directory)
- `git clean -f`
- `sudo`
- `curl|sh`, `curl|bash`, `wget|sh`
- `chmod -R 777`, `chmod -R a+rwx`
- `chown -R`

## Bounded Storage

`ShellOutputStore` enforces limits:
- **Per command**: 1MB (256KB head + 256KB middle dropped + 256KB tail)
- **Total**: 8MB across all commands
- **History**: 100 entries max
- Eviction: oldest entries removed first

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
    "confirm_dangerous": true,
    "auto_promote_bangbang": true
  }
}
```

## Key Types

- `ShellOrigin` — Who initiated: `HumanEphemeral`, `HumanPromoted`, `AgentTool`
- `ShellCapturePolicy` — What to store: `DisplayOnly`, `StoreEphemeral`, `StoreAndPromote`
- `ShellCommandId` — Newtype `u64`, monotonically allocated
- `ShellEvent` — Stream events: `Started`, `Stdout`, `Stderr`, `Exited`, `TimedOut`, `FailedToStart`
- `ShellRuntime` — Spawns child processes via `$SHELL -lc`
- `ShellHandle` — Abort handle for killing running commands
- `BoundedOutput` — Head/tail split storage with omitted byte tracking
- `ShellOutputEntry` — Stores `exit_code: Option<i32>` alongside status, stdout, stderr, elapsed time
- `ShellDigest` — Structured failure extraction from output

## Relationship to Other Modules

- **tool::bash** — Agent bash tool uses `ShellOrigin::AgentTool`; separate from human shell
- **shell_session** — Metadata-only module for terminal sessions (no PTY, no execution)
- **tui** — Renders `MsgPart::ShellCell`, handles `/shell-*` commands via `TuiCommand` variants
