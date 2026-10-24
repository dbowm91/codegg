# TUI Composer Helix Keybinding Parity — Second Post-Closure Corrective Addendum

Status: proposed — C001, C002 and C003 planned; none started

Baseline: `c2d781319` (`origin/main`, the merge of pull request #182)

Parent roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md`

Historical milestones preserved (all closed, none reopened by this addendum):

- M001 closed: `plans/closure/tui-composer-helix-parity/001-status.md`
- M002 closed: `plans/closure/tui-composer-helix-parity/002-status.md`
- M003 closed: `plans/closure/tui-composer-helix-parity/003-status.md`
- M004 closed: `plans/closure/tui-composer-helix-parity/004-status.md`
- M005 closed: `plans/closure/tui-composer-helix-parity/005-status.md`
- M006 closed: `plans/closure/tui-composer-helix-parity/006-status.md`
- M007 closed: `plans/closure/tui-composer-helix-parity/007-status.md`
- C001 closed (the first corrective): `plans/closure/tui-composer-helix-parity-corrective/001-status.md`

Long-term references:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-session-tui`
- `plans/003-planning-process.md` (§7, "Corrective passes")

Related ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19, with the
  `space`-chord amendment of 2026-10-19 and the wrapped-visual-row amendment of 2026-10-23) —
  governs the composer's claim set and the `j`/`k` condition. **C003 amends it again**, by a dated
  section appended to the accepted record, following the precedent its own 2026-10-23 amendment
  set. The Decision text is not edited.

Peer workstreams whose published text this addendum changes (their milestones stay closed):

- `plans/subsystems/tui-composer-text-layout-roadmap.md` — owns the composer's word wrap, its
  visual-row geometry and the app-level page routing. M002 of that roadmap is the plan that
  recorded the `Ctrl+u`/`Ctrl+d` ownership on the *overflow* condition; C003 changes that
  condition and does not reopen the milestone.

## 1. Corrective purpose

The composer workstream closed with its verbs implemented, its claim table in place and its
guard armed. Four properties of the **composer's caret, its selection and its vertical key
ownership** are nonetheless wrong, and three of them are wrong in a way the user sees on the
first keystroke.

The four findings, in the order the user reported them:

1. **The caret does not advance on a space.** Typing a space at the end of the composer buffer
   inserts the byte and advances the caret **in the model**, but the caret's *drawn cell* does not
   move: the wrap drops the trailing whitespace, so the space's byte is on no cell, and the
   caret's column falls back to "one column past the last character before it" — the same column
   it already had. The space becomes visible only when the next word arrives, at which point the
   caret jumps two columns.
2. **The Normal-mode caret is nearly invisible.** The block is painted with an explicit
   foreground/background pair *and* `Modifier::REVERSED`. `REVERSED` is SGR 7: the terminal
   swaps the two colours, so the block's effective background is the composer's own background
   colour. On a blank cell — the caret's usual cell at the end of the text — there are no visible
   pixels at all.
3. **The composer's selection is not the editor's selection.** The editor has a third mode
   (`EditorMode::Select`), a word motion that *selects* the word it lands on (`e`, `b`), and a
   rule that a movement outside select mode **drops** the held selection. The composer has none
   of the three: `v` arms a one-shot character selection, `e` and `b` only extend a selection
   that already exists, and a movement in Normal mode extends the held selection instead of
   dropping it.
4. **The vertical keys follow the box, not the text.** `j`/`k` and `Ctrl+u`/`Ctrl+d` belong to
   the composer only when its content **does not fit its box**. A composer holding three lines
   that fit is, to the user, a composer they are using — and its `j`/`k` scroll the transcript
   instead.

The first corrective (C001 of `tui-composer-helix-parity-corrective`) corrected a false premise
about the binding maps. This one corrects four behaviours.

## 2. Discovered corrective findings

### F1 — The caret does not advance on a space

**Evidence.**

