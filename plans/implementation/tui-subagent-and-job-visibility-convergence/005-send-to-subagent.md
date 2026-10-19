# TUI Subagent and Job Visibility Convergence Milestone 005 — Send-to-Subagent

Status: **ready for handoff**

Repository baseline: `e8a6b5ca2` plus the **uncommitted working tree**
(C001–C005 implementation). This tree is not committed; the line numbers below
were read against it and are load-bearing for this plan, not decoration.

Source roadmap:

- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`
  §7 (C007), §5 (D-C5), §6 (dependency graph)

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0040-subagent-run-addressing-and-human-interaction-authority.md`
  (**accepted 2026-10-18, Option A**) — decides *whom* a human may address.
- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (**accepted**) —
  fixes what `TurnSteer` means and guarantees. **Not relaxed here.**
- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (**accepted**) — owns
  who may cancel or interrupt a turn.
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (**accepted**) —
  governs queue mutation *between* turns.

Primary class: **capability** (K6)

> Milestone/numbering note: this subsystem's documents are numbered by
> sequence, not by milestone id — `001` is C001, `002` is C002, `003` is C005.
> This plan is **`005`** and implements **C007**. There is no plan `004`: C004 was
> implemented without one (governance gap D-8) and is closed by
> `plans/closure/tui-subagent-and-job-visibility-convergence/004-status.md`
> directly against the addendum's boundary.

## 1. Objective

One bounded outcome: while a named, running subagent's tab is focused, a user can
compose and send a reply **addressed to that subagent**, and the reply is
delivered to the **controlling parent turn** as an already-authorized `TurnSteer`
whose text names the subagent as the addressee — with the UI stating plainly that
it was routed to the controlling agent, not delivered to the subagent.

This is ADR-0040 Option A and nothing more. It does not make a subagent's own
run steerable.

## 2. Why this milestone is ready

Closed hard dependencies:

- **ADR-0040 — accepted 2026-10-18, Option A.** The decision blocker is gone.
  Acceptance authorized the *design*; this plan is the implementation work it
  left behind.
- **C005 — closed** (`plans/closure/.../003-status.md`). Its hard dependency was
  "a name worth addressing, with its descriptor beside it". C005 put `profile`
  and `usefulness` on the same row as `display_name`, and C004's closure review
  confirmed the additive-field chain that carries them is intact.
- **C001 — closed.** Tabs exist, `Ctrl+T` reaches the work, and D-C9 established
  the precedent that a surface must work in the mode it is opened from.
- **C002 — closed.** `/sessions` lists the live subagents, so the addressee can
  also be armed from a list rather than only from an already-open tab.

Stable interface dependencies:

- `CoreRequest::TurnSteer` exists, is authorized, is dispatched, and is audited.
  No new protocol surface is required.

## 3. Current implementation evidence

**The send path exists and is complete — only the TUI issuer is missing.**

| Link | Location | State |
|---|---|---|
| Request | `crates/codegg-protocol/src/core.rs:2482` `CoreRequest::TurnSteer { session_id, turn_id, text }` | present |
| Authorization | `crates/codegg-core/src/authorization/policy.rs:442` → `turn_steer`, `ScopeKind::ViaSession`, `Capability::AgentInvoke` | present |
| Dispatch | `src/core/daemon_turns.rs:1283` | present |
| Controller predicate | same arm, `check_turn_controller(&session_id, &turn_id, …)` — shared with cancel | present |
| Channel codes | `src/core/daemon_turns.rs:2674` `steer_channel_code` → `steer_unavailable` / `steer_queue_full` | present |
| Audit | `src/core/daemon.rs:3302` sets `chain.turn_id` for `TurnSteer` | present |
| Request family | `src/core/daemon_family.rs:138` → `Self::Turns` | present |
| **TUI issuer** | — | **zero. `grep -rn TurnSteer src/tui/` returns nothing.** |

