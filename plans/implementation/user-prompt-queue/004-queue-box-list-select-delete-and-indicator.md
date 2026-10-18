# User Prompt Queue Milestone 004 — `/queue` Box: List, Select, Delete, and Status Indicator

Status: **closed** — see `plans/closure/user-prompt-queue/004-status.md`

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the
uncommitted M001/M002a/M002b/M003 tree. Re-read every cited line before editing;
§3 was measured against that tree and `HEAD` alone does not contain it.

Source roadmap:

- `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Milestone 4)

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted 2026-10-18)
- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (accepted
  2026-10-18) — **does not gate this milestone**, but fixes the vocabulary: a row
  that leaves the list leaves because **delivery** happened, and this box must
  not imply a steer removed it. M006 owns send-now.
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (accepted 2026-10-18)
- `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` (accepted 2026-10-18)

Primary class: **capability** — the first milestone in this workstream that a
person can operate. M001–M003 were a correctness fix, two infrastructure
milestones, and the daemon-side delivery path; none of them is reachable by a
key.

## 1. Objective

Give the user a visible, navigable list of queued prompts and an
always-readable indicator that prompts are waiting, and let them discard one
they no longer want.

Delivered as exactly three things: a `/queue` modal that lists the active
session's queue with keyboard selection and scrolling; `delete` of the selected
entry; and a queue-count suffix appended to working strip row 0.

**Not** in this milestone: add, edit, reorder, send-now, timers.

## 2. Why this milestone is ready

| Dependency | State |
|---|---|
| M001 closed | `plans/closure/user-prompt-queue/001-status.md` |
| M002a closed | `plans/closure/user-prompt-queue/002a-status.md` |
| M002b closed | `plans/closure/user-prompt-queue/002b-status.md` |
| M003 closed | `plans/closure/user-prompt-queue/003-status.md` |
| ADR-0035 / ADR-0037 / ADR-0038 | accepted 2026-10-18 |

**The interface dependency named by the roadmap does not exist, and its absence
does not block this milestone.** The roadmap lists `panel-convergence M004` as
the owner of the shared chrome/geometry helper
(`plans/subsystems/user-prompt-queue-roadmap.md:307`, `:700`). Measured facts:

- The **geometry half already exists and is already de-duplicated**:
  `centered_rect` at `src/tui/components/component.rs:290`, called from
  `src/tui/app/render.rs:2623` and elsewhere. Panel-convergence M003 retired the
  duplicate.
- The **chrome half does not exist**: every widget declares its own
  module-private `enum Chrome { Owned, Hosted }` — `mcp.rs:23`,
  `security_review.rs:27`, `plugin.rs:19` — and there is no `render_keymap_hint`
  free function, only the `Component::keymap_hint` seam
  (`src/tui/components/component.rs:234`).
- **`M004` has zero occurrences in
  `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md`.**
  That addendum defines only M001/M002/M003, and M003's own text defers the
  helper: *"The shared chrome/geometry helper is deferred out of M003. It has no
  seam, no failing test to define it… the helper should be its own milestone,
  written against a settled shape."* No such milestone was opened.

The roadmap already supplies the fallback — *"if it has not landed, this
milestone duplicates the established chrome rather than forking the helper"* —
so M004 proceeds by duplication and **does not** wait for, or attempt to write,
that helper. Do not open a second chrome abstraction here: with no settled shape
and no owning milestone, inventing one would fork a convention thirteen widgets
already follow.

## 3. Current implementation evidence

### Backend that already exists and needs no change

Every request this milestone issues is already implemented, authorized, audited,
and tested. M004 is a **frontend** milestone with no storage, protocol, or daemon
work.

| Variant | Fields | Source |
|---|---|---|
| `UserPromptQueueList` | `session_id` | `crates/codegg-protocol/src/core.rs:2338-2340` |
| `UserPromptQueueDelete` | `session_id`, `entry_id` | `crates/codegg-protocol/src/core.rs:2365-2368` |
| `CoreResponse::UserPromptQueue` | `session_id`, `entries` | `crates/codegg-protocol/src/core.rs:1202-1206` |
| `UserPromptQueueEntryDto` | `id`, `ordinal`, `text`, `text_bytes`, `status`, `created_at_ms`, `expires_at_ms` — every field `#[serde(default)]` | `crates/codegg-protocol/src/core.rs:1959-1974` |
| `CoreEvent::UserPromptQueue` | `session_id`, `entries` — the M003 push path | `crates/codegg-protocol/src/core.rs:3721-3725` |

