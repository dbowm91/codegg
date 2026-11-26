# TUI Modularity and Render Invariants Milestone 007 — Closure Record

Status: conditionally closed

Source plan: `plans/implementation/tui-modularity-and-render-invariants/007-contrast-ratchet-to-zero.md`
Roadmap: `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-007--contrast-ratchet-to-zero`
Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d` (worktree uncommitted)
Record written: 2026-11-12

**Status rationale.** This record said `blocked` because the plan's own §14 stop
condition fired: **76.1% of the recorded baseline was a derived-token failure,
not a render-path failure**, and the plan says that class "changes every theme
and needs an owner decision; enumerate the affected pairs and stop." That stop
was correct when it was taken, and it is now **resolved**: `[contrast] = 0`
across the 50 bundled themes, reached by the **M011 Halloy mapping** rather than
by editing the third-party themes. `assets/` is byte-identical to `HEAD`
(measured: `git status --porcelain -- assets/` returns nothing), and
`plans/implementation/tui-modularity-and-render-invariants/011-halloy-projection-mapping.md`
owns the fix — five projection gates (`base.foreground` at 4.5; `text.muted`,
`status.success`, `status.error`, `status.warning` and `ui.border` at 3.0), each
deriving a hue-preserving value that stops at the bar and warns. What the record
previously called blocked — an owner decision on the token layer — has been
taken: ADR-0062 and its `## Amendment (2026-11-13)` rule that a colour read
through the Halloy importer is a **translation**, so the projection owns its
legibility. The remaining condition is a commit, not a decision.

## 1. Executive finding

The baseline is **2,495 entries**, and it is not one kind of thing.

| Class | Entries | Share |
|---|---|---|
| (a) wrong token for the fill — fixable at a render site | 110 | 4.4% |
| (b) foreground that is not a token of the drawing theme | 393 | 15.8% |
| (c) the theme's own token, on the fill it exists for, below the bar | **1,898** | **76.1%** |
| (d) unmeasurable cells | 0 | — |
| (H) harness artefact — a frame no theme paints | 94 | 3.8% |
| unclassified | 0 | — |

Class (c) is the escalation. For every one of those pairs the *same surface
passes in the other 43–49 themes*, so the cause is the palette and not the
site: `muted` fails on 6 of 50 themes across 59 surfaces, `secondary` on 7 of 50
across 34, `warning` on 7 of 50 across 5. A render path that chose the wrong
token would fail everywhere, not in six palettes.

Five token families are below the thresholds **this repository already
documents** in `src/theme/validate.rs`, on themes that ship:

- `foreground` — rose-pine-dawn at **2.45** against a **4.5** contract,
- `muted` — 6 themes at 2.33–2.81 against `MUTED_TEXT_RATIO = 3.0`,
- `warning` — 7 themes, as low as **1.34**, against `ACCENT_RATIO = 3.0`,
- `success` — 6 themes at 1.67–2.96, `error` — 5 themes at 2.69–2.85,
- `border` — 22 themes at 1.31–2.99 (1,210 entries; see §10 for why its
  membership in this invariant is itself an open decision).

`validate_theme` only *warns*, so nothing shipped red. Fixing these means
editing `assets/themes/halloy/*.toml` — explicitly out of scope per §6 — or
changing the token layer, which needs an ADR.

