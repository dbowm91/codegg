# User Prompt Queue and Turn Contention Milestone 002b — Queue Mutation Operations and Mutation Authority

Status: implemented

Authority: `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md`
(**accepted** 2026-10-18, Option B). The blocker that held this milestone is
cleared; ADR-0037 is now the authority this plan implements against, and its
Decision section overrides anything here that conflicts with it.

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the M001
closure tree, **plus** M002a closed (its closure record will be written to
`plans/closure/user-prompt-queue/002a-status.md`; it does not exist yet). §3 is
anchored to the committed baseline and to M002a's landed v80 — re-inspect both at
implementation time.

Source roadmap: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones`
(Milestone 2b, mutation slice)

Long-term requirements:

- `plans/000-long-term-specification.md#6-canonical-identity-relationships`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (**accepted**
  2026-10-18 — Option B; this milestone's authority)
- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (**accepted**
  2026-10-18)
- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted)

Primary class: **infrastructure**

## 1. Objective

Add the four authorized queue mutation operations — add, edit, delete, and
reorder — on top of the durable, session-scoped queue-owner authority ADR-0037
decided, so that a queue can be authored **while its session is idle**.

Idle mutation is the point. It is the case ADR-0035's clause could not express and
the case ADR-0037 was written to make expressible. A mutation path that only works
while a turn holds the controller lease would not have satisfied either ADR.

## 2. Why this milestone is ready

ADR-0037 was **accepted** on 2026-10-18 selecting **Option B**: a durable,
session-scoped queue-owner binding, independent of turn state, surviving restart,
changed only by explicit audited transfer. The authority model is therefore
decided rather than open, which was the only thing blocking this milestone.

The finding that produced ADR-0037 remains the design's reason for existing, so it
is recorded here as standing context rather than as a blocker:

- `SessionControllerStore::acquire`
  (`crates/codegg-core/src/session_control.rs:461`) validates a non-empty
  `turn_id` via `validate_turn_id` (`:293`); `release` (`:780`) runs
  `DELETE FROM session_turn_controller WHERE session_id = ? AND turn_id = ?`. The
  lease row therefore exists only while a turn runs.
- `CoreDaemon::controller_lease_for` (`src/core/daemon_control.rs:85`) returns
  `None` when no row exists, and `check_turn_controller` (`:124`) then takes its
  `not_controller()` arm for **every** principal, the legitimate owner included.
- The one idle-passing path is the early return at `:74` when
  `is_controller_bypass(authority)` holds — `authorization::is_local_owner_broad`,
  an identity-only comparison against `LOCAL_OWNER_PRINCIPAL_ID` that is **not
  bound to a session**.

An implementer who reaches for `is_controller_bypass` to make an idle mutation
test pass has re-created the defect ADR-0037 rejects. The acceptance criterion that
catches it is criterion 9.

## 3. Current implementation evidence

Beyond M002a's evidence in
`002a-durable-queue-storage-and-read-projection.md#3-current-implementation-evidence`:

- **M002a landed v80** — `user_prompt_queue_entry`, its covering index on
  `(session_id, status, ordinal)`, the four bounds `CHECK`s, `UNIQUE (session_id,
  ordinal)`, and the lazy expiry sweep on the read path.
  `STORAGE_LAYOUT_VERSION` is 80. **v81 does not exist yet** and this milestone
  owns it.
