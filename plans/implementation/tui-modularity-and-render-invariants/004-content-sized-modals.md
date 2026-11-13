# TUI Modularity and Render Invariants Milestone 004 — Content-sized modals

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-004--content-sized-modals`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — a dialog
  presents one interaction concept; the rect it occupies is a presentation
  decision this milestone makes content-derived.

Applicable ADRs:

- None required. Local presentation change. No protocol, storage, authority or
  compatibility surface.

Primary class: capability

## 1. Objective

Replace the unconditional `centered_rect(60, 50)` with a size derived from what
the dialog actually contains, clamped to a minimum and a maximum, so a
two-option confirm stops occupying half the frame and a dense dialog gets the
room it needs.

## 2. Why this milestone is ready

Blocked on M001 (hard, verification gate) and on M003 (hard, design
dependency): content-derived sizing needs `Section` to report its content
height, and `Section` does not exist before M003.

The seam is already shaped correctly. `Component` is the trait every dialog
implements, `App::render_dialog` is the single site that computes the modal
rect, and `ProjectPicker` already proves that a dialog-specific area works —
it is just the only one that does it, through a bespoke function rather than a
trait method.

## 3. Current implementation evidence

At baseline `7642b9e38`:

- `App::render_dialog` (`src/tui/app/render.rs:3263`) computes
  `centered_rect(60, 50, area)` once and passes that rect to whatever is
  mounted. Every modal is therefore 60% of the width and 50% of the height of
  the frame, whatever it contains.
- `centered_rect` (`src/tui/components/component.rs:296`) is percentage-based:
  it splits `area` into `(100 - pct) / 2`, `pct`, `(100 - pct) / 2` on each
  axis. It cannot express a content size.
- `ProjectPicker` is the only exception: `App::render_dialog` special-cases
  `DialogType::ProjectPicker` and calls `ProjectPickerDialog::picker_area`
  (`src/tui/components/dialogs/project_picker.rs:51`), which is tested for
  centring and for clamping to the available area.
- The consequence is arithmetic, not opinion. On a 24-row terminal the popup is
  12 rows, so a Confirm dialog — whose own layout is
  `Length(4) / Min(4) / Length(5)` — receives 3 content rows for two options.
  On a 60-row terminal the same Confirm receives 30 rows.
- `DialogType` has 52 real variants served by 41 dialog files; the `InfoDialog`
  fan-out alone covers 14 panels.
- Every dialog already scrolls or clamps its content, so a rect smaller than
  its preference is a supported state rather than a new failure mode.

## 4. Invariants that must not regress

- **No dialog becomes unusable at a small size.** I2 covers panic-freedom; this
  milestone adds "the dialog's own minimum is honoured or the dialog scrolls",
  asserted per dialog.
- **No dialog paints outside its area** (I1) at any of the three named sizes.
- **The default is today's behaviour.** A dialog that does not override the new
  seam keeps `centered_rect(60, 50)` exactly, so adoption is opt-in and the
  corpus shows precisely which dialogs moved.
- **`centered_rect` remains the single definition of the percentage shape.** It
  is not deleted; it becomes the default implementation.
- **No dialog gains or loses a key, a hint or a row it used to show.**
- **The render-cost shape guard is unchanged.** Sizing must not introduce a
  second measure of a dialog's content per frame.
- **The comment ratchet holds.**

## 5. Scope

### In scope

- A `preferred_area(&self, available: Rect) -> Rect` seam on `Component`, with
  a default implementation that returns `centered_rect(60, 50, available)`.
- Adoption by dialogs that can state a content size, beginning with Confirm,
  Permission, Question, Import, Goto and the Info family.
- Migrating `ProjectPicker`'s bespoke area onto the seam, so one sizing path
  exists.
- Clamps: a minimum of 20×6 (the I2 size) and a maximum of 90% of the
  available rect on each axis.
- Per-dialog corpus diffs and a size record at 80×24, 120×40 and 40×12.

### Explicitly out of scope

- Resizing the frame's own regions (chat, sidebar, panels). That is M008's
  `FramePlan`.
- User-resizable dialogs or remembered sizes.
- Any dialog whose content size is not statically derivable — those keep the
  default and are listed in the closure record as not adopted.
- Changing a dialog's internal layout to suit a new rect. If a dialog's content
  does not fit its own preference, the preference is wrong, not the layout.

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

- `Component::preferred_area` with the `centered_rect(60, 50)` default, so
  every existing dialog is correct by construction and adoption is a deliberate
  edit.
- `App::render_dialog` consults `preferred_area` and stops special-casing
  `ProjectPicker`.
- A shared clamp helper so the minimum and maximum are applied once, in the
  same place, rather than by each dialog.
- Dialogs that adopt the seam derive their preference from `Section`'s content
  height (M003) plus their own row budget.

### Security and authorization

None.

### Documentation and static guards

- `architecture/tui.md` — the sizing contract: a dialog states a preference,
  the host clamps it, and the default is the legacy percentage.
- No new guard. I1 and I2 at three sizes are the enforcement.

## 7. Ordered work packages

### Work package A — The seam with the legacy default

Intent: introduce the seam without changing a single pixel.

Required changes: `Component::preferred_area` returning
`centered_rect(60, 50, available)`; `App::render_dialog` calls it.

Acceptance evidence: the corpus is **byte-identical for every dialog**. This is
the control case and it must be reported as such.

### Work package B — ProjectPicker migration

Intent: one sizing path instead of two.

Required changes: `ProjectPickerDialog` implements `preferred_area` with its
existing `picker_area` logic; the special case in `App::render_dialog` is
removed.

Acceptance evidence: corpus byte-identical for the picker, and its existing
centring and clamping tests pass unmodified.

### Work package C — Clamp helper

Intent: one place that decides the floor and the ceiling.

Required changes: a helper applying the 20×6 minimum and the 90% maximum,
including the degenerate case where the available rect is smaller than the
minimum.

Acceptance evidence: unit tests at 120×40, 80×24, 40×12, 20×6, 10×3 and 1×1,
asserting the returned rect is inside the available rect in every case.

### Work package D — Adoption by the simple dialogs

Intent: prove the seam on dialogs whose content size is obvious.

Required changes: Confirm, Permission, Question, Import and Goto derive their
preference from their section count and row budget.

Acceptance evidence: per-dialog corpus diff showing the dialog shrinking to its
content, with no content row lost. Confirm at 24 rows must stop receiving 3
content rows for 2 options.

### Work package E — Adoption by the Info family

Intent: cover the 14-panel fan-out with one change.

Required changes: `InfoDialog` derives its preference from its line count,
clamped.

Acceptance evidence: the corpus diff is reviewed for the fan-out as a whole,
and `tui_info_dialog_panel_chrome` passes unmodified (its docked path must not
be affected by modal sizing).

### Work package F — Size matrix

Intent: make "fits its content" a checked property.

Required changes: a test rendering every dialog at the three named sizes and
asserting the resulting rect equals `clamp(preferred_area(available))`, plus I1
and I2 at each.

Acceptance evidence: the matrix fails when a dialog's preference is replaced
with a value larger than the available rect, proving the clamp is exercised.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Rendering is synchronous. The one hazard is a dialog whose
preference oscillates with its own rendered size — a preference derived from
the rect it is given would feed back. The seam takes `available` and must not
read the previously assigned rect; a test asserts idempotence by calling it
twice with the same `available` and comparing.

## 9. Compatibility and migration

No compatibility surface. A dialog that does not adopt the seam renders exactly
as today, so the change is incremental and no dialog is forced to adopt.
`ProjectPicker`'s public `picker_area` may remain as a thin wrapper if a caller
outside the render path uses it; if nothing does, it is removed and the closure
record says so.

## 10. Required tests

### Focused unit tests

- The clamp helper at six sizes including degenerate ones.
- `preferred_area` is idempotent for the same `available`.
- The default implementation equals `centered_rect(60, 50, available)`.

### Integration tests

- Every dialog at 80×24, 120×40 and 40×12: resulting rect equals the clamped
  preference, I1 green, I2 green.
- Confirm at 24 rows shows both options without scrolling.
- `tui_info_dialog_panel_chrome` passes unmodified.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- A dialog claiming a preference larger than the available rect is clamped, and
  the test asserts the clamp rather than the claim.
- A dialog claiming a zero-height preference gets the minimum, not an empty
  rect.

### Migration and compatibility tests

- The full corpus is byte-identical after work package A, proving the default
  preserves legacy behaviour.
- `ProjectPicker`'s centring and clamping tests pass unmodified after B.

## 11. Required verification commands

```bash
# the control case for the default seam
cargo test --test tui_render -- tui_render_invariants

