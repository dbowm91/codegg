# TUI Incremental Improvement Roadmap

## Context

The current `codegg` TUI has a workable state/component split, but several reliability and responsiveness risks remain concentrated around the runtime boundary. The most important issues are not broad architectural failure; they are blocking work on the TUI event loop, synchronous filesystem diffing during event handling, fragile terminal setup/teardown, inconsistent debug logging, input/help drift, and correctness gaps in newer shell UI paths.

This roadmap intentionally avoids a rewrite. The goal is to improve the TUI in small, verifiable passes while preserving the current single-owner UI state model. Async work should move out of the render/input loop, and UI mutation should remain centralized through typed completion messages.

## Guiding Principles

1. The TUI event loop must remain hot. Keyboard input, mouse input, resize handling, streaming redraws, spinner animation, and toast expiry should continue even while core requests, filesystem reads, research loading, session loading, or diagnostics are slow.

2. UI state should be mutated on the TUI thread through typed events or commands. Background work may perform I/O, compute diffs, query the core, or load files, but it should return results through bounded channels and generation-aware completion messages.

3. Terminal lifecycle must be guarded. Entering alternate screen, enabling raw mode, bracketed paste, and mouse capture should either fully succeed or roll back partial setup. Exit should restore exactly once in reverse order.

4. Debugging should not write ad hoc files into user projects during normal execution. Diagnostics should use `tracing` and explicit user-configured sinks or feature-gated debug output.

5. Help text should reflect actual input semantics. Insert, normal, command, and dialog modes should not advertise shortcuts that are deliberately shadowed by text input.

6. Every phase must include tests or verification hooks. Widget unit tests are not enough; the roadmap should add fake-core responsiveness tests, render tests using `ratatui::backend::TestBackend`, and lifecycle tests around background tasks and terminal guards.

## Phase Sequence

### Phase 1: Event Loop Responsiveness Foundation

Move high-latency `TuiCommand` handlers off the event loop. Introduce a standard request/result pattern for async command work. The event loop should mark UI surfaces as loading, spawn the real work, and later consume a typed completion command. Start with session reloads, session message loading, tree loading, import preview/confirm, research browser loading, memory commands, doctor, and other core-backed operations.

Outcome: slow core requests no longer freeze input, resize, spinner, toasts, or streaming redraws.

### Phase 2: Async File Diff and Sidebar Update Pipeline

Move `sidebar_diff_stats` and changed-file diff computation out of `AppEvent::FileChanged`. Introduce a bounded diff worker with file size caps, binary detection, stale-result protection, and immediate pending-state sidebar updates.

Outcome: large files, generated files, lockfiles, or slow filesystems cannot stall the TUI while an agent edits files.

### Phase 3: Terminal Lifecycle and Render Recovery Hardening

Add a terminal lifecycle guard that tracks setup state and restores terminal features safely. Replace broad render reset behavior with component-level fallbacks where practical. Keep full app reset only as a final root-level recovery path.

Outcome: fewer corrupted-terminal exits, less state loss after render failures, and better behavior inside terminal multiplexers.

### Phase 4: Logging and TUI Diagnostics Cleanup

Remove or feature-gate unconditional debug file writes from `src/tui/mod.rs`. Route diagnostics through `tracing`. Add low-overhead diagnostics for event-loop stalls, render duration, command duration, dropped events, active dialog, and slow handlers. Optionally surface this through `/doctor tui` or `/tui-stats`.

Outcome: normal TUI use no longer pollutes project directories with debug logs, and developers get better structured evidence when the TUI feels sluggish.

### Phase 5: Input Mode and Help Text Consistency

Make help text mode-aware and reconcile advertised shortcuts with actual keybinding behavior. Insert mode should prioritize text input; normal mode can expose bare navigation/help keys. Generate or centralize help metadata from the binding tables where possible to reduce drift.

Outcome: users see accurate help and fewer shortcuts appear broken because mode semantics are implicit.

### Phase 6: Human Shell UI Correctness and Polish

Fix shell result correctness, especially preserving actual exit codes in shell digest/inclusion/ask flows. Add tests for failed command propagation. Improve shell listing and lifecycle display enough that shell commands behave predictably as first-class ephemeral UI artifacts.

Outcome: human shell execution becomes safe to rely on for iterative development without misleading success/failure summaries.

### Phase 7: Background Task Lifecycle Cleanup

Centralize ownership of spawned TUI-side tasks such as file indexing, diff workers, shell handles, config watcher tasks, and async command tasks. Add explicit shutdown/abort behavior and tests that background tasks stop when the app exits or restarts.

Outcome: fewer orphaned tasks and less risk in future daemon/multi-frontend/multi-session work.

### Phase 8: Remote TUI Protocol Rationalization

Decide whether remote mode is event-driven or frame-driven. If event-driven, deprecate unsupported `RenderFrame` behavior or turn it into an explicit protocol warning. Add robust resync handling for lagged clients and share state application logic between embedded and remote modes.

Outcome: remote/daemon mode becomes less ambiguous and safer to extend.

### Phase 9: Layout and Render Regression Coverage

Use `ratatui::backend::TestBackend` to render realistic app states at small, normal, wide, and tall terminal sizes. Cover main view, sidebar states, core dialogs, completion overlay, security review, research browser, and shell UI. Include pathological text cases: long lines, wide Unicode, malformed JSON, huge tool output, empty messages, tool-only messages, and deep session trees.

Outcome: future TUI changes are guarded against panics, layout overflows, and terminal-size regressions.

### Phase 10: Dialog Loading, Cancellation, and Stale Result Handling

Standardize dialog state machines across session list, tree, import, research, model, memory, and doctor surfaces. Use request IDs or generations to ignore stale completions. Closing a dialog should cancel or invalidate in-flight work.

Outcome: async UI work becomes predictable even under rapid user navigation and slow core responses.

### Phase 11: TUI Runtime Module Decomposition

Split `src/tui/mod.rs` into runtime/event-loop, command dispatch, app-event application, and per-domain handlers. Preserve behavior while reducing the size and risk of the central file. Keep the design rule that background tasks perform work and typed messages perform UI mutation.

Outcome: smaller, safer patches for future TUI features.

### Phase 12: UX Consistency and Discoverability Polish

Move long multi-line output out of toasts and into scrollable surfaces. Standardize labels for session, turn, model, agent, subagent, goal, task, shell command, and tool call. Improve status bar state distinctions: idle, thinking, streaming, tool-running, awaiting permission, awaiting question, background task active, and error.

Outcome: the TUI becomes more readable and more predictable as codegg grows into richer agent and daemon workflows.

## Recommended Execution Order

Execute phases 1 through 6 first. They address concrete runtime risk and correctness without requiring a broad refactor. Phases 7 through 12 should follow once the event loop, filesystem work, terminal lifecycle, logging, input semantics, and shell correctness are stabilized.

## Cross-Phase Verification Checklist

Each phase should keep these checks green or add them if absent:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- TUI-specific unit tests under `tests/tui.rs` and module-local tests
- Fake-core responsiveness tests for async command paths
- Headless render tests with `ratatui::backend::TestBackend`
- Manual smoke test in a real terminal and inside `zellij` or another multiplexer

## Non-Goals

This roadmap does not attempt to replace ratatui, rewrite the TUI, move to a GUI frontend, or change the core session protocol in one pass. It also does not require implementing every future remote/frontend capability before making the embedded TUI more robust.
