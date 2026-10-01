# Eggplan Assessment Integration M003 — Repository Plan Binding and Writeback

Status: ready for coordinated handoff

Repository baseline:

- CodeGG planning head after M003 status reconciliation:
  `9e93e949e1a4abcc91fd7755d929a54fd6531160`
- CodeGG M002 closure:
  `ffa1c15e654776c3ebe1022f4ce7de2582bc5d98`
- Eggplan M003 contract plan registration:
  `6ba3db24efb8ed5952be8c5a522a9c4c52f7ed63`
- Eggplan M003 implementation/pin:
  `3f7c603315131bb169bfdd2bb575531d228532b1`
- Eggplan hosted native/MSRV qualification:
  `36868055136` (success on exact pin)

Source roadmap:

- `plans/subsystems/eggplan-assessment-integration-roadmap.md`

Coordinated Eggplan plan:

- `eggstack/eggplan:plans/implementation/codegg-integration/003-repository-plan-binding-contract.md`

Hard predecessor:

- M002 staged production assessment adoption — closed.

Primary class: cross-repository binding / persistence / authority integration

## 1. Objective

Allow an explicitly bound CodeGG session or one-shot WorkOrder occurrence to
execute against an existing repository-local Eggplan Plan.

For a bound plan:

- Eggplan repository Plan is canonical for plan/item intent and lifecycle;
- Eggplan immutable ledger is canonical for durable evidence;
- Eggplan guarded closure is canonical for terminal success;
- CodeGG keeps a durable runtime mirror for scheduler/agent/Todo/Goal use;
- CodeGG remains owner of WorkOrder/session/scheduler/job/AgentRun/worktree
  execution.

M003 must not create two competing plan authorities.

## 2. Authority model

### Eggplan owns for a bound plan

- Plan objective;
- item identity/order/parent/dependencies;
- acceptance criteria and evidence requirements;
- Plan and PlanItem lifecycle;
- blocker/next-action canonical values;
- evidence observations and supersession lineage;
- assessment;
- Closed state and ClosureRecord.

### CodeGG continues to own

- session and project/workspace bindings;
- WorkOrder and occurrence release/materialization;
- WorkPlan runtime mirror IDs/revisions;
- Goal/Todo/checkpoint/context epoch;
- Job/JobAttempt/AgentRun;
- scheduler/worktree/Eggwork;
- model/runtime owner references;
- verification-spec derivation;
- runtime UI/projection.

The CodeGG WorkPlan mirror is never an independent source of truth once bound.

## 3. Eggplan dependencies

The coordinated Eggplan M003 pure projection contract is implemented and
qualified at `3f7c603315131bb169bfdd2bb575531d228532b1`. Update the existing
exact Eggplan pin to that immutable revision when beginning implementation.

Production root-package dependencies allowed for M003:

- `eggplan-core`;
- `eggplan-codegg-compat`;
- `eggplan-repo`.

This is the first milestone intentionally consuming `eggplan-repo` in
CodeGG production.

Still forbidden in production:

- eggplan-cli;
- eggplan-projection;
- eggplan-markdown;
- eggplan-integrations.

`codegg-core` remains free of Eggplan crate dependencies. Repository I/O and
translation stay in the CodeGG application layer.

Add/extend dependency guards to enforce that boundary.

## 4. State-root scope

M003 binds only the canonical repository-local state root:

    <canonical CodeGG workspace root>/.eggplan

Do not accept:

- arbitrary absolute Eggplan state paths;
- caller-supplied parent traversal;
- URLs;
- another workspace's state root;
- auto-initialization of a missing store.

The state root must already exist and be valid. The target Plan must already
exist and be Active or Blocked.

Draft requires explicit Eggplan activation first. Closed/Cancelled are
historical and cannot be live-bound.

## 5. Subject/repository identity proof

M002 CodeGG provenance uses a CodeGG namespace such as
`codegg-workspace:<workspace_id>`. Eggplan repository subjects use a stable
`epr_*` repository ID.

M003 MUST NOT blindly replace one repository_id with the other.

At initial binding:

1. resolve the canonical session WorkspaceId and CodeGG RepositoryId through
   ProjectStorage/SessionBinding;
2. open `workspace_root/.eggplan`;
3. read Eggplan `RepositoryStore::repository_id()`;
4. independently capture CodeGG current Git subject through governed `egggit`;
5. independently capture Eggplan subject through
   `RepositoryStore::subject_source().capture()`;
6. require both are Git and have equal:
   - revision OID;
   - clean/dirty state;
   - normalized dirty digest;
7. record the proven tuple:
   - CodeGG RepositoryId;
   - WorkspaceId;
   - CodeGG subject namespace;
   - Eggplan repository_id;
8. only that durable binding may translate later CodeGG historical subjects
   into the Eggplan repository namespace.

If workspace/project repository rebinding changes the CodeGG relation, the
Eggplan binding becomes Conflict until explicitly rebound.

A capture mismatch is `repository_subject_mismatch`, never a warning.

## 6. Storage — additive v68

Current CodeGG storage layout is v67. M003 owns additive v68.

Add a CodeGG-core generic persistence seam with no Eggplan types.

### work_plan_eggplan_binding

Recommended fields:

- work_plan_id PK/FK;
- workspace_id;
- codegg_repository_id;
- eggplan_repository_id;
- eggplan_plan_id;
- last_seen_plan_revision;
- intent_digest;
- projection_digest;
- binding_state:
  - synced;
  - needs_reconcile;
  - conflict;
  - released;
- created_at_ms;
- updated_at_ms;
- released_at_ms nullable.

Enforce one live binding for the same
`workspace_id + eggplan_repository_id + eggplan_plan_id`.

### work_plan_eggplan_item_binding

- work_plan_id;
- work_item_id;
- eggplan_item_id;
- unique(work_plan_id, work_item_id);
- unique(work_plan_id, eggplan_item_id).

### work_order_eggplan_binding

For explicit one-shot WorkOrder inheritance:

- work_order_id PK/FK;
- codegg_repository_id;
- eggplan_repository_id;
- eggplan_plan_id;
- intent_digest;
- created_at_ms.

This table stores a binding request, not scheduler authority.

All migrations are additive and leave historical unbound plans/orders
unchanged.

## 7. Binding service

Add one application-layer owner, recommended:

    src/work_plan_repository_binding.rs

It owns all Eggplan repository access and cross-store reconciliation.

Do not scatter `RepositoryStore::open` throughout tools/arbiter/scheduler.

Recommended operations:

    bind_session_plan(...)
    bind_work_order_plan(...)
    load_binding(...)
    reconcile_bound_plan(...)
    update_bound_item(...)
    sync_terminal_evidence(...)
    assess_bound_plan(...)
    finalize_bound_plan(...)

Exact names may vary.

## 8. Explicit session binding

Add a host/user-authorized daemon/protocol operation, not a model-selected
planning tool, conceptually:

    WorkPlanBindRepository {
        session_id,
        eggplan_plan_id
    }

No state-root path parameter is accepted.

Binding requires:

- canonical session workspace;
- no existing active CodeGG WorkPlan for the session;
- valid CodeGG project/repository/workspace relation;
- valid `.eggplan` store;
- subject identity proof;
- Active/Blocked target Plan;
- supported M003 requirement profile;
- no other live CodeGG binding to that repository Plan.

Use the Eggplan M003 projection to construct the CodeGG runtime mirror and
persist mirror + binding/item map transactionally in SQLite.

Do not call legacy `create_active` in a way that silently cancels another
active plan. Reject and require the existing plan to be resolved explicitly.

## 9. Supported repository requirement profile

M003 automatic CodeGG execution/closure supports repository requirements whose
provider is:

- unspecified; or
- exactly `epp_codegg_host`;

and whose evidence kind is:

- Test;
- Command;
- DelegatedRun.

Human-judgment criteria may bind and surface
`AwaitingUserJudgment`, but CodeGG M003 does not manufacture HumanJudgment
observations or auto-close them. Explicit out-of-band Eggplan action may
eventually close such a Plan, and CodeGG then reconciles terminal state.

Reject live binding when a criterion requires an explicit different provider
or another unsupported evidence kind.

Reason: Eggplan provider trust is explicit host policy and no repository-global
trusted-provider registry exists. Presence of an observation file is not trust.

Standalone Revision/Commit and Artifact requirement authority remains
deferred.

## 10. Repository projection -> CodeGG mirror

Consume Eggplan's M003
`RepositoryPlanProjectionV1`/equivalent.

Generate new CodeGG WorkPlanId/WorkItemId values and persist the item map.
Never reuse Eggplan IDs as CodeGG storage authority.

Mirror:

- objective;
- plan lifecycle;
- item lifecycle;
- descriptions;
- parent/dependencies;
- blocker/next action.

Legacy WorkAcceptance is display-only for a bound plan:

- criterion statement preserved;
- RequiresUserJudgment only when repository criterion permits it;
- otherwise Unmet;
- never Satisfied from projection.

Repository requirement semantics stay in the binding projection/Eggplan Plan.

Bound assessment reads the repository Plan directly; it does not reconstruct
canonical acceptance semantics from the CodeGG mirror.

## 11. Structural drift and reconciliation

Persist Eggplan M003 `intent_digest`.

Before every bound mutation/assessment-completion boundary:

1. reload repository Plan;
2. recompute projection;
3. compare intent digest;
4. compare repository revision against binding state.

### Same intent, newer repository lifecycle revision

Reconcile CodeGG mirror lifecycle/blocker/next-action from Eggplan and advance
`last_seen_plan_revision`.

### Structural intent digest changed externally

Set binding Conflict and fail closed with
`repository_plan_structure_changed`.

Do not silently rebuild a live runtime mirror whose objective/dependencies/
criteria changed while jobs may be in flight.

### Repository Plan Closed

Require a valid ClosureRecord from `RepositoryStore::get`/closure validation,
then terminalize CodeGG mirror Completed and release binding.

### Repository Plan Cancelled

Terminalize mirror Cancelled and release binding.

### Repository missing/corrupt/revision regresses

Conflict; no legacy fallback.

## 12. Cross-store mutation ordering

Eggplan and CodeGG SQLite cannot commit atomically. Do not claim they can.

For a bound item lifecycle/blocker/next-action update:

1. validate caller authority and CodeGG expected item revision;
2. load/reconcile binding and repository Plan;
3. construct next Eggplan Plan revision;
4. commit Eggplan `compare_and_swap` first;
5. then update CodeGG mirror + binding last_seen revision in one SQLite
   transaction.

Why repository-first:

- Eggplan is canonical;
- a crash after repository CAS is recoverable by mirror reconciliation;
- CodeGG-first could expose runtime progress that never became canonical.

If step 5 fails or process crashes:

- next load sees Eggplan revision > last_seen;
- if intent digest matches, reconcile lifecycle and return Synced;
- never roll the repository backward.

All bound `work_plan_update_item` paths must route through this service.
Todo feedback remains one-way through WorkPlan and inherits the same service;
Todo never writes Eggplan directly.

## 13. Evidence writeback

Reuse M002 authoritative verification-spec and execution-subject machinery.

Before bound assessment may complete an item/Plan, run a bounded terminal
evidence sync over mapped CodeGG items.

Persist only durable terminal observations:

- Passed;
- Failed/terminal non-success mapped truthfully.

Do not persist transient InProgress using an observation ID later reused for a
different terminal body.

For each terminal execution observation:

1. resolve native job/attempt/run;
2. require Stable execution subject;
3. require complete materialization where applicable;
4. derive M002 VerificationDigest from durable native spec;
5. translate historical subject to Eggplan `epr_*` only through the proven
   binding;
6. build schema-v2 EvidenceObservation with fixed
   `epp_codegg_host`;
7. append idempotently with `PlanStore::append_observation`.

Observation conflict on same ID/different bytes is a hard error.

### Artifact provenance

When a terminal execution has a durable RunStore record, attach verified
RunStore artifacts as Eggplan ArtifactRef values on that execution
observation:

- artifact ID/opaque handle;
- SHA-256 digest;
- bounded logical path/metadata where allowed;
- size.

Verify artifact record/digest through RunStore before writeback.

This satisfies the roadmap's artifact writeback without inventing standalone
Artifact requirement authority.

Standalone `EvidenceKind::Artifact` and Commit/Revision requirement
resolution remains deferred.

## 14. Bound assessment engine

Extend M002 engine selection with:

    EggplanRepositoryBound

When a live binding exists, this engine takes precedence over transient
M002 `EggplanGit`.

It:

1. reconciles repository lifecycle if needed;
2. syncs terminal CodeGG evidence at mutating/completion boundaries;
3. loads repository observations + supersessions;
4. uses explicit provider policy containing only
   `epp_codegg_host` for M003 automatic assessment;
5. runs `eggplan_core::assess_plan` on the canonical repository Plan;
6. projects the assessment into existing
   `WorkPlanCompletionAssessment`.

Read-only `work_plan_get` must not append evidence. It may inspect repository
state and report `sync_required`/Conflict; mutation/turn-end boundaries own
writeback.

No bound plan falls back to M002 transient assessment when repository access
fails.

## 15. Item completion

Eggplan assessment requires lifecycle + evidence.

When CodeGG attempts to mark a bound item Completed:

1. sync its terminal evidence;
2. assess the repository item requirements;
3. reject completion if evidence is missing/failed/stale/unbound;
4. if complete, CAS repository Plan item to Completed;
5. mirror the new lifecycle into CodeGG.

A model's `status=completed` request remains a proposal, not evidence.

Blocked/InProgress/Actionable transitions map through normal Eggplan transition
rules.

## 16. Guarded Plan completion

Do not use CodeGG M002's unbound
`complete_plan_with_subject_revalidation` as repository closure authority.

For `EggplanRepositoryBound`:

1. reconcile lifecycle;
2. sync terminal evidence;
3. capture current Eggplan repository subject;
4. load effective observations/supersessions;
5. assess repository Plan with fixed CodeGG provider policy;
6. require Complete;
7. build `ClosureCandidate::build`;
8. call `RepositoryStore::finalize_closure`;
9. only after guarded closure succeeds:
   - mark CodeGG mirror Completed;
   - release binding;
   - project Todo/Goal state.

The Eggplan finalizer owns its S1/S2 capture. Do not inject CodeGG's current
subject as finalization authority.

If repository closure succeeds but CodeGG SQLite terminalization fails/crashes,
restart reconciliation sees a valid Closed Plan + ClosureRecord and finishes
the mirror transition.

## 17. Cancellation

Cancelling a bound CodeGG WorkPlan is also a canonical repository lifecycle
operation.

Repository CAS to Cancelled first, then mirror Cancelled/release.

Do not merely unbind an Active Eggplan Plan and leave ambiguous runtime
ownership.

M003 does not expose a general "detach while active" operation.

## 18. WorkOrder binding

A WorkOrder may carry an explicit host-authorized Eggplan Plan binding request
through `work_order_eggplan_binding`.

Constraints for M003:

- `repeat_count == 1`;
- target Plan is validated when binding is authored and again when occurrence
  materializes;
- repository identity/intent digest are pinned in the request;
- the occurrence must resolve to the same CodeGG repository and Eggplan
  repository identity;
- no silent fallback to an unbound session.

### Workspace-policy limitation

Mutation-capable `AutoIsolated` resolves to `UseManagedWorktree`. A managed
worktree does not reliably carry the repository-local untracked `.eggplan`
administrative root.

Therefore M003 WorkOrder Plan binding supports only occurrence materialization
that resolves to:

- ShareReadOnly; or
- ShareSerialized.

If it resolves to `UseManagedWorktree`, move the occurrence to Attention with
a bounded diagnostic such as
`eggplan_binding_requires_shared_repository_state`.

Do not copy `.eggplan` into a managed worktree.

A future explicit shared-state/materialization contract may lift this
restriction.

### Materialization ordering

After the coordinator resolves the concrete shared workspace but before the
initial turn/job is submitted:

1. re-open/validate `.eggplan`;
2. prove identity/subject binding;
3. revalidate Plan lifecycle + intent digest;
4. create the occurrence session's CodeGG mirror/binding;
5. only then submit the normal initial turn.

Failure records occurrence Attention; it never launches unbound.

WorkOrder still decides *when* a session is born. Eggplan does not become a
release gate or scheduler.

## 19. Runtime owner refs

CodeGG owner_run_id/owner_job_id remain runtime mirror provenance only.

Do not write them into Eggplan Plan structure.

Evidence invocation/native metadata may carry bounded host references where
normal EvidenceObservation rules permit them.

## 20. Restart/recovery matrix

Test at minimum:

- clean restart with Synced binding;
- crash after Eggplan item CAS before CodeGG mirror CAS;
- crash after Eggplan closure before CodeGG terminal mirror update;
- external lifecycle-only repository update;
- external structural update -> Conflict;
- repository missing;
- repository corruption;
- workspace/repository rebind mismatch;
- Eggplan epr identity mismatch;
- duplicate active binding attempt;
- stale CodeGG item revision;
- observation append retry;
- observation same-ID/different-content conflict;
- WorkOrder occurrence restart before/after binding creation.

Recovery must never reconstruct canonical repository state from the CodeGG
mirror.

## 21. Static guards

Add a repository-binding ownership guard proving:

- `RepositoryStore::open` production use is confined to the approved
  application binding module;
- `eggplan-repo` is absent from `codegg-core`;
- no blind SubjectRevision repository_id replacement exists outside the
  validated translator;
- bound completion cannot call ordinary CodeGG Completed transition without
  guarded Eggplan closure;
- bound WorkPlan item mutations cannot bypass the binding service;
- no code copies `.eggplan` into managed worktrees.

Preserve existing M002 assessment boundary guards.

## 22. Public/protocol surface

Add an explicit host-facing bind operation with:

- session ID;
- Eggplan PlanId.

Optionally add WorkOrder bind/unbind-at-authoring operations as part of the
existing WorkOrder protocol, but the model-facing WorkOrder tool must not be
able to substitute arbitrary repository paths or provider policy.

Expose bounded binding status in WorkPlan projections:

- unbound;
- synced;
- needs_reconcile;
- conflict;
- released;
- Eggplan PlanId/revision;
- no absolute state-root path.

## 23. Required tests

### Binding/identity

- valid same-workspace bind;
- dirty same-subject bind;
- revision mismatch rejects;
- dirty digest mismatch rejects;
- epr mismatch rejects;
- no active CodeGG plan prerequisite;
- Draft/Closed/Cancelled rejects;
- duplicate active binding rejects.

### Projection

- all item lifecycle mappings;
- item mapping stable across restart;
- intent digest drift detection;
- rich criteria remain repository authority;
- CodeGG acceptance never fabricates Satisfied.

### Cross-store lifecycle

- repository-first item CAS + mirror success;
- injected mirror failure followed by reconcile;
- structural external change conflicts;
- lifecycle-only external change reconciles;
- cancellation repository-first.

### Evidence

- passing Test/Command/Delegated terminal writeback;
- failed terminal writeback;
- stale/drifted/incomplete evidence not appended as authoritative pass;
- verification mismatch cannot complete;
- retry append idempotence;
- RunStore artifact refs/digests attached to terminal observation;
- unsupported standalone Artifact/Commit requirement rejected at binding.

### Closure

- all completed + evidence -> guarded Eggplan closure -> mirror Complete;
- worktree drift in Eggplan finalizer leaves repository Active and mirror
  non-terminal;
- crash after repository closure reconciles mirror;
- Closed Plan without valid closure is conflict/corrupt, never Complete.

### WorkOrder

- one-shot Shared/Serialized binding materializes;
- repeat_count > 1 rejected;
- AutoIsolated mutation -> Attention/no initial turn;
- identity/intent changed before occurrence -> Attention/no initial turn;
- restart preserves WorkOrder binding request and occurrence correlation.

### Regression

- unbound M002 EggplanGit behavior unchanged;
- legacy non-Git/unsupported paths unchanged;
- Goal/Todo/checkpoint/context epoch;
- WorkOrder scheduling/gates;
- scheduler/job/worktree authority;
- v67 historical DB migration -> v68;
- current CodeGG full hosted suite.

## 24. Verification commands

At minimum:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test -p codegg-core --lib -- work_plan
    cargo test -p codegg-core -- migration
    cargo test --test work_plan_eggplan_differential
    cargo test --test work_plan_projection_arbiter
    cargo test --test work_plan_resolved_evidence
    cargo test --test long_horizon_trajectory_qualification
    cargo test --test scheduler_authority_matrix
    cargo test --test eggwork_remote_execution
    python3 scripts/check_execution_subject_ownership.py
    python3 scripts/check_execution_ownership.py
    python3 <new repository-binding ownership guard>
    bash scripts/check-core-boundary.sh
    ./scripts/verify.sh quick
    git diff --check

Add focused repository-binding/WorkOrder tests and exact commands to closure.

Hosted canonical main/live qualification is required.

## 25. Documentation

Update:

- `architecture/work_plan.md`;
- `architecture/work_orders.md`;
- `architecture/identity.md` or project identity storage doc;
- `architecture/testing.md` if new hosted fixtures are needed;
- Eggplan integration roadmap/registry.

Document explicitly:

- Eggplan canonical vs CodeGG mirror;
- v68 binding tables;
- identity proof;
- repository-first cross-store ordering;
- conflict/recovery semantics;
- WorkOrder shared-state limitation;
- deferred standalone Artifact/Commit authority.

## 26. Acceptance criteria

M003 closes when:

1. a session can explicitly bind an existing Active/Blocked repository Plan;
2. binding proves CodeGG and Eggplan repository identities against the same Git
   state before translation;
3. v68 binding/item mappings survive restart;
4. Eggplan remains canonical and CodeGG mirror divergence is detectable/
   recoverable;
5. bound item mutations write Eggplan first and cannot bypass repository CAS;
6. terminal CodeGG execution evidence writes idempotent schema-v2 observations
   with authoritative verification binding;
7. terminal RunStore artifact provenance is attached without adding standalone
   Artifact authority;
8. bound assessment reads canonical repository state;
9. bound completion closes through `RepositoryStore::finalize_closure`
   before CodeGG mirror completion;
10. one-shot WorkOrder binding materializes only on supported shared repository
    workspace semantics and never silently falls back;
11. `codegg-core` remains Eggplan-dependency-free;
12. unbound M002 behavior is unchanged;
13. hosted canonical CI/live qualification passes;
14. both repositories record exact implementation and closure revisions.

## 27. Stop conditions

Stop and report if:

- repository identity can only be established by blind ID relabeling;
- arbitrary Eggplan state-root paths are required;
- cross-store correctness would require pretending SQLite + repository files
  are one atomic transaction;
- managed worktrees would require copying `.eggplan`;
- existing repository criteria must be flattened/lost to run;
- CodeGG would need to bypass guarded Eggplan closure;
- provider trust would need to be inferred from observation files;
- one WorkOrder Plan binding would require multiple occurrences against one
  canonical Plan;
- a new standalone Artifact/Commit trust model becomes necessary to close the
  milestone.

## 28. Closure evidence

Create:

    plans/closure/eggplan-assessment-integration/004-m003-status.md

Record:

- CodeGG implementation SHA(s);
- Eggplan reverse-projection implementation/pin;
- v68 schema/migration evidence;
- subject identity proof matrix;
- binding/reconciliation crash matrix;
- evidence writeback + artifact-reference matrix;
- guarded closure end-to-end evidence;
- WorkOrder shared/serialized/auto-isolated matrix;
- before/after dependency graph;
- static ownership guard outputs;
- exact hosted canonical run IDs;
- Eggplan-side closure/reconciliation SHA;
- residual limitations and next milestone disposition.
