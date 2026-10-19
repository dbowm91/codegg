# TUI Composer Helix Parity M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity/001-composer-keyboard-ownership-and-cursor-presence.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md#7-milestones` (M001)

Repository baseline reviewed: `61cbe9222` (branch `feat/auto-20261019-8cc543fb`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the
  implementation, the ADR amendment, the two planning documents, and this
  record. No hosted CI run is claimed.

Applicable ADRs (unchanged by this milestone):

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted**
  2026-10-19, with the `space`-chord amendment. M001 introduces no key claim,
  so it neither implements nor depends on the ADR's claim table; the ADR is
  cited here only because the milestone landed in the same change set.

## 1. Executive finding

The composer's capability boundary — *does the composer hold the keyboard,
and what does it draw when it does* — is complete, and the two facts it
depends on are now derived rather than stored.

At the baseline the composer had **no way to know whether it had the
keyboard**. `PromptWidget` carried a `focused: bool` that was `true` from
construction and never written again: `PromptWidget::blur()`'s only caller in
the whole tree was a test (`tests/tui.rs`, the `focus`/`blur` round trip). The
flag gated the composer's only caret — a reversed cell painted at the caret
whenever the buffer was non-empty — so the app showed a caret in the composer
while the user typed into a side panel, and showed **no** caret in an empty
composer even when the composer was the surface being typed into. "Show no
cursor when another surface holds the keyboard" was unimplementable as
specified, because nothing in the app recorded where the keyboard was.

That is the same defect class `App::composer_input_mode` was written to kill:
one field, read twice, both wrong. The fix is the same shape — **derive the
fact, read it once** — applied to ownership rather than to mode.

What landed:

- `App::composer_holds_keyboard` — a derived ownership fact whose order
  mirrors `App::on_key`'s priority chain.
- `App::panel_owns_keyboard` — the `panel_active` condition extracted out of
  `on_key` so the key chain and the caret rules read **one** derivation
  instead of two copies of the panel rule.
- `PromptWidget::caret_cell` — the single computation of where the caret is
  on screen, used by both the painted block and the terminal-cursor
  positioning.
- The **dead `focused` flag removed**, and with it `TuiMsg::FocusPrompt`,
  whose only effect was setting that flag to a value it already had.

The blink is the **terminal's** cursor, not the app's. The app runs one
animation cadence (the 100 ms working strip, one `ActivityPhase` enum), and a
second timer for a blinking caret is not allowed; ratatui hides the terminal
cursor whenever a frame did not position it, so "position nothing" is both the
correct and the sufficient way to show no cursor at all.

No protocol, storage, migration, authorization, or dependency change. No closed
milestone of any other roadmap was reopened or amended.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A derived helper answers "does the composer hold the keyboard" | `App::composer_holds_keyboard` (`src/tui/app/mod.rs`) | pass | It is a function of app state; there is no stored copy. |
| The helper reads the same inputs the badge's resolution reads | `composer_holds_keyboard` and `composer_input_mode` sit together and share `panel_claims_insert_keys` and `terminal_box_has_keyboard` | pass | The panel rule is now one function (`panel_owns_keyboard`) used by both `on_key` and the caret rules. |
| Agreement with the badge in every state the badge can show | `the_badge_and_the_cursor_agree_in_every_focus_state` | pass | Main buffer in Insert, main buffer in Normal, side panel focused — each in both modes. |
| No staleness after a focus change | `ownership_follows_the_focus_without_an_invalidation_call` | pass | Focus a panel, then return focus to the composer; no invalidation call exists. |
| The caret cell is computed once and used by both consumers | `PromptWidget::caret_cell`; the block paint and `Frame::set_cursor_position` both read it | pass | The block no longer computes its own position. |
| The computed cell matches the cell the old paint modified | `the_caret_cell_is_one_row_below_the_prefix_row`, `the_caret_cell_follows_the_text_column`, `the_caret_cell_is_on_the_second_line_of_a_multiline_buffer` | pass | Start, middle, end of a line, and a second line. |
| The computed cell follows the scroll offsets | `the_caret_cell_follows_the_horizontal_scroll`, `the_caret_cell_is_none_when_the_caret_line_is_scrolled_off` | pass | Horizontal scroll moves the cell; a scrolled-off caret has no cell at all, where the old paint clamped it to the first row. |
| A `None` caret cell is handled | `the_caret_cell_is_none_for_an_empty_area`; `caret_cell` returns `Option` and every caller treats `None` as "no terminal cursor" | pass | No panic path. |
| The block is painted in Normal mode, at the caret, including an empty composer | `the_composer_paints_a_block_in_normal_mode`, `the_composer_paints_a_block_in_normal_mode_even_when_empty` | pass | The old `!text.is_empty()` guard no longer suppresses it. |
| The block is not painted in Insert mode | `the_composer_paints_no_block_in_insert_mode` | pass | The terminal's cursor is the caret there. |
| The block is not painted when another surface holds the keyboard | `the_composer_paints_no_block_when_a_panel_holds_the_keyboard` | pass | |
| The terminal cursor is positioned at the caret in Insert mode | `insert_mode_positions_the_terminal_cursor_at_the_caret`, `insert_mode_positions_the_terminal_cursor_for_an_empty_composer` | pass | Uses `Frame::set_cursor_position`; the deprecated `Frame::set_cursor` is not used anywhere. |
| The terminal cursor is unset in Normal mode and when another surface holds the keyboard | `normal_mode_positions_no_terminal_cursor`, `a_panel_holding_the_keyboard_positions_no_terminal_cursor`, `a_focused_editor_buffer_takes_the_keyboard` | pass | Ratatui hides the terminal cursor when a frame did not position it. |
| The interactive terminal box shows no composer cursor | the box is a modal, so arm 1 of the ownership derivation covers it | pass (by construction) | A dedicated fixture would need a live PTY, which the test suite does not provide; the modal arm is asserted directly by `an_open_dialog_takes_the_keyboard`. |
| The block and the terminal cursor are never both visible | `the_block_and_the_terminal_cursor_are_never_both_present` | pass | Mutually exclusive by construction: exactly one of the two branches runs. |
| The app does not call `Terminal::show_cursor` / `hide_cursor` | grep of `src/` — no such call | pass | The per-frame mechanism is the only cursor API in use. |
| The editor keeps painting its own fake cursor | `src/tui/components/editor.rs` untouched; the editor route's `buffer_focused` path is unchanged | pass | The app positions the terminal cursor only where the composer owns the keyboard. |
| No new timer, poll interval, or animation cadence | no timer, interval, or tick added in this change set | pass | The blink is the terminal's. |
| A resize recomputes the caret cell | `the_terminal_cursor_follows_a_resize` | pass | The cell is derived per frame. |
| The composer's text, caret, and scroll behaviour is unchanged | the change set touches the widget's *presentation* only | pass | Text editing, caret movement, and scrolling are untouched. |

## 3. Production implementation evidence

**Ownership (frontend).**

- `src/tui/app/mod.rs` — `App::composer_holds_keyboard` added beside
  `App::composer_input_mode`. Its arms, in order: a mounted modal
  (`focus_manager`), a focused side panel (`panel_owns_keyboard`), the
  timeline overlay (`ui_state.timeline_visible`), the editor's buffer
  (`editor_buffer_owns_keyboard`). The terminal box needs no arm of its own:
  it is a modal.
- `src/tui/app/mod.rs` — `App::panel_owns_keyboard` added, and `on_key`'s
  local `panel_active` replaced with a call to it. The extracted condition is
  byte-for-byte the condition that was inline, so the key chain's behaviour is
  unchanged; what changed is that the caret rules and the key chain now read
  one function.
- `src/tui/app/mod.rs` — `App::editor_buffer_owns_keyboard` added. The editor
  route keeps the ordinary composer editable unless the document was
  explicitly focused, so "the editor is open" is not enough.

**The caret (frontend).**

- `src/tui/components/prompt.rs` — `PromptWidget::caret_cell` added: the
  caret's cell in the composer's area, or `None` when it is not on screen. It
  accounts for the text's line structure, the vertical scroll, the horizontal
  scroll, and the composer's prefix row.
- `src/tui/components/prompt.rs` — the reversed-cell paint now calls
  `caret_cell` instead of computing its own position, and is gated on the
  composer's cursor style rather than on a focus flag.
- `src/tui/components/prompt.rs` — the `focused` field, `focus()`, `blur()`,
  and `is_focused()` are **removed**. `ComposerCursor` (`None` | `Block`) and
  `set_cursor_style` replace them.
- `src/tui/app/render.rs` — the render layer computes the ownership and the
  resolved mode once, sets the widget's cursor style from them, renders the
  widget, and then — in Insert mode only — positions the terminal cursor at
  the caret cell with `Frame::set_cursor_position(Position::new(x, y))`.

**The dead message.**

- `src/tui/app/types.rs`, `src/tui/app/input.rs`, `src/tui/app/mod.rs` —
  `TuiMsg::FocusPrompt` removed. Its only effect was
  `PromptWidget::focus()`, which set a flag that was already `true`. With the
  flag gone the message has no referent. `InputAction::FocusPrompt` keeps its
  real behaviour (opening the command palette at the start of the buffer).
  `TuiMsg` is the TUI app's internal message enum and is not part of the
  wire protocol; the crate is `0.1.0`, so the removal is a pre-1.0 API
  change.

**Tests.**

- `tests/tui_composer_cursor.rs` — new; 24 tests.
- `tests/tui.rs` — `test_prompt_widget_focus_blur` replaced by
  `test_prompt_widget_cursor_style`; `test_prompt_widget_new` updated to
  assert the derived cursor style instead of the removed focus flag.

**Documentation.**

- `architecture/tui.md` — the ownership derivation, the two caret shapes, the
  single caret-cell computation, and the badge/caret agreement rule.
- `docs/tui.md` — a user-facing "The cursor in the composer" section.
- `.opencode/skills/tui/SKILL.md` — the two invariants a future change must
  not break: ownership is derived and there is no `focused` flag, and the two
  carets are mutually exclusive with the terminal providing the blink.

## 4. Verification executed

### Commands run

```bash
cargo fmt
cargo fmt --check
cargo clippy -p codegg --lib --tests --all-targets
cargo test -p codegg --lib tui::
cargo test --test tui --test tui_main_buffer --test tui_render \
           --test modifier_chords --test tui_composer_cursor
python3 scripts/check_work_plan_repository_binding.py
scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy -p codegg --lib --tests --all-targets` | clean — no errors, no warnings |
| `cargo test -p codegg --lib tui::` | **2105 passed**, 0 failed, 0 ignored (4539 filtered out) |
| `cargo test --test tui` | **167 passed**, 0 failed |
| `cargo test --test tui_main_buffer` | **56 passed**, 0 failed |
| `cargo test --test tui_render` | **111 passed**, 0 failed |
| `cargo test --test modifier_chords` | **20 passed**, 0 failed |
| `cargo test --test tui_composer_cursor` | **24 passed**, 0 failed |
| `python3 scripts/check_work_plan_repository_binding.py` | passed (exit 0) |
| `scripts/verify.sh quick` | **passed** — "Quick verification passed", including the agent schema, core-boundary, sandbox, execution-ownership, TUI-authority, HTTP-route-disposition, audit-coverage, queue-delivery-callsites, queue-write-transaction-shape, scheduler-bypass, config-merge-coverage, no-hardcoded-models, provider wire/catalog/resilience, OpenAI endpoint, and Eggwork routing guards, plus `cargo check --workspace --all-targets --locked` |

### What was **not** run

- `scripts/verify.sh full` (clippy `-D warnings` over the whole workspace plus the
  workspace test suite and `cargo test -p codegg --features server,plugins,lsp-test-support`).
  The change set is confined to the TUI's presentation path and to the plans
  documents, and the clippy and test runs above cover every crate the change
  touches. The full sweep was not run, and no claim is made about it.
- A hosted CI run. No CI job was executed for this change set; the evidence
  above is local.

### One pre-existing test that had to change

`tests/tui.rs::test_prompt_widget_cursor_on_empty_line` asserted that the
widget paints the reversed cell at the caret. The block is now **opt-in** —
the app tells the widget which caret to draw — so the test now sets
`ComposerCursor::Block` before rendering. The test's intent (the block lands
on the *empty* second line, not the first) is unchanged and still asserted;
only the widget's new precondition was added.

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| **I1 — the badge and the caret agree** | `the_badge_and_the_cursor_agree_in_every_focus_state` | pass |
| **I2 — ownership is derived, never stored** | no `focused` field remains on `PromptWidget`; `App::composer_holds_keyboard` is a pure read | pass |
| **I3 — one caret at a time** | `the_block_and_the_terminal_cursor_are_never_both_present` | pass |
| **I4 — the caret's shape follows the resolved mode** | `the_composer_paints_a_block_in_normal_mode`, `the_composer_paints_no_block_in_insert_mode` | pass |
| **I5 — the caret cell has one computation** | `PromptWidget::caret_cell`, used by both consumers | pass |
| **I6 — no second cadence** | no timer, interval, or tick added | pass |
| **I7 — the composer's text model is untouched** | no change to the widget's editing, caret, or scroll behaviour | pass |

