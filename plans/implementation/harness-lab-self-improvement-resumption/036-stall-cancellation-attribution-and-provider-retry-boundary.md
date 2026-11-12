# Harness Lab Self-Improvement Resumption Milestone S036 — Stall Cancellation Attribution and Provider Retry Boundary

Status: closed at stop-attribution scope; H007 remains unqualified after attempt 45 verifier failures

Repository baseline: ac584c886f03847ff6b829f8509667637e42a052 (shared dirty worktree; preserve unrelated edits)

Source roadmap: plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#30-s036--stall-cancellation-attribution-and-provider-retry-boundary

Long-term requirements:

- plans/000-long-term-specification.md §§23–29
- plans/002-long-term-roadmap.md

Applicable ADRs:

- plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md

Primary class: infrastructure

## 1. Objective

Make a cancellation caused by the H007 native-tool inactivity watcher distinguishable from an earlier successful end_turn in the same ACP session. Preserve the provider retry evidence and classify the run as a provider-stream stall followed by the configured activity cancellation. Reattempt unchanged fixture v93 on the already-authorized Muse/OpenCode Go route after the correction.

## 2. Why this milestone is ready

Attempt 43 exercised S035 on the unchanged v93 workload and ended unqualified after 30 minutes. It recorded 1,499,285 ms of active work, then 300,481 ms with no native tool activity. The 300-second ActivityStallWatcher fired and cancelled the ACP session. Event observation remained complete: 140/140 tool starts and terminals, with no gaps or drops; the disk minimum was 41,490,014,208 bytes.

Provider accounting recorded 92 attempts started, 90 succeeded, one failed and one unresolved at cancellation. The failed attempt received no decoded response event, timed out after 240,004 ms in first_response_event, and scheduled a retry. The later attempt remained unresolved when the stall watcher cancelled the turn. The task remained incomplete: zero of seven public stages and zero of twelve gates were complete; the reviewer and required release overlap were unobserved; the verifier ran five tests and did not pass.

The receipt's top-level stop_reason nevertheless said end_turn, copied from the previous prompt. Prompt 3 itself was classified as cancelled, and activity_stall.triggered was true. In run_case, stop_reason is only overwritten after session/prompt returns normally; a later HarnessTurnFailure leaves the earlier prompt's value in place. The attempt therefore supplies a production-shaped reproduction of stale terminal attribution.

## 3. Current implementation evidence

- dev/harness_lab/autonomy_run.py keeps one stop_reason across same-session follow-up requests.
- A normal prompt return assigns its ACP stopReason; a subsequent HarnessTurnFailure marks the terminal cancelled or failed without clearing or replacing the old reason.
- The final receipt separately records activity_stall.triggered, which exposed the inconsistency, but consumers can mistake the stale end_turn for the cause of termination.
- Existing provider-attempt telemetry is sufficient to distinguish a first-response timeout, a scheduled retry, an unresolved request, and the later activity cancellation without retaining provider text.
- Attempt 43 is recorded in evidence/s019-attempt43-safe-diagnostic-20261111.json; its private raw transcript and verifier output remain sealed.

## 4. Invariants that must not regress

- Use only opencode_go/muse-spark-1.3-contributor; keep the replacement credential in the protected auth store and never print or persist its value.
- Preserve manifest v93, hidden verifier, paired ARM64 CodeGG binary/helper, task inputs, public tests, and all H007 qualification gates.
- Keep the 1,200 provider-request, 1,600 native-call, 3,300-second prompt-work, 3,600-second wall, 300-second tool-inactivity, 100-follow-up, 10-GiB runtime disk floor, 30-GiB preflight reserve, and five-minute verifier-reserve bounds unchanged.
- Keep cancellation, provider failure, provider timeout, returned end_turn, prompt timeout, and verifier outcome as distinct observations.
- Keep receipt projections body-free: no prompt/provider text, raw verifier output, raw tool arguments or outputs, paths, raw job IDs, or delegated task IDs.
- Do not alter provider retry policy, model selection, daemon/scheduler/ACP architecture, workload, verifier, or qualification thresholds without separate direct evidence.

## 5. Scope

### In scope

- Clear prompt-local stop state before each session/prompt call.
- When the prompt raises HarnessTurnFailure, assign a stop classification for that failure instead of retaining the prior prompt's return value.
- On cancellation, record a bounded cancellation_source (activity_stall, disk_floor, or external_or_transport) alongside the terminal classification.
- Distinguish provider timeout from generic request failure in the prompt boundary projection while preserving the existing 300-second cancellation policy.
- Add a production-shaped regression with one successful end_turn, then a later cancellation from the activity watcher; assert the final receipt and prompt timeline report cancellation and do not inherit the previous reason.
- Run the unchanged v93 case as attempt 45 under the remaining explicit authorization.

### Explicitly out of scope

- Raising request, stream-idle, inactivity, wall-clock, disk, or provider-retry limits.
- Treating a pending provider request as task progress or as a completed response.
- Restarting a failed model turn, changing OpenCode Go adapter behavior, or switching providers/models.
- Reopening job lifecycle, daemon, scheduler, ACP event capture, hidden verifier, or workload architecture.
- Inferring that attempt 43's incomplete task is a correct implementation or a CodeGG product defect.

## 6. Required production changes

### Stop and cancellation attribution

- Introduce a small typed/allowlisted projection helper for HarnessTurnFailure and watcher state.
- Reset stop_reason before each prompt request so an exception cannot inherit a previous end_turn.
- On cancelled, keep the terminal stop class cancelled and separately project the watcher source.
- On typed provider_timeout, record provider_timeout; do not call it a controller prompt timeout unless HarnessTimeout actually fired.
- Add cancellation_source to the run receipt with only the three declared values or None.
- Keep the H007 prompt-boundary timeline bounded and versioned; add provider_timeout to its accepted stop classes if required to retain that distinction.

