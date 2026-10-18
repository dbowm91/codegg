# Agent Recovery Escalation Authority Corrective Addendum

Status: proposed

Long-term references:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/001-terminology-and-domain-model.md#7-agent-terms`

Related ADRs:

- None required. This addendum repairs the implementation of an already-accepted
  bounded-recovery contract; it establishes no new authority boundary.

## 1. Purpose and ownership boundary

This addendum owns **turn-local progress detection and graduated tool recovery**
inside the agent loop: `src/agent/progress_recovery.rs` and the observation /
decision wiring in `src/agent/loop.rs`.

It is a post-closure corrective against
`plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md` Milestone 006,
whose closure (`plans/closure/agent-runtime-model-adaptation-acp/006-status.md`)
accepted the recovery controller on the evidence of "typed observations,
SHA-256 fingerprints, fixed ring, bounded evidence" plus six unit tests.

It does **not** own provider transport retry, scheduler admission, tool
authorization, or the turn/run budget. Those remain with their existing owners.

## 2. Work classification

### Invariants

- A tool result that cannot be a repeat (the turn has never made this exact
  call) is never escalated.
- Escalation is driven only by evidence about the **whole call** — canonical
  tool plus normalized arguments — never by tool name alone.
- A bounded recovery ladder terminates a turn only on evidence that the model is
  not progressing, and never on a count of merely-unclassified results.
- Ladder state is per turn and per incident identity. It does not accumulate
  across unrelated incidents or across turns.
- Every field of the observation contract is either populated from production
  evidence or removed. A field that is permanently `false` in production is not
  a signal; it is a misleading one.

### Capabilities

- A false stall is reported with enough bounded evidence to diagnose which call
  pattern produced it.

### Infrastructure

- Typed `ProgressObservation` / `RecoveryDecision` contracts and the fixed ring
  already exist and remain.

### Polish

- Recovery diagnostics, dead-knob removal, and guard coverage follow the
  correctness boundary and are sequenced after it.

## 3. Non-goals

- Changing `ExecutionLimits::max_turns` or any run budget. The outer bound stays
  the run budget, not the recovery ladder.
- Changing the escalation ladder's rung order or vocabulary
  (`Nudge` → `Correct` → `RestoreBasePalette` → `Replan` → `Stall`).
- Provider transport retry policy.
- Adding new tool-surface narrowing heuristics.
- Re-litigating the already-removed `ShortCycle` variant; its removal is correct
  and its regression tests stay.

## 4. Current state

The controller's *incident keying* is correct and matches
`architecture/agent.md`. The defects are in what counts as **progress** and in
how ladder state accumulates.

**Dead progress signals.** `ProgressSignal` has four variants. In production,
`ProgressObservation` is constructed at exactly one site. There,
`new_evidence` and `child_advanced` are hardcoded `false`, and no other writer
exists anywhere in `src/` or `crates/`. `ProgressSignal::ChildAdvanced` is
therefore unreachable, and half of `NewEvidence` is dead. A code comment at that
site states child progress "is populated only by a concrete child-state
observation, when one is exposed" — implying such an observation exists. It does
not.

**Narrow surviving progress.** `StateChanged` requires a drained file-change bus
event **and** a file-modifying tool **and** success, and the file-modifying tool
set is exactly `write`, `edit`, `replace`, `apply_patch`. A successful `bash`
that mutates the tree, a successful git mutation, and a successful job/task
submission are all **not** progress.

**Argument-blind error keying.** `EquivalentError` matches on
`canonical_tool` plus `error_class` only, ignoring arguments and error text.
Because a failing call can never produce a progress signal, the ladder has no
reset path on that path: five consecutive failing calls to the same tool with the
same error class reach `Stall` even when every call was a different command with
a different error message. The unit test asserting this behaviour is named for a
cosmetic-retry case and does not cover distinct-command bisection.

**Unbounded accumulation across incident kinds.** `recoveries` is reset only by
a progress signal, never by a change of incident identity. Two repeats followed
by three unrelated errors, with no progress between them, reach the top of the
ladder.

**Dead knobs.** `recovery_parallel_limit` is read by `max_parallel_tools` but is
assigned `None` at every site and never assigned `Some`, so no parallel
throttling ever occurs. `RestoreBasePalette` overwrites the request tool list
with the full base surface, discarding the context runtime's deliberate
narrowing, which can present the model with a different tool list mid-turn.

**Thin diagnostics.** The user-visible stall error renders only
`"<Kind> observed repeatedly for tool <name>"` — no arguments, no occurrence
count, no elapsed observations.

## 5. Target architecture

One bounded observation contract in which **every field is live**, progress is
computed from evidence the loop can actually observe, and ladder state is keyed
to incident identity so unrelated failures cannot accumulate into a stall.

Concretely: a successful call to a mutating or process-class tool that produced
observable state change, or a successful call whose result differs from the last
result of the identical call, is progress. Error incidents key on the whole
`action_key()` plus the error class. Ladder state resets when the incident
identity changes. Either the dead fields are populated from real evidence or
they are removed from the contract.

## 6. Dependency graph

| Milestone | Depends on | Kind | Notes |
|---|---|---|---|
| C001 escalation authority correctness | — | hard | No unclosed dependency. All inputs are in-tree. |
| C002 diagnostics and guard coverage | C001 | hard | Cannot diagnose or guard a contract that is still changing shape. |

