# TUI Modularity and Render Invariants Milestone 012 — Hint producer convergence

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

**Repository state at the time of writing.** The tree is **uncommitted**:
`7642b9e38` is HEAD, and every file this plan names exists only as
working-tree work on `feat-auto-20261109-f8abad0d` (`src/tui/components/hint_line.rs`
is still untracked, so it has no predecessor to diff against). `scripts/verify.sh full`
was green on this tree at **2026-11-13** — 17,368 workspace tests and 8,433
feature-pass tests, 0 failures. The ratchets read `[contrast] = 0`,
`[advertised_keys] = 4` (each with a `reason`) and `[hint_truncation] = 0`;
the render corpus is 1236 tests. Re-measured while writing this plan:
`python3 scripts/check_tui_render_invariant_baseline.py` reports *"4
advertised-key, 0 contrast and 0 hint-truncation entries recorded; every one
reproduces and none was added unbanked"*, `check_tui_render_cost.py` passes, and
`check_comment_budget.py` passes against the banked 139,245 lines.

Source milestone this corrects:

- `plans/implementation/tui-modularity-and-render-invariants/005-one-hint-vocabulary.md`
- `plans/closure/tui-modularity-and-render-invariants/005-status.md` — the
  specification: status `corrective pass required`, with its residue table and
  the owner directive of 2026-11-13.

Adjacent residue this plan decides but does not take:

- `plans/closure/tui-advertised-keys-corrective/002-status.md` §3–§4.

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the surface
  presents what the product can do; a hint that cannot be painted presents
  nothing.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — a hint
  describes a key and never decides which stage claims a chord. This milestone
  changes producers and layout, never ownership.

Primary class: capability

## 1. Objective

Make M005's acceptance criterion 1 true for the rows it is actually about, and
restate it so that it is about those rows. Concretely: converge every painted
`<key> <verb>` row on `HintLine`; make the two rows that are converted but never
painted paint; make the one row that is painted but clipped truncate whole
items; and record the surfaces that are *not* hint rows so a later pass cannot
mistake them for unfinished work.

The deliverable is a tree where "one hint vocabulary" is a property of the
painted product rather than of a declaration the product may never draw.

## 2. Why this milestone is ready

The hard dependency is M005's own corrective, and it is a *scope* dependency
rather than a code one: the builder, the trait seam and the painted-frame scan
all exist and are green, so this pass adds no machinery — it finishes the
conversion the previous corrective measured and left.

Three things make it ready rather than speculative:

1. **The scan already sees the class.** `every_painted_advertisement_row_uses_the_one_vocabulary`
   (`tests/tui_render/invariants/vocabulary.rs:410`) went red on 82 findings
   across 28 rows on 38 surfaces before the previous pass, and the 35 converted
   rows are green. The detector is not in question; the coverage is.
2. **The residue is measurable, and smaller than the record says.** Every site
   the 005 closure record names was re-read for this plan (§3). Of the eight:
   one is dead state with no reader at all, two are dead because a dialog
   discards them by its own rule, one is a diagnostics value line, one is a row
   fragment already in the right dialect that arrives as an opaque item, two are
   painted rows still in the old dialect, and one is a painted row the box
   renders empty.
3. **Two of the three layout defects have a single-line root cause each**,
   verified against the vendored `ratatui` source (§3.4, §3.5) rather than
   inferred from a screenshot.

There is one genuine regression on the tree that this plan must fix and that no
record currently names: the `/terminal-show` footer is **blank**, not clipped
(§3.3). It is the strongest single argument that the corrective is unfinished
work rather than a tidy-up.

## 3. Current implementation evidence

Every claim below was read out of this tree at the cited line. Numbers marked
*measured* were produced by a command quoted in §11 or in this section.
`HintLine` is `src/tui/components/hint_line.rs`; the vocabulary module is
`tests/tui_render/invariants/vocabulary.rs`.

### 3.1 The builder and the scan (context, not scope)

- `HintLine` owns the separator (`HINT_SEPARATOR = " · "`, `:15`), lowercases
  the verb on `push` (`:66`), orders required items first (`:76`, `:57`), and
  renders through `fit_parts` (`:126`), whose first element is protected: a
  budget below the first item's width returns `""` rather than a partial key
  (`:132-134`).
- `Component::keymap_hint_line` (`src/tui/components/component.rs:241`) is the
  declaration, `keymap_hint` (`:255`) is its rendering, and `advertised_hints`
  (`:264`) is the multi-row form. 14 dialog modules override the declaration.
- The painted scan (`vocabulary.rs:257-302`) renders each content-bearing
  fixture at 100×30 through `capture::modal`, splits each frame row at the
  column rules, and counts a column as an advertisement only when **≥2 of its
  items are `<key> <verb>`** (`is_advertisement_row`, `:251`). The floor is 47
  painted rows (`PAINTED_ROW_FLOOR`, `:443`), and `panel_and_row_hints`
  (`:314`) adds the three panels plus `ComposerRow`.

### 3.2 The eight producers the record names — measured, one by one

**1. `src/tui/app/mod.rs:5987-6000` — "mouse-hover context hints": dead state.**
`update_context_hint` writes `self.context_hint` from a `ClickTarget` match.
The field is declared at `:480`, initialised at `:1236` and `:1827`, written at
`:5983-6004` from `:6100`, and **read nowhere**. A repo-wide search for
`context_hint` returns exactly those six sites and no consumer — no `Line`, no
`Span`, no paragraph, no status row, no test. Nothing is painted, so nothing is
in a dialect. The nearest real hover surface is a different mechanism: the
sidebar's `tooltip_text` (`src/tui/components/sidebar.rs:262`, painted as a
bordered one-line overlay at `:2246-2269`), whose strings are prose
(`"Click to collapse/expand goal"`, `:2020`) attached to a cell, not a row of
items.

