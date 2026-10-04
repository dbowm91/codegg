# Tool-Selection Advisor Causal Frontier Timing Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-timing-corrective/001-warm-best-of-n-budget-gates.md`

Source corrective addendum:

- `plans/subsystems/tool-selection-advisor-causal-frontier-timing-corrective-addendum.md`

Predecessor records this corrective discharges (both immutable, neither edited):

- `plans/closure/tool-selection-advisor-causal-frontier-experiment/005-status.md`
  §10 — the deferred "documented load flake" classification.
- `plans/closure/desktop-frontend-ide-foundation/005-status.md` §10 — the
  medium finding that the flake keeps every branch red and truncates the
  workspace sweep.

Repository baseline reviewed: `cdfd6257` (fix head); flake reproduced at
`main` `614e983e`

Implementation commits:

- `cdfd6257` — causal-frontier timing C001 source fix (warm best-of-15 at two
  sites, sanity check at the third, frozen constants untouched).

## 1. Executive finding

The three single-shot wall-clock latency assertions in the causal advisor gates
are now measurement-correct, and the defect they carried — a gate that
intermittently failed for reasons unrelated to the code under test — is
eliminated by repetition rather than by a single green run. The frozen 5.0 ms
budgets are unchanged and still enforced: distributionally by the two p95 gates
that already existed, and per-site by an assertion at every one of the three
original sites. The gates were proven still able to fail. No production code,
contract, schema, or authority changed; the diff is three test files. The
corrective closes with no successor work.

The defect was pre-existing and had been observed three times before this fix —
including once by the causal frontier milestone itself, which classified it as a
load flake and deliberately deferred the fix. It was left unfixed because each
occurrence was a different test in a different run, which reads as noise rather
than as one defect with three faces. Establishing the baseline reproduction
(4 failures in 8 runs at `main` with no M006-A code present) is what converted
three unrelated-looking failures into a single actionable defect.

## 2. Requirement-to-evidence matrix