## 6. Failure and recovery review

- **Failure.** The render layer must not panic when the caret cell cannot be
  computed. `caret_cell` returns `Option`, every caller treats `None` as "no
  terminal cursor", and the arithmetic uses `checked_add` so an overflowing
  coordinate is a `None` rather than a wrap. A zero-sized area returns `None`
  before any arithmetic.
- **Cancellation.** Not applicable; this milestone adds no cancellable
  operation.
- **Restart.** Not applicable; nothing is persisted.
- **Contention.** The composer is single-owner and frontend-local. The only
  interaction is with the app's own key-dispatch chain, and this milestone adds
  no key handling. The ownership derivation reflects focus changes
  immediately, because focus changes are the app's own state transitions.
- **Resize.** The caret cell is recomputed per frame from the derived state,
  so a resize cannot leave the cursor at a stale position. Asserted by
  `the_terminal_cursor_follows_a_resize`.
- **Malformed input.** Not applicable; the milestone parses nothing.

## 7. Migration and compatibility review

- **Schema migration.** None. Nothing is persisted.
- **Protocol.** None. No daemon message, session state, or wire format
  changes.
- **Configuration.** None. The cursor style and the blink source are not
  user-configurable.
- **Backward compatibility.** Two changes are user-visible, and both are
  improvements rather than breaks:
  1. The composer in Insert mode now shows the **terminal's** cursor instead
     of a static reversed cell. A terminal that hides its cursor entirely
     would show no caret there; that is the terminal's choice, and the same
     trade every other TUI makes.
  2. In Normal mode the composer's caret is a solid block **at the caret**,
     and it is now shown for an empty composer too, where the old code
     suppressed the cell for an empty buffer.
  3. The caret now disappears when another surface holds the keyboard, which
     it never did before.
