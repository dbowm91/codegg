# TUI Composer Helix Parity Milestone 007 — Keymap Consistency and Guard Coverage

Status: implemented — closed at `plans/closure/tui-composer-helix-parity/007-status.md`
substantive rather than procedural (M006 takes the `a` key, and this milestone's deliverable is
derived from the claim table, so a hint row or a help section written against the 13-row table
would be rewritten by M006)

Repository baseline: branch `feat/auto-20261019-8cc543fb`, tip `3cad45929`, **plus** the
uncommitted M004–M006 work. M004, M005 and M006 close on 2026-10-19 but are not committed; the
implementing agent must confirm the M006 closure tree is in place before starting — the claim
table's `w`, `b`, `e`, `j`, `k`, `v`, `x`, `d`, `p`, `u`, `U`, `a` and `o` rows are `Live`, the
composer's hint row and the help's composer section exist, and the guard's two new rules are
present — and work from that tree.

Source roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md#m007--keymap-consistency-and-guard-coverage`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19) — the
  composer's claim set, the one-owner rule, and **part 7**, which gives the copy gesture to the
  help box the `space` chord opens. Part 7 is the governing decision for M007's help work: the
  `y` verb belongs to the composer's help and to no other box, and the box knows which box it is
  by its **route**, not its mode.

Primary class: polish (roadmap P1 and P2), with the closing consistency sweep. The **guard rules
added here are infrastructure** (N3), and the discovery work is what makes roadmap capability
**C7** true.

## 1. Objective

Make the composer's keymap **discoverable** and **consistent**, and close the remaining gaps
between the composer, the editor, and the terminal panel.

- The composer's hint row names the Normal-mode verbs that are actually available **right now**.
- The help surface lists every composer verb, and the listing is **derived** from the claim
  table so it cannot drift from it.
- The three Helix surfaces agree on selection semantics and word boundaries; any divergence found
  is either fixed or recorded as a known difference.
- The help audit and the modifier-chord audit pass with the complete claim set.

## 2. Why this milestone is blocked, and what it needs

- The roadmap's dependency line is "M004, M005, M006 closed"
  (`plans/subsystems/tui-composer-helix-parity-roadmap.md:497`). **M004 and M005 are closed**
  (`plans/closure/tui-composer-helix-parity/004-status.md`, `005-status.md`). **M006 is not** —
  its plan is written and it is `ready`, but it has not been implemented.
- The dependency is **substantive, not procedural**. M007's deliverable is **derived from the
  claim table**, and M006 takes the `a` key — the table grows from 13 rows to 14 — so a hint row
  or a help section written against the 13-row table would be rewritten by M006. The two audits
  (G10, G11) read the same table, so their expected values move with it.
- **M006's closure discharges this milestone's blocker.** When M006 closes, M007 moves to
  `ready` in the registry, in the same commit, and the plan's status line changes from `blocked`
  to `ready for handoff`.
- The claim table is otherwise complete — every letter roadmap §3 names is claimed — so M006 is
  the **last** milestone that changes it, and M007 can treat the table as final.
- The composer's model, the shared geometry and the shared selection machine all exist
  (`src/tui/composer_text.rs`, `src/tui/text_geometry.rs`, `src/tui/helix_selection.rs`).
- The guard `scripts/check_tui_composer_key_authority.py` exists with rules G1–G9 and a
  `--self-test`. M007 **extends** it; it does not replace it.

M007 is the workstream's **closing sweep**. Nothing after it: this is the last milestone in the
roadmap.

## 3. Current implementation evidence

### The hint row exists but says nothing about the composer

`App::composer_hint_text` (`src/tui/app/mod.rs:13032-13082`) is the one-line contextual key row
above the composer. Its sources, in priority order:

1. Resize mode (`src/tui/app/mod.rs:13038-13046`) — "RESIZE {subject} · h/j/k/l drag its own
   borders out · Esc/Enter exit".
2. An armed chat seed (`src/tui/app/mod.rs:13047-13052`).
3. The main buffer's turn keys (`src/tui/app/mod.rs:13088-13100`) — **only while a turn is
   running**.
4. The focused panel's own hints (`src/tui/app/mod.rs:13073-13075`), via
   `SidePanelHost::composer_hints` (`src/tui/panels/mod.rs:1516`).

When there is no turn and no focused panel, the row returns `None` and is **not reserved**
(`src/tui/app/render.rs:175`: `let hint_rows = u16::from(self.composer_hint_text().is_some());`).
So the composer's own Normal-mode verbs — the ones M004, M005 and M006 just landed — are
**invisible** outside the help box.

### The terminal panel is the precedent, and it gates on focus, not on mode