**The frontend has never sent a steer, but it already owns the vocabulary for
what happens when one fails.** The user-prompt queue's M006 send-now
(`CoreRequest::UserPromptQueueSendNow`, `core.rs:2474`) pushes into the *same*
steering channel and **is** issued by the TUI (`src/tui/commands/queue.rs:774`).
Its refusal vocabulary in `src/tui/commands/queue.rs:68-77` and `:118+` already
maps every relevant code to a user-facing sentence — `steer_queue_full`,
`steer_unavailable`, `steer_not_supported`, `turn_id_mismatch`,
`session_control_not_controller`, `session_control_conflict`, `session_not_found`.
`steer_channel_code` is deliberately shared by both paths so they cannot drift.

**The composer cannot express an addressee.** `InputTarget`
(`src/tui/panels/mod.rs:1075-1083`) has exactly `Build` and `Chat`; the badge is
rendered from it at `src/tui/app/render.rs:1940-1957`.

**A subagent tab already holds everything needed to address one.**
`SubagentTabState` (`src/tui/app/state/subagent_tabs.rs:123-137`) carries
`row: SubagentRow` — which has `display_name` and `run_id` — plus a `liveness`
discriminator whose `is_running()` (`:157-162`) is **false for a detached tab**.

**The TUI knows the controlling turn id.** `session_state.active_turn_id:
Option<String>` is maintained by the turn lifecycle in `src/tui/app/mod.rs`.

## 4. Invariants that must not regress

- **I5 — a subagent tab cancels nothing.** Addressing a run must not become
  cancellation by another name. No path added here may cancel, interrupt, or
  evict anything.
- **I1 — projection-derived rows.** The addressee is captured from a projected
  `SubagentRow`. No client-side pool lookup, no name minting, no second
  derivation. The pool-isolation guard
  (`scripts/check_tui_subagent_name_pool_isolation.py`, added by C005) must stay
  green: the name travels **on the row**, never by resolving it at send time.
- **ADR-0036 is unchanged.** No relaxation of the steering guarantee, no new
  failure mode, no new retry.
- **No new authority.** The reply is delivered under the parent turn's existing
  controller predicate. Nothing previously refused becomes reachable.
- **`InputTarget` does not grow a variant** (ADR-0040, *Neutral or deferred*).
  The addressee is **data on the composer**, not a focus mode.
- **`check_execution_ownership.py` and `check_scheduler_bypass.py` stay green and
  `docs/execution-ownership.toml` is unchanged** — a reply is not a new execution
  surface.
- **The working strip row 0 stays reserved** and remains the main buffer's only
  status surface (unchanged by this milestone; recorded so it is not borrowed).

## 5. Scope

### In scope

- A composer **addressee** that is captured from the focused subagent's row at
  arm time and is visibly displayed while armed.
- Issuing `CoreRequest::TurnSteer` for the **controlling parent turn**, with the
  subagent's captured name as an explicit addressee in the text.
- Honest reporting of ADR-0036's three failure modes plus the controller and
  mismatch refusals, reusing the existing queue sentence vocabulary.
- A dismissal path and the guards that keep the armed addressee from outliving
  its subject.

### Out of scope

- **Option B.** No `CoreRequest::SubagentSteer`, no per-run steering channel, no
  new `OperationDescriptor`, no third `InputTarget`. Deferred behind ADR-0040's
  four recorded conditions, not rejected.
- **Reaching a subagent whose parent turn has ended.** Accepted Option A cost.
  It must be *reported*, not worked around.
- **New chords.** This milestone binds no key. If arming needs a key, it must be
  declared in help on the surface the user is already looking at and must pass
  the I6 guard (`documented_keys_resolve_in_the_active_map`); inventing a bare
  `Ctrl+<letter>` is prohibited by I3.
- **Cancellation, worktree, or scheduler changes.**
- **Durable subagent names.** D1 still holds; no migration, no
  `STORAGE_LAYOUT_VERSION` churn.
- **Desktop bridge parity.** `SubagentSummaryView` already omits the row's
  identity fields; widening it is a separate milestone. If this plan's fields
  reach the bridge, the parity decision must be recorded — see WP-5.

## 6. Required production changes

- **Frontend state** — an addressee on the composer, holding the captured
  `display_name` and the `task_id` it came from. It must be invalidatable: when
  the originating row leaves the projection, the addressee is cleared rather
  than left armed against a stale name.
