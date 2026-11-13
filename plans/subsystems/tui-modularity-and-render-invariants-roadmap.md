# TUI Modularity and Render Invariants Roadmap

Status: active — M001–M004 implemented; M005–M010 in progress or pending

Long-term references:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the state
  hierarchy and interaction concepts the TUI must present. It is the
  **surface inventory** this roadmap is accountable to.
- `plans/000-long-term-specification.md#29-system-invariants` — invariant 24
  ("the daemon owns one command catalog") is the boundary §7 M010 must not
  cross.
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` —
  accepted. The daemon owns the command catalog; a frontend dispatches a
  command, it does not define one. M010 reshapes **dispatch** only and does
  not touch catalog ownership.
- `plans/adrs/ADR-0062-bundled-palette-contrast-authority.md` — **proposed,
  not accepted.** It exists because M007 reached its §14 stop condition: 76.1%
  of the `[contrast]` baseline is a plain palette token failing on the fill it
  is designed for, which no render path can fix. The ADR proposes that
  authority over such a token is decided by whether the theme file names the
  key — inherited values are a projection repair, authored values are
  re-authored — and it does **not** move the contrast bar. M007 stays blocked
  until it is accepted or replaced. Its sibling research memo,
  `plans/closure/tui-modularity-and-render-invariants/007-border-as-text-research.md`,
  settled the stroke question, and the later mapping round removed the whole border family through the projection rather than the palettes.
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
  — the frontend holds no text authority. No milestone here creates a second
  text buffer or a second mutation path.
- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` —
  accepted. One owner per key chord. M005 and M006 change what a hint
  *says*, never which stage owns a key.

## Authority note: what the canonical spec does not say

The canonical long-term documents specify the TUI's **state hierarchy**
(`000` §25) and the **system invariants** (`000` §29), and they say nothing
about presentation: no requirement on contrast, clipping, focus indication,
or uniformity exists in `plans/000-`, `plans/001-`, or `plans/002-`. A search
for `contrast`, `accessib`, `readab`, `legib`, and `visual` across all three
returns one unrelated line about project-relative paths.

This roadmap therefore establishes **interim presentation invariants** (I1–I6,
§2) under the authority of `plans/003-planning-process.md`. They are
subsystem-level requirements, not canonical ones, and §10 records that
promoting them into `plans/000-long-term-specification.md` is an open owner
decision rather than something a milestone may decide.

## 1. Purpose and ownership boundary

Make the TUI's **rendering** answerable to a small set of mechanically
checkable invariants, and remove the structural duplication that lets those
invariants drift — without changing what any surface can do.

The premise this roadmap is built on, stated plainly: the TUI's maintainability
problem and its visual problem are the same problem. There is no shared
visual primitive, so 41 dialogs and 6 panel surfaces each re-decide border,
title, focus treatment, hint text and spacing. Fixing the duplication without
fixing the presentation would leave the drift; fixing the presentation without
fixing the duplication would not hold.

**This subsystem owns** the frontend's presentation contract: what a panel,
dialog, section, hint line and focus indicator look like, the primitives that
produce them, and the invariants that keep them usable.

**It consumes** `Theme` and the derived-token machinery in `src/theme/`,
`Component`/`FocusManager`/`SidePanelHost`, and the existing `tests/tui_render`
family.

**It must not own** text authority (ADR-0011), key-chord ownership (ADR-0042),
command catalog ownership (ADR-0052), or any transport, permission or
scheduler boundary. No milestone here adds, removes or rebinds a user-visible
capability except where §7 names it explicitly.

**It must not become** a redesign. The bar for every change is §11: nothing
renders broken or unusable. That is deliberately *not* pixel-identity — the
owner has ruled that out — but it is also not licence to restyle.

## 2. Work classification

### Invariants

The six presentation invariants. Each is executable; M001 delivers the
harness and every later milestone is gated on it.

- **I1 — Nothing is painted outside its area.** A widget writes no cell
  outside the `Rect` it was given. Checked by rendering into a
  sentinel-filled buffer and asserting the sentinel survives outside the
  area.
- **I2 — No panic and no blank frame at the smallest supported size.** Every
  dialog and panel renders at ~20×6 and 1×1. The guard is currently ad hoc:
  `panels/chat.rs` and `panels/terminal.rs` check `width < 12 || height < 4`,
  `panels/agents.rs` and `panels/help.rs` check nothing.
