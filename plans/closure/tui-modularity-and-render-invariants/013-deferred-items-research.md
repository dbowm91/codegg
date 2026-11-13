# M013 — The four deferred items, and what each actually needs

Research memo, 2026-11-13

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`, worktree
uncommitted (nothing committed). The tree is green: `[contrast] = 0`,
`[advertised_keys] = 4` (each with a `reason`), `[hint_truncation] = 0`, render
corpus 1237/0.

**What this memo ran, and what it only read.** I ran
`python3 scripts/check_comment_budget.py` (passed) and the guard's own scanner
for per-file and whole-corpus comment mass. I did **not** run the render corpus
or `verify.sh` in any mode, so every corpus figure below is the state the
records report, not a re-measurement. Every claim about a fixture, a key arm or
a row's width was read out of the source at the cited line; the arithmetic is
mine and is reproducible from the strings in the tree.

Related:

- `plans/closure/tui-modularity-and-render-invariants/005-status.md` — the
  residue table that names these four items.
- `plans/closure/tui-advertised-keys-corrective/002-status.md` §3–§4, §10 — the
  parser widening and its twelve rows.
- `plans/implementation/tui-modularity-and-render-invariants/012-hint-producer-convergence.md`
  §5 (scope), §9 (the widening's deferral), §14 (stop conditions).

## 0. Two measurements that correct the premises

### 0.1 Comment headroom is 15 lines globally, not zero — but seven of the eight files listed are at zero

Measured with the guard's own scanner (`scripts/rust_test_scan.py`, the same
`measure` the guard uses):

| File | Comment lines | Ceiling | Headroom |
|---|---|---|---|
| `src/tui/components/dialogs/security_review.rs` | 64 | 64 | **0** |
| `src/tui/components/dialogs/keybind.rs` | 46 | 46 | **0** |
| `src/tui/components/hint_line.rs` | 51 | 51 | **0** |
| `tests/tui_render/invariants/baseline.rs` | 315 | 315 | **0** |
| `tests/tui_render/invariants/fixtures.rs` | 144 | 144 | **0** |
| `tests/tui_render/invariants/vocabulary.rs` | 107 | 107 | **0** |
| `src/tui/components/dialogs/diff.rs` | 107 | 107 | **0** |
| `src/tui/app/mod.rs` | 2877 | 2884 | **7** |
| *(global)* | **139,230** | **139,245** | **15** |

The global figure is the one place the premise is wrong: the corpus is **15
lines under** its ceiling, so a comment line can be added without a re-bank as
long as fewer than 16 are added. Every per-file ceiling still binds, and for
seven of the eight files listed the ceiling is *exactly* the measurement — a
single new line there fails `R4`. There is no "unbanked reduction" rule in the
guard's R4 block (it checks `measured > recorded` only) — and the guard passes
on this tree with the corpus 15 lines *under* its ceiling, which is the direct
proof. So a **reduction** is free to land; it is `--bank` that records it.

### 0.2 The record's "60 columns" is a *budget*, not a terminal width

`SecurityReviewDialog` has no `preferred_area` override, so the modal host gives
it `centered_rect(60, 50, available)` (`src/tui/components/component.rs:285-287`),
and the footer is drawn inside `Section::plain(...).draw(status_area, ...)`,
which spends one row on each border: the footer's inner width is
`⌊0.6 × terminal⌋ − 2`, not the terminal width.

The row is **113** columns with the default `All` filter and **126** with
`Medium+ severity` (the filter label is `src/security/workflow/receipt.rs:136`;
the row is rebuilt on every render, `security_review.rs:512-522`). So:

| Terminal cols | Modal (60%) | Footer inner | Verbs shown (All) | Verbs shown (Medium+) |
|---|---|---|---|---|
| 60 | 36 | 34 | **2** | 2 |
| 80 | 48 | 46 | **3** | 3 |
| 100 | 60 | 58 | **4** | 3 |
| 120 | 72 | 70 | **5** | 4 |
| 160 | 96 | 94 | **6** | 5 |
| 192 | 115 | 113 | **8** | 7 |
| 214 | 128 | 126 | **8** | **8** |

The figures in circulation — "4 of 8 at 60 columns, 7 of 8 at 100" — are the
**budget** values (inner 58 → 4; inner 98 → 7). At a 60-column *terminal* the
box shows **two** verbs; at a 100-column terminal it shows four. Both records
are internally consistent; only the shorthand is misleading, and it matters,
because it is the difference between "a wide terminal nearly fits" and "the
common case shows half".

---

## 1. `/security-review` geometry

### What it needs

Nothing that reaches the goal. The row shows all eight items at an inner width
of 113 (All) or 126 (Medium+); the footer's inner width is `0.6 × terminal − 2`.
Every candidate fix was costed:

- **A wider dialog.** `clamp_area` caps a dialog at 90 % of the frame
  (`dialog_size.rs:19`). At a 100-column terminal that is 90 wide → inner 88 →
  **6 of 8**, and the cap makes 8/8 structurally unreachable below a
  ~128-column terminal. A `preferred_area` override buys one or two verbs and
  cannot reach the stated target in the common case.
- **A shorter item set.** All eight verbs are real in the modal: `h` jumps the
  detail pane to the hunk (`security_review.rs:335-338`), `H` copies it
  (`:340-364`), `]/[` move between hunk-backed items (`:366-374`). Removing
  items to make the row fit is plan 012 stop condition §14.5 verbatim ("do not
  remove items from the row to make it fit ... this is the
  `/security-review` temptation"), and it would hide working keys.
- **A two-line footer.** There is no row-count API on `HintLine` (`render` is
  the only renderer, `hint_line.rs:90-94`), so this needs a new builder method,
  a taller chunk (`Length(3)` → `Length(4)`), and a minimum-size arm: at
  `20×6` the box already early-returns (`:421`), and `header 3 + footer 4` is
  7 of 6 rows. At budget 58 two rows fit (54 + 56); at 46 it needs **three**;
  at 34 it needs **four**. So the option fixes 100-column terminals and not
  80-column ones — i.e. it moves the boundary from "4 of 8" to "8 of 8 at ≥100
  columns, 6 of 8 at 80".

### Cost

Two-row version: one new `HintLine` method plus its doc (that file has **0**
comment headroom), ~10 lines in `security_review.rs`, one layout constant, a
minimum-size test, and a new test at production geometry. Rendered bytes
**move** at every width. No `tests/tui_render` member asserts this footer's
bytes — the only assertion is the lib test
`security_review.rs:810 the_row_is_clipped_at_an_item_boundary_not_mid_item` —
so the 1237-test corpus does not re-bank; that one lib test must be rewritten
(it would fail, which is the point). The lib test also measures the **component
rect** (`dialog.render(frame, frame.area(), …)`), i.e. inner 58 at "60×15",
where production at a 60-column terminal gives 34. That instrument is the
reason the loss looked smaller than it is.

### Should it be done

**No.** The criteria the milestone owns are already met and are the ones that
matter: the row truncates at whole-item boundaries, the exit item is first and
protected, and no row ends mid-item (`hint_line.rs:131-153`, criterion 5 of
012 §13). "Every verb is visible at every width" was never a criterion, and the
arithmetic above shows no layout choice in this box can deliver it below a
~192-column terminal. A two-row footer is a builder feature — a per-row budget
with an item-priority order — that must be derived and measured across all 38
painted rows, not a fix for one dialog. Record the measured table; do not carry
it as deferred work.

### What would prove it, if it were ever done

A test that renders at the **dialog's own rect**
(`dialog.preferred_area(Rect::new(0, 0, w, h))`) rather than at `frame.area()`,
at 60×24, 100×30 and 160×40, asserting for each width (a) no row ends mid-item
and (b) the second row's first item. The existing lib test fails that
instrument today — it would pass while production shows two verbs.

---

## 2. `/keybind`'s `Widget for &KeybindDialog`

### What it needs

Deletion of `src/tui/components/dialogs/keybind.rs:456-586` (131 lines: 120
code, 11 blank, **0 comment**) and `Widget` off the `ratatui::widgets` import at
`:7`. Nothing else: no caller exists.

Measured, not assumed:

- `grep -rn "KeybindDialog" src/` returns the state field
  (`app/state/dialog.rs:33`), one construction (`app/modal.rs:431`), the action
  list in `app/mod.rs:5339`, and this file. No `render_widget(&…)` call takes a
  `KeybindDialog` (`render_widget(&` appears only for messages, the prompt, the
  status bar and `share.rs:240`'s own widget dialog).
- `apps/` has no reference at all, so the `pub` surface is not consumed by the
  desktop host either.
- The impl is also **wrong** in three ways the live path fixed, which is the
  argument against keeping it as a reference:
  1. the selected row is `fg(primary)` on `bg(selection)` (`:473-484`), where
     the live path uses `selection_text` and says why
     (`:326-339`: "`primary` is an accent and fails that fill on 14 themes");
  2. Export's footer is the prose `"Press Esc to close"` (`:546`), which is
     false for the reason item 3 establishes;
  3. the two hint rows it does have are appended to a body list that is taller
     than its rect (`:505-521`, `:571-575`) — the exact defect 012's WP E fixed
     in `render_body`.

Converting it (the alternative the 005 record considered) would preserve all
three defects in a second implementation with no test and no caller.

### Cost

One edit, one file. **The comment ratchet is the trap, and it is the ratio, not
the line count.** Deleting 120 code lines while removing 0 comment lines moves
the file's ratio from `46/756 = 0.0608` to `46/636 = 0.0723` against a recorded
`max_ratio = 0.0676` — a violation of `R4` that no amount of "the corpus shrank"
offsets. The parent must either lower `keybind.rs`'s `max_comment_lines` to 42
(and delete four of its 46 comment lines, all of which are load-bearing) or
re-bank `docs/comment-budget.toml`. Cheap in code; not free in the ratchet.

Rendered bytes: **none**. Corpus: unchanged (1237).

### Should it be done

**Yes.** It is dead code that a reader will mistake for the implementation —
131 lines of a fourth entry point into this box (`render` → `render_body`,
`render_in_panel` → `render_body`, and this) — and it is the only one of the
four items where the defect is unambiguous and the fix is subtraction. Deletion
is strictly cheaper than conversion and strictly safer than a comment saying
"do not use this".

### What proves it

There is no honest "test that fails without the change" for a deletion. The
proof is the inverse and should be quoted as such: the tree compiles with the
impl gone, `every_mode_paints_its_row_in_the_reserved_chunk`
(`keybind.rs:830-867`) still passes, the corpus is still 1237/0, and

```bash
grep -rn "impl Widget for &KeybindDialog" src/ tests/ apps/   # empty
grep -rn "render_widget(&" src/tui | grep -i keybind           # empty
```

If the parent wants it mechanical, the guard shape already exists: the
painted-row vocabulary test would go red if a second implementation were
reintroduced *and wired*, but not if it were merely re-added. A one-line
absence assertion is the only mechanical form, and it is arguably not worth a
guard; the grep above is the evidence.

---

## 3. Export's verb is `Esc back`

### What it needs

**A test, not a content change.** The row is already right:

- `mode_footer` returns `HintLine::from_items(&[("Esc", "back")])` for Export
  (`keybind.rs:47`), and `render_body` paints it in the reserved chunk
  (`:416-438`).
- The live dispatch agrees. `/keybind` is intercepted before the `FocusManager`
  (`app/mod.rs:4683-4686` → `handle_keybind_key`), and Export's `Esc` calls
  `kd.cancel_mode()` (`:5355-5359`), which resets `mode` to `Normal` and clears
  `conflict` (`keybind.rs:166-169`) — the list returns, the dialog stays open.
- Only Normal's `Esc` closes: `handle_keybind_key`'s Normal arm takes
  `InputAction::Cancel` → `keybind_dialog = None; close_dialog()`
  (`app/mod.rs:5382-5385`). So `Esc close` is correct for Normal and `Esc back`
  is correct for Export. (The 005 record cites `app/mod.rs:5361-5366`; the arm
  has moved — it is `5355-5359`.)

**Which verb is right, and why `back` rather than `cancel`.** Export carries no
user-owned state: `export_text` is *derived* from the bindings on entry
(`start_export`, `:136-151`) and Export mode has no input, so leaving it
discards nothing — "cancel" would claim a loss that does not happen, and
"close" claimed a dismissal that does not happen. Import's `Esc cancel` is the
mirror image and is also right: `import_text` *does* accumulate typed
characters (`app/mod.rs:5371-5376`) and `cancel_mode()` discards them. The
asymmetry between the two rows is content, not drift, and both should stay.

Nothing else belongs on the Export row: the app-level Export arm answers only
`Esc` (`:5355-5359`). `Component::handle_key` additionally maps `Enter` to
`cancel_mode()` (`keybind.rs:614-616`), but that path is the *docked column's*,
and the Export footer is Owned-only — advertising `Enter` would describe a
dispatch the modal never performs.

### Cost

Zero production lines. One app-level test (below).

### Should it be done

**Yes — the test.** The verb needs no change and the prose is already gone from
the live path; the dead path's prose goes with item 2. What is missing is
coverage of the claim: **no test anywhere exercises `handle_keybind_key`**
(`grep -n "keybind_dialog\|KeybindMode" src/tui/app/mod.rs` returns only the
two initialisers and the handler itself). The row advertises a behaviour whose
implementation is untested, and the pre-fix prose proved how easy it is for that
claim to be false.

### What proves it

A test that puts a dialog in Export, sends `Esc` through
`handle_keybind_key` — the dispatch production uses, not
`Component::handle_key` — and asserts `mode == Normal` and the dialog is still
mounted. It fails if anyone re-routes Export's `Esc` to `close_dialog()`, which
is exactly the behaviour the old prose promised.

---

## 4. The I4 parser widening

### What it needs

In code, small: seven or so arms in `key_of`
(`tests/tui_render/invariants/baseline.rs:367-407`) for `Home`, `End`,
`PageUp`/`PgUp`, `PageDown`/`PgDn`, `Backspace`, `⌫`, `[`, `]`; shrink
`UNREADABLE_SPELLINGS` (`:349-360`); rewrite `the_parser_*` (`:1569`) — its
first loop asserts every listed spelling is *unreadable*, so the widening is
red until the list moves; and delete the hard-coded `Queue`/`[` assertion in
`the_i4_report_names_only_documented_spellings` (`:1559-1565`).

In evidence, large — and the record's plan of record does not survive a read:

1. **Two of the surfaces the record names are not rows at all.** `TaskView`'s
   `Home`/`End` return `Some(TuiMsg::TaskViewMove { delta: ±10_000 })`
   (`task_view.rs:223-224`) — `Some` is `PressOutcome::Claimed` before any
   repaint is considered (`baseline.rs:597-599`). `Queue`'s `[`/`]` call
   `reorder_selected` (`queue.rs:1135-1137`) over the fixture's `LIST_ROWS`
   entries, which repaints. Neither becomes a recorded row.
2. **`ProjectChat`'s `]` is *pressed* only from the declaration.** The item is
   one string, `("[ ]", "channel")` (`chat_browser.rs:832`). `item_spellings`
   splits on whitespace *and* `/` (`baseline.rs:644-650`), so the declaration
   yields both keys; `spellings` takes only the first whitespace token
   (`:415-418`), so the painted row yields `[` alone — the two sources disagree
   about how many keys one item carries. Either way, with one channel
   `step_channel` returns `ChatKeyAction::Consumed` (`chat_browser.rs:466-470`)
   and both presses are inert. Two rows, fixed by a two-channel snapshot
   (`fixtures.rs:689-697` already builds `ChatSnapshot` with `..default()`, so
   this one really is ~8 lines).
3. **`Home` and `PgUp` cannot be fixed by any fixture.** A fresh mount starts
   at the top, and the only position prelude the harness has is
   `is_up_direction` = `Up | k` (`baseline.rs:611-613`, applied at `:777`). At
   the top, `Home` resets scroll 0 → 0 and `PgUp` does `-10` → 0; both are
   `Inert` **by construction**, in any fixture, because the verdict must be "a
   function of the key alone, never of the order keys are pressed in"
   (`:623-624`). Making them non-inert needs the prelude widened to a
   *top-direction* class — a change to the measurement's semantics, not to a
   fixture. `Diff` is the surface that shows this: `Home` and `PgUp` are
   unfixable and `End`/`PgDn` repaint (and, incidentally, see the adjacent
   finding in §7).
4. **The `SecurityReview` hunk fixture is possible, but only one way round.**
   `h` and `H` both act on `selected_item()`, and the fresh mount starts at
   index 0 (`security_review.rs:148-188`, `:340-364`). Put the hunk on a
   *later* item and `[`/`]` navigate to it (handled, no longer rows) while `h`
   and `H` stay correct no-ops on item 0 and `H` never reaches
   `copy_to_clipboard`. Put it on item 0 and the fixture presses a key whose
   banked reason is "a render measurement must not write the operator's
   clipboard" (`docs/tui-render-invariant-baseline.toml:176-184`). So this one
   is cheap *if* the fixture is designed around a banked row it must not
   disturb — a constraint the record does not mention.
5. **The count is not the record's.** `Diff` 4 + `SecurityReview` 4 +
   `TaskView` 2 + `ProjectChat` 2 = 12, then "one each" for five more = 17
   (012 §9 noticed the arithmetic). Re-derived from the source, and **not
   measured** (I did not run the harness), the set is roughly: `Diff` 2,
   `SecurityReview` 1–3, `ProjectChat` 2 (both fixture-fixable), `Model` 1
   (`Backspace` clears an empty filter, `model.rs:1082`), `Connect` 1
   (`Backspace` outside a text-input step, `connect.rs:1281`),
   `SourcePreview` 1–2 (`PageDown`/`PageUp` past a short body, `:160-167`), and
   **0** for `TaskView` and `Queue` — so 6–10, and it depends on a design
   choice: mapping `⌫` to `Backspace` (then `Research`/`Review`'s `⌫ back` is
   handled and records nothing) or to `Char('⌫')` (then it records two more
   rows). Whichever number is right, `Diff`'s two and `SecurityReview`'s
   `PgUp` are fixtures' business in no sense at all.

### Cost

Code: ~10 lines in `key_of`, ~10 lines of list edits, ~10 lines of test edits —
but `baseline.rs` has **0** comment headroom and `fixtures.rs` **0**, so every
comment line there needs a manifest edit — a per-file one, not just the global.
Evidence: 6–10 new `[advertised_keys]` rows,
each carrying a reason whose content is "the harness's fixture cannot ask" —
which is not what the ratchet is for. The four banked rows each name why the
**product** cannot answer (`Tab` is `FocusManager`'s; `H` writes the
clipboard). A fixture gap is a debt of the harness, and banking it in the
product's ratchet makes the ratchet lie about the product. Rendered bytes:
**none**. Corpus: unchanged (1237); the guard fails on the unbanked rows
instead (`baseline.rs:317-320`: "the harness measures … and the manifest does
not record it").

### Should it be done

**No.** Keep the parser narrow and keep `UNREADABLE_SPELLINGS` as the asserted
blind spot — that is what the list is for, and it already carries a property
test (`:1569-1585`) that makes the limit impossible to widen silently.

The argument is not "widening is hard". It is:

- the rows it records are **fixture gaps**, which belong in the fixtures, and
  two of them cannot be fixed by fixtures at all;
- the measured set is not the record's, so "bank twelve rows" is not a
  description of an achievable end state;
- one of the fixtures it needs would press a key the ratchet deliberately
  protects the operator from.

**The one thing that is genuinely wrong today is adjacent, and it is cheap.**
`spellings` filters unreadable spellings out
(`:415-418`, `.filter(|s| key_of(s).is_some())`), so only *declaration* holes
are reported; a **painted-only** advertisement the parser cannot read is
silently dropped. Concrete example: the chat footer paints `jk move`
(`chat_browser.rs:822`, `:836` — one item, two keystrokes), `key_of("jk")`
returns `None`, and `jk` is not in `UNREADABLE_SPELLINGS` because the painted
path never reports. So when the record says "the blind spot is named", it is
named for declarations only. If the parent wants one small correction to this
item's subject, it is to record that asymmetry in the harness's own doc
comment — a comment line in a file with zero headroom, so it needs a re-bank —
or to report painted-unreadable spellings through the same channel, which would
go red on `jk` and needs a decision about whether `jk` is one key or two.

### What proves it

Nothing, if dropped. If it were ever taken, the proof is a *re-measured*
per-key report (`cargo test --test tui_render -- the_i4_per_key_report
--nocapture`) whose INERT set is the four banked rows and no more — taken
**after** the fixtures were seeded, not before, so the widening adds no row
whose reason is a harness limitation.

---

## 5. Recommendation table

| # | Item | Verdict | One line |
|---|---|---|---|
| 1 | `/security-review` geometry | **drop** | 8 verbs need 113 (All) / 126 (Medium+) inner columns — a ≥192/214-column terminal; the 90 % dialog cap gives 90 at a 100-column frame, so no layout choice reaches it, and a 2-row footer fixes only the ≥80-column half while costing a builder API, a taller chunk and bytes. Record the table instead. |
| 2 | `keybind.rs`'s `Widget for &KeybindDialog` | **do** | 131 lines, no caller in `src/`, `tests/` or `apps/`, and it is wrong in three ways the live path fixed; deletion is cheaper than conversion and cheaper than keeping a trap. |
| 3 | Export's verb | **do (test only)** | `Esc back` is the right verb and the row already says it; the missing piece is a test of `handle_keybind_key`, which is the dispatch production actually uses and which nothing exercises today. |
| 4 | The I4 parser widening | **drop** | The rows are fixture gaps, two of the surfaces it names are not rows at all, `Home`/`PgUp` are inert by construction at a fresh mount, and the one fixture that would help has to be built around `H`'s banked reason; the real hole (painted-only spellings such as `jk`) is not closed by widening. |

Items 1 and 4 should be recorded in the 005 closure record's residue table as
**decided, not deferred** — with the arithmetic above as the reason — so a later
pass does not re-open them as unfinished work.

---

## 6. Plan sections for the two items to be done

Both are small enough to be one work package each; they are ordered so the
deletion lands first (it touches no test) and the new test lands second.

### Work package A — Delete the second keybind render (item 2)

**Intent.** Remove the only render path in the tree that no caller reaches and
that contradicts the one that is reached, so the next reader cannot copy a
fixed defect back in.

**Required changes.**

- `src/tui/components/dialogs/keybind.rs` — delete `impl Widget for
  &KeybindDialog { … }` (`:456-586`) and drop `Widget` from the
  `ratatui::widgets` import (`:7`). Nothing else: `render_body` (`:310`) is the
  one routine the two live paths share, and it already owns the reserved
  footer chunk.
- `docs/comment-budget.toml` — **parent-owned.** The deletion removes 120 code
  lines and 0 comment lines, moving the file's ratio from 0.0608 to ~0.0723
  against a recorded 0.0676. Either lower `keybind.rs`'s `max_comment_lines` to
  42 and remove four comment lines, or re-bank the manifest. This is the only
  reason the change is not a pure subtraction.

**Acceptance evidence.**

- `cargo clippy -p codegg --lib --tests -- -D warnings` is clean (the unused
  `Widget` import is a hard error under `-D warnings`, so this is the compile
  check for the import).
- `cargo test -p codegg --lib -- tui::components::dialogs::keybind` still
  passes, including `every_mode_paints_its_row_in_the_reserved_chunk`.
- `cargo test --test tui_render` is still 1237/0 (no rendered byte moves).
- `grep -rn "impl Widget for &KeybindDialog" src/ tests/ apps/` is empty.
- `python3 scripts/check_comment_budget.py` passes on the edited manifest.

**Rollback.** Restore the impl; nothing depends on its absence.

### Work package B — Pin Export's `Esc` (item 3)

**Intent.** Give the row's verb a test of the dispatch that actually runs, so
`Esc back` cannot silently become a lie the way `Press Esc to close` was.

**Required changes.**

- `src/tui/app/mod.rs`, inside the existing `#[cfg(test)] mod tests` — one test
  built on `App::new_for_testing("/tmp".into())` (the constructor the other app
  tests use, e.g. `:14689`): construct a `KeybindDialog`, `start_export()`,
  store it in `app.dialog_state.keybind_dialog`, call
  `app.handle_keybind_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))`,
  and assert `mode == KeybindMode::Normal` **and** that
  `dialog_state.keybind_dialog` is still `Some`.
- No production change. No comment is required by
  `check_test_assertion_quality.py`; if a doc comment is added, `app/mod.rs`
  has 7 lines of per-file headroom and the global corpus has 15.

**Acceptance evidence.** The test fails if the Export arm is changed to
`self.dialog_state.keybind_dialog = None; self.close_dialog();` — which is the
behaviour the deleted prose promised — and passes on the tree as it stands.
Corpus **unchanged at 1237**: the test belongs in `app/mod.rs`'s lib test
module, so it moves the lib count, not a `tui_render` member.

---

## 7. Adjacent findings, and what I could not determine

**Found while reading, out of scope, not fixed, not measured by a run:**

1. **`Diff`'s `End` almost certainly blanks the pane.** `scroll_to_bottom` sets
   the scroll to `total_lines() - 1` (`diff.rs:302-305`, the sum over *all*
   hunks), while every paint applies `scroll_offset` **per hunk**
   (`dialogs/diff.rs:165`, `:296`, `:397` — all three render paths use
   `hunk.lines.iter().skip(scroll_offset)`). With
   `total_lines() = 160` and two hunks, both hunks are skipped entirely. The
   same per-hunk skip makes every scroll offset mean something different from
   what the scrollbar implies. Read out of the source only; I did not render
   it. It is worth its own record, and it is a reason not to treat a
   `Repainted` verdict from I4 as evidence a key is *correct* — only that it
   did something.
2. **`spellings` silently drops painted-unreadable keys** (§4), so the named
   blind spot covers declarations only; `jk` is the example.
3. **`/keybind`'s `Component::handle_key` is not the modal's dispatch.** The app
   intercepts `DialogType::Keybind` before the `FocusManager`
   (`app/mod.rs:4683-4686`), so that method answers a *docked* column only.
   I4 presses the component, so the Keybind verdicts in the per-key report
   describe a path the modal never uses. That is a measurement-authority gap,
   not a defect in this milestone's work, and it is the same class as plan 012
   §16's note that `painted_hints` measures every box at full width.
4. **The two records disagree on the harness's own count** — 005 reports 238
   presses, C002 reports 225. Neither is wrong for its own tree (rows were
   converted in between); it is a reminder that these figures are timestamped.

**What I could not determine:**

- **The exact widened row set.** Deriving it needs the harness to run, which
  this session was not permitted to do. My 6–10 is read out of the arms and
  fixtures and is offered as a refutation of the record's 12/17, not as a
  replacement figure.
- **Whether `End` blanks the `Diff` pane in every configuration** (finding 1) —
  I read the code and did not render it.
- **How a `SecurityReview` `preferred_area` override would interact with
  `clamp_area`** at frame widths below 20 — not on the path of anything
  recommended here.
