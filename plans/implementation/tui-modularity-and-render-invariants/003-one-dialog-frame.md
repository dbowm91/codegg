# TUI Modularity and Render Invariants Milestone 003 — One dialog frame

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-003--one-dialog-frame`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — dialogs
  present the interaction concepts named there; this milestone changes their
  chrome, not their contents.

Applicable ADRs:

- None required. Local rendering change inside `src/tui/components/dialogs/**`
  and `src/tui/components/`. No protocol, storage, authority or compatibility
  surface is touched.

Primary class: capability

## 1. Objective

One `Section` primitive producing a bordered block with an optional hint on its
border, adopted for every dialog's outer frame, title row and footer row — so
dialog chrome is produced once instead of 41 times, and the title padding,
casing and focus-marker drift disappears.

## 2. Why this milestone is ready

Blocked on M001 (hard, verification gate) with an interface dependency on M002
that is stable: `Section` reuses `PanelChrome`'s focus-style resolution, which
M002 §6 fixes.

The `Component` trait already defines the seam this milestone builds on.
`src/tui/components/component.rs:243` documents `render_in_panel` as the
override a widget must supply when it draws its own chrome, precisely because
the host has already painted a border, a title and the focus dot into the same
rect. `Section` is the primitive that makes obeying that rule the easy path
instead of a discipline each dialog must remember.

## 3. Current implementation evidence

At baseline `7642b9e38`:

- **41 dialog files.** 40 define `handle_key`. 26 draw `Borders::ALL` without
  overriding `render_in_panel`; 15 are dockable; 14 override.
- **Box counts range from 1 to 10.** `run_detail.rs` draws 10 bordered boxes,
  `plan.rs` 9, `question.rs` and `connect.rs` 6, against 1 for `queue.rs` and
  `session.rs`.
- **Every modal is laid out from one call.** `App::render_dialog`
  (`src/tui/app/render.rs:3263`) computes `centered_rect(60, 50)` and hands the
  same rect to whatever is mounted; `ProjectPicker` is the only dialog with its
  own area.
- **Title formatting has drifted.** Of 59 border titles, **10 lack the leading
  padding space and all ten are in `run_detail.rs`**. `" Job detail"` is the
  only sentence-case title among Title Case peers. Four titles carry a focus
  dot: `" Plan ● "`, `" Tree ● "`, `" Diff ● "` trailing, and `" ● Files "`
  leading.
- **A live instance of the double-chrome defect.**
  `src/tui/components/dialogs/agent.rs:173` returns `true` from
  `docks_as_viewer`, `:221` paints its own `Borders::ALL`, and the file
  contains **no** `render_in_panel`. Docking the agent editor therefore paints
  its border over the host's border, title and `●` — the same failure
  `tests/tui_render/tui_info_dialog_panel_chrome.inc` was written to catch for
  `InfoDialog`, where it was visible in fourteen panels at once.
- **The test that proves the good case exists.**
  `tui_info_dialog_panel_chrome.inc` renders a hosted column and asserts the
  host's title and dot survive. It covers `InfoDialog` only, and it reads
  symbols only.
- **`centered_rect` is already a documented de-duplication.**
  `src/tui/components/component.rs:296` states it is "the one definition of
  this shape", noting it was once duplicated in `app/mod.rs` and
  `run_detail.rs`. This milestone is the same move for chrome.

## 4. Invariants that must not regress

- **No dialog loses a verb, a hint, or a row of content.** The corpus diff for
  every dialog is reviewed individually.
- **Every dockable dialog leaves the host's chrome intact.** `AgentDialog`
  gains the behaviour `InfoDialog` already has.
- **No dialog gains or loses a key.** `dialog_dispatch_coverage.inc`'s three
  recorded gaps stay recorded unless M006 closes them; this milestone neither
  closes nor widens them.
- **I1, I2 and I3b stay green.**
- **`centered_rect` remains the single definition of the modal shape.** M004
  changes the arguments, not the function's ownership.
- **The comment ratchet holds.** Splitting any dialog file requires banking the
  split and redistributing per-file ceilings in `docs/comment-budget.toml`.

## 5. Scope

### In scope

- A `Section` primitive: bordered block, optional title, optional hint on the
  border, optional focus marker, returning its inner `Rect`.
- Adoption for each dialog's **outer frame**, **title row** and **footer/hint
  row**.
- Title normalisation: one leading and one trailing space, Title Case, and a
  trailing `●` when and only when the dialog is docked.
- Fixing `AgentDialog`'s missing `render_in_panel`.
- An I1 assertion that a docked dialog preserves the host's border, title and
  dot.

### Explicitly out of scope

- Rewriting a dialog's **content** layout. A dialog that draws a content group
  as a bordered box keeps that box; `Section` governs chrome, not content
  grouping. Consequently a 10-box dialog will still have several boxes — the
  milestone removes the ones that exist only to hold a title or a hint, and it
  must not claim more than that.
- Dialog sizing — that is M004.
- Hint wording and separator — that is M005.
- Any change to `Component::handle_key` or to `FocusManager`.

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

- `Section` in `src/tui/components/` with `title`, `hint`, `focused` and a
  `draw(area, buf) -> Rect` returning the inner area. It resolves border and
  title style through the same policy `PanelChrome` uses, so the two cannot
  diverge.
- `Section` owns the title string construction — padding, casing and the
  trailing dot — so a caller supplies `"Files"` and not `" ● Files "`.
- `Component::render_in_panel`'s default remains "delegate to `render`". A
  dialog that adopts `Section` for its outer frame must still override
  `render_in_panel` when docked, because the host owns the outer edge. The
  adoption should make that structural rather than remembered: `Section` gains
  a variant that draws no outer border, which is what a docked dialog needs.

### Security and authorization

None.

### Documentation and static guards

- `architecture/tui.md` — the dialog chrome contract: a docked dialog draws no
  outer border, and `render_in_panel` is the seam that guarantees it.
- No new guard. The I1 assertion and the per-dialog corpus diff are the
  enforcement.

## 7. Ordered work packages

### Work package A — The primitive

Intent: one producer of dialog chrome.

Required changes: `Section` with the four decisions above, plus the
borderless-when-docked variant. Title construction moves inside it.

Acceptance evidence: unit tests assert padding, casing, dot placement, and that
the borderless variant insets nothing on the edges it does not draw.

### Work package B — The control case: InfoDialog

Intent: adopt in the one dialog that already has a passing panel-chrome
contract, so the primitive is proved against known-good behaviour.

Required changes: `InfoDialog` builds its three rows from `Section`.

Acceptance evidence: `tui_info_dialog_panel_chrome` passes **unchanged**, and
its corpus is byte-identical. If it moves, the primitive is wrong.

### Work package C — Fix AgentDialog

Intent: close the live double-chrome defect before the sweep, so the sweep
adopts a known-good pattern.

Required changes: `AgentDialog` overrides `render_in_panel` and draws no outer
border when docked; its modal path keeps its border.

Acceptance evidence: a test renders it through `render_hosted` and asserts the
host's title and `●` survive — the same shape as the `InfoDialog` test.

### Work package D — Title normalisation

Intent: one spelling of a title.

Required changes: the 10 unpadded titles in `run_detail.rs` gain padding;
`" Job detail"` becomes Title Case; `files.rs` moves its dot from leading to
trailing.

Acceptance evidence: corpus diff shows only the title row changing, one dialog
at a time.

### Work package E — The sweep

Intent: every dialog's outer frame, title and footer come from `Section`.

Required changes: adopt across the remaining dialogs. Group the work by file
so each commit is reviewable; do not attempt all 41 in one diff.

Acceptance evidence: a per-dialog corpus diff, reviewed individually. The
milestone is not complete until every dialog file has one.

### Work package F — I1 assertion for docked dialogs

Intent: make "docking preserves host chrome" a property of every dockable
dialog rather than a test of one.

Required changes: extend the harness to render each dockable dialog through
`render_hosted` and assert the host's border, title and dot survive.

Acceptance evidence: the assertion fails against the pre-fix `AgentDialog`,
proving it detects the defect this milestone repairs.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Rendering is synchronous. The one real hazard is partial
adoption: a dialog half-converted between two chrome producers. The work
packages are ordered so the primitive is proved (B) and the defect is fixed (C)
before the sweep (E), and the sweep is grouped by file so no file is left in a
mixed state at a commit boundary.

## 9. Compatibility and migration

No compatibility surface. Dialog identity, key handling, focus and sizing are
unchanged except for M004's later sizing work. A user sees the same dialog with
a consistently spelled title and, for the agent editor, a panel that keeps its
host's chrome.

## 10. Required tests

### Focused unit tests

- `Section` padding, casing and dot placement.
- The borderless variant insets only the edges it draws.
- Title construction rejects a caller-supplied pre-padded string (or strips
  it), so the 10 `run_detail` titles cannot reintroduce the drift.

### Integration tests

- Every dialog renders at 20×6, 40×12, 80×24 and 120×40 without panic.
- Every dockable dialog renders through `render_hosted` with the host's title
  and dot intact.
- I1, I2 and I3b remain green for every dialog.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- The docked-chrome assertion fails against a fixture that paints its own
  border, proving it is not vacuous.
- A dialog with a 60-character title renders without panicking at 20 columns
  and without painting outside its area.

### Migration and compatibility tests

- `tui_info_dialog_panel_chrome` passes unmodified, and its corpus is
  byte-identical.

## 11. Required verification commands

```bash
# the control case, which must pass unmodified
cargo test --test tui_render -- tui_info_dialog_panel_chrome

# the dialog dispatch record, which must be unchanged by this milestone
cargo test --test tui_render -- dialog_dispatch_coverage

# the invariant harness
cargo test --test tui_render -- tui_render_invariants

# a representative spread of converted dialogs
cargo test --test tui_render -- tui_main_buffer
cargo test --test tui_render -- file_picker_surface_corrective
cargo test --test tui_render -- tui_logs_window

# the render-cost shape guard
python3 scripts/check_tui_render_cost.py

# the comment ratchet, since dialog files may be split
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the dialog chrome contract and the docked-dialog
  rule.
- The milestone's corpus diff note, one entry per dialog.
- `docs/comment-budget.toml` — ceilings if any dialog file is split.

## 13. Acceptance criteria

1. Every dialog's outer frame, title row and footer row come from `Section`.
2. No dialog file constructs a title string with its own padding or dot.
3. `AgentDialog` preserves host chrome when docked, proved by a test.
4. `tui_info_dialog_panel_chrome` passes unmodified with a byte-identical
   corpus.
5. Every dialog has a reviewed corpus diff recorded.
6. No dialog lost a verb, a hint or a row of content.
7. `scripts/verify.sh quick` is green; the render-cost guard is unmodified.

## 14. Stop conditions

Report rather than improvise when:

- the `InfoDialog` control case moves at all;
- a dialog cannot adopt `Section` without a content-layout change — that means
  the primitive is under-specified; report the dialog and the shape it needs;
- a dialog's hint is load-bearing in a way the primitive cannot express (for
  example a hint that changes with state) — report it, do not special-case the
  primitive for one caller;
- the per-dialog diff for any dialog shows a content row moving;
- adopting `Section` in a file would require crossing the comment ceiling
  without a bank.

## 15. Closure evidence required

- The primitive and its unit tests.
- The `InfoDialog` control-case result, explicitly byte-identical.
- The `AgentDialog` before/after, with the failing assertion that proves the
  test detects the defect.
- A per-dialog corpus diff record covering all 41 files.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/003-status.md`.

## 16. Handoff notes

Do the sweep by file, not by pattern. A single diff touching 41 dialogs cannot
be reviewed, and the whole point of this milestone is that a reviewer can see
what moved on each surface.

Resist widening the primitive. `Section` governs chrome — border, title, hint.
A caller that wants it to lay out content is asking for a different primitive,
and the roadmap defers that (§3: no design-token layer).

The `AgentDialog` fix is not incidental cleanup. It is the same defect class the
`InfoDialog` test was written for, and finding it live means the class is not
closed by one test. Package F exists so the next instance is caught by
construction rather than by someone reading a trait doc.