**2. `src/tui/app/render.rs:1207` — painted, drifted.**
`render_home` paints `"  Ctrl+N  new session    Ctrl+L  change model    ?  help"`
centred on the empty-buffer screen. It is a real advertisement of three keys,
it is in the second dialect (bare double-space separators, no ` · `), and it
has no exit item because the empty buffer has no mode to leave. No test asserts
its bytes: the two home-screen tests (`tests/tui_render/tui_render.inc:415`,
`:428`) only assert that no "Rendering Error" appears and that the render does
not fail.

**3. `src/tui/chat_browser.rs:818-837` — right dialect, wrong granularity.**
`normal_hints(show_help)` returns a `&'static str` of 9 or 10 items and
`input_hints(seed)` returns a `String` built with `format!`. Both are already
` · `-separated with lowercase verbs. The defect is at the seam: the two
callers push the whole returned string as **one** `HintLine` item —
`panels/chat.rs:404` (`hint.push("CHAT","").push(normal,"").push("⇧S","panel")`)
and `components/dialogs/chat.rs:446-447`
(`hint.push_required("Esc","close").push(normal_hints(...),"")`) — and the
composer's chat-seed branch bypasses the builder entirely with
`format!("CHAT · {}", input_hints(seed))` (`app/mod.rs:10277-10281`).

Consequence, and the reason this is not cosmetic: `fit_parts` treats each item
as indivisible, so at a narrow width the whole nine-or-ten-item fragment is
dropped at once and nothing of the chat row's verbs survives, and `items()`
reports one item whose "key" is the entire row, which is a false structured
advertisement. The rendered bytes are identical today
(`push(s, "").render(0)` joins with the same ` · ` the fragment already
contains), so the structural conversion is free.
Separately, `input_hints` puts `Esc` **second**
(`"↵ send {label} · Esc cancel {label} (keeps text)"`), which is an exit-key
order defect in a footer that the box clips.

**4. `src/tui/commands/interactive_terminal.rs:1718-1728` — painted, drifted,
and blank.** The `/terminal-show` footer is 181 columns:

```
j/k scroll  |  ^-u/^-d page  |  e/b word  |  v select chars  |  x select lines  |  d delete  |  space+y copy  |  C clear  |  i focus  |  ^-] unfocus  |  D diagnostics  |  Esc closes
```

It is handed to `InfoDialog::set_custom_footer` (`:1739`, `:1747`), and
`InfoDialog` renders a caller footer by pushing it as **one item**:
`hint.push(custom, "")` (`src/tui/components/dialogs/info.rs:344`, `:347`). The
budget is the footer box's inner width (`:340`). Since `fit_parts` returns `""`
when the first item alone exceeds the budget (`hint_line.rs:132-134`), and the
modal is 60 % of the terminal width (`dialog_size::preferred_rows`,
`src/tui/components/dialog_size.rs:31-40`), the row is **empty at every
terminal narrower than ~305 columns** — including the 100×30 fixture. This is a
live regression from the M005 conversion, not a deferred defect: before the
budget was introduced the same string was at least drawn and clipped.

**5–6. `src/tui/commands/shell.rs:1555` and `:1683` — never painted, by
design.** `show_shell_detail` (`:1559`) always uses `InfoType::ShellShow`, and
`InfoDialog::renders_caller_footer` returns `false` for that type
(`info.rs:178-180`), so `set_custom_footer` at `:1568` / `:1575` stores a string
the box never draws. That is deliberate and load-bearing: the five verbs the
row names (`i`/`a`/`r`/`k`/`e`) have no `handle_key` arm, and
`dialog_dispatch_coverage::the_shell_show_modal_advertises_only_keys_it_handles`
constructs the same string to prove the box does not paint it. Neither site is
a producer; both are a trap for the next author, and both are M006's evidence.

**7. `src/tui/interactive_terminal.rs:2165` — not a hint row.**
`"focus:       {} (Esc leaves focus; input needs explicit focus)"` is one of
six labelled value lines in the terminal diagnostics dump
(`full_header_lines`, `:2146`). It mentions a key; it is not a row of items, and
`is_advertisement_row` does not classify it as one.

**8. `src/tui/runtime/command_dispatch.rs:128` — painted, drifted.**
`/project-init` sets `"a: publish   Esc: cancel"` on the modal `InfoDialog`.
`ProjectInit` renders caller footers (`info.rs:178`), the row is 24 columns, and
it fits at every width, so this one paints. Its form is the retired one: colon
separators, double spaces, exit second.

### 3.3 The producers neither list names

Three more sites are painted or structurally relevant, and two of them matter
to the criterion:

- **The tasks strip row** — `src/tui/app/render.rs:1462`:
  `" Shift+N/P open · ↵ select · i inspect · Esc close "`. A genuine painted
  hint row (4 items) in the right dialect with the exit item **last**, pushed
  as the final element of a `lines[..shown]` clamp (`:1471`). I did not
  establish how `render_tasks_strip`'s area height is derived, so whether the
  row can be clipped away is **unverified**; the exit-order defect is not.
- **The terminal dialog header** — `src/tui/interactive_terminal.rs:2068-2079`:
  `"{command} · {status} · INS typing (Esc reads, Ctrl+] sends esc)"`. A status
  header with parenthetical prose, not a row of items.
- **`situational_hints`** — `src/tui/app/mod.rs:10372-10397`: the armed-exit
  prose (`"press q again to quit"`, `exit_control.rs:60-65`), a latched
  `✗ {error}`, and `N queued`, joined with ` · `. A status row that happens to
  share the separator; it advertises no key pair.

