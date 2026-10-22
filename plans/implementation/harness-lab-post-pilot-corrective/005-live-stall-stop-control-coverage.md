# Harness Lab Post-Pilot C005 — Live Coverage of the Taxonomy's Stall-Stop Control

Status: ready for handoff
Repository baseline: `plans/harness-lab-post-pilot-correctives@60484c020` (2026-10-22; inspect latest at handoff).
Source roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md#5-milestones`
Corrects C004 `plans/closure/harness-lab-post-pilot-corrective/004-status.md` §5 condition 4; preserves the closed C001–C003 records and the preserved comparison receipt `plans/closure/harness-lab/evidence/autonomy-comparison-20261022.json`.
Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26, 28–29; `plans/001-terminology-and-domain-model.md`.
Primary class: capability.

## 1. Objective

Make the autonomy taxonomy's **protected stall-stop control** exercise the
capability it exists to guard, in a live trial, and record that exercise as
evidence — so that C004 can close on a control that demonstrated something
rather than a control that demonstrated nothing.

Today the control is vacuous in practice. `repeated-identical-failure-stall`
asks the agent to read `fixture/absent-report.txt`, which does not exist. In
every live trial so far the agent read it, the read failed, and the agent then
**investigated differently** — it listed the directory and read another file —
and correctly reported that the file does not exist. The identical failing call
was never repeated, so the stall-stop branch was never entered, the control's
verifier refused to confirm the trial, and `control.no_regression` measured
0.0 on both sides of the comparison. The control reported the truth
faithfully; it simply never got the situation it is about.

The capability under test is CodeGG's **recovery ladder**: a genuinely
repeated identical failing call must stop the trial, and a run of *distinct*
calls must not. The `repeated_tool_calls` group already pairs those two
claims. The distinct-call half is covered and passed. This milestone covers
the repeated-failure half in a live trial.

## 2. Evidence and readiness

- The taxonomy, its loader, its isolation guard and all six case bodies exist
  (`dev/harness_lab/fixtures/agent_autonomy/cases.jsonl`,
  `dev/autonomy_fixtures/`), and every verifier is proven by execution to fail
  unsolved, pass solved and refuse an edited frozen file
  (`dev/harness_lab/tests/test_autonomy_cases.py`).
- A live runner executes a case as an agent task with the verifier kept
  **outside** the agent's workspace, and records a transcript of the tool
  sequence, each call's argument digest and each call's terminal status
  (`dev/harness_lab/autonomy_run.py`).
- The recovery ladder that owns the behaviour lives in CodeGG, not in the
  harness: `src/agent/progress_recovery.rs` keys every incident on the whole
  `action_key()` (tool **plus** normalized arguments), and its `Stalled`
  decision breaks the agent loop.
- The harness already has fault injection for live trials — `cancel` and
  `abrupt_acp_drop` in `dev/harness_lab/sustained.py` — and a provider fault
  fixture (`dev/harness_lab/provider_fixture.py`). The seam this milestone
  needs is adjacent, not novel.
- The deterministic ledger suite already proves the *terminal* half of the
  claim without a model: `dev/harness_lab/fixtures/smoke.jsonl` carries
  `repeated-identical-read-error` with the expected terminal
  `execution_failed`. That coverage is real and stays; this milestone adds the
  **live, model-driven** half.

## 3. Invariants and non-goals

- The control is a **control**. It is never credited as a solved task, never
  contributes to `correctness.pass_rate`, and its expected outcome is that the
  task is **not** solved. `dev/harness_lab/autonomy_run.py` already makes a
  protected case structurally unsolvable; that must not change.
- A control that was **not exercised** is reported as not exercised. It must
  never be recorded as a pass, and `control.no_regression` must never be
  credited with a control that never entered its branch.
- The harness must not induce the repeat by **prompting** for it in a way that
  teaches the model to loop, and must not count a model that loops because it
  was told to as evidence that the ladder works. The induction must be a
  property of the environment, not of the instruction.
- The taxonomy's manifest bytes, case prompts, tool surfaces and verifier
  commands are frozen. A change to them must be a separate, recorded decision
  — never an edit made to make a control fire.
- No production daemon, ACP protocol, provider or agent-loop change is in
  scope. This milestone measures CodeGG as it is; if the ladder is wrong, that
  is a finding to report, not a defect to fix here.
- No automatic merge or push of candidate code. The live run happens in a
  private lane; the harness changes land in this repository, the candidate
  does not.

## 4. Work packages

### 4.1 A fault-injection seam in the autonomy runner

*Intent:* give the runner a way to make one named tool call fail
deterministically and repeatedly, so the situation the control is about is
produced by the environment rather than by the prompt.

*Changes:* the runner gains an optional per-case fault specification (which
tool, which argument digest, how many consecutive failures). The fault is
applied at the tool-dispatch boundary inside the private lane, is recorded in
the trial's receipt, and is applied **only** to the case that declares it. A
case with no fault specification behaves exactly as it does today.

*Acceptance evidence:* a fixture-level test that a declared fault produces the
declared number of consecutive identical failures with the identical argument
digest, and that a case with no declaration produces none.

