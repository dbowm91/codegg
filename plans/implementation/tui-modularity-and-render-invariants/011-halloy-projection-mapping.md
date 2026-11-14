# TUI Modularity and Render Invariants Milestone 011 — The Halloy projection owns legibility

Status: conditionally closed — implemented 2026-11-13; `[contrast]` went 1,448 → 0 with `assets/` byte-identical to HEAD. The one outstanding condition is the research's own falsifier: a border derived to exactly 3.0 may read as a wireframe, which needs a rendered look at `macaw-dark`, `intellij-light` and `catppuccin-latte`

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d` (worktree
uncommitted).

Source research:

- `plans/closure/tui-modularity-and-render-invariants/008-halloy-mapping-research.md`
  — §A (the audited mapping), §C (the 1,232-row split), §D (the options), §E
  (the revert audit), §F (the verdict).

Source decision:

- `plans/adrs/ADR-0062-bundled-palette-contrast-authority.md` — **Amendment
  (2026-11-13)**, which replaces the authority rule and withdraws clauses 3 and
  4. This plan implements that amendment; it does not implement the original
  clauses.

Source closure record:

- `plans/closure/tui-modularity-and-render-invariants/007-status.md` §3 and the
  2026-11-12 addendum (`[contrast]` = 1,448; 1,232 `border`, 215 `Theme`, 1
  `app:100x32`).

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §2 (I5) and
  §12 row 007, whose stop condition this plan closes.

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — a surface that
  cannot be read does not present the project state hierarchy.

Primary class: corrective (a defect in an existing projection, surfaced by a
measurement that already ships).

## 1. Objective

Make the Halloy import a faithful translation and clear the I5 `border` family,
**without editing a single bundled theme file**.

Two outcomes, in this order and in one change:

1. `assets/themes/halloy/*.toml` returns to its `HEAD` content, so the 50 files
   are the third-party gallery again — the 20 values this repository re-authored
   are reverted.
2. `src/theme/halloy.rs` owns the legibility of the five tokens it translates
   and fails: `ui.border` (1,232 rows), `text.muted` (354), `status.success` (8),
   `status.error` (6), `base.foreground` (47).

The bar does not move. `validate_theme` stays warn-only. No render site changes.
No token is added to or removed from the semantic schema.

## 2. Why this milestone is ready

The measurement is complete and the diagnosis is arithmetic rather than
aesthetic:

- The `border` population decomposes exactly into **22 themes × 56 surfaces**,
  so every per-theme decision converts to rows by ×56 (memo §C.1).
- **Only 3 of the 22 themes have a passing border value anywhere in their file**
  (`catppuccin-macchiato` 3.73, `catppuccin-mocha` 3.84, `one-dark` 3.29, all
  `general.border`), and switching to `general.border` unconditionally would
  break 19 currently-passing themes for a net **+896 rows** (memo §D, Option 3).
- The 20 re-authored values are all in slots the projection can derive, and 16 of
  the 20 are the derivation's own answer within a few units (memo §E.1).
- The mechanism already exists and is already accepted for two other tokens:
  `derive_readable_variant` (`src/theme/halloy.rs:601-641`) is what ADR-0062
  clause 2 uses for `status.warning` and a rejected `accent_secondary`.

Blocked on nothing. M001–M007 are closed or blocked only on this decision.

## 3. Current implementation evidence

### 3.1 The projection today

| Site | Line | What it does |
|---|---|---|
| `text.primary` → `base.foreground` (+ three conversation roles) | `halloy.rs:188-197` | straight copy, no gate |
| `text.tertiary` → `text.muted` | `halloy.rs:198-206` | straight copy, no gate |
| border chain → `ui.border` **and** `ui.border_focused` | `halloy.rs:207-222` | `buffer.border_selected` → `buffer.border` → `general.border` → `general.horizontal_rule`, all collapsed, no gate |
| `text.success` → `status.success` + `diff.added` | `halloy.rs:404-407` | straight copy, no gate |
| `text.warning` → `status.warning` + `diff.modified` | `halloy.rs:408-436` | **gated**: absent key → `derive_readable_variant` at 3.0 |
| `text.error` → `status.error` + `diff.removed` | `halloy.rs:437-440` | straight copy, no gate |
| `derive_readable_variant` | `halloy.rs:601-641` | private; hue-preserving bisection toward the better of black/white |

### 3.2 The bar each token is measured against

`MIN_DERIVED_TEXT_RATIO = 3.0` (`src/theme/schema.rs:79`) is what the I5 sweep
applies to **every painted cell** (`tests/tui_render/invariants/baseline.rs`).
`validate_theme` documents four bars (`src/theme/validate.rs:51-54`):
`PRIMARY_TEXT_RATIO` 4.5 for `base.foreground` on `background` **and** on
`input_background`, `MUTED_TEXT_RATIO` 3.0, `ACCENT_RATIO` 3.0,
`SELECTION_TEXT_RATIO` 3.0. `validate_theme` **never checks `ui.border`**
(`validate.rs:56-155`).

### 3.3 What is painted where

- Panel body fill: `theme.background` (`src/tui/panels/chrome.rs:116-134`) — the
  same fill as the app backdrop, so the stroke is the only separator.
- Focus: `theme.primary` for the edge and title, plus a `●`
  (`chrome.rs:9-20`, `61-67`). `theme.border` paints **unfocused** edges only.
- `alternate_bg` (Halloy's `buffer.background`) paints editor current-line and
  scrollbar tracks, not panels.
- `ui.title_background` and `ui.border_focused` are resolved and **never
  projected** (`src/theme/target.rs:52-76`).

### 3.4 The working-tree state this plan must undo

`git diff --stat -- assets/` — 14 files, 20 insertions, 20 deletions. The full
list with originals and derived replacements is memo §E.1.

## 4. Invariants that must not regress

| Invariant | Why it is at risk |
|---|---|
| **I5's bar** | the temptation is to derive to 3.1 or 4.0 "for margin". Derive to the documented bar, no higher. |
| **I5's scope** | nothing may be exempted. This plan changes values, never what is measured. |
| **I3b (focus is visible)** | `ui.border_focused` becomes `accent_primary`; the rendered focus channel is unchanged, so I3b's sweep must be unchanged too. |
| **`theme_selection_tokens.inc`** | the two derived selection foregrounds must not move. This plan touches neither. |
| **Native themes keep their authored values** | the gate applies only on the Halloy path (amendment, "unchanged"). A native theme that names `ui.border` keeps it. |
| **`validate_theme` stays warn-only** | a derivation adds a warning; it must not add an error or a load failure. |
| **The bundle stays loadable** | 50 themes parse before and after; `looks_like_halloy` is untouched. |
| **Render-cost shape** | no new widget, no extra pass. |

## 5. Scope

### In scope

- `src/theme/halloy.rs` — the five gates, the border source order, the
  `border_focused` value, and the shared derivation helper.
- `src/theme/schema.rs` — only if the helper is promoted here for reuse; no
  change to `MIN_DERIVED_TEXT_RATIO`, `derive_text_on` or
  `derive_selection_texts`.
- `src/theme/validate.rs` — **no threshold change**. One optional additional
  check (`ui.border` at `ACCENT_RATIO`) is *out of scope*; see §14.
- `assets/themes/halloy/*.toml` — **revert to `HEAD` only**. No other edit, ever.
- `docs/tui-render-invariant-baseline.toml` — re-bank `[contrast]`.
- `architecture/theme.md`, `docs/themes.md` — the mapping table and the
  authority rule.

### Explicitly out of scope

- **No theme value is authored, adjusted or normalised.** The only permitted
  change under `assets/` is `git checkout --` of the 14 files to `HEAD`.
- **No token split** (memo §D Option 2). `ui.border_focused` exists and is reused;
  no new `Theme` field, no render-site focus change.
- **No stroke exemption** and no change to the I5 measurement. The owner's
  2026-11-12 decision (strokes stay at the text bar) stands.
- **The 215 `Theme` picker swatches** (memo §D Option 5) and the single
  `app:100x32` cell. They are the residue this plan leaves behind and they get
  their own decision.
- **The `text.tertiary` slot question** (37 of 50 chromatic). Recorded in memo
  §E.2 with a named follow-up; not fixed here, because fixing it restyles 37
  themes and clears no row.
- **`ui.title_background` projection.** Halloy authors it in 50 of 50 themes and
  we drop it; using it is a render-design change, not a contrast fix.
- **Alpha.** `#00000000` still becomes `#000000`. The gate simply never depends on
  a transparent value.

## 6. Required production changes

### Core/domain, storage, migrations, protocol, DTOs, runtime, concurrency

None. This is a pure importer change: `SemanticTheme` in, `SemanticTheme` out.

### Frontend or operator surface

None. Every consumer keeps reading the same `Theme` field with a different
value. The only visible difference is the colour of unfocused panel and dialog
edges on 22 themes, plus tokens on the 20 reverted files.

One operator surface changes content: `/theme diagnostics` now reports a warning
per derived token per theme. On the bundled gallery that is roughly 20–40
warnings where it previously reported none for these tokens. The message must
name the key and both ratios, in the shape `halloy.rs:418-433` already uses.

### Security and authorization

None. No filesystem, process, network or authorization surface is added.

### Documentation and static guards

- `architecture/theme.md` — the authority rule (translated vs named), the bar
  per token, and the note that `ui.title_background`/`ui.border_focused` are
  semantic-only.
- `docs/themes.md` — fix the four stale rows in "Halloy field mapping"
  (memo §A.4), add the border precedence with its new gate, and regenerate the
  gallery section for the reverted values.
- `docs/tui-render-invariant-baseline.toml` — re-bank `[contrast]`.
- No guard is added or relaxed. `check_tui_render_invariant_baseline.py` must
  accept the smaller set; `check_no_hardcoded_selection_colours.py` must stay
  clean (no `Color::` literal enters a render path — this plan adds none).

## 7. Ordered work packages

The order is load-bearing. **WP A must land with WP B–D in one change**: the
revert alone raises `[contrast]` by up to 415 rows, and the ratchet refuses an
unbanked growth.

### Work package A — Revert the 20 values

```
git checkout -- assets/themes/halloy
```

Expected: 14 files, 20 values back to `HEAD`; `git diff --stat -- assets/`
empty. Nothing else changes in this step. Verify by re-reading the 20 rows of
memo §E.1 against `git show HEAD:assets/themes/halloy/<file>.toml`.

### Work package B — One derivation helper, one bar table

Promote `derive_readable_variant` (`halloy.rs:601-641`) to the single definition
used by every gate in the file, and add a small private helper:

```rust
/// A translated palette value that must read on `background`.
fn legible_on(
    value: Rgb,
    background: Rgb,
    min_ratio: f64,
    theme_id: &str,
    field: &str,
    authored_as: Option<&str>,
    diagnostics: &mut Vec<ThemeDiagnostic>,
) -> Rgb
```

It returns `value` unchanged when it already clears, and otherwise the derived
value plus a `warn` naming `field`, the authored hex, the measured ratio and the
one it reached. `derive_readable_variant` stays private; the helper is the only
caller.

Do **not** move this to `schema.rs` unless the native importer is also changed;
it is not (§5). If it does move, `MIN_DERIVED_TEXT_RATIO` is not the parameter —
each caller passes its own bar.

### Work package C — `ui.border` and `ui.border_focused`

1. Keep the source chain as it is, but skip a **transparent** candidate instead
   of alpha-stripping it into `#000000`: a value whose literal form is 8 digits
   with alpha `00` is "no paint" upstream (memo §A.3) and must never become a
   black frame.
2. If the resolved candidate clears `MIN_DERIVED_TEXT_RATIO` against
   `base.background`, ship it unchanged — **28 themes must be byte-identical**.
3. Else if `general.border` clears, ship that (3 themes:
   `catppuccin-macchiato`, `catppuccin-mocha`, `one-dark`).
4. Else ship `derive_readable_variant(candidate, background, MIN_DERIVED_TEXT_RATIO)`
   (19 themes).
5. Set `ui.border_focused = ui.accent_primary`, which is the value the renderer
   actually paints (`chrome.rs:61-67`). This changes no pixel and makes the token
   truthful for the next frontend.

Expected total: **1,232 rows cleared**, 28 themes unchanged, 3 taking their own
`general.border`, 19 derived (8 of them by ≤10 per channel).

The two existing tests that assert `border_focused == border`
(`halloy.rs:901-934`, `halloy.rs:878-882`) are updated to assert the new
contract; they are contract tests, not incidental assertions.

### Work package D — `text.muted`, `status.success`, `status.error`, `base.foreground`

Apply `legible_on` with each token's own bar:

| Token | Bar | Call site |
|---|---|---|
| `text.muted` ← `text.tertiary` | `MUTED_TEXT_RATIO` (3.0) | `halloy.rs:198-206` |
| `status.success` ← `text.success` | `ACCENT_RATIO` (3.0) | `halloy.rs:404-407` |
| `status.error` ← `text.error` | `ACCENT_RATIO` (3.0) | `halloy.rs:437-440` |
| `base.foreground` ← `text.primary` | `PRIMARY_TEXT_RATIO` (**4.5**) | `halloy.rs:188-197` |
| `status.warning` | `ACCENT_RATIO` (3.0) | unchanged behaviour, restated through `legible_on` so the absent-key arm and the failing-value arm are one rule |

`base.foreground` must be derived **before** the three conversation roles are
assigned, and all four assignments must use the derived value — otherwise the
chat body text and the chrome would disagree.

`diff.added`/`diff.modified`/`diff.removed` must follow `status.*` as they do
today.

### Work package E — Re-bank `[contrast]`

Run `cargo test --test tui_render -- invariants::baseline`. On disagreement the
failure message prints the complete measured manifest as TOML
(`baseline.rs:257-300`, `to_toml`); copy the `[contrast]` section and
`[totals] contrast` into `docs/tui-render-invariant-baseline.toml`, keeping the
file's prose and comments. Do not hand-write rows.

Expected: `[contrast]` drops from 1,448 to **216** — 215 `Theme` swatches plus
one `app:100x32` cell. If the number is not 216, the deltas identify which gate
did not fire; fix the gate, not the manifest.

### Work package F — Documentation and the closure record

`architecture/theme.md`, `docs/themes.md`, then
`plans/closure/tui-modularity-and-render-invariants/009-status.md` in the §1–§12
shape of `007-status.md`.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. The importer is a pure function over a `&str`; it has no I/O
beyond `parse_halloy_file`'s `read_to_string`, no state, no cancellation point
and no contention. A malformed value still produces a warning and keeps the
fallback, as today.

The only failure mode worth naming: `derive_readable_variant` must always
terminate and always clear the bar. It bisects 24 times over a monotone mix, so
it does; if a future edit changed the mix direction it would return `mix(1.0)` =
the better pole, which clears ≥4.58 by construction (`halloy.rs:606-609`).

## 9. Compatibility and migration

- **Bundled theme files** return to their `HEAD` content — a content change with
  no migration. Rollback is `git revert`.
- **User Halloy themes** keep loading and keep their hues; tokens that fail a bar
  now render at a derived lightness and report a warning. That is a visible
  change for a user whose theme ships `#1A1A1A` borders; it is the intended
  effect and must be reported, not applied silently.
- **Native `codegg` themes are untouched** — the gate is on the Halloy path only,
  so a native theme that writes `ui.border` directly keeps it, exactly as the
  amendment says.
- **No persisted id, no storage layout, no protocol frame, no scheduler surface**.
  Theme ids are unchanged; `git checkout` does not rename anything.
- **`theme_selection_tokens.inc` passes unmodified** — this is the contract that
  the two derived selection foregrounds did not move.

## 10. Required tests

Unit tests live in `src/theme/halloy.rs`'s test module and use
`bundled_themes()` (`halloy.rs:972-985`).

### Focused unit tests

1. `every_bundled_border_clears_the_bar_on_its_own_background` — for all 50,
   `theme.ui.border.contrast_ratio(theme.base.background) >= MIN_DERIVED_TEXT_RATIO`.
2. `the_themes_whose_border_already_read_are_byte_identical` — the 28-theme
   golden list of resolved values, so a future change to the chain cannot
   silently restyle the gallery.
3. `a_failing_border_is_reported_not_silently_derived` — a synthetic theme with
   `buffer.border_selected = "#101010"` on `#333333` yields a border at ≥3.0 and
   `warns_about(&diags, "ui.border")`.
4. `a_border_that_already_reads_is_untouched` — same value in, same value out, no
   warning.
5. `a_transparent_buffer_border_never_becomes_a_black_frame` — a theme that
   authors only `buffer.border = "#00000000"` and `general.border = "#9A9A9A"`
   resolves to `#9A9A9A`, never `#000000`.
6. `the_preferred_source_is_general_border_when_only_it_clears` — a theme with a
   failing `border_selected` and a passing `general.border` takes the latter
   (the `catppuccin-mocha` shape).
7. `every_bundled_muted_clears_the_muted_bar` — 50 themes, `MUTED_TEXT_RATIO`.
8. `every_bundled_status_token_clears_the_accent_bar` — `success`, `warning`,
   `error` for 50 themes.
9. `every_bundled_body_text_clears_the_primary_bar` — `base.foreground` at
   `PRIMARY_TEXT_RATIO` **and** against `input_background`, matching
   `validate_theme`'s two checks.
10. `the_derivation_preserves_hue` — for each failing token, the derived value's
    dominant-channel ordering equals the authored value's (the existing warning
    test's shape, `halloy.rs:1014-1018`).
11. `the_derivation_is_idempotent` — `derive(derive(x)) == derive(x)`.
12. `border_focused_is_the_accent_the_renderer_paints` — replaces the two
    `border_focused == border` assertions.

### Integration tests

- `cargo test --test tui_render -- invariants::baseline` — the re-banked manifest
  reproduces exactly, in both directions.
- `cargo test --test tui_render -- theme_selection_tokens` — passes **unmodified**.

### Restart and recovery tests

None. Nothing persists.

### Contention and cancellation tests

None.

### Security and negative tests

- A theme that authors *no* border key at all still resolves in the palette's
  family and never to `#000000` (test 5 covers the transparent case).
- A theme whose `background` is `#000000` and whose border is `#000000` derives
  to a lighter value rather than returning the background.

### Migration and compatibility tests

- `git diff --stat -- assets/` is **empty** after WP A — asserted in §11, not in
  a test (a test that shells out to git is not a property of the product).
- A native theme file with a low-contrast `ui.border` keeps its value: the gate
  is not on that path.

## 11. Required verification commands

```bash
CARGO_BUILD_JOBS=2 cargo test --lib theme::halloy
# expect: 0 failed. This is WP B–D's own module; no count is quoted because this
# plan was written without running it.

CARGO_BUILD_JOBS=2 cargo test --lib theme::validate
CARGO_BUILD_JOBS=2 cargo test --lib theme::registry
# expect: 0 failed each — 2 and 18 tests respectively today. `registry` proves
# the 50 themes still load and the accent gates still hold.
#
# There is deliberately NO `cargo test --lib theme::schema`: `src/theme/schema.rs`
# carries zero tests, and a filter that selects nothing exits 0. The
# derived-token contract is asserted by the next command instead.

CARGO_BUILD_JOBS=2 cargo test --test tui_render -- theme_selection_tokens
# expect: 0 failed, and the test file unmodified in the diff.

CARGO_BUILD_JOBS=2 cargo test --test tui_render -- invariants::baseline
# expect: 0 failed, with [contrast] = 216. This is the re-measurement; it fails
# and prints the whole manifest until WP E is done. (13 tests today.)

CARGO_BUILD_JOBS=2 cargo test --test tui_render -- the_i5_measurement_detects_an_unreadable_cell
# non-vacuity: the sweep must still detect a cell it is supposed to reject. A
# filter naming a test that does not exist exits 0, so this one is named exactly.
# expect: 1 passed; 0 failed.

python3 scripts/check_tui_render_invariant_baseline.py
# expect: exit 0, and the contrast count it prints is 216 rather than 1448.

python3 scripts/check_tui_render_invariant_baseline.py --self-test
# expect: every injected defect rejected, exit 0.

python3 scripts/check_no_hardcoded_selection_colours.py
# expect: clean. No literal enters a render path in this change.

git diff --stat -- assets/
# expect: EMPTY. This is the plan's first acceptance criterion and the owner's
# instruction in one command.

CARGO_BUILD_JOBS=2 scripts/verify.sh quick
# expect: green — rustfmt, the generated-asset checks, the guards, workspace check.

CARGO_BUILD_JOBS=2 scripts/verify.sh scoped
# `src/theme/` is in the root package, which every other crate depends on, so the
# selection widens to the root's full surface. Green before closure.
```

`scripts/verify.sh full` is required before the closure record is written,
because `src/theme` is consumed by `codegg-core`, `codegg-client` and the TUI —
the theme module is not a leaf. Record the observed counts; do not copy a
previous run's.

## 12. Documentation updates

- `architecture/theme.md` — a new subsection under the Halloy importer: "The
  projection owns legibility" with the translated-vs-named rule, the per-token
  bar table (§6 WP D), and the two semantic-only tokens
  (`ui.title_background`, `ui.border_focused`) marked as not projected.
- `docs/themes.md` — repair the four stale rows (memo §A.4), replace the border
  precedence description with the gated chain, and regenerate the gallery for
  the reverted values.
- `docs/tui-render-invariant-baseline.toml` — the re-banked `[contrast]` and its
  `[totals]`, plus one paragraph in the header recording that the `border`
  family closed by projection, not by value.
- `plans/adrs/ADR-0062-bundled-palette-contrast-authority.md` — already amended
  (2026-11-13); this plan does not amend it again.
- `plans/registry.md` and the roadmap — **the parent session applies these
  centrally**; this plan does not edit them.

## 13. Acceptance criteria

1. `git diff --stat -- assets/` is **empty**: the 50 bundled themes are
   byte-identical to `HEAD`.
2. `[contrast]` = **216**, and no `border` row remains.
3. All 50 bundled themes resolve `ui.border` at ≥ 3.0 against their own
   `base.background`, and the 28 themes that passed before resolve to the *same*
   value they did before.
4. `theme_selection_tokens.inc` passes **unmodified**.
5. `MIN_DERIVED_TEXT_RATIO`, `PRIMARY_TEXT_RATIO`, `MUTED_TEXT_RATIO`,
   `ACCENT_RATIO` and `SELECTION_TEXT_RATIO` are unchanged.
6. `validate_theme` emits no new `Error` and no diagnostic becomes load-failing.
7. Every derivation is reported as a warning naming the key.
8. A native theme's authored `ui.border` is unchanged by the importer.
9. No `Color::` literal enters a render path; `check_no_hardcoded_selection_colours.py` clean.
10. `scripts/verify.sh quick` green, `scripts/verify.sh scoped` green, and
    `scripts/verify.sh full` green with the counts recorded in the closure record.

## 14. Stop conditions

- **If a derivation cannot reach its bar** for any bundled theme, stop and
  report the theme rather than widening the derivation (a mix toward black or
  white always reaches ≥4.58, so this can only mean the background is
  miscalculated).
- **If `[contrast]` does not land on 216**, the difference is the finding. Do not
  bank a number you cannot explain; each unexplained row names a token and a
  theme.
- **If more than 28 themes change their resolved border**, the source chain moved
  further than WP C authorises. Stop: the goal is 22 themes, not 50.
- **If a bundled theme file must be edited to make the sweep pass**, the
  amendment has been misread. Stop and escalate; the owner's instruction forbids
  it.
- **If `theme_selection_tokens.inc` must change**, a derived token moved. That is
  a schema change and needs a different ADR.

Deliberately **not** in this plan, and each needs its own decision: adding
`ui.border` to `validate_theme` (it should probably be there, but adding a check
changes what every user theme reports); the `Theme` picker's 215 swatches; the
`text.tertiary` slot question; projecting `ui.title_background`; an alpha
concept.

## 15. Closure evidence required

`plans/closure/tui-modularity-and-render-invariants/009-status.md`, §1–§12 in the
shape of `007-status.md`, must carry:

- the four guard and test transcripts from §11, verbatim, with their counts;
- the `[contrast]` before/after (1,448 → 216) and the `border` family's absence;
- `git diff --stat -- assets/` printed, showing nothing;
- the 22 themes' before/after border values, marked 28-unchanged / 3-general /
  19-derived;
- the list of warnings `/theme diagnostics` now emits for the bundled gallery;
- a statement that no threshold and no measurement rule moved.

## 16. Handoff notes

- **The revert and the projection rule are one change.** Landing the revert
  alone grows `[contrast]` by up to 415 rows and the ratchet will reject it;
  landing the projection alone leaves 1,232 rows and no visible change. Say so in
  the commit message.
- **`ui.border_focused = accent_primary`** is not a contrast fix and must not be
  justified as one. It exists because the token currently claims a colour nothing
  paints, and the next frontend to read `SemanticTheme` will believe it.
- **The derivation's target is the bar, not the middle.** If a reviewer asks for
  "more margin", the answer is that 3.0 is the published non-text minimum and
  every point above it is a restyle nobody asked for.
- **The 28-theme golden test is the guard against scope creep.** If it fails,
  read the failing names before touching the chain: it is telling you the
  gallery's appearance moved where it did not need to.
- **The next decision this unblocks is the picker's 215.** Memo §D Option 5 has
  the shape (paint the chip as a fill, not a glyph) and it is a five-line render
  change with no palette question. It is not this milestone's.
