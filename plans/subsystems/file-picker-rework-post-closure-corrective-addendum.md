# File Picker Rework Post-Closure Corrective Addendum

Status: active (M001 active; M002 ready for handoff)

Repository baseline audited: `6e097dd3a` (`Merge pull request #180 from dbowm91/fix/repository-audit-corrective-implementation`, 2026-10-23)

Parent: `plans/subsystems/file-picker-rework-roadmap.md` — **conditionally closed** (M001 implemented and committed on `main` as `b4f4b916f`; closure record `plans/closure/file-picker-rework/001-status.md` records it as *conditionally closed*; M002 candidate not approved for planning).

Long-term references:

- `plans/000-long-term-specification.md` — TUI-first product; correctness before transparent magic.
- `plans/001-terminology-and-domain-model.md` — workspace-relative path identity; document attachment.
- `plans/002-long-term-roadmap.md` — TUI evolution on frontend-neutral projections.

Related ADRs:

- ADR-0022 (daemon-authorized workspace directory listing) — consumed unchanged.
- ADR-0033 (directory-tied project identity) — consumed unchanged.

No new ADR is required. Nothing in this addendum changes an identity, an ownership
boundary, a protocol, an authorization rule, a persistence format, or a compatibility
contract. It corrects **which copy of an existing frontend state is authoritative**, and
it **reopens no closed decision** — see §14.

## 1. Purpose and ownership boundary

The workspace file navigator (`/files`) is one widget presented in three places: a
modal box, a docked side panel, and the editor-side Ctrl+T tree pane.
`architecture/tui.md` states that these "all run the *same* `FileTreeState` navigation,
so a fix in one is a fix in all three". At the baseline that claim is **false for the
docked panel**, and the modal box has a defect of its own.

This addendum owns:

- the **ownership** of the navigator's shared tree state between the app and its widget;
- the **`/files` exit verb** in its modal form;
- the **focus handoff** from a `/files` surface to the editor.

It does not own the listing protocol, the tree pane's own keymap, the daemon listing
worker, the editor, or the panel host's chrome contract.

## 2. Work classification

### Invariants

- **I1 — One authority for the navigator's tree.** The navigator's tree structure — its
  nodes, expansion set, scope and selection — is owned by exactly one value. A surface
  may hold a **read-only view** of it; it may not hold a second mutable copy that can
  disagree.
- **I2 — A view never writes the authority.** Drawing a surface must not change what
  the other surfaces see.
- **I3 — A view is never the source of truth.** A surface's view is derived from the
  authority and carries no independent mutation.

### Capabilities

- **C1 — The docked Files panel navigates like the box.** `l`, `Enter`, `h`, `r`, `j`,
  `k`, `Ctrl+d`, `Ctrl+u` and `i` behave identically in the docked panel and in the
  modal box. This is the defect the user reported.
- **C2 — The box's `h` verb works.** Ascending is observable in the modal box, not only
  in the authority.
- **C3 — `q` closes the `/files` box.** The modal box is dismissible with `q`.
- **C4 — Opening a file hands the keyboard to the editor.** From any `/files` surface,
  opening a file moves the keyboard to the editor, and the editor starts in its Normal
  mode.

### Infrastructure

- **A1 — A typed seam for reaching a hosted panel's widget.** The app needs one
  documented, type-checked way to hand the authority to a widget the panel host owns.
- **A2 — A revision counter on the shared tree state**, so a surface's view is
  refreshed only when the authority actually changed.

### Polish

- **P1 — The published keymap is truthful.** The hint string, `architecture/tui.md` and
  `docs/tui.md` describe the same verbs, and `l` is described as the peek it is.
- **P2 — No dead code in the surface.** The widget's unused listing-adoption method is
  removed or made the refresh's mechanism.

Infrastructure and polish are **not** presented as completed capability.

## 3. Non-goals

- The Ctrl+T tree pane's own keymap, chrome and behaviour are unchanged.
- The daemon listing protocol, the bounded walk, the worker-permit model and the
  listing DTO are unchanged (ADR-0022 consumed unchanged).
- No key is added to the tree pane.
- **The `/files` box's filter stays box-local.** This is a closed decision — see §14.
- `Ctrl+B` keeps toggling the sidebar. It is not repurposed.
- `Shift+Q` keeps its meaning everywhere it has one today.
- No new panel, no new route, no new command.
- The editor's own modes, keymap and undo are unchanged. The editor already starts in
  Normal mode; that fact is pinned by a test, not re-implemented.

