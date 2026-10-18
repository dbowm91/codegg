# Interactive Terminal Liveness Post-Closure Corrective C004 — Terminal Surface Ergonomics

Status: closed 2026-10-17 — closure record:
`plans/closure/interactive-terminal-liveness-post-closure-corrective/004-status.md`. All four
owned items delivered, each falsified on baseline first. §13 criterion 4's default wording
("the line above the last one") was **deliberately superseded** by the recorded rule — the last
row that has content — and the deviation and its reason are stated in the closure record rather
than left to be inferred from the code. Unblocked 2026-10-17 by C003's closure
(`plans/closure/interactive-terminal-liveness-post-closure-corrective/003-status.md`), which
delivered the armed cadence the blink needs and landed the derived-mode work whose layout
arithmetic this milestone previously shared. Both former blockers are discharged.
Repository baseline: `18c3dca92` (`feat/auto-20261017-f117dc16`)

Source roadmap:

- `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md#c004--terminal-surface-ergonomics`

Long-term requirements:

- `plans/000-long-term-specification.md#44-frontends-render-projections`

Applicable ADRs: none.

Primary class: **polish** (ergonomics and diagnostics).

## 1. Objective

Make the terminal panel's own surface legible and honest about where the user's attention
belongs: the contextual hint row must disappear when the panel is not holding the keyboard, the
cursor must blink in Insert so the user knows the PTY is live, a freshly created or cleared
terminal must show a usable prompt line instead of blank space, and leaving Insert must place
the reader cursor near where the user was working rather than at the bottom of the viewport.

## 2. Why this milestone is ready

Blocked on **C003** (`plans/implementation/interactive-terminal-liveness-post-closure-corrective/003-live-surface-cadence-and-honest-mode-state.md`), and the dependency is technical rather than merely conventional:

- **Cursor blink is unimplementable before C003.** A blink is a time-varying render. The event
  loop only repaints while something arms it (`src/tui/runtime/event_loop.rs:288-292`), so on
  this baseline a blinking cursor would advance only while a toast or a keystroke happened to
  drive the frame — which reads as a worse defect than no blink. C003 makes the loop awake while
  a surface is mounted, which is the precondition.
- **The hint-row change is a layout change that should not race C003's derived-mode work.** Both
  touch `composer_hint_text` and the hint row's reserved height. Doing them in one milestone
  makes a one-row reflow indistinguishable from a mode-reporting regression.
- **`Esc` cursor placement interacts with the armed cadence.** With output arriving on its own,
  "the line above" is a moving target unless the viewport/viewport-offset discipline is already
  settled by C003.

No hard dependency outside this workstream is open; `interactive-process-sessions`,
`human-execution-corrective` C003, and `tui-panel-surface-convergence` M001 are all closed.

## 3. Current implementation evidence

Baseline `18c3dca92`.

**The hint row survives losing the keyboard.**

- `TerminalPanel::composer_hints` returns `Some(...)` in every branch, including `!hints.focused`
  (`src/tui/panels/terminal.rs:397-414`), where it renders `"TERM · focus to scroll, select and
  type"`.
- Every other panel returns `None` when unfocused (`chat.rs`, `session.rs`, `agents.rs`,
  `help.rs`).
- `composer_hint_text` merges the agent keys and the panel hint and keeps the row if either is
  `Some` (`src/tui/app/mod.rs:12322-12331`), so after a `Shift+←/→` focus swap the TERM line is
  still drawn directly above the composer even though the keyboard is in the LLM buffer.
- `hint_rows = u16::from(self.composer_hint_text().is_some())` (`src/tui/app/render.rs:175`)
  feeds `prompt_height` **before** `session_layout` (`:226-237`), so the row is genuinely
  reserved; returning `None` frees it and gives the height back.

**Design tension, stated rather than silently reversed.** The seam's own documentation justifies
the unfocused branch: "a docked panel names itself and the gesture that enters it, which is the
only way its keymap is readable before anyone presses the wrong key"
(`src/tui/panels/mod.rs:1519-1527`). The terminal panel is currently its only user. The reported
defect is that a hint describing the *terminal's* verbs persists above an input the terminal no
longer owns. C004 resolves this by narrowing the contract rather than deleting the seam: an
unfocused panel may advertise *how to get to it*, but must not claim the row while another
surface holds the keyboard. If C004 instead decides to remove the unfocused branch entirely, the
seam documentation must be updated in the same change and the deletion recorded here.

**The cursor never blinks.** `styled_lines` paints the cursor cell with a static
`Modifier::REVERSED` (`src/tui/interactive_terminal.rs:949-952`); there is no phase input to the
render path at all.

