# TUI Panel Surface Convergence Corrective Milestone M001 — Closure Status

Status: closed

The milestone is implemented and verified. The blocking condition recorded in
the plan's §2 — `file-picker-rework` M001 uncommitted and unclosed — has been
resolved for this work: its closure record exists, its seam signatures were
re-inspected and match what this milestone assumed, and the work was performed
in a **separate worktree** so the two could not collide in one dirty tree. The
one residual condition is that neither milestone is committed yet; see §6.

Source implementation plan:

- `plans/implementation/tui-panel-surface-convergence-corrective/001-panel-manager-outcome-contract-and-registry-reachability.md`

Source subsystem roadmap:

- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md`

Repository baseline reviewed: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`
plus the uncommitted `file-picker-rework` M001 working-tree changes.

Implementation commits or pull requests:

- *uncommitted* — working-tree diff, on branch
  `fork/446e57902e2e-0ae9640ab4bb432f9fa08368b8ae2faf`
  (worktree `.worktrees/fork-446e57902e2e-0ae9640ab4bb432f9fa08368b8ae2faf`).
  Commit/push were not requested.

## 1. Executive finding

Selecting a panel in `/panels` is now one honest act. Every arm of
`open_panel_box` declares what it did as a typed [`PanelFill`], and the panel
column says the same thing in words: it fills, a load started, or it cannot act
and why. The instructions are gone — no panel, in any state, tells the user to
run a `/` command or to press `Shift+←`, because the panel is already in place
and there is nothing to move into it.

Three defects named in the plan are fixed: the placeholder copy, the two literal
panel-id branches at the call site, and the silent box drop in `dock_open_box`.
`SidePanelId::Workspace` is removed from the registry, so the manager no longer
lists a row that could never hold anything.

**One plan defect was found and had to be resolved rather than implemented
literally** — the adoption-failure pre-check. §7.2.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Selecting a panel performs exactly one action | `PanelFill` returned by every arm of `open_panel_box`; `Filled` / `Loading` / `Cannot` each record a `PlaceholderState` or adopt a box | pass | The `Some(other)` inference and the `dock_target()` re-read are gone |
| No panel displays a `/`-command or `Shift+←` instruction | `committing_a_panel_never_asks_the_user_to_go_and_come_back` sweeps all 32; `a_hosted_placeholder_never_asks_the_user_to_go_and_come_back` covers 4 panels × 3 states | pass | Baseline-failing: 3 of 4 integration tests failed at `b77c8156` (§4) |
| No panel claims "No X loaded" after a commit | `committing_a_panel_never_leaves_it_claiming_nothing_is_loaded` sweeps all 32 | pass | |
| `Goal`/`Doctor`/`Plugins` never read as empty while loading | `PanelFill::Loading` sets `PlaceholderState::Loading`; `a_panel_whose_load_is_in_flight_does_not_read_as_an_empty_panel` | pass | Baseline-failing |
| Every hosted placeholder keeps the host's chrome | `a_hosted_placeholder_keeps_the_panel_title_and_focus_dot` — asserts `● Files` survives `render_hosted`, across all three states | pass | Baseline-failing; was a `Block` painted over the host's |
| Every panel in `SidePanelId::ALL` is reachable or no longer listed | `committing_a_panel_never_leaves_it_in_the_default_empty_state` sweeps all 32; `Workspace` removed | pass | See §5 — this one is a forward-looking guard, not baseline-failing |
| No adoption failure is silent | `dock_open_box` pre-checks `SidePanelId::discards_a_docked_box()` **before** unmounting; `a_box_with_no_panel_to_receive_it_is_refused_before_it_is_unmounted` | pass | §7.2 |
| No new command, key, or gesture; none changes meaning | `git diff` — no new `KeyCode` arm, no new `InputAction`, no new `TuiMsg` | pass | |
| Panel variants, config spellings, placement, sizing unchanged | Every remaining `SidePanelId` keeps its `config_key` verbatim; `PanelPlacement` untouched; `STORAGE_LAYOUT_VERSION` untouched | pass | Only `Workspace` removed, by decision |
| No panel gains a data read, daemon call, or privilege | The only new reads are ones `open_panel_box` already performed; `handle_diff_command` and `start_tasks_command` changed **signature only** | pass | |
| `/agents` still routes through one `P::Agents` arm | The arm's work is unchanged and still lives in `open_panel_box` | pass | It returns `PanelFill::Bespoke` — it selects and focuses, it does not adopt |
| `Shift+S` + `Enter` remains one step | `materialize_hosted_panel` still selects on every outcome that is not a live box | pass | |
| `tests/tui_panel_docking.rs` passes unchanged | 52/52 | pass | Only a stale doc comment edited; no assertion touched |
| Bespoke panel content and keymaps unchanged | `tui_chat_surfaces` 31/31, `tui_panel_placement` 30/30, `tui_agents_panel` 3/3 | pass | §7.2 — a first attempt did change them |

