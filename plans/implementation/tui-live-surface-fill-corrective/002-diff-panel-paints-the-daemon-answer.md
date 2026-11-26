# TUI Live Surface Fill Corrective M002 — Diff Panel Paints the Daemon Answer

Status: implemented — conditionally closed
Closure record: `plans/closure/tui-live-surface-fill-corrective/002-status.md`
Repository baseline: `d319afec15b8b8b33ced2c0af7a3ee50eb1a82c3`

Source roadmap: `plans/subsystems/tui-live-surface-fill-corrective-addendum.md#m002--diff-panel-paints-the-daemon-answer`
Long-term requirements:
- `plans/000-long-term-specification.md#44-frontends-render-projections`
Applicable ADRs: none.
Primary class: capability

## 1. Objective

The Diff panel the user selected shows the daemon's answer: a diff, an explicit empty state, or a typed refusal. It does not stay on Loading, and it does not only toast.

## 2. Why this milestone is ready

No hard dependency is open. The daemon `diff` command and `DiffDialog` already exist. `tui-panel-surface-convergence-corrective` M001–M003 are closed and are not reopened. Their acceptance covered chrome, keymap, and placeholder copy. It did not require a Diff completion to paint the panel that started the read.

## 3. Current implementation evidence

- The Diff arm of `open_panel_box` calls `start_diff` with an empty path and returns `PanelFill::Loading`. The placeholder is set to Loading and is not cleared by any completion arm.
- `apply_diff_finished` toasts on transport error, `CoreResponse::Error`, unexpected variant, and `old_content == new_content`. The success arm calls `process_msg(OpenDiffDialog)`, which enqueues `TuiCommand::OpenDiffDialog`.
- `handle_open_diff_dialog` stores a `DiffDialog` and opens `Dialog::Diff`. It does not adopt the widget into `SidePanelId::Diff`.
- The daemon bare branch runs `git diff` and returns `old_content` empty and `new_content` set to that text. A clean tree therefore takes the equal-content toast. A dirty tree opens a modal that compares an empty file with a unified patch, which is not the panel the user selected.
- Dialogs render after panels, so a modal can cover the column. The column itself remains the Loading placeholder.

## 4. Invariants that must not regress

- The TUI does not spawn `git` and does not read the worktree for this command.
- A typed daemon refusal is shown as that refusal. No local fallback diff.
- Equal content is an empty state, not a fabricated diff.
- Leaving the Diff panel before completion does not adopt the result into a different panel.

## 5. Scope

In: the Diff panel completion path, including clean, dirty, refused, and failed answers.

Out: `Ctrl+Alt+D` example diff, per-file review, and any new git command. Tree stays out of scope.

## 6. Required production changes

- Frontend only, unless the bare `GitDiff` result cannot be rendered without mislabeling a patch as a file body. In that case, render the patch text in the panel. Do not add a second read.
- `apply_diff_finished` clears Loading on every arm. Success adopts the viewer into the Diff panel. Error and refusal set the panel's cannot/refusal state and may also toast.
- Equal content sets an explicit empty state in the panel. It must not leave Loading.
- A modal may still open from `/diff` typed with no panel selected. A read started by the panel paints the panel. Do not open a modal over the panel and call that the display.

## 7. Ordered work packages

### W1 — Failing completion receipt
Intent: prove a successful and a clean completion leave the Diff panel on Loading.
Required changes: a test that runs the completion applier after the panel arm has set Loading. It fails on this baseline if the panel is still Loading or if only a modal was opened.
Acceptance evidence: baseline failure.

### W2 — Paint every arm in the panel
Intent: dirty, clean, refused, and failed each change the panel the user is looking at.
Required changes: §6. The dirty arm must show the daemon text, not an empty viewer.
Acceptance evidence: four assertions, one per arm, on the panel state rather than on a toast alone.

### W3 — Do not misrender the bare patch
Intent: a unified diff in `new_content` with an empty `old_content` is shown as that patch, or the result type gains an explicit patch field if the viewer cannot do so without lying.
Required changes: whichever of those two the current `DiffViewer` can do without a second git read. If it cannot, stop and report. Do not invent old/new file bodies.
Acceptance evidence: a fixture patch is visible in the panel body.

## 8. Failure, cancellation, restart, contention semantics

- Transport error and `CoreResponse::Error`: panel shows the refusal. Loading ends.
- Unexpected variant: panel shows that the response was not a diff. Loading ends.
- User switches panels before completion: do not adopt into the newly selected panel. Clear the Diff placeholder if it is still Loading.
- A second Diff select while a read is in flight: one completion paints once. Do not stack two modals.

## 9. Compatibility and migration

None. `/diff <path>` keeps the daemon file-body result. The bare branch's empty-old plus patch-text shape is existing protocol; this milestone renders it honestly.

## 10. Required tests

- Baseline-failing Loading survival test.
- Dirty, clean, refused, and failed each end Loading and change the Diff panel.
- A completion after the user left Diff does not adopt into another panel.
- No test spawns `git` from the TUI.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --test tui_render -- diff
scripts/verify.sh scoped
```

Run the family that owns the new test if the filter above matches nothing. Do not substitute `cargo check`.

## 12. Documentation updates

If the Diff panel's empty copy changes, keep it in the panel placeholder vocabulary already used by the host. No canonical edit.

## 13. Acceptance criteria

- Selecting Diff on a dirty tree shows the diff in that panel.
- Selecting Diff on a clean tree shows an explicit empty state in that panel, not a stuck Loading column and not only a toast.
- A daemon refusal is visible in that panel.
- The TUI still does not read the worktree itself.

## 14. Stop conditions

- Stop if painting the bare result requires a new git read or a frontend worktree walk. Report the result shape.
- Stop if adopting a dialog into the hosted panel is impossible without breaking the closed chrome contract. Report the seam. Do not paint a second unofficial viewer beside the panel.
- Stop if the command catalog does not return `CoreResponse::GitDiff` for the panel's request. Report the variant. Do not parse a string out of an error toast.

## 15. Closure evidence required

- Baseline failure and post-change pass of the completion test.
- Focused command and `scripts/verify.sh scoped`, labeled local.
- A manual or socket-attached note for one dirty tree and one clean tree, each naming what the Diff panel showed.

## 16. Handoff notes

The closed panel-surface record already said Diff reaches a placeholder on the clean-tree path. Do not cite that closure as proof this symptom is fixed. The symptom is the panel after completion.
