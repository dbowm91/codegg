# TUI Modularity and Render Invariants Milestone 001 — Render invariant harness and contrast ratchet

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-001--render-invariant-harness-and-contrast-ratchet`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the surface
  inventory the harness must cover.
- `plans/000-long-term-specification.md#29-system-invariants` — no invariant
  here may weaken one recorded there.

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
  — the harness reads rendered cells only; it never reads or mutates document
  text.
- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — the
  I4 baseline records advertised keys without changing which stage owns them.

Primary class: invariant

## 1. Objective

Make the six presentation invariants (I1–I6) executable in one place, and
record the failures that already exist as a checked-in ratchet so later
milestones can be measured against a fixed start.

Three invariants assert and pass at the end of this milestone (I1, I2, I3a).
Three cannot pass yet, because the defects they detect are real and belong to
later milestones (I4, I5, I6); those are recorded as baselines that may shrink
and may not grow. **No production render path changes in this milestone.**

## 2. Why this milestone is ready

No dependencies. This is the roadmap's root milestone and the only one that is
`ready for handoff` at authoring time.

The harness builds on infrastructure that already exists and is in use:

- `ratatui::backend::TestBackend` is already used at 25+ sites in
  `src/tui/**` and `tests/tui_render/**`.
- `tests/tui_render/tui_info_dialog_panel_chrome.inc` already renders a hosted
  column through the production path into a `Buffer` and reads it back as
  rows — the shape this milestone generalises.
- `Color::contrast_ratio` exists at `src/theme/color.rs:88` and
  `MIN_DERIVED_TEXT_RATIO` is exported from `codegg::theme::schema`, so I5
  needs no new threshold and no new dependency.
- `plans/registry.md` records the roadmap as `proposed` and this plan as
  `ready for handoff`.

## 3. Current implementation evidence

At baseline `7642b9e38`:

- `tests/tui_render` contains 60 members. The family is selected as
  `cargo test --test tui_render -- <member>` because the root package's
  integration tests are families.
- `tests/tui_render/tui_info_dialog_panel_chrome.inc:57` defines
  `hosted_rows(dialog, panel, area) -> Vec<String>` and reads only
  `cell.symbol()`. A colour regression therefore passes it. This is the
  precise gap the capture helper closes.
- `tests/tui_render/tui_agents_panel.inc:77` asserts
  `app.panel_host.is_focused()` and never reads a rendered cell, so focus
  *appearance* is untested.
- `tests/tui_render/theme_selection_tokens.inc:82` asserts contrast on
  **derived tokens**, not on rendered cells. I5 extends that bar from the
  token to the frame.
- `src/tui/app/render.rs:101` defines `readable_on(...)`, applied only in the
  completion popup.
- `src/tui/app/mod.rs:58` defines `fit_parts`, which drops whole parts to fit
  a budget. It has **zero tests** in the tree.
- `src/tui/components/component.rs:215` defines `Component` with
  `render(frame, area, theme)`, `render_in_panel(...)`, `keymap_hint()` and
  `docks_as_viewer()`; `src/tui/panels/mod.rs:78` defines `render_hosted`,
  which paints host chrome and then calls `render_in_panel`.
- `src/tui/components/component.rs:296` defines `centered_rect`, already
  documented as the single definition of that shape.
- `tests/tui_render/dialog_dispatch_coverage.inc:14` records three live
  dispatch gaps, including the `/shell-show` verbs. That record is the seed of
  the I4 baseline.
- `docs/comment-budget.toml` already establishes the ratchet pattern this
  milestone copies: a recorded ceiling that may fall and may not rise,
  enforced by a guard with a manifest a reviewer can see.

## 4. Invariants that must not regress

- **No production render path changes.** The only files this milestone may add
  or edit outside `tests/` and `docs/` are none. If a production change looks
  necessary to make an invariant assertable, that is a stop condition (§14),
  not a scope expansion.
- **The comment corpus ratchet holds.** `docs/comment-budget.toml` global
  ceiling is 134,015 comment lines and no file may exceed its recorded
  ceiling. New test files carry at most `unlisted_max_comment_lines = 20`
  comment lines unless they are added to the manifest in the same change.
- **The render-cost shape guard is unchanged.**
  `scripts/check_tui_render_cost.py` must pass without edit.
- **No new dependency.** The harness uses `ratatui`, `crossterm` and
  `codegg::theme` only.
- **No wall-clock assertion anywhere.** I5 measures contrast ratios, not time.
- **The I5 bar is the shipped one.** `MIN_DERIVED_TEXT_RATIO` is used as
  exported; no local constant redefines it.

## 5. Scope

### In scope

- A capture helper that renders a surface and returns every cell with its
  symbol, foreground, background and modifiers.
- Assertions for I1, I2 and I3a across every dialog and panel surface.
- Baseline recording for I4, I5 and I6, with each recorded failure named by
  surface and, for I5, by theme and cell.
- A guard that fails when a baseline grows, wired into
  `scripts/verify.sh quick` and CI, with `--self-test` proving non-vacuity.
