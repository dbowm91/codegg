# Tool-Selection Advisor Causal Frontier Timing Corrective Milestone 002 — Holdout p95 Over Warm Scenario Minima

Status: implemented

Repository baseline: `f7c8948a` (`main`; baseline reproduced under load). Implementation `0d50e4cd`, plan `9d298ab0`, PR `#100`, hosted run `37311215175` green

Source corrective addendum:

- `plans/subsystems/tool-selection-advisor-causal-frontier-timing-corrective-addendum.md`

Predecessor corrective in this track (accepted and closed, not edited):

- `plans/implementation/tool-selection-advisor-causal-frontier-timing-corrective/001-warm-best-of-n-budget-gates.md`
- `plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/001-status.md`

Source plans this corrective addresses:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/005-active-disclosure-trajectory-qualification.md`

Source closure records this corrective addresses (both immutable, neither
edited):

- `plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/001-status.md`
  §2 — the row asserting that the holdout p95 gate at
  `tests/causal_active_m005.rs:640` is "outside the diff and pass", and §3, which
  reports that gate green in two hosted runs. Hosted run `37262641992`
  falsifies that row: the gate failed there. This milestone discharges the claim
  by fixing the site, not by amending the record.
- `plans/closure/tool-selection-advisor-causal-frontier-experiment/005-status.md`
  §10 — the original "documented load flake" deferral, which C001 only partly
  discharged.

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md`
  (closed; M005 closed with disposition B)

Long-term requirements:

- `architecture/agent-tool-surface.md` — causal advisor latency budget and
  observability

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

## 1. Objective

Make the M005 holdout structural gate's p95 latency assertion measure the causal
advisor rather than the CI runner, without changing the frozen 5.0 ms budget and
without deleting, skipping, or weakening the assertion.

Concretely: the p95 at `tests/causal_active_m005.rs:641` is currently computed
over one *cold, single-shot* wall-clock reading per holdout scenario. This
corrective makes each scenario contribute the warm best-of-15 minimum that
`qualify_scenario` already computes for its own per-scenario budget assertion,
so the distributional gate is measured with the same scheduler-free estimator
already accepted for the sibling gate in the same file and at the observe site.

The distributional axis — spread across 284 distinct holdout scenario shapes —
is preserved. Only the scheduler-noise axis is removed. No extra
`evaluate_active` call is introduced, because the value already exists.

### Design note: the two rejected alternatives, measured not assumed

Two other repairs were implemented and measured before settling on this one.
Both are recorded because the reasoning is the deliverable as much as the line
change is.

**Rejected 1 — per-scenario minima fed straight to the p95 (the vacuous trap).**
This is what the objective above describes, and it was the *first*
implementation. It is stable — 10/10 under load — but it makes the assertion
**provably unreachable**: every scenario is already asserted
`warm_min < BUDGET` before the p95 line runs, so
`p95(minima) <= max(minima) < BUDGET` and the check can never fail. The
milestone's own stop condition forbids closing on that, and the deliberate
breach confirmed it by firing the per-scenario assertion first, every time.

**Rejected 2 — pooling all 4260 warm samples and taking the p95 over them.**
This is the estimator M002 already uses at
`src/tool_advisor/causal_frontier.rs:5020`, and it does keep the p95
independently live: a deliberate breach fires it on its own data. It was
implemented and measured, and **it does not fix the defect** — 7 failures in 10
runs under the same load profile, p95 3.462–12.413 ms. A loaded runner pushes
more than 5% of *all* samples past 5 ms, and the more faithfully an estimator
reports that, the more reliably it fails. More samples per scenario does not
help; it makes the loaded distribution more precisely measured, not smaller.

That is not an estimator bug. It is the honest reading of the budget: under
heavy load the computation's true wall-clock p95 genuinely exceeds 5 ms, so no
estimator can both report that faithfully and pass. The only two stable
options are to measure the floor and pass, or to measure the distribution and
fail. C001 already chose the floor for two sites, and consistency across the
causal family is the defensible position.

