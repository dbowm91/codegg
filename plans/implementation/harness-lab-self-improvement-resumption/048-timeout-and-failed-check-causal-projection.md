# Harness Lab Self-Improvement Resumption S048 — Timeout and Failed-Check Causal Projection

Status: **closed at safe causal-projection scope; attempt 60 unqualified**

Repository baseline: ac584c886f03847ff6b829f8509667637e42a052 (shared dirty worktree; preserve unrelated edits)

Source roadmap:

- plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#42-s048--timeout-and-failed-check-causal-projection

Long-term requirements:

- plans/000-long-term-specification.md §§26–29
- plans/002-long-term-roadmap.md

Applicable ADRs:

- plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md

Primary class: **diagnostic observability**

## 1. Objective

Expose the minimum additional fixed, content-free evidence needed to tell whether H007's failed repair, continuation-layout, and job-ID checks came from incomplete agent work, missing instrumentation, or a request timeout at the verifier-reserve boundary.

## 2. Why this milestone is ready

Attempt 59 used the same Muse route and v97 task, completed 3,299.871 seconds of runtime with 3,291.890 seconds of measured active work, and stopped at turn_timeout with task_outcome=failed. The run made 322/1,200 provider requests and recorded 385 matched native tool starts/terminals plus 78 job observations. The 3,600-second hard wall and 300-second verifier reserve imply a 3,300-second prompt-work ceiling.

Per-check evidence now says the continuation is present but invalid, with progress ordering and pointer placement unavailable. The transcript-job lifecycle has exact running/terminal state, productive overlap, complete event measurement, terminal test jobs, review, release overlap, and a matching serialized projection; however, named repair pairs, job-tool ID binding, and request/result ID correlation are false. The current campaign projection discards the safe line-layout counts, repair-group predicates, correlation state counts, unbound-call state counts, and turn-timeout boundary timing that already exist in bounded producer diagnostics.

The stored OpenCode Go entry already matched the user-authorized temporary key when compared in memory; no credential change was required. The value and any fingerprint were not emitted.

## 3. Current implementation evidence

h007_results._verifier_public_reconciliation already produces named Boolean checks and an unbound-call summary. summarize_job_tool_sequence has bounded request/result correlation state counts, summarize_required_repair_jobs has fixed target-group Booleans/counts, and summarize_continuation_line_layout reports fixed structural counts. _h007_prompt_timeout_diagnostic already contains request count, last-request timing, remaining-wall timing, and the verifier reserve. S047 projects only the per-check Boolean/null layer.

## 4. Invariants

- Keep H007 v97 task content, verifier, route, and qualification criteria unchanged.
- Use only opencode_go/muse-spark-1.3-contributor.
- Keep the 1,200 provider-request, 1,600 native-call, 3,600-second hard wall, 3,300-second prompt-work ceiling, 300-second verifier reserve, 300-second inactivity cancellation, 10-GiB runtime disk floor, and 30-GiB preflight reserve.
- Do not inspect or retain private receipts, prompt/transcript contents, hidden verifier source/output, assertions, raw IDs, tool payloads, provider text, or credentials.
- Add only allowlisted booleans, enum counts, bounded integer counts, and bounded timing values. Unknown fields and invalid types are dropped.
- Preserve failed attempts and count only provider-admitted starts. USD remains unknown when the provider does not report it.

## 5. Scope

### In scope

- Project fixed continuation-line section counts and named repair-group Boolean/count diagnostics.
- Project bounded job request/result correlation state counts and unbound-call state counts.
- Project the existing H007 turn-timeout diagnostic with request/follow-up counts, last-request timing, remaining wall time, and the verifier reserve.
- Add privacy regressions and a production-shaped CLI envelope test.
- After offline checks and a fresh 30-GiB disk preflight, run the unchanged v97 H007 task once to obtain the richer safe diagnostic.

### Explicitly out of scope

- Changing H007 instructions, task difficulty, hidden verifier, source fixtures, request caps, hard wall, verifier reserve, timeout semantics, daemon, scheduler, ACP/job architecture, provider integration, or cancellation behavior.
- Treating correct duration, matching serialized projection, or public check completion as qualification.

## 6. Required production changes

Extend the safe H007 projection at the existing campaign boundary. The repair projection may use only the two fixed H007 groups and their known Boolean predicates; structural projection may use only fixed counts and booleans; correlation projection may use only fixed state names and counts; timeout projection may use only the existing timing fields and fixed schema/state. Reject booleans where integers are expected, reject out-of-range counts/times, and discard all unknown names and values.

The task row must continue to be selected from the suite receipt's runs array, while launch identity remains sourced only from the passed preflight. A missing projection remains unavailable.

## 7. Ordered work packages

### Work package A — Safe producer and campaign projection

Intent: carry already-computed bounded correlation counts into the public reconciliation and preserve fixed continuation, repair, and timeout details at the campaign boundary.

Required changes: add fixed allowlists and bounded projectors in campaign_authorization.py; add the correlation count projection in h007_results.py.

Acceptance evidence: unit fixtures cover valid, missing, malformed, unknown, private-string, boolean-as-integer, and over-limit inputs; output contains no injected private marker.

