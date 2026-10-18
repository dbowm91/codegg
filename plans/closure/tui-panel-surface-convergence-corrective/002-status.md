# TUI Panel Surface Convergence Corrective Milestone M002a — Closure Status

Status: closed

The milestone is implemented and verified locally. `InfoDialog` no longer
overwrites the chrome of the panel that hosts it, and it no longer advertises a
key it does not have.

Source implementation plan:

- `plans/implementation/tui-panel-surface-convergence-corrective/002-info-dialog-chrome-and-keymap-adoption.md`

Source subsystem roadmap:

- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` §7 M002a,
  §7.1 split rationale

Repository baseline: `f8286a2849660d1e386a61ceaf663906451961d0`, which carries
both predecessors — `file-picker-rework` M001 `b4f4b916f` and
`tui-panel-surface-convergence-corrective` M001 `18bcfa3d9`. Performed in worktree
`.worktrees/fork-446e57902e2e-0ae9640ab4bb432f9fa08368b8ae2faf` on branch
`fork/446e57902e2e-0ae9640ab4bb432f9fa08368b8ae2faf`.

Implementation commit: M002a landed in the working tree uncommitted, and was
then committed together with M002b and M003 as a single commit, on the grounds
recorded in `004-status.md` §5 — the three interleave in the same files and were
only ever verified as one unit. That commit was fast-forwarded (by rebase) onto
`origin/main` `e432e6e47`; see `004-status.md` §4.1.

## 1. Executive finding

One widget, fourteen panels. `InfoDialog` is mounted by the host for
`/cost`, `/usage`, `/context`, `/shell-show`, `/stats`, `/tasks`, `/goal`,
`/memory-search`, `/doctor`, `/collaborators`, `/team`, `/control`, `/logs` and
`/project-init`, and its modal render painted three stacked bordered boxes into
a rect the host had **already** filled with a border, a panel name and the `●`
focus marker. Painting over them did not layer; it overwrote all three. A docked
`/cost` and an unfocused `/cost` rendered identically, and the one piece of
information on screen about which panel had the keyboard was the thing
destroyed.

The second defect was a lie on a line. `HostPanel::composer_hints` falls back to
`"j/k scroll · i act"`, and `InfoDialog` dispatches no `i`. It now answers
`Component::keymap_hint` with what its own `handle_key` arms do: `j/k scroll`,
plus `a publish` for `/project-init` alone.

**Two plan findings changed the design, and one of them is a real defect this
milestone deliberately did not fix.** §5 and §6.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Host chrome survives a hosted `InfoDialog` | `a_hosted_info_dialog_leaves_the_hosts_border_title_and_focus_dot_alone`, and `every_reachable_hosted_info_type_leaves_the_host_alone` over all 14 reachable `InfoType`s | pass |
| The widget draws no box of its own | `the_hosted_path_draws_no_border_of_its_own`; `assert_no_second_box` inside the two chrome tests | pass |
| Last wrapped line reachable when hosted | `a_hosted_info_dialog_can_scroll_to_the_last_wrapped_line`; `the_hosted_path_shows_the_last_wrapped_row_and_keeps_the_sentinel` | pass |
| Hosted and modal agree on the end position | `a_hosted_info_dialog_reaches_the_same_end_position_as_a_modal_one`; `both_paths_reach_the_same_last_wrapped_row` | pass |
| The status line names only dispatched verbs | `a_hosted_info_dialog_advertises_only_verbs_it_dispatches`; `no_undispatched_or_undeliverable_verb_is_left_out_by_accident` | pass |
| `ProjectInit` advertises `a`, `Team` does not | `project_init_advertises_publish_and_no_other_content_type_does`; `project_init_and_team_hints_differ_despite_sharing_a_dialog_type` | pass |
| Modal rendering unchanged | `modal_rendering_is_unchanged` — every row of the 44×16 frame asserted | pass |
| Bespoke targets still refuse | `the_three_unreachable_info_types_are_still_unreachable` | pass |
| No new command, key, gesture, panel, read or daemon call | `git diff` — no new `KeyCode` arm, no `InputAction`, no `TuiMsg`, no `SidePanelId`, no `DialogType` | pass |
| `SidePanelId`, `config_key`, bindings, placement, sizing unchanged | Untouched; the seam counts are the only measured change | pass |
| Degenerate column does not panic | `a_hosted_info_dialog_at_a_degenerate_size_paints_no_second_box` over 2×2, 3×3, 4×2, 2×4 | pass |
| M001 and file-picker M001 suites unaffected | `tui_panel_docking` 52, `tui_panel_manager_reachability` 5, `tui_side_panels` 31 — all pass unmodified | pass |

## 3. Production implementation evidence

`src/tui/components/dialogs/info.rs` is the only production file changed. No
other file needed editing — see §5.2.

### `Chrome` — the flag, and one routine

```rust
enum Chrome { Owned, Hosted }
```

`render_body(&mut self, frame, area, theme, chrome)` owns the wrapping, the
`visible_lines` computation, the clamp and the content render. `render` calls it
with `Chrome::Owned`; `render_in_panel` calls it with `Chrome::Hosted`. The plan
mandated this structurally and it is the reason the milestone is safe: see §5.1.

In `Hosted` mode the routine insets `area` by the host's frame
(`area.inner(Margin::new(1, 1))`), returns without painting if the interior is
empty, and then reserves **one** row at the bottom for the scroll indicator. The
row is reserved unconditionally whenever `inner.height >= 2`, never
conditionally on whether there is something to report — otherwise
`visible_lines` would depend on the indicator's own output.

### `text_area` — the single source of the viewport

```rust
let text_area = match chrome {
    Chrome::Owned => Rect { x: chunks[1].x + 1, y: chunks[1].y + 1,
                            width: chunks[1].width - 2, height: chunks[1].height - 2 },
    Chrome::Hosted => body,
};
let visible_lines = text_area.height as usize;
let inner_width = text_area.width;
```

Per-dimension `saturating_sub`, **not** `Rect::inner`: ratatui's `Rect::inner`
returns `Rect::ZERO` if *either* axis is too small, which would have zeroed
`visible_lines` for a content chunk that is tall but only one column wide. The
per-dimension form matches the original expression exactly, so the `Owned` path
is byte-identical.

### The scroll indicator — kept, not dropped

Recorded here because the plan required an explicit decision. It stays, on the
reserved row, rendered only when there is something to report.

The alternative was to drop it. It was kept because `Down`/`j` is **deliberately
unbounded** and `scroll_to_end` stores a sentinel, so a reader scrolled into the
middle of wrapped content otherwise has nothing at all telling them they are not
seeing the end. `Down`/`j` scrolls forever and silently clamps; the indicator is
the only signal that it did. `docs/tui.md` now documents the line.

The modal footer is unchanged and keeps its verb text.

### `keymap_hint` — derived, not transcribed

```rust
fn keymap_hint(&self) -> Option<String> {
    Some(match self.info_type {
        InfoType::ProjectInit => format!("{SCROLL_VERBS} · {PUBLISH_VERB}"),  // "j/k scroll · a publish"
        _ => SCROLL_VERBS.to_string(),                                        // "j/k scroll"
    })
}
```

Per `InfoType`, never per `DialogType`: `ProjectInit` and `Team` share
`DialogType::Team` and therefore one panel, and only the first arms `a`.

## 4. Verification executed

**Baseline proof, recorded rather than asserted.** The production change was
stashed (`git stash push -- src/tui/components/dialogs/info.rs`, tests kept) and
the new suite run against `f8286a284`'s production code:

```text
test the_three_unreachable_info_types_are_still_unreachable ... ok
test the_hint_does_not_describe_a_caller_supplied_modal_footer ... FAILED
test project_init_and_team_hints_differ_despite_sharing_a_dialog_type ... FAILED
test modal_rendering_is_unchanged ... ok
test a_hosted_info_dialog_leaves_the_hosts_border_title_and_focus_dot_alone ... FAILED
test every_reachable_hosted_info_type_leaves_the_host_alone ... FAILED
test a_second_hosted_info_type_also_leaves_the_host_alone ... FAILED
test a_hosted_info_dialog_can_scroll_to_the_last_wrapped_line ... ok
test a_hosted_info_dialog_at_a_degenerate_size_paints_no_second_box ... ok
test a_hosted_info_dialog_reaches_the_same_end_position_as_a_modal_one ... ok
test no_undispatched_or_undeliverable_verb_is_left_out_by_accident ... FAILED
test project_init_advertises_publish_and_no_other_content_type_does ... FAILED
test a_hosted_info_dialog_advertises_only_verbs_it_dispatches ... FAILED
test result: FAILED. 5 passed; 8 failed; 0 ignored; 0 measured; 0 filtered out
```

Captured failure text is the defect verbatim — `"hosted ShellShow: host side
border broken at row 2 — \"└──────…────┘\""` (the widget's own title box, where
the host's border used to be) and `"Context does not advertise the scroll keys
it dispatches: \"\""`.

**Five of the thirteen passed at baseline and are recorded as guards, not as
signals.** Four of them are guards by nature:

- `modal_rendering_is_unchanged` is a regression net for W2's extraction and must
  be green *before and after*; a baseline failure would have meant the modal
  render had already moved.
- `a_hosted_info_dialog_can_scroll_to_the_last_wrapped_line` and
  `..._reaches_the_same_end_position_as_a_modal_one` could not fail at baseline,
  because before W2 `render_in_panel` *was* `render` and the modal clamp was
  already correct. They are the §3.3 hazard guards: they pass now and would fail
  if a future change ever computes the hosted clamp from a different rect. The
  plan anticipated this — "if it needed a manual nudge, the extraction is wrong".
- `the_three_unreachable_info_types_are_still_unreachable` is an invariant.

**Runs after the change (all local):**

```text
cargo test --test tui_info_dialog_panel_chrome          13 passed
cargo test -p codegg --lib dialogs::info                 8 passed
cargo test --test tui_panel_docking                     52 passed
cargo test --test tui_panel_manager_reachability          5 passed
cargo test --test tui_side_panels                       31 passed
cargo fmt --check                                        clean
cargo clippy --workspace --all-targets --locked -- -D warnings   clean
scripts/verify.sh quick                          exit 0 — "Quick verification passed."
scripts/verify.sh full                                  see §4.1
```

### 4.1 `verify.sh full` — run, not green

**`verify.sh full` does not pass in this environment, and did not before this
milestone.** Reported as run, with the failures attributed.

- `cargo fmt --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — **passed**.
- `cargo nextest run --workspace --locked --profile ci` — exits 100 at
  `core::daemon::tests::session_create_through_daemon` with
  `InvalidRootPath("/tmp")` (this machine's daemon refuses `/tmp` as a workspace
  root), and fail-fast leaves 13088 tests unrun. That is **the same test, with the
  same message, that M001's closure §4.1 recorded as environmental**, and the
  sweep stops at the same point (1110 of 14198).