## 4. Current state

All statements below were read at baseline `6e097dd3a`.

### 4.1 The tree has two copies, and only one of them is reachable

`App` owns `file_tree_state: FileTreeState` — the authority. The `/files` widget
(`src/tui/components/dialogs/files.rs`) owns `pub state: FileTreeState` — a **clone**
taken when `/files` opens (`src/tui/app/modal.rs`, `open_file_picker`). The widget
cannot reach `App` from `render`, so the clone exists.

Every navigation verb changes the authority through the command layer
(`src/tui/commands/file_tree.rs`). The clone must therefore be pushed back into the
widget after each change. That push-back exists in exactly **one** place: the mirror
inside `apply_listing`, gated on `app.focus_manager.has_dialog(DialogType::Files)`.

A docked panel's widget is owned by `SidePanelHost`, not the focus manager.
`dock_open_box` (`src/tui/commands/panels.rs`) calls `close_dialog()` — which unmounts
the widget from the modal stack — and then adopts it into the panel. The gate is
therefore false for a docked panel, and the panel's widget keeps a **frozen** copy for
as long as it is docked. The panel renders that copy, so:

- `l` on a directory expands it **in the authority** and re-walks the tree; the panel
  never shows the expansion;
- `h` ascends in the authority; the panel never collapses;
- `r` re-walks; the panel never repaints;
- `j`/`k` move a cursor in the panel's own copy, which the authority does not see.

The mirror's own comment says it exists "so without this a refresh would repaint the
Ctrl+T pane and leave the box showing the previous listing" — the intent was right and
the predicate was incomplete.

### 4.2 The `h` verb is inert in the modal box too

`ascend_one_level` has three arms. `PeekUp::Slide` re-walks the tree, so a listing
lands and the mirror runs. `PeekUp::Leave` and `PeekUp::Collapse` mutate the authority
(`leave_into`, `collapse`) and **do not re-walk**, so no listing lands and the mirror
never runs. Ascending therefore changes nothing the user can see — in the box and in
the panel alike.

The mirror is bound to a listing **completion**. A collapse is a state **mutation**.
That is the whole defect.

### 4.3 The widget owns a second cursor

`FilesDialog::handle_key` mutates **its own copy** for `j`, `k`, `Ctrl+d` and
`Ctrl+u`, while `l`, `h`, `Enter` and `r` are intents resolved against the
**authority**. Two cursors therefore coexist: the authority's selection is stale
relative to the widget's, so `h` ascends from the wrong row; and every mirror replaces
the widget's cursor with the authority's, so `r` visibly snaps the cursor back to
where it was before the `j`/`k` presses.

`l` already carries a **path** rather than a cursor index, which is the right shape: it
does not depend on two copies agreeing about an index. `h` does not, and that is the
asymmetry.

### 4.4 `Enter` and `l` are not one verb, despite both key tables saying they are

`Enter` on a file emits `TuiMsg::FilesOpenSelected`, which calls `open_editor` directly
and **never arms the editor handoff**. `l` on a file emits `TuiMsg::FilesDescend`, which
reaches `descend_to`'s `PeekStep::Open` arm — and that arm **does** arm the handoff.

Both `architecture/tui.md` and `docs/tui.md` publish `Enter` as "the same motion as
`l`". In the code they are two paths, and only one of them hands the keyboard over.

### 4.5 The handoff does not release the side panel

`hand_off_to_editor` clears `app.ui_state.tree_focused` — the **Ctrl+T tree pane** — and
enters the editor in Normal mode. It never touches the side panel's focus. So opening a
file from the docked Files panel opens the editor and leaves the **panel** holding the
keyboard.

### 4.6 The editor's mode is already correct — this is a finding, not a defect

`open_editor` → `open_editor_in` → `enter_editor_normal` sets `InputMode::Normal` and
`EditorMode::Normal`, routes to `Route::Editor`, and the document's own reset puts the
editor's mode back to `EditorMode::Normal` (the default). **The editor already starts in
Normal mode.** The missing half of the user's requirement is the focus switch, not the
mode.

### 4.7 Two smaller findings

- `FilesDialog::apply_listing` has **no production caller**; the mirror replaces
  `dialog.state` wholesale instead.
- The widget's `HINT` describes `l` as "open" and `Enter` as "open", while the published
  tables correctly describe `l` as a **peek** — the directory opens in place and the
  cursor moves into it.

