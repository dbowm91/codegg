# Harness Lab Self-Improvement Resumption Milestone S024 — Live Repair Count Feedback

Status: **implemented; attempt 31 unqualified; S025 implemented after attempt 32; S026 active**

Repository baseline: branch `corrective/s016-raised-request-horizon-20261102`, working tree at `ac584c886f03847ff6b829f8509667637e42a052` with the recorded S019–S024 changes.

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md` §18

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None; this is a bounded agent-facing follow-up diagnostic.

Primary class: **reliability**

## 1. Objective

Expose the current observed and documented repair-cycle counts in the H007 same-session follow-up so the agent can reconcile `CONTINUATION.md` without access to private harness trace files. Re-run the unchanged task once using the existing Muse-only authorization and per-run caps.

## 2. Evidence and failure classification

Attempt 30 used `opencode_go/muse-spark-1.3-contributor` and reached the 3,300-second prompt-work cap after 54.9 minutes active. The duration, exact-ID productive overlap, all 19 submitted test-job terminal observations, reviewer lifecycle, event capture, provider telemetry, and disk floor were healthy. The independent verifier passed 3/5; its safe domains were `continuation_boundary` and `transcript_job_lifecycle`.

The causal trace contained seven correlated repair cycles, all seven were persisted, and both required named repair pairs qualified. The continuation note still documented `repair_cycles: 2`; its validator therefore reported the count mismatch and invalid document. The v85 prompt asked the agent to reconcile a private trace, while `_h007_followup_prompt` emitted the current numeric count only when the repair diagnostic was already complete. In the mismatch branch it supplied the rule but not the observed and documented values. The agent-facing feedback was insufficiently actionable; no daemon, scheduler, ACP, provider, event-loss, activity-stall, or disk defect was observed.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt30-safe-diagnostic-20261110.json` (receipt SHA-256 `becdcaef17d99b00bbec9fa56add026b2c6766131cd032ff372820d05df84ef7`).

## 3. Scope

### In scope

1. Change the live repair follow-up to state the exact count currently observed by the public native-call projection and the exact count documented in `CONTINUATION.md`.
2. When they differ or the note has no single valid count, say explicitly what value the note must use and that the two named required pairs are a minimum separate gate, not the total cycle count. Recompute this message after each later turn so the count can move as new qualifying cycles occur.
3. Add a regression with seven distinct observed repair cycles and a note documenting two; assert the follow-up reports both numbers and gives the correction without exposing job IDs.
4. Create v86 as a prompt-only derivative of v85 that reports attempt 30's safe result and tells the agent to use the live numeric reconciliation delivered by the harness.
5. Run focused offline verification, then attempt 31 under the already-authorized route, budget, cancellation rules, and start limits.

### Explicitly out of scope

- Hidden verifier source, assertions, output, or altered acceptance criteria.
- Changes to the task, Rust workload, provider route, model, paired binaries, daemon, scheduler, ACP protocol, request cap, or resource limits.
- Editing the agent's continuation note automatically or synthesizing a successful task result.
- New test corpora, artificial waiting, or broad harness redesign.

## 4. Invariants

- Use only `opencode_go/muse-spark-1.3-contributor` with the existing paired ARM64 binaries.
- Preserve the 1,200-request, 1,600-native-call, 3,600-second wall, 3,300-second prompt-work, 300-second inactivity, 10-GiB disk-floor, and 100-follow-up caps. Do not add a USD ceiling; report provider allowance when available and billed USD as `unknown` when unavailable.
- Never emit or persist raw job IDs, arguments, provider text, assertion text, transcript, or task result body in diagnostics or closure records.
- Keep task success, verifier success, and horizon qualification separate. Duration alone never qualifies.
- The model must author and read back `CONTINUATION.md`; harness feedback may report safe counts but must not mutate the artifact.

## 5. Ordered work packages

### A — Preserve attempt 30

Record its safe result, repair-count mismatch, receipt digest, and consumption of one start in the ledger, S019 status, S023 closure, roadmap, and registry.

### B — Add current-value follow-up feedback and regression

