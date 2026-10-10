# Human Execution Corrective C003 — Terminal Keyboard and VT Fidelity

Status: ready for handoff

Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b`

Source roadmap: `plans/subsystems/human-execution-corrective-roadmap.md#c003--pty-keyboard-and-vt-fidelity`.

Predecessor: closed `plans/subsystems/interactive-process-sessions-roadmap.md` M001–M003; `plans/closure/interactive-process-sessions/003-status.md`, `tests/interactive_terminal_tui.rs`, and `architecture/tui.md`. This corrective adds capability beyond the earlier tested PTY byte-transport contract; it does not rewrite historical closure.

Long-term requirements: `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`; `#29-system-invariants`; `#45-locality-by-default`; `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`. Applicable ADRs: none; new remote process/PTY ownership would need another decision.

Primary class: capability/polish.

## 1. Objective

Make the existing daemon-owned Unix PTY usable as an interactive shell rather than merely a byte-oriented process with a newline-split transcript. Fix literal Esc/focus behavior, faithfully bounded VT/ANSI screen interpretation, resize and resync, without coupling the terminal to model context or adding another process owner.

## 2. Why this milestone is ready

The real PTY `InteractiveProcessService`, attach/resume protocol, daemon authorization and TUI projection/controller already exist and M001–M003 are closed. C003 can proceed independently of C001/C002 because no human one-shot process or promotion API needs to change.

## 3. Current implementation evidence and testing gap

`src/tui/interactive_terminal.rs::classify_key` intercepts Esc to exit terminal focus unconditionally; Esc never reaches the child, interfering with programs such as Vim. `TerminalKey` currently maps a restricted set of printable/control/arrows; investigate other modifiers, paste and global TUI keybindings. `InteractiveTerminalController::render_lines` converts bounded recent output with `String::from_utf8_lossy`, splits on newline, strips one trailing CR and truncates lines. It does not maintain terminal cursor, clear/erase state, SGR or alternate screen. `src/tui/commands/interactive_terminal.rs` launches 80x24 PTYs and forwards terminal resize commands. `tests/interactive_terminal_tui.rs` has real `cat` and focus/detach tests, proving byte flow but not faithful full-screen terminal rendering. Linux/macOS `openpty` is supported; non-Unix is `PlatformUnsupported`.

## 4. Invariants

- `src/interactive_process.rs` remains the only PTY spawn/lifecycle owner; `src/interactive_process_attach.rs` remains the sole attach/cancel authority. TUI owns display/input adapters only.
- Keyboard bytes reach only the explicitly focused, authorized, live attachment; they must never submit a chat/agent prompt.
- Escape/control sequences are parsed in a bounded terminal-screen representation, never forwarded raw to Ratatui's surrounding host terminal.
- Raw terminal output is not promoted into model/session/observer context. No C001 promotion path is introduced by this milestone.
- History/sequence gaps must reset or clearly degrade screen state; cannot fabricate a fully valid screen from incomplete VT state.

## 5. Scope

In: keymap/focus UX and tests, bounded VT/ANSI state with safe screen painting, input modifier/escape fidelity, viewport dimensions and resize, reconnection/lag behavior and representative full-screen program tests. Out: Windows ConPTY, shell persistence after daemon restart, multiplexer/SSH layer, generalized clipboard or OSC52 support, model-facing PTY access, rebuilding daemon PTY engine, implementing an entire VT parser from scratch without justification.

## 6. Required production changes

### Core/domain
No new daemon or process identity. Reuse existing immutable terminal handle, attachment ID, output cursor, resize dimensions, and typed connection state. Terminal-screen presentation state must be bounded per active view.

### Storage and migrations
No persistent new storage. No terminal raw bytes in session messages. Bound local screen buffers and scrolling independently of daemon's existing bounded output ring.

### Protocol and DTOs
Reuse existing `InteractiveProcessInput`, `Resize` and `Resume` operations; the existing base64 byte input can represent Esc and control keys. Add protocol only if a verified requirement cannot be expressed with present primitives; retain version negotiation and backward compatibility.

### Runtime and concurrency
Evaluate maintained VT emulator/parser crates and their MSRV, licenses, unsafe/transitive deps and performance before selecting one; use an existing parser when practical. Maintain incremental state across split CSI/OSC and UTF-8 chunks. Support at least cursor positioning, CR-only progress, LF, wrapping, tab/backspace, clear line/screen, SGR and alternate screen behavior as the chosen parser supports. Avoid OSC/DCS exfiltration (e.g. clipboard) and unbounded escape payloads; never blindly emit child control bytes to the host TUI. Ensure bounded per-frame updates and stale-generation fencing. Resize actual PTY viewport and corresponding renderer; replay/resync must explicitly reset or reconstruct state with complete bounded history.

### Frontend/operator
Use a dedicated leave-focus chord, preferably Ctrl-], so literal Esc is transmitted to the focused PTY; verify actual key interception and avoid collision with existing TUI commands before choosing. Unfocused keys remain inert; focus is explicit and visible. Preserve common Enter, Backspace, Tab, Ctrl-C, Ctrl-D and arrows, add justified modifier/paste handling, and keep global bindings from intercepting keys while focused. Document terminal close-as-detach, terminate action and focus indicator. Provide a usable full-screen view sized to dialog; avoid reliance on initial 80x24.

