# ADR-0062: Bundled Palette Contrast Authority

Status: accepted — 2026-11-12 by the project maintainer. The accepted
decision is this ADR's own authority rule: a failing plain palette token is
the token layer's problem when the theme file **inherits** the value, and the
bundled file's problem when the theme **authors** it. The contrast bar itself
does not move. Clause 5 (`border` is excluded to the stroke decision) is
resolved in the companion memo: the owner kept border strokes at the text bar,
so `border` is *measured* as text and its 1,210 recorded rows are ordinary token
failures, not an exemption.

Date: 2026-11-12

> **Owner instruction, 2026-11-13 — the bundled themes are third-party and stay
> unedited.** Verbatim: *"I'd prefer to keep the theme files themselves as they
> are, since they are technically third party themes, so research how to best
> handle this."* The owner also observed that these themes "are pretty good
> about this" when used in Halloy itself, which points at **our adaptation** of
> them rather than at their values.
>
> This narrows clause 2 of the decision below. "Authored values are re-authored
> in the bundled files" is **withdrawn as a default**: a failing authored value
> is now evidence that our mapping borrows a Halloy field for a purpose it does
> not have there, and the fix belongs in `src/theme/halloy.rs` (or in a token
> split, which needs its own decision). The 20 values this ADR's implementation
> changed across 14 bundled files are **pending review for revert** under that
> reading; the research is
> `plans/closure/tui-modularity-and-render-invariants/008-halloy-mapping-research.md`.
>
> Until that research lands, **no further edit to `assets/themes/halloy/*.toml`
> is authorised.** A contrast failure in a bundled theme is a mapping finding,
> not a value to change.


Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the TUI
  presents the project state hierarchy; a surface that cannot be read does not
  present it.
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#2-work-classification`
  — invariant I5, its bar, and the ratchet that records its failures.
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#7-milestones`
  — M007, the contrast ratchet to zero, whose stop condition this ADR answers.

