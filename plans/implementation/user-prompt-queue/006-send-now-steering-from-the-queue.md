# User Prompt Queue Milestone 006 — `/queue` Send-Now: Steering the Running Turn

Status: **ready for handoff**

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the
uncommitted M001–M005 tree (61 porcelain entries, 17 untracked, at plan time).
Re-inspect before implementing; §3 records measured facts at this baseline only.

Source roadmap:
`plans/subsystems/user-prompt-queue-roadmap.md#milestone-6--send-now-steering-from-the-queue`

Long-term requirements:

- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (**accepted
  2026-10-18**, Option A) — **this milestone is authorized by it.** It names
  `plans/implementation/user-prompt-queue/006-*` explicitly.
- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted) — the
  queue stays the single durable home for user-authored text.
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (accepted) — its
  deliberate exclusion of steering from the owner binding is quoted verbatim in §3.
- `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` (accepted) — orthogonal;
  M003's turn delivery uses no steering channel.

Primary class: **capability**.

## 1. Objective

Make the existing, implemented, controller-gated `CoreRequest::TurnSteer` backend
reachable from the `/queue` box, so a user can inject a queued prompt into the turn
that is **running now** and be told honestly what happened — including every one of
the three backend error branches that have never before been product-reachable.

One bounded outcome: one new key on the existing box, one new request/response pair
on the frontend, honest retention on failure.

## 2. Why this milestone is ready

Every hard dependency is **discharged**, and each was checked rather than assumed:

| Dependency | State |
|---|---|
| M004 closure (`plans/closure/user-prompt-queue/004-status.md`) | closed — the box, selection, and close/requeue lifecycle exist |
| M005 closure (`plans/closure/user-prompt-queue/005-status.md`) | closed — the entry identity, the browsing/text step split, and the refusal vocabulary exist |
| ADR-0036 acceptance | **accepted 2026-10-18, Option A**, and it names this plan path |

There is **no interface dependency on M007**. The two are independent: send-now has
no timer, and timers have no steering. They do, however, **contend for one migration
number** — see §9.

The one decision ADR-0036 explicitly deferred to this milestone is made in §3, not
left open: *what happens to the entry after a successful steer*.

## 3. Current implementation evidence

### The backend exists, is authorized, and is unreachable

`CoreRequest::TurnSteer { session_id, turn_id, text }`
(`crates/codegg-protocol/src/core.rs:2398-2402`) is dispatched in
`src/core/daemon_turns.rs:1258-1310` and forwards to `handle.steer_tx` via
`try_send`. **No TUI code path issues it** — the grep for `TurnSteer` across `src/`
returns only the daemon arms, the authorization policy, and test fixtures.

The frontend already holds the turn identity this needs:
`app.session_state.active_turn_id` is an `Option<String>` that `turn_submit.rs`
already reads (`:449`, `:638`). M006 needs no new projection field to address a turn.

### The five distinguishable outcomes already exist as wire codes

| Code | Meaning | Class |
|---|---|---|
| `Ack` (`CoreResponse::Ack`) | accepted into the turn's steering channel | success |
| `steer_queue_full` | channel full — retry later, entry untouched | channel condition |
| `steer_unavailable` | receiver closed — the turn is finishing | channel condition |
| `steer_not_supported` | this turn has no steering channel at all | capability gap |
| `turn_id_mismatch` / `no_active_turn` | the addressed turn is not the active one | staleness |
| controller-predicate denial | observer or non-controller contributor | **authorization** |

ADR-0036 requires the authorization refusal to stay **distinct from a channel-full
failure**. Collapsing them into one "send failed" toast violates an accepted ADR, so
the mapping in §7 is one code → one distinct sentence.

### The decision ADR-0036 deferred to this milestone

ADR-0036, under *Neutral or deferred*, states the rule and the ownership:

> Whether a successfully steered entry remains visible in the queue after delivery
> is a UX decision for the milestone that implements the box, not a contract change
> here. It MUST NOT be deleted before delivery is acknowledged, and MUST NOT be
> re-sent without an explicit user action. **M004 and M005 must not decide this
> either way.**

**Decision: on `Ack`, the entry is marked `delivered`, bound to the turn that
accepted it — it is not left `pending` and it is not deleted.**

Reasoning, stated so a reviewer can disagree with it:

- Leaving it `pending` makes the **same instruction run twice** — once steered into
  the running turn, then again claimed by M003 as a fresh turn when the session frees
  up. ADR-0036's own decision drivers forbid exactly this: *"must not be silently
  duplicated if it was delivered."*
- The schema already has the right columns: `delivered_turn_id` and `delivered_at_ms`
  are populated by the existing `Delivered { turn_id }` transition.
- It satisfies both MUSTs: the entry is not deleted before acknowledgement, and it is
  not re-sent without an explicit user action.
- "Delivered" here means *delivered into turn T*, which `delivered_turn_id` states
  precisely. It does not claim the model read it, so it does not overstate the
  guarantee ADR-0036 fixed.

**Rejected alternative:** keep the entry visible with a new "steered" status. That
needs a new `status` value, and v80's `status` `CHECK` cannot be `ALTER`ed — SQLite
would force a rebuild of a durable user table to add one. Rejected on cost against a
benefit the `delivered_turn_id` already provides.

**Refusal path:** a refusal writes **nothing**. The entry stays `pending` at its
current position. This is ADR-0036's rule and needs no new state.

### The store transition does not exist yet

Every current path into a terminal or restored status is guarded on
`status = 'delivering'`: `resolve_stranded_delivery` (`:1607+`),
`release_delivery` (`:1530-1547`), `retry_failed` (`:1554-1571`). There is **no**
transition from `pending` to `delivered`, so M006 adds exactly one new store method,
guarded on `status = 'pending'` for the same at-most-once reason the rest are.

### `is_controller_bypass` is correctly not involved, and must stay that way

ADR-0037's exclusion is confirmed verbatim rather than assumed:

> Whether ADR-0036's steering path should require the same owner binding is out of
> scope here; `TurnSteer` keeps its existing controller predicate

Steering addresses a **running** turn, which has a live controller lease; ADR-0037
governs mutation **between** turns. M006 therefore routes send-now through the
existing turn controller predicate and **must not** route it through
`ControllerScope::QueueMutation` or the owner binding. Asking whether the caller owns
the *queue* is the wrong question here and would refuse a legitimate steering call.

### Two keys and one file layout, established

M005 shipped the box with a **structural** text-step guard: `handle_key` matches the
text step first as a separate match and returns, so the browsing arm is unreachable
while a composer is open. M006's key is added to the **browsing** arm and inherits
that protection for free — `s` typed into a composer produces an `s`.

Free letters in the box: `j k [ ] a e d r`. `s` is unbound **inside the box**. Whether
the host answers `s` first is **not** assumed — M005 proved that `QueueDialog`
overrides `docks_as_viewer()` to `true` and that the docking-steal branch is dead only
because `App::dock_target` has no `DialogType::Queue` arm. M006 must re-assert that
arm's absence rather than inherit the belief.

## 4. Invariants that must not regress

1. **The queue never executes anything.** Send-now is the only queue action that
   touches a turn, and it does so through `TurnSteer`, never `TurnSubmit`, never by
   cancelling a turn.
2. **A prompt is never lost.** Every refusal leaves the entry exactly where it was.
3. **Steering is best-effort and the UI says so.** No copy may use "force", "guarantee",
   or "always reaches".
4. **Authorization is distinct from a channel condition**, and both are distinct from
   staleness.
5. **Closing the box performs no action**, including cancelling an in-flight steer
   whose response has not arrived (the M005 `requeue_open_permission` shape).
6. **No prompt text in any toast, hint, log, or audit record** — only entry identity
   and a stable reason code.
7. **At-most-once per user action**: one key press issues one `TurnSteer`, and the
   entry is marked delivered at most once (guarded on `pending`).
8. **M005's text-step guard is not weakened** by adding a sixth browsing key.
9. `check_queue_delivery_callsites.py` still passes: send-now adds **no** third caller
   of `submit_queued_prompt_turn`.

## 5. Scope

### In scope

- One new key (`s`) in the box's browsing arm, advertised in the browsing hint.
- One new `CoreRequest`/`CoreResponse` variant pair that carries queue-entry identity
  plus turn identity to the daemon and returns a typed outcome — **the existing
  `TurnSteer` variant is not reused directly for the queue action**, because the
  frontend must also settle the entry, and a raw `TurnSteer` response cannot report
  the entry-level outcome. The daemon arm internally calls the same
  `try_send` path; there is still one steering channel.
