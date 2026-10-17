# Agent Recovery Escalation Authority Milestone C001 — Progress-Recovery Escalation Authority

Status: ready for handoff

Repository baseline: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`
(branch `feat/auto-20261016-9ebb3f33`)

Source roadmap: `plans/subsystems/agent-recovery-escalation-authority-corrective-addendum.md#milestone-c001--progress-recovery-escalation-authority`

Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`;
`plans/001-terminology-and-domain-model.md#7-agent-terms`

Applicable ADRs: None required. This milestone repairs the implementation of an
already-accepted bounded-recovery contract.

Post-closure corrective against: `plans/closure/agent-runtime-model-adaptation-acp/006-status.md`
(Milestone 006, `plans/implementation/agent-runtime-model-adaptation-acp/006-progress-loop-and-tool-recovery-controller.md`).

Primary class: invariant

## 1. Objective

Make every progress-recovery escalation decision rest on evidence about the
whole call, and ensure every field of `ProgressObservation` is either populated
from production evidence or removed — so that ordinary agent work cannot be
terminated as a stall.

## 2. Why this milestone is ready

No unclosed dependency. Both `src/agent/progress_recovery.rs` and the observation
wiring in `src/agent/loop.rs` are in-tree and self-contained. The predecessor
roadmap is closed, so this is corrective work with no interface negotiation
pending. No ADR is required because no authority boundary changes: the incident
keying rule, the ladder rungs, and the run budget are all unchanged.

## 3. Current implementation evidence

At baseline, with the single production construction site in `src/agent/loop.rs`
building the observation inside the per-tool-call loop:

- `new_evidence: false` and `child_advanced: false` are hardcoded. A repository-wide
  search of `src/` and `crates/` finds no other writer. `ProgressSignal::ChildAdvanced`
  is therefore unreachable, and `ProgressSignal::NewEvidence` is reachable only
  through the derived `novel_read` / `novel_read_result` paths.
- `state_changed` is computed from a file-change bus event drained once per
  batch, ANDed with `is_file_modifying_tool`, which matches only
  `write | edit | replace | apply_patch`.
- `RecoveryController::detect` classifies `EquivalentError` on
  `canonical_tool` plus `error_class` alone — not on `action_key()`, and not on
  result or error text.
- `RecoveryController::observe` resets `recoveries` and `incident` only on a
  non-`None` progress signal. A change of incident identity does not reset them.
- `RecoveryController::recoveries` and `AutonomyState::MAX_TRANSITIONS` are both
  `4`, so the fifth consecutive non-progressing incident-classified result
  returns `RecoveryDecision::Stalled`, which sets `recovery_stalled`, publishes an
  `AppEvent::Error`, and breaks the turn.
- `services.recovery_parallel_limit` is read by `max_parallel_tools` but is only
  ever assigned `None`; it is never assigned `Some`.
- Existing regression tests already pin the removed `ShortCycle` behaviour and
  must continue to pass unchanged.

## 4. Invariants that must not regress

- No tool-name-only escalation check may be reintroduced. Every incident keys on
  the whole `action_key()`.
- A call the turn has never made is not a repeat and must never be escalated.
- Genuinely repeated work — the identical call, or the identical call with an
  identical result — must still escalate and must still terminate the turn.
- `ExecutionLimits::max_turns` remains the outer bound on a run.
- The module stores fingerprints and small classifications only: never tool
  arguments, output, or model reasoning.
- Typed execution status stays authoritative over rendered model text. Denied,
  timeout, and cancelled outcomes are not stalls.
- `RestoreBasePalette` must not discard the context runtime's deliberate tool
  narrowing in a way that changes which tools a turn may use.
- `Push_control_instruction` recovery text stays bounded and carries no secrets.

## 5. Scope

In scope:

- `src/agent/progress_recovery.rs`: the observation contract, progress
  computation, incident classification, and ladder state accumulation.
- `src/agent/loop.rs`: the single observation construction site and the decision
  application.
- Unit and integration tests for both.

Out of scope:

- `ExecutionLimits`, turn budgets, or goal budgets.
- Provider transport retry.
- Tool authorization, sandbox, or permission policy.
- Removal or wiring of `recovery_parallel_limit` (that is C002).
- Diagnostic enrichment of the stall message (C002).
- Any change to the ladder's rung order or vocabulary.

## 6. Required production changes

### Contract

Decide, per field, between populating and deleting:

- `new_evidence` — currently no producer. Either give the loop a real
  new-evidence signal or delete the field and keep the derived rules.
- `child_advanced` — currently no producer, and `ProgressSignal::ChildAdvanced`
  is unreachable. Either populate it from a concrete child-state observation that
  genuinely exists, or delete the field and the variant. Deleting is acceptable
  and is the conservative choice if no such observation is available; do not
  leave an unreachable variant that reads as a supported signal.
- `state_changed` — widen the notion of "this call changed observable state" to
  cover mutations the loop can actually observe, or document precisely why a
  mutating `bash` or a git mutation is deliberately excluded.
- `effect_class` — keep sourced from the tool contract; confirm behaviour when
  the registry lookup returns `None` (an unregistered tool name) and make that
  case deliberate rather than incidental.

### Progress computation

- Novelty stays keyed on the whole call.
- A result that differs from the previous result of the identical call stays
  progress, and the existing bounded "first changed result only" rule must
  survive so volatile output cannot reset recovery forever.
- Decide and record whether a successful mutating or process-class call is
  progress when no state-change observation exists. If not, say so in a comment
  so the next reader does not rediscover it.

### Incident classification

- `EquivalentError` must key on the whole call, not on tool plus error class
  alone. Distinct commands that fail the same way are not equivalent work.
  Concretely: include `action_key()` in the match, and decide whether result or
  error text participates.