**The clock to reuse exists.** `src/tui/components/spinner.rs` already holds
`SPINNER_FRAME_INTERVAL` (100ms) and `spinner_frame(elapsed)` as a **pure function of elapsed
time** (`:25-39`), explicitly documented as drift-free and reproducible under `TestBackend`. That
is the correct seam for a blink, and AGENTS.md's one-cadence rule requires the blink ride it.

**A new terminal and a cleared terminal both render blank.**

- A fresh interactive shell prints its prompt after the attach response returns. On this baseline
  the bytes sit unread until the next keystroke — a C003 defect, and C003 is expected to remove
  it without work here. The panel's own empty-state line is
  `" no output — press i to type"` (`src/tui/panels/terminal.rs:262-266`).
- Clear is genuinely blank and is **not** fixed by C003: `clear_scrollback`
  (`src/tui/interactive_terminal.rs:1482-1504`) clears the scrollback bytes *and* calls
  `view.screen.reset(rows, cols)`, a purely local wipe. Nothing reaches the PTY, so the shell has
  no reason to re-print `PS1` and the panel stays empty until the next command. The same function
  also parks the reader cursor at `(rows-1, 0)` — bottom-left.

**`Esc` leaves the cursor at the bottom.** `escape`
(`src/tui/interactive_terminal.rs:1440-1451`) flips `TerminalFocus::Focused → Viewing` and
touches nothing else; it never moves `nav`. The reader cursor therefore stays wherever
`sync_viewport` last clamped it, and the viewport tracks the tail of live output, i.e. the
bottom. `escape` additionally sweeps `Hidden → Viewing` as a bundled side effect, so a naive fix
placed in the same function would move a hidden view's cursor for a reason the user never
triggered.

## 4. Invariants that must not regress

- **I1–I5 from this workstream** (bounded reads; no terminal bytes on the bus; key routing not
  reopened; single cadence; a poll may not supersede a user action). The blink in particular must
  not introduce a second ticker — it rides `spinner_frame`'s clock.
- **I3 — Key routing is not reopened.** `Esc` must still be forwarded as a literal byte inside
  Insert and must still leave focus; `Ctrl-]` must still send a literal escape. Cursor placement
  after `Esc` is a reader-view concern and must not consume or re-route any key.
- **The working strip's reserved row 0 stays reserved unconditionally.** The hint row is a
  separate row carved from the prompt chunk (`src/tui/app/render.rs:239-261`); changing its
  presence must not disturb `session_layout`'s `Min(10)` transcript guarantee for the working
  strip, nor gate the working strip on turn state.
- **No fabricated terminal content.** Whatever C004 shows on a cleared screen must be either
  something the process emitted or text the product labels as its own. This is the same rule the
  `clear_scrollback` doc already states for resync ("A clear must not manufacture a resync
  event").
- **Render may not move the keyboard.** Blink phase and cursor placement are presentation reads.

## 5. Scope

### In scope

- The hint row when the terminal panel is **unfocused** (narrow or remove; see Work package A).
- Cursor blink in Insert, on the existing frame clock.
- A usable prompt line for a newly created terminal and for a cleared terminal.
- Reader-cursor placement when leaving Insert.
- Box-surface viewport plumbing, already recorded as low-severity deferred work from C001, if
  the same investigation already has it open.
- Documentation of the delivered affordances.

### Explicitly out of scope

- Any change to the event loop's arming or to the derived mode answer. Those are C003's.
- Any change to what a key means, the key classification table, `claims_insert_keys`,
  `claims_panel_directions`, or `owns_exit_key`.
- Scrollback retention size, scrolling geometry, or the VT parser.
- Any transport, protocol, storage, or daemon change.
- Reintroducing terminal bytes on the bus to make the blink or the prompt cheaper.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

**Blink.** Make the cursor's visibility a pure function of elapsed time on the existing
`spinner.rs` clock — a sibling of `spinner_frame` in the same frame table, not a new interval.
`styled_lines` needs the phase as an input; prefer threading elapsed time in from the render path
over storing a mutable phase on the view, so two renders at the same instant agree and a dropped
tick cannot desync. Tests must be able to pin the phase rather than sample it.

Blink must be Insert-only: a reader cursor in NOR is a selection anchor and should stay steady so
`x`/`d`/yank stay predictable.

**Reader-cursor placement on `Esc`.** Move the cursor in `escape` using the same
`SelectionState` + clamp discipline `clear_scrollback` and `apply_nav` already use, clamped
against `max_row` with the existing `rows.saturating_sub(1)` precedent so a one-row screen cannot
underflow. Decide and record the rule for a multi-line output block before implementing — landing
on the block's last line and landing on the prompt line above it are both defensible and the user
should be able to see which was chosen.

Separate the `Hidden → Viewing` sweep from the focus-exit path so a hidden view's cursor does not
move for a gesture the user never made.

### Frontend or operator surface

**Hint row.** Resolve the tension in §3. Recommended: an unfocused terminal panel returns `None`,
so the row disappears and its reserved height returns to the transcript; the panel's own border
and the `[TERM]` destination badge already say a terminal is selected, and the
`Shift+←/→` gesture is discoverable from the panel-selection affordance. If a reduced
"how to get here" line is preferred, it must not name terminal verbs that no surface will honor.

**Prompt line.** Two viable options, and the plan names the decision rather than hiding it:

- **(a) Drive the shell's own clear (recommended).** Have clear send the real `clear` (or the
  canonical ED/EL sequence) through the existing typed-input path so the process repaints its own
  `PS1`. Truthful — nothing is fabricated — and correct for zsh, fish, and a custom `PS1`.
  Costs one round trip and defers to the child's own `clear` binding.
