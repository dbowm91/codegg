# TUI Panel Surface Convergence Corrective Milestone M002a — `InfoDialog` chrome and keymap adoption

Status: implemented and closed — see
`plans/closure/tui-panel-surface-convergence-corrective/002-status.md`

Three findings from the implementation changed this plan's design, and the
closure record §5 is the authority on them:

1. **`Esc` never reaches a hosted widget and `Enter` closes nothing** (§4's first
   invariant, resolved against advertising either). `SidePanelHost::handle_key`
   claims `Esc`/`q` as the universal exit and blurs before the panel is
   consulted, and docking popped the box off the modal stack, so
   `TuiMsg::CloseDialog` runs against an empty one. A docked `InfoDialog` has no
   in-panel dismiss key. Recorded as a host-level design question under §14's
   stop condition, **not fixed here**.
2. **The `custom_footer` strings are not replayed into `keymap_hint`** (§6's
   "prefer it" option, declined on evidence). Both reachable footers are
   modal-path text whose `Esc: cancel` / `Esc close` is false hosted, and
   `/shell-show`'s additionally advertises `k kill` where `handle_key` scrolls.
   All four call sites needed no edit.
3. **The `scroll` sentinel is not written back by the render pass**, contrary to
   §3.3's phrasing, and must not be — `set_styled_content_preserving_scroll`
   restores it every refresh. The clamp applies to a local `start_idx`.

Repository baseline: `f8286a2849660d1e386a61ceaf663906451961d0`
(`docs(plans): record the tui-panel M001 commit SHA`), which carries both `file-picker-rework`
M001 (`b4f4b916f`) and `tui-panel-surface-convergence-corrective` M001 (`18bcfa3d9`).

Source roadmap:

- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` §7 M002a, §7.1 split
  rationale, §4.1 seam counts, §4.2 chrome clobber, §4.8 keymap fallback

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs: none required. This milestone changes no identity, ownership boundary,
protocol, storage layout, or authorization semantics. `SidePanelId` variants, `config_key`
spellings and persisted bindings are untouched.

Adjacent, not owned here: `plans/subsystems/file-picker-rework-roadmap.md` (M001 committed;
its own M002 deliberately unopened). `plans/implementation/tui-panel-surface-convergence-corrective/003-dockable-box-chrome-and-keymap-adoption.md`
is M002b and depends on this milestone.

Primary class: capability (with polish)

## 1. Objective

Make a hosted `InfoDialog` stop overwriting its host's chrome, and make it advertise its own
real verbs — in one widget, which retires 16 `DialogType` clobber sites and the
`GENERIC_KEYS` misadvertisement for every panel behind them.

Acceptance is two externally visible facts: inside a hosted side column, the host's border,
title and `●` focus marker survive an `InfoDialog` render; and the panel status line states
verbs `InfoDialog::handle_key` actually dispatches, instead of the shared
`"j/k scroll · i act"` fallback that names an `i` this widget never arms.

## 2. Why this milestone is ready

Both hard dependencies are closed and committed:

- `tui-panel-surface-convergence-corrective` M001 is `closed`
  (`plans/closure/tui-panel-surface-convergence-corrective/001-status.md`) and committed as
  `18bcfa3d9`. It established `PanelFill` and the `Placeholder` `render_in_panel` override,
  which is the second worked example of this same pattern.
- `file-picker-rework` M001 is committed as `b4f4b916f` and is the **first** worked example.
  `Component::keymap_hint() -> Option<String>` and
  `Component::render_in_panel(&mut Frame, Rect, &Arc<Theme>)` are final, both defaulted, and
  `FilesDialog` already carries both.

The seam signatures were re-read at `f8286a284` and are unchanged since the closure record
recorded them. No interface blocker remains, and the shared-file collision risk that blocked
M001 does not apply: `src/tui/components/dialogs/info.rs` is touched by neither closed
milestone.

## 3. Current implementation evidence

Read at `f8286a284`. Re-verify before editing; symbols may have moved.

### 3.1 The widget and its fan-out

`src/tui/components/dialogs/info.rs` — one `InfoDialog` struct, `impl Component` at the top
of the file. `docks_as_viewer()` returns `true`, so the host will resolve
`Shift+←/→/↑/↓` as the docking gesture before this dialog sees the key.

`dialog_type_for_info_type()` maps **17 `InfoType` arms onto 16 distinct `DialogType`s**:

| `InfoType` | `DialogType` | | `InfoType` | `DialogType` |
|---|---|---|---|---|
| `Context` | `Context` | | `WorktreeList` | `WorktreeList` |
| `Cost` | `Cost` | | `GoalShow` | `GoalShow` |
| `Usage` | `Usage` | | `MemoryResults` | `MemoryResults` |
| `ShellShow` | `ShellShow` | | `DoctorReport` | `DoctorReport` |
| `TerminalShow` | **`Terminal`** | | `Agents` | **`Agent`** |
| `Stats` | `Stats` | | `Collaborators` | `Collaborators` |
| `TaskList` | `TaskList` | | `Team` | `Team` |
| `Logs` | `Logs` | | `Control` | `Control` |
| `ProjectInit` | **`Team`** | | | |

Three facts here are not in the roadmap and each constrains this milestone:

1. **`InfoType::ProjectInit` and `InfoType::Team` both map to `DialogType::Team`.** Two
   distinct content types share one `DialogType`. Any test keyed by `DialogType` cannot
   distinguish them, and this milestone adds a `keymap_hint` whose content differs between
   them (only `ProjectInit` arms `a`). Test by `InfoType`, not `DialogType`.
2. **`TerminalShow` → `DialogType::Terminal`, which is a bespoke panel.** A hosted
   `InfoDialog` carrying terminal output can never be adopted. This is the case M001's
   `SidePanelId::discards_a_docked_box()` guard exists for; this milestone must not
   disturb it, and must not assume every `InfoDialog` is dockable.
3. **`Agents` → `DialogType::Agent`, the one panel that genuinely discards a docked box.**
   Same constraint.

So "retires 16 clobber sites" is the upper bound. The reachable-hosted subset is those whose
`DialogType` resolves to a non-bespoke panel; the exact subset must be enumerated in W1
rather than assumed.

### 3.2 The clobber

`InfoDialog::render` splits its `area` into **three** chunks and renders each with its own
`Block::default().borders(Borders::ALL)`: `title_block`, `content_para`, `footer_block`.
`render_hosted` (`src/tui/panels/mod.rs`) has already painted the panel border, its title and
the `●` focus marker into the same rect and then calls `render_in_panel`, which defaults to
`render`. Three stacked bordered boxes therefore overwrite the host's single border, title and
focus dot. A focused and an unfocused hosted `InfoDialog` render identically, and nothing on
screen says which panel holds the keyboard.

### 3.3 The viewport coupling — the real hazard

The content chunk's height feeds `visible_lines`, which feeds both the scroll clamp and the
`Showing {start}-{end} of {total}` indicator. `handle_key`'s `Down`/`j` arm deliberately does
**not** bound `scroll`:

> No logical-line bound here: a line that wraps occupies several rows, so the reachable maximum
> is larger than `lines.len()` … The render pass clamps `scroll` to the true maximum, and
> `saturating_add` keeps the `usize::MAX` sentinel from wrapping on repeat.

and `scroll_to_end()` — used by append-only windows such as `/logs` — sets
`self.scroll = usize::MAX`, with the same comment: *"Scroll offsets are counted in wrapped
rows, so the end position is not derivable from `lines.len()` without the render width. The
render pass clamps `scroll` to the real maximum … clamping against the logical line count here
left the final line off-screen whenever an earlier line wrapped."*

**The hosted path must compute `visible_lines` exactly as the modal path does, or the tail of
the content becomes unreachable with no error anywhere.** This is the single most likely
silent failure in this milestone and it is invisible in a screenshot — the panel simply stops
scrolling two rows early. It is also the reason this plan forbids the two obvious shortcuts:

- defaulting `render_in_panel` to a bare `area.inner(Margin::new(1, 1))` call into the
  existing three-chunk path (the clamp then silently shrinks by two rows);
- deleting the footer chunk without recomputing the content chunk's height.

The mitigation is structural, not a test: one private routine owns wrapping, clamping and
content rendering, parameterised by whether to draw chrome, and **both** `render` and
`render_in_panel` call it. The two paths cannot diverge because there is only one
implementation.

### 3.4 The footer, and where its text comes from

The footer text is composed in `render`:

- `custom_footer` when set, otherwise the scroll indicator, otherwise the literal
  `" j/k scroll  |  Esc/Enter close "`, joined with `"  |  "`.

`set_custom_footer` is called from **four sites outside `info.rs`**:

| Site | Text |
|---|---|
| `src/tui/runtime/command_dispatch.rs` | `"a: publish   Esc: cancel"` |
| `src/tui/commands/interactive_terminal.rs` (two sites) | caller-supplied |
| `src/tui/commands/shell.rs` (two sites) | caller-supplied |

Every one of these describes verbs, so retiring the in-panel footer relocates all of it to
`keymap_hint`. The `ProjectInit` case is the one where they must agree exactly: the footer
says `a: publish` and `handle_key` arms `a` → `TuiMsg::ProjectInitApprove` for that
`InfoType`. That agreement is the model for every other `keymap_hint` in this milestone.

### 3.5 The keymap the box actually implements

From `handle_key`, the complete verb set:

| Key | Effect | Condition |
|---|---|---|
| `Up`, `k` | scroll up, clamped at 0 | always |
| `Down`, `j` | scroll down, `saturating_add`, unbounded | always |
| `a` | `TuiMsg::ProjectInitApprove` | `info_type == InfoType::ProjectInit` only |
| `Enter`, `Esc` | `TuiMsg::CloseDialog` | always |

`GENERIC_KEYS` advertises `"j/k scroll · i act"`. For this widget `i` is dispatched by
nothing, and `Enter`/`Esc` and the conditional `a` are absent. That is the defect M002a
removes.

### 3.6 Existing coverage

`tests/tui_panel_docking.rs` already asserts the shared docking gesture for `Cost` and
`Usage` — two members of this fan-out. Its own comment records that the remainder "are
produced by `open_info_dialog` / dedicated helpers rather than by `open_dialog`, so a test
cannot mount them without reaching into `dialog_state`". No test asserts that the host's
chrome survives an `InfoDialog` hosted render.

## 4. Invariants that must not regress

- **`Enter`/`Esc` close must keep working when hosted, and the hint must say so only if it
  does.** Verify what the host does with `Esc` on a focused hosted panel before asserting it
  in a `keymap_hint`. If the host intercepts either, the hint names the real verbs minus the
  intercepted ones — this is a finding, not a licence to advertise a dead key.
- **The scroll clamp must be identical in both paths.** Assert reachability of the last
  wrapped line in both, not just correctness of the chrome.
- **`InfoType::ProjectInit` and `InfoType::Team` keep distinct behaviour.** They share a
  `DialogType`; nothing may collapse them.
- **Bespoke targets stay bespoke.** `TerminalShow` and `Agents` must continue to refuse
  adoption through M001's `discards_a_docked_box()` path, untouched.
- **`custom_footer` callers keep working.** The four sites above must produce a visible hint
  somewhere; a retired in-panel footer with no replacement is a regression.
- **No `InfoDialog` content, scroll semantics, or line wrapping changes.** Only chrome and
  the advertisement of verbs move.
- **No new command, key, gesture, panel, data read, or daemon call.**
- `SidePanelId` variants, `config_key` spellings, persisted bindings, placement and sizing are
  unchanged.

## 5. Scope

### In scope

- `src/tui/components/dialogs/info.rs` — `render_in_panel` override, the shared
  body-rendering routine, the `keymap_hint` override.
- `src/tui/commands/interactive_terminal.rs`, `src/tui/commands/shell.rs`,
  `src/tui/runtime/command_dispatch.rs` — reconcile `custom_footer` callers with the retired
  in-panel footer.
- `tests/` — rendered-buffer chrome assertions for the hosted `InfoDialog`, scroll-reachability
  assertions in both paths, and a `keymap_hint` correctness assertion keyed on `InfoType`.
- `docs/tui.md`, `architecture/tui.md` — the panel list entry for the `InfoDialog` family and
  the keymap authority note.

### Explicitly out of scope

- Every other dockable widget. `diff`, `research`, `review`, `plan`, `mcp`, `plugin`,
  `security_review`, `panel_picker`, `keybind`, `theme`, `tree`, `task_view` and `plugin` are
  **M002b**.
- Bespoke panels (`Session`, `Agents`, `Chat`, `Terminal`, `Help`). They own their
  `SidePanel` impls and never render through `render_hosted`.
- Any `Component` trait default. The inset-by-one default flip was **withdrawn** at the
  roadmap §7 on evidence; it does not reduce the override count, because the host passes
  `frame.area()` — border cells included — so an inset moves a widget's own `Block` one
  character inward without preventing it.
- Any static guard asserting "hosted widgets do not paint a `Block`". M001 established this is
  unachievable: `Component::render` is the modal path where a border is correct, so no static
  check distinguishes the two. The guard here is **behavioural**.
- M003: `set_theme()` colour caches, `Color::White` literals, `plan`'s duplicated render
  paths, `centered_rect` de-duplication, any shared chrome/geometry helper.
- File-picker behaviour. New panels. Protocol, storage, identity, authorization.

## 6. Required production changes

### Core/domain

None. No domain type, no `SidePanelId` variant, no `DialogType` variant.

### Storage and migrations

None. `STORAGE_LAYOUT_VERSION` unchanged.

### Protocol and DTOs

None. No DTO field, no projection, no wire contract.

### Runtime and concurrency

None. No new task, request, lease, or generation fence. The widget is unchanged in
lifecycle; only its render and advertisement change.

### Frontend or operator surface

**`InfoDialog` — extract one body routine, two chrome modes.** The wrapping, the
`visible_lines` computation, the `scroll` clamp and the content rendering must live in one
private routine taking the rect to fill and a flag (or an equivalent) for whether to draw
chrome. `render` calls it with chrome; `render_in_panel` calls it without.

This is required rather than merely tidy: §3.3 shows `scroll_to_end()` depends on the render
pass clamping against the true wrapped-row maximum, so two independent implementations of
the clamp are a latent off-by-N that no unit test will reliably catch.

**`render_in_panel` — inset, then render chrome-free.** Follow `FilesDialog`:

```rust
let inner = area.inner(Margin::new(1, 1));
if inner.height == 0 || inner.width == 0 {
    return;
}
```

`area` is the host's **full** column rect, border cells included; the inset is mandatory, not
cosmetic. The guard is copied verbatim from `FilesDialog::render_in_panel` because a
zero-sized inner rect reaching `Paragraph` is a panic path.

In chrome-free mode: no `title_block`, no `footer_block`, no `Borders::ALL` anywhere. The
host's border, title and `●` must survive untouched.

**Retire the in-panel footer, but do not lose its content.** The footer is removed as a
*drawn box*. Its verbs move to `keymap_hint`. The scroll indicator is a status affordance,
not a verb — either render it inside the chrome-free body (bottom row of the inset) or drop
it; **record which in the closure record**, because dropping it removes the only visible
"you are not seeing everything" signal in a narrow column.

**`keymap_hint` — declare the real verbs, per `InfoType`.** The hint is
`Some(..)` for every `InfoType`, and its text must be derived from what `handle_key`
dispatches in that mode:

- always: `j/k scroll`;
- `Enter`/`Esc` close — **only if the hosted path actually honours them** (see §4);
- `a` publish — **only** for `InfoType::ProjectInit`, matching the existing
  `"a: publish   Esc: cancel"` footer and the armed key.

A `custom_footer` supplied by a caller is the same information, so if `keymap_hint` can read
`self.custom_footer` it should prefer it over any hardcoded default — that keeps the four
external call sites honest without editing their strings. Prefer that; if the footer's shape
is too varied to reuse verbatim, normalise it in `keymap_hint` and update the four call sites
accordingly. Either is acceptable; silently preferring a hardcoded default over a
caller-supplied footer is not.

### Security and authorization

None. No box gains a read, a fetch, or a privilege. Removing `InfoDialog`'s chrome does not
change what any panel contains.

### Documentation and static guards

No static guard. Two behavioural test families, both in §10: host-chrome survival, and
scroll-reachability parity between the two render paths.

## 7. Ordered work packages

### W1 — Baseline-failing tests (the acceptance signal)

Land first, failing. As in M001, these are the proof the work landed.

1. A rendered-buffer test mounting a hosted `InfoDialog` and asserting the host's border,
   title and `●` focus marker are present in the column. Mount via the same path production
   uses; `render_hosted` is `pub` in `src/tui/panels/mod.rs`, so this needs no
   `dialog_state` reach-through — if it turns out to, stop per §14.
2. The same test for a **second** `InfoType` chosen to have different content (e.g. `Logs`,
   whose `scroll_to_end` path is exercised).
3. A scroll-reachability test asserting the last wrapped line is reachable in the hosted
   path, with content long enough to wrap. This is the §3.3 hazard and the highest-value
   test in the milestone.
4. A `keymap_hint` test asserting no advertised verb is absent from `handle_key` and no
   dispatched verb is unadvertised, keyed on `InfoType` (not `DialogType`, per §3.1).
5. An assertion that `InfoType::ProjectInit` and `InfoType::Team` produce different hints.

Do not modify production code in W1.

### W2 — Shared body routine and `render_in_panel`

Extract the one routine, add the override, retire the chrome. Re-run W1 tests 1–3. **Test 3
must pass here** — if the clamp needed a different `visible_lines` than the modal path and the
two are now genuinely one routine, it passes; if it needed a manual nudge, the extraction is
wrong.

### W3 — `keymap_hint`

Implement the hint, prefer `custom_footer` where it applies, reconcile the four call sites.
Re-run W1 tests 4–5.

### W4 — Docs and guards

`docs/tui.md` and `architecture/tui.md`. No static guard is added (§6). If W1 needs a
production-visible seam to mount an `InfoDialog`, stop per §14 rather than adding a
test-only door.

## 8. Failure, cancellation, restart, and contention semantics

- A widget whose hosted render produces a zero-sized inner rect returns without painting. The
  host's chrome is already painted, so the panel shows an empty column rather than a panic.
- Scroll state is preserved across the chrome change: `scroll` is an offset in wrapped rows and
  stays in the same units in both paths. Changing its unit is a defect, not a refactor.
- `scroll_to_end()`'s `usize::MAX` sentinel must still be clamped by the render pass in the
  hosted path. If the hosted path ever fails to clamp, `/logs` as a docked panel renders an
  empty body. W1 test 3 is the guard.
- No partial-failure, duplicate-delivery or contention behaviour changes: the widget is
  single-owner and synchronous.

## 9. Compatibility and migration

No persisted format change. `STORAGE_LAYOUT_VERSION` unchanged. No config key added or
removed. No protocol negotiation.

The only behavioural change a user can observe is that a hosted `InfoDialog` no longer paints
its own border, and its status line states its real verbs instead of `i act`. Modal rendering
is byte-identical — that is the property W2 must preserve, and it is asserted, not assumed.

## 10. Required tests

### Focused unit tests

- `render_in_panel` in the chrome-free mode emits no `Borders::ALL` cell inside the column.
- `keymap_hint` per `InfoType`: `ProjectInit` advertises `a`; `Team` does not; both advertise
  scroll; both advertise close if §4's check confirms the hosted path honours it.
- `custom_footer` takes precedence over the hardcoded default in `keymap_hint`.

### Integration tests

- Rendered-buffer: host border, title and `●` survive a hosted `InfoDialog`, for at least two
  `InfoType`s.
- Rendered-buffer: no in-panel footer box is present when hosted.
- Scroll: the final wrapped line is reachable in the hosted path (the §3.3 hazard).
- Scroll: the same content reaches the same end position modally and hosted.
- Modal rendering is unchanged: an existing modal `InfoDialog` render test still passes
  unmodified. This is the regression net for W2's extraction.
- `tests/tui_panel_docking.rs` passes in full, including its `Cost` and `Usage` cases.

### Restart and recovery tests

None. No persistence, no process lifecycle.

### Contention and cancellation tests

None. Synchronous single-owner widget.

### Security and negative tests

- Every verb named by `keymap_hint` for a given `InfoType` is dispatched by `handle_key` for
  that `InfoType`. This is the negative direction and it is the one that matters: advertising
  a dead verb is the defect being removed.
- `InfoType::ProjectInit` and `InfoType::Team` hints differ despite sharing a `DialogType`.

### Migration and compatibility tests

None. No migration.

## 11. Required verification commands

```bash
cargo test -p codegg info_dialog              # focused
cargo test --test tui_panel_docking           # existing 7-box gesture sweep
cargo test --test tui_panel_manager_reachability   # M001's exhaustive sweep
cargo fmt
scripts/verify.sh quick                        # the gate
scripts/verify.sh full                         # before closure
```

Never `--all-features`. Note that CI runs a **strict subset** of `quick`, so a green CI run
does **not** imply a green local `quick`; report local and CI evidence separately and never
present an unrun command as passing.

**Known environment condition at this baseline:** `verify.sh full` exits 100 in this
environment on pre-existing failures unrelated to this work — M001's closure record §4.1
records 11 failures that reproduce identically with `src/` stashed to `b77c8156`, plus one
load-sensitive timeout that passes in isolation. Re-derive that baseline before attributing
any failure to this milestone, and record the comparison in the closure record.

## 12. Documentation updates

- `docs/tui.md` — the panel list rows for the `InfoDialog` family, and the panel gesture /
  keymap section to agree with the new status line.
- `architecture/tui.md` — panel host module map; the `render_in_panel` / `keymap_hint`
  adoptor counts (currently 2 and 1, both in `architecture/tui.md`'s verified-counts table if
  present); the `InfoDialog` description.
- `.opencode/skills/tui/SKILL.md` — panel authoring guidance, which names `InfoDialog` as
  the template for chrome and keymap adoption.
- `plans/registry.md` in the same commit as any status change.

## 13. Acceptance criteria

1. A hosted `InfoDialog` leaves the host's border, title and `●` focus marker intact in the
   rendered buffer, for every `InfoType` reachable through a non-bespoke panel.
2. A hosted `InfoDialog` draws no bordered box of its own.
3. The last wrapped line of `InfoDialog` content is reachable by scrolling when hosted, by
   the same end position as modally.
4. The panel status line for a hosted `InfoDialog` names only verbs `handle_key` dispatches
   in that `InfoType`, and omits none of them.
5. `InfoType::ProjectInit` advertises `a`; `InfoType::Team` does not.
6. Modal `InfoDialog` rendering is unchanged.
7. `InfoType::TerminalShow` and `InfoType::Agents` still refuse adoption through M001's
   existing path, unchanged.
8. `tests/tui_panel_docking.rs` and the M001 reachability sweep pass unmodified.
9. No new command, key, gesture, panel, data read, or daemon call exists.
10. `docs/tui.md` and `architecture/tui.md` describe the shipped behavior.

## 14. Stop conditions

Stop and report rather than improvise if:

- **The hosted path intercepts `Enter` or `Esc` on a focused `InfoDialog`.** Then the real verb
  set differs between modal and hosted, `keymap_hint` cannot state one honest set, and
  resolving that is a host-level design question, not a widget fix.
- **`visible_lines` cannot be made identical between the two paths** without duplicating the
  clamp. That means the extraction in W2 did not work; report it. Do not hand-tune a constant.
- **Mounting an `InfoDialog` for a test requires reaching into `dialog_state`.** The existing
  suite documents this obstacle for the `InfoDialog` family. If a production-only seam is
  required, that is a design question — report it, do not add a test-only public door.
- **The bespoke targets (`TerminalShow`, `Agents`) would need different handling** to adopt
  chrome than the rest. That is M001's `discards_a_docked_box()` territory; report it.
- **`custom_footer`'s four call sites cannot be reconciled** with `keymap_hint` without
  changing a verb or a message a user relies on.
- **Any change would alter `InfoDialog` content, wrapping, or scroll semantics**, or would
  touch bespoke panel behaviour.

## 15. Closure evidence required

- W1's tests green, including the scroll-reachability test.
- Proof that the tests fail before W2 — recorded, not asserted.
- `scripts/verify.sh quick` green locally; `cargo fmt --check` clean; clippy `-D warnings`
  clean.
- `scripts/verify.sh full` run locally, with any failure attributed against the M001-recorded
  pre-existing baseline rather than left unexplained.
- An explicit statement of what ran locally versus what only CI ran. No unrun verification
  presented as passing.
- The decision recorded for the scroll indicator (kept in the body, or dropped).
- `plans/closure/tui-panel-surface-convergence-corrective/002-status.md` written and accepted;
  only then is M002a `closed`.
- Registry audit: whether M002b became newly eligible. A closure that unblocks nothing is
  valid and must be recorded as such.

## 16. Handoff notes

- The seam is `render_in_panel(&mut Frame, Rect, &Arc<Theme>)`. `area` is the host's **full**
  column rect including border cells. `FilesDialog::render_in_panel` in
  `src/tui/components/dialogs/files.rs` is the worked template for the inset, the zero-size
  guard, and the doc-comment style that records why the in-box footer was retired with it.
- `Placeholder::render_in_panel` in `src/tui/panels/hosted.rs` is the second worked example,
  added by M001. Both are small; this milestone is the first where the box has real scroll
  state.
- **The viewport coupling (§3.3) is the thing to get right.** `Down`/`j` is deliberately
  unbounded and `scroll_to_end()` sets a `usize::MAX` sentinel precisely because the render
  pass owns the clamp. Two render paths with two clamps is the bug this milestone exists to
  avoid creating.
- `InfoType::ProjectInit` and `InfoType::Team` share `DialogType::Team`. Test by `InfoType`.
- `InfoType::TerminalShow` and `InfoType::Agents` resolve to bespoke panels and are not
  adoptable. Do not assume all 16 are reachable as hosted panels; enumerate in W1.
- Nothing here may touch another dockable widget. That is M002b, and M002b's plan assumes this
  one lands first.