| Site | Fact |
|---|---|
| `src/tui/wrap.rs:615-618` | The wrap's contract: *trailing whitespace is dropped*. |
| `src/tui/wrap.rs:481-488`, `:514-515` | A whitespace run is parked in `space` and flushed with no word following it, so a run at the end of the buffer is never attached to anything and is dropped. |
| `src/tui/wrap.rs:659-671` | A run is drawn only when a **word follows** it: the run becomes the single space that separates two words on one row. |
| `src/tui/composer_geometry.rs:176-196` | `VisualLayout::caret_position`: a byte on a cell is that cell; a byte just after a line break is the next row's column 0; **any other byte is "one column past the last character before it"**. |
| `src/tui/composer_geometry.rs:219-235` | `last_cell_before` returns the last cell that *starts* before the byte, and the fallback returns `cell.column + cell.width`. |
| `src/tui/components/prompt.rs:603-645` | `caret_cell` / `caret_cell_in` read that position; the column is clamped to the box's last column. |
| `src/tui/app/render.rs:2041-2044` | The only `set_cursor_position` in the tree: Insert mode puts the terminal's own cursor on that cell. |

**The measured consequence.** For the buffer `"hello "` (six bytes, the last one a space) the
wrap produces one row whose cells cover bytes `0..5`; byte 5 — the space — is on no cell. The
caret at byte 5 is at column 5 and the caret at byte 6 is **also** at column 5. One keystroke,
zero columns of movement. Typing the next word then draws the space as the row's separator
(`wrap.rs:663-669`), so the caret jumps two columns at once.

**Why the original verification did not catch it.**

- The **model** is correct. `ComposerText::insert` advances the caret by the inserted byte
  length (`src/tui/composer_text.rs:487-493`), so every model-level test passes: the caret byte
  moves, only the caret *cell* does not.
- The geometry unit tests prove the mapping over texts that include `"trailing spaces   "`
  (`src/tui/composer_geometry.rs:339`) — but only the **cell** bijection
  (`the_mapping_is_a_bijection_over_widths_and_texts`, `:352`) and the **line-break** caret
  (`:492`). No test asserts the caret's column for a byte after dropped whitespace.
- The render tests (`tests/tui_render/tui_composer_cursor.inc`) compare the caret cell with the
  text column, and no test types a space.

### F2 — The Normal-mode caret is nearly invisible

**Evidence.**

| Site | Fact |
|---|---|
| `src/tui/components/prompt.rs:1007-1018` | The block is painted `fg(theme.background)`, `bg(theme.primary)`, `add_modifier(Modifier::REVERSED)`. |
| `src/tui/components/prompt.rs:646-655` | The widget paints a block only when its cursor style is `Block`; whether that style is in force is decided by the caller. |
| `src/tui/app/render.rs:2027-2033` | The block is painted only when the composer holds the keyboard and the resolved mode is Normal. |
| `src/tui/app/render.rs:2037-2045` | In Normal mode no terminal cursor is positioned, so ratatui hides it: the painted block **is** the caret. |
| `src/tui/components/editor.rs:1809-1819` | The editor's caret, for contrast: `fg(primary)` + `UNDERLINED`, patched onto the base style — visible. |

`Modifier::REVERSED` is SGR 7, and a terminal that honours it swaps the cell's foreground and
background. The cell therefore ends up drawn as `fg(primary)` on `bg(theme.background)` — the
composer's own background. On a **blank** cell (the caret at the end of the text) the cell's
symbol is a space, so the painted "block" is a background-coloured rectangle on a
background-coloured box: no pixels change.

**Why the original verification did not catch it.** The render test
`the_composer_paints_a_block_in_normal_mode` (`tests/tui_render/tui_composer_cursor.inc`) asserts
the `REVERSED` **bit** and never the colours. An invisible block satisfies that assertion
exactly.

### F3 — The composer's selection is not the editor's selection

**Evidence — the editor, which is the reference.**

| Site | Fact |
|---|---|
| `src/tui/editor.rs:1660-1667` | `EditorMode::{Normal, Insert, Select}` — a surface-local third mode. |
| `src/tui/editor.rs:1788-1829` | `apply_motion_selection`: in **Select** mode the selection follows the cursor; with nothing held, a word motion selects the word it landed on and any other motion starts a selection at the cursor and grows it. In **Normal** mode a movement **drops** the selection, and the word motions (`WordEnd`, `WordBackward`) are the ones that **create** one. |
| `src/tui/editor.rs:1770-1776` | `selected_word_bounds` — only `WordEnd` and `WordBackward` select a word; `WordForward` does not. |
| `src/tui/commands/editor.rs:4010-4018` | `v` is a **mode** (`HelixVerb::SelectChar`), `x` is an action. |

