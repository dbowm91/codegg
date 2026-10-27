# ADR-0054: The Daemon Owns the Provider-Visible Sequence Across a Turn Boundary

Status: accepted

Date: 2026-10-26

Accepted: 2026-10-26, by the project maintainer, on the direction selected for the
post-closure corrective `context-continuity-compaction-post-closure-corrective`
C001: **the daemon drops the superseded prefix**. The three candidate directions
were put to the maintainer with their trade-offs; the daemon-side drop was
selected over publishing an epoch boundary to the frontend and over keeping the
frontend transcript authoritative.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmaps:

- `plans/subsystems/context-continuity-compaction-post-closure-corrective-addendum.md` (new; owns C001)
- `plans/subsystems/context-continuity-compaction-roadmap.md` (closed; M001–M004 remain closed and are **not** reopened by this record)
- `plans/subsystems/long-horizon-work-execution-roadmap.md` (closed; its M004 context-epoch handoff integration remains closed and unchanged)

Related ADRs:

- ADR-0003 (accepted) — long-horizon work state and context epochs. This record is
  bound by ADR-0003's conclusions and does not revisit them: a fresh
  provider-visible **context epoch** remains a legitimate mechanism, and where it
  applies it is still a host-state-driven rebuild.
- ADR-0051 (accepted) — durable delegated run transcript and continuation authority.
  Not revisited; this record concerns the **primary** session's provider-visible
  sequence, not a delegated run's.
- ADR-0052 (accepted) — cross-frontend command execution ownership. Not revisited.

## Context

CodeGG's compaction is a **transition between bounded context epochs over one
durable task state**. The compaction owner replaces the provider-visible
message sequence in memory after a verified checkpoint is installed
(`architecture/context-compaction-ownership.md`, "Transactional rollover (M004)"
and "Fresh context epochs — M004 handoff integration"). That document states the
intended contract plainly:

> durable session history is never deleted; only the in-memory/provider-visible
> sequence is replaced after verification

and M004's own plan requires, at turn construction, that the daemon "load the
latest installed checkpoint for the session" and inject its bounded projection
"**before the current user turn**"
(`plans/implementation/context-continuity-compaction/004-transactional-rollover-and-multi-compaction-qualification.md` §6.5).
M004's invariant list also forbids that "compaction state does not become a
frontend-owned concern".

**The shipped topology does not satisfy that contract.** The frontend owns the
transcript and re-sends all of it on every turn:

- `App::build_provider_context` (`src/tui/app/mod.rs:3382`) renders the TUI's
  whole in-memory transcript into provider messages. It emits only
  User/Assistant/Tool rows — **never a `Message::System`** — so the daemon's
  own system prompt and every daemon-injected frame are absent from it.
- `dispatch_turn_submit_request` (`src/tui/app/mod.rs:3113`) attaches that list
  to `CoreRequest::TurnSubmit`.
- The daemon's `TurnSubmit` arm (`src/core/daemon_turns.rs:312`) forwards it
  unchanged as `messages_dto` (`src/core/daemon_turns.rs:985`), and
  `src/agent/turn_runtime.rs:1074` turns it into the `ChatRequest` the agent
  loop runs on. The daemon does **not** re-read durable session history to
  build that request.

Meanwhile the daemon's compaction replaced only its **own** copy of the
sequence, and that copy is discarded at the turn boundary. The TUI transcript
is never rewritten — the only reaction to a compaction is a toast
(`src/tui/runtime/app_events.rs:525`).

The result is that a turn that compacts produces **no durable effect on what
the next turn sends**. Turn N+1 re-sends turn N's full pre-compaction body and
compacts again. Compaction never shrinks what crosses the wire.

On top of that, `inject_installed_continuation_for_turn`
(`src/agent/context_runtime.rs:773`) re-injects the installed checkpoint's
bounded projection at the start of every subsequent turn
(`src/agent/loop.rs:1330`), because `strip_prior_frames`
(`src/context/rollover.rs:653`) only removes `Message::System` rows that are
CodeGG-owned frames (`src/context/compaction.rs:2478`) and the frontend's
transcript contains none.

So turn N+1's provider request carries turn N's material **twice** — once raw
from the frontend transcript, once distilled from the checkpoint — with the new
user message in between. The distilled copy restates the goal, current task,
constraints, **decisions**, commands, tests, errors and next steps
(`src/context/rollover.rs:507`), and a pending question or a permission verdict
is a decision, so questions and permissions the agent already sent come back.

The projection is also placed **after** the new user message on the default
profile: `push_control_instruction`
(`crates/codegg-core/src/model_profile/policy.rs:29`) appends a
`Message::System` at the tail when the profile allows late system messages
(`policy.rs:56`), and the default profile allows them
(`crates/codegg-core/src/model_profile/resolve.rs:160`). M004 §6.5 required the
opposite.

### Why the existing verification did not catch this

