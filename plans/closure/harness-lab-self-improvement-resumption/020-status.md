# S020 status — terminal-sequenced seeded repair calibration

Status: **superseded by S021; attempt 27 unqualified**

## Attempt 25 finding

Attempt 25 completed after 55 minutes with healthy event, job, overlap, and resource telemetry. The task remained incomplete and the independent verifier failed. Its safe receipt showed that both named repair jobs were submitted before the corresponding seed jobs had been observed in terminal `failed` state. The note claimed two cycles while six generic pairs were observed.

## Attempt 26 finding

Attempt 26 used v81 and completed normally after 42.3 active minutes. It used 350/1,200 provider requests and 386/1,600 native calls; all 35 test jobs were terminal, the 917-event trace had no gaps or drops, continuation and resource gates passed, and no provider, daemon, scheduler, ACP, event-capture, stall, or disk failure was observed. The independent verifier passed four of five tests and failed its transcript lifecycle/repair assertion. The safe receipt showed `required_pair_contract_enabled: false` and no per-pair diagnostic because v81 omitted the four literal `job_id_*` activation markers. This leaves the named repair sequence unqualified and unattributed. Evidence: `../closure/harness-lab-self-improvement-resumption/evidence/s019-attempt26-safe-diagnostic-20261110.json`.

## v82 corrective and preflight

v82 preserves the frozen task and verifier, restores all four named job-ID markers and all four corresponding job-state fields, and names attempt 27. Its focused regression proves v81 does not activate `_h007_case_requires_repair_job_pairs` while v82 does, and checks that all other manifest fields are unchanged.

Verification passed: `test_h007` (67 tests); `test_autonomy_run`, `test_h007_attribution`, and `test_verifier_diagnostics` (97 tests); assertion-quality and duplication guards; Python compilation; and `git diff --check`. Harness doctor and budget admission passed. The manifest SHA-256 is `c593389268b71587846d3d44d09d1c952c5c4c69aa8b36a4619859d2fba930f7` (9,996 prompt characters). The same ARM64 binary pair remains verified (`ac668725…` / `f82a711e…`); the independent verifier hash remains `13617671…`. Preflight recorded 191,565,185,024 free bytes, above the 30-GiB reserve, with an empty attempt-27 lane and receipt. Evidence: `evidence/s019-attempt27-preflight-20261110.json`.

Attempt 27 started on v82 after the preflight below. The authoritative ledger records 28 of 120 combined starts consumed, 92 remaining; the current 40-start supplemental grant has consumed 5, leaving 35. The only route/model is `opencode_go/muse-spark-1.3-contributor`; limits remain 1,200 provider requests, 1,600 native calls, 3,600 seconds wall time, 3,300 seconds prompt work, 300 seconds inactivity, 10-GiB hard disk floor, and at most 100 same-session follow-ups. Unreported USD cost is recorded as `unknown`.

Attempt 27 must demonstrate both exact-ID terminal-failure → direct-edit → distinct repair-submit → terminal-completed sequences, the full independent verifier pass, and every existing H007 horizon, continuation, event, overlap, and resource gate in one run. A failure will be classified from the bounded per-pair and causal diagnostics before any further narrow correction.


## Attempt 27 result and S021 handoff

Attempt 27 reached 54.9 active minutes and ended at the 3,300-second prompt-work limit. It used 310/1,200 provider requests (reported allowance USD 0.179443) and 361/1,600 native calls. Causal observation was complete at 815 events with zero gaps/drops and all calls terminal; maximum activity gap was 124,734 ms. The activity-stall and disk-floor guards did not trigger. One read-only reviewer completed the required exact status-then-get lifecycle. No provider, daemon, scheduler, ACP, disk, or event-capture failure was observed.

The task did not qualify. The verifier ran five tests, passed two, and failed three: `test_durable_continuation_records_completed_phases_and_next_boundary`, `test_end_to_end_import_recovery_lineage_diff_and_deterministic_export`, and `test_transcript_proves_same_id_running_overlap_terminal_repairs_and_continuation`. Assertion text, traceback, hidden verifier source/output, provider text, raw job IDs/arguments, and transcript are not retained. The run ended with `agent_terminal: timeout`, `continuation_stop_reason: turn_timeout`, and an incomplete task diagnostic even though the public note listed complete stages/gates and `stop_reason` was `end_turn`.

The strict v82 projection found one of two required pairs qualified: `catalog_format` missed immediate status after seed submission; `catalog_store` passed. Both pairs had distinct IDs and terminal failed/completed observations. The pair validator reported one qualified pair, but `_observed_repair_cycle_pairs` and the causal trace reported zero cycles. This internal projection mismatch is the narrow reproducible harness corrective owned by S021. The failed format sequence remains a model/task behavior. The end-to-end import/recovery/lineage/diff/export assertion is a separate unqualified task result; its exact assertion is unavailable under the verifier privacy boundary, so no more specific source defect is claimed.

Safe evidence: `evidence/s019-attempt27-safe-diagnostic-20261110.json`; receipt SHA-256 `4c4361f6a656c0efcf9ebfa6125d89f80c7617773c47bf68bc308d5701a35c4b`. S019 has consumed 28/120 starts, with 92 remaining; the current supplemental grant has consumed 5/40, with 35 remaining.


S021’s offline corrective is now implemented and verified. v83’s exact next-native-call prompt regression and the repeated-terminal repair-correlation regression pass within 182 focused tests; both quality guards, compilation, and diff check also pass. The next permitted action is only attempt 28 preflight/live qualification under the existing authorization.
