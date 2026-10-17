# TUI Panel Surface Convergence Corrective Addendum

Status: proposed (M001 closed; M002/M003 not approved)

Repository baseline audited: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`
(`feat(tui): editor cursor, readable colours, in-file / search, syntax colours`, 2026-10-16),
plus the **uncommitted** `file-picker-rework` M001 working-tree changes present in that
worktree. Re-inspect before each implementation pass.

Parent roadmaps (both closed; this addendum does not reopen them):

- `plans/subsystems/tui-editor-correctness-usability-roadmap.md` — panel host provenance.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — M006 panel host seam.

Adjacent, **in-flight and not owned by this addendum**:

- `plans/subsystems/file-picker-rework-roadmap.md` — M001 **conditionally closed**,
  implemented in the working tree but uncommitted. See §5.

Long-term references:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Related ADRs: none required. This workstream changes no identity, ownership boundary,
protocol, storage layout, or authorization semantics. It converges presentation on seams
that already exist. If implementation discovers that a canonical owner or wire contract
must change, that milestone **must stop** and request a separate ADR and plan.

## 1. Purpose and corrective trigger

The side-panel host is a good design that was never finished. `Component` gained three
optional seams precisely so that a widget could be hosted without re-deciding how it looks
or which keys it has — `docks_as_viewer()`, `keymap_hint()`, and `render_in_panel()` — and
`render_hosted` documents the exact failure the last of these exists to prevent. As of the
audited baseline, one widget of thirty-three has adopted any of it.

The consequence is that `/panels` reads as thirty-three hand-built boxes. They disagree on
chrome, on colour sourcing, on where the keymap lives, and on what a panel says when it has
nothing to show. The worst instance is user-visible and self-refuting: selecting an empty
panel can render *"Run `/memory-search` to open it, then Shift+← to move it here."* — an
instruction to leave, to move something that is already in place, ending in a gesture whose
meaning depends on where the panel currently sits.

Three panels make that worse rather than better by contradicting themselves in the same
frame: `Goal`, `Doctor`, and `Plugins` toast *"Loading…"* and simultaneously render a
placeholder asserting that nothing has been opened. Two more (`ShellShow`, `Diff`) can
report a reason and leave the same instruction on screen.

One registered panel cannot work at all. `SidePanelId::Workspace` is listed, has a hotkey
slot and a config key, but the only constructor for its box is `#[cfg(test)]` and its mount
arm is an empty comment; the modal path is documented as obsolete. The `dock_target` arm
that would serve it is unreachable.

This addendum owns the smallest coherent pass that makes the panel surface one surface.

## 2. Work classification

**Invariants.** No panel may instruct the user to leave `/panels` in order to fill the panel
they just selected. The host owns chrome: no hosted widget may paint a border, title, or
footer over the rect `render_hosted` has already prepared. A docked box's colour must come
from the live theme, never from a value captured when the box was opened. A failed adoption
is never silent. `SidePanelId` variants, `config_key` spellings, and persisted panel
bindings are frozen.

**Capabilities.** Selecting any panel in `/panels` either fills it, begins a load with an
honest progress state, or states why it cannot act — one step, no composer detour. Every
listed panel is reachable or is not listed. Docked boxes present one shared look: host
border, host title, host focus dot, one status line carrying the widget's own verbs.

**Infrastructure.** A typed outcome from `open_panel_box` replacing the implicit
box-or-nothing convention. A shared dialog chrome/geometry helper. A static guard that
fails when a new hosted widget paints chrome or ships a placeholder instruction.

**Polish.** Placeholder wording. Footer wording. Border-title keymaps. Colour literals in
`plan` and `mcp` selection rows.

## 3. Non-goals

- No new panel, no new `SidePanelId` variant, no change to `config_key` spellings or
  persisted panel bindings, no change to panel placement or sizing.
- No change to what any panel *contains* — no new data, no new daemon call, no new
  protocol field, no new projection. `STORAGE_LAYOUT_VERSION` unchanged.