M004's own multi-compaction trajectory models a daemon that rewrites its own
history. `tests/agent_loop/context_continuity_m004.inc:443` carries the comment
"Replace history only after verification (transactional seam)" and then does
`messages = result.messages.clone()` — carrying the compacted list into the
next epoch. That is exactly the assumption the shipped topology violates. Every
other continuity test asserts the *daemon-internal* invariant "exactly one
continuation frame in the compacted list"
(`src/context/compaction.rs:3881`, `tests/agent_loop/context_continuity_m004.inc:1201`).

No test drives two `AgentLoop::run` turns on one session with a compaction
between them, and `inject_installed_continuation_for_turn` has **zero** test
call sites — it is `pub(super)`, so it is not reachable from `tests/` at all.
The seam that is broken is the TUI→daemon boundary, and no test spans it.

## Decision drivers

1. **The compaction must actually shrink what crosses the wire.** A compaction
   that is undone by the next turn's request is not a compaction.
2. **The frontend's transcript is a display record, not a second history
   authority.** M004 already decided that compaction state must not become
   frontend-owned; the frontend may not re-assert compacted history.
3. **Do not redefine the cross-frontend turn-submit contract.** The shared
   client library requires a non-empty provider-message list
   (`crates/codegg-client/src/compose.rs:124`, `ComposerError::EmptyMessages`),
   and that contract is used by every frontend. Changing it is a larger,
   riskier surface than the defect warrants.
4. **The frontend's scrollback is the user's record.** The correction must not
   delete what the user can read in the TUI.
5. **Bounded, verifiable, and honest.** Any new durable state must be bounded
   and its purpose recorded; the fix must be provable by a test that fails on
   the baseline.

## Considered options

### Option A — The daemon drops the superseded prefix (**selected**)

The daemon recognises the prefix of the frontend-supplied message list that it
has already compacted and replaced, drops it, and continues with its own
provider-visible sequence plus the checkpoint's bounded projection.

- *Benefits*: preserves the cross-frontend `TurnSubmit` contract unchanged
  (driver 3); keeps the frontend's full transcript on screen (driver 4); makes
  the compaction effective (driver 1); restores the daemon as the owner of the
  provider-visible sequence, which is what the ownership document already
  states (driver 2).
- *Costs*: the daemon needs a durable, comparable identity for the history it
  replaced, and a durable form of the replacement it will re-install — the
  in-memory replacement does not survive the turn boundary. Both are additive
  fields on the existing bounded checkpoint payload
  (`crates/codegg-core/src/session/continuation.rs:319`), not a new store.
- *Failure modes*: the daemon must fail **closed** on the "is this the prefix I
  replaced?" question — a mismatch means the daemon keeps the frontend's list
  rather than dropping the wrong rows. A false positive would silently discard
  live material; the comparison must therefore be an exact identity over a
  stated projection of the incoming list, not a heuristic.

### Option B — Publish the epoch boundary to the frontend

The daemon tells the frontend which prefix it replaced; the frontend then sends
only the post-boundary tail.

- *Benefits*: the daemon does not have to re-derive anything; the wire payload
  shrinks at the source; the frontend's display and its send-path share one
  boundary.
- *Costs*: a **new protocol surface** — a boundary/epoch identity that the
  frontend must carry and echo, plus a stale-frontend case to define (a
  frontend that missed the boundary sends a list the daemon cannot interpret).
  Every frontend that composes a turn submit must adopt it.
- *Failure modes*: a lagging, restarted or independently-attached frontend
  sends an unrecognised list. The daemon must still have a fallback, which
  means keeping Option A's machinery anyway.

### Option C — The frontend transcript stays authoritative

Keep the frontend's transcript as the whole history; stop re-injecting the
redundant continuation projection and accept that every later turn re-sends the
full pre-compaction body.

- *Benefits*: the smallest change; it removes the *visible* duplication.
- *Costs*: it does not remove the underlying defect — the compaction remains
  ineffective, so every turn re-sends a body that grows without bound and a
  long trajectory still fails on capacity. It also directly contradicts M004's
  "compaction state does not become a frontend-owned concern".
- *Failure modes*: this option treats the symptom. The reported "the loop
  ultimately fails" outcome is a capacity failure caused by the un-shrunk
  re-send, and it survives this option.

## Decision

**Option A. The daemon owns the provider-visible sequence across a turn
boundary.**

The rules this record fixes:

1. The daemon's provider-visible sequence for a turn is the daemon's, not the
   frontend's. A frontend-supplied message list supplies the **new** material
   of a turn; it does not re-assert history the daemon has already compacted.
2. When the daemon holds an installed checkpoint whose recorded source covers a
   prefix of the frontend-supplied list, the daemon **drops that prefix** and
   continues from its own compacted replacement plus the checkpoint's bounded
   projection.
3. The identity comparison is **exact and fail-closed**. If the incoming
   prefix does not match the recorded source exactly, the daemon keeps the
   frontend's list unchanged and emits a bounded diagnostic. It never drops a
   prefix it cannot identify.
4. The checkpoint's bounded projection is injected **before the current user
   turn** (restoring M004 §6.5), for every model profile, and is never
   reachable as "the current user message".
5. The daemon's durable checkpoint must therefore carry, in addition to what it
   already carries, (a) a comparable identity for the history it replaced and
   (b) the bounded form of the replacement it will re-install. Both are
   additive, bounded fields on the existing checkpoint payload. **No new
   storage, no second history service, and no change to the `TurnSubmit`
   contract.**