### Work package B — Production-shaped CLI regression

Intent: prove the case-row envelope preserves the new safe fields without leaking any private content.

Acceptance evidence: CLI fixture nests diagnostics under the selected H007 runs row and asserts timeout timing, repair-group state, line counts, and job-correlation counts in the safe evidence only.

### Work package C — Offline verification and live diagnostic

Intent: validate the privacy boundary, then use one authorized Muse run to select the next corrective owner.

Acceptance evidence: focused campaign/CLI and H007 diagnostics tests, test-quality guards, Python compilation, and git diff --check pass; attempt 60 uses the unchanged v97 task and all listed controls and records safe terminal evidence.

## 8. Failure, cancellation, and retry semantics

Refuse before provider admission if identity, grant headroom, exact suite/binary, credential availability, or the 30-GiB preflight reserve fails. Preserve the existing inactivity, request, tool-call, wall, and runtime disk stops. A timeout remains unqualified unless the independent verifier and every existing H007 gate pass. Never retry within a run after terminal completion.

## 9. Compatibility

All new receipt fields are additive campaign evidence. No persisted product schema, provider auth record, or external protocol changes.

## 10. Required tests

### Focused unit tests

- Valid and invalid bounded timeout, line-layout, repair-group, unbound-state, and correlation-state projections.
- Hostile unknown keys and private strings do not survive.
- true is rejected as an integer count; oversize counts and timing are rejected.

### Integration tests

- Production-shaped CLI receipt with the selected H007 case nested under runs.
- Safe projection still uses canonical launch identity from preflight.

## 11. Required verification commands

```bash
rtk proxy python3 -m unittest -v dev.harness_lab.tests.test_campaign_authorization dev.harness_lab.tests.test_cli
rtk proxy python3 -m unittest -v dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics dev.harness_lab.tests.test_h007
rtk proxy python3 scripts/check_test_assertion_quality.py
rtk proxy python3 scripts/check_test_duplication.py
rtk proxy python3 -m py_compile dev/harness_lab/campaign_authorization.py dev/harness_lab/h007_results.py dev/harness_lab/tests/test_campaign_authorization.py dev/harness_lab/tests/test_cli.py dev/harness_lab/tests/test_h007.py
rtk proxy git diff --check
```

## Offline execution record

The first focused H007 suite run caught a missing local bound constant in the new correlation projection; the code was corrected to use its explicit 1,600-call cap. The final campaign/CLI suite passed 18 tests. The autonomy, verifier-diagnostic, and H007 suites passed 197 tests. Test assertion-quality and duplication guards, Python compilation, and git diff --check passed.

Candidate start 60 passed preflight at 132.880 GiB and ran under the unchanged v97 task and configured caps. It completed 3,299.655 seconds with a 3,288.685-second active-work span, 390 provider requests, and exit code 2. The timeout occurred at the 3,300-second prompt-work ceiling with 299.974 seconds of verifier reserve remaining. The independent verifier did not qualify H007. Safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt60-preflight-20261112.json` and `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt60-safe-diagnostic-20261112.json`. Runtime disk samples stayed above 119.303 GiB; the 10-GiB runtime floor did not trigger. Counters are 60/200 campaign, 16/40 grant, and 31/120 task authorization.

## 12. Documentation updates

The S048 closure records the attempt 60 result in the S019 roadmap and registry. Attempt 60 reached the declared prompt-work ceiling and did not qualify; it used 390/1,200 provider requests, so it was not request-exhausted. S049 owns the one failed job-status error-class detail that remains unavailable in public evidence.

## 13. Acceptance criteria

- Safe evidence identifies whether the missing line layout was absent, duplicate, or incomplete by fixed counts.
- Safe evidence identifies which fixed repair predicates failed and how many job calls were unbound or mismatched.
- Safe evidence distinguishes turn timeout from verifier-reserve stop and reports the bounded request timing and remaining wall budget.
- Unknown/private values never survive the projection.
- Attempt 60 remains unqualified because the unchanged independent verifier and existing H007 requirements did not pass.

## 14. Stop conditions

- Stop if any required field can be projected only from raw task, transcript, provider, tool, credential, or hidden-verifier content.
- Stop if the projection requires weakening H007 gates or increasing any authorized ceiling.
- Do not inspect private receipts or raw verifier output.

## 15. Closure evidence required

Recorded offline test results, exact attempt 60 preflight/safe-evidence paths, route and model, campaign/grant/task counters, request and tool counts, bounded failure checks, observed disk samples, and the unqualified result in `plans/closure/harness-lab-self-improvement-resumption/048-status.md`. Raw private receipt and runner log were not inspected.

## 16. Handoff notes

S047 is closed at its per-check public-reconciliation scope. S048 is closed at safe causal-projection scope; H007 remains unqualified. Attempt 60 shows the verifier domains are genuinely incomplete: the lifecycle projection matches the event trace, while one `catalog_format` seed status was not the immediate next call and its failed status result has no returned ID. S049 will add a fixed error-class count for failed job-status calls. Preserve failed evidence; never commit the credential, private receipt, or raw runner log.
