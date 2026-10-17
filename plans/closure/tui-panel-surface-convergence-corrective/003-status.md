# TUI Panel Surface Convergence Corrective Milestone M002b — Closure record

Status: **closed**

Milestone: extend what M002a proved on `InfoDialog` to the remaining twelve
dockable boxes — each stops painting its own chrome into the host's rect, and
each advertises the verbs its own `handle_key` dispatches *from a column*.

Plan: `plans/implementation/tui-panel-surface-convergence-corrective/003-dockable-box-chrome-and-keymap-adoption.md`
Roadmap: `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md`

Baseline: `f8286a2849660d1e386a61ceaf663906451961d0`
(`docs(plans): record the tui-panel M001 commit SHA`), plus M002a landed.

M002a and M002b were committed together with M003 as a single commit, on the
grounds recorded in `004-status.md` §5 — the three interleave in the same files
and were only ever verified as one unit. That commit was fast-forwarded (by
rebase) onto `origin/main` `e432e6e47`; see `004-status.md` §4.1.

---

## 1. What landed

Twelve widget files, all in `src/tui/components/dialogs/`:

| Widget | Panel | HINT |
|---|---|---|
| `tree.rs` | Tree | `j/k move · e expand · f fork · enter select` |
| `keybind.rs` | Keybinds | `j/k move · enter remap · r reset · e export · i import` (Normal, WaitingForKey); `None` in Export/Import |
| `theme.rs` | Themes | `j/k navigate · enter apply` / `j/k preview · enter commit` |
| `task_view.rs` | Task View | `j/k move · ^d/^u page · Home/End end · g/G end · enter open · d detail · r refresh · x cancel · u resume · t trigger · T rotate · X revoke · e trigger refresh` |
| `review.rs` | Review | `j/k move · enter diff` / `⌫ back to list` |
| `research.rs` | Research | `j/k move · enter open · r refresh` / `j/k move · enter section` / `j/k scroll · ^d/^u page · Home/End jump · ⌫ back` |
| `security_review.rs` | Security | `j/k move · ^d/^u page · f filter · n notes · p prompts · enter preview · h jump hunk · ]/[ hunk nav` |
| `plugin.rs` | Plugins | `j/k scroll` |
| `mcp.rs` | MCP | `j/k move · enter actions` / `j/k move · enter run` |
| `diff.rs` | Diff | `j/k scroll · ^d/^u page · s side-by-side · Home/End jump` |
| `plan.rs` | Plan | `j/k select` |
| `panel_picker.rs` | (none) | `j/k move · ^d/^u page · i/a filter · enter send` / `⌫ pop · enter send` |

Each file gained the same three things: a module-local `enum Chrome { Owned,
Hosted }`, one private **inherent** body routine that both `render` and
`render_in_panel` call, and a `keymap_hint` derived from `handle_key`. In
`Chrome::Hosted` the widget paints no `Block`, no title, no heading, no footer
and no border-title keymap; `render_in_panel` insets `area.inner(Margin::new(1, 1))`
and returns early when the interior is empty.

Seam counts, re-measured by `grep -rn '    fn render_in_panel' src/` and
`grep -rn '    fn keymap_hint' src/` excluding `components/component.rs` where
the seam is declared:

| Seam | Before | After |
|---|---|---|
| `render_in_panel` overrides | 3 | **15** (13 dialogs + `Placeholder`) |
| `keymap_hint` overrides | 2 | **14** |

`GENERIC_KEYS` (`"j/k scroll · i act"`) is unchanged and now correct for **no**
dockable box. It remains the fallback for the bespoke panels.

## 2. Requirement-to-evidence matrix