Authority: `user_prompt_queue_list` is `ViaSession` + `Capability::SessionRead`;
`user_prompt_queue_delete` is `ViaSession` + `Capability::AgentInvoke`
**plus** the durable queue owner enforced by
`ControllerScope::QueueMutation` (`src/core/daemon_control.rs:48`). The
descriptor comment is explicit that the capability gate is necessary, not
sufficient (`crates/codegg-core/src/authorization/policy.rs:392-396`).

Dispatch arms: read at `src/core/daemon_turns.rs:873-875`; delete at
`:914-928`; shared mutation handler at `:1969-1975` with the documented
authorize → mutate → audit order at `:1967-1968`.

**`delete` returns the fresh projection.** `queue_projection`
(`src/core/daemon_turns.rs:2144-2162`) re-reads the queue after a successful
write and returns it as `CoreResponse::UserPromptQueue`. M004 must apply that
response directly instead of issuing a second read — one round trip, and the
rendered list cannot disagree with the write that produced it. `store_error`
short-circuits so a refusal never reports a projection the call did not produce.

### TUI state that already exists

- `SessionState::user_prompt_queue: Vec<UserPromptQueueEntryDto>` —
  `src/tui/app/state/session.rs:189`. Its doc (`:182-188`) states that a
  `delivering` entry appears here and is not editable.
- `apply_user_prompt_queue_loaded` — `src/tui/app/mod.rs:2693-2715`. Adopts
  through a superseded-session guard and preserves daemon ordinal order
  verbatim; **no local sort**.
- `load_user_prompt_queue` — `src/tui/app/mod.rs:2649-2685`. Builds the request,
  calls `client.request`, maps the typed response, and emits
  `TuiCommand::UserPromptQueueLoaded`.
- `App::load_user_prompt_queue` has **exactly one caller**:
  `src/tui/app/mod.rs:12310`, inside `App::set_session`. **There is no manual
  refresh, no key binding, and no `/queue` command** — `grep '"/queue"'` over
  `src` returns nothing. The box therefore needs a producer that does not exist
  yet; see WP-B.
- The M003 push arm — `src/tui/app/mod.rs:2469-2478` — routes through the same
  applier, so a pushed and a read projection cannot diverge.

### Dialog and key plumbing

- `Dialog` enum: `src/tui/app/types.rs:1-80` (49 variants). `DialogType`:
  `src/tui/components/component.rs:22`.
- **Only two `match` sites are exhaustive for a new variant** — the two
  conversion impls at `src/tui/components/component.rs:139-143` and `:149-202`,
  neither of which has a wildcard — plus the enums themselves and the
  round-trip table at `src/tui/components/component.rs:381`. `close_dialog`
  (`src/tui/app/mod.rs:9731-9868`), `open_dialog`
  (`src/tui/app/modal.rs:271-583`), `handle_dialog_key`'s pre-switch
  (`:4337-4358`) and `render.rs:407-414` all end in `_ => {}`.
- **The legacy per-variant key router is `#[cfg(any())]`-disabled** at
  `src/tui/app/mod.rs:4364-4368`. `FocusManager` is the sole key authority.
  A new box **must** be a FocusManager-mounted `Component`; adding a `Dialog`
  variant with an arm in the dead router would compile and do nothing.
- Modal ownership: `App::on_key` short-circuits the whole prompt pipeline when a
  modal is up (`src/tui/app/mod.rs:3802-3806`) and routes to
  `handle_dialog_key` (`:4294`), which then resolves the docking gesture first
  (`:4328-4336`, gated on `focus_manager.top_docks_as_viewer()`) and otherwise
  hands the key to `self.focus_manager.handle_key(key)` (`:4359-4361`).