### Security and authorization
Escape sequences never mutate the outer TUI environment, clipboard or host terminal configuration. Bound parser memory/CPU and output payloads, reject unauthorized input after disconnect/exit. Redact only metadata/audit output as appropriate; terminal screen itself is private local user output, never an automatic model context source.

### Documentation/static guards
Update `architecture/tui.md`, `architecture/process-tool-execution-ownership.md`, `docs/tui.md`, .opencode TUI skill, terminal help/shortcut overlay and, if useful, an additive pointer from the closed PTY roadmap. Do not alter `plans/closure/interactive-process-sessions/003-status.md` as though its original tests never passed. Keep `scripts/check_execution_ownership.py` passing.

## 7. Ordered work packages

A. Enumerate current terminal key handling, dialog focus event routing, resize/viewport mapping, control-sequence rendering and coverage. Produce a fixed capability matrix and deterministic short VT byte transcripts; do not depend on locally installed Vim/htop in CI.

B. Implement explicit focus-exit chord and literal Esc forwarding; test modal key interception, supported controls, focus gain/loss, disconnected/exited modes and non-interference with prompt submission.

C. Add bounded incremental VT screen projection, screen repaint and sequence-gap/reset behavior with maintainable parser; reconcile viewport resize and protect outer Ratatui output. Retain raw bounded scrollback only if useful for debugging, not as a substitute for interpreted state.

D. Qualify actual Unix PTY input/output, representative alternate-screen/cursor-control fixtures, reconnect/resize, malicious escape sequences and multi-terminal stale completions; fix help/skill/docs and record closure evidence.

## 8. Failure, cancellation, restart and contention

Focus is cleared when terminal is gone, disconnected, exited or unauthorized; queued key bytes cannot cross to a new handle. Stale output and resize replies cannot repaint another terminal. Invalid or oversized VT sequences do not panic, execute commands or leak raw terminal escapes. On truncated scrollback/resync, explicitly reset or label screen state degraded. Detach leaves process alive; daemon restart marks handle gone.

## 9. Compatibility and migration

No migration anticipated. Escape chord is a deliberate visible keybinding change, documented in help. Non-Unix remains `PlatformUnsupported`, not a fake PTY. Retain prior protocol and user terminal commands.

## 10. Required tests

- Keyboard matrix: unfocused keys ignored, literal Esc delivered when focused, documented exit-focus chord does not forward, Enter/Backspace/Tab/arrows/Ctrl-C/Ctrl-D/modifiers work, no chat prompt submission.
- Real PTY `cat` confirms Esc and typed bytes; detach/reattach/terminate/lag/restart continue to produce typed states and bounded retained views.
- Incremental VT golden fixtures split at every byte boundary: cursor moves, erase/clear, SGR, CR progress, wrap, alternate screen enter/exit, Unicode partial sequences, resize. Assert correct cell content/rendered representative state and bounded memory.
- Negative OSC/DCS/clipboard and long malicious sequences, invalid UTF-8, rapid input/resize/switch, resync gap and stale handle; raw sequences cannot break the outer TUI.
- Existing `tests/interactive_terminal_tui.rs` and focused controller tests, no model/observer coupling.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib interactive_terminal
cargo test --test interactive_terminal_tui
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
```

Add focused VT parser/unit/E2E fixtures; run supported Linux/macOS evidence. On Windows only compile/unsupported-path evidence, no claim of functional PTY. Do not add a new CI lane.

## 12. Documentation updates

TUI architecture, PTY help/shortcut reference, TUI and human-shell skill distinctions, user terminal guide and execution-ownership semantics. Truthfully distinguish real PTY byte transport from interactive full-screen emulation.

## 13. Acceptance criteria

An explicitly focused terminal forwards Esc and a documented key chord reliably exits focus. Common line-oriented and alternate-screen VT programs display correct bounded screen state; resize, detach/reconnect and history-loss are handled without raw control-sequence leakage. PTY/process ownership and agent separation remain intact.

## 14. Stop conditions

Stop if a renderer choice has unacceptable license, unsafe dependency, resource bounds or portability issues; if proposed key chord conflicts with an unbypassable TUI invariant; if full-screen fidelity requires duplicate daemon execution state; or if acceptance can only be shown by a byte-echo test. Document and split unsupported features rather than incorrectly claiming completion.

## 15. Closure evidence

Key matrix; parser/library review; deterministic rendered-screen golden tests; real Unix PTY lifecycle test outputs; cross-platform support verdict; resize/reconnect/lag behavior; no raw escape injection; skills/docs modifications and exact commands.

## 16. Handoff notes

The closed `cat` PTY tests are still useful but not screen-emulation acceptance. Preserve unrelated user work. This corrective does not authorize model-facing PTY tools or revising historical closure records.