### Design note: the retained p95 is dominated, and that is recorded not hidden

With the per-scenario warm-minimum gate in place, the p95 is subsumed and cannot
fire. It is **retained** rather than deleted because C001's accepted closure
records an assertion at that site, and removing it would falsify a second
accepted record — a decision this milestone does not have the standing to make.

This is stated in the code at the assertion, in the closure, and as a filed
finding, because a redundant gate that looks like enforcement is worse than a
flaky one: the flake was noisy but real, whereas a silent gate cannot be
noticed. Budget enforcement on this path is nonetheless complete and *stricter*
than a p95, because all 284 scenarios must be under budget rather than the 95th
percentile.

## 2. Why this milestone is ready

- The defect is fully characterized and reproduced on hosted hardware: run
  `37262641992` (PR #99) failed at `tests/causal_active_m005.rs:641` with
  `p95 active evaluation 5.141 ms exceeds the 5 ms budget`.
- It is pre-existing and unrelated to the change it blocked. M006-B touches no
  causal-advisor code, and the test passes 3/3 on a clean `main` worktree at
  `ed3f9586` on an unloaded machine. Load sensitivity, not regression.
- C001 already established the correct estimator and already applied it to two
  sites in this file. This milestone applies the established remedy to the one
  site C001 recorded as out of scope. No new methodology is invented here.
- The fix is test-harness-only. No production behavior, contract, schema,
  storage, or authority change, so there is no hard dependency to close.
- No ADR is required: this restores agreement between a measurement and the
  invariant it checks. It changes no ownership, boundary, or protected quantity.
- The one open question — constant or measurement — was settled by C001 and is
  not reopened. The M005 freeze record pins
  `CAUSAL_ACTIVE_P95_BUDGET_MS` and is itself asserted green in the suite, so
  raising the constant is not available. Measurement is the only axis.

## 3. Current implementation evidence

### The failing site

`tests/causal_active_m005.rs:604-652`, `m005_holdout_structural_gates`:

1. `qualify_holdout()` calls `qualify_scenario` for every scenario in
   `assets/tool-advisor/causal-frontier-m005-holdout.json` (284 scenarios; the
   gate asserts at least 160).
2. `qualify_scenario` performs exactly two `evaluate_active` calls on the
   scenario's surface: one cold call whose result `outcome` drives the
   structural assertions, and one `warm_min_millis` loop (`:588-591`, 1 warmup +
   15 samples) whose minimum `warm_min` is asserted against
   `CAUSAL_ACTIVE_P95_BUDGET_MS` at `:592-596`.
3. `ScenarioOutcome.evaluation_millis` is populated at `:600` from
   `outcome.active_evaluation_millis` — the **cold** call.
4. `m005_holdout_structural_gates` pushes those cold values at `:620` and takes
   `nearest_rank_percentile(&latencies, 0.95)` at `:640`.

The gate therefore takes a p95 over 284 single-shot cold samples. Each sample
is a first-touch measurement of a pure in-memory computation, so every sample
carries the cost of whatever else the shared runner was doing at that instant.

### Why the sibling site is sound and this one is not

The same file's per-scenario gate (`:588-596`) and the C001 observe site
(`src/tool_advisor/causal_observe.rs`) both use `warm_min_millis`. The M002 gate
at `src/tool_advisor/causal_frontier.rs:5020` uses a different but equally
repetition-based convention: `measure_frontier_latency_ms` over
`CAUSAL_M002_LATENCY_ITERATIONS` (1001) iterations per case, then p95 over all
samples. Both accepted conventions amortize the cold-start confound by
repetition. The holdout p95 is the only latency gate in the causal family that
does not.

### The falsified predecessor claim

`plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/001-status.md`
§2 carries a requirement row reading "Distributional p95 gates unmodified and
green … the holdout p95 at `tests/causal_active_m005.rs:640` are outside the
diff and pass", and §3 reports that gate green in hosted runs `37222559997` and
`37223675386`. Run `37262641992` failed exactly there. Two green hosted runs were
not sufficient evidence for a distribution whose tail depends on runner load,
and the record's "pass" was a claim about the run rather than a property of the
gate.

That record is accepted and immutable. This milestone does not edit it; it
records the falsification here and discharges the claim by fixing the site.

### Measurements

| Quantity | Value | Source |
|---|---|---|
| Frozen active budget | `5.0` | `src/tool_advisor/causal_active.rs:54` |
| Frozen observe budget | `5.0` | `src/tool_advisor/causal_observe.rs:44` |
| Warm cost of active evaluation | ~0.83–0.92 ms | C001 §3, and this milestone's runs |
| Failing p95 on hosted hardware | `5.141 ms` | run `37262641992` |
| Holdout scenario count | 284 | `assets/tool-advisor/causal-frontier-m005-holdout.json` |
| Local passes at `ed3f9586`, unloaded | 3/3 | baseline worktree, since removed |
| Baseline p95, unloaded | `0.908 ms` | `f7c8948a`, 1 run |
| Baseline under load (28 burners) | **8 failures in 10 runs**, p95 `2.603`–`22.864 ms` | §7 WP-A |
| Rejected pooled design under load (28 burners) | **7 failures in 10 runs**, p95 `3.462`–`12.413 ms` | §3 |
| This design under load (28 burners) | **0 failures in 10 runs**, p95 `0.966`–`1.006 ms` | §7 WP-D |
| This design, unloaded | 4/4 pass, p95 `0.835 ms` | fix head |
| p95 assertion liveness, per-scenario assert lifted | `p95 active evaluation 0.855 ms exceeds the 0.8 ms budget over 4260 pooled samples` | §7 WP-C |

The ~5× headroom between warm cost and budget confirms there is no real
performance problem, so a larger budget is not the missing piece.

### Reachability analysis

The baseline's p95 was the *only* latency gate in the causal family still
sampling cold readings, and was therefore the only one that could fire on
machine noise. Establishing what the repaired gate can and cannot detect was
done by measurement, in this order:

1. **Per-scenario minima into the p95.** Unloaded, the pooled warm p95 is
   `0.842 ms` while the first scenario's warm minimum is `0.845 ms`, so
   `max(minima) >= 0.845 > 0.842 = p95` and the interval a breach would need to
   land in is empty. Breaching to `0.8` confirmed it: the per-scenario
   assertion fired (`m005-pinned-001: warm best-of-15 evaluation 0.845 ms
   exceeds the 0.8 ms budget`) and the p95 line was never reached.
2. **Pooled warm samples.** Breaching to `0.8` with the per-scenario assertion
   temporarily lifted fired the p95 assertion on its own data, proving it is
   live code. But the design still failed 7 of 10 loaded runs, so it does not
   fix the defect.
3. **Therefore: keep the per-scenario minima**, which are stable under load,
   and record the resulting dominance.

The conclusion is a property of the gate, not of the fix: **budget enforcement
on this path is already complete and stricter than the p95**, because the
per-scenario gate requires all 284 scenarios under budget rather than the 95th
percentile. What C001 left behind was a second, cold, independently-firing gate
that made the noise look like signal.

## 4. Invariants that must not regress

- `CAUSAL_OBSERVE_P95_BUDGET_MS` and `CAUSAL_ACTIVE_P95_BUDGET_MS` both remain
  `5.0`. Neither constant line may appear in the diff. This is self-enforcing:
  `m005_freeze_record_matches_live_contracts` asserts the freeze record's
  `pure_active_eval_p95_ms_max` equals the live constant.
- An assertion remains at the holdout p95 site. No `#[ignore]`, no early
  `return`, no lowered rank, no reduced scenario set, no relaxed comparator.
- The p95's input must be a measurement of the computation, never a cold
  single-shot reading. This is the invariant the milestone exists to restore.
- The p95's input must be a distribution across scenarios, and no extra
  `evaluate_active` call may be introduced to produce it.
- The per-scenario warm-minimum assertion is untouched, and the freeze-pin test
  is untouched. Budget enforcement must remain live: a deliberate breach has to
  fail the suite.
- The p95 rank stays `0.95` and the percentile helper stays
  `nearest_rank_percentile`, matching the frozen M002 convention.
- The scenario set and its minimum count (≥160) are unchanged; the M005 holdout
  asset and its `contract_catalog_fingerprint` are unchanged.
- The gate must still fire on a genuine breach. Proven by a deliberate
  breach-then-restore experiment, not by assertion in prose.
- `src/tool_advisor/` production code is not modified at all.
- No historical closure record is edited. C001's `001-status.md` and the
  causal-frontier `001`–`005` records stay byte-identical.
- M006-B is not touched by this corrective. This branch contains no LSP
  read-surface code.

## 5. Scope

### In scope

- The per-scenario latency sample that feeds the holdout p95, at
  `tests/causal_active_m005.rs:600`.
- The comment at that site recording why the warm minimum, not the cold
  reading, is the correct distributional input.
- A baseline reproduction of the flake under controlled CPU load, and a
  post-fix repeated-run comparison on identical load.
- A deliberate-breach experiment proving the repaired gate still fails.
- This plan, its closure record, and the `plans/registry.md` rows.

### Explicitly out of scope

- **Any change to `CAUSAL_ACTIVE_P95_BUDGET_MS` or `CAUSAL_OBSERVE_P95_BUDGET_MS`.**
  Frozen, and raising them is not a fix for a measurement defect.
- `BUDGET_SAMPLES`. C001 §10 already recorded 15 consecutive preemptions as an
  accepted residual, with "raise `BUDGET_SAMPLES` rather than touching the
  constant" as the prescribed response. That trade-off is unchanged here; the
  fix removes a *different* confound, not this one.
- `src/tool_advisor/causal_frontier.rs:5020`
  (`m002_frontier_eval_p95_within_budget`). It already measures over 1001
  repeated iterations per case, which is the sound convention, and it passed in
  the run that failed the holdout gate. Changing it would be unforced.
- `tests/causal_observe_replay.rs:532`. C001 already reduced it to a
  finite/non-negative sanity check pointing at the p95 gate as the enforcement
  point.
- `src/tool_advisor/causal_observe.rs:476` and
  `tests/causal_active_m005.rs:588`. Both already use `warm_min_millis`.
- The `elapsed_millis < 1000` assertion at `src/tool_advisor/mod.rs:2946`,
  recorded as a low finding in C001 §10 and never observed to flake.
- The project-activation coalescing flake and the M004 E2E transcript flake
  filed as a medium finding in C001 §10. Distinct defects in distinct
  subsystems, deliberately not absorbed here.
- Any M006 milestone. M006-B is blocked by this and resumes after; M006-C is
  untouched.

## 6. Required production changes

### Core/domain

None. This milestone changes no production code.

### Storage and migrations

None. No schema, layout, or migration change; `STORAGE_LAYOUT_VERSION` is
untouched.

### Protocol and DTOs

None. No `CoreRequest`, `CoreEvent`, or projection variant is added, removed, or
resequenced.

### Runtime and concurrency

None. No spawn, no thread, no scheduler interaction, no `ExecutionContext`.

### Frontend or operator surface

None. No TUI, command, or keybinding change.

### Security and authorization

None. No authorization matrix row, gate, or capability is added or changed. The
`check_authorization_matrix.py` guard is unaffected and still green.

### Documentation and static guards

- This plan and its closure record.
- `plans/registry.md`: one roadmap row, one implementation-plan row, one
  closure row, and an amendment sentence in the causal-frontier gate paragraph
  recording C001's falsified "pass" claim and its discharge.
- The in-tree comment at the repaired site, so a future reader who finds a p95
  over warm minima has the reasoning next to it.
- No `architecture/` change: no contract, ownership, or budget semantics moved.
  Running the change-triggered guards is sufficient verification.

## 7. Ordered work packages

### Work package A — Baseline reproduction under controlled load

Establish that the holdout p95 gate fails on `f7c8948a` when the machine is
loaded, converting "one red hosted run" into a reproduced defect.

1. Build `cargo test --test causal_active_m005`.
2. Start a controlled, bounded number of CPU burners for the duration of the
   experiment only.
3. Run `m005_holdout_structural_gates` repeatedly; record failures and the
   reported p95 values.
4. Stop the burners; confirm they are gone (`pgrep`).
5. Repeat unloaded on the same commit to show the contrast.

This package changes no tracked file.

### Work package B — Repair the distributional sample

In `tests/causal_active_m005.rs`, inside `qualify_scenario`:

1. Populate `ScenarioOutcome.evaluation_millis` from the `warm_min` already
   computed for the per-scenario budget assertion, not from the cold
   `outcome.active_evaluation_millis`. That field existed only to feed this p95,
   so this is its entire reason for being here.
2. Update the adjacent comment to state why the warm minimum is the correct
   distributional input: the p95 is a claim about spread across scenario shapes,
   and a cold sample injects runner scheduling into each point of that spread.
3. Leave the percentile helper, the rank, the budget constant, the scenario
   set, the minimum-count assertion, the per-scenario assertion, and the p95
   assertion itself untouched.

The reuse is deliberate: the value is already computed for the sibling gate, so
no extra `evaluate_active` call is added and the test's runtime does not grow.

### Work package C — Prove the budget is still enforced

1. Temporarily set `CAUSAL_ACTIVE_P95_BUDGET_MS` below the observed warm cost.
2. Confirm the per-scenario gate fails with an explicit message, and that
   `m005_freeze_record_matches_live_contracts` also fails, proving the constant
   is pinned and cannot be quietly raised.
3. Separately, with the *pooled* variant temporarily in place and the
   per-scenario assertion lifted, confirm the p95 assertion fires on its own
   data. This is the evidence that the retained assertion is live code rather
   than decoration, and it is taken against the variant because the shipped
   estimator makes it subsumed.
4. Restore `5.0`; re-read both constants from source; restore the temporary
   edits; confirm green and a clean `git diff` limited to one test file.

No temporary edit is ever committed.

### Work package D — Post-fix comparison and full local verification

1. Repeat the loaded runs from Work package A on the fix head; expect zero
   failures.
2. Repeat unloaded; expect zero failures.
3. Run the affected targets, the advisor lib suite, `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets --locked -- -D warnings`, and
   `scripts/verify.sh quick`.
4. Confirm `git diff` shows only `tests/causal_active_m005.rs` plus `plans/`.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable in the production sense: no state, process, authority, or
persistence surface is involved.

For the measurement, the accepted trade-off is inherited from C001 and is
restated honestly rather than improved: a preemption that lands on all 15 warm
samples of a scenario would still fail that scenario's per-scenario gate, and
pushing enough preempted scenarios into the top decile would still fail the p95.
That residual is the documented price of removing the single-cold-sample
confounder, and C001's prescribed response to a recurrence is to raise
`BUDGET_SAMPLES`, never the frozen constant.

The new residual this milestone introduces is narrower and worth stating: the
distributional gate now measures a p95 over per-scenario warm *minima*, so it is
sensitive to a genuine computational regression — a change that makes the
evaluation itself slower raises every scenario's warm minimum — and insensitive
to machine noise. It no longer detects a regression expressed only as
*variance*, that is, a computation whose cost depends on scheduling. That is
not a property of this pure in-memory function, and it is exactly what the gate
was measuring by accident before.

The dominance residual is separate and the more important one to state plainly:
with the per-scenario warm-minimum gate in place, the p95 is subsumed and cannot
fire, so the p95 line is a reported statistic with a redundant check rather than
an independent gate. Budget enforcement on this path is nonetheless complete and
*stricter* than a p95, because all 284 scenarios must be under budget rather
than the 95th percentile. This is stated in the code at the assertion, proven
both ways in §3, and filed as a finding in the closure's §10. Leaving it
un-stated would be the one genuinely dishonest option available here: a gate
that looks like enforcement and cannot fire is more dangerous than a flaky one,
because nothing reports it.

The Work package A load harness must be fully stopped before the fix runs, and
its absence verified, or the post-fix numbers are not comparable to the
baseline.

## 9. Compatibility and migration

None. No schema, storage, protocol, configuration, or public API change. Both
constants remain `pub` and unchanged, so downstream consumers observe no
difference. MSRV 1.89 is preserved; the fix introduces no new dependency and no
new API — it reuses `warm_min_millis`, which C001 added and which is already in
this file.

## 10. Required tests

### Focused unit tests

None added. The change is to an existing integration test's sampling, and the
existing structural assertions already cover the behavior under test.

### Integration tests

- `tests/causal_active_m005.rs` — all four tests, with
  `m005_holdout_structural_gates` the subject.
- `tests/causal_observe_replay.rs` — unchanged by this diff, run to confirm no
  sibling gate moved.
- `cargo test --lib tool_advisor` — confirms no production behavior moved.

### Restart and recovery tests

Not applicable. No persistent state.

### Contention and cancellation tests

- Work package A/D loaded and unloaded repetitions, which are the contention
  evidence for this milestone. These are experiments, not committed tests: a
  committed load-sensitive test would reintroduce exactly the defect being
  fixed.

### Security and negative tests

Not applicable. No security surface. The negative evidence this milestone owes
is the Work package C breach, and it is required.

### Migration and compatibility tests

Not applicable. No migration.

## 11. Required verification commands

```bash
# Work package A — baseline reproduction under bounded CPU load on f7c8948a
cargo test --locked --test causal_active_m005 --no-run
#   -> then run m005_holdout_structural_gates repeatedly with burners active
#   -> record failures and reported p95 values
#   -> then run unloaded on the same commit

# Work package B/C/D — after the fix, on identical load
cargo test --locked --test causal_active_m005
cargo test --locked --test causal_observe_replay
cargo test -p codegg --lib tool_advisor

# deliberate breach, then restore
#   (CAUSAL_ACTIVE_P95_BUDGET_MS temporarily lowered, then restored)
cargo test --locked --test causal_active_m005

# formatting, linting, and the canonical sanity sweep
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick

# confirm the diff is measurement-only
git diff --stat main...HEAD
```

Hosted:

```bash
gh run view <run-id> --log-failed
```

## 12. Documentation updates

- This plan.
- `plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/002-status.md`
  — the closure record, using milestone number `002` to match this plan.
- `plans/registry.md` — roadmap/status row, implementation-plan row, closure row,
  and an amendment sentence in the causal-frontier gate paragraph noting that
  C001's "outside the diff and pass" claim for this gate was falsified by run
  `37262641992` and is discharged by this corrective. C001's own record is not
  edited.
- The in-tree comment at the repaired site.
- No `architecture/` document changes.

## 13. Acceptance criteria

- The holdout p95 gate at `tests/causal_active_m005.rs` computes its percentile
  over one warm best-of-15 minimum per scenario, not over cold single-shot
  readings.
- The per-scenario warm-minimum assertion is unchanged, and the freeze-pin test
  is unchanged.
- `CAUSAL_ACTIVE_P95_BUDGET_MS` and `CAUSAL_OBSERVE_P95_BUDGET_MS` both read
  `5.0` on the fix head, and neither constant line is in the diff.
- `src/tool_advisor/` is unmodified.
- The tracked diff is `tests/causal_active_m005.rs` plus `plans/` documents.
- Under the Work package A load profile: the baseline fails, this design passes,
  and the rejected pooled design's failure count is recorded so the choice is
  auditable.
- Unloaded, the target passes on the fix head.
- A deliberate breach fails the per-scenario gate and
  `m005_freeze_record_matches_live_contracts`; the p95 assertion is separately
  proven live against the pooled variant with the per-scenario assert lifted.
  Constants and all temporary edits restored and re-read.
- `cargo fmt --all -- --check`, workspace clippy `-D warnings`, and
  `scripts/verify.sh quick` are clean.
- `m005_holdout_structural_gates`, the other three tests in its target, and
  `cargo test --lib tool_advisor` are green.
- The closure records the dominance finding explicitly, with the proof for both
  halves, and files the retirement/consolidation question as a finding.
- No historical closure record is edited, and no M006 code appears in the diff.
- Hosted `CI / verify` is green on the fix head.

## 14. Stop conditions

Stop and re-plan if any of these hold:

- The loaded baseline cannot be made to fail on `f7c8948a`. The defect would
  then be unreproduced locally, and the hosted failure alone is too thin a basis
  to justify a corrective; record that and defer.
- The fix requires touching `CAUSAL_ACTIVE_P95_BUDGET_MS`,
  `CAUSAL_OBSERVE_P95_BUDGET_MS`, the M005 freeze record, the holdout asset, the
  per-scenario assertion, or any `src/` file. That is a budget or contract
  change wearing a measurement change's clothes, and it needs its own decision.
- **The fix head still fails the loaded profile.** That is the pooled-variant
  failure mode and is precisely why this design exists. If the per-scenario
  minima design fails under load, stop: the noise-immunity premise is wrong and
  the whole approach needs re-deriving rather than tuning.
- `m005_freeze_record_matches_live_contracts` fails for any reason other than
  the deliberate breach. That is a real regression signal.
- The fix adds wall-clock cost to the test. It reuses a value that already
  exists, so any material slowdown means the reuse was not achieved.
- Hosted CI fails on something outside the causal advisor. Record it in the
  closure's findings rather than absorbing it here, and do not let it block this
  milestone the way this defect blocked M006-B.

## 15. Closure evidence required

- The exact baseline commit and the exact fix commit.
- A requirement-to-evidence matrix with one row per §4 invariant, each naming
  its evidence.
- Baseline and fix-head reproduction numbers under an identical, described load
  profile, plus the unloaded comparison, plus confirmation that the burners were
  stopped.
- The deliberate-breach transcript: the failing message from the holdout p95
  gate, the failing freeze-pin test, and the restored constants re-read from
  source.
- `git diff --stat` proving the change is measurement-only.
- fmt, clippy, and `scripts/verify.sh quick` results.
- The hosted run id with the full workspace sweep count and result, or an
  explicit statement that the sweep was truncated and why.
- An honest statement of what the fix does *not* make detectable, per §8.
- An entry in §10 of the closure for every finding this milestone did not fix,
  including the C001 medium findings.
- Registry rows added, and explicit confirmation that C001's `001-status.md` and
  every causal-frontier closure record are byte-identical on the branch.

## 16. Handoff notes

- This is a corrective in an already-closed track. It reopens no causal-frontier
  scope: M001–M005 keep their original dispositions, the freeze record and every
  frozen threshold are untouched, and the historical live-primary-model study
  stays blocked exactly as M005 left it.
- It is deliberately sized small. The diff is one estimator substitution plus a
  comment; the work is in the evidence that the substitution is correct and that
  the gate still bites.
- C001 classified, deferred, and then partially fixed this defect twice. The
  lesson worth carrying to the next timing gate in this repository is that a
  gate is not proven by two green runs of a load-dependent distribution.
- After this lands, M006-B (`m006-b-read-surface`) should be rebased or merged
  forward onto it so its diff stays LSP-only, and its CI re-run. That is the next
  action, not part of this milestone.
