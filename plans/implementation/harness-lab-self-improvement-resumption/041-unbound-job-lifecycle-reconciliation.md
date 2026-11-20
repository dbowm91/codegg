# Harness Lab Self-Improvement Resumption S041 — Unbound Job-Lifecycle Reconciliation

Status: **closed at public-reconciliation scope; attempt 51 unqualified; S042 active**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#35-s041--unbound-job-lifecycle-reconciliation`

Long-term requirements: `plans/000-long-term-specification.md` §§26–29; `plans/002-long-term-roadmap.md`

Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: infrastructure

## 1. Objective

Make the safe transcript-job projection report incomplete when native job actions are unbound to an exact observed job ID, and expose each safe predicate that contributed to a verifier/public-projection disagreement.

## 2. Why this is ready

Attempt 50 completed 45.98 minutes of active work and passed its public phase, gate, artifact, exact-ID overlap, reviewer, and three repair-cycle checks. The independent verifier still failed one continuation-boundary and one transcript-job-lifecycle assertion. A bounded diagnostic counted three unbound job-tool calls, which the public predicate ignored. Attempt 51 completed 50.48 minutes of active work, all seven stages and twelve gates, twenty-two terminal test jobs, two repair pairs, and the reviewer lifecycle. The unchanged verifier again failed one assertion in each domain. The new predicate correctly classified transcript lifecycle as publicly incomplete because one failed `job.status` call had no bound ID. The continuation domain still disagreed with the public projection; neither observation proves the hidden assertions' cause.

## 3. Current implementation evidence

`dev/harness_lab/h007_results.py::_verifier_public_reconciliation` now receives the bounded job-sequence projection, exposes each public check, and refuses to call transcript lifecycle complete when any job action is unbound. Attempt 51 also confirmed the remaining limit of that data: the transcript projection identifies the unbound event as one failed `job.status` call but cannot distinguish an omitted input ID from missing ACP job metadata. The continuation projection confirms its current values and order checks but did not record where footer fields sat relative to completed progress sections.

## 4. Invariants

- Never retain or display hidden-verifier output, source, test names, raw job IDs, tool arguments, or tool outputs.
- A transcript lifecycle is not publicly complete while job actions are unbound from an exact ID.
- Keep independent verifier outcome separate from public projection state.
- Preserve the existing task, verifier, model route, qualification gates, and run caps.

## 5. Scope

### In scope

- Add per-predicate booleans to the safe verifier/public reconciliation.
- Include `unbound_job_tool_call_count == 0` in transcript-lifecycle completeness.
- Pass the bounded job-sequence diagnostic from the runner into task-completion summarization.
- Add a prompt-only v97 derivative directing the agent to use only exact returned IDs and to leave the final continuation pointers at `none` before ending.

### Explicitly out of scope

- Inspecting hidden verifier text or source, changing verifier assertions, or expanding the Rust workload.
- Changes to ACP, daemon, scheduler, provider, reviewer, or native-job protocol architecture.

## 6. Required changes

The public reconciliation returns domain states plus named Boolean checks. Unbound job calls make `transcript_job_lifecycle` incomplete with a specific safe reason. If every named check is true while the verifier still reports a failure, the result remains explicitly classified as a verifier/public evidence mismatch.

## 7. Ordered work packages

### Work package A — Predicate-level safe reconciliation

Acceptance evidence: synthetic diagnostics with all checks true project complete; an unbound job-tool call changes transcript lifecycle to incomplete and identifies only its bounded count.

### Work package B — v97 exact-ID instruction

Acceptance evidence: only the prompt derivative changes; it prohibits placeholder/alias IDs for job status/output/cancel, preserves all task and acceptance text, and is admitted only after S040 ledger repair.

## 8. Failure and retry semantics

Missing diagnostics produce `unavailable`, not `complete`. Unbound calls remain visible as bounded counts but do not expose their arguments or attempted identifiers. A verifier failure is never rewritten as task success.

## 9. Compatibility

Additive receipt fields only. Existing readers may ignore named checks; existing run limits and verifier commands remain unchanged.

## 10. Required tests

- Unit tests for complete, incomplete, unavailable, and unbound-call reconciliation.
- Fixture derivative tests proving only allowed prompt fields changed.
- Focused H007/verifier-diagnostic suites and quality guards.

## 11. Required verification

```bash
rtk proxy python3 -m unittest dev.harness_lab.tests.test_verifier_diagnostics -v
rtk proxy python3 -m unittest dev.harness_lab.tests.test_h007 -v
rtk proxy python3 scripts/check_test_assertion_quality.py
rtk proxy python3 scripts/check_test_duplication.py
rtk proxy git diff --check
```

## 12. Documentation

Update S019 evidence/closure notes and the subsystem roadmap with the exact scope of the new predicate. Do not describe the unbound-call count as the proven cause of the hidden assertions.

## 13. Acceptance criteria

The attempt-51 public lifecycle state is incomplete in the presence of its unbound call, and safe reconciliation names the failing predicate without leaking IDs or raw verifier content. Attempt 51 remains unqualified: the unchanged verifier passed 3/5 checks and the continuation-boundary domain is still a public-projection disagreement. S041 is closed at its stated reconciliation scope; S042 adds the missing input-shape and continuation-line-layout diagnostics before the next live run.

## 14. Stop conditions

Stop and record a remaining verifier/public mismatch when all safe predicates pass but the independent verifier still fails. Do not inspect its hidden source or output to force a diagnosis.

## 15. Closure evidence

Include test results, fixture manifest hash, named predicate values, safe verifier domain counts, privacy flags, and the next live attempt number and remaining authorization.

## 16. Handoff notes

S040 and S041 are closed at their bounded corrective scopes. S042 is active. Use only `opencode_go/muse-spark-1.3-contributor`; keep the 1200-request / 3600-second / 10-GiB-floor bounds and cancellation behavior unchanged. The latest supplemental grant has 8 of 40 starts consumed; 32 remain.