| Requirement (plan §4/§13) | Evidence | Result | Notes |
|---|---|---|---|
| Flake eliminated by repetition, not one green run | 20 consecutive runs of `--test causal_active_m005 --test causal_observe_replay` on the fix head: 0 failures. Baseline `main` `614e983e`, 8 runs: 4 failures | pass | Baseline worktree confirmed to contain no `src/tui/editor.rs` |
| `CAUSAL_OBSERVE_P95_BUDGET_MS` / `CAUSAL_ACTIVE_P95_BUDGET_MS` remain `5.0` | `src/tool_advisor/causal_observe.rs:44` and `src/tool_advisor/causal_active.rs:54` read `5.0` on the fix head; neither file's constant line is in the diff | pass | `src/tool_advisor/causal_active.rs` is not modified at all by this change |
| Freeze record and benchmark fingerprint unchanged | `m005_freeze_record_matches_live_contracts` green (it asserts `structural_gates.pure_active_eval_p95_ms_max == CAUSAL_ACTIVE_P95_BUDGET_MS`, so it fails if the constant moves) | pass | This is the mechanism that makes raising the budget unavailable |
| An assertion still exists at all three sites; none deleted or skipped | `src/tool_advisor/causal_observe.rs:476`, `tests/causal_active_m005.rs:592`, `tests/causal_observe_replay.rs:531` — all three assert; no `#[ignore]`, no `#[skip]`, no `return` added | pass | The third became a sanity check because its sample cannot be re-run |
| Gate can still fail (not silently weakened) | With both constants temporarily set to `0.000001`, all four affected gates failed with explicit messages; after restoration all pass | pass | Messages: "warm best-of-15 evaluation 0.855 ms exceeds…", "p95 0.924 ms exceeds…", "warm best-of-15 observation 0.849 ms exceeds…", plus the freeze-pin failure |
| Distributional p95 gates unmodified and green | `replay_p95_within_budget` (201 warm samples) and the holdout p95 at `tests/causal_active_m005.rs:640` are outside the diff and pass | pass | The same constants remain enforced here |
| Change confined to three test files | `git show --stat cdfd6257`: `src/tool_advisor/causal_observe.rs` (+37/−1), `tests/causal_active_m005.rs` (+41/−2), `tests/causal_observe_replay.rs` (+18/−4) | pass | No production path touched |
| No historical closure record edited | Causal frontier `001`–`005-status.md` and the M006-A `005-status.md` are absent from every commit on this branch | pass | Verified by inspection of the branch's file list |
| fmt and clippy clean | `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean | pass | — |
| Affected suites green | `cargo test --lib tool_advisor`: 172 passed, 0 failed, 6 ignored. `causal_active_m005`: 4 passed. `causal_observe_replay`: 13 passed | pass | The 6 ignored are pre-existing and unrelated |
| Hosted `CI / verify` green with the full sweep | see §4 hosted block | pass | The sweep was previously truncated by fail-fast; this is the first green run of the two gates' own targets on hosted hardware |

## 3. Production implementation evidence

None, by design. This milestone changes no production code. The change is
confined to test modules and integration test files.

What changed, at each of the three sites:

- `src/tool_advisor/causal_observe.rs` — added `BUDGET_SAMPLES: usize = 15`
  and `warm_min_millis`, which performs one discarded warmup and returns the
  minimum of 15 warm samples. `observe_never_mutates_or_suppresses` now asserts
  that value against the unchanged `CAUSAL_OBSERVE_P95_BUDGET_MS`, keeping
  `std::hint::black_box` so the measured call is not optimized away.
- `tests/causal_active_m005.rs` — the same helper, added next to the file's
  existing `nearest_rank_percentile`. The per-scenario check inside
  `qualify_scenario` now asserts the warm best-of-15 value against the
  unchanged `CAUSAL_ACTIVE_P95_BUDGET_MS`, keeping the scenario id in the
  message. This is the site that failed as `m005-pinned-002` and as
  `m005-holdout-109`.
- `tests/causal_observe_replay.rs` — this site's sample is recorded inside a
  live agent-loop request preparation and cannot be re-run with identical
  inputs, so best-of-N is not available. The budget assertion became a
  finite/non-negative sanity check that the live path recorded a real duration,
  with a comment pointing at `replay_p95_within_budget` as the enforcement
  point for the same constant. The now-unused constant import was removed from
  that inner module.

What deliberately did not change:

- `CAUSAL_OBSERVE_P95_BUDGET_MS` and `CAUSAL_ACTIVE_P95_BUDGET_MS`, both still
  `5.0`. The M005 freeze record pins the active budget and is itself asserted
  in the suite, so raising it was never available.
- The M005 freeze record, benchmark fingerprint, holdout scenario set, and
  every other frozen gate threshold.
- `replay_p95_within_budget` and the holdout p95 — the distributional checks
  that were already correct and remain the primary enforcement of the budget.
- Every production causal-advisor behavior: scoring, admissibility, promotion,
  disclosure, effect-path planning, and request preparation.

Root cause, verified rather than inferred: a single cold wall-clock sample of
a pure in-memory computation measures the machine as much as the code. The
measured warm cost is roughly 0.85–0.92 ms against a 5 ms budget, so there was
never a real performance problem and never a need for a larger budget — the
headroom was always about 5×. The failures were cold caches, page faults, and
scheduler preemption on a shared runner. The minimum of N warm samples is the
standard estimator for how long a computation takes when it is *not*
descheduled, which is what a budget on pure in-memory work should be checked
against.

## 4. Verification executed

### Commands run

Local (darwin aarch64, toolchain 1.89):

```bash
# flake reproduction at baseline (separate worktree at main, 614e983e)
git worktree add /tmp/codegg-baseline 614e983e
#   -> 8 runs of the two targets: 4 failures

# the fix, repeatedly
cargo test --test causal_active_m005 --test causal_observe_replay
#   -> 20 consecutive runs: 0 failures

