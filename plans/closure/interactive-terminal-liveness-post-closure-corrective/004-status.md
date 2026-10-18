# Interactive Terminal Liveness Post-Closure Corrective C004 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/interactive-terminal-liveness-post-closure-corrective/004-terminal-surface-ergonomics.md`
Source subsystem roadmap: `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md#c004--terminal-surface-ergonomics`
Repository baseline reviewed: `18c3dca92` (`feat/auto-20261017-f117dc16`)
Implementation commits: none — the working tree at closure time contains the change; it is **not committed** at the time of writing.

Class: **polish**, as registered. It was planned as polish and it is closed as polish; one
defect found during execution was a *test fixture* defect rather than a product defect and is
reported in §6 as such rather than reclassified into a capability claim.

## 1. Executive finding

All four owned items were real, all four are now delivered, and the cause in every case was the
same shape as C003's: **a surface making a claim it had not earned.** Not one of these was a
cosmetic preference; each was the interface asserting something untrue about the state behind it.

**The hint row kept naming a terminal that had given the keyboard back.** Every verb in
`TERM · focus to scroll, select and type` — including the `Shift+←/→` gesture the line itself
advertised — requires the keyboard that the composer had just taken. So the line sat above the
LLM buffer describing a surface the user was not in, and the gesture that would have fixed it
was the thing being advertised by the broken state. The seam's own documentation had authorised
this: it recorded that a docked panel may advertise itself without the keyboard, which was a
correct rule for a *hosted* `/files` box and was silently extended to a *bespoke* panel that did
not need it. The seam is now split **by kind**, not by convenience.

**The caret never moved, because nothing had a phase.** `styled_lines` painted the caret's
`REVERSED` modifier once and there was no input that could change it — not a missing animation,
a missing *phase*. The temptation was a frame counter on the view; that would have been the
second source of truth the one-cadence rule exists to prevent, so the phase is a pure function
of elapsed time and is threaded in, exactly as `spinner_frame` already works.

**Clear blanked the screen and the shell had no reason to repair it.** The wipe was entirely
local — `clear_scrollback` plus `screen.reset` — so a live shell sat on an empty screen showing
nothing, because nothing had been typed at it. The tempting fix was to draw a prompt; that would
have been the exact fabrication this workstream forbids, and it would also have been wrong for
any non-default `PS1`. Clear now asks the process to clear itself.