- **Frontend send path** — when an addressee is armed, submit a `TurnSteer`
  carrying `session_id` = the bound session, `turn_id` = the **controlling parent
  turn**, and `text` = the composed body prefixed with the addressee. It must not
  issue `TurnSubmit` for an addressed reply.
- **Frontend display** — the armed addressee is shown next to the existing
  `[NOR]`/`[INS]`/`BUILD` badges, and the accepted outcome must say the reply was
  handed to the controlling agent.
- **No daemon change.** `TurnSteer` is complete. If implementation reveals the
  daemon cannot express something ADR-0040 assumed, that is a **stop condition**,
  not an invitation to extend the daemon surface.

## 7. Ordered work packages

### WP-1 — Addressee state and invalidation (no send path yet)

Model the addressee as plain composer data. Capture it from the focused row at
arm time. Establish the invalidation rule first, because it is the rule every
later package depends on: **an addressee armed against a row the projection no
longer reports is cleared, and a detached tab never arms one**
(`SubagentTabState::is_running()` is already false when detached).

*Acceptance:* an addressee survives a re-render and a snapshot refresh that still
reports the row; it is cleared when the row goes terminal or detaches; a
detached tab cannot arm one.

### WP-2 — The send path

Issue `TurnSteer` for the controlling parent turn. Record the three facts this
must get right, because each has a recorded failure:

1. **`turn_id` is the parent's, never the subagent's.** `TurnSteer` is keyed on a
   *session turn*. A subagent's `SubagentRow.task_id` is a daemon-local `u64`
   rendered in decimal and its `run_id` is a UUID — neither is the controlling
   turn id. The parent's roadmap already records a dead join between the `u64`
   and the UUID; do not resurrect it here.
2. **The frontend must supply `turn_id`, so the race is real.** Unlike M006's
   send-now — which deliberately sends *no* turn id and lets the daemon resolve
   it (`src/tui/commands/queue.rs:743-748`) — `TurnSteer` takes the caller's id
   and returns `turn_id_mismatch` when it is stale. Read
   `session_state.active_turn_id`, and surface the mismatch.
3. **An addressed reply must not fall through to `TurnSubmit`.** The two differ in
   authority and in what the user is told; routing one as the other is the defect.

*Acceptance:* an addressed send issues exactly one `TurnSteer` for the parent
turn; an unarmed send is byte-identical to today's `TurnSubmit`; a stale turn id
reports `turn_id_mismatch`.

### WP-3 — Honest reporting

Reuse the `send_now_outcome_message` vocabulary; do not write a second, thinner
one. Every code gets a sentence, and the **accepted** sentence must say the reply
was handed to the controlling agent — never "sent to Sally". ADR-0040's
*Failure modes* require this explicitly: the UI must not present the reply as
delivered to the subagent.

*Acceptance:* each of `steer_queue_full`, `steer_unavailable`,
`steer_not_supported`, `turn_id_mismatch`, `session_control_not_controller`,
`session_control_conflict`, and `session_not_found` has a distinct user-facing
sentence; the accepted sentence names the parent as the recipient.

### WP-4 — Guards and tests

Extend the existing chain suites rather than adding parallel ones (§10).

### WP-5 — Comments, docs, and the parity decision

Correct any prose that claims a subagent tab is read-only, in the **generator
first** where applicable (C005's recorded lesson: comments emitted by a generator
must be fixed in the generator, or the next regeneration reintroduces them).
Update `architecture/tui.md` and `docs/tui.md`. Record the desktop-bridge parity
decision explicitly — see §5.

## 8. Failure, cancellation, restart, contention semantics

- **Channel failure** — `steer_queue_full`, `steer_unavailable`,
  `steer_not_supported`: reported, never swallowed. ADR-0036 forbids a control
  that silently drops input.
- **Controller denial** — `session_control_not_controller` /
  `session_control_conflict`: the reply is refused exactly as an unaddressed
  steer would be.
- **Turn race** — `turn_id_mismatch`: the turn ended between reading its id and
  the daemon reaching the runtime.
- **Two replies racing a finishing turn** — one is delivered, the other reports
  `steer_unavailable`. No retry; a retry would duplicate.
- **Restart** — the addressee is frontend state and does not survive a
  reconnect. It must not be silently re-armed against a name that may have been
  re-leased.
