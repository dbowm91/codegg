# Eggplan Assessment Integration M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggplan-assessment-integration/001-durable-execution-subject-provenance.md`

Source subsystem roadmap:

- `plans/subsystems/eggplan-assessment-integration-roadmap.md#m001--durable-execution-subject-provenance`

Repository baseline reviewed: CodeGG `f5f8d96d7b7371c8583196c58d36ef7b3118ed3c` (current-head reconciliation); final implementation `4fab486f`.

Implementation commits:

- `a7cf63c4` — durable execution-subject capture, attempt persistence, scheduler and evidence resolver integration, and snapshot qualification.
- `4fab486f` — updated live execution test context for the new subject store.
- `418fdc85` — classify failed remote S2 capture as unavailable, preserving fail-closed validation.

External handoff rechecked: Eggplan `main` at `85c4c7ef5dc826dd0be7cf65d71843c3264d842b`; pure live assessment bridge `088968bd58680ae2b3741e2f1feb0614e0ff81a0`.

## 1. Executive finding

M001 is closed. CodeGG captures attempt-scoped Git identity before execution, persists it before external work, and seals local execution after cleanup or Eggwork at immutable snapshot construction. The Eggwork seal records the canonical manifest digest and omission counts; any omitted non-regular or oversize source entry leaves the subject unavailable for exact-subject evidence. Historical resolution uses persisted attempt provenance and never reconstructs it from the current worktree.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Versioned native subject and bounded Git capture | `ExecutionSubjectRevision`; `egggit::capture_git_source_subject`; `subject::tests` | pass | Git HEAD plus deterministic staged, unstaged, untracked, deletion, symlink, and bounded nested-submodule identity. |
| Attempt authority and durable restart-compatible storage | `JobAttempt.source_subject`; SQLite v67 nullable `source_subject_json`; in-memory and SQLite roundtrip tests | pass | Legacy rows remain NULL; no fabricated backfill. |
| Local drift and truthful job terminal state | Scheduler S1/S2 capture and terminal seal | pass | Drift blocks exact-subject qualification without rewriting execution outcome. |
| Remote immutable-input binding | `run_remote` seals after `build_snapshot`; `WorkspaceManifest::digest`; bounded skip counts | pass | Stable requires equal S1/S2 and complete snapshot. Omitted entries produce `Unavailable(MaterializationIncomplete)`. |
| RunStore and AgentRun correlation | scheduler-owned manifest projection; exact job+attempt resolution | pass | Missing/dangling link stays unavailable. |
| Enriched WorkPlan resolver | `assemble_resolved` and resolver tests | pass | Status and source subject remain distinct facts; no live-worktree fallback. |
| Cross-repo bridge compatibility | Eggplan bridge/head recheck above | pass | M001 does not add an Eggplan production dependency or replace the assessor. |

## 3. Production implementation evidence

`ExecutionSubjectRevision` schema v1 contains subject kind, durable namespaced repository identity, exact revision, clean/dirty state, and optional dirty digest. `ExecutionSubjectProvenance` records start/seal disposition and seal kind. Materialized Eggwork provenance adds the canonical manifest digest, completeness bit, and bounded skipped-entry counts. Storage layout advanced from v66 to v67 with an additive nullable attempt column.

| Schema | Fields | Meaning |
|---|---|---|
| `ExecutionSubjectRevision` v1 | `schema_version`, `subject_kind`, `repository_identity`, `revision`, `state`, `dirty_digest` | Exact Git OID and clean/dirty workspace state; dirty state is represented by a canonical SHA-256 digest. |
| `ExecutionSubjectProvenance` v1 | `captured`, `sealed`, `disposition`, `seal_kind`, `unavailable_reason`, optional `materialization` | Attempt-scoped start and boundary seal with explicit Stable/Drifted/Unavailable classification. |
| `materialization` | `manifest_digest`, `complete`, `skipped_non_regular`, `skipped_oversize` | Canonical Eggwork input integrity and transfer completeness; counts are bounded integers. |

| Case | Persisted disposition | Exact-subject eligibility |
|---|---|---|
| Git clean, unchanged | Stable; equal revisions; no dirty digest | eligible |
| Git dirty, unchanged | Stable; equal revisions and dirty digest | eligible |
| Git changed during local execution or snapshot assembly | Drifted | unavailable |
| Eggwork omitted source entries | Unavailable / `MaterializationIncomplete`; digest and omission counts retained | unavailable |
| Legacy/missing start provenance | NULL or Unavailable | unavailable; never backfilled |
| S2 capture fails | Unavailable / `CaptureFailed` | unavailable |

The scheduler owns capture for the canonical workspace. Local executors seal after process/output cleanup. Eggwork seals S1/S2 around immutable snapshot construction before upload and submit. RunStore projection and enriched WorkPlan resolution consume the persisted attempt record. Legacy attempts are left unavailable.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk ./scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk cargo check -p codegg-core --lib
rtk cargo check -p codegg --lib --locked
rtk cargo test -p egggit --lib subject::tests
rtk cargo test -p codegg-core --lib source_subject_tests -- --test-threads=1
rtk cargo test -p codegg-core --lib jobs -- --test-threads=1
rtk cargo test -p codegg-core --lib run_store -- --test-threads=1
rtk cargo test -p codegg-core --lib work_plan -- --test-threads=1
rtk cargo test -p codegg-core -- migration --test-threads=1
rtk cargo test -p codegg-core --test work_plan_foundation
rtk cargo test -p codegg-core --test work_plan_projection_arbiter
rtk cargo test --test work_plan_projection_arbiter
rtk cargo test --test long_horizon_trajectory_qualification
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_execution_ownership.py
rtk git diff --check
```

### Results

All listed local commands passed. Counts: subject capture 3; attempt provenance persistence 2; jobs 28; RunStore 18; WorkPlan 58; migration 7; WorkPlan foundation 10; core projection arbiter 7; root projection arbiter 9; Eggwork remote execution 25; long-horizon qualification 27. Quick verification passed all formatting, static guards, and workspace all-target checks. Clippy reported no issues after the final capture-failure fix. The incomplete-materialization invariant test passed with the two subject-schema tests. Hosted `CI` workflow run [`36106606574`](https://github.com/dbowm91/codegg/actions/runs/36106606574) passed on exact pushed commit `418fdc85656e7e1faa57f71e5e7f10f7f4859c60`, including Workspace Clippy and Workspace tests.

## 5. Invariant review

- Provenance authority is JobAttempt, not JobRecord: pass; retry attempts capture independently.
- Historical lookup never captures current Git state: pass; resolver reads stored attempt and exact AgentRun linkage only.
- Stable local evidence requires S1 == S2: pass; drift is recorded as drifted.
- Stable remote evidence requires S1 == S2 and complete immutable snapshot: pass; manifest digest and omission counts persist; incomplete materialization is unavailable.
- Existing execution status and CodeGG ownership remain authoritative: pass; no assessor or scheduling ownership swap.

## 6. Failure and recovery review

Capture/store failure before executor side effects prevents the launch. Missing executor seal fails closed for exact-subject evidence. A crash after S1 leaves Started provenance, not Stable; restart reads the attempt row and never promotes Started. SQL writes are attempt-scoped and reject conflicting starts or terminal reseals. Retries get independent capture. Concurrent seal attempts are rejected by the persisted-start comparison and terminal immutability guard.

A failed or drifted provenance seal does not fabricate success or rewrite the execution terminal status. Existing scheduler cancellation, generation/lease validation, resource permits, and Eggwork upload/submit ordering remain authoritative. Provenance capture does not add an event or artifact stream. If the later RunStore projection fails, the attempt remains authoritative and the execution outcome is unchanged. Canonical DTO validation rejects malformed values; capture is rooted at the lease workspace and uses bounded Git process/output, path, byte, and recursion limits. S1/S2 is bounded revalidation, not a global transaction against arbitrary external writers; remote manifest digest binds the copied input bytes.

## 7. Migration and compatibility review

Storage layout v67 adds nullable `job_attempt.source_subject_json`; existing rows remain NULL. JobAttempt JSON is versioned. RunManifest adds an optional defaulted source subject; RunDraft and RunCompletion remain source-compatible. No old manifest rewrite or legacy backfill occurs. The Eggwork manifest digest is independent of Git subject identity and is retained as materialized-input integrity metadata.

## 8. Security review

Persisted values contain bounded schema/disposition, OID, digest, namespaced stable ID, manifest digest, and bounded skip counts. File contents, paths, Git status text, remotes, credentials, and environment data are not persisted. Git capture uses the governed bounded `egggit` process seam and canonical workspace root. Snapshot construction retains existing traversal and size bounds; incomplete transfers are not exact proof.

## 9. Documentation and operations

Updated `architecture/git.md`, `jobs.md`, `run_store.md`, `storage.md`, and `work_plan.md`, plus the integration roadmap and registry. Execution ownership and core-boundary guards passed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None. | No unresolved finding prevents closure. | — |

## 11. Roadmap disposition

M001 is closed. CodeGG M002 and Eggplan CodeGG Integration M002 are unblocked to `ready`: the audited hard dependency was this upstream handoff, while verification-digest derivation and differential adoption remain work owned by M002. M003 repository Plan binding remains downstream of M002.

## 12. Registry updates

The CodeGG registry and roadmap mark M001 closed and M002 ready. Eggplan's CodeGG Integration M002 is moved from blocked to ready with the accepted CodeGG closure SHA and current Eggplan bridge/head recheck. Other CodeGG M003 work remains gated on M002; no unrelated downstream plan was unblocked.