- No change to the `Enter` / `l` split in the file picker, to the walk, to `local_walk_root`,
  or to any file-picker behaviour. That is `file-picker-rework` M001/M002 territory.
- No rework of bespoke panels' content or keymaps. `Session`, `Chat`, `Terminal`, `Help`,
  and `Agents` keep their own `SidePanel` impls; M001 only stops a silent adoption drop.
- No widget redesign, no visual restyling, no new iconography or typography.
- No `Component` trait default flip. The cost/risk of inverting `render_in_panel` to
  inset-by-one across all implementors is **unestablished**; treat it as a research item in
  M002, not as a starting assumption.

## 4. Current state (repo evidence at baseline)

### 4.1 Three seams, one adoptor

| Seam | Purpose | Implementors |
|---|---|---|
| `Component::docks_as_viewer()` | opt into the shared docking gesture | 16 dialogs |
| `Component::render_in_panel()` | suppress own chrome when hosted | 1 — `dialogs/files.rs` |
| `Component::keymap_hint()` | declare own verbs for the status line | 1 — `dialogs/files.rs` |

`docks_as_viewer` worked: 16 boxes answer the shared `Shift+←/→/↑/↓` gesture correctly, and
`dock_target` covers every dockable box, so no docking gesture is a dead key. The other two
seams have no coverage.

### 4.2 The host's chrome is destroyed on every dock except `/files`

`render_hosted` paints the panel border, its title, and the `●` focus marker into the rect,
then calls `render_in_panel`, which **defaults to `render`**. Any widget that paints its own
`Block` into the same rect therefore overwrites all three. The module documentation in
`src/tui/panels/mod.rs` describes this failure mode by name; it was diagnosed and not
propagated.

At least 15 dockable boxes paint their own chrome. `InfoDialog` alone accounts for 13
`DialogType`s (`Cost`, `Usage`, `Stats`, `Context`, `Logs`, `DoctorReport`, `ShellShow`,
`TaskList`, `GoalShow`, `Collaborators`, `Team`, `Control`, `MemoryResults`). Two further
boxes (`plan`, `diff`) hide their verb list in the border title, so the overwrite destroys
the keymap text as well as the identity.

**The `Placeholder` is itself a clobberer.** It draws its own all-borders `Block` and has no
`render_in_panel` override, so the one widget guaranteed to be a placeholder always erases
its own panel's title and focus marker.

### 4.3 The placeholder instructs the user to perform a gesture that is already done

`src/tui/panels/hosted.rs` renders, verbatim:

```text
No <label> loaded.

Run  <load_command>  to open it,
then Shift+← to move it here.
```

The panel **is** here; there is no box to move, and `Shift+←` names a placement-dependent
gesture. This copy is reached whenever `open_panel_box` does not mount a box — which it does
not, by design, for panels that need an argument, need a receipt, or need an async load.

### 4.4 Three panels contradict themselves in one frame

`Goal`, `Doctor`, and `Plugins` call an async starter and toast *"Loading goal…"* /
*"Running doctor…"* / *"Loading plugins…"*. Their box opens later through the normal
completion path. In the meantime the placeholder is rendered — asserting in the same frame
that nothing was opened and that the user should run the very command already in flight.

`ShellShow` instructs in two places at once: its toast names `/shell-show <id|last>` **and**
the `L` gesture, and the placeholder repeats both. `Diff` can reach the placeholder on the
ordinary path — a clean working tree yields `git diff` with no changes, which toasts *"No
changes detected"* and then displays *"Run `/diff` to open it."*

### 4.5 Two ad-hoc branches inside the one shared seam

`materialize_hosted_panel` special-cases `matches!(id, SidePanelId::Research | SidePanelId::TaskView)`
to toast *"{label} opened — press L to dock it"* and refuse to select, and separately toasts
*"Opened as {label} — press L to dock it"* whenever `dock_target` resolves to a different
panel. Two literal panel ids and two message shapes encode what should be one property of the
widget — whether it is safe to adopt while its load is in flight.