## 5. Target architecture

### 5.1 One authority, read-only views everywhere

`App::file_tree_state` is the single owner of the navigator's tree **structure**: its
nodes, expansion set, scope, selection and pending cursor. The `/files` widget holds a
**read-only view** of that structure for rendering, plus the things that are genuinely
per-surface: the filter text, the insert mode, and the scroll offset.

The widget's view is a distinct type from the authority, so it **cannot** hold an
authoritative field it might mutate. That is the invariant made structural rather than
conventional.

The **filter stays per-surface.** The box's filter is box-local and the tree pane's
filter is the authority's, exactly as the closed `tui-buffer-chrome-and-panel-navigation`
M002 decided (§14). The two surfaces can therefore show different rows — that is the
accepted contract, not a defect this addendum fixes.

### 5.2 The selection is a path, and every verb names one

`selected` is already a path. The widget's `l` already names a path. The remaining
cursor-dependent verb is `h`, which today resolves "one level up" against the
**authority's** selection — which is the stale one.

So `h` becomes path-bearing too: the surface names the row it is on, and the command
layer resolves the relation **structurally**, against the unfiltered row set. "The
parent of this row" is a structural question, not a filtered one, and `peek_into`
already resolves that way.

Movement (`j`, `k`, `Ctrl+d`, `Ctrl+u`) becomes path-bearing in the same way: the
surface names the row it wants — it knows its own filtered rows — and the authority
records the selection.

### 5.3 The view is refreshed at render time

The authority's structure is pushed into every mounted `/files` surface **once per
frame, before those surfaces are drawn**, gated on a revision counter so the view is
copied only when the authority actually changed.

Render-time refresh is chosen over refresh-at-each-mutation-site because a per-site list
of mutations is a list that can be incomplete: the `h` defect is exactly that list
having been incomplete. A revision counter makes the refresh total.

The sync point is the app's frame entry point, ahead of both the side-panel draw and the
modal-dialog draw.

### 5.4 The exit verb is box-only, and it is explicit

`q` closes the **modal box**. In the **docked panel** `q` does nothing — the panel is
left with `Shift+Q`, and hiding the column remains `Ctrl+B`'s job. This is the user's
decision, and it is also what the closed `tui-buffer-chrome-and-panel-navigation` M004
requires (§14).

The intent is resolved by the app, not by the widget: the app closes the box only when a
`/files` **dialog** is mounted. The widget cannot know whether it is modal or hosted,
and a bare `CloseDialog` in the docked panel would pop an unrelated modal stack — the
`/plan` box documents that failure mode already.

### 5.5 Opening a file hands the keyboard over, from every surface

`Enter` and `l` become one verb: both arm the same editor handoff, and the handoff
releases **both** the tree pane and the side panel before entering the editor in Normal
mode. The editor's Normal mode is already correct and is pinned by a test.

## 6. Dependency graph

| Milestone | Hard deps | Interface deps | Soft deps |
|---|---|---|---|
| M001 — shared tree state authority | none | `Component`, `SidePanel`, `HostPanel`, `FileTreeState`, `TuiMsg` | none |
| M002 — exit verb and editor handoff | M001 (the same intent plumbing and the same authority) | `FocusManager`, `open_editor`, `SidePanelHost::blur` | none |

M001 and M002 are **not** independently releasable in the strict sense: M002's handoff
is easier to prove on top of M001's single authority, because the handoff's correctness
depends on the surface's view being current when the file is opened. They are registered
as two milestones because they are two separable correctness boundaries with two
separable acceptance surfaces, and because M001 is the larger, riskier change that
benefits from landing and being observed first.

No ADR is required for either.

## 7. Milestones

### M001 — One authority for the navigator's tree

Class: **invariant** (I1–I3) with a **capability** consequence (C1, C2).

Objective: make `App::file_tree_state` the single owner of the navigator's tree
structure, turn every `/files` widget verb into a path-bearing intent resolved against
it, and refresh each mounted surface's view from it before that surface is drawn — so the
docked panel and the modal box navigate identically, and the `h` verb is observable in
both.

Exit conditions:

- The docked panel's `l` expands a directory, `h` collapses it, `Enter` enters it, `r`
  repaints it, and `j`/`k` move a cursor the authority sees.
