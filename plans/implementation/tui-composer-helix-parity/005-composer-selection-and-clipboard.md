# TUI Composer Helix Parity Milestone 005 — Composer Selection and Clipboard Verbs

Status: implemented — closed at `plans/closure/tui-composer-helix-parity/005-status.md`

Repository baseline: `3cad45929` (branch `feat/auto-20261019-8cc543fb`)

Source roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md#m005--composer-selection-and-clipboard-verbs`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19) — the
  composer's claim set, and in **part 7** the ownership of the copy gesture. Part 7 is the
  governing decision for this milestone: the composer does **not** claim `y`; the help box the
  `space` chord opens owns it, and the box must know how it was opened.

Primary class: capability (roadmap C2 and C3, editable selection).

## 1. Objective

Let the user **select**, **see**, **delete**, **copy** and **paste** text in the composer:

- `v` selects characters, `x` selects lines, and both are extendable and cancellable.
- The selection is **rendered** with the theme's selection colours.
- `d` deletes the selection.
- `space` then `y` — in the help box the `space` chord opens — copies the selection to the system
  clipboard.
- `p` pastes the clipboard's contents at the caret.

## 2. Why this milestone is **not** ready yet

- **M004 is open.** The roadmap's dependency graph lists M004 as a hard dependency of M005:
  "Selection verbs operate on the motion-derived caret and the selection model." A selection that
  can be entered but not extended is not the capability; the extension rule is a rule about
  *movement*, so the motions have to exist first.
- **M003 is closed** and supplies the rest: the selection state machine
  (`src/tui/helix_selection.rs`), the composer's point type and model (`src/tui/composer_text.rs`),
  and the deletion semantics (`ComposerText::delete_selection`).
- **The remaining work is the part ADR-0042 part 7 was written for**, and it is not blocked on
  anything: the route-carrying into the help box, the copy verb, the rendering, and the paste
  verb.

When M004 closes, this plan moves from `blocked` to `ready for handoff` in the same commit that
closes M004. **M004 is now closed**, and this plan was implemented in the same commit; see
`plans/closure/tui-composer-helix-parity/005-status.md`.

## 3. Current implementation evidence

At the baseline:

- **Four of the five verbs are already claimed and reserved.** `COMPOSER_CLAIMS`
  (`src/tui/composer_claims.rs:267-374`) holds `d` → `DeleteSelection` (`:276`), `p` → `Paste`
  (`:332`), `v` → `SelectChars` (`:353`) and `x` → `SelectLine` (`:367`), each with
  `ClaimStatus::Reserved`. The keys are consumed and do nothing.
- **`x` and `d` were re-spelled away in M002.** The turn stop/resume action is now `Ctrl+Alt+X`
  and open-diff is `Ctrl+Alt+D`, in both binding maps. The composer owning the bare `x` and `d`
  no longer costs the app those capabilities.
- **The shared selection machine already has every rule this milestone needs.**
  `src/tui/helix_selection.rs` provides `select_char` (`:213`), `select_line` (`:218`),
  `toggle_char` (`:229`), `line_select_extends` (`:242`), `grow_to` (`:250`), `move_to` (`:263`),
  `deactivate` (`:291`), `bounds` (`:106`) and `line_range` (`:99`). The comments state the
  contracts: only character mode is extended by movement; line mode's head must not follow the
  cursor; `grow_to` needs the caller to resolve "the next line" because "the next line" is
  geometry the surface owns.
- **The composer already holds a selection.** `ComposerText` holds the shared machine's
  `SelectionState` (`src/tui/composer_text.rs:240-250`), exposes `selection()` (`:288`) and
  `is_selected()` (`:293`), and implements `delete_selection()` (`:426-455`) with the editor's
  convention: a character selection is inclusive of the head character, and a line selection spans
  whole lines including their terminators.
- **The selection is not rendered.** `PromptWidget`'s render (`src/tui/components/prompt.rs:372-387`)
  splits the buffer on `\n` and draws each line as **one** `Span::styled(line.to_string(), …)`.
  There is no notion of a selected range in the widget at all. The theme already has the colours:
  `selection`, `selection_dim` and `selection_text` (`src/tui/theme.rs:57-62`).
- **The clipboard has both directions.** `crate::util::clipboard::copy_to_clipboard`
  (`src/util/clipboard.rs:4`) and `read_from_clipboard` (`:19`), both behind the default-on
  `arboard` feature. `read_from_clipboard` returns `None` when the feature is off or the read
  fails.