**`Esc` handed back the keyboard and left the cursor at the bottom.** `escape` never moved `nav`,
so the reader's cursor stayed wherever `sync_viewport` had last clamped it — which after a clear
is the bottom row of an otherwise empty screen. The first `k` then looked inert and the first
`x` armed a line nobody could see.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Result |
|---|---|---|
| Work package A: the hint row survives losing the keyboard, falsified before the fix | `the_panel_declines_the_row_when_another_surface_holds_the_keyboard` failed on baseline with `left: Some("TERM · focus to start a shell")`, and the integration test captured a rendered frame showing `▌ TERM · focus to scroll, select and type` directly above `[NOR] [build][LLM]  ❯` | pass |
| Unfocused panel contributes no row; the row returns on refocus | `the_terminal_hint_row_only_exists_while_the_panel_holds_the_keyboard` asserts the row is present, absent after `Shift+←`, present again after `Shift+←`, and that the reflow is symmetric in both directions | pass |
| The row's disappearance must not move the input box | `the_terminal_hint_row_only_exists_while_the_panel_holds_the_keyboard` pins the composer on the *same* row before and after, and asserts the vacated row is absorbed above it. This contradicted the closure's first draft, which asserted the composer moved up one row — see §6 | pass |
| Other panels keep their advertised keymaps | The three hosted `/files` tests (`a_docked_files_panel_advertises_its_own_keymap_without_the_keyboard`, `a_focused_panel_names_the_way_back_to_the_composer`, `every_other_hosted_panel_keeps_its_original_hint_line`) and the remaining 40 in `tui::panels::tests` pass unmodified; the seam doc is rewritten to justify both answers | pass |
| The seam doc is updated in the same change | `SidePanelHost::composer_hints` now states the hosted/bespoke split and records that the old unfocused terminal branch outlived its reasoning; `App::composer_hint_text`'s own doc no longer claims a docked panel names itself | pass |
| Blink: a pure sibling of `spinner_frame`, on the existing clock | `spinner::cursor_blink_visible(elapsed)`, three unit tests in `tui::components::spinner` pinning both phases, purity, and the derivation from `SPINNER_FRAME_INTERVAL` | pass |
| Blink: render tests at two pinned phases, cursor present and absent | `the_insert_caret_blinks_on_the_phase_it_is_given` and `the_docked_caret_blinks_on_the_phase_the_host_grants` (panel, through a real `PanelCtx`) — 1 reversed cell on, 0 off | pass |
| The hidden phase changes the modifier and nothing else | `the_hidden_phase_keeps_the_caret_cell_it_reserves` compares the rendered text of both phases cell-for-cell; the cell stays painted so a prompt line cannot trim and blink away | pass |
| NOR never blinks | `the_reader_cursor_never_blinks` renders both phases in NOR and asserts one reversed cell each time | pass |
| I4 — no second ticker | No `tokio::time::interval`, thread, or new driver. The phase is read from the one epoch in `spinner`, and the armed 100ms pull C003 registered is what repaints it. A test asserts the phase is a whole number of `SPINNER_FRAME_INTERVAL` frames | pass |
| Prompt line after clear originates from process output (option a) | `a_cleared_terminal_is_replaced_by_a_prompt_the_process_printed` types into a **real `sh` behind a real PTY** and asserts the prompt that lands on screen is the shell's own; a fixture that echoed the clear would pass with a local wipe and no process interaction, which is the fabrication the milestone forbids | pass |
| A new terminal shows its prompt with no keystroke | `a_new_terminal_shows_the_processes_own_prompt_without_a_keystroke`, driven through `pull_until_rendered_armed`, which re-asserts the production arming decision every pass | pass |
| Clear does not manufacture a resync | `clear_preserves_the_sequence_cursor` compares the sequence cursor before and after and asserts no resync text | pass |
| A refused clear reports itself and leaves the screen intact | `a_refused_clear_reports_itself_and_leaves_the_screen_intact` fills the coalescing buffer to within the clear's own length, then asserts the refusal toast **and** that the previous screen is untouched | pass |
| Clear goes through the existing bounded, permissioned, live-link path | The bytes go through `queue_input`, the same function every keystroke uses. No new write path, no privileged write, no input-gate bypass | pass |
| Nothing on the cleared screen is fabricated | No prompt string is synthesised anywhere. The empty states remain labelled product text (`" no output — press i to type"`, `(no output yet)`) and say nothing about a prompt | pass |
| `Esc` placement: 0-row, 1-row and multi-line geometry | `leaving_insert_on_an_empty_screen_lands_on_row_zero`, `leaving_insert_cannot_underflow_on_a_one_row_screen`, `leaving_insert_lands_the_reader_cursor_on_the_last_line_with_content`, `leaving_insert_on_a_full_screen_lands_on_the_last_row` | pass |
| The `Hidden → Viewing` sweep does not move a cursor the user never moved | `an_unrelated_escape_does_not_move_a_hidden_views_cursor` | pass |
| Placement is not a navigation gesture | `leaving_insert_keeps_the_selection_the_reader_built` — `set_cursor`, not `move_to`, so a `v` selection survives Esc from Insert | pass |
| I1–I5 and I3 preserved | No change to read bounds (`max_bytes: None`, explicit `from_seq`), no bus event or projection change, no key-routing change, one cadence, and no poll that supersedes a user action — the placement runs on the `Esc` the user pressed, never on a tick | pass |
| No existing test weakened or deleted | None removed or relaxed. One C003 test fixture was **strengthened** — see §6 | pass (with the recorded reason) |

## 3. Production implementation evidence

### 3.1 The hint row

`TerminalPanel::composer_hints` answers `None` when `!hints.focused`, with the removed branch's
reasoning kept in the doc comment rather than deleted: the branch existed because the seam
permitted it, and a future reader needs to know why it was wrong, not only that it is gone.

`SidePanelHost::composer_hints` is rewritten to state the split it actually has. A *hosted*
panel — an adopted box, whose verbs exist nowhere else — keeps advertising without the keyboard
and leads with the gesture that makes them live. A *bespoke* panel holds a live projection the
reader can already see, so it declines and the row goes away. The terminal is bespoke.

**The input box does not move, and that is worth stating because it is not what the arithmetic
looks like.** `session_layout` gives the prompt chunk a fixed, bottom-anchored height, and the
working strip, hint line and todo strip are carved off its *top* in that order. Removing the hint
therefore shrinks the carved region and hands the row to the transcript (`Min(10)`), while the
composer stays exactly where it was; the working strip, carved from the chunk's top edge, moves
down into the vacated row. This closure's first draft claimed the opposite — that the composer
moves up one row — on the strength of the reserved-row arithmetic alone. A rendered frame showed
the composer on row 35 both with and without the hint row, so the draft claim was withdrawn and
the documentation triple corrected with it. Writing down a reflow consequence without rendering
it is precisely the habit this workstream exists to correct.