- The new `pending → delivered` store transition and its wiring.
- Per-code presentation, one sentence each, with no text echoed.
- Docs and cross-references.

### Explicitly out of scope

- **No timer.** `run_at_ms` stays v82 for M007.
- **No change to the steering channel, its capacity, or the agent-loop follow-up
  drain.** Capacity and fairness policy remain open (roadmap §7 M006).
- **No new status value**, no migration, no table rebuild.
- **No auto-retry** of a steer (§8).
- **No adoption or transfer** control in the box — ADR-0037 requires those to stay
  explicit, audited operations outside it.
- **No desktop surface** (M008).
- **No change to M003's delivery path**, its pump, or its callsite guard.

## 6. Required production changes

### Core / domain

- `UserPromptQueueStore::mark_steer_delivered(entry_id, turn_id, now_ms)` — guarded
  on `status = 'pending'`, writing `delivered`, `delivered_turn_id`,
  `delivered_at_ms`, `failure_reason = NULL`; returns whether this call won
  (`rows_affected == 1`). Same at-most-once discipline as every other transition here.
- It runs through the existing `UNINSTRUMENTED_OPERATIONS` /
  `INSTRUMENTED_OPERATIONS` classification and the audit builder used by M003,
  recording **entry identity and outcome only**.

### Storage and migrations

**None.** `delivered_turn_id` and `delivered_at_ms` already exist from v80. This is a
recorded narrowing of the roadmap's expectation that send-now needs a schema change —
it does not.

### Protocol and DTOs

- One new request variant, e.g. `UserPromptQueueSendNow { session_id, entry_id }`, and
  one response carrying `Ack | Refused { code } | EntryNotFound`.
- Additive on the wire: a frontend that predates it never issues it; a daemon that
  predates it answers with an unmapped code, which the frontend renders as the M005
  verbatim-code fallback sentence rather than as a success.
- The daemon resolves `turn_id` from its **own** active-turn handle for the session
  rather than trusting a client-supplied id, then calls the existing
  `try_send` path.

### Runtime and concurrency

- Dispatch through the TUI command registry and `dispatch_tui_command`, never a direct
  call from the widget.
- A **fourth** `AsyncUiRequestState` slot, `queue_send_request`, alongside M005's three
  (`queue_add_request`, `queue_edit_request`, `queue_reorder_request`). Four separate
  slots so a delete racing a steer cannot cancel the steer.
- Slow work uses `spawn_tui_task` + `finish(request_id)` / `fail(request_id, err)`,
  with a stale-completion test, per the TUI skill's convention.
- `close_dialog`'s `Dialog::Queue` arm cancels `queue_send_request` too.
- Transport authority: route by `App::is_ws_attach_transport()` — **not** by app mode.
  `RemoteCore` covers both the WS-attach frontend and the socket-daemon frontend, and
  a mode-only gate drops the answer into an absent channel, orphaning the waiter.
  `scripts/check_tui_transport_authority.py` enforces this.

### Frontend surface

- `QueueDialog`: `s` in the browsing arm; hint text becomes
  `"j/k move · [ ] reorder · a add · e edit · d delete · s send now · r refresh"`,
  subject to the width check M005 already established.
- A send-now press on a `delivering` row is refused **locally**, as reorder already is:
  that row is already being delivered as a turn, so steering it would duplicate it.
  The refusal names the condition rather than issuing a doomed request.
- A send-now press with **no active turn** is refused locally with an honest message
  (the entry waits for the next turn — which is what the queue already does).

### Security and authorization

- The daemon keeps the existing controller predicate for the addressed turn. An
  observer is refused with the **authorization** presentation, not a channel one.
- The store transition re-verifies session ownership through the same
  `ControllerScope::QueueMutation`-shaped ownership check M002b established, so a
  caller who steered someone else's turn cannot mark their entry delivered.
- Prompt text is never logged, echoed, or put in an audit record. The steer request
  carries the text to the daemon — that is the mechanism — but no diagnostic,
  rejection sentence, or audit field contains it.

### Documentation and static guards

- `architecture/tui.md`, `.opencode/skills/tui/SKILL.md`, `docs/tui.md`: the key, the
  accepted guarantee, and the five outcomes.
- `architecture/core.md` + the `core` skill: the pending → delivered steer settlement
  and why send-now is not `TurnSubmit`.