The 005 closure record's §1 list is **stale in the other direction**: it names
`panels/help.rs:117-121`, `panels/chat.rs:402`, the `⇧Q` wrapper at
`panels/mod.rs:1553`, `situational_hints`, and 24 dialog footers as
unconverted. Measured now: `panels/help.rs:111-127`, `panels/mod.rs:1549-1554`
and `panels/chat.rs:403-405` all build through `HintLine`; and
`grep -rl ' |  ' src/tui/components/dialogs/*.rs` returns **1** file
(`plan.rs`), whose single hit is inside a doc comment, not a literal. The
dialog footer conversion is complete; the record's count is out of date.

### 3.4 `/plugin` — the footer chunk is spent on a border

`PluginDialog::render_body` (`src/tui/components/dialogs/plugin.rs:76-92`)
splits the modal into `[Length(3), Min(1), Length(1)]` and then draws the last
chunk with `Section::plain(theme).draw(...)` (`:153`). `Section::draw` paints a
`Block` with `Borders::ALL` (`section.rs:202-217`), and `Block::inner` on a
1-row rect yields `height = 0` (`ratatui-0.29.0/src/widgets/block.rs:638-647`:
`saturating_sub(1)` for the top border, then again for the bottom). The
paragraph is then rendered into a zero-height
area, so `hint.render(0)`'s text at `:150` is **never written**. The row is not
long — 36 columns with a scroll indicator — it has nowhere to go. Root cause:
the chunk reserves one *content* row and spends it on chrome.
`Section::owned_height(1)` (`section.rs:151`) is 3 and is what `info.rs`
(`FOOTER_ROWS = 1`, `info.rs:26`, `preferred_area` `:391-399`) and `help.rs`
(`chunks[3] = Length(3)`, `help.rs:495-501`) already use.

### 3.5 `/keybind` — the hint is the 59th line of a 13-row body

`KeybindDialog::render_body` appends its per-mode footer to the **body** line
list (`keybind.rs:349-367` for Normal/WaitingForKey, `:392-398` for Export,
`:423-429` for Import) and renders one `Paragraph` into the section's inner
rect (`:433-441`). `ActionKey::all()` has **57** entries
(`src/tui/input.rs:363-422`, counted), so the body is 57 action rows + one blank
+ the footer = **59** lines. The dialog has no `preferred_area` override, so it
gets the default 60 %×50 % (`component.rs:285-287`): at 100×30 that is 60×15,
an inner height of 13. At 200×60 it is 120×30, inner 28. The footer sits at
line 59, so it needs an inner height of 59 — a **122-row terminal** — and is
therefore never painted at any real size. The Export footer is prose
(`"Press Esc to close"`) rather than a row, so even if it painted it would fail
the dialect check silently (one segment, not an advertisement).

Adjacent finding, out of scope: the dialog has no scroll offset either, so
actions 11–57 are unreachable on screen while `select_up`/`select_down` can move
the cursor through all of them. Reserving the footer does not fix that.

### 3.6 `/security-review` — painted, and clipped mid-item

`security_review.rs:511-527` builds an 8-item footer and renders it with
**`render(0)`** — unbounded — into a `Length(3)` chunk drawn by
`Section::plain`, i.e. a 1-row inner area with a real width. `Paragraph` clips
at the row width with no wrapping, so the row ends wherever the box ends.

Measured widths (computed from the strings and the 60 % geometry): the full row
is **113** columns with the default `All` filter and **126** with
`Medium+ severity`. Inner widths are 46 / 58 / 70 / 94 at 80 / 100 / 120 / 160
column terminals, so a budgeted row shows 3 / 4 / 5 / 6 of 8 items and the
whole row fits only at ~240 columns.

**Why the painted scan cannot see this.** `capture::modal(component, area,
&theme)` calls `component.render(frame, area, theme)` with `area = 100×30`
(`tests/tui_render/invariants/capture.rs:240-261`), so every fixture is measured
at the full frame rather than at the 60×15 the modal host would give it — the
scan does not consult `preferred_area`. At inner 98 the clipping happens to fall
on an item boundary (`… H copy hunk `, 98 of 113 columns, `Enter preview` gone),
which is why the row passes the dialect check; at the *production* width the cut
is mid-item: at inner 58 the row is
`Esc close · j/k move · PgUp/PgDn page · f filter (all) · ]`, the last six
characters of `]/[ hunk nav` missing. The scan measures every box at 67 % more
width than the modal host gives it, so a width-dependent defect is invisible to
it by construction.

## 4. Invariants that must not regress

- **No hint advertises a key the surface does not handle.** I4's recorded set is
  four rows (`docs/tui-render-invariant-baseline.toml:166-184`); this plan adds
  none and presses none.