- **I3 — Focus is visible, in one channel.** For every focusable surface,
  focused and unfocused renders differ, and the difference is a border or
  title **style** change — not only a glyph. This is the invariant that
  finding §4 makes concrete: five bespoke panels currently express focus five
  different ways and one expresses it only with a `●`. It is delivered in two
  halves, because the halves become true at different times: **I3a** ("focused
  differs from unfocused") already holds today, since every panel paints a
  `●`, and M001 asserts it so M002 cannot remove the only difference; **I3b**
  ("the difference is a border or title style, uniformly") cannot hold before
  M002 and is added there.
- **I4 — Every advertised key is handled.** Both an advertisement's sources
  are authoritative: when a `keymap_hint` and a painted footer disagree about a
  key, that disagreement is itself a finding, not a tie to break — the surfaces
  that name `PgUp/PgDn` in the footer while the hint says `^d/^u` are the
  recorded case. For each verb named in a
  `keymap_hint` or a border title, pressing that key changes the rendered
  frame or closes the surface. The rendered effect is the assertion rather than
  `handle_key`'s return value, because `Option<TuiMsg>` returns `None` for both
  "handled, nothing to report" and "not handled" — see §7 M006.
- **I5 — Every rendered foreground clears a contrast bar against its fill.**
  Read every cell of a frame and compare its foreground against the
  background actually painted under it, on all 50 bundled themes. The bar is
  the shipped `MIN_DERIVED_TEXT_RATIO` from `codegg::theme::schema`, reusing
  `Color::contrast_ratio`; no new threshold is introduced.
- **I6 — A hint line never loses its exit key.** The exit item is ordered
  first, so truncation from the end cannot remove it: no budget that shows any
  part shows a row without it. Truncation still drops whole parts only, and a
  budget too small for the exit item alone renders an empty row rather than a
  partial key, which is the rule `fit_parts` already documents.

Also invariant, and inherited rather than new:

- **The comment corpus is a ratchet.** `docs/comment-budget.toml` holds a
  per-file ceiling for every file this roadmap splits, and
  `unlisted_max_comment_lines = 20`. Moving code between files means banking
  the split and redistributing ceilings in the same change. No milestone may
  raise the global ceiling of 134,015 comment lines.
- **The render path keeps one producer per derived result.** Enforced by
  `scripts/check_tui_render_cost.py`, which is a *shape* check. M008 in
  particular must satisfy it, not relax it.

### Capabilities

- **Uniform focus indication** across every panel and dialog (M002, M003).
- **A dialog that fits its content** rather than occupying 50% of the frame
  unconditionally (M004).
- **One hint vocabulary** — one separator, one verb spelling, one casing
  (M005).
- **No advertised key that does nothing** (M006).
- **Every surface readable under every bundled theme** (M007).

Each of these is user-visible. None may be reported as delivered on the
strength of the primitive existing.

### Infrastructure

- The invariant harness and the contrast ratchet (M001) — test-only, and
  explicitly **not** a capability.
- `FramePlan` and the retirement of the ten `Option<Rect>` fields from `App`
  (M008) — internal geometry ownership.
- `ListCursor` (M009) — internal list-navigation and scroll-follow.
- Slash dispatch tables (M010) — internal command routing.

Infrastructure milestones MUST NOT be presented as user-visible improvement.
M008–M010 are justified by maintainability alone, and their acceptance
evidence is that the corpus did not move.

### Polish

None scheduled. The roadmap deliberately has no polish milestone: every item
in §7 either closes an invariant or removes a duplication that causes
invariant drift. Anything cosmetic that is not one of those belongs in a
successor.

## 3. Non-goals

Normative. Each item is **not covered** by this roadmap, with the reason.

- **A redesign of any surface.** The bar is "not broken or unusable" (§11).
  Restyling, new glyph sets, new layout concepts, and a new colour palette are
  out. M002–M005 align what exists; they do not invent.
- **A design-token system for spacing, glyphs or casing.** §4 records that
  `Theme` has 19 colour fields and no other visual vocabulary. A full token
  layer is a plausible successor and is explicitly deferred: it would need an
  ADR (it establishes a public compatibility contract for every bundled
  theme), and doing it inside a refactor is how a refactor becomes a redesign.
- **Splitting `App` beyond frame geometry.** The `Option<Rect>` cluster is
  owned by M008 because it is exactly the frame-geometry cluster and
  `FramePlan` is the contract that absorbs it. The remaining ~105 fields are
  **not** in scope; §10 records the successor.
- **`dispatch_tui_command`.** M010 converts the 161-arm synchronous slash
  dispatcher only. The 259-arm async `TuiCommand` dispatcher is deferred: its
  arms own task spawning, so it needs an async-handler contract as well as a
  table, and that is a second pass.
- **New keyboard capabilities.** No milestone binds, unbinds or rebinds a
  chord. M005 and M006 change what a hint says and whether an already-
  advertised key works; ADR-0042's ownership contract is untouched, and the
  two key-authority guards must report the same chord counts after every
  milestone as before it.
- **Contrast work outside the TUI.** `src/theme/` derives tokens; this
  roadmap consumes them. A token that is wrong is a `src/theme/` defect, and
  M007 reports it rather than working around it.
- **Desktop and web frontends.** The desktop tree is a separate Cargo
  workspace with its own qualification lane. This roadmap is TUI-only.
- **Performance.** `scripts/check_tui_render_cost.py` must stay green, but no
  milestone is a performance milestone and no wall-clock claim is made.

## 4. Current state

Measured at baseline `7642b9e38` (branch `feat-auto-20261109-f8abad0d`).
Figures are counts and structure, not timings.

**Scale.** `src/tui` is 210 files and 223,328 lines — 35.2% of all of `src/`.
Of that, 152,969 lines are production and 70,359 (31.5%) are inline tests.
There are 4,227 production functions with a median length of 9 lines; 184
exceed 100 lines, 48 exceed 200, and 18 exceed 400.

**Concentration.** `src/tui/app/mod.rs` holds 16,872 production lines, an
`App` struct with **115 fields**, and a single `impl App` with **330
methods**. Ten of those fields are `Option<Rect>` frame-geometry scratch.
`dispatch_tui_command` is 3,210 lines over 259 `TuiCommand` variants;
`dispatch_builtin_command` is 3,101 lines over 161 slash actions, of which 23
delegate to `commands::*` modules and 101 call `self.` inline. `App::render`
is 428 lines that compute the row budget, carve strips and paint in one pass.

**Duplication.** 41 dialog files; 40 define `handle_key`; 34 hand-roll
`j`/`k`/`Up`/`Down`; `select_up`/`select_down` are defined 16 times each; 29
dialogs declare their own scroll-offset field while only 12 use
`CenteredScroll`. 26 dialogs draw `Borders::ALL` without overriding
`render_in_panel`, against 15 dockable dialogs and 14 overrides. A three-line
`command_mode` teardown appears 30 times in `app/mod.rs` alone
(`command_mode = false` appears 95 times across the tree).

**Panel chrome is not uniform.** Five bespoke panels plus the hosted host
express focus six different ways:

| Surface | Outer border | Border colour changes on focus | Title style changes on focus |
|---|---|---|---|
| Chat | yes | yes | yes |
| Terminal | yes | yes | yes |
| Session | yes | yes | no `title_style` at all |
| Agents | yes | **never** | **always** `primary` + BOLD |
| Help | **none** | n/a | dot rides the first inner box |
| Hosted (18 panels) | yes | **never** | **never** — only the `●` glyph |

**Dialog chrome is not uniform.** The number of bordered boxes per dialog
ranges from 1 (`queue`) to 10 (`run_detail`), with `plan` at 9 and `question`
and `connect` at 6. All modals are laid out from one
`centered_rect(60, 50)` in `App::render_dialog`; `ProjectPicker` is the only
dialog that sizes itself, so a two-option Confirm receives 30 rows on a 60-row
terminal.

**Hint text has three dialects.** Of 93 hint strings, 50 use ` | `, 37 use
bare spacing, and 6 use `:`. The panel hint line uses `·` (382 occurrences
across 47 files) while dialog border titles use `|`. `Esc` is spelled five
ways (`Esc close` 31, `Esc cancel` 10, `Esc: cancel` 2, `Esc back` 2,
`Press Esc` 2) and verb casing splits 78 lowercase against 5 Title Case. Of 59
dialog border titles, 10 lack the leading padding space — all ten in
`run_detail.rs` — and 4 carry a focus dot, 3 trailing and 1 leading.

**Presentation vocabulary is colour-only.** `Theme` has 19 fields, all
colours. `readable_on` exists in `src/tui/app/render.rs` and is applied
in the completion popup only.

**The harness is partial.** `tests/tui_render` has 60 members, and 25+ sites
use `TestBackend`. `tui_info_dialog_panel_chrome.inc` already renders a column
to `Vec<String>` and asserts the host's chrome survives — the right shape, but
it reads symbols only, so a colour regression passes it. No test asserts
focus *appearance*: `tui_agents_panel.inc` asserts
`app.panel_host.is_focused()`, which is state, not pixels. `fit_parts` — the
function that decides which hint keys survive a narrow pane — has zero tests.

**Known pre-existing defects** in scope for the milestones that own them:
`ImportDialog::handle_key` handles only `Esc` and `Enter`, so a typed URL
never reaches the preview while paste works; the `/shell-show` modal
advertises `i`/`a`/`r`/`k`/`e` and handles none of them; and
`handle_dialog_key_legacy` is a 615-line `#[cfg(any())]` body that no compiler
checks.

**Guards already in place.** 16 `scripts/check_tui_*` guards plus
`check_no_hardcoded_selection_colours.py`, of which
`check_tui_render_cost.py` constrains render shape and
`check_no_hardcoded_selection_colours.py` already forbids a literal selection
foreground in a dialog render path.

## 5. Target architecture

Five primitives and one harness. Each primitive is the single producer of its
concern; each is adopted in the milestone that introduces it, so no milestone
leaves an unused abstraction behind.

```text
PanelChrome      border + title + focus marker for a panel column
Section          a bordered block with a hint on its border (dialog chrome)
HintLine         an ordered verb list rendered under one separator and casing
FramePlan        named regions computed in one pass, handed to painters
ListCursor       clamped index + scroll-follow for a list surface
```

The harness is `tests/tui_render/invariants/` plus a capture helper that
renders a surface and returns its cells, and
`docs/tui-render-invariant-baseline.toml` as the ratchet for the invariants
that cannot pass yet (I4, I5, I6), enforced by
`scripts/check_tui_render_invariant_baseline.py`.

Two rules make the target architecture safe rather than aspirational. First,
**a primitive is parameterised to reproduce every current variant exactly
before any caller adopts a uniform policy** — that is what makes M002 and M003
reviewable. Second, **the corpus is the change detector**: a milestone whose
plan claims neutrality must produce a byte-identical corpus, and a milestone
whose plan claims a visual change must ship a reviewed diff of it.

## 6. Dependency graph

```text
M001 harness ──┬──> M002 panel chrome ──┐
               │                        │
               ├──> M003 dialog frame ──┼──> M007 contrast to zero
               │         │              │
               │         v              │
               │      M004 modal sizing ┤
               │                        │
               ├──> M005 hint vocabulary┤
               │         │              │
               │         v              │
               ├──> M006 advertised keys┘
               │
               ├──> M008 frame plan
               ├──> M009 list cursor
               `──> M010 slash dispatch
```

- **Hard** — M001 → every other milestone. No later milestone can demonstrate
  an invariant without the harness, so the edge is a verification gate rather
  than a design dependency; each becomes `ready` the moment M001 closes.
- **Hard** — M003 → M004. Content-sized modals need `Section` to report its
  content height; sizing cannot be derived before the primitive exists.
- **Hard** — M002, M003, M004, M005, M006 → M007. The ratchet can only reach
  zero once the chrome work has stopped moving.
- **Interface** — M002 → M003. `Section` reuses `PanelChrome`'s focus-style
  resolution so the two cannot diverge.
- **Interface** — M005 → M002, M003, M006. The hint builder is adopted by both
  panel and dialog chrome, and M006 reads the advertised verbs as structured
  `(key, verb)` pairs rather than parsing them back out of a rendered string.
  This is why M005 is ordered after M002 and M003 in §7 despite depending on
  neither, and why M006 follows it.
- **Soft** — M008 → M002, M003. `FramePlan` does not need the primitives, but
  the primitives make its call sites simpler.
- **None** — M009 and M010 are independent of everything but M001. They touch
  list state and command routing, neither of which the chrome work moves.

## 7. Milestones

### Milestone 001 — Render invariant harness and contrast ratchet

Class: invariant

Objective: make I1–I6 executable, and record the current I5 failure set as a
ratchet so the roadmap has a measurable start.

Dependencies: none.

Deliverable boundary: `tests/tui_render/invariants/`, a shared capture helper
returning cells with their full style, and
`docs/tui-render-invariant-baseline.toml`.
No production file changes.

User or operator value: none directly. This is the gate every later milestone
is measured against, and it converts "I did not change the rendering" from a
claim into a test.

Exit conditions: I1, I2, I3, I4 and I6 assert and pass; I5 measures and
records a baseline with the failing cells named by surface and theme; the
ratchet guard runs in `scripts/verify.sh quick` and fails when the baseline
grows.

Deferred work: driving the I5 baseline to zero (M007).

### Milestone 002 — One panel chrome

Class: capability

Objective: one `PanelChrome` primitive adopted by all five bespoke panels and
the hosted host, so focus is indicated the same way everywhere.

Dependencies: M001 (hard).

Deliverable boundary: `src/tui/panels/**`. Closes I3.

User or operator value: a focused panel is visibly focused, and the same
visual signal means the same thing on every edge. Today a focused Agents
panel differs from an unfocused one only by a glyph.

Exit conditions: I1–I3 green for every panel at every placement; the corpus
diff for each panel is reviewed and recorded; no panel loses its title,
its dot, or its content inset.

Deferred work: the spacing and glyph token layer named in §3.

### Milestone 003 — One dialog frame

Class: capability

Objective: one `Section` primitive producing a bordered block with a hint on
its border, adopted for every dialog's outer frame, title row and footer row.

Dependencies: M001 (hard), M002 (interface).

Deliverable boundary: `src/tui/components/dialogs/**`. Removes the bordered
boxes that exist only to hold a title or a hint, and normalises title padding,
casing and focus-marker placement. A dialog's **content** grouping is
untouched, so a 10-box dialog keeps the boxes that group content.

User or operator value: a dialog's chrome reads the same everywhere, and no
title hugs a corner or switches case.

Exit conditions: every dialog's outer frame and hint rows come from
`Section`; the corpus diff is reviewed per dialog; no dialog loses a hint, a
verb, or a row of content.

Deferred work: content-derived sizing (M004).

### Milestone 004 — Content-sized modals

Class: capability

Objective: replace the unconditional `centered_rect(60, 50)` with a size
derived from the dialog's content, clamped to a minimum and a maximum.

Dependencies: M001 (hard), M003 (hard).

Deliverable boundary: `App::render_dialog` plus a per-dialog size hint. A
two-option Confirm stops receiving half the frame.

User or operator value: a small question looks like a small question, and a
dense dialog gets the room it needs.

Exit conditions: every dialog reports a content size; every dialog renders
within its clamp at 80×24, 120×40 and 40×12; I1 and I2 green at all three;
the corpus diff is reviewed and recorded.

Deferred work: none.

### Milestone 005 — One hint vocabulary

Class: capability

Objective: one `HintLine` builder with one separator, one verb vocabulary and
one casing, adopted by panel hints and dialog footers; `fit_parts` gains the
I6 guarantee.

Dependencies: M001 (hard).

Deliverable boundary: `src/tui/components/component.rs`, the hint producers in
`app/mod.rs`, and every dialog footer. Closes I6.

User or operator value: the keys under a surface are spelled the same way
everywhere, and a narrow pane can no longer drop the key that leaves.

Exit conditions: one separator and one casing across every hint string; I6
asserts for every hint producer at 20, 40 and 80 columns; the corpus diff is
reviewed and recorded; no hint loses a verb it used to advertise.

Deferred work: localisation, if ever wanted, is out of scope.

### Milestone 006 — Every advertised key is handled

Class: capability

Objective: close I4 by making every advertised verb real, and fix the two
known instances.

Dependencies: M001 (hard), M005 (interface — the advertised verbs arrive as
structured `(key, verb)` pairs rather than a rendered string).

Deliverable boundary: the `/shell-show` modal and `ImportDialog`, plus the
generic I4 assertion.

User or operator value: a footer that names a key becomes a promise the
surface keeps. Typing a URL into the import dialog starts working.

Exit conditions: I4 green for every `keymap_hint` and every border-title
verb; `/shell-show` either handles `i`/`a`/`r`/`k`/`e` or stops advertising
them; `ImportDialog` accepts typed characters; the three gaps recorded in
`tests/tui_render/dialog_dispatch_coverage.inc` are either closed or still
named there with a reason.

Deferred work: any verb whose fix would need a new capability decision is
reported, not improvised.

### Milestone 007 — Contrast ratchet to zero

Class: capability

Objective: drive the M001 I5 baseline to zero across every render surface.

Dependencies: M001 (hard), M002, M003, M004, M005, M006 (hard — the chrome
work must have stopped moving).

Deliverable boundary: the render surfaces the chrome milestones did not
touch — transcript, sidebar, composer, file tree, status bar, change review —
plus the baseline file, which must end empty.

User or operator value: every surface is readable under every bundled theme.

Exit conditions: the ratchet baseline is empty and the guard fails if any
cell is added back; each surface's fixes are recorded with the theme and cell
that failed; any failure traced to a wrong derived token is reported to
`src/theme/` rather than worked around.

Deferred work: none. If a surface cannot reach zero without a redesign, it is
a stop condition, not a deferral.

### Milestone 008 — Frame plan and layout ownership

Class: infrastructure

Objective: extract the arithmetic half of `App::render` into a pure
`FramePlan` that returns named regions, and retire the ten `Option<Rect>`
fields from `App` into it.

Dependencies: M001 (hard), M002, M003 (soft).

Deliverable boundary: `src/tui/app/render.rs` and `src/tui/layout.rs`.

User or operator value: none directly. The value is that the carve-order
invariant — currently maintained by comment — becomes testable.

Exit conditions: `App::render` paints and no longer computes; `FramePlan` is
unit-tested for the ordering rule; the corpus is byte-identical;
`scripts/check_tui_render_cost.py` still passes unchanged.

Deferred work: the remaining `App` fields; §10 records the successor.

### Milestone 009 — List cursor and scroll follow

Class: infrastructure

Objective: one `ListCursor` owning clamped index and scroll-follow, adopted
where the semantics match.

Dependencies: M001 (hard).

Deliverable boundary: `src/tui/components/scroll.rs` and the list surfaces
that hand-roll it. `McpDialog` keeps its `browse_mode`/`action_mode`
dispatch and uses only the inner step — its `select_up` is a different
function and deduplicating it naively would change behaviour.

User or operator value: none directly. The value is 16 copies of one rule
becoming one, and 29 scroll fields becoming a shared type where they agree.

Exit conditions: the corpus is byte-identical; every converted surface keeps
its clamp or wrap policy, asserted by a test naming that policy; `McpDialog`
behaviour is unchanged and its test still passes.

Deferred work: surfaces whose scroll policy genuinely differs stay as they
are, recorded rather than forced.

### Milestone 010 — Slash dispatch tables

Class: infrastructure

Objective: split the 161-arm `dispatch_builtin_command` into family modules
behind a table, keeping exhaustiveness.

Dependencies: M001 (hard).

Deliverable boundary: `src/tui/app/mod.rs` and `src/tui/commands/`. Catalog
ownership is untouched (ADR-0052).

User or operator value: none directly. The value is that adding a command
stops meaning editing a 3,101-line function.

Exit conditions: the match stays exhaustive — a missing arm fails to compile;
every action's handler is reachable and tested; the corpus is byte-identical;
`scripts/check_command_catalog_ownership.py` passes unchanged.

Deferred work: `dispatch_tui_command` (259 async arms), per §3.

## 8. Cross-cutting requirements

### Storage and migration

No storage change. No migration. No `STORAGE_LAYOUT_VERSION` movement. If a
milestone appears to need one, it has left scope.

### Protocol and compatibility

No protocol change and no DTO change. `docs/tui-render-invariant-baseline.toml`
is a repository artifact, not a wire format, and it is not shipped.

### Security and authorization

No authority change. No milestone adds a key, a command or a surface. I4 must
never be satisfied by making a widget claim a key it does not own — the
ADR-0042 ownership contract governs, and the two key-authority guards are the
arbiter.

### Concurrency, cancellation, and recovery

No async boundary moves except in M010, where the converted dispatcher stays
synchronous. M008 must not change when a painter runs, only where the
arithmetic lives. `check_tui_render_cost.py` guards the render path's shape
against accidental extra work.

### Observability and audit

The existing `render_cost` counters are the observability seam and are not
extended by this roadmap. The contrast baseline doubles as the audit record of
what I5 found.

### Performance and resource use

No milestone is a performance milestone and **no wall-clock claim is made
anywhere in this roadmap**. The only performance obligation is negative: the
render-cost shape guard must keep passing, and no milestone may introduce a
second producer of a derived result to make a primitive convenient.

### Documentation and operations

Each milestone updates `architecture/tui.md` where the behaviour it describes
changes, and `architecture/theme.md` if a token's use changes. `M001` adds a
short section to `architecture/testing.md` for the invariant harness. The
comment budget must be banked in any milestone that splits a file.

## 9. Verification strategy

Three layers, in order of authority.

1. **The invariant harness (I1–I6).** Every milestone runs it. It is the only
   layer that answers "is this broken or unusable", which is the owner's bar.
2. **The render corpus.** A change detector. Neutral milestones must produce
   byte-identical output; change milestones must ship a reviewed diff. The
   corpus is never the acceptance criterion on its own, because it cannot tell
   a regression from an intended change — that judgement is the review.
3. **The existing guards.** `scripts/verify.sh quick` plus the 16
   `check_tui_*` guards and `check_no_hardcoded_selection_colours.py`. A
   green `quick` is necessary and not sufficient; AGENTS.md records that CI
   omits 14 guards `quick` runs, so CI alone is not evidence for any
   milestone here.

Focused commands per milestone are in each plan's §11. The repo's test-family
rule applies throughout: the root package's integration tests are families, so
a member is selected as `cargo test --test tui_render -- <member>`.

## 10. Risks and decision points

- **Promoting I1–I6 into the canonical specification.** The canonical
  documents are silent on presentation (see the authority note). A milestone
  must not amend `plans/000-`. Whether these invariants become canonical is
  an owner decision; until then they are subsystem-level.
- **A design-token layer needs an ADR.** Adding glyph, spacing and casing
  tokens would establish a compatibility contract for every bundled theme and
  for every user-authored theme file. That is an ADR threshold item, not a
  milestone.
- **I5 may fail widely on first run.** 50 themes × every surface is a large
  colour-pair space and `readable_on` is currently applied in one popup.
  M001 expects to *find* defects. The ratchet exists so finding them does not
  block the harness from landing — and so they cannot be forgotten.
- **M004 changes a visual constant every user sees.** The 60×50 modal is
  familiar. Content sizing is an improvement, but it is the milestone most
  likely to produce an unintended consequence at an unusual terminal size,
  which is why its exit conditions name three sizes explicitly.
- **M010 is the highest-effort, lowest-visible-value milestone.** It is
  registered because the 3,101-line dispatcher is the largest single
  maintainability liability after `App` itself, and because doing it later
  means doing it after more commands exist. It is also the most droppable: if
  the roadmap is cut short, M010 is the item to drop, and that leaves M001–M009
  coherent.
- **Splitting `App` beyond geometry is deferred, not solved.** §3 records it.
  A successor roadmap must own it, and this roadmap's §11 does not claim it.

## 11. Completion definition

The roadmap is complete when all of the following are true:

1. I1–I6 assert and pass for every dialog and panel surface at 20×6, 40×12,
   80×24 and 120×40.
2. The I5 ratchet baseline is empty, and the guard fails when any cell is
   added back.
3. Every panel and every dialog obtains its chrome from `PanelChrome` and
   `Section`; no render path hand-rolls a border and a title.
4. One hint vocabulary is in force, and `fit_parts` cannot drop an exit key.
5. No advertised key is unhandled.
6. `App::render` paints and does not compute, and `FramePlan` owns frame
   geometry including the ten retired `Option<Rect>` fields.
7. The slash dispatcher is table-driven and exhaustive.
8. `scripts/verify.sh quick` is green, every `check_tui_*` guard passes, and
   the render-cost shape guard is unchanged.

Nothing in that list is "the code compiles" or "the diff is large". A
milestone that only compiles is not complete.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 001 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/001-render-invariant-harness-and-contrast-ratchet.md` | `plans/closure/tui-modularity-and-render-invariants/001-status.md` | green `quick` and `cargo fmt --check` on the merged tree (the tree-wide comment budget is currently red) |
| 002 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/002-one-panel-chrome.md` | `plans/closure/tui-modularity-and-render-invariants/002-status.md` | two §15 evidence items, green `quick`/`fmt --check`, the 78 banked contrast rows owned by M007, and the reported theme focus-presentation defect |
| 003 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/003-one-dialog-frame.md` | `plans/closure/tui-modularity-and-render-invariants/003-status.md` | green `quick`/`fmt --check` and a green or re-banked comment ratchet on the merged tree; the per-dialog corpus diff note or a recorded replacement; acceptance criterion 1 amended for the `permission.rs` title-badge exception (39 of 40 surfaces) |
| 004 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/004-content-sized-modals.md` | `plans/closure/tui-modularity-and-render-invariants/004-status.md` | `scripts/verify.sh full`; uncommitted tree |
| 005 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/005-one-hint-vocabulary.md` | `plans/closure/tui-modularity-and-render-invariants/005-status.md`; corrective `plans/implementation/tui-modularity-and-render-invariants/012-hint-producer-convergence.md` | Criterion 1 restated (a *hint row*) and **1b added**: every hint row must be painted. Dead `context_hint` deleted; `/plugin` and `/keybind` footers now paint (the test was red first — neither painted any row); `/security-review` truncates at item boundaries; the tasks overlay's clipped hint fixed and mutation-proven. Residue: `/security-review` geometry, keybind's unrendered `Widget` impl, Export's `Esc back` |
| 006 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/006-advertised-keys-are-handled.md` | `plans/closure/tui-modularity-and-render-invariants/006-status.md`; corrective `plans/subsystems/tui-advertised-keys-corrective-addendum.md` → `plans/implementation/tui-advertised-keys-corrective/001-i4-baseline-to-its-irreducible-residue.md`, `…/002-structured-pairs-from-the-producer.md`; corrective C001 record `plans/closure/tui-advertised-keys-corrective/001-status.md` | Corrective landed: the I4 residue is 4 reasoned rows, read structurally from the producer with the painted scan as a cross-check. Remaining is fixture work (the parser widening) plus `full` and a commit |
| 007 | conditionally closed — `[contrast]` empty | `plans/implementation/tui-modularity-and-render-invariants/007-contrast-ratchet-to-zero.md` | `plans/closure/tui-modularity-and-render-invariants/007-status.md` | `contrast = 0` across 50 themes. The mapping owned the fix: the projection gates five tokens and the secondary accent against both fills; the third-party theme files are byte-identical to HEAD. Remaining: `full` on the merged tree and a commit |
| 008 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/008-frame-plan-and-layout-ownership.md` | `plans/closure/tui-modularity-and-render-invariants/008-status.md` | `sidebar_area`/`dialog_area` remain, blocked by `src/tui/commands/**`; `full`; uncommitted tree |
| 009 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/009-list-cursor-and-scroll-follow.md` | `plans/closure/tui-modularity-and-render-invariants/009-status.md` | `project_picker` did not adopt; 16 files still define `select_up`/`select_down`; the `architecture/tui.md` list-state contract is unwritten; `full`; uncommitted tree |
| 010 | conditionally closed | `plans/implementation/tui-modularity-and-render-invariants/010-slash-dispatch-tables.md` | `plans/closure/tui-modularity-and-render-invariants/010-status.md` | `full`; uncommitted tree. The plan's `check_command_argument_contract.py` "passes unmodified" claim was false and the guard has been re-pointed |

**The remaining conditions are not this roadmap's to discharge alone.** The
`full`-and-a-commit blocker repeated in rows 004–010, the residue named in rows
005 and 006, and the four readings the records cannot settle are owned by the
**cross-line** corrective
`plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md`
(registered `conditionally closed` in `plans/registry.md`, closure
`plans/closure/five-line-closure-corrective/001-status.md`), which carries them
for this line,
the helix line, the harness line and the sandbox audit together. Two items this
table previously listed as residue are **dropped rather than deferred**, with the
reason recorded there: `/security-review`'s geometry (eight verbs need 113–126
inner columns, i.e. a ≥192-column terminal against a 90 % dialog cap) and the I4
parser widening (its rows are fixture gaps, not defects).
