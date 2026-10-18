# TUI Panel Surface Convergence Corrective Addendum

Status: active (M001, M002a and M002b closed; M003 approved into planning, plan written)

Repository baseline audited: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`
(`feat(tui): editor cursor, readable colours, in-file / search, syntax colours`, 2026-10-16),
plus the `file-picker-rework` M001 changes that preceded it. §1 and §4 describe that baseline
and are a historical record of the defects, not current counts — the live seam counts are in
§4.1 as re-measured at `f8286a284` and updated by M002a. Re-inspect before each implementation
pass.

Parent roadmaps (both closed; this addendum does not reopen them):

- `plans/subsystems/tui-editor-correctness-usability-roadmap.md` — panel host provenance.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — M006 panel host seam.

Adjacent, **in-flight and not owned by this addendum**:

- `plans/subsystems/file-picker-rework-roadmap.md` — M001 **conditionally closed** and
  committed as `b4f4b916f`. Its `Component::keymap_hint` / `render_in_panel` seams are
  final and consumed here. Its own M002 candidates (typeahead, `..` row, narrow-column
  guide policy) remain deliberately unopened.

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
box-or-nothing convention. A shared dialog chrome/geometry helper. Per-widget
rendered-buffer assertions that a hosted widget does not clobber the host's chrome — these
replace the originally-planned *static* guard, which is not achievable: `Component::render`
is the modal path where a border is correct, so no static check can distinguish a widget that
paints chrome while hosted from one that paints it as a modal.

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
- No `Component` trait default flip. The `render_in_panel` inset-by-one default was
  considered as a research item and **withdrawn** — §7 records the evidence. The per-widget
  override is the fix, and it costs the same either way.

## 4. Current state (repo evidence at baseline)

### 4.1 Three seams, one adoptor

Counts re-measured at `f8286a284`, after M001 landed:

| Seam | Purpose | Implementors |
|---|---|---|
| `Component::docks_as_viewer()` | opt into the shared docking gesture | 18 — 17 dialogs + `Placeholder` |
| `Component::render_in_panel()` | suppress own chrome when hosted | 15 — 13 dialogs + `Placeholder` |
| `Component::keymap_hint()` | declare own verbs for the status line | 14 — 14 dialogs |

Re-measured after M002b landed, by `grep -rn '    fn render_in_panel' src/` and
`grep -rn '    fn keymap_hint' src/`, counting overrides only and excluding
`component.rs` where the seam is *declared*. The thirteen dialogs adopting
`render_in_panel` are `files`, `info`, `tree`, `plan`, `diff`, `review`,
`task_view`, `research`, `security_review`, `plugin`, `mcp`, `keybind`, `theme`,
`panel_picker`; the fourteen for `keymap_hint` are the same list without
`Placeholder`.

The two rows above this one are now saturated, and that is the point. `docks_as_viewer`
was already complete — 16 boxes answer the shared `Shift+←/→/↑/↓` gesture correctly, and
`dock_target` covers every dockable box, so no docking gesture is a dead key. `render_in_panel`
and `keymap_hint` had no coverage at all, which is what M002a, M002b and
`file-picker-rework` M001 closed. `GENERIC_KEYS` in `panels/hosted.rs` stays as the
fallback for the remaining widgets; it is simply correct for none of these any more.


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

Dockable boxes ignore their `theme` parameter and read a `set_theme()`-populated cache. A
docked box therefore renders stale colours after the user changes theme, while the rest of
the UI updates. Separately, `plan` and `mcp` hardcode `Color::White` for selection foreground
over a themed background, which is unreadable on a light theme, and `plan` carries four
near-identical render implementations with the same literals copied between them.

> **Corrected at M003 planning.** This section originally named five boxes
> (`plugin`, `review`, `security_review`, `diff`, `ui_node`) and stated that
> "`dialogs/mcp.rs` already demonstrates the correct pattern". Re-measured against the
> post-M002b tree, **ten** dockable boxes read a stale cache and `mcp` is the **worst**
> instance — its shared body reads the cache 44 times and the live theme zero times, so the
> claim was the inverse of the truth. `ui_node` has no `render_in_panel` and was never a
> docked box. Nine boxes read the cache exclusively and one (`diff`) is stale through its
> `DiffViewer` sub-widget; four of them mix live and cached reads, which repaints
> incoherently rather than uniformly stale. The colour-literal counts (`plan` 6, `mcp` 2)
> were correct; two further literals in `confirm.rs` and `run_detail.rs` were not listed.
> The derived token these literals need **cannot be ingested from Halloy** —
> `foreground_selected` is declared by the Halloy schema and defined by 0 of the 50 bundled
> themes. See
> `plans/implementation/tui-panel-surface-convergence-corrective/004-theme-sourcing-and-shared-chrome-geometry.md`
> §3.2 and §3.4.

> **Corrected again at M003 closure — "ten" is a count of code, not of defects.**
> The correction above measured `self.theme.` occurrences, which counts *how much code
> reads a cache*, not *whether the cache is stale*. `tests/tui_theme_source_of_truth.rs`
> settles it by rendering each box with theme A and then theme B and inspecting the cells.
> Measured that way the stale set is **four**: `review`, `plugin`, `security_review`, and
> `diff` via `DiffViewer`.
>
> - `mcp` (44 reads) and `research` (46) are **not stale** — each reassigns
>   `self.theme = Arc::clone(theme)` inside its own render body, so the cache is overwritten
>   on the frame that reads it. They led the source count and are the two boxes that are
>   actually fine; `mcp` was named here as "the worst instance" for the second time.
> - `keybind` (21), `tree` (11), `plan` (17) and `theme` (5) were recorded as mixing live and
>   cached reads. Their cached reads are all in the **dead legacy** paths — `impl Widget for
>   &XDialog`, plus `plan`'s uncalled third `render`. Their live bodies already read the
>   seam's theme, so they never repainted incoherently because there was nothing cached in
>   the path that runs.
> - The `review` "second sub-widget with the same problem" does not exist; `ReviewDialog`'s
>   diff view colours from the passed theme.
>
> W2 converted `mcp` and `research` anyway — a cache a render method overwrites is a hazard,
> and the milestone's acceptance criterion covers every render path — but they are recorded
> here as **not defects**, because a roadmap that lists them as the worst cases points the
> next milestone at the wrong code.

### 4.10 No shared chrome geometry

There is no dialog chrome helper. `centered_rect` is defined twice, in `App` and in
`dialogs/run_detail.rs`. Fifty-six `Layout::…split(…)` sites across 31 files hand-type
their constraint sets.

> **Corrected at M003 planning.** The two `centered_rect` definitions are semantically
> identical but **not** "byte-identical" as originally recorded: the parameter is named
> `area` in one and `r` in the other, and `run_detail` qualifies `Direction` as
> `ratatui::layout::Direction`. The layout-site figure also drifted from "63 across 24
> files" to 56 `Layout::` sites across 31 files. The *absence* of a chrome helper is
> unchanged and still true.

### 4.11 Why none of this was caught

`tests/tui_panel_docking.rs` asserts the shared docking gesture for seven boxes — `Cost`,
`Usage`, `Context`, `Theme`, `Mcp`, `Keybind`, `Files` — and its own comment records that the
remainder "are produced by `open_info_dialog` / dedicated helpers rather than by
`open_dialog`, so a test cannot mount them without reaching into `dialog_state`." Nothing
asserts over `SidePanelId::ALL`, so a panel can be listed, unconstructible, and still pass.

## 5. Relationship to `file-picker-rework` M001 (read first)

`file-picker-rework` M001 created `keymap_hint()` and `render_in_panel()`, wired them into
`HostPanel::composer_hints`, and adopted both in `FilesDialog`. Its roadmap already diagnoses
§4.2 for one widget. It is **committed** as `b4f4b916f` and its closure record exists, so the
seams are final and the shared-file collision risk is gone.

Two consequences bind this addendum:

1. **No seam may be re-created.** `keymap_hint`, `render_in_panel`, and the `GENERIC_KEYS`
   fallback already exist. M002a/M002b generalize what M001 proved on one widget, and
   `Placeholder` extended it in M001 of this addendum.
2. **`FilesDialog` is the worked template.** Its `render_in_panel` is an inset plus the
   widget's own row-rendering, and its doc comment records why the in-box footer was retired
   with it. M002a/M002b follow that shape; they do not invent another one.

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

And one guard: an exhaustive sweep over `SidePanelId::ALL` asserting that every listed panel
constructs a box or is bespoke (delivered by M001), plus a per-widget rendered-buffer
assertion that the host's border, title and `●` survive a hosted render (delivered by
M002a/M002b). A **static** guard for the second was considered and rejected — see §2.

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

**Class:** capability (with polish). **Status: approved into planning, split into M002a and
M002b** — see §7.1 for the split rationale.

Generalizes what `file-picker-rework` M001 proved on `/files` to every dockable box:
`render_in_panel` overrides for the widgets that paint their own chrome, `Placeholder`
included; `keymap_hint` for every box that owns distinct verbs, retiring in-panel footers and
border-title keymaps in favour of the shared status line.

**The `render_in_panel` default flip is dropped, not deferred.** It was to be a research item.
It is withdrawn on the evidence: the host passes `frame.area()` — the full column, border
cells included — to `render_in_panel`, so an inset-by-one default would not stop a widget
painting its own `Block`, it would only move that block one character inward. The flip
therefore does not reduce the override count at all; it only helps widgets that already draw
no chrome. Against that, `Component::render` is the modal path where a border is *correct*
— the same fact that made M001 reject a static chrome guard — so the flip's blast radius
covers every box in the tree for a marginal gain. The per-widget override is the fix.

### M003 — Theme sourcing and shared chrome geometry

**Class:** polish. **Status: approved into planning; plan written** at
`plans/implementation/tui-panel-surface-convergence-corrective/004-theme-sourcing-and-shared-chrome-geometry.md`.
The theme-cache and colour-literal counts below **have now been re-derived** (see §4.9's
correction note); they are no longer the audited-baseline figures.

Retire the `set_theme()` colour caches so a docked box follows the live theme — **ten**
dockable boxes, not five — and replace the `Color::White` selection literals with theme
tokens **derived** from colours every Halloy theme already carries, so no Halloy key is
required; de-duplicate `centered_rect`.

Two scope changes were made at planning time and are recorded deliberately:

- **The four duplicated `plan` render paths no longer exist as a live problem.** M002b
  already collapsed the two live ones into a single shared body routine. What remains is
  two *dead* entry points, which join the legacy `Widget` cleanup below rather than
  expanding M003.
- **The shared chrome/geometry helper is deferred out of M003.** It has no seam, no failing
  test to define it, and would run directly against the `Chrome::{Owned,Hosted}` contract
  M002b just established. M003 takes only the mechanical half (`centered_rect`); the
  helper should be its own milestone, written against a settled shape.
- **The legacy `impl Widget for &XDialog` impls are also deferred.** There are **ten**
  (not the five M002b's closure recorded) and **none** has a production caller — but
  `connect` and `share` have test-only callers whose purpose is comparing the Widget path
  against the Component path, so they must be rewritten before those two can go. A clean
  deletion is eight impls; the other two need a replacement signal first. Recommended as a
  standalone cleanup milestone.

Deferred until M002b, so the colour reads are written against the post-adoption shape rather
than being re-broken by the next chrome change. M002b is closed, so this is satisfied.

### 7.1 Why M002 is split in two

One `InfoDialog` override retires 13 clobber sites, because `InfoDialog` backs `Cost`,
`Usage`, `Stats`, `Context`, `Logs`, `DoctorReport`, `ShellShow`, `TaskList`, `GoalShow`,
`Collaborators`, `Team`, `Control` and `MemoryResults`. The remaining widgets are
single-`DialogType` boxes, several of which carry multiple in-panel footers (`diff` has
three render paths with three footers, `research` three, `review` two) and therefore need
per-widget reasoning about which viewport state the inset changes.

That is two differently-sized, differently-risky units. M002a is one widget with a large
fan-out and a single scroll-clamp hazard; M002b is ~12 widgets with scattered viewport state
and up to three footer sites each. Keeping them together would put a small, reviewable,
single-file change in the same pass as the bulk of the migration.

| | M002a | M002b |
|---|---|---|
| Scope | `InfoDialog` chrome + keymap | remaining dockable boxes |
| Clobber sites retired | 13 | ~12 widgets |
| Plan | `plans/implementation/tui-panel-surface-convergence-corrective/002-info-dialog-chrome-and-keymap-adoption.md` | `plans/implementation/tui-panel-surface-convergence-corrective/003-dockable-box-chrome-and-keymap-adoption.md` |
| Depends on | M001, `file-picker-rework` M001 | M001, `file-picker-rework` M001, M002a |

M002b following M002a is what lets M002b treat `InfoDialog`'s inset as the established
pattern rather than a second opinion.

## 8. Blockers

No blockers outstanding. The `file-picker-rework` M001 sequencing blocker is **resolved**:
that milestone is committed as `b4f4b916f`, its closure record exists, and the seams it
created are final and consumed. The M002 default-flip blocker is **withdrawn** — see §7.

No ADR is required. No other dependency is outstanding.

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
- An exhaustive `SidePanelId::ALL` sweep, added by M001, is the regression gate for
  M002a/M002b and M003.
- A negative test per milestone: an added chrome painter in a hosted widget must be caught.
  M001 established that a **static** `Block` guard cannot work here — `Component::render` is
  the modal path, where a border is correct, so a static check cannot tell the two apart and
  would false-positive on every dialog. M002a/M002b therefore detect the defect
  **behaviourally**: a rendered-buffer assertion per widget that the host's border, title and
  `●` survive inside a hosted column. A widget that paints its own `Block` fails it, and a
  widget that draws nothing cannot.
- Every widget whose render derives a scroll viewport or selection window from `area` must
  be proven to derive it from the inset rect instead. This is the one silent-failure class in
  this milestone: an inset that leaves the viewport unadjusted makes the tail of the content
  unreachable with no error anywhere.

## 11. Risks and decision points

- **The inset/viewport hazard, not the chrome.** Painting over the host's title is visible and
  self-announcing; a scroll clamp left two rows short is not. Every widget M002a/M002b adopts
  needs its viewport proven against the inset rect, and the acceptance signal is "the last
  line is reachable", not "the border survived".
- **Keymap honesty cuts both ways.** `GENERIC_KEYS` advertises `i act`, which several
  adopting boxes do not implement. Correcting that is the point — but declaring a box's real
  verbs means asserting what `handle_key` dispatches. A hint that names a verb its own
  `handle_key` does not arm is the same defect in a new place.
- **Removing `SidePanelId::Workspace` is a user-visible surface change.** Resolved in M001 by
  removal; recorded here so the decision is not silently revisited. See the M001 closure
  record §2 for the option set and the fail-soft verification of the persisted binding.
- **A guard that is too strict blocks legitimate modal chrome.** Any static check must
  distinguish "hosted" from "modal" rendering, or it will forbid the modal border every box
  legitimately draws.
- **Adoption semantics.** Making a failed adoption visible risks surfacing gestures that were
  always silently dropping a box. That is the point, but the message must name the panel,
  not expose an internal enum.
- **M001's sequencing risk is realised, not live.** M001 and `file-picker-rework` M001 both
  ran dirty in one worktree; the collision was avoided by a separate worktree and by
  sequencing after the closure record. M002a/M002b have no equivalent hazard — both seams are
  committed and the plans name their boundary files.

## 12. Completion definition

This addendum closes when selecting any panel in `/panels` produces exactly one of three
honest states with no composer detour and no `Shift+←` instruction; every panel in the list
constructs a box or is bespoke, proven by an exhaustive sweep; no hosted widget paints over
the host's title or focus marker; every dockable box's verbs appear once, on the shared
status line; docked boxes follow the live theme; `verify.sh quick` and `verify.sh full` are
green; and `docs/tui.md` and `architecture/tui.md` match shipped behavior.

M003 is never implicitly approved, and neither M002a nor M002b was implicitly approved — all
three entered planning explicitly, M003 by an explicit instruction to write its plan after
its counts were re-derived. A closure that unblocks nothing downstream is a valid outcome
and MUST be recorded as such in the closure record.

## 13. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/tui-panel-surface-convergence-corrective/001-panel-manager-outcome-contract-and-registry-reachability.md` | `plans/closure/tui-panel-surface-convergence-corrective/001-status.md` | none |
| M002a | **closed** | `plans/implementation/tui-panel-surface-convergence-corrective/002-info-dialog-chrome-and-keymap-adoption.md` | `plans/closure/tui-panel-surface-convergence-corrective/002-status.md` | none |
| M002b | **closed** | `plans/implementation/tui-panel-surface-convergence-corrective/003-dockable-box-chrome-and-keymap-adoption.md` | `plans/closure/tui-panel-surface-convergence-corrective/003-status.md` | none |
| M003 | **approved into planning** (ready for handoff) | `plans/implementation/tui-panel-surface-convergence-corrective/004-theme-sourcing-and-shared-chrome-geometry.md` | — | none — counts re-derived at planning; the shared chrome/geometry helper and the legacy `Widget` cleanup are deliberately deferred out |