- **The advertised-verb set per surface does not change.** No item is added to
  or removed from any declaration. Two rows reorder (`input_hints`' `Esc`;
  the tasks strip's `Esc`) under criterion 4, and one row's item set is
  re-expressed without loss (`normal_hints`).
- **`fit_parts` keeps its whole-part rule and its first-part guarantee.** One
  definition (`hint_line.rs:126`); `app::fit_row` delegates (`app/mod.rs:66-68`).
- **`hint_truncation` stays 0 and `[advertised_keys]` stays 4.** Neither section
  may grow for this work.
- **I1 (no paint outside the given rect), I2, I3b stay green.** The two
  geometry changes (§7 WP E) are inside the dialog's own rect.
- **The comment ratchet holds.** The plan adds comments only where a rule is
  new (the reserved-row rule and the footer-chunk arithmetic); the banked
  139,245 lines is the ceiling.
- **The composer's composite row is not flattened.** §5 excludes it and §14
  makes a flattening attempt a stop condition.

## 5. Scope

### In scope — the scope table

`Kind` is the research verdict this plan turns on: **drift** (a real hint row in
the wrong form, convert it), **structural** (a real row whose items are lost at
a seam), **layout-defect** (the row is right and cannot be seen), **dead**
(written, never read — not a producer), or **different-surface** (not a row of
items; excluded by name).

| # | Site | Kind | What changes | Renders differently? |
|---|---|---|---|---|
| 1 | `app/mod.rs:480`, `:1236`, `:1827`, `:5983-6004`, `:6100` | dead | delete `context_hint`, `update_context_hint` and its call | **no** — nothing reads it |
| 2 | `app/render.rs:1207` (home) | drift | `HintLine::from_items(&[("Ctrl+N","new session"),("Ctrl+L","change model"),("?","help")])` | **yes** — home screen only; no test pins it |
| 3 | `chat_browser.rs:818-824` (`normal_hints`) | structural | return `HintLine`; the 3 call sites extend the host line | **no** — same items, same separators |
| 4 | `chat_browser.rs:827-837` (`input_hints`) | structural + order drift | return `HintLine` with `push_required("Esc", ...)` first | **yes** — `Esc` moves to the front |
| 5 | `commands/interactive_terminal.rs:1718-1728` | drift + layout-defect | build items; typed setter; render at the box's width | **yes** — from *empty* to 3–4 whole items |
| 6 | `commands/shell.rs:1555` | dead (by design) | same items, typed setter; still never painted | **no** — `renders_caller_footer()` is false |
| 7 | `commands/shell.rs:1683` | dead (by design) | same as 6 | **no** |
| 8 | `interactive_terminal.rs:2165` | different-surface | **nothing** — excluded | no |
| 9 | `runtime/command_dispatch.rs:128` | drift | `from_items(&[("a","publish"),("Esc","cancel")])` | **yes** — 24 → 21 columns |
| B | `dialogs/plugin.rs:79-92`, `:153` | layout-defect | footer chunk `Length(Section::owned_height(1))`; budget the render | **yes** — row becomes visible |
| B′ | `dialogs/keybind.rs:349-367`, `:433-441` | layout-defect | reserve a footer chunk; per-mode row; budget | **yes** — row becomes visible; Export's prose becomes a row |
| C | `dialogs/security_review.rs:511-527` | layout-defect | `hint.render(footer_inner.width)` | **yes** — items drop whole instead of a mid-item cut |
| D | `dialogs/info.rs:168`, `:324-348` | seam | `set_custom_footer(HintLine)`; no `push(custom, "")` | via 5 / 9 |
| E | `app/render.rs:1462` (tasks strip) | drift | `from_items`, exit first | **yes** — order only |

Item 1's deletion is safe against the one out-of-tree consumer:
`apps/desktop/src-tauri/Cargo.toml:37` depends on `codegg`, and a search of
`apps/` finds no reference to `context_hint`.

### Explicitly out of scope, with the reason

- **Mouse-hover tooltips.** The sidebar tooltip (`sidebar.rs:2246-2269`) is a
  bordered one-line overlay attached to a cell, showing prose
  (`"Click to collapse/expand goal"`). It is a different *kind* of surface: it
  has no item list, no exit key and no budget, and it is not painted in a row
  of the frame's text layout. Building it with a footer's builder would be a
  conversion with no defect behind it. The *other* hover path (`context_hint`)
  is dead and deleted (item 1) rather than converted.
- **Prose and value rows.** `situational_hints` (`app/mod.rs:10372`), the
  latched-error substring, the terminal header (`interactive_terminal.rs:2068`)
  and the diagnostics value lines (`:2165`) are status text. They share the
  ` · ` separator and name keys inside sentences; they are not `<key> <verb>`
  item rows.
- **The composer's composite row.** `composer_hint_text_bounded`
  (`app/mod.rs:10261-10360`) stacks a situational prose part, an agent-turn
  part, a panel row and the composer's own row, with a priority order
  (situational first) that the builder's required-first ordering cannot express.
  Forcing it through `HintLine` would either lose the exit key or demote the
  armed-exit confirmation. §14 makes that a stop condition. Its `keys` parts
  are already built by `HintLine` (the panel rows) and the two literal parts
  (`llm_buffer_key_hints`, `composer_hint_row`) are in-dialect and bounded, so
  the row is consistent even though it is not one `HintLine`.
- **The I4 parser widening and its 12 fixture rows** (C002 §4). Deferred, with
  the reasoning in §9.
- **The keybind body's missing scroll window** (`keybind.rs:433-441`). Reserving
  the footer does not give 57 actions a viewport; that is a body change.
- **`architecture/tui.md`'s other claims** and any copyedit.

## 6. Required production changes

### Core/domain, storage, protocol, runtime, security

None. No scheduler, bus, permission, persistence or protocol surface is touched.

### Frontend / operator surface

- `src/tui/components/hint_line.rs` — no change to `render`, `fit_parts`,
  `push` or `items`. One additive convenience is allowed if a caller needs it
  (an `extend` over `&[(&str,&str)]`, or `push_line(&HintLine)`); the builder's
  four decisions stay as they are.
- `src/tui/components/dialogs/info.rs` — `set_custom_footer` takes a `HintLine`;
  the Owned path pushes its items, not its rendering, and keeps the
  `renders_caller_footer` rule unchanged.
- `src/tui/components/dialogs/plugin.rs` — the footer chunk becomes
  `Constraint::Length(Section::owned_height(1))` and the row is rendered at
  `footer_inner.width`.
- `src/tui/components/dialogs/keybind.rs` — the body is
  `Layout::vertical([Constraint::Min(1), Constraint::Length(Section::owned_height(1))])`;
  the hint moves out of the line list and into the reserved chunk, per mode.
- `src/tui/components/dialogs/security_review.rs` — `render(0)` becomes
  `render(usize::from(footer_inner.width))`.
- `src/tui/chat_browser.rs` — `normal_hints`/`input_hints` return `HintLine`.
- `src/tui/app/render.rs` — the home row and the tasks-strip row are built
  from items; the home row loses its alignment-only leading spaces.
- `src/tui/app/mod.rs` — the dead hover field and function go; the chat-seed
  branch builds a row instead of formatting one.
- `src/tui/commands/interactive_terminal.rs`, `src/tui/commands/shell.rs`,
  `src/tui/runtime/command_dispatch.rs` — the five `set_custom_footer` call
  sites build items.

### Documentation and static guards

- `architecture/tui.md` — replace "`HintLine` is the single producer of hint
  text" with the restated criterion, its exclusion list, and the reserved-row
  rule (§12).
- No new guard. The check is a test over painted frames, which is the only
  layer that can see a row that is not there.

## 7. Ordered work packages

Order is by risk: the deletion first (it cannot change a byte), then the seam
every caller footer goes through, then the rows whose visibility changes, then
the two reorders.

### Work package A — Delete the dead hover state (item 1)

Intent: remove a second producer that does not produce.

Required changes: delete the `context_hint` field, its two initialisers, its
`String::new()` default, `update_context_hint` and the `:6100` call. No
replacement.

Acceptance evidence: `grep -rn 'context_hint' src/ tests/ apps/` returns
nothing; `cargo clippy -p codegg --lib --tests -- -D warnings` is clean (a
`pub` field removal that nothing reads produces no warning, so this is a
compile check, not a lint check); no test file changes.

### Work package B — One structured footer seam (items 5–7, 9; the `D` row)

Intent: a caller-supplied footer stops being a string a dialog can only
concatenate.

Required changes: `InfoDialog::set_custom_footer(HintLine)`; the Owned arm
renders `custom.items()` through the same budgeted `HintLine` as the
non-custom arm; the **five** call sites build items
(`runtime/command_dispatch.rs:128`, `commands/interactive_terminal.rs:1739` and
`:1747`, `commands/shell.rs:1568` and `:1575`).

The `/terminal-show` row is the reason this is a seam and not five edits: its
181 columns cannot fit a 58-column box, so the *items* are what lets
`fit_parts` drop whole items instead of blanking the row. Item order is the
caller's priority: exit first, then the reading/selection verbs, then the
diagnostics verbs; `D diagnostics` and `Esc closes` cannot both be first, and
the exit wins.

Acceptance evidence: a lib test in `tui::components::dialogs::info` renders a
`TerminalShow` dialog with the real caller row at 60×15 and asserts the row is
**non-empty**, that its first item is `Esc close` and that every item is whole
(the assertion is the measured bounded row, not an approximation — the expected
prefix is `Esc close · j/k scroll · ^-u/^-d page`); and that no rendered item is
a prefix of the next. The
`/project-init` path keeps one assertion, updated from `a: publish` to
`a publish` (`dialog_dispatch_coverage.inc:235`).

### Work package C — The two remaining literal rows (items 2 and E)

Intent: no painted advertisement row is still a hand-formatted literal.

Required changes: `app/render.rs:1207` and `:1462` build through
`HintLine::from_items`; the tasks-strip row puts `Esc close` first. The home row
drops its two leading spaces, which exist only to offset a centred span.

Acceptance evidence: a `tui_render` assertion in `tests/tui_render/tui_render.inc`
adds `buffer_contains(&buf, "Ctrl+N new session · Ctrl+L change model · ? help")`
to `render_empty_home_state_all_sizes` at the smallest size that fits it; the
tasks-strip row is asserted where its overlay is already rendered.

### Work package D — Structured chat fragments (items 3 and 4)

Intent: the chat row's items survive the seam.

Required changes: `normal_hints` returns `HintLine`; `input_hints` returns
`HintLine` with `Esc` required and first; the three call sites
(`panels/chat.rs:404`, `components/dialogs/chat.rs:446-447`,
`app/mod.rs:10277-10281`) extend the host line and render at the same budget
the host row uses.

Acceptance evidence: the composed chat row's `items().count()` equals
1 + the fragment's count + 1 (not 3), asserted in a lib test; a budget that
drops the fragment's last item renders the row without ending mid-item; the two
existing chat tests (`chat_browser.rs:1296`, `:1307`) keep passing with their
string assertions rewritten to render the returned line.

### Work package E — The two footers that are never painted (B and B′)

Intent: a converted row that cannot be seen is not converted.

Required changes:

- `/plugin`: `Constraint::Length(Section::owned_height(1))` in `plugin.rs:82`,
  and `hint.render(footer_inner.width)` at `:150`. The content box loses two
  rows (24 → 22 at 100×30). At the 20×6 minimum the body reaches 0 rows; the
  box still paints its title and its footer, which is the honest degradation.
- `/keybind`: a reserved footer chunk per §6; the Normal (and WaitingForKey)
  row is `Esc close · enter remap · r reset`, Export is `Esc close`, Import is
  `Esc cancel · Enter import`. Export's prose (`"Press Esc to close"`) becomes a
  row; that is a content change, and §14 makes it a review item rather than an
  assumption.

Acceptance evidence — **this is the test the current scan cannot provide**. A
new `tests/tui_render/invariants/vocabulary.rs` test,
`every_footer_the_scan_converted_is_actually_painted`, renders the two dialogs
through `capture::modal` at 100×30 and asserts the row is present **whole**:

```rust
for (surface, make, expected) in [
    ("Plugin", plugin_fixture, "Esc close · j/k scroll"),
    ("Keybind", keybind_fixture, "Esc close · enter remap · r reset"),
] {
    let frame = capture::modal(make(), area, &theme);
    assert!(
        painted_rows(&frame).iter().any(|row| row.contains(expected)),
        "{surface}: the converted footer is not painted at all: {expected:?}"
    );
}
```

The test must be written first and must fail on this tree: `/plugin` paints no
footer line and `/keybind`'s is on line 59. The red output is the evidence the
test bites; the closure record quotes it.

### Work package F — `/security-review`'s budget (C)

Intent: the row ends at an item boundary.

Required changes: `security_review.rs:521` becomes
`hint.render(usize::from(footer_inner.width))`.

Acceptance evidence: a lib or `tui_info_dialog_panel_chrome` test renders the
dialog at 60×15 (its production geometry) and at 100×30, asserts the painted
row equals the measured bounded string, and asserts that the row's last
character is the end of a whole item. **The row loses four of its eight verbs
at 100 columns** (measured: 54 of 113 columns fit at inner 58). That is a
recorded finding, not a fixed one: §13 criterion 5 is satisfied, criterion 4 is
satisfied, and "every verb is visible" is not a criterion this plan can meet
without a geometry change (§10).

### Work package G — Record the exclusions and the rule

Intent: the next author does not re-open a settled question.

Required changes: the `architecture/tui.md` edit in §12, and a one-line
comment at each excluded site that names the exclusion (`situational_hints`,
the terminal header, `renders_caller_footer`, the sidebar tooltip) so the
exclusion is a value a reader can grep rather than a claim in prose.

Acceptance evidence: the doc states the rule, §13's four excluded kinds and the
composite-row note; the comments exist at the named lines;
`check_comment_budget.py` passes.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Every change is a pure per-frame string or layout build. There
is no durable state, no I/O, no async path and no process. The one hazard the
source milestone names — a hint cached across a focus change — does not exist:
every converted row is rebuilt inside `render` or `composer_hints` from current
state, and `HintLine::render` is a fresh allocation.

The one *layout* hazard is degradation: reserving a footer chunk removes rows
from a body. Both affected boxes must be checked at the harness's smallest size
(20×6). `/plugin` degrades to a zero-row body; `/keybind` must not, because its
body is the only content — if the reserved chunk would leave the body below one
row, fall back to a one-row unbordered footer rather than losing the list
(§14, stop condition 4).

## 9. Compatibility and migration

No schema, protocol, configuration or dependency surface. `App` is a `pub`
struct in a lib that `apps/desktop` depends on by path, so the field deletion in
WP A is the only cross-crate change; no consumer reads the field, and the
desktop's own qualification lane compiles `codegg` (it is the lane that would
notice). `set_custom_footer`'s signature change is crate-internal: its five call
sites and two tests are in this tree.