`src/tui/panels/terminal.rs:400-427`:

```rust
fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
    if !hints.focused {
        return None;
    }
    Some(if self.handle.is_none() {
        "TERM · i start a shell · v chars · x lines · d delete · space+y copy · C clear"
            .to_string()
    } else if hints.input_mode == InputMode::Insert {
        "TERM · INS typing to the shell · Esc read · Ctrl+] literal esc".to_string()
    } else {
        "TERM · i type · hjkl move · v chars · x lines · space+y copy · C clear".to_string()
    })
}
```

The row's **presence** is gated on *focus*; the **text** changes with the mode. That is the shape
P1 asks for ("matching the interactive terminal panel's hint row", roadmap `:110`), and it is
also the shape that avoids a layout jump — see §4's I5.

### The help lists no composer verb at all

`default_help_entries` (`src/tui/input.rs:1394-…`) is a static table of `HelpEntry` rows. Its
Normal-mode section contains the app's own keys (`Space` → "Show help", `g/G`, `?`, `:`, `n`,
`q`, `Ctrl+Alt+D`, `Ctrl+Alt+X`, `Ctrl+Alt+N`, `Ctrl+Alt+B`, the arrow keys, …). There is **no
composer section**: `w`, `b`, `e`, `j`, `k`, `v`, `x`, `d`, `p`, `u`, `U`, `a` and `o` are absent.

Two rows in it are about the composer and are **conditions on other surfaces**:

- `j/k` → "Navigate up/down", condition "single-line composer buffer"
  (`src/tui/input.rs:1615-1623`). This is the precedent for how a composer's conditional claim
  is documented: the row says when the key is the *transcript's* rather than the composer's.
- `a` → "Focus sidebar (agent tree)", condition "vim mode"
  (`src/tui/input.rs:1630-1635`). **M006 removes this row**, because `a` is the composer's now.

The content is built by `build_help_lines(vim_mode, active_mode)`
(`src/tui/input.rs:2140`) and `searchable_help_rows(vim_mode, active_mode)`
(`src/tui/components/dialogs/help.rs:171`), and the dialog is created by
`HelpDialog::new_with_mode(theme, vim_mode, input_mode)`
(`src/tui/components/dialogs/help.rs:165-181`), which the app builds with the route
(`src/tui/app/modal.rs`).

### The guard's rules today

`scripts/check_tui_composer_key_authority.py` enforces G1–G9: the table is closed and
single-owner (G1); a claimed letter must not reach an app-level action (G2, the fallthrough
rule); a conditional claim must use a known condition (G3); a re-spelled action is bound in both
maps (G4), is reachable by a chord in both, and has no spelling that is a claimed key (G5); a
claimed capital answers to both encodings (G6); an action whose every spelling is a chord is not
listed in `is_normal_only` (G7); a claimed key bound under `SHIFT` has its action listed in
`is_normal_only` (G8); and the two maps agree on caret motions (G9).

**Nothing in it looks at the help.** The help is where a wrong key becomes a *teaching* bug
rather than a *runtime* one, and it is currently unguarded.

### The three surfaces' shared assets

- **Selection semantics are already shared.** All three surfaces use the same state machine,
  `src/tui/helix_selection.rs`: the composer through `ComposerText::state`, the editor through
  its own `SelectionState`, and the terminal through `view.nav`
  (`src/tui/interactive_terminal.rs:1856`, `:1870`). M003 made the machine **generic over the
  point type**, so the composer and the terminal run the same rules: a character selection is
  inclusive of the head character, a line selection grows by one line per press, a deliberate
  movement extends a character selection and disarms line growth.
- **Word geometry is shared by two of the three.** The composer
  (`src/tui/composer_text.rs:421-433`) and the editor (`src/tui/editor.rs:1135-1165`) both
  delegate to `src/tui/text_geometry.rs`. **The terminal panel does not**: its `e`/`b` go to
  `InteractiveTerminalController::word_motion` (`src/tui/interactive_terminal.rs:1817-1835`),
  which computes its own targets with `next_word_end(&text, col)` and
  `previous_word_start(&text, col)` over `view.screen.row_text(row)` — the **current row's**
  text, in **column** units.

  That is a real divergence, and it has two parts:
  - **Different target.** The terminal's forward motion lands on the **end of the next word**;
    `text_geometry::word_end` lands on the end of the **current** word.
  - **Different scope.** The terminal's motion is **row-scoped** — it cannot cross a line
    boundary — while the composer's and the editor's are buffer-scoped.

  The sweep must resolve it: either the terminal moves onto the shared rule, or the difference
  is recorded with its reason.