- **Name re-lease** — a curated name is released when the subagent finishes and
  can be drawn again by another run. ADR-0040 lists this as an Option A failure
  mode. The UI must show the name it captured, and the plan's acceptance records
  it as an accepted limitation, not a defect to fix here.

## 9. Compatibility and migration

None. No storage migration, no new protocol field, no configuration key, no
`STORAGE_LAYOUT_VERSION` change. `TurnSteer` already exists on the wire. A
frontend without this milestone ignores the feature entirely; a mixed-version
daemon and TUI keep connecting.

## 10. Required tests

- **Addressee lifecycle** — arm from a running row; survives re-render and a
  snapshot refresh; cleared when the row goes terminal or detaches; a detached
  tab refuses to arm.
- **Request shape** — an armed submit issues exactly one `TurnSteer` whose
  `turn_id` is the controlling parent turn and whose text carries the captured
  name; it issues **no** `TurnSubmit`.
- **Unarmed regression** — an unarmed submit is unchanged.
- **Refusal vocabulary** — one test per refusal code asserting a distinct
  sentence, and one asserting the accepted sentence names the parent.
- **Invariant pins** — I5 (addressing never cancels) and the C005 pool-isolation
  guard remain green.
- **No duplicated chain suite** — this milestone adds no new projection field, so
  it adds no parallel adapter or row test.

## 11. Required verification commands

```bash
cargo fmt --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --lib subagent
cargo test --test tui_main_buffer
cargo test --test tui_sessions_pending_sections
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_tui_subagent_name_pool_isolation.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
# and, if any field reaches the desktop bridge:
cd apps/desktop && node scripts/check-bindings.mjs
```

`git diff --stat -- docs/execution-ownership.toml` must be empty.

## 12. Documentation updates

- `architecture/tui.md` — the addressee as composer data, the parent-turn
  routing, and the honest-reporting rule.
- `docs/tui.md` — user-facing: how to address a subagent and what the outcome
  sentence means.
- **ADR-0040 is accepted and immutable.** Do not edit it. If implementation
  contradicts it, that is a stop condition and a supersession question, not an
  edit.

## 13. Acceptance criteria

Externally observable, mirroring ADR-0040 §Verification:

1. A composed reply addressed to a named subagent reaches `TurnHandle::steer_tx`
   for the **controlling turn**.
2. Each of ADR-0036's three failure modes is reported to the user.
3. The controller predicate refuses a non-controller exactly as for an unaddressed
   steer.
4. A subagent tab is writable only through this feature, and closing a tab still
   cancels nothing (I5).
5. The accepted outcome names the controlling agent as the recipient.

## 14. Stop conditions

Report rather than improvise if any of these is reached:

- The daemon cannot express something ADR-0040 assumed. **No daemon surface
  extension in this milestone.**
- A durable identity is required to address a run unambiguously (this is parent
  D5 and needs an ADR).
- A new `InputTarget` variant, a new `OperationDescriptor`, or a new capability
  appears to be necessary. ADR-0040 settled that these are not needed.
- Anyone proposes Option B, or un-defers it.
- A chord must be invented to reach this feature.
- The pool-isolation guard would have to be weakened for the addressee to work.

## 15. Closure evidence required

The closure record must carry: the request actually issued (captured, not
described), one refusal per ADR-0036 mode, proof the controller predicate is
unchanged, proof I5 held, the I1/pool-isolation guard green, `docs/execution-ownership.toml`
unchanged, and an honest statement of what was **not** observed at runtime —
which for this milestone includes the parent actually relaying the message, since
no milestone can guarantee that and ADR-0040 says so.

## 16. Handoff notes

- Start at WP-1, not WP-2. The invalidation rule is the part that is expensive to
  retrofit and cheap to get right first.
- Read `src/tui/commands/queue.rs` before writing any refusal string. Most of
  the honest vocabulary already exists; a second thinner one is the likely
  failure here, not a missing feature.
- The three facts in WP-2 are the correctness core. Each one is a silent failure
  — a wrong `turn_id` reports a mismatch rather than misbehaving visibly, and an
  armed reply that falls through to `TurnSubmit` would look like it worked.