- **(b) Synthesize a prompt locally** from a detected shell name. Instant, but fabricates a line
  the process never emitted, which §4 prohibits without an explicit recorded decision, and it is
  wrong for any non-default `PS1`.

If (a) is chosen, the newly-created-terminal case should additionally keep a clearly-labelled
starting state until the process's own prompt arrives, rather than showing a fake one.

**Cursor blink and placement** are described under Runtime above; both surface in
`styled_lines`/`render_view`.

### Security and authorization

Unchanged. Option (a) routes clear through the same bounded, authorized, permissioned input path
as any other keystroke — it must not become a privileged write that bypasses the existing input
gating.

### Documentation and static guards

- `architecture/tui.md`, `docs/tui.md`, `.opencode/skills/tui/SKILL.md` together per AGENTS.md.
- No new guard script required. If the implementation adds a ticker, that is an I4 violation.

## 7. Ordered work packages

### Work package A — Decide and narrow the unfocused hint row

Intent: the row above the composer must describe whoever holds the keyboard.

Required changes:

- Record the chosen resolution (narrow to `None`, or a reduced non-verb line) and, if the
  unfocused branch is removed entirely, update the seam documentation at
  `src/tui/panels/mod.rs:1519-1527` in the same change.
- Leave the other panels' behaviour untouched.

Acceptance evidence:

- A test asserting the hint row is absent above the composer immediately after a `Shift+←/→`
  focus swap away from the terminal panel, and present again when the panel is re-focused.
- A layout test asserting `prompt_height` returns the row's height to the transcript on the swap
  — the reflow is intended here and must be pinned, not incidental.
- Every other panel's hint behaviour unchanged, asserted explicitly so a shared-seam edit cannot
  silently change them.

### Work package B — Insert-mode cursor blink on the existing clock

Intent: the user can tell at a glance that the PTY owns the keyboard.

Required changes: as described under Runtime. Sibling pure `blink_visible(elapsed)`-style
function beside `spinner_frame`; elapsed time threaded into the render path; blink suppressed
outside Insert.

Acceptance evidence:

- Unit tests pinning both phases at chosen elapsed values, proving the function is pure.
- A render test at two pinned phases showing the cursor cell present and absent.
- A test asserting the cursor does **not** blink in NOR (reader anchor stays steady).
- No new interval/thread in the diff, asserted by inspection of the diff and by the existing
  quick guard set.

### Work package C — A usable prompt line for new and cleared terminals

Intent: opening or clearing a terminal shows that it is ready for input.

Required changes: option (a) or (b) per §6, with the decision recorded here and in the closure
record.

Acceptance evidence:

- After clear, the surface shows a prompt line, and the test asserts it originates from process
  output (option a) or is explicitly labelled product text (option b).
- A test asserting clear does **not** reset `next_seq` and does not manufacture a resync — the
  existing `clear_scrollback` contract.
- A new terminal reaches its prompt without a keystroke, once C003 has armed the cadence.

### Work package D — Reader-cursor placement on `Esc`

Intent: leaving Insert puts the reader where the user was, not at the bottom.

Required changes: as described under Runtime, with the multi-line rule chosen and recorded; the
`Hidden → Viewing` sweep separated.

Acceptance evidence:

- Tests for 0-row, 1-row, cursor already at row 0, and `Esc` from Insert after a multi-line
  output block.
- A test asserting a hidden view's cursor is untouched by an unrelated `Esc`.
- Existing NOR navigation tests pass unmodified.

### Work package E — Documentation and roadmap reconciliation

As Work package D of C003, for the C004 contract.

## 8. Failure, cancellation, restart, and contention semantics

- **Blink and focus.** Losing Insert stops the blink; regaining it restarts from the current
  phase. No state is retained per view that could survive an attachment change.
- **Clear through the shell.** If the input path refuses (non-live link, detached, over the
  per-write bound), clear must report the typed refusal and **leave the screen intact** rather
  than performing a local wipe that the process does not know about.