Update the incomplete-repair branch in `_h007_followup_prompt` so every follow-up reports the current observed count, documented count (or missing/malformed state), and the exact correction. State that the two named repair gates are independent of the total observed-cycle count. Test an attempt-30-shaped 7-versus-2 mismatch and assert no raw job IDs appear in the prompt.

### C — Create v86 and verify offline

Derive v86 from v85, preserving every non-prompt case field and hidden-verifier identity. Add only safe attempt-30 evidence and instructions to follow the live numeric reconciliation. Run the focused harness tests, assertion-quality and duplication guards, Python compilation, manifest equivalence check, and `git diff --check`. Do not rebuild paired Rust binaries.

### D — Attempt 31

Stage a private manifest copy and pass no-request auth, route, binary/helper hash, verifier identity, budget, lane isolation, and 30-GiB disk-reserve checks. Count the authorized start before invoking the provider. Monitor request admissions, tool/job/event observations, inactivity, disk, and cancellation thresholds. Inspect only safe receipt projections after terminal state.

## 6. Acceptance criteria

- The regression proves a 7-observed/2-documented case receives explicit numeric feedback and a direct correction instruction.
- v86 changes only prompt/group/title metadata from v85.
- Attempt 31 establishes whether numeric repair-count feedback corrects the prior continuation failure; full qualification still requires task outcome passed and all five unchanged verifier checks passing.
- All existing H007 horizon gates pass, including at least 30 minutes of substantive active work, exact-ID productive overlap, at least two qualified repair pairs, completed reviewer lifecycle, all jobs terminal, and valid continuation count reconciliation.
- All caps remain respected. Any incomplete task, failed/timed-out verifier, count mismatch, or horizon gate failure is recorded as unqualified and drives only a narrow evidence-backed successor plan.

## 7. Stop conditions and authorization

- Stop before provider invocation if route/auth availability, paired binaries, verifier identity, budget admission, lane isolation, or 30-GiB preflight reserve fails.
- Stop on user cancellation, the 10-GiB disk floor, 300-second inactivity limit, 1,200 requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, or 100 follow-ups.
- The user authorized up to 40 additional starts for this Muse-only diagnostic loop. Attempt 31 consumed start 9/40; 31 supplemental starts remain. The combined ledger is 32/120 starts consumed, with 88 remaining.

## 8. Closure evidence

Record the code/test diff, regression outcome, v86 and v85 hashes, paired binary/verifier identities, preflight and authorization counts, safe receipt digest, model route, separate task/verifier/horizon outcomes, provider allowance or `unknown`, event/job/repair/reviewer telemetry, resource minima, and the next owner if unqualified. Do not store credential material, raw logs, transcript, hidden verifier content, raw IDs, or arguments.

## 9. Attempt 31 result

Attempt 31 used v86 on the authorized Muse route and ended normally after 1,940,553 ms total and 1,933,992 ms active work. The 30-minute duration gate and all measured horizon conditions passed. The agent's continuation was valid: all seven stages and twelve gates were complete, the three observed repair cycles matched the three documented cycles, both required named repair pairs qualified, all ten test jobs reached terminal status, exact-ID productive overlap was observed, and the reviewer completed.

The independent verifier passed 2/5 checks; task outcome was `failed`, so H007 did not qualify. Safe failure domains were `continuation_boundary: 1`, `task_behavior: 1`, and `transcript_job_lifecycle: 1`. Public continuation and transcript/job projections were complete, leaving two verifier/public-projection disagreements and one unmapped task-behavior failure. This does not establish whether the verifier's additional condition is missing from public evidence or whether the task implementation is defective. S025 adds aggregate domain diagnostics before the next unchanged-v86 run.

The complete 1,447-event trace paired 679/679 tool calls with no gaps or drops and persisted three correlated repair events. The ten test jobs ended as seven completed and three failed, all observed terminally. Provider request count was 639/1,200, with 639 attempts started, 635 succeeded and four failed before retry/recovery. Provider-reported allowance was USD 0.006132; billed USD remains unknown. Lowest free disk was 165,652,250,624 bytes; the 10-GiB floor did not trigger. Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt31-safe-diagnostic-20261110.json`; private receipt SHA-256 `e16a0174a5afd2f5ef0cf32d7034ed8fc2f4f645e7e1063f04a5bf127f28b4ff`.