- **Rollback.** The change set is self-contained in five source files and two
  test files; reverting it restores the previous behaviour exactly.
- **Legacy paths.** The `focused` flag and `TuiMsg::FocusPrompt` are removed,
  not deprecated. Nothing else in the tree referenced them.

## 8. Security review

- **Authorization.** Unchanged. This milestone adds no key handling, no tool
  path, and no daemon interaction.
- **Secrets.** None handled. No credential, token, or path is read or logged.
- **Privilege boundaries.** Unchanged.
- **Denial of service.** `caret_cell` is O(text length) per frame, on the same
  order as the widget's existing per-frame line building, and it is bounded by
  the composer's own buffer. No unbounded work, no new allocation of note.
- **Clipboard.** Not touched. The copy verb ADR-0042 places in the `space`
  chord's help box lands with M005, where the composer's selection exists.
- **Audit.** No new audit event. Composer presentation is not an auditable
  action.

## 9. Documentation and operations

- `architecture/tui.md` — the ownership table, the two caret shapes, the
  caret-cell contract, and the badge/caret agreement rule.
- `docs/tui.md` — "The cursor in the composer": what the user sees in Insert,
  in Normal, and while another panel has the keyboard.
- `.opencode/skills/tui/SKILL.md` — the two invariants, stated for the next
  change to this surface.