- **Clear racing output.** A chunk already in flight when clear is requested must not repopulate
  the wiped screen with pre-clear text; sequence handling stays the daemon's.
- **Cursor placement racing a resize.** Placement must clamp against the post-resize geometry and
  must not underflow on a zero-height viewport.
- **Two surfaces.** Box and panel share one controller. A placement rule must be identical on
  both; the box's viewport plumbing must not make the panel behave differently.

## 9. Compatibility and migration

None. No wire, storage, configuration, or migration change. Clear may change from a purely local
wipe to a process interaction (option a); that is a behaviour change users can observe but it
introduces no compatibility surface, and the alternative — continuing to blank a live terminal —
is the reported defect.

## 10. Required tests

### Focused unit tests

- Blink phase function: pure, both phases, and stable across two identical calls.
- Hint-row presence/absence across focus transitions.
- `escape` cursor placement for each degenerate geometry.

### Integration tests

- Focus swap away and back, asserting the row and the prompt height.
- Clear through the shell producing a real prompt line.
- `Esc` after a multi-line command output.

### Restart and recovery tests

- Clear on a non-live attachment is refused and leaves content intact.

### Contention and cancellation tests

- Clear while a pull is in flight.
- Blinking while output arrives; no interference with chunk application.

### Security and negative tests

- Clear does not bypass input permissioning or the live-link gate.

### Migration and compatibility tests

None.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tui::panels::terminal
cargo test -p codegg --lib tui::interactive_terminal
cargo test -p codegg --lib tui::components::spinner
cargo test --test tui_panel_docking
cargo test --test tui_render
cargo test --test tui_terminal_lifecycle

bash scripts/verify.sh quick
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 12. Documentation updates

- `architecture/tui.md` — the terminal affordances: hint row visibility rule, blink clock, prompt
  line after clear/new, reader-cursor placement.
- `docs/tui.md` — user-facing: what to expect after `Esc`, and that clear produces a fresh prompt.
- `.opencode/skills/tui/SKILL.md` — hazards: a reserved layout row is part of the prompt
  allocation and its removal is a reflow; a blink must ride the one frame clock.
- Roadmap status table and registry.

## 13. Acceptance criteria

1. The TERM hint row is absent above the composer when the terminal panel does not hold the
   keyboard, and returns when it does.
2. In Insert the cursor blinks on the shared 100ms clock; in NOR it is steady.
3. A newly created terminal and a cleared terminal both show a prompt line without a keystroke.
4. Leaving Insert places the reader cursor on the line above the last one, or the current line if
   there is none, with the multi-line rule recorded.
5. No second timer, thread, or ticker is introduced.
6. The 157 C001 and 6 C002 terminal/panel tests plus C003's tests pass unmodified unless a change
   is recorded with its reason.
7. `verify.sh quick` passes.

## 14. Stop conditions

Stop and report rather than improvise when:

- C003 has not closed. The blink is unimplementable before it, and the hint-row change would race
  its derived-mode work.
- Resolving the hint row would require changing what a key means, or would silently alter another
  panel's behaviour.
- Making clear show a prompt requires fabricating terminal content without an explicit recorded
  decision.
- The blink appears to require a second interval. Report it; do not add one.
- Cursor placement would require changing `apply_nav`, the selection model, or the VT parser.
- Work would expand into the event loop's arming, the transport boundary, or R001.

## 15. Closure evidence required

- The recorded decision for the unfocused hint row and, if the seam documentation changed, the
  exact change.
- The recorded clear option and why; explicit statement of whether any line shown was fabricated.
- The recorded multi-line `Esc` rule.
- Requirement-to-evidence matrix for each §13 criterion.
- Invariant review for I1–I5, with explicit evidence that no second cadence exists.
- Exact verification commands run, local vs CI labelled truthfully.
- The unblock audit.

## 16. Handoff notes

- Do not start this milestone until C003 closes. The first symptom a user would otherwise see is
  a cursor that blinks only while they happen to press a key.
- `src/tui/panels/mod.rs:1519-1527` documents why the unfocused hint branch exists. Overturning it
  is a deliberate contract change; update the doc in the same commit or the next agent will
  restore the branch.
- The blink must be a pure function of elapsed time for the same reason `spinner_frame` is:
  reproducibility under `TestBackend` and no drift when a render tick is coalesced. A mutable
  frame counter on the view is the specific wrong answer.
- `clear_scrollback` deliberately preserves `next_seq` so the next honest chunk does not fail its
  gap check. Do not "fix" the blank screen by touching the sequence cursor.
- Prefer the honest clear (option a) unless the user explicitly chooses otherwise; synthesizing a
  prompt is a visible product decision, not an implementation detail.