**The I4 parser widening (C002 §4) — deferred, and the order matters.** The
widening is cheap in code (seven match arms in `key_of`,
`tests/tui_render/invariants/baseline.rs:367-407`, for `Home`, `End`,
`PageUp`/`PgUp`, `PageDown`/`PgDn`, `Backspace`, `⌫`, `[`, `]`) and it changes
no rendered byte. It is *not* cheap in evidence: it makes twelve-plus
advertisements pressable, the presses are inert where the fixture has no state,
and every inert press must be **banked as an `[advertised_keys]` row with a
reason**. The four rows already banked each name why the *product* cannot
answer (`FocusManager` owns `Tab`; `H` writes the clipboard). "The fixture has
no second channel" names why the *harness* cannot ask, which belongs in the
fixture, not in the ratchet.

The record's table does not reconcile with its own count: `Diff` 4 +
`SecurityReview` 4 + `TaskView` 2 + `ProjectChat` 2 = **12**, and the same
table then adds "one each" for `Help`, `Model`, `Question`, `Queue` and
`Permission` — five more, for 17. `UNREADABLE_SPELLINGS`
(`baseline.rs:349-360`) has ten spellings, and the per-key report's "12
UNREADABLE" is a different twelve. Any future package must **re-measure** the
row set before banking anything; the table cannot be used as a plan of record.

