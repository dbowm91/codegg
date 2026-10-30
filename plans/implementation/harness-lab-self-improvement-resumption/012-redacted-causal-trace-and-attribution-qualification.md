# Harness Lab Self-Improvement S012 — Causal Trace Sufficiency and Failure Attribution

Status: **ready (offline)**. Primary class: **infrastructure**. Hard dependency: S001 evidence-limited attribution closure and H007 observation contract. Interface dependencies: S010 integrated run IDs and S011 dossier input; coordinate field names before final integration. Not a prerequisite to repeat H007 live runs.

Authority: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`. Existing S001–S009 closure records are historical evidence, not re-opened milestones.

Baseline reviewed: `main@2b3785bfd743325f08149e9b6eb41828e823e4df` (2026-10-30), containing PR #219 and S001–S009 closure artifacts. At implementation, re-read `origin/main`, its CI, existing user changes and current `dev/harness_lab` interfaces; use the actual resulting source SHA in every receipt. Do not assume this planning baseline remains HEAD.

Ownership: development-only Python `dev/harness_lab` controls evaluation, qualification, admission and receipts. Native CodeGG daemon/broker remains authority for tools, permissions, jobs and ACP; `opencode_go` is the sole CodeGG provider adapter in scope; OpenCode CLI is an external proposer and never the evaluator.

## 1. Objective

The seven H007 sanitized summaries contained paired start/terminal counts but insufficient causal detail to determine why an agent stopped or omitted a second repair cycle. S001 inventoried private sidecars but could not rely on full redacted event trajectories. Add a **bounded, privacy-preserving, per-run causal trace** to the Harness Lab controller that distinguishes observed provider transport failure, execution failure, tool/job failure, verifier setup failure, task failure, missing observation and possible CodeGG product defect **without inferring intent or manufacturing a root cause**.

This is a measurement improvement, not a new user-facing ACP protocol or replay of old live evidence. Keep endurance and correctness as separate outcomes as in H007; do not turn wallclock duration into correctness failure.

## 2. Expected implementation boundary

Review `dev/harness_lab/supervisor.py`, `autonomy_run.py`, `pilot.py`, `sustained.py`, `ledger.py`, `h007_results.py`, `h007_attribution.py` and existing native ACP event projections. Prefer a **single versioned causal-trace projection** at the Python observation boundary and reuse existing event stream/counters; do not create a second event subscriber with independent lifecycle semantics. No Rust product change unless a concrete unobservable native event is independently demonstrated and assigned to its owning component.

An entry should minimally carry: run/case/trial identity, source+binary+daemon generation, monotonic order, relative elapsed timestamp, event type, tool-call correlation key, sanitized tool name, typed start/terminal/result (including unknown), background-job identity correlation and observed status, bounded duration, provider request admission/timeout class, explicit cancellation cause, continuation/checkpoint markers, repair-cycle evidence markers, verifier launched/completed/error and activity-gap measurements. Preserve exact internal job correlation while publishing only stable bounded pseudonymous IDs if raw identifiers carry sensitive information. Never retain raw tool arguments, full prompts, filesystem contents, stdout/stderr, credentials or unbounded provider payload. Capture **unknown** instead of fabricated zero or failure.

Use streaming append-only bounded JSONL with a version, schema/digest, per-run size/event/field caps, scrub rules and integrity indicators (missing sequence, orphan terminal, dropped capture, truncation); tie it to the existing immutable ledger and original start/terminal counts. Collector errors must result in `measurement_incomplete`, not a task failure or accepted improvement. Preserve opt-in private diagnostic sidecars separately with explicit retention/cleanup ownership.

## 3. Ordered work packages

A. **Trace contract and privacy threat model.** Specify event fields, allowed values, typed failure taxonomy, truncation, counter conservation, pseudonymous job-ID binding, retention and backward compatibility. Define what can be diagnosed from ACP alone and what remains unobservable; do not infer model thought/intent.

B. **Instrument native observation once.** Project the existing ACP stream into the bounded causal trace, correlating `tool_call` and `tool_call_update` by `toolCallId` (not terminal title) and preserving output-derived job status for exact submitted IDs. Reuse existing provider-boundary request receipts. Record when an activity gap exceeds the configured cap versus a normal short successful turn.

C. **Provide deterministic offline causal report.** Add a pure read-only `report/attribution` command that accepts frozen trace+receipt identities and returns `observed_failure_class`, `evidence_sufficiency`, `plausible_owners`, `unknowns` and `required_reproduction`. It may suggest an owner only conditionally; an S011-eligible CodeGG defect still requires independent failing-first reproduction. A model that failed to do a second repair is not automatically a scheduler defect.

D. **Prove fault discrimination and privacy.** Offline native loopback scenarios: complete success, two repair cycles, one repair only, job submitted but never observed, job failed, orphan tool terminal, provider timeout, ACP drop, truncated stream, verifier setup error, late tool updates and explicit user cancellation. Verify no raw tool arguments/prompts/credentials appear, bounded on-disk size, zero versus unavailable counters, and event identity conservation.

E. **Backwards compatibility and documentation.** Keep H005–H007 original receipts byte-identical. A trace-less historical run is `causal_trace_unavailable`; never backfill invented events. Document when private sidecars are needed and why their absence limits attribution.

## 4. Failure/restart/contention/compatibility

On cancellation or controller crash, flush available trace atomically or leave a typed truncated terminal with immutable previous rows; do not silently rewrite successful records. Repeated resume must not duplicate event or job identity and must bind source, suite, subject generation and authorization fingerprint. A trace cap hit halts capture safely and declares measurement incomplete, not a zero-gap success. Keep local file ownership/cleanup and cross-platform fallback explicit. No new daemon storage, ACP method, subscription integration or live calls.

## 5. Verification and acceptance

Add focused `test_causal_trace.py` or equivalent executable tests plus native tool/job fixture coverage. Must demonstrate the causal report refuses to classify H007's incomplete summaries as a reproduced CodeGG defect and preserves `provider_timeout`, `verifier_setup_error`, `task_failure`, and `unknown` separately. Run cap/leak tests, interruption/restart, event gap and ordinal property tests with at least one complete native ACP trace.

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# Run scripts/verify.sh full and exact-source-head hosted CI when production/integration code changes.
```

Close S012 when fresh offline native runs produce bounded causally useful traces and an honest deterministic report, while legacy receipts remain unchanged and incomplete observations never qualify a candidate. This does **not** assert live root-cause detection, statistical event-loss rate, or H007 sustained success. Stop on redaction leaks, duplicate independent stream ownership, unbounded growth or inaccurate attribution. Closure: `plans/closure/harness-lab-self-improvement-resumption/012-status.md`, with schema, evidence matrix, leak/cap tests, exact-head CI, and remaining unknowns.