- **Operation descriptors** — `crates/codegg-core/src/authorization/policy.rs`.
  `turn_submit`, `turn_steer`, `turn_cancel`, and `human_shell_promotion_stage` use
  `ScopeKind::ViaSession` with `Capability::AgentInvoke` (`:355-385`).
  `session_control_transfer` (`:1170`) is `ScopeKind::ViaSession` +
  `Capability::AgentInvoke` — **the exact capability ADR-0037 requires for queue
  ownership transfer** ("the same capability requirement as
  `session_control_transfer`"). M002a's `user_prompt_queue_list` is
  `ScopeKind::ViaSession` + `Capability::SessionRead`; reads keep `SessionRead`
  and are not changed here.
- **`SessionControlTransfer`'s shape is the transfer precedent** —
  `{ session_id, recipient_principal, expected_revision, reason }`. The
  `expected_revision` fence is what makes concurrent transfers converge on one
  winner, and it is reused rather than reinvented.
- **Audit classification** — mutating operations belong in
  `INSTRUMENTED_OPERATIONS` (`crates/codegg-core/src/audit_instrumentation.rs`).
  ADR-0037 fixes the action: every queue operation, **including ownership
  adoption and transfer**, reuses the existing `prompt_submit` shape ADR-0035
  selected. Note this differs from `session_control_transfer`, which maps to
  `membership_change` (`:665`) — the divergence is directed by ADR-0037, not an
  oversight, and must be recorded rather than "corrected".
  `scripts/check_audit_coverage.py` fails a mutating-capability operation in
  neither list.
- **Controller predicate** — `src/core/daemon_control.rs:124`. The precedent to
  follow is *generalising this one predicate*, not copying it. **Two divergent
  copies of the controller check are not acceptable.**
- **Turn-family registration** — `src/core/daemon_family.rs`,
  `src/core/daemon.rs` `session_id_for_request`, `src/core/daemon_turns.rs`.
- **Observer control** — `is_observer_allowed_command`
  (`src/tui/app/state/observe.rs`) is a fail-closed read-only allowlist. A
  mutating command is blocked by omission, which is the correct default and must
  stay that way.
- **Ordinal constraint** — M002a added `UNIQUE (session_id, ordinal)`. Reorder
  therefore **must** renumber collision-free; a direct renumber that can collide
  mid-transaction is not acceptable.

## 4. Invariants that must not regress

Everything in M002a §4, plus:

- **Two ownership notions sit adjacent and must stay distinguishable.** ADR-0037's
  Negative consequences require that every future reader can tell which question
  is being asked. The predicate's call shape names it explicitly (see §6).
- **The turn controller lease keeps its meaning for turns.** ADR-0007 is not
  modified by this milestone, and no queue rule changes turn-admission semantics.
- **`is_controller_bypass` gains no queue-mutation authority.** ADR-0037 rules
  this out explicitly.
- **An unauthorized mutation is refused, never silently applied**, and the
  refusal names the reason.
- **An unowned queue fails closed** — no authorized mutator — and only an explicit
  adoption operation repairs it.
- **Ordering is a daemon-owned total order**, enforced under transaction, not a
  last-writer-wins index patch.
- **Edit replaces text without changing position or identity.**
- **Bounds still refuse rather than truncate** on every mutation path, including
  edit.
- **Prompt text never reaches an audit record, receipt, diagnostic, or error
  message verbatim.**
- **No queue mutation acquires a scheduler permit, executor ownership, or
  workspace execution context.**
- **No mutation is a second submission path.** A queued entry becomes a turn only
  through ordinary `TurnSubmit` (ADR-0035); that is M003.

## 5. Scope

### In scope

- The four typed mutation operations: add, edit, delete, reorder.
- The **v81** additive migration creating the durable queue-owner binding.
- Queue adoption (including the implicit atomic adoption on first add) and
  explicit ownership transfer, both audited.
- The single generalized controller predicate.
- Protocol variants and DTOs for every operation above.
- `OperationDescriptor` entries with `ScopeKind::ViaSession` +
  `Capability::AgentInvoke` for mutations and transfer, `Capability::SessionRead`
  unchanged for reads.
- `INSTRUMENTED_OPERATIONS` entries mapping every mutating operation to the
  `prompt_submit` audit action.
- Daemon handlers for all of the above.
- Store-level implementations including collision-free reorder and dense
  renumber-on-delete.

### Explicitly out of scope

- Delivery into turns — M003.
- Any user-facing surface — M004/M005. No dialog, no `/queue` command, no row 0
  suffix, no composer affordance.
- `run_at_ms` and delayed delivery — M007, which is now **v82**, not v81.
- `TurnSteer` wiring and send-now — M006, gated on ADR-0036.
- Modifying ADR-0007, ADR-0035, or ADR-0037. If implementing the accepted
  decision appears to require changing one, that is a supersession decision for
  the maintainer, not an implementation edit.

## 6. Required production changes

**ADR-0037 is accepted and is the authority. This section implements Option B.**

### Core/domain — the ownership substrate

Migration **v81** creates the durable binding additively, and
`STORAGE_LAYOUT_VERSION` moves 80 → 81:

```sql
CREATE TABLE IF NOT EXISTS user_prompt_queue_owner (
    session_id TEXT PRIMARY KEY,
    owner_principal TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    adopted_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
```

`codegg-core` gains an owner store beside `UserPromptQueueStore`, or the owner
operations join that store if that produces the smaller, clearer surface — either
is acceptable, but the owner row must be readable and writable in the same
transaction as the mutation it authorizes, because **authorization and mutation
must not be separable by a crash**.

Ownership state is a three-valued fact, and the three values are **not**
interchangeable:

| State | Meaning | Who may mutate |
|---|---|---|
| `Absent` | no owner row **and** no queue entries — the queue has never existed | nobody; the first `add` creates both, atomically |
| `Owned(p)` | an owner row names `p` | `p`, subject to the turn-controller term |
| `Unowned` | queue entries exist but **no** owner row | nobody — this is an invalid state |

**The adoption policy this milestone decides** (ADR-0037 deferred it here). The
first `add` for a session in state `Absent` is **one transaction** that inserts the
owner row and the entry together, with the calling principal as owner. This is not
optional policy — without it the feature is unreachable, because a queue in `Absent`
has no authorized mutator and therefore nothing could ever create the first entry.

The distinction from `Unowned` is load-bearing and is what honors ADR-0037's rule
that *only an explicit adoption operation repairs an unowned queue*:

- `Absent` → first `add` **adopts and inserts** in one transaction. This is
  creation, not repair.
- `Unowned` (entries exist, owner row missing) → **every** mutation including
  `add` is refused with `QueueUnowned`. First-add must **not** silently adopt here,
  or an invalidated queue would quietly become owned by whoever happened to touch
  it next — which is exactly the silent widening ADR-0037 exists to prevent.
- Only the explicit `UserPromptQueueAdopt` operation converts `Unowned` to
  `Owned`.

`Unowned` is unreachable through this milestone's own code paths, because the only
path that creates entries also creates the owner atomically. It is therefore a
**defensive** state whose repair path is required by the ADR rather than by an
observed defect; it is tested by constructing the state directly.

### Core/domain — the single predicate

Generalise `check_turn_controller` (`src/core/daemon_control.rs:124`) into one
predicate whose call shape **names the question being asked**, because ADR-0037's
Negative consequences require that distinction to be visible at every call site:

```rust
enum ControllerScope<'a> {
    /// Turn admission/cancel/steer: the turn-scoped controller lease.
    Turn(&'a str),
    /// Queue mutation: the durable session-scoped queue owner.
    QueueMutation { session_id: &'a str },
}
```

This is deliberately **not** the `Option<&str>` shape a blocked draft suggested:
`None` cannot distinguish "a queue mutation on an unowned queue" from "a turn
operation with no turn", and those two states require different refusals.

Turn-scoped callers pass `ControllerScope::Turn(turn_id)` and keep their **exact**
current behaviour, proven by their existing authorization tests passing unmodified.

Mid-turn, `ControllerScope::QueueMutation` requires the caller to be the queue
owner **and**, when a turn lease row exists, to be that turn's controller. This is
the sharp edge ADR-0037 names, and its rule is fail-closed with no precedence
order:

| Queue owner | Turn controller | Result |
|---|---|---|
| caller | caller | allowed |
| caller | someone else | refused — `ControllerMismatch`, naming both |
| someone else | caller | refused — `NotQueueOwner` |
| someone else | someone else | refused — `NotQueueOwner` |

When no lease row exists (idle), the turn-controller term is simply absent and the
queue-owner term alone decides. An unowned queue yields `QueueUnowned`.

### Storage and mutations

- **Add** — client-supplied `entry_id` as `PRIMARY KEY`, so a duplicate retry
  converges on the existing entry instead of creating a second one. This is
  turn-admission's at-most-once discipline applied to identity, and it deliberately
  adds **no** receipt namespace, which ADR-0035 forbids. Append at
  `max(ordinal) + 1` inside the transaction that adopts ownership when needed.
- **Edit** — replaces `text` and `text_bytes` only. `id` and `ordinal` are
  unchanged. Bounds are re-checked; an over-bound edit is refused, never truncated.
  An edit against an expired entry is refused and must not resurrect it.
- **Delete** — removes the entry, then renumbers densely so the queue stays a
  dense total order.
- **Reorder** — its own operation, never an index patch smuggled through another
  request (ADR-0035 forbids reusing a request whose semantics differ). Because of
  `UNIQUE (session_id, ordinal)` it must renumber **collision-free** in one
  transaction. The strategy: first displace every affected row out of the target
  band with a single offset well above `USER_PROMPT_QUEUE_MAX_PENDING`
  (`UPDATE … SET ordinal = ordinal + 1000000 WHERE session_id = ?`), then write
  the dense target ordinals. A band offset is used rather than negative sentinels
  because M002a's `CHECK (ordinal >= 0)` forbids them. Every mutation runs in one
  transaction.

### Protocol and DTOs

`UserPromptQueueAdd`, `UserPromptQueueEdit`, `UserPromptQueueDelete`,
`UserPromptQueueReorder`, `UserPromptQueueAdopt`, and
`UserPromptQueueTransfer`, each with a typed response carrying the resulting
projection or a typed error. `UserPromptQueueTransfer` mirrors
`SessionControlTransfer`'s `{ session_id, recipient_principal, expected_revision,
reason }` shape, and `expected_revision` is load-bearing: it is the fence that makes
two concurrent transfers converge on exactly one winner.

Every new DTO field is `#[serde(default)]`.

