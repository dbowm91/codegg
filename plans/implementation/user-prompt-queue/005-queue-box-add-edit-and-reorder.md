# User Prompt Queue Milestone 005 — `/queue` Box: Add, Edit in Place, and Reorder

Status: **closed** — see `plans/closure/user-prompt-queue/005-status.md`
(closure **discharged 2026-10-18**). Its sole hard dependency, the M004 closure,
was discharged 2026-10-18; no ADR gate applies.

> **§2 "Why this milestone is not yet ready" is historical.** The M004 closure it
> lists as absent now exists. The reasoning below is kept as the record of what
> was true when the plan was written and was not rewritten at closure.
>
> **Two §3 premises were corrected against source, and one is a wrong premise:**
>
> 1. **§3(b) is settled by measurement.** A released `delivering` entry resumes at
>    the **back** of the queue and keeps the displacement ordinal
>    (`USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET` = 1 000 000) rather than being
>    re-densified, so it sits outside the dense band entirely. Recorded as
>    accepted-and-documented behaviour; a fix reopens closed M002b/M003.
> 2. **The "Key availability for reorder" reasoning is a wrong premise for this
>    box.** It concluded shifted `K`/`J` stay available because
>    `docks_as_viewer()` defaults to `false`, but `QueueDialog` **overrides** it to
>    `true` (M004). The real reason they survive is that `App::dock_target` has no
>    `DialogType::Queue` arm, so the docking-steal branch is dead regardless of the
>    viewer flag. `[`/`]` were shipped instead — a shifted variant of the movement
>    key that does something *other* than move is a pairing people misfire.
>
> §3's store line numbers were re-verified unchanged against the M004 tree.

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the
uncommitted M001/M002a/M002b/M003 tree, **plus the M004 tree**. Re-read every
cited line before editing; §3 was measured against the M001–M003 tree and
assumes M004's `queue.rs` exists as described in
`plans/implementation/user-prompt-queue/004-queue-box-list-select-delete-and-indicator.md`.

Source roadmap:

