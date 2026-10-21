# Harness Lab Post-Pilot Corrective 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-lab-post-pilot-corrective/002-scoring-and-resumable-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`
- `plans/subsystems/harness-lab-roadmap.md`

Repository baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`

## 1. Executive finding

The harness now decides candidate acceptance through a versioned score contract
instead of a prose comparison, refuses an incomplete or identity-mismatched
comparison instead of scoring a smaller denominator, and survives a crash
without duplicating a committed row or silently re-running an interrupted
attempt.

The prior failure mode was that `ledger.compare` paired whatever rows it
happened to find. A partial pairing shrank the denominator silently, so a
candidate could look better than it was by being *less* completely evaluated
than its baseline. `compare` now refuses an incomplete pairing, a comparison
of a candidate against itself, a zero denominator, and an identity mismatch
between the arms.

The prior failure mode on the resume path was that a crash between a
checkpoint's JSONL append and its pointer replacement made the run
unresumable with an opaque error string, and a crash between the ledger's
manifest write and its index append left an orphan attempt that no reader
could see. Both now have a recorded terminal: `interrupted_unknown`. The
orphan is reconciled into the ledger rather than lost, and a resumed run
appends no duplicate row.

The prior failure mode in the scoring was that "did the candidate improve?"
had no contract, so a candidate that merely got faster and a candidate that
fixed a real defect were indistinguishable, and a candidate that broke a
protected negative control looked like a candidate that improved everything.
The contract separates those cases: a protected control regression is a
safety regression that rejects the candidate outright, a correctness fix is
reported as `correctness_fix`, and an evidence gap is reported as
`insufficient_evidence` rather than being coerced into a verdict.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| §4 contract: versioned score contract outside candidate worktrees | `dev/harness_lab/scoring.py`, `SCORE_CONTRACT_VERSION = 1`; `check_scope` already forbids candidate patches from touching `dev/harness_lab/` | Met |
| §4 contract: correctness fix vs reliability/efficiency distinguished | `objective` is `correctness_fix` / `reliability` / `efficiency` / `none`; `improve.py` maps trial rows into `scoring.Observation`s | Met |
| §4 contract: protected negative controls | `control.no_regression`, `protected=True`; excluded from the improvement set, the objective, and the weighted aggregate | Met |
| §4 contract: explicit insufficient-evidence outcome | `DECISION_INSUFFICIENT_EVIDENCE` with `score is None` | Met |
| §4 contract: predeclared regression floors | `Criterion.regression_blocks`; efficiency is advisory, correctness and reliability block | Met |
| §4 compare: complete paired repeats required | `compare` refuses a partial pairing, a self-comparison, and a zero denominator | Met |
| §4 compare: digests and identities validated | group/case/suite digests, revision, agent, policy, subject binary fingerprint, model, tool surface | Met |
| §4 compare: null usage/cost never equals zero | `report()` and `compare()` keep unmeasured aggregates `None`; `usage_unavailable` carries a named reason | Met |
| §4 crash-safe: recorded `interrupted_unknown` terminal | `TERMINALS` gained `interrupted_unknown`; `CheckpointInterrupted`; `run_interrupted` events | Met |
| §4 crash-safe: no duplicate rows on resume | `test_resumed_run_does_not_append_a_second_row_for_a_recorded_pair` | Met |
| §4 crash-safe: orphan reconciliation | `load_runs` reconciles an orphan run directory into an `interrupted_unknown` manifest | Met |
| §4 crash-safe: explicit ledger-append acknowledgment | `record()` writes the manifest, then the index row, then reconciles an orphan on the next load | Met |
| §5-A: 12 cases classified into objectives vs controls | `protected_control_keys()` derives the control set from `expected_terminal != "passed"`; the suite schema is untouched | Met |
| §5-A: all-matched suite yields "no improvement possible" | `test_fully_matched_candidate_is_not_accepted` | Met |
| §5-B: metric traps and mixed profiles | `compare` refuses a subject-identity mismatch and a revision mismatch | Met |
| §5-C: crash matrix with real subprocesses | Four SIGTERM/SIGKILL tests in `dev/harness_lab/tests/test_improve.py` | Met |
| §5-D: provider-free E2E | `dev/harness_lab/provider_fixture.py` fault injection; `test_provider_fixture_faults.py` | Met |
| §3: historical 12-case fixture and pilot receipts immutable | `canonical_digest` unchanged; no per-case field added | Met |

## 3. Implementation and safety evidence

### 3.1 The score contract

`dev/harness_lab/scoring.py` holds `ObjectiveV1`/`ComparisonReceiptV1` in the
shape the plan allows the names to take: a `ScoreContract` of `Criterion`
values, an `Observation` per criterion per side, and a `ScoreOutcome`.

The decision order is:

1. A protected negative control regressed → **rejected**, whatever else
   changed. This check runs first and is not overridable.
2. `evidence_pairs < minimum_evidence_pairs` → **insufficient_evidence**,
   `score` is `None`.
3. A required criterion unmeasured on either side → **insufficient_evidence**,
   `score` is `None`.
4. Otherwise a weighted comparison over the criteria measured on both sides →
   **accepted** when at least one criterion improved and no *blocking* floor
   regressed.

A `None` measurement is never coerced to `0`, anywhere. A `score` is `None`
whenever the decision is `insufficient_evidence`, so a missing measurement
cannot be represented as a zero score.

**Regression floors.** `Criterion.regression_blocks` is the contract's
"primary regression floor". In the default contract `correctness.pass_rate`
and `reliability.success_rate` block, while `efficiency.duration_ms` is
advisory. This is a deliberate reading of §4: the plan's correctness-fix type
requires the controls to hold and the defect to be fixed, and does not make a
slower run disqualifying — while a candidate that trades correctness or
reliability for speed must still be rejected. A candidate that fixes a real
defect and is slower is accepted, and the latency regression is recorded in
the receipt as `secondary_criterion_regressed`.

### 3.2 Protected negative controls

The control set is derived, not stored, so the frozen suite schema and every
stored baseline digest stay untouched: a case is a protected negative control
when its `expected_terminal` is not `"passed"`. The suite schema permits only
`passed`, `assertion_failed`, `cancelled`, `timeout`, and `execution_failed`,
so every non-`passed` expectation is a deliberately negative case. An
unknown non-`passed` terminal is refused rather than guessed.

In the smoke fixture the controls are `repeated-identical-read-error`
(`execution_failed`), `cancel-after-read` (`cancelled`), `timeout-after-read`
(`timeout`), and `verifier-negative` (`assertion_failed`).

### 3.3 The hardened comparator

`ledger.compare` pairs by `(task_id, repeat)` and refuses, with a message
naming the problem:

- a comparison of a candidate against itself;
- a zero expected denominator;
- an **incomplete pairing** — the pairing is smaller than the repeat-expanded
  expected pair set, and the message names the missing pairs;
- a duplicate `(task_id, repeat)` inside one arm;
- a disagreement between the arms on `revision`, `agent`, `policy`, or the
  frozen group/case/suite digests;
- a disagreement inside one arm on the subject binary fingerprint, revision,
  agent, model, policy, tool-surface digest, or suite digest.

The subject binary fingerprint is enforced **within** an arm, not across arms.
Cross-arm equality would refuse every candidate qualification, because the
baseline and candidate arms are different builds by construction; across arms
the enforced identity is the revision, the agent, the policy, and the frozen
digests.

### 3.4 Crash-safe resume

`interrupted_unknown` is now a recorded terminal, distinct from a finished
trial. A crash between a checkpoint's JSONL append and its pointer replacement
is detected on the next resume: the checkpoint records an
`INTERRUPTED_UNKNOWN` event naming the pointer sequence, the unpointed
sequences, and the unpointed state, then raises `CheckpointInterrupted`.
The checkpoint is loadable again afterwards and a repeated resume appends no
duplicate event. The interrupted attempt is never silently resubmitted.

A crash between the ledger's manifest write and its index append leaves an
orphan `runs/<run_id>/` directory. `load_runs` reconciles it into an
`interrupted_unknown` manifest with every measurement field `None` — no
fabricated usage — and the attempt is visible instead of lost.

A torn **final** line in `index.jsonl` or `events.jsonl` is tolerated, because
that is the classic crash-during-append signature, and a subsequent `record()`
still succeeds. Corruption in the **middle** of a file is still a hard error.

### 3.5 Ledger schema versioning

`SCHEMA_VERSION` moved from 1 to 2 with `SUPPORTED_SCHEMA_VERSIONS = (1, 2)`.
Index rows, event rows, and run manifests are all range-checked, so the stored
pilot ledgers remain readable while a v2 row is also accepted. A v1 row read
back defaults its `usage_unavailable` reason to `not_reported` when its usage
is entirely absent.

## 4. Verification executed

See `plans/closure/harness-lab-post-pilot-corrective/002-evidence.md` for the
verbatim command output.

The crash matrix uses **real subprocesses and real signals**, which was the
explicitly identified gap in the pilot:

| Test | Signal | Assertion |
|---|---|---|
| `test_sigkill_between_checkpoint_append_and_pointer_is_recorded` | `SIGKILL` | The resume raises `CheckpointInterrupted` naming `interrupted_unknown`; an `INTERRUPTED_UNKNOWN` event is recorded; the checkpoint loads again; a repeated resume appends no duplicate |
| `test_sigterm_between_optimization_append_and_pointer_is_recorded` | `SIGTERM` | A `run_interrupted` event with `outcome == "interrupted_unknown"` is recorded and the pointer is reconciled; a repeated resume appends no duplicate |
| `test_sigkill_during_ledger_write_leaves_no_duplicate_trial_rows` | `SIGKILL` | The ledger still loads; the orphan is never reported as `passed` and is not a scored trial; the resumed attempt commits exactly one index row |
| `test_resumed_run_does_not_append_a_second_row_for_a_recorded_pair` | — | The pending set is exactly the missing pairs; a duplicated completed pair is refused instead of being scored twice |

## 5. Remaining conditions

None that block this plan. Two findings are recorded for C003 rather than
fixed here:

1. **The OpenCode CLI version pin is stale.** `improve.py` pins
   `SUPPORTED_OPENCODE_VERSION = "1.19.0"` and compares it for exact string
   equality; the installed CLI reports `1.19.3`. The pinned preflight
   therefore refuses the installed binary. The plan requires the *actual
   installed* version and its documented `run --auto --format json`
   semantics, so re-pinning is C003 work.
2. **The integrated partial-candidate path is refused by the ledger before
   the contract sees it.** `ledger.compare` raises on an incomplete pairing,
   so a partially evaluated candidate is refused at the ledger rather than
   reaching the contract's `insufficient_evidence` branch. Both refusals are
   correct and safe; the contract's partial-candidate handling is unit-tested
   directly. The ledger-level refusal is the stronger gate and was kept.

## 6. Planning audit

- No plan file was edited to make this implementation look complete.
- `canonical_digest` was not changed and no per-case field was added, so the
  frozen 12-case suite and the stored pilot receipts are unchanged.
- The score contract lives under `dev/harness_lab/`, which `check_scope`
  already protects from candidate patches.
- No live provider call was made; every test in this closure uses the local
  fixture or a local fake process.
- The Python harness suite was wired into `scripts/verify.sh quick` and the CI
  workflow, so the evidence above is a standing guard rather than a one-off
  claim.

## 7. Registry updates

- `plans/registry.md`: the C002 milestone row moves from `in_progress` to
  `closed` and points at this record.
- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`: C002
  moves from `ready` to `closed`; C003's dependency on C002 is discharged.
- `plans/subsystems/harness-lab-roadmap.md`: the post-pilot corrective
  milestone row records the closure.