### Runtime and concurrency

- Registration in `session_id_for_request`, the family map (all six are
  `Self::Turns`), and the turn dispatcher.
- `OperationDescriptor` per operation: `ScopeKind::ViaSession` +
  `Capability::AgentInvoke` for the five mutations and transfer. Reads stay
  `Capability::SessionRead`.
- `INSTRUMENTED_OPERATIONS` entries mapping each to `prompt_submit`, including
  adoption and transfer, per ADR-0037.
- Audit records reference entry identity and owner identity, never entry text.

### Frontend or operator surface

**None.** No command, no dialog, no key, no status change, no projection field.
The mutation surface is protocol- and store-level only. If this milestone ships
something a user can see or press, it has taken M004's scope.

### Security and authorization

- Authority is the durable owner binding, never the identity bypass.
- Every refusal is typed and names its reason.
- **Queue-absent versus not-yours.** An unauthorized caller must not be able to
  enumerate which sessions have queues. `Absent` returns `NotFound`; `Owned` by
  another principal returns `NotQueueOwner`; the two are distinguishable to a
  caller who already holds `AgentInvoke` on the session. This is permitted by
  ADR-0037, which scopes the refusal rather than requiring indistinguishability,
  and it is recorded here as the decision rather than left implicit. Note the
  refusal carries **no prompt text** and **no owner identity** for a
  non-owner — naming the owner to a refused caller would itself be a leak.
