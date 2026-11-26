# TUI Editor IDE Presence M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-ide-presence/004-debounced-insert-completion.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-4--debounced-insert-completion`

Dependency: M001 closed; M002 closed (soft).

## 1. Executive finding

M004 is complete. An Insert-mode edit that changes the document arms one idle
deadline; when it fires, the editor issues the existing completion read at the
cursor. Arrival never inserts: the popup opens inert and the candidate applies
only on the existing explicit key, through the existing revision witness. The
three immediate triggers (`F8`, Insert `Ctrl-x`, `:complete`) supersede the
timer and call the same function, so there is one request path and one accept
path.

Interval that shipped: **200 ms** (`EDITOR_COMPLETION_DEBOUNCE` in
`crates/codegg-tui/src/commands/editor.rs`). Not configurable, not shared with
the managed `didChange` debounce.

The closed M004 (`plans/implementation/tui-editor-correctness-usability/004-editor-completion-presentation-and-qualification.md`)
was not reopened: this milestone adds a request schedule, not an accept path,
a snippet path, or an automatic trigger that inserts.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| An edit burst produces one request | `insert_completion_debounce_coalesces_an_edit_burst_into_one_request` | pass |
| Arrival does not change document text | `idle_completion_arrival_opens_only_an_inert_suggestion` | pass |
| A stale/edited-away response is not actionable | `idle_completion_response_after_a_new_insert_edit_is_non_actionable` | pass |
| `F8` supersedes the timer and sends the explicit request | `f8_supersedes_a_pending_insert_completion_deadline` | pass |
| Insert `Ctrl-x` supersedes the timer and reuses the path | `insert_ctrl_x_supersedes_the_timer_and_reuses_completion` | pass |
| Leaving Insert cancels the schedule | `leaving_insert_cancels_the_pending_completion_request` | pass |
| A queued idle answer cannot survive a reconnect | `reconnect_epoch_fence_drops_a_queued_pre_reset_idle_answer` | pass |
| The deadline actually wakes the loop | `completion_debounce_arms_the_idle_timed_loop` | pass |
| No new chord, no `Ctrl-c` takeover | The four explicit-trigger tests above, plus `the_completion_trigger_fires_in_both_modes` | pass |

## 3. Production implementation evidence

`crates/codegg-tui/src/app/state/editor.rs` holds
`EditorCompletionDebounce { generation, local_revision, due_at }` with
`arm_completion_debounce`, `take_completion_debounce_due`,
`completion_debounce_is_armed`, and `cancel_completion_debounce`. A
generation bump (close, switch, reconnect) cancels it.

`crates/codegg-tui/src/commands/editor.rs`: `handle_editor_key` compares the
local revision before and after a key; only an Insert-mode key that changed it
arms the deadline. `service_editor_completion_debounce` drops a schedule whose
generation, editor view, mode, revision, or completion scope no longer matches,
and otherwise calls `start_editor_completion`, which itself cancels the
schedule first. No second request shape and no second accept path were added.

`crates/codegg-tui/src/runtime/event_loop.rs` keeps the loop awake while the
deadline is armed and services it on the wake: the timer does not need a
keypress to fire.

`docs/tui.md` and `architecture/tui.md` describe the schedule, the three
immediate triggers, the 200 ms interval, and the unchanged explicit accept.

## 4. Verification executed

All results are LOCAL to this worktree. No hosted CI run and no live TUI
receipt is claimed.

```bash
CARGO_BUILD_JOBS=2 cargo test -p codegg-tui --test tui_render -- tui_completion_trajectory::
# 32 passed; 0 failed (includes every M004 case listed above)

CARGO_BUILD_JOBS=2 cargo test -p codegg-tui --lib -- completion_debounce_arms_the_idle_timed_loop
# 1 passed

CARGO_BUILD_JOBS=2 cargo check -p codegg-tui --all-targets
# clean

python3 scripts/check_tui_editor_text_authority.py
# passed
```

`scripts/verify.sh scoped` is the roadmap-level closing sweep and is run once
for M001–M006; it is recorded at the roadmap closeout rather than claimed here.

## 5. Handoff

M004 does not unblock a registered successor. M005 was operationally gated on
M001, which is closed.
