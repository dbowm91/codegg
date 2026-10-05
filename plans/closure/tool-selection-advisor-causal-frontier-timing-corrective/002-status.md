# Tool-Selection Advisor Causal Frontier Timing Corrective C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-timing-corrective/002-holdout-p95-warm-latency-distribution.md`

Source corrective addendum:

- `plans/subsystems/tool-selection-advisor-causal-frontier-timing-corrective-addendum.md`

Predecessor records this corrective discharges (accepted and immutable,
neither edited):

- `plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/001-status.md`
  §2 — the requirement row asserting the holdout p95 at
  `tests/causal_active_m005.rs:640` is "outside the diff and pass", and §3,
  which reports that gate green in hosted runs `37222559997` and `37223675386`.
  Hosted run `37262641992` failed exactly there at p95 5.141 ms. The claim is
  falsified, and it is discharged by fixing the site, not by amending the
  record.
- `plans/closure/tool-selection-advisor-causal-frontier-experiment/005-status.md`
  §10 — the original deferred "documented load flake" classification.

Repository baseline reviewed: `f7c8948a` (baseline reproduced under load)

Implementation commits:

- `9d298ab0` — C002 plan and registry rows (plan row, roadmap status, and the
  additive amendment to the causal-frontier gate paragraph).
- `0d50e4cd` — C002 source fix (holdout p95 sampled from warm scenario minima;
  frozen constants and `src/tool_advisor/` untouched).

PR: `#100`

## 1. Executive finding

The last cold-sampling latency gate in the causal advisor family is now
measurement-correct, and the defect it carried — a gate that failed
intermittently for reasons unrelated to the code under test — is eliminated by
reusing an already-computed warm measurement rather than by a single green run.
The frozen 5.0 ms budgets are unchanged. No production code, contract, schema,
authority, or advisor behavior changed: the diff is one integration test file
plus `plans/`.

The triggering failure was pre-existing, unrelated to the work it was blocking,
and reproducible: on baseline `f7c8948a` the holdout gate failed **8 times in 10
runs** under controlled load, with p95 values from 2.603 ms to 22.864 ms, while
the same test reported 0.908 ms unloaded.

The most consequential result of this milestone is not the fix but the two
designs it rejected, each implemented and measured rather than argued:

- Pooling all 4260 warm samples keeps the p95 independently fireable but does
  **not** fix the defect — 7 failures in 10 loaded runs, p95 3.462–12.413 ms.
- The shipped estimator is stable (10/10 loaded) but makes the p95 **provably
  unreachable**, because every scenario is already asserted strictly under
  budget before the p95 line runs.

Both facts are the same fact seen twice: under heavy load the computation's true
wall-clock p95 genuinely exceeds 5 ms. Any estimator that reports that faithfully
fails; any estimator that passes is measuring something else. C001 already chose
the floor for two sites, and this milestone makes the family consistent and
records the price honestly rather than discovering it later.

## 2. Requirement-to-evidence matrix

