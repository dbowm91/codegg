# TUI Editor Inline LLM Assist Milestone 002 — The cursor-anchored review menu

Status: blocked

Repository baseline: `b864c78a1f7f725335e8b19588b0fbff7208d60a` on
`feat/auto-20261109-f8abad0d-artifact-safe`

Blocked on: M001 closure. This milestone builds on M001's payload and M001's
actions; it does not re-decide them.

Source roadmap:

- `plans/subsystems/tui-editor-inline-llm-assist-roadmap.md#milestone-2--the-cursor-anchored-review-menu`

Long-term requirements:

- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0066-editor-inline-generation-and-pending-region-contract.md` (accepted 2026-11-24; this milestone implements its Decision)
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted)

Primary class: capability

## 1. Objective

Moving the cursor into a pending generated region raises a menu at the cursor
offering Approve, Deny and Edit, and moving the cursor out of the region
dismisses the menu.

## 2. Why this milestone is ready

- **M001 closed** is the only hard dependency. M001 leaves the payload, the
  anchor, the approve action and the discard action in place, and this
  milestone is a surface over them.
- **No interface dependency is outstanding.** The editor's overlay patterns,
  its span layers and its key tables are closed lines; the cursor's screen
  geometry is computable from the editor's own viewport.

## 3. Current implementation evidence

- The editor's overlays are floating: the keymap box is anchored to the text
  area's bottom-right corner, the command-match list floats on the row above
  the command line, and the completion popup is centred in the editor area.
  None of them reserves a row, so the document does not reflow when one opens.
- The cursor's position on screen is computed per row inside `line_spans`
  through `cell_for_offset`, which returns a **line-relative byte range** and
  refuses a position scrolled out of the horizontal window. The **display
  column** is a separate computation: `display_width_str` exists in the
  editor's renderer, and the horizontal scroll offset is the row's
  `visible_start`.
- `composer_geometry::VisualLayout` is **not** used by the editor renderer.
  A cursor-anchored menu must use the editor's own geometry, not the
  composer's.
- The editor's Normal table matches on exact `(modifiers, code)` pairs, and
  the editor's key authority is a guard that rejects a chord claimed twice or
  shadowed.
- The editor's `:` modal line owns every key while it is open, and the
  completion popup owns its keys while it is open, in the same manner this
  menu will.
- The completion popup's status vocabulary distinguishes `Loading`,
  `Ready`, `NotReady`, `Stale`, `Unsynced`, `Unsupported` and
  `Error` — a vocabulary the menu can borrow for its own refusals.

## 4. Invariants that must not regress

- The menu never reserves a row, so opening it does not reflow the document.
- The menu's visibility is **derived** from the cursor's position, never
  latched, so a menu that is on screen is always on screen because the cursor
  is in the region.
- The menu owns the keyboard only for the verbs it implements; every other
  key falls through to the editor's Normal table, which may move the cursor
  and thereby dismiss the menu.
- Approve and Deny are M001's actions. The menu adds a surface and one new
  action; it does not add a second approve path with different semantics.
- One keyboard axis. The menu's keys are read while it is open, and the
  editor's `ui_state.input_mode` is unchanged.
- The generated text is never logged, and the menu never renders it as
  document text.

## 5. Scope

### In scope

- The cursor-anchored floating menu, with its geometry, its flip at the text
  area's edges and its horizontal clamp.
- The derived visibility rule.
- The Edit action: discard the region and re-open the prompt field with the
  previous instruction.
- The menu's key handling while it is open.

### Explicitly out of scope

- A menu for more than one region, or a menu that survives a cursor move.
- A diff view of the generated text against the document.
- A model or thinking-level choice inside the menu; those belong to the
  prompt field.
- Streaming the answer.
- The desktop frontend.

## 6. Required production changes

### Core/domain

None. The daemon is untouched by this milestone.

### Storage and migrations

None.

### Protocol and DTOs

None. The Edit action is local: it discards a frontend payload and re-opens a
frontend field.

### Runtime and concurrency

- The menu's visibility is recomputed from the cursor's byte position and the
  region's anchor every frame, so it cannot drift from the cursor.
- A region invalidated by a document change is dismissed with the menu, and
  the dismissal is stated in the status cluster rather than being silent.
- The Edit action clears the outstanding request if one is in flight for the
  same region, and the new request supersedes it.

### Frontend or operator surface

- **The geometry.** The menu's anchor is the cursor's **display column** on
  the cursor's **screen row**, both computed from the editor's own viewport
  and the row's horizontal scroll offset. A byte offset is not a column: the
  anchor must account for tabs and wide characters, and it must be `None`
  when the cursor is scrolled out of the window, in which case the menu does
  not open.
- **The placement.** The menu is drawn below and to the right of the cursor,
  flipping above and to the left when it would leave the text area, and it is
  clamped rather than clipped. A menu that is partly outside the text area
  reads as a rendering defect.
- **The visibility.** The cursor is "in the region" when its document position
  lies within the region's anchor span. The region's own bytes are not
  document bytes, so the comparison is in document coordinates and the
  region's display height does not enter it.
- **The verbs.** Approve and Deny are M001's actions, reached through the
  menu. Edit is new: it discards the region and re-opens the prompt field
  with the previous instruction, so the operator can revise and retry without
  retyping.
- **The keys.** While the menu is open it claims its three verbs. Every other
  key falls through to the editor's Normal table.

### Security and authorization

None new. The menu introduces no request.

### Documentation and static guards

- The editor's keymap overlay gains the menu's verbs.
- `docs/tui.md` gains the menu's gesture, in the same change.
- `architecture/tui.md` gains the menu's geometry and visibility contract.
- The key-authority guard must see the menu's claimed verbs and must not see
  them claimed twice.

## 7. Ordered work packages

### Work package A — The anchor and the placement

Intent: the menu is drawn where the cursor is, and never leaves the text area.

Required changes: the cursor's display column and screen row; the flip and
the clamp; the `None` case for a cursor scrolled out of the window.

Acceptance evidence: a rendering test that the menu is drawn at the cursor's
cell; a test that a cursor on the last visible row flips the menu above; a
test that a cursor near the right edge clamps the menu; a test that a cursor
scrolled out of the horizontal window opens no menu.

### Work package B — The derived visibility

Intent: the menu is open exactly when the cursor is in the region.

Required changes: the in-region test in document coordinates; the
recomputation on every cursor movement; the dismissal when a region is
invalidated.

Acceptance evidence: a test that the menu opens when the cursor enters the
region and closes when it leaves; a test that the menu does not open for a
cursor outside the region; a test that an invalidated region dismisses the
menu.

### Work package C — The verbs

Intent: Approve and Deny do what they do everywhere else, and the menu
claims only its own keys.

Required changes: the menu's key handling; the fall-through to the editor's
Normal table for every other key.

Acceptance evidence: a test that the menu's Approve produces the same
document outcome as M001's approve; a test that the menu's Deny produces the
same outcome as M001's discard; a test that an unclaimed key reaches the
editor's Normal table and moves the cursor.

### Work package D — Edit

Intent: the operator can revise the instruction and try again.

Required changes: the Edit action; the field's re-open with the previous
instruction; the cancellation of an in-flight request for the same region.

Acceptance evidence: a test that Edit clears the region and re-opens the
field with the previous instruction; a test that a request in flight for the
discarded region is superseded and its answer dropped; a test that a second
attempt produces its own region and the first region's residue is gone.

### Work package E — The guards and the docs

Intent: the change is discoverable and policed.

Required changes: the keymap overlay rows; the `docs/tui.md` entry; the
architecture account; the key-authority guard's view of the new claimed
verbs.

Acceptance evidence: the guard runs and the self-tests.

## 8. Failure, cancellation, restart, and contention semantics

- **A cursor inside a region whose payload was invalidated** shows no menu,
  and the status cluster says the region was discarded.
- **A cursor scrolled out of the horizontal window** shows no menu; the menu
  reappears when the cursor is visible again.
- **A document change while the menu is open** invalidates the region, closes
  the menu and states the reason.
- **A request in flight when Edit is pressed** is superseded; its answer is
  dropped by the existing request-id and generation fence.
- **A daemon restart** discards the payload, so the menu disappears with it.
  That is the stated non-durability of the payload, not a defect.
- **Contention.** The menu is a function of the cursor and the payload, so two
  editors of the same document in different frontends each see their own.

## 9. Compatibility and migration

None. No protocol, storage or event change. The menu is a frontend surface
over M001's payload and actions.

## 10. Required tests

### Focused unit tests

- The cursor's display column and screen row, including tabs and wide
  characters, and the horizontal-scroll offset.
- The in-region test in document coordinates.
- The flip and the clamp at each text-area edge.

### Integration tests

- The cursor enters the region and the menu opens; the cursor leaves and it
  closes.
- The menu's Approve, Deny and Edit each produce their document-level
  outcome.
- Edit re-opens the field with the previous instruction and a second attempt
  replaces the first region.
- The document's rows do not move when the menu opens.

### Restart and recovery tests

- A daemon restart discards the payload and the menu with it.
- A document change invalidates the region and closes the menu.

### Contention and cancellation tests

- Edit supersedes an in-flight request and its answer is dropped.

### Security and negative tests

- The menu renders no document text beyond the region's own bounded payload.
- The generated text is absent from the logs.

### Migration and compatibility tests

None. This milestone changes no wire shape.

## 11. Required verification commands

```bash
# focused tests first
cargo nextest run -p codegg-tui --profile ci -E 'test(/inline_assist/) or test(/editor/)'