Because fail-fast hides everything after the first failure, the suite was
re-run with `--no-fail-fast` for real coverage; the result is in §4.2.

### 4.2 Full sweep with `--no-fail-fast`

See §9 for the attributed result and the comparison against M001's recorded
baseline. **No panel, docking, placeholder, `InfoDialog`, chat-surface,
placement or reachability test fails.**

## 5. Findings that changed the design

### 5.1 `Esc` never reaches a hosted widget, and `Enter` closes nothing

This is the plan's §4 first invariant, and it resolved against advertising
either key. Both were traced in production code, not inferred:

- **`Esc` is intercepted by the host.** `SidePanelHost::handle_key` treats
  `is_exit_key(key)` — unmodified `Esc` or `q` — as the universal panel exit. It
  asks the panel whether it claims the key via `SidePanel::owns_exit_key`, and
  `HostPanel` does not override it, so the default `false` stands. The host
  therefore calls `self.blur()` and returns `PanelKey::Consumed`.
  `InfoDialog::handle_key`'s `Esc` arm is **unreachable** while the box is
  hosted and focused.
- **`Enter` reaches the widget and does nothing.** Docking is a move, not a copy:
  `dock_open_box` calls `app.close_dialog()` and hands the returned component to
  `adopt`, so the box is **popped off the modal stack**. `Enter` then yields
  `TuiMsg::CloseDialog`, `process_msg` runs `close_dialog()`, and
  `focus_manager.active_dialog_type()` is `DialogType::None` — a pop against an
  empty stack.