- **Two routes open the help box, and neither carries its identity.** `InputAction::Help` (the
  `space` chord, bound in both maps at `src/tui/input.rs:915` and `:1215`) reaches
  `TuiMsg::OpenHelpDialog` (`src/tui/app/mod.rs:4191`) → `open_dialog(Dialog::Help)`
  (`src/tui/app/input.rs:21`). The `/help` command reaches `B::Help`
  (`src/tui/app/mod.rs:5573-5576`) → `open_dialog(Dialog::Help)`. Both land on the same unit
  variant (`src/tui/app/types.rs:7`), and `open_dialog` builds the widget at
  `src/tui/app/modal.rs:314-325` from the theme, the vim flag and the current input mode.
- **The mode is not enough to tell the two boxes apart — and that is the whole reason the route
  must be carried.** `HelpDialog::handle_key` (`src/tui/components/dialogs/help.rs:316-377`)
  treats Normal mode as the verb mode: `q` closes (`:356`), `i`/`a` enter Insert (`:359`),
  `j`/`k` scroll (`:364-365`). But a `/help` box **starts** in Insert and `Esc` moves it to
  Normal (`help.rs:339-344`). So a `/help` box can be in Normal mode, where a mode-only rule
  would make `y` a verb — in a box that must not copy. Conversely a `space`-opened box can be
  switched to Insert with `Enter` (`help.rs:355`), where `y` must remain a filter character.
  ADR-0042 part 7 anticipates exactly this: "The box must know **how it was opened**.
  `open_dialog(Dialog::Help)` cannot answer that on its own, so the opening route is carried into
  the widget."
- **The dialog's key handler cannot perform the copy itself.** `open_dialog` pushes a **clone** of
  the dialog onto the focus manager (`src/tui/app/modal.rs:322-324`), and the clone is what
  receives the keys. A clone cannot mutate the app's composer. The established pattern is for the
  component to **return a `TuiMsg`** and for the app to act on it — the way `q` returns
  `TuiMsg::CloseDialog` (`help.rs:346`, `:357`).
- **The app has a transient-message surface for feedback that is not a turn failure.**
  `self.messages_state.toasts` (`ToastManager`, `src/tui/components/toast.rs:136`) offers `info`,
  `success`, `warning` and `error`. `report_turn_error` is for turn failures and must not be used
  for clipboard feedback.

## 4. Invariants that must not regress

- **I1 — the claim set is fixed.** This milestone claims no new key. `y` is **not** a composer
  claim: ADR-0042 part 7 gives the gesture to the help box, and "one gesture, one owner" is the
  reason. The table still has thirteen rows.
- **I2 — a claimed key never falls through.** `v`, `x`, `d` and `p` are consumed by the composer
  in Normal mode whether or not the verb did anything.
- **I3 — the composer is a frontend-local string.** The selection and the copied text never
  reach the daemon, the storage layer or the protocol.
- **I5 — the resolved mode is the gate.** The composer's claims stay live only when
  `App::composer_holds_keyboard()` and the resolved mode is Normal.
- **I7 — one redraw cadence.** Unchanged. The selection is painted by the widget's existing
  render; no timer, no poll, no second cadence.
- **One gesture, one owner.** `y` belongs to the help box the `space` chord opens, and to no
  other surface. The composer has no second spelling of the copy gesture.
- **The claim table is the composer's whole Normal-mode surface.** A verb that is not in the
  table is not reachable in the composer's Normal mode — with the single exception ADR-0042 part
  7 names, which lives in another surface on purpose.
- **The editor and the terminal panel are unchanged.** This milestone introduces the first paste
  verb in the tree, and it does not give the other two surfaces one.

## 5. Scope

**In scope.**

- Character selection (`v`), line selection (`x`), their extension, and their cancellation.
- The selection's rendering, using the theme's selection colours.
- Delete-selection (`d`).
- The **route-carrying** into the help box, and the copy verb (`y`) in the box the `space` chord
  opens.
- Paste (`p`) from the system clipboard.
- The feedback each verb reports.

**Out of scope.**

- **Undo and redo verbs** (`u`, `U`). The model has the history and the verbs; M006 owns them.
- **Insert transitions** (`i`, `o`, `a`). M006 owns them, including the `a` conflict recorded as
  decision **D6** in the roadmap's §10.
- **The composer's claim set.** No key is added, removed or re-spelled.
- **The editor's and the terminal panel's keymaps.** Untouched. The editor has its own paste
  milestone in its own roadmap; the terminal panel has none, which is decision **D5**.