- **Paste and undo/redo are deliberately different.** The composer has both (M005 and M006). The
  editor has its own roadmap milestones for both. The terminal has **neither**: its scrollback is
  a read-only view of a PTY, not an editable buffer. Decision **D5** already recorded the
  terminal's missing paste as its own follow-up
  (`plans/subsystems/tui-composer-helix-parity-roadmap.md:614`).

### The claim table's live set at M006

Fourteen rows, all `Live`: `b`, `d`, `e`, `i`, `j`, `k`, `o`, `p`, `u`, `U`, `v`, `w`, `x`, `a`.
`j` and `k` carry `ClaimCondition::MultiLineBuffer`; the rest carry `ClaimCondition::Always`.
`U` carries `ClaimEncodings::CapitalBoth`; the rest carry `ClaimEncodings::Bare`.

## 4. Invariants that must not regress

- **I1 — one owner per key.** Unchanged. M007 adds no claim and removes no key.
- **I2 — a consumed claim that does nothing is a dead key.** A hint row or a help row that
  advertises a verb the composer does not have is a **documentation** version of this defect, and
  it is what M007's audits exist to prevent.
- **I3 — the claim table is the one place that says who owns a key.** The hint row and the help
  must be **derived** from it. A hand-written copy of the verb list is a second source of truth
  and will drift — which is the defect M007 is called to close.
- **I4 — Insert mode types bare letters.** The hint row must not suggest otherwise, and the
  Insert-mode hint must not claim a Normal-mode verb.
- **I5 — no layout jump on a mode change.** The hint row's **presence** must not depend on the
  input mode. A row that appears on every `Esc` and disappears on every `i` resizes the
  transcript on the most frequent action in the app. The terminal panel already has the right
  shape: **focus gates the row, the mode changes the text** (`src/tui/panels/terminal.rs:416`).
- **I6 — the single redraw cadence.** No timer, no poller, no animation tick. The hint row is
  drawn in the frame that is already being drawn.
- **I7 — the badge and the cursor agree.** The hint row is a third surface describing the mode;
  it must not become a fourth answer to "which mode am I in".
- **I8 — the terminal panel's and the editor's keymaps do not change.** Roadmap §3 (`:123-124`):
  "Not a change to the editor's or the terminal panel's keymaps. Those surfaces keep their
  current behaviour exactly." **M007's sweep is allowed to *find* a divergence in the terminal
  panel's word geometry; it is not allowed to change the terminal panel's keymap.** Fixing the
  geometry is a change to the terminal's *behaviour*, which §3 forbids without evidence — so the
  default disposition of a found divergence is **record it**, and a fix needs an explicit
  decision recorded in the closure.

## 5. Scope

### In scope

- The composer's **hint row** (P1).
- The help's **composer section**, derived from the claim table (C7).
- The **consistency sweep** across the composer, the editor, and the terminal panel.
- The **help audit** and the **modifier-chord audit**, as two new rules in the composer guard,
  each with a self-test case.
- The user documentation (`docs/tui.md`), the architecture documentation (`architecture/tui.md`),
  and the TUI skill (`.opencode/skills/tui/SKILL.md`).
- The roadmap's completion definition (§11) and the workstream's close.

### Out of scope

- **Any new key, claim, or verb.** M007 adds no row to the claim table.
- **The editor's and the terminal panel's keymaps** (roadmap `:123-124`). The sweep compares and
  records; it does not re-bind.
- **A configurable keymap** (roadmap `:125-126`).
- **The daemon protocol, the turn submission path, the prompt queue, the human-shell staging
  model** (roadmap `:127-128`).
- **The composer model, the shared geometry, the shared selection machine.** M007 consumes them;
  it does not change them. A divergence in the terminal panel's word geometry is **recorded**,
  not fixed here (§5's I8).
- **The transcript selection** (roadmap `:129-130`).
- **Editing ADR-0042.** An accepted ADR is immutable.
- **Closing the roadmap's decision points.** D4 and D6 close in M006. **D7** (the line start/end
  motion) stays open and is recorded as still open.

## 6. Required production changes

| Area | File | Change |
|---|---|---|
| App | `src/tui/app/mod.rs` | `composer_hint_text` gains the composer's own verbs, gated on the composer holding the keyboard |
| Claims | `src/tui/composer_claims.rs` | a **derived** verb list: the live claims, their human labels, and the condition each carries |
| Input | `src/tui/input.rs` | the help's composer section, derived from the claim table |
| Help | `src/tui/components/dialogs/help.rs` | the copy gesture's row appears only in the Chord route's box |
| Guard | `scripts/check_tui_composer_key_authority.py` | rules **G10** (the help audit) and **G11** (the modifier-chord audit), each with a self-test case |
| Docs | `docs/tui.md`, `architecture/tui.md`, `.opencode/skills/tui/SKILL.md` | the composer's keymap, the hint row, the audits, the known differences |