- The owner row is deleted by the same `ON DELETE CASCADE` that removes the
  session, so a deleted session cannot leave a dangling authorization behind.

### Documentation and static guards

`architecture/core.md` (the mutation contract and authority, including an
explicit statement that two ownership notions exist and which is which),
`architecture/authorization.md` (descriptor rows, the accepted ADR-0037 decision,
and the deliberate `prompt_submit`-not-`membership_change` divergence from
`session_control_transfer`), `architecture/session.md` (the v81 table), and the
roadmap's cross-references. A guard test asserting that no observer-allowed
command can reach a mutation handler.

## 7. Ordered work packages

### Work package A — Confirm the accepted authority model

Intent: verify ADR-0037 is `accepted` and re-read its Decision section before
writing code.

Required changes: none.

Acceptance evidence: the ADR's status line is `accepted`, its Option B is the
recorded decision, and the implemented authority matches it. **If the ADR is not
`accepted`, this milestone does not start.**

### Work package B — Ownership substrate (v81, adoption, transfer)

Intent: make the authority durable and changeable explicitly, before any mutation
depends on it.

Required changes: `migrate_v81` + dispatch arm + `STORAGE_LAYOUT_VERSION = 81`;
the owner store; the three-valued ownership state; implicit atomic adoption on
first add; explicit adoption and revision-fenced transfer.

