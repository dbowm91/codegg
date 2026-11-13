# TUI Modularity and Render Invariants Milestone 007 — Contrast ratchet to zero

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-007--contrast-ratchet-to-zero`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the TUI
  presents the project state hierarchy; a surface that cannot be read does not
  present it.

Applicable ADRs:

- None required. This milestone consumes the derived-token contract in
  `src/theme/` and does not change it.

Primary class: capability

## 1. Objective

Drive the I5 contrast baseline to zero, so every rendered foreground on every
surface clears the product's own contrast bar against the fill actually painted
under it, on all 50 bundled themes.

## 2. Why this milestone is ready

Blocked on M001 (hard, it defines the measurement and the ratchet) and on M002,
M003, M004, M005 and M006 (hard, because each of them moves cells that I5
measures; sweeping before them would be re-done).

This is a convergence milestone. It is the only milestone whose scope is defined
by a measurement rather than by a module boundary, and that is deliberate: the
invariant is cross-cutting, and every earlier milestone deliberately leaves it
alone so that this one sees the finished shape.

## 3. Current implementation evidence

At baseline `7642b9e38`:

- The bar exists and is shipped: `MIN_DERIVED_TEXT_RATIO` is exported from
  `codegg::theme::schema`, and `Color::contrast_ratio` is implemented at
  `src/theme/color.rs:88`.
- The bar is currently applied to **derived tokens**, not to frames:
  `tests/tui_render/theme_selection_tokens.inc:82` asserts
  `selection_text.contrast_ratio(selection)` and the accent pair across every
  bundled theme, and its doc states the behavioural contract it guards.
- It is applied in **one render path**: `readable_on` at
  `src/tui/app/render.rs:101` resolves the completion popup's foreground against
  the selection fill. That function exists because a theme could pick a
  `selection` and an accent at the same luminance, and it is the only place the
  render path reasons about the fill it paints on.
- `check_no_hardcoded_selection_colours.py` already forbids `Color::White` and
  `Color::Black` as a selection foreground in a dialog render path, and its doc
  records why: ten render paths used `Color::White`, so a light theme rendered
  every selected row in that set as white-on-white.
- 88 `Color::` literals remain outside `theme.rs`, concentrated in
  `components/tool_output.rs` (26), `dialogs/run_detail.rs` (20),
  `dialogs/theme.rs` (18 — a theme-name mapping, legitimate),
  `interactive_terminal.rs` (7), `components/messages.rs` (4), `wrap.rs` (3),
  `components/prompt.rs` (3) and `components/editor.rs` (3). Most are `Reset` or
  named colours rather than literal RGB, but each is a cell whose foreground is
  not a theme token.
- `Theme` has 19 fields, all colours, and no other visual vocabulary.

The measurement has one honest limitation that this milestone must not hide:
a cell whose foreground or background is `Color::Reset` or otherwise
unresolvable inherits the terminal's own palette, so its ratio cannot be
computed. Those cells are skipped, and **the skip count is reported per theme**
so that a theme which is mostly unmeasurable is a finding rather than a pass.

## 4. Invariants that must not regress

- **No token is changed to make a cell pass.** The derived tokens in
  `src/theme/schema.rs` are a shipped contract for 50 bundled themes and every
  user-authored theme; changing one changes every theme. A failing token is
  reported, not adjusted.
- **No surface loses text to fix a contrast failure.** Reducing information to
  reach zero is not a fix.
- **I1, I2, I3b, I4 and I6 stay green.**
- **The render-cost shape guard is unchanged.** Contrast resolution must not
  add a per-frame allocation or a second producer.
- **The comment ratchet holds.**
- **No wall-clock claim.** This milestone measures ratios, not time.

## 5. Scope

### In scope

- Fixing render-path foreground choices so each cell uses the token that reads
  against its actual fill.
- Replacing a hardcoded foreground with a theme token where the literal is the
  cause.
- Reporting, with evidence, any failure whose cause is a derived token.
- Driving the M001 `[contrast]` baseline to empty, and recording the per-theme
  skip counts.

### Explicitly out of scope

- Changing any value in `src/theme/schema.rs` or any bundled theme file.
- Adding a new theme token. If a surface needs one that does not exist, that is
  a roadmap §10 decision point (a token layer needs an ADR), not a milestone
  action.
- Restyling. The fix is "use the token that reads", not "pick a nicer colour".
- The desktop and web frontends.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

- Render paths whose foreground is a literal or a token that does not read
  against the fill it is painted on. The likely sites are the ones §3 lists,
  plus any surface the earlier milestones did not touch: transcript, sidebar,
  composer, editor, file tree, status bar, change review, diff, tool output and
  interactive terminal.
- Where `readable_on` is the right mechanism, reuse it. Where a surface paints
  a foreground on a fill it does not own, the fix is to resolve against the
  fill, not to choose a different constant.

### Security and authorization

None.

### Documentation and static guards

- `architecture/theme.md` — the relationship between a derived token and a
  render path: a render path chooses *which* token, never *what colour*.
- `architecture/testing.md` — the contrast measurement, its bar, and its
  unmeasurable-cell limitation with the reporting requirement.

## 7. Ordered work packages

### Work package A — Measure and triage

Intent: turn the baseline into a work list grouped by cause, not by cell.

Required changes: classify every baseline entry as (a) wrong token for the
fill, (b) hardcoded foreground, (c) derived token itself below the bar, or
(d) unmeasurable. Produce counts per class and per surface.

Acceptance evidence: the classification totals equal the baseline size, and
class (c) is enumerated separately because it is the one that escalates.

### Work package B — Class (a): wrong token

Intent: the common case — a token that is right elsewhere and wrong here.

Required changes: resolve the foreground against the actual fill, reusing
`readable_on` where it applies.

Acceptance evidence: the baseline shrinks by exactly the entries fixed, with no
new entry appearing.

### Work package C — Class (b): hardcoded foreground

Intent: remove the literals that caused the original defect class.

Required changes: replace the literal with the token that reads against the
fill.

Acceptance evidence: the count of `Color::` literals outside `theme.rs` falls,
and `check_no_hardcoded_selection_colours.py` still passes.

### Work package D — Class (d): unmeasurable cells

Intent: make the blind spot visible instead of silently passing.

Required changes: report the per-theme skip count, and for any theme where the
skipped fraction is high, record why. Where the cause is a `Reset` that could be
an explicit token, converting it is in scope; where it is a deliberate
inheritance, record the reason.

Acceptance evidence: the report exists and is part of the closure record.

### Work package E — Ratchet to empty

Intent: leave nothing recorded.

Required changes: the `[contrast]` section is empty; the guard fails if any
entry is added back.

Acceptance evidence: the guard reports an empty baseline and rejects a synthetic
re-addition via its `--self-test`.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. The measurement is a synchronous buffer walk. The one
operational concern is cost: 50 themes × every surface is a large matrix, and if
it cannot run inside `quick` it must run in the full sweep with a documented
reason, not be reduced to a subset. Reducing the theme set to make the gate fast
is how the gate stops meaning anything.

## 9. Compatibility and migration

No compatibility surface. A user sees text that was unreadable become readable
under their theme. No theme file changes, so a user's own theme keeps working
and, if it fails, now fails visibly with a named cell rather than silently
rendering white-on-white.

## 10. Required tests

### Focused unit tests

- `readable_on` is exercised for the pairs it is applied to, including a pair
  that forces it to fall through to its last candidate.
- The classification helper groups a synthetic baseline correctly.

### Integration tests

- The full matrix passes with an empty baseline.
- A synthetic unreadable cell is detected on a deliberately broken theme,
  proving the measurement is not vacuous.
- The per-theme skip counts are reported and asserted to be below a recorded
  fraction, so the blind spot cannot grow unnoticed.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- A theme whose `selection_text` equals `selection` is detected.
- A cell using `Color::Reset` is skipped, counted, and does not fail the run —
  proving the skip rule is implemented rather than the cell being misread as
  passing.

### Migration and compatibility tests

- `theme_selection_tokens.inc` passes unmodified, proving no derived token
  moved.

## 11. Required verification commands

```bash
# the measurement, over every bundled theme
cargo test --test tui_render -- tui_render_invariants