So the real hosted verb set is `j/k` plus `a` for `ProjectInit`. `Esc` still has
an effect — it returns the keyboard to the composer — but that is the *host's*
universal exit, not this widget's verb, and describing it as "close" would be
false. `Shift+←` returns the box to a modal where `Esc` closes it, and
`composer_hints` already prints that gesture.

`docs/tui.md` now tells the user this plainly.

**This is a real usability gap, and this milestone did not fix it.** A docked
`InfoDialog` has no in-panel way to be dismissed. The plan's §14 names this as a
stop condition on the grounds that resolving it is a host-level design question,
and §4 directs that the hint name the real verbs minus the intercepted ones.
Both were followed: the hint is honest, and the gap is recorded here rather than
absorbed silently. It is not a regression — before this milestone the box
overwrote its own chrome and advertised `i act`, and neither was better.

### 5.2 `custom_footer` is deliberately **not** reused in the hint

The plan said to prefer `set_custom_footer` in `keymap_hint` "if it can be read",
and to normalise the four call sites otherwise. Reusing it verbatim was rejected
on evidence: every one of the two *reachable* footers is a **modal** footer, and
each names `Esc`:

- `/project-init` → `"a: publish   Esc: cancel"`. `Esc: cancel` is false hosted
  (§5.1).
