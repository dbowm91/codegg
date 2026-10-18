# Milestone 004 — `/queue` Box: List, Select, Delete, and Status Indicator: Closure Record

Status: **closed**

Milestone: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Milestone 4)

Plan: `plans/implementation/user-prompt-queue/004-queue-box-list-select-delete-and-indicator.md`

Authority: ADR-0035, ADR-0036, ADR-0037, and ADR-0038 — **all accepted**
2026-10-18. M004 needs none of them to gate: ADR-0036 constrains copy and
belongs to M006/M007, and the rest are already discharged into M002a/M002b.

Closure record: `plans/closure/user-prompt-queue/004-status.md`

Baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the uncommitted M001,
M002a, M002b, and M003 closure trees. Nothing in this milestone was committed;
the whole working tree remains uncommitted.

## 1. Class and delivered capability

Primary class: **capability**.

M001 through M003 made the queue correct and made it deliver. Every one of them
was still invisible to a person: a prompt could be queued, owned, mutated,
delivered into a turn, and pushed back to the frontend, and nothing a user
could press would ever show it to them. **M004 is the milestone where the queue
becomes something you can look at.**

What is now reachable: `/queue` opens a box listing this session's waiting
prompts in the daemon's order; `j`/`k` (and the arrows) move the selection, `r`
re-reads, `d` deletes the selected entry, `Esc` closes. Row 0 of the working
strip names how many prompts are waiting. A `delivering` row is listed, is
marked as such, and refuses deletion **with a message that says it is being
delivered rather than expired**.

What is deliberately still absent: add, edit, reorder, send-now, and steering.
Those are M005, M006, and M007. The box names none of them — in its hint, in
its empty state, or anywhere on screen — and three separate tests enforce that
by scanning the rendered buffer.

## 2. What shipped

| File | Change |
|---|---|
| `src/tui/components/dialogs/queue.rs` | **New.** `QueueDialog`: list, selection, paging, truncation, status, empty state, keymap hint |
| `src/tui/commands/queue.rs` | **New.** `open_queue_box`, `refresh_queue_box`, `adopt_queue_projection`, `start_queue_delete`, `apply_queue_delete_finished` |
| `src/tui/app/types.rs` | `Dialog::Queue`; `TuiMsg::QueueDeleteSelected`, `TuiMsg::QueueRefreshRequested` |
| `src/tui/components/component.rs` | `DialogType::Queue`; both exhaustive `From` arms; the round-trip table row |
| `src/tui/components/dialogs/mod.rs` | `pub mod queue;` |
| `src/tui/command.rs` | `BuiltinSlashAction::Queue`; the `/queue` registration |
| `src/tui/app/mod.rs` | dispatch arm; `close_dialog` arm; the applier now syncs the mounted box |
| `src/tui/app/modal.rs` | an explicit no-op `Dialog::Queue` arm in `open_dialog` |
| `src/tui/app/input.rs` | both `TuiMsg` arms |
| `src/tui/app/commands.rs` | `TuiCommand::UserPromptQueueDeleteFinished` |
| `src/tui/runtime/command_dispatch.rs` | that command's dispatch arm |
| `src/tui/app/state/dialog.rs` | `DialogState::queue_delete_request` |
| `src/tui/app/render.rs` | the row 0 `queued` suffix |