Affected subsystem roadmaps:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md`

Related ADRs: none. This is the first decision that assigns authority over the
*value* of a bundled palette token. It does not amend ADR-0053 (projection
surfaces) or ADR-0052 (command execution ownership); it touches neither.

Related records:

- `plans/closure/tui-modularity-and-render-invariants/007-status.md` — the
  stop-condition report whose §3 and §10 this ADR answers.
- `plans/closure/tui-modularity-and-render-invariants/007-border-as-text-research.md`
  — the stroke research that supplies decision 2 (`border`).
- `plans/implementation/tui-modularity-and-render-invariants/007-contrast-ratchet-to-zero.md`
  §4, §5, §13 and §14.

## Context

M007 was stopped by its own §14 stop condition: of the 2,495 recorded I5
entries, **1,898 are class (c)** — the theme's own token, painted on the fill
that token exists for, below the bar the harness applies. The remainder is
render-path work (class (a), 110), a fixture defect (class (b), 393) and a
harness artefact (class (H), 94).

The derived tokens are **not** the problem. Across all 50 bundled themes
`selection_text` on `selection` never falls below 3.36 and `accent_text` on
`accent_primary` never below 3.07, both against a 3.0 bar
(`007-status.md` §3). Every class-(c) entry is a **plain** palette token.

The decision is therefore narrow and unavoidable: **what is the authority for
the value of a plain palette token that ships below a threshold this repository
documents?** There is no version of this in which the question goes away,
because the measurement is recorded and the ratchet refuses both growth and an
unbanked reduction.

### The bar the harness applies

Two different bars exist in the tree today, and they are not the same contract:

| Bar | Value | Where | What it governs |
|---|---|---|---|
| `PRIMARY_TEXT_RATIO` | 4.5 | `src/theme/validate.rs:51` | `base.foreground` on `background` and on `input_background` |
| `MUTED_TEXT_RATIO` | 3.0 | `src/theme/validate.rs:52` | `text.muted` on `background` |
| `ACCENT_RATIO` | 3.0 | `src/theme/validate.rs:53` | `accent_primary`, `status.error/warning/success`, `text.link` |
| `SELECTION_TEXT_RATIO` | 3.0 | `src/theme/validate.rs:54` | the two derived selection foregrounds |
| `MIN_DERIVED_TEXT_RATIO` | 3.0 | `src/theme/schema.rs:79` | **every painted cell** in the I5 sweep (`tests/tui_render/invariants/baseline.rs:426`) |
| `MIN_ACCENT_CONTRAST` | 3.0 | `src/theme/halloy.rs:30` | a *candidate* accent, which the importer rejects rather than ships |

The harness reuses `MIN_DERIVED_TEXT_RATIO`, which the roadmap's I5 definition
requires ("no new threshold is introduced"). The consequence is worth stating
plainly, because it constrains every option below: **the render invariant
already asserts less for body text than `validate_theme` documents** (3.0, not
4.5). A proposal to "lower the bar for non-body tokens" is therefore a proposal
to go *below* the large-text and non-text minimum, not to align two equal bars.

### What the failing values actually are

The families were re-derived independently for this ADR from
`assets/themes/halloy/*.toml` and the recorded pairs in
`docs/tui-render-invariant-baseline.toml`. The result changes the diagnosis for
two of the seven families:

| Family | Token | Entries | Themes | Ratio range | Where the failing value comes from |
|---|---|---|---|---|---|
| `border` | `ui.border` | 1,210 | 22 | 1.31 – 2.99 | **authored**; every theme supplies one; used with no contrast gate |
| `muted` | `text.muted` ← `text.tertiary` | 354 | 6 | 2.33 – 2.81 | **authored** |
| `secondary` | `ui.accent_secondary` | 238 | 7 | 1.95 – 2.58 | **not authored by the failing themes** — 41 of 50 authored values are below 3.0 and are *discarded* by `halloy.rs:342-363`; the 7 light themes then inherit `placeholder_fallback`'s `#B48CFF` |
| `foreground` | `base.foreground` ← `text.primary` | 47 | 1 | 2.45 | **authored**, and the slot holds an accent, not body text |
| `warning` | `status.warning` | 35 | 7 | 1.34 – 1.77 | **not authored at all** — `[text].warning` exists in **0 of 50** theme files; the 7 light themes inherit `#FFB43C` from `placeholder_fallback` (`src/theme/registry.rs:733`) |
| `success` | `status.success` | 8 | 7 | 1.67 – 2.96 | **authored** |
| `error` | `status.error` | 6 | 5 | 2.69 – 2.85 | **authored** |

Two notes on attribution, because hex matching alone cannot separate a token
from a collision. `007-status.md` §3 prints "6 themes" for `success` while
listing seven names; the entry count (8) and the seven names are right and the
column is a typo. `macaw-light` sets `success` and `error` to the same
`#959595`, so its single recorded row is attributable to either; it is counted
once, under `success`, which is why `error` is 6 entries over 5 themes. The
entry totals, not the theme columns, are what the 1,898 partition depends on.

Three facts follow, and none of them is a matter of taste:

1. **273 entries (35 `warning` + 238 `secondary`) are an inheritance defect,
   not a palette opinion.** The failing colour is not in the theme's file. It is
   the placeholder fallback's colour, borrowed by a theme that never chose it,
   and it fails only on themes whose background is light enough to disagree with
   it. Exactly 7 of the 50 themes have a light background, and they are exactly
   the 7 themes in both families — the two families are one mechanism observed
   twice.
2. **368 entries (`muted` 354 + `success` 8 + `error` 6) are authored values.**
   These are the theme author's choice, used as written, with no gate between
   the file and the frame. `border`'s 1,210 are authored too, but they are the
   stroke decision's (§"Option F") and are counted apart.
3. **`foreground`'s single theme is a slot defect, not a contrast preference.**
   `Rose Pine Dawn` sets `[text].primary = "#D7827E"`, which is the palette's
   *rose accent*; the same file carries `secondary = "#797593"` at 3.79 and
   `tertiary = "#907AA9"` at 3.27. The body-text slot holds an accent, so the
   theme renders body text at 2.45 while its own real text colours are legible.

`validate_theme` **warns**; it does not fail (`src/theme/validate.rs:157-173`).
It also never checks `ui.border` or `ui.accent_secondary` at all — so the two
families with the most entries and the largest counts are the two the validator
does not look at. That is why these failures are shippable today, and it is the
compatibility surface this decision must not break (§"Compatibility and
migration").

### Why this needs an ADR rather than a milestone action

The M007 plan forbids exactly the edit that would fix these:

> **No token is changed to make a cell pass.** The derived tokens in
> `src/theme/schema.rs` are a shipped contract for 50 bundled themes and every
> user-authored theme; changing one changes every theme. A failing token is
> reported, not adjusted.
>
> — `007-contrast-ratchet-to-zero.md` §4

and its §5 puts "changing any value in `src/theme/schema.rs` or any bundled
theme file" explicitly out of scope. That rule exists for a good reason: a
render path that swaps `muted` for `foreground` to clear a cell *moves* the
failure and leaves the palette broken. But the rule also means that the only
remaining action — repairing the palette itself — is unavailable to a milestone
and needs an owner. This ADR is that decision.

## Decision drivers

- **A value the theme file authors belongs to the theme.** Re-deriving it
  silently rewrites the author's palette, and rewrites every user theme too.
- **A value the theme file omits is not the theme's.** Inheriting another
  palette's colour is a projection defect; the fix belongs in the projection,
  not in a theme file that never mentioned the key.
- **A defect is not repaired by declaring it acceptable.** Two of the seven
  families fail because of where the value came from; lowering a threshold
  would leave that mechanism in place for every future theme.
- **The bar is a contract, not a constant.** `MIN_DERIVED_TEXT_RATIO` is
  exported from `codegg::theme::schema` and asserted by
  `tests/tui_render/theme_selection_tokens.inc`; a change to it is an API
  change to every consumer.
- **`validate_theme` stays warn-only.** A user theme that suddenly refuses to
  load is a worse outcome than a user theme that loads with a named warning.
- **A stroke is not decided here.** `border` is 64% of class (c) and its
  membership in the invariant is a different question (is a line of `─` a
  foreground?) with its own research. Settling it by threshold would conflate
  two decisions.
- **Nothing may be changed to make a cell pass without saying so.** Any clause
  that moves a shipped value is a deliberate supersession of the M007 plan rule,
  recorded here with the owner's ratification as its precondition.

## Considered options

### Option A — Re-derive the failing tokens in the token layer

Extend the derivation family in `src/theme/schema.rs` so `muted`, `secondary`,
`warning`, `success`, `error` and `border` are computed from the theme's own
body colours whenever they fail their tier, in the same way `selection_text`
and `accent_text` are computed today.

What changes for a user: every theme's de-emphasis, status and (if included)
stroke colours move together, including themes that pass today, and including
themes the user wrote. The de-emphasis tier is the casualty: deriving `muted`
as "the candidate that reads" collapses it toward `foreground`, which is the
one thing decision 3 in `007-status.md` §11 warns against.

Themes and values: 50 bundled themes plus every user theme; 5–7 token families,
i.e. up to 350 rendered values, of which 56 `(theme, family)` values across 28
themes currently fail.

Schema change: yes — a new derivation contract in `src/theme/schema.rs`, with
no new field. It is a shipped-API change: the two existing derived tokens are
asserted by `theme_selection_tokens.inc` and consumed by every importer.

Evidence that it landed: `theme_selection_tokens.inc` passes unmodified (the
two existing derived tokens did not move); a per-theme palette diff for all 50
themes is reviewed as a visual change; the I5 sweep for the re-derived families
is empty.

Failure mode: the derivation is a *guess* about the author's intent. Nothing in
a Halloy file says whether `#FF0000` was meant as a muted tier or as an accent,
so a derivation that assumes "muted" will sometimes rewrite a deliberate accent
into grey — and there is no witness in the format to contradict it.

### Option B — Re-author the failing values per theme in `assets/themes/halloy/*.toml`

Edit the bundled theme files so each authored value clears its documented bar.

What changes for a user: only the failing values in the bundled gallery move;
a user-written theme is untouched, because the change is in the shipped files
and the importer is unchanged.

Themes and values: 56 `(theme, family)` pairs across 28 of the 50 files
(`border` 22, `muted` 6, `secondary` 7, `foreground` 1, `warning` 7, `success`
7, `error` 6). Two caveats that make the number larger than it looks: `warning`
is authored in **no** file, so re-authoring it means *adding* 7 keys (or 50, if
the gallery is made explicit); and 41 of the 50 `secondary` values that do
exist are discarded by the importer today, so re-authoring the 7 failing themes
does not repair the 41 that render a foreign purple.

Schema change: no.

Evidence that it landed: the I5 sweep for those families is empty; the bundle
still loads through `parse_halloy_theme`; the theme gallery in
`docs/themes.md` is regenerated; the diff is a reviewed list of colours.

Failure mode: it is 22–56 hand-picked colours, which is exactly the kind of
edit a reviewer cannot verify except by looking. And it fixes the *symptom* for
`warning`/`secondary` while leaving the inheritance mechanism that produced
them, so the next imported theme fails the same way.

### Option C — Lower the bar for non-body tokens

Keep 3.0 for text and introduce a lower tier for tokens that are not body copy.
Which tokens would move, and to what:

| Tier | Tokens | Candidate bar | Justification available today |
|---|---|---|---|
| text | `foreground`, `muted`, `secondary`, `success`, `error`, `warning`, `link` | 3.0 | WCAG AA large text / the shipped `MIN_DERIVED_TEXT_RATIO` |
| decor | `border` (and only `border`) | 1.5 – 2.0, owner-named | none published; WCAG 1.4.11 puts *non-text* at 3.0 |

An illustrative package — stroke tier 1.5, `muted`/`secondary`/status tier 2.0
— removes 1,506 of the 2,495 entries and leaves 989. `border` alone at 1.5
removes 935 and leaves 275; at 2.5 it removes 550; at 3.0 it removes 0, because
the WCAG non-text minimum *is* the bar the harness already uses. So a stroke
tier only removes entries by asserting less than the published standard, and a
"non-body text" tier below 3.0 asserts less than the large-text minimum.

What changes for a user: some text that is hard to read is declared acceptable.
Nothing on screen changes.

Themes and values: none — two constants and a per-token tier table.

Schema change: no field; a contract change, which is this ADR.

Evidence that it landed: the sweep measures each token against its tier; the
baseline drops by exactly the entries the tiers release; `architecture/theme.md`
documents which tier each token belongs to.

Failure mode: `warning` at 1.34 survives every tier at or above 1.5, so option
C alone can never reach zero and must be combined with A, B or E anyway. And
because the harness's 3.0 is already *below* the documented 4.5 body bar,
lowering further means the product's strongest automated accessibility gate
asserts less than its own validator warns about.

### Option D — Keep the bar and record the failures as accepted

Leave every value alone. Rewrite the invariant's claim to what it can honestly
promise: a cell that uses a token the theme did not design for its fill is a
defect; a token that is below its own documented bar is a recorded palette
limitation, named per family with its theme list.

What changes for a user: nothing on screen. The gate stops calling these
defects and starts reporting them as known limits.

Themes and values: none.

Schema change: no.

Evidence that it landed: `docs/tui-render-invariant-baseline.toml` carries a
per-family rationale and the theme list; `007-status.md` §11 decision 1 is
closed as "accepted, recorded"; the ratchet still refuses growth and unbanked
reduction; M007's acceptance criterion 1 ("the baseline is empty") is formally
withdrawn.

Failure mode: "accepted" is not stable. The next audit re-opens it, and a
recorded limitation with no owner becomes a permanent exception — the exact
shape `docs/comment-budget.toml` and the writer-disposition manifest exist to
avoid elsewhere in this repository.

### Option E — Repair what is inherited, then re-author what is authored

Treat the seven families as two populations, because they are.

1. **Inherited values** (`warning`, `secondary`): the projection stops
   borrowing another palette's colour. A theme's own accent is used or a
   readable variant is derived from it, and an omitted status key is either
   required of the bundle or derived from that theme's own palette — never
   inherited from `placeholder_fallback`.
2. **Authored values** (`muted`, `success`, `error`, `foreground`): re-authored
   per theme in the bundled files, which is option B restricted to the values
   that are actually the author's.
3. **`border`**: not decided here (see Option F).

What changes for a user: the 7 light themes stop rendering `#B48CFF` and
`#FFB43C`, colours no theme file mentions; 6 `muted`, 7 `success`, 6 `error`
and 1 `foreground` value move in the gallery; user themes keep their authored
colours and gain a *warning* where they previously gained a silent foreign
value.

Themes and values: 7 files for `warning` (or a derivation), 7 files for the
destructively-inherited `secondary`, 14 files for the 19 authored values, 1
file for `foreground` — at most 28 distinct files, and no theme touched twice.

Schema change: none for the authored half. The inherited half changes importer
*policy* (`halloy.rs`), which is a projection contract change but adds no field.

Evidence that it landed: the four I5 families fall to zero; a test asserts that
a theme omitting `[text].warning` does not receive another theme's colour; a
test asserts that a rejected `accent_secondary` is derived from the theme's own
values rather than replaced; `theme_selection_tokens.inc` passes unmodified.

Failure mode: it is two mechanisms and a value sweep in one milestone, so it
needs ordering — the inheritance repair first, because it changes which values
the re-authoring half sees.

### Option F — Decide `border` as a stroke, separately (cross-reference)

`border` is 1,210 of the 1,898 entries and is not the same kind of token: it
paints a line, not a glyph a reader reads. The options are in
`007-border-as-text-research.md` §2b. This ADR records only that no `border`
*value* may be changed under it, and that the invariant's scope for `border` is
the stroke decision's business.

## Decision

Status is `proposed`: the clauses below are the proposed decision, and clauses
2, 3 and 4 are the ones a maintainer must ratify because they move shipped
values or change a projection contract.

1. **Two authorities, decided by whether the theme file names the key.** A value
   the file authors is the theme's and is repaired *in the file*. A value the
   file omits is a projection defect and is repaired *in the projection*. No
   bundled theme may render a colour that appears in no theme file and no
   theme-derived rule.
2. **`warning` and `secondary` are repaired as inheritance, not as values.**
   `placeholder_fallback`'s `#FFB43C` and `#B48CFF` must not reach a theme that
   did not ask for them. Either the bundle authors the key (7 files for
   `warning`) or the projection derives it from that theme's own body colours;
   the ADR leaves the mechanics to the milestone but not the outcome. An
   `accent_secondary` the accent gate rejects is derived from the theme's own
   values, not replaced with a foreign hex. **This clause is a projection
   contract change and supersedes nothing**; it does not change any authored
   palette value.
3. **`muted`, `success` and `error` are re-authored per theme.** 19 values in 14
   files (6 + 7 + 6 values; a theme can appear in two families with two
   different tokens). The de-emphasis tier is kept: `muted` stays a distinct
   token, and a value that collides with `foreground` is a failure of this
   clause, not a success. **This clause supersedes `007-contrast-ratchet-to-zero.md` §5
   ("changing any value … in any bundled theme file") for exactly these files**,
   and is the owner decision §14 reserves.
4. **`foreground` is corrected in `Rose Pine Dawn` only.** `[text].primary` holds
   the palette's rose accent; the body-text slot gets the theme's body colour.
   The maintainer confirms the value, because the file does not currently carry
   it — `secondary = "#797593"` at 3.79 is the file's own legible candidate and
   the upstream palette's body text is `#575279`. **This clause supersedes the
   same plan rule**, and is a value edit in one shipped theme.
5. **`border` is out of this decision.** No `border` value changes until the
   stroke decision is taken. `007-status.md` §11 decision 2 is answered by
   `007-border-as-text-research.md`, not here.
6. **The text bar does not move.** `MIN_DERIVED_TEXT_RATIO` stays 3.0;
   `PRIMARY_TEXT_RATIO` stays 4.5; `MUTED_TEXT_RATIO` and `ACCENT_RATIO` stay
   3.0. Option C is not selected. This ADR also records that the I5 sweep's
   3.0 bar is *weaker* than the documented 4.5 body bar, so no one may cite a
   green I5 as evidence that body text clears 4.5.
7. **`validate_theme` stays warn-only.** These failures are shippable today
   because the validator warns; promoting a warning to an error would make
   every user theme carrying a low-contrast token fail to load. A new
   diagnostic (the inheritance check in clause 2) is added as a **warning**,
   with a `field` naming the key, in the same shape as the existing checks.
8. **Options A and D are rejected.** A rewrites authored palettes for all 50
   themes and every user theme to repair 56 values, and it destroys the
   de-emphasis tier. D leaves the inheritance mechanism in place and turns a
   defect into a permanent exception. Option B is selected only for the values
   clause 1 classifies as authored, which is Option E clauses 2–3.
9. **Scope boundary.** No file under `src/`, `tests/` or `assets/` is changed by
   this ADR. It authorizes a follow-up milestone; it does not perform one.

If clauses 2–4 are ratified, the entries cleared are
35 + 238 + 354 + 8 + 6 + 47 = **688**. With clause 5, the remaining 1,210 are
the stroke decision's. The two numbers are the whole of class (c).

## Consequences

### Positive

- The 7 light themes stop rendering two colours that no theme file mentions,
  and the mechanism that produced them is closed rather than recorded.
- 19 authored values are repaired without touching a user theme, and the
  de-emphasis tier survives.
- The invariant keeps its bar, so a green sweep still means what it says for
  every token it still covers.
- `border`'s 1,210 entries are separated from the palette question, which is
  the only way either decision can be argued on its merits.

### Negative

- Two of the seven families are repaired by changing shipped theme values,
  which is a visible restyle of parts of the gallery even though it is a
  correction. The M007 plan rule against it is superseded deliberately, not
  bypassed.
- `Rose Pine Dawn`'s `foreground` change is the largest single-theme visual
  change in the set (47 entries, body text at 2.45 today).
- The inheritance repair changes `halloy.rs`'s rejection policy, so a theme
  whose accent was previously replaced now gets a derived one — two themes with
  the same authored value can end up with different accents, by design.
- The bar mismatch (3.0 in the sweep vs 4.5 in the validator) remains. This ADR
  records it instead of fixing it.

### Neutral or deferred

- Decision 3 in `007-status.md` §11 ("is the muted tier allowed to exist?") is
  answered in the affirmative by clause 3 and negatively by Option A; that is
  the substance of rejecting A.
- Decision 4 (the focus collisions: `primary == border` on 4 themes,
  `primary == muted` on 7, `border == muted` on 3) is untouched. It is a
  *collision* problem, not a contrast problem, and clause 2's derivation may
  change it — the milestone must re-measure rather than assume.
- Decision 5 (the two harness corrections) is a precondition for any re-bank and
  stays with M007.
- `status.info`, `status.debug`, `status.trace` and `conversation.*` are not
  checked by `validate_theme` and are not in this decision. They may carry the
  same class of defect; nothing here claims otherwise.

## Compatibility and migration

- **User theme files keep working.** Clauses 3 and 4 edit bundled files only;
  the importer is unchanged for authored values. A user theme continues to load
  with its own colours.
- **A user theme that omits `[text].warning` changes appearance** under clause
  2, because it stops inheriting `#FFB43C` and gets a value derived from its own
  palette. That is the intended effect, and it is a change a user can see — it
  must be reported as a warning diagnostic naming the key, not applied silently.
- **`validate_theme` stays warn-only.** No diagnostic becomes load-failing.
  A theme that loads today still loads.
- **`theme_selection_tokens.inc` must pass unmodified.** The two derived tokens
  are the contract this ADR must not disturb; if either moves, the change is a
  schema change and needs a different ADR.
- **No durable data, protocol frame, storage layout or scheduler surface
  changes.** No migration.
- **Rollback** is reverting the theme files and the importer policy; both are
  content changes with no persisted state.

## Security and reliability implications

No authorization, secret-handling, contention, cancellation or recovery effect.
No filesystem or process surface. The reliability concern is the ordinary one
for a presentation contract: a derivation that throws must not leave a theme
unloadable, and a rejected colour must fall back to a *readable* value rather
than an invisible one. Clause 2's fallback is therefore required to be a
computed legibility answer (the `derive_text_on` rule already guarantees ≥4.5
for its fallback), not a hardcoded second foreign colour.

The accessibility implication is the point of the change: 688 recorded
unreadable pairs become readable on themes that ship.

## Verification

- `cargo test --test tui_render -- theme_selection_tokens` passes **unmodified**,
  proving no derived token moved.
- `cargo test --test tui_render -- invariants::baseline` reports the six
  repaired families (the seven minus `border`) absent from `[contrast]` and
  every other family unchanged (no entry added anywhere).
- `python3 scripts/check_tui_render_invariant_baseline.py` passes, and its
  `--self-test` still rejects a synthetic re-addition.
- A test proves clause 2: a theme file with no `[text].warning` does not
  resolve `status.warning` to `placeholder_fallback`'s value, and a theme whose
  `accent_secondary` fails the accent gate resolves to a value derived from its
  own palette rather than to `#B48CFF`.
- A test proves clause 3's tier survives: for every bundled theme,
  `muted != foreground` and `muted.contrast_ratio(background) >=
  MUTED_TEXT_RATIO`.
- `python3 scripts/check_no_hardcoded_selection_colours.py` passes; no new
  `Color::` literal enters a render path.
- `architecture/theme.md` and `docs/themes.md` are updated with the two
  authorities, the tier table and the regenerated palette.
- `scripts/verify.sh quick` is green.

## What would falsify this decision

- **If a theme's `muted` and `foreground` cannot both clear their bars while
  remaining visually distinct**, the de-emphasis tier is incompatible with the
  bar and Option D is the honest answer. The decision assumes they can; that
  assumption must be measured for all 50 themes, not asserted.
- **If a sampled set of upstream Halloy themes uses `text.tertiary` as a
  saturated accent** — `intellij-light`'s `#FF0000` and `macaw-light`'s
  `#8839EF` suggest it — then `muted ← text.tertiary` is a slot-mapping error,
  and the repair is a projection change, not 6 value edits. This is the single
  most likely way clause 3 is wrong.
- **If `Rose Pine Dawn`'s upstream palette was deliberately imported with the
  rose as its primary text**, clause 4 is a restyle and must be withdrawn in
  favour of Option D for that theme.
- **If user themes in the wild rely on the literal `#B48CFF` fallback** — i.e.
  someone prefers the substituted accent — clause 2 breaks them. Nothing in the
  tree records such reliance, but nothing disproves it either.
- **If the stroke decision lands on "strokes stay at the text bar"**, clause 5
  resolves to 22 more value edits and the entries this decision clears become
  1,898 rather than 688. The decision is still correct; its arithmetic changes.
- **If the two harness corrections (M007 findings 1–2) re-bank `[contrast]`
  downward by more than the families above**, the entry counts here are
  superseded by the re-banked manifest. The families and their causes are not.

## Supersession

None. This ADR supersedes no accepted ADR. It **supersedes two clauses of an
unclosed milestone plan** — `007-contrast-ratchet-to-zero.md` §4's "no token is
changed to make a cell pass" and §5's "changing any value in `src/theme/schema.rs`
or any bundled theme file" — for exactly the files and values named in clauses
3 and 4, and only when the maintainer ratifies those clauses. The plan's §14
requires an owner decision for a failure of this class; this is that decision,
not an exception to it.

---

## Amendment (2026-11-13)

Status of this amendment: **proposed**, drafted under the owner instruction
quoted at the head of this ADR ("the bundled themes are third-party and stay
unedited"). It changes the authority rule in clause 1 and withdraws clauses 3
and 4. It does not rewrite the decision text above; where the two disagree, this
amendment governs.

Research: `plans/closure/tui-modularity-and-render-invariants/008-halloy-mapping-research.md`.

### What the old rule got wrong

Clause 1 asks **"does the theme file name the key?"** and answers *authored →
repair in the file* / *inherited → repair in the projection*. That test is
unavailable to the question it is being asked, because it cannot see **what the
key means**. Two failures follow, both measured in the memo:

1. **It cannot tell a focus indicator from chrome.** `ui.border` is resolved
   from `buffer.border_selected` — the edge Halloy draws around the *selected*
   buffer, which upstream documents as a focus indicator a theme may disable by
   authoring it transparent — and painted on **55 unfocused surfaces per theme**
   (`halloy.rs:207-222`, `tui/panels/chrome.rs:62-67`). The file authors the key,
   so the rule called the resulting 1,232 failing rows the file's problem. They
   are not: no value of *that field* is a chrome colour.
2. **It cannot tell a text tier from a third accent.** `text.tertiary` is a
   saturated accent (chroma > 40) in **37 of 50** bundled themes, so
   `muted ← text.tertiary` imports an accent into the de-emphasis tier. The file
   authors the key, so the rule again assigned the value to the file.

Because four of the five failing families were *authored*, and the rule offered
exactly one remedy for an authored value, **the file was the only bin with an
action in it** — and 20 values across 14 third-party files were edited even
though the defect was in the translation. Two of those edits made the mapping
worse rather than better: `macaw-dark`'s violet `text.tertiary` and
`base16-atelier-forest-light`'s orange one were each replaced with a grey,
fabricating a de-emphasis colour the author never wrote.

### The rule that replaces clause 1

> **Authority follows whether the value is *translated*, not whether it is
> written down.** A colour read through the Halloy importer is a translation of
> another format's field, and the projection owns its legibility. A colour
> written by a `codegg` native theme names *this* schema's token, and the file
> owns it.

Operationally, for a translated token:

1. **The file owns the hue.** The authored value is the derivation's input, so
   the colour stays in the palette's family.
2. **The projection owns the legibility.** If the translated value fails the bar
   its consumer documents, move it along the lightness axis — toward whichever
   of black or white reads better on the theme's own `background`, stopping at
   the bar — and ship that. `derive_readable_variant` already does exactly this
   and is already the accepted mechanism for `status.warning` and a rejected
   `accent_secondary`.
3. **The direction is not a choice.** On a dark background a darker stroke can
   never reach 3.0 (`macaw-dark`'s floor is 1.66); on a white background nothing
   lighter than white exists. Only one direction is arithmetically available.
4. **It is reported, not silent.** Each derivation emits a `warn` diagnostic
   naming the key, in the shape clause 7 already requires.
5. **Values that already read are untouched.** The gate fires only on a value
   below its bar, so this is strictly narrower than Option A: it cannot rewrite
   an author's choice that works, and it cannot destroy the de-emphasis tier
   because it never runs on a tier that reads.

This satisfies clause 1's surviving sentence — "no bundled theme may render a
colour that appears in no theme file **and no theme-derived rule**" — because
every derived value is a function of the file's own hue and the theme's own
background.

### The bars, per token

| Token | Bar | Source of the bar |
|---|---|---|
| `ui.border` | 3.0 | `MIN_DERIVED_TEXT_RATIO`, the bar the I5 sweep already applies to every painted cell |
| `text.muted` | 3.0 | `MUTED_TEXT_RATIO` |
| `status.success`, `status.error` | 3.0 | `ACCENT_RATIO` |
| `base.foreground` | **4.5** | `PRIMARY_TEXT_RATIO` — body copy, and the bar `validate_theme` already documents |

No new threshold is introduced. Clause 6 is unaffected: the bar does not move.

### Withdrawn

- **Clause 3 is withdrawn.** `muted`, `success` and `error` are **not**
  re-authored per theme. The 19 values are reverted to their `HEAD` contents and
  the outcomes are produced by the projection rule above.
- **Clause 4 is withdrawn.** `Rose Pine Dawn`'s `[text].primary` returns to
  `#D7827E` (the palette's own rose), and the projection lifts it at the 4.5 bar
  to a value in the same hue. The slot was never in doubt; only the lightness
  was.
- **Clause 8's rejection of Option A is narrowed, not reversed.** Option A
  re-derived tokens in `schema.rs` for every theme, including themes that pass,
  and that is still rejected. The amendment adopts a *gated* form: importer-side,
  firing only below the bar, hue-preserving, and never applied to a native theme
  that names our token.

### Unchanged

- **Clause 2 (the inheritance repair) stands** and is already implemented. If
  anything it is strengthened: upstream documents that an omitted or invalid
  colour "will fall back to transparent", so a missing key in Halloy means *no
  paint*, not another palette's colour. We cannot express transparent, so a
  derived value is the closest available approximation — and it must be reported.
- **Clause 5 is superseded in effect, not in text.** `border`'s *measurement*
  stays at the text bar (the owner's 2026-11-12 decision, recorded in
  `007-border-as-text-research.md`); its *value* is now projection-derived like
  any other translated token, so no `border` value is edited in any file.
- **Clause 6 (the bar does not move)**, **clause 7 (`validate_theme` stays
  warn-only)** and **clause 9 (scope boundary)** stand.

### Consequences

- `assets/themes/halloy/*.toml` returns to **byte-identical third-party
  content**. The bundle is the Halloy gallery again, and `docs/themes.md`'s claim
  that the 50 files are "sourced from the upstream Halloy themes gallery" becomes
  true again.
- 22 themes' stroke values move (8 imperceptibly, 14 visibly; `macaw-dark`'s
  recessed groove becomes an outline). 28 themes are byte-identical. This is the
  price of a 1-cell stroke at 3:1 and it is stated rather than hidden.
- `[contrast]` 1,448 → 216 once every gate lands and the revert is in the same
  change. The revert and the projection rule **must ship together**, or the
  baseline rises by up to 415 before it falls.
- The two `success`/`error` families are repaired for 50 bundled themes *and*
  every user theme, where the withdrawn clause reached only 50 files.

### What would falsify this amendment

The same falsifier as the memo's §F.3: a rendered comparison showing that a
border derived to exactly 3.0 reads as a wireframe rather than an edge. If a
full-cell stroke at 3:1 is perceptually dominant, then no derivation satisfies
both the bar and the design, and the honest answer is a token split plus an
exemption decision — which would reopen the stroke decision of 2026-11-12 rather
than this authority rule.
