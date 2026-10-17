# TUI Panel Surface Convergence Corrective Milestone M003 — Theme sourcing and shared chrome geometry

Status: ready for handoff

Approved into planning after M002b closed. Three things the roadmap's M003 entry did not
know when it was written, all re-derived against the current tree and recorded here rather
than copied forward:

- **The theme-cache defect is ten boxes wide, not five, and `mcp` is its worst instance.**
  §4.9 names `plugin`, `review`, `security_review`, `diff` and `ui_node` and asserts that
  "`dialogs/mcp.rs` already demonstrates the correct pattern". Measured at this baseline,
  `mcp`'s shared body routine reads its `set_theme` cache 44 times and the live theme
  **zero** times — the exact inverse of the claim. `ui_node` has no `render_in_panel` at
  all and was never a docked box. Ten dockable boxes read a stale cache; four are clean.
- **The roadmap's colour-literal counts are right; its `centered_rect` claim is not.**
  `plan` 6 and `mcp` 2 hold exactly. The two `centered_rect` definitions are *not*
  "byte-identical" — the parameter is named `area` in one and `r` in the other, and one
  qualifies `Direction` as `ratatui::layout::Direction`. The `Layout::…split(…)` figures
  also drifted from "63 across 24 files" to 56 `Layout::` sites across 31 files.
- **The selection-foreground token cannot be *ingested* from Halloy, only *derived*.**
  The Halloy format does declare a `foreground_selected` slot, and **0 of the 50 bundled
  Halloy themes define it** (all 50 define `background_selected` and `buffer.selection`).
  Sourcing from it would yield `None` for every real theme. The in-repo precedent for what
  to do instead is `derive_selection_dim`, which derives a token from two Halloy colours
  every theme already carries and therefore imposes no requirement on theme authors.

Repository baseline: `f8286a2849660d1e386a61ceaf663906451961d0`
(`docs(plans): record the tui-panel M001 commit SHA`), **plus M002a and M002b landed**.
All counts in §3 were re-measured against that working tree, not inherited from the
roadmap.

Source roadmap:

- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` §7 M003, §4.9
  colour sourcing, §4.10 chrome geometry, §6 target architecture, §9 cross-cutting
  requirements

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs: none required. No identity, ownership, protocol, storage layout,
persistence, or authorization change. Theme *values* change; the theme **schema gains two
optional-derived colour tokens** that are computed, never user-supplied as an authority.
`SidePanelId` variants, `config_key` spellings and persisted bindings are untouched.

Depends on: `003-dockable-box-chrome-and-keymap-adoption.md` (M002b) closed, which is
recorded in `plans/closure/tui-panel-surface-convergence-corrective/003-status.md`. M003
writes its colour reads against the **post-adoption** shape — one shared body routine per
widget, parameterised by `Chrome::{Owned,Hosted}` — because that is the shape the roadmap
deferred it until.

Primary class: polish (with one capability-visible consequence — a docked box stops
rendering stale colours after a theme change)

## 1. Objective

Make a dockable box's colours a function of the theme in force at render time rather than
the theme that happened to be current when the box was opened, and stop the selection rows
of `plan` and `mcp` hardcoding a foreground that is unreadable on a light theme.

Concretely: ten dockable boxes stop reading a `set_theme()` cache and read the theme the
`Component` seam already hands them; eight `Color::White` selection literals become derived
theme tokens; the two `centered_rect` definitions become one.

## 2. Why this milestone is ready

- Hard dependency M002b is closed. Its closure record is the authority for the
  `Chrome::{Owned,Hosted}` seam, the per-widget `keymap_hint` strings, and the
  viewport-ownership rule (render-derived for `files`/`diff`, externally-owned for
  `tree`/`theme`) — none of which M003 may reshape.
- The interface M003 depends on is not merely stable, it is **already carrying the thing
  M003 needs**. `render_hosted` passes the live `&Arc<Theme>` into
  `widget.render_in_panel(frame, frame.area(), theme)` (`src/tui/panels/mod.rs`), and both
  modal call sites pass `&self.ui_state.theme` (`src/tui/app/render.rs`). Every
  `Component` therefore already receives the live theme on both paths. M003 adds **no
  plumbing**; it changes what ten bodies read out of the parameter they were already given.
- The theme layer has a working precedent for a derived token
  (`derive_selection_dim`), the contrast math (`Rgb::relative_luminance`,
  `contrast_ratio`, `is_dark`), and a diagnostic channel with a threshold already declared
  for exactly this case (`SELECTION_TEXT_RATIO` in `src/theme/validate.rs`).

## 3. Current implementation evidence

### 3.1 How a docked box goes stale

`set_theme` is called only when a box is **opened** — from `src/tui/app/modal.rs`,
`src/tui/app/plugin_ui.rs`, `src/tui/commands/*`, `src/tui/runtime/command_dispatch.rs`,
and `src/tui/panels/session.rs`. It is never called on a theme change. Seven sites
reassign the live theme (`src/tui/app/input.rs` — four sites; `src/tui/app/mod.rs` — three
sites) and none of them re-pushes it to an open dialog.

So for any box whose render body reads `self.theme`, the rendered colours are frozen at
open time. The rest of the UI follows the new theme. The **mixed** cases are worse than the
uniform ones: a box that reads the live theme in some spans and the cache in others
repaints *incoherently* rather than uniformly stale.

### 3.2 The measured set

Counted mechanically over each widget's shared body routine — cache reads are `self.theme.`
occurrences after the body; live reads are bare `theme.` occurrences that are not
`self.theme.`:

| Widget | cache reads | live reads | body signature | verdict |
|---|---|---|---|---|
| `research` | 46 | 0 | takes `theme`, ignores it | cache-only |
| `mcp` | 44 | 0 | no theme param | cache-only |
| `security_review` | 40 | 0 | no theme param | cache-only |
| `keybind` | 21 | 22 | takes `theme` | mixed |
| `review` | 17 | 0 | `_theme`, unused | cache-only |
| `tree` | 11 | 13 | takes `theme` | mixed |
| `plan` | 7 | 12 | takes `theme` | mixed |
| `plugin` | 6 | 0 | no theme param | cache-only |
| `theme` | 4 | 13 | takes `theme` | mixed |
| `diff` | 0 in the dialog | — | no theme param | stale via sub-widget |
| `task_view`, `files`, `panel_picker`, `info` | 0 | — | — | clean |

`diff`'s dialog layer holds no cache of its own but delegates to `DiffViewer`
(`src/tui/components/diff.rs`), which holds 17 `self.theme.` reads and is refreshed only
by the same open-time `set_theme`. It is stale and must be counted. `review` carries a
second sub-widget with the same problem.

**Ten stale, four clean.** The four clean boxes are the control: they prove the seam
delivers a live theme and that reading it is all that is required.

> **Corrected 2026-10-17 during implementation. "Ten stale, four clean" is wrong, and
> the table above counts the wrong thing.** Read against a render-level test rather than
> a source count, the set is **four stale, and six that were already correct**:
>
> - **`mcp` (44) and `research` (46) were never stale.** Both re-assign
>   `self.theme = Arc::clone(theme)` *inside their own render body*
>   (`McpDialog::render`/`render_in_panel`, `ResearchBrowserDialog::render_body`), so
>   the cache is overwritten on the frame that reads it. They are correct by accident of
>   a line that looks exactly like the defect it is meant to be — which is why a source
>   count ranks them worst and a rendered-buffer test does not. `mcp` is **not** "the
>   single worst instance"; it is one of only two that are safe.
> - **`keybind`, `tree`, `plan` and `theme` were not "mixed".** Their cache reads all
>   sit in the **dead legacy** paths — `impl Widget for &XDialog`, plus `plan`'s uncalled
>   third `render` — not in the shared body. Their live bodies already read the seam's
>   theme. For `theme` the two remaining `self.theme` reads are `self.theme.id`, which is
>   semantic (which theme is active), not colour.
> - **`review` has no second sub-widget with the same problem.** Its 17 reads are all in
>   `render_body`; the diff view it renders builds its colours from the passed theme.
> - The genuinely stale set is **`review`, `plugin`, `security_review`, and `diff`/
>   `DiffViewer`**.
>
> `tests/tui_theme_source_of_truth.rs` is what settles this, and it exists because this
> table was wrong: it renders each box with theme A and then theme B and inspects the
> cells. `mcp` and `research` pass at the pre-change baseline; the four above fail.
> W2 therefore converted five dialogs — four because they were stale, `mcp` and
> `research` because a cache that a render method overwrites is a hazard even when it is
> currently harmless.

### 3.3 The `Color::White` selection literals

Eight real style literals, all a selected-row foreground over a themed fill:

- `plan` — 6, every one `fg(Color::White).bg(…primary)`, duplicated across its three
  render paths
- `mcp` — 2: one `fg(Color::White).bg(…selection)`, one `fg(Color::White).bg(…primary)`
- `confirm.rs` — 1, same `…primary` shape
- `run_detail.rs` — 1

`src/tui/components/dialogs/theme.rs` also contains a `Color::White` occurrence, but it is
a **theme-name mapping** (`Color::White => "ffffff"`), not a style literal. It is not a
defect and must not be "fixed".

`Theme::selection_style()` has the same class of problem in a different shape: it paints
`fg(self.primary).bg(self.selection)`, which is accent-on-selection rather than
foreground-on-accent. It is in scope to route through the new tokens.

### 3.4 The theme layer and what Halloy actually provides

`src/theme/` is the authoritative theme system: `SemanticTheme` (`schema.rs`) is the
canonical frontend-neutral value, `native.rs` imports codegg TOML, `halloy.rs` imports
Halloy TOML, `validate.rs` diagnoses, and `target.rs` projects into `tui::theme::Theme`.
All 50 themes in `assets/themes/halloy/` are embedded via `include_str!` in `registry.rs`.

Measured across those 50 bundled files:

| Halloy key | Defining themes |
|---|---|
| `buttons.*.background_selected` | 50 / 50 |
| `buffer.selection` | 50 / 50 |
| `buttons.*.foreground_selected` | **0 / 50** |

`HalloyButtonSection::foreground_selected` is declared and never consulted. It is schema
surface no theme author exercises. Deriving from it would ship a token that is `None`
everywhere.

What does work, and is already the established pattern: `derive_selection_dim` computes
`ui.selection_dim` from `ui.selection` and `base.background`, and the `accent_primary`
resolution walks a candidate chain of Halloy fields, contrast-tests each against the
background, and rejects any candidate too close to the background to read as foreground —
with a documented reason at `src/theme/halloy.rs` for why a widget *fill* cannot serve as a
*foreground*. M003's derivation follows that precedent exactly.

`codegg-config` exposes no theme colour keys, so the configuration schema is untouched.
The native theme file format (`native.rs`) does carry optional `ui.*` keys and is the
place an explicit override belongs if one is offered at all.

### 3.5 Chrome geometry

Two `centered_rect` definitions — `src/tui/app/mod.rs` and
`src/tui/components/dialogs/run_detail.rs`. Semantically identical; textually not. There is
no dialog chrome or box-geometry helper anywhere. `src/tui/layout.rs` holds *frame*
geometry (`PanelRegions`, `split_column`, `session_layout`, `viewport_with_scrollbar`) and
is the right conceptual home, but box geometry is a distinct scope and there is no seam for
it yet.

### 3.6 Legacy render paths (context only — explicitly not M003's work)

There are **ten** legacy `impl Widget for &XDialog` impls (`connect`, `goto`, `theme`,
`import`, `plan`, `tree`, `keybind`, `share`, `diff`, `template`) plus the public
`render(&self, frame, area)` on `diff` and `plan`. Verified: **none has a production
caller**; all ten dispatch exclusively through `Component`. Eight have no caller at all,
including tests. `connect` and `share` have **test-only** callers
(`src/tui/components/dialogs/connect.rs` and `tests/connect_tui_restoration.rs`, and
`src/tui/components/dialogs/share.rs`), in tests whose purpose is to compare the Widget
path against the Component path.

This corrects the M002b closure record's §6 statement, which recorded "five legacy
`impl Widget for &XDialog` bodies" and "no caller in `src/` or `tests/`". The count is ten
and two are not caller-free. Deleting the eight free ones is trivial; `connect` and `share`
require their comparison tests to be rewritten first, which is why this is deferred rather
than absorbed. See §5 and §14.

## 4. Invariants that must not regress

- **Modal rendering stays byte-identical** for every touched widget, except where a colour
  token change is the point of the milestone. M003 changes *what colour a span resolves
  to*; it must not change layout, wrapping, row counts, scroll clamps, or chrome.
- **The `Chrome::{Owned,Hosted}` seam is not reshaped.** `render` and `render_in_panel`
  must continue to delegate to one shared body routine per widget, so the viewport and
  clamp cannot diverge between paths. M003 changes the routine's *theme source*, never its
  structure.
- **No box may newly advertise a key**, and no existing `keymap_hint` string changes.
- **No new Halloy key is required.** Any theme that ingests today must ingest after this
  milestone, unchanged. A theme author must never be asked to add a field for M003.
- **All 50 bundled themes keep their current ids, names, and every colour token they have
  today** except the selection foregrounds, which are new and previously hardcoded.
- **No theme value is persisted as an authority.** The new tokens are derived at import
  time; a native TOML file may set them explicitly, but nothing reads them back as a
  stored user preference that could drift.
- `SidePanelId` variants, `config_key` spellings, persisted bindings, panel placement and
  sizing, protocol, storage layout, identity, and authorization semantics are unchanged.
- **No `Color::White` is reintroduced** as a selection foreground anywhere, and the
  theme-name mapping in `dialogs/theme.rs` is left alone.

## 5. Scope

### In scope

- Retire the `set_theme()` colour cache from the ten stale boxes so both render paths read
  the theme the seam passes; delete each `set_theme` that this makes dead, and its call
  sites.
- Add two **derived** selection-foreground tokens — one readable on `ui.selection`, one
  readable on `ui.accent_primary` — computed in `src/theme/halloy.rs` from colours every
  Halloy theme already carries, projected through `target.rs`, defaulted in the native
  importer, and diagnosed by `validate.rs`.
- Replace the eight `Color::White` style literals with those tokens.
- De-duplicate `centered_rect` onto one definition.

### Explicitly out of scope

- **Deleting the legacy `impl Widget for &XDialog` impls.** Confirmed dead of production
  callers, but `connect` and `share` have test-only comparison tests that must be rewritten
  first, and that rewrite is its own bounded unit with its own evidence. Recommended as a
  separate cleanup milestone; see §14.
- **A general shared chrome/geometry helper.** The roadmap's M003 entry bundles "give the
  box set one shared chrome/geometry helper" with the de-duplication. That is a design
  task with no seam and no failing test to define it, and it would run directly against the
  chrome contract M002b just established. M003 takes only the mechanical half
  (`centered_rect`). The helper should be its own milestone, written against a settled
  post-adoption shape. This is a deliberate narrowing of the roadmap entry and is recorded
  as such in the roadmap's M003 section.
- The pre-existing scroll and viewport defects M002b recorded and deliberately did not fix
  (`review`'s inert diff scroll, the reversed `k`/`j` in `review`/`research`, and
  `tree`/`theme`'s never-set `visible_height`). Those are behaviour changes, not colour.
- Any change to how the theme *picker* presents themes, to theme discovery, or to theme
  resolution order.
- Making the new tokens settable in `codegg.example.jsonc` or any config surface.

## 6. Required production changes

### Core/domain

None. No `codegg-core` type changes; the theme schema is frontend-neutral and lives in the
root crate's `src/theme/`.

### Storage and migrations

None. No migration, no layout version change. Theme *files* on disk gain optional keys; no
stored session, project, or job record changes.

### Protocol and DTOs

None. No `TuiMsg` variant, no protocol field, no wire change. The two new tokens travel
only in-process inside `tui::theme::Theme`.

### Runtime and concurrency

The ten bodies switch from reading a stored `Arc<Theme>` field to reading the borrowed
parameter they already receive. The borrow must not outlive the frame — no storing the
`&Arc<Theme>` anywhere, and no `Clone`-ing it into a longer-lived field as a shortcut,
which would reintroduce the cache under a new name. `set_theme` removals must not leave a
call site compiling against a deleted method.

There is no new concurrency surface: this is a render-path read, and the render path is
single-threaded per frame.

### Frontend or operator surface

- `src/theme/schema.rs`: two optional-derived `Rgb` fields on `UiColors`. They are
  **derived, not required** — a `SemanticTheme` that never passes through an importer must
  still construct, so give them a default or construct them in one place.
- `src/theme/halloy.rs`: two derivation functions beside `derive_selection_dim`, each
  picking whichever of `base.foreground` / `base.background` has the higher
  `contrast_ratio` against its target background. Document, as
  `derive_selection_dim` and `accent_primary` already do, why the Halloy format cannot
  supply this and why that is acceptable.
- `src/theme/native.rs`: optional explicit keys for the native TOML format, defaulting to
  the same derivation, so a hand-written native theme can override without being forced to.
- `src/theme/validate.rs`: diagnostics for both new tokens, using the existing
  `check()` helper and the existing `SELECTION_TEXT_RATIO` threshold where it applies.
- `src/theme/target.rs`: project both into `tui::theme::Theme`.
- `src/tui/theme.rs`: two fields on `Theme`, two on `ThemeData`, two in `to_theme`, and
  **all 32 `ThemeData` literals**. Route `selection_style()` and `highlight_style()`
  through the new tokens.
- The ten stale dialog bodies: read the parameter. Where a widget's own `render` already
  receives the live theme (all `Component::render` impls do), the shared body takes it and
  both paths pass theirs.
- `diff`/`review`: thread the theme into `DiffViewer`/`DiffView` rather than through a
  stored field.
- The eight `Color::White` style literals become `.fg(theme.<token>)`.
- `centered_rect`: one definition, one call-site update.

### Security and authorization

None. No secret, credential, or path handling is touched. Theme files are local
configuration, already trusted as input by both importers.

### Documentation and static guards

- `docs/themes.md` and `architecture/theme.md`: the two derived tokens, why they are
  derived rather than ingested, and the `foreground_selected` measurement.
- `architecture/tui.md`: the corrected stale-box count and the rule that a render path
  reads the theme the seam passes.
- A static guard in the house `scripts/check_*.py` style: no `Color::White` as a style
  literal under `src/tui/components/dialogs/`, permitting the theme-name mapping in
  `dialogs/theme.rs`. Model it on `check_no_hardcoded_models.py`, which is the existing
  "no hardcoded X" guard, and wire it into `scripts/verify.sh quick`.
- Correct the addendum's §4.9/§4.10 counts and this plan's predecessor's §6 claim.

## 7. Ordered work packages

### W1 — Baseline-failing tests, including a real theme change

Intent: establish the acceptance signal before any production edit.

Required changes: add an integration test that constructs a box, renders it hosted,
**changes the theme**, re-renders the same hosted rect, and asserts the buffer's cell
styles follow — not string reachability, and not a static source check. Add an exhaustive
sweep asserting, for **all 50 bundled themes**, that every derived token meets its
contrast threshold against its target background.

Acceptance evidence: the staleness tests fail at this baseline with a message naming the
box and the stale colour. Record the exact count that fail and the count that are guards.

### W2 — Retire the caches

Intent: ten boxes stop reading `self.theme` in a render path.

Required changes: thread the live theme into each of the ten shared body routines,
including into `DiffViewer`/`DiffView` for `diff` and `review`. Delete each `set_theme`
that becomes dead, and every call site.

Acceptance evidence: W1's tests pass; the W1 sweep is green; a grep for `self.theme.`
inside a render path returns only the four intentionally-clean boxes and any non-render
helper.

### W3 — The two derived tokens

Intent: a selection foreground that is readable on any of the 50 themes, with no new Halloy
key.

Required changes: schema fields, the two derivations in `halloy.rs`, optional native
keys, `validate.rs` diagnostics, the `target.rs` projection, and the `Theme`/`ThemeData`
plumbing including all 32 literals.

Acceptance evidence: all 50 bundled themes ingest and every derived token clears its
threshold; the `foreground_selected`-absent measurement is recorded in the closure so the
derivation is not later "simplified" into a dead read.

### W4 — Replace the hardcoded literals

Intent: no selection foreground is `Color::White`.

Required changes: the eight style literals; route `selection_style()` through the tokens.
Leave `dialogs/theme.rs`'s theme-name mapping alone.

Acceptance evidence: the new static guard passes; a render test asserts the selected row's
foreground resolves to a token, on both a dark and a light theme.

### W5 — `centered_rect`

Intent: one definition.

Required changes: collapse to one, update the call site, keep behaviour byte-identical.

Acceptance evidence: existing modal-render tests unchanged and green.

### W6 — Docs, guards, and corrections

Intent: the recorded facts match the source.

Required changes: the documentation and guard work in §6, plus correcting the roadmap's
§4.9/§4.10 counts and the M002b closure record's §6 legacy-path claim.

Acceptance evidence: `scripts/verify.sh quick` green including the new guard; corrections
landed as text edits, verifiable by reading them.

## 8. Failure, cancellation, restart, and contention semantics

None of this is a durable or async path. The relevant semantics:

- **A partially-derived theme must still render.** If either derivation cannot produce a
  usable colour, fall back to the existing behaviour for that token rather than leaving
  the widget with an unset colour. A theme that fails to derive must never panic or render
  a default-colour row.
- **A rejected candidate is a diagnostic, not a hard failure.** Follow `accent_primary`:
  reject the low-contrast candidate, emit a `ThemeDiagnostic`, keep the theme.
- **Import failure is unchanged.** `parse_halloy_theme` keeps returning
  `Result<(SemanticTheme, Vec<ThemeDiagnostic>), ThemeError>`; the derivation cannot turn a
  previously-valid theme into an error.
- **Restart/contention:** not applicable. There is no new persistence and no new shared
  state. The 32 `ThemeData` literals are compile-time constants.

## 9. Compatibility and migration

- **Themes on disk:** unchanged and backward compatible. A native theme file that omits
  the new optional keys derives them. No file needs editing.
- **Halloy themes:** unchanged and fully compatible. This is the hard constraint — no new
  required key, so every theme that ingests today ingests after this milestone, and any
  theme authored against Halloy upstream ingests without modification.
- **Persisted session/project/job state:** unchanged. Nothing new is persisted, so there is
  no migration and no `STORAGE_LAYOUT_VERSION` bump.
- **`codegg.example.jsonc`:** unchanged; theme colours are not a config surface.
- **Removal criteria for the legacy render paths:** deliberately deferred (§5, §14), not
  removed here.
- **Theme file round-trip:** if the native importer can export, the two derived tokens must
  either export explicitly or be documented as derived-and-not-exported. Decide during
  W3 and record which.

## 10. Required tests

### Focused unit tests

- Each derivation function: dark background, light background, a selection colour close to
  its background, and the degenerate `selection == background` case.
- Each of the ten retirements: the body reads the passed theme. Assert this
  **behaviourally** — render with theme A, render with theme B, assert the buffer's cell
  styles differ and match B.
- `target.rs` projection: both tokens cross the boundary.

### Integration tests

- The theme-change staleness test from W1, for each of the ten boxes.
- The all-50-theme contrast sweep.
- Selected-row foreground on a light theme and a dark theme, for `plan` and `mcp`.

### Restart and recovery tests

Not applicable — no durable path. Say so explicitly in the closure record rather than
omitting the heading.

### Contention and cancellation tests

Not applicable — the render path is single-threaded per frame and no new shared state
exists.

### Security and negative tests

- A theme whose selection colour equals its background must still produce a usable token
  and must not panic.
- A malformed native theme file must still fail to parse exactly as before.

### Migration and compatibility tests

- All 50 bundled themes parse before and after, with identical `id`, `name`, and every
  pre-existing token. This is the regression gate for "no Halloy theme was asked for a new
  field".

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg --lib theme::
cargo test -p codegg --lib dialogs::tree dialogs::plan dialogs::mcp
cargo test --test tui_panel_host_chrome

# the new static guard
python3 scripts/check_no_hardcoded_selection_colours.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# the gate
scripts/verify.sh quick
```

Never `--all-features`. Before closure, `scripts/verify.sh full` or an equivalent
`--no-fail-fast` workspace sweep, with every non-passing test name attributed to a recorded
baseline. Do not claim any command in the closure record that was not actually run, and
label local runs as local — CI is a strict subset of `quick` and a green CI run does not
imply a green local `quick`.

## 12. Documentation updates

- `docs/themes.md` — the two derived tokens; why they are derived and not ingested, with
  the `foreground_selected` 0-of-50 measurement.
- `architecture/theme.md` — the derivation, the `validate.rs` diagnostics, the
  `target.rs` projection, and the rule that a token absent from the upstream format is
  derived rather than required.
- `architecture/tui.md` — the corrected stale-box count (ten, not five) and the rule that a
  render path reads the theme the seam passes.
- `docs/tui.md` — no user-facing change is expected; update only if a selection row's
  appearance is documented.
- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` — correct §4.9
  and §4.10; mark M003 approved-into-planning; update §13.
- `plans/closure/tui-panel-surface-convergence-corrective/003-status.md` — correct the §6
  legacy-path count and the "no caller in `src/` or `tests/`" claim.
- `plans/registry.md` — register this milestone.
- `.opencode/skills/tui/SKILL.md` — the rule that a new adapter reads the seam's theme.

## 13. Acceptance criteria

- A docked box re-renders with the new theme's colours after the user changes theme, for
  all ten affected boxes, with no other visible change.
- No `set_theme` colour cache remains in any render path; every render path reads the theme
  the `Component` seam passes.
- No selection foreground is `Color::White`. Each resolves to a derived token that clears
  its contrast threshold on every one of the 50 bundled themes.
- All 50 bundled Halloy themes ingest with no new required key and with every
  pre-existing token byte-identical.
- `centered_rect` has exactly one definition, and modal rendering is byte-identical for
  every widget except where a selection colour changed by design.
- `scripts/verify.sh quick` is green, including the new guard.
- `docs/themes.md` and `architecture/tui.md` match shipped behavior.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **Writing the shared chrome/geometry helper would require changing the
  `Chrome::{Owned,Hosted}` seam or any widget's body structure.** That is M002b's contract.
  Report and leave it for its own milestone.
- **Deleting a legacy `impl Widget for &XDialog` would require rewriting a test whose
  purpose is comparing the Widget and Component paths** without a replacement signal.
  Report the specific test and do not delete the impl.
- **Retiring a cache would require storing the borrowed theme anywhere.** A cache under a
  new name is the defect, not the fix.
- **A Halloy key turns out to be required** to produce a correct token — that would break
  "any Halloy theme ingests unchanged" and needs a decision, not a workaround.
- **The 32 `ThemeData` literals or the native export round-trip cannot be settled
  coherently** — in particular if native export would persist a derived token as an
  authority.
- **Modal rendering changes for any reason other than a selection colour.** That means a
  structural edit crept in.
- Repository evidence contradicts a canonical invariant, or the work would expand into the
  theme subsystem's own roadmap.

## 15. Closure evidence required

- W1's baseline failure count, recorded before any production edit, with verbatim messages.
- The post-change sweep proving all 50 themes ingest with identical pre-existing tokens.
- The contrast measurements for both derived tokens across all 50 themes.
- Command-by-command verification results, labelled local vs CI, with unrun commands named
  as unrun.
- Attribution of every non-passing test to a recorded baseline.
- A corrected record of §3.2's stale-box set and §3.6's legacy-path set, stating plainly
  where this plan corrected its predecessor.
- Explicit statements that restart and contention are not applicable, rather than silence.
- A registry update and an explicit statement of what, if anything, this unblocks.

## 16. Handoff notes

- **Do not "simplify" the derivation into a read of `foreground_selected`.** It is declared
  in the Halloy schema and absent from all 50 bundled themes, so such a read is dead code
  that looks correct. Record the measurement in the closure so the next reader does not
  repeat it.
- **`mcp` is the trap.** The roadmap calls it the exemplar of the correct pattern; it is
  the single worst instance of the defect (44 cache reads, zero live reads). Verify, do not
  trust the roadmap.
- **The `Color::White` in `dialogs/theme.rs` is a theme-name mapping, not a style.** The
  static guard must permit it, or it will fail on a non-defect.
- **The 32 `ThemeData` literals are mechanical but voluminous.** A partial edit is a
  compile error, so this is safe, but budget the pass.
- **Watch the borrow.** The fix is to read the parameter. Cloning the `Arc` into a field to
  satisfy the borrow checker reintroduces the cache.
- `verify.sh full` has a pre-existing failure under fail-fast
  (`session_create_through_daemon`, `InvalidRootPath("/tmp")`). A `--no-fail-fast`
  workspace sweep is the equivalent evidence; attribute every non-passing name to the
  recorded M001 baseline rather than treating it as new.
- This milestone changes colours on screen. Expect the render baselines in
  `tests/tui_panel_host_chrome.rs` and `tests/tui_render.rs` to need reviewed updates —
  and treat a *chrome* assertion changing as a signal that a structural edit crept in.