Acceptance evidence: adoption on a session with no queue; a transfer test proving
two concurrent transfers converge on exactly one winner via `expected_revision`; a
restart test proving ownership survives unchanged; a test proving an unowned queue
is refused rather than implicitly owned by the session creator **or** the local
interactive owner.

### Work package C — Single controller predicate

Intent: one predicate, two named call shapes, no second copy.

Required changes: the `ControllerScope` generalisation of
`check_turn_controller`; route existing callers through it unchanged.

Acceptance evidence: the existing `TurnCancel` and `TurnSteer` authorization tests
pass **unmodified**; the four-row mismatch table in §6 is each covered by a test
naming its exact typed refusal; the idle case is covered with no lease row present.

### Work package D — Store mutations

Intent: add, edit, delete, and collision-free reorder, each transactional.

Required changes: the four store operations; dense renumber on delete; the
band-offset renumber on reorder.

Acceptance evidence: per-operation round-trip tests; a reorder test proving a
dense total order rather than a patch; a reorder test that starts from a
deliberately colliding intermediate state; a concurrent-reorder test proving one
serialised outcome; an edit test proving position and identity are unchanged; a
bounds test proving edit refuses rather than truncates; a duplicate-add test
proving convergence rather than a second row.

### Work package E — Protocol, descriptors, audit, handlers

Intent: expose all six operations through the existing authorized and audited
seams.

Required changes: the six request variants and responses; `session_id_for_request`;
the family map; dispatcher arms; six `OperationDescriptor` entries; six
`INSTRUMENTED_OPERATIONS` entries.

Acceptance evidence: per-operation authorization tests for owner, non-owner,
non-controller contributor, observer, and unrelated session, **idle and mid-turn**;
audit tests proving identity is recorded and text is not;
`scripts/check_audit_coverage.py` green; a test proving the absence of an
owner-identity leak in a `NotQueueOwner` refusal.

## 8. Failure, cancellation, restart, and contention semantics

- **Restart:** entries and ownership survive unchanged; a restart cannot widen or
  narrow the mutator set.
- **Concurrent mutation:** concurrent reorders and add/delete interleave into one
  serialised outcome under transaction; no interleaved order is observable.
- **Transfer racing a mutation:** converges on one winner via `expected_revision`;
  never leaves a queue with two owners.
- **Concurrent transfers:** exactly one succeeds; the loser is refused by name.
- **Unauthorized mutation:** refused with a typed reason and **no partial write** —
  authorization is evaluated inside the same transaction that writes.
- **Edit racing expiry:** the edit either wins on a live entry or is refused
  against an expired one. It must never resurrect an expired entry.
- **Cancellation:** a cancelled frontend operation leaves no partial mutation.
- **Duplicate add:** an identical retry converges on the existing entry.
- **Crash mid-adoption:** adoption and first add share a transaction, so a crash
  leaves neither or both — never an owner with no queue or a queue with no owner.

## 9. Compatibility and migration

- **v81 is additive** and owns the owner table only.
  `STORAGE_LAYOUT_VERSION` moves 80 → 81 under both existing assertions
  (`scripts/check_project_catalog_invariants.py`, `tests/storage_migrations.rs`).