## 3. Production implementation evidence

Files owned by this milestone (the working tree additionally carries the
uncommitted `file-picker-rework` M001 change, which is **not** part of this
milestone — see §6):

- `src/tui/commands/panels.rs` — `PanelFill` consumption, per-arm outcomes,
  adoption pre-check.
- `src/tui/panels/hosted.rs` — `PlaceholderState`, `Placeholder::render_in_panel`,
  state-driven copy.
- `src/tui/panels/mod.rs` — `PanelFill` (registry vocabulary, next to
  `SidePanelId`), `SidePanelHost::set_placeholder_state`,
  `SidePanelId::discards_a_docked_box`, `Workspace` removal,
  `SidePanelId::load_command` removal.
- `src/tui/commands/work_orders.rs` — `start_tasks_command` returns its outcome.
- `src/tui/app/mod.rs` — `handle_diff_command` returns whether it opened a box;
  the dead `WorkspaceDashboard` arm is gone from `dock_target`.
- `tests/tui_panel_manager_reachability.rs` — new, 275 lines, 5 tests.

### `PanelFill` — the contract, in the registry

Six variants, each produced by at least one real arm: `Filled`, `Loading`,
`StillLoading`, `OpenedAs(SidePanelId)`, `Cannot(&'static str)`, `Bespoke`.
It lives in `panels/mod.rs` because `work_orders.rs` returns it too and a module
cycle is not worth the tidiness.

The plan's §6 sketch had three variants. Two more were needed, and the reason is
the `Research`/`TaskView` protection: those two mount a box *and* start a load
behind it, so the box is live but incomplete, whereas `Goal`/`Doctor`/`Plugins`
mount nothing at all. Collapsing them would either adopt a spinner or leave a
panel selected behind a box that does not exist. `StillLoading` **is** the old
literal list, moved next to the work that needs the protection. `Bespoke` exists
because "nothing to do" and "could not act" are different answers and `Cannot`
would have claimed the second.

### `Placeholder` — state, never instructions

`PlaceholderState` is `Idle`, `Loading`, or `Cannot(&'static str)`. Copy:
`"Nothing loaded." / "Open it from the panel manager (⇧S)."`,
`"Loading <Label>… / It opens here when it arrives."`, and
`"<Label> can't open here." / <reason>`. `render_in_panel` renders the body into
`area.inner(Margin::new(1,1))` with no `Block`, so the host's border, title and
`●` survive. `render` keeps the bordered form for the standalone path only.

`SidePanelId::load_command` was **deleted**: its sole production consumer was
the copy that named it. Leaving a registry method whose documented purpose was
the defect just removed would be exactly the drift this module is written
against. Its registry test was rewritten to pin `config_key`/`label`/`hint`.

### Arms that pre-check a helper

`Tasks`, `Team`, `Collaborators` and `Control` delegate to helpers whose outcome
depends on runtime state, so the arm states which condition it is mirroring —
mirroring the helper's own first branch rather than re-deriving intent later.
`Tasks` returns from `start_tasks_command` itself, because its three-way branch
(`start_list_tasks` async / `open_task_view` box / no project) cannot be
reproduced at the call site without duplicating the branch.

## 4. Verification executed

**Baseline proof.** The new suite was run against true baseline `b77c8156`
(`git stash push -- src/`, which also reverted the inherited file-picker work,
then restored):

```text
running 4 tests   # the five-test suite minus the adoption test added later
test committing_a_panel_never_asks_the_user_to_go_and_come_back ... FAILED
test committing_a_panel_never_leaves_it_claiming_nothing_is_loaded ... FAILED
test a_panel_whose_load_is_in_flight_does_not_read_as_an_empty_panel ... FAILED
test committing_a_panel_never_leaves_it_in_the_default_empty_state ... ok
```

The captured failure text is the defect verbatim: `"then Shift+← to move it
here."`. The fourth passes only because `"Nothing loaded."` did not exist at
baseline — it is a **forward-looking guard**, not a baseline-failing one, and is
recorded as such rather than claimed as a signal.

The three `hosted.rs` unit tests could not be run at baseline: they reference
`PlaceholderState`, which W2 introduces. Their baseline failure is structural,
not observed. Only the three integration tests above are claimed as observed.

**Two harness defects were found by making these tests and had to be fixed before
they meant anything.** Both are recorded because the failure mode — a passing
assertion that checks nothing — is invisible in a green run:

1. `panel_column` first read `app.sidebar_area`, which is `None` unless the
   occupied edge happens to be showing `Session`. Three of the four tests
   "passed" against an empty string. It now reads `panel_areas`, and
   `committed_column` asserts the column is non-empty before anything else.
2. The toast overlay paints a fixed 60×10 block at `x = width - 60`, which lands
   on the default right column and hid the text under test. The panel is now
   parked on the **left** edge, clear of it, and `panel_column` asserts the
   column and the toast block do not intersect.

**Runs after the change (all local):**

```text
cargo test -p codegg --lib tui::panels                     40 passed
cargo test --test tui_panel_manager_reachability             5 passed
cargo test --test tui_panel_docking                         52 passed
cargo test --test tui_chat_surfaces                         31 passed
cargo test --test tui_panel_placement                       30 passed
cargo test --test tui_agents_panel                           3 passed
cargo test --test tui_side_panels                           29 passed
cargo fmt --check                                            clean
scripts/verify.sh quick                            exit 0 — "Quick verification passed"
scripts/verify.sh full                           exit 100 — pre-existing failures only, §4.1
cargo nextest run --workspace --locked --profile ci --no-fail-fast
                                          14175 run, 14163 passed, 11 pre-existing fails
```

### 4.1 `verify.sh full`

**`verify.sh full` does not pass in this environment, and did not before this
milestone either.** Reported as run, not as green.

- `cargo fmt --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — **passed**.
- `cargo nextest run --workspace --locked --profile ci` — exits 100 at
  `core::daemon::tests::session_create_through_daemon`
  (`InvalidRootPath("/tmp")`; this machine's daemon refuses `/tmp` as a
  workspace root), and fail-fast leaves 13065 tests unrun.

Because fail-fast hides everything after the first failure, the whole suite was
re-run with `--no-fail-fast` to get real coverage:

```text
Summary [385.721s] 14175 tests run: 14163 passed (1 leaky), 11 failed, 1 timed out, 6 skipped
```

**All 11 failures are pre-existing.** The 12 non-passing tests were re-run with
`git stash push -- src/` — reverting `src/` to `b77c8156`, which also reverts
the inherited file-picker change — and 11 fail **identically at baseline**:

| Test | Baseline | With this change |
|---|---|---|
| `core::daemon::tests::session_create_through_daemon` | FAIL | FAIL — `InvalidRootPath("/tmp")`, environmental |
| `core::daemon::tests::turn_submit_rejects_when_active` | FAIL | FAIL |
| `tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input` | FAIL | FAIL |
| `bash_background_m002_routes::background_shell_returns_handle_promptly_then_notifies_on_cancel` | FAIL | FAIL |
| `command_routing_execution_ownership` × 4 | FAIL | FAIL |
| `control_m004_controller_lease` × 2 | FAIL | FAIL |
| `workspace_isolation::two_workspaces_register_and_resolve_independently` | FAIL | FAIL |
| `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | **PASS in isolation** | TIMEOUT at 120s under 4-way sweep load |

The last one is the known load-sensitive test the `file-picker-rework` closure
already recorded: it passes in isolation and times out only under sweep load,
before and after this change alike.

**No panel, docking, placeholder, chat-surface, placement, agents, diff, or
work-order test fails.** Nothing in the failing set touches the changed code.
Three of these (`session_create_through_daemon`,
`same_scope_requests_coalesce_to_one_publication`,
`pressing_i_on_the_open_box_enters_focused_input`) were already documented as
pre-existing by the sibling closure; the other eight are **newly observed, not
newly caused** — the earlier run stopped at 1110 tests and never reached them.

## 5. Deviations from the plan, and why

1. **`PanelFill` has six variants, not the plan's three.** See §3. Collapsing
   them would either adopt a spinner onto `Research`/`TaskView` or leave a panel
   selected behind a box that was never mounted.
2. **The "hosted widget must not paint a `Block`" static guard was not written.**
   The plan flagged this itself, and the flag is right: `Component::render` is
   the *modal* path, where a border is correct and every dockable dialog draws
   one. A static check cannot tell the two apart and would false-positive on
   every dialog in the tree. The honest version is each box overriding
   `render_in_panel`, which is M002's scope; M001 does the placeholder and adds
   the regression test that pins it.
3. **The "a listed panel cannot be unconstructible" guard is a test, not a
   static check.** Constructibility is a runtime property — `Workspace` was
   listed for the whole life of the registry and only a commit that walks
   `SidePanelId::ALL` through the real gesture can catch the next one. It is
   `committing_a_panel_never_leaves_it_in_the_default_empty_state`.