| # | Requirement (plan §13) | Evidence |
|---|---|---|
| 1 | Host border, title and `●` survive a hosted render, every widget | `*_leaves_the_panel_chrome_alone` ×12 — `assert_host_chrome` |
| 2 | No hosted widget emits a `Block` of its own | same 12, plus `assert_no_second_box`; and each `render_body` has a `Chrome::Hosted` arm with no `Block` |
| 3 | Last row / full selection range reachable hosted | `a_docked_report_…ends_at_its_last_line`, `a_docked_plugin_still_says_where_the_end_is`, `both_paths_reach_the_same_last_row` |
| 4 | The status line names exactly the verbs `handle_key` dispatches, per mode | `*_advertises_only_the_verbs_*` — `hint_matches` checks **both** directions |
| 5 | No keymap survives only inside a border title or an in-panel footer | `plan`'s title exists once, in the hint; twelve footers retired to `Chrome::Owned` |
| 6 | Modal rendering unchanged | every `*_leaves_the_panel_chrome_alone` was red from the same widget's modal render at baseline; `the_modal_still_draws_its_own_box`, `the_modal_keeps_its_border_and_its_footer`, `the_modal_keeps_all_three_of_its_boxes` |
| 7 | `diff` and `plan` chrome-free in **every** render path | `no_hosted_size_lets_the_body_paint_its_own_chrome` — 5 sizes each, plus structural: both route through one `render_body` |
| 8 | `panel_picker` gains no `dock_target` arm; `Shift+←` recorded | `the_panel_picker_has_no_dock_target_and_loses_the_docking_gesture`; `App::dock_target` untouched |
| 9 | `tui_panel_docking.rs` and `tui_panel_manager_reachability.rs` pass unmodified | verification log §7 |
| 10 | No M003 item landed | §5.3 |
| 11 | No new command, key, gesture, panel, data read or daemon call | §5.4 |

## 3. Baseline transcript

`tests/tui_panel_host_chrome.rs` landed first, failing, with no production code
touched:

```
$ cargo test --test tui_panel_host_chrome
test result: FAILED. 2 passed; 24 failed
```

Representative verbatim failures at `f8286a284`:

```
thread 'tree_leaves_the_panel_chrome_alone' panicked at tests/tui_panel_host_chrome.rs:130:5:
TreeDialog: the panel title "Tree ●" is the host's, not the widget's — row 0 is "┌ Tree ───…─┐"

thread 'plan_leaves_the_panel_chrome_alone' panicked at tests/tui_panel_host_chrome.rs:124:9:
PlanDialog: the host's side border was overwritten at row 3 — "└────────┘"

thread 'review_advertises_only_the_verbs_its_current_mode_delivers' panicked at …:183:9:
ReviewDialog/FileList: answers no keymap_hint, so the panel host advertises
"j/k scroll · i act" for a box that dispatches none of it

thread 'a_docked_report_whose_scroll_is_saturated_ends_at_its_last_line' panicked at …:
saturating the report scroll and rendering hosted left the last line unreachable —
the tail is scrolled past with nothing to scroll back.
```

**The two that passed are guards, not signals**, and are recorded as such:

- `a_docked_plugin_still_says_where_the_end_is` — `plugin`'s scroll already
  worked modally, so this could not fail at baseline. It exists to catch the
  milestone *removing* the indicator when it retired the footer.
- `the_panel_picker_has_no_dock_target_and_loses_the_docking_gesture` — this is
  §3.5's recorded answer, not a behaviour change: the structural facts it
  asserts were already true at baseline, and the test pins them so the record
  cannot silently go stale.

`assert_no_second_box` is likewise a guard for any widget whose modal layout is
a single full-rect block: the second border lands exactly on the first and
leaves no interior corner behind, so it could not fail at baseline. It catches a
*partial* fix — an outer border dropped while an inner one remains, which is
exactly what `mcp`'s two panes would have produced.

After the change: `test result: ok. 26 passed; 0 failed`.

## 4. In-module unit tests

`cargo test -p codegg --lib dialogs::` → **292 passed, 0 failed**. Sixteen of those
are new, all appended by this milestone — 6 in `tree.rs`, 5 in `diff.rs`, 5 in
`plan.rs`; `diff.rs` and `plan.rs` had no `#[cfg(test)]` module at all before this
(`grep -c cfg(test)` → 0 for both), and `tree.rs`'s is entirely new. What they are:

- `tree.rs`: `the_hosted_path_draws_no_border_of_its_own`,
  `the_hosted_path_keeps_the_boxes_own_heading_and_footer`,
  `both_paths_reach_the_same_last_row`, `keymap_hint_names_no_key_the_host_answers_first`,
  `the_modal_still_draws_its_own_box`,
  `a_column_with_no_interior_paints_nothing_and_does_not_panic`.