Verdict: **defer**, and if taken, take it fixture-first — give `Diff` and
`TaskView` a moved cursor, `Question` an answer, `ProjectChat` a second channel
and `SecurityReview` a hunk, then widen, so the recorded set stays at four.
That is five edits in `tests/tui_render/invariants/fixtures.rs::CONTENT_MAKERS`
and one in `seeded_security_review`; the security one is the only non-trivial
fixture, because a hunk needs a `SecurityReviewReceipt` with a hunk-backed
finding. This plan does not take it: it changes no bytes, it is not a producer
defect, and it belongs to the advertised-keys workstream.

## 10. Required tests

### Focused unit tests

- `tui::components::dialogs::info` — the caller footer with the real 181-column
  `/terminal-show` item set at 60×15 renders non-empty, whole items only, exit
  first; the `/project-init` footer renders `a publish · Esc cancel`.
- `tui::components::dialogs::hint_line` — unchanged (9 tests). If an `extend` or
  `push_line` is added, one test asserts the composed line's `items()` is the
  concatenation in painted order.
- `tui::components::dialogs::chat` / `chat_browser` — the composed chat row's
  item count is the sum, not 3; `input_hints`' first item is `Esc`.
- `tui::components::dialogs::plugin` — the footer row is painted at 100×30 and
  at 60×15; the content box lost exactly two rows.
- `tui::components::dialogs::keybind` — three modes each paint their row in the
  reserved chunk; the Export row is a row, not prose.

### Integration tests