### 4.6 A listed panel that cannot work

`SidePanelId::Workspace` is in `SidePanelId::ALL`, has a hotkey slot and a config key, and is
offered by the manager. `WorkspaceDashboardDialog::new()` is called only from `#[cfg(test)]`;
the `Dialog::WorkspaceDashboard` mount arm is an empty comment; the modal path is recorded as
obsolete in favour of `Route::Workspace`. Consequently `App::dock_target`'s
`DialogType::WorkspaceDashboard` arm can never fire and the panel can only ever render a
placeholder.

### 4.7 A failed adoption is silently discarded

`dock_open_box` closes the dialog and calls `SidePanelHost::adopt(id, widget)`, **ignoring
the returned `bool`**. `adopt` returns `false` for a bespoke panel. `dock_target` maps
`DialogType::Agent` to `SidePanelId::Agents` and `DialogType::ProjectChat` to
`SidePanelId::Chat`, both bespoke — so those gestures unmount and discard a box, then focus a
panel with different content. For `/terminal` the `InfoDialog` the user was reading vanishes.

### 4.8 The status line advertises one hardcoded keymap for twenty-eight panels

`HostPanel::composer_hints` falls back to `GENERIC_KEYS = "j/k scroll · i act"`, shared by
every hosted panel that does not answer `keymap_hint`. Its own doc comment records that this
string "had `i` wrong for every box it described." Fifteen of the twenty dockable boxes
instead hand-roll a footer row inside their own render — a second, divergent place to state
the same verbs, which the chrome overwrite then destroys.

### 4.9 Colour sourcing is inconsistent, and five boxes freeze the theme at open time

Five dockable boxes ignore their `theme` parameter and read a `set_theme()`-populated cache
(`dialogs/plugin.rs`, `dialogs/review.rs`, `dialogs/security_review.rs`, `dialogs/diff.rs`,
`dialogs/ui_node.rs`). A docked box therefore renders stale colours after the user changes
theme, while the rest of the UI updates. `dialogs/mcp.rs` already demonstrates the correct
pattern. Separately, `plan` and `mcp` hardcode `Color::White` for selection foreground over a
themed background, which is unreadable on a light theme, and `plan` carries four
near-identical render implementations with the same literals copied between them.

### 4.10 No shared chrome geometry

There is no dialog chrome helper. `centered_rect` is defined twice, byte-identical, in
`App` and in `dialogs/run_detail.rs`. Sixty-three `Layout::…split(…)` sites across 24 files
hand-type their constraint sets.

### 4.11 Why none of this was caught

`tests/tui_panel_docking.rs` asserts the shared docking gesture for seven boxes — `Cost`,
`Usage`, `Context`, `Theme`, `Mcp`, `Keybind`, `Files` — and its own comment records that the
remainder "are produced by `open_info_dialog` / dedicated helpers rather than by
`open_dialog`, so a test cannot mount them without reaching into `dialog_state`." Nothing
asserts over `SidePanelId::ALL`, so a panel can be listed, unconstructible, and still pass.

## 5. Relationship to `file-picker-rework` M001 (read first)