| Requirement (plan §4) | Evidence | Result |
|---|---|---|
| p95 samples a warm measurement, never a cold single-shot reading | `tests/causal_active_m005.rs` `ScenarioOutcome.evaluation_millis` is now the `warm_min` computed by `warm_min_millis`; `outcome.active_evaluation_millis` no longer feeds the distribution | pass |
| Distributional shape preserved | one value per holdout scenario, 284 total; `nearest_rank_percentile`, rank `0.95`, and the scenario set are unchanged | pass |
| No extra evaluation cost | the warm loop already ran for the per-scenario assertion; the p95 reuses its result. Loaded runtime unchanged (~4.4 s) | pass |
| `CAUSAL_ACTIVE_P95_BUDGET_MS` stays `5.0` | `src/tool_advisor/causal_active.rs:54` reads `5.0`; `git diff -- src/` is empty | pass |
| `CAUSAL_OBSERVE_P95_BUDGET_MS` stays `5.0` | `src/tool_advisor/causal_observe.rs:44` reads `5.0`; file not modified | pass |
| Budget enforcement remains live | deliberate breach to `0.5` failed `m005_holdout_structural_gates` with `m005-pinned-001: warm best-of-15 evaluation 0.838 ms exceeds the 0.5 ms budget` | pass |
| The constant is pinned, not raisable | the same breach failed `m005_freeze_record_matches_live_contracts` (`tests/causal_active_m005.rs:190`) | pass |
| The retained p95 is live code, not decoration | against the pooled variant with the per-scenario assertion temporarily lifted, a breach to `0.8` produced `p95 active evaluation 0.855 ms exceeds the 0.8 ms budget over 4260 pooled samples` | pass |
| Per-scenario assertion untouched | unchanged in the diff | pass |
| Scenario set and minimum count untouched | `outcomes.len() >= 160` assertion unchanged; asset and fingerprint unchanged | pass |
| `src/tool_advisor/` unmodified | `git diff --stat HEAD~2 HEAD -- src/` is empty; the diff is `tests/` plus `plans/` | pass |
| No historical closure record edited | C001 `001-status.md` and the causal-frontier `001`–`005` records are absent from both commits | pass |
| No M006 code in the diff | `git diff --stat HEAD~2 HEAD` lists only the test file and `plans/` | pass |
| Flake eliminated under load, same profile | 10 runs, 28 burners: baseline 2 passed / 8 failed (p95 2.603–22.864 ms); fix head 10 passed / 0 failed (p95 0.951–1.034 ms) | pass |
| Rejected design did not fix the defect | pooled variant, same profile: 3 passed / 7 failed (p95 3.462–12.413 ms) | pass |
| Unloaded behaviour unchanged | 0.908 ms baseline vs 0.835 ms fix head, both well inside the 5.0 ms budget | pass |
| fmt and clippy clean | `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean | pass |
| Affected suites green | `causal_active_m005` 4/4; `causal_observe_replay` 13/13; `--lib tool_advisor` 172 passed, 0 failed, 6 pre-existing ignored | pass |
| Canonical sanity sweep | `scripts/verify.sh quick` exit 0, every guard green | pass |
| Hosted `CI / verify` green | run `37311215175`, 24/24 steps green, sweep 12250/12250 | pass | see §4 |

## 3. Production implementation evidence

None, by design. This milestone changes no production code. The diff is one
integration test file and `plans/` documents.

What changed, at the single site:

- `tests/causal_active_m005.rs` — `ScenarioOutcome.evaluation_millis` is now
  assigned from the `warm_min` already computed for the per-scenario budget
  assertion, rather than from the cold `outcome.active_evaluation_millis`. Two
  comments were added: one at the sampling site explaining that a cold reading
  measures the runner, and one at the p95 assertion recording the dominance
  analysis and the rejected pooled variant. Nothing else changed.

What deliberately did not change:

- `CAUSAL_OBSERVE_P95_BUDGET_MS` and `CAUSAL_ACTIVE_P95_BUDGET_MS`, both still
  `5.0`, with `src/tool_advisor/causal_active.rs` not modified at all.
- The M005 freeze record, benchmark fingerprint, holdout asset, and every other
  frozen threshold.
- The per-scenario warm best-of-15 assertion, the percentile helper, the `0.95`
  rank, the scenario set, and the ≥160 minimum-count assertion.
- `replay_p95_within_budget`, the M002 p95 gate, and the observe-site gate — all
  outside the diff and all green.
- Every production causal-advisor behavior: scoring, admissibility, promotion,
  disclosure, effect-path planning, and request preparation.

Root cause, verified rather than inferred: the holdout p95 was the only latency
gate in the causal family still sampling a single cold wall-clock reading, which
is why it alone flaked. The warm cost of active evaluation is ~0.83–0.92 ms
against a 5.0 ms budget, so there was never a real performance problem and never
a need for a larger budget; the headroom was always about 5×. The failures were
cold caches, page faults, and scheduler preemption on a shared runner.

## 4. Verification executed

### Commands run

Local (darwin aarch64, 14 cores, toolchain 1.89):

```bash
# baseline reproduction, f7c8948a, before any edit
cargo test --locked --test causal_active_m005 --no-run
# 28 CPU burners started, then 10 runs of m005_holdout_structural_gates
#   -> 2 passed, 8 failed; reported p95 per run:
#      8.979 F, 7.516 F, 5.228 F, 4.013 pass, 8.511 F,
#      5.744 F, 22.864 F, 2.603 pass, 12.741 F, 7.223 F
# burners killed by recorded PID, survivors verified 0

