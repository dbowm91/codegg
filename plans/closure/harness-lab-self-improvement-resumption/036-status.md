# Harness Lab Self-Improvement Resumption Milestone S036 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/036-stall-cancellation-attribution-and-provider-retry-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#30-s036--stall-cancellation-attribution-and-provider-retry-boundary`

Repository baseline reviewed: `ac584c886f03847ff6b829f8509667637e42a052` (shared worktree; unrelated edits preserved)

Implementation commits or pull requests: none recorded; changes remain in the shared worktree.

## 1. Executive finding

S036's bounded stop-attribution correction is implemented and its offline acceptance evidence is complete. Prompt-local stop state is reset, cancellation source is allowlisted, and provider timeout is distinguished from controller timeout. H007 qualification is not complete: attempt 45 met the sustained horizon and observation criteria but failed two independent verifier assertions. S036 closure does not claim a successful task or a live cancellation reproduction.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A later prompt cannot inherit an earlier `end_turn` | `dev/harness_lab/tests/test_autonomy_run.py`; 71 focused tests | pass | Includes completed prompt followed by activity-watcher cancellation projection. |
| Cancellation source is bounded and classified | Focused autonomy/H007 suites | pass | `activity_stall`, `disk_floor`, and `external_or_transport` are the only projected values; unknown text is unavailable. |
| Provider timeout differs from controller timeout | Focused autonomy/H007 suites | pass | Typed provider timeout remains separate from `HarnessTimeout`. |
| Production-shaped event sequence protects the stop projection | Regression tests and test-quality guards | pass | Prompt/provider text, arguments, outputs, paths, and raw IDs are excluded. |
| Corrective was exercised in the authorized route | Attempt 45 preflight and safe diagnostic | pass, scope-limited | Run completed normally; no cancellation occurred, so the live run did not exercise the cancellation branch. |
| H007 task and verifier qualified | Attempt 45 safe diagnostic | fail | Active-work, exact-ID overlap, continuation, reviewer, and repair criteria passed; verifier ran five tests and failed two assertions. |

## 3. Production implementation evidence

`dev/harness_lab/autonomy_run.py` resets H007 prompt stop state, classifies the current prompt's failure, and adds a bounded cancellation-source projection. Its prompt timeline retains typed provider timeouts without conflating them with controller timeouts. No provider retry policy, runtime cap, fixture, verifier, model route, daemon, scheduler, or ACP event protocol changed.

## 4. Verification executed

### Commands run

```bash
python3 -m unittest dev.harness_lab.tests.test_autonomy_run
python3 -m unittest dev.harness_lab.tests.test_h007
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m py_compile dev/harness_lab/autonomy_run.py
git diff --check
```

### Results

All focused offline checks passed: 71 autonomy-run tests, 82 H007 tests, both quality guards, Python compilation, and diff check. The attempt 45 verifier ran five tests: three passed and two assertion failures remained. Raw verifier output and source were not retained or inspected.

## 5. Invariant review

The 1,200-request, 1,600-native-call, 3,300-second prompt-work, 3,600-second hard-stop, 300-second inactivity, and 10-GiB runtime disk limits were unchanged. The 30-GiB reserve remained a preflight check. Provider accounting reported 219 requests and one failed attempt that was retried with no unresolved attempt. Attempt 45 recorded 254/254 tool starts/terminals and zero event gaps or drops. Secret values, prompt text, raw provider text, raw verifier text, tool arguments/outputs, paths, and job IDs were absent from the safe evidence.

## 6. Failure and recovery review

Attempt 45 ended normally before its wall limit. It did not trigger activity-stall, disk-floor, external cancellation, provider timeout, or event-loss handling. Its minimum free-disk sample was 31,930,093,568 bytes, below the 30-GiB preflight reserve but above the 10-GiB runtime floor; no runtime disk cancellation occurred. The task was unqualified because the independent verifier reported a numeric equality mismatch in its task-behavior domain and a transcript-pattern mismatch in its transcript/job-lifecycle domain. The latter remains ambiguous because the harness's separate horizon projection confirmed exact-ID running/terminal observation, productive overlap, four repairs, continuation, and reviewer completion. This evidence does not establish a harness stop or event defect.

## 7. Migration and compatibility review

No database, protocol, fixture, verifier, or provider-policy migration was required. The receipt addition is bounded and backward compatible; missing cancellation-source fields remain unavailable.

## 8. Security review

The temporary credential was rotated by the revision-checked daemon connection flow. Evidence records only connection revision and file mode; it does not retain the key. The active auth store remains mode `0600`. Safe evidence contains aggregates and redacted verifier classifications only.

## 9. Documentation and operations

Updated S019 status and campaign ledger, the S036 implementation plan, subsystem roadmap, registry, and this closure. Attempt 45 safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt45-safe-diagnostic-20261111.json` (mode `0600`).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Attempt 45's independent verifier reported two assertions; one transcript-pattern failure does not align cleanly with the separate passing horizon projection | H007 remains unqualified; underlying task-artifact mismatch is not fully attributed | Continue the same bounded live qualification and use only safe verifier diagnostics; do not infer a harness defect without reproduced evidence. |

## 11. Roadmap disposition

S036 is closed at its stop-attribution diagnostic scope. S019 remains active. Attempt 45 does not qualify H007.

## 12. Registry updates

- Mark S036 closed at stop-attribution scope.
- Keep S019 active with attempt 45 recorded as unqualified and the fresh 40-start authorization counters reconciled.
- Keep H007 qualification unclaimed until every task, horizon, exact-ID job, repair, continuation, reviewer, verifier, event, and resource gate passes.