- `tests/tui_render/invariants/vocabulary.rs`:
  `every_footer_the_scan_converted_is_actually_painted` (WP E) — the one test
  the current scan structurally cannot provide. It must be red before the fix
  and green after, and the closure record quotes the red.
- `invariants::vocabulary::every_painted_advertisement_row_uses_the_one_vocabulary`
  and `every_surface_hint_uses_the_one_vocabulary` — unchanged, still green;
  the floor (32 surfaces / 47 rows) does not move.
- `tests/tui_render/tui_render.inc` — the home row assertion; the tasks-strip
  row assertion.
- `tests/tui_render/tui_info_dialog_panel_chrome.inc` — the `/security-review`
  bounded row at 60×15 and 100×30; the `/project-init` footer's new spelling.

### Security and negative tests

- The painted-row test's non-vacuity is the pre-fix red; the vocabulary
  detector's own negative case
  (`the_vocabulary_check_rejects_both_recorded_dialects`) is unchanged.
- A regression test for the blank-footer bug: a caller footer **longer than the
  box** must render a non-empty prefix of whole items, never `""`.

### Migration and compatibility tests

- The advertised-verb set per surface is unchanged: `RECORDED`
  (`vocabulary.rs:340`) is byte-identical. Two rows reorder and one row's items
  are re-expressed; no declared item is added or removed.

## 11. Required verification commands

**The filter effect of every selector below was measured on this tree**, because
a selector that matches nothing exits 0 and "it passed" is then not evidence.
The counts are the tree's values *before* this plan's work: WP E adds one test,
which moves `invariants` 40 → 41 and `invariants::vocabulary` 5 → 6.
The `tui_render` figures come from one `cargo test --test tui_render -- --list`
(1236 tests, 0 benchmarks) and a count over that list; the `--lib` figures come
from `--list` on the two lib test binaries already built from this tree
(`codegg-7f071e724f49550c`, `codegg-914f98c04d0181d7`), which agree, and were
not re-run through cargo.

```bash
# ── the invariant member: 40 tests (baseline 13, vocabulary 5, the rest)
cargo test --test tui_render -- invariants

# ── the module this pass adds a test to: 5 tests today
cargo test --test tui_render -- invariants::vocabulary

# ── the members whose surfaces this pass touches
cargo test --test tui_render -- tui_info_dialog_panel_chrome   # 13 selected
cargo test --test tui_render -- dialog_dispatch_coverage       #  7 selected
cargo test --test tui_render -- tui_usability_fixes            # 26 selected
cargo test --test tui_render -- tui_render                     # 111 selected (member)

# ── the lib targets that own the converted widgets
cargo test --lib -- tui::components::hint_line                 #  9 selected
cargo test --lib -- tui::components::dialogs::plugin           #  9 selected
cargo test --lib -- tui::components::dialogs::keybind          #  4 selected
cargo test --lib -- tui::components::dialogs::info             #  8 selected
cargo test --lib -- tui::panels                                # 51 selected

# ── the ratchets and the shape guard
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_cost.py
python3 scripts/check_comment_budget.py

# ── format, lint, and the routine gate
cargo fmt --check
cargo clippy -p codegg --lib --tests -- -D warnings
scripts/verify.sh quick
```

**The family/member trap, measured.** `cargo test --test tui_render` selects
**1236** tests (the whole family); `cargo test --test tui_render -- tui_render`
selects **111** (the member of that name). A plan that writes the former and
intends the latter runs five times the tests and still looks green; a plan that
writes a member name that does not exist (for example
`cargo test --test tui_render -- tui_sidebar_surfaces`, measured: **0**) exits
0 having selected nothing. Both are quoted here so the reader can tell which
line is which.

## 12. Documentation updates

- `architecture/tui.md` (the section at `:4396-4436` today) — replace the
  sentence "`HintLine` is the single producer of hint text" with the restated
  criterion verbatim (§13.1), the four excluded kinds and the composite-row
  note (§13), and the reserved-row rule: **a hint row is painted in a reserved
  row or chunk, never as the tail of a body that clips**. Add the two layout
  traps as one sentence each: a chunk that reserves `Length(1)` and hands it to
  a bordered `Section` spends the row on the border (`Section::owned_height`
  exists for this), and a footer appended to a `Paragraph` that is taller than
  its rect is not painted at all.
- The `[hint_truncation]` comment block in
  `docs/tui-render-invariant-baseline.toml` gains one line saying the section
  measures the composer row only, so a reader does not read 0 as "no row
  truncates anywhere".
- `plans/implementation/tui-modularity-and-render-invariants/005-one-hint-vocabulary.md`
  §13 criterion 1 — the parent applies the restatement from §13 below.
- `plans/closure/tui-modularity-and-render-invariants/005-status.md` §10–§11 —
  the parent applies the new residue state.
- `plans/registry.md`, the roadmap, and `AGENTS.md` — parent-owned; not touched
  by this plan.

## 13. Acceptance criteria

**1 (restated).** `HintLine` is the only producer of a **hint row** — a painted
row of `<key> <verb>` items — and no hint row is composed from a hand-formatted
string, a `format!`, an opaque row fragment, or a caller-supplied footer string.

**1b (new).** Every hint row is **painted**: it occupies a reserved row or
chunk, so it is drawn at every size its surface renders at, and a declared row
that no frame contains is a failure.

**What the restatement excludes, and why.** A criterion that says "the only
producer of a rendered hint string" is false for four kinds of surface that are
not hint rows, and satisfying it by conversion would be gaming:

- **tooltips** — a prose line attached to a cell (the sidebar's
  `tooltip_text`), with no item list and no exit key;
- **status rows** — the composer's situational layer and the terminal header,
  which mix prose with key names and whose priority order the builder cannot
  express;