## 7. Ordered work packages

### WP1 — A derived verb list

**Intent.** One place that says what the composer can do, derived from the claim table, so the
hint row and the help cannot disagree with it.

**Required changes.**

- `src/tui/composer_claims.rs` gains a **derived** description of each live claim: the key, the
  human label ("word forward", "select chars", …), and the condition it carries. The
  `ComposerVerb` enum already documents each verb's meaning; the derived list turns that into
  something renderable.
- The derivation must respect `ClaimStatus`: a **`Reserved`** verb is not advertised. It must
  also carry the **condition**, so a consumer can decide whether the verb is available *now*
  (which is what makes `j`/`k` correct on a single-line buffer).
- This is a **read-only** view over `COMPOSER_CLAIMS`. It does not add a claim, and it does not
  change the table.

**Acceptance evidence.**

- A test that the derived list has one entry per **live** claim and none per reserved claim.
- A test that the derived list's key set equals the claim table's live key set — the two cannot
  drift, because one is computed from the other.

### WP2 — The composer's hint row (P1)

**Intent.** The one-line row above the composer names the Normal-mode verbs that are available
**right now**, and it does not claim verbs another surface holds.

**Required changes.**

1. `App::composer_hint_text` (`src/tui/app/mod.rs:13032`) gains a composer branch. The branch is
   consulted when **the composer holds the keyboard** — the same derived fact the key chain
   uses (`App::composer_holds_keyboard`) — and its text is derived from WP1's list.
2. **The row's presence is gated on the composer holding the keyboard, never on the mode** (I5).
   The terminal panel's shape is the precedent
   (`src/tui/panels/terminal.rs:416`): focus gates the row, the mode changes the text.
   - In **Normal** mode: the composer's live verbs, with the conditional ones (`j`/`k`) shown
     **only when their condition holds** — on a single-line composer buffer, `j`/`k` are the
     transcript's, and the row must not claim them.
   - In **Insert** mode: an Insert-mode hint that is true (the composer's own verbs are not
     available in Insert mode, so they must not be listed). Something like "INS · type · Esc
     normal", in the same voice as the terminal panel's
     ("TERM · INS typing to the shell · Esc read · Ctrl+] literal esc").
3. **The row is bounded.** The renderer truncates to the row's width
   (`src/tui/app/render.rs:1898`: `let budget = usize::from(area.width).saturating_sub(4);`).
   The composer's verb list is longer than the terminal panel's, so the row must be written to
   fit a narrow terminal or it will be cut mid-word. The implementer must measure the row at a
   realistic width (80 columns) and either shorten the labels or drop the least-important verbs
   **in a defined order** — not by an ad-hoc truncation that cuts a key in half.
4. **The row composes with the existing sources, not instead of them.** The resize-mode row
   (`src/tui/app/mod.rs:13038`) and the armed chat seed (`:13047`) outrank it: they describe a
   layer that owns the frame. The turn keys and the focused panel's hints are **other** surfaces'
   verbs — when a panel holds the keyboard the composer does not, and vice versa — so the
   composer branch and the panel branch are mutually exclusive by construction, not by a
   priority guess.

**Acceptance evidence.**

- A test that in Normal mode with the composer holding the keyboard and a **multi-line** buffer,
  the hint names `w`, `b`, `e`, `j`, `k`, `v`, `x`, `d`, `p`, `u`, `U`, `a` and `o`.
- A test that on a **single-line** buffer the hint does **not** name `j`/`k` — they belong to the
  transcript there.
- A test that the row's **presence** is the same in Normal and Insert mode while the composer
  holds the keyboard — the layout does not jump on `Esc`.
- A test that the row names **no** verb that is not a live composer claim (the audit's subject,
  at the app level).
- A test that the row is truncated to the available width and never renders a partial key.

### WP3 — The help's composer section (C7)

**Intent.** Every composer verb is listed in the help, and the listing is derived.

**Required changes.**

1. `default_help_entries` (`src/tui/input.rs:1394`) gains a **composer** section, built from
   WP1's derived list. The rows are `HelpEntry`s in the Normal-mode section, with the verb's
   label as the action and the claim's condition as the `condition` field — exactly the shape
   the existing `j`/`k` row already uses (`src/tui/input.rs:1615-1623`).