Two measurement defects mean part of the work list describes a frame nobody can
paint, and they are prerequisites rather than fixes (§10, findings 1–2).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP-A: classify every baseline entry by cause; totals equal the baseline size | the partition in §1 sums to 2,495 with 0 unclassified | **pass** | Classification is mechanical for (b)*(c)*: the recorded `fg`/`bg` hexes are mapped to each labelled theme's resolved tokens, obtained by a probe against the built `libcodegg` rlib, so the values are the registry's, not a re-implementation of the Halloy parser. |
| WP-A: class (c) enumerated separately | §3 | **pass** | 1,898 entries, seven token families, per-theme ratios given. |
| WP-B: fix class (a) | not attempted | **not done** | 7 sites identified with file:line; 42 of the 110 already have a derived answer (`selection_text`). |
| WP-C: fix class (b); literal count outside `theme.rs` falls | not attempted | **not done** | All 393 are one surface (`Theme`) and **no entry traces to a `Color::Rgb` literal** — the plan's WP-C acceptance cannot move them, so that criterion needs restating. |
| WP-D: per-theme skip count reported | `Measured::unmeasurable_cells` is one global `usize` (`baseline.rs:131-138`), asserted `== 0` once | **fail** | The skipped fraction is 0/50 globally and **unmeasured per theme**. A theme that was mostly unmeasurable would pass today. |
| WP-E: `[contrast]` empty | untouched, 2,495 entries | **not done** | Reachable only after decisions 1–2 in §10. |
| AC 2: measurement proved non-vacuous against a broken theme | existing harness test | **pass (pre-existing)** | Not re-derived here. |
| AC 4: `theme_selection_tokens.inc` passes unmodified | not re-run for this milestone | **no evidence** | No token was changed, so no risk taken. |
| AC 5: no file under `src/theme/` changed | `git status` | **pass** | Nothing in `src/` was edited by this milestone. |
| AC 7: I1, I2, I3b, I4, I6 stay green | render corpus 1227 passed / 0 failed on this tree; `scripts/verify.sh quick` passed | **pass at the time of writing** | Recorded by the parent session, not by this milestone. |

## 3. Class (c), the escalation list

First, the derived tokens are clean: across all 50 themes `selection_text` on
`selection` never falls below **3.36** and `accent_text` on `primary` never
below **3.07**, both against a 3.0 bar. The plan's literal reading of (c) — "a
derived token itself below the bar" — is **empty**. Every (c) entry is a *plain*
palette token painted on the surface it is designed for.

| Token | Entries | Surfaces | Themes | Ratio range |
|---|---|---|---|---|
| `border` | 1,210 | 57 | 22 | 1.31 – 2.99 |
| `muted` | 354 | 59 | 6 | 2.33 – 2.81 |
| `secondary` | 238 | 34 | 7 | 1.95 – 2.58 |
| `foreground` | 47 | 47 | 1 | 2.45 |
| `warning` | 35 | 5 | 7 | 1.34 – 1.77 |
| `success` | 8 | 2 | 6 | 1.67 – 2.96 |
| `error` | 6 | 2 | 5 | 2.69 – 2.85 |

Themes per family, so the decision is concrete:

- `border` (22): discord, discord-80-saturation, intellij-light,
  catppuccin-latte, macaw-dark, ferra-light, dark-green,
  catppuccin-macchiato, macaw-light, catppuccin-mocha, berlin, flexor-dark,
  rose-pine-dawn, oxocarbon, one-dark, rose-pine-moon, nostromo-terminal, cork,
  bam, broc, acton, ferra.
- `muted` (6): macaw-dark, cyber-red, base16-atelier-forest-light, vik,
  discord, discord-80-saturation.
- `secondary` (7): catppuccin-latte, noctis-lilac, ferra-light, rose-pine-dawn,
  base16-atelier-forest-light, macaw-light, intellij-light.
- `foreground` (1): rose-pine-dawn (`#D7827E` on `#F4EDE8`).
- `warning` (7): catppuccin-latte, noctis-lilac, ferra-light, rose-pine-dawn,
  base16-atelier-forest-light, macaw-light, intellij-light.
- `success` (6): intellij-light, noctis-lilac, catppuccin-latte, macaw-light,
  zenburn, base16-atelier-forest-light, rose-pine-dawn.
- `error` (5): gruvbox, noctis-lilac, discord, discord-80-saturation, berlin.

Also latent, with no recorded cell: `link` on background is 1.77 in
noctis-lilac against a 3.0 contract.

## 4. Verification executed

| Command | Result |
|---|---|
| `cargo test --test tui_render -- invariants::baseline` | `6 passed; 0 failed … 1221 filtered out` — the recorded baseline **is** what today's harness measures |
| theme-token probe against the built `libcodegg` rlib | 50 themes, 16 colour tokens each, resolved from `ThemeRegistry::load_builtins().all_tui_themes()` |
| baseline parse and partition | 2,495 entries; totals per class as in §1, 0 unclassified |

Not run: `scripts/verify.sh full`; the class (a)/(b) fixes (not attempted);
`theme_selection_tokens.inc` (no token changed).