- **value rows** — `focus: … (Esc leaves focus …)` in a diagnostics dump;
- **dead strings** — a footer a box discards by its own rule
  (`renders_caller_footer`), which is not rendered at all.

2. One separator is in force across panels and dialogs (` · `), verified on
   painted rows.
3. Verbs are lowercase and key names keep their conventional capitalisation.
4. The exit item is first on every hint row, and no budget that shows a part
   omits it.
5. No rendered hint row ever ends mid-item — including the rows the scan cannot
   see, proven by the new painted-row test.
6. The advertised-verb set per surface is unchanged from baseline; `RECORDED` is
   byte-identical.
7. `[hint_truncation]` stays 0, `[advertised_keys]` stays 4, `[contrast]` stays
   0; no ratchet entry is added by this work.
8. `scripts/verify.sh quick` is green and the render-cost guard is unmodified.

Criterion 1 is **not** claimed for the composer's composite row, and §14 makes
an attempt to flatten it a stop.

## 14. Stop conditions

Report rather than improvise when:

1. **A conversion would change rendered bytes outside the dialect.** The owner
   directive of 2026-11-13 ("unify the form, keep the context") authorises the
   separator/casing/order change on **rows that are already painted**. It does
   *not* cover: making a row visible for the first time with content it never
   showed (`/terminal-show`'s items, `/keybind`'s three footers), turning
   Export's prose into a row, or changing which verbs a row names. Those rows
   are this plan's *point*, so they are built — but each resulting row is
   recorded in the closure record for review, and if the owner declines a row,
   the fix is reverted to "recorded, not fixed" rather than silently kept.
2. **A row's content would have to be formatted by the caller.** That is the
   composer row; it is excluded by criterion 1, and the correct action is to
   leave it and amend the criterion, not to teach `HintLine` a second ordering.
3. **The exit key is genuinely unknown at render time.** Criterion 4 then
   cannot be met and the row must say so rather than name a guess.
4. **A reserved footer chunk leaves a body below its own minimum.** `/keybind`
   at 20×6 must keep at least one body row; fall back to a one-row unbordered
   footer rather than losing the list.
5. **A conversion drops a verb that the surface still dispatches, and the row
   is the surface's only advertisement.** Report the row and the verbs; do not
   remove items from the row to make it fit. The budget may drop whole items at
   render time — that is WP F — but the item set is not shortened (this is the
   `/security-review` temptation).
6. **The I4 recorded set moves.** Any new `[advertised_keys]` row means the
   change crossed into the advertised-keys workstream.
7. **`check_comment_budget.py` is red before the work starts.** It is green on
   this tree; if it is red on the tree the implementer picks up, the comment
   ratchet cannot be cited and the difference must be re-banked by its owner
   first.

## 15. Closure evidence required

- The scope table of §5 with each row's final disposition (converted / deleted /
  excluded / deferred) and the measured corpus effect.
- The pre-fix red of `every_footer_the_scan_converted_is_actually_painted`,
  quoted verbatim.
- The measured `/security-review` row at 60×15 and 100×30, with the four verbs
  it cannot show named.
- The `/terminal-show` row's measured items and the width at which it truncates.
- The item-count assertion for the chat rows before and after.
- `RECORDED` byte-identical; `[hint_truncation] = 0`; `[advertised_keys] = 4`.
- The selector list of §11 with the effect each selector had, re-measured on
  the post-change tree (the two `invariants` counts must read 41 and 6).
- `quick` output, and the comment-budget figure.
- A closure record at `plans/closure/tui-modularity-and-render-invariants/012-status.md`,
  and the 005 record's residue table reduced to the items this plan excludes,
  each with its reason.

## 16. Handoff notes

The eight-producer list in the 005 closure record is the wrong unit of work.
Measured, it is: one dead field with no reader, two strings a box discards by
design, one diagnostics value line, one row fragment that is already in the
right dialect but arrives as an opaque item, two painted rows still in the old
dialect, and one painted row the box renders empty. Only four are conversions
in the ordinary sense; the rest are a deletion, a type change and three layout
defects — and the layout defects are where the product is worse today than the
record says.

Two traps are worth carrying forward. First, `HintLine::push(x, "")` is an
escape hatch: it makes any string an item, so a caller can "use the builder" and
still paint a second dialect, and an over-long item then renders an *empty* row
because `fit_parts` refuses a partial first item. Every use of it is a seam that
should be typed (the `InfoDialog` footer was the one that mattered). Second,
the painted scan cannot see a row that is not painted — its floor counts rows,
not surfaces — so "the scan is green" never meant "every declared row is
painted". The new per-row test is the missing half, and it must be recorded red
before it is recorded green.

A third harness limit is worth recording because it is what let
`/security-review` pass: `painted_hints` renders every fixture into the **full
100×30** rect (`capture.rs:259-261`) instead of the dialog's `preferred_area`,
so it measures a box at 100 columns where the modal host gives it 60. Teaching the
scan to call `preferred_area` would be the honest fix, but it moves every
fixture's width, re-derives the 47-row floor, and would go red on rows this plan
does not own — so it is a follow-up for the harness, not a work package here.
Until it lands, a width-dependent row must be tested by the surface, at the
surface's own geometry.

The `f filter ({})` item lowercases its label (`security_review.rs:516`
`push("f", &format!("filter ({})", self.filter.label()))`), so `All` renders as
`all`. I judged that consistent with the row's casing and did not add a
value-preserving constructor; if the filter chips elsewhere in the box are
Title Case, that judgement is wrong and the fix is a builder method, not a
shout in the string.

Do not convert the sidebar tooltip. It is the one surface in this plan's
neighbourhood that is *correctly* prose, and a criterion that demands a tooltip
be built by a footer's builder is a criterion that will be gamed.
