# S046 — Campaign identity and safe H007 completion evidence

Status: **closed at safe identity/projection scope; H007 remains unqualified**

Completed: 2026-11-12

## Result

- The validated preflight remains authoritative for attempt, grant, route, model, launched suite manifest, and binary identity. Attempt 57 and 58 safe evidence both retain the launched suite digest; attempt 56's historical mismatch was preserved and separately documented.
- The campaign projector now receives the selected H007 case row nested under the suite receipt's `runs` array. A production-shaped CLI regression protects that envelope boundary and proves embedded private fields are omitted.
- The safe projection now carries task completion/reason, verifier outcome and coarse failure domains, plus public continuation stage/gate completion, validity, review, overlap, and artifact presence.
- Focused campaign/CLI tests passed: 17 tests. The autonomy, verifier-diagnostic, and H007 suites passed: 197 tests. Test assertion-quality and duplication guards passed, Python compilation passed, and `git diff --check` passed.

## Live evidence

Attempt 58 used `opencode_go/muse-spark-1.3-contributor`, the unchanged v97 H007 fixture/verifier, 175/1,200 provider requests, and the existing 3,600-second wall limit. It ran 2,060.705 seconds with 2,039.993 seconds of active work; event measurement was complete and duration qualified. The safe trace recorded 215 paired native tool starts/terminals, 36 job observations, two repair-cycle events, one continuation, and verifier start/terminal.

The task ended normally but did not pass the independent verifier: `task_outcome=failed`, `agent_terminal=completed`, `stop_reason=end_turn`, `horizon_reason=task_not_passed`, and verifier outcome `assertion_failures`. The safe continuation summary reports all 7 stages, all 12 gates, required review, release overlap, and public artifacts complete. The coarse verifier comparison reports one continuation-boundary failure domain with public evidence unavailable and one transcript-job-lifecycle domain whose public projection is complete but disagrees with the verifier. No raw prompt, transcript, tool payload, private receipt, or verifier output was inspected.

Attempt 58 is therefore **not** a qualification pass. The observation horizon and evidence projection worked; remaining work is to expose which fixed public checks were missing or disagreed so a narrow task/evidence corrective can be assigned. S047 owns that bounded telemetry extension.

## Evidence

- Preflight: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt58-preflight-20261112.json`
- Safe diagnostic: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt58-safe-diagnostic-20261112.json`
- Attempt 57 preflight and safe diagnostic preserve the projection gap and corrected launch identity.

## Limits and counters after attempt 58

- Campaign: 58/200 starts consumed; 142 remain.
- Supplemental grant `s019-supplemental-20261111-04`: 14/40 consumed; 26 remain.
- Current task authorization: 29/120 consumed; 91 remain.
- Cost remains `unknown if unreported`.