## 5. Invariant review

- **No token was changed to make a cell pass.** The stop is the mechanism that
  keeps this true.
- **No surface lost text**: nothing was edited.
- **I1, I2, I3b, I4, I6**: untouched by this milestone; corpus green on this
  tree.
- **Render-cost shape**: untouched.
- **Comment ratchet**: untouched by this milestone; banked by the parent at
  138,557 lines / 1,234,205 words / ratio 0.1360.

## 6. Failure and recovery review

Not applicable — no runtime behaviour changed. The measurement is a synchronous
buffer walk.

## 7. Migration and compatibility review

None. No schema, no storage, no protocol, no theme file.

## 8. Security review

None.

## 9. Documentation and operations

`architecture/theme.md` and `architecture/testing.md` were **not** updated,
because the milestone did not land a policy: writing "a render path chooses
*which* token, never *what colour*" while five palettes ship below their own
documented thresholds would document the opposite of the truth. The
documentation belongs with the decision.

## 10. Unresolved findings

| # | Severity | Finding | Impact | Required action |
|---|---|---|---|---|
| 1 | high | **`app:100x32` (94 entries) measures a frame no theme paints.** `capture::app_frame` takes no theme, and `App::new_for_testing` pins legacy `Theme::dark()`; `Reset` cells are then resolved against the *loop* theme. Measured: **0 of 94** pairs come from one palette. | 94 entries are unsatisfiable by construction, and a `Reset` cell is judged against a palette it was not painted with. | Pass the theme through `measure_contrast` into `capture::app_frame`. Re-banks `[contrast]` wholesale. |
| 2 | medium | **The `Theme` dialog fixture measures a list production does not use.** `fixtures.rs:158` calls `ThemePickerDialog::new`, whose fallback is the 31 legacy static palettes; production passes `theme_registry.all_tui_themes()`. | 393 entries (all of class (b)) measure a list that does not exist in the product. | Build the fixture with the registry list; then re-classify (b), which may be much smaller. |
| 3 | medium | **Per-theme skip counts do not exist.** One global `usize`, asserted once. | WP-D's requirement — "a theme which is mostly unmeasurable is a finding rather than a pass" — has no mechanism. | Make it a `BTreeMap<String, usize>`, report it, assert the fraction per theme. |
| 4 | medium | **The suite holds strokes to a text bar.** `Frame::painted` is `!cell.is_blank()`, so `border` (1,210 entries, 22 themes) is measured as text. | 48% of the baseline may be a scope error rather than a defect; the answer decides whether this milestone is 1,898 or 688 entries of class (c). | Owner decision, §10 decision 2. |
| 5 | medium | **M002's collision finding is larger than recorded.** `primary == border` in 4 themes (as recorded), `primary == muted` in **7**, `border == muted` in **3**. | Every such collision erases the focus/title channel I3b relies on, and I3b still sweeps `Theme::dark()` only (`focus.rs:48`). | Decide re-derivation vs a second focus signal, and extend the I3b sweep to all 50 themes. |
| 6 | low | **WP-C's acceptance criterion cannot be met as written**: no class (b) entry traces to a `Color::Rgb` literal, so "the literal count outside `theme.rs` fell" does not move them. Conversely the 26 literals in `components/tool_output.rs` produce zero recorded pairs. | A criterion that cannot move the class it targets. | Restate WP-C for what (b) actually is: a swatch painted in one theme's palette on another theme's fill. |

**Update, 2026-11-13 (final) — the `[contrast]` baseline is EMPTY.**

`contrast = 0`. The measurement reports no painted cell below its bar on any of
the 50 bundled themes, and the guard says so: *"4 advertised-key, 0 contrast and
0 hint-truncation entries recorded; every one reproduces and none was added
unbanked."* Acceptance criterion 1 is met, and the milestone's central claim —
"every surface is readable under every bundled theme" — is now a measurement
rather than an aspiration.

The last two families closed after the mapping round:

- **215 `Theme` picker swatches → 0.** `render_color_swatch` painted the sample
  as *the hex string in that colour* on the active theme's background, so a
  light sample on a light chrome was unreadable. It now paints the sample as a
  **filled chip** — the sample is the fill, the hex is a foreground derived from
  it via `derive_text_on` — so every swatch clears by construction. A
  non-vacuity test paints the old pair and requires it to fail (1.10:1 and
  1.00:1 on the two polarities).