2. **The copy gesture is route-dependent.** `y` is **not** a composer claim (ADR-0042 part 7),
   so it does not come from the derived list. It is the **second step of the `space` chord**, and
   only the box the `space` chord opens performs it. So:
   - The **Chord** route's box lists `space` then `y` → "copy the composer's selection".
   - The **Command** route's box (the `/help` command, the docked help panel) does **not** list
     `y` as a verb. Listing it in a box where `y` is a filter character would teach a gesture
     that does nothing there.
   - This means the content builders gain a **route** parameter:
     `build_help_lines(vim_mode, active_mode, route)` and
     `searchable_help_rows(vim_mode, active_mode, route)`
     (`src/tui/input.rs:2140`, `src/tui/components/dialogs/help.rs:171`). The dialog already
     carries the route (`src/tui/components/dialogs/help.rs:124`), so the route reaches the
     content builder rather than being guessed at the call site.
   - **The default route is the one that cannot copy** (`HelpRoute::Command`). A call site that
     forgets to pass a route therefore gets a box that does not lie.
3. **The composer's own verbs appear in both routes.** They are *documentation*: a user who
   opens `/help` is exactly the user who needs to know what `w` does. Only the `y` **verb** is
   route-dependent, because only the `y` verb is *performed* by a route.

**Acceptance evidence.**

- A test that the help contains a row for every live composer verb, with the right key.
- A test that the help contains **no** row for a reserved verb.
- A test that the Chord route's box contains the copy gesture and the Command route's box does
  not.
- A test that the pre-rendered constructor (`HelpDialog::new`) keeps the Command route's
  behaviour — it is the app's help.

### WP4 — The consistency sweep across the three surfaces

**Intent.** The three Helix surfaces agree on selection semantics and word boundaries, and any
divergence found is either fixed or recorded as a known difference.

**What to compare, and what the baseline says.**

| Dimension | Composer | Editor | Terminal panel | Baseline verdict |
|---|---|---|---|---|
| Character selection is inclusive of the head character | `ComposerText::selection_range` | `editor.rs`'s `selection_range` | the shared machine | **agree** — one machine, generic over the point type |
| Line selection spans whole lines and grows one line per press | `ComposerText::select_line` | the editor's | `toggle_line_selection` (`src/tui/interactive_terminal.rs:1868`) | **agree** — one machine |
| A deliberate movement extends a character selection and disarms line growth | `move_caret` → `move_to` | the editor's | `view.nav.move_to` (`src/tui/interactive_terminal.rs:1832`) | **agree** — one machine |
| Word boundary rule | `text_geometry::word_forward` / `word_backward` / `word_end` | the same, via `src/tui/editor.rs:1135-1165` | **its own**: `next_word_end` / `previous_word_start` over the current row (`src/tui/interactive_terminal.rs:1817-1835`) | **diverge** — different target and different scope |
| Vertical motion keeps the column where the target line allows | `text_geometry::move_vertical` | the same | the terminal's `move_cursor` scrolls at the edges | **diverge by design** — the terminal is a viewport |
| Paste | M005 | the editor's own roadmap | none | **known difference** (D5) |
| Undo / redo | M006 | the editor's own | none | **known difference** — the terminal's scrollback is read-only |

**Required changes.**

1. The sweep is **written down**, as a table in the closure record, with the verdict per
   dimension and the evidence for it.
2. The **word-geometry divergence** is resolved one of two ways, and the choice is recorded:
   - **Record it** (the default, and the only option that keeps §3's non-goal): the terminal's
     `e`/`b` are a **row-scoped reader's** motion over a PTY's screen, not a buffer motion, and
     the composer's and the editor's are buffer motions. The record states the difference, the
     reason, and that it is deliberate.
   - **Fix it** — only with an explicit decision, because it changes the terminal panel's
     behaviour, which §3 forbids without one. If the implementer believes the divergence is a
     defect, that is a **decision to record**, not a change to make quietly.
3. The **shared-asset tests** the roadmap's verification strategy already calls for
   (`:597-599`) are added: the same assertions about the shared selection machine and the shared
   geometry run against the composer's point type **and** against the terminal's, so a change to
   one surface's semantics cannot silently diverge from the other's.
4. The known differences (paste, undo/redo, vertical motion) are recorded in
   `architecture/tui.md`, so a reader does not have to re-derive them.

**Acceptance evidence.**

- A test that runs the shared selection machine's rules against both point types and asserts the
  same outcomes.
- A test that the composer and the editor agree on `word_forward` / `word_backward` /
  `word_end` for the same text and offset — the composer and the editor already share
  `text_geometry`, and the test makes that a fact rather than an inference.
- The sweep table in the closure record, with every dimension's verdict.

### WP5 — The help audit (guard rule G10)

**Intent.** The help cannot drift from the claim table.

**Rule G10.** The app's help entries must:

1. Contain a row for **every live** composer claim, naming the claim's key.
2. Contain **no** row for a **reserved** composer claim.
3. Contain **no** row that advertises a **claimed key** for a *different* action. (This is the
   class M006 hit with the `a` → "Focus sidebar" row: the key was claimed by the composer while
   the help still taught it as the sidebar's.)
4. Contain a row for a **conditional** claim only with the condition stated — the `j`/`k` row
   (`src/tui/input.rs:1615-1623`) is the model.

**Why a guard and not a test.** A test fails when someone changes the code. A guard fails when
someone changes the code **and** does not update the guard, and it runs in `verify.sh quick` and
CI without anyone remembering to run it. The composer's guard already has the `--self-test`
convention, so this is an extension of an existing mechanism rather than a new one.

**Required changes.**

- Add G10 to `scripts/check_tui_composer_key_authority.py`. The guard already parses the claim
  table (`CLAIMS_SRC`) and the binding maps (`INPUT_SRC`); it must additionally parse the help
  entries from `src/tui/input.rs`.
- Add a **self-test case**: a help that drops a live verb's row, and a help that advertises a
  claimed key for a different action, must each be **rejected**.

### WP6 — The modifier-chord audit (guard rule G11)

**Intent.** The documentation cannot teach the old bare letter for a re-spelled action.

**Rule G11.** For every action in `RESPELLED_ACTIONS`
(`scripts/check_tui_composer_key_authority.py:82-86`):

1. The help must list it under a **modifier chord** spelling — a spelling that carries a
   modifier, so it cannot be confused with the bare letter the composer now owns.
2. The help must **not** list the action under its **old bare letter** (the `RESPELLED_ACTIONS`
   value — `x`, `d`, `i`, and `a` after M006).

**Required changes.**

- Add G11 to the guard, with a self-test case: a help that lists `x` as "stop / resume agent"
  (the pre-M002 spelling) must be **rejected**; a help that lists only `Ctrl+Alt+X` must pass.

**Why the two audits are separate rules.** G10 asks "does the help describe the composer's
verbs?"; G11 asks "does the help describe the *app's* re-spelled verbs?" They read different
tables and catch different classes. One rule doing both would be a rule nobody could read.

### WP7 — Documentation (P2)

**Required changes.**

- `docs/tui.md` — the composer Normal-mode keymap, complete and **derived from the same source
  as the hint row**; the re-spelled app-level chords and their current spellings; the cursor
  rules; the hint row; the copy gesture and the route that owns it.
- `architecture/tui.md` — the composer's ownership boundary, the claim-table rule, the
  relationship to the shared selection and geometry assets, the hint row's derivation, the two
  audits, and the **known differences** table from WP4.
- `.opencode/skills/tui/SKILL.md` — the live verb list, the hint row's rule (focus gates the
  row, the mode changes the text), the two audits, and the rule that the hint row and the help
  are **derived** from the claim table.

### WP8 — The workstream's close

**Required changes.**

- The roadmap's §11 completion definition is checked, item by item, in the closure record.
- The roadmap's §12 table is updated: M007 closed.
- `plans/registry.md` is updated: the TUI composer Helix keybinding parity row goes to **closed**,
  and the milestone rows are recorded under recently-closed work.
- The roadmap's remaining open decision (**D7**) is recorded as **still open**, with the
  recommendation it already carries ("settle it with the next ADR that touches the claim set").
  A closed workstream with an open decision is a legitimate outcome **when the decision is
  recorded**; silently closing D7 would be a false closure.

## 8. Failure, cancellation, restart, contention semantics

- **The hint row is presentation only.** It reads the claim table and the composer state; it
  never mutates anything. A row that cannot be computed (no composer, no claims) is simply
  absent, exactly as today.
- **The help is content only.** A malformed derived row must not panic the dialog. The
  derivation is total: every live claim has a label, and a claim without one is a compile-time
  exhaustiveness question, not a runtime `Option`.
- **The guard is a build-time check.** A G10 or G11 violation fails `verify.sh quick` and CI. It
  must never be a runtime path.
- **The sweep changes no behaviour.** It reads code and writes a record.
- **No daemon interaction, no state, no migration.** M007 is entirely frontend-local and
  documentation-facing.

## 9. Compatibility and migration

- **No key changes.** M007 re-binds nothing. The hint row and the help are the only user-visible
  changes, and both are additive.
- **The hint row is new.** A user who has never seen it now sees one line above the composer
  naming the composer's verbs. That is the point (P1), and it is the same line the terminal
  panel already has.
- **The help gains a composer section.** The `/help` box gets longer. The filter is what makes
  that acceptable, and the section's rows are searchable (`searchable_help_rows`).
- **The `y` row disappears from the `/help` box.** It was never there — M005 gave the copy
  gesture to the `space` chord's box — so this is not a removal from the app's help; it is the
  *absence* of a row the Command route must not carry.
- **No stored data, no schema, no protocol, no configuration change.**

## 10. Required tests

**New integration test file `tests/tui_composer_discoverability.rs`** — the hint row and the
help:

1. The hint row names every live composer verb in Normal mode on a multi-line buffer.
2. The hint row does **not** name `j`/`k` on a single-line buffer.
3. The hint row's presence is identical in Normal and Insert mode while the composer holds the
   keyboard.
4. The hint row names no verb that is not a live composer claim.
5. The hint row is truncated to the available width and never renders a partial key.
6. The hint row is absent when another surface holds the keyboard.
7. The help contains a row for every live composer verb.
8. The help contains no row for a reserved verb.
9. The Chord route's help box contains the copy gesture; the Command route's does not.
10. The help contains no row advertising a claimed key for a different action.

**Shared-asset test file `tests/tui_helix_surfaces.rs`** — the three-surface sweep:

11. The shared selection machine's rules produce the same outcomes for the composer's point type
    and the terminal's.
12. The composer and the editor agree on `word_forward`, `word_backward` and `word_end` for the
    same text and offset.
13. The terminal's word motion's divergence from the shared rule is **pinned by a test** — the
    test asserts what the terminal actually does today, so a later change to either side is
    visible rather than silent.

**Guard self-test cases, in `scripts/check_tui_composer_key_authority.py`:**

- G10: a help that drops a live verb's row is rejected.
- G10: a help that advertises a claimed key for a different action is rejected.
- G10: a help that lists a reserved verb is rejected.
- G11: a help that lists a re-spelled action under its old bare letter is rejected.
- G11: a help that lists a re-spelled action under a chord passes.

**No new guard script.** The existing composer guard is extended. The editor guards are **not**
modified (roadmap `:568`).

## 11. Required verification commands

```bash
# Format and compile
cargo fmt --all --check
cargo check --workspace --all-targets --locked

# Lints (both toolchains: the repo's 1.89 and CI's stable)
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo +stable clippy --workspace --all-targets --locked -- -D warnings

# The guard, both modes
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test

# Focused tests while iterating
cargo test --lib tui::
cargo test --test tui_composer_discoverability --test tui_helix_surfaces \
          --test tui_composer_claims --test tui_composer_motions \
          --test tui_composer_selection --test tui_composer_cursor \
          --test tui_composer_undo --test tui_composer_insert_transitions --test tui_render

# Canonical gate
bash scripts/verify.sh quick

# Full gate before closure
bash scripts/verify.sh full
```

**Baseline comparison.** As in M004–M006, `verify.sh full` is not green on every machine; the
implementing agent must establish the baseline at the M006 tree and compare the final sweep
against it. The set of failures must not **grow**.

## 12. Documentation updates

- `docs/tui.md` — the complete composer Normal-mode keymap; the hint row; the re-spelled
  app-level chords; the cursor rules; the copy gesture and its route.
- `architecture/tui.md` — the ownership boundary, the claim-table rule, the shared assets, the
  hint row's derivation, the two audits, and the **known differences** table.
- `.opencode/skills/tui/SKILL.md` — the live verbs, the hint row's rule, the two audits, and the
  "derived, never hand-written" rule for the hint row and the help.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — §7's M007 block, §10's D7 row
  (still open, recorded), §11's completion definition, and §12's table. **These edits happen in
  the closure commit.**

## 13. Acceptance criteria

- **AC-M1** — The composer's hint row names every Normal-mode verb that is available right now,
  and omits the ones that are not — including `j`/`k` on a single-line buffer.
- **AC-M2** — The hint row's presence does not change with the input mode, so the transcript
  does not resize on `Esc`.
- **AC-M3** — The hint row names no verb that is not a live composer claim, and no verb another
  surface holds.
- **AC-M4** — The help lists every live composer verb, and the listing is **derived** from the
  claim table rather than hand-written.
- **AC-M5** — The `space`-chord box lists the copy gesture; the `/help` box does not.
- **AC-M6** — The three surfaces agree on selection semantics, proved by running the shared
  machine's rules against more than one point type.
- **AC-M7** — The word-geometry divergence between the terminal panel and the shared rule is
  **fixed or recorded**, with the reason, in the closure record and in `architecture/tui.md`.
- **AC-M8** — The composer and the editor agree on word boundaries, proved by a test rather than
  by an inference from a shared import.
- **AC-M9** — The help audit (G10) passes with the complete claim set, and its self-test
  rejects each of its three violation classes.
- **AC-M10** — The modifier-chord audit (G11) passes, and its self-test rejects a help that
  teaches an old bare letter.
- **AC-M11** — `docs/tui.md`, `architecture/tui.md` and `.opencode/skills/tui/SKILL.md` describe
  the composer's keymap, its ownership boundary, the hint row, and the audits.
- **AC-M12** — No new timer, poller, or animation cadence was introduced.
- **AC-M13** — The roadmap's §11 completion definition is checked item by item, and the one
  remaining open decision (**D7**) is recorded as still open with its recommendation.

## 14. Stop conditions

Report rather than improvise when:

- **The hint row cannot be made to fit** a narrow terminal without cutting a key in half. The
  row must not render a half-key; report the width that does not work and the labels that must
  be dropped.
- **The row's presence would have to depend on the mode.** That would reflow the transcript on
  the most frequent action in the app (I5). Report it rather than shipping the jump.
- **The sweep finds a divergence that is a genuine defect** in the terminal panel's or the
  editor's behaviour. Fixing it changes a surface §3 protects. Report it; a fix needs an
  explicit decision recorded in the closure.
- **The help's structure cannot carry a route-dependent section** — for example if
  `build_help_lines` turns out to be reachable from a context that has no route. Report it; the
  route's default must stay "the one that cannot copy".
- **The derived verb list turns out to need a second source of truth.** It must not. Report the
  duplication rather than introducing it.
- **The guard's G10 or G11 turns out to need a hand-maintained list** of help rows. Report it:
  a list is the drift the rule exists to prevent.
- **Any work lands in the daemon, the scheduler, the tool broker, or the turn admission path.**
  Stop. This milestone is frontend-local.
- **A second redraw cadence appears to be needed** for the hint row. Stop: the row is drawn in
  the frame that is already being drawn (I6).

## 15. Closure evidence required

A closure record at `plans/closure/tui-composer-helix-parity/007-status.md` with:

1. An executive finding.
2. A requirement-to-evidence matrix mapping **AC-M1 … AC-M13** to a named test or a named
   command's output.
3. Production implementation evidence: the derived verb list, the hint row's derivation, the
   help's composer section, the guard's two new rules.
4. Verification executed: the exact commands and their results, with **local** and **CI**
   results labelled truthfully, and the **baseline comparison** for the workspace sweep.
5. An invariant review against §4, item by item — in particular I5, the layout, with evidence.
6. **The consistency sweep's table**: every dimension, every surface, the verdict, and the
   evidence. The word-geometry divergence's disposition is named explicitly.
7. A failure and recovery review.
8. A migration and compatibility review: no key changed; the hint row is additive; the help grew
   a section.
9. A security review: the help and the hint row carry **key names and labels only** — no
   composer text, no buffer contents.
10. Documentation and operations.
11. Unresolved findings, each with a severity. **A high-severity finding blocks strict closure.**
12. **Roadmap disposition:** the workstream's §11 completion definition checked item by item;
    **D7 recorded as still open**, with its recommendation; the roadmap's §7 M007 block and §12
    table updated.
13. **Registry updates:** the composer workstream's row goes to **closed** with the closure link,
    and the milestone rows are recorded under recently-closed work.

## 16. Handoff notes

- **This is the last milestone of the workstream.** There is no M008. Anything the sweep finds
  that is not a documentation fix is a **follow-up workstream**, registered in the registry's
  deferred-work section — not a quiet extension of M007.
- **The hint row and the help are both derived.** If you find yourself writing a verb list by
  hand in either place, stop: the claim table already has it, and a second copy is the drift
  this milestone exists to remove.
- **Focus gates the row; the mode changes the text.** That is the terminal panel's shape
  (`src/tui/panels/terminal.rs:400-427`) and it is what keeps the transcript from resizing on
  every `Esc`.
- **The `y` row is route-dependent; the other composer verbs are not.** The composer's verbs are
  documentation and belong in both boxes. `y` is a **verb a route performs**, and only the
  `space` chord's box performs it.
- **The default route is the one that cannot copy.** A call site that forgets to pass a route
  gets a box that does not lie.
- **Do not touch the editor's or the terminal panel's keymaps.** §3 protects them, and the
  sweep's default disposition for a divergence is **record it**.
- **Do not edit ADR-0042.**
- **Do not commit the roadmap or registry changes with the implementation.** The status flips
  happen in the closure commit.
