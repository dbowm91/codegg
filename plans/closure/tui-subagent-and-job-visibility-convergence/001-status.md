# TUI Subagent and Job Visibility Convergence — Milestone 001 — Closure Status

Status: **closed**

Source implementation plan:

- `plans/implementation/tui-subagent-and-job-visibility-convergence/001-ctrl-t-consolidation-and-reachability.md`

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`

Repository baseline: `2acce2138`

Implementation: **working tree, uncommitted** at the time of this record. No
commit hash is claimed because none exists; this record describes the diff, not
a commit.

## 1. Executive finding

C001 is implemented and verified locally. `Ctrl+T` is now the single key that
answers "what is this session waiting on" — todos, subagents, and jobs together
— and it works from the mode the app boots in. The overlay opens navigably from
Insert mode and hands composition mode back when it closes.

The milestone did not go as planned in one respect, and the deviation is
recorded below as **D-C9**. Moving the chord from a Normal-only `Shift+T` to
`Ctrl+T` made the overlay reachable from Insert, but the overlay's own keys
(`j`/`k`/`Enter`/`Esc`) are Normal-mode gestures by contract. Shipping only the
rebind would have produced an overlay that opened and then silently ignored
every navigation key — a different instance of the same "the feature is dead"
defect this milestone exists to remove.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `Ctrl+T` opens the combined block | `ctrl_t_opens_the_tasks_overlay_in_both_modes` | pass | Asserts `ToggleTasksList` in **both** `InputMode::Insert` and `InputMode::Normal`. |
| `Shift+T` retired in both encodings | `capital_t_composes_text_in_insert_mode` | pass | `SHIFT` and `NONE` spellings both compose `Char('T')`. |
| `Ctrl+T` is the overlay's only spelling | `ctrl_t_is_the_only_spelling_of_the_tasks_overlay` | pass | Asserts the exact set of spellings in both keymaps, so a re-added capital fails. |
| No capital `T` regression in the composer | `capital_t_composes_text_in_insert_mode` | pass | This is exit condition 3 — the test that fails if `Shift+T` were kept *and* the mode gate relaxed. |
| D-C7 settled | `todo_strip_is_one_row_whatever_the_list_length` | pass | `todo_expanded` retired with `ToggleTodoList`, `toggle_todo_list`, `TuiMsg::ToggleTodoList`, and `MAX_TODO_STRIP_TASKS`, in one change. |
| Overlay navigable from Insert (D-C9) | `opening_the_tasks_overlay_from_insert_hands_composition_mode_back`, `opening_the_tasks_overlay_from_normal_does_not_move_modes` | pass | Opening from Insert enters Normal; closing restores Insert; a Normal round trip is a no-op. |
| I6 — every advertised chord resolves to what it advertises | `documented_keys_resolve_in_the_active_map` | pass | Strengthened to a label↔action comparison against `ActionKey::label`. |
| I6 guard is non-vacuous | `i6_advertisement_check_rejects_a_mislabelled_action` | pass | Asserts the comparator rejects `Toggle sidebar` for `ToggleTasksList` — the exact defect — and accepts the correct prose. |
| Orphan module purged | `git status`: `src/tui/components/help_overlay.rs` deleted | pass | Its unique Editor key table was moved to `docs/tui.md` rather than discarded. |
| Panel transport authority | `scripts/check_tui_transport_authority.py` | pass | Run by `scripts/verify.sh quick`. |
| Bounded suite | `scripts/verify.sh quick` | pass | Local run; see §6 for the truthfulness labels. |

## 3. WP-1 evidence: what the strengthened I6 guard found

The guard was strengthened **before** any help text was edited, and it found
**nine** mislabelled rows that the old guard passed because it only asserted a
key resolves to *something*:

| Key | Advertised | Actually performs |
|---|---|---|
| `Ctrl+T` (Normal) | `Toggle sidebar` | `ToggleTodoList` → now `ToggleTasksList` |
| `Space` (Normal) | `Focus sidebar (agent tree)` | `Help` |
| `Ctrl+L` | `Model selector` | `SelectModel` |
| `Shift+S` | `Open the side-panel manager: …` | `OpenPanelManager` (label `Open panel manager`) |
| `Shift+F` | `Open the project picker; …` | `OpenProjectPicker` (label `Open project picker`) |
| `Shift+X` | `Close the active subagent tab; …` | `CloseSubagentTab` (label `Close subagent tab`) |
| `Shift+L` / `Shift+Right` | `Dock the open box …` | `DirectionRight` (label `Pane right`) |
| `Shift+Left` / `Shift+H` | `Return the keyboard to the main buffer` | `DirectionLeft` (label `Pane left`) |
| `Shift+Up` / `Shift+Down` | `Cycle which panel is loaded …` | `DirectionDown` (label `Next panel`) |
| `Esc` (3 rows) | `Switch to normal mode` / `Clear draft …` / `Leave sidebar focus` | `Cancel` |
| `Ctrl+C` (2 rows) | `Quit` | `ForceQuit` |

The measurement was taken by a temporary probe that resolved all 149 advertised
rows through the real key resolver in both modes and both keymap
configurations; the probe was removed before the milestone closed.

**The comparator is not a second hand-kept table.** `advertises()` compares the
advertised prose against `ActionKey::label()` — the same canonical naming
authority that already produces `ActionDescriptor` — with any trailing key hint
stripped. Asserting against a second list would have relocated the drift rather
than removing it, which is the specific failure mode the plan warned about.

Two exemptions are explicit and structural, not per-entry allowlists: a
**conditional** row (`condition: Some(..)`) describes focus-dependent
behaviour and is exempt from the label comparison (it must still resolve), and a
key resolving to `InputAction::Char` is typing rather than an advertised action.

The guard **aggregates** every violation before asserting. Its first version
failed fast, which turned an eight-row correction into eight separate runs — the
same "stop being run" dynamic the aggregation now prevents.

## 4. Decisions taken during this milestone

- **D-C7 — `todo_expanded` retired.** The compact one-line strip above the input
  is unaffected (`todo_strip_height` still returns 1 whenever todos exist); only
  in-place expansion is gone, and the full list remains the overlay's first
  group. `ToggleTodoList`, `toggle_todo_list`, `TuiMsg::ToggleTodoList`,
  `InputAction::ToggleTodoList`, `ActionKey::ToggleTodoList`, and the now-dead
  `MAX_TODO_STRIP_TASKS` were removed **together** — a half-retired toggle is
  worse than either state. The configurable action name `toggle_tasks_list` is
  retained so existing keybind configs keep loading.
- **D-C8 — Editor route keeps its `Ctrl+T`.** The file-tree toggle is unchanged
  and is now documented as a conditional help row, alongside a note that
  `Ctrl+S` saves the buffer on that route rather than stashing the prompt.
- **D-C9 (new) — opening the overlay from Insert takes Normal mode with it.**
  `tasks_restore_mode` records the mode to hand back on close. The alternative —
  leaving navigation Normal-only — was rejected because it ships an overlay that
  looks navigable and is not.

## 5. User-visible behaviour changes

Stated plainly, because both are real:

1. **`Shift+T` no longer opens the overlay.** `Ctrl+T` does. A user who learned
   `Shift+T` in the last two days loses it; `Ctrl+T` is the replacement and works
   in both modes.
2. **`Ctrl+T` no longer expands the todo strip in place.** It opens the combined
   todos/subagents/jobs block. The one-line progress strip above the input is
   unchanged.

Neither is a configuration or data migration.

## 6. Verification commands and honest status

| Command | Status | Label |
|---|---|---|
| `scripts/verify.sh quick` | **pass** | Local run, this working tree. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **pass** | Local run. |
| `cargo test --lib tui::input` | pass | Local. |
| `cargo test --lib -p codegg-protocol projection` | pass (89) | Local. |
| `cargo test --lib subagent_row_chain` | pass (4) | Local. |
| `cargo fmt` | pass | Local. |
| `scripts/check_tui_transport_authority.py` | pass | Local. |
| `scripts/check_execution_ownership.py` | pass | Local. |
| `scripts/check_scheduler_bypass.py` | pass | Local. |
| `docs/execution-ownership.toml` | **unchanged** | `git diff` empty. |
| `cargo nextest run --workspace --locked --profile ci` | **1 pre-existing failure + 1 load timeout** | See below. |

**The full suite does not pass, and neither failure is mine — both were
checked rather than assumed:**

1. `core::daemon::tests::session_create_through_daemon` — **FAILED** with
   `InvalidRootPath("/tmp")`. Verified **pre-existing**: the C001 changes were
   stashed and the test run on the unmodified baseline, where it fails with the
   identical panic. This machine rejects `/tmp` as a workspace root.
2. `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
   — **TIMEOUT** at the suite's 120 s cap. Verified **not a regression**: run in
   isolation on this tree it **passes in 0.149 s**. It timed out only because
   another worktree's full suite was saturating the machine at the time.