- **1 `app:100x32` cell → 0.** `kanagawa`'s authored `accent_secondary` read
  3.5:1 on the window background and **2.42:1 on the composer's input fill**
  (`#363646`), which the gate never consulted. `ui.accent_secondary` is now
  gated against **both** fills — and the input fill joins the gate only when the
  theme *declares* one, because an unauthored `buffer.background_text_input`
  leaves a dark placeholder that would reject correct values in a light theme.

**One acceptance criterion is superseded rather than met.** §13 criterion 5
reads "No file under `src/theme/` changed", and that is now **literally false**:
`src/theme/halloy.rs` is where the fix lives. The criterion existed to enforce
§4's invariant — *"no token is changed to make a cell pass"* — and it enforced it
by forbidding the only place a token could legitimately be fixed. ADR-0062's
`## Amendment (2026-11-13)` replaces the test: a colour read through the Halloy
importer is a **translation**, so the projection owns its legibility, and a
failing translated value is repaired by derivation (hue kept, lightness moved to
the bar, a warning emitted) rather than by editing a third-party theme file. The
invariant §4 protects is intact and stronger: no bundled theme value was changed
(`assets/` is byte-identical to `HEAD`), and every derivation is a computed
answer that re-derives if the bar moves.

What remains of the milestone is not contrast: it is the record's own honesty
about how the numbers were reached, and the two named items in §10 that this
round did not touch (the per-theme skip reporting landed; the token-layer
decision is answered by ADR-0062's amendment).

**Update, 2026-11-13 — the token layer was the wrong shape, and it is now the
right one.** The owner ruled that the bundled Halloy themes are third-party and
must stay unedited, which turned the remaining contrast work into a **mapping**
question. The research (`008-halloy-mapping-research.md`) found the defect:
`halloy.rs` preferred `buffer.border_selected` — a **focus** colour — and painted
it on 55 *unfocused* surfaces per theme, because our panel body shares the app
backdrop's fill and the stroke became the only separator. Halloy separates panes
by **fill**, and its own chrome edge (`general.border`) clears 3:1 in only 12 of
50 themes — it never asks a stroke to carry separation at all.

Implemented (`011-halloy-projection-mapping.md`): **all 20 hand-edited theme
values reverted** (`assets/` is byte-identical to `HEAD`), and five projection
gates now own legibility — `base.foreground` at 4.5, `text.muted` /
`status.success` / `status.error` / `status.warning` / `ui.border` at 3.0 — each
deriving a hue-preserving value that stops **at** the bar and warns. `general.border`
is preferred where the theme's own frame already clears (3 themes), which keeps
**28 of 50 themes byte-identical**; `border_focused` is now `accent_primary`,
which is what is actually painted.

**`[contrast]` is 216**, down from 2,495 at the triage: `border` is **0** (not
recoloured — the family is absent, cross-checked against all 50 resolved pairs),
`Theme` picker swatches are 215, and one `app:100x32` cell remains.

**Update, 2026-11-12 (later) — everything decidable has been implemented.**
The milestone's remaining mechanical work landed; the numbers below supersede
the triage figures above.

- **`[contrast]` is now 1,448**, down from 2,495. Composition: **1,232
  `border`** (1,210 recorded + 22 that an honestly-themed app frame revealed),
  **215 `Theme`** picker swatches, and one `app:100x32` row.
- **ADR-0062 implemented** (−688): the projection now derives `status.warning`
  and `ui.accent_secondary` instead of inheriting values no theme authors
  (`0 of 50` author `[text].warning`; 41 of 50 secondary values were rejected),
  and 20 values across 14 bundled themes were re-authored. `foreground` reached
  **zero** — rose-pine-dawn's body text went from 2.45 to 6.28 against a 4.5
  contract.
- **Class (a) cleared** (−92): `policy_selector` (50 rows, `background` on the
  window background at **1.00:1**), and `keybind`/`question`/`theme` (14 each)
  now use `selection_text`. The three mode badges needed no change — ADR-0062's
  `muted` fix had already cleared them.
- **Finding 1 fixed** (−72 net): the app frame is now painted with the theme
  under measurement instead of the legacy palette `new_for_testing` pins. Before
  the fix, **0 of 94** recorded `app:100x32` pairs had both colours from the
  labelled theme. It adds 22 real `border` rows and removes 94 cross-palette
  artefacts; the honest frame is the one that shows the failures.
- **Finding 2 fixed** (−195): the `Theme` fixture builds the 50 registry themes
  production passes, not the 31-palette legacy list.
- **WP-D landed**: per-theme cell counts, a 5% unmeasurable-fraction assertion,
  and a skip report. Measured: **1,443,300 painted cells, 0 unmeasurable**.

**What still blocks an empty baseline** is now only three items, each a
decision rather than work: `border`'s 1,232 (needs the 22 themes' border value
re-authored, or acceptance), the `Theme` picker's 215 (exempt swatch glyphs or
restyle), and whether the app frame's own 1 row is reachable.