### 3.2 The blink

`spinner::cursor_blink_visible(elapsed)` is a pure sibling of `spinner_frame`: half a second
visible, half a second hidden, derived from the same 100ms frame interval. The epoch is one
process-wide `OnceLock<Instant>` (`cursor_blink_visible_now`) rather than a field per view, so
the box and the docked panel agree on the phase and nothing has to be reset when a terminal is
attached, detached or replaced. The phase reaches a render as an **input** —
`PanelCtx::caret_blink_on` for the panel, an argument to `render_view` for the box — which is
the only reason any of this can be tested without racing a clock.

Two rules are load-bearing and both are pinned:

- The hidden phase drops the `REVERSED` modifier and **nothing else**. The cell stays painted.
  A cursor that stopped reserving its cell would let `styled_lines` trim the line back to its
  last character, and at the end of a prompt the whole line would vanish for half of every
  second — text flickering instead of a caret.
- NOR never blinks. There the cursor is a selection anchor, and `x`, `v` and `d` all act from
  it. A blinking anchor turns "which line am I about to delete" into a coin flip.

`CURSOR_BLINK_PHASE` is written as a literal duration because `Duration`'s `Mul` is not `const`
on this toolchain; a test asserts it is exactly `SPINNER_FRAME_INTERVAL * 5` and a whole number
of frames, so retuning the frame interval without rechecking it fails loudly.

The box is a *pushed* dialog: its content is styled when `refresh_terminal_dialog` runs rather
than every frame. That is sufficient because C003's armed cadence re-runs it on every output
pull — five ticks per blink phase. A box whose link stopped reading does not repaint and its
caret holds its last phase; the same frame shows that read stopped, so the frozen caret is
explained rather than mysterious.

### 3.3 The prompt line after clear

`clear_terminal` now queues `CLEAR_THROUGH_PROCESS` (`ESC [ 2 J ESC [ H CR`) through
`queue_input` — the same bounded, permissioned, live-link path as any keystroke — **before** the
local wipe, and only wipes once the bytes are admitted. A typed refusal reports itself and
leaves the screen intact, because a local wipe whose repaint was never sent is a blank screen the
user did not ask for.

Both halves of the sequence are load-bearing and the reason is not obvious: the erase-display and
cursor-home is what a terminal means by "clear", but sent alone it leaves the screen blank,
because a shell draws `PS1` after a *command*, not after an escape. The trailing newline
completes an empty command line, which is what makes the shell repaint its prompt.

The literal `clear` command was considered and rejected: it depends on a `clear` binary on the
child's `PATH`, and when it is missing the user is shown `clear: command not found` — an error
for a keystroke they never typed. The escape sequence has no such dependency and works for `sh`,
`zsh`, `fish` and a custom `PS1` alike.

**Recorded deviation from the plan.** §8 of the plan proposed that any input refusal leave the
screen intact. That is right for every refusal *except* `NotLive`: an exited, gone or detached
view has no process left to print a prompt, so nothing is being suppressed, and refusing would
take away the only way to clear the output of a shell that has finished. Such a view takes the
local wipe alone and reports nothing, because no write was attempted. The blank screen it leaves
is the truth about a dead process rather than a fabrication.

### 3.4 Reader-cursor placement on `Esc`

**The rule: the last row that has content, or row 0 when nothing has.**

This supersedes the plan's default wording — "the line above the last one" — for the reason the
plan itself asked to be settled before implementing: that rule trades one wrong place for
another. The overwhelmingly common case is a shell whose prompt sits on row 0 of a 24-row
screen; "one row above the bottom" would park the cursor in blank space *below* the only line on
screen. Anchoring on content puts it on the prompt line after a short command and on the last
line of the block after a long one, which is where the reader was either way. The plan's own
rejected alternative ("landing on the block's last line") is what this rule produces on a full
screen.

Placement happens in `InteractiveTerminalController::escape`, so both surfaces get it from one
call. `sync_viewport` runs first so the row is measured against the offset the screen is actually
parked at, and `set_cursor` is used rather than `move_to`, so placement is bookkeeping and does
not extend a `v` selection. The `Hidden → Viewing` sweep stays a separate pass and never touches
`nav`: a hidden view becoming visible is not a gesture the reader made.

## 4. Verification

