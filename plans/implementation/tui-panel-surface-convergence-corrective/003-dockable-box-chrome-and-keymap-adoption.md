# TUI Panel Surface Convergence Corrective Milestone M002b — Dockable box chrome and keymap adoption

Status: implemented — **closed**; closure record
`plans/closure/tui-panel-surface-convergence-corrective/003-status.md`

Its only stated blocker was M002a closing. Four things it did not know when it
was written, all of which are recorded rather than re-derived here:

- **The host answers four more keys than M002a found.** Besides `Esc`/`q` and the
  dead `Enter`, `SidePanelHost::handle_key` resolves a panel `InputAction` and
  routes it to `MoveDirection` *before* the widget is consulted, and
  `insert_panel_direction_bindings` binds the four arrows plus bare **and**
  shifted uppercase `H`/`J`/`K`/`L`. Every `↑/↓ navigate` in §3.1's footers was
  dead, and so were `/tasks`' `J/K reorder` and `/security-review`'s `H copy
  hunk`. `Home`/`End`/`PageUp`/`PageDown` do arrive.
- **§7's work packages cover eleven of §3.1's twelve widgets.** `mcp` — two
  bordered panes, two copies of the same false footer — is in the §3.1 table and
  in no package. It was implemented anyway; "missing one path is the most likely
  way this ships half-done" applies to a missing *widget* at least as much.
- **§14 fires for `panel_picker`.** It declares `docks_as_viewer() == true` but
  has no `dock_target` arm, so it never docks — which makes its footer its only
  affordance, and retiring it into an unreachable `keymap_hint` would leave the
  modal with no keymap at all. The footer stays; the two seams were added anyway.
- **The §3.3 hazard was real, in `research`.** Its report view stored a row
  offset no render pass clamped, so `Home`'s `usize::MAX` sentinel blanked the
  report unreachably. Fixed in the shared body routine, in both paths — the one
  deliberate modal-output change in this milestone.

Two inherited rules were kept as written and both held: **no box may advertise
`Esc` or `Enter` as a close verb**, and **a caller-supplied footer is not
replayed into the hint**. `InfoDialog`'s one-body-routine extraction was used as
the worked template for all twelve.

Repository baseline: `f8286a2849660d1e386a61ceaf663906451961d0`
(`docs(plans): record the tui-panel M001 commit SHA`), **plus M002a landed**.

Source roadmap:

- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` §7 M002, §7.1 split
  rationale, §4.2 chrome clobber, §4.5 literal branches, §4.8 keymap fallback

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs: none required. No identity, ownership, protocol, storage layout, or
authorization change. `SidePanelId` variants, `config_key` spellings and persisted bindings
are untouched.

Depends on: `plans/implementation/tui-panel-surface-convergence-corrective/002-info-dialog-chrome-and-keymap-adoption.md`
(M002a) must be closed. M002b treats `InfoDialog`'s one-body-routine extraction as the
established pattern and does not re-derive it.

Primary class: capability (with polish)

## 1. Objective

Extend what M002a proves on `InfoDialog` to the remaining dockable boxes: each one stops
painting its own chrome into the host's rect, and each one advertises the verbs its own
`handle_key` dispatches instead of the shared `"j/k scroll · i act"` fallback.

## 2. Why this milestone is ready

- `tui-panel-surface-convergence-corrective` M001 is `closed` and committed as `18bcfa3d9`.
- `file-picker-rework` M001 is committed as `b4f4b916f` and supplies the `keymap_hint` /
  `render_in_panel` seams plus the `FilesDialog` template.
- **M002a is closed**, which is what makes this milestone tractable: by then two widgets
  (`FilesDialog`, `InfoDialog`) have shipped the pattern, and the per-widget hazards — the
  inset, the zero-size guard, the viewport coupling, the footer-to-hint relocation — are
  known rather than discovered here.

Seam signatures were re-read at `f8286a284` and are unchanged. No interface blocker remains.

## 3. Current implementation evidence

Read at `f8286a284`. Re-verify before editing; symbols may have moved.

### 3.1 The set to adopt

Every widget below declares `docks_as_viewer() == true` **unconditionally** (verified), has
a `DialogType` that `App::dock_target` maps to a **non-bespoke** panel, and paints its own
`Block`. Together they are the remaining clobber sites:

| Widget | `DialogType` | `SidePanelId` | Footer / keymap sites |
|---|---|---|---|
| `dialogs/tree.rs` | `Tree` | `Tree` | 4 `Block` refs |
| `dialogs/plan.rs` | `Plan` | `Plan` | 4 render paths; keymap in **border title** |
| `dialogs/diff.rs` | `Diff` | `Diff` | **3** render paths, 3 footers |
| `dialogs/review.rs` | `Review` | `Review` | 2 footers |
| `dialogs/task_view.rs` | `TaskView` | `TaskView` | 2 `Block` refs |
| `dialogs/research.rs` | `ResearchBrowser` | `Research` | 3 footers |
| `dialogs/security_review.rs` | `SecurityReview` | `Security` | 1 footer |
| `dialogs/plugin.rs` | `Plugin` | `Plugins` | 1 `footer_block` |
| `dialogs/mcp.rs` | `Mcp` | `Mcp` | keymap in border title |
| `dialogs/keybind.rs` | `Keybind` | `Keybinds` | 4 `Block` refs |
| `dialogs/theme.rs` | `Theme` | `Themes` | 4 `Block` refs |
| `dialogs/panel_picker.rs` | `PanelPicker` | **none** | 3-row footer — see §3.5 |

`files.rs` is excluded: it adopted both seams in `file-picker-rework` M001. `chat.rs`,
`help.rs` and `agent.rs` are excluded: their `DialogType`s map to **bespoke** panels
(`Chat`, `Help`, `Agents`), which own their own `SidePanel` impls and never render through
`render_hosted`.

`InfoDialog` is excluded: M002a owns it.

### 3.2 The uniform clobber and the uniform fix

`render_hosted` (`src/tui/panels/mod.rs`) paints the panel border, its title and the `●` focus
marker into the rect, then calls `widget.render_in_panel(frame, frame.area(), theme)`, which
defaults to `render`. `area` is the **full** column rect, border cells included. Any `Block`
the widget paints into that rect overwrites all three.

The fix is identical in every case, following `FilesDialog` and M002a's `InfoDialog`:

```rust
fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
    let inner = area.inner(Margin::new(1, 1));
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    /* same body routine as render(), chrome-free */
}
```

### 3.3 The hazard that repeats per widget

M002a found that `InfoDialog`'s scroll clamp is owned by the render pass — its `Down`/`j` arm
is deliberately unbounded and `scroll_to_end()` sets a `usize::MAX` sentinel, both relying on
the render pass to clamp against the true wrapped-row maximum. **Any widget here with the
same shape breaks the same way** if `render_in_panel` computes its viewport from `area` rather
than from `inner`.

This is not hypothetical for the boxes in this set. `review` and `research` both dispatch on
`self.mode`, `diff` has three render paths, and any of them may derive a selection window or
visible-row count from the rect it was handed.

The mitigation is the same structural rule M002a adopted, and it is **not** optional:

> Each widget's body — wrapping, visible-row computation, selection windowing, scroll
> clamping — moves into **one** private routine parameterised by whether to draw chrome.
> `render` and `render_in_panel` both call it.

Two render paths with two viewport computations is exactly how a docked box ends up with its
last two lines unreachable and no error anywhere. Prefer extracting a shared routine over
adding a second implementation with a corrected constant.

### 3.4 Two widgets hide their verbs in the border title

`plan.rs` titles one of its blocks `" Options (↑↓ select  |  Enter confirm) "` — in four
places, once per render path. That is a keymap painted into chrome, so it is destroyed by
exactly the overwrite this milestone fixes, and it would be invisible to a
`keymap_hint`-only migration. `mcp.rs` titles blocks `" Servers "` and `" Details "`, which
are identities rather than verbs, but `mcp` is one of the two boxes the roadmap flags for
hardcoded `Color::White` selection foreground (M003's scope — see §5).

Every keymap that currently lives in a border title or an in-panel footer must end up in
`keymap_hint`. A verb advertised only inside chrome the host overwrites is a verb the user
never sees.

### 3.5 `PanelPicker` declares itself dockable but has no `dock_target`

`panel_picker.rs` returns `docks_as_viewer() == true`, and `DialogType::PanelPicker` has **no
arm in `App::dock_target`** — it falls to `_ => None`.

Consequence: the host resolves `Shift+←` as the docking gesture and swallows it, then
`dock_target()` returns `None` and the gesture refuses. The key never reaches
`PanelPicker::handle_key`. For the `/panels` picker itself this is probably harmless —
`Shift+←` has no meaning there — but it is an inconsistency between the two halves of the
docking contract, and W1 must record what it actually does rather than assume.

**Do not add a `dock_target` arm for `PanelPicker`.** The picker is the `/panels` modal; there
is no panel a picker should be adopted into. If W1 confirms the swallowed key causes a visible
problem, that is a stop condition (§14), not a licence to invent a mapping.

### 3.6 Existing coverage

`tests/tui_panel_docking.rs` asserts the shared docking gesture for seven boxes, of which
`Mcp`, `Keybind`, `Theme`, `Tree` and `Files` are in this milestone's set. That suite proves
the *gesture* works; it does not assert host-chrome survival, so it will not catch a
regression here. `tests/tui_panel_manager_reachability.rs` (M001) sweeps `SidePanelId::ALL`
for manager reachability and is the regression gate, not a chrome guard.

## 4. Invariants that must not regress

- **Modal rendering is byte-identical for every widget touched.** This is the primary
  regression net and it is asserted per widget, not assumed. The extraction must be
  behaviour-preserving in the modal path.
- **The shared docking gesture keeps working for all seven boxes** already asserted in
  `tests/tui_panel_docking.rs`.
- **Scroll reachability is unchanged in both paths.** The viewport must be derived from the
  rect actually being filled.
- **No widget's content, filtering, selection semantics, or mode switching changes.** Only
  chrome moves.
- **Every advertised verb is dispatched, and every dispatched verb is advertised** — per
  widget, and per mode where the keymap is mode-dependent (`review`, `research` dispatch on
  `self.mode`).
- **`GENERIC_KEYS` remains the fallback** for any widget that does not adopt
  `keymap_hint`. It is not removed here; it is simply correct for fewer widgets.
- M002a's `InfoDialog` behaviour is untouched.
- No new command, key, gesture, panel, data read, or daemon call.
- `SidePanelId` variants, `config_key` spellings, persisted bindings, placement and sizing are
  unchanged.

## 5. Scope

### In scope

- The twelve widget files in §3.1 — `render_in_panel` override, shared body routine,
  `keymap_hint` override, in-panel footer and border-title keymap retirement.
- `tests/` — per-widget rendered-buffer chrome assertions, per-widget scroll-reachability
  assertions in both paths, per-widget `keymap_hint` correctness assertions.
- `docs/tui.md`, `architecture/tui.md`, `.opencode/skills/tui/SKILL.md`.

### Explicitly out of scope

- **M003's work, even where it sits in the lines being edited.** `plan` and `mcp` carry
  hardcoded `Color::White` selection foregrounds; those literals stay exactly as they are.
  `plan`'s four duplicated render paths are **not** collapsed here — the §6 extraction is
  mandated precisely so M003's de-duplication is a mechanical follow-on instead of a
  re-implementation. Do not "while we're here" any of M003.
- **The five `set_theme()` colour caches** in `plugin`, `review`, `security_review`, `diff`
  and `ui_node`. Note that four of the five are files this milestone edits: extracting a body
  routine must carry the `theme` parameter through unchanged and must not introduce or remove
  a `set_theme` read. If a cache genuinely blocks the extraction, that is a stop condition.
- `InfoDialog` (M002a). Bespoke panels (`Session`, `Agents`, `Chat`, `Terminal`, `Help`).
- Any `Component` trait default, including the inset-by-one default flip **withdrawn** at the
  roadmap §7 — it does not reduce the override count, because the host passes `frame.area()`
  and an inset merely moves a widget's own `Block` one character inward.
- Any static "hosted widgets do not paint a `Block`" guard: unachievable, since
  `Component::render` is the modal path where a border is correct. The guards here are
  behavioural.
- `centered_rect` de-duplication and any shared chrome/geometry helper (M003).
- `PanelPicker` gaining a `dock_target` arm (§3.5).
- File-picker behaviour. New panels. Protocol, storage, identity, authorization.

## 6. Required production changes

### Core/domain

None. No domain type, no `SidePanelId` variant, no `DialogType` variant. No change to
`App::dock_target`.

### Storage and migrations

None. `STORAGE_LAYOUT_VERSION` unchanged.

### Protocol and DTOs

None.

### Runtime and concurrency

None. No new task, request, lease, or generation fence. M001's `PanelFill` outcome and
`SidePanelId::discards_a_docked_box()` are untouched.

### Frontend or operator surface

**Per widget, the same three moves:**

1. **Extract one body routine.** Wrapping, visible-row computation, selection windowing and
   scroll clamping move into a single private routine taking the rect to fill plus a
   chrome flag. `render` and `render_in_panel` both call it. This is the structural
   guarantee that the two paths cannot diverge (§3.3), and it is required for every widget
   with scroll or selection state.

2. **Add `render_in_panel`.** Inset by `Margin::new(1, 1)`, guard the zero-size inner rect, call
   the body routine chrome-free. No `Borders::ALL`, no title block, no footer block in the
   hosted path.

3. **Add `keymap_hint`, and retire the duplicate advertisement.** Move the in-panel footer and
   any border-title keymap into the hint. The hint must be derived from `handle_key` — for
   mode-dependent widgets (`review`, `research`) it must vary by `self.mode`, because a hint
   naming a verb the current mode does not arm is the same defect in a new place.

**`plan` and `diff` carry multiple render paths.** `plan` has four and `diff` has three, each
with its own footer and (for `plan`) its own border-title keymap. Apply the extraction across
all of them. Because `plan`'s four paths differ only in chrome and content arrangement, the
body routine should be the *one* shared routine they already almost are — that is what makes
M003's later de-duplication mechanical. Do not collapse the outer paths here; that is M003.

**`plan`'s border title `" Options (↑↓ select  |  Enter confirm) "`** becomes `keymap_hint`
content. It currently exists in four copies; after this milestone it exists once, in the hint,
and the border title says what the block is.

### Security and authorization

None. No box gains a read, a fetch, or a privilege.

### Documentation and static guards

No static guard. Per-widget behavioural tests, §10.

## 7. Ordered work packages

### W1 — Baseline-failing tests, per widget (the acceptance signal)

Land first, failing, one test family per widget in §3.1:

1. Rendered-buffer: host border, title and `●` survive a hosted render of that widget.
2. Scroll/selection: the last row and the full selection range are reachable when hosted,
   at the same end position as modally.
3. `keymap_hint`: no advertised verb is absent from `handle_key`, and no dispatched verb is
   unadvertised — parameterised over modes for `review` and `research`.
4. A recorded answer to §3.5: what actually happens to `Shift+←` on `PanelPicker`.

Where a widget cannot be mounted in a test without reaching into private `dialog_state`,
**stop and report** (§14) rather than adding a test-only door.

Do not modify production code in W1.

### W2 — Simple viewer widgets

`tree`, `keybind`, `theme`, `task_view`. Lowest-risk group: no in-panel footer, minimal state.
Apply all three moves per widget. Re-run W1 for these four.

### W3 — Widgets with footers

`review`, `security_review`, `plugin`, `research`. Two of these (`review`, `research`) are
mode-dependent, so `keymap_hint` varies by mode. Retire the footer into the hint. Re-run W1.

### W4 — Multi-path widgets

`diff` (3 paths, 3 footers), then `plan` (4 paths, border-title keymap, 4 footer blocks).
These carry the most duplicated surface and the highest chance of one path being missed.
Re-run W1 for both, and confirm **all** paths are chrome-free by inspection, not by spot
check.

### W5 — `panel_picker`

Record §3.5's answer, retire its 3-row in-panel footer into `keymap_hint`. **No
`dock_target` arm is added** (§3.5).

### W6 — Docs and guards

`docs/tui.md`, `architecture/tui.md` (including the seam adoptor counts, which move from 2/1
to 14/13 after this milestone), `.opencode/skills/tui/SKILL.md`.

## 8. Failure, cancellation, restart, and contention semantics

- A widget whose hosted render produces a zero-sized inner rect returns without painting. The
  host's chrome is already painted, so the panel shows an empty column rather than a panic.
- Scroll and selection state stay in the same units across both paths. Changing a unit while
  extracting a routine is a defect, not a refactor.
- Any sentinel-style scroll value (`usize::MAX` or equivalent) must still be clamped by the
  render pass in the hosted path. If a hosted path fails to clamp, the widget renders an empty
  or truncated body with no error. W1 test 2 is the guard.
- Mode switching (`review`, `research`) must keep the hint in sync with the mode. A hint that
  lags a mode change is the defect class this milestone removes; do not cache it across modes.
- No partial-failure, duplicate-delivery or contention behaviour changes.

## 9. Compatibility and migration

No persisted format change. `STORAGE_LAYOUT_VERSION` unchanged. No config key added or
removed. No protocol negotiation.

Observable changes: a hosted box stops painting its own border, and its status line states its
real verbs instead of `j/k scroll · i act`. Modal rendering is byte-identical.

## 10. Required tests

### Focused unit tests

- Per widget: `render_in_panel` emits no `Borders::ALL` cell inside the hosted column.
- Per widget: `keymap_hint` is `Some(..)` with content drawn from `handle_key`.
- Per mode for `review` and `research`: the hint names only that mode's dispatched verbs.
- `panel_picker`: `Shift+←` behaviour recorded per §3.5.

### Integration tests

- Per widget: rendered-buffer assertion that host border, title and `●` survive.
- Per widget: the final row is reachable hosted, at the same end position as modal.
- Per widget: modal rendering unchanged — the pre-existing modal render test still passes
  unmodified. This is the regression net for the extraction.
- `tests/tui_panel_docking.rs` passes in full, all seven boxes.
- `tests/tui_panel_manager_reachability.rs` passes unmodified (M001's gate).
- `diff` and `plan`: **every** render path asserted chrome-free, not one representative path.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- Per widget and mode: every verb in `keymap_hint` is dispatched by `handle_key`; every
  dispatched verb appears in the hint. The first direction is the one that matters —
  advertising a dead verb is the defect being removed.
- No widget's `keymap_hint` names the withdrawn `i act` unless `handle_key` really arms `i`.

### Migration and compatibility tests

None.

## 11. Required verification commands

```bash
cargo test -p codegg tui::panels            # focused
cargo test --test tui_panel_docking         # existing 7-box gesture sweep
cargo test --test tui_panel_manager_reachability
cargo test --test tui_side_panels
cargo fmt
scripts/verify.sh quick                      # the gate
scripts/verify.sh full                       # before closure
```

Never `--all-features`. CI runs a **strict subset** of `quick`, so a green CI run does not
imply a green local `quick`; report local and CI evidence separately and never present an
unrun command as passing.

**Known environment condition:** `verify.sh full` exits 100 in this environment on
pre-existing failures unrelated to this work (M001's closure record §4.1: 11 failures
reproducing identically with `src/` stashed to `b77c8156`, plus one load-sensitive timeout
that passes in isolation). Re-derive that baseline before attributing any failure here.

## 12. Documentation updates

- `docs/tui.md` — panel gesture table and panel list, agreeing with the new status line.
- `architecture/tui.md` — panel host module map; the `render_in_panel` and `keymap_hint`
  adoptor counts in the verified-counts table (both move after this milestone — verify the
  new numbers against source rather than copying them forward); per-widget descriptions.
- `.opencode/skills/tui/SKILL.md` — panel authoring guidance. `InfoDialog` is the template
  once M002a closes; record it as such with its real adoptor count.
- `plans/registry.md` in the same commit as any status change.

## 13. Acceptance criteria

1. For every widget in §3.1, a hosted render leaves the host's border, title and `●` intact
   in the rendered buffer.
2. No hosted widget in §3.1 emits a `Block` of its own.
3. For every widget in §3.1, the last row and full selection range are reachable hosted, at
   the same end position as modally.
4. For every widget in §3.1, the status line names exactly the verbs `handle_key` dispatches,
   per mode where mode-dependent.
5. No keymap survives only inside a border title or an in-panel footer.
6. Modal rendering is unchanged for every widget touched.
7. `diff` and `plan` are chrome-free in **every** render path.
8. `panel_picker` gains no `dock_target` arm, and its `Shift+←` behaviour is recorded.
9. `tests/tui_panel_docking.rs` and `tests/tui_panel_manager_reachability.rs` pass unmodified.
10. No M003 item landed: the `Color::White` literals in `plan` and `mcp` and the five
    `set_theme()` caches are unchanged.
11. No new command, key, gesture, panel, data read, or daemon call exists.

## 14. Stop conditions

Stop and report rather than improvise if:

- **A widget's body cannot be extracted without duplicating its viewport computation.** That
  is the defect M002b exists to prevent; report the widget instead of shipping a second
  implementation.
- **A widget cannot be mounted in a test without reaching into private `dialog_state`.**
  Report it. Do not add a test-only public door. (`InfoDialog` was found mountable in M002a;
  a widget that is not is a design question, not an obstacle to route around.)
- **A `set_theme()` colour cache blocks the extraction** in `plugin`, `review`,
  `security_review` or `diff`. That cache is M003's; report the coupling rather than retiring
  the cache here.
- **A hosted box's real verb set differs from its modal verb set** because the host
  intercepts a key. Then `keymap_hint` cannot state one honest set, and resolving it is a
  host-level design question.
- **Retiring a footer would remove the only visible status affordance** in a widget with no
  other progress or scroll indicator — as M002a's scroll indicator was. Report it.
- **`PanelPicker`'s swallowed `Shift+←` causes a visible problem.** Do not add a
  `dock_target` arm; report it.
- **Any change would touch bespoke panel behaviour, a `DialogType`, or `App::dock_target`.**
- **Scope would expand into M003** (colours, theme caches, geometry helpers).

## 15. Closure evidence required

- W1's per-widget tests green, including scroll reachability for every widget.
- Proof each test family failed before its work package.
- `scripts/verify.sh quick` green locally; `cargo fmt --check` clean; clippy `-D warnings`
  clean.
- `scripts/verify.sh full` run locally, with failures attributed against the M001-recorded
  pre-existing baseline rather than left unexplained.
- An explicit statement of what ran locally versus what only CI ran.
- The §3.5 `PanelPicker` answer recorded, whatever it turned out to be.
- `plans/closure/tui-panel-surface-convergence-corrective/003-status.md` written and accepted;
  only then is M002b `closed`.
- Registry audit: whether M003 became newly eligible, and confirmation that no M003 item
  landed here.

## 16. Handoff notes

- The seam is `render_in_panel(&mut Frame, Rect, &Arc<Theme>)`. `area` is the host's **full**
  column rect including border cells. The inset is mandatory, not cosmetic.
- Two worked templates exist by the time this starts: `FilesDialog::render_in_panel` and, after
  M002a, `InfoDialog`. Between them they carry the inset, the zero-size guard, the shared
  body-routine shape, and the doc-comment convention that records *why* a footer was retired.
  Follow them; do not invent a third idiom.
- **The viewport coupling is the recurring hazard.** If a widget's scroll clamp or selection
  window is owned by its render pass, the hosted path must compute it from the rect it is
  actually filling. Two implementations of the clamp is the bug this milestone must not
  create.
- `review` and `research` dispatch on `self.mode`; a `keymap_hint` that ignores the mode will
  advertise a verb the current mode does not arm. Do not cache the hint across modes.
- Four of the five M003 theme-cache files (`plugin`, `review`, `security_review`, `diff`) are
  edited here. Thread `theme` through the extraction unchanged; do not retire a cache.
- `plan` has four render paths and `diff` has three. Missing one is the most likely way this
  milestone ships half-done — assert every path.
- M002a must be closed before starting. M002b's premise is that `InfoDialog` has already
  established the pattern and resolved the two questions (viewport coupling, footer-to-hint
  relocation) that would otherwise be answered twelve times.