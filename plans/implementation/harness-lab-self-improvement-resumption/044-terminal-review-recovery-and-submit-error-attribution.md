# Harness Lab Self-Improvement Resumption S044 — Terminal Review Recovery and Submit Error Attribution

Status: **active; attempt 54 supplies the diagnostic baseline; attempt 55 awaits paired binaries and fresh admission**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#38-s044--terminal-review-recovery-and-submit-error-attribution`

Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/003-planning-process.md` §§6–7.

Applicable ADRs: ADR-0040, evidence-based agent recovery and termination; ADR-0051, delegated-run continuation authority.

Primary class: infrastructure

## 1. Objective

Use attempt 54's safe evidence to make terminal delegated-run recovery unambiguous to the parent agent and classify failed background `test` submissions without retaining error text. Re-run the unchanged v97 qualification case under the existing Muse authorization only after all offline checks pass and the 30-GiB disk reserve is available.

## 2. Evidence motivating the plan

Attempt 54 used `opencode_go/muse-spark-1.3-contributor` for 49.1 active minutes. It reached the existing 100-followup limit after 1,063 provider requests, with about 11 minutes remaining before the hard wall-clock stop. The task failed; the unchanged verifier passed 3/5. This was not a request-cap, native-call-cap, inactivity, or disk-floor stop.

The reviewer reached terminal `failed/deadline_exceeded` about 21.9 minutes after spawn. The parent fetched the result once after 9.1 minutes, then made 74 status/wait calls and did not use the one permitted continuation. The safe events show that the failure was surfaced. The measured gap is terminal recovery behavior, not a missing daemon/ACP notification.

S043 now proves 55/55 applicable job request/result IDs match and the exact serialized transcript projection matches its ACP source. One failed background test submission had no job ID, so the lifecycle is correctly left incomplete. The tool failure is observed, but its safe typed class is unavailable. That single event is not claimed as the cause of either hidden assertion.

## 3. Invariants

- Keep the v97 workload, independent verifier, Muse route, provider cap (1,200), native call cap (1,600), prompt-active limit (3,300 seconds), wall stop (3,600 seconds), inactivity cancellation (300 seconds), and runtime disk floor (10 GiB) unchanged.
- Keep the current 100-followup ceiling unchanged. Do not trade the task's substantive requirements for more polling or increase another budget to compensate for repeated status/wait calls.
- Never retain or emit task/job IDs, raw error text, arguments, result bodies, prompts, transcripts, verifier messages, or credentials in safe diagnostics.
- A continuation is offered only for a terminal failure class already deemed continuable, only when the parent has authority, and only within the existing one-continuation bound.
- A failed submission without a job ID remains unbound and cannot satisfy exact-ID lifecycle qualification.
- Any live attempt is counted only after provider admission; unreported external USD cost remains unknown.

## 4. Scope

### In scope

- Add a short, explicit terminal-run recovery cue to `task` status/wait/get results: terminal states do not change under more status/wait calls; retrieve the result once; if a terminal failure is a recognized transient class and the work remains wanted, use the single permitted `continue` action.
- Preserve the existing completion/push behavior, authorization checks, continuation implementation, and fail-closed handling of unknown failure classes.
- Emit an allowlisted content-free error class for failed background `test` submissions in the ACP tool update, then carry only the enum through the Harness Lab projection and safe diagnostics.
- Add bounded safe counts for status/wait calls after the first observed terminal reviewer failure, delay to the first result retrieval, whether continuation was attempted for the exact spawned reviewer run, and failed-submit error-class counts.
- Add regressions for transient vs. non-continuable terminal guidance, no repeated polling recommendation, typed submission error mapping, unknown error handling, exact-ID continuation attribution (including rejection of an unrelated run), and privacy of safe receipts.
- Rebuild the paired ARM64 binaries and run the unchanged v97 task/verifier once under the existing authorization after the reserve is restored.

### Out of scope

- Changing H007 task or verifier content, relaxing qualification, changing budgets or followup limit, extending reviewer timeout, or reclassifying attempt 54 as a success.
- Reopening scheduler, daemon, ACP event-delivery, or job-projection architecture: attempt 54 observed the terminal failure and the S043 transcript/source comparison matched.
- Inferring the exact cause of the one failed test submission from attempt 54's raw output; it remains uninspected.

## 5. Implementation notes

The current `TaskTool` description contains continuation rules, but they are embedded in a long coordination guide. The observed status/wait response also reports terminal state and failure class without a concise next-action cue. Add the cue at the response boundary so it appears exactly when the parent must choose between `get`, `continue`, and further observation. Do not silently auto-continue or suppress a parent's authorized action.

ACP receives a typed success bit and a rendered `ToolError` string. For a failed `test` submit, map only stable `ToolError` display prefixes to a closed enum; discard the source text immediately. Unknown formats map to `unclassified`. Carry the enum in a namespaced optional field and accept only the finite set in Python. Do not copy raw error text into a job projection or receipt.

## 6. Ordered work packages

1. Add an agent-facing terminal recovery cue and focused TaskTool regression tests.
2. Add an ACP classifier that applies only to failed background `test` submissions, with a closed vocabulary and no raw-text retention.
3. Carry that safe enum into the in-memory failed-submit projection and the bounded S019 diagnostic; include a content-free `unclassified` fallback.
4. Add redaction, unknown-prefix, unrelated-tool, successful-submit, and serialized-safe-receipt tests.
5. Run focused Rust and Harness Lab suites, test quality guards, formatting, and `git diff --check`; document exact command results.
6. Recheck ARM64 paired binaries, unchanged v97 manifest/verifier, credential source (without reading the secret), authorization balance, caps, budget-policy digest, and free disk ≥30 GiB. If disk is below reserve, do not build or launch; continue independent source/plan work and monitor.
7. Run one Muse attempt with unchanged per-run controls, observe safe lifecycle/disk telemetry, and reconcile the grant exactly once. Continue the already authorized S019 corrective loop only while starts remain and each corrective has evidence.

## 7. Acceptance criteria

- Terminal task output directly tells the parent that further status/wait calls do not advance a terminal run and names the safe next step.
- Only recognized continuable failures recommend `continue`; cancelled, authentication, and unclassified failures do not.
- A continuation is counted for the reviewer lifecycle only when its hashed run identity matches the run returned by that reviewer's spawn; unrelated continuation calls do not qualify.
- Failed test submissions expose only an allowlisted class; unknown messages and other tools reveal no new data.
- Safe diagnostics can attribute the failed submit by class, or report `unclassified`, without retaining text or IDs.
- Focused offline checks pass, paired ARM64 binaries match their recorded hashes, and the unchanged live verifier either passes all qualification requirements or produces actionable safe evidence.
- H007 remains marked unqualified unless the independent verifier and every task, lifecycle, and horizon gate pass.

## 8. Stop conditions

Do not run if free disk is below 30 GiB at preflight; stop the active run below the existing 10-GiB floor or on existing cancellation conditions. Stop if error classification cannot be content-free, terminal guidance would bypass delegated authority, provider/request telemetry is lost, the model route differs, or the active 120-start authorization is exhausted. Preserve all prior failed attempts and consumed authorization.

## 9. Handoff

Attempt 54 is the baseline and remains unqualified. It used `opencode_go/muse-spark-1.3-contributor`; 1,063 provider requests; 1,031 native calls; 49.1 active minutes; and 100 followups. Current grant `s019-supplemental-20261111-04` is 10/40 consumed with 30 remaining. Combined starts are 54/200. External billed USD is unknown if unreported. Disk was about 18 GiB at plan creation; after the user restored space, a fresh check observed 143 GiB before the current ARM64 test build. The latest mid-build sample is 129 GiB. A new live run still requires its own preflight at or above the 30-GiB reserve.

An attempt55 wrapper probe was invoked with `--help`; the wrapper emitted a monitor instead of help and finished without provider admission. Its safe monitor recorded zero provider requests, zero tool calls, no disk-floor trigger, and a 120-GiB minimum free-space sample. The paired-binary directory was empty at that point; the monitor did not establish a specific exit cause, model, route, or task outcome. The campaign and grant counters remained at 54 and 10/40. This was not a live qualification attempt and is preserved at `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt55-wrapper-probe-20261111.json`. The probe's runner log and any private receipt were not inspected. Stage the exact-head paired binaries and perform a fresh documented preflight before invoking the wrapper without extra arguments.

The active continuation-goal authorization reported 25/120 starts before attempt 55. Attempt 55 was admitted and is now preserved at 26/120 task starts, 55/200 durable campaign starts, and grant `s019-supplemental-20261111-04` at 11/40. Keep these counters distinct; reconcile each provider-admitted attempt in both scopes without rewriting historical rows. S045 owns the preflight identity mismatch found during this requalification attempt.
