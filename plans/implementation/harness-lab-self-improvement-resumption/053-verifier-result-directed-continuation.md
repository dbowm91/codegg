# Harness Lab Self-Improvement Resumption S053 — Verifier-Result-Directed Continuation

Status: **implemented and offline-verified; live attempt 71 pending exact-head preflight**

Repository baseline: `c59e04b687a5790bc7f4516efb954840e7984db1`

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#47-s053--verifier-result-directed-continuation`

Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/003-planning-process.md` §§6–7.

Applicable ADRs: ADR-0040, evidence-based agent recovery and termination; ADR-0051, delegated-run continuation authority.

Primary class: infrastructure

## 1. Objective

When the H007 agent ends normally with all public continuation gates complete but the unchanged independent verifier reports assertion failures in a recognized safe domain, let that same ACP session perform one bounded corrective follow-up. Give the agent only the allowlisted failure-domain labels, then rerun the unchanged verifier and qualify only if the final result and all existing gates pass.

## 2. Why this milestone is ready

Attempt 70 ran 2,416.303 seconds total and 2,398.938 seconds active, used 157 provider requests, and ended normally. All seven stages, twelve gates, both repair pairs, required success jobs, review, productive overlap, continuation predicates, 200/200 tool events, and 30/30 job request/result correlations passed their public projections. The unchanged verifier still reported `continuation_boundary` and `transcript_job_lifecycle` failures.

The public reconciliation classified both domains as `verifier_public_projection_disagreement`; every projected check was true. The failed test-submit class histogram was empty because no failed test submission occurred. The verifier was previously invoked only after ACP shutdown, preventing same-session recovery. This is a bounded harness control-flow gap; the evidence does not prove the hidden verifier or the agent's implementation is wrong.

## 3. Current implementation evidence

The runner now verifies H007 after the public gates and active-work horizon complete but before the ACP client closes. It pauses the activity-stall watcher during host-side verification and keeps the disk watcher active. Host-verifier time remains charged to the hard wall clock but is excluded from the agent active-work and activity-gap measurements, matching the prior one-shot verifier's position outside the measured run. An intermediate verifier call is bounded so it cannot consume the final 300-second verifier reserve or the minimum 60-second feedback window. One same-session follow-up is allowed only for assertion/test outcomes with positive counts in `continuation_boundary` or `transcript_job_lifecycle`, and only when provider, native-call, wall-clock, and disk headroom remain.

After feedback, the unchanged verifier runs again against the final workspace and transcript. A verifier result is reused only when both the transcript digest and the feedback-follow-up count match. The safe receipt records attempt count, same-session reuse, fixed failure-domain counts, an allowlisted decision reason, bounded verifier pause time, final verifier state, and whether the final transcript was verified. It retains no test IDs, assertion text, verifier source/output, session ID, transcript, or tool payloads.

## 4. Invariants

- Keep the v97 manifest, task prompt, independent verifier, model route, paired ARM64 binaries, and verifier isolation unchanged.
- Keep the existing limits unchanged: 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, 300-second inactivity cancellation, 100 same-session follow-ups, 10-GiB runtime disk floor, and 30-GiB preflight reserve.
- Never expose verifier test names, assertion text, source, raw output, transcript, tool arguments/results, continuation body, or job IDs to the agent or public evidence.
- Pass only fixed allowlisted failure-domain names and bounded counts to one same-session feedback prompt.
- The verifier remains independent and authoritative. A successful intermediate result cannot qualify the run if the final unchanged verifier or any other gate fails.

## 5. Scope

### In scope

- Keep the ACP session available until the final verifier result is known.
- Run the unchanged verifier after public continuation completion; if it reports assertion/test failures in allowlisted H007 domains and time remains, send one same-session corrective follow-up containing only those safe domain labels.
- Preserve disk, inactivity, request, native-call, prompt-work, wall-clock, and follow-up bounds while the verifier runs and while the session resumes.
- Rebuild the transcript from the complete final tool-event sequence and rerun the same verifier after the feedback turn.
- Add bounded safe counts for verifier attempts and verifier-directed follow-ups; do not retain raw feedback inputs.

### Out of scope

- Changing the task, manifest, verifier, route, tool surface, runtime caps, test gates, or qualification threshold.
- Exposing per-test identifiers, messages, assertion operations, source, or verifier output to the model.
- Allowing repeated verifier-directed turns or an open-ended verification loop.
- Reopening daemon, scheduler, ACP projection, or provider architecture without new independently reproduced evidence.

## 6. Required production changes

Add a narrow H007 verifier-feedback decision and prompt builder in `dev/harness_lab/autonomy_run.py`. It must accept only recognized final assertion/test-failure outcomes and positive counts for the fixed safe domains `continuation_boundary` and `transcript_job_lifecycle`; unknown or malformed diagnostics produce no domain labels. Permit at most one verifier-directed follow-up per run and only when the existing wall-clock policy still preserves the final verifier reserve.

Keep the same `AcpClient`, session ID, lane, task inputs, and verifier command alive through the intermediate verifier call. Stop the 300-second tool-activity watcher during host-side verifier execution and restart it before the follow-up; retain the disk-floor watcher. Preserve a 300-second final verifier reserve and at least 60 seconds for a feedback turn. After the model responds, drain ACP events, rebuild the transcript, and run the unchanged verifier again. Final cleanup and safe receipts must reflect the final verifier result and complete event stream.

## 7. Ordered work packages

1. Add a pure safe-domain projection and feedback-prompt builder; reject unknown labels, invalid counts, setup failures, timeouts, and non-H007 outcomes. **Complete.**
2. Move H007 final verifier execution inside the same-session lifecycle and add exactly one bounded follow-up path after a recognized failed verifier result. **Complete.**
3. Ensure the final transcript, tool-call projection, event measurement, repair/review/job diagnostics, cleanup, and task outcome include any follow-up activity exactly once. **Complete.**
4. Add focused tests for safe-domain filtering, prompt privacy, verifier pass/fail sequencing, one-follow-up enforcement, final-verifier authority, and reserve/cancellation behavior. **Offline coverage complete; attempt 71 exercises the production path.**
5. Run the focused Harness Lab tests, both test-quality guards, Python compilation, and `git diff --check`; record exact results. **Complete.**
6. Recheck the unchanged v97 manifest/verifier hashes, paired ARM64 binaries, credential presence without reading its value, request budget, grant/task authorization balances, and free disk ≥30 GiB before the next live start. **Required in attempt 71 preflight.**
7. Run one new Muse attempt under the unchanged controls. Preserve its safe outcome; qualify only if the final unchanged verifier and every original H007 gate pass. **Pending.**

## 8. Failure and cancellation semantics

Verifier execution consumes the existing wall-clock budget and cannot extend it. The follow-up is refused when the remaining time cannot preserve the verifier reserve, when the verifier outcome has no recognized safe failure domain, when no provider/request headroom remains, or after one verifier-directed follow-up. Disk-floor and inactivity cancellation remain active for model work; the inactivity watcher is paused only during host-side verifier execution and restarted before another agent prompt. Unknown status, transport errors, cancellation, or verifier setup failure never become a passing result.

## 9. Compatibility

No task, verifier, manifest, protocol, storage, provider, or qualification contract changes. Existing runs remain immutable and keep their original one-shot verifier behavior. The new feedback path applies only to H007 attempts launched after this corrective commit.

## 10. Required tests

- Allowlisted domains survive prompt projection; unknown labels, raw test names, messages, IDs, and malformed counts do not.
- Only assertion/test-failure outcomes can request feedback; setup error, timeout, runner error, empty suite, and success cannot.
- A bounded state-transition regression covers verifier failure → one feedback decision → final verifier success, rejects a second feedback request, and refuses to reuse a pre-feedback verifier result. Attempt 71 exercises the full ACP session path.
- Final qualification follows the last verifier result and complete post-feedback tool events; an intermediate verifier failure never becomes a pass.
- The feedback path preserves the final verifier reserve, inactivity cancellation outside host verification, and active disk-floor watcher behavior.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_campaign_authorization
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m py_compile dev/harness_lab/autonomy_run.py dev/harness_lab/campaign_authorization.py
git diff --check
```