- `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Milestone 5)

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted 2026-10-18)
- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (accepted
  2026-10-18) — governs vocabulary only. **No M005 action may be described as
  "send now" or "force";** M006 owns steering. M005's `add` queues, and M003
  delivers automatically.
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (accepted
  2026-10-18) — adoption policy is settled: **the calling principal becomes the
  owner on first add.** An `Unowned` queue is refused for every mutation and
  repaired only by the explicit adoption operation.

Primary class: **capability**.

## 1. Objective

Let the user author, revise, and order queued prompts, so the queue is
correctable rather than append-only.

Three actions on the M004 box: **add** an entry, **edit the selected entry in
place**, and **reorder the selected entry** toward earlier or later.

## 2. Why this milestone is not yet ready

| Dependency | State |
|---|---|
| M004 closed | **required, absent** — `plans/closure/user-prompt-queue/004-status.md` does not exist |
| M003 closed | discharged 2026-10-18 |
| M002b closed | discharged |
| ADR-0035 / ADR-0037 | accepted 2026-10-18 |

M005 adds three more key bindings to one component, two of which require
capturing typed text inside a list widget that is simultaneously navigating. The
list, the selection contract, and the close/requeue lifecycle are M004's
deliverables, not assumptions. Starting before M004 closes would mean building
the selection model twice.

**No ADR is required.** ADR-0036 is accepted and constrains M005's copy but does
not gate it; ADR-0037 already decided adoption.

## 3. Current implementation evidence

### The three requests, all already implemented

| Variant | Fields | Source |
|---|---|---|
| `UserPromptQueueAdd` | `session_id`, **`entry_id`**, `text` | `crates/codegg-protocol/src/core.rs:2351-2355` |
| `UserPromptQueueEdit` | `session_id`, `entry_id`, `text` | `crates/codegg-protocol/src/core.rs:2359-2363` |
| `UserPromptQueueReorder` | `session_id`, `entry_ids: Vec<String>` | `crates/codegg-protocol/src/core.rs:2374-2377` |

**`entry_id` is client-supplied.** The TUI must mint it — follow the existing
shape at `src/tui/app/mod.rs:2653`
(`uuid::Uuid::new_v4().to_string()`). Minting it client-side is what makes add
idempotent under retry: the same id re-derives the same row rather than
duplicating the user's prompt.

`entry_ids` is documented as *"a permutation of the live set, not a patch and
not a move; anything else is refused"* (`crates/codegg-protocol/src/core.rs:2371-2373`).

All three require `Capability::AgentInvoke` **plus** `ControllerScope::QueueMutation`
(`crates/codegg-core/src/authorization/policy.rs:397-416`,
`src/core/daemon_control.rs:48`), and **all three return the post-write
projection** via `queue_projection` (`src/core/daemon_turns.rs:2144-2162`).
Apply that response; never issue a second read.

### The three sharp edges, measured in the store

These are the parts that decide whether M005 is a frontend milestone or a store
correction. All three are in
`crates/codegg-core/src/session/user_prompt_queue.rs`.

**(a) Reorder's permutation is checked against the PENDING set only.**

```rust
const LIVE_STATUS_PREDICATE:     &str = "status IN ('pending', 'delivering')";  // :50
const OPERABLE_STATUS_PREDICATE: &str = "status = 'pending'";                  // :57
```

`reorder` (`:839-865`) calls `list_live_in`, whose `WHERE` uses
`OPERABLE_STATUS_PREDICATE` (`:1157`), then `is_permutation(&live, entry_ids)`
(`:856`). `is_permutation` requires **exact length equality** plus set equality
and rejects duplicates.

Consequence: the client projection contains `pending` **and** `delivering`
(`list_projected_in`, `:1184`, using `LIVE_STATUS_PREDICATE`). **A TUI that
sends every id it rendered will be refused with `OrderMismatch` the moment any
entry is `delivering`.** M005 must build the permutation from pending rows only.
This is the single most likely defect in this milestone.

**(b) `renumber_dense` displaces every row in the session, then densifies the
targets.**

```rust
// Displace first, uniformly. Distinct ordinals stay distinct under a
// uniform offset, and every displaced ordinal leaves the 0..n band the
// dense targets occupy, so neither statement can violate
// `UNIQUE (session_id, ordinal)`.
sqlx::query("UPDATE user_prompt_queue_entry SET ordinal = ordinal + ? WHERE session_id = ?")
```

The displacement is **session-wide and status-blind**. So a `delivering` row
does not collide — the band offset guarantees that — but its ordinal jumps far
above every pending row. Since projections sort by ordinal, a delivering row
renders **last** regardless of where the user queued it. And if that delivery is
then *released* (a transient refusal, which M003 deliberately treats as
requeueable rather than failed), the entry returns to `pending` **at the end of
the queue**. The user's prompt silently moves to the back.

This is closed M003/M002b behaviour, not something M005 introduces. M005 must
**test it and record the outcome**. If the answer is "the entry jumps to the
back", that is either accepted-and-documented behaviour or a store defect —
and a store change reopens closed milestones, so it is a stop condition (§14),
not a TUI workaround.

**(c) Ownership is decided at first add, and an unowned queue has no in-box
repair.**

`Absent` adopts atomically on first add; `Unowned` is refused for **every**
mutation and repaired only by `UserPromptQueueAdopt`. Adoption is deliberately
explicit and audited. M005 must therefore surface the unowned refusal
distinctly and must **not** add an adopt action to the box — but it must also
say plainly that the user has no in-box repair, rather than leaving them stuck.

### Text-input safety — the mechanism add and edit both need

A queue box that both navigates and accepts typed text will swallow letters.
Typing `d` must not delete and `e` must not edit.

The modal short-circuit means the host binding map is never consulted
(`src/tui/app/mod.rs:3802-3806`), so the box itself must decline keys while a
text field has focus. The precedent is `ConnectDialog`:

```rust
crossterm::event::KeyCode::Char('k') if !self.is_text_input_step() => {
    self.navigate(false);
    None
}
```
— `src/tui/components/dialogs/connect.rs:1189-1191`, predicate at `:495-501`,
comment at `:1187-1188`: *"name or host may legitimately contain them — so they
must fall through to `insert_char` instead of being swallowed here."*
`App::on_char` mirrors it at `src/tui/app/mod.rs:4892-4896`, backspace at
`:4937-4941`.

**There is no shared `captures_text` / `is_text_input` / `wants_text`
predicate on `App`.** The only implementation is that one per-dialog method. M005
must introduce the queue box's own equivalent and guard **every** action letter
with `!is_text_step()`.

### Key availability for reorder

- `d` is delete (M004), `a` is add, `e` is edit. Reorder needs a different pair.
- The host steals **shifted `H`/`J`/`K`/`L`** only when
  `focus_manager.top_docks_as_viewer()` is true
  (`src/tui/app/mod.rs:4328-4336`), and **`docks_as_viewer()` defaults to
  `false`** (`src/tui/components/component.rs:221-222`). A modal box that does
  not override it keeps its Shift-qualified keys — shifted `K`/`J` are therefore
  available. This must be asserted, not assumed; `plan.rs:526`
  (`plan_keymap_hint_names_no_key_the_host_answers_first`) is the exact
  precedent test, and the M004 box's advertised keys must also be re-checked
  against the same rule now that a new surface sits under it.

## 4. Invariants that must not regress

- **M004's invariants all still hold**: nothing advertised that is not
  implemented; no `Esc`/`Enter` close verb; `close_dialog` never undoes the
  action that preceded it; row 0 keeps its unconditional reservation and
  precedence; prompt text never in a toast, hint, or log.
- **Queue order is a daemon-owned total order.** The TUI sends a permutation and
  renders what comes back. It never sorts locally and never computes an ordinal.
- **Edit changes neither identity nor position.**
- **Text is never silently truncated.** Length, count, and expiry bounds are
  typed refusals; a refusal is shown, not absorbed.
- **Observer mode stays blocked by omission** — `queue-add`, `queue-edit`, and
  `queue-reorder` must never enter `is_observer_allowed_command`.
- **Two identical prompts are two entries.** Idempotency is entry identity, never
  text equality.

## 5. Scope

### In scope

- `add`: a text-entry step that queues immediately.
- `edit`: in-place text replacement of the selected entry.
- `reorder`: move the selected entry earlier or later.
- The `is_text_step()` predicate and the guards it enables.
- Empty-state copy upgrade to name the add key (M004 deliberately did not).
- Reordering rejection and messaging for a `delivering` entry.
- Documentation and the keymap-hint test.

### Explicitly out of scope

- **Steering / send-now.** M006. No M005 copy may say "send now" or "force".
- **Timers and `run_at_ms`.** M007, which owns migration v82.
- **`adopt` / `transfer`.** Explicit, audited operations outside the box.
- **Dispatching the new entry immediately.** Add always queues; M003's wake
  delivers it. A "run this now" affordance would be steering by another name.
- Any migration, protocol change, store change, or daemon change.
- A shared text-input predicate on `App`. One box's own predicate, matching the
  `Connect` precedent.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

**None.** M005 takes no migration number. `run_at_ms` stays unclaimed for M007.

### Protocol and DTOs

**None.** Reuse the three existing variants and the existing response.

### Runtime and concurrency

- Mint `entry_id` client-side with `uuid::Uuid::new_v4().to_string()`.
- Apply `CoreResponse::UserPromptQueue` from the mutation's own response.
- **Build reorder's permutation from `status == "pending"` rows only.**
- On `OrderMismatch`, **re-read once and do not retry blind.** The likely cause
  is a stale projection — another frontend mutated the queue — or a `delivering`
  row the client failed to filter. Re-reading surfaces the truth; an automatic
  retry loop against a length-equality check would be a contention source.
- Every new async path uses `spawn_tui_task` / `spawn_registered_tui_task` plus
  `AsyncUiRequestState::finish`/`fail`
  (`src/tui/app/state/async_request.rs:82-87`, `:98-103`), following
  `src/tui/commands/project_catalog.rs:24-51`.
- Text-entry step state is cleared **before** `close_dialog()`.

### Frontend or operator surface

- Three new actions on M004's `queue.rs`, plus its `is_text_step()` predicate
  guarding every action letter, following `connect.rs:1189-1195`.
- `keymap_hint` extended to name only the keys actually handled, and re-verified
  against the host-first rule of `plan.rs:526`.
- Add and edit commit on an explicit confirm step; the keys must be advertised.
- Selection must survive a rejected edit: a failed edit must not move the cursor
  or clear the buffer.

### Security and authorization

- Add nothing to `is_observer_allowed_command`.
- `queue-add`, `queue-edit`, `queue-reorder` must be proven never
  observer-allowed by executing the predicate. The existing assertion at
  `tests/identity_m003_daemon_authorization.rs:1472-1484` already names all
  three; extend rather than duplicate it.
- Prompt text in the text-entry buffer is user-authored content in the TUI only.
  It must not reach a `tracing` call, a toast, or an error string.
- Bounds refusals are shown as codes, never as the offending text.

### Documentation and static guards

- `architecture/tui.md`, `.opencode/skills/tui/SKILL.md`, `docs/tui.md`.
- No new guard script required; existing guards must pass unchanged.

## 7. Ordered work packages

### Work package A — The text-input step predicate

Intent: build the seam add and edit both need, before either uses it.

Required changes: an `is_text_step()` predicate on the queue box mirroring
`ConnectDialog::is_text_input_step` (`connect.rs:495-501`); route `Char` and
`Backspace` into the buffer under it; guard **every** action letter — `j`, `k`,
`d`, `a`, `e`, and the reorder pair — with `!is_text_step()`.

Acceptance evidence: a sweep that types each of `d e a j k` and the reorder keys
into a queue box in its text step and asserts **none** triggers an action, while
the same key outside the text step **does**. Proven non-vacuous by asserting the
guard's removal changes the result.

### Work package B — Add

Intent: let the user queue a prompt.

Required changes: an add step that captures text and commits on confirm;
mint a `Uuid` entry id; send `UserPromptQueueAdd`; apply the returned
projection; select the new entry. On a length/count/expiry refusal, show the
code and **keep the text** so the user can shorten it.

Acceptance evidence: a test adding from the M004 box and asserting the new row
appears in daemon position; a test proving two identical texts produce two
distinct entries; a test proving an over-length add is refused visibly and
retains the composed text.

### Work package C — Edit in place

Intent: correct a queued prompt without retyping the rest.

Required changes: `e` on the selected row enters a text step seeded with the
entry's current text; commit sends `UserPromptQueueEdit`; the response
projection is applied. `delivering` rows are refused by the daemon with
`user_prompt_queue_entry_delivering` and must be reported as such, distinctly
from a missing entry.

Acceptance evidence: a test asserting edit changes text while `id` and `ordinal`
are unchanged, and that the row keeps its position after the daemon re-reads.

### Work package D — Reorder

Intent: correct queue order.

Required changes: move the selected entry earlier or later. **Filter to
`status == "pending"` before building `entry_ids`**, then permute that subset.
On `OrderMismatch`, re-read once and report. Disabled, or refused with a clear
message, when the selection is `delivering`.

**WP-D must also settle edge (b) in §3 by test**, and record the answer:
- a `delivering` row's ordinal after a reorder, and
- where the entry lands if that delivery is subsequently released.

Acceptance evidence: a test that reorders with **no** delivering row and asserts
the resulting order; a test that reorders **with** a delivering row and asserts
the request succeeds and the delivering row does not collide (proving the band
offset); a test that a delivering row alone cannot be selected for reorder.

### Work package E — Copy, empty state, and documentation

Intent: the box should now describe everything it does, and nothing else.

Required changes: empty state names the add key (M004 deliberately did not);
`keymap_hint` names add, edit, delete, and reorder; re-run the host-first keymap
check; update `architecture/tui.md`, the `tui` skill, and `docs/tui.md`; update
the roadmap and registry.

Acceptance evidence: a test asserting the hint names exactly the handled keys,
and that no advertised key is answered by the host first.

## 8. Failure, cancellation, restart, and contention semantics

- **Add refused (unowned queue):** say the queue has no owner and that
  adoption is an explicit separate operation. Do not silently adopt, and do not
  hide the reason.
- **Add refused (bounds):** keep the composed text; show the code.
- **Edit refused (delivering):** keep the box and the selection; report.
- **Edit refused (stale row):** re-read; do not re-seed the text step from a row
  that no longer exists.
- **Reorder refused (`OrderMismatch`):** re-read **once**, then report. Never a
  retry loop.
- **Reorder with a delivering row:** succeeds against the pending subset; see
  edge (b).
- **Text step abandoned:** closing the box with unsaved text must not add, not
  edit, and not leave the buffer to be committed by a later open.
- **Two frontends:** the loser's reorder is refused by the permutation check
  rather than silently overwriting. Converge through a re-read.
- **Daemon restart:** order is daemon-owned and survives; nothing in the box
  persists.

## 9. Compatibility and migration

- **No migration.** No `STORAGE_LAYOUT_VERSION` change.
- **No protocol change.** M004's keys keep working; the new keys are additive
  inside a modal that predates no one, since `/queue` itself is new in M004.
- If M004 has not shipped to any frontend, the three keys may be added without a
  compatibility window; if it has, they must not shadow an existing key — which
  the M004 keymap test already constrains.
- No configuration, no feature flag, no removal criteria.

## 10. Required tests

Prefix every new test `user_prompt_queue_`.

### Focused unit tests

- `is_text_step()` gating: each action letter is inert in a text step and live
  outside it.
- Permutation construction filters `delivering` rows and preserves the pending
  relative order otherwise.
- Selection survives a rejected edit and a rejected reorder.
- Bounds refusals render their code and not their text.

### Integration tests

- Add, edit, and reorder each driven through the real key path on the M004 box,
  asserting the mutation issues exactly one request and applies the returned
  projection with no second read.
- `d`, `e`, `a`, `j`, `k`, and the reorder pair typed into a text step produce
  **text**, not actions.
- The reorder keys reach their action through `handle_key`, proven non-vacuous
  by asserting a key the box does not bind does nothing.
- The keymap hint names no key the host answers first (the `plan.rs:526` rule).

### Restart and recovery tests

- A stale mutation completion after the box closed mutates nothing.
- A re-read after `OrderMismatch` converges on the daemon's order.

### Contention and cancellation tests

- A reorder racing another frontend's add is refused, not merged.
- Reorder with a `delivering` row present succeeds and preserves uniqueness of
  ordinals.

### Security and negative tests

- `queue-add`, `queue-edit`, `queue-reorder` are never observer-allowed, proven
  by executing `is_observer_allowed_command`.
- An observer cannot open `/queue`, therefore cannot reach any of the three.
- Prompt text appears in no toast, no hint, and no `tracing` call.

### Migration and compatibility tests

- None required; assert `STORAGE_LAYOUT_VERSION` is unchanged.

## 11. Required verification commands

```bash
# narrow tests first — per target, never a zero-select filter
cargo test -p codegg-core --test user_prompt_queue
cargo test -p codegg-core --test user_prompt_queue_mutation
cargo test -p codegg-core --test user_prompt_queue_delivery
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