# rejected pooled variant, same profile
#   -> 3 passed, 7 failed; p95 3.462-12.413 ms

# shipped design, same profile
#   -> 10 passed, 0 failed; p95 0.951-1.034 ms

# deliberate breach and restore
#   CAUSAL_ACTIVE_P95_BUDGET_MS temporarily 5.0 -> 0.5, then restored
cargo test --locked --test causal_active_m005
cargo test --locked --test causal_observe_replay
cargo test --locked --lib tool_advisor
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Hosted:

```bash
gh run view 37311215175
```

### Results

Local:

- Baseline reproduction on `f7c8948a` under 28 burners: **8 failures in 10
  runs**, reported p95 from 2.603 ms (a pass) to 22.864 ms, with the failing runs
  spanning 5.228–22.864 ms. Unloaded on the same commit: 0.908 ms, pass. This
  converts "one red hosted run" into a reproduced defect and is the evidence the
  milestone was justified on.
- Rejected pooled variant under the identical profile: **7 failures in 10**,
  p95 3.462–12.413 ms. Recorded because it is the reason the shipped estimator
  is the floor and not the distribution.
- Shipped design under the identical profile: **10 passed, 0 failed**, p95
  0.951–1.034 ms — a 0.08 ms spread against the baseline's 20 ms.
- Unloaded, the shipped design reports 0.835 ms and the test passes in ~4.4 s,
  the same runtime as baseline, confirming the reused value added no cost.
- Deliberate breach at `0.5`: `m005_holdout_structural_gates` failed with
  `m005-pinned-001: warm best-of-15 evaluation 0.838 ms exceeds the 0.5 ms
  budget`, and `m005_freeze_record_matches_live_contracts` failed at
  `tests/causal_active_m005.rs:190`. Constants restored to `5.0` and re-read
  from source; the test file was restored from a copy taken before the
  experiment; `git diff -- src/` verified empty afterwards.
- p95 liveness, taken against the pooled variant with the per-scenario assertion
  temporarily lifted: `p95 active evaluation 0.855 ms exceeds the 0.8 ms budget
  over 4260 pooled samples`. Both temporary edits restored.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean, no
  diagnostics.
- `cargo test --locked --test causal_active_m005`: 4 passed.
- `cargo test --locked --test causal_observe_replay`: 13 passed.
- `cargo test --locked --lib tool_advisor`: 172 passed, 0 failed, 6 ignored
  (pre-existing and unrelated).
- `scripts/verify.sh quick`: exit 0, all guards green.

Hosted (PR `#100`, head `0d50e4cd`) — run **`37311215175`**, verify job
**success**, all 24 steps green, no failed step:

- `Workspace tests`: **`12250` tests run, `12250` passed, `7` skipped**, in
  391.488s. The sweep ran to completion rather than truncating under fail-fast,
  so the workspace is covered by evidence and not by absence of failure.
- Every causal timing gate passed on hosted hardware, including the one this
  milestone fixed: `causal_active_m005::m005_holdout_structural_gates`,
  `causal_active_m005::m005_freeze_record_matches_live_contracts`,
  `causal_observe_replay::replay_p95_within_budget`,
  `causal_observe::tests::observe_never_mutates_or_suppresses`, and
  `causal_frontier::tests::m002_frontier_eval_p95_within_budget`.
- All guard and lint steps green, including `Formatting`, `Workspace Clippy`,
  the TUI authority guards, and the scheduler-bypass guard.

The 7 skipped are pre-existing and unrelated; every causal gate executed and
passed.

A direct comparison is available: the triggering run `37262641992` on the same
hosted configuration truncated its sweep at 5,716 of 12,287 because this gate
failed, leaving roughly 6,500 tests unexecuted. This run reached 12,250 of
12,250.

A note on how the reproduction was run, since it affects how much the numbers
are worth. 28 burners on a 14-core machine is heavier than any hosted runner in
this repository's history, so the absolute failure counts overstate the hosted
rate; the baseline's run 3 failed at 5.228 ms against the hosted 5.141 ms,
which is the comparison that matters. What the profile establishes reliably is
the *contrast*: two designs that fail under identical load and one that does
not, on the same commit lineage and the same binary path.