4. **`SidePanelId::load_command` was deleted**, not left as dead API. §3.
5. **Two helpers changed signature**: `handle_diff_command` returns `bool`,
   `start_tasks_command` returns `PanelFill`. Both are `pub(crate)` with two
   call sites each, and both were re-deriving truth by inference. No caller
   outside `open_panel_box` needed the value.

## 6. Residual conditions

- **Neither milestone is committed.** `file-picker-rework` M001 was already
  recorded as *conditionally closed* for exactly this reason, and this milestone
  is in the same state. The §2 unblock condition is satisfied in substance
  (closure record exists, seam signatures verified as landed: `keymap_hint() ->
  Option<String>` and `render_in_panel(&mut Frame, Rect, &Arc<Theme>)`, both
  defaulted) but not in the letter the plan asked for. **M002 should still
  re-inspect both boundary files at the eventual commit before starting.**
- **The `Workspace` persisted hotkey is now unbound, silently.** A user who had
  bound `Ctrl+digit` to `workspace` loses that binding with no error. This is
  the fail-soft path the plan's §9 required and it was verified before the
  change (`panel_from_key` returns `None` for an unknown key; `from_map` skips
  it), so it is a known, accepted cost of removing the row rather than an
  oversight. It is **not** a deprecation path.
- **`choose_by_label` in `tests/tui_panel_docking.rs` is subtly wrong** and was
  left alone: `PanelListState::move_cursor` clamps, it does not wrap, so the
  helper's forward-distance arithmetic is only correct while the active panel is
  index 0 — which it always is in those tests. The new suite does not use it and
  clamps before walking. Out of scope here; worth a follow-up.

## 7. Defects found while implementing

### 7.1 The plan's defect list was right, and wider than stated

`Goal`, `Doctor` and `Plugins` were named as the self-contradicting panels. The
contradiction also covered `Memory`, `Plan`, `Security`, `ShellShow` and
`Diff`, all of which reach the placeholder on ordinary paths. The sweep does not
enumerate them, which is the point: it walks the registry.

### 7.2 The adoption pre-check could not be `is_bespoke`

The plan's §3.6 named `DialogType::Agent → SidePanelId::Agents` **and**
`DialogType::ProjectChat → SidePanelId::Chat` as the two bespoke targets that
silently discard a box. The second is wrong, and implementing it literally broke
five existing tests: `Help`, `Terminal` and `Chat` are bespoke, but each builds
or shares the very content its box was showing, so docking them changes the
frame and loses nothing — `tui_chat_surfaces.rs::docking_the_chat_window_moves_it_into_the_side_panel`
pins that for `Chat` explicitly.

The real predicate is narrower and is now `SidePanelId::discards_a_docked_box()`:
`Agents` alone, because the panel is the management **rows** list while
`Dialog::Agent` is the agent **editor**. `is_bespoke` answers "does this panel
build its own surface", which is a different question; conflating them would
have made a legitimate gesture refuse.

This is recorded rather than quietly corrected because the plan's stop condition
("any change would alter bespoke panel content or keymaps") is what caught it.

## 8. Registry audit

M002's own registry entry said it requires M001 **and** `file-picker-rework` M001
closed. M001 is now closed and `file-picker-rework` M001 has a closure record,
so **M002 becomes newly eligible on the plan's stated terms.** Its second
dependency — the `render_in_panel` default-flip research, needing a whole-tree
sweep over `Component` implementors before adoption — is untouched by this
milestone and remains its own work.

M002 stays **not approved**. This milestone unblocked it; it did not open it.

## 9. Files changed

Production (this milestone only):

```text
src/tui/app/mod.rs                    dead dock_target arm; handle_diff_command -> bool
src/tui/commands/panels.rs            PanelFill wiring, per-arm outcomes, pre-check
src/tui/commands/work_orders.rs       start_tasks_command -> PanelFill
src/tui/panels/hosted.rs              PlaceholderState, render_in_panel, copy
src/tui/panels/mod.rs                 PanelFill, set_placeholder_state,
                                      discards_a_docked_box, Workspace + load_command
                                      removed
```

Tests:

```text
tests/tui_panel_manager_reachability.rs   new (275 lines, 5 tests)
tests/tui_panel_docking.rs                one stale doc comment
src/tui/panels/hosted.rs                  3 unit tests
src/tui/panels/mod.rs                     registry test rewritten
```

Docs:

```text
docs/tui.md                    "What Enter does" section
architecture/tui.md            placeholder, PanelFill, panel counts, picker table
.opencode/skills/tui/SKILL.md  panel authoring + the inference that replaced PanelFill
```