`CARGO_BUILD_JOBS=2`. No `--all-features`. Report only commands actually run,
with real counts; never claim a hosted CI run.

## 12. Documentation updates

- `architecture/tui.md` — the three new actions, the text-step rule, and the
  pending-only permutation rule.
- `.opencode/skills/tui/SKILL.md` — the `is_text_step()` pattern and the
  host-first keymap rule.
- `docs/tui.md` — user-facing guidance for add, edit, and reorder, and an
  explicit note that a reordered entry that was mid-delivery may resume at the
  end of the queue if its delivery is released.
- `plans/subsystems/user-prompt-queue-roadmap.md` and `plans/registry.md` —
  Milestone 5 status.

## 13. Acceptance criteria

1. `a` composes a prompt that is queued, never dispatched, and appears in the
   list in daemon position.
2. `e` replaces the selected entry's text, changing neither its id nor its
   position.
3. Reorder moves the selected entry earlier or later and the result survives a
   re-read and a daemon restart.
4. Reorder succeeds when a `delivering` row is present, because the permutation
   is built from pending rows only.
5. Typing `d`, `e`, `a`, `j`, `k`, or a reorder key inside a text step produces
   text and never an action.
6. Every advertised key works through the real key path, and the hint names no
   key the host answers first.
7. Closing the box with unsaved text adds nothing and edits nothing.
8. Bounds and delivery-state refusals are shown as codes, retain the composed
   text, and are distinct from one another.