# prove the gate still fires
#   (both constants temporarily set to 0.000001, then restored)
cargo test --test causal_active_m005 --test causal_observe_replay
#   -> all four affected gates fail with explicit messages

# affected suites
cargo test --lib tool_advisor
cargo test --test causal_active_m005
cargo test --test causal_observe_replay

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Hosted:

```bash
gh run view 37222559997      # CI / verify, PR #92, head cdfd6257
```

### Results

Local:

- Baseline reproduction at `main` `614e983e` (separate worktree, confirmed to
  contain no `src/tui/editor.rs`): **4 failures in 8 runs**. Fix head:
  **0 failures in 20 runs**. The M006-A branch before this fix: 2 in 8.
- Deliberate breach: with both constants temporarily set to `0.000001`, the
  corrected per-scenario gate failed with `warm best-of-15 evaluation 0.855 ms
  exceeds the 0.000001 ms budget`, the corrected observe gate failed with
  `warm best-of-15 observation 0.849 ms exceeds the 0.000001 ms budget`,
  `replay_p95_within_budget` failed with `p95 0.924 ms exceeds the 5 ms
  budget`, and `m005_freeze_record_matches_live_contracts` failed — the last
  being direct proof that the constant is frozen and pinned. Constants
  restored to `5.0` and re-verified in source; all tests pass.
- `cargo test --lib tool_advisor`: 172 passed, 0 failed, 6 ignored (the 6
  ignored are pre-existing and unrelated to this change).
- `cargo test --test causal_active_m005`: 4 passed.
- `cargo test --test causal_observe_replay`: 13 passed.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean, no
  diagnostics.