- **Multi-selection.** One selection, held by the shared machine.
- **A clipboard register.** The system clipboard is the only register.
- **The help surface's key list.** Adding the composer's verbs to the help rows is the roadmap's
  C7 and belongs to M007's sweep or to a documentation milestone, not here. This milestone changes
  what the help box **does**, not what it lists.
- **Any change to the daemon protocol, the turn submission path, or the human-shell staging
  model.**

## 6. Required production changes

- **`src/tui/composer_text.rs`.**
  - **One selection range, used by everything.** Extract the byte range the selection covers into
    a single method on the model — char mode inclusive of the head character, line mode whole
    lines including their terminators — and have `delete_selection`, the yank, and the rendering
    all use it. Today that range is computed inside `delete_selection` (`:426-455`); if the yank
    and the renderer each compute their own, "the selection" becomes three answers to one
    question, and delete and yank can disagree about what was selected.
  - The selection verbs: `select_chars` (`v`), `select_line` (`x`), and the line-growth rule.
    `x` selects the caret's line the first time and **extends to the next line** on a second
    press while the selection is growing — the shared machine exposes exactly this through
    `line_select_extends` and `grow_to`, and the caller resolves the next line's point because
    the next line is geometry the surface owns.
  - `v` **toggles**: pressing it again discards the held character selection rather than
    re-anchoring on the caret. The shared machine's `toggle_char` is that rule.
- **`src/tui/components/prompt.rs`.** Render the selection. The buffer's lines are currently one
  span each (`:372-387`); the render must split a line into the unselected part, the selected
  part, and the rest, painting the selected part with `theme.selection` and `theme.selection_text`
  (and `theme.selection_dim` where the theme calls for it). The unselected rendering must stay
  byte-identical to today's.
- **`src/tui/composer_claims.rs`.** Flip `v` (`:353`), `x` (`:367`), `d` (`:276`) and `p` (`:332`)
  from `ClaimStatus::Reserved` to `ClaimStatus::Live`. No other field changes.
- **`src/tui/app/mod.rs`.** `run_composer_claim` gains the four verbs. The **copy** is not among
  them: it is the help box's verb, reached through a new `TuiMsg`, and the app performs it.
- **The help dialog's route.**
  - A route type distinguishing the `space` chord from the `/help` command, carried **into the
    widget** with the open request — the recommended shape is a parameter on the `Dialog::Help`
    variant (`src/tui/app/types.rs:7`), so the route travels with the request and cannot be
    forgotten at a call site. The widget never infers it and never hardcodes it.
  - `open_dialog` (`src/tui/app/modal.rs:314-325`) builds the dialog with the route and the
    starting mode.
  - `HelpDialog::handle_key` gains a Normal-mode `y` arm that is live **only** for the
    `space`-chord route, and it **returns a `TuiMsg`** rather than copying, because the focused
    component is a clone of the dialog.
  - The new message is handled by the app, which reads the composer's selection, copies it, and
    reports the outcome.
- **`src/tui/app/types.rs` / `src/tui/app/input.rs` / `src/tui/app/mod.rs`.** The two open sites
  carry their route: the `space` chord's (`src/tui/app/input.rs:21`) is the chord route, the
  `/help` command's (`src/tui/app/mod.rs:5575`) is the command route.
- **`src/tui/components/component.rs`.** The `DialogType` / `Dialog` mapping (`:95`, `:154`,
  `:337`) follows the `Dialog::Help` shape change.

## 7. Ordered work packages

**WP1 — The route.** The route type, the carrying of it through `Dialog::Help` and
`open_dialog`, and the two open sites. This is first because it is the part that is easy to get
subtly wrong: the mode is *not* a substitute for the route, and a `/help` box reaches Normal mode
with `Esc`.

**WP2 — One selection range on the model.** The extracted range method, with `delete_selection`
rewritten to use it. The delete verb's behaviour must not change; the range it deletes must be
byte-identical to today's.

**WP3 — The selection verbs on the model.** `select_chars`, `select_line` with its growth rule,
and the cancellation rule. Each is a thin call into the shared machine.

**WP4 — The rendering.** The widget paints the selected range. The unselected bytes must render
exactly as they do today, and the selection must survive a scroll and a horizontal scroll.

**WP5 — The composer's verbs.** The four claim statuses flipped, and `run_composer_claim`
implementing them: `v`, `x`, `d`, `p`. The paste verb reads the clipboard and inserts at the
caret; with no clipboard contents it is a no-op that still consumes the key.

**WP6 — The copy verb.** The help box's Normal-mode `y` arm, the new `TuiMsg`, and the app's
handling: copy exactly the selection's range, and report the outcome — a success message naming
what was copied, and a "there is nothing to copy" message when nothing is selected. The failure
case must not be silent: ADR-0042 part 7 says a silent no-op "would teach the user that the key
is dead".

