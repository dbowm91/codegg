# M007 — Border strokes counted as text

Research memo, 2026-11-12

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`, worktree
uncommitted.

**Owner decision, 2026-11-12: option A — keep strokes at the text bar.**

The recommendation below was D′ (exempt `border`-painted stroke glyphs). The
owner chose **A** instead, and the choice is recorded here with its consequences
rather than argued:

- `border` stays inside `[contrast]`. Nothing is exempted, so the invariant keeps
  asserting what it asserts today.
- The **1,210 `border` rows stay recorded** and cannot be cleared at a render
  site: the stroke is already the border token, drawn on the background token, and
  the same surface passes on the other 28 themes. Clearing them requires
  re-authoring the border value in the 22 affected bundled themes — a palette
  change with visible consequences, which is a separate decision and is **not**
  authorised by this one.
- Consequence for the milestone: after ADR-0062 cleared its 688
  inherited/authored token rows, `[contrast]` stood at 1,807. The later work
  (class-(a) sites, the `Theme` fixture, the app-frame theme) brought it to
  1,448, of which 1,232 were `border`. The later mapping round
  (`008-halloy-mapping-research.md`) then removed the whole `border` family by
  gating the *projection* instead of the theme files: **`[contrast]` is 216** —
  215 picker swatches and one app-frame cell — with `border` at **zero** and
  `assets/` byte-identical to `HEAD`. This decision (keep strokes at the text
  bar) still holds: strokes are measured, and they now pass because the
  translation produces a legible edge rather than because they were exempted.

What this decision buys: the invariant is not widened, no threshold is invented,
and the archaeology above — that no stroke was ever held to a text bar before the
harness — is recorded as a **deliberate** change of position rather than an
accident. A reader who later asks "why is a border held to a text bar?" gets an
answer with a date and an owner.

Related: `plans/closure/tui-modularity-and-render-invariants/007-status.md` §10
finding 4 and §11 decision 2;
`plans/adrs/ADR-0062-bundled-palette-contrast-authority.md` clause 5;
`plans/implementation/tui-modularity-and-render-invariants/007-contrast-ratchet-to-zero.md`
§14.

## The question, and the steer

The I5 sweep applies `MIN_DERIVED_TEXT_RATIO` (3.0) to **every non-blank cell**
(`tests/tui_render/invariants/baseline.rs:426`, over
`Frame::painted` = `!cell.is_blank()` at
`tests/tui_render/invariants/capture.rs:174-177`). A panel edge is drawn with
`─│┌┐└┘`, which are non-blank, so 1,210 of the 2,495 recorded entries are
`theme.border` measured as if it were a glyph a reader reads.

The owner's steer, verbatim:

> compare options for border stroke counting as text. if this is the way it
> was, likely safer to leave it, unless refactoring changed things.

So the archaeology decides the recommendation: the steer's own condition is
"**if this is the way it was**". The sections below answer that first, then
compare the options.

## 1. Archaeology

### 1.1 Which value paints a border, and since when

`Theme::border` has painted an unfocused edge since the repository's first
commit. Every step below is a citation, not a summary.

| Revision | Surface | Border foreground | Evidence |
|---|---|---|---|
| `d820dfb4c` (2026-09-25, root) | dialogs | `theme.border` | `git show d820dfb4c:src/tui/components/dialogs/help.rs` lines 111, 132, 152 — `.border_style(Style::default().fg(theme.border))` |
| `d820dfb4c` | theme model | `border` exists as a token with a per-palette value | `src/tui/theme.rs` `pub border: Color`, 31 `ThemeData` literals each carrying a `border` tuple |
| `7642b9e38` (HEAD) | Chat panel | focused `primary`, unfocused `border`; title `primary`/`muted` | `git show HEAD:src/tui/panels/chat.rs:279-292` |
| `7642b9e38` | Agents panel | `border`, unconditionally; title `primary` + `BOLD` | `git show HEAD:src/tui/panels/agents.rs:195-204` |
| `7642b9e38` | Terminal panel | focused `primary`, unfocused `border` | `git show HEAD:src/tui/panels/terminal.rs:209-215` |
| `7642b9e38` | Session panel (sidebar) | focused `primary`, unfocused `border` | `git show HEAD:src/tui/components/sidebar.rs:2230-2233` |
| `7642b9e38` | hosted panels | `theme.border`, with **no focus branch at all** | `git show HEAD:src/tui/panels/mod.rs:111-114` |
| worktree (M002, untracked) | all six panel surfaces | focused `primary`, unfocused `border` | `src/tui/panels/chrome.rs:62-67` — `ChromeStyles::resolve` |
| worktree (M003, untracked) | dialog outer frames | `primary` (`Section::focused(true)`) | `src/tui/components/section.rs:156` and the `focused(true)` call sites in `dialogs/*` |

The M002 primitive **replaced an inline expression with the same token**. For
`chat.rs` the replacement is literal: HEAD's inline block resolves
`border: if focused { primary } else { border }` and
`title: if focused { primary } else { muted }`
(`git show HEAD:src/tui/panels/chat.rs:281-291`), which is exactly what
`ChromeStyles::resolve` returns today. For `agents.rs` and the hosted host the
refactor *added* a focus branch, so a focused edge became `primary` instead of
`border` — a change of token, in the direction of the contrast-gated value.

I found no commit that moved a border **onto** `border` from some other token.
The pickaxe over `fg(theme.border)` (13 commits) and over the hosted
`border_style` line (13 commits) returns feature commits that add, move or
delete bordered surfaces; none substitutes a different token for the same edge.
The theme files themselves are untouched by the worktree
(`git status --short assets/` is empty), so every one of the 22 failing
`border` values is the value the bundle shipped.

### 1.2 Was any border stroke ever measured?

No test, guard, validator or document in this repository has ever held a border
stroke to a contrast bar. Four independent checks:

- **The validator.** `validate_theme` checks eleven pairs
  (`src/theme/validate.rs:61-152`) and **never checks `ui.border`**. It also
  never checks `ui.accent_secondary`, which is why the two largest failing
  families ship silently.
- **The tests.** At HEAD, only two files under `tests/` mention contrast:
  `theme_selection_tokens.inc` (the two *derived* tokens, and the derivation
  rule) and `tui_usability_fixes.inc:255-260` (highlighted **labels** at 3.0).
  Neither measures a stroke.
- **The documents.** `docs/themes.md` documents an accent-contrast floor of 3.0
  (`MIN_ACCENT_CONTRAST`) and applies it to *accent candidates* in the importer.
  Because a **focused** edge is painted with `primary`, the focused stroke is
  the only stroke that has ever been bounded — and only incidentally, because it
  is the accent token. The **unfocused** edge (`border`) was never bounded by
  anything.
- **The guards.** `check_no_hardcoded_selection_colours.py` guards selection
  foregrounds; `check_tui_render_cost.py` guards render shape. No guard mentions
  a stroke. The ratchet that counts these entries
  (`check_tui_render_invariant_baseline.py`) is new with M001.

So "strokes were held to a text bar" is **not** a fact about this product's
history. The treatment is new, introduced by the M001 harness — which is
untracked (`git ls-files tests/tui_render/invariants/baseline.rs` →
"did not match any file(s) known to git"), so "before the harness existed" can
only be read at HEAD.

One nuance in the harness's favour: the roadmap **defined** I5 as "every
rendered foreground", and required "no new threshold is introduced"
(`plans/subsystems/tui-modularity-and-render-invariants-roadmap.md:104-108`).
The harness implemented the invariant as written. The scope question is
therefore about the invariant's **wording**, not about a defect in its
implementation.

### 1.3 Did the chrome refactor change it?

Three separate claims, answered separately.

**(i) The 22 themes' border strokes already failed on the day the invariant was
introduced — TRUE.** The `border` values were in the bundle before the harness
existed (no theme file is modified in this worktree), and every one of them is
the same value the frame paints today. The harness recorded a pre-existing
state. The 1,210 entries are exactly 22 themes × 55 surfaces, one
`border`-on-`background` pair per surface per theme (re-derived independently
for this memo; see §4). The recorded ratios, 1.31 (discord, discord-80) to 2.99
(ferra), match `007-status.md` §3 exactly.

**(ii) The refactor moved surfaces onto `border` and thereby created failures —
FALSE, and the movement is the other way.** Three movements, all off `border`:

- **M003 moved every dialog outer frame from `border` to `primary`.** The
  baseline's own header records the measurement: "a modal is the focused
  surface, so its edge and title are `theme.primary` instead of
  `theme.border` … The edge pairs that failed the bar on 24 dialog surfaces
  across 22 themes stop existing: the 528 `[contrast]` rows removed in this
  same diff, and none was added"
  (`docs/tui-render-invariant-baseline.toml:105-116`). 24 × 22 = 528.
- **M002 gave the hosted host a focus branch.** At HEAD the hosted border was
  `theme.border` regardless of focus (`HEAD:src/tui/panels/mod.rs:113`); now a
  focused hosted edge is `primary`. That too removes rather than adds border
  entries, and it is invisible to the sweep, which renders panels unfocused
  (`baseline.rs:480-485` passes `false`).
- **M002 drew one border that did not exist: Help's outer edge.** It is painted
  `border` when unfocused — but the same surface already painted
  `border`-on-`background` from Help's inner title box
  (`HEAD:src/tui/components/dialogs/help.rs:508-511`), and entries are
  deduplicated per `(surface, theme, fg, bg)`, so it added no row. That is
  consistent with the header's "none was added".

Net: the refactor **removed 528 stroke rows** (1,738 → 1,210 for this token,
if no other surface changed), and created none.

**(iii) The fill under the stroke changed, moving the ratio without moving the
stroke — FALSE.** A stroke sits on `theme.background` in both revisions. At
HEAD, panels drew `Block::style(Style::default().bg(theme.background))` and the
hosted host pre-filled every cell with `theme.background`; the current
`PanelChrome::draw` sets the same `.style(bg(theme.background))`
(`src/tui/panels/chrome.rs:124-131`), and `Section::draw` does the same for
dialogs. Where a HEAD dialog left the cell background `Reset`, the harness
resolves `Reset` to `theme.background` (`baseline.rs:395-400`), i.e. the same
value the explicit fill now supplies. No ratio moved because a fill moved.

### 1.4 What the baseline itself records

Two entries in the manifest's header are the refactor's own audit of this
question, written at the time:

- `docs/tui-render-invariant-baseline.toml:93-103` — the 78 `muted` rows M002
  banked are the **unfocused hosted title**, not a stroke.
- `docs/tui-render-invariant-baseline.toml:105-116` — the 528 rows M003 removed
  are **dialog edge** pairs, one per surface per theme (24 × 22).

Neither touches a border that stayed `border`, and neither records a stroke
being added.

## 2. The options, compared

Entry effects are exact for (A), (B) and the (C) bars, computed from the
recorded pairs and the resolved `border` value per theme; see §4 for how.
`n = 2,495` is the whole `[contrast]` baseline.

| | Option | Entries removed | Remaining | What it stops catching | Changes a shipped token? | Kind of change |
|---|---|---|---|---|---|---|
| A | Keep strokes in `[contrast]` at the text bar | **0** | 2,495 | nothing — but it asserts that a 1-cell `─` must reach large-text contrast | no | none (status quo) |
| B | Exempt strokes from `[contrast]`; measure them separately | **1,210** (48.5%) | 1,285 | any unreadable or invisible edge; and, implemented as "foreground is the border token", 22 `PanelManager` hint-text entries | no | measurement |
| C | Stroke-specific bar | **935** at 1.5, **880** at 1.75, **715** at 2.0, **550** at 2.5, **0** at 3.0 | 275 / 330 / 495 / 550 / 1,210 | strokes between the bar and 3.0; same identification problem as B, and the same over-exemption if the bar is applied to every stroke cell | no | measurement + a new documented constant |
| D | Measure only glyph cells (glyph-class exemption) | **1,188 – 1,209** | 1,286 – 1,307 | `border` strokes below 3.0 **and every state-bearing stroke drawn with a box glyph** — the 21 `warning` frame entries on `Confirm`, `Plan` and `Question` would be exempted too | no | measurement |
| D′ | Exempt **`border`-painted stroke-glyph cells only** | **1,188** | 1,307 | `border`-painted stroke cells below 3.0 — the class that never had a bar — and nothing else | no | measurement |

Notes that decide between them:

**A.** 1,210 of 2,495 is 48.5%. Against (B)/(C)/(D) this is the full swing; it is
one token's cells, not a scattered correction. A is not a no-op in principle:
it says the neutral chrome token is a text colour.

**B.** "Separate stroke measurement" needs a way to know a cell is a stroke. At
the frame level the only handle is the foreground: exempt every cell whose
resolved foreground equals `theme.border`. That over-exempts, because `border`
is also used as a **text** colour — `panel_list.rs:164` paints the panel-list
hint subject with it, and its 22 failing entries are exactly that pair on the
22 failing themes. A token-level stroke measurement (assert `border` against the
fills it is painted on, without the frame walk) does not over-exempt, but then
it cannot see *where* the token is used and so cannot distinguish the stroke
from that hint either.

**C.** The bar has to be named, and every plausible value below 3.0 asserts less
than WCAG 2.1 SC 1.4.11 (non-text contrast, 3:1). At 3.0 the option removes
**nothing**, because 3.0 is already the number the harness applies. So C is
only an option if the owner decides the *neutral* edge is decorative; the bar
then still needs a number, and the table above prices the candidates. Note
`border` has no cluster near any candidate: the 22 ratios are spread 1.31–2.99,
so a bar chosen for convenience moves 55–165 entries per 0.25 of ratio between
1.5 and 2.0.

**D.** As the plan stated it ("measure only cells that carry glyphs") this is
the **current** rule: `painted()` is `!is_blank()` and a stroke *is* a glyph.
The workable form is a glyph-*class* predicate. The capture **can** support
that: `Cell` carries `symbol`, so a codepoint test is possible — but it can only
be a codepoint test, because a ratatui `Buffer` carries no widget provenance and
`Frame` records none. That is a real constraint, not a detail: the harness can
never know that a given `─` came from a `Block` rather than from a `Paragraph`.
Practically the predicate is easy, because the product draws **no** custom
border sets (`BorderType` and `border_set(` appear nowhere under `src/tui`), so
every border glyph is in U+2500–U+257F, and scrollbar thumbs add U+2588.

D's defect is that it generalises beyond the neutral token. Of the 35 `warning`
entries, 21 are **strokes** — the warning frames on `Confirm`
(`HEAD:dialogs/confirm.rs:93`), `Plan` and `Question` — and they are a *state
signal*, not chrome; exempting them is wrong. The other 14 are **text**: the
risk labels on `Permission` (`HEAD:dialogs/permission.rs:145,246,251`) and the
hex swatch labels on `Theme` (`HEAD:dialogs/theme.rs:324-333`), and a glyph
predicate would correctly keep those. So D over-exempts exactly the 21 cells
whose colour *is* the warning.

**D′.** The intersection of B's token scope and D's glyph class: exempt a cell
iff its resolved foreground is `theme.border` **and** its glyph is a stroke.
That removes exactly the 54 border-painted stroke surfaces × 22 themes = 1,188
entries, keeps the 22 `PanelManager` hint-text entries measured, keeps every
`warning`/`error`/`primary` edge measured, and needs no number. It is the only
option that is both a measurement change and no broader than the class that
never had a bar.

**What D′ leaves in the sweep that a reader might expect it to remove:** the
`warning` family (35 entries, 5 surfaces) is untouched — correctly, since a
warning frame carries state. ADR-0062 clause 2 repairs those values rather than
exempting them.

## 3. Recommendation

**D′ — exempt `border`-painted stroke-glyph cells from `[contrast]`, and leave
everything else measured.** 1,188 entries (47.6%) leave the baseline; the
invariant's text claim becomes true for every glyph a reader reads.

This is what the evidence supports, not a preference:

1. **The steer's condition resolves against A.** "If this is the way it was" is
   false: strokes were never measured at all (§1.2), so leaving the current
   treatment in place would *newly* assert a bar the product has never had. D′
   restores the pre-existing position for exactly the cells that never had one.
2. **The refactor did not change which token paints a border** (§1.3), and where
   it did change a token it moved edges *onto* the contrast-gated `primary` and
   removed 528 rows. There is no refactor debt to fix here.
3. **The informational stroke is already gated.** A focused edge is `primary`,
   which the importer holds to 3.0 (`halloy.rs:314`). The stroke that sits at
   1.31 is the *unfocused* neutral edge whose only job is to separate
   surfaces — and the panel is still identified by its title and, when focused,
   by the `●` marker and the colour change (I3b).
4. **D′ invents no threshold**, so no one has to defend a number below the
   published non-text minimum. (C) would require that defence; (A) requires
   defending the opposite, that a 1-cell line is body text.
5. **D′ is the narrowest possible exemption**: one token, one glyph class. It
   leaves the `warning` family, the `PanelManager` hint text and every
   accent-edged stroke in the measurement.

**If the owner prefers to leave the treatment alone (A)**, that is a coherent
choice — but it must be recorded as a decision that a stroke *is* a text
foreground, and then the 22 `border` values are a value defect and must be
re-authored under ADR-0062's option B. It cannot be recorded as "this is the
way it was", because it is not.

**If the owner wants a numeric bound on decorative strokes (C)**, set it and
document it in `architecture/theme.md` as a *stroke* tier; the table in §2
prices 1.5 / 1.75 / 2.0 / 2.5. C and D′ compose: D′ decides *what* is measured,
C decides *at what bar*.

**The one piece of evidence that would overturn D′:** a state-bearing edge
painted with `theme.border`. Today none exists — `Confirm`, `Plan` and `Question`
paint warning *frames* with `theme.warning`, `Permission` paints warning
*labels* with it, and error frames use `theme.error` — so `border` is purely
neutral chrome. If a future surface uses
`border` to signal a state (disabled, blocked, inactive), D′ would exempt that
signal, and the exemption must then be narrowed or the surface re-tokened. That
is a guard to add when the option lands, not an assumption to leave implicit.

## 4. Provenance of every number

| Number | Value | Provenance |
|---|---|---|
| `[contrast]` baseline | 2,495 | **read** — `docs/tui-render-invariant-baseline.toml` `[totals]`, and `grep -c '^\[\[contrast\]\]'` = 2,495 |
| class (c) total | 1,898 | **read** — `007-status.md` §1; cross-checked per family below |
| `border` entries | 1,210 | **measured** — resolved each theme's border from `assets/themes/halloy/*.toml` (precedence `buffer.border_selected` → `buffer.border` → `general.border` → `general.horizontal_rule`, as `halloy.rs:212-222`) and matched the recorded `fg`: 22 themes × 55 surfaces. Independently matches `007-status.md` §3 and its ratio range 1.31–2.99. |
| `border` per-theme ratio | 1.31 – 2.99 | **measured** — WCAG ratio computed from the recorded `fg`/`bg` pairs; per theme: discord/discord-80 1.31, intellij-light 1.32, catppuccin-latte 1.36, macaw-dark 1.38, ferra-light 1.63, dark-green 1.79, catppuccin-macchiato 1.86, macaw-light 1.99, catppuccin-mocha 2.00, berlin 2.18, flexor-dark 2.34, rose-pine-dawn 2.57, one-dark/oxocarbon 2.58, rose-pine-moon 2.77, cork/nostromo-terminal 2.82, bam 2.84, broc 2.89, acton 2.97, ferra 2.99 |
| entries removed by (C) | 935 / 880 / 715 / 550 / 0 | **measured** — count of the 1,210 rows whose ratio is below 1.5 / 1.75 / 2.0 / 2.5 / 3.0 |
| `warning` entries | 35 (5 surfaces × 7 themes) | **measured** — recorded rows whose `fg` is `#FFB43C`, on `Confirm`, `Permission`, `Plan`, `Question`, `Theme`; matches `007-status.md` §3. Of these, **21 are strokes** (`Confirm`, `Plan`, `Question` frames) and **14 are text** (`Permission` risk labels, `Theme` hex swatches) |
| `PanelManager` border-token entries | 22 | **measured** — recorded rows for `PanelManager` whose `fg` equals the theme's border; the token is used as text at `panel_list.rs:164` |
| rows M003 removed | 528 (24 surfaces × 22 themes) | **read** — `docs/tui-render-invariant-baseline.toml:105-116` |
| rows M002 banked | 78 (13 surfaces × 6 themes) | **read** — `docs/tui-render-invariant-baseline.toml:93-103`, `007-status.md` §1 |
| border glyph set | U+2500–U+257F (+ U+2588 thumbs) | **measured** — no `BorderType::` and no `border_set(` under `src/tui`; the only hand-drawn rule strokes are `─` at `messages.rs:3388` and `│`/`█` at `editor.rs:1017` (plus one assertion string at `status_bar.rs:392`, which paints nothing) |

**What is not measured here:** the exact split of `PanelManager`'s 22 entries
between a stroke cell and the hint-text cell. Both share one
`(surface, theme, fg, bg)` key, so the entry survives under D′ either way; the
1,188 figure assumes only that no *other* surface paints text with `border`
(searched: `sidebar.rs:2240` and `editor.rs:1012` are scrollbar tracks, i.e.
strokes). No harness run was needed or performed for this memo.

## 5. Open questions for the owner

1. Is a **neutral** panel edge information ("this is a surface boundary") or
   decoration ("two surfaces are separated")? A says information; D′ says
   decoration. This is the whole decision; the entry counts only price it.
2. If C, what number, and where is it documented? `architecture/theme.md` owns
   token semantics today; a stroke tier would be its first non-text bar.
3. Should the invariant's wording change with the decision? I5 currently says
   "every rendered foreground"; if strokes are exempt, the honest wording is
   "every glyph a reader reads", and
   `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md:104`
   should say so.
