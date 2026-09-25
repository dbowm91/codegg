# Eggplan Assessment Integration M001 — Durable Execution-Subject Provenance

Status: closing

Repository baselines:

- original planning baseline:
  `f4e6e69d9e968e2adbb4228b3a7d45f55bd1294c`
- current handoff baseline after Eggwork M002/M002a:
  `f5f8d96d7b7371c8583196c58d36ef7b3118ed3c`
- current storage layout: v66; M001 owns the next additive migration, v67
- current pinned Eggwork production revision:
  `6cc813418c3f14740a635fef79208e85219175bb`

External blocker/evidence reviewed:

- Eggplan blocker/registry head:
  `a4edb31b90f5f8a8a47e244110aa69747a0ac4bc`
- Eggplan pure live assessment bridge:
  `088968bd58680ae2b3741e2f1feb0614e0ff81a0`
- Eggplan downstream plan:
  `plans/implementation/codegg-integration/002-staged-eggplan-assessment-adoption.md`

Source roadmap:

- `plans/subsystems/eggplan-assessment-integration-roadmap.md`

Related CodeGG roadmaps/contracts:

- `plans/subsystems/long-horizon-work-execution-roadmap.md`
- `architecture/work_plan.md`
- `architecture/jobs.md`
- `architecture/run_store.md`
- `architecture/git.md`

Primary class: infrastructure / provenance invariant / cross-repository unblock

## 0. Current-head planning reconciliation

M001 has not been implemented or closed, so CodeGG planning governance does
not permit inventing a post-closure corrective lineage. The handoff is instead
revised in place against current repository evidence.

The current-head review found three material deviations from the original
handoff that are authoritative for implementation:

1. **Storage version** — CodeGG remains at `STORAGE_LAYOUT_VERSION = 66`;
   the provenance column/envelope therefore uses the next sequential
   migration, v67. Do not reuse or renumber v66.
2. **Eggwork seal point** — current `EggworkExecutor::run_remote` builds a
   bounded immutable in-memory `WorkspaceSnapshot` first, then uploads its
   copied bytes, creates the remote workspace, and finally submits execution.
   The exact-source stability window for remote evidence is therefore around
   `build_snapshot(&ctx.workspace_root)`, not around the later
   `create_workspace`/remote-submit boundary.
3. **Materialization completeness** — current `build_snapshot` can omit
   symlinks/non-regular entries and oversize files. A remote execution over an
   incomplete projection MUST NOT be labeled stable exact-subject evidence
   merely because Git S1 == S2. Exact-subject eligibility requires a complete
   materialization under the active transfer contract.

The current Eggwork manifest has a canonical `WorkspaceManifest::digest()`.
Persist that digest as remote-input provenance when exact-subject eligibility
is evaluated. It is integrity for the transferred input, not a replacement for
the Git SubjectRevision.

The original architectural intent remains unchanged: CodeGG owns capture and
attempt persistence; Eggplan later consumes the resulting subject as pure
assessment input.

## 1. Objective

Persist the exact source state observed by CodeGG execution so a historical
completed job/run can later be used as exact-subject evidence without
consulting or trusting the current worktree.

The current blocker is precise:

- `WorkPlanEvidenceSnapshot` stores only host status;
- `JobRecord` has no execution subject;
- `JobAttempt` has no execution subject;
- `RunManifest` has no source subject;
- a completed job may be hours or days older than the current worktree;
- using the current worktree subject for that historical completion would
  manufacture provenance.

M001 adds CodeGG-owned execution-subject capture and durable provenance. It
does not adopt Eggplan assessment yet.

## 2. Why provenance belongs to JobAttempt

Do not add the authoritative subject to `JobRecord`.

One logical job may have multiple attempts, and retries may execute after the
workspace changed. The historical source subject is therefore attempt-scoped.

Required authority chain:

    canonical workspace/worktree lease
      -> capture source subject at execution input boundary
      -> persist on JobAttempt before external execution relies on it
      -> executor/run/AgentRun links remain correlated to that attempt
      -> evidence resolver loads the recorded attempt subject
      -> later Eggplan adapter converts it to SubjectRevision

Job-level fields may expose a convenience projection of the current/terminal
attempt, but JobRecord is not the provenance authority.

## 3. CodeGG-native subject DTO

Add a small versioned type in the narrowest core-owned provenance module.
Recommended semantic shape:

    ExecutionSubjectRevision {
        schema_version: u16,
        subject_kind: Git,
        repository_identity: String,
        revision: String,
        state: Clean | Dirty,
        dirty_digest: Option<String>,
    }

Exact names may differ.

Requirements:

- no Eggplan production dependency is required for this type;
- serialized shape is bounded and strict;
- `revision` is the exact Git HEAD object ID;
- clean implies no dirty digest;
- dirty requires a canonical SHA-256 dirty digest;
- repository identity is durable identity, never an absolute path;
- use existing `RepositoryId` when the workspace has one;
- where CodeGG currently has only a durable WorkspaceId at the execution
  boundary, use an explicit versioned workspace namespace such as
  `codegg-workspace:<workspace_id>`, not a path-derived pseudo-ID;
- record enough schema/version information that the later Eggplan adapter can
  perform a lossless conversion into Eggplan's current five-field
  `SubjectRevision`.

Do not persist:

- full Git status text;
- file contents;
- absolute paths;
- credentials;
- Git config/remotes;
- branch display names as authority.

## 4. Git capture owner

Implement capture through CodeGG's existing Git/workspace ownership, preferably
inside `crates/egggit` as a reusable bounded subject-capture function.

Do not add a new raw `std::process::Command::new("git")` owner outside the
existing governed Git execution seams.

The capture must be rooted at the scheduler/workspace-owned canonical root,
not process CWD.

### Capture content

At minimum incorporate:

- exact HEAD OID;
- staged changes;
- unstaged changes;
- untracked files;
- deletions;
- symlink identity/target where safely representable;
- submodule HEAD + dirty state within a bounded recursion policy;
- deterministic sorted path ordering;
- relevant index/blob identity;
- file content digest for dirty regular files.

Use a canonical bounded manifest internally and persist only its SHA-256 digest.

Bounds must cover:

- number of paths;
- total bytes hashed/read;
- path length;
- submodule depth.

Non-Unicode/unsafe paths or bound exhaustion return typed capture
unavailability; never silently fall back to a weaker subject.

## 5. Compatibility target with Eggplan

The immediate M001 goal is CodeGG-native provenance, but its semantics must be
losslessly convertible to Eggplan's current:

    SubjectRevision {
        subject_kind,
        repository_id,
        revision,
        state,
        dirty_digest,
    }

Use cross-repository golden cases copied as reviewed fixtures, not a production
dependency on `eggplan-repo`.

The M002 adapter may depend on `eggplan-core` and construct the Eggplan value
from this durable CodeGG record.

Do not claim that CodeGG's repository identity is already the same as an
Eggplan repository-store ID. M003 repository Plan binding owns any explicit
cross-repository identity translation.

## 6. Execution stability and remote-input semantics

One start capture alone is insufficient to claim that a live workspace stayed
stable throughout a command.

Persist a versioned provenance envelope, conceptually:

    ExecutionSubjectProvenance {
        captured: ExecutionSubjectRevision,
        sealed: Option<ExecutionSubjectRevision>,
        disposition: Stable | Drifted | Unavailable,
        seal_kind: LiveExecutionEnd | SnapshotBuilt,
        materialization_digest: Option<String>,
        materialization_complete: Option<bool>,
        unavailable_reason: Option<...>,
    }

Exact names may differ, but the information boundary must remain equivalent.

### Local live-workspace executors

1. capture S1 after the scheduler owns the canonical workspace lease and
   immediately before executor side effects;
2. persist S1 on the attempt before launching;
3. execute;
4. capture S2 after process/output cleanup and before terminal evidence is
   promoted;
5. Stable only when S2 == S1;
6. drift does not rewrite the job's terminal execution status, but the
   execution is not eligible as stable exact-subject evidence.

### Eggwork/current immutable-snapshot path

Current CodeGG flow is:

    preflight
      -> build_snapshot(workspace_root)
      -> upload_snapshot(copied bytes)
      -> create_remote_workspace(manifest)
      -> execute_in_workspace(...)

The authoritative source seal is therefore the successful construction of the
immutable in-memory snapshot, not the later remote workspace API call.

Required protocol:

1. capture/persist S1 immediately before `build_snapshot`;
2. build the bounded `WorkspaceSnapshot`;
3. capture S2 immediately after `build_snapshot` returns, before upload;
4. compute/persist the canonical `snapshot.manifest.digest()`;
5. classify materialization completeness;
6. exact-subject Stable eligibility requires:
   - S1 == S2;
   - no omitted source entries that could affect execution semantics;
   - a valid canonical manifest digest;
7. upload/create/execute from the already copied snapshot bytes;
8. later local edits after step 6 do not invalidate that historical remote
   input because upload consumes the immutable copied bytes.

Do NOT recapture the current worktree after upload or remote execution and use
that later value as the historical subject.

### Materialization completeness

Current `build_snapshot` records `skipped_non_regular` and
`skipped_oversize`; it deliberately skips symlinks/non-regular entries and
oversize files. Current Eggwork `WorkspaceManifest::validate` also forbids
symlink materialization.

Therefore:

- if either skip count is nonzero, remote execution MAY continue under current
  scheduler policy, but provenance is
  `Unavailable(MaterializationIncomplete)` for exact-subject evidence;
- do not silently treat "manifest successfully uploaded" as proof that the
  full source subject was transferred;
- preserve skip counts and manifest digest only as bounded provenance;
- a later Eggwork materializer contract may broaden exact-subject eligibility,
  but M001 must not overclaim it.

Extra transfer metadata such as repository administrative files may be present;
that does not itself invalidate exact-source eligibility. Omission of
source-relevant entries does.

### Concurrent mutation / ABA limitation

S1/S2 does not globally serialize arbitrary external workspace writers. It is
a bounded revalidation protocol analogous to Eggplan closure capture, not a
filesystem transaction.

For remote execution, the actual executed bytes are additionally bound by the
manifest digest. Closure evidence must state this limitation rather than claim
global exclusion of all transient mutations.

### Non-Git workspaces

Execution may proceed under existing policy, but provenance is
`Unavailable(NotGit)`. Such a result cannot satisfy Eggplan exact-Git
evidence.

## 7. JobAttempt persistence

Add an additive JobStore field for execution-subject provenance.

Preferred storage:

- `JobAttempt.source_subject_json: Option<ExecutionSubjectProvenance>`;
- SQLite nullable bounded JSON/text column;
- in-memory store parity;
- protocol/job-detail projection only if useful for authorized diagnostics.

The next sequential job/schema migration must:

- add the nullable column;
- leave existing rows NULL;
- never backfill a legacy row from the current workspace;
- preserve all current migrations/jobs/attempts.

Add intent-named store operations such as:

- `set_attempt_source_subject_started`;
- `seal_attempt_source_subject`;

or one equivalent CAS-safe API.

The store must reject:

- attempt/job mismatch;
- second conflicting S1 write;
- seal without a matching start record;
- mutation of an already stable/drifted terminal provenance record.

Subject persistence failure before launch must prevent the executor from
claiming subject-qualified evidence.

## 8. Scheduler/executor wiring

The scheduler already constructs `JobExecutionContext` from the canonical
workspace lease. Add subject provenance at that boundary rather than inside
individual command tools.

The generic scheduler/executor path should:

1. own workspace lease;
2. create/begin attempt;
3. capture/persist S1;
4. dispatch executor;
5. perform the appropriate live-end or materialization seal;
6. persist S2/disposition;
7. terminalize the attempt.

Executor-specific hooks may declare the seal point, but executors must not
become competing subject authorities.

Do not place source provenance in free-form labels.

## 9. RunStore propagation

Add optional source-subject provenance to RunStore records so a run remains
self-describing after restart/export.

Recommended behavior:

- `RunDraft` may carry the started subject where available;
- `RunCompletion` supplies the sealed/stability disposition;
- `RunManifest` persists the final bounded provenance envelope.

For scheduler-owned runs, the JobAttempt provenance is the authority and the
RunManifest copy is a correlated projection. Assert equality when both exist.

For direct non-scheduler RunStore producers, use the same governed capture
service if they need to become exact-subject evidence. M001 need not convert
every historical RunStore producer; unsupported producers remain
subject-unavailable.

## 10. AgentRun propagation/resolution

`AgentRunRecord` already carries optional `job_id` and `attempt_id`.

Prefer resolving its execution subject through that durable attempt link
rather than duplicating the subject into `agent_run` rows.

Rules:

- AgentRun with exact job+attempt link -> resolve attempt provenance;
- link missing or dangling -> subject unavailable;
- do not use current AgentRun worktree state to fill missing historical
  provenance;
- if a future non-job AgentRun path requires subject authority, add an explicit
  capture field then; do not overload owner IDs.

## 11. Enriched WorkPlan evidence resolver

Do not break the existing status-only `WorkPlanEvidenceSnapshot` during M001.

Add a parallel bounded host-resolution API for Eggplan adoption, conceptually:

    ResolvedWorkEvidence {
        kind,
        ref_id,
        status,
        source_subject: Option<ExecutionSubjectRevision>,
        source_subject_disposition,
        native_job_id,
        native_attempt_id,
        native_run_id,
    }

and:

    assemble_resolved_evidence(pool, items) -> ...

Existing `assemble(...)->WorkPlanEvidenceSnapshot` may continue as the legacy
CodeGG projection over status only until M002 differential adoption closes.

For completed execution refs:

- Stable subject -> return it;
- Drifted -> return status plus no authoritative subject / typed drift reason;
- legacy NULL -> status plus SubjectUnavailable;
- current workspace capture is never used as a substitute.

## 12. Verification-spec handoff

M001 does not need to persist Eggplan `VerificationDigest` if CodeGG's native
job/run record already durably preserves the canonical execution specification
required to derive it later.

However the enriched resolver must preserve enough identity to let M002 derive
the digest from the authoritative native object:

- TestJob -> canonical TestRunner/job execution specification;
- SchedulerJob -> canonical typed job payload/argv/cwd/execution policy;
- DelegatedRun/AgentRun -> canonical delegated task/run verification
  specification.

Stop and report if any supported evidence type cannot reconstruct that
specification from durable host state. Do not hash ref IDs or display prose as
a substitute.

## 13. Legacy compatibility

All pre-M001 records are valid historical CodeGG records.

Their subject disposition is:

    Unavailable(LegacyMissingProvenance)

Never:

- backfill from current HEAD;
- assume clean;
- infer subject from timestamps;
- infer subject from branch name;
- infer subject from artifact/result contents.

Legacy evidence remains usable by the existing CodeGG status-only assessor
until Eggplan-backed M002 adoption determines its explicit compatibility
policy, but it cannot become exact-subject Eggplan proof.

## 14. Failure/restart/contention semantics

### Capture failure

Persist/return a typed unavailability reason where possible. Existing execution
may continue if subject-qualified evidence is not an execution prerequisite,
but the attempt cannot later claim exact-subject evidence.

### Crash after S1 before execution

Attempt retains started provenance but not Stable disposition. Recovery must
not promote it to stable evidence.

### Crash during execution

Same rule: started-only is not stable proof.

### Restart after Stable seal

The exact subject reloads from the durable attempt without touching the
worktree.

### Concurrent workspace change

Local: S1 != S2 -> Drifted, not exact evidence.

Remote materialized: S1/S2 surround snapshot materialization. Changes after S2
do not alter the remote execution's bound input subject.

### Retry

Every new attempt captures independently. Attempt N+1 never inherits the
subject of attempt N.

## 15. Migration and storage scope

Update the current sequential storage version from v66 to v67 with the
additive execution-subject provenance migration. The current-head recheck
confirmed that Eggwork M002/M002a did not consume a later storage migration.

At minimum update:

- JobAttempt domain serialization;
- InMemoryJobStore;
- SqliteJobStore;
- attempt row mapping;
- migration fixtures/version guards;
- job protocol DTO only if exposing the field is necessary.

RunStore manifest schema remains additive through optional/defaulted fields.

Do not rewrite old manifest files.

## 16. Security/privacy

Persist only:

- schema/version;
- stable repository/workspace namespace;
- commit OID;
- clean/dirty state;
- SHA-256 dirty digest;
- stable/drifted/unavailable reason;
- bounded native IDs.

Never persist through this feature:

- file contents;
- absolute paths;
- environment values;
- credentials;
- remote URLs;
- branch descriptions;
- raw Git status;
- prompts/model text.

Diagnostics may expose the OID/digest and stable IDs to authorized local
operators but must remain bounded.

## 17. Static ownership guards

Update or add guards proving:

- subject capture lives only in the approved Git/workspace owner;
- `src/work_plan_evidence.rs` cannot call Git/current-worktree capture as a
  historical fallback;
- no new `std::process::Command::new("git")` owner is introduced;
- current workspace state is not consulted when resolving a completed
  historical attempt;
- JobAttempt, not JobRecord labels, is provenance authority.

Reuse existing execution-ownership tooling where practical.

## 18. Ordered work packages

### WP1 — Native subject model and bounded capture

Add `ExecutionSubjectRevision`, provenance/disposition enums, validation,
canonical dirty-manifest hashing, and clean/dirty fixtures in the Git owner.

### WP2 — Attempt persistence and migration

Add the nullable durable attempt provenance field, in-memory/SQLite parity,
intent-named set/seal operations, and migration/restart coverage.

### WP3 — Scheduler capture/seal integration

Capture S1 before local side effects. For local execution, seal at terminal
cleanup. For Eggwork, capture S1 immediately before `build_snapshot`, capture
S2 immediately after snapshot construction, persist the canonical manifest
digest/completeness disposition, and only then upload/submit. Ensure
drift/incomplete materialization/unavailability never becomes exact proof.

### WP4 — RunStore/AgentRun correlation

Project authoritative attempt subject into scheduler-owned RunManifest records
and resolve AgentRun subject through exact job+attempt linkage.

### WP5 — Enriched WorkPlan evidence resolution

Add the bounded status+subject resolver needed by Eggplan M002 while retaining
the legacy status-only snapshot unchanged.

### WP6 — Cross-repo qualification handoff

Produce fixtures/results demonstrating that Eggplan's pure CodeGG bridge can
consume the CodeGG subject without current-worktree reconstruction. Do not
perform the M002 production assessor swap in this milestone.

## 19. Required tests

### Subject capture

- clean repository;
- staged modification;
- unstaged modification;
- untracked file;
- deletion;
- symlink;
- bounded nested submodule;
- deterministic repeated dirty digest;
- unsafe/non-Unicode/bound-exceeded -> typed unavailable.

### Attempt persistence

- in-memory round trip;
- SQLite round trip;
- restart round trip;
- conflicting second S1 rejected;
- seal without S1 rejected;
- Stable record immutable;
- retry gets independent subject.

### Local execution

- stable S1/S2 -> Stable;
- source mutation during execution -> Drifted;
- terminal job status remains truthful even when subject drifted;
- drifted completion is not subject-qualified evidence.

### Remote/materialized execution

- S1/S2 around `build_snapshot` with complete snapshot -> Stable;
- source mutation during snapshot construction -> Drifted;
- `skipped_non_regular > 0` -> execution may proceed, exact subject unavailable;
- `skipped_oversize > 0` -> execution may proceed, exact subject unavailable;
- canonical manifest digest persisted and stable across retry/restart;
- local mutation after the in-memory snapshot seal does not rewrite the
  historical subject;
- upload/create-workspace consumes the sealed snapshot rather than rereading
  the live worktree;
- remote attempt restart retains exact subject + manifest provenance;
- current-worktree recapture after remote completion is never used as
  historical evidence.

### Legacy

- pre-migration DB opens;
- legacy attempt has NULL/LegacyMissingProvenance;
- resolver never captures current workspace to fill it.

### AgentRun/RunStore

- linked AgentRun resolves exact attempt subject;
- dangling/missing attempt -> unavailable;
- scheduler-owned RunManifest matches attempt provenance;
- restart/export preserves optional provenance.

### WorkPlan resolver

- completed job + Stable subject returns Passed + exact subject;
- failed job + Stable subject returns Failed + exact subject;
- running returns InProgress with current started provenance only as
  non-terminal metadata;
- legacy completed returns Passed status + subject unavailable;
- drifted completed returns terminal status + subject drifted;
- ref missing remains unavailable.

## 20. Required verification

Use the repository's current canonical checks. At minimum:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test -p codegg-core --lib -- jobs
    cargo test -p codegg-core --lib -- run_store
    cargo test -p codegg-core --lib -- work_plan
    cargo test -p codegg-core -- migration
    cargo test -p codegg-core --test work_plan_foundation
    cargo test -p codegg-core --test work_plan_projection_arbiter
    cargo test --test work_plan_projection_arbiter
    cargo test --test long_horizon_trajectory_qualification
    bash scripts/check-core-boundary.sh
    python3 scripts/check_execution_ownership.py
    ./scripts/verify.sh quick
    git diff --check

Use current equivalent commands if names changed and record actual commands in
closure.

Hosted CI evidence must be recorded according to CodeGG planning conventions.

## 21. Documentation updates

Update:

- `architecture/jobs.md`;
- `architecture/run_store.md`;
- `architecture/work_plan.md`;
- `architecture/git.md` if the subject capture API is added there;
- `plans/subsystems/eggplan-assessment-integration-roadmap.md`;
- `plans/registry.md`.

Document:

- attempt-level subject authority;
- local vs materialized seal points;
- legacy unavailable semantics;
- enriched WorkPlan evidence resolver;
- explicit non-goal of replacing CodeGG execution ownership.

## 22. Acceptance criteria

M001 closes when:

1. each new evidence-relevant JobAttempt can durably record its execution
   source subject;
2. provenance is attempt-scoped and survives restart;
3. local live-workspace drift is detected and cannot become stable exact
   evidence;
4. remote/materialized executions bind subject at the in-memory snapshot seal,
   persist the canonical manifest digest, and withhold exact-subject
   eligibility when materialization is incomplete;
5. legacy attempts remain subject-unavailable with no current-worktree
   backfill;
6. AgentRun and scheduler-owned RunStore evidence can resolve the authoritative
   attempt subject;
7. enriched WorkPlan evidence resolution returns status and subject as
   separate facts;
8. CodeGG can hand the durable subject to Eggplan's pure bridge without a
   production dependency on eggplan-repo;
9. existing WorkPlan/Goal/Todo/checkpoint/scheduler behavior remains unchanged;
10. full focused/canonical verification passes.

## 23. Stop conditions

Stop and report if:

- exact historical provenance would require current-worktree reconstruction;
- the design requires putting the subject on JobRecord as sole authority;
- capture requires a new ungoverned Git subprocess owner;
- legacy rows would need fabricated backfill;
- CodeGG cannot distinguish live-workspace and materialized-input seal
  semantics;
- remote exact-subject qualification would require treating a snapshot with
  skipped source entries as complete;
- M001 would require swapping the production WorkPlan assessor before
  differential qualification;
- source provenance would include file contents/paths/secrets.

## 24. Closure evidence required

Create
`plans/closure/eggplan-assessment-integration/001-status.md` containing:

- implementation SHA(s);
- exact migration/storage version (expected v67 from current v66);
- ExecutionSubjectRevision/provenance schema table;
- clean/dirty/drift/legacy/materialization-incomplete matrix;
- local vs remote seal-point evidence;
- Eggwork manifest-digest and snapshot-completeness evidence;
- JobAttempt persistence/restart evidence;
- AgentRun/RunStore correlation evidence;
- enriched WorkPlan resolver evidence;
- static ownership-guard evidence;
- exact verification commands and hosted workflow IDs;
- current Eggplan bridge SHA/head recheck;
- explicit disposition unblocking or continuing to block CodeGG/Eggplan M002.

## 25. Handoff notes

This is the upstream provenance primitive only.

Do not:

- add Eggplan repository persistence to CodeGG;
- change WorkPlan completion families;
- delete the legacy assessor;
- manufacture verification digests from reference IDs;
- start repository Plan binding.

After positive M001 closure, resume the coordinated Eggplan/CodeGG M002 plan
from its blocked checkpoint and perform the differential adoption work.
