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

## Registry narrative

Moved verbatim from the `plans/registry.md` row for **TUI subagent and job visibility convergence (corrective)** on 2026-10-23, so that the registry stays an index.

C001–C007 **all closed** (closures `plans/closure/tui-subagent-and-job-visibility-convergence/001-status.md` through `007-status.md`; note the sequence numbering — `003-status.md` closes C005, `006-status.md` closes C003, `007-status.md` closes C006); **C007 is closed** — plan `plans/implementation/tui-subagent-and-job-visibility-convergence/005-send-to-subagent.md`, closure `plans/closure/tui-subagent-and-job-visibility-convergence/005-status.md`. Consolidating to `Ctrl+T` **resolves** cause (1) rather than documenting it: `ToggleTasksList` is `is_normal_only` only because `Shift+T` is a capital letter, so retiring that spelling lets the action leave the gate entirely and work from Insert mode. `Ctrl+S`/`Ctrl+J` are **withdrawn** (D-C1/D-C2) — `Ctrl+J` remains independently undeliverable, byte `0x0A` being reported as `Enter` (parent D4, not to be re-opened); `Shift+W` stays `/workspace`. D-C4 **reversed**: `/sessions` gains real Subagents + Jobs sections rather than a pointer, fed from the same derived `SubagentRow` vector so no fourth projection is created. Also recorded: `names.toml` descriptors are curated and compiled but no producer puts them on the event; the reducer drops `SubagentStarted` with no active turn; and M001–M005 were marked "landed" with `—` in both the implementation-plan and closure-record columns, so no closure gate ever ran. **Progress:** C001 is **closed** (`plans/closure/tui-subagent-and-job-visibility-convergence/001-status.md`) — `Ctrl+T` now consolidates todos/subagents/jobs and works from Insert mode, `Shift+T` is retired, D-C7 retired `todo_expanded`, and the I6 guard found **nine** help rows advertising the wrong action (the old guard passed them because it only checked that a key resolved to *something*). A further decision **D-C9** was recorded: opening the overlay from Insert takes Normal mode with it and hands it back on close, because the overlay's `j`/`k`/`↵`/`Esc` are Normal-mode gestures and shipping only the rebind would have opened a dead surface. **C003 is closed** (`plans/closure/tui-subagent-and-job-visibility-convergence/006-status.md`) — bare `/agents` opens `Dialog::Agent`, and the panel picker it used to open was **deleted outright** (`open_panel_picker` has zero occurrences in `src/`), which is the correct outcome: the addendum forbade routing around the picker by relabelling it. The stop condition was never hit, because `Dialog::Agent` already existed as `/agent`'s surface. Closing it found two things: the test file's module doc still described the retired picker in detail while the tests below it drove `/panels`, and `agents_and_agent_open_the_same_surface` was **vacuous** — both spellings resolve through the completion popup to the same highlighted row, so the test passed unchanged when bare `/agents` was repointed at the wrong dialog. That test was **deleted rather than rewritten**: a replacement was attempted and withdrawn. The assertion that matters (`/agent <name>` changes the selected agent, reachable only via the leading-token fallback) cannot be driven from an integration test — tracing shows the composer receives `/agent <name>` correctly, but the popup's rows still win at `Enter`, their label resolves to no command, and the resolver falls through to “send it as a prompt”. Whether the popup should match a query carrying its own argument is a **product question for the command-palette routing milestone**, not a missing assertion. **C006 is closed as answered-and-escalated** (`plans/closure/tui-subagent-and-job-visibility-convergence/007-status.md`), and the answer is **yes**: `AppEvent::SubagentStarted` is published unconditionally at the top of `run_subagent_task_with_cancel` (`src/agent/worker.rs`), *before* the `tokio::select!` that observes the cancel tokens, so a turn cancelled while its subagent request is still queued — or a `MAX_CHILD_TIMEOUT` expiry in `specialized_runtime.rs` — emits the event against a terminal turn and the reducer drops the node. The reducer test's stated rationale ("not a turn a subagent can start under") was **false** and is corrected; its assertions are unchanged. Retaining such a node needs a home outside `active_turn.agent_tree`, which is the addendum's stop condition, escalated to parent **D5** and an ADR. **That residual is not fixed.** **C004 is closed** (`plans/closure/tui-subagent-and-job-visibility-convergence/004-status.md`) — and closing it changed the tree: the chain was covered link by link but **not at two of its eight links**, so closure added the two missing named tests, for the `AppEvent → CoreEvent` mapping (the hop the **default** daemon mode depends on — deleting `display_name` from that one match arm would have removed every name with the suite green) and for the serde wire round-trip (a snapshot genuinely crosses serde to the frontend, and only the *absent* direction was covered). Both were proven non-vacuous by injecting the defect each exists to catch. **No defect was found in the additive-field chain**, so C005's conditional re-review is **discharged**, not pending. C004 has **no implementation plan** — the D-8 governance gap repeating inside the corrective workstream — and none was written retrospectively; its closure was argued against the addendum's boundary directly. **C002 is also closed** (`plans/closure/tui-subagent-and-job-visibility-convergence/002-status.md`) — `/sessions` (`Dialog::Session`) now lists the session's live subagents and jobs beneath its session list, navigable with the same `j`/`k` and skipping the section headings, with `Enter` on a subagent opening the same tab the sidebar and overlay open for that `task_id`. The sections are fed by the **existing** `App::task_rows()` rather than a second derivation, so the liveness rule stays in one place and the plan's stop condition was avoided rather than hit. `job_id` was added to `TaskRow::Job` — it had been dropped when `JobLike` was built from the projection, so job rows previously had no identity to key on at all. `selected_session()` and `toggle_selection()` were made type-safe against the new rows: positionally, a subagent row resolved to the session above it, and bulk mode ticked a checkbox against an entity bulk-archive cannot act on. Focus is carried by **identity** across a snapshot refresh, so a subagent that finishes mid-scroll does not slide the cursor onto a different entity. The dual-instance rule is honoured on open *and* on every live refresh, and the regression test reads the painted focus-manager clone rather than the App-owned field. C005 is **closed** (`plans/closure/tui-subagent-and-job-visibility-convergence/003-status.md`); see also its row in *Recently closed*. **C007 is closed** (`plans/closure/tui-subagent-and-job-visibility-convergence/005-status.md`) — ADR-0041 Option A as planned: `/subagent-send` arms the composer and one addressed reply issues one `TurnSteer` for the **controlling parent turn**, never the subagent's `task_id` (that dead join is pinned by a test). `InputTarget` still has only `Build`/`Chat` — the addressee is composer data, not a focus mode — and **no key** is bound, so it cannot regress the I3/I6 guards. An armed composer **cannot fall through to `TurnSubmit`**: proven by deleting the branch and watching the test fail naming `submit:…`. Three findings the plan did not have: the frontend **must** supply `turn_id` (M006's send-now deliberately sends none), so it is read at **send** time and `turn_id_mismatch` is user-visible; the parent model has **no name pool**, so the payload carries name *and* agent *and* task description or `Sally` is unresolvable; and a steer is **never stored**, so `send_now_outcome_message` could not be reused verbatim — its accepted sentence claims a queue write a bare steer never makes. It now takes a `SteerConsequence` so one vocabulary serves both senders. M006's own sentences changed as a result and its three tests were updated, not weakened. |