**Update, 2026-11-12 — three of the five decisions now have records.**

- **Decision 1 (token layer)** is drafted: `plans/adrs/ADR-0062-bundled-palette-contrast-authority.md`,
  `Status: proposed`. Its proposal is that authority over a plain palette token
  is decided by *whether the theme file names the key* — inherited values
  (`warning`, `secondary`, 273 entries) are a **projection** defect to repair in
  `halloy.rs`; authored values (`muted`, `success`, `error` 368; `foreground` 47)
  are re-authored in the bundled theme files; `border`'s 1,210 go to decision 2;
  and the text bar itself does not move. It clears 688 of class (c). Measured
  supporting facts: **0 of 50 bundled themes author `[text].warning`** (so every
  `warning` failure is inherited, not authored) and **41 of 50 secondary values
  are rejected** by the slot mapping.
- **Decision 2 (is a stroke text?)** is answered by
  `plans/closure/tui-modularity-and-render-invariants/007-border-as-text-research.md`. The archaeology settles the
  owner's own condition: `theme.border` has painted unfocused panel and dialog
  edges **since the repository's first commit `d820dfb4c` (2026-09-25)**, no test,
  guard, validator or document ever held a stroke to a contrast bar
  (`validate_theme` checks eleven pairs and never `ui.border`), and the M002/M003
  chrome refactor **preserved the token exactly** while moving dialog outer
  frames *off* `border` onto the contrast-gated `primary` — removing **528** rows
  and adding none. Its recommendation is **D′**: exempt cells that are
  `border`-painted *and* carry a stroke glyph, which removes 1,188 rows (47.6%)
  and invents no threshold below the published WCAG non-text minimum.
- **Decision 3 (is the muted tier allowed?)** is answered affirmatively by
  ADR-0062 clause 3: `muted`'s failures are authored values, so the tier stays
  and its values are re-authored.
- **Decision 4 (the focus collisions)** remains open — the memo confirms
  `border` is currently purely neutral chrome (state-bearing edges use `warning`
  and `error`), which is what would overturn D′ if it ever changed.
- **Decision 5 (harness corrections)** is unchanged and still a prerequisite;
  findings 1–2 below re-bank `[contrast]` wholesale.

**Attribution caveat on §3's per-theme lists.** An independent re-derivation
reproduced every family total but over-counts by hex collision where two tokens
share a value, so §3's `success` list has seven names against a count of six and
`error`'s five omit `macaw-light`. The counts come from the mechanical token
match; the lists were transcribed from it. ADR-0062 carries the attribution note
that explains the discrepancy rather than silently restating either figure.

## 11. Roadmap disposition

**Conditionally closed.** `[contrast]` is **empty** — `contrast = 0` in
`docs/tui-render-invariant-baseline.toml`, and the guard reads
"4 advertised-key, 0 contrast and 0 hint-truncation entries recorded; every one
reproduces and none was added unbanked". Acceptance criterion 1 is met, and the
milestone's central claim — every surface readable under every bundled theme —
is a measurement rather than an aspiration. The bundled theme files are
byte-identical to `HEAD`, so the fix is the projection, not a third-party edit.

The stop condition's five decisions are answered or superseded rather than
dropped:

1. **Token layer — answered** by ADR-0062 and its `## Amendment (2026-11-13)`: a
   failing translated value is repaired by derivation (hue kept, lightness moved
   to the bar, a warning emitted), not by re-authoring the theme.
2. **Is a stroke text? — answered by the projection**: `ui.border` is gated at
   3.0 like the text tokens, so `border` no longer fails and does not need
   exempting; `border_focused` is `accent_primary`, which is what is painted.
3. **Is the muted tier allowed? — answered affirmatively**: `text.muted` is a
   projection gate at 3.0, so the de-emphasis tier stays and is legible.
4. **The focus collisions — carried, not re-checked here.** The mapping changed
   the pair the focus policy reads (focused edge is `accent_primary`, unfocused
   is the gated `ui.border`), which is the token-layer answer the collision
   needed; whether `Cyberpunk`, `Nord`, `Solarized Dark` and `VESPER` still
   resolve the two to one colour is a rendered check this record does not claim.
   M002's §10 carries the sweep extension.
5. **The harness corrections — landed**: the app frame is painted with the theme
   under measurement, the `Theme` fixture builds the 50 registry themes
   production passes, and WP-D's per-theme skip reporting landed (1,443,300
   painted cells, 0 unmeasurable).

What remains is not contrast: it is a **commit** carrying this tree, and the
record's own honesty about how the numbers were reached — which the update
blocks in §10 supply, including that §13 criterion 5 ("no file under
`src/theme/` changed") is **superseded** rather than met, because `halloy.rs` is
where a projection fix legitimately lives.

## 12. Registry updates

- `plans/registry.md` — TUI M007 row: `blocked` → **`conditionally closed`**;
  closure record `plans/closure/tui-modularity-and-render-invariants/007-status.md`;
  the blocker is now the commit alone, with `[contrast] = 0` and the M011
  mapping as the evidence.
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §12 row
  007: status `conditionally closed — [contrast] empty`, which is what the
  roadmap already records, with the mapping named as the owner of the fix.
- `plans/implementation/tui-modularity-and-render-invariants/007-contrast-ratchet-to-zero.md`
  header: still `Status: blocked`; it should read `conditionally closed` at the
  next sweep. This record does not edit the source plan.
- TUI subsystem row in `plans/registry.md`: M007 is no longer blocked on a token
  decision; it is conditional on the commit alone.

## Correction log (2026-11-13)

- **`Status:` header.** Was `blocked`. Flipped to **`conditionally closed`**. Evidence: `[contrast] = 0` in `docs/tui-render-invariant-baseline.toml` (`[[contrast]]` count 0; `[totals] contrast = 0`, `advertised_keys = 4`, `hint_truncation = 0`), reached by the **M011 Halloy mapping** — `plans/implementation/tui-modularity-and-render-invariants/011-halloy-projection-mapping.md` — with the bundled theme files byte-identical to `HEAD` (`git status --porcelain -- assets/` is empty). The roadmap row and the plan already state the conditional form.
- **The "Status rationale" paragraph.** Said the milestone "is `blocked`, not `conditionally closed`" because 76.1% of the baseline was a derived-token failure needing an owner decision. The decision was taken: ADR-0062's `## Amendment (2026-11-13)` makes a Halloy-imported colour a translation owned by the projection, and the five projection gates derive legible values. The paragraph now says what the record previously called blocked and what resolved it.
- **§11, the disposition.** Was "**Blocked — stop condition reached, owner decision required.** Do not open WP-B/C/D/E…" with a five-item decision list. Rewritten as `conditionally closed`: decisions 1, 2, 3 and 5 are answered or landed, decision 4 (the focus collisions) is carried to M002's §10 rather than claimed resolved, and the remaining condition is the commit.
- **§12, the registry-update recommendations.** They previously instructed the registry to record `blocked (stop condition)`; they now instruct it to record `conditionally closed` with the mapping as evidence, and note that the source plan's own header still reads `blocked` and is for a later sweep.

**No promotion to `closed`.** The commit is outstanding, and decision 4's rendered confirmation is not claimed. The record therefore stays `conditionally closed`, which is the status the plan's §3 evidence classification and the roadmap both expect.
