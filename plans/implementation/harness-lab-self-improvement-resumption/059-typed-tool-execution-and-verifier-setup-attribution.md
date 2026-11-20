# Harness Lab Self-Improvement Resumption S059 — Typed Tool Execution and Verifier Setup Attribution

Status: **implemented**

Repository baseline: `22c1d481ec8cefe8a29b275f3ce232dbb059c185`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#54-s059--typed-tool-execution-and-verifier-setup-attribution`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`
- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md`

Primary class: invariant

## 1. Objective

Preserve the distinction between ordinary tool execution errors and unavailable infrastructure at the broker boundary, and carry the verifier's existing fixed setup-failure category into the safe H007 campaign projection. This makes failed final job submissions and verifier setup failures actionable without retaining private messages or changing the frozen H007 task, verifier, route, or limits.

## 2. Why this milestone is ready

Attempt 79 used the S058 exact-head ARM64 pair and unchanged v97 workload. It ran 3,298.972 seconds with 3,284.983 seconds of active work, admitted 427 provider requests, and measured 466/466 native tool starts and terminals with zero gaps. All 169 applicable job request/result ID correlations matched. The failed-status projection recorded one `job_not_found` and 45 ordinary `tool_execution_failed` results, confirming S058's response identity and error-class behavior.

The same safe diagnostic recorded two failed required test submissions as `execution_backend_unavailable`, no returned job IDs for those calls, and neither required final success job as submitted. Source inspection shows `TestTool` maps scheduler-submit errors to `ToolError::Execution`, while both broker terminal fallback paths map generic execution errors to `InfrastructureError`. This can make an ordinary execution failure appear to the model as an unavailable backend. The attempt's independent verifier was `setup_error`, but the verifier's already-computed bounded `setup_failure_reason` is dropped from the public campaign projection. Attempt 79's exact underlying submit and setup errors remain unknown; the source-level boundary defect and projection omission are independently reproducible.

## 3. Current implementation evidence

- `src/tool/broker.rs`: the legacy and retry-enabled terminal fallbacks preserve timeout, permission, and explicit `BackendUnavailable` types, but map `ToolError::Execution` through the generic infrastructure fallback.
- `src/agent/tool_batch.rs`: a terminal `Error` is translated back to `ToolError::Execution`; `InfrastructureError` becomes `ToolError::BackendUnavailable`.
- `src/tool/test.rs`: background-test submission and notification errors are represented as `ToolError::Execution`; accepted execution profiles can separately return a true `ToolError::BackendUnavailable`.
- `dev/harness_lab/diagnostics.py`: verifier setup output is already reduced to a fixed safe category.
- `dev/harness_lab/h007_results.py` and `dev/harness_lab/campaign_authorization.py`: the safe reconciliation and campaign allowlists currently omit that category.

## 4. Invariants that must not regress

- `ToolError::BackendUnavailable` remains `InfrastructureError` and returns to the agent as `BackendUnavailable`.
- `ToolError::Execution` becomes terminal `Error` and returns to the agent as `Execution` in both broker fallback modes.
- Timeout, permission denial, cancellation, disabled tool, missing tool, and process-denial behavior remain unchanged.
- No retry policy, idempotency decision, scheduler admission, or tool side effect changes.
- Verifier output, exception messages, paths, tracebacks, assertion text, and raw job identifiers remain absent from campaign evidence.
- The setup reason is copied only from the existing allowlisted fixed categories.
- H007 task, verifier, route, acceptance gates, request/time/native-call/disk limits, and authorization remain unchanged.

## 5. Scope

### In scope

- Correct the broker's two generic terminal fallback matches for `ToolError::Execution`.
- Add a small `ToolValue` constructor for execution-error terminal status if needed to avoid hand-built values.
- Project the bounded verifier setup-failure reason through the H007 reconciliation and campaign sanitizer.
- Add focused regressions for execution versus backend-unavailable terminal statuses and for safe setup-category projection.
- Reconcile attempt 79 evidence and S058 closure records; prepare an exact-head attempt 80 preflight if all offline gates and disk reserves pass.

### Explicitly out of scope

- Changing H007 task or hidden verifier behavior, prompt text, workload stages, or scoring.
- Changing TestTool submission behavior, scheduler, daemon, ACP, provider integration, request caps, or run budget.
- Recovering or inspecting attempt 79 private receipts, runner output, transcript, raw tool payloads, verifier output, or credential data.
- Inferring the exact historical cause of attempt 79's two test-submit errors or verifier setup error.

## 6. Required production changes

### Runtime and terminal status

The broker must preserve a generic `ToolError::Execution` as `ToolTerminalStatus::Error` in both the legacy single-attempt branch and the retry-chain terminal branch. The display may keep its existing bounded error text. An explicit `BackendUnavailable` must continue to map to infrastructure status. All other error mappings and retry decisions remain unchanged.

### Safe verifier projection

Carry `setup_failure_reason` from the existing verifier diagnostic into `verifier_public_reconciliation` only when it is one of: `build_lock_wait`, `disk_full`, `missing_path`, `python_import_failure`, `workspace_build_failure`, `setup_command_timeout`, or `unclassified_setup_failure`. Apply the same fixed allowlist at the campaign projection boundary. Do not expose exception classes or source output.

### Documentation and static guards

Record attempt 79 and S058's completed correction scope. Register S059 and update the subsystem roadmap. Preserve the campaign authorization record and all previous attempts.

## 7. Ordered work packages

### Work package A — Preserve ordinary execution terminal status

Intent: Ensure the agent receives the correct type for ordinary execution failures.

Required changes: Update both broker fallback branches and add tests that exercise `ToolError::Execution` and `ToolError::BackendUnavailable` independently. Verify tool-batch translation preserves the distinction.

Acceptance evidence: Execution errors produce terminal `Error`; explicit backend-unavailable errors produce `InfrastructureError`; timeout and permission regressions remain unchanged.

### Work package B — Preserve bounded verifier setup attribution

Intent: Make an existing safe diagnostic available in the campaign record.

Required changes: Add the fixed setup-reason allowlist to the H007 reconciliation output and campaign sanitizer. Add positive, absent, and hostile-value tests.

Acceptance evidence: A recognized category survives end to end; unrecognized text and raw verifier output do not.

### Work package C — Verify and requalify

Intent: Qualify the exact corrective pair against the unchanged task.

Required changes: Run focused Rust/Python suites, quality guards, formatting, compilation, and diff checks. Build and inspect an ARM64 pair from the exact corrective commit, verify the Rosetta host probe, and pass a fresh side-effect-free preflight before attempt 80.

Acceptance evidence: Offline checks pass; preflight binds exact source, manifest, and pair hashes; safe campaign request count reconciles with authorization accounting. H007 qualifies only when all original gates and the unchanged independent verifier pass.

## 8. Failure, cancellation, restart, and contention semantics

This plan changes result classification and safe projection only. It launches no process, performs no retry, and submits no job. Existing cancellation, idempotency, scheduler, and admission semantics remain unchanged. A live requalification uses the current 300-second inactivity cancellation, 10-GiB runtime disk floor, 30-GiB preflight reserve, 1,200 provider-request cap, 1,600 native-call cap, 3,300-second prompt-work cap, 3,600-second runner timeout, and 3,590-second external hard stop.

## 9. Compatibility and migration

No persisted schema or wire protocol changes. The additive safe diagnostic field is optional and contains only an existing fixed category. Broker terminal status becomes more precise for `ToolError::Execution`; callers already receive that error variant through `tool_batch` for structured tool errors.

## 10. Required tests

### Focused unit tests

- Legacy and retry-chain broker fallbacks map `ToolError::Execution` to terminal `Error`.
- Both fallbacks keep explicit `ToolError::BackendUnavailable` as `InfrastructureError`.
- Timeout, permission, and cancellation terminal mappings are unchanged.
- Tool-batch translation maps `Error` to `Execution` and `InfrastructureError` to `BackendUnavailable`.

### Integration and privacy tests

- An H007 reconciliation with a recognized setup category preserves that category.
- Campaign sanitization preserves allowlisted categories and drops arbitrary strings, raw output, exception data, and paths.
- Missing verifier diagnostics produce no fabricated setup reason.

### Restart, contention, and security tests

No new persistence, scheduling, process-launch, restart, or contention behavior is introduced.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tool::broker
python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_campaign_authorization
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
cargo fmt --check
python3 -m compileall -q dev/harness_lab
git diff --check
```