# the token contract must be untouched
cargo test --test tui_render -- theme_selection_tokens

# the guard and its baseline
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_invariant_baseline.py --self-test

# the literal-colour guard
python3 scripts/check_no_hardcoded_selection_colours.py

# the render-cost shape guard
python3 scripts/check_tui_render_cost.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/theme.md` — a render path chooses which token, never what
  colour.
- `architecture/testing.md` — the bar, the measurement, and the unmeasurable
  reporting requirement.
- The closure record carries the classification counts and the per-theme skip
  fractions.

## 13. Acceptance criteria

1. The `[contrast]` baseline is empty.
2. The measurement is proved non-vacuous against a broken theme.
3. Per-theme skip counts are reported and below the recorded fraction.
4. `theme_selection_tokens.inc` passes unmodified.
5. No file under `src/theme/` changed.
6. The literal-colour guard passes and the literal count outside `theme.rs` fell.
7. I1, I2, I3b, I4 and I6 remain green.
8. `scripts/verify.sh quick` is green; the render-cost guard is unmodified.

## 14. Stop conditions

Report rather than improvise when:

- a failure traces to a derived token in `src/theme/schema.rs` rather than a
  render path. That changes every theme and needs an owner decision; enumerate
  the affected pairs and stop;
- reaching zero would require a token that does not exist;
- the matrix cannot run inside `quick` — report the measured wall time and
  propose the full-sweep placement, do not subset the themes;
- a surface's unmeasurable fraction is high enough that the measurement says
  little about it — report it rather than declaring that surface green;
- a fix would remove text or a hint.

## 15. Closure evidence required

- The classification counts per cause and per surface.
- The per-theme skip fractions.
- The non-vacuity result against a broken theme.
- The empty baseline and the guard's self-test output.
- Confirmation that `src/theme/` is unchanged.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/007-status.md`.

## 16. Handoff notes

This milestone is where the roadmap is most likely to discover that a real
defect is upstream of it. If a derived token is below the bar against the fill
it is meant for, the honest outcome is to stop and report, not to pick a
different token that happens to pass — that would move the failure rather than
fix it, and it would leave 50 bundled themes carrying a token nobody trusts.

The unmeasurable cells are the quiet risk. A theme that paints `Reset` on
`Reset` is inherited behaviour and cannot be measured, and a large skipped
fraction would let a surface pass without being tested. Reporting the fraction
is not bureaucracy; it is the difference between a green gate and an empty one.

Do not treat this as a restyle. The fix for "this text is unreadable" is "use
the token that reads against the fill", and a reviewer should be able to check
that claim from the diff without seeing a colour they did not expect.