- **Observer blocking is fail-closed by omission**, not by a flag. `Command` has
  no role field (`src/tui/command.rs:309-330`). `is_observer_allowed_command`
  (`src/tui/app/state/observe.rs:476-528`) normalizes the name and
  `matches!`es a narrow read-only allowlist; everything unlisted is blocked, and
  `ObserverState::blocks_command` (`:436-444`) drives
  `src/tui/commands/observe.rs:605-616`. **`/queue` is therefore already blocked
  for observers by default and needs no allowlist entry** — and must never get
  one.

### Row 0

- `render_working_strip` — `src/tui/app/render.rs:1653`; the row-0 `Line` is
  built at `:1709-1779` with this precedence: latched `last_error` (`:1709-1713`,
  comment: *"a latched failure outranks every live phase"*) → `stopped_turn`
  (`:1714-1717`) → working activity (`:1718-1726`) → awaiting-task count
  (`:1737-1744`) → `idle` (`:1748`). Suffixes are then `" · "`-appended at
  `:1752-1775` and the line is pushed at `:1776-1779`.
- **The roadmap misdescribes the stop hint.** It records that row 0's
  "right-hand side is reserved for a named escape gesture"; in code
  `stop_hint` (`src/tui/app/render.rs:1690-1703`) is a **left-aligned
  `" · "`-joined suffix** on the same line, with the rationale comment at
  `:1683-1687`. Compose with that shape.
- Reservation is unconditional: `working_strip_rows` returns `1 + shown +
  overflow` (`src/tui/app/render.rs:1634-1642`), subtracted from the prompt
  chunk at `src/tui/app/render.rs:181` and `:243-252`; `session_layout` sets the
  transcript to `Constraint::Min(10)` at `src/tui/layout.rs:359`.

### Existing list-dialog precedent

`TreeDialog` (`src/tui/components/dialogs/tree.rs:54-62`) is the closest
complete instance: `selected: usize`, `scroll: CenteredScroll`, `handle_key` at
`:421` with `Esc` (`:423`), `Up | Char('k')` (`:424`), `Char('e')` (`:441`),
`render` at `:487` delegating to `render_body(..., Chrome::Owned)` (`:488`).
`PolicySelectorDialog` (`policy_selector.rs:141-259`) is the smallest complete
chrome instance and the closest precedent for the open/close lifecycle:
`open_policy_selector` sets state **then** calls `open_dialog` and clears state
**before** `close_dialog()` (`src/tui/commands/policy.rs:310-348`).

There is **no shared list-selection helper**; selection movement is 19 separate
per-widget method pairs. The only shared paging primitive is `CenteredScroll`
(`src/tui/components/scroll.rs:2`) with `clamp(cursor, total, visible)` at `:23`.

## 4. Invariants that must not regress

- **A queued prompt is never silently lost.** Delete is the user asking for it to
  go; every other path must still leave the entry visible with its status.
- **Prompt text is never logged verbatim and never placed in an error message,
  a keymap hint, or a diagnostic.** Refusals carry bounded codes only.
- **Row 0 keeps its unconditional single-row reservation and its precedence.**
  A latched failure outranks the queue suffix; the suffix never becomes the
  status line.
- **No new row, no transcript reflow, no geometry change.** The suffix is text
  appended to an existing line.
- **The box advertises nothing it does not do.** No box may advertise `Esc` or
  `Enter` as a close verb, and a caller-supplied footer is not replayed into the
  keymap hint.
- **`close_dialog` must not undo the action that preceded it.** Anything still
  mounted at close time was dismissed *unanswered*. Every delete path clears its
  state **before** calling `close_dialog`, exactly as
  `src/tui/commands/policy.rs:337-338` does for its commit path.
- **Observer mode is blocked by omission and stays that way.**
- **Transport authority:** a TUI answer is never routed through `AppMode`
  alone. This milestone issues only `core_client.request` calls, which are the
  socket-daemon path and need no `is_ws_attach_transport()` gate; that gate
  governs `send_remote_message` sites only.

## 5. Scope

### In scope

- `/queue` command registration and its dispatch arm.
- `Dialog::Queue` and `DialogType::Queue`, both conversion arms, and the
  round-trip table row.
- A FocusManager-owned `queue.rs` component: list render, selection, scrolling,
  bounded per-row text, status display, empty state, keymap hint.