**WP7 — Verify.** The model's tests, the claim tests, the cursor tests, the help dialog's tests,
the workspace gates.

## 8. Failure, cancellation, restart, contention semantics

The composer's selection is a frontend-local value with no I/O and no durable state. The two
external edges are the **clipboard** and the **help box**, and both have their own semantics:

- **The clipboard is best-effort.** `copy_to_clipboard` returns `Result` and `read_from_clipboard`
  returns `Option`. A failed write is reported, not swallowed; an empty or unavailable clipboard
  makes the paste a no-op that still consumes the key. On a build without the default-on `arboard`
  feature, `read_from_clipboard` always returns `None` and the paste verb is inert — which is a
  reason the paste verb must report rather than fail silently.
- **The selection survives the modal.** The composer does not hold the keyboard while the help box
  is open (`composer_holds_keyboard` excludes a mounted modal), so the copy reads a selection the
  user made before the modal opened. Nothing may clear the selection when the modal opens, and the
  modal must not move the caret.
- **Cancelling a selection is not an edit.** Discarding a held selection is not an undoable edit;
  only the text change is. The undo history records the deletion, not the arming.
- **Deleting with nothing selected is a no-op that still consumes the key** — the fallthrough rule
  applied to a live verb.

## 9. Compatibility and migration

- **Migration.** None. Nothing is stored, and no schema or protocol changes.
- **Compatibility.** The composer renders as before **except** that a held selection is painted.
  The widget's public API is preserved; every existing method keeps its semantics.
- **The `Dialog::Help` shape change** is internal to the TUI: `Dialog` is a frontend type, and
  the call sites are enumerated in §6.
- **The help box's other behaviour is unchanged.** The filter, the scrolling, the `q`/`i`/`a`/
  `j`/`k` verbs, and the Insert-mode typing all keep working. The route adds one verb to one
  box.
- **Insert mode is untouched.** `v`, `x`, `d` and `p` are still typed as text in Insert mode.
- **The editor and the terminal panel are untouched.**

## 10. Required tests

- **`src/tui/composer_text.rs`**: `v` arms a character selection at the caret and a second `v`
  discards it; `x` selects the caret's line and a second `x` extends it to the next line and
  stops at the last line; the selection range is inclusive of the head character in char mode and
  covers whole lines in line mode; delete and the yank read **the same** range; deleting with no
  selection changes nothing and returns false.
- **`src/tui/components/prompt.rs`**: the rendered buffer paints the selected range with the
  theme's selection colours; the unselected text renders as before; a selection that spans a
  newline is painted on both lines; the placeholder path is unaffected.
- **`tests/tui_composer_claims.rs`**: the four claims are `Live`; the Insert-mode sweep over the
  claim set still passes; `d` with no selection changes nothing and does not reach an app-level
  action; `v`/`x`/`d`/`p` are consumed by the composer in Normal mode.
- **The help dialog's tests** (`src/tui/components/dialogs/help.rs`): in the `space`-chord route,
  `y` in Normal mode returns the copy message; in the `/help` route, `y` in Normal mode does
  **not** — including after `Esc` has moved a `/help` box into Normal mode, which is the case the
  mode alone cannot distinguish; in Insert mode, `y` is still a filter character in both routes.
- **The copy's own tests**: the copied text is exactly the selection's range and nothing else;
  with no selection the verb reports and changes nothing; the clipboard write failure is reported.
- **`tests/tui_composer_cursor.rs`**: the caret and the selection agree, and the caret cell is
  still painted once.
- **`scripts/check_tui_composer_key_authority.py`**: unchanged and passing, plus its
  `--self-test`. The claim set did not change, so the guard's verdict must not change.
- The editor's suites and the terminal panel's suites, unchanged and passing.

## 11. Required verification commands

```bash
cargo test --lib tui::composer_text
cargo test --lib tui::helix_selection
cargo test --lib tui::components::prompt
cargo test --lib tui::components::dialogs::help
cargo test --test tui_composer_claims --test tui_composer_cursor --test tui
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test
scripts/verify.sh quick
```

## 12. Documentation updates

- `src/tui/composer_claims.rs` — the module doc's account of which verbs are live, and the
  comments on the four flipped rows.
- `src/tui/composer_text.rs` — the module docs for the selection verbs and for the single
  selection-range rule.
- `src/tui/components/dialogs/help.rs` — the module docs for the route, and why the route and
  not the mode decides whether `y` is a verb.