No interface dependency on another subsystem. C001 does not require any ADR.

## 7. Milestones

### Milestone C001 — Progress-recovery escalation authority

- **Class:** invariant
- **Objective:** Make every escalation decision rest on evidence about the whole
  call, and make every field of the observation contract either live or removed.
- **Deliverable boundary:** `src/agent/progress_recovery.rs` and the observation
  construction / decision application in `src/agent/loop.rs`. No protocol,
  storage, config, or provider change. No new tool.
- **Exit conditions:**
  - Every `ProgressObservation` field is populated from production evidence or
    deleted; no field is permanently `false` in the only construction site.
  - A successful read-only call that this turn has not made is progress, and
    reading many files or alternating read/shell never escalates. A successful
    mutating or process-class call is **not** progress on its own (see §10) —
    but it is never escalated either, because a call the turn has never made
    cannot be a repeat, and a change of incident identity resets the ladder.
  - A successful read-only call whose result differs from the previous result of
    the identical call is progress, bounded to the first change so volatile
    output cannot reset recovery forever.
  - A run of failing calls with the same tool and error class but distinct
    arguments or distinct error text does not reach `Stall`.
  - Changing incident identity resets ladder state.
  - A genuinely repeated identical call still escalates and still terminates the
    turn.
  - A regression test sweeps the read/explore/bisect/build shapes that ordinary
    agent work takes and asserts no stall.

### Milestone C002 — Recovery diagnostics and guard coverage

- **Class:** polish
- **Objective:** Make a stall diagnosable and prevent recurrence of the dead-knob
  class of defect.
- **Deliverable boundary:** bounded evidence string and its render, removal or
  wiring of dead knobs, and a static guard over the observation contract.
- **Exit conditions:**
  - The stall message names the incident identity, the occurrence count, and the
    bounded observation shape without echoing arguments, output, or secrets.
  - Dead knobs are removed or wired, with no behavioural regression from
    removal.
  - A guard fails when an observation field is declared but never written in
    production.

## 8. Cross-cutting requirements

### Storage and migration

None.

### Protocol and compatibility

None. The observation contract is internal to the agent loop.

### Security and authorization

Bounded evidence must never include tool arguments, tool output, or model
reasoning. The module's existing "fingerprints and small classifications only"
constraint is preserved and is extended to any new diagnostic field.

### Concurrency, cancellation, and recovery

Controller state is turn-local. It must reset at turn start and must not be
shared across turns or sessions. Cancellation and denial remain typed outcomes
and must not be reclassified as stalls.

### Observability and audit

The existing recovery `tracing` event remains. Diagnostic enrichment is
additive and bounded.

### Documentation and operations

`architecture/agent.md` §"Progress recovery keys on the whole call, never the
tool name" and `AGENTS.md` currently state the outer run bound is
`ExecutionLimits::max_turns`, "not the recovery ladder". If C001 changes when the
ladder can terminate a turn, both documents are updated in the same commit.

## 9. Verification strategy

- Focused unit tests for the controller, including the non-escalation sweeps.
- Integration test driving the real `AgentLoop` observation path with a stubbed
  provider, asserting an ordinary explore-and-edit turn completes.
- `cargo fmt`, `cargo clippy -D warnings`, `scripts/verify.sh quick`.
- No hosted CI claim unless a run is actually executed and cited.

## 10. Risks and decision points

- **Widening "progress" too far** would mask genuine loops. The guard is that
  novelty is still a *fact* about the whole call, and that identical repeated
  calls still escalate.
- **Deciding whether mutating `bash` is progress** was resolved during C001, and
  the answer is **no**. The candidate fallback recorded here before C001 — treat
  a successful `bash` as progress when its result differs from the previous
  identical call — was **rejected**, because it contradicts the other invariant:
  the changed-result rule must stay bounded to read-only calls so that volatile
  mutating output cannot reset recovery forever
  (`volatile_mutating_output_does_not_reset_recovery` pins this). Widening it
  makes a shell loop that echoes a timestamp look productive. Instead, a
  successful mutating or process-class call stays uncredited, and it is protected
  from false stalls by the two rules it *does* get: a call the turn has never
  made is never escalated, and a change of incident identity resets the ladder.
  Recording it again here because the next reader should not rediscover it, and
  because the rejected fallback is the tempting one.
  **Residual, for C002:** a successful mutating call that *is* a repeat leaves
  `AutonomyState.transitions` untouched rather than resetting it, so a long turn
  can exhaust the transition budget without a corresponding number of incidents.
  Worth confirming against real trajectories; fixing it needs the per-tool
  state-change observation described in §4, not a heuristic.
- **Removing a dead knob** is safe; **wiring** it changes parallelism under
  recovery and must not be done incidentally in C001.

## 11. Completion definition

Both C001 and C002 closed, with closure evidence showing that ordinary agent
work shapes do not stall, that genuinely repeated work still does, and that the
observation contract contains no permanently-false field.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | closed | `plans/implementation/agent-recovery-escalation-authority/001-progress-recovery-escalation-authority.md` | `plans/closure/agent-recovery-escalation-authority/001-status.md` | None |
| C002 | not started; eligible for fresh planning | — | — | C001 now closed |