- Manual refresh on open, plus a re-read after any external change the box can
  observe.
- Delete of the selected entry, applying the response's returned projection.
- Working strip row 0 queue-count suffix.
- Observer-blocked regression evidence.
- `architecture/tui.md`, the `tui` skill, `docs/tui.md`, and `AGENTS.md` if the
  key surface changes.

### Explicitly out of scope

- Add, edit, and reorder — M005.
- Send-now / steering — M006, gated on nothing now that ADR-0036 is accepted,
  but not this milestone.
- Delayed delivery and `run_at_ms` — M007, **which owns migration v82**.
- A shared chrome or keymap-hint helper. Duplicating the established shape is
  this milestone's answer.
- Any migration. **M004 takes no migration number.**
- Any change to the daemon, the store, the policy table, or the protocol.

## 6. Required production changes

### Core/domain

None. `UserPromptQueueStore`, the six operations, and the delivery path are
closed and unchanged.

### Storage and migrations

**None.** No migration, no `STORAGE_LAYOUT_VERSION` bump, no schema change.

### Protocol and DTOs

**None.** Reuse `UserPromptQueueList`, `UserPromptQueueDelete`,
`CoreResponse::UserPromptQueue`, and `UserPromptQueueEntryDto` as they are.

Two facts the render must respect rather than assume away:

- **The DTO has no `reason` field.** A `failed` entry's recorded reason is not
  visible to any client. M004 may show `status`; it may **not** promise to show
  why. Surfacing a reason is a protocol change this milestone does not make.
- **The projection contains `pending` and `delivering`.** That is deliberate:
  the `delivering` row is exactly the one the user must see and must not be able
  to delete. Read `status` from the DTO — `user_prompt_queue_entry_to_dto`
  (`src/core/daemon_turns.rs:2341-2353`) carries the entry's own status and
  stopped hardcoding `"pending"`. Never assume the list is pending-only.

### Runtime and concurrency

- A read producer on open. `load_user_prompt_queue` currently fires only from
  `App::set_session`; opening the box must issue its own read.
- Apply `CoreResponse::UserPromptQueue` from the delete call directly
  (`queue_projection`, `src/core/daemon_turns.rs:2144-2162`) — no second read.
- The push path stays authoritative for out-of-band changes; the box's own
  re-read is for staleness the push cannot cover (a second attached frontend).
- Delete is refused for a `delivering` entry with
  `user_prompt_queue_entry_delivering`
  (`UserPromptQueueError::Delivering`). Surface that as its own message; it is
  **not** the same condition as a missing entry and must not share a sentence.
- `pending_turn_submission` is a single `Option` and the shared completion guard
  is mandatory for any new async path; follow
  `src/tui/commands/project_catalog.rs:24-51` and
  `AsyncUiRequestState::finish`/`fail`
  (`src/tui/app/state/async_request.rs:82-87`, `:98-103`).

### Frontend or operator surface

- Registration: `Command::new("/queue", CommandCategory::…, CommandAction::Builtin(BuiltinSlashAction::Queue))` with a description, in the builder chain in `impl Default for CommandRegistry` (`src/tui/command.rs:1173` onward; `/logs` at `:702` is the minimal example). `action` must agree with the sibling fields (`src/tui/command.rs:325-329`).
- Dispatch arm in `dispatch_builtin_command` (`src/tui/app/mod.rs:5358-5361`);
  its `match` ends in a wildcard, so the arm is required but not for
  exhaustiveness.
- `Dialog::Queue` in `src/tui/app/types.rs`, `DialogType::Queue` in
  `src/tui/components/component.rs:22`, both `From` arms at `:139-143` and
  `:149-202`, and a row in the round-trip table at `:381` — **without that row
  the round-trip test is vacuous**.
- Component in `src/tui/components/dialogs/queue.rs`, mounted via `push_dialog`
  (`src/tui/app/mod.rs:9665-9676`) or `open_dialog`
  (`src/tui/app/modal.rs:271-583`).
- Selection: `selected: usize` + `scroll: CenteredScroll`, `j`/`k` **and** arrow
  keys. The modal intercepts before the host binding map, so bare letters are
  safe here; only the shifted docking chords in `handle_dialog_key`
  (`:4328-4336`) are stolen, and only when `top_docks_as_viewer()`.