- `architecture/tui.md` — the composer's selection, the copy gesture's two steps, and the paste
  verb.
- `docs/tui.md` — the user-facing description of the composer's selection and clipboard verbs,
  including the `space` then `y` gesture.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — M005's status and the §10 decision
  table (**D5**, the paste scope).
- `plans/registry.md` — M005's row and the dependency audit.

## 13. Acceptance criteria

- **AC-M1** — pressing `v` in Normal mode arms a character selection at the caret, a movement
  extends it, and a second `v` discards it; the same is true of `x` for whole lines, which
  extends a line at a time and stops at the last line.
- **AC-M2** — the held selection is **rendered** with the theme's selection colours, and the
  unselected text renders exactly as before.
- **AC-M3** — the selection has **one** byte range: delete removes it, the copy copies it, and
  the renderer paints it, and the three agree.
- **AC-M4** — deleting with nothing selected changes nothing and still consumes the key.
- **AC-M5** — the copy is reached by pressing `space` and then `y` in the help box that opened,
  and it copies exactly the selected text and nothing else to the system clipboard.
- **AC-M6** — the `/help` command's box does **not** copy — including after `Esc` has moved it
  into Normal mode, and including in the `space`-chord box after `Enter` has moved it into
  Insert mode.
- **AC-M7** — the copy reports its outcome in both cases: a success message naming what was
  copied, and a "nothing to copy" message when nothing is selected.
- **AC-M8** — pressing `p` inserts the clipboard's contents at the caret, and is a no-op that
  still consumes the key when the clipboard is empty or unavailable.
- **AC-M9** — the composer is the only surface with a paste verb; the editor and the terminal
  panel are unchanged, and their suites pass.
- **AC-M10** — the claim table still has thirteen rows, the four verbs are `Live`, `y` is **not**
  claimed, and every claimed letter is still typed as text in Insert mode.
- **AC-M11** — the workspace gates are green: `verify.sh quick`, clippy on the repository's
  minimum toolchain and on the CI toolchain, and the composer guard with its self-test.

## 14. Stop conditions

Report rather than improvise if:

- **The copy verb cannot be carried into the help box** without a second copy of the dialog's
  state. The focused component is a clone, so the copy has to be a message the app handles; a
  design that mutates a clone's copy of the composer cannot work, and a design that gives the
  dialog a reference to the app breaks the component boundary.
- **The route cannot be carried** without changing `Dialog` in a way that reaches outside the
  TUI. `Dialog` is a frontend type; a change that reaches the protocol is a different milestone.
- **The selection needs a second selection state or a second caret.** One selection, held by the
  shared machine; a second copy is the drift class M003 removed.
- **The rendering cannot keep the unselected bytes byte-identical.** The existing rendering is the
  specification for the unselected text; a change to it is a change to what the user sees, and it
  has to be said out loud.
- **The paste verb would have to be added to the editor or the terminal panel** to be coherent.
  That is decision **D5**, and it is the maintainers' to make.
- **The selection cannot survive the help box opening**, or the modal would have to move the
  caret. The copy reads a selection the user made before the modal opened.
- **Any verification command fails for a reason this milestone did not cause**: report the failure
  and its evidence rather than widening the change.

## 15. Closure evidence required

- The focused test runs, with their counts.
- `scripts/verify.sh quick` output.
- The clippy gate on both the repository's minimum toolchain and the CI toolchain.
- The composer guard's output and its `--self-test` output.
- A requirement-to-evidence matrix mapping AC-M1…AC-M11 to the evidence above.
- The **D5** decision recorded with its reasoning, and the registry update and dependency audit.

## 16. Handoff notes

- **The copy gesture is two keys and a menu, and that is the recorded price.** ADR-0042's
  amendment rejected re-spelling `space`, so the copy verb lives in the help box the chord opens.
  The help box therefore has to name the verb and report its outcome — a silent no-op teaches the
  user that the key is dead.
- **The route is not the mode.** A `/help` box reaches Normal mode with `Esc`, and a `space` box
  reaches Insert with `Enter`. If the verb is decided by the mode, one of those two boxes copies
  when it must not, or filters when it must copy. This is the specific reason ADR-0042 part 7
  requires the opening route to be carried into the widget.
- **The component that receives the key is a clone.** The copy has to be a message the app
  handles; the dialog cannot reach the composer.
- **One selection range.** Delete, copy and render must agree about what the selection covers,
  or "copy" and "delete" stop meaning the same thing.
- **Nothing here touches the daemon, storage, or the protocol.** The composer stays a
  frontend-local string, and the clipboard is the only register.