9. Observers are refused the box and therefore all three actions; none of
   `queue-add`, `queue-edit`, `queue-reorder` is observer-allowed.
10. No prompt text appears in any toast, hint, diagnostic, or log.
11. No migration, no protocol change, no store change, and no daemon change.

## 14. Stop conditions

Stop and report rather than improvise when:

- **WP-D's edge (b) resolves such that a released delivering entry visibly jumps
  to the end of the queue and that appears to be wrong.** Fixing it changes
  `renumber_dense`, which is closed M002b/M003 behaviour and needs a maintainer
  decision, not a TUI workaround.
- The permutation check appears to require a protocol change (for example an
  `OrderMismatch` that cannot be distinguished from a genuine stale projection).
  Distinguishing them is a store/daemon question.
- An unowned queue appears to need an in-box repair. ADR-0037 requires adoption
  to stay explicit and audited.
- Implementing text capture appears to require a shared `App`-level
  `captures_text` predicate. Follow the `Connect` precedent instead.
- The reorder keys turn out to be intercepted by the host, which would mean
  `docks_as_viewer()` is doing something other than what
  `src/tui/components/component.rs:221-222` documents.
- Scope would pull in steering, timers, adoption, transfer, or dispatch-now.
- Any migration appears necessary. M005 takes none.

## 15. Closure evidence required

- The exact command list run, with real counts, and what was **not** run.
- The answer to WP-D edge (b), stated as an observed test result rather than an
  inference from the code.
- Proof the text-step sweep is non-vacuous: the assertion that fails when the
  guard is removed, observed.
- A statement that the queue is now authorable and correctable, and that
  send-now, timers, adoption, and transfer are still unreachable from the box.
- Confirmation that `STORAGE_LAYOUT_VERSION` and the migration set are unchanged.
- Residual risks, including the absent `reason` field and the missing shared
  chrome helper.

## 16. Handoff notes

- **Preserve the uncommitted M001–M003 tree and M004's.** Unstaged and
  unstashed by design. Do not stash, revert, reformat, or `git add -A`.
- M004 must be **closed**, not merely implemented, before this plan opens.
- Re-verify every §3 line number against the M004 tree; the store file has not
  moved since M003, but the TUI files will have.
- `CARGO_BUILD_JOBS=2`; limited free disk. No `--all-features`.
- Run tests per target. A filter selecting zero tests is a failure to report.
- Prove guards non-vacuous by injecting a violation and observing the failure.