## 12. Documentation

Update this plan, the S019 roadmap, registry, S019 status, and safe attempt evidence. State clearly that verifier-directed feedback is one bounded same-session continuation and that test names and raw verifier material remain hidden.

## 13. Acceptance criteria

- The ACP session remains open across the verifier-feedback boundary and retains the exact same session and workspace.
- At most one safe-domain feedback turn occurs per run, within all existing limits.
- The unchanged verifier is rerun against the final workspace and transcript; only its final exit status can pass task verification.
- No hidden verifier details or private run contents enter model prompts or public evidence.
- Focused tests and quality guards pass, and the next exact-head live attempt preserves every prior qualification gate.

## 14. Stop conditions

Do not run live if the agent could receive anything beyond allowlisted domain labels, the verifier reserve cannot be preserved, the same ACP session cannot be retained, exact task/verifier/binary identity differs, the route or caps differ, disk is below reserve, or either campaign or task authorization is exhausted.

## 15. Closure evidence required

Record the implementation commit, focused test and guard results, unchanged manifest/verifier/binary hashes, and one new safe campaign diagnostic showing verifier-attempt count, whether the feedback turn was used, final task outcome, request count, active duration, event completeness, and disk minimum. Do not inspect or publish private receipt, runner output, transcript, tool payloads, verifier source, or verifier output.

## 16. Handoff

Attempt 70 is preserved as unqualified at 70/200 campaign starts, 26/40 starts in grant `s019-supplemental-20261111-04`, and 41/120 task-authorized starts. Fourteen starts remain in the grant and 79 in the task authorization. The run used 157 provider requests, 2,398.938 seconds active work, and at least 60,918,022,144 bytes free disk; external billed USD is unknown. All public gates were true, but the final verifier failed both recognized domains. Attempt 71 must follow this corrective and a fresh exact-head preflight.

Offline verification on 2026-11-14: the three focused Harness Lab modules passed 191 tests; both test-quality guards, Python compilation, and `git diff --check` passed. No Rust source or paired binary changed. This qualifies the corrective implementation only, not H007.