## 5. Invariant review

- Both constants remain `5.0` in source; neither constant line appears in the
  diff and `git diff -- src/` is empty. The invariant is self-enforcing:
  `m005_freeze_record_matches_live_contracts` fails if the constant moves, which
  was demonstrated rather than assumed.
- The M005 freeze record and benchmark fingerprint are untouched.
- The p95 rank, the percentile helper, the scenario set, and the ≥160 minimum
  count are untouched. No assertion was deleted, skipped, or ignored.
- The per-scenario warm best-of-15 assertion is unchanged and still fires.
- No production code path changed. The diff is one integration test file plus
  `plans/`.
- No causal-frontier scoring, admissibility, promotion, disclosure, or
  effect-path behavior changed; no model retrained; no advisor artifact
  refrozen; `ResolvedToolSurface` authority and advisor default-off/local-only
  behavior are untouched.
- No historical closure record was edited. C001's `001-status.md` and the
  causal-frontier `001`–`005` records remain byte-identical, and this corrective
  is recorded additively alongside them.
- `plans/registry.md` gained one implementation-plan row, one roadmap status
  update, and one additively-worded amendment to the causal-frontier gate
  paragraph. No existing row was rewritten to imply C001 was right.

## 6. Failure and recovery review

Not applicable in the production sense: no state, process, authority, or
persistence surface is involved.

For the measurement, two residuals are stated rather than smoothed over.

The inherited residual, unchanged from C001 §6: a preemption that lands on all
15 warm samples of a scenario still fails that scenario's gate. Fifteen
consecutive preemptions is a materially different signal from one cold sample,
and accepting that residual is the price of removing the 15-in-15 confounder.
C001's prescribed response to a recurrence remains: raise `BUDGET_SAMPLES`,
never the frozen constant.

The residual this milestone introduces is that the p95 gate no longer detects a
regression expressed only as *variance* — a computation whose cost depends on
scheduling. That is not a property of this pure in-memory function, and it is
precisely what the gate was measuring by accident before.

The residual that matters most is the dominance finding in §10: the p95
assertion is retained but cannot fire. It is documented in the code at the
assertion rather than left to be discovered, because a gate that looks like
enforcement and cannot fire is harder to notice than a flaky one — nothing
reports it.

The load harness started its burners as child processes, killed them by recorded
PID rather than by pattern, and verified zero survivors after each experiment, so
no stray CPU load contaminated a later measurement. All temporary source edits
were made against copies or `.bak` files and restored, with `git diff -- src/`
verified empty and the test file restored before either commit.

## 7. Migration and compatibility review

None. No schema, storage, protocol, configuration, or public API change. Both
constants remain `pub` and unchanged, so downstream consumers observe no
difference. MSRV 1.89 is preserved. The change reuses `warm_min_millis`, which
C001 added and which was already in this file; no new dependency and no new API.

## 8. Security review

None applicable. No authorization, credential, network, sandbox, or privilege
surface is touched, and no production code changes. The change reduces the
influence of ambient runner scheduling on a pass/fail decision; it does not
grant any capability, relax any authorization, or widen any input path.

## 9. Documentation and operations

- `plans/implementation/tool-selection-advisor-causal-frontier-timing-corrective/002-holdout-p95-warm-latency-distribution.md`
  — this milestone's plan, including both rejected designs and the reasoning.
- This closure record.
- `plans/registry.md` — roadmap status, implementation-plan row, closure row,
  and the additively-worded amendment to the causal-frontier gate paragraph.
- Two in-tree comments at the repaired sites carry the rationale in-tree, so a
  future reader who finds a p95 over warm minima, or who is tempted to "restore"
  independence to a subsumed assertion, has the reasoning next to it.
- No `architecture/` document changes. No production contract, ownership, or
  behavior changed, and the budget semantics those documents describe are
  unchanged.
