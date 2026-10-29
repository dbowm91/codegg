# H007 — Deep Rust Workload and Separate Horizon Measures

Status: ready for handoff

Repository baseline: `origin/main` at `f3c0a9e2e9903bb097d8bf56641138e03d964eb7` (H006 corrective PR #216 merged)

Source roadmap:

- `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md#h007--deep-rust-workload-and-separate-horizon-measures`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md`

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` — recovery and stall evidence remain evidence-based.
- No new architecture decision is required.

Primary class: capability

## 1. Objective

Replace H006's quickly completed event-ledger task with a materially deeper, dependent Rust engineering scenario that can observe long-session behavior without treating a fast correct solution as a task failure. Keep task correctness and long-session reliability as separate measurements.

Deliver a new offline fixture and holdout verifier with independently checked phases, a real CodeGG background job observed by exact ID while dependent implementation continues, two distinct failure/repair cycles, and a continuation boundary. Preserve H006's activity-span and maximum-gap gates for horizon qualification. Do not run a provider-backed attempt as part of this plan; any later live qualification needs new, explicit authorization.

## 2. Why this milestone is ready

- H006's job-observation and request-cap corrections were integrated by PR #216 at `f3c0a9e2e9903bb097d8bf56641138e03d964eb7`.
- Exact-head hosted CI passed for the corrective at [run 40897534593](https://github.com/dbowm91/codegg/actions/runs/40897534593), including strict Clippy, the Harness Lab suite and the workspace test sweep.
- The third H006 live attempt completed the existing task, passed all five hidden-verifier checks, repaired two failures, continued work while observing the same job ID as `running` and `completed`, and recorded zero ACP gaps. It ended after 10m43s with a 10m34s activity span. This is evidence that the current task was too small to observe a 30-minute horizon; it is not evidence of a daemon, scheduler, or ACP defect.
- All H006 live authorizations are consumed. H007 is offline fixture, verifier and measurement work only.

## 3. Current implementation evidence

- `dev/harness_lab/autonomy_run.py` enforces H006's 30-minute minimum active span, five-minute maximum observed activity gap, explicit request cap, disk floor and 60-minute maximum wall time.
- `dev/harness_lab/fixtures/agent_autonomy_h005/` and `sustained_event_ledger.case.json` describe the current four-crate ledger fixture and its verifier.
- H006 currently computes one `solved` boolean from verifier success, normal completion and duration qualification. A correct task completed early is therefore not counted as solved. Do not reinterpret or rewrite historical H006 receipts; add a versioned result shape for the new scenario.
- Native ACP job metadata is projected from bounded tool output. The existing production-path observer and exact-ID regression are proven and remain the owner of that contract.

## 4. Invariants that must not regress

- A hidden verifier passing determines task correctness; duration does not change that result.
- A correct task completed before the horizon remains a task pass. Its horizon observation is `not_observed` with a specific early-completion reason, not a task failure.
- Long-session qualification remains independently bounded by observed tool activity spanning at least 30 minutes, no observed event gap greater than five minutes, exact submitted-job ID observed running and terminal, continuation evidence, required repair cycles, and a complete independent verifier result.
- No sleep, artificial wait, repeated no-op, or inflated loop count may be used to manufacture a horizon.
- Every live provider request is behind an explicit new authorization and provider-boundary request cap. Authorization is one attempt and is never inherited from H006.
- Preserve H005 and H006 manifests, hidden verifiers, receipts, and recorded outcomes byte-for-byte.
- The CodeGG daemon remains execution authority. H007 adds no job lifecycle protocol or alternative scheduler.

## 5. Scope

### In scope

- A new immutable, offline Rust workspace fixture with a genuinely dependent multi-crate implementation path.
- Independent phase and end-to-end hidden verification, a reference solution, starter failure calibration, frozen-input checks, and sanitized receipts.
- A new versioned autonomy result projection separating task outcome from horizon observation and qualification.
- Offline proof of exact-ID background-job observation while the same agent performs dependent source work, two repairs, and continuation.
- Existing request, time, disk, cancellation and event-gap controls applied to the new case.

### Explicitly out of scope

- Changes to daemon, scheduler, ACP transport/projection architecture, job storage, or job-control protocol.
- New or alternative providers, model routing, or multi-provider comparisons. Keep the existing `opencode_go` adapter for any later separately authorized live attempt.
- Changes to H006's three historical live outcomes, score, verifier, or authorization ledger.
- Artificial duration extension, sleeps, background busy loops, or running the same test repeatedly solely to consume time.
- A live provider attempt or inferred authorization, request allowance, credential probe, or billed USD cost.

## 6. Required production changes

### Fixture and verifier

Create a distinct local **build-provenance catalog** Rust workspace. Its dependent phases should require real API and data-model changes rather than parallel toy crates:

1. **Versioned records:** define and validate build attestations, canonical package identities, schema-version normalization, checksums, and explicit handling of malformed or unsupported records.
2. **Durable import:** implement atomic, idempotent ingestion and checkpoint/recovery behavior for a journal interrupted at a record boundary; conflicting duplicate identities must be diagnosed rather than silently overwritten.
3. **Incremental dependency index:** derive forward and reverse edges from imported records; handle missing references, cycles, duplicate inputs, deterministic ordering, and updates without rebuilding unrelated state.
4. **Provenance queries:** answer a package/build lineage query across imported snapshots, report incomplete provenance, and produce stable diffs between two catalog revisions.
5. **CLI and export:** expose import, verify, lineage, diff and deterministic export commands over the preceding APIs, with stable machine-readable output and meaningful diagnostics.

The phase boundaries are a dependency chain: CLI behavior depends on query semantics; query semantics depend on the index; the index depends on durable normalized records; ingestion depends on the versioned schema. Keep the starter small enough to understand but require implementation across the chain.

Add a deterministic but bounded corpus that exercises shuffled record order, repeated imports, schema upgrades, invalid checksums, interrupted recovery, missing references, dependency cycles, incremental updates and byte-stable exports. Size the corpus from measured CPU, memory and disk use. Do not use record volume as a substitute for implementation depth. The background Cargo test should do useful integration and index work and stay active during downstream edits, but it need not run for 30 minutes by itself.

The hidden verifier must independently check each phase, both failure/repair chains, the end-to-end CLI, idempotent recovery, continued work during an exact-ID running job, a terminal observation for that same ID, continuation, event integrity and frozen fixture inputs. Demonstrate that the untouched starter fails implementation checks and a reference solution passes them.

### Result projection

Add a new schema version for H007 (and shared helpers only where compatible) that exposes separate fields:

- `task_outcome`: `passed`, `failed`, or `inconclusive`, determined by the hidden verifier and terminal integrity;
- `horizon_outcome`: `qualified`, `not_qualified`, or `not_observed`;
- `horizon_reason`: a bounded reason such as `activity_span_short`, `activity_gap_exceeded`, `job_lifecycle_missing`, `continuation_missing`, `task_completed_before_threshold`, or `measurement_incomplete`.

Task outcome and horizon outcome must not feed each other. A verifier-passing task that ends early is `task_outcome=passed`, `horizon_outcome=not_observed`, and `horizon_reason=task_completed_before_threshold`. A verifier failure remains a task failure even when substantial session time was observed; its horizon evidence is recorded independently. Do not use duration to revise task correctness or silently discard either denominator in paired comparisons.

Keep legacy H006 JSON unchanged. Version the H007 receipt and add migration-free readers/tests that distinguish old and new schemas. Do not claim multi-run model ranking from one H007 attempt.

## 7. Ordered work packages

### Work package A — Define and freeze the dependent scenario

Intent: write the phase contracts, public prompt, starter workspace, bounded corpus and holdout verifier outside the agent-writable checkout.

Acceptance evidence:

- Each phase has observable inputs, outputs and prerequisite contracts.
- The starter fails phase and integration checks; the independent reference solution passes them.
- Mutating any frozen manifest or public contract is rejected.
- The corpus and background test have measured resource bounds and no wall-clock padding.

### Work package B — Prove background overlap and recovery

Intent: use CodeGG's existing native tool and job surface; do not alter its architecture.

Acceptance evidence:

- A real test job submission yields an exact ID, the agent observes that same ID `running`, does productive downstream implementation, then observes that ID terminal before relying on its result.
- The independent verifier proves the ordering and at least two meaningful Rust failure/repair cycles.
- Continuation evidence identifies completed phases and the next dependent stage without duplicating completed mutations.
- Event capture is complete and the existing maximum-gap gate rejects a gap above five minutes.

### Work package C — Separate task and horizon outcomes

Intent: project independent outcomes for the new fixture without rewriting historical evidence.

Acceptance evidence:

- A quick reference solution produces task pass and horizon `not_observed` with the early-completion reason.
- An implementation failure with a long activity span remains a task failure while retaining separate horizon observations.
- A correct solution with a 30-minute active span and all job, continuation and event conditions meets horizon qualification.
- A gap, mismatched job ID, missing terminal state or incomplete measurement prevents horizon qualification without changing task outcome.
- Paired-result summaries report correctness and horizon qualification in separate denominators and labels.

### Work package D — Offline calibration and authorization handoff

Intent: make the fixture and receipt reviewable before any live admission.

Acceptance evidence:

- Run the frozen-input, starter-negative and reference-positive verifier calibrations.
- Record corpus cardinality, worst-case resource measurements, phase outcomes, verifier digest and result-schema version.
- Verify all budget guards are wired for H007: new explicit request cap, 60-minute hard wall-clock maximum, explicit free-disk floor of at least 10 GiB, cancellation conditions, and unknown billed USD handling.
- Exact-head hosted CI passes. Only then may a later task request a separate explicit live authorization. This plan itself does not grant it.

## 8. Failure, cancellation, restart, and contention semantics

- Fixture tests are offline and deterministic. A missing offline dependency or unavailable cargo artifact is reported as an infrastructure failure, not an agent task failure.
- The asynchronous job is owned and tracked by the existing CodeGG session. Continue with other productive phases while it runs; wait only on its exact returned ID. Mismatch, unrecoverable job-state loss, or incomplete terminal observation invalidates horizon measurement and cancels a live run.
- A continuation resumes from durable task/job evidence and must not replay completed writes. A restart or event gap makes the affected observation incomplete; do not infer a successful status.
- For any separately approved provider run, stop on user cancellation, request/time cap, free-disk floor, cap-accounting uncertainty, unrecoverable daemon/job observation loss, or identity/status mismatch. Never retry under the same authorization.

## 9. Compatibility and migration

- Keep existing H005/H006 receipts and schemas immutable.
- Introduce a distinct H007 case ID and additive receipt schema version. Readers must branch explicitly on schema version and preserve the original `solved` interpretation for historical records.
- Do not add a migration to the production session database. Fixture persistence is local to the offline case and is reset between isolated runs.
- No provider or public job protocol changes are allowed.

## 10. Required tests

### Focused unit tests

- Result projection truth table for task pass/fail/inconclusive crossed with horizon qualified/not-qualified/not-observed.
- Exact boundary tests for 30-minute active span and five-minute maximum event gap.
- Reason mapping for quick completion, long-running implementation failure, incomplete event capture and job mismatch.

### Integration tests

- Full starter-negative and reference-positive hidden-verifier runs.
- One native CodeGG run proving the exact-ID background job lifecycle and productive dependent work during its running interval.
- End-to-end CLI import, verify, lineage, diff and stable export behavior.

### Restart and recovery tests

- Idempotent repeated import and recovery from a truncated final journal record.
- Continuation from a recorded phase boundary without reapplying already completed source edits or imports.

### Security and negative tests

- Unsupported schema, corrupt checksum, duplicate-conflict, missing dependency and cycle diagnostics.
- Holdout verifier and frozen public inputs remain outside the candidate-writable root; edited frozen inputs refuse qualification.
- Request N+1 remains refused before provider transport through the existing provider-boundary test; malformed or missing cap cannot qualify a run.

## 11. Required verification commands

The implementation agent selects exact focused commands from the resulting test targets, then runs the repository's canonical checks on the plan's exact head. Expected shape:

```bash
python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_sustained
python3 -m unittest <new-fixture-verifier-calibration-tests>
cargo test --manifest-path <new-fixture>/Cargo.toml --locked
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
scripts/verify.sh scoped origin/main
scripts/verify.sh full
```

Use real target names discovered from the implementation; do not copy placeholders into closure evidence. Run hosted CI at the exact final source SHA before any live authorization request.

## 12. Documentation updates

- Add the H007 fixture, manifest, phase contracts, result schema and verifier digests to the Harness Lab implementation records.
- Update the subsystem addendum and registry with H007's actual status and exact evidence.
- Preserve H006's final disposition: corrective implementation landed successfully; sustained qualification remains unachieved.
- Record any future live authorization and result only after the authorization is explicitly received and consumed.

## 13. Acceptance criteria

- The five dependent phases form a coherent project and the hidden verifier checks every phase plus end-to-end behavior.
- Starter-negative, reference-positive, frozen-input and resource-bound evidence is recorded.
- Native job lifecycle, productive overlap, two repairs and continuation pass against the production-shaped tool surface.
- H007 reports task outcome independently from horizon outcome; fast correct completion is a task success and is not scored as a task failure for missing the observation horizon.
- The activity-span, activity-gap, request, wall-clock, disk and cancellation gates remain enforceable.
- No daemon, scheduler, ACP architecture, job protocol or provider integration is changed.
- Exact-head hosted CI passes; the closure record does not claim live sustained qualification without a separately approved qualifying run.

## 14. Stop conditions

Stop and report rather than expanding scope if:

- The design requires changes to daemon, scheduler, ACP projection or provider architecture.
- Resource limits cannot be set from measured fixture behavior.
- Task correctness and horizon outcome cannot be represented independently without changing historical H005/H006 meaning.
- The hidden verifier cannot distinguish a genuine implementation from the untouched starter.
- Offline dependencies cannot be resolved reproducibly.
- Anyone proposes a live provider run without new explicit route, request, time, disk, cancellation and cost-handling authorization.

## 15. Closure evidence required

- Exact source SHA, fixture and verifier digests, schema version and changed-file list.
- Starter-negative, reference-positive, frozen-input and per-phase verifier results.
- Corpus dimensions and bounded CPU, peak memory, elapsed time and disk growth.
- Production-shaped exact-ID running/terminal overlap, repair cycles, continuation, event-gap and cleanup evidence.
- Task/horizon result truth-table evidence, including a quick correct completion that remains a task pass.
- Focused tests, assertion/duplication guards, scoped/full verification and exact-head hosted CI.
- Explicit statement that no live authorization or live qualification is implied by offline completion.
- Unblock audit and roadmap/registry reconciliation.

## 16. Handoff notes

- Use the H006 corrective as the measurement baseline and H005/H006 fixtures as historical comparators only; do not copy H006 and merely increase loop count.
- Keep verifier inputs outside the agent-writable tree. Store only sanitized tool/job metadata, never raw provider output or credentials.
- The request ceiling for any future live run is a fresh operator decision; do not inherit H006's 120-request value. Use a hard 60-minute maximum, a free-disk floor of at least 10 GiB, and record unavailable billed USD as unknown.
- The selected route remains the existing `opencode_go` integration if and only if separately authorized. No alternate provider is a fallback.
- Report task success even when the run is too short to qualify horizon reliability. Those are independent conclusions.