- The agent-loop follow-up drain is cross-referenced wherever steering is described
  (ADR-0036 requires the cross-reference so neither reader infers the other's
  semantics).

## 7. Ordered work packages

### Work package A — The store transition

*Intent:* one guarded `pending → delivered` write.

*Required changes:* `mark_steer_delivered` plus its `LiveEntry` row mapping.

*Acceptance evidence:* a store test asserting the transition wins exactly once, refuses
a `delivering`/`failed`/`expired` row, sets both timestamp columns, and clears
`failure_reason`.

### Work package B — Protocol and daemon arm

*Intent:* one additive request that resolves the active turn server-side, steers, and
settles the entry on `Ack`.

*Required changes:* request/response variants; the daemon arm; the ownership
re-check; the audit record.

*Acceptance evidence:* a daemon test for `Ack`, for each of the three channel codes,
for the authorization refusal, and for an entry that is no longer `pending` by the
time the response lands (must not resurrect or double-settle).

### Work package C — Frontend wiring

*Intent:* `s` reaches the action and the answer reaches the user.

*Required changes:* `App` message types, input arms, `DialogState` slot, dispatch arm,
the local refusals, the hint.

*Acceptance evidence:* integration tests driving `s` through the **real key path**, on
both transports, with the toast text asserted per code.

### Work package D — Copy, docs, and the guard re-assertion

*Intent:* the box advertises only what it does, and says what steering guarantees.

*Required changes:* hint and toast copy; docs; the `DialogType::Queue` arm-absence
assertion.

*Acceptance evidence:* a test asserting the forbidden words ("force", "guarantee") appear
in no hint or toast; the M005 guard assertions still hold with a sixth key.

## 8. Failure, cancellation, restart, and contention semantics

- **Channel full / unavailable / unsupported:** the entry is untouched. The user is
  told which condition, in words that do not imply the message was lost.
- **Authorization refusal:** distinct presentation; the entry is untouched; the
  refusal never reads as a channel condition.
- **Staleness** (`turn_id_mismatch`, `no_active_turn`, or the turn ended between the
  key press and the response): the entry is untouched. It remains `pending` and will be
  delivered as the next turn — the honest outcome.
- **Lost response (transport error, outcome unknown):** the entry is left `pending`,
  the UI reports that the outcome is unknown, and **the steer is not retried
  automatically.** ADR-0036 requires reconciliation "by entry identity rather than by
  resubmission"; true at-most-once across a lost `Ack` needs a durable steer-attempt
  record, which is new durable surface and is **explicitly out of scope** (§14).
  The conservative consequence — the text may later be delivered as an ordinary turn —
  is reported, not hidden.
- **Cancel / restart:** cancelling the box cancels the request state; a completion
  landing afterwards writes nothing (the M005 `RefusalOutcome::Stale` shape, reused).
  A daemon restart cannot lose a settled entry: settlement is a durable write, not a
  frontend projection.
- **Contention:** two frontends racing the same entry — the store transition's
  `status = 'pending'` guard lets exactly one win.

## 9. Compatibility and migration

**No migration.** Nothing in §6 changes a schema; `STORAGE_LAYOUT_VERSION` stays **81**
and no v82 row is consumed.

M007's `run_at_ms` remains **v82**. If C001 (§10 of the roadmap's risk register) is
authorized before M007 lands, the two must agree on numbering: whichever lands first
takes v82 and the other becomes v83. That is a landing-order fact, not a planning
blocker, and both plans say so.

## 10. Required tests

### Focused unit tests

- Per-code message mapping: six outcomes → six distinct sentences, none containing
  the prompt text.
- Local refusals: `delivering` selection, and no-active-turn.
- The hint names `s` only in the browsing form, never in the text-step form.

### Integration tests (`tests/user_prompt_queue_box.rs`, extended fake transport)

- `s` on a pending row with a live turn → `Ack` → the entry leaves the live
  projection, the toast says accepted, and **no** second delivery is queued.
- each channel code → the entry is still listed, in the same position, with its
  distinct message.
- authorization refusal → message distinct from `steer_queue_full`.
- close-during-steer → nothing changes.
- stale completion after close → nothing changes (the M005 stale-guard shape, on the
  new slot).

### Transport tests

- Send-now on the WS-attach transport and on the socket-daemon transport.
- `check_tui_transport_authority.py` and `check_queue_delivery_callsites.py` both
  pass unchanged.