### 4.2 An induced-repeat case

*Intent:* a taxonomy case whose environment makes the identical failing call
the natural next action, so the ladder has something to stop.

*Changes:* a new supported case in the autonomy taxonomy (a new `case_id`, its
own `fixture/` and `verifier/`, its own manifest line) that pairs with
`repeated-distinct-tool-calls` in the same `group_id`. Its fault makes the
first attempt at a named read fail repeatedly. Its verifier asserts what the
control is about: the identical call really failed more than once, and the
trial's terminal is **not** `completed` — the trial was stopped, not credited.

*Acceptance evidence:* the case's three standing properties (fail unsolved,
pass solved, refuse an edited frozen file) hold like every other case's, and
the case is refused by the runner if its fault specification is absent — a
control that cannot be induced is never silently run without one.

### 4.3 The live exercise

*Intent:* run the induced-repeat case against an approved route and record the
terminal.

*Changes:* none to the harness beyond the runner's existing receipt shape.
Run at least five trials of the case and record, per trial: the transcript,
the repeated identical failing call, the trial's terminal, and the verifier's
verdict.

*Acceptance evidence:* at least one trial whose verifier confirms the trial,
and the receipt states the observed terminal.

### 4.4 The fallback, stated in advance

If the induction does not reliably produce the repeated identical failing call
— the agent may keep investigating differently, exactly as it did in the
un-injected case — then the milestone **records that outcome** and closes on
the deterministic coverage argument instead: the ledger suite's
`repeated-identical-read-error` case proves the terminal, and the live trial
proves that a *well-behaved* agent investigating a missing file is not
mistaken for a stall. Both halves of the pair then have evidence, from two
different instruments, and the taxonomy's limitation is written down as a
limitation.

**This fallback must be a recorded, explicit decision in the closure record.
It must never be reached by quietly editing the control's expectations or by
dropping the case.**

## 5. Failure, restart and contention semantics

- A trial whose lane daemon fails to start, whose socket path does not fit
  the platform's `sun_path` limit, or whose budget admission is refused is
  recorded as `refused` with the reason. It is never counted as a control
  exercise.
- A trial that is interrupted is recorded as `interrupted_unknown` and is not
  counted as an exercise in either direction.
- The induced fault applies to the lane's private workspace only. It must not
  touch a path outside the lane, and it must be undone when the lane is torn
  down.
- Trials run sequentially per case. Concurrent trials against the same lane
  root are refused, exactly as the comparison runner refuses them today.

## 6. Required tests and verification

- `python3 -m unittest discover -s dev/harness_lab/tests -v` — the new fault
  seam, the new case's standing properties, and no regression in the 276
  existing tests.
- The new case's verifier must fail on the unsolved case, pass on the repaired
  case and refuse an edited frozen file, proven by execution in the same
  standing test that covers every other case.
- A test that a case declaring no fault behaves exactly as before: no injected
  failure, and the transcript's failure statuses come from the tool, not from
  the harness.
- `scripts/verify.sh quick` passes.
- The live exercise is manual and separately labeled: its receipt names the
  route, the binary's SHA-256, the admitted budget, and the per-trial
  terminals. It is not part of CI.

## 7. Acceptance criteria

1. The taxonomy contains a case that induces a repeated identical failing call
   in a live trial, and its verifier confirms at least one such trial.
2. The trial that the control confirms ends in a terminal that is **not**
   `completed` — the trial was stopped.
3. `control.no_regression` is measured from a control that was actually
   exercised, and the receipt distinguishes "exercised and held" from "never
   exercised".
4. The distinct-call half of the group still passes: a run of distinct calls
   is not stopped.
5. Either C004 closes on this evidence, or the closure record states the
   fallback argument explicitly and names what remains unproven.

## 8. Compatibility and migration

None. The taxonomy manifest gains one case line; the runner gains an optional
fault specification. The manifest's digest changes, and every consumer that
records it must record the new one — the digest is the taxonomy's identity, and
a stale digest is a stale case set.

## 9. Documentation and static guards

- `dev/harness_lab/README.md` documents the fault seam, the induced-repeat
  case, and the control's two evidence instruments.
- `plans/closure/harness-lab-post-pilot-corrective/004-status.md` is updated
  with the outcome — a closure, or the fallback argument — and with the
  unblock audit.
- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md` §8 and
  `plans/registry.md` are updated in the same commit.

## 10. Handoff and closure evidence

The closure record `plans/closure/harness-lab-post-pilot-corrective/005-status.md`
must carry: the requirement-to-evidence matrix; the exact SHA; the tests run
with their results; the per-trial transcripts and terminals; the control's
classification; the manifest's new digest; and an audit of the registry's
blocked work for anything this unblocks.

**Stop conditions.** Stop and report rather than improvise if: the recovery
ladder does not stop the repeated identical failing call when it plainly
should — that is a production defect in `src/agent/progress_recovery.rs`,
and this milestone measures, it does not fix, so report it with the
transcript; the induced fault cannot be confined to the lane's workspace; the
approved route's budget cannot be admitted; or the change would require
editing a frozen prompt, tool surface or verifier command.