- **Static guards.** None added. M001 adds no key claim, so the composer
  key-ownership guard belongs to M002, and it must ship **with** the claim
  table rather than after it. The existing TUI guards are untouched and
  remain green.
- **Operator diagnostics.** None. Nothing to diagnose.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The Insert caret is the terminal's cursor, so a terminal configured with a thin or underline cursor shows a thin Insert caret against a wide painted Normal block. | The two modes are distinguished by shape *and* by the badge, but the shape difference is smaller than a block-vs-block would be. | Accepted and recorded in the plan's platform note; documented in `docs/tui.md`. Not a defect to fix in the app. |
| low | A terminal configured to hide its cursor shows no caret in the composer in Insert mode. | The composer has no visible caret in that configuration. | Accepted: the app is forbidden from driving a second cadence, and a terminal that hides its cursor hides every other cursor too. |
| low | Clicking the composer while a side panel has focus sets the caret but does not move the keyboard to the composer. | The click appears to do nothing visible, because the composer shows no caret while the panel owns the keyboard. | Recorded, not fixed: moving the keyboard on a click is a behaviour change, and M001's scope is presentation. M002 owns key ownership and is the right place to decide it. |
| low | `TuiMsg::FocusPrompt` was removed from a `pub` enum in a `0.1.0` crate. | An out-of-tree consumer matching on that variant would need updating. | Accepted: the message had no effect, and `TuiMsg` is the TUI app's internal enum rather than a wire type. |

## 11. Roadmap disposition

**Milestone closed, and the next dependency may proceed.**

M002's remaining hard dependency is M001's closure, and that is now
discharged. M002's other dependency, ADR-0042's acceptance, was discharged on
2026-10-19 with the `space`-chord amendment. M003 was already eligible for
fresh planning and is unaffected.

## 12. Registry updates

- `plans/registry.md` — the M001 row moves to **closed** with this record; the
  M002 row's blocker loses its ADR clause, leaving M001 (now discharged);
  the roadmap row reads **active**.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — the M001 row of
  the milestone status table becomes **closed** with this record, and the
  M002 row's blocker becomes M001 alone.