- The modal box's `h` collapses an entered directory, observably.
- The widget holds no mutable copy of the navigation structure.
- The box's filter stays box-local; the tree pane's filter stays in the authority.
- The refresh costs one copy per changed authority per frame, not one per frame.

### M002 — The box's exit verb and the editor handoff

Class: **capability** (C3, C4).

Objective: make `q` close the `/files` modal box, and make opening a file from any
`/files` surface hand the keyboard to the editor — which starts in Normal mode —
instead of leaving the panel holding it.

Exit conditions:

- `q` in the modal box closes it.
- `q` in the docked Files panel does nothing: the panel stays docked and focused, and
  `Shift+Q` remains its exit.
- `Enter` on a file and `l` on a file behave identically: the editor opens, the side
  panel releases the keyboard, the tree pane releases the keyboard, and the editor is
  in Normal mode.
- The published keymap — the hint string, `architecture/tui.md` and `docs/tui.md` —
  describes the same verbs, and says that `l` peeks.

## 8. Cross-cutting requirements

- **Storage**: none. No migration, no schema change, no `STORAGE_LAYOUT_VERSION` move.
- **Protocol**: none. The listing DTO and ADR-0022 are consumed unchanged.
- **Security**: none. No new authority, no new read surface, no new path resolution. The
  daemon remains the only filesystem reader (`scripts/check_daemon_listing_authority.py`
  must stay green).
- **Concurrency**: the app is single-threaded for TUI state; the listing round-trip is
  generation-guarded and its completion is already stale-checked. The revision counter
  rides alongside the existing generation/request-id discipline and must not weaken it.
- **Render cost**: the refresh must not introduce a per-frame copy of the whole tree.
  The revision counter is the mechanism, and `scripts/check_tui_render_cost.py` must stay
  green.
- **Docs**: `architecture/tui.md`, `docs/tui.md`, and the widget's hint string describe
  one keymap.

## 9. Verification strategy

- Focused unit tests beside the changed modules for each verb and each refresh path.
- An integration family member that drives the **real** path — open `/files`, dock it
  with `Shift+L`, press the verbs — and asserts the observable result, because the
  defect lives in the seam between two surfaces and no unit test of either alone can
  see it.
- A static guard that the published keymap and the widget's hint describe one set of
  verbs, modelled on `scripts/check_tui_editor_doc_drift.py`, wired into **both**
  `scripts/verify.sh`'s `run_guards()` and `.github/workflows/ci.yml`, with a
  `--self-test` proving it is not vacuous.
- `scripts/verify.sh quick` green, and `cargo clippy --workspace --all-targets --locked
  -- -D warnings` clean.
- The daemon-listing-authority guard and the TUI render-cost guard stay green.

## 10. Risks and decision points

- **R1 — The widget cannot reach `App` from `render`.** This is why the clone exists.
  The view design keeps that constraint and moves the *refresh* to the app's frame pass,
  which is not a widget render.
- **R2 — The two surfaces' filters differ, so their row sets differ.** This is the
  accepted contract (§14), not a defect. It does mean a selection can name a row the
  other surface's filter excludes; that is already true today and is not changed here.
- **R3 — A per-frame copy of the tree would regress render cost.** Mitigated by the
  revision counter; `scripts/check_tui_render_cost.py` is the gate.
- **R4 — The `q` verb touches the program's quit key.** Inside a modal the key never
  reaches the app's quit arm, so the two do not collide; the sequence is "close the box,
  then `q` twice to leave the program", which the existing two-press exit rule already
  governs. `Shift+Q` is retained everywhere.
- **R5 — Scope creep into the tree pane's keymap.** Out of scope; a stop condition.
- **R6 — A view type is a new type in a hot module.** It must carry no behaviour that
  duplicates `FileTreeState`'s, and its construction must be one function.
- **D1 (decision) — Render-time refresh, not per-mutation refresh.** Chosen because a
  per-mutation list can be incomplete, and the `h` defect is exactly such a gap. A
  revision counter makes the refresh total.
- **D2 (decision) — The filter stays per-surface; the selection becomes path-bearing.**
  The filter is a closed decision (§14). The selection is made path-bearing rather than
  index-bearing because a path is the one thing both surfaces can name without agreeing
  on a filter or an index.

## 11. Completion definition

This addendum is complete when:

1. M001 and M002 are closed with closure records, and every acceptance criterion in
   both plans is satisfied by evidence recorded in those records.
2. The docked Files panel and the modal box navigate identically, proven by the
   integration family member rather than by assertion.
3. `q` closes the box and does nothing in the docked panel.
4. Opening a file from any `/files` surface hands the keyboard to the editor, in Normal
   mode.
5. The published keymap and the widget's hint describe one set of verbs, and a static
   guard keeps them from drifting.
6. `scripts/verify.sh quick` is green, workspace clippy is clean, and the daemon-listing
   authority guard and the TUI render-cost guard are green.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | active | `plans/implementation/file-picker-rework-post-closure-corrective/001-one-authority-for-the-navigator-tree.md` | — | none |
| M002 | ready for handoff | `plans/implementation/file-picker-rework-post-closure-corrective/002-box-exit-verb-and-editor-handoff.md` | — | M001 |

## 13. Relationship to the parent roadmap

`plans/subsystems/file-picker-rework-roadmap.md` stays **conditionally closed**. This
addendum does not reopen M001 or M002 of that roadmap and does not claim M002's
candidate scope (typeahead, a `..` row, narrow-column guide policy). It is a
**post-closure corrective**: a new line of work on a defect discovered after that
roadmap's closure record was written. The parent roadmap's M002 remains not approved.

The parent's M001 closure condition — that a commit must exist at which
`tui-panel-surface-convergence-corrective` M001 can re-inspect `component.rs` and
`hosted.rs` — is discharged by `b4f4b916f` and the subsequent history; this addendum
does not reopen it.

## 14. Closed decisions this addendum must not reopen

The authority order is canonical long-term → accepted ADRs → subsystem roadmap →
implementation plan → repository evidence. Three closed decisions in this area bear
directly on the work, and each is carried forward rather than reopened.

### 14.1 The box's filter is box-local

`plans/closure/tui-buffer-chrome-and-panel-navigation/002-status.md` §4 records it as a
**deviation**: *"The `/files` box's filter is box-local: it lives in the box's copy of
the shared state and does not survive the box being closed, while the Ctrl+T pane's
filter lives in the app's shared state and does. … Sharing one filter across the two
surfaces would mean the pane's filter and the box's filter are the same string, which
is a different contract from the one that was asked for."*

**This addendum keeps the filter box-local.** M001 therefore does not unify the filters,
and the selection is made path-bearing instead (§5.2) so that coherence does not depend
on the two surfaces agreeing about a filter. Unifying the filter would be a contract
change that needs its own decision, not a mechanical consequence of this work.

### 14.2 `q` is the program's quit key; `Shift+Q` is the panel exit; `Esc` is the mode switch

`plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` M004 and its
closure record `plans/closure/tui-buffer-chrome-and-panel-navigation/004-status.md` §2
established: *"`is_exit_key` matches `Shift+Q` only — `q` is the program's quit key, and
one press that meant both 'leave this panel' and 'quit the program' was two meanings for
one key."* The same record states that *"a bare `q` inside the chat panel is inert"*
and that `Esc` is the mode switch, never an exit.

**M002 honours all of it.** `q` outside the `/files` box remains the program's two-press
quit key. `Shift+Q` remains the panel's exit and the box's close. `Esc` remains the mode
switch. And in the **docked** Files panel a bare `q` is **inert**, exactly as it is in
the chat panel. The only addition is that a bare `q` inside the `/files` **modal box**
closes that box — a surface the modal already owns the keyboard for, and a key that
today reaches nothing at all there, because a modal consumes unhandled keys before the
app's bindings are consulted. The program's quit key is not taken away from anywhere it
works today.

### 14.3 `Enter` and `l` are one motion

`plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` M001 made `Enter`
descend, and `plans/closure/tui-buffer-chrome-and-panel-navigation/001-status.md`
records it. M002 preserves that contract and only makes `Enter` and `l` agree about the
**editor handoff** as well, which is what both published key tables already claim.

### 14.4 Prior art on the parity claim

`research/tui-files-panel-depth-and-input-chrome-2026-10-21.md` §1.1 already recorded
that the parity claim in `architecture/tui.md` *"is false for the filter and false for
`Enter`"*. The `Enter` half was fixed by that roadmap's M001. The **filter** half was
deliberately left as the recorded deviation in §14.1. The **state-ownership** half —
the mirror not reaching a hosted panel, and the widget's second cursor — is recorded
there as a surface map and a defect list, and is not closed by any milestone. This
addendum is that corrective; it does not claim the discovery as new, and it cites the
research artifact as prior evidence.
