# TUI Editor and Files Panel Helix UX Improvement Roadmap

Status: closed (M001–M020 closed)

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md`

Authorizing predecessor: `plans/subsystems/tui-editor-helix-parity-roadmap.md`
(closed). This roadmap does **not** reopen it: the Helix-parity roadmap's
milestones are closed and its §4 "Current state" text is stale, so the source
of truth for every claim below is the code, not that document.

Research artifact: `research/tui-editor-helix-ux-improvements-2026-10-20.md` —
the symptom-by-symptom analysis, the root causes, and the decisions D1–D3 that
this roadmap implements.

Reference dialect: <https://docs.helix-editor.com/keymap.html> and
<https://docs.helix-editor.com/command-line.html>. Helix is the *normative
source for the key semantics*, not a dependency. No Helix code, config, or
asset is vendored.

## 1. Purpose and ownership boundary

Make the CodeGG TUI editor and file-tree panel behave the way a Helix user
expects: word motions that never stall and never lose a symbol, a page that
scrolls like a page, a select mode the reader can see, a jump to a line, a
scrollbar, a status surface small enough to read past, and a file tree that
can be filtered and navigated to any depth.

**This subsystem owns** the editor's word-motion geometry, its scroll and
viewport rules, its held-selection policy, its command-line and keymap
surfaces, its chrome layout, and the file-tree panel's scrolling, filtering,
scope navigation, and focus handoff.

**It consumes** the daemon-owned document service and the daemon-owned
workspace listing service (`WorkspaceDirectoryList`).

**It must not own** text authority, directory listing authority, LSP process
lifecycle, or any mutation path other than `DocumentController::apply_local`.
The file tree keeps its listing on the daemon side in every mode, including
standalone: `scripts/check_daemon_listing_authority.py` enforces that and
this roadmap does not weaken it.

## 2. Work classification

### Invariants

- **One word policy.** The editor and the composer share one word
  classification, in `src/tui/text_geometry.rs`. A second copy is a second
  answer to "what is a word".
- **One reveal rule.** Motions, `/` search, and the renderer resolve the
  viewport through the same function, so a jump to a match lands where a jump
  to the end of the buffer lands.
- **One keyboard axis.** Which region owns the keyboard is answered by
  `ui_state.input_mode` alone. `EditorMode::Select` is a *verb* mode and never
  a second focus axis.
- **No new pending-prefix mechanism.** A new grammar adds a variant to
  `EditorPrefix` and a leaf to its table, never a field to the presentation.
- **The listing stays daemon-owned.** The tree never reads the filesystem.
- **Unclaimed-key fallthrough** and **Insert-mode text integrity** are
  unchanged: a new binding narrows the composer's keyspace, and that narrowing
  is intended.

### Classes

- **invariant** — a correctness property that must hold for every frame.
- **capability** — a behaviour a reader can now perform.
- **clarity** — a surface that stops lying or stops taking space it does not
  need.

## 3. Non-goals

- Full Helix parity. This roadmap is a specific set of reader-visible
  improvements, not a claim that every Helix key exists.
- A count prefix (`123G`). `:50` and `:goto 50` answer the same intent; a
  general count grammar is separate work.
- Horizontal view alignment (`z m`). The viewport is one screen row per
  logical line, so "middle" horizontally has no meaning here.
- Soft-wrap display-line motions (`gj`/`gk` are `j`/`k`).
- A configurable `editor.scrolloff`. The value is a documented constant.
- Multi-document attachment, alternate buffers, and the file/alternate-buffer
  goto leaves.

## 4. Editor milestones

### M001 — Three-way word classification

Class: invariant

Objective: replace the two-way "is this a word byte" predicate with Helix's
three-way classification — word content, punctuation, whitespace.

Dependencies: none.

Deliverable boundary: `src/tui/text_geometry.rs` gains `WordClass`,
`classify_byte`, and the run primitives `word_run_at`, `next_word_run`,
`prev_word_run`, `word_end_run`, and `word_bounds`. The editor's local
`is_word_byte` copy and the interactive terminal's separate
whitespace-token rule stay visibly separate rules in the same module.

User value: punctuation runs become words, so `e` steps over `.!,` instead of
stalling inside it.

Exit conditions: `classify_byte` is the single classifier; `is_word_byte` is
defined in terms of it; the multi-byte rule still holds (every byte `>= 0x80`
is word content, so a UTF-8 character is never split); unit tests cover
punctuation runs, underscores, and multi-byte characters.

### M002 — Word motions that advance, and select

Class: capability

Objective: make `e` and `b` never stand still, and make them select the word
they landed on.

Dependencies: M001.

Deliverable boundary: `word_forward`, `word_backward`, and `word_end` resolve
against the run model. `e` lands on a word's **last character** and always
advances — a caret already on a word's last byte moves on to the next word
rather than freezing. `b` lands on a word's **first character**. The editor
selects the landed word: the selection's head is the caret and the anchor is
the word's other end, so `b` produces a backwards selection and the
highlighted span is the word either way.

User value: holding `e` walks the file with each word picked out in turn, and
`d` after `e` deletes that word.

Exit conditions: a held `e` never repeats an offset; `e` on a punctuation run
advances; `b` lands on the start of a word; the auto-selection is asserted at
the byte level for `e` and `b`.

### M003 — Select mode, and the selection that does not outlive its movement

Class: capability

Objective: add a visible `SEL` mode, and make a movement drop a held
selection outside it.

Dependencies: M002.

Deliverable boundary: `EditorMode` gains `Select`. `v` enters it and arms a
character selection; `v` again and `Esc` both leave it and drop the
selection. Outside select mode a movement clears the selection; inside it a
movement grows it. The composer badge reads `[SEL]`.

User value: a selection that outlives the movement that made it reads as
"something is still selected" long after the reader thought it was gone. And
the reader can now see which of the two they are in.

Exit conditions: `v` shows `[SEL]`; a movement in Normal mode clears the
held selection; a movement in Select mode grows it; `Esc` leaves Select and
clears the selection; the badge reports all three states.

### M004 — `x` on an empty line highlights the line

Class: clarity

Objective: make a line-mode selection on an empty line visible.

Dependencies: none.

Deliverable boundary: `editor::line_selection_covers_line` answers the
row-level question a byte range cannot, and the renderer paints the whole
row's background for a line-selected row.

User value: `x` on a blank line now looks selected, instead of leaving the
reader pressing `x` and seeing nothing happen.

Exit conditions: an empty line under a line selection renders the selection
background across the row; a character selection still highlights only its
bytes.

### M005 — Scrolloff

Class: capability

Objective: stop the page moving out from under the reader.

Dependencies: none.

Deliverable boundary: `editor::SCROLL_OFF` (5, Helix's `editor.scrolloff`
default) and the single reveal rule in `reveal_cursor`. A window of `H` rows
keeps the caret between row `scrolloff` and row `H - scrolloff - 1`; the page
stands still while the caret walks the last few rows. At the two ends of the
document the margin gives way, so the caret can reach the first and last
lines. `resolve_viewport` uses the same rule.

User value: the text the reader is on does not move mid-sentence.

Exit conditions: the reveal rule is shared by motions, `/` search, and the
renderer; a short window shrinks the margin rather than breaking the rule;
the caret can sit on the last line at the end of the document.

### M006 — A full page is a full page

Class: capability

Objective: separate `PageDown`/`PageUp` from `Ctrl+d`/`Ctrl+u`.

Dependencies: M005.

Deliverable boundary: `EditorCommand::PageDown`/`PageUp` are their own
commands. A page moves the **view** by a page and the caret with it, keeping
the caret's screen row; a half page moves both by half. The caret's
discontinuity at the end of the document is unchanged: a page that would land
past the end continues to the end.

User value: `PageDown` moves a page, as its name says. It used to be an alias
of the half-page verb, so the key's name was a lie.

Exit conditions: `PageDown` and `Ctrl+d` are distinct commands with distinct
distances; a page is measured against the **real** pane height, not a fixed
guess; the caret keeps its screen row across a page.

### M007 — The real viewport, and view mode

Class: capability

Objective: resolve screen-relative motions against the measured pane, and let
the page scroll without the caret moving.

Dependencies: M006.

Deliverable boundary: the renderer publishes the measured text height on the
presentation, and the motion layer prefers it over the nominal fallback. A
`z` prefix arms view mode with `c` (centre), `t` (top), `b` (bottom), `j`
(scroll down) and `k` (scroll up); the view offset rides on the presentation
and any caret motion re-attaches the view. `gt`/`gc`/`gb` become bound leaves:
the caret moves to the top, middle, and bottom of the screen.

User value: "the top of the screen" is a line the reader can name, and a
reader can look at another part of the file without losing their place.

Exit conditions: the measured height reaches the motion layer; a short pane
pages by what it shows; `z c` centres the view without moving the caret;
`gt`/`gc`/`gb` land on the screen's own edges.

### M008 — Jump to a line

Class: capability

Objective: make `:50` work.

Dependencies: none.

Deliverable boundary: `EditorCommandName::GotoLine` in the `:` registry.
`parse_editor_command` recognises a **numeric** line before it looks for a
name, and `complete_editor_command` short-circuits the fuzzy matcher for a
numeric query — otherwise `:5` offers `split-regex` and `complete` ahead of
the line jump. The line number is one-based, and `:0` is refused rather than
clamped.

User value: "go to line 50" is the most-typed navigation a reader has, and
CodeGG had no answer for it.

Exit conditions: `:50` and `:goto 50` resolve to the same command; a numeric
query offers only the line jump; the caret lands on the requested line; `:0`
is refused with a notice.

### M009 — The scrollbar

Class: capability

Objective: draw the column the viewport has always reserved.

Dependencies: M005.

Deliverable boundary: the editor widget renders a scrollbar in the rightmost
column of the text area. The column was already paid for —
`text_columns = area_width - gutter - 1` — and nothing ever drew in it.

User value: "how far down am I" has an answer on screen, and a reader who has
scrolled away from the caret can still see where they are.

Exit conditions: the scrollbar renders in the reserved column; the thumb is
sized by how much of the document is visible and placed by where the view
starts; a document that fits on screen still shows a thumb.

### M010 — One bottom row

Class: clarity

Objective: replace the two-row editor chrome with one row that says what
matters.

Dependencies: M004, M005.

Deliverable boundary: `CHROME_ROWS` is 1. The block's top border carries **no
label**. The bottom row has a left slot — the file's path — and a right slot
— the status cluster: the **modified** state, the scope the document is
confined to, writable or read-only, the LSP semantic-coverage note, and the
"line too long" marker. The cluster reads left to right: the **persistent
signals**, then the **transient disclosure** (the find note, the search count,
the history position), then the **notice**, then the **armed prefix** — last,
because it is the one element that comes and goes on its own and the right end
of the row is a home nothing else competes for. The two slots never overlap:
the path keeps up to a third of the row, the cluster takes the rest, and a
cluster that still does not fit is **fitted by rank** (`ClusterWeight`) — the
least important group is dropped first and a group that is alone and too wide
is cut from the right, so the head of the sentence survives. The line number
and the column are gone: they restated the caret, which is already on screen.

User value: a reader scanning the bottom of the editor sees the path, the
dirty state, and the read-only answer, and nothing else.

Exit conditions: the editor block reserves exactly one chrome row; the title
is empty; the path is on the left of the bottom row; the cluster is on the
right; the line number and column are gone.

### M011 — The `:` line on the bottom row, with its matches above

Class: clarity

Objective: put the command line where the reader is typing, and its matches
where they can see them.

Dependencies: M010.

Deliverable boundary: the `:` and `/` lines take the **bottom row** — the
path is erased while either is open — and the `:` command's fuzzy match list
renders as a floating overlay on the row above. The overlay is an overlay
rather than an allocated row on purpose: allocating one changes the text
area's height, which changes the visible line count, which re-resolves the
viewport and can move the caret's screen row under the reader's first
keystroke.

User value: the reader types into the line at the bottom of the screen, and
the matches they are choosing from are directly above it.

Exit conditions: the `:` line renders on the bottom chrome row; the path is
not drawn while it is open; the match list is drawn above it; the text area's
height does not change when the line opens.

### M012 — One mode badge, and no duplicated panel

Class: clarity

Objective: stop saying the same thing twice.

Dependencies: M011.

Deliverable boundary: the composer's hint row no longer carries the `COMP`
and `NOR`/`INS` prefix — the badge directly above the input box already says
it — and the hint row is **removed** while the editor is the active surface,
because the editor has a keymap of its own and two surfaces claiming the same
keys is how a keymap becomes a lie.

User value: the mode is said once, and the editor's own chrome stops being
competed with by a row of keys that belong to another surface.

Exit conditions: the hint row carries keys only; the badge above the input
box carries the mode; the hint row is absent while the editor is active.

### M013 — The keymap box

Class: clarity

Objective: make "what can I press in here?" answerable in one place.

Dependencies: M012.

Deliverable boundary: `space ?` toggles a **keymap box** in the editor's
bottom-right corner, listing the editor's own verbs. It used to open the `:`
command palette — but the `:` line is the command matcher, and the palette was
a second way to reach the same commands, so the key that was supposed to
answer "what can I press here?" answered a different question. `Esc` closes
the box.

User value: the editor's keymap is discoverable from the editor, without
leaving it, and without a dialog that covers the document.

Exit conditions: `space ?` opens and closes the box; the box lists the
editor's own keys; `Esc` closes it; the box is a floating overlay that does
not change the text area's height.

### M014 — The mode badge knows about select mode

Class: clarity

Objective: report the third mode the badge did not have.

Dependencies: M003.

Deliverable boundary: the composer badge reads `[SEL]` while the editor is in
select mode.

User value: the difference between "my selection vanished" and "my selection
grew" is invisible until a movement goes the other way.

Exit conditions: the badge reports `[NOR]`, `[INS]`, and `[SEL]`.

## 5. Files-panel milestones

### M015 — A scroll clamp that knows the window's height

Class: invariant

Objective: stop `Ctrl+d` from scrolling the listing off the page.

Dependencies: none.

Deliverable boundary: `FileTreeState::reveal_selected` clamps the scroll
offset against the **measured** window height rather than the row count, and
`page_selection` falls back to `FALLBACK_PAGE_ROWS` when no height has been
measured instead of paging by zero.

User value: `Ctrl+d` in the file tree moves the listing by a page and keeps
the selection on screen.

Exit conditions: the scroll offset never exceeds
`total - viewport_height`; paging with no measured height uses the fallback.

### M016 — Panel insert mode and a live filter

Class: capability

Objective: let the reader filter the file tree by typing.

Dependencies: M015.

Deliverable boundary: `FileTreeState` gains a `filter: String` and a
**panel-local** mode, distinct from the app-wide `InputMode` — a panel-local
INS must not be reported as the composer's INS. The filter survives the
Normal↔Insert transition, because the point of a filter is that it is still
there after the reader stops typing. The panel renders the filter as an
in-panel input box with a caret.

User value: a large tree is navigable by typing part of a path.

Exit conditions: the filter matches case-insensitively on name and path; it
survives the mode change; the panel-local mode does not move the app's
`InputMode`; the filter is rendered.

### M017 — Peek motion

Class: capability

Objective: navigate a tree of any depth without losing the context above it.

Dependencies: M016.

Deliverable boundary: `l`/`Right`/`Enter` on a directory **peeks** into it —
expands it in place and moves the caret into it — rather than re-rooting the
pane, which used to hide the parent and every sibling. At the deepest level
the daemon's listing window allows, the scope slides to the parent and the
directory is expanded there. `h`/`Left`/`Backspace` moves the caret to the
parent row and collapses it; on a top-level row the scope slides up; at the
workspace root it collapses in place. These are **directional** motions, not
undo and redo: `l` and `h` are not inverses of each other. The inline
`toggle_selected` verb is retired — `l`/`h` subsume it.

User value: a deep project is navigable with the same two keys a Helix user
already uses, and the reader can always see where they are.

Exit conditions: `l` expands in place and moves the caret in; `l` at the
depth limit slides the scope to the parent; `h` from inside a directory moves
to the parent row and collapses; `h` on a top-level row slides the scope up;
`h` at the workspace root collapses in place; `l` on a file is a no-op; no key
is dead.

### M018 — Indent relative to the scope

Class: clarity

Objective: keep the tree's indent meaningful at any depth.

Dependencies: M017.

Deliverable boundary: `display_depth` renders the indent **relative to the
scope**. Under peek the absolute scope depth is unbounded while the relative
depth stays within 0-2, so the absolute clamp flattened exactly the deep
scopes peek is designed to reach.

User value: the tree's shape is readable at any depth.

Exit conditions: `display_depth` is relative to the scope; the two existing
tests that assert the old absolute contract are rewritten to the new one.

### M019 — Open a file and land in the editor, in Normal mode

Class: capability

Objective: make opening a file hand the keyboard to the editor.

Dependencies: none.

Deliverable boundary: after a file opens successfully, the tree's focus is
released (`file_tree::set_focus(app, false)`) and the editor is put in
`EditorMode::Normal` (`commands::editor::enter_editor_normal`). The handoff
runs on the **success** path, not the request path: handing the keyboard to
an editor whose document has not attached yet would show the reader an empty
buffer.

User value: opening a file from the tree lands the reader in the file, ready
to edit, rather than in a file whose caret is somewhere else.

Exit conditions: a successful open releases the tree's focus and forces
Normal mode; a failed open leaves the tree focused.

### M020 — The keymap the tree shows is the one it has

Class: clarity

Objective: retire the verb the peek motion replaced.

Dependencies: M017.

Deliverable boundary: the tree's advertised keymap names the peek motion and
the insert-mode filter, and no longer names a verb that does not exist.

User value: the line that tells the reader what the tree can do is true.

Exit conditions: the tree's hint line names only chords the tree handles.

## 6. Current state

**M001–M020 are closed.** Every milestone's closure record is in
`plans/closure/tui-editor-helix-ux-improvement/`, and each one states its
exit conditions as met or not met with the evidence that says so.

Two items are **deferred and are not milestones here**: a general count prefix
(`123G`) and a configurable `editor.scrolloff`. Both are listed in §8 and in
the research document's §0.0.

## 7. Verification

- `scripts/verify.sh quick` — the canonical sanity sweep, a superset of CI.
- `scripts/verify.sh full` — quick plus Clippy and the workspace tests.
- `cargo nextest run --workspace --locked --profile ci` — the capped suite.
- `cargo fmt` — rustfmt at `max_width` 100.

The closure records for this roadmap are
`plans/closure/tui-editor-helix-ux-improvement/001-status.md` through
`020-status.md`.

The guards that cover this surface are `check_tui_editor_key_authority.py`,
`check_tui_editor_prefix_state.py`, `check_tui_editor_apply_path.py`,
`check_tui_editor_completion_authority.py`,
`check_tui_editor_text_authority.py`, `check_tui_editor_leaf_authority.py`,
`check_tui_editor_doc_drift.py`, and `check_daemon_listing_authority.py`.