### Ladder state

- Reset ladder state when incident identity changes, so unrelated failures cannot
  accumulate into a terminal stall.
- Keep the bound: the ladder must still terminate a turn that genuinely repeats.

### Loop wiring

- Keep `RestoreBasePalette` from widening the request's tool list in a way that
  contradicts the context runtime's narrowing. If a genuine conflict exists,
  record it as a stop condition rather than silently choosing.

## 7. Ordered work packages

**W1 — Establish the failing evidence first.** Add regression tests, against the
current code, that fail for the right reason: (a) five failing calls to the same
tool with distinct arguments and distinct error text reach `Stalled`; (b) two
repeats followed by three unrelated errors reach `Stalled`; (c) a successful
mutating `bash` following a failing sequence does not reset the ladder. These are
the acceptance signal for the milestone; if they cannot be made to fail first, the
finding is wrong and the milestone should stop and report.

**W2 — Settle the contract.** Apply the per-field decision from §6. Delete rather
than leave dead fields. Update any doc comment that implies an observation
producer exists when it does not.

**W3 — Fix progress computation.** Implement the decided progress rules.

**W4 — Fix incident classification.** Re-key `EquivalentError` on the whole call.

**W5 — Fix ladder accumulation.** Reset on incident-identity change.

**W6 — Non-escalation and true-positive sweeps.** Keep the existing
`ShortCycle`-regression tests passing unchanged. Add sweeps for read-heavy,
read/shell-interleaved, bisecting-failure, and build-fix-loop shapes, each
asserting the turn is not stalled.

**W7 — Preserve detection.** Assert the identical repeated call still escalates
and still terminates the turn, with the same bounded rungs.

## 8. Failure, cancellation, restart, and contention semantics

- Denied, timeout, and cancelled outcomes must remain typed and must not be
  reclassified as stalls.
- A stall still publishes a bounded `AppEvent::Error` and still breaks the turn;
  this milestone changes *when* a stall is declared, never what happens once it
  is.
- Controller state is turn-local and must reset at turn start. No new shared or
  cross-turn state is introduced, so no new contention surface exists.

## 9. Compatibility and migration

None. No protocol, storage, config, or public-surface change. The observation
contract is internal to the agent loop.

## 10. Required tests

Focused:

- `new_evidence` / `child_advanced` contract: either assert they are populated on
  the paths that claim to populate them, or assert they no longer exist.
- Novel read-only call is progress regardless of interleaving.
- Identical call with a changed result is progress, bounded to the first change.
- Distinct failing calls with the same tool and error class do not escalate.
- Ladder state resets on incident-identity change.
- Identical repeated call still escalates; the ladder still terminates the turn.
- Successful mutating `bash` progress behaviour matches the recorded decision.
- `effect_class: None` (unregistered tool name) behaves deliberately.

Integration:

- Drive the real `AgentLoop` observation path with a stubbed provider emitting a
  read/explore/edit sequence and assert the turn completes without a stall.
- Emit a genuinely repeating sequence through the same path and assert the turn
  terminates as stalled.

## 11. Required verification commands

```bash
cargo fmt
cargo test -p codegg --lib agent::progress_recovery
cargo clippy --all-targets -- -D warnings
scripts/verify.sh quick
```

`verify.sh quick` covers the agent schema guard, core boundary, sandbox,
execution-ownership, scheduler-bypass, and no-hardcoded-models checks relevant
to this change. Do not use `--all-features`.

## 12. Documentation updates

- `architecture/agent.md` §"Progress recovery keys on the whole call, never the
  tool name" — update the `EffectClass` note and the claim that the outer bound
  is `ExecutionLimits::max_turns` if the changed escalation rule makes the ladder
  able to terminate a turn sooner.
- `AGENTS.md` — the progress-recovery gotcha must state the corrected rule if it
  changes; it currently documents the removed `ShortCycle` variant and the
  novel-read rule.
- `.opencode/skills/agent/SKILL.md` if its module summary changes.

## 13. Acceptance criteria

Externally observable:

- A model that reads many files, alternates read and shell, or retries several
  distinct failing commands completes its turn without an "Agent stalled" error.
- A model that repeats an identical call with an identical result still receives
  the bounded ladder and still has its turn terminated.
- No `ProgressObservation` field is permanently `false` in production; any field
  that remains is written from real evidence.
- `cargo test -p codegg --lib agent::progress_recovery` and
  `scripts/verify.sh quick` pass.

## 14. Stop conditions

Report rather than improvise if:

- W1's failing tests cannot be made to fail against the baseline — the premise
  of the milestone is then wrong.
- Removing a dead field would require changing the public progress taxonomy that
  another subsystem depends on.
- The chosen progress rule would make a genuinely looping model look productive.
  Under-escalation is a worse failure than a false stall; do not trade one for
  the other silently.
- Widening progress to cover mutating shell commands turns out to require a new
  per-tool state-change observation. That is a separate milestone; stop and
  report.
- `RestoreBasePalette` and the context runtime's narrowing prove to be in genuine
  conflict.

## 15. Closure evidence required

- Closure record at `plans/closure/agent-recovery-escalation-authority/001-status.md`
  with a requirement-to-evidence matrix.
- The W1 failing tests, shown failing at baseline and passing after.
- Focused test results, `cargo clippy -D warnings`, and `scripts/verify.sh quick`
  results, labelled local vs hosted truthfully.
- Explicit statement of which progress signals remain reachable in production.

## 16. Handoff notes

Start at W1. The milestone's value is measured entirely by whether ordinary
agent work stops producing false stalls, so the failing-evidence step is the
acceptance signal, not ceremony. If the review's diagnosis is wrong, the correct
outcome is an early stop with evidence, not a milestone closed around the
original premise.