3. `tui::async_cmd_tests::socket_daemon_mode_never_takes_the_remote_input_path`
   fails in the `cargo test --lib` path in this environment; also verified
   pre-existing by the same stash-and-run method.

None of the three touches input bindings, the tasks overlay, subagent rows,
`/agents`, or the retired picker.

**Not verified.** Runtime behaviour was never observed — no TUI session was
launched. Every claim above rests on static reading plus the test suite. The
Insert-mode defect this milestone fixes was **inferred** from the binding tables
and mode gates, not reproduced by hand first; it is confirmed only by the tests
that now pin the corrected behaviour.

## 7. One C003 consequence worth stating plainly

Pointing `/agents` at the agents dialog removed the **only** caller of
`open_panel_picker`, and that helper's only caller was bare `/agents`. Its
selection half was hard-wired to `SidePanelId::Agents`, so `PanelPickerDialog`
was a one-off shim for that route rather than a general placement tool.
Retiring it was not optional: `verify.sh full` runs clippy with `-D warnings`
and the orphaned helper fails it. `panel_picker.rs` (447 lines),
`Dialog::PanelPicker`, `DialogType::PanelPicker`, and
`pending_panel_picker_subject` are gone, along with two tests that pinned the
old routing and a chrome test that documented a seam the widget could never
reach. **Panel placement is not lost** — `/panels` still selects and focuses
any panel, and `the_panel_manager_still_loads_and_focuses_the_agents_panel`
now covers it where the picker used to.

## 8. Milestones beyond C001

This record covers **C001 only**. C003, C004, and C006 are implemented in the
same working tree and have **no closure record yet** — the registry still lists
them as `proposed`, which is the honest state until each has its own record.
C002 and C005 are **not implemented**. C007 remains blocked; its ADR is drafted
at `plans/adrs/ADR-0040-subagent-run-addressing-and-human-interaction-authority.md`
(status `proposed`) and no capability implying interaction was added.