# dialogs that adopt the seam
cargo test --test tui_render -- dialog_dispatch_coverage
cargo test --test tui_render -- tui_info_dialog_panel_chrome
cargo test --test tui_render -- file_picker_surface_corrective

# the picker migration
cargo test --lib -- components::dialogs::project_picker

# the render-cost shape guard
python3 scripts/check_tui_render_cost.py

# the comment ratchet
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the sizing contract, the clamps, and the statement
  that the default is the legacy percentage so an unconverted dialog is correct
  rather than merely tolerated.
- The milestone's corpus diff note, one entry per dialog, with the resulting
  rect at each of the three sizes.
- The closure record lists every dialog that did **not** adopt the seam and why.

## 13. Acceptance criteria

1. `Component::preferred_area` exists with the legacy default.
2. The corpus is byte-identical after the seam is introduced (control case).
3. `ProjectPicker` uses the seam and its special case is gone.
4. Every dialog renders inside its clamp at the three named sizes, with I1 and
   I2 green.
5. A Confirm dialog shows both options without scrolling at 24 rows.
6. `tui_info_dialog_panel_chrome` passes unmodified.
7. `scripts/verify.sh quick` is green; the render-cost guard is unmodified.

## 14. Stop conditions

Report rather than improvise when:

- the corpus is not byte-identical after work package A — the default is wrong
  and the whole milestone depends on it;
- a dialog's content does not fit at its own preferred size, which means the
  preference is mis-derived;
- a dialog's preference would need to read its own rendered output to be
  computed;
- the 90% maximum produces an unusable dialog at 40×12 for a dialog that cannot
  scroll;
- adopting the seam would require changing a dialog's content layout.

## 15. Closure evidence required

- The seam, the clamp helper and their unit tests.
- The control-case result, explicitly byte-identical.
- The size matrix output at all three sizes for every dialog.
- The list of non-adopting dialogs and the reason for each.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/004-status.md`.

## 16. Handoff notes

The default is the milestone. If `preferred_area`'s default does not reproduce
`centered_rect(60, 50)` exactly, every dialog changes at once and the diff
becomes unreviewable. Prove that first, then adopt one dialog at a time.

Do not let a dialog's preference depend on the rect it was given. That is the
one way this seam can oscillate, and it will show up as a dialog that resizes
between frames on a live terminal rather than in a test — so the idempotence
test is not ceremony.

Finally, "content-sized" does not mean "as small as possible". A dialog that
fits in three rows but jumps around the screen as its text changes is worse
than one that is too big. If a dialog's content length varies while it is open,
prefer a stable size for its class and say so in the closure record.