# static guards
python3 scripts/check_tui_editor_key_authority.py --self-test
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_text_authority.py --self-test
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite appropriate to the change
scripts/verify.sh scoped
```

## 12. Documentation updates

- `architecture/tui.md` — the menu's geometry, its visibility rule and its
  verbs.
- `docs/tui.md` — the menu's gesture.
- The editor's keymap overlay — the menu's verbs.

## 13. Acceptance criteria

- Moving the cursor into the region raises a menu at the cursor; moving it
  out dismisses the menu.
- The menu is drawn at the cursor's display column and flips rather than
  clipping at the text area's edges.
- Opening the menu does not move the document's rows.
- Approve, Deny and Edit each produce the outcome they name, and Approve and
  Deny produce the same outcomes as M001's actions.
- Edit restores the previous instruction in the prompt field.
- An invalidated region dismisses the menu and says why.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- M001 is not closed, or M001's pending region is not the option this plan
  assumes.
- The cursor's display column cannot be computed from the editor's own
  geometry without reaching into the composer's, which the editor does not
  use.
- The menu cannot be drawn without reserving a row, which would reflow the
  document under the reader's first keystroke.
- The menu's visibility cannot be derived rather than latched.
- The work would expand into a diff view, a multi-region menu, or the desktop
  frontend.

## 15. Closure evidence required

- The requirement-to-evidence matrix for every exit condition in the roadmap's
  Milestone 2.
- The command-by-command verification record, with local and hosted results
  labelled truthfully.
- The guard runs and their self-test output.
- A live `tui-test` record of the menu opening and closing with the cursor,
  and of each of the three verbs, against an isolated daemon.
- The measured comment-budget before-and-after, with the manifest unchanged.
- An explicit statement of what was **not** verified.

## 16. Handoff notes

- **The geometry is the work.** Everything else in this milestone is small.
  Budget the time on the display column, the horizontal scroll offset and the
  flip, and write the rendering tests before the key handling.
- **The menu must not reserve a row.** The editor's own chrome rule is
  explicit that an allocated row re-resolves the viewport.
- **Derived, not latched.** A latched menu is a menu that can be on screen
  when the cursor is not in the region, which is the failure the user's
  request names explicitly.