M002a closed with one finding it did not fix and did not absorb: **`Esc` never
reaches a hosted widget** (`SidePanelHost::handle_key` claims `Esc`/`q` as the
universal exit and blurs before the panel is consulted) **and `Enter` closes
nothing** (docking pops the box off the modal stack, so `TuiMsg::CloseDialog`
runs against an empty one). A docked `InfoDialog` therefore has no in-panel
dismiss key; `Shift+←` returns it to a modal where `Esc` works. This is a
host-level design question, not a widget fix — see the closure record §5.1.

M002b inherited it for all twelve boxes and, rather than re-deriving it,
**found two more keys the host answers first**: the four arrows and bare
**and** shifted uppercase `H`/`J`/`K`/`L`, all of which resolve to a panel
`InputAction` that `SidePanelHost::handle_key` routes to `MoveDirection`
before the widget is consulted. Every `↑/↓ navigate` in those twelve
footers, plus `/tasks`'s advertised `J/K reorder` and
`/security-review`'s `H copy hunk`, were therefore dead the moment the box
docked. All twelve boxes now name only keys a column can deliver, and the
host-level exit gap itself is recorded per widget rather than fixed — see
the M002b closure record §5.

Three pre-existing scroll defects were found and deliberately **not** fixed,
because they are behaviour changes and this milestone moves chrome:
`review`'s diff view renders from line 0 and ignores its own `diff_scroll`
(so its scroll is inert), `review`'s `k` and `research`'s `k`/`j` move the
wrong way relative to the arrows, and `tree`/`theme` carry a
`visible_height` that no production code sets. The `research` report
view's *unclamped* `usize::MAX` offset **was** fixed, because the
milestone's own one-body-routine rule requires the clamp to live in the
shared routine — see the M002b closure record §6.

M003's planning corrected three of this addendum's own recorded facts, and one of them
was an outright false claim rather than a drifted number. **§4.9's "five boxes" is ten,
and `mcp` — named here as the exemplar of the correct pattern — is the worst instance of
the defect.** §4.10's "byte-identical" `centered_rect` pair is semantically but not
textually identical, and its layout-site count drifted. Correcting all three required
re-deriving the milestone's headline number rather than inheriting it, which is what §4.9's
original "must be re-derived before this milestone is planned" note anticipated. The
M002b closure record's §6 legacy-path claim was corrected in the same pass: **ten** legacy
`impl Widget for &XDialog` impls exist, not five, and `connect` and `share` do have
callers — test-only ones, in tests that exist to compare the two render paths.

Two things M003 explicitly does **not** do, having been considered and declined at
planning: it does not build the shared chrome/geometry helper (no seam, no failing test to
define it, and it would run against the `Chrome::{Owned,Hosted}` contract M002b just
established), and it does not delete the legacy `Widget` impls (the eight with no callers
are trivial; `connect` and `share` need their comparison tests rewritten first). Both are
recorded as recommended standalone milestones rather than silently dropped.