Hosted (`CI / verify`, PR #92, head `cdfd6257`) — run **`37222559997`**, verify
job **success**, all 23 steps green:

- All sixteen guard and lint steps green, including `TUI editor text-authority`,
  `Formatting`, and `Workspace Clippy`.
- `Workspace tests`: **`12180` tests run, `12180` passed, `7` skipped**, in
  318.796s. This is the substantive result: every prior red run was truncated
  by `nextest` fail-fast at roughly test 5,300, leaving ~6,900 of 12,180
  unexecuted. The sweep has now run to completion, so the rest of the
  workspace is finally covered by evidence rather than by absence of failure.
- Every causal gate passed on hosted hardware, including both tests that had
  previously failed: `causal_active_m005::m005_holdout_structural_gates`
  (8.764s), `causal_observe_replay::live_request_preparation_delta::observe_mode_leaves_live_definitions_byte_identical`,
  `causal_active_m005::m005_freeze_record_matches_live_contracts`,
  `causal_active_m005::live_request_preparation_active::active_mode_abstains_byte_identical_without_host_state`,
  `causal_observe::tests::observe_never_mutates_or_suppresses`, and
  `causal_observe_replay::replay_p95_within_budget` (0.330s).
- Desktop E2E run `37222560006`: **success** on the same head.

Note on the 7 skipped: pre-existing and unrelated to this change. They are not
attributed to the timing gates, which all executed and passed.

### Second run on identical code (factual correction, recorded per closure rule 1)

A subsequent hosted run on the branch head `b3ad2a31` — run **`37223675386`** —
is **red**, and this is recorded rather than omitted because it is the same code
with only `plans/*.md` differing (`git diff cdfd6257 b3ad2a31 --stat` is
documentation only; the compiled workspace is byte-identical).

- `CI / verify` failed at `Workspace tests` with **1 failed of 6,888 run**,
  truncating the sweep at 5,292 of 12,180 not run.
- The failure is `codegg::project_activation::concurrent_same_owner_activation_coalesces_scope_and_bundle`
  (`tests/project_activation.rs:146`), asserting
  `results.iter().all(|(lease_id, _)| lease_id == &results[0].0)` — eight
  concurrently spawned activations of the same owner did not all coalesce onto
  one lease id. It is a `multi_thread, worker_threads = 2` test, so a loaded
  shared runner can let a task miss the coalescing window.
- Desktop E2E run `37223675354` also failed: `m004-session.e2e.ts`,
  "completed assistant transcript never rendered". The prior E2E run
  `37222560006` on identical code was green.

Both failures are **outside this corrective's scope and outside its diff**.
`git diff main...HEAD -- tests/project_activation.rs` is empty, so neither this
corrective nor any M006-A change touches that test or its subject.
`concurrent_same_owner_activation_coalesces_scope_and_bundle` passes 12/12
locally on an unloaded machine, consistent with load sensitivity rather than a
deterministic defect.

**All five causal timing gates passed in this red run as well**:
`m005_holdout_structural_gates` (13.340s), `m005_freeze_record_matches_live_contracts`,
`observe_mode_leaves_live_definitions_byte_identical`,
`observe_never_mutates_or_suppresses`, and `replay_p95_within_budget`. Two
hosted runs of identical code, one fully green across all 12,180 tests and one
red on an unrelated concurrency test, with the causal gates green in both, is
the strongest available evidence that the timing fix holds.

This is a second, independent pre-existing flake discovered *because* the sweep
now runs far enough to reach it: at `main` the same fail-fast truncation hid
everything after roughly test 5,300. It is filed as a new medium finding in §10
rather than absorbed here.

## 5. Invariant review

- Both constants remain `5.0` in source; neither constant line appears in the
  diff. `src/tool_advisor/causal_active.rs` is not modified at all.
- The M005 freeze record and benchmark fingerprint are untouched, and
  `m005_freeze_record_matches_live_contracts` is green. This invariant is
  self-enforcing: raising the budget breaks a passing test.
- `replay_p95_within_budget` and the holdout p95 are unmodified and green, so
  the budget is still enforced distributionally over 201 warm samples and
  across the holdout scenarios.
- An assertion exists at all three sites. None was deleted, skipped, or
  ignored. The count of budget-asserting sites is unchanged.
- The gates still fail on a real breach, demonstrated deliberately rather than
  assumed.
- No production code path changed. The diff is three test files: a unit-test
  module and two integration test files.
- No causal-frontier scoring, admissibility, promotion, disclosure, or
  effect-path behavior changed; no model was retrained; no advisor artifact was
  refrozen; `ResolvedToolSurface` authority and advisor default-off/local-only
  behavior are untouched.
- No historical closure record was edited. The causal frontier `001`–`005`
  records and the M006-A `005-status.md` remain byte-identical, and this
  corrective is recorded additively alongside them.

## 6. Failure and recovery review

Not applicable in the production sense: no state, process, authority, or
persistence surface is involved.

For the measurement itself, the accepted trade-off is explicit: a preemption
that lands on all 15 warm samples would still fail the gate. That is
deliberate. Fifteen consecutive preemptions is a materially different signal
from one cold sample, and accepting that residual is the price of removing the
15-in-15 confounder. A future recurrence would be a genuine sustained-load
signal, not this defect returning.

The baseline-reproduction worktree was removed with `git worktree remove
--force` after use; no stray worktree or scratch artifact remains in the
repository. Temporary budget modifications made for the breach proof were
restored and re-read from source before commit.

## 7. Migration and compatibility review

None. No schema, storage, protocol, configuration, or public API change. Both
constants remain `pub` and unchanged, so downstream consumers observe no
difference. MSRV 1.89 is preserved; `std::hint::black_box` is stable well below
it.

The removed import in `tests/causal_observe_replay.rs` is a test-internal
detail with no compatibility surface, and leaving it would have been a
`-D warnings` failure.

## 8. Security review

None applicable. No authorization, credential, network, sandbox, or privilege
surface is touched, and no production code changes. The change reduces the
influence of ambient runner scheduling on a pass/fail decision; it does not
grant any capability, relax any authorization, or widen any input path.

## 9. Documentation and operations

- `plans/subsystems/tool-selection-advisor-causal-frontier-timing-corrective-addendum.md`
  — the corrective record, with the three triggering hosted runs and the
  provenance of the deferred M005 classification.
- `plans/implementation/tool-selection-advisor-causal-frontier-timing-corrective/001-warm-best-of-n-budget-gates.md`
  — this milestone's plan.
- This closure record.
- `plans/registry.md` — roadmap row, implementation-plan row, closure row, and
  the causal-frontier gate paragraph.
- No `architecture/` document changes. No production contract, ownership, or
  behavior changed, and the budget semantics those documents describe are
  unchanged. The `warm_min_millis` doc comments at both sites carry the
  rationale in-tree, so a future reader encountering best-of-15 in a latency
  assertion finds the reasoning next to it.
- Operator note: if this class of failure reappears under a *different* frozen
  budget elsewhere, the correct response is the one taken here — fix the
  measurement, never the frozen constant.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The fix accepts that 15 consecutive scheduler preemptions would still fail a gate. | A genuinely sustained-load runner could still produce a false failure. | Accepted trade-off, documented in §6. If it recurs, raise `BUDGET_SAMPLES` rather than touching the constant. |
| low | `src/tool_advisor/mod.rs:2946` has a loose `elapsed_millis < 1000` (1 s) assertion. | A different defect class with a much looser budget; not a flake source today. | Deliberately out of scope for this corrective (§5). File separately if it ever flakes. |
| medium | `codegg::project_activation::concurrent_same_owner_activation_coalesces_scope_and_bundle` fails under hosted load: eight concurrent same-owner activations do not all coalesce onto one lease id (`tests/project_activation.rs:146`). Discovered in run `37223675386` on identical code to the green run; passes 12/12 locally; untouched by this PR. A separate `WebKitGTK` E2E failure (`m004-session.e2e.ts`, "completed assistant transcript never rendered") failed in the same window and was green in the prior run. | Keeps the branch red for reasons unrelated to the causal advisor and to M006-A, and re-truncates the sweep at ~5,300 of 12,180 — the exact failure mode this corrective just removed for the advisor gates. | File its own corrective for the project-activation coalescing test and the M004 E2E transcript wait. Do **not** fix it inside this or M006-A: it is a distinct defect in a distinct subsystem, and repeating "classify and defer" is how the causal timing flake survived three hosted runs. |

Neither finding is a blocker, and neither is a regression introduced here.

Resolved by this milestone, for the record:
- The medium finding in `plans/closure/desktop-frontend-ide-foundation/005-status.md`
  §10 (causal advisor timing flake) is discharged. That record is not edited;
  the discharge is recorded here and in the registry.
- The deferred load-flake note in
  `plans/closure/tool-selection-advisor-causal-frontier-experiment/005-status.md`
  §10 is now fixed rather than merely classified. That record is not edited
  either.

## 11. Roadmap disposition

The corrective addendum is terminally satisfied; no successor milestone exists
in this track.

This closes the defect that kept PR #92 — and any other branch touching the
advisor suites — red, and restores the full 12,180-test workspace sweep that
fail-fast had been truncating. It reopens no causal-frontier scope: M001–M005
remain closed with their original dispositions (positive, A, D, positive, B),
the frozen budgets and freeze record are untouched, and the historical
live-primary-model study stays blocked exactly as M005 left it.

M006-A's conditional close is unaffected in substance and now lacks its named
outstanding evidence: the condition recorded against it — a pre-existing flake
keeping the branch red — no longer applies.

## 12. Registry updates

- `plans/registry.md` roadmap/status table: add the causal-frontier timing
  corrective row, closed, with implementation `cdfd6257`.
- `plans/registry.md` implementation-plan table: add C001, closed, pointing at
  the plan and this closure.
- `plans/registry.md` closure table: add C001, closed, with the hosted run id
  and the flake-elimination result.
- `plans/registry.md` causal-frontier gate paragraph: append that the
  single-sample latency flake observed in run `37047118019` and again in
  M006-A's run `37219495080` is now fixed by this corrective with the frozen
  constants unchanged.
- No other registered plan lists this corrective as a hard dependency, so
  nothing else is unblocked by it. The practical effect is the M006 branch's
  hosted evidence, which was blocked on this.