- `diff.rs` and `plan.rs`: one **every-render-path sweep** each, over five area
  sizes chosen to make each layout branch take a different shape (including the
  size that trips `diff`'s `height < 7` early return), plus the
  modal-still-chrome direction, the no-dead-key-in-hosted-copy direction, the
  zero-interior guard, and the hint check.

## 5. Findings

### 5.1 The host answers four more keys than M002a found

M002a recorded that a hosted box never sees `Esc`/`q`, and that `Enter` arrives
but closes nothing. Both true, and both incomplete. Reading
`SidePanelHost::handle_key` (`src/tui/panels/mod.rs`) top to bottom, before a
widget is consulted the host also resolves a panel `InputAction` and routes it
to `MoveDirection`:

```rust
if ctx.input_mode == crate::tui::input::InputMode::Normal {
    let dir = match action { Some(InputAction::DirectionLeft) => …, … };
    if let Some(dir) = dir { return Some(PanelKey::Intent(TuiMsg::MoveDirection(dir))); }
}
```

and `insert_panel_direction_bindings` (`src/tui/input.rs`) binds **the four
arrows** plus **bare and shifted uppercase `H`/`J`/`K`/`L`**.

Consequences, all verified against `handle_key` rather than inferred:

- Every `↑/↓ navigate` in the twelve in-panel footers was dead the moment the box
  docked.
- `/tasks` advertised `J/K reorder`. It cannot fire from a column. The doc-comment
  on `task_view.rs`'s `HINT` records it.
- `/security-review` advertised `H copy hunk`. It cannot fire from a column. That
  one was false *even modally* in a vim-style binding table where `H` means
  "left".
- **Lowercase `j`/`k` are not panel directions and do arrive**, which is why
  every hint spells scrolling `j/k` and never `↑/↓`.
- `Home`, `End`, `PageUp`, `PageDown` are not panel directions and arrive, so
  `diff`, `task_view`, `research` and `security_review` legitimately advertise
  them.

`hint_matches` in the test suite encodes this as `HOST_ANSWERS_FIRST`, so the
rule is enforced per widget rather than trusted to review.

### 5.2 The plan's work packages covered eleven of twelve widgets

§3.1 of the plan lists twelve widgets. W2 names `tree`, `keybind`, `theme`,
`task_view`; W3 names `review`, `security_review`, `plugin`, `research`; W4 names
`diff` and `plan`; W5 names `panel_picker`. **`mcp` is in none of them** — it is in
the §3.1 table, with two bordered panes and two copies of the same false footer.

It was implemented, and the suite caught it: `mcp_advertises_only_the_verbs_its_
current_mode_delivers` failed with *"answers no keymap_hint"* long after W2–W5
were green. Recorded because "missing one path is the most likely way this
milestone ships half-done" applies to a missing *widget* at least as much, and
because a reader of the plan should know its work packages did not cover its own
inventory.

### 5.3 §14 fired for `panel_picker`; the §3.3 hazard was real for `research`

**§14, footer retirement.** `panel_picker` declares `docks_as_viewer() == true`
but `DialogType::PanelPicker` has no `App::dock_target` arm, so it never docks.
Its footer is therefore its *only* affordance, and moving it to a `keymap_hint`
the host never reads would leave the modal with no keymap at all — the exact
stop condition "retiring a footer would remove the only visible status affordance
in a widget with no other affordance". **The footer stays**, with a doc-comment
saying why; `keymap_hint` and `render_in_panel` were added anyway so the seam is
truthful if a mapping ever appears. **No `dock_target` arm was added.**

**§3.5's recorded answer.** The host resolves the docking gesture,
`dock_target()` returns `None`, and the gesture refuses — the key is swallowed
before `PanelPicker::handle_key` runs. For the `/panels` picker itself this is
harmless: `Shift+←` has no meaning there. It is an inconsistency between the two
halves of the docking contract, not a visible defect. No visible problem, so no
arm.

**§3.3, viewport.** `research`'s report view stored a row offset that no render
path clamped:

```rust
KeyCode::Home => { self.content_scroll = usize::MAX; None }   // handle_key
…
Paragraph::new(lines).scroll((self.content_scroll as u16, 0))  // render_report_view
```

`usize::MAX as u16` is `u16::MAX`, so `Home` rendered the report **blank**, and
because the offset only ever grew, `j`/`k` could not walk back out. The clamp now
lives in the shared body routine, derived from the rect actually being filled, in
**both** paths. This is the one deliberate modal-output change in the milestone,
and it is in one state only (after `Home`); it is mandated by the milestone's own
structural rule rather than being a new feature, because the alternative was
shipping a box whose End key erases its content.

### 5.4 Three pre-existing scroll defects found and deliberately not fixed

Found by reading the code. Not fixed, because fixing them is a behaviour change
and §4 forbids those:

1. `review`'s `ReviewMode::DiffView` renders from line 0 and never reads
   `diff_scroll`, so its scroll is entirely inert — and its `Up|Char('k')` arm
   does `saturating_add(1)`, the same as `Down|Char('j')`.
2. `research`'s `ReportView` has `k` → `saturating_add(1)` and `j` →
   `saturating_sub(1)` on a top-down `.scroll()` offset — inverted relative to
   every other widget and relative to the arrows it shares an arm with.
3. `tree` and `theme` carry a `visible_height` field that **no production code
   ever sets**: `set_visible_height` is called only from `tests/tui.rs`, so it
   stays at its `new()` default of 10 however tall the box is. A 12-row `/tree`
   modal offers 10 rows into 6 usable ones and ratatui clips the tail. Left
   alone deliberately — re-deriving the field from the rect would change modal
   rendering, and a single field shared by the render pass and the selection clamp
   is still one source of truth, so the two paths cannot disagree with each other.
   It only disagrees with the box's own height.

**The general rule these produced**, now in `architecture/tui.md`: a widget's
viewport is either **render-derived** (`files`, `diff` — recomputed every pass
from the rect being filled, correct in both paths by construction) or
**externally-owned** (`tree`, `theme` — one field shared by paint and clamp, so
the paths agree with each other but possibly not with the box's height). Neither
is a per-widget viewport computation, which is the thing the milestone forbade.

## 6. Deviations

| Plan said | What happened | Why |
|---|---|---|
| W2–W5 cover §3.1's twelve widgets | `mcp` was in no package; implemented anyway | §5.2 — the test suite caught it |
| W5: retire `panel_picker`'s 3-row footer into `keymap_hint` | Footer **kept**; both seams added | §5.3 — §14 stop condition |
| §4: modal rendering byte-identical | Identical, **except** `research` after `Home` | §5.3 — the mandated clamp fixes a blank view |
| §10: "`panel_picker`: `Shift+←` behaviour recorded" | Recorded in §5.3 above and in the test name | §3.5 |
| W6: counts "move from 2/1 to 14/13" | Measured as **15 / 14** | The plan's base was stale (pre-M002a). Counted from source, not derived from the plan |

Ten legacy `impl Widget for &XDialog` bodies (`connect`, `goto`, `theme`, `import`,
`plan`, `tree`, `keybind`, `share`, `diff`, `template`) and the public
`render(&self, frame, area)` on `diff` and `plan` were **left in place**. Verified: none
has a **production** caller — all ten dispatch exclusively through `Component`. They are
duplicated surface, not a hosted path, and removing them is later de-duplication work.

> **Corrected during M003 planning.** This originally read "five legacy
> `impl Widget for &XDialog` bodies (`tree`, `keybind`, `theme`, `diff`, `plan`)" and
> "Verified dead: no caller in `src/` or `tests/`". Both statements were wrong. The count
> is **ten**, and the "no caller in `tests/`" half is false: `&ConnectDialog` is called
> from `src/tui/components/dialogs/connect.rs` and `tests/connect_tui_restoration.rs`,
> and `&ShareDialog` from `src/tui/components/dialogs/share.rs` — all **test-only**, and in
> tests whose purpose is to compare the Widget path against the Component path
> (`widget_and_component_render_paths_are_equivalent`, the connect restoration tests). The
> other eight have no caller at all, tests included. The production-caller finding above is
> unaffected and remains sound; the deletion is simply not a uniform cut, which is why it
> belongs to its own milestone rather than this one.

## 7. Verification log

All of this ran locally in this environment. Nothing here is a CI result.

| Command | Result |
|---|---|
| `cargo test --test tui_panel_host_chrome` (baseline) | **2 passed, 24 failed** — §3 |
| `cargo test --test tui_panel_host_chrome` (after) | **26 passed, 0 failed** |
| `cargo test -p codegg --lib dialogs::` | **292 passed, 0 failed** |
| `cargo fmt` then `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **exit 0** |
| `scripts/verify.sh quick` | **exit 0 — "Quick verification passed."** |
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` | §7.1 |

### 7.1 Full sweep

```
$ cargo nextest run --workspace --locked --profile ci --no-fail-fast
Summary [ 396.016s] 14240 tests run: 14228 passed, 11 failed, 1 timed out, 6 skipped
```

Every non-passing test is one M001 recorded, and the set is identical:

| Test | M001 recorded |
|---|---|
| `core::daemon::tests::session_create_through_daemon` | yes (`InvalidRootPath("/tmp")`) |
| `core::daemon::tests::turn_submit_rejects_when_active` | yes |
| `tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input` | yes |
| `codegg::bash_background_m002_routes background_shell_returns_handle_promptly_then_notifies_on_cancel` | yes |
| `command_routing_execution_ownership` ×4 | yes |
| `session_family control_m004_controller_lease` ×2 | yes |
| `workspace_isolation two_workspaces_register_and_resolve_independently` | yes |
| `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | yes — **timed out** at 120s |

That last one is the load-sensitive case M001 described as passing in isolation and
timing out only under a 4-way sweep. It timed out here and did **not** in M002a's
run a few hours earlier on the same machine — so it is variance in the direction
M001 recorded, not something this change introduced or fixed. Nothing in the list
touches a panel, chrome, keymap, chat, placement, agents, diff, review, research,
mcp, plan, theme, keybind, task_view or panel-picker test, and none of the twelve
widgets this milestone edits appears in any failing name.

M001 established the baseline by re-running with `src/` stashed back to
`b77c8156`; that re-derivation was **not** repeated here, so the attribution rests
on the failure *names* matching M001's recorded list exactly, not on a fresh
stash-and-run.


### 7.2 Not run

`scripts/verify.sh full` exits 100 in this environment on the pre-existing
`session_create_through_daemon` / `InvalidRootPath("/tmp")` failure that M001 and
M002a both recorded, under fail-fast. The `--no-fail-fast` sweep above is the
equivalent evidence and is what §7.1 attributes.

## 8. What ran locally versus what only CI ran

Everything in §7 ran locally. **CI has not run.** CI is a strict *subset* of
`scripts/verify.sh quick` — it omits the client/desktop boundary and the provider
wire/catalog/resilience, OpenAI endpoint and Eggwork routing guards — so a green
CI run would not have implied a green local `quick`, and nothing here should be
read as a CI pass.

## 9. Registry audit

- **Did M003 become newly eligible?** Yes. Its only stated blocker was M002b.
- **Did M003 land here?** **No.** Verified mechanically, not by inspection:
  - `Color::White` counts unchanged — `plan` 6, `mcp` 2, identical to `HEAD`. The
    only `Color::White` diff lines are two removals and two additions of the
    identical expression, i.e. an indentation move.
  - `set_theme` signatures unchanged: zero `fn set_theme` lines in the diff.
  - No `DialogType`, `SidePanelId`, `config_key`, persisted binding, placement or
    sizing change.
  - No `Component` trait default flipped.
  - No new command, key, gesture, panel, data read or daemon call.
- **M003's own counts still need re-deriving.** The five `set_theme()` colour
  caches and the `Color::White` selection foregrounds are now inside files this
  milestone restructured, so the roadmap's recorded counts for them predate this
  tree and must be re-measured, not copied forward.
- `plans/registry.md` updated in the same change: row 96 (active roadmap), row 163
  (M002a, unchanged), row 164 (M002b → closed, with the duplicated fragment left
  by an earlier truncated edit removed).
