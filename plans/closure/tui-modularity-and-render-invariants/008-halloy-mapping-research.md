# M008 — Mapping Halloy themes onto the TUI's tokens

Research memo, 2026-11-13

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`, worktree
uncommitted (20 re-authored values across 14 bundled theme files, plus the whole
M001–M007 tree).

`[contrast]` baseline at the time of writing: **1,448** rows
(`docs/tui-render-invariant-baseline.toml` `[totals]`), of which **1,232 are
`border`**, **215 are `Theme` picker swatches**, and **1 is one `app:100x32`
cell**.

Subject: the owner's instruction of 2026-11-13 —

> "it may be worth revisiting the themes themselves, or reviewing the way in
> which we adapt halloy themes for our TUI. the Halloy themes themselves when
> used in halloy are pretty good about this, so it may be that there are better
> ways to map this. I'd prefer to keep the theme files themselves as they are,
> since they are technically third party themes, so research how to best handle
> this."

**No theme file was edited, and no production file was changed, to produce this
memo.** Every number below is either read from a file, computed from
`assets/themes/halloy/*.toml` and `docs/tui-render-invariant-baseline.toml`, or
read from upstream Halloy documentation. The render corpus was **not** run.

---

## 0. The finding

Our Halloy import is a **translation**, and four of the fields it translates do
not mean in Halloy what we use them for: `ui.accent_primary` is an unread-message
dot, `ui.accent_secondary` is a button fill, `text.muted` is a third accent in
37 of 50 themes, and `ui.border` is a selection edge. The first three are already
contrast-gated (that is what makes them safe); the fourth is the 1,232-row
population and has no gate at all: we take
`buffer.border_selected` — the edge Halloy draws around the **selected** buffer,
and upstream's documented fallback for its sidebar focus indicator
(`buttons.primary.border_active`, which a theme disables by authoring it
transparent) — and use it as the edge of **every unfocused panel and
dialog**, 55 surfaces per theme. Halloy's own neutral chrome edge is
`general.border`, and that value clears 3:1 in only **12 of the 50** bundled
themes, because Halloy never asks a stroke to carry a separation: it paints a
pane with its own fill (39 of 50 themes), a title bar with a third fill (50 of
50 themes), layout spacing, and a focus indicator, and the stroke is the fourth
signal. Our TUI collapses the three fill channels and keeps only the stroke —
on the same fill as the app backdrop.

The consequence is arithmetic rather than aesthetic. `general.border` at 1.01 –
2.82 on 19 of the 22, `buffer.border_selected` at 1.31 – 2.99 on all 22, and
`buffer.border` **transparent** on 13 themes: the gallery's border values were
never designed to be read as 1-cell text foregrounds, so no choice of *source*
can make them clear a text bar. The one thing that can is a derivation in the
projection, and the palette already contains the material for it: the theme's
own hue, moved along the lightness axis until it clears.

That is the shape of the fix, and it also answers the re-authoring question:
**all 20 edited values should be reverted**, because 16 of them are
lightness-only nudges that the same derivation reproduces within a few units,
and 4 of them replaced the author's hue with a grey or with a different palette
entry — which is a defect the edit *created*, not one it fixed.

---

## A. The mapping table, audited

### A.1 What the code actually reads

`src/theme/halloy.rs:153-525` is the whole projection. The table below is
re-derived from the code, not from `docs/themes.md` (which is stale in four
rows; see A.4). "Halloy's meaning" is from the upstream theme schema
(`https://halloy.chat/guides/custom-themes.html`) and the upstream changelog,
both cited where they decide something.

The ratatui `Theme` (`src/tui/theme.rs:44-75`) has 19 fields, 16 of which are
colours. The semantic layer resolves **19** colours from Halloy fields — 2
`base`, 11 `ui`, 2 `text`, 3 status, 1 `code` — and derives 2 of them. The table
covers all 19 plus the alias tokens that share a source.

| # | Token (`SemanticTheme` → `Theme`) | Halloy source, in precedence order | What Halloy means by it | Kind match? |
|---|---|---|---|---|
| 1 | `base.background` → `background` | `general.background` | the window background | **yes** |
| 2 | `base.foreground` → `foreground` | `text.primary` (also sets `conversation.user/assistant/system`) | the default text colour | **yes** |
| 3 | `ui.accent_primary` → `primary` | `general.unread_indicator` → `general.highlight_indicator` → `buttons.primary.background_selected` → `text.secondary` → `buffer.url` → `text.primary`, each rejected below 3.0 (`halloy.rs:267-328`) | an unread-message **dot**; a button **fill**; a URL; text | **no** for candidates 1–3, which are indicators and fills used as text; the gate at 3.0 is what makes it safe, and candidates 4–6 are text |
| 4 | `ui.accent_secondary` → `secondary` | `buttons.secondary.background_selected` gated at 3.0, else derived from `text.secondary`/`buffer.url`/`text.primary`, else `derive_readable_variant` (`halloy.rs:339-402`) | a button **fill** | **no**, mitigated by the gate — already recorded as a known hazard |
| 5 | `ui.border` → `border` | `buffer.border_selected` → `buffer.border` → `general.border` → `general.horizontal_rule`, all four collapsed into one value (`halloy.rs:207-222`) | (a) the edge of the **selected** buffer; (b) the edge of an **unselected** buffer; (c) the **window frame**; (d) a horizontal **divider** | **no** — see A.2. This is the defect |
| 6 | `ui.border_focused` → *(no field)* | same as #5 | same as #5 | **dead**: the renderer paints focus with `primary` (`src/tui/panels/chrome.rs:61-67`), and `Theme` has no `border_focused`, so the token is resolved and never read |
| 7 | `ui.selection` → `selection` | `buffer.selection` | the selected-text **fill** | **yes**, except that the file's alpha is discarded (A.4) |
| 8 | `ui.selection_dim` → `selection_dim` | derived (`halloy.rs:643-656`) | — | n/a |
| 9 | `ui.selection_text` → `selection_text` | derived (`schema.rs:120-123`) | — | n/a |
| 10 | `ui.accent_text` → `accent_text` | derived | — | n/a |
| 11 | `ui.panel_background` → `alternate_bg` | `buffer.background` | the **buffer pane fill** | **yes as a value, no as a use**: our panel body is filled with `background`, not with this (`chrome.rs:116-134`); `alternate_bg` paints editor current-line and scrollbar tracks only |
| 12 | `ui.input_background` → `input_bg` | `buffer.background_text_input` | the text-input fill | **yes** |
| 13 | `ui.title_background` → *(no field)* | `buffer.background_title_bar` | the **pane title-bar fill**, authored by **50 of 50** themes | **no**: parsed at `halloy.rs:247-254`, never projected (`target.rs:52-76` has no field), never painted |
| 14 | `text.muted` → `muted` | `text.tertiary` (`halloy.rs:198-206`) | the third **text** tier | **partly**: the slot is a text tier, but in **37 of 50** themes `text.tertiary` is a saturated accent, not a de-emphasis colour (E.3) |
| 15 | `text.link` → `link` | `buffer.url` | URL text | **yes** |
| 16 | `status.success` → `success` | `text.success` | status text | **yes** |
| 17 | `status.warning` → `warning` | `text.warning`, authored by **0 of 50**, else `derive_readable_variant` of the inherited value (`halloy.rs:408-436`) | status text | **yes**, and already derivation-gated |
| 18 | `status.error` → `error` | `text.error` | status text | **yes** |
| 19 | `status.info` → *(no field)* | `text.info` (0/50) → `buffer.server_messages.default` (50/50) | status text | **yes** by accident: the `text.info` slot is empty in the whole gallery, and the server-message default fills it |
| — | `status.debug` | `text.debug` (0/50) | status text | inherited from the fallback on every bundled theme |
| — | `status.trace` | `text.trace` — **the field is declared and never read** (`HalloyText::trace`, `halloy.rs:96`; no read anywhere) | status text | **no**: the importer never consults it, so `status.trace` is always the fallback's value |
| — | `code.foreground` (`code_theme`) | `buffer.code` | inline-code text | **yes** |
| — | `diff.added/removed/modified` | mirrors `status.*` | — | unprojected: `Theme` carries no diff field |
| — | `conversation.*` | `text.primary`, `buffer.timestamp`, `buffer.action`, `server_messages.default` | — | unprojected, except `user/assistant/system`, which are all `text.primary` |
| — | `agents.*` | `text.info`, `buffer.highlight` | a **highlight** colour | unprojected |

Field presence across the gallery (all 50 files, re-derived for this memo):

| Key | Present | Key | Present |
|---|---|---|---|
| `general.border` | 50/50 | `text.success` | 50/50 |
| `general.horizontal_rule` | 50/50 | `text.warning` | **0/50** |
| `general.unread_indicator` | 50/50 | `text.error` | 50/50 |
| `general.highlight_indicator` | **0/50** | `text.info` | **0/50** |
| `buffer.border` | 50/50 | `text.debug` | **0/50** |
| `buffer.border_selected` | 50/50 | `text.trace` | **0/50** |
| `buffer.background` | 50/50 | `buffer.focus_border` | **0/50** |
| `buffer.background_title_bar` | 50/50 | `buttons.*.border_active` | **0/50** |
| `buffer.selection` | 50/50 | `formatting.highlight` | **0/50** |
| `buttons.secondary.background_selected` | 50/50 | `buffer.server_messages.default` | 50/50 |

### A.2 The border collapse, in the code's own words

The projection states its intent:

> "Uniform border: pick one source and use it for both `ui.border` and
> `ui.border_focused`. The TUI's border usage is dominated by neutral chrome
> (panels, dialogs, list tracks) which shouldn't shift color when an element
> gains focus. The fallback chain prefers the most 'prominent' border the theme
> author provided."
>
> — `src/theme/halloy.rs:207-211`

Every clause is defensible on its own and the conclusion is still wrong:

1. It is right that our chrome should not shift colour on focus — it doesn't;
   focus is `primary` (`chrome.rs:61-67`).
2. It is right that one token is needed.
3. **"The most prominent border the theme author provided" is precisely
   `buffer.border_selected`**, which is *the author's focus colour*. We take the
   author's focus colour for the surfaces that are **not** focused, and paint
   the focused one with a third colour entirely. Both channels are shifted one
   step up in prominence relative to Halloy, and the author's quiet value
   (`buffer.border`) is never used at all.

Upstream decides the semantics beyond argument. The 2026.7 changelog entry:

> "The focused buffer is indicated in the sidebar by a thin border (can be
> configured with the optional theme color `buttons.primary.border_active`,
> falls back to `buffer.border_selected` if no color is configured; set to
> `"#00000000"` to disable)"

So `buffer.border_selected` is *the sidebar's focus indicator*, and a theme may
express "no indicator" by authoring it transparent. `general.border` is the
window frame — a different surface, with a different job.

### A.3 The `buffer.border` trap

Tempting as it is to restore the faithful mapping
(`buffer.border` → `ui.border`, `border_selected` → focus), it is not available
today:

- **13 of 50 themes author `buffer.border = "#00000000"`** — upstream's
  documented spelling of "no paint" ("if the key is removed, the color will fall
  back to transparent"). `Rgb::from_hex` strips alpha and yields `#000000`, and
  `halloy.rs` has a test asserting exactly that
  (`parses_eight_digit_hex_in_general_border`, `halloy.rs:793-811`). Mapping it
  to chrome would paint **black** frames on 13 themes and would push
  `buffer.border` below 3.0 on **46 of 50** themes.
- The repo has no alpha concept (`docs/themes.md` §"8-digit hex"), so
  "transparent" cannot be represented.

This is not a reason to abandon the mapping; it is the reason the *stroke
value* has to be computed rather than copied.

### A.4 Where the documentation disagrees with the code

`docs/themes.md` "Halloy field mapping" is stale in four rows. They are harmless
today and actively misleading to anyone auditing this question:

| Documented | Actual (`halloy.rs`) |
|---|---|
| `[text].secondary` → `text.muted` | `text.tertiary` → `text.muted`; `text.secondary` is only an accent candidate |
| `[text].tertiary` → `conversation.assistant` | no such mapping; `text.primary` sets all three conversation roles |
| `[buffer].border_selected` → `ui.border_focused` | it is the **first** source for `ui.border`, and `border_focused` is unprojected |
| `[buffer].topic` → `conversation.assistant`, `[buffer].nickname` → `conversation.user` | deliberately not mapped (`halloy.rs:483-488`) |

Two more tokens are resolved and then dropped: `ui.title_background` (50/50
themes author the key) and `ui.border_focused`. `status.trace`'s source is never
read. None of the three is a contrast problem; all three are the kind of thing
that makes a mapping audit conclude the wrong thing about what we paint.

---

## B. Why these themes look right in Halloy

The owner's premise — "the Halloy themes themselves when used in halloy are
pretty good about this" — is **true for glyphs and false for lines**. That is
the whole of section B, and it is measurable.

### B.1 The gallery is good about text

| Population | Result |
|---|---|
| `text.primary` against `general.background`, **original** (HEAD) values | **48 of 50 clear 4.5:1**; `cyber-red` sits at 4.50, `rose-pine-dawn` at 2.45 |
| `text.tertiary` against `general.background`, **original** (HEAD) values | **44 of 50 clear 3.0**; the 6 that do not are 2.33–2.81, and §E.2 shows they clear with a hue-preserving lift |
| `text.secondary` against `general.background` | 39/50 clear 3.0 |

So a Halloy theme is a competent *text* palette. Read a paragraph in Halloy and
it reads.

### B.2 The gallery is not good about lines

| Population | Result |
|---|---|
| `general.border` against `general.background` | **clears 3.0 in 12 of 50**; the other 38 sit at 1.01–2.82 |
| `buffer.border_selected` against `general.background` | clears 3.0 in 28 of 50 |
| `buffer.border` after alpha-stripping | clears 3.0 in **4 of 50** |
| `2 ×` border values below 3.0 among the 22 themes we measure | 22 |

The extreme is `dark-green`, whose `general.border` (`#020E01`) is
indistinguishable from its background (`#020F01`, **1.01:1**) — the theme does
not author a frame colour at all; the only visible line in the file is
`horizontal_rule = #1C451A`.

### B.3 What carries the separation in Halloy, and what we dropped

Halloy's pane separation is a stack of four signals. We projected one of them
and picked its weakest variant.

| Signal | Halloy | This TUI |
|---|---|---|
| **pane fill** | `buffer.background` differs from `general.background` in **39 of 50** themes (median 1.11, max 1.32 on `macaw-dark`) | the panel body is filled with `theme.background` — **the same fill as the app backdrop** (`src/tui/panels/chrome.rs:116-134`). `alternate_bg` carries the value and paints editor lines and scrollbar tracks |
| **title-bar fill** | `buffer.background_title_bar`, authored by **50 of 50** | parsed into `ui.title_background` and **never projected** (`target.rs:52-76`) |
| **layout spacing** | iced containers with padding; the theme sets pane padding and gaps | a terminal grid: adjacent panels share a cell boundary, so separation must be a glyph |
| **stroke** | a 1-logical-pixel vector line, anti-aliased, at `scale_factor` device pixels, around the window (`general.border`) and each pane (`buffer.border` / `border_selected`) | a full-cell `─│┌┐└┘` block glyph at 100% of the foreground colour, unresampled, around **55 surfaces per theme** |

Two consequences follow, and both are checkable:

1. **Halloy paints single-digit borders; we paint 55 per theme.** Halloy's
   surfaces are: the window frame, one edge per open pane (a typical layout is
   1–4), and the sidebar's single focus indicator. The 55 measured surfaces are
   23 dialogs + 32 panels, every one of them edged.
2. **The stroke is redundant there and load-bearing here.** In Halloy a 1.31:1
   edge is one of four cues, two of which are large-area fills; if you cannot
   see the edge you still see the pane. In this TUI the edge is the only cue on
   an identical fill, so the same ratio is not the same perceptual fact.

The clean formulation: **Halloy's border values are tuned to be a whisper
around a filled pane; ours is the only line on an unfilled one.**

### B.4 Halloy never inherits, either

Upstream: *"Each `<color string>` is expected to be a valid hex color. If
invalid, or if the key is removed, the color will fall back to transparent."*
An omitted key means **no paint**, not another palette's colour. ADR-0062's
clause 2 (repair inheritance in the projection) is therefore not a divergence
from Halloy — it is the closest available approximation of it, since our layer
cannot express transparent. Note the residual: we substitute a *derived* value
where Halloy substitutes *nothing*.

### B.5 What this means for "the themes look right in Halloy"

They do, and for reasons our mapping does not reproduce:

- a filled pane behind the stroke (dropped: we fill with the backdrop);
- a filled title bar (dropped: unprojected);
- a single-digit border count (not dropped — our layout simply needs edges);
- a focus indicator that is *allowed* to be a whisper (inverted: we use it for
  the unfocused edges, and paint focus with the accent).

So the premise is correct and it points at the projection, exactly as the owner
suspected. It does not point at a better *single field*: §C shows there is none.

---

## C. The `border` population, split by cause

### C.1 The population is uniform, which is what makes the split tractable

All **1,232** rows are `(surface, theme, fg, bg)` records for which `fg` equals
the theme's resolved `ui.border`. They decompose exactly:

- **22 themes × 56 surfaces = 1,232.** No theme contributes more or fewer.
- 56 = **23 dialogs + 32 panels + 1 `app:100x32`**. The first 1,210 rows are the
  55 dialog/panel surfaces; the last 22 are the app frame, one per theme, which
  M007 finding 1 revealed when it themed the frame honestly.
- Each failing theme therefore contributes exactly **56 rows**, so any per-theme
  split converts to rows by ×56.

The 22 themes: `acton`, `bam`, `berlin`, `broc`, `catppuccin-latte`,
`catppuccin-macchiato`, `catppuccin-mocha`, `cork`, `dark-green`, `discord`,
`discord-80-saturation`, `ferra`, `ferra-light`, `flexor-dark`,
`intellij-light`, `macaw-dark`, `macaw-light`, `nostromo-terminal`, `one-dark`,
`oxocarbon`, `rose-pine-dawn`, `rose-pine-moon`.

### C.2 Per-theme table

`cur` is the value the projection resolves today (Halloy precedence), `r` its
ratio against `general.background`, `chr` its max-minus-min channel (0 = a pure
grey), `gen` is `general.border` and `r_gen` its ratio, `→` is
`derive_readable_variant(cur, background, 3.0)`, and `Δ` is that derivation's
largest single-channel move.

| Theme | cur | r | chr | gen | r_gen | cause | → derived | Δ |
|---|---|---|---|---|---|---|---|---|
| `acton` | `#B76891` | 2.97 | 79 | `#73628D` | 2.12 | (i) | `#B86992` | 1 |
| `bam` | `#9F488C` | 2.84 | 87 | `#374B1B` | 1.64 | (i) | `#A24E8F` | 6 |
| `berlin` | `#883D28` | 2.18 | 96 | `#1A4458` | 1.60 | (i) | `#985745` | 29 |
| `broc` | `#4F75A2` | 2.89 | 83 | `#585335` | 1.78 | (i) | `#5278A4` | 3 |
| `catppuccin-macchiato` | `#4C4163` | 1.86 | 34 | `#6E738D` | **3.73** | (i) | `#6A617D` | 32 |
| `catppuccin-mocha` | `#4C4163` | 2.00 | 34 | `#6C7086` | **3.84** | (i) | `#655C79` | 27 |
| `cork` | `#4969AA` | 2.82 | 97 | `#254F29` | 1.62 | (i) | `#4E6EAD` | 5 |
| `discord` | `#404449` | 1.31 | 9 | `#222326` | 1.22 | (i) | `#787B7E` | 56 |
| `discord-80-saturation` | `#404449` | 1.31 | 9 | `#222326` | 1.22 | (i) | `#787B7E` | 56 |
| `oxocarbon` | `#635081` | 2.58 | 49 | `#262626` | 1.20 | (i) | `#6C5B89` | 11 |
| `macaw-dark` | `#1A1A1A` | 1.38 | 0 | `#505050` | 1.57 | (ii) | `#7C7C7C` | 98 |
| `intellij-light` | `#E0E0E0` | 1.32 | 0 | `#BFBFBF` | 1.84 | (iii) | `#949494` | 76 |
| `catppuccin-latte` | `#CCB8EF` | 1.36 | 55 | `#9CA0B0` | 1.97 | (iii) | `#87799E` | 81 |
| `ferra-light` | `#C7B9B1` | 1.63 | 22 | `#DDD0C8` | 1.29 | (iii) | `#918781` | 54 |
| `macaw-light` | `#ADADAD` | 1.99 | 0 | `#B5B5B5` | 1.82 | (iii) | `#8B8B8B` | 34 |
| `rose-pine-dawn` | `#9893A5` | 2.57 | 18 | `#DFDAD9` | 1.19 | (iii) | `#8C8798` | 13 |
| `dark-green` | `#1C451A` | 1.79 | 43 | `#020E01` | **1.01** | (iv) | `#446642` | 40 |
| `ferra` | `#7D6E76` | 2.99 | 15 | `#4F474D` | 1.61 | (iv) | `#7D6F76` | 1 |
| `flexor-dark` | `#575653` | 2.34 | 4 | `#343331` | 1.36 | (iv) | `#676764` | 17 |
| `nostromo-terminal` | `#555555` | 2.82 | 0 | `#555555` | 2.82 | (iv) | `#5A5A5A` | 5 |
| `one-dark` | `#606368` | 2.58 | 8 | `#737373` | **3.29** | (iv) | `#6A6D72` | 10 |
| `rose-pine-moon` | `#6E6A86` | 2.77 | 28 | `#44415A` | 1.46 | (iv) | `#73708B` | 6 |

### C.3 The split

| Cause | Themes | Rows | What it is |
|---|---|---|---|
| **(i) a selection/focus edge reused as chrome** | **10** — `acton`, `bam`, `berlin`, `broc`, `catppuccin-macchiato`, `catppuccin-mocha`, `cork`, `discord`, `discord-80-saturation`, `oxocarbon` | **560** | the resolved value is `buffer.border_selected`, and the file distinguishes it from `general.border`. In 8 of the 10 it is chromatic (chroma 34–97): an accent hue imported into chrome. In `discord`×2 it is a neutral, but it is still the *selected-pane* value (`#404449`) against a near-black window frame (`#222326`) and a groove (`#2E3338`) |
| **(ii) a deliberate shadow** | **1** — `macaw-dark` | **56** | `buffer.border == buffer.border_selected == #1A1A1A`, which is **darker than its own pane fill** `#1E1E1E`: a recessed groove, not an outline. It is the only one of the 22 that resolves a border darker than the fill it is drawn on, on a dark background |
| **(iii) a pale hairline on a light background** | **5** — `intellij-light` (`#E0E0E0` on `#FFFFFF`), `catppuccin-latte`, `ferra-light`, `macaw-light`, `rose-pine-dawn` | **280** | background luminance > 0.5 and a near-background value. `catppuccin-latte` is *also* a selection tint (`#CCB8EF`, chroma 55, and **paler** than its own `general.border` `#9CA0B0`) — the two causes compose; it is counted once, here |
| **(iv) no legible chrome value authored / a near-miss mid-tone** | **6** — `dark-green`, `ferra`, `flexor-dark`, `nostromo-terminal`, `one-dark`, `rose-pine-moon` | **336** | `dark-green`: `general.border` is the background (1.01) and `buffer.border` is 1.09, so the theme authors no frame; `nostromo-terminal`: all four keys are `#555555`; `ferra` 2.99, `one-dark` 2.58, `flexor-dark` 2.34, `rose-pine-moon` 2.77: a single authored neutral with no stronger alternative |
| **Total** | **22** | **1,232** | |

Three cross-cutting facts, because they decide §D:

1. **Only 3 of the 22 have a passing value already in their file** —
   `catppuccin-macchiato` 3.73, `catppuccin-mocha` 3.84, `one-dark` 3.29, all
   `general.border`. That is **168 rows** clearable by choosing a different
   field, and no more.
2. **The split does not partition the remedy.** It explains *why* each value is
   low; the same one-directional lightness move fixes all four classes, because:
3. **The direction of any fix is forced by the background's luminance, and the
   shadow style is arithmetically impossible at 3.0.** On `macaw-dark`
   (`#333333`) the floor for darkening is `contrast(black, #333333) = 1.66`; on
   `discord` (`#303236`) it is 1.64. No darker border can ever reach 3.0 on
   those backgrounds. Symmetrically, on `#FFFFFF` the ceiling for lightening is
   1.00, so `intellij-light`'s pale hairline can only be fixed by darkening.
   **A shadow-style border cannot satisfy a 3.0 stroke bar on a dark theme.**
   That is not an opinion about the palette; it is arithmetic.

---

## D. Options for handling it without touching the theme files

Entry effects are exact: 56 rows per theme, so a theme that stops failing
removes 56 rows.

### Option 1 — derive the chrome edge in the projection

Two variants, both in `src/theme/halloy.rs`, both reusing
`derive_readable_variant` (`halloy.rs:601-641`), which moves a colour along the
lightness axis toward whichever of black/white reads better on the background
and stops at the bar. It preserves hue by construction, and it is already the
accepted mechanism for `status.warning` and a rejected `accent_secondary`.

**1a — minimal lift.** `ui.border = derive_readable_variant(cur, background, 3.0)`.
- Removes **1,232 of 1,232**.
- Appearance: **8 themes move by ≤10 in every channel** (imperceptible:
  `acton` Δ1, `ferra` Δ1, `broc` Δ3, `cork` Δ5, `nostromo-terminal` Δ5,
  `bam` Δ6, `rose-pine-moon` Δ6, `one-dark` Δ10); **7 move by 11–34**
  (`oxocarbon` 11, `rose-pine-dawn` 13, `flexor-dark` 17, `mocha` 27,
  `berlin` 29, `macchiato` 32, `macaw-light` 34); **7 move by >34** and are a
  visible restyle of the stroke (`dark-green` 40, `ferra-light` 54,
  `discord` 56 ×2, `intellij-light` 76, `catppuccin-latte` 81, `macaw-dark` 98).
- On `macaw-dark` specifically: `#1A1A1A` groove → `#7C7C7C` outline. The
  recessed look becomes an outlined one. This is the option's clearest cost and
  the reason the verdict in §F is priced rather than free.

**1b — minimal lift, with the semantically correct source preferred when it
already clears.** Keep the current chain; if the resolved value fails, try
`general.border`; derive only if that fails too.
- Removes **1,232 of 1,232**, identically.
- Appearance: **28 themes byte-identical**; 3 take the file's own
  `general.border` (`#6E738D`, `#6C7086`, `#737373`); 19 take a derivation.
- Why 1b over 1a: for those 3 the correct *kind* of value already exists in the
  file at a passing ratio, and using it costs one line and changes nothing the
  author did not already write.
- Why not "always prefer `general.border`": it clears 3.0 in only 12 of 50, and
  switching unconditionally would break **19 currently-passing themes**
  (1,064 rows) to fix 22. Net **+896 rows**. This is the decisive number
  against Option 3.

ADR: **amendment required** — see the amendment appended to ADR-0062.

Evidence it landed: the `border` family is absent from `[contrast]`; a test
asserts every bundled theme's resolved `ui.border` clears
`MIN_DERIVED_TEXT_RATIO` against its own `base.background`; the 28 unchanged
themes' resolved borders are asserted byte-identical to today's, so a future
change to the chain cannot silently restyle the gallery.

### Option 2 — split the token (focused edge and unfocused edge)

Project `ui.border_focused` separately and let unfocused chrome be deliberately
quieter.
- Removes **0 of 1,232**. The unfocused edge is still painted, so the harness
  still measures it. The split changes what the token *means*, not what any cell
  contains.
- Worse than neutral: it invites routing focus to `border_selected`, which is
  **1.31:1 on `discord`** — the focus edge would then fail the invariant that
  the accent currently satisfies. Our focus channel is *safer* than Halloy's own
  because it uses the gated accent.
- Needs an ADR (it reopens the stroke decision M007 settled: a decorative
  separator is or is not text). The owner chose "it is text" on 2026-11-12, so
  this option is a request to reverse an accepted decision in order to avoid a
  projection fix.
- Verdict: **not a contrast option.** It is a semantic cleanup with a real but
  separate argument (the meta-layer currently claims a focus value the renderer
  never uses). Worth doing as a 1-line honesty fix — set
  `ui.border_focused = accent_primary`, which is what is painted — and not as a
  remedy.

### Option 3 — map from a different Halloy field

| Candidate | Clears 3.0 | Would remove | Would add |
|---|---|---|---|
| `general.border` only | **12/50** | 168 (3 themes) | **1,064** (19 themes) |
| `buffer.border` only | **4/50** (46 below, 13 of them transparent→black) | 0 | ~2,500 |
| `general.horizontal_rule` only | fewer still (it is a divider, usually quieter than `general.border`) | 0 | more |

No field in the Halloy schema means "a stroke that must clear 3:1 against the
app background", because Halloy does not need one. Option 3 is **rejected on
measurement**, not on taste: it is the only option that makes the baseline
larger.

Its useful residue is step 2 of Option 1b: use `general.border` *when it
already clears*, which is 3 themes and free.

### Option 4 — keep the mapping and accept the rows

- Removes **0**. Appearance unchanged.
- Requires formally withdrawing M007 acceptance criterion 1 ("the baseline is
  empty") and recording 1,232 accepted rows, i.e. converting the strongest
  automated accessibility assertion in the product into a known-false claim for
  22 of 50 shipped themes.
- ADR-0062 Option D already rejected this shape, for the reason that still
  applies: an accepted limitation with no owner becomes a permanent exception,
  and the next audit reopens it.
- It also does not answer the owner's question, which presumes handling.

### Option 5 — the residue: the 215 `Theme` picker swatches

Not part of the 1,232 and not decided here, but it must be named because it is
the second-largest population and it is a different defect. `render_color_swatch`
(`src/tui/components/dialogs/theme.rs:310-316`) paints a listed theme's
`primary/secondary/success/warning/error` **as a foreground glyph** on the
*drawing* theme's background. A swatch exists to show a colour that may be
unreadable on the current fill — that is what a swatch *is* — so the pair is
cross-palette by construction (209 distinct `(fg,bg)` pairs; 7 of them are the
fallback `#FFB43C` inherited by a dark theme and shown on a light one).

The honest render fix, for a later milestone: paint the chip as a **background**
(`bg(swatch)`) and its hex text in `derive_text_on(fg, bg, swatch)` — a colour
sample is a fill, not a glyph. That clears all 215 by construction and makes the
picker more like what it depicts. I have not priced it and it is out of scope
here.

### Comparison

| | Removes | Shipped appearance changes | ADR | Evidence |
|---|---|---|---|---|
| **1a** derive, minimal lift | **1,232** | 22 themes; 8 imperceptible, 14 visible | amendment | §C table + a 50-theme border test |
| **1b** derive + prefer a passing `general.border` | **1,232** | 22 themes; 28 themes untouched; 3 take the file's own value | amendment | same, plus a "28 unchanged" assertion |
| **2** split the token | 0 | none | yes (reverses the stroke decision) | none — no row moves |
| **3** different field | 168 | 41 themes restyled (19 worse) | no | **net +896 rows** |
| **4** accept | 0 | none | yes (withdraws M007 AC1) | baseline stays 1,232 |
| **5** picker swatch as a fill | 215 | the picker only | no | 215 → 0 by construction |

---

## E. The same audit for the families ADR-0062 "fixed" by re-authoring

The working tree carries **20 changed values across 14 files**
(`git diff --stat -- assets/`). Every one is a value in a third-party file. This
section asks, for each, whether a projection rule would have produced the same
outcome.

### E.1 The 20 values, and what a derivation gives instead

`derived` is `derive_readable_variant(original, general.background, bar)` with
the bar each token's own contract names (4.5 for body text, 3.0 otherwise).
`Δ` is the largest single-channel difference **between the hand edit and the
derivation** — small means the edit was a manual version of the same move.

| Theme | Key | Original | ratio | Hand-edited to | ratio | Derived | ratio | Δ(edit,derived) |
|---|---|---|---|---|---|---|---|---|
| Catppuccin Latte | `text.success` | `#40A02B` | 2.53 | `#377F24` | 3.04 | `#3A9127` | 3.02 | 18 |
| Cyber Red | `text.tertiary` | `#7D3F50` | 2.66 | `#8A4E5F` | 3.29 | `#834959` | 3.02 | 11 |
| Discord | `text.tertiary` | `#72767D` | 2.81 | `#7E828A` | 3.33 | `#777B81` | 3.02 | 10 |
| Discord (80%) | `text.tertiary` | `#72767D` | 2.81 | `#7E828A` | 3.33 | `#777B81` | 3.02 | 10 |
| Discord | `text.error` | `#C64F52` | 2.84 | `#ED4245` | 3.35 | `#C85658` | 3.01 | 37 |
| Discord (80%) | `text.error` | `#C64F52` | 2.84 | `#ED4245` | 3.35 | `#C85658` | 3.01 | 37 |
| Gruvbox | `text.error` | `#CC241D` | 2.69 | `#FB4934` | 4.06 | `#D13731` | 3.02 | 42 |
| IntelliJ Light | `text.success` | `#B0D1AB` | 1.67 | `#4E8C4A` | 3.12 | `#839B7F` | 3.02 | 53 |
| Macaw Dark | `text.tertiary` | `#8839EF` | 2.33 | `#959595` | 4.22 | `#9A57F1` | 3.01 | **91** |
| Macaw Light | `text.success` | `#959595` | 2.66 | `#7A7A7A` | 3.18 | `#8B8B8B` | 3.02 | 17 |
| Macaw Light | `text.error` | `#959595` | 2.66 | `#7A7A7A` | 3.18 | `#8B8B8B` | 3.02 | 17 |
| Noctis Lilac | `text.success` | `#00B368` | 2.35 | `#008F53` | 3.21 | `#009D5C` | 3.00 | 14 |
| Noctis Lilac | `text.error` | `#FF530F` | 2.76 | `#E04A0D` | 3.31 | `#F44F0E` | 3.01 | 31 |
| Rose Pine Dawn | `text.primary` | `#D7827E` | 2.45 | `#575279` | 6.28 | `#985C59` | 4.52 | **122** |
| Rose Pine Dawn | `text.success` | `#56949F` | 2.96 | `#4B8894` | 3.26 | `#55929D` | 3.03 | 11 |
| Zenburn | `text.success` | `#5F8700` | 2.77 | `#709B00` | 3.55 | `#668D0C` | 3.00 | 16 |
| base16-atelier-forest-light | `text.tertiary` | `#C38418` | 2.76 | `#8A8280` | 3.14 | `#BA7E17` | 3.01 | **91** |
| base16-atelier-forest-light | `text.success` | `#7B9726` | 2.91 | `#708A1E` | 3.26 | `#799425` | 3.02 | 10 |
| berlin | `text.error` | `#9C4F3D` | 2.85 | `#B35E4A` | 3.17 | `#9F5342` | 3.01 | 24 |
| vik | `text.tertiary` | `#964A36` | 2.79 | `#A05A45` | 3.01 | `#9A503D` | 3.01 | 8 |

**What the table says.**

- **10 of the 20 were within 0.25 of the bar** (2.76–2.96). For those the edit
  moved the light by 1–18 units — indistinguishable from the derivation, and
  indistinguishable on screen from the original.
- **16 of the 20 are hue-preserving lightness nudges** that the derivation
  reproduces. The edit was a manual execution of the same rule.
- **4 replaced the author's hue or substituted a different palette entry**, and
  those are the ones a projection rule improves on:
  - `macaw-dark` `#8839EF` (a saturated violet) → `#959595` (**grey**). The
    theme's third text tier is violet; the edit made our muted tier grey. Δ 91.
  - `base16-atelier-forest-light` `#C38418` (base16 orange) → `#8A8280`
    (**grey**). Δ 91.
  - `rose-pine-dawn` `text.primary` `#D7827E` (the palette's *rose*, the theme's
    namesake) → `#575279` (rose-pine's standard body text). Δ 122. The edit is
    defensible as "fixing upstream", but it changes the theme's identity, and a
    hue-preserving lift to `#985C59` reads as a rose at 4.52:1 without
    substituting another palette entry.
  - `intellij-light` `text.success` `#B0D1AB` (a pale green) → `#4E8C4A` (a
    saturated green). Δ 53. A lift to `#839B7F` is the same hue, quieter.

### E.2 `muted` — is `text.tertiary` the right slot?

ADR-0062's own falsification clause: *"If a sampled set of upstream Halloy
themes uses `text.tertiary` as a saturated accent … then `muted ← text.tertiary`
is a slot-mapping error."* The clause fires, and much harder than "a sampled
set":

| `text.tertiary` chroma | Themes |
|---|---|
| **> 40** (a saturated accent, not a de-emphasis colour) | **37 of 50** |
| 20–40 | 8 |
| ≤ 20 (a genuine grey) | 5 |

Examples: `intellij-light` `#FF0000` (pure red, 4.00:1 — so it *passes* and is
invisible to the invariant), `macaw-light` `#8839EF`, `one-dark` `#F44747`,
`nord` `#D08770`, `sonokai` `#F39660`, `catppuccin-latte` `#8839EF`. Meanwhile
`text.secondary` is much more neutral in the same themes (chroma 25–45, and 0 on
`macaw-dark`, `oxocarbon`, `solarized-dark`).

So `muted` **is** a slot error in the sense that the slot is not a de-emphasis
tier in this gallery. But:

- **The contrast fix does not need the slot fixed.** Gate `text.tertiary` at 3.0
  and derive on failure: 6 themes, and the derivation reproduces the hand edit
  within 11 units on 4 of them.
- **Fixing the slot would restyle 37 themes' de-emphasis colour** — a far larger
  visible change than the 6 contrast failures, and one the invariant cannot see.
  It is a design decision with no accessibility content.
- **The hand edit is worse than the derivation on 2 of the 6.** `macaw-dark` and
  `base16-atelier-forest-light` are the two whose `text.tertiary` is an accent at
  2.33/2.76; the edit replaced the accent with a grey (Δ 91 each), which
  fabricates a de-emphasis tier the author did not write. A gated derivation
  keeps the violet/orange at 3.01. Neither is *pretty*, but only one of them is
  the author's.

Recommendation: **keep `muted ← text.tertiary`, gate and derive.** Record the
37/50 slot finding as a known limitation with a named owner (a future "is the
muted tier allowed to be chromatic?" decision), and do **not** resolve it by
editing files — that is exactly how the two grey substitutions happened.

### E.3 `foreground` — the one genuinely defective upstream value

`rose-pine-dawn` ships `[text].primary = "#D7827E"` — the palette's *rose*
accent — as its body text, at **2.45:1**. In Halloy that value is the default
text colour, so the same defect exists there; this is the one theme in the
gallery where the owner's premise does not hold, and it is worth saying plainly
rather than inferring a mapping error.

The slot is right (body text → body text). The file's value is wrong. The
projection can fix it with the same rule at the **4.5** bar rather than 3.0,
because `base.foreground` is body copy and `validate_theme` already documents
4.5 (`validate.rs:51`): `derive_readable_variant(#D7827E, #F4EDE8, 4.5)` =
`#985C59` at 4.52.

### E.4 Verdict on the revert

**Revert all 20 values.** No edit was necessary, and four were actively harmful:

- 16 are manual implementations of the derivation rule, at a hand-picked
  lightness slightly above the bar;
- 4 changed the author's hue or substituted a different palette entry;
- the two `success`/`error` families whose values are *genuinely* low-contrast
  in a legitimate slot (`text.success`/`text.error` mean status text in both
  systems) are still better repaired in the projection, because a file edit
  fixes 13 values for 50 themes and leaves the 51st theme — and every user theme
  — failing.
- the one value that is genuinely wrong upstream (`rose-pine-dawn`'s
  `text.primary`) is better handled by a hue-preserving lift than by replacing
  it with a different palette entry.

The three files' `[text].warning` situation is unchanged: **0 of 50 author it**,
clause 2 already derives it, and nothing here touches that.

---

## F. Verdict

### F.1 The right shape

**A projection change in `src/theme/halloy.rs`.** One rule, applied to five
tokens, with a per-token bar:

> A colour read through the Halloy importer is a **translation**, and the
> projection owns its legibility. If the translated value fails the bar its
> consumer documents, raise its lightness toward the more legible pole — keeping
> its hue — until it clears, and record a warning naming the key. A value a
> `codegg` native theme writes into this schema names *our* token and stays the
> file's.

| Token | Bar | Failing themes today | Rows |
|---|---|---|---|
| `ui.border` | 3.0 | 22 | 1,232 |
| `text.muted` | 3.0 | 6 (after revert) | 354 |
| `status.success` | 3.0 | 7 (after revert) | 8 |
| `status.error` | 3.0 | 6 (after revert) | 6 |
| `base.foreground` | **4.5** | 1 (after revert) | 47 |
| `status.warning` | 3.0 | 7 — **already done** by ADR-0062 clause 2 | 0 |

Functions touched: `parse_halloy_theme` (`halloy.rs:153-525`) at the border block
(207-222), the muted block (198-206), the foreground block (188-197), and the
success/error blocks (404-440); plus `derive_readable_variant` promoted from a
private helper to the single shared definition (already `halloy.rs:601-641`). No
change to `validate.rs` constants, no change to `schema.rs`'s derived-token
contract, no change to `target.rs`, no render-site change, no theme file.

`ui.border_focused` is set to `accent_primary` in the same change — the value
the renderer actually paints — because a semantic token that claims a colour
nobody draws is a trap for the next frontend. This changes no pixel.

**Not** a token split: Option 2 moves zero rows and asks the owner to reverse an
accepted measurement decision in order to avoid a value fix.

**Not** an accepted residue: it is 22 of 50 shipped themes painting a chrome
that cannot be read, and the arithmetic says the palette cannot fix it in place.

### F.2 What it costs

- `[contrast]` 1,448 → **216** (215 picker swatches + 1 `app:100x32` cell), if
  all five gates land. The border gate alone takes it to 1,448 − 1,232 = **216**
  only because the other 415 rows do not exist in the current baseline (they were
  cleared by the re-authored values). **Reverting the 20 values and landing the
  projection rule must be one change**, or the baseline rises by up to 415 in
  between. That ordering is the plan's first work package.
- 22 themes' stroke values move: 8 imperceptibly, 14 visibly; `macaw-dark`'s
  groove becomes an outline. 28 themes byte-identical.
- 20 values revert to their third-party originals; the gallery's rendered
  appearance returns to what the Halloy gallery shows, except where the
  projection lifts a value to clear a bar.
- ADR-0062 needs an amendment: the authority rule changes from "does the file
  name the key" to "does the file name *this* token, or are we translating".
- One warning per derived token per theme, naming the key — 50-odd diagnostics
  across the gallery, visible in `/theme diagnostics`, noisy on first load and
  the whole point of the rule.

### F.3 The one piece of evidence that would overturn it

**A rendered comparison showing that a border derived to exactly 3.0 reads as a
hard wireframe** — i.e. that a full-cell `─` at 3:1 carries so much more ink than
the "contrast ratio" model implies that 14 themes' chrome becomes the dominant
element on screen. The derivation is deliberately minimal, but a 1-cell stroke
at 3:1 is a *line* where Halloy had a whisper, and if `macaw-dark` at `#7C7C7C`
framing 56 surfaces looks like a wireframe rather than an edge, then no
derivation can satisfy both the bar and the design, and the honest answer
becomes the token split plus an exemption decision (i.e. reopen M007's option
D′, which the owner declined on 2026-11-12).

Everything else in this memo is arithmetic or a citation. That is the only
judgement call.

---

## Provenance

| Number | Value | How |
|---|---|---|
| `[contrast]` total | 1,448 | **read** — `docs/tui-render-invariant-baseline.toml` `[totals] contrast`, and `grep -c '^\[\[contrast\]\]'` |
| border rows | 1,232 | **measured** — parsed `[[contrast]]`, resolved each labelled theme's `ui.border` from `assets/themes/halloy/*.toml` with `halloy.rs`'s precedence, matched on `fg`: exactly 22 themes × 56 surfaces, no residue |
| the 56 surfaces | 23 dialogs + 32 panels + `app:100x32` | **measured** — the surface names in the border rows |
| `Theme` rows | 215 | **measured** — `surface == "Theme"`, 39 themes, 209 distinct pairs |
| the 1 non-border app row | `kanagawa`, `#727169` on `#363646` | **read** — the row; `#363646` is `buffer.background_text_input`, `#727169` is `text.secondary` |
| per-theme ratios | as tabulated | **computed** — WCAG 2.1 relative luminance over the recorded/resolved hexes |
| `general.border` clears 3.0 | 12/50 | **computed** — over the 50 files |
| `buffer.border` clears 3.0 (alpha-stripped) | 4/50 | **computed**; 13 files author `#00000000` |
| `text.primary` clears 4.5 (originals) | 48/50 outstanding, `cyber-red` 4.50, `rose-pine-dawn` 2.45 | **computed** — from `git show HEAD:…` for all 50 |
| `text.tertiary` chroma > 40 | 37/50 | **computed** — max−min channel |
| buffer/general fill separation | 39/50 differ, median 1.11 | **computed** |
| `buffer.background_title_bar` authored | 50/50 | **computed** |
| derivation outcomes and Δ | as tabulated | **computed** — reimplementation of `derive_readable_variant` (24-step bisection, hue-preserving mix toward the better of black/white) at 60 steps for display precision; the crate's own helper must be the one that lands |
| upstream semantics | quoted | **read** — `https://halloy.chat/guides/custom-themes.html` (field list, "falls back to transparent") and the upstream `CHANGELOG.md` 2026.7 entry (`border_active` falls back to `buffer.border_selected`) |

**Not measured:** the render corpus was not run. No harness number is
re-derived here; the 1,448 is read from the file the harness wrote, and every
projection of what a change would leave behind is arithmetic from the recorded
pairs, not a re-measurement. The plan states the command that proves it.