- Operator note: if this class of failure reappears under a *different* frozen
  budget elsewhere, the correct response is the one taken here — fix the
  measurement, never the frozen constant. And if a p95 assertion is found to be
  dominated by a per-item assertion over the same value, that is a structural
  finding to file, not a cosmetic one.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The holdout p95 assertion is **subsumed** by the per-scenario warm-minimum assertion and cannot fire: every scenario is already asserted strictly under budget before the p95 line runs. Proven both ways — breaching at `0.8` fires the per-scenario gate first, and firing the p95 requires lifting that assertion. | Budget enforcement is complete and strictly stronger than a p95 (all 284 scenarios, not the 95th percentile), so nothing is unenforced. The risk is that a future reader mistakes a redundant gate for an independent one, and that redundancy is invisible until someone tries to rely on it. | Decide, in its own pass with its own governance, whether to retire the redundant p95 line or consolidate onto a single distributional gate. **Not taken here** because C001's accepted closure records an assertion at that site, and removing it would falsify a second accepted record. Filed rather than absorbed for that reason. |
| low | The fix accepts that 15 consecutive scheduler preemptions would still fail a scenario's gate. | A genuinely sustained-load runner could still produce a false failure. | Accepted, inherited from C001 §6. If it recurs, raise `BUDGET_SAMPLES`, not the constant. |
| low | The p95 no longer detects a regression expressed only as variance. | A change whose cost depends on scheduling would not be caught. | Accepted. Not a property of this pure in-memory function. |
| low | `src/tool_advisor/mod.rs:2946` has a loose `elapsed_millis < 1000` (1 s) assertion. | A different defect class with a much looser budget; not a flake source today. | Out of scope. Carried forward from C001 §10. File separately if it ever flakes. |
| medium | `codegg::project_activation::concurrent_same_owner_activation_coalesces_scope_and_bundle` and the M004 E2E transcript flake, filed by C001 §10. | Keeps branches red for reasons unrelated to the causal advisor and re-truncates the sweep. | Its own corrective. Distinct defect, distinct subsystem; deliberately not absorbed here. |
| low | `scripts/check_projection_transport_lifecycle.py` fails on `main` with `daemon_socket.rs: raw forwarder is spawned without an owned handle`. Verified pre-existing at `cde7dbfe` on a clean worktree. | A guard outside the CI quick subset. | Unrelated to this milestone; noted so it is not mistaken for fallout. |

Neither this milestone's finding nor the carried ones are regressions introduced
here.

Resolved by this milestone, for the record:

- The C001 §2 claim that the holdout p95 was "outside the diff and pass" is
  **discharged by fixing the site**. That record is not edited; the
  falsification and the discharge are recorded here and in the registry.
- The deferred load-flake note in
  `plans/closure/tool-selection-advisor-causal-frontier-experiment/005-status.md`
  §10 is now fixed at every site it named, not merely classified. That record is
  not edited either.
- M006-B is no longer blocked by this defect.

## 11. Roadmap disposition

The corrective addendum remains terminally satisfied and now covers every latency
site in the causal family; no successor milestone exists in this track.

This closes the defect that kept PR `#99` red and restores the workspace sweep
that fail-fast had been truncating. It reopens no causal-frontier scope: M001–M005
keep their original dispositions, the frozen budgets and freeze record are
untouched, and the historical live-primary-model study stays blocked exactly as
M005 left it.

The medium finding in §10 is a genuine follow-up decision about gate structure,
not a defect blocking anything, and it is explicitly not taken in this
corrective.

M006-B resumes on this base: `m006-b-read-surface` should be rebased or merged
forward so its diff stays LSP-only, and its CI re-run.

## 12. Registry updates

- `plans/registry.md` roadmap/status table: the causal-frontier timing
  corrective row records C001 closed and C002 closed, with the falsification and
  the unblocking of M006-B.
- `plans/registry.md` implementation-plan table: add C002, closed, pointing at
  the plan and this closure.
- `plans/registry.md` closure table: add C002, closed, with the hosted run id
  and the load-reproduction result.
- `plans/registry.md` causal-frontier gate paragraph: carries an additive
  **Amendment (C002)** noting that C001's "outside the diff and pass" row was
  falsified by run `37262641992`, that C001's record is not edited, and that the
  rejected pooled variant is recorded with its measurement.