All verification is **local**. No CI run was obtained for this change, and a green local
`quick` is not a CI result.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `bash scripts/verify.sh quick` | `==> Quick verification passed.` (exit 0) |
| `cargo clippy -p codegg --all-targets -- -D warnings` | clean (exit 0) |
| `cargo test -p codegg --lib tui::panels::` | 43 passed, 0 failed — includes the three hosted `/files` tests, unmodified |
| `cargo test -p codegg --lib tui::interactive_terminal` | 57 passed, 0 failed |
| `cargo test -p codegg --lib tui::commands::interactive_terminal` | 37 passed, 0 failed, **stable across four consecutive runs** |
| `cargo test -p codegg --lib tui::components::spinner` | 6 passed, 0 failed |
| `cargo test --test tui_panel_docking` | 53 passed, 0 failed |
| `cargo test --test tui_chat_surfaces` | 29 passed, 0 failed |
| `cargo test --test tui_render` | 111 passed, 0 failed |
| `cargo test --test interactive_terminal_tui` | 6 passed, 0 failed |

21 new tests in total: 10 in `tui::interactive_terminal`, 3 in `tui::components::spinner`, 3 in
`tui::panels::terminal`, 5 in `tui::commands::interactive_terminal`, and 1 integration test in
`tests/tui_panel_docking.rs`.

Two compile errors were caught during this work rather than by the targeted runs, and both are
worth recording because each is the kind that hides:

- The `visible()` projection helper in `tests/tui_panel_docking.rs:696` still called `render_view`
  with the old arity. `verify.sh quick`'s `cargo check --workspace --all-targets` caught it, not
  the suite runs, because the helper is only compiled for the integration target. It passes the
  caret-present phase explicitly, being a text projection with no clock to read a phase from.
- A section banner placed directly after a doc comment attached itself to the next function and
  tripped `clippy::empty_line_after_doc_comments`. Fixing it naively moved a function body; the
  region was rebuilt and re-verified rather than patched twice.

### Non-vacuity

Each of the four fixes was falsified on the pre-change baseline and only then implemented — and
then, as a second pass, **re-reverted in place and the tests re-run**, because a test written
against a change and never seen failing proves nothing about the test. All four reversions
failed their own tests and all four files were restored byte-for-byte afterwards:

| Reverted fix | Test | Result of the reversion |
|---|---|---|
| `if !hints.focused { return None; }` restored in `TerminalPanel::composer_hints` | `the_panel_declines_the_row_when_another_surface_holds_the_keyboard` | FAILED — 1 failed |
| `&& cursor_visible` dropped in `styled_lines` | `the_insert_caret_blinks_on_the_phase_it_is_given` + the spinner unit tests | FAILED |
| `CLEAR_THROUGH_PROCESS` replaced with an empty write in `clear_terminal` | `a_cleared_terminal_is_replaced_by_a_prompt_the_process_printed` | FAILED after the full 15s `PULL_TIMEOUT` — the prompt never arrives with no bytes sent, which is the defect itself |
| `place_reader_cursor_after_insert()` removed from `escape` | the four `leaving_insert_*` tests | FAILED — 2 failed, landing on the clamped bottom row |

## 5. What was deliberately not done

- **No push transport and no terminal bytes on the bus.** Both remain ADR-gated stop conditions.
  `src/server/**`, `crates/codegg-client/**`, `crates/codegg-protocol/**` and
  `crates/codegg-core/**` are unmodified; no `CoreRequest`/`CoreResponse` variant was added.
- **No second cadence.** The blink deliberately rides the frame clock and the armed pull rather
  than adding an interval, a thread, or a per-view timer.
- **No synthesized prompt line**, and no relaxation of the product text that labels an empty
  surface.
- **No change to what any key means.** `classify_key`, `TerminalKey`, `TerminalFocus` and
  `handle_key` are untouched.
- **No C001–C003 closure record was edited.** The three records in
  `plans/closure/interactive-terminal-liveness-post-closure-corrective/` are immutable; this one is
  additive.
- **The event loop's `select!`/sleep is still not exercised by a test.** Only the arming decision
  is, exactly as C003 recorded. A test of the loop itself would require a timer-driven harness,
  and nothing in this milestone made one more affordable.

## 6. A fixture defect found and fixed, not worked around

`drain_tui_commands` stops at the first 25ms gap in the command channel. That is fine for
whatever it happens to consume, but the terminal tests depend on **two** asynchronous steps, and
the second is spawned by the first:

1. The **create** command is what materialises the view, opens the box and gives the panel
   something to adopt. A drain that consumes nothing and then sees 25ms of silence returns with
   no terminal at all.
2. The **attach** is spawned *by* the create completion. A drain that consumed the create and then
   saw 25ms of silence returns with a view that exists and has **no attachment** — and pressing
   `i` against that is refused by `focus`, whose refusal re-attaches instead of focusing, so the
   test goes on to assert against a terminal nobody was typing into, and `Esc` then correctly
   blurs the panel because `owns_exit_key` reports that no PTY holds the keyboard.

Three C003 tests were carrying this and were passing by luck. `box_rendered_buffer_shows_live_pty_output`
failed on `the box must be mounted before its output can be asserted on` (`left: None`,
`right: Terminal`); the two panel tests failed on `the panel adopted the new terminal`. All three
are failures of a *premise*, which is the worst shape for a race to have: the assertion names
something the reader cares about, and the real cause is a millisecond.

This was latent and invisible until this milestone, for a precise reason. The hint row used to
answer `Some` in *both* focus states, so "the panel never got the keyboard" and "the panel just
handed the keyboard back" were indistinguishable to every existing test. Narrowing the row to
`None` made the two distinguishable and the race surfaced — twice, at two different steps, before
the root cause was named.

The fix is two helpers rather than sleeps: `await_terminal_create`, which polls the command channel
until the create has been applied, and `await_live_attachment`, which polls the pull and the
channel until the view holds an attachment. Both are bounded by `PULL_TIMEOUT` so a genuinely dead
create fails with a message instead of hanging. Every test in the module that presses a key at a
PTY now goes through them.

No product defect was found here. The controller behaved correctly throughout — `focus` refusing an
unattached view and `owns_exit_key` declining a keyboard nobody held were both right. Only the test
fixtures' timing assumptions were wrong. It is recorded in this section rather than in §2 because a
test-only defect that changes three tests in another milestone is worth being findable.

## 7. Unresolved findings

1. **The box's viewport plumbing remains deferred.** The box still does not report its viewport to
   the PTY the way the docked panel does, so a process with an interactive TUI may lay itself out
   for a geometry it was never told about. This arrived from C001 as low-severity deferred work and
   this milestone did not open it: it needs the box's own geometry to be authoritative, which is a
   layout question rather than an ergonomics one, and folding it in here would have made a layout
   change indistinguishable from the four fixes. Still open, still owned by this workstream.
2. **CI evidence is outstanding**, as it is for C001, C002 and C003.
3. **No commit was created.** The change is in the working tree; nothing was pushed.
4. **Workspace-wide clippy was not run** (`cargo clippy --workspace --all-targets`). The root
   package is clean and no crate outside `codegg` is touched by this milestone.

## 8. Registry and roadmap updates

- `plans/registry.md` — the C004 row is now **closed** with the closure link and the four
  deliverables recorded; the roadmap row's status is `C004 closed`; the execution-order
  paragraphs name C004's closure. The C004 row's dependency cell previously carried a *blocked*
  paragraph about M008 and proposed ADR-0034 (syntax-node text objects) belonging to the
  selection-advisor workstream; it contradicted the row's own `ready` status, was a copy error,
  and is corrected here with that correction stated rather than silently dropped.
- `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md` — status is
  **complete**; the §12 table's C004 row names this closure record; the C004 section records that
  the placement rule was decided and how.
- `plans/implementation/.../004-terminal-surface-ergonomics.md` — status `closed`, with the
  criterion-4 supersession stated on the status line so the deviation is visible without reading
  this record.
- The two predecessor closure records, and `plans/closure/interactive-process-sessions/003-status.md`
  and `plans/closure/human-execution-corrective/003-status.md`, are **not edited**.
- Documentation triple updated together: `architecture/tui.md`, `docs/tui.md`, and
  `.opencode/skills/tui/SKILL.md`.

## 9. Roadmap disposition

**This closes the corrective.** C001 is conditionally closed with its condition discharged by C003;
C002 is closed; C003 is closed; C004 is closed. No milestone in
`plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md` remains open,
and no item from the five defects originally reported against `/terminal` is unowned — the three
this workstream took are delivered here and by C003, and C004 closed the last of them.

The workstream's two standing requirements were honoured: every acceptance signal is driven
through the real `App`/`Controller` rather than by a harness that supplies the loop production
withholds, and nothing in this milestone manufactures a line the process did not emit. The one
item left explicitly open is §7 finding 1, the box's viewport plumbing, which is deferred **with a
recorded reason** rather than silently dropped — as §11 requires.