- A manifest at `docs/tui-render-invariant-baseline.toml`.
- A section in `architecture/testing.md` describing the harness.

### Explicitly out of scope

- Fixing any recorded failure. I4 belongs to M006, I5 to M007, I6 to M005.
- I3b ("the focus difference is a border or title style change, uniformly").
  It cannot hold before M002 and is added there.
- Any change to `src/tui/**`.
- Any change to `src/theme/**`.
- A wall-clock or allocation benchmark.
- Golden-image files. The corpus is text in the repository, not a binary
  artifact.

## 6. Required production changes

None. This milestone is test and documentation only. The subsections below
record that explicitly so a later reader does not look for the missing change.

### Core/domain

None.

### Storage and migrations

None. `STORAGE_LAYOUT_VERSION` does not move.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The harness renders synchronously in tests.

### Frontend or operator surface

None. The harness observes the existing surface.

### Security and authorization

None.

### Documentation and static guards

- `scripts/check_tui_render_invariant_baseline.py` — new guard. Rules: the
  baseline file parses; every recorded failure still reproduces; the recorded
  set is a subset of what the harness currently measures; a recorded entry
  that no longer fails must be removed (an unbanked reduction is an error, in
  both directions, exactly as `check_comment_budget.py` does it). `--self-test`
  injects a synthetic growth and a synthetic stale entry and proves both are
  rejected.
- Wire the guard into `run_guards()` in `scripts/verify.sh` and into
  `.github/workflows/ci.yml`. AGENTS.md records that CI runs a strict subset of
  `quick`; this guard must be in **both**, because a baseline that only one
  lane checks is a baseline the other lane can grow.

## 7. Ordered work packages

### Work package A — Capture helper

Intent: one function that renders a surface and returns its cells, so no
invariant re-implements buffer reading.

Required changes: a module under `tests/tui_render/invariants/` exposing a
`Cell` record (symbol, fg, bg, modifiers) and three entry points — render a
`Component` through the modal path, render a `Component` through
`render_hosted`, and render a whole `App` frame. Reuse the buffer-reading loop
already proven in `tui_info_dialog_panel_chrome.inc`, extended to carry style.

Acceptance evidence: a test renders `InfoDialog` through `render_hosted` and
asserts the helper returns exactly `area.width * area.height` cells, and that
a cell known to carry the host's border style reports that style. A
single-symbol assertion that ignores style must fail against a styled cell —
that is the property the old helper lacked.

### Work package B — I1 and I2

Intent: prove no widget paints outside its area, and none panics or paints
nothing at the smallest size.

Required changes: render into a buffer pre-filled with a sentinel style;
assert the sentinel survives every cell outside the widget's rect. Render each
dialog and each panel at 20×6 and 1×1; assert no panic and assert that a
non-degenerate size paints at least one non-blank cell.

Acceptance evidence: the assertion fails when a widget is deliberately given
an area one cell larger than it paints (proving I1 is not vacuous), and the
small-size matrix runs over every `DialogType` and every `SidePanelId` with no
panic.

### Work package C — I3a

Intent: prove a focused surface renders differently from an unfocused one.

Required changes: for every panel placement and every dockable dialog, capture
focused and unfocused renders and assert they differ in at least one cell.
This half passes today because every panel paints a `●`; the assertion exists
so M002 cannot accidentally remove the only difference.

Acceptance evidence: the assertion fails when the focus marker is removed from
a fixture, proving it detects a lost focus indicator.

### Work package D — I4, I5, I6 baselines

Intent: record what is already broken, precisely enough to fix later and
tightly enough that it cannot grow.

Required changes: a manifest with three sections. `[advertised_keys]` lists
each advertised-but-unhandled verb by surface. `[contrast]` lists each failing
cell by surface, theme and the two colours involved. `[hint_truncation]` lists
each budget at which a hint producer drops its exit key or returns empty.
Seed `[advertised_keys]` from `dialog_dispatch_coverage.inc`'s recorded gaps
and from a live scan of `keymap_hint()` and border-title verbs.

Acceptance evidence: the guard rejects a manifest with an extra entry, a
missing entry, and an entry that no longer reproduces; `--self-test` exercises
all three.

### Work package E — Guard wiring and docs

Intent: make the ratchet run automatically and be documented.

Required changes: add the guard to `scripts/verify.sh` `run_guards()` and to
`.github/workflows/ci.yml`; add the harness section to
`architecture/testing.md`; add the manifest's ceilings to
`docs/comment-budget.toml` if the new files exceed the unlisted floor.

Acceptance evidence: `scripts/verify.sh quick` runs the guard and reports it;
deliberately growing the baseline turns `quick` red.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable in the daemon sense: this milestone adds no runtime path. The
harness is a test-only synchronous render. Two test-process properties do
apply and must be respected:

- Tests run under nextest profile `ci`, one process per test. The harness must
  not depend on process-global state; each test builds its own `Theme` and
  `App`.
- A theme iteration over 50 bundled themes must not mutate a global registry.
  Build each theme from the registry by value.

## 9. Compatibility and migration