- `/shell-show` → `"i include  |  a ask  |  r rerun  |  k kill  |  e expand  |
  j/k scroll  |  Esc close"`. `Esc close` is false hosted, and `k kill`
  **contradicts** `handle_key`, where `k` scrolls up.

Replaying either would re-advertise exactly the dead `Esc` the milestone exists
to stop describing — the defect, reproduced through a different door. So the hint
is derived from `handle_key`, and **none of the four call sites needed editing**:
the modal footer is the modal footer, and it is untouched. The other two sites
(`interactive_terminal.rs`) are `InfoType::TerminalShow`, which resolves to a
bespoke panel and is never hosted at all.

`the_hint_does_not_describe_a_caller_supplied_modal_footer` pins this: with
`"a: publish   Esc: cancel"` set, the hint keeps `a publish` and drops `cancel`.

### 5.3 The scroll sentinel is not written back, and that is correct

The plan assumed the render pass clamps the `scroll` **field**. It does not: the
clamp is applied to a local `start_idx` and `self.scroll` is left holding
`usize::MAX`. That is required — `set_styled_content_preserving_scroll` restores
the stored offset on every refresh, so consuming the sentinel there would lose
the reader's position on the next update.

The first version of the in-module test asserted the opposite and failed. The
test now asserts both halves, which is the stronger pair: the rendered body
reaches the last wrapped row **and** the sentinel survives the pass. The doc
comment on `render_body` was corrected to match.

## 6. Reachable-hosted subset — 14 of 17, enumerated

The plan required W1 to enumerate rather than assume. Derivation, from live code:
an `InfoType` is reachable-hosted when its `DialogType` maps through
`App::dock_target` to a non-bespoke `SidePanelId` that is not
`discards_a_docked_box`.

| Excluded `InfoType` | Resolves to | Why it is not hosted |
|---|---|---|
| `TerminalShow` | `DialogType::Terminal` | `SidePanelId::Terminal.is_bespoke()` — owns its own `SidePanel` impl, never renders through `render_hosted` |
| `Agents` | `DialogType::Agent` | bespoke **and** `discards_a_docked_box()` — the panel shows the management rows list, not the editor box |
| `WorktreeList` | `DialogType::WorktreeList` | absent from `dock_target`, so `dock_target()` returns `None` and the gesture refuses |

The remaining **fourteen** are covered by
`every_reachable_hosted_info_type_leaves_the_host_alone`, which asserts chrome
survival and no-second-box for each. `ProjectInit` appears there against
`SidePanelId::Team`, which is the shared-`DialogType` case from §3.

So "retires 16 clobber sites" from the roadmap was an upper bound; **fourteen
panels** actually change.

## 7. Deviations from the plan

