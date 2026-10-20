# TUI Subagent and Job Visibility Convergence — Milestone 007 — Closure Status

**Milestone:** C006 of `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`

Status: **closed** — as an *answered and escalated* milestone, not as a fixed
one. The residual is unresolved and is stated in §3.

Document numbering: this subsystem numbers closure documents by **write
sequence**, not by milestone id — `003-status.md` closes **C005** and
`005-status.md` closes **C007**. This document is the seventh closure record and
closes **C006**.

Source implementation plan:

- **None.** C006 was implemented without one. This is governance gap **D-8** of
  the corrective addendum, the same gap C004 and C003 closed against. No plan was
  written retrospectively. This record closes C006 **directly against the
  addendum's deliverable boundary and stop condition**, which is the only
  specification that ever existed for it.

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`
  §7 (C006), §4 (D-7)

Parent escalation target: **D5** of
`plans/subsystems/tui-subagent-and-job-visibility-roadmap.md` — "The dead
`run_id` join may be more than a cleanup. If fixing it properly requires
re-keying the per-turn tree onto durable run ids, that is a projection-identity
change and may warrant its own ADR." D5 itself was **resolved by the narrow
route** (the real `run_id` is now carried on the event); what C006 needs from it
is its *escalation path*, not its answer.

Repository baseline: `e8a6b5ca2`.

Implementation: **committed at `e8a6b5ca2`** for the reducer behaviour; the
corrected rationale and this record are this session's working-tree changes.

Scope note: this record covers only C006. The working tree separately carries
C002's, C005's and C007's uncommitted implementation.

## 1. Executive finding

C006 asked a yes/no question: can a running subagent exist with **no active
parent turn**? The milestone's own deliverable boundary made the answer
consequential — *yes* means the reducer's drop is a defect and the node must be
retained; *no* means pin the invariant with a test.

**The answer is yes, and the existing test's stated rationale for the drop was
wrong.**

The reducer's `SubagentStarted` arm
(`crates/codegg-protocol/src/projection/reducer.rs`) drops a node when
`snapshot.active_turn` is `None` and records an `orphan_subagent_started`
diagnostic instead. The test guarding that behaviour
(`a_subagent_started_without_an_active_turn_is_diagnosed_not_applied`) justified
it by asserting that the event's only publisher is `src/agent/worker.rs`, which
"publishes from inside a subagent execution the parent turn is still awaiting",
so "a turn that has already completed is therefore not a turn a subagent can
start under" — i.e. the drop could only ever see a malformed log.

Reading the worker settles it the other way. §2.

## 2. The evidence

`AppEvent::SubagentStarted` is published from exactly one place —
`src/agent/worker.rs`, in `run_subagent_task_with_cancel`. Every other
occurrence in `src/` is a match arm, a consumer, or a comment.

The decisive detail is **where** in that function the publish sits. It is
unconditional and first — the event is published at the top of the function,
*before* the `tokio::select!` that observes `scheduler_cancel`, `cancel_token` and
`lineage_token`, and well before `execute_agent_task` is even constructed. Already-
cancelled tokens cannot suppress it, because they are not consulted yet.

That makes two paths reachable in which the event is emitted against a terminal
turn:

1. **Cancellation before dispatch.** A turn is stopped or cancelled while the
   subagent request is still queued on `request_tx`. The worker dequeues it
   later, publishes `SubagentStarted`, and then — via the cancellation arm —
   publishes `SubagentFailed` without executing anything. By then `active_turn`
   is `None`.
2. **Parent-side timeout.** `src/agent/specialized_runtime.rs:193` wraps
   `send_and_wait` in `MAX_CHILD_TIMEOUT` (120s). Expiry drops the awaiting
   future; the spawned worker continues to run and publishes normally.

In both cases the node describes work that genuinely started, and the reducer
silently discards it.

## 3. Disposition: answered and escalated

The addendum's boundary:

> **Deliverable boundary:** establish from the job/subagent lifecycle whether a
> running subagent can exist with no active parent turn. If yes, the reducer's
> drop is a defect and the node must be retained (bounded, evicted on terminal
> state). If no, pin the invariant with a test so a future change cannot quietly
> invalidate it.
>
> **Stop condition:** if the answer requires durable subagent identity, stop and
> escalate — that is parent D5 and needs an ADR.

The answer is **yes**, which routes the milestone to the *fix* branch — and the
fix branch requires a home for a node that has no owning turn. `SubagentStarted`
nodes currently live only in `active_turn.agent_tree`, so retaining one means
either a session-level subagent list or re-keying the tree on a durable
identity. Both are projection-identity changes: a new snapshot field, a reducer
branch, an adapter, a DTO, and the transport guards that check them.

That is precisely D5's "projection-identity change … may warrant its own ADR",
and precisely the addendum's stop condition. **C006 therefore stops and
escalates, as instructed, rather than implementing a schema change under a
milestone that was scoped to answer a question.**

Two things make the escalation narrower than it looks, and are recorded so the
ADR does not start from zero:

- **Durable identity already exists on the node.** D5's narrow route put the real
  `run_id` on the event and onto `AgentTreeNodeProjection`. What is missing is not
  a key but a *home* — a snapshot location for a node with no owning turn.
- **The diagnostic path is bounded and safe.** `record_diagnostic` pushes through
  `push_diagnostic`, capped at `MAX_PROJECTION_DIAGNOSTICS` = 32
  (`crates/codegg-protocol/src/projection/limits.rs:124`). Today's behaviour
  cannot grow the snapshot without bound; the only cost of the current drop is
  that the subagent is invisible in the TUI.

**The residual is not fixed.** A subagent that starts after its parent turn went
terminal is still discarded today. This record does not claim otherwise, and the
status `closed` here means the *question was answered and routed*, not that the
drop is correct.

## 4. What changed

- **The test's rationale was corrected.** The doc comment on
  `a_subagent_started_without_an_active_turn_is_diagnosed_not_applied` claimed the
  drop is unreachable for real events. It now states what §2 establishes, names
  both reachable paths, and says explicitly that the test pins *what happens*
  rather than claiming it is unavoidable. **The test's assertions are unchanged**
  — it still pins "diagnosed, not applied, and the snapshot must not grow an
  active turn to host it" — because that behaviour is still what the code does
  and is still what an ADR will have to change.
- **The test was left passing.** Correcting a false comment does not weaken a
  test. Rewriting the assertions to match the new understanding would have
  deleted the only thing standing between a silent drop and a silent regression.

No production behaviour changed. This milestone changed a comment, and that is
recorded plainly rather than dressed as a fix.

## 5. Verification executed

**All verification was local. No CI receipt exists for this work, and no runtime
observation was made — the TUI was never launched and no subagent was ever run.**

```bash
cargo fmt --check                                                     # pass
cargo clippy --workspace --all-targets --locked -- -D warnings      # pass, no warnings
scripts/verify.sh quick                                              # pass ("Quick verification passed.")
cargo test -p codegg-protocol --lib projection                       # 94 passed, 0 failed
cargo test --lib subagent                                            # 119 passed, 0 failed
python3 scripts/check_projection_transport_isolation.py             # pass
python3 scripts/check_projection_transport_lifecycle.py             # pass
python3 scripts/check_projection_disclosure.sh                      # pass
python3 scripts/check_websocket_bounds.py                            # pass
python3 scripts/check_adr_integrity.py                               # pass
(cd apps/desktop && node scripts/check-bindings.mjs)                 # pass
```

`cargo test -p codegg-protocol --lib projection` reports **94 — unchanged**.
That is the point: closing C006 changed a comment and no assertion, so no
count moved. A delta here would have meant the record had quietly rewritten the
test it claims only to have re-documented.

### Non-vacuity result

**Injection:** the diagnostic code `orphan_subagent_started` was renamed to
`orphan_subagent_started_INJECTED` in the reducer's `SubagentStarted` arm.

**Result:** `a_subagent_started_without_an_active_turn_is_diagnosed_not_applied`
**FAILED**, panicking at `crates/codegg-protocol/src/projection/reducer.rs:2143`
— the assertion that the drop stays observable. The injection was reverted and
the suite returned green.

This proves the test still guards the behaviour after its rationale was
corrected. Had it not, the honest move would have been to leave the false
comment in place rather than claim a corrected-and-verified test.


## 6. Invariant review

- **I1 — projection-derived rows.** Holds. Nothing here fabricates a row; the
  question is whether a real row is dropped, and it is.
- **Bounded behaviour.** Holds today: diagnostics cap at 32, and no node is
  retained. It is the *fix* that would need an eviction bound, which the addendum
  already anticipated ("retained (bounded, evicted on terminal state)").
- **Replay determinism.** Unaffected. The drop is deterministic given the event
  log; retaining a node would have to stay deterministic too, which the ADR
  should require explicitly.

## 7. What was **not** verified

- **No runtime observation.** The two reachable paths in §2 are established by
  reading the code, not by triggering a cancellation under a live daemon and
  watching a node vanish. That experiment is the obvious first step of the ADR
  and is **not** claimed as done here.
- **No CI receipt.**
- **No exhaustive search for further publishers.** `src/` was searched for
  `SubagentStarted` and every other hit was inspected; a publisher in another
  crate or behind a macro was not separately excluded.
- **No assessment of the fix's blast radius.** How a session-level subagent list
  would interact with the projection transport guards, the websocket bounds, and
  the desktop bridge is unexamined — deliberately, because that is the ADR's
  job, not this milestone's.