No compatibility surface. The manifest is a repository artifact and is not
shipped. Removing an entry is the intended direction of travel; adding one
requires a reviewer to see it in the same diff as the code that caused it,
which is the same contract `docs/comment-budget.toml` uses.

## 10. Required tests

### Focused unit tests

- The capture helper returns one cell per buffer position, with style.
- The sentinel survives outside the area for a deliberately oversized paint.
- `fit_parts` is exercised at budgets 0, 1, 5, 20 and 80 and the exit key
  survives at every non-zero budget (this is the I6 measurement, which
  records rather than fixes).

### Integration tests

- Every `DialogType` renders at 20×6, 40×12, 80×24 and 120×40 without panic.
- Every `SidePanelId` renders at each placement at those four sizes.
- Focused and unfocused renders differ for every panel and dockable dialog.

### Restart and recovery tests

None. No durable state is touched. State that the harness holds no file
handles across tests.

### Contention and cancellation tests

None. If a test needs concurrency it must not use
`flavor = "multi_thread"` — the harness is synchronous by design.

### Security and negative tests

- The guard's `--self-test` injects baseline growth and a stale entry.
- A theme whose `selection_text` is deliberately identical to `selection` is
  used to prove the I5 measurement detects an unreadable cell, so the
  measurement is not vacuous before M007 relies on it.

### Migration and compatibility tests

None. No migration.

## 11. Required verification commands

```bash
# the new harness and baseline, as a family member
# The family member is `invariants`; `tui_render_invariants` selects zero tests.
cargo test --test tui_render -- invariants

# the existing panel-chrome contract must still pass unchanged
cargo test --test tui_render -- tui_info_dialog_panel_chrome

# the contrast measurement against all bundled themes
cargo test --test tui_render -- theme_selection_tokens

# the new guard, including its self-test
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_invariant_baseline.py --self-test

# the render-cost shape guard must be unchanged and green
python3 scripts/check_tui_render_cost.py

# the comment ratchet, since new files were added
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/testing.md` — a new subsection: "TUI render invariants",
  naming I1–I6, the capture helper, and the baseline manifest. It must state
  that the baseline may shrink and may not grow, and that a no-longer-failing
  entry is an error rather than a harmless leftover.
- `architecture/tui.md` — one pointer from the render section to the harness.
  No behavioural description changes in this milestone.
- `docs/comment-budget.toml` — new-file ceilings if the harness crosses the
  unlisted floor.
- `plans/registry.md` — move this plan to `active` when work begins.

## 13. Acceptance criteria

Externally observable, in the sense that a reviewer can run them:

1. `scripts/verify.sh quick` is green and reports the new guard.
2. The baseline manifest names every failure the harness currently measures,
   and no more.
3. Growing the baseline turns `quick` red; shrinking it without banking turns
   `quick` red.
4. I1, I2 and I3a assert and pass across every dialog and panel surface.
5. `scripts/check_tui_render_cost.py` passes without modification.
6. **No production rendering file changed as a consequence of this milestone.**
   **Amended by owner decision, 2026-11-14.** The original wording —
   "`git diff` shows no change under `src/tui/**` or `src/theme/**`" — could not
   be read: the milestone shared a worktree with M002, M003 and four other lines,
   so no M001-only diff ever existed, and the tree is now one commit. The
   property it protected is that the harness is additive, and that is what the
   structural reading measures: no M001 artifact is a production file, and the
   harness is wired into both lanes.
7. No wall-clock assertion and no timing output exist in the change.

## 14. Stop conditions

Report rather than improvise when:

- an invariant cannot be asserted without a production change — that means the
  invariant is wrong or the production change belongs to a later milestone;
- I1 or I2 fails on a surface in a way that looks like a real rendering defect
  rather than a harness artefact — record it and report, do not fix it here;
- the 50-theme iteration is too slow to run in `quick` — report the measured
  wall time and propose a subset, do not silently drop themes;
- a theme fails to construct for an unrelated reason;
- the guard cannot be made non-vacuous (its `--self-test` cannot inject a
  detectable defect).

## 15. Closure evidence required

- The harness file and the manifest, with the recorded counts.
- `quick` output showing the guard reporting.
- The `--self-test` output for the guard.
- Confirmation that no production file changed, with the command that shows it.
- The I5 measurement's theme count and failing-cell count, stated as counts.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/001-status.md`.

## 16. Handoff notes

The single most important design constraint: **do not let the baseline become
a dumping ground.** Three entries are expected and legitimate (one per
recorded invariant). If the first run finds two hundred contrast failures, that
is the roadmap's §10 risk materialising — record them, report the count, and
let M007 own the sweep. Do not narrow the theme set to make the number smaller,
and do not add a per-surface allowlist; the manifest is the allowlist.

The second constraint: **I3a is deliberately weak.** It passes today because
every panel paints a `●`. Its purpose is to stop M002 removing the only
difference while it changes everything else. Do not strengthen it here — the
uniform-channel half (I3b) belongs to M002, and adding it now would make this
milestone unclosable.

The third: the capture helper is the contract every later milestone builds on.
If it returns strings, later milestones cannot assert style and will re-read
buffers by hand, which is how the current gap happened. Return cells.
