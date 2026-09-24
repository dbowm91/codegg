# Eggplan Assessment Integration M001 — Closure Status

Status: corrective pass required

Source implementation plan:

- `plans/implementation/eggplan-assessment-integration/001-durable-execution-subject-provenance.md`

Source subsystem roadmap:

- `plans/subsystems/eggplan-assessment-integration-roadmap.md`

Repository baseline reviewed: `f351d014`

Implementation commits:

- `edcfd0d` — add local attempt-scoped execution subject capture, persistence, sealing, and enriched resolver

## 1. Executive finding

The local scheduler path now captures a CodeGG-native subject from the
canonical leased root, persists it on JobAttempt, detects start/end drift, and
resolves historical attempt provenance without consulting the current
worktree. The M001 capability is incomplete: Eggwork materialization sealing,
RunManifest propagation, exact AgentRun resolution, and full bounded
submodule/index identity qualification remain absent. This is a corrective
pass, not a positive M001 close; Eggplan M002 remains blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Versioned native subject DTO and validation | `crates/codegg-core/src/execution_subject.rs`; unit test | partial | DTO and stable/drifted seal exist. |
| Governed bounded Git capture | `crates/egggit/src/execution_subject.rs`; focused capture test | partial | Canonical root, HEAD, status, index/worktree raw blob identities, symlinks, deletions, and content digest. Recursive submodule capture and all unsafe/bound cases lack qualification. |
| Attempt persistence and legacy semantics | v67 nullable column; JobStore implementations; migration suite | partial | Additive NULL migration and CAS-like start/seal operations exist; explicit store round-trip/restart/conflict/retry tests are missing. |
| Local scheduler S1/S2 and drift | `src/scheduler/scheduler.rs`; core subject tests | partial | Captures before dispatch and after executor returns. No scheduler-level mutation-during-execution acceptance test. |
| Remote/materialized seal | No implementation evidence | fail | Current scheduler seals after executor completion, which is not the accepted-input materialization boundary. |
| RunStore propagation | Architecture documents record absence | fail | RunManifest is not self-describing with subject provenance. |
| AgentRun exact attempt resolution | No implementation evidence | fail | Enriched resolver currently resolves job refs only. |
| Enriched WorkPlan status/subject separation | `src/work_plan_evidence.rs::assemble_resolved_evidence` | partial | Job refs resolve the latest durable attempt; AgentRun exact link support is absent. |
| Cross-repository Eggplan golden handoff | No current upstream head recheck / golden fixtures | fail | Must be performed before M002 can unblock. |
| No historical current-worktree fallback | Resolver code inspection; execution ownership guard | pass | Resolver only queries durable JobStore attempts. |

## 3. Production implementation evidence

The implementation adds `ExecutionSubjectRevision`, provenance/disposition
types, and attempt-level `source_subject_json` under migration v67. In-memory
and SQLite stores expose intent-named start/seal operations. `egggit` captures
HEAD and a deterministic digest at an explicit root through the governed Git
process seam. The scheduler performs local S1/S2 capture. The enriched
WorkPlan resolver returns status separately from stable or unavailable
subject metadata.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test -p egggit execution_subject::tests::clean_and_dirty_capture_is_deterministic
rtk cargo test -p codegg-core --lib -- jobs
rtk cargo test -p codegg-core --lib -- run_store
rtk cargo test -p codegg-core --lib -- work_plan
rtk cargo test -p codegg-core -- migration
rtk cargo test -p codegg-core --test work_plan_foundation
rtk cargo test -p codegg-core --test work_plan_projection_arbiter
rtk cargo test --test work_plan_projection_arbiter
rtk cargo test --test long_horizon_trajectory_qualification
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_execution_ownership.py
rtk ./scripts/verify.sh quick
rtk git diff --check
```

### Results

All listed local commands passed. Counts: egggit capture 1; core jobs 23;
RunStore 18; WorkPlan 58; migration 11; WorkPlan foundation 10; core
projection arbiter 7; root projection arbiter 9; trajectory qualification
27. Quick verification passed. Workspace Clippy reported no issues. Hosted
CI has not yet been observed for this pushed change and is therefore not
claimed here.

## 5. Invariant review

JobAttempt, not JobRecord labels, is the subject authority. Legacy rows remain
NULL. Resolver code does not use current Git state. Capture is rooted at the
scheduler lease and uses `egggit`'s governed Git seam. Local S1/S2 mismatch
classifies as Drifted. Remote and RunStore invariants cannot yet be claimed.

## 6. Failure and recovery review

Capture failure can be represented as unavailable and does not prevent
ordinary execution. A started-only subject is not Stable. Conflicting initial
writes and reseals are rejected. Explicit process restart round-trip evidence
is missing. Remote cancellation/restart is not qualified by this change.

## 7. Migration and compatibility review

Migration v67 adds nullable `job_attempt.source_subject_json`; prior attempts
remain NULL and are not backfilled. `STORAGE_LAYOUT_VERSION` is 67. RunManifest
schema is unchanged. A dedicated old-database fixture asserting the v67
column/NULL semantics remains part of the corrective work.

## 8. Security review

Persisted records contain only bounded namespace/revision/state/digest and
disposition metadata. Capture hashes changed regular-file bytes and does not
persist those bytes or Git status text. Non-Unicode paths fail capture. Full
submodule recursion and bound-exhaustion tests remain outstanding.

## 9. Documentation and operations

Updated `architecture/jobs.md`, `architecture/run_store.md`,
`architecture/work_plan.md`, and `architecture/git.md`. The docs explicitly
state that remote sealing and RunManifest projection are not qualified.
Core-boundary and execution-ownership guards passed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Remote subject is sealed after executor completion rather than after immutable input materialization | Historical remote source subject can be wrong | Complete M004 after Eggwork C001 establishes the accepted-input seam. |
| high | RunManifest and AgentRun do not resolve the authoritative attempt provenance | Evidence paths lack self-describing/exact lineage | Implement and test exact correlation in M004. |
| high | Full submodule/index/bounds semantics and restart/CAS tests are incomplete | Dirty subject digest compatibility and persistence guarantees are not fully established | Complete M004 fixtures and persistence tests. |
| medium | Current Eggplan head/bridge and reviewed conversion fixtures were not rechecked | Downstream compatibility cannot be asserted | Recheck before M004 closure. |
| low | Hosted CI result is unavailable at this record's creation | Local checks only | Add run ID/result as a factual closure-record correction after CI completes. |

## 11. Roadmap disposition

Corrective implementation plan required. M001 does not positively close. M002
remains blocked. M004 is registered as blocked on Eggwork C001's stable
accepted-input contract and current Eggplan bridge/head revalidation.

## 12. Registry updates

- M001 implementation plan moved from active to implemented; closure status is
  corrective pass required.
- Roadmap records M001 corrective pass required and M002 blocked.
- Registered M004 as blocked; the dependency audit found no plan newly ready.
- Eggplan M002 remains blocked because the hard M001 dependency is not
  positively closed; no downstream plan was unblocked.