Docs: `architecture/tui.md` (a new `/queue` section, a row 0 suffix note, and
the directory entry), `.opencode/skills/tui/SKILL.md` (a `M004 /queue` box`
subsection), `docs/tui.md` (the `/queue` table row and a user-facing
`## The prompt queue` section).

Tests: `tests/user_prompt_queue_box.rs` (**26**, new) and 22 new unit tests
across `dialogs::queue` and `commands::queue`.

**No migration.** `STORAGE_LAYOUT_VERSION` and `run_at_ms` are untouched;
`run_at_ms` keeps v82 for M007. Nothing in `codegg-core`, the protocol crate,
the daemon, or the store changed — this milestone is entirely `src/tui` plus
tests and docs.

## 3. Three seams that had to be got right

### The box is a `Component`, not a `Dialog` key arm

`handle_dialog_key_legacy` is `#[cfg(any())]`-disabled and `FocusManager` is the
sole key authority. A new `Dialog` variant with arms in the dead router
compiles and does nothing. `Dialog::Queue` therefore has **no key arms at all**;
`QueueDialog::handle_key` is the only implementation, reached through
`FocusManager`. `App::dock_target` has no queue arm either, so the box is a
modal even though it declares `docks_as_viewer() == true`.

Only the two `From` impls are exhaustive over a new variant, and both were
extended along with the round-trip table row.

### Selection follows `entry_id`, never the index

`QueueDialog::set_entries` resolves the previously selected entry by id. A
projection that loses a row — because it was deleted, delivered, or expired —
otherwise slides the cursor onto a different prompt, and the `d` pressed next
removes something the user never looked at. When the selected entry is gone the
cursor clamps to the nearest survivor. Three unit tests cover the survive, the
disappear, and the empty-projection cases.

### One applier, two producers

The read (`open`, `r`), the M003 push (`CoreEvent::UserPromptQueue`), and the
delete response all converge on `apply_user_prompt_queue_loaded`, which now also
updates the mounted box through `focus_manager.with_dialog_mut`. That is what
makes it impossible for row 0's count and the open list to disagree about how
many prompts are waiting, and it means the M003 push reaches an open box with no
re-read — asserted directly.

## 4. Two guards, both proven non-vacuous

Neither of these is asserted by reading source; both were proved by injecting a
violation and observing the failure.

**Observer blocking.** `/queue` is blocked by **omission** in the fail-closed
`is_observer_allowed_command` allowlist — no code registers it, so nothing had to
change. Adding `"queue"` to the allowlist makes
`user_prompt_queue_is_never_observer_allowed` fail, with the observed message
quoted in the transcript. The paired
`user_prompt_queue_observer_check_is_non_vacuous` asserts `/stats`, `/help`, and
`/status` **are** allowed, so the negative cannot pass against a predicate that
refuses everything.

**No second read on delete.** `queue_projection` returns the post-write
projection inside the delete response, so the frontend issues no
`UserPromptQueueList`. This is measured at a recording fake transport through
the real key path.

This one required two corrections before it measured anything, and both are
worth recording:

- The first draft counted `CoreClient::request` calls immediately after
  `on_key` returned. That is a **race**: the request is made on a
  `spawn_tui_task` worker. The counts came back 0 and 1 where 1 and 2 were
  expected. Replaced with a bounded wait on the observed request count.
- With that fixed, an injected `refresh_queue_box(app)` inside
  `apply_queue_delete_finished` **still passed**. The cause was structural, not
  a timing window: everything the test drove ended at the *request*, because
  `start_queue_delete` answers on `tui_cmd_tx` and no frontend test dispatches
  that channel. The apply path was never executed. The test now drives
  `apply_queue_delete_finished` directly and drains; with the injection still in
  place it fails with `["list", "delete:b", "list"]`, left `2`, right `1`.

The source-greep version of this guard was deleted rather than repaired: it
tripped on the module's own **doc comment**, which contained the string
`UserPromptQueueList`. A guard that fires on its own explanation measures
documentation, not behaviour.

## 5. One defect the build caught

The first draft of `refusal_message` mapped
`user_prompt_queue_entry_expired`. **That code does not exist.** The `entry_`
prefix was carried over from the neighbouring `user_prompt_queue_entry_delivering`,
and the real code is `user_prompt_queue_expired`. Without a pin the expired
refusal would have rendered "The queue refused that change
(user_prompt_queue_entry_expired)" forever.

`user_prompt_queue_every_mapped_code_matches_the_daemon` now pins **all six**
mapped codes against `UserPromptQueueError::code()`, and additionally asserts
each has its own sentence — otherwise a constant could be "pinned" while dead.
`user_prompt_queue_unmapped_codes_are_reported_verbatim` covers the fall-through,
and `user_prompt_queue_refusals_do_not_share_a_sentence` asserts the four
user-visible refusals are pairwise distinct. Collapsing `delivering` into
`expired` would tell the user their prompt expired at the moment a turn started.

This is precisely why the TUI matches wire codes as literals rather than
importing the daemon's enum: with the enum imported, the rename and the mapping
would move together and the mismatch would become silent.

## 6. One removal, recorded

The first draft implemented `Widget for &QueueDialog` alongside `Component`,
duplicating the row rendering. Nothing in production uses it — `FocusManager`
renders through `Component` — and two render paths for one box is precisely the
divergence this codebase documents elsewhere. It also made `dialog.render(...)`
in tests resolve to the 2-argument `Widget` method instead of the 4-argument
`Component` one. **Removed**; there is now one render path.

## 7. Scope discipline

The box advertises `j/k`, `r`, and `d`, and nothing else.

- The empty state names **no** verb for putting something in the queue. Add is
  M005, and "press `a` to add" would be a key that does nothing.
- `Esc` is answered by `handle_key` but deliberately **absent from the hint**,
  for the same reason `TreeDialog` omits it: the modal host claims `Esc` as its
  universal exit before consulting the widget, so advertising it would teach a
  key that never reaches this code.
- The box shows no edit, reorder, steer, or send-now affordance anywhere.

Three tests enforce this by scanning the rendered buffer for forbidden strings
and by asserting every key named in the hint reaches an action through
`handle_key` — with `z` asserted inert, so the hint cannot be describing a
fiction.

`d` **is** offered on a `delivering` row. The daemon refuses it with
`user_prompt_queue_entry_delivering`; the box does not pre-empt that with a
local rule, because the refusal is the authority's answer to a real attempt
rather than a silent no-op the user cannot interpret. The entry stays listed
after the refusal.

## 8. Row 0

The suffix composes as a **left-aligned `" · "`-joined suffix** on the existing
status row — the real `stop_hint` shape, not the "right-hand side reserved" the
roadmap described before M004 corrected it against source.

It is text on a reserved row and never a condition on row 0's existence, because
`session_layout` splits `prompt_height` against `Min(10)` and row 0's height is
subtracted from the transcript. Gating row 0 on queue state would reflow the
transcript on every submit and completion.

It sits **after** the phase selection, so a latched `last_error` still outranks
it: the failure is what explains why nothing is progressing, and a waiting count
is not. Asserted.

`user_prompt_queue_suffix_adds_no_row_and_changes_no_geometry` compares the
**screen row** the status line lands on with an empty queue against a three-entry
queue and requires them to be identical — the geometry claim is measured, not
argued.

## 9. Concurrency

`DialogState::queue_delete_request` is an `AsyncUiRequestState`. `start_queue_delete`
calls `begin()`, and `apply_queue_delete_finished` calls `finish`/`fail` first and
drops the completion when either returns `false`. `close_dialog`'s `Dialog::Queue`
arm calls `cancel()`, which bumps the generation — so a delete still in flight
when the box closes mutates **nothing**: not the projection, not a toast, not the
selection. Asserted end to end.

Close does **not** call `requeue_open_permission`. That rule exists because
anything still mounted at close time was dismissed *unanswered*; the queue box
has a cursor, not a pending human decision. Asserted.

A second `/queue` is **answered, not swallowed**: `FocusManager::push` refuses a
duplicate dialog type, so the second open would otherwise do nothing while the
command looked swallowed. It reports that the box is already open.

## 10. Prompt-text confidentiality

A prompt is user input. It appears in the list and nowhere else. Tested by
placing a canary string in the queue and asserting it reaches neither the
keymap hint nor any toast produced by any refusal path.

## 11. Verification actually run

Every command below was executed in this session, on this tree.

- `cargo fmt` — clean
- `cargo check --workspace --all-targets --locked` — clean (via `verify.sh quick`)
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — **clean,
  no warnings**
- `cargo test -p codegg --lib -- dialogs::queue commands::queue` — **22 passed,
  0 failed**, 6487 filtered out
- `cargo test --test user_prompt_queue_box` — **26 passed, 0 failed**
- `scripts/verify.sh quick` — **passed**, including the tui-authority,
  http-route-disposition, audit-coverage, queue-delivery-callsites,
  scheduler-bypass, config-merge-coverage, no-hardcoded-models, and provider
  wire/catalog/resilience guards

Regression targets on the surfaces M004 touched, all green:

| Target | Result |
|---|---|
| `command_routing_adversarial` | 139 passed |
| `identity_m003_daemon_authorization` | 23 passed |
| `tui_logs_window` | 9 passed |
| `tui_main_buffer` | 46 passed |
| `tui_panel_host_chrome` | 26 passed |
| `tui_panel_manager_reachability` | 5 passed |
| `tui_presentation_regressions` | 16 passed |
| `tui_render` | 111 passed |
| `user_prompt_queue_box` | 26 passed |

Injected-violation runs, both reverted before the final sweep: the observer
allowlist entry, and a second read on the delete path.

**The full workspace test suite was not run**, and no hosted CI run is claimed.
Nothing here has been pushed.

## 12. What was deliberately not done

- **No migration, no protocol change, no daemon or store change.** `run_at_ms`
  keeps v82 for M007.
- **No shared chrome helper.** The roadmap's `panel-convergence M004` does not
  exist and geometry has already landed de-duplicated, so `QueueDialog`
  duplicates the established per-widget `enum Chrome { Owned, Hosted }` shape
  rather than forking a helper with no settled shape. It does not open that
  milestone.
- **No second render path** (see §6).
- **No adoption, transfer, or dispatch-now surface.** `user_prompt_queue_unowned`
  and `user_prompt_queue_not_owner` are reported distinctly, but the box adds no
  in-box repair; adoption stays an explicit, audited operation outside it.
- **No in-box add/edit/reorder.** M005.

## 13. Carried-forward items

- The `renumber_dense` / released-`delivering` question recorded during M005
  planning is unchanged and still unresolved. M004 displays `status` from the
  DTO and never reorders, so it neither triggers nor masks it.
- M005's blocker — M004 closure — is **discharged** by this record.
- M006 and M007 have no ADR gate left and now wait only on M004/M005 closure.
- The positive receipt-identity test fixture noted as still absent after M003 is
  still absent.
- Pre-existing and unrelated to this milestone: `session_family` symlink
  failures and two dangling documentation references.