### Evidence and privacy

- Reconcile top-level, prompt-level, and watcher-level terminal fields before writing the private receipt.
- Keep the existing provider attempt counts and timeout timings; do not expose attempt IDs, exception messages, model responses, or raw logs.
- Preserve backward compatibility: existing receipts without cancellation_source remain readable as unavailable.

## 7. Ordered work packages

### Work package A — Prompt-local stop state

Clear stop state before each prompt request and assign a failure classification in the HarnessTurnFailure path. Derive cancellation source from the existing disk and activity watchers. Do not alter what triggers either watcher.

### Work package B — Production-shaped regression

Use an ordered ACP sequence with a completed first prompt (end_turn), then a second prompt interrupted after the inactivity watcher requests cancellation. Assert the second prompt is cancelled, the run terminal is cancelled, cancellation_source is activity_stall, and no stale end_turn is exposed as the terminal reason. Add companion assertions that typed provider timeout and actual controller timeout remain distinct.

### Work package C — Focused verification and live retry

Run the focused autonomy/H007 tests, assertion-quality and duplication guards, Python compilation, and git diff --check. Then run attempt 45 against the same v93 manifest and ARM64 binary/helper pair, using the existing Muse/OpenCode Go route and all original caps. Continue monitoring free disk at least every 15 seconds and cancel on the existing floor or inactivity limit.

## 8. Failure, cancellation, restart, and contention semantics

The inactivity watcher continues to cancel after 300 seconds without ACP tool-call activity. A provider request that has not produced a response event is not silently treated as progress; its first-response timeout, retry, unresolved state, and any later activity cancellation remain separate. A watcher cancellation must not be represented as a normal end_turn. No retry, restart, or cap reset is introduced by this plan.

## 9. Compatibility and migration

Receipt changes are additive. No database, protocol, fixture, verifier, or provider-policy migration is required. Older receipts with no cancellation source remain valid and are interpreted as unavailable evidence.

## 10. Required tests

- A completed prompt followed by a watcher cancellation cannot retain the earlier end_turn.
- Disk-floor and activity-stall cancellations preserve their own bounded source.
- A provider timeout is distinct from a controller HarnessTimeout.
- Unknown cancellation-source text is projected as unavailable, never copied through.
- Production-shaped prompt-boundary projection contains no sentinel prompt/provider text, error text, arguments, outputs, paths, raw job IDs, or task IDs.

## 11. Required verification commands

    python3 -m unittest dev.harness_lab.tests.test_autonomy_run
    python3 -m unittest dev.harness_lab.tests.test_h007
    python3 scripts/check_test_assertion_quality.py
    python3 scripts/check_test_duplication.py
    python3 -m py_compile dev/harness_lab/autonomy_run.py
    git diff --check

## 12. Documentation updates

Update S019 status, the S019 ledger, the S035/S036 plans, the subsystem roadmap, and registry with attempt 43's provider timeout and watcher cancellation. Record that the harness honored the 300-second rule, that the model task and hidden verifier did not pass, that event capture and disk remained healthy, and that stop_reason=end_turn was stale. Do not infer a provider-side root cause beyond the observed timeout and unresolved attempt.

## 13. Acceptance criteria

- The final receipt reports a terminal cancellation with its bounded source; it cannot retain a previous prompt's end_turn.
- Prompt-boundary records distinguish provider timeout, activity-stall cancellation, and controller timeout.
- Regression tests prove the production-shaped sequence and privacy guarantees.
- Attempt 45 used the unchanged task, verifier, model route, binary/helper pair, and caps; it completed normally with corrected prompt-local stop attribution.
- H007 remains unqualified unless all existing task, horizon, exact-ID job overlap, two-repair, continuation, reviewer, gate, and hidden-verifier criteria pass.

## 14. Stop conditions

Do not launch if the exact model/route, protected credential availability, fixture/verifier identity, ARM64 binary/helper pair, budget, authorization counters, or 30-GiB preflight reserve fails. During the run, stop on the existing 10-GiB floor, 300-second inactivity cancellation, request/native-call cap, prompt-work cap, wall cap, operator cancellation, or untrustworthy telemetry.

## 15. Closure evidence required

- Focused tests and guards with truthful results.
- A production-shaped regression covering stale prior end_turn followed by activity cancellation.
- Attempt 45's preflight, redacted receipt, exact route/model attribution, event/provider diagnostics, disk watch, and reconciled authorization ledger are recorded in the linked S019 evidence.
- A requirement matrix that separates task incompletion, provider-stream timeout, harness cancellation, and event/resource infrastructure.
- If H007 remains unqualified, a successor only when the new receipt establishes a narrower corrective.

## 16. Handoff notes

Attempt 43 used start 44, leaving 34/40 starts in the prior grant and 116/160 combined starts; a fresh 40-start user authorization is now active. The active OpenCode Go credential was rotated through the revision-checked connection flow to revision 8; its value was not recorded and the store remains mode 0600. Offline verification passed: 71 autonomy-run tests, 82 H007 tests, both quality guards, Python compilation, and git diff --check. Attempt 44 was a launcher failure before provider startup because the command omitted the required --model argument; zero provider requests and no receipt were produced, and its start was consumed. The launcher now supplies the authorized model. Attempt 45 passed preflight and used start number 46. It completed normally with the corrected prompt-local stop projection; active work reached 3,230,530 ms. The task remained unqualified because two independent verifier assertions failed. Provider allowance was $0.033622; separately billed USD remains unknown. The run had no activity-stall cancellation or unresolved provider attempt, so it does not provide live cancellation attribution evidence beyond the corrected receipt path.