- Row 0 suffix in `render_working_strip`, appended in the existing suffix chain
  (`:1752-1775`), shown only when the projection is non-empty.

### Security and authorization

- **Add nothing to `is_observer_allowed_command`.** `/queue` is blocked by
  omission. The mutation is separately refused by the daemon for a non-owner,
  so the box's delete is refused twice over and the UI must not pre-empt the
  daemon into a different answer.
- `queue-add`, `queue-edit`, `queue-reorder` subcommands are **not** introduced
  here. M004 advertises `/queue` and `delete` only.

### Documentation and static guards

- `architecture/tui.md`, `.opencode/skills/tui/SKILL.md`, `docs/tui.md`.
- No new guard script is required. The two existing TUI guards
  (`scripts/check_tui_transport_authority.py`, and the panel-convergence key
  reachability checks) must still pass unchanged.

## 7. Ordered work packages

### Work package A — Command registration, dialog identity, observer evidence

Intent: give the box a name and an identity, and prove the observer boundary
before anything renders.

Required changes: add the `/queue` registration and `BuiltinSlashAction::Queue`
variant; add `Dialog::Queue`, `DialogType::Queue`, both `From` arms, and the
round-trip table row; add the dispatch arm that opens the box.

Acceptance evidence: a test drives `/queue` through the **command registry** and
`App::on_key` — not by calling a handler — and asserts the box is mounted.
A test asserts `/queue` is refused for an observer while observing, and asserts
it stays absent from `is_observer_allowed_command`. Extend the existing assertion
at `tests/identity_m003_daemon_authorization.rs:1472-1484`, which already
requires `queue`, `queue-add`, `queue-edit`, `queue-delete`, and `queue-reorder`
never to be observer-allowed.

### Work package B — The list component

Intent: render the queue honestly.

Required changes: `queue.rs` with selection, `CenteredScroll` paging clamped to
the visible height, bounded per-row truncation with an announced indicator, the
entry's `status` shown, an empty state, and a `keymap_hint`.

**The empty state must not name a way to add the first entry.** Add arrives in
M005; advertising it now would violate "the dialog advertises nothing it does
not do" and the no-`Esc`/`Enter`-as-close-verb rule. M005 upgrades the copy.

Acceptance evidence: render tests for empty, one-row, many-rows-needing-scroll,
a `delivering` row, and a long-prompt truncation. Selection and scrolling driven
through `handle_key`, with arrow and vim paths both covered.

### Work package C — Delete the selected entry

Intent: discard one entry, correctly and visibly.

Required changes: a `delete` action bound to the selected row; issue
`UserPromptQueueDelete { session_id, entry_id }`; on
`CoreResponse::UserPromptQueue` apply the **returned** projection; on
`CoreResponse::Error` show the code and **clear selection state before any
close**; keep the box open on failure with the entry still listed.

Acceptance evidence: a test deletes the selected entry and asserts exactly one
row disappeared and the survivors kept daemon order; a test deletes a
`delivering` row and asserts the refusal is reported as *delivering*, not as a
generic failure or a missing entry; a test asserts that closing the box after a
delete does not resurrect or requeue anything (the `close_dialog` rule).

### Work package D — Working strip row 0 suffix

Intent: show that prompts are waiting without adding a row.

Required changes: append the count to the existing suffix chain, only when the
projection is non-empty, under the established precedence — a latched
`last_error` must still outrank it.

Acceptance evidence: render tests proving the suffix appears when entries exist,
is absent when the queue is empty, composes while a latched error is displayed
without replacing it, and leaves `working_strip_rows` unchanged at every state.

### Work package E — Refresh path and documentation

Intent: make the box correct after out-of-band change, and record the contract.

Required changes: issue a read on open; document the box, its keys, and the row-0
suffix in `architecture/tui.md`, the `tui` skill, and `docs/tui.md`; update
`plans/subsystems/user-prompt-queue-roadmap.md` Milestone 4 status and
`plans/registry.md`.

Acceptance evidence: a test that opens the box after a projection-push shows the
pushed rows without a re-read, and that a forced re-read after an external
mutation converges.