Run additional focused checks selected by the actual touched files. Do not claim a full verification sweep unless it completes cleanly.

## 12. Documentation updates

- Close S058 at its failed job-status identity/error-typing scope and record that H007 remains unqualified.
- Record attempt 79's safe evidence and counters in the S019 closure and subsystem roadmap.
- Register S059 and keep the registry, roadmap, and plan statuses consistent.

## 13. Acceptance criteria

- Ordinary `ToolError::Execution` failures cannot be reported as unavailable infrastructure by either broker fallback path.
- A genuine `ToolError::BackendUnavailable` remains typed as infrastructure failure.
- The safe H007 campaign diagnostic preserves only an allowlisted verifier setup reason.
- Focused tests and quality guards pass.
- Any live attempt uses the exact corrective pair, unchanged H007 workload, Muse/OpenCode Go route, and authorized limits.

## 14. Stop conditions

Stop before live admission for a changed task/verifier/route/cap, mismatched ARM64 pair, failed exact-head preflight, unresolved accounting, or less than 30 GiB free disk. Stop if preserving typed execution errors changes retry semantics or exposes unbounded private error data. Do not claim the new categories explain attempt 79 retrospectively.

## 15. Closure evidence required

Record the implementation commit, exact tests and guards, execution/backend terminal-status regressions, setup-reason allowlist tests, attempt 79 preflight/safe/monitor hashes, attempt 80 preflight/safe/monitor hashes if admitted, provider request reconciliation, task/horizon/verifier outcomes, disk minimum/ending capacity, authorization counters, privacy treatment, and unreported-USD handling.

## 16. Handoff notes

S058 committed as `22c1d481ec8cefe8a29b275f3ce232dbb059c185`. Attempt 79 used that exact ARM64 pair, admitted 427 requests in 54.98 minutes, and remained unqualified: the agent timed out after completing seven implementation stages, both repair pairs, and review, but neither required final test job completed and the independent verifier reported `setup_error`. No verifier setup reason survived the safe campaign projection. Five starts remain in grant `s019-supplemental-20261111-04`; total/task authorization counters must be read from the reconciled campaign ledger before any new preflight. Use only `opencode_go/muse-spark-1.3-contributor`; USD remains unknown if unreported.


## Attempt 80 handoff update

S059's exact ARM64 pair at `1761f8689bdaa36f29ba3b9dd2545929a1e0c9b8` passed its offline focused checks and ran attempt 80 against the unchanged v97 task and verifier. The broker error-type correction and allowlisted setup-reason projection are complete at this plan's source scope. Attempt 80 remained unqualified: the agent timed out at the prompt-work boundary, completed both final test jobs, but failed the `catalog_store` seed-before-repair ordering predicate; the independent verifier reported assertion failures in the continuation and transcript/job-lifecycle domains. No event gap, job-ID mismatch, disk stop, inactivity stop, or external wall stop occurred. S060 owns one controlled unchanged-task requalification; no production defect was established by attempt 80. Four starts remain in grant `s019-supplemental-20261111-04`; task count is 51/120 and combined count is 80/200.
