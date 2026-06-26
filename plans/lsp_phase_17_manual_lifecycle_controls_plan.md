# LSP Phase 17 Plan: Manual Lifecycle Controls for Start and Replay

Status date: 2026-06-26
Phase type: lifecycle control / service ergonomics / optional user control
Prerequisites: Phase 13 validation evidence and Phase 15 diagnostics. Do not start unless real-world usage shows the current auto-start/replay behavior is insufficient.

## Purpose

Phase 17 should add manual LSP lifecycle controls only where they are clearly useful, scoped, and safe. Phases 9-12 deliberately deferred `/lsp-start` and `/lsp-replay-docs` because the service auto-starts on demand and handles document replay internally, and because there was no clean scoped API for these commands.

This phase should not add manual controls merely for symmetry. It should be justified by Phase 13 evidence: repeated user confusion, failed auto-start scenarios, stale replay issues, or real-world servers needing explicit lifecycle intervention.

## Current baseline

The repo already has:

- automatic server start/on-demand behavior,
- restart and stop commands,
- server health states,
- generation tracking,
- stale evidence labeling,
- root diagnosis,
- capability summaries,
- document sync/replay internals,
- `/lsp-doctor` planned in Phase 13.

## Non-goals

Do not add manual lifecycle commands unless service APIs can scope them cleanly.

Do not create a parallel lifecycle manager outside the existing service/supervisor model.

Do not make `/lsp-start` silently start arbitrary servers outside allowed root.

Do not replay documents across unrelated roots or generations.

Do not hide generation changes from context freshness.

Do not auto-apply previews or execute server commands.

## Workstream 1: evidence gate

### Problem

Manual controls add complexity. Require evidence before implementation.

### Evidence sources

Use Phase 13 `/lsp-doctor` and smoke-test findings to decide whether manual controls are needed.

Acceptable triggers:

- auto-start fails silently in common cases,
- users need to pre-warm LSP before a long agent turn,
- servers fail to replay open docs after restart,
- per-key stop-all fallback is too coarse in multi-root sessions,
- root/profile selection needs explicit user override,
- real server smoke tests show lifecycle race conditions.

### Acceptance criteria

- A short decision note explains which commands are justified.
- If no evidence justifies implementation, Phase 17 may close as deferred/no-op.

## Workstream 2: scoped `/lsp-start`

### Command shape

Proposed:

```text
/lsp-start <path>
/lsp-start --profile <profile> <root>
```

Only implement the profile form if profiles are already explicit in the service API.

### Required behavior

- validate path against allowed root,
- diagnose selected root/profile before start,
- start only one scoped server if possible,
- report server key, root, profile, PID/process state if available,
- report generation,
- do not block the TUI for full indexing,
- show progress through `/lsp-status` or `/lsp-doctor`,
- handle already-running server idempotently.

### Target files

- `src/tui/command.rs`,
- `src/tui/app/mod.rs`,
- `src/tool/lsp.rs`,
- `crates/egglsp/src/service.rs`,
- `crates/egglsp/src/supervisor.rs`,
- `crates/egglsp/src/root.rs`,
- tests.

### Acceptance criteria

- `/lsp-start` is scoped to an allowed root/profile.
- It does not start arbitrary external processes.
- Already-running case is safe and clear.
- Failed startup reports actionable detail.

## Workstream 3: scoped `/lsp-replay-docs`

### Command shape

```text
/lsp-replay-docs <server-key>
/lsp-replay-docs --root <root>
```

### Required behavior

- validate server key/root,
- identify open documents associated with that server/root,
- replay only those documents,
- increment or preserve generation according to existing service semantics,
- label evidence stale/retained where appropriate,
- report number of documents replayed,
- report failures by file,
- avoid replaying documents outside allowed root.

### Target files

- document sync/replay module,
- service/supervisor module,
- `src/tool/lsp.rs`,
- TUI command code,
- tests.

### Acceptance criteria

- Replay is scoped and auditable.
- Replay failures do not mark stale evidence fresh.
- Users can see what was replayed.

## Workstream 4: per-key stop refinement

### Problem

The current per-key stop behavior may use a `shutdown_all` fallback. If real usage shows multi-root sessions need narrower control, refine this.

### Required behavior

- stop one server key without killing unrelated servers,
- preserve registry/cache state appropriately,
- mark evidence stale for the stopped server,
- report state as stopped,
- make restart possible after stop.

### Acceptance criteria

- Per-key stop no longer requires stop-all fallback if service APIs support it.
- Multi-root behavior is tested.

## Workstream 5: lifecycle races and generation semantics

### Problem

Manual start/replay/stop can race with auto-start, restart, and context collection.

### Required tests

- start while already starting,
- start while ready,
- start invalid root,
- replay while restarting,
- replay invalid server key,
- stop during pending request,
- restart after stop,
- context collected across generation change is stale or retained, not fresh,
- preview registry survives or is invalidated according to documented semantics,
- cache entries are evicted or bypassed after generation change.

### Acceptance criteria

- Generation changes remain visible.
- Stale evidence semantics remain correct.
- No command panics on lifecycle races.

## Workstream 6: TUI and agent-facing messaging

### Messaging principles

- Lifecycle commands should return immediate status, not pretend indexing is complete.
- Messages should include server key/root/profile/generation.
- Errors should suggest `/lsp-doctor <path>` where appropriate.
- Manual controls should not be suggested unless they are supported for the current profile.

### Acceptance criteria

- Users understand whether the server is starting, ready, degraded, failed, or stopped.
- Commands suggest the next diagnostic action.

## Workstream 7: docs

Update:

- `architecture/lsp.md`,
- `.opencode/skills/lsp/SKILL.md`,
- `README.md` command list if public,
- `AGENTS.md`,
- roadmap status docs.

Document:

- whether `/lsp-start` and `/lsp-replay-docs` are implemented or deferred,
- command syntax,
- scoping and allowed-root behavior,
- generation/stale semantics,
- lifecycle race behavior,
- when to prefer `/lsp-doctor`.

## Test matrix

Focused:

```bash
cargo fmt --check
cargo test -p egglsp health
cargo test -p egglsp root
cargo test -p egglsp tui_summary
cargo test --test phase5_context_integration lsp
```

If lifecycle service tests exist:

```bash
cargo test -p egglsp service
cargo test -p egglsp restart
cargo test -p egglsp document_sync
```

Broader:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Final acceptance criteria

Phase 17 is complete when one of these is true:

1. Evidence shows manual controls are not needed yet, and the phase closes as explicitly deferred with a decision note.
2. `/lsp-start` and/or `/lsp-replay-docs` are implemented with clean scoped APIs, tests, docs, generation/stale semantics, and race handling.

In either case, the LSP service must retain one coherent lifecycle model. Do not leave behind a second, partially overlapping manual lifecycle path.
