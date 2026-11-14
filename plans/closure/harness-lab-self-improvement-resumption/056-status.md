# Harness Lab Self-Improvement Resumption S056 Closure — Safe Runner Exception Origin

Status: **closed at safe exception-origin telemetry scope; attempt 75 unqualified**

Repository baseline: `e94435fb1ad82c92e6bebd1bd2adcfee178f0e4b`

Implementation commit:

- `e3fce7b99f13380d9e36fafc49c844ccc646be69` — allowlisted exception type and bounded CodeGG function/line context.

Source plan: `plans/implementation/harness-lab-self-improvement-resumption/056-safe-runner-exception-origin.md`.

## 1. Executive finding

S056's diagnostic objective is complete. Attempt 75's safe projection identified a `NameError` in `dev.harness_lab.autonomy_run:run_case` at line 3729, with the outer `run_suite` frame at line 4236. Source inspection confirms that `tool_calls` is read at line 3729 before its assignment later in the function. This is a CodeGG harness ordering defect, not an inferred model/provider failure. Attempt 75 remains unqualified because task completion and verifier projections are unavailable; S057 owns the narrow fix.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Allowlisted exception type | Attempt 75 safe monitor and campaign evidence | pass | `NameError`; custom exception names remain suppressed. |
| Bounded source-line context | Attempt 75 safe monitor and campaign evidence | pass | `run_case:3729` and `run_suite:4236`; no paths, messages, or locals. |
| Independent sanitizer | `test_campaign_authorization` | pass | Malformed, path-bearing, and invalid line values are removed. |
| H007 qualification | Attempt 75 safe campaign evidence | fail/unavailable | The runner raised before completion and verifier projections were unavailable. |

## 3. Production implementation evidence

Commit `e3fce7b99f13380d9e36fafc49c844ccc646be69` added `runner_failure_context` to the existing exception diagnostic. Only an allowlisted built-in or known CodeGG exception label and at most eight validated in-repository module/function/line tuples are retained. Attempt 75 used the unchanged v97 manifest, verifier, Muse route, limits, and ARM64 pair. It ran for 3,252.913 seconds and admitted 209 provider requests. The safe runner class was `unexpected_exception`, while the new context provided the more useful `NameError` type and line locations.

## 4. Verification executed

### Commands run

```bash
python3 -m unittest dev.harness_lab.tests.test_cli dev.harness_lab.tests.test_campaign_authorization dev.harness_lab.tests.test_autonomy_run
python3 -m py_compile dev/harness_lab/__main__.py dev/harness_lab/campaign_authorization.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
```

The focused modules passed 116 tests; compilation, both test-quality guards, and diff checks passed. Attempt 75's safe provider-boundary aggregate and campaign row both report 209 requests. Safe evidence: `evidence/s019-attempt75-safe-diagnostic-20261114.json`, `evidence/s019-attempt75-preflight-20261114.json`, and `evidence/s019-attempt75-monitor-summary-20261114.json`. The private receipt, runner output, transcript, raw provider/tool payloads, and verifier material were not inspected. USD is unknown when unreported.

## 5. Invariant review

Exception types come from a fixed allowlist; custom types use a generic label. Source locations include only validated `dev.harness_lab` module/function identifiers and positive bounded line numbers. Paths, messages, locals, provider content, and verifier content are not retained. The original task, verifier, model route, and caps remain unchanged.

## 6. Failure and recovery review

The exception accounting matched 209 requests and consumed one start. The external disk and wall stops did not fire. Free disk was 52,085,145,600 bytes at start, 51,126,001,664 bytes minimum, and 169,186,324,480 bytes at exit. The safe diagnostic and source at the pinned head establish that a local variable is used before assignment in the post-run qualification projection. No task or verifier result is inferred.

## 7. Security and privacy

The exception text and private run artifacts were not inspected. The committed diagnostic contains only sanitized projections and non-secret identity/hash fields. Unreported billed USD is recorded as unknown.

## 8. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | `run_case` uses `tool_calls` in the H007 qualification projection before assigning it later in the post-run path. | Attempt 75 stops before task/verifier projections are produced; H007 cannot qualify. | S057 collects raw tool calls before deriving pause-adjusted qualification calls and adds a regression. |

## 9. Roadmap disposition

S056 is closed at safe exception-origin telemetry scope. S057 owns the reproducible local binding-order defect. H007 remains unqualified; no daemon, scheduler, ACP, provider, or workload redesign is indicated by this evidence.

## 10. Registry updates

Mark S056 closed at source-location/type diagnostic scope and register S057 active. Preserve attempt 75 and its consumed authorization.