1. **The scroll indicator was kept, not dropped.** Recorded in §3 as the plan
   requires. The plan left it open; the reason for keeping it is that unbounded
   `Down`/`j` makes it the only end-of-content signal.
2. **One status row is reserved unconditionally** (when `inner.height >= 2`)
   rather than only when the indicator has something to say. Deciding it from
   the indicator would make `visible_lines` depend on its own output — a
   one-pixel feedback loop in the clamp.
3. **No production file outside `info.rs` was edited.** The plan's §5 listed
   three `set_custom_footer` sites under "in scope"; §5.2 shows none needed
   changing, because the modal footer is a modal-path affordance and the hint is
   a hosted-path one.
4. **No static guard was added**, exactly as §5 directed. The substitute is
   behavioural: every chrome claim is a rendered-buffer assertion.
5. **`Rect::inner` was rejected for the owned-mode inset** in favour of
   per-dimension `saturating_sub`, for the narrow-column reason in §3. The
   `Hosted` path still uses `area.inner(Margin::new(1, 1))`, matching
   `FilesDialog::render_in_panel` and `Placeholder::render_in_panel` verbatim.

## 8. Registry audit

**M002b becomes newly eligible on the plan's stated terms** — its only blocker
was M002a closing, and M002a is closed.

Its own stated caveats carry over unchanged: the same viewport hazard repeats per
widget and the plan already mandates one body routine each; `review` and
`research` dispatch on `self.mode`, so their `keymap_hint` must vary by mode; and
**M003 must still not land inside M002b** — the `Color::White` literals in
`plan`/`mcp` and the five `set_theme()` caches stay untouched, which is why the
body-routine extraction matters mechanically.

M003 remains **not approved**. Its theme-cache and colour-literal counts are
still baseline-measured rather than re-derived, and the roadmap says so.

Nothing else was unblocked.

## 9. Full-sweep attribution

```text
cargo nextest run --workspace --locked --profile ci --no-fail-fast
Summary [345.016s] 14198 tests run: 14187 passed (1 leaky), 11 failed, 6 skipped
```

The **same 11** failures M001's closure §4.1 recorded as pre-existing, named
identically:

| Test | M001 baseline | With M002a |
|---|---|---|
| `core::daemon::tests::session_create_through_daemon` | FAIL | FAIL — `InvalidRootPath("/tmp")`, environmental |
| `core::daemon::tests::turn_submit_rejects_when_active` | FAIL | FAIL |
| `tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input` | FAIL | FAIL |
| `bash_background_m002_routes::background_shell_returns_handle_promptly_then_notifies_on_cancel` | FAIL | FAIL |
| `command_routing_execution_ownership` × 4 | FAIL | FAIL |
| `control_m004_controller_lease` × 2 | FAIL | FAIL |
| `workspace_isolation::two_workspaces_register_and_resolve_independently` | FAIL | FAIL |

Twelve of the thirteen is a difference worth naming: M001's run also recorded
`agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
**timing out** under 4-way sweep load while passing in isolation. It did not
time out in this run. That is the known load-sensitive test, and its absence
here is variance, not a fix — it was never caused by this change.

**Nothing in the failing set touches the changed code.** No panel, docking,
placeholder, `InfoDialog`, keymap-hint, chat-surface, placement or reachability
test fails. All 13 new integration tests and all 8 new unit tests pass inside
this sweep.

## 10. Files changed

Production:

```text
src/tui/components/dialogs/info.rs     Chrome, render_body, render_in_panel,
                                       keymap_hint, SCROLL_VERBS/PUBLISH_VERB,
                                       6 new unit tests
```

Tests:

```text
tests/tui_info_dialog_panel_chrome.rs  new, 13 tests
```

Documentation:

```text
docs/tui.md                            "Where the keys are"; the no-Esc note
architecture/tui.md                    keymap_hint / render_in_panel sections,
                                       the Dialog::Info bullet
.opencode/skills/tui/SKILL.md          panel authoring guidance
plans/subsystems/…-addendum.md          §4.1 seam counts, §13 status
plans/registry.md                      M002a row
```

No storage, protocol, DTO, config, identity or authorization surface was touched.