6. The frontend's transcript remains the frontend's display record. It is not
   truncated, rewritten, or emptied by this correction.

This record fixes the **contract and the authority**. It deliberately does not
prescribe the comparison's projection, the checkpoint's field layout, or
whether a bounded fresh-epoch rebuild is the appropriate vehicle for a given
profile. Those are implementation-mechanism choices inside the boundary this
record sets, and they belong to
`plans/implementation/context-continuity-compaction-post-closure-corrective/001-provider-visible-sequence-survives-the-turn-boundary.md`.

## Consequences

**Positive.**

- A compaction becomes effective: the next turn's request carries the compacted
  sequence, not the pre-compaction body.
- The duplicate is gone by construction — the raw copy is dropped, so the
  distilled projection is the only carrier of that material.
- Questions and permissions the agent already sent stop reappearing as
  duplicated decisions, because the raw decision record is no longer resent
  beside the projection that summarises it.
- The frontend's scrollback is untouched; the user still sees everything.
- The cross-frontend `TurnSubmit` contract is unchanged.

**Negative.**

- The daemon carries more durable state (two additive bounded fields on the
  checkpoint payload), and the checkpoint payload's bound
  (`CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES = 128 * 1024`,
  `crates/codegg-core/src/session/continuation.rs:51`) must be respected.
- The daemon still receives the frontend's superseded list over the wire before
  it can drop it. The transport cost of a large superseded prefix remains.
  This is recorded as a known cost, not a defect; bounding the wire payload is
  explicitly out of scope for this milestone (see the plan's non-goals).
- Every frontend that composes a turn submit inherits the same daemon-side
  behaviour, whether or not it was audited.

**Neutral.**

- The durable session transcript is unchanged: it remains the uncompacted record
  of what happened, and "durable session history is never deleted" continues to
  hold.
- No permission, tool authority, workspace, Git, job, budget or runtime-asset
  semantics change.

## Compatibility and migration

- **No wire-format change.** `CoreRequest::TurnSubmit` keeps its shape, and
  `crates/codegg-client`'s requirement of a non-empty provider-message list
  (`crates/codegg-client/src/compose.rs:124`) is untouched.
- **No storage migration is required** for the decision itself. Any new
  checkpoint-payload field MUST be `#[serde(default)]` and additive, so a
  checkpoint written by an older build stays readable and renders safely
  (the same rule M004 applied to `work_plan` provenance).
- **Older checkpoints remain valid.** A checkpoint that carries no source
  identity simply never matches an incoming prefix; the daemon then keeps the
  frontend's list and emits a diagnostic. Absence of the identity is not
  evidence of duplication.
- **A daemon and frontend of different builds interoperate**, because the
  authority is entirely daemon-side.

## Security and reliability implications

- **No authority is widened.** The daemon drops only history it has already
  compacted and replaced; it never drops live material, tool results, or
  permission records that the checkpoint does not cover.
- **Fail-closed on identity.** An unidentifiable prefix is kept, not dropped.
  A wrong drop is worse than a redundant send, because a redundant send costs
  tokens and a wrong drop costs history.
- **Bounded payloads.** The checkpoint payload stays within its 128 KiB bound,
  and the projection stays within the existing continuation bounds.
- **No secret-bearing material is copied.** The existing rule that hidden
  reasoning, credentials and secret-bearing tool arguments are never copied
  into continuation state is unchanged and must be re-verified for the new
  fields.
- **Availability.** This is a capacity-correctness fix: it removes a mechanism
  by which a long trajectory's request grows without bound and the turn fails
  on provider capacity.

## Verification

1. A test that drives **two turns on one session with a compaction between
   them** and asserts that the second turn's provider-visible sequence does
   **not** contain the first turn's pre-compaction body. No such test exists
   today; this is the regression the milestone must land.
2. A test that asserts the second turn's sequence contains **exactly one**
   continuation projection and that the projection is positioned **before** the
   current user message.
3. A test that the "current user message" read is unaffected by any
   daemon-injected row, including a peer observation and a control instruction.
4. A fail-closed test: an incoming prefix that does not match the recorded
   source is **kept**, and the diagnostic is emitted.
5. A bounds test: the enlarged checkpoint payload stays within its bound and
   carries no body-level secret.
6. The existing continuity suites
   (`tests/agent_loop/context_continuity_m004.inc`,
   `tests/agent_loop/context_epoch_handoff.inc`,
   `src/context/compaction.rs` continuity tests) remain green and unmodified in
   their intent.
7. `scripts/verify.sh quick`, workspace `cargo clippy --all-targets -- -D
   warnings`, and the workspace test sweep.

## Supersession

None. This record is new. It does not supersede ADR-0003, ADR-0051 or ADR-0052,
and it does not reopen the closed `context-continuity-compaction` M001–M004
milestones or their closure records. It is bound by them, and where M004 §6.5
required the continuation projection to be injected "before the current user
turn", this record makes that requirement binding for every model profile.
