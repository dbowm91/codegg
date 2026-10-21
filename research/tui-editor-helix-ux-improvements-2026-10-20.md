# TUI Editor & Files Panel — Helix UX Improvement Research

Date: 2026-10-20 (revised 2026-10-20 after three decisions — see §0.1)
Scope: research only — no code written, no files changed in the product tree.
Method: direct reading of the CodeGG source at `a588a264`, cross-checked against
upstream Helix (keymap, command-line docs, `helix-core/src/movement.rs`,
`helix-view/src/editor.rs`).

Every claim below is anchored `path:line` in the CodeGG tree or a URL for Helix.

---

## 0.0 Implementation status (2026-10-20)

This research was **implemented**. The roadmap is
`plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M001–M014 the
editor half, M015–M020 the files half).

Three findings changed shape between the research and the implementation, and
the plan records the implemented contract rather than the researched one:

1. **The `e` landing position is one past the word's last character in
   upstream Helix, and on the word's last character here.** The research
   traced upstream's `helix-core/src/movement.rs` to "the exclusive end of the
   run". CodeGG's cursor rests *on* a character, and its existing documented
   contract for `word_end` was "the last byte of the word", so the
   implementation keeps that and selects the word it landed on. The
   **category model** and the **always-advance** rule are Helix's, and those
   are what fixed the stall.
2. **The word classification is three-way, not two-way.** `classify_byte` sorts
   bytes into word content, punctuation, and whitespace, because a two-way
   predicate has nowhere to put punctuation and reads it as a separator. The
   research named the defect ("gets stuck on symbols") and the fix is the
   three-way split.
3. **The `:` match list is a floating overlay, not an allocated row.** The
   research preferred the conditional-`CHROME_ROWS` variant and named the
   hazard — an allocated row changes the text area's height, which re-resolves
   the viewport and can move the cursor's screen row under the reader's first
   keystroke. The implementation took the constant-height variant and drew the
   match list over one row of text instead.
4. **The bottom row's right slot is a union fitted by rank, not a
   single-priority slot.** The research's B2 proposed one slot showing the
   highest-priority signal alone — pending prefix, then find/history note,
   then status/notice, then the persistent cluster. The implementation shows
   them **together**, ordered `[persistent][disclosure][notice][prefix]`, and
   decides what to drop when the row is too narrow by a **rank**
   (`ClusterWeight` in `src/tui/components/editor.rs`) rather than by
   position. Two reasons. A priority slot makes the scope badge *disappear*
   every time a prefix is armed, and the scope badge is the one thing on
   screen that says whether `:wq` reaches this project or every project on
   the machine — a safety signal cannot flicker. And the armed prefix is the
   one element that comes and goes on its own, so it belongs at the **far
   right**, where nothing else competes for it. The armed prefix being last
   also means a whole-row text search cannot find it: the scope badge
   (`[project]`) and the writable marker carry the same letters as the `t`,
   `f`, `m`, and `[` indicators, so the indicator is read out of the frame's
   **styles** instead.

Two items were **deferred**, and the roadmap's non-goals say so:
- A **count prefix** (`123G`). `:50` and `:goto 50` answer the same intent and
  were built; the general count grammar is separate work.
- A **configurable** `editor.scrolloff`. The value is a documented constant
  (`editor::SCROLL_OFF`, Helix's default of 5).

One open question was **answered by the implementation**: the research's
"mechanism that moves keyboard ownership from the `/files` tree to the primary
view" is `app.ui_state.tree_focused` plus
`commands::file_tree::set_focus(app, false)` and
`commands::editor::enter_editor_normal(app)`, and the handoff runs on the
**open's success path** rather than at the request.

The research's own §C3.5 file map was **wrong** about where the file-tree pane
is rendered: the pane renderer is `src/tui/components/file_tree.rs`, and
`src/tui/file_tree.rs` is the tree model and walker.

### The research's milestones and the roadmap's

The research numbered the work `E1`–`E14` (editor) and `F1`–`F6` (files
panel). The roadmap renumbered it `M001`–`M020`, in the same order and with
the same content:

| Research | Roadmap | | Research | Roadmap |
|---|---|---|---|---|
| E1 | M001 | | E9 | M009 |
| E2 | M002 | | E10 | M010 |
| E3 | M003 | | E11 | M011 |
| E4 | M004 | | E12 | M012 |
| E5 | M005 | | E13 | M013 |
| E6 | M006 | | E14 | M014 |
| E7 | M007 | | F1–F6 | M015–M020 |
| E8 | M008 | | | |

## 0. Executive summary

| # | Requested | Current behavior | Root cause | Verdict |
|---|---|---|---|---|
| 1 | `e`/`b` skip words continuously, select the word, handle symbols | `e` **stalls** on a word's last char; punctuation is a separator, not a word; no selection | `word_end` returns the same offset when already at a word end; `is_word_byte` has no punctuation category | **Real defect + parity gap** |
| 2 | Page/scroll like Helix (5-line margin, cursor 6 above bottom, page holds) | Minimal-scroll only; cursor parks on the *last* row; `PageDown` == half page | `reveal_cursor` / `resolve_viewport` have no scrolloff; `PageDown` aliased to `HalfPageDown` | **Real defect** |
| 3 | `x` on an empty line highlights the line | Selection computed but **invisible** | Empty line has zero content cells; the single cursor cell is painted over the selection | **Real defect** |
| 4 | `SEL` mode; movement deselects unless SEL | No SEL mode; `x` leaves a stale highlight after moving | `EditorMode` has only `Normal`/`Insert`; `move_to` never clears | **New capability** |
| 5 | `:50` jumps to line 50 | Unknown command | No goto-line in the `:` registry; parser assumes a name | **New capability** (Helix-native) |
| 6 | Scrollbar | Column **reserved**, nothing drawn | `text_columns` subtracts 1 for a scrollbar that is never rendered | **Half-built** |
| 7 | Top line empty; one bottom line, path only; drop ln/col | Top line carries path·mode·status; 2 bottom rows carry ln/col + writable + LSP notes | `CHROME_ROWS = 2` + block title | **Layout change** |
| 8 | `:` prompt on the very bottom, matches above, path erased | Prompt on the *upper* bottom row, matches on the lower | Row roles inverted vs. request | **Layout change** |
| 9 | Composer: keep only the badge above the input; drop panel name | Hint row shows `COMP · NOR · <keys> · space help` | `composer_hint_row` hardcodes `COMP`/`NOR` | **De-duplication** |
| 10 | Editor: no key list / space-help line; `space` → bottom-right keymap box | Editor has no such line; the *composer* hint row shows keys; `space ?` opens the editor palette | Wrong surface targeted; no bottom-right overlay precedent | **Scope correction + new overlay** |
| 11 | `/files`: scrolling/`Ctrl+d` must not leave the page | Can scroll far off-page | `reveal_selected` clamps to `total-1`; `ensure_visible` no-ops when height is 0 | **Real defect** |
| 12 | `/files`: INS mode with live filter, NOR keeps filter | None | No insert/filter mode on the panel | **New capability** |
| 13 | `/files`: arbitrary nesting depth via `h`/`l` — **peek mode** | `l` re-roots at the directory (parent vanishes) | `descend_to` → `set_root_prefix` re-roots; siblings are lost | **Semantics change (decided)** |
| 14 | `/files`: Enter on a file → select the editor panel in NOR | Opens the file; focus stays on the tree | `activate_selected` never moves focus | **New capability** |

**Governance note:** `plans/subsystems/tui-editor-helix-parity-roadmap.md` is **closed**
(M001–M013, M015–M020; M014 blocked on ADR-0034). This work is a *new* roadmap
milestone set, not an amendment to a closed one. `plans/subsystems/file-picker-rework-roadmap.md`
is the live home for the `/files` half.

---

## 0.1 Decisions taken (2026-10-20)

These three were asked and answered; the plan below is written to them.

**D1 — the bottom row keeps its status cluster.** The single bottom line of the
editor is not "path only". It is **path on the left**, and on the **right** a
status cluster that keeps every signal the current readout row carries:

- the **modified** indicator (moved down from the top title, not deleted),
- **writable / read-only**,
- the **LSP semantic-coverage** note,
- the **line-too-long** note.

Only the **line number** and **column** are dropped, because those are already
derivable from the cursor and were the pure noise. The one-line budget is
honored by *right-aligning* the cluster, not by deleting information. See B2.

**D2 — `/files` `l` becomes a peek motion.** `l`/Enter on a directory no longer
re-roots the pane at that directory (which made the parent and all siblings
vanish). It now **expands the directory in place and moves the cursor into it**,
keeping the parent and the siblings visible. When the listing window runs out —
the directory is at the deepest relative depth the daemon's 3-level listing can
serve — the motion instead **slides the scope**: the pane's scope becomes the
directory's **parent**, the selected directory stays selected and is **expanded
in place**, and everything above the parent is dropped. That slide is the
"enter the parent directory and expand the selected directory" case. See C3.

**D3 — `h`/Backspace is "up one level", uniformly.** It is the mirror of `l`:
the cursor moves to its parent row and that directory collapses; when the
cursor is already on a top-level row, the **scope slides up** one level instead
(the old scope root becomes an expanded top-level row and the cursor lands on
it). At the workspace root it collapses the selected directory in place, so it
is never a dead key. The pair reads as a horizontal Helix motion: `l` goes
right (deeper), `h` goes left (shallower). See C3.

**Consequence of D2/D3 worth flagging:** the scope can now be pushed arbitrarily
far down while the *relative* depth of the view stays within 0–2. `display_depth`
(`src/tui/app/state/file_tree.rs:336`) clamps the **absolute** depth at
`MAX_INDENT_DEPTH = 8`, so deep scopes flatten — the user slides three
directories down and the tree stops indenting. The fix that fits peek is to
render the indent **relative to the scope** and let the scope path carry the
absolute position. See C3.4.

---

## Part A — Editor motions and scrolling

### A1. Word motions `e` / `b` / `w`

**Current implementation.** Word classification is a single predicate:

- `src/tui/text_geometry.rs:112` — `is_word_byte(byte)` = ASCII alphanumeric, `_`,
  or any byte `>= 0x80` (UTF-8 continuation/lead bytes).
- `src/tui/text_geometry.rs:192` `word_forward` — skip non-word bytes, land on the
  next word's **first** byte.
- `src/tui/text_geometry.rs:216` `word_backward` — walk back over word bytes to the
  **first** byte of the previous word.
- `src/tui/text_geometry.rs:249` `word_end` — if the cursor is on a non-word byte,
  delegate to `word_forward`; then advance while the *next* byte is a word byte.

Wired in as `EditorCommand::WordForward` / `WordBackward` / `WordEnd`
(`src/tui/editor.rs:1689-1699`, dispatched at `src/tui/editor.rs:4417-4422`).
The variant's own doc comment states the intent: *"Helix `e` — the last byte of
the word **at or after** the cursor"* (`src/tui/editor.rs:1691`). Helix's actual
rule is the end of the **next** word — that one-word difference is the whole
defect.

**Defect 1 — `e` stalls.** `word_end` advances the cursor only when the *next* byte
is a word byte. When the cursor already sits on a word's last byte the loop breaks
immediately and `word_end` returns the **same offset**. Pressing `e` repeatedly
therefore freezes on the last character of a word. Helix does not: upstream
`helix-core/src/movement.rs` defines `e` as `move_next_word_end`, and its own test
asserts *"Basic forward motion from the end of a word to the end of the next"* —
`(0,5) -> (5,13)`. Helix's `e` **always advances**.

**Defect 2 — symbols are separators, not words.** Because `is_word_byte` excludes
punctuation, `e` in `a.b.c` from the `.` skips the whole `.b.` region and lands on
`b`; the next `e` then stalls on `b` (Defect 1). Helix classifies characters into
`Word` / `Punctuation` / `Whitespace` and treats a *category change* as the word
boundary; upstream's own test is named *"alphanumeric.!,and.?=punctuation are
considered 'words' for the purposes of word motion"*, and asserts
`(0,12) -> (12,15)` — i.e. `e` lands on the **last character of a punctuation
run**. That is precisely the "gets stuck on symbols" symptom.

**Missing capability — auto-select.** `e`/`b` are pure motions today. The only
call site is `state.move_to(editor_point(snapshot, next))`
(`src/tui/editor.rs:2835`), which *preserves* a held selection but never creates
one. To "automatically select the word", the motion must produce both the word's
start and end.

**Good news / design synergy.** Helix's `e` and `b` need a *word-boundary*
computation (a category classifier plus a scanner), not just a byte predicate. The
same boundary function that yields the new cursor position yields the word's
extent — so the motion and the auto-selection are one function, not two. One
`word_bounds(source, offset) -> (start, end)` helper, used by `e`, `b`, and the
selection, resolves all three asks (continuous skipping, symbol handling,
auto-select).

**Also required:** `w` must use the same classifier or `w`/`e`/`b` will disagree
about what a word is.

**Helix reference:** keymap `e` = "Move next word end", `b` = "Move previous word
start", `w` = "Move next word start"
(<https://docs.helix-editor.com/keymap.html>); semantics from
<https://raw.githubusercontent.com/helix-editor/helix/master/helix-core/src/movement.rs>.

---

### A2. Paging and scrolling (Helix `scrolloff` model)

**Current implementation — minimal scroll, no margin.**

- `src/tui/editor.rs:4501` `reveal_cursor(...)` — moves the window the minimum
  distance required to put the cursor on screen. If the cursor passes the bottom
  edge, `first = cursor_line + 1 - visible_lines`, which parks the cursor on the
  **last row** of the window.
- `src/tui/editor.rs:5172` `resolve_viewport(...)` — the same minimal rule for the
  live viewport (`:5188-5195`).

**What Helix does.** Helix has a dedicated setting:

> `editor.scrolloff` — *Number of lines of padding around the edge of the screen
> when scrolling* — default **5**
> (`helix-view/src/editor.rs` → `Config::scrolloff`).

`ensure_cursor_in_view(doc, scrolloff)` scrolls so the cursor keeps `scrolloff`
lines of padding. Concretely, for a window of `H` rows the view starts scrolling
when the cursor reaches row `H - scrolloff`, and the cursor then rests at row
`H - scrolloff - 1` — i.e. **5 lines below the cursor, so 5+1 lines above the
bottom**. While the cursor walks the last 5 rows, the page does **not** move.
This is exactly the behavior requested ("a 5 line buffer above and under, keep
page put … keep the cursor 5+1 lines above the bottom and scroll the page").

CodeGG has no equivalent. Every `j` at the bottom row scrolls the page by exactly
one line, forever — no dead band, no margin.

**Paging verbs are wrong in two ways.**

- `src/tui/commands/editor.rs:3868-3875` — `PageDown` → `EditorCommand::HalfPageDown`,
  `PageUp` → `HalfPageUp`. They are **aliases of the half-page verbs**, not
  full-page moves. Helix distinguishes them: `Ctrl-b`/`PageUp` = `page_up`
  ("Move page up"), `Ctrl-f`/`PageDown` = `page_down`, `Ctrl-u` = "Move cursor and
  page half page up", `Ctrl-d` = "Move cursor and page half page down"
  (<https://docs.helix-editor.com/keymap.html>).
- `src/tui/editor.rs:3115` — `NOMINAL_HELIX_PAGES = 20` is a hardcoded nominal
  window used when a motion runs from a verb, so a "half page" is 10 lines
  regardless of the real pane height. A short editor pane and a tall one page by
  the same amount.

**Missing capability — view scrolling without cursor movement.** Helix's `z` view
mode (`z c` center, `z t` top, `z b` bottom, `j`/`k` scroll the view) scrolls the
*page* without moving the cursor. "Actual scrolling of the editor to adopt the
behavior of helix" is this feature; it is a separate milestone from the scrolloff
work, because it needs a viewport offset that is **decoupled** from the cursor.
Today `presentation.viewport_line` is derived from the cursor on every resolve
(`src/tui/editor.rs:5189-5195`), so an independent scroll offset does not exist.

**Dependency:** a decoupled scroll offset and the scrolloff margin interact. Do
scrolloff first (it changes `reveal_cursor` and `resolve_viewport` only), then
introduce the independent offset (which must still respect scrolloff at the
document ends).

---

### A3. `x` on an empty line

**Current behavior:** the line selection is computed but never visible.

- `x` produces a `SelectionMode::Line` selection
  (`src/tui/helix_selection.rs:95`; the range spans the line terminator —
  `src/tui/editor.rs:2699-2710` `member_range`).
- Rendering: `src/tui/components/editor.rs:1142` builds `boundaries` from the
  line's **content bytes**. An empty line has zero content bytes, so the only
  cell is the cursor's virtual cell (`src/tui/components/editor.rs:1336`
  `cell_for_offset` → `Some((0, 1))`).
- The selection *is* detected (`src/tui/components/editor.rs:1270-1274`) — the
  range `line_start..line_start+1` overlaps the single cell.
- But that cell is then overwritten by the cursor's reverse video, which is
  painted **last** so nothing can bury it
  (`src/tui/components/editor.rs:1309-1321`).

⇒ On an empty line the line selection collapses into the cursor's own cell and
becomes invisible.

**Fix shape:** a line-mode selection must paint a **row-level** background
independent of the cell partition — extend the line-mode selection to the full
text column width (or paint the row background) so an empty line still shows a
full-width highlight. A cell-level selection background alone cannot express it.

---

### A4. `SEL` mode and auto-deselect on movement

**Current state.**

- `src/tui/editor.rs:1581-1585` — `EditorMode` is `Normal | Insert`. No select mode.
- `src/tui/helix_selection.rs:137` — `Selection::extends_on_move()` is true only
  for `SelectionMode::Char`.
- `src/tui/helix_selection.rs:89-97` — the module's own doc states the current
  design: *"This is the only mode a movement extends, which is what makes `v` a
  mode and `x` an action."* So `v` toggles a Char selection that extends on
  movement, and `x` sets a Line selection that does not.
- `src/tui/helix_selection.rs:488-493` — `SelectionState::move_to` moves the
  cursor and clears the `growing` flag, but **never clears the selection**. After
  `x`, moving the cursor leaves a stale highlighted line behind.

**Requested model (Helix-native).** Helix's `v` is *"Enter select (extend) mode"*:
*"Select mode echoes Normal mode, but changes any movements to extend selections
rather than replace them."* <https://docs.helix-editor.com/keymap.html>

So the target is a **persistent mode**, not a per-selection property:

- `EditorMode::Select` — a third variant beside `Normal`/`Insert`.
- `v` enters Select; `Esc` leaves it (Normal).
- In Select, movements extend the held selection.
- In **any** other mode, a movement **clears** the selection — this is the
  "automatically deselected on cursor movement" rule, and it fixes the stale-`x`
  highlight for free.

**Interaction with A1.** If `e`/`b` auto-select the word (A1), the selection they
create must survive as a *visible* selection without the user being in Select
mode — otherwise the auto-select is invisible. The cleanest reading: the
auto-select sets a selection, and the next movement clears it (NOR semantics),
so the highlight marks the word you just jumped to and disappears on the next
motion. `SEL` mode is what makes the selection *stick and grow*.

This also means the mode badge must show **three** states, not two — which
matches the user's "add SEL (select) mode along with NOR/INS".

**Guard impact:** `scripts/check_tui_editor_key_authority.py` asserts every chord
has exactly one owner, and `check_tui_editor_prefix_state.py` covers the prefix
state machine. `v` is already bound; adding `Esc`-to-leave-Select and the
Select-mode movement semantics must keep both guards green.

---

### A5. `:50` — jump to a line number

**Current state.** The `:` registry is nine commands — `w`, `q`, `wq`/`x`, `q!`,
`e`, `wa`/`wall`, `new`, `open`/`o`, `complete`/`split-regex`
(`src/tui/editor.rs:1487-1497`, `EditorCommandName::ALL`). There is **no**
goto-line command. `parse_editor_command` splits on the first space and matches
the name against the registry, so a bare `:50` falls through to "unknown command".

**This is Helix-native, not an invention.** Helix's command line documents
`:goto 123`, and `:123` and `123G` as equivalent spellings
(<https://docs.helix-editor.com/command-line.html>). CodeGG's `G` verb is already
Helix-correct for the no-argument case — `BufferEnd` (`src/tui/editor.rs:1686`),
with `gg` → `BufferStart` (`:1685`) — so the missing piece is precisely the
*argument* form, `123G`.

**Required changes:**

1. A new `EditorCommandName::GotoLine` (or a numeric argument on an existing
   verb) in the registry.
2. A **numeric branch before the name match** in `parse_editor_command` — the
   current parser assumes a non-empty name, so `:50` has no name to match.
3. `complete_editor_command` (`src/tui/editor.rs:1522`) is a fuzzy matcher over
   the registry. A numeric query must **not** be fuzzy-matched against command
   names, or `:5` will surface `split-regex` / `complete` ahead of the line
   jump. The completion path needs an explicit numeric short-circuit.
4. The match list rendering must show the resolved target (e.g. `:50 → line 50`).

---

### A6. Scrollbar

**Current state — the column is already paid for, the widget does not exist.**

- `src/tui/editor.rs:5179-5182` — `text_columns = area_width - gutter - 1`, and
  the field doc reads *"Columns available for text after the gutter and
  scrollbar"*. The rightmost column of the editor block is reserved.
- `src/tui/components/editor.rs` contains **no** `Scrollbar` import or render
  (verified: zero matches for `Scrollbar|scrollbar` in that file). The reserved
  column renders as blank space.

**Precedent exists.** The chat transcript already has one:
`src/tui/components/messages.rs:1427` `scrollbar_state`, `:1438` `needs_scrollbar`,
with `ScrollbarState` imported at `:9`.

**Needed:** build a `ScrollbarState` for the editor from
`(viewport.first_line, viewport.visible_lines, snapshot.len_lines())` and render it
in the reserved column. Because the column already exists, this is a *fill-in*,
not a layout change — but it does need a new dependency edge to the
`Scrollbar`/`ScrollbarState` types, and it must not fight the block border.

**Caveat:** with the scrolloff change (A2) and a decoupled scroll offset (A2),
the scrollbar's thumb must reflect the *independent* offset, not the
cursor-derived one.

---

## Part B — Layout de-cluttering

### B1. Where `NOR`/`INS` is actually rendered — four sites, not three

| # | Site | Location | Renders | Duplication |
|---|---|---|---|---|
| 1 | Editor block **title** (top line) | `src/tui/components/editor.rs:120-129` | `{path} · {NOR\|INS} · {status}` | duplicates 2 and 4 |
| 2 | App **header**, editor route | `src/tui/app/render.rs:767-789` | `editor  {path}  {NOR\|INS}` | duplicates 1 and 4 |
| 3 | Composer **badge**, first line of the prompt widget | `src/tui/components/prompt.rs:474-497` (via `src/tui/app/render.rs:2025-2064`) | `[NOR]` / `[INS]` | — (this is the one to keep) |
| 4 | Composer **hint row** | `src/tui/app/mod.rs:13266-13284` (painted `src/tui/app/render.rs:1924-1952`) | `COMP · NOR · <keys> · space help` | duplicates 1, 2 and 3 |

The user's "2 here" on the input box = **site 3** (the badge) and **site 4** (the
hint row). "Keep only the one directly above the input box" = keep site 3.

Separately, dialogs carry their own NOR/INS badges
(`src/tui/components/dialogs/chat.rs:444`, `dialogs/help.rs:306`,
`dialogs/project_picker.rs:229`) — different surfaces, out of scope unless the
user wants them gone too. Worth confirming.

### B2. Editor panel chrome layout

- `src/tui/components/editor.rs:41-43` — `CHROME_ROWS = 2`, reserved at the
  **bottom** of the block's inner area; the text area is the inner area minus
  those rows (`:141-146`).
- **Row −2 (banner)** — `render_banner` (`:155-398`): status/notice, history
  `p`/`n`, find note, pending prefix. Overridden by the `/` search line
  (`:202-208`) or the `:` command line (`:209-215`).
- **Row −1 (readout)** — `ln N  col M  [scope]  writable|read-only`
  (`:309-352`), plus the LSP semantic-coverage note (`:357-363`) and the
  "line too long" note (`:369-379`).
- **Row 0 (top border)** — the block title, i.e. the "filepath / mode / modified"
  line the user wants emptied.

**Target (per D1):** top line empty; **one** bottom row = **path on the left**,
**status cluster on the right**; no line number; no column.

**The bottom row's right-hand cluster (D1).** The readout row already computes
four real signals. All four survive, right-aligned, on the single bottom row:

| Signal | Current source | New home |
|---|---|---|
| **modified** (dirty) | the block title's `status` word (`src/tui/components/editor.rs:120-129`) | bottom row, right |
| **writable / read-only** | readout (`:309-352`) | bottom row, right |
| **LSP semantic-coverage** | readout (`:357-363`) | bottom row, right |
| **line too long** | readout (`:369-379`) | bottom row, right |
| ~~line number~~ | readout (`:309-352`) | **dropped** |
| ~~column~~ | readout (`:309-352`) | **dropped** |

The "modified" word is *moved*, not deleted — it is the only unsaved-changes
signal in the editor, so it moves from the top title down to the bottom row's
right side. The mode moves to the badge (B3). Only the coordinates go.

**What the top line loses.** The title carries `{path} · {mode} · {status}`. The
path moves to the bottom row, the mode to the badge, and `status` moves to the
bottom row's right side. The title itself becomes empty — the block's top border
renders with no label.

**Where the banner row's contents go.** Removing the banner row
(`CHROME_ROWS` 2 → 1) also removes the row that carried the **status/notice**,
the **`p`/`n` history note**, the **find note**, and the **pending prefix**
(`g`, `space`). These need explicit homes, and they are *transient* — they only
need to be visible while they apply:

- **Pending prefix** (`g`, `space`) → the bottom row's right side, replacing the
  cluster while a prefix is armed. It is the one thing that must be visible at
  the instant the key is pressed.
- **Status/notice** (e.g. "Saved", `NotReady`) → the bottom row's right side, or
  the existing **top-right toast** (`src/tui/app/render.rs:485-493`) if the
  notice is not prefix-scoped.
- **`p`/`n` history note and the find note** → the bottom row's right side
  while a search is active.

So the bottom row's right side is a **single slot** that shows, in priority
order: pending prefix → find/history note → status/notice → the persistent
cluster (modified / writable / LSP / line-too-long). One slot, not four, so the
row never grows and the height never changes.

> **Implemented differently — see §0.0 item 4.** The slot is a **union fitted
> by rank**, not a priority switch: the parts are shown together, ordered
> `[persistent][disclosure][notice][prefix]`, and what is dropped when the row
> is narrow is decided by how much each part would cost to lose. A priority
> switch would make the scope badge disappear every time a prefix is armed,
> and the scope badge is a safety signal.

**Layout mechanics to plan for.** `CHROME_ROWS` is a compile-time constant
today. Making it *conditional* (1 at rest, 2+ when the `:` command is active —
B5) changes the text area height, which changes `visible_lines`, which re-resolves
the viewport and can move the cursor's screen row. The `:` command's first
keystroke would therefore reflow the buffer. That needs to be designed, not
discovered later.

**Alternative worth one sentence:** rather than making `CHROME_ROWS` conditional,
keep it at 2 and let the **banner row collapse to blank** at rest, with the
bottom row carrying the path + cluster. That keeps the text area height constant
and removes the reflow hazard entirely, at the cost of one permanently blank row.
The user asked to "minimize vertical space", so the conditional variant is the
better fit — but the constant variant is the safer one, and the difference is one
row.

### B3. Composer indicators

- The badge (site 3) is the **first line of the prompt widget** itself
  (`src/tui/components/prompt.rs:474-497`) — the one "directly above the input
  box". Keep it.
- The hint row (site 4) is a **separate reserved row** above the widget: it is
  carved out of the prompt allocation as `hint_rows` (`src/tui/app/render.rs:175`,
  allocated `:232-261`) and painted at `:1924-1952` with a `▌` marker.
- Its content comes from `App::composer_hint_text_bounded`
  (`src/tui/app/mod.rs:13181-13246`) → `composer_hint_row`
  (`src/tui/app/mod.rs:13266-13284`), which hardcodes the `COMP` and `NOR`/`INS`
  chunks at `:13271` and `:13274`.

**Action:** drop the `COMP` and `NOR`/`INS` chunks from `composer_hint_row`. That
removes the duplicate mode indicator *and* the duplicated panel name, and keeps
the row's remaining content (the verb list and "space help") for the surfaces
that still want it.

**Reflow hazard.** The hint row's presence is deliberately gated on the
**keyboard**, not on the mode (`src/tui/app/mod.rs:13231-13233`), precisely so
`Esc` never reflows the transcript. Removing the row entirely when the editor is
active reintroduces a route-gated reflow. The safe shape is: keep the row
**allocated** (so the height never changes) and render it **empty** when the
editor is active. That satisfies "remove that line entirely when editor is
active" visually while keeping the layout stable. `tests/tui_panel_docking.rs:1703-1711`
asserts the hint row is "never vacated" — the allocated-but-blank approach keeps
that test's intent intact.

**Test fallout.** `tests/tui_chat_surfaces.rs:479` and `:578` assert the hint row
contains `"COMP"`; `tests/tui_composer_keymap_consistency.rs:174` treats
`"COMP" | "NOR" | "INS"` as permitted prose in the row. All three encode the
duplication the user wants removed and must be updated together with the change.

### B4. The editor "key list + space help" line — scope correction

**The line the user is describing is the composer hint row, not an editor
surface.** The editor panel itself has no key list: its two chrome rows are the
banner and the readout (B2).

But there is a real collision with the requested `space` behavior:

- `Space` in the editor buffer is the **Helix prefix**, not a standalone action:
  `EditorPrefix::Space` (`src/tui/editor.rs:107`), with leaves `y k s S d h f ? p P Y r a`
  (`src/tui/editor.rs:577-664`).
- `space ?` already opens the editor's own command palette
  (`src/tui/commands/editor.rs:2647` `open_editor_command_palette`, bound at
  `:11266-11281`). Its doc comment states it is deliberately **not** the global
  `?` help dialog: *"space ? is the editor's list of what it can do, which is a
  different question and a different reader."*
- There is **no** bare-`Space` action today; pressing `Space` arms the prefix and
  waits.

So "space should bring up a box in the right bottom corner for keymap" means
either (a) bare `Space` becomes a new action that opens a keymap box, or (b) the
existing `space ?` palette is **re-anchored** to the bottom-right corner and its
content stays the editor's keymap. (b) preserves the Helix grammar and satisfies
the intent; (a) breaks the prefix machine. **Recommend (b).**

**Overlay precedent.** There is **no** bottom-right overlay in the codebase:

- Toast: **top-right** (`src/tui/app/render.rs:485-493`; right-aligned in
  `src/tui/components/toast.rs:257-262`).
- LSP panel and completion popup: **centered** (`src/tui/components/editor.rs:512-525`,
  `:994-1004`).

A bottom-right keymap box is a **new overlay primitive**. It must not overlap the
bottom chrome row that now carries the path (B2), and it must not overlap the
`:` command's match list (B5).

### B5. The `:` command line on the very bottom row

**Current:** the `:` prompt is drawn on the **banner** row (row −2) via the
`:cmd_` prefix (`src/tui/components/editor.rs:209-215`), and the fuzzy match list
is drawn on the **readout** row (row −1, `:269-307`).

**Requested:** the prompt on the **very bottom** line, the match list **above**
it, and the file-path line **erased** while the command is active.

So the two rows swap roles, and the resting path row is replaced:

- Resting state: 1 bottom row = path (left) + surviving notices (right).
- Command active: bottom row = `:cmd_…` prompt; the rows **above** it carry the
  match list; the path line is gone.

The match list has no fixed height (it is capped by
`MAX_EDITOR_COMMAND_MATCHES`), so the "rows above" region must be a **floating
overlay above the prompt row**, not a fixed chrome allocation — otherwise the
buffer would resize with every keystroke. That is the same overlay-primitive
question as B4, and the two overlays (match list, keymap box) must not collide.

---

## Part C — The `/files` panel

### C1. Scrolling and `Ctrl+d` leaving the page

**Defect confirmed by code reading.** The scroll offset is clamped against the
**row count**, not the **window height**:

- `src/tui/app/state/file_tree.rs:189-202` `reveal_selected` —
  `self.scroll = self.scroll.min(total.saturating_sub(1))`, then adjusts so the
  selected index is in `[scroll, scroll + 1)`. The clamp allows `scroll` to reach
  `total - 1`, i.e. a window showing a **single row** at the end of the list.
- `src/tui/app/state/file_tree.rs:176-186` `move_selection` calls
  `reveal_selected` — **not** `ensure_visible`.
- `src/tui/app/state/file_tree.rs:236-244` `page_selection` calls
  `ensure_visible(index, self.viewport_rows)`, but `ensure_visible` returns early
  when `height == 0` (`:206-208`), and `viewport_rows` is `0` until the renderer
  publishes a measured height (`:135-141`).

⇒ **`Ctrl+d` before the first measured frame leaves `scroll` wherever
`reveal_selected` put it — up to `total - 1` rows off-page — and the early
return in `ensure_visible` means nothing corrects it.** That is the reported
symptom.

**Fix shape:**

1. Clamp `scroll` to `total.saturating_sub(viewport_height)`, not `total - 1`.
2. Make `page_selection` use the same fallback the page-step calculation already
   uses — `FALLBACK_PAGE_ROWS = 10` (`src/tui/app/state/file_tree.rs:41`) —
   when `viewport_rows` is 0, instead of passing 0 into `ensure_visible`.
3. Route `move_selection` through `ensure_visible` rather than `reveal_selected`,
   or make `reveal_selected` height-aware.

### C2. INS mode with a live filter

**Current state:** the panel has no insert mode and no filter. Its keys are
handled by `handle_file_tree_key` (`src/tui/commands/editor.rs:2196-2264`), and
the tree is read-only.

**Reusable precedent in the codebase:**

- The editor's own `:` command line — `presentation.command_active`,
  `command_line`, `command_selection` — is an in-panel insert-over-filter with
  a live match list. This is the closest analogue to what is being asked for.
- The composer's insert mode — `App::composer_input_mode`
  (`src/tui/app/mod.rs:12859-12871`).
- The editor's `/` in-document search (`src/tui/editor.rs` search state), which
  filters as you type.

**Needed on `FileTreeState`** (`src/tui/app/state/file_tree.rs:85`): a `filter:
String` field, a panel-local mode (Normal/Insert), and a filtered variant of
`rows()` (`:149`) that matches on the path or the file name, case-insensitively.
The filter must survive the Normal↔Insert transition, which means the filter
string must live on the panel state and not be cleared by the mode change.

**Gap:** there is no text-input surface in the panel today, so the "in box form"
needs a new widget surface — the filter field rendered inside the panel, with a
cursor.

**Note on scope:** this is a genuine new capability, not a refinement. It also
needs its own `InputMode`-like enum distinct from the app-wide `InputMode`, or
it must participate in the app's mode arbitration (the composer badge at site 3
reports the app-level mode — a panel-local INS must not be reported as the
composer's INS).

### C3. `/files` `h`/`l` — peek mode (D2, D3)

#### C3.1 What already works

Arbitrary-depth navigation is already implemented — the piece the user believes
is missing is not missing:

- `src/tui/app/state/file_tree.rs:48` `MAX_TREE_DEPTH = MAX_LISTING_DEPTH` (3) is
  a **response window**, not a navigation limit. `architecture/tui.md:6484-6488`
  says so explicitly: *"Depth is a window, not a navigation limit. `MAX_LISTING_DEPTH`
  is 3, single-sourced into the TUI as `MAX_TREE_DEPTH`. One listing is scoped:
  `WorkspaceDirectoryList` carries a `prefix`, and `l` re-walks the selected
  directory as a new root, so a three-level embed bounds one response rather than
  the workspace."*
- `l` → `descend_to` (`src/tui/commands/file_tree.rs:467`) → `set_root_prefix`
  (`src/tui/app/state/file_tree.rs:300`) → a prefix-scoped daemon listing.
- `h` → `ascend_one_level` (`src/tui/commands/file_tree.rs:481`) → `ascend_root`
  (`src/tui/app/state/file_tree.rs:273`), backed by `root_history` (`:125`).
- `architecture/tui.md:6484-6488` also records *why* the depth is 3: a deeper
  single embed spent the wrong budget.

**The real limit the user is hitting** is the **display** clamp:
`src/tui/app/state/file_tree.rs:51` `MAX_INDENT_DEPTH = 8` — a render clamp, not a
navigation cap. Rows deeper than 8 all draw at the same indent, so a deep tree
looks flat and reads as "not enough depth". The fix is C3.4.

#### C3.2 The current semantics vs. the requested ones

Current keymap (`src/tui/commands/editor.rs:2196-2263`):

| Key | Current action | Code |
|---|---|---|
| `j` / `Down` | move selection | `:2223` |
| `k` / `Up` | move selection | `:2227` |
| `l` / `Right` | **re-root the pane at the selected directory** | `:2235-2246` → `descend_to` |
| `h` / `Left` | step out one level (at the workspace root, falls back to an in-listing collapse) | `:2247-2250` → `ascend_one_level` |
| `Enter` | open a file, **or expand a directory inline** | `:2253-2256` → `activate_selected` |
| `r` | re-walk the workspace | `:2258-2261` |
| `Ctrl+d` / `Ctrl+u` | page the listing | `:2205-2212` |
| `Esc` | leave the tree | `:2219-2222` |
| **Backspace** | **unbound** | — |

The gap: `l` **re-roots**, so the directory's parent *and every sibling vanish*.
The requested result (D2) is the **peek**: the directory is expanded in place
and the cursor moves into it, so the parent and the siblings stay visible; the
scope only slides — to the directory's parent — when the listing window runs
out.

The two are not the same motion, and the difference is visible immediately:

```
/workspace
  src/            <- selected
  Cargo.toml
```

| | current `l` (re-root) | requested `l` (peek) |
|---|---|---|
| scope | `/workspace/src` | `/workspace` (unchanged) |
| rows | `tui/`, `Cargo.toml` gone | `src/` (expanded) + `tui/`, `Cargo.toml` visible |
| parent visible? | no | **yes** |

#### C3.3 The peek model (D2, D3)

**State.** The pane already has the two pieces this needs:

- `scope_root` — `root_prefix()` (`src/tui/app/state/file_tree.rs:318`), a
  workspace-relative prefix; empty means the workspace root.
- `expanded` — the set of expanded directories
  (`src/tui/app/state/file_tree.rs:110-133`, `TreeListing.expanded`).
- `scope_depth()` (`:344`) — how many components the scope sits below the
  workspace root; `at_workspace_root()` (`:380`) — whether it is the root.

**Invariants.**

1. The **scope** is the directory whose children are the top-level rows.
2. **Relative depth** = the number of path components below the current scope.
   The daemon's listing window is `MAX_LISTING_DEPTH = 3`, so rows exist at
   relative depths 0, 1 and 2.
3. The view is therefore always within the window, and the scope can advance
   without bound — navigation depth is **unbounded**.

**`l` / `Right` / `Enter` — down one level.**

On a **directory** `D`:
- If `D`'s children would land at relative depth **≤ 2** (i.e. `D` is at relative
  depth ≤ 1) → **expand `D` in place** and move the cursor **into** `D` (to its
  first child). The scope does not change.
- If `D` is at relative depth **2** — the deepest level the window allows — its
  children cannot be fetched, so **slide the scope**: the new scope is
  `parent(D)`, `D` is expanded in it (so `D` sits at relative depth 0 and its
  children at 1), and the cursor moves into `D`.

In both cases the listing is re-requested scoped to the resulting scope.

On a **file**: `l` is a no-op. `Enter` opens it (C4).

**`h` / `Left` / `Backspace` — up one level.**

- If the cursor is **not** on a top-level row → it is inside a directory: the
  cursor moves to its **parent** row, and that directory is collapsed.
- If the cursor **is** on a top-level row → **slide the scope up**: the new scope
  is `parent(scope_root)`, the old `scope_root` is expanded in the new scope
  (so it appears as a top-level row with its children beneath it), and the cursor
  moves onto it.
- At the workspace root the scope cannot go higher, so the motion collapses the
  selected directory in place — never a dead key. This fallback already exists in
  `ascend_one_level` (`src/tui/commands/file_tree.rs:481`); it is preserved.

**These are directional motions, not undo/redo.** `l` is "one level deeper",
`h` is "one level shallower" — the Helix `h`/`l` pair, mapped onto a tree. They
are *not* inverses of each other, because `l` and `h` can take different paths
through the same subtree (one enters a directory, the other slides the scope).
`root_history` (`src/tui/app/state/file_tree.rs:125`) already records the scope
chain, so an explicit "back" affordance is available later if one is wanted.

**Worked example.** Tree:

```
/workspace
  src/
    tui/
      editor.rs
      app/
        mod.rs
  Cargo.toml
```

```
step    scope               rows (relative depth: name)
------  ------------------  ------------------------------------------------
start   /workspace          0: src/   0: Cargo.toml
l→src   /workspace          0: src/   1: tui/    0: Cargo.toml
        (src expanded in place; cursor into src → tui/)
l→tui   /workspace          0: src/   1: tui/    2: editor.rs   2: app/
        (tui expanded in place; cursor into tui → editor.rs)
l→app   /workspace/src/tui  0: app/   1: mod.rs
        (app is at relative depth 2 → SLIDE the scope to its parent;
         app expanded in the new scope; cursor into app → mod.rs)
h       /workspace/src/tui  0: app/
        (cursor is inside app → up to its parent; app collapsed)
h       /workspace/src      0: tui/   1: editor.rs   1: app/
        (cursor on a top-level row → SLIDE the scope up to parent;
         the old scope root, tui/, is expanded in the new scope)
h       /workspace          0: src/   1: tui/    0: Cargo.toml
        (slide up again)
```

The pair reads as a horizontal motion: `l` goes right (deeper), `h` goes left
(shallower), and the **scope slide** is what happens when the listing window
runs out — exactly the "when the depth is at maximum" case.

**Why this satisfies "any amount of depth".** The scope slide keeps the view's
*relative* depth inside the daemon's 3-level listing window no matter how deep
the user goes, and the scope itself can be pushed arbitrarily far down. There is
no depth at which navigation stops working — the previously fatal "max depth"
case simply cannot occur, because hitting it is what *triggers* the slide.

**What changes in the code.**

| Concern | Today | After |
|---|---|---|
| `l` on a directory | `descend_to(app, &path)` — scope = `D` | expand in place + cursor into `D` (relative depth ≤ 1); **slide** the scope to `parent(D)` and expand there (relative depth 2) |
| `Enter` on a directory | `toggle_selected` — inline expand/collapse | same as `l` |
| `Enter` on a file | `open_editor` | same, plus the C4 handoff |
| `h` | `ascend_one_level` — scope = `parent(scope)` | cursor to the parent row + collapse it (cursor inside a directory); **slide** the scope up to `parent(scope)` with the old scope root expanded (cursor on a top-level row); collapse in place at the workspace root |
| `Backspace` | unbound | bound to `h` |
| inline toggle (`toggle_selected`) | `Enter` | **subsumed** — `l` expands, `h` collapses (see below) |

**The inline toggle is subsumed.** `toggle_selected`
(`src/tui/app/state/file_tree.rs:250`) used to be the only way to expand or
collapse a directory. Under peek, `l` expands and moves the cursor into the
directory, and `h` moves the cursor back out and collapses it — the same two
effects, bound to the two directions of the motion. So the standalone toggle
verb can be **retired** rather than re-homed, and `toggle_selected` reduced to
the helper that `l` and `h` both call. The only affordance lost is "expand a
directory *without* entering it", which peek deliberately trades for the
enter-or-slide behavior.

**Peek is a change of muscle memory.** Today `l` re-roots: the pane shows
*only* the selected directory's contents and the parent and siblings vanish.
After this, `l` expands and enters, and the parent stays visible until the
listing window forces a slide. A user who relied on "the pane now shows only
this directory" loses that; the replacement is that the same rows are visible
one indent deeper, with the parent still in view. Worth stating in the release
note.

#### C3.4 The indent clamp (consequence of D2)

`display_depth` (`src/tui/app/state/file_tree.rs:336`) computes
`scope_depth() + row.depth`, clamped at `MAX_INDENT_DEPTH = 8` (`:51`). The
clamp exists for a good reason — the module's own test
(`:1227-1231`) says the guide *"is what runs out of room first — silently
pushing the name off a narrow column is the failure this prevents"*.

But under peek, the **absolute** scope depth is unbounded while the **relative**
depth stays within 0–2. The clamp therefore flattens exactly the deep scopes
that peek is designed to reach — the user slides the scope a few directories
down and the tree stops indenting.

**Two ways out:**

- **(a) Render the indent relative to the scope.** `display_depth` returns
  `row.depth` (≤ 2 under peek), and the absolute position is carried by the
  scope path, which is already displayed. The clamp stops being load-bearing at
  all, because the relative depth cannot grow. This is the peek philosophy: the
  view is about the current scope.
- **(b) Raise the clamp** to cover the deepest reachable scope. But the scope
  depth is unbounded, so no finite clamp is correct — this pushes the problem
  rather than solving it.

**Recommend (a).** It also satisfies the de-cluttering goal: the tree stops
growing a staircase as you navigate, and the path carries the position. This
changes an existing test (`display_depth_adds_back_the_scope_a_subtree_walk_restarted`,
`:1196`) and the module's stated contract, so it is a deliberate change, not a
side effect.

#### C3.5 Notes

- The file tree lives in `src/tui/file_tree.rs` (not `src/tui/components/`), and
  the panel's key handling lives in `src/tui/commands/editor.rs` — the file
  tree's keys are dispatched from the *editor* command module.
- `architecture/tui.md:6484-6488` and the `MAX_LISTING_DEPTH` rationale must be
  updated with the peek semantics; the "depth is a window, not a navigation
  limit" statement stays true but its `l` explanation changes.

### C4. Enter on a file → select the editor panel in NOR mode

**Current:** `activate_selected` (`src/tui/commands/file_tree.rs:504-515`) calls
`open_editor(app, &row.path)` (`src/tui/commands/editor.rs:140` →
`open_editor_in` at `:153`). The document opens; **focus never moves** — the
tree keeps the keyboard.

**Needed:** after a successful open, (a) move keyboard ownership to the editor
and (b) force the editor into `Normal` mode.

**Mechanisms available:**

- The editor's keyboard-ownership check is `app.editor_state.buffer_focused(mode)`
  (`src/tui/app/mod.rs:12751`).
- Panel focus is `app.panel_host.focus()` / `focus_at(placement)`
  (`src/tui/commands/panels.rs:668-702`); `PanelPlacement`
  (`src/tui/panels/mod.rs:246`) is `Left | Right | Bottom` — a **docking**
  placement, not a panel identity, so the editor's identity needs a different
  handle.
- `app.input_target()` (`src/tui/app/mod.rs:12754`) returns
  `InputTarget::Build | Chat` (`src/tui/panels/mod.rs:1075`) — also not the
  editor.

⇒ The handoff needs whatever selects the **primary view** (editor) as the
keyboard owner; that concept is not yet located in this research pass and is the
one open implementation unknown in Part C.

**Mode forcing:** the editor's mode is `EditorMode` on the editor presentation
(`src/tui/editor.rs:1581`). A `Select` mode (A4) means the handoff must set
`Normal` **specifically**, not merely "not Insert" — otherwise opening a file
from the tree could leave the editor in Select and silently extend a selection
that no longer makes sense.

**Interaction with the peek motion.** Under peek (C3.3), `Enter` on a
**directory** is the peek motion and `Enter` on a **file** is the open. The
dispatch must branch on `row.kind` — `TreeNodeKind::Directory` versus
`TreeNodeKind::File` — because `activate_selected`
(`src/tui/commands/file_tree.rs:504-515`) currently conflates them through
`toggle_selected` (`:250`). `l` stays a no-op on files, so the only way to open
a file from the tree remains `Enter`.

**Ordering matters.** The handoff must run *after* the open succeeds, not
before: `open_editor` is asynchronous (it issues a daemon request), so moving
the keyboard to the editor before the document is attached would hand the user an
editor that has nothing in it. The tree's `Esc` handler
(`src/tui/commands/editor.rs:2219-2222`) is the existing precedent for moving
focus off the tree, and it is a *synchronous* focus change — the open path
needs its own, deferred equivalent.

---

**Files panel — revised milestone ordering.** F3 (peek) and F4 (relative
indent) land together: the relative indent is only correct once the scope
advances one level per `l`, because that is what makes the relative depth
bounded at 1–2. Doing the indent first would encode the old absolute-depth
contract into the new one.

---

## Part D — Governance, guards, tests, and docs

### D1. Planning

- `plans/subsystems/tui-editor-helix-parity-roadmap.md` is **closed** (status
  line 3: *"closed (M001–M013 and M015–M020 closed; M014 blocked on ADR-0034
  acceptance — the named exception, and not a precondition for closing)"*).
  This work **cannot** amend it; it needs a new roadmap.
  - Note: its §4 "Current state" (lines 123-179) is already stale in places
    (e.g. item 3, "Single selection", was closed by M007). Do not use it as the
    source of truth for current behavior — the source is the code.
- `plans/subsystems/file-picker-rework-roadmap.md` is the live home for the
  `/files` half (Part C).
- `plans/README.md` defines the hierarchy; `plans/registry.md` is the
  authoritative status register. `plans/closure/` holds per-milestone records.
- `plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` is relevant only to
  M014 (tree-sitter text objects), which is **not** in this request's scope.

### D2. Guards that will fire

| Guard | Why it matters here |
|---|---|
| `scripts/check_tui_editor_key_authority.py` | Every editor chord must have exactly one owner. New `SEL`-mode verbs, the `Esc`-to-leave-Select chord, and any `space` action all land here. The guard's own doc explains the class of bug: a chord declared in a later stage is unreachable the moment an earlier stage claims it. |
| `scripts/check_tui_editor_prefix_state.py` | The Helix prefix machine (`g`, `space`, `[`, `]`, `Alt-`). Touched if `space` gains a bare-Space action. |
| `scripts/check_tui_editor_apply_path.py` | Where a verb's effect is applied. |
| `scripts/check_tui_editor_completion_authority.py` | The `:` completion source of truth — directly hit by A5 (numeric branch) and B5 (match list). |
| `scripts/check_tui_editor_text_authority.py` | Text-transform verbs — hit by any new selection-producing motion (A1). |
| `scripts/check_tui_editor_leaf_authority.py` | Prefix leaf ownership. |
| `scripts/check_tui_editor_doc_drift.py` | Doc/registry drift — hit by the new goto-line command. |
| `scripts/check_daemon_listing_authority.py` | The `/files` listing path (C3) — any change to how the tree is scoped must respect the daemon's listing authority. |

### D3. Tests that will break (and must be updated in the same change)

- `src/tui/components/editor.rs:1936` — asserts the readout shows `"ln 1"`.
- `src/tui/components/editor.rs:1956` — asserts `"ln 41"`.
- `src/tui/components/editor.rs:2030-2037` — the `banner()` test helper
  **locates** the readout row by searching for the literal `"ln "` and then
  returns the row above it. Removing ln/col breaks the *locator*, not just the
  assertion — every test using `banner()` needs a new anchor.
- `src/tui/components/editor.rs:2453-2470` — asserts the title contains
  `"NOR"`/`"INS"`. Removing the title's mode breaks it.
- `tests/tui_chat_surfaces.rs:479`, `:578` — assert the composer hint row
  contains `"COMP"`.
- `tests/tui_composer_keymap_consistency.rs:174` — treats `"COMP" | "NOR" | "INS"`
  as permitted prose parts in the hint row.
- `tests/tui_panel_docking.rs:1703-1711` — asserts the hint row is never vacated
  and that the vacated row names the composer's verbs.
- `src/tui/commands/editor.rs:6037-6046` — asserts `PageDown` moves a **half**
  page (`cursor == 70`). Making `PageDown` a full page breaks this test, and the
  test name documents the current (wrong) behavior.

**File-tree fallout from the peek change (F3/F4).**

- `src/tui/app/state/file_tree.rs:1196`
  `display_depth_adds_back_the_scope_a_subtree_walk_restarted` — asserts the
  absolute-depth contract that the relative-to-scope indent (C3.4) changes.
- `src/tui/app/state/file_tree.rs:1231`
  `display_depth_clamps_at_the_one_cap_both_renderers_share` — asserts the
  `MAX_INDENT_DEPTH` clamp; becomes moot under relative indent.
- `src/tui/app/state/file_tree.rs:1249`
  `display_depth_is_a_read_and_changes_nothing_about_navigation` — states the
  separation between "visually deeper" and "reachable differently", which the
  peek change makes a *design* statement rather than a guard.
- The `descend_to` / `ascend_one_level` command tests in
  `src/tui/commands/file_tree.rs` — they assert the current re-root semantics
  (scope = the selected directory). Peek changes the observable scope, so these
  are rewritten, not adjusted.
- Any test asserting that `l` hides the parent — the peek model deliberately
  keeps it visible.

### D4. Documentation to update

- `architecture/tui.md` — the editor key-line section, the banner/history
  section, the find-note section, and the `/files` depth discussion at
  `:6484-6488` (the "depth is a window, not a navigation limit" statement stays
  true; its `l` explanation becomes the peek model).
- `docs/tui.md` — user-facing keymap and layout.
- `architecture/overview.md` — only if the "Verified counts" table changes.
- `.opencode/skills/tui/SKILL.md` — the operational guide; it names
  `architecture/tui.md` as its authoritative doc. Per `AGENTS.md`, when a module
  contract changes the skill and its `architecture/` doc change **together**.
- The `/files` keymap is documented as a surface, so the peek motion (and the
  new `Backspace` binding) must be reflected wherever the tree's keys are
  described — `docs/tui.md` and the `architecture/tui.md` tree section.

### D5. Verification

`scripts/verify.sh quick` is the canonical gate, and it is a **superset** of CI
(`.github/workflows/ci.yml` is a strict subset — a green CI run does not imply a
green `quick`). For this change set:

- `cargo fmt` (rustfmt: `max_width 100`, 4-space).
- `scripts/verify.sh quick` — includes the editor key/prefix/completion/text/leaf
  authority guards and the doc-drift guard.
- `scripts/verify.sh full` — clippy `-D warnings` + workspace tests +
  `cargo test -p codegg --features server,plugins,lsp-test-support`.
- `cargo nextest run --workspace --locked --profile ci` for the capped suite
  (process-global env mutation ⇒ one process per test).

---

## Open questions / decisions needed

**Resolved on 2026-10-20** (see §0.1): the dirty-state signal, the surviving
notices, and the `/files` scope semantics. They are removed from this list and
folded into the plan.

1. **`space` in the editor.** Bare `Space` as a new action (breaks the prefix
   machine) vs. re-anchoring the existing `space ?` palette to the bottom-right
   (preserves Helix grammar)? **Recommend the latter.**
2. **`/` search.** The request is silent on it, but the `:` change moves the
   prompt to the bottom row and the `/` search uses the same banner row. Does
   `/` get the same treatment (bottom row, results above)?
3. **The retired inline toggle.** Under peek (D2), `l` expands and enters and
   `h` exits and collapses — so the standalone toggle verb
   (`toggle_selected`, `src/tui/app/state/file_tree.rs:250`) is **subsumed**.
   Retire it outright, or keep it on a dedicated key for a pure toggle?
4. **`SEL` semantics on `x`.** Under the new model, does `x` (line select) *enter*
   Select mode, or set a Line selection that the next movement clears? The
   user said "`v` activates SEL" and "text is automatically deselected on cursor
   movement" — implying `x` does **not** enter SEL. Confirm.
5. **Auto-select and `d`/`y`.** If `e`/`b` auto-select, does `d` now delete the
   word you jumped to? That is a destructive consequence of a convenience
   feature and should be an explicit decision.
6. **The `/files` indent.** Relative-to-scope (recommended, C3.4) or a raised
   absolute clamp? Relative changes an existing test
   (`display_depth_adds_back_the_scope_a_subtree_walk_restarted`,
   `src/tui/app/state/file_tree.rs:1196`) and the module's stated contract.
7. **Bottom-right overlay.** New primitive — no precedent. Confirm the position,
   and how it interacts with the `:` match list and the bottom chrome row.
8. **Composer hint row.** Confirm the intent is to *blank* it when the editor is
   active (keeping it allocated) rather than remove it from the layout —
   removing it changes the transcript height.
9. **Dialog badges.** `dialogs/chat.rs:444`, `dialogs/help.rs:306`,
   `dialogs/project_picker.rs:229` also render NOR/INS. Out of scope?

---

## Suggested milestone decomposition

Ordered by dependency, sized for the existing plan format:

**Editor — motions and modes**
- **E1** — Word boundary classifier + `word_bounds` helper; rewrite `e`/`b`/`w` to
  Helix's "next word end" / "previous word start" rules over
  Word/Punctuation/Whitespace categories. *(A1)*
- **E2** — Auto-select on `e`/`b`; continuous skipping under key repeat. *(A1)*
- **E3** — `EditorMode::Select`; `v` enters, `Esc` leaves; non-Select movement
  clears the selection (fixes the stale-`x` highlight). *(A4)*
- **E4** — `x` on an empty line: row-level line-selection background. *(A3)*
- **E5** — Scrolloff 5 in `reveal_cursor` / `resolve_viewport`; decouple the page
  from the cursor with a 5-line dead band. *(A2)*
- **E6** — Full-page `PageUp`/`PageDown` verbs, distinct from `Ctrl+u`/`Ctrl+d`;
  real pane height instead of `NOMINAL_HELIX_PAGES`. *(A2)*
- **E7** — Decoupled scroll offset + `z` view mode. *(A2)*
- **E8** — `:50` / `:goto N` / `NG`; numeric branch in the parser; numeric
  short-circuit in completion. *(A5)*
- **E9** — Editor scrollbar in the already-reserved column. *(A6)*

**Editor — layout**
- **E10** — `CHROME_ROWS` 2 → 1; empty top line; **path on the bottom left**;
  **status cluster on the bottom right** (modified / writable / LSP / line-too-long
  in one priority slot); drop ln/col; give the pending prefix, find note and
  status/notice a home. *(B2, D1)*
- **E11** — `:` prompt on the bottom row, match list above it, path line erased
  while active; floating match-list overlay. *(B5)*
- **E12** — Composer: drop `COMP` and `NOR`/`INS` from the hint row; keep the
  badge. *(B3)*
- **E13** — Blank the hint row when the editor is active (keep it allocated). *(B3)*
- **E14** — Bottom-right keymap overlay; re-anchor `space ?` into it. *(B4)*

**Files panel**
- **F1** — Height-aware scroll clamp; `Ctrl+d` uses the fallback page size. *(C1)*
- **F2** — Panel insert mode + live filter; filter survives Normal. *(C2)*
- **F3** — **Peek motion**: `l`/Enter on a directory → expand in place and move
  the cursor into it; at the window's edge, **slide the scope** to the
  directory's parent and expand it there. `h`/Backspace mirrors it (cursor to
  the parent + collapse; slide the scope up at a top-level row; collapse in
  place at the workspace root). *(C3.3, D2, D3)*
- **F4** — Relative-to-scope indent so deep scopes stop flattening. *(C3.4)*
- **F5** — Open a file → move keyboard ownership to the editor, force
  `EditorMode::Normal`. *(C4)*
- **F6** — Retire or re-home the inline toggle verb now that `l`/`h` cover
  expand and collapse. *(C3.3, open question 3)*
