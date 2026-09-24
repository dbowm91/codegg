# Eggwork Remote Execution M001 — Fixed-Target Finite-Job Executor

Status: ready for handoff

Source roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`

Canonical references:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md` Phase 14 and later distributed-execution direction
- `plans/003-planning-process.md`

CodeGG plan-authoring baseline:

- `e508de52083a30a1b234f5f9b619910cad6792d4`

Eggwork baseline reviewed:

- `e7d9a8e5a9b66f7c68ff4236c9a074ba81e035d7`
- Control Plane M003 closed
- Foundation M003 closed
- Workspace/Artifact M003 and Security M003 already closed/qualified

Implementation MUST re-audit both repository heads before editing and pin any Eggwork Git dependency to an immutable revision.

## 1. Objective

Let the CodeGG scheduler execute explicitly targeted finite jobs on one named Eggwork node while preserving CodeGG's scheduler, attempt, workspace, authorization, and RunStore authority.

M001 establishes one complete vertical slice:

```text
JobSubmissionService
  -> durable JobRecord(target = EggworkNode)
  -> JobScheduler + ResourcePermitGuard
  -> EggworkExecutor
  -> eggwork-client
  -> one selected eggworkd
  -> remote workspace/process/artifacts
  -> CodeGG progress + RunStore + ExecutorCompletion
```

## 2. Explicit non-goals

Do not implement:

- automatic node selection or load balancing;
- Eggwork-side scheduling;
- provider/model/AgentRun loops;
- remote Tool Programs;
- interactive PTY;
- SSH fallback;
- local fallback after remote failure;
- Git-aware transfer optimization;
- reverse-connect relay;
- service installation/update integration;
- arbitrary shell-string reconstruction.

## 3. Dependency and crate policy

Prefer direct use of Eggwork's public Rust crates:

- `eggwork-client`;
- `eggwork-core`.

At implementation start:

1. verify the packages build as Git dependencies from the selected Eggwork revision;
2. pin an exact immutable `rev` in CodeGG;
3. keep Eggwork dependencies in the root consumer unless a second CodeGG crate genuinely needs them;
4. avoid enabling Eggwork's optional `eggress-route` feature in M001;
5. verify that transitive `eggfetch-core` resolves compatibly with CodeGG's existing 0.2.0 ownership.

Do not vendor/copy Eggwork protocol structs.

## 4. Work package A — Durable execution target

Add a typed execution-target domain under `codegg-core::jobs` or the narrowest equivalent core-owned module.

Required semantic shape:

- `Local` default;
- `EggworkNode { node_id }` explicit remote target.

Update all durable creation/persistence surfaces that must preserve it:

- `NewJob`;
- `JobRecord`;
- schedule template/materialization only where target propagation is semantically valid;
- InMemoryJobStore;
- SqliteJobStore and schema/migration;
- protocol conversions/DTOs for authorized job submission and inspection;
- test fixtures/builders.

Compatibility:

- historical persisted jobs with no target become `Local`;
- old callers constructing `NewJob` get an explicit/default migration path;
- target identifiers are bounded and validated;
- credentials/endpoints are not persisted in jobs.

Run `scripts/check-core-boundary.sh` after core changes.

## 5. Work package B — Named Eggwork node configuration

Add daemon-owned configuration for named Eggwork nodes.

Each profile should contain or reference:

- node id;
- HTTPS endpoint;
- CA certificate/trust material;
- client certificate;
- client private key;
- optional required capability declarations.

Requirements:

- reuse CodeGG config loading/validation conventions;
- private key material is never serialized into ordinary diagnostics;
- Debug/Display output is redacted;
- relative/ambiguous secret paths follow existing configuration policy;
- no environment-global mutable current-node setting.

The job stores only node id. The executor resolves that id against current daemon configuration before any upload/execute side effect.

## 6. Work package C — Target-aware executor routing

Update the existing central routing boundary instead of bypassing it.

Add `ExecutorKind::Eggwork` or a comparably explicit typed variant.

`executor_kind_for_job` (or its intentionally renamed successor) remains the single mapping authority.

Rules:

- local target -> unchanged existing executor kind;
- Eggwork target + eligible job -> Eggwork executor;
- Eggwork target + unsupported job -> typed validation failure/blocked outcome;
- remote failure -> no fallback to local executor.

Initial eligible jobs:

- Build;
- Lint;
- Format;
- ManagedProcess with canonical argv;
- Test only if its existing canonical argv + result semantics can be mapped without bypassing TestRunner-required durable evidence.

If Test cannot meet that bar in one coherent pass, stop that sub-scope and close M001 with the other finite kinds only if the roadmap/closure explicitly records Test as deferred rather than silently claiming it.

## 7. Work package D — Eggwork executor and deterministic identity

Implement `EggworkExecutor` as a normal `JobExecutor`.

It must:

1. call `ctx.validate_runtime()`;
2. resolve the configured node;
3. verify Eggwork protocol/capabilities;
4. build/upload the remote workspace;
5. derive or load the persisted remote execution handle;
6. submit exactly one idempotent Eggwork execution;
7. stream bounded progress;
8. propagate cancellation;
9. renew/reconcile lease as required;
10. retrieve declared artifacts;
11. map terminal facts to `ExecutorCompletion`;
12. return only after remote terminal/cleanup convergence or typed interrupted/lost handling.

Identity requirements:

- `ExecutionId` is deterministic for CodeGG job id + attempt id;
- generation is stable for that attempt;
- retransmission uses the same Eggwork handle;
- a CodeGG retry/new attempt maps to a different Eggwork execution identity.

Persist the remote provenance needed for restart/reconciliation through the existing attempt executor-provenance boundary. Do not hide it in progress text.

## 8. Work package E — Workspace snapshot

Build a bounded Eggwork workspace manifest from `ctx.workspace_root`.

Required protections:

- canonical root is the scheduler-owned workspace lease root;
- reject traversal and symlink escape;
- regular files only unless Eggwork explicitly supports another type safely;
- bound entry count, path bytes, total logical bytes, and individual upload sizes to Eggwork limits;
- compute SHA-256;
- call find-missing then upload only missing blobs;
- create a deterministic/persisted remote workspace id;
- map payload cwd to an Eggwork relative cwd.

Do not write remote results back into the local workspace automatically.

## 9. Work package F — Command/result mapping

Map only typed existing payload fields.

For ManagedArgv/build/lint/format:

- preserve argv item boundaries exactly;
- preserve relative cwd;
- map timeout to Eggwork timeout;
- use explicit environment allowlist/mapping; do not copy the daemon environment wholesale.

For Test, if included:

- use the existing argv field;
- preserve the CodeGG test/run evidence expected by current callers;
- do not reconstruct the historical human-readable `command` through a shell.

Terminal mapping must distinguish:

- completed;
- nonzero failed;
- cancelled;
- timed out;
- interrupted/lost transport/recovery.

Eggwork `busy`, `draining`, capability mismatch, and transport failure are not successful execution completions and must remain distinguishable in CodeGG diagnostics.

## 10. Work package G — Progress and artifacts

Map Eggwork stdout/stderr/event facts into bounded `JobProgressSink` messages or a structured progress adapter without leaking secrets or flooding the bus.

Import declared artifacts into `RunStore` using its existing bounded artifact APIs. Preserve Eggwork digest/identity as provenance metadata where the RunStore schema permits.

Do not put large remote output directly into job rows or audit metadata.

## 11. Work package H — cancellation, lease, and restart semantics

Cancellation:

- queued cancellation before remote submit produces no remote execution;
- preparation cancellation stops before/while upload where possible;
- running cancellation invokes Eggwork cancel on the exact handle;
- CodeGG scheduler permit is held until remote cleanup/terminal convergence.

Restart:

- persist remote handle/provenance before relying on it for recovery;
- on daemon restart, do not blindly resubmit under a new remote identity;
- reconcile/observe the accepted Eggwork execution when the public API permits;
- otherwise mark the stale CodeGG attempt interrupted conservatively;
- any safe-repeat requeue creates a new CodeGG attempt and therefore a new Eggwork execution identity.

Network partition:

- shorter-than-lease reconnect resumes/observes the same execution;
- longer-than-lease path is typed interrupted/cancelled according to Eggwork evidence;
- no alternate node is selected.

## 12. Static/architecture guards

Update:

- `architecture/jobs.md`;
- `architecture/scheduler.md`;
- `docs/execution-ownership.md`;
- scheduler/jobs skills where necessary.

Add or extend tests/guards proving:

- target selection remains CodeGG-owned;
- Eggwork executor cannot be reached for `Local` jobs;
- unsupported remote kinds do not fall back local;
- scheduler permit lifetime includes remote execution;
- no new direct process-spawn owner is introduced by the remote adapter.

## 13. Required tests

At minimum:

- durable Local target backward compatibility;
- durable Eggwork target SQLite round-trip;
- protocol DTO round-trip/authorization behavior;
- unknown node id;
- bad endpoint/certificate/client identity;
- capability mismatch before execution;
- remote echo/argv fixture;
- build/lint/format/managed argv remote fixture;
- Test fixture if Test is claimed;
- one attempt -> one Eggwork execution under duplicate submission;
- remote busy/draining;
- cancellation during upload/preparing/running/artifact fetch;
- short partition + resume;
- lease-expiry/long partition;
- CodeGG restart after remote acceptance;
- Eggwork restart;
- local workspace mutation isolation;
- artifact import;
- scheduler permit contention while remote work is live;
- explicit no-local-fallback assertion.

Use a local Eggwork daemon/test CA fixture; do not require internet service.

## 14. Broad verification

Use the repository's normal canonical checks plus focused suites. At minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo check --workspace
bash scripts/check-core-boundary.sh
git diff --check
```

Also run the focused scheduler/job/Eggwork integration suites added by this milestone.

## 15. Acceptance criteria

1. An authorized CodeGG job can explicitly target one named Eggwork node.
2. Existing jobs default to local execution.
3. CodeGG scheduler remains the sole admission/fairness/retry authority.
4. The scheduler permit spans the whole remote execution lifetime.
5. One CodeGG attempt produces at most one accepted Eggwork execution.
6. Local workspace content is transferred safely and not mutated remotely in place.
7. Cancellation/progress/artifacts/terminal state cross the adapter.
8. Remote failure never silently executes locally or chooses another node.
9. Credentials remain daemon/config-owned and secret-safe.
10. Restart/partition behavior is conservative and duplicate-safe.
11. Local executors remain available and unchanged for Local jobs.
12. No unresolved high/medium finding remains.

## 16. Stop conditions

Stop and record a blocker rather than improvising if:

- Eggwork public client cannot support the required idempotent/reconnect path;
- CodeGG cannot persist remote provenance without a materially broader JobStore redesign;
- Test semantics require bypassing existing durable RunStore/TestRunner ownership;
- remote target selection would need to live in Eggwork;
- credentials would need to be embedded in durable job payloads;
- workspace transfer would require unsafe symlink/path behavior;
- a remote failure would have to fall back to local execution to pass tests.

## 17. Closure evidence

Create `plans/closure/eggwork-fixed-target-remote-execution/001-status.md` containing:

- exact CodeGG implementation SHA(s);
- exact Eggwork dependency revision and feature set;
- durable target/provenance schema evidence;
- local-vs-remote routing matrix;
- identity/idempotency evidence;
- workspace transfer bounds;
- cancellation/lease/restart evidence;
- artifact/RunStore evidence;
- scheduler-permit lifetime evidence;
- secret-negative evidence;
- focused and full verification;
- hosted CI where applicable;
- residual findings and M002 disposition.