## 8. Failure, cancellation, restart, and contention semantics

- **Delete refused because the entry is `delivering`:** the entry stays listed
  with its status; the message says it is being delivered. Not an error toast
  that hides the list.
- **Delete refused because the caller is not the queue owner:** surfaced as a
  refusal. Do not clear the selection — the user may retry as the owner.
- **Delete refused because the entry is gone:** re-read rather than guessing;
  a stale row must not leave a permanently unselectable index.
- **Read fails (transport):** the previous projection stays. Clearing it would
  make a dead socket indistinguishable from an empty queue — the reason
  `apply_user_prompt_queue_loaded` early-returns on error
  (`src/tui/app/mod.rs:2693-2715`).
- **Box closed mid-flight:** a stale completion must not mutate selection or
  projection. The applier already guards on superseded session; the delete
  completion must additionally be inert when the box is closed.
- **Two attached frontends:** the daemon is the authority. The loser's view
  converges through the push path, not through a merge.
- **Daemon restart:** nothing here persists frontend state. The read on open is
  authoritative.
- **Row 0:** never gate the row on queue state. The row exists unconditionally
  and the suffix is optional text.

## 9. Compatibility and migration

- **No migration.** No `STORAGE_LAYOUT_VERSION` change. `run_at_ms` stays
  unclaimed for M007 at v82.
- **No protocol change.** A frontend that predates the queue sees an absent
  projection and renders nothing; `/queue` simply does not exist for it.
- **`/queue` is additive to the command registry.** It must not be added to the
  observer allowlist, so observer-mode completion never advertises it.
- No configuration, no feature flag, no removal criteria.

## 10. Required tests

Prefix every new test `user_prompt_queue_` so it is greppable and so a
zero-select global filter can never be mistaken for a passing run.

### Focused unit tests

- Selection movement across `j`, `k`, `Up`, `Down`, with clamping at both ends.
- `CenteredScroll` paging when rows exceed the visible height.
- Per-row truncation of a long prompt, and the status string rendering.
- The empty state renders and names nothing that M004 does not implement.
- `keymap_hint` names only keys the component actually handles.

### Integration tests

- `/queue` opens through the command registry and `App::on_key`, not a direct
  handler call. **This distinction is load-bearing:** an existing
  `workspace_command_and_hotkey_open_same_dashboard` test called its handler
  twice directly and therefore proved nothing about key reachability.
- Every advertised key reaches its action through the real key path. The test
  must be proven non-vacuous by asserting a key the box does **not** bind does
  nothing.
- Delete applies the response projection with no second read (assert the
  request count).
- Open issues a read even when `SessionState::user_prompt_queue` is already
  populated.
- A `CoreEvent::UserPromptQueue` push updates an open box.

### Restart and recovery tests

- A stale delete completion arriving after the box closed mutates nothing.
- A read failure leaves the previous projection intact.

### Contention and cancellation tests

- Delete while a turn is active: refused by the same controller predicate, with
  a message distinct from a channel condition.
- A row deleted by another frontend between open and delete does not leave an
  unselectable index.

### Security and negative tests

- An observer cannot open `/queue`; the toast is the observer-blocked message.
- `/queue` is absent from `is_observer_allowed_command`, proven by executing the
  predicate rather than by reading the source.
- Prompt text appears in no toast, no `keymap_hint`, and no `tracing` call —
  asserted by scanning the built string, not by inspection.
- A non-owner caller is refused by the daemon; the UI does not pre-empt it.

### Migration and compatibility tests

- None required; assert explicitly that `STORAGE_LAYOUT_VERSION` is unchanged
  if any storage file is touched at all (it should not be).

## 11. Required verification commands

```bash
# narrow tests first — run each target, never a zero-select filter
cargo test -p codegg-core --test user_prompt_queue
cargo test -p codegg-core --test user_prompt_queue_delivery
cargo test --test user_prompt_queue_indicator    # if this milestone adds one
cargo test --test tui_render

# guards
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_queue_delivery_callsites.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
bash   scripts/check-core-boundary.sh

# formatting and linting
cargo fmt --check
cargo check --lib
cargo clippy --all-targets -- -D warnings
```