`file-picker-rework` M001 is **implemented but uncommitted and unclosed** in the working
tree audited here. It is the work that created `keymap_hint()` and `render_in_panel()`,
wired them into `HostPanel::composer_hints`, and adopted both in `FilesDialog`. Its roadmap
already diagnoses §4.2 for one widget (§4.5 there: *"The hosted panel's own title is painted
over"*).

Three consequences bind this addendum:

1. **No seam may be re-created.** `keymap_hint`, `render_in_panel`, and the
   `GENERIC_KEYS` fallback already exist. M002 generalizes what M001 proved on one widget.
2. **`src/tui/panels/hosted.rs` is a shared, currently-dirty boundary.** M001 here edits
   `Placeholder`, which M001-of-file-picker does not, but in the same file and the same
   uncommitted worktree. Serialize.
3. **Do not read `component.rs` as closed.** Both methods may still change shape before that
   milestone closes.

## 6. Target architecture

One property per widget, three questions asked once:

1. *Can this widget be adopted while it is still loading?* — replaces the `Research` /
   `TaskView` literal list. Expressed on the trait, defaulted to the current behaviour.
2. *What chrome does this widget draw?* — already `render_in_panel`. Every widget that
   paints a border, title, or footer inside a hosted panel either overrides it or draws
   nothing.
3. *What verbs does this widget own?* — already `keymap_hint`. The in-panel footer is
   retired in favour of the single status line.

Plus one host-side change: `open_panel_box` returns a **typed outcome** —
`Filled` / `Loading` / `Cannot(reason)` — so the manager renders one honest state per
panel, and the `Placeholder` describes state instead of routing the user through the
composer.

And one guard: a static check over `SidePanelId::ALL` asserting that every listed panel
constructs a box or is bespoke, and that no hosted widget paints chrome.

## 7. Milestones

### M001 — Panel manager outcome contract and registry reachability

**Class:** capability (with polish). **Status: closed** —
`plans/closure/tui-panel-surface-convergence-corrective/001-status.md`. The §8 sequencing
condition resolved; the closure record §6 records what is still outstanding (neither
milestone is committed), and §7.2 there records the one plan defect found.

**Objective:** Selecting any panel in `/panels` fills it, starts a load and says so, or
states why it cannot — in one step, without telling the user to run a `/` command or press
`Shift+←`. Every listed panel is provably reachable or removed from the list.

**Scope:** `src/tui/commands/panels.rs`, `Placeholder` in `src/tui/panels/hosted.rs`,
`SidePanelId::ALL` and `App::dock_target()` reachability, the adoption-failure surfacing in
`dock_open_box`, the `Research`/`TaskView` literal list, and an exhaustive
`SidePanelId::ALL` test.

**Deliverable boundary:** `plans/implementation/tui-panel-surface-convergence-corrective/001-panel-manager-outcome-contract-and-registry-reachability.md`.

### M002 — Chrome and keymap adoption across dockable boxes

**Class:** capability (with polish). **Status: proposed — not approved for planning.**

Generalizes what `file-picker-rework` M001 proved on `/files` to every dockable box:
`render_in_panel` overrides for the ~15 chrome-painting widgets, `Placeholder` included;
`keymap_hint` for every box that owns distinct verbs, retiring in-panel footers and
border-title keymaps in favour of the shared status line; `InfoDialog` as the template that
retires 13 clobber sites at once.

Requires M001 and `file-picker-rework` M001 closed. Includes the `render_in_panel` default
flip only as a **research item with a whole-tree sweep**; the plan must prove non-dockable
implementors are unaffected before adopting it, and may drop it if the evidence does not
support it.

### M003 — Theme sourcing and shared chrome geometry

**Class:** polish. **Status: proposed — not approved for planning.**

Retire the five `set_theme()` colour caches so a docked box follows the live theme; replace
the `Color::White` selection literals in `plan` and `mcp` with theme tokens; collapse
`plan`'s four duplicated render paths behind one; de-duplicate `centered_rect` and give the
box set one shared chrome/geometry helper.

Deferred until M002, so the helper is written against the post-adoption shape rather than
being re-broken by the next chrome change.

## 8. Blockers

| Blocker | Class | Owner | Resolution |
|---|---|---|---|
| `file-picker-rework` M001 is uncommitted and unclosed in the same worktree, and edits `src/tui/panels/hosted.rs` and `src/tui/components/component.rs` — both inside this addendum's M001 boundary | interface | `plans/subsystems/file-picker-rework-roadmap.md` | That milestone commits, verifies, and writes `plans/closure/file-picker-rework/001-status.md`. Until then both seams may change shape and `Placeholder` edits collide in the same file. |
| M002 cannot finalize the `render_in_panel` default flip | hard | M002's own planning pass | A whole-tree sweep over `Component` implementors must establish that non-dockable boxes are unaffected, or the flip is dropped. |

No other dependency is outstanding. No ADR is required.

## 9. Cross-cutting requirements

- **Storage/protocol:** none in any milestone. `STORAGE_LAYOUT_VERSION` unchanged, no DTO
  field added, no projection change.
- **Security/authorization:** none changed. No box gains a read, a fetch, or a privilege.
  Panel removal in M001 must not remove a surface that was the only path to an
  authorization-gated action — if one is found, that is a stop condition, not a deletion.
- **Concurrency:** M001 touches async starters (`goal`, `doctor`, `plugins`) only at their
  **toast** site; the starter and its completion path are unchanged. The typed outcome must
  not convert "load started" into "load succeeded".
- **Layout:** no change to panel placement, sizing, composer height, or the working strip
  row reserved at transcript row 0. The placeholder rewrite must fit its column at the
  narrowest supported width without wrapping mid-word.
- **Docs:** `docs/tui.md` (panel gesture table, panel list) and `architecture/tui.md`
  (panel host module map) land with the code. `plans/registry.md` is updated in the same
  commit as any status change.

## 10. Verification strategy

- Focused tests per box, then `scripts/verify.sh quick` as the gate and `verify.sh full`
  before closure. Never `--all-features`.
- **Render tests assert buffer contents, not string reachability** — the panel title and the
  `●` focus marker must be present in the hosted rect; the placeholder must not contain a
  `Shift+←` or a `/`-command instruction.
- An exhaustive `SidePanelId::ALL` sweep, absent today, is the primary acceptance signal for
  M001 and the regression gate for M002/M003.
- A negative test per milestone: an added `Block` in a hosted widget's `render` fails a
  static guard rather than silently clobbering.

## 11. Risks and decision points

- **`render_in_panel` default flip.** The root-cause fix, and the one change that could
  silently alter every non-dockable box. Gated on a whole-tree sweep; droppable.
- **Removing `SidePanelId::Workspace` is a user-visible surface change.** If `Route::Workspace`
  is not reachable from the manager, the panel must either gain a real entry path or leave
  the list. Do not leave a permanently-empty row in the manager.
- **A guard that is too strict blocks legitimate modal chrome.** Any static check must
  distinguish "hosted" from "modal" rendering, or it will forbid the modal border every box
  legitimately draws.
- **Adoption semantics.** Making a failed adoption visible risks surfacing gestures that were
  always silently dropping a box. That is the point, but the message must name the panel,
  not expose an internal enum.
- **M001 blocks on an uncommitted sibling.** If that milestone stalls, this addendum stalls
  with it. That is preferable to two plans editing one dirty file.

## 12. Completion definition

This addendum closes when selecting any panel in `/panels` produces exactly one of three
honest states with no composer detour and no `Shift+←` instruction; every panel in the list
constructs a box or is bespoke, proven by an exhaustive sweep; no hosted widget paints over
the host's title or focus marker; every dockable box's verbs appear once, on the shared
status line; docked boxes follow the live theme; `verify.sh quick` and `verify.sh full` are
green; and `docs/tui.md` and `architecture/tui.md` match shipped behavior.

M002 and M003 are never implicitly approved. A closure that unblocks nothing downstream is a
valid outcome and MUST be recorded as such in the closure record.

## 13. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | blocked | `plans/implementation/tui-panel-surface-convergence-corrective/001-panel-manager-outcome-contract-and-registry-reachability.md` | — | `file-picker-rework` M001 uncommitted/unclosed; shared `hosted.rs` boundary |
| M002 | proposed (not approved) | — | — | requires M001 + `file-picker-rework` M001 closed; fresh planning pass |
| M003 | proposed (not approved) | — | — | requires M002; fresh planning pass |