### Security tests

- Observer refused the box **and** send-now; the refusal is the authorization one.
- A principal who does not own the queue cannot settle the entry, even having
  addressed the turn.

### Guards proven non-vacuous

Every guard in this milestone must be shown to fail on an injected violation, as M004
and M005 did. Two are mandatory:

1. Invert the text-step arm so the browsing key is reachable during a composer — the
   test must fail with `s` acting instead of typing.
2. Delete the `queue_send_request` cancel in `close_dialog` — the stale-completion test
   must fail with the projection written.

A guard that has never rejected anything is a comment; report the injection, not the
pass.

## 11. Required verification commands

```bash
# narrow tests first — per target, never a zero-select filter
cargo test -p codegg --lib -- dialogs::queue commands::queue
cargo test --test user_prompt_queue_box

# guards
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_queue_delivery_callsites.py
python3 scripts/check_queue_delivery_callsites.py --self-test

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# canonical sanity
scripts/verify.sh quick
```

`CARGO_BUILD_JOBS=2`. Never `--all-features`.

## 12. Documentation updates

- `architecture/tui.md` — the send-now key, the five outcomes, the local refusals, and
  the M006 authoring subsection.
- `.opencode/skills/tui/SKILL.md` — the same, under the box's own section.
- `docs/tui.md` — a user-facing "Sending now" section stating the accepted guarantee
  in the user's terms, and updating the `/queue` key table and "What it still cannot
  do".
- `architecture/core.md` + `core` skill — the settlement transition and the
  authorization question steering asks.
- The follow-up-drain cross-reference ADR-0036 requires.

## 13. Acceptance criteria

1. Pressing `s` on a queued prompt while a turn runs injects that prompt into the
   running turn through `TurnSteer`, and nothing else.
2. On `Ack` the entry leaves the live queue, bound to the accepting turn, and is never
   delivered again as a new turn.
3. All three channel conditions plus staleness plus authorization each have a distinct
   user-visible outcome.
4. Every refusal leaves the entry `pending` at its exact prior position.
5. No copy anywhere claims delivery, force, or certainty.
6. The key works through the real key path on both attach transports, and
   `s` typed inside a composer produces an `s`.
7. An observer is refused, and the refusal is not reported as a channel condition.
8. Closing the box mid-steer changes nothing.
9. No prompt text appears in any toast, hint, diagnostic, or audit record.
10. Both guards in §10 are proven non-vacuous by injection, and the injections are
    reverted.
11. `scripts/verify.sh quick`, clippy `-D warnings`, and `cargo fmt --check` pass.

## 14. Stop conditions

Report rather than improvise if:

- **True at-most-once across a lost `Ack` is requested.** It needs a durable
  steer-attempt record (a new column, hence a migration). Out of scope; do not fake it
  with a client-side retry.
- **The `pending → delivered` settlement is judged to overstate the guarantee.** Then
  the alternative needs a new `status` value, which v80's `CHECK` cannot take without a
  table rebuild — report that cost instead of adding a parallel truth.
- **Send-now appears to require an owner-binding check.** It does not; §3 quotes
  ADR-0037's exclusion. Stop and re-read rather than widening authority.
- **Any second injection channel, or any cancel-and-resubmit, is tempting.** Both were
  rejected by ADR-0036; using either is a supersession, not an implementation choice.
- **Capacity or fairness on the agent-loop side comes up.** Deferred by the roadmap;
  do not widen into it.
- **A migration number is consumed.** Stop and reconcile v82/v83 with M007 and C001.

## 15. Closure evidence required

- Requirement-to-evidence matrix over the eleven acceptance criteria.
- Real command output with counts for every command in §11, labelled local.
- The two injection transcripts from §10.
- An explicit statement of whether any claim rests on an unrun test.
- A registry unblock audit: state what M006 unblocks (M008's interface edge) and what
  it does not.

## 16. Handoff notes

Start at work package A. The store transition is the only piece with no precedent in
this workstream; everything else is the M005 add/edit/reorder machinery applied to a
different request. The highest-risk part of this milestone is not the send — it is the
**copy**, because the temptation to tell the user their prompt "was sent" is exactly
the overstatement ADR-0036 exists to prevent.

Do not rename the agent-loop follow-up drain, and do not let "queue" and "steer" drift
into each other in any file you touch.