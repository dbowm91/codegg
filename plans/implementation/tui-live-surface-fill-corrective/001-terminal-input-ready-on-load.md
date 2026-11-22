# TUI Live Surface Fill Corrective M001 — Terminal Input Ready on Load

Status: implemented — conditionally closed
Closure record: `plans/closure/tui-live-surface-fill-corrective/001-status.md`
Repository baseline: `d319afec15b8b8b33ced2c0af7a3ee50eb1a82c3`

Source roadmap: `plans/subsystems/tui-live-surface-fill-corrective-addendum.md#m001--terminal-panel-is-input-ready-on-load`
Long-term requirements:
- `plans/000-long-term-specification.md#44-frontends-render-projections`
Applicable ADRs: none.
Primary class: capability

## 1. Objective

Selecting the Terminal panel leaves a live shell focused for input, or shows a typed refusal. The user does not press `i` to make a freshly loaded terminal able to take a key.

## 2. Why this milestone is ready

No hard dependency is open. Interactive-process attach, the panel host, and the terminal controller already exist. `interactive-terminal-liveness-post-closure-corrective` C001–C004 are closed and are not reopened. Their acceptance proved pull, refusal wording, cadence, and ergonomics. It did not prove that loading the panel focuses the PTY.

## 3. Current implementation evidence

- `open_panel_box` returns `PanelFill::Bespoke` for Terminal. Materialize does not spawn.
- `TerminalPanel::on_activate` adopts `active_handle()` and otherwise leaves the panel empty. The comment says `i` starts the shell.
- Unfocused-controller `i` and Enter emit `StartTerminal` or `TerminalPanelFocus`. That arm runs only after `panel_owns_keyboard`, which requires a focused visible panel and either Normal mode or `claims_insert_keys`.
- `claims_insert_keys` is true only when `InteractiveTerminalController::is_focused` is already true. A just-loaded terminal therefore does not take Insert-mode keys. A `Shift+H` dock that does not focus the panel sends `i` to the composer.
- `spawn_default_terminal_for_panel` sets `terminal_panel_spawn` and returns if a create is already in flight.
- `apply_terminal_attach_finished`, on the panel-spawn bit, calls `show_terminal` and does not call `focus`.
- `focus` returns false unless the link allows mutation and an attachment id is present. `focus_terminal_panel` then re-attaches or respawns. The re-attach does not set the panel-spawn bit, so its completion opens the terminal box.

## 4. Invariants that must not regress

- No local PTY. Create and attach stay daemon commands.
- The WS-attach refusal from the closed C002 stays a refusal. This milestone does not add a transport.
- A second select while a create is in flight does not start a second shell.
- `Esc` still leaves insert. `Shift+Q` still leaves the panel. A bare `q` is still PTY input once focused.
- `focus` is not called on a handle that cannot take input.

## 5. Scope

In: the select/load path for the Terminal panel, the panel-spawn attach completion, and the `i` path once a shell exists.

Out: the terminal box, output pull, resize, VT rendering, remote authorization, and any new protocol variant.

## 6. Required production changes

- Frontend only, in `crates/codegg-tui`. Selecting and focusing the Terminal panel requests the default shell when the controller has no live handle, through the existing `spawn_default_terminal_for_panel`.
- Panel-spawn attach success shows the handle and focuses it. Failure clears the spawn bit and leaves a visible refusal, not an input-ready panel.
- `i` on an already-live attached handle focuses the controller. `i` on a handle that cannot take input replaces or re-attaches it and, on success, focuses it. It does not open the box over the panel the user is reading.
- No daemon, protocol, storage, or security change.

## 7. Ordered work packages

### W1 — Failing load receipt
Intent: prove the baseline does not leave the controller focused after select.
Required changes: a test that selects the Terminal panel and asserts the controller is not focused and that a following key is not queued as PTY input. It must fail on this baseline.
Acceptance evidence: the test fails before the production change.

### W2 — Spawn and focus on select
Intent: select starts the shell and the attach completion focuses it.
Required changes: the select path and `apply_terminal_attach_finished` as in §6. Do not focus before the attachment id exists.
Acceptance evidence: the W1 test passes, and a failed create or attach does not report ready.

### W3 — `i` after load
Intent: once a shell is live, `i` is unnecessary for the first input, and `i` after `Esc` returns to input without opening the box.
Required changes: `focus_terminal_panel` keeps the panel route. A key after successful load is `TerminalPanelInput`.
Acceptance evidence: one test for the post-`Esc` `i`, and one test that a key immediately after successful attach is queued input.

## 8. Failure, cancellation, restart, contention semantics

- Create or attach error: clear `terminal_panel_spawn`, toast or in-panel refusal, controller not focused.
- Superseded request id: ignore, as today. Do not focus a stale handle.
- Overlapping select: the existing in-flight guard stays. A completion focuses only the handle the panel is showing.
- User leaves the panel before attach completes: do not steal keyboard focus back. Show the handle if the panel is still the terminal panel; do not force focus.

## 9. Compatibility and migration

None. Older clients keep the current `i` gesture. The new behavior is additive on select.

## 10. Required tests

- The W1 regression, failing on this baseline.
- Successful attach focuses and queues the next key.
- Failed create and failed attach do not focus.
- A second select during an in-flight create does not call create again.
- Post-`Esc` `i` focuses and does not open the terminal box.
- Existing terminal pull and refusal tests stay green.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --test tui_render -- terminal
scripts/verify.sh scoped
```

If the terminal regressions live in a different family, run that family member. Do not substitute `cargo check`.

## 12. Documentation updates

If select-to-input replaces "press i to start", update the panel hint and the terminal section of `docs/tui.md` in the same change. Do not edit a closed closure record.

## 13. Acceptance criteria

- Selecting the Terminal panel on the socket-daemon frontend either accepts the next typed key as PTY input or shows a refusal. A second `i` is not required.
- `i` after `Esc` returns to input.
- A dock that does not focus the panel does not send `i` to the PTY. Focusing that panel then meets the same ready-or-refusal rule.
- No new transport and no local shell spawn.

## 14. Stop conditions

- Stop if readiness requires a PTY on the WS-attach frontend. Report it. Do not route around C002.
- Stop if `focus` cannot succeed after a successful attach that carried an attachment id. Report the controller predicate. Do not mark the panel focused in the UI while the controller refuses.
- Stop if the only way to spawn is a new daemon command. The existing create path is the one to call.

## 15. Closure evidence required

- The W1 test's baseline failure and post-change pass.
- Focused test command and `scripts/verify.sh scoped` output, labeled local.
- A manual or socket-attached note: select Terminal, type one character, and record that it reached the shell or that a refusal was visible. A unit test is not this note.

## 16. Handoff notes

The closed C001 tests call the pull function directly. Do not add another test of that shape and call the panel ready. The receipt is select, then a key, then queued input or a visible refusal.