- **Never edit a shipped migration.** v80 is closed; the owner table goes in v81
  exactly as ADR-0037's corrected Compatibility section states.
- Protocol additions are new variants, additive on the wire. A client that
  predates them never issues them, so no negotiation is needed.
- `#[serde(default)]` on every new projection field; an absent projection reads
  as an empty queue.
- An unowned queue fails closed — no authorized mutator — rather than open.
- **M002a's inherited finding stands.** `delivering`, `delivered`, and `failed`
  remain unreachable in the `status` `CHECK` vocabulary until M003. This milestone
  does not widen or rebuild that constraint; it records that the option is still
  M003's, and that SQLite cannot `ALTER` a `CHECK`.

## 10. Required tests

Name every new test with a `user_prompt_queue` prefix so the §11 filters select
real tests (a `user_prompt_queue_` prefix is the natural fit here).

### Focused unit tests

- Each store mutation's success and each typed failure.
- Each ownership state transition: adopt, transfer, unowned refusal.
- Reorder renumbering from a deliberately colliding intermediate state.

### Integration tests

- Per-operation round-trip through the store.
- Per-operation authorization: owner, non-owner, non-controller contributor,
  observer, unrelated session — **idle and mid-turn**, plus the full four-row
  mismatch table from §6.
- Audit record shape for each mutating operation, including adoption and transfer.
- Duplicate-add convergence.

### Restart and recovery tests

- Entries and ownership survive restart.
- Transfer survives restart.
- Expiry does not resurrect on edit.
- Crash-equivalent: adoption and first add are all-or-nothing.

### Contention and cancellation tests

- Concurrent add/delete/reorder converge on one order.
- Concurrent transfers converge on one winner.
- Cancel mid-mutation leaves no partial state.

### Security and negative tests

- Prompt text absent from every audit record and error message.
- Bounds refuse on every path, including edit.
- No observer-allowed command reaches a mutation handler.
- `is_controller_bypass` unchanged and carrying no queue authority.
- No owner identity leaks into a `NotQueueOwner` refusal.

### Migration and compatibility tests

- `STORAGE_LAYOUT_VERSION == 81` equals the highest wired migration.
- v80 → v81 migration leaves existing queue rows intact.
- Both layout assertions and the guard `--self-test` pass.

## 11. Required verification commands

```bash
cargo fmt

# new mutation tests (TEST-NAME filter, not a module path)
cargo test -p codegg-core user_prompt_queue

# M002a's storage slice must still be green
cargo test -p codegg-core --test user_prompt_queue

# turn-admission authority must be unchanged by the predicate refactor
cargo test -p codegg-core session_control
cargo test -p codegg --test turn_submit

# migration and layout assertions
cargo test --test storage_migrations

# audit and layout guards
python3 scripts/check_audit_coverage.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_project_catalog_invariants.py --self-test
bash scripts/check-core-boundary.sh

# lint
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy -p codegg --all-targets -- -D warnings

# canonical sweep
scripts/verify.sh quick
```

`CARGO_BUILD_JOBS=2`; use background execution for long sweeps. Never
`--all-features`.

**Report only commands actually run, with real counts.** If a filter selects zero
tests, say so and state what covered the area instead. Do not claim hosted CI.

## 12. Documentation updates

- `architecture/core.md` — the mutation contract, the accepted authority model,
  and an explicit "two ownership notions, here is which is which" statement.
- `architecture/authorization.md` — the descriptor rows, the accepted ADR-0037
  decision, and the deliberate `prompt_submit`-not-`membership_change`
  divergence from `session_control_transfer`.
- `architecture/session.md` — the v81 owner table and its revision fence.
- No `docs/tui.md` change; nothing here is user-visible.

## 13. Acceptance criteria

1. A queued entry can be added, edited, deleted, and reordered **while the
   session is idle**, with no active turn and no lease row — the case that
   blocked this milestone and ADR-0037's headline requirement.
2. An unauthorized principal is refused for **every** mutation, idle and
   mid-turn, with a typed reason and no partial write.
3. Mid-turn, all four rows of the §6 mismatch table behave exactly as specified;
   an owner/controller mismatch is a typed refusal naming the mismatch, not a
   silent precedence rule.
4. Reorder produces a dense daemon-owned total order under transaction, and two
   concurrent reorders converge on one outcome.
5. Edit replaces text without changing position or identity, and refuses rather
   than truncating; it never resurrects an expired entry.
6. Every mutating operation, including adoption and transfer, is audited with
   entry and owner identity and no prompt text; `check_audit_coverage.py` passes.
7. Ownership survives daemon restart unchanged, and two concurrent transfers
   converge on exactly one winner.
8. `TurnCancel` and `TurnSteer` authorization tests pass **unmodified**.
9. `is_controller_bypass` is unchanged and gains no queue authority — proven by a
   test, not by inspection.
10. v80's queue rows survive v81 intact, and
    `STORAGE_LAYOUT_VERSION == 81` equals the highest wired migration.
11. **No user-visible surface exists**, and the closure record says so.

## 14. Stop conditions

Stop and report rather than improvise when:

- **ADR-0037 is not `accepted`.** It is, but re-verify; a later supersession
  would invalidate this plan.
- Implementing the authority appears to require changing turn-admission
  semantics. That would re-open accepted ADR-0007/ADR-0035 and is a maintainer
  decision.
- Reorder cannot be made collision-free under `UNIQUE (session_id, ordinal)`
  without weakening it. Report; do not drop the constraint.
- A refusal would have to disclose an owner identity or distinguish "absent" from
  "not yours" in a way ADR-0037 does not sanction.
- M003's status-vocabulary inheritance turns out to force a `CHECK` rebuild
  *during this milestone*. Report it; do not rebuild a table outside its milestone.
- The work would need a second submission path or any scheduler interaction.
- Scope would expand into M003, M004, M005, M006, or M007.

## 15. Closure evidence required

1. Proof ADR-0037 was `accepted`, and its Option B recorded, before work began.
2. Per-operation authorization results for all five principal classes, **idle and
   mid-turn**, plus the full four-row mismatch table.
3. The idle-session test ADR-0037 was written to enable, stated as the
   milestone's headline evidence.
4. Reorder total-order, collision-free renumber, and concurrency results.
5. Edit identity/position, bounds-refusal, and expiry-non-resurrection results.
6. Audit records showing identity and no text, for all six operations.
7. Restart, adoption atomicity, and transfer-convergence results.
8. `TurnCancel`/`TurnSteer` tests passing unmodified.
9. `is_controller_bypass` unchanged, with the test that proves it.
10. Guard, clippy, and `verify.sh quick` results; hosted CI explicitly not claimed
    unless run.
11. The unresolved-finding list, including M002a's inherited M003 `CHECK`-vocabulary
    decision and the now-**v82** assignment for `run_at_ms`.
12. **An explicit statement that this milestone delivers no user-visible
    capability**; the next milestone is M003.
13. The unblock audit against the registry's Blocked work section.

## 16. Handoff notes

- **ADR-0037 is accepted; this milestone is authorized.** The ADR-0030/ADR-0034
  "do not start on a proposed ADR" rule no longer applies here — but it still
  applies to ADR-0036, which gates M006.
- **Do not reach for `is_controller_bypass`.** ADR-0037 exists because the bypass
  is identity-only and session-unbound. An idle mutation test that only passes via
  the bypass is a failed test, not a convenient one.
- **The single-predicate rule is the point of this milestone.** Two copies of the
  controller check would drift, and drift here means either a security hole or a
  lockout.
- **Two ownership notions are now real.** Document which is which at every seam.
- **No UI.** Anything a user can press belongs to M004/M005.
- **Preserved work.** M001's and M002a's trees may be uncommitted. Do not stash,
  revert, or reformat them.
- **Build cost.** `CARGO_BUILD_JOBS=2`; background long sweeps.