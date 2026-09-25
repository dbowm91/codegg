# Eggwork Fixed-Target Remote Execution Roadmap

Status: active roadmap; M001+C001+M002+M002a closed, M003 blocked on materializer contract, M004 deferred

Canonical authority:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`, especially Phase 14 remote workspace/execution abstraction and the later coordinator/node execution direction
- `plans/003-planning-process.md`

External execution substrate:

- Eggwork repository: `https://github.com/eggstack/eggwork`
- reviewed Eggwork implementation baseline: `2566e6a54451468011119844824b644aea891b0c`
- reviewed Eggwork Workspace M004 planning commit: `ed0fc838bd006103021f1fb4749f579cabc2db87`
- Control Plane M003 closure: `7fe07c0b75c2ab022f2a17b5999ee589e3d94e62`
- Foundation M003 closure: `b3b5459a6077f59278fa1694a65f8c29a76000ef`

Implementation MUST re-check Eggwork current head and public crate/API shape before pinning a dependency.

## 1. Purpose and ownership boundary

This subsystem lets CodeGG deliberately execute selected finite scheduler jobs on one explicitly selected Eggwork node.

CodeGG remains authoritative for:

- durable `JobRecord` / `JobAttempt` lifecycle;
- `JobScheduler` admission, fairness, retries, cancellation policy, and resource permits;
- project/workspace/worktree identity;
- principal/authorization/audit;
- RunStore semantics;
- target selection and future placement policy;
- AgentRun and WorkOrder semantics.

Eggwork owns only the selected remote node's:

- authenticated fixed-target transport;
- remote process lifecycle;
- execution idempotency/generation/lease;
- bounded event stream;
- remote materialized workspace;
- declared output/artifact capture;
- node-local sandbox/resource enforcement.

Eggwork MUST NOT become a second CodeGG scheduler.

## 2. Current CodeGG seams

Reviewed CodeGG implementation baseline: `841ad117399c9279e3668828d5d9866983603381`.

Relevant current seams:

- `JobSubmissionService` is the single durable submission boundary.
- `JobScheduler` owns the fair queue and `ResourcePermitGuard`.
- `JobExecutionContext` carries the full `JobRecord`, attempt id, daemon generation, canonical workspace root, RunStore, cancellation token, progress sink, and live resource permit.
- `executor_kind_for_job` is the single current `JobRecord -> ExecutorKind` mapping authority.
- `JobStore::set_attempt_executor` persists executor provenance before running.
- local finite argv execution remains owned by `ManagedProcessService`.
- CodeGG already uses Rust 1.89 and `eggfetch-core 0.2.0`.

## 3. Durable invariants

1. A CodeGG job target is selected before Eggwork invocation.
2. Target identity is durable job/attempt state, not an ambient process-global setting.
3. Node TLS credentials/configuration are daemon-owned references and are never copied into job payloads, labels, logs, or RunStore artifacts.
4. The CodeGG scheduler permit remains held until remote execution reaches terminal/cleanup convergence or a typed lost/interrupted disposition.
5. One CodeGG attempt maps to exactly one Eggwork execution identity.
6. Transport reconnect to the same accepted Eggwork execution is not a new CodeGG attempt.
7. Eggwork `busy`/`draining`/capability/transport facts return to CodeGG policy; the Eggwork adapter never selects another node.
8. Local jobs continue to use existing local executors.
9. A remote workspace is a snapshot/materialization; Eggwork cannot mutate the authoritative local CodeGG worktree in place.
10. AgentRun/WorkOrder authority does not move into Eggwork.
11. Shell strings are never synthesized to bypass Eggwork's argv-first contract.
12. Large output/artifacts remain bounded and flow through existing CodeGG progress/RunStore abstractions.

## 4. Milestones

### M001 — Fixed-target finite-job executor

Class: capability/invariant

Status: qualified through closed corrective C001

Implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/001-fixed-target-finite-job-executor.md`

Historical closure:

- `plans/closure/eggwork-fixed-target-remote-execution/001-status.md`
- implementation `67f8f3d33651846bbdbd3e4a3bc239e50a0237a6`

Post-closure corrective authority:

- `plans/subsystems/eggwork-fixed-target-remote-execution-post-closure-corrective-addendum.md`
- C001: `plans/implementation/eggwork-fixed-target-remote-execution-corrective/001-lease-identity-and-live-node-qualification.md`

A later review found that the original M001 implementation created divergent live/persisted lease tokens. Corrective C001 closed that defect and is the current qualification evidence; the historical M001 closure remains immutable predecessor evidence.

Corrective closure:

- `plans/closure/eggwork-fixed-target-remote-execution-corrective/001-status.md`
- implementation `f4e6e69d` (+ follow-ups through `3ca3b7b6`); hosted CI run `36051370501` success

C001 restored single-source lease identity, qualified the production NodeClient against a real loopback node over mTLS (valid renew/cancel, typed `invalid_lease` rejection, live + terminal restart reconciliation with no second submit), and proved the pinned node only admits unrestricted specs (HTTP 409 otherwise; executor requests `None` + `Unrestricted`, see the closure finding). M001 remains historical evidence; C001 is the current correctness disposition.

Objective:

Add a durable explicit Eggwork execution target and an Eggwork-backed scheduler executor for a bounded set of finite job kinds, preserving CodeGG scheduler/workspace/RunStore authority.

### M002 — Target capability projection and operator policy

Class: infrastructure/polish

Status: closed

Closure record:

- `plans/closure/eggwork-fixed-target-remote-execution/002-status.md`

Implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/002-target-capability-projection-and-operator-policy.md`

C001 follow-up owned here: surface the per-node unrestricted-execution posture in operator diagnostics/policy, make required isolation/network policy fail closed before upload, and construct restricted specs only when authenticated node capability evidence supports them.

Interface dependency:

- Eggwork remote-admission corrective feature contract (`isolation.landlock.workspace-rw.v1`, `network.unrestricted.v1`, existing resource features) is registered upstream. M002 may implement against that written contract before the upstream code lands.

Objective:

Project Eggwork node capabilities/status into CodeGG diagnostics, executor health, and caller-owned target policy without implementing selection inside Eggwork.

Expected scope:

- named node status/capability cache with bounded TTL;
- executor health projection;
- user/operator diagnostics;
- explicit capability mismatch before expensive workspace upload where possible;
- no global automatic placement yet.

### M002a — Restricted-spec live requalification

Class: invariant/qualification

Status: closed; M002 and Eggwork remote-admission corrective C001 are closed; CodeGG live required-Landlock qualification passed hosted Linux CI

Implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/002a-restricted-spec-live-requalification.md`

Objective:

Pin the corrected Eggwork revision and prove CodeGG's production mTLS path executes a required Landlock-isolated job with filesystem escape denial, exact lease/restart behavior, and no network-isolation overclaim. Closure evidence: `plans/closure/eggwork-fixed-target-remote-execution/002a-status.md`.

### M003 — Workspace transfer optimization

Class: infrastructure/capability

Status: blocked on Eggwork Workspace/Artifact M004 closure

Implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/003-content-aware-derived-workspace-transfer.md`

Upstream prerequisite:

- Eggwork `plans/implementation/workspace-artifact-transport/004-reusable-manifest-cas-and-derived-materialization.md`
- planning commit `ed0fc838bd006103021f1fb4749f579cabc2db87`

Objective:

Reduce repeated full-manifest workspace transfer using Eggwork's Git-neutral retained-manifest + deterministic-patch contract while preserving CodeGG's existing full local snapshot, exact-source provenance, worktree authority, and dirty-state semantics.

M003 deliberately keeps local full snapshot construction authoritative; it optimizes transfer representation rather than introducing metadata-only hashing shortcuts.

### M004 — Whole remote AgentRun worker

Class: capability

Status: deferred; M002 target policy and M002a restricted live qualification are closed. Still blocked on workspace optimization (M003) and the stable AgentRun worker-entry contract.

Objective:

Run a bounded CodeGG worker process under Eggwork while the central CodeGG daemon remains authoritative for the AgentRun, tools, permissions, and returned Git/artifact evidence.

## 5. M001 target model

M001 should introduce a typed durable target rather than encoding node selection in arbitrary labels.

Conceptual model:

```rust
enum ExecutionTarget {
    Local,
    EggworkNode { node_id: String },
}
```

Requirements:

- existing jobs deserialize/default to `Local`;
- target is persisted by both in-memory and SQLite JobStore implementations;
- schedule/job templates preserve the selected target where scheduled remote work is explicitly supported;
- protocol DTOs expose it only where an authorized caller is allowed to select a target;
- node endpoint/certificate/key paths do not live in this type.

Node connection profiles belong to CodeGG daemon/config state keyed by node id.

## 6. Executor routing

Extend the current single routing authority rather than adding a side dispatcher.

For `ExecutionTarget::Local`, current `executor_kind_for_job` behavior remains unchanged.

For `ExecutionTarget::EggworkNode`, M001 may add `ExecutorKind::Eggwork` and route only explicitly supported finite kinds to the Eggwork executor.

Initial eligible kinds:

- `Build`;
- `Lint`;
- `Format`;
- `ManagedProcess` with explicit argv;
- `Test` only where the existing payload already carries the canonical argv and result/artifact mapping can preserve TestRunner-visible semantics.

Not in M001:

- `Shell` if it requires reconstructing an implicit shell string;
- Python;
- ToolProgram;
- Subagent;
- AgentTurn;
- GitMutation;
- interactive process sessions.

Unsupported remote kinds fail typed preflight validation; they never silently execute locally.

## 7. Remote attempt provenance and restart

CodeGG must persist enough executor provenance before/at remote acceptance to inspect and reconnect to the exact Eggwork execution.

Persist or otherwise durably derive:

- selected CodeGG node id;
- Eggwork execution id;
- Eggwork generation;
- lease id/renewal state required by the public Eggwork client;
- workspace id/materialization identity;
- protocol/capability baseline as needed for diagnostics.

A recommended identity mapping is a collision-safe deterministic digest over CodeGG job id + attempt id for Eggwork `ExecutionId`, with generation `1` for each unique CodeGG attempt. Transport retries reuse the same tuple. A new CodeGG attempt receives a distinct Eggwork execution id.

Do not derive remote identity from display titles or mutable workspace paths.

On CodeGG restart, current generic generation recovery must not blindly create a second remote process. M001 must either reconnect/reconcile an accepted remote execution using persisted provenance or conservatively mark the attempt interrupted according to an explicitly tested policy. Safe-repeat requeue must create a new CodeGG attempt and therefore a new Eggwork execution identity.

## 8. Workspace and artifact contract

Snapshot the scheduler-owned `workspace_root` through Eggwork's content-addressed manifest/blob API.

Requirements:

- apply Eggwork path/count/size bounds before network mutation;
- do not follow a symlink outside the authoritative CodeGG workspace;
- compute SHA-256 blobs and use find-missing before upload;
- materialize under an Eggwork-owned workspace id derived/persisted for the attempt;
- remote cwd is relative to that materialization;
- remote mutation never writes directly into CodeGG's local worktree.

Declared remote outputs are imported/referenced through the existing RunStore contract. Large artifact bodies remain streamed/handle-backed.

M001 may use a bounded full workspace snapshot. Git-aware optimization belongs to M003.

## 9. Security and configuration

Define named Eggwork node profiles in CodeGG configuration with, at minimum:

- stable node id;
- HTTPS endpoint;
- CA trust source;
- client certificate;
- client private-key reference/path;
- optional expected capability requirements.

Requirements:

- endpoint must remain HTTPS;
- credentials never enter `JobRecord`, labels, progress strings, audit metadata, or Debug output;
- config path/permission rules follow existing CodeGG secret-reference conventions;
- mTLS identity remains Eggwork transport identity; CodeGG cannot self-assert a principal in request payloads;
- direct route only is sufficient for M001. Eggress-routed CodeGG connectivity can be a later policy/config extension.

## 10. Verification strategy

M001 closure requires:

- local executor regression coverage;
- explicit remote target persistence and restart serialization;
- one CodeGG attempt -> one Eggwork execution;
- duplicate submit/reconnect cannot create a second remote child;
- scheduler permit remains held through remote cleanup;
- remote busy/draining/capability mismatch;
- cancellation before submit, during preparation, during running, and during artifact capture;
- network partition shorter/longer than lease;
- CodeGG restart with an accepted remote execution;
- Eggwork restart while CodeGG waits;
- local workspace remains unchanged by remote mutation;
- artifact import;
- auth/certificate failure;
- no target fallback from remote to local;
- no second scheduler/static ownership review.

## 11. Deferred work

Automatic multi-node placement belongs to future CodeGG scheduling work, not Eggwork.

Reverse-connect, PTY, Git-aware materialization, and whole AgentRun execution are independent later milestones and must not expand M001.
