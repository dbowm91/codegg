# Harness Lab Post-Pilot Corrective Milestone 005 — Closure Status

Status: conditionally closed
Source implementation plan: `plans/implementation/harness-lab-post-pilot-corrective/005-live-stall-stop-control-coverage.md`
Source subsystem roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`
Repository baseline reviewed: `1ee4e3aa2`
Implementation commits: see §3

## 1. Executive finding

The fault-injection seam and the induced-repeat taxonomy case **landed and are
verified**. The live half of the milestone did **not** run in this environment:
the live exercise needs an operator-approved route and an admitted budget, and
neither was available in the window this milestone was executed in. The
milestone therefore closes on the milestone's own pre-stated fallback
(plan §4.4) and **names what remains unproven**.

Two findings came out of the work and are recorded here rather than buried in
the diff:

1. **The recovery ladder has no terminal stall.** The plan's §2 says the
   ladder's `Stalled` decision "breaks the agent loop". It does not:
   `RecoveryController::observe` in `src/agent/progress_recovery.rs` returns
   only `Progress`, `Continue` or `Recover { action }`. `RecoveryDecision::Stalled`
   is **never constructed** anywhere in `src/`. What bounds a repeated cycle
   is the run/turn budget — ADR-0040 Option B — and the ladder's escalating
   control instruction, not a stall terminal. The plan's stop condition
   ("the recovery ladder does not stop the repeated identical failing call
   when it plainly should — that is a production defect … this milestone
   measures, it does not fix, so report it with the transcript") is met and
   reported here. It is **not** a defect introduced by this milestone and it
   is **not** fixed by it.

2. **The ring buffer cannot cover a broadcast lag.** The natural "re-deliver
   the dropped events" repair is impossible: the event log's ring buffer and
   the bounded broadcast have the same capacity and evict the **same** window,
   so the events a lagged subscriber missed are exactly the ones the ring has
   already evicted. This is recorded in `EventLog::replay_ring_from_any`'s
   doc comment so the next implementer does not attempt it.

The new control is built on the honest version of the capability: the
**exercised** fact is that the identical call really failed again, and the
**stopped** fact is that the trial was stopped at the declared bound rather
than credited with a task it could not do.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| §4.1 A per-case fault specification, applied at the environment boundary, recorded in the receipt | met | `dev/harness_lab/faults.py`; `dev/harness_lab/autonomy.py` (`fault` key); `dev/harness_lab/autonomy_run.py` (`run_case`) |
| §4.1 A declared fault is applied or the trial is refused | met | `faults.FaultSpec.parse` raises `FaultError` for an unsupported tool, mode, key, path or bound; `run_case` turns that into a `HarnessError` before a provider is reached |
| §4.1 The fault is confined to the lane and restored afterwards | met | `faults.apply_fault` writes only inside the lane workspace; the `missing` stash lives outside it; `AppliedFault.restore` runs in the runner's `finally` |
| §4.1 The bound is the declared number of consecutive identical failures | met | `_FaultWatcher` counts the faulted tool's failures from the live notification stream and cancels the turn at the bound |
| §4.1 A fixture-level test that a declared fault produces the declared number of consecutive identical failures, and that a case with no declaration produces none | met | `dev/harness_lab/tests/test_autonomy_faults.py` (26 tests) |
| §4.2 A new supported case in the autonomy taxonomy with its own fixture, verifier and manifest line | met | `dev/autonomy_fixtures/repeated-identical-failure-injected/`; one new line in `dev/harness_lab/fixtures/agent_autonomy/cases.jsonl` |
| §4.2 The manifest's existing case lines are unchanged | met | The new line is additive; no existing line's bytes changed |
| §4.2 The verifier asserts the identical call really failed more than once and the trial's terminal is not `completed` | met | `dev/autonomy_fixtures/repeated-identical-failure-injected/verifier/test_injected_repeat_contract.py` |
| §4.3 The live exercise | **not run** | No operator-approved route with an admitted budget was available in this window. §4.4's fallback was used. |
| §4.4 The fallback is stated in advance and names what remains unproven | met | This record, §10 |
| §7.1 The taxonomy contains a case that induces a repeated identical failing call and its verifier confirms at least one such trial | **not met (live)** | The case and its verifier exist and are exercised by the unit tests; no live trial confirmed one |
| §7.2 The confirmed trial ends in a terminal that is not `completed` | **not met (live)** | Same reason |
| §7.3 `control.no_regression` is measured from a control that was actually exercised, and the receipt distinguishes "exercised and held" from "never exercised" | met | `autonomy_run._control_exercised` and `_control_rate`; `autonomy_run._control_exercise_counts` names the controls that never ran |
| §7.4 The distinct-call half of the group still passes | met, with a caveat | The `repeated-distinct-tool-calls` case is untouched. Its live pass is a property of the last recorded comparison, not of this milestone. |
| §7.5 Either C004 closes on this evidence, or the closure record states the fallback argument explicitly and names what remains unproven | met | §10 |

## 3. Production implementation evidence

### 3.1 The seam — `dev/harness_lab/faults.py` (new)

- `FaultSpec` — the declared fault: `tool`, `path`, `mode`,
  `min_consecutive_failures`, `max_consecutive_failures`. Every rule is a
  **refusal**, not a repair.
- `apply_fault` — applies the fault inside the lane's private copy of the
  fixture. `unreadable` takes the target's read permission away; `missing`
  moves the target into a stash outside the workspace.
- `AppliedFault.reapply` — puts the fault back in force if the agent undid it
  (a `chmod`, a write). The fault is a property of the lane for the whole
  trial.
- `observe_fault` — reports, from the transcript, how many times the faulted
  call really failed, the longest run of consecutive identical failures, and
  whether the declared bound was reached. It reports; it does not judge.

### 3.2 The runner — `dev/harness_lab/autonomy_run.py`

- `run_case` applies the fault before the prompt, starts a `_FaultWatcher`
  once the session exists, cancels the turn at the declared bound, restores
  the fault in `finally`, and records the fault, the observation and the
  restore outcome in both the transcript and the receipt.
- `_ToolCallProjector` — the projection of the ACP notification stream into
  ordered tool-call records, now incremental so the watcher and the transcript
  are two views of one record.
- `_control_exercised` / `_control_rate` — a protected control that was
  **never exercised** is excluded from the rate rather than counted as a
  control that failed to hold, and when no control was exercised the metric
  is **unmeasured**, not zero.

### 3.3 The taxonomy

- New case `repeated-identical-failure-injected` in the `repeated_tool_calls`
  group, `protected: true`, with its own fixture and verifier.
- The suite's manifest digest changes from
  `2b564ab0bc3f1119c49378fd3217ba863261f4676e51f0f663111523ec24725d`
  to `335088946102c7047412e4a65efc2e327e649c396197dcf0f6ab2653944390fb`.
  The C003/C004 closure records keep their historical digests: those records
  are pinned to the runs they describe and are not rewritten.

## 4. Verification executed

| Command | Result |
|---|---|
| `python3 -m unittest discover -s dev/harness_lab/tests` | **316 tests, OK** (1 skipped) — local, not CI |
| `python3 -m unittest discover -s dev/harness_lab/tests -p test_autonomy_faults.py` | 26 tests, OK — local |
| `bash scripts/verify.sh quick` | see §4.1 — local |
| `cargo fmt --all` | clean — local |

### 4.1 `scripts/verify.sh quick`

See the commit message for the recorded result. It covers the agent schema,
the core/client/desktop boundaries, the sandbox, execution ownership, the TUI
authority, the HTTP-route disposition, audit coverage, the queue-delivery
call sites, the queue-write transaction shape, the scheduler bypass, the
config merge coverage, the no-hardcoded-models check, the provider
wire/catalog/resilience checks, the OpenAI endpoint check, the Eggwork target
routing, and `cargo check` over the workspace.

**No live provider call was made by this milestone.** The live exercise
(§4.3) is the only part of the plan that needs one, and it did not run.

## 5. Invariant review

| Invariant | Status |
|---|---|
| The taxonomy's existing case lines, tool surfaces and verifier commands are unchanged | held — the new line is additive |
| A control is never credited because it was never exercised | held — `_control_rate` excludes never-exercised controls and reports `None`, not `0.0` |
| A missing measurement is never read as a zero | held — `observe_fault` reports the observed count; the control metric is `None` when nothing was observed |
| The fault is confined to the lane and never touches another trial's workspace | held — `apply_fault` resolves the target inside the workspace and refuses a path that leaves it |
| A fault the seam cannot produce is refused, not degraded | held — `FaultSpec.parse` and `apply_fault` both raise |
| No candidate code is merged or pushed (plan §9) | held — no production code changed |

## 6. Failure and recovery review

- A fault that cannot be applied refuses the trial before a provider is
  reached. The refusal is a `HarnessError` naming the case and the reason.
- A fault that cannot be restored is recorded in the receipt as
  `restore_error`; it never raises out of the runner.
- The watcher's cancel is recorded as `cancel_error` when it fails. A trial
  whose cancel failed is **not** claimed as stopped: its terminal says what
  the ACP call returned.
- The trial is bounded even when the agent never gives up: the fault's bound
  cancels the turn, and the case's own abort window bounds it again.

## 7. Migration and compatibility review

- The taxonomy manifest gains one case line. Every consumer that records the
  suite digest records the new one; the historical records are not rewritten.
- `AutonomyCase` gains an optional `fault`. A case with no declaration
  behaves exactly as before: the receipt's `fault` is `None` and the trial is
  untouched.
- The control metric's meaning changed: it is now measured over the
  **exercised** controls. A comparison whose controls were never exercised is
  **undecidable** rather than accepted or rejected. That is the direction the
  plan asks for — "the receipt distinguishes 'exercised and held' from 'never
  exercised'" — and it makes an unexercised control fail safe.

## 8. Security review

- The fault touches only files inside the lane's private workspace and one
  stash directory outside it. It starts no process, opens no socket and reads
  no credential.
- The fault is restored when the trial ends, and the watcher keeps it in
  force while the trial runs.
- No credential, prompt or raw body is written to any plan or receipt.

## 9. Documentation and operations

- `dev/harness_lab/README.md` documents the fault seam, its refusals and how
  a case declares a fault.
- The addendum records this milestone's findings and its status.

## 10. Unresolved findings

| Severity | Finding |
|---|---|
| **high** | **The live half did not run.** No operator-approved route with an admitted budget was available in this window, so the induced-repeat control has **no live trial** behind it. What remains unproven: (i) that a live agent, meeting an environment-induced failure, repeats the identical failing call rather than investigating differently — the C004 observation was that it investigated differently; (ii) that the trial then ends in a terminal that is not `completed`; (iii) therefore that `control.no_regression` can be measured from an exercised control in a live comparison. The deterministic half of the pair — the ladder's incident detection keyed on the whole `action_key()` — is covered by the ledger suite's `repeated-identical-read-error` case and by this milestone's own unit tests. |
| **medium** | **`RecoveryDecision::Stalled` is unreachable.** The ladder escalates a control instruction and the run/turn budget bounds the cycle; no turn ends in a stall terminal. C004's blocked-work row names a stall terminal as the control's expected outcome, so the taxonomy's expected terminal set contains a state the product cannot produce. **This milestone measures and reports it; it does not fix it.** A successor that adds a real stall terminal is a product change with an ADR question attached, not a harness change. |
| **low** | The induced-repeat case's fault is `unreadable`, which a `bash chmod` could undo. The watcher puts it back in force, so the control holds; but an agent that chmods and re-reads successfully would produce a successful read of the target, which the verifier's "the faulted call never succeeded" assertion would catch. |
| **low** | `autonomy_run._success_rate` now excludes `infrastructure_error` trials from its denominator. A candidate that produced **only** infrastructure errors leaves the metric unmeasured rather than failed. That is the plan's intent ("an infrastructure error is an unmeasured trial"), and it fails safe: the comparison becomes undecidable, not accepted. |

## 11. Roadmap disposition

- The addendum's F007 ("the stall-stop control is never exercised") is
  **partly** closed: the control can now be exercised, deterministically and
  by induction, and the harness distinguishes exercised from never-exercised.
  The live confirmation is not closed.
- The addendum's F008 ("live-trial integrity and cumulative resume deadline")
  is the subject of milestone 006 and is unaffected by this one.
- C004 stays **conditionally closed**. This milestone does not close it: the
  live half did not run, so the control's live confirmation is still absent.

## 12. Registry updates

- `plans/registry.md`: this milestone's row moves from `ready` to
  `conditionally closed`; the C004 blocked-work row keeps its condition and
  now names this record.
- Nothing else is unblocked. No other registered plan listed C005 as a
  dependency.