**Evidence — the composer, which is the subject.**

| Site | Fact |
|---|---|
| `src/tui/composer_text.rs` (`move_caret` / `set_caret`) | Two caret movements: `move_caret` extends the held selection, `set_caret` re-anchors it. Neither knows the app's mode. |
| `src/tui/composer_text.rs` (`toggle_select_chars`) | `v` reaches the shared machine's `toggle_char`: on an empty set it **arms** a character selection at the caret, and on a second press it **clears** it. It does not change a mode. |
| `src/tui/composer_claims.rs:59-88` | The composer's verb list has `SelectChars` ("Helix `v` — select characters"), `SelectLine`, `DeleteSelection`, `Paste`. There is no select *mode*. |
| `src/tui/input.rs:65-70` | `InputMode` is two-valued: `Insert`, `Normal`. The composer has no third mode of its own. |
| `src/tui/app/render.rs:1910-1939` | The badge renders `[SEL]` from the **editor's** mode and `[INS]`/`[NOR]` from the composer's — one row, two surfaces, no rule about which surface the badge describes. |

**The three differences, stated as the user stated them.**

| Behaviour | Editor | Composer today |
|---|---|---|
| `e` / `b` | Select the word they land on (Normal mode) or extend it (Select mode) | Move the caret; extend a selection only if one is already held |
| `v` | Enters `EditorMode::Select` — a **mode**, visible in the badge | Arms a character selection at the caret (and clears it on a second press); the badge still says `[NOR]` |
| A movement outside select mode | **Drops** the held selection | Extends it (`move_caret`) or leaves it alone (`set_caret`) |

**Why the original verification did not catch it.** M005 closed on the *verbs* (`v`, `x`, `d`,
`p`), on the selection state the model holds, and on the model's `move_caret`/`set_caret` pair —
all of which are correct as implemented. The property that was never stated, and never tested, is
the **mode-dependent** consequence of a motion. Nothing in the model is wrong; the model is
missing the mode that decides what a motion means.

### F4 — The vertical ownership condition is "fits the box", not "is multiline"

**Evidence.**

| Site | Fact |
|---|---|
| `src/tui/composer_claims.rs:221-227` | `ClaimCondition::{Always, ContentOverflowsBox}` — the only conditional claim. |
| `src/tui/composer_claims.rs:346-355`, `:191-208` | `holds` reads the cached `ComposerFit`; `overflows()` is `row_count > visible_rows`. |
| `src/tui/app/mod.rs:4166-4191` | The gate: the composer claims a letter only when the **condition** holds against the **cached** fit, and only in the resolved Normal mode. |
| `src/tui/app/mod.rs:4418-4438` | `Ctrl+u`/`Ctrl+d` reach `PageUp`/`PageDown`, which route to `prompt.page_up()`/`page_down()` when `prompt.content_overflows()` and to the transcript otherwise. |
| `src/tui/components/prompt.rs:750-775` | The composer's page moves the window **and** the caret by a page of visual rows, so a page that fits still moves the caret. |
| `src/tui/composer_claims.rs:495-508` | `j` and `k` carry `ClaimCondition::ContentOverflowsBox`. |
| `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md:421-483` | The 2026-10-23 amendment: the condition changed from "the composer buffer has more than one line" to "the composer's content does not fit its box", so that a wrapped single line that overflows stays reachable. |
| `docs/tui.md:376-381`, `:396-402` | The published tables state the overflow condition and `v` as "Start selecting characters; a caret motion then extends the selection". |

**The gap.** The 2026-10-23 amendment answered one narrow question — *must a wrapped single line
that overflows stay reachable?* — and generalised the condition from newlines to rows. Its
rationale treats "fits the box" as the right boundary, and the implemented condition matches the
amendment's **predicate** word for word (`row_count > visible_rows`); its label and the ADR's wording differ, which is part of finding F4. But the boundary the user wants is different, and it is not a measure
question: **the presence of more than one row of composer text is the signal that the composer
is the surface in use.** A three-line composer that fits the box is multiline, and it is exactly
the state the user described as "the user is using the input box".

**Why the original verification did not catch it.** The composer-text-layout milestone verified
that the implemented condition matched the amended ADR, and the guard verified that the claim
table's conditions were known. Both are true and both are silent about which boundary is
*right*. No test asserted the behaviour the user wants, because the behaviour was a decision
rather than a requirement.

## 3. Work classification

### Invariants

- **C-I1 — The caret names a source position.** Every keystroke advances the caret by exactly one
  column, and the caret's cell is a function of the **source** — not of which characters the
  wrap chose to draw. A whitespace byte occupies a column in the source even when the wrap does
  not paint it.
- **C-I2 — A painted caret is visible.** A rendered caret differs from the surface it is painted
  on, in at least one visible attribute, on **every** cell it can land on — a blank cell
  included.
- **C-I3 — The composer and the editor share one motion rule.** The composer's
  mode-dependent motion and selection semantics are the editor's. A difference is a defect, not
  a flavour, and it is recorded as one when it exists.
- **C-I4 — The published keymap is the implemented keymap.** The composer's verbs, the condition
  under which the conditional ones are live, and their mode-dependent meanings are stated once
  and machine-checked against the tables that publish them.
- **C-I5 — One fact, one predicate.** The composer's "is it multiline" is one derived fact with
  one predicate. The claim table and the app-level page routing read that predicate; neither
  computes a second answer to it.

### Capabilities

- **C-C1 — The caret advances one column per keystroke.** Typing a space moves the caret; typing
  the next character moves it again, by one column, not two.
- **C-C2 — The Normal-mode caret is visible.**
- **C-C3 — The composer selects like the editor.** `e` and `b` select the word they land on;
  `v` is a mode, and the badge says so; a movement outside select mode drops the held
  selection.
- **C-C4 — The vertical keys belong to a multiline composer.** `j`, `k`, `Ctrl+u` and `Ctrl+d`
  move inside the composer while its content occupies more than one row, and return to the
  transcript when it does not.

### Infrastructure

- **C-N1 — The published composer keymap is guarded.** A static guard reads the claim table,
  the condition's own label and the published tables in `docs/tui.md` and `architecture/tui.md`,
  and fails when they disagree. It carries a `--self-test` that proves each rejection.

### Polish

- **C-P1 — The records state the corrected facts.** ADR-0042's `j`/`k` rows, the parent
  roadmap's decision table, `docs/tui.md` and `architecture/tui.md` describe the behaviour that
  ships.

## 4. Non-goals

- **No new Helix verb.** The claim set stays the same thirteen letters. `v` changes what it
  *means*; it does not join the set.
- **No change to the shared wrap primitive.** `wrap_styled_spans_by_word_rows` is the main
  buffer's engine too, and the transcript's trailing-whitespace policy is a separate decision.
  The correction is in the composer's geometry, not in the engine.
- **No change to the editor.** The editor is the reference. Nothing in `src/tui/editor.rs`,
  `src/tui/components/editor.rs` or the editor's own key authority changes.
- **No change to the app's mode axis.** `InputMode` stays two-valued. The composer's select mode
  is **surface-local**, exactly as `EditorMode::Select` is — a third state of a surface, not a
  third state of the app.
- **No change to the transcript's scroll.** What changes is *which surface owns the key*; the
  transcript's own page and row motions are untouched.
- **No new cadence, timer or animation.** The Insert-mode caret remains the terminal's own
  cursor; the Normal-mode caret remains a painted block.
- **No re-specification of the composer's text model, undo history or claim set.** M003's and
  M006's closed contracts stand.
- **No closure record rewritten.** The closed records are historical evidence. The corrections
  are recorded in *new* records.

## 5. Current state

- The composer's claim table holds thirteen letters; `j`/`k` are the only conditional claims, and
  their condition is the overflow of the box.
- The composer's model already carries a real selection (`SelectionState<ComposerPoint>` from
  `src/tui/helix_selection.rs`) and already distinguishes an extending movement (`move_caret`)
  from a re-anchoring one (`set_caret`). What it does not have is a mode that decides which one a
  motion uses.
- The composer's layout knows the byte of every **drawn** cell; the bytes the wrap drops are on
  no cell, and the caret's fallback column is derived from the drawn cells alone.
- The composer's Normal-mode block is painted with `REVERSED`, which cancels it.
- The published tables state the overflow condition and the current `v`.

## 6. Target architecture

- **The caret's column counts the source.** A byte the wrap did not draw resolves to the column it
  occupies in the **source**: the last drawn character's column plus its width, plus the width of
  the whitespace run the wrap dropped between it and the caret — a run is **one column**, the same
  width a drawn run has. Where a row break swallowed that run, the caret belongs to the **next**
  row (the text continues there), and the gap's **first** byte stays at the previous row's end. The
  cells the composer draws are unchanged; only the caret's position among them changes.
- **The block is a block.** The Normal-mode caret paints an explicit foreground/background pair
  and no `REVERSED`, so the terminal renders the block it was asked for — including on a blank
  cell.
- **The composer has a surface-local select mode.** `PromptWidget` carries it, the badge reports
  it, and the widget's motion methods implement the editor's rule: outside select mode a
  movement drops the held selection and the word motions select the word they land on; inside
  select mode a movement grows the selection, and with nothing held a word motion selects the
  word.
- **The badge describes the surface that holds the keyboard.** The composer's row shows the
  composer's mode when the composer holds the keyboard and the editor's mode otherwise.
- **"Multiline" is one derived fact.** `ComposerFit` grows a `multiline()` predicate over the
  measured row count; the claim table's `ClaimCondition::Multiline` and the app-level
  `Ctrl+u`/`Ctrl+d` routing both read it.
- **The published keymap is checked.** A guard reads the claim table, the condition's label and
  the two published tables and fails when they disagree.

## 7. Milestones

### C001 — The caret tells the truth

**Class:** capability (C-C1, C-C2), with invariants C-I1 and C-I2.

**Objective.** The caret advances one column per keystroke, including on a space, and the
Normal-mode block is visible on every cell it can land on.

**Dependencies.** None. M001–M007 and C001 of the first corrective are closed.

**Deliverable boundary.** The geometry change in `src/tui/composer_geometry.rs`, the caret's
style in `src/tui/components/prompt.rs`, the regression tests, and the documentation
corrections. No model change, no claim-table change, no selection change.

**Exit conditions.** See `plans/implementation/tui-composer-helix-parity-post-closure-corrective/001-the-caret-tells-the-truth.md`.

### C002 — The composer selects like the editor

**Class:** capability (C-C3), with invariant C-I3.

**Objective.** `e` and `b` select the word they land on; `v` is a mode the badge reports; a
movement outside select mode drops the held selection; a movement inside it grows the selection.

**Dependencies.** None hard. C001 shares the badge row and the composer's render path, so
landing C001 first keeps the two diffs separable.

**Deliverable boundary.** The composer's select mode, the badge's rule, the mode-dependent
motion semantics in the composer's widget and model, the tests, and the published tables. No
editor change, no new verb, no change to the app's mode axis.

**Exit conditions.** See `plans/implementation/tui-composer-helix-parity-post-closure-corrective/002-the-composer-selects-like-the-editor.md`.

### C003 — The vertical keys follow a multiline composer

**Class:** capability (C-C4), with invariants C-I4 and C-I5, and infrastructure C-N1.

**Objective.** `j`, `k`, `Ctrl+u` and `Ctrl+d` move inside the composer while its content
occupies more than one row, and return to the transcript when it does not; and the published
keymap that describes this is machine-checked.

**Dependencies.** C002's published `v` wording is part of the guard's input, so C003 lands after
C002. ADR-0042's amendment is a prerequisite of the *implementation*, not of the plan.

**Deliverable boundary.** The claim condition, the app-level page routing, the dated ADR-0042
amendment, the published tables, and the new guard with its self-test. No change to the
transcript's scroll behaviour and no change to the composer's motion set.

**Exit conditions.** See `plans/implementation/tui-composer-helix-parity-post-closure-corrective/003-the-vertical-keys-follow-a-multiline-composer.md`.

## 8. Cross-cutting requirements

- **Storage and migration.** None. No schema, no migration, no durable state.
- **Protocol and compatibility.** None. The corrections are confined to the frontend's composer
  render path, its selection semantics and its Normal-mode key routing. The daemon, the wire
  protocol and the session store are untouched.
- **Security and authorization.** None. The composer remains a frontend-local string; no submit
  path and no authority boundary changes.
- **Concurrency, cancellation and recovery.** None. No new state, no new task, no new timer.
- **Observability and audit.** None.
- **Documentation and static guards.** `scripts/check_tui_composer_key_authority.py` learns the
  new condition name in its `KNOWN_CONDITIONS` set and the guard's rules stay as they are.
  `scripts/check_tui_composer_keymap_drift.py` is **new**: it reads the claim table, the
  condition label and the published tables, and it carries a `--self-test` that proves each
  rejection. Both guards are registered in `scripts/verify.sh`'s `run_guards()` **and** in
  `.github/workflows/ci.yml`. ADR-0042 gains a dated amendment. `docs/tui.md` and
  `architecture/tui.md` state the corrected facts.
- **Performance and resource use.** The caret fix is one layout computation per frame, over a
  layout that is already cached. The new guard reads three source files at lint time.
- **Counts.** The new guard changes the *Guard scripts* and *CI guard scripts* rows of
  `architecture/overview.md`'s verified-counts table. The counts are re-measured with
  `python3 scripts/measure_overview_counts.py` and the reconciliation is recorded in
  `architecture/count_audit_history.md`.

## 9. Verification strategy

- `python3 scripts/check_tui_composer_key_authority.py` and
  `python3 scripts/check_tui_composer_key_authority.py --self-test` — green on the corrected
  tree, with the new condition known to the guard.
- `python3 scripts/check_tui_composer_keymap_drift.py` and
  `python3 scripts/check_tui_composer_keymap_drift.py --self-test` — the new guard, and the
  evidence that it is armed: it must **fail** against the pre-correction tree with the three
  vocabulary findings.
- `cargo test --test tui_render -- tui_composer_cursor` — the caret's cell and the block's
  style.
- `cargo test --test tui_render -- tui_composer_motions` — the vertical motions and the pages.
- `cargo test --test tui_render -- tui_composer_claims` — the conditional claim's behaviour.
- `cargo test --test tui_render -- tui_composer_selection` — the selection's mode-dependent
  semantics.
- `cargo test --test tui_render -- tui_composer_keymap_consistency` — the hint row and the help
  against the claim table.
- `cargo test -p codegg --lib -- composer_geometry` — the layout's unit tests, including the
  new caret-column cases.
- `scripts/verify.sh quick` — the canonical gate.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `python3 scripts/measure_overview_counts.py` and `python3 scripts/check_comment_budget.py`.

## 10. Risks and decision points

- **R-C1 — The multiline condition takes the transcript's `j`/`k` from a composer whose content
  fits.** This is the requested trade-off, and it is recorded rather than absorbed: while the
  composer holds more than one row of text, `j`/`k` move the caret and the transcript does not
  scroll. The recovery is automatic and needs no key: the composer returns to one row, and the
  keys return to the transcript.
- **R-C2 — ADR-0042 gains a second dated amendment.** The record's Decision text is not edited,
  following the precedent its own 2026-10-23 amendment set. The 2026-10-23 amendment's *reason*
  (a wrapped single line that overflows must stay reachable) is **preserved** by the new
  condition, because an overflowing composer is necessarily multiline. What changes is the
  boundary in the other direction: a composer that is multiline and **fits** now keeps the keys.
- **R-C3 — The caret fix touches a shared geometry module.** `VisualLayout` is the map the mouse's
  hit-testing reads (`PromptWidget::set_cursor_at_column` → `layout.row_cells`) as well as the
  caret's, and the selection painting reads the same rows' byte ranges. The change adds the
  dropped whitespace's width to the caret's **column** only; the cells the composer draws are
  unchanged, and the round-trip tests (`the_byte_and_the_cell_are_inverses`) must still hold.
- **R-C4 — The badge is one row for two surfaces.** A composer's `[SEL]` and an editor's `[SEL]`
  must not be confused. The rule — the badge describes the surface that holds the keyboard — is
  stated in C002 and tested on both sides.
- **R-C5 — A guard that parses markdown can find nothing and pass.** The precedent from
  `scripts/check_tui_files_keymap_drift.py` applies: a table that cannot be found is a
  **violation**, not a clean tree.
- **R-C6 — The select mode is a second place a "mode" lives.** The app has `InputMode`, the
  editor has `EditorMode`, and the composer would have a third. The composer's is
  surface-local and lives in the widget that owns it, exactly as the editor's lives in the
  editor's state.

### Decision points

| Point | Status | Owner |
|---|---|---|
| C-D1 — Is "multiline" the **rendered row** count or the **logical line** count? | **Decided: the rendered row count.** The user sees rows, not newlines: a long single line that wraps is multiline in the box, and it is exactly the content that used to be unreachable. The measure is the cached `ComposerFit`'s `row_count`, which is the same number the overflow predicate uses. | Maintainers, in C003 |
| C-D2 — Does `Ctrl+u`/`Ctrl+d` join the claim table? | **Decided: no.** The claim table is keyed by a character and its encodings, and `CONTROL` is not one of them; the chords are routed at the app layer today, and C003 changes the **predicate** both call sites read, not the routing's shape. | Maintainers, in C003 |
| C-D3 — Which surface does the badge describe? | **Decided: the surface that holds the keyboard.** The composer's row shows the composer's mode when the composer holds the keyboard, and the editor's mode otherwise. This also corrects today's behaviour, where an editor in Select mode shows `[SEL]` over the composer even while the composer holds the keyboard. | Maintainers, in C002 |
| C-D4 — Is the composer's select mode a third `InputMode`? | **Decided: no.** `InputMode` is the app's two-valued axis and every binding map branches on it. A third variant would put a surface's state on the app's axis, which is the coupling `EditorMode::Select` was created to avoid. | Maintainers, in C002 |

## 11. Completion definition

This addendum is complete when:

- The caret advances one column per keystroke, including on a space, and a regression test proves
  it against the pre-correction tree.
- The Normal-mode block is visible on a blank cell, and a regression test proves the colours,
  not the modifier bit.
- `e` and `b` select the word they land on; `v` is a mode the badge reports; a movement outside
  select mode drops the held selection — each proved by a test.
- `j`, `k`, `Ctrl+u` and `Ctrl+d` belong to a multiline composer and return to the transcript
  when it is not, each proved by a test.
- The published composer keymap is machine-checked, and the guard's `--self-test` proves it is
  armed.
- ADR-0042 records the change as a dated amendment; its Decision text is unchanged; M001–M007
  and the first corrective stay closed.
- No document states a corrected fact as false, and no document states the true fact as false.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | **ready** | `plans/implementation/tui-composer-helix-parity-post-closure-corrective/001-the-caret-tells-the-truth.md` | `plans/closure/tui-composer-helix-parity-post-closure-corrective/001-status.md` | none |
| C002 | **ready** | `plans/implementation/tui-composer-helix-parity-post-closure-corrective/002-the-composer-selects-like-the-editor.md` | `plans/closure/tui-composer-helix-parity-post-closure-corrective/002-status.md` | none — lands after C001 for a separable diff, not for a dependency |
| C003 | **ready** | `plans/implementation/tui-composer-helix-parity-post-closure-corrective/003-the-vertical-keys-follow-a-multiline-composer.md` | `plans/closure/tui-composer-helix-parity-post-closure-corrective/003-status.md` | none — lands after C002, whose published `v` wording is the guard's input |

**Reading this table.** `closed` means a closure record was accepted. `ready` means the
implementation plan is written and its dependencies are discharged. `not started` means the
milestone has not been taken. This addendum adds no milestone to the parent workstream: the
parent's M001–M007 remain closed, and this addendum corrects four behaviours of a closed
subsystem rather than re-opening any of them. The first corrective
(`tui-composer-helix-parity-corrective`) remains closed and is referenced here as history.