Use `CARGO_BUILD_JOBS=2`. Do not use `--all-features`. Report only commands
actually run, with real counts; never claim a hosted CI run.

## 12. Documentation updates

- `architecture/tui.md` — the box, its keys, its lifecycle, the row-0 suffix,
  and the pointer to M005/M006 for what it deliberately does not do.
- `.opencode/skills/tui/SKILL.md` — the command-dispatch boundary and the
  FocusManager-only key authority, since the legacy router is `#[cfg(any())]`.
- `docs/tui.md` — user-facing guidance for `/queue` and `delete`.
- `architecture/session.md` — only if the read/write projection split is
  described somewhere a reader would now be misled.
- `plans/subsystems/user-prompt-queue-roadmap.md` and `plans/registry.md` —
  Milestone 4 status and the corrected chrome-helper dependency.

## 13. Acceptance criteria

1. `/queue` opens a modal listing the active session's undelivered queue in
   daemon ordinal order, and the order is not re-sorted locally.
2. Selection moves with both arrow and vim keys and is clamped; a long list
   scrolls with `CenteredScroll` clamped to the visible height.
3. `delete` removes exactly the selected entry, applies the projection returned
   by that same call, and leaves the survivors in daemon order.
4. Deleting a `delivering` entry is refused with a message that says so and is
   distinct from both a missing entry and a non-owner refusal.
5. Closing the box after a delete does not requeue, resurrect, or undo it.
6. The row-0 suffix appears only when the projection is non-empty, composes
   under the existing precedence, never replaces a latched failure, adds no row,
   and changes no geometry.
7. Every advertised key works through the real key path, and the box advertises
   nothing it does not do — in particular no `Esc`/`Enter` close verb and no
   add key.
8. An observer is refused `/queue`, and the refusal is the observer-blocked
   message rather than a generic one.
9. No prompt text appears in any toast, hint, diagnostic, or log.
10. No migration, no protocol change, no daemon change, and no shared chrome
    helper is introduced.

## 14. Stop conditions

Stop and report rather than improvise when:

- Implementing the box appears to require a change to
  `ControllerScope::QueueMutation` or to the policy table.
- The projection turns out not to carry `delivering` entries as M003's closure
  claims — verify against the store before assuming either answer.
- The row-0 suffix cannot be added without changing `working_strip_rows` or
  reflowing the transcript. That is a roadmap non-goal, not a layout problem to
  solve here.
- A second chrome helper appears to be needed. Duplicate instead; if duplication
  genuinely does not compose, stop and report.
- Scope would pull in add, edit, reorder, send-now, or timers — those are M005,
  M006, and M007.
- Any migration appears necessary. M004 takes none.

## 15. Closure evidence required

The closure record must contain:

- The exact command list run, with real pass/fail counts, and an explicit
  statement of what was **not** run.
- Proof the key-path test is non-vacuous: the assertion that fails when a
  non-vacuous guard is removed, observed rather than asserted.
- A statement that M004 is the workstream's **first user-operable** milestone,
  and exactly what is still unreachable (add, edit, reorder, send-now, timers).
- The empty-state copy, verbatim, with a note that it names nothing M005 adds.
- Confirmation that `STORAGE_LAYOUT_VERSION` and the migration set are
  unchanged.
- Residual risks, including the missing shared chrome helper and the absent
  `reason` field on the DTO.

## 16. Handoff notes

- **Preserve the uncommitted M001–M003 tree.** It is unstaged and unstashed by
  design. Do not stash, revert, reformat, or `git add -A`.
- `CARGO_BUILD_JOBS=2`; the host has limited free disk. Do not run
  `--all-features`; `verify.sh full` drags in real-server tests.
- Run tests per target. A `-- user_prompt_queue` filter that selects zero tests
  is a failure to report, not a pass.
- The line numbers in §3 were measured against the M001–M003 tree. Re-verify
  before editing; the file has moved since `HEAD`.
- Guard scripts must be proven non-vacuous by injecting a violation and
  observing the failure, not by reading the pattern.
- Do not edit `src/tui/app/mod.rs:4364-4368` to re-enable the legacy key router.
  It is dead on purpose, and FocusManager owns keys.