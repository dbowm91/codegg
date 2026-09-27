# Eggplan Assessment Integration M002 — Staged Production Assessment Adoption

Status: implemented (closed; see
`plans/closure/eggplan-assessment-integration/003-status.md`)

Repository baseline:

- CodeGG: `6ad127c9913ee6999f44295db2fc03f8b3e5063b`
- provenance implementation/closure: `418fdc85656e7e1faa57f71e5e7f10f7f4859c60`
- provenance corrective implementation: `91b2bc7bf4276d01a4662267595e2b241d96f181`
- provenance corrective closure: `88d6831a2d4d1fa7d6d88393f8b81fef00bc96f6`

External Eggplan baseline:

- Eggplan current head reviewed:
  `0d4a6af7adc6f80f975aca1bfe9bae04e2eb27d8`
- pure compatibility bridge implementation:
  `088968bd58680ae2b3741e2f1feb0614e0ff81a0`
- coordinated Eggplan plan:
  `plans/implementation/codegg-integration/002-staged-eggplan-assessment-adoption.md`

Source roadmap:

- `plans/subsystems/eggplan-assessment-integration-roadmap.md`

Hard predecessor:

- M001 durable execution-subject provenance — closed.
- C002 AgentRun-link/bridge-fixture corrective — closed.

Operational closure dependency:

- CodeGG CI/test throughput corrective C002 must restore a trustworthy green
  hosted main/live baseline before this milestone may cite hosted CI as closure
  evidence. This is NOT a hard implementation dependency; coding/local
  qualification may proceed in parallel.

Primary class: integration / staged authority adoption

## 1. Objective

Make Eggplan's generic plan/evidence assessment the production assessment
substrate for the supported Git-backed CodeGG WorkPlan path without moving
CodeGG storage, scheduler, Goal/Todo/checkpoint, worktree, or agent-loop
ownership into Eggplan.

M002 must:

1. pin Eggplan to one immutable reviewed revision;
2. derive exact verification identity from authoritative CodeGG execution
   specifications;
3. translate CodeGG's durable current/historical subject records into Eggplan
   subjects without current-worktree historical backfill;
4. run the Eggplan compatibility bridge from a CodeGG application-layer
   assessment facade;
5. preserve `WorkPlanCompletionAssessment` and existing arbiter/tool result
   semantics;
6. differential-test the Eggplan path against the legacy assessor;
7. switch production Git-backed supported-evidence call sites only after no
   permissive delta exists.

M002 does not bind CodeGG sessions to repository-resident Eggplan Plans. That
is M003.

## 2. Why the integration lives above codegg-core

Current architecture separates:

- `codegg_core::work_plan::assess_work_plan` — pure/store-independent legacy
  assessment over `WorkPlanEvidenceSnapshot`;
- `src/work_plan_evidence.rs::assemble_resolved` — application-layer durable
  job/AgentRun/status/subject resolution;
- `egggit::capture_git_source_subject` — current exact source capture;
- SQLite/JobStore/AgentRun authority — application/runtime ownership.

Eggplan exact assessment needs both:

- the current exact subject against which the plan is being judged; and
- historical attempt-scoped subjects plus authoritative native execution data.

Do NOT inject SQLite, JobStore, Git capture, or scheduler authority into
`codegg-core` merely to call Eggplan.

Add a narrow application-layer module/facade, recommended:

    src/work_plan_eggplan.rs

with an API conceptually like:

    assess_work_plan_with_eggplan(
        pool,
        workspace_root,
        plan,
        items,
    ) -> Result<EggplanBackedAssessment, AssessmentAdapterError>

The facade may depend on Eggplan crates. `codegg-core` remains usable without
Eggplan repository/runtime dependencies.

## 3. Dependency boundary

Pin one exact immutable Eggplan Git revision in CodeGG.

Approved production dependency surface:

- `eggplan-core`;
- `eggplan-codegg-compat`.

Preferred placement:

- root `codegg` package/application layer only.

Do NOT add production dependencies on:

- `eggplan-repo`;
- `eggplan-cli`;
- `eggplan-projection`;
- `eggplan-markdown`;
- `eggplan-integrations`.

Do not use an Eggplan branch dependency.

Both Eggplan crates must resolve from the same exact revision. Record the pin
in closure evidence.

If Cargo feature unification or packaging requires `eggplan-core` to be
visible transitively through `eggplan-codegg-compat`, avoid a duplicate
direct dependency unless CodeGG needs public core types itself.

## 4. Current-subject authority

Historical evidence subject authority is already persisted by M001.

M002 additionally needs the exact *current* source subject to decide whether
historical evidence applies now.

### Current assessment capture

For Git-backed assessment:

1. resolve the canonical CodeGG workspace root;
2. capture current subject S1 through the governed `egggit` subject API;
3. construct Eggplan `SubjectRevision` from the resulting CodeGG subject
   fields;
4. resolve evidence and run Eggplan assessment against S1.

Never construct current subject from HEAD alone when dirty.

Never use current subject to fill a historical evidence record.

### Completion revalidation

A read-only display may report the assessment based on S1.

Any CodeGG lifecycle transition that treats the assessment as allowing
completion must re-capture S2 immediately before the status transition/CAS and
require S2 == S1.

If S2 differs or capture fails:

- do not mark the WorkPlan complete;
- return a stable diagnostic such as `subject_changed_before_completion`;
- preserve the existing plan revision/status;
- no current subject is written into historical evidence.

This is bounded revalidation, not a global filesystem transaction.

## 5. Supported M002 evidence subset

Eggplan-backed production assessment in M002 covers:

- `TestJob`;
- `SchedulerJob`;
- `DelegatedRun`;
- `AgentRun`.

Current CodeGG host resolution leaves `Artifact` and `Commit` unavailable.
Do not invent new authority for them in this milestone.

If a plan contains only the supported execution evidence kinds (or human
judgment criteria), it is eligible for the Eggplan engine.

If a plan contains an unsupported evidence kind:

- choose the explicit legacy compatibility engine for the whole assessment;
- emit an engine diagnostic identifying the unsupported kind;
- do not mix legacy and Eggplan satisfaction decisions inside one criterion.

A later bounded milestone may add authoritative Artifact/Commit adapters.

## 6. Explicit assessment-engine selection

Introduce an explicit result/diagnostic enum, conceptually:

    AssessmentEngine {
        EggplanGit,
        LegacyNonGit,
        LegacyUnsupportedEvidence,
    }

Rules:

### EggplanGit

Required when:

- workspace is Git-backed;
- current subject capture succeeds;
- all non-human evidence kinds are supported by M002.

Missing/stale/drifted historical subject or verification identity does NOT
fall back to legacy. It remains fail-closed inside Eggplan assessment.

### LegacyNonGit

Allowed only when CodeGG can positively establish that the workspace is not a
Git subject supported by M002.

Do not classify a Git capture error as NonGit.

### LegacyUnsupportedEvidence

Allowed only when the WorkPlan contains a currently unsupported evidence kind
(`Artifact`/`Commit`).

The engine choice must be observable in diagnostics/tests and deterministic
from structured host state.

## 7. Canonical verification specification

Execution-derived evidence must carry an Eggplan `VerificationDigest`.

Create a versioned CodeGG-native canonical specification, conceptually:

    CodeggVerificationSpecV1

Serialize using Eggplan's canonical JSON/digest rules and wrap the resulting
`sha256:<lowercase-hex>` as `VerificationDigest`.

Do not hash arbitrary display strings or ref IDs.

Do not include:

- credentials;
- environment secrets;
- timestamps;
- lease IDs;
- attempt IDs as semantic identity;
- progress/log output;
- mutable labels;
- transient node address.

### TestJob

Derive from the durable `JobRecord` execution semantics actually used:

- variant/version;
- canonical argv;
- normalized workspace-relative cwd;
- test scope where semantically relevant;
- effective timeout/policy fields that change what was verified;
- execution target class when it changes execution semantics.

The display `command` field alone is not authoritative when argv exists.

### SchedulerJob

Support only durable payload variants whose executed specification can be
reconstructed exactly.

At minimum qualify current executable forms used by WorkPlan evidence:

- `ManagedArgv`;
- canonical `Shell` with explicit argv;
- `Python` using script/source digest plus args/cwd/mode/effective timeout;
- `Git` where the durable argv/cwd represents the actual invocation.

If a SchedulerJob payload cannot reconstruct exact execution semantics, return
`verification_unavailable`; do not hash its job ID.

### DelegatedRun / AgentRun

Use durable delegated execution specification, not the evidence ref alone.

For `SubagentRun`, canonical fields should include bounded/hash forms of:

- prompt content digest, not raw prompt in persisted Eggplan metadata;
- agent/model selection;
- parent identity where semantically part of the delegated operation;
- denied/allowed tool/path policy;
- max tool calls;
- task/run/delegation identity as provenance fields where needed;
- immutable base commit when present.

The canonical verification spec may contain a hash of sensitive/large
execution input but must not expose the original content.

Legacy `Subagent` payloads that cannot prove the same durable specification
must remain verification-unavailable unless an exact reconstruction is
demonstrated.

## 8. Resolved evidence -> Eggplan observation adapter

Extend the application integration layer; do not mutate M001's historical
subject records.

For each supported reference:

1. obtain `ResolvedWorkEvidence` from `assemble_resolved`;
2. require the authoritative terminal/native object needed to derive the
   verification spec;
3. require Stable historical source provenance for terminal passing/failed
   execution evidence;
4. project `ExecutionSubjectRevision::to_eggplan_fields()` into Eggplan
   `SubjectRevision`;
5. derive the verification digest;
6. create a bounded Eggplan `EvidenceObservation`;
7. return the same digest as the bridge's expected verification binding.

Use one fixed trusted provider identity owned by CodeGG, e.g.
`epp_codegg_host`, with an explicit descriptor allowing only:

- Test;
- Command;
- DelegatedRun.

Native payload data cannot select the provider ID or widen allowed kinds.

### Observation status

Map host terminal state truthfully:

- Passed -> Eggplan Passed;
- Failed -> Eggplan Failed;
- InProgress -> Eggplan InProgress only if the subject semantics are valid for
  non-terminal display; it cannot satisfy completion;
- unavailable/drifted/unsealed/incomplete materialization -> do not fabricate
  a passing observation.

For exact-subject completion, terminal observation subject must equal current
S1.

Use deterministic observation IDs derived from stable native identity +
attempt/generation + semantic kind, not random IDs on each assessment.

Use the native terminal timestamp when available; do not make repeated
read-only assessments look like new observations by stamping "now".

## 9. Plan snapshot mapping

Create the `CodeggPlanSnapshot` input for `eggplan-codegg-compat` directly
from current CodeGG `WorkPlan` + `WorkItem` values.

Preserve M001 compatibility rules:

- plan/item source IDs preserved as source identity;
- source revision is provenance only;
- owner run/job IDs are provenance only;
- serialized `Satisfied` is never host evidence;
- Completed source status does not manufacture Eggplan closure;
- dependencies remain exact;
- bounds/errors fail closed.

Only active/blocked CodeGG plans enter live Eggplan assessment.

Already-terminal CodeGG Completed/Cancelled records remain CodeGG lifecycle
history and are not retroactively re-opened/re-assessed by M002.

## 10. CodeGG-facing assessment facade

Return CodeGG's existing public DTO:

    WorkPlanCompletionAssessment

plus bounded diagnostics:

    EggplanBackedAssessment {
        assessment: WorkPlanCompletionAssessment,
        engine: AssessmentEngine,
        eggplan_completion_family: Option<...>,
        eggplan_reason_codes: Vec<String>,
        mapping_digest: Option<String>,
        subject: Option<...>,
    }

Do not expose Eggplan storage/repository handles to callers.

### Family -> DTO projection

Preserve the existing CodeGG variants:

- Complete;
- ActionableWorkRemaining;
- Blocked;
- AwaitingUserJudgment;
- InFlight.

Use the Eggplan completion family as the decision family for `EggplanGit`.

Populate CodeGG-specific detail from authoritative CodeGG plan/evidence state:

- actionable item ID/description/next action from the source WorkItem;
- blocker from source structured blocker;
- in-flight handle kind/ID from resolved CodeGG evidence/owner provenance;
- awaiting-user reasons from bounded source criteria + Eggplan reason codes.

Do not let free-form Eggplan diagnostic text become scheduler authority.

## 11. Production call-site migration

Switch application/runtime call sites that have DB/workspace context to the
new facade.

At minimum audit and migrate:

- `src/work_plan_arbiter.rs`;
- `src/tool/work_plan.rs`;
- root integration paths that currently call
  `codegg_core::work_plan::assess_work_plan` after assembling host evidence.

Do not force pure `codegg-core` unit tests or callers without repository
context to acquire a database/workspace solely to preserve the same helper
name.

The old core assessor remains:

- compatibility API for pure callers;
- differential oracle during M002;
- explicit fallback implementation for LegacyNonGit /
  LegacyUnsupportedEvidence.

After positive M002 closure, production Git-backed supported-evidence paths
must not call the legacy assessor directly.

Add a static/source guard for those production call sites.

## 12. Differential qualification

Before switching production authority, run both engines over the same
structured cases.

Required matrix:

- no evidence;
- passing TestJob;
- failed TestJob;
- missing TestJob;
- in-flight Test/Scheduler job;
- blocked item;
- dependency-gated item;
- human-judgment-only item;
- forged serialized Satisfied disposition;
- stable exact subject;
- stale historical subject;
- current subject changed before completion CAS;
- historical subject drift/unavailable;
- materialization incomplete;
- verification digest missing;
- verification digest mismatch;
- completed item without host proof;
- multiple items/dependencies;
- DelegatedRun;
- positively linked AgentRun;
- dangling AgentRun link;
- unsupported Artifact;
- unsupported Commit;
- non-Git workspace.

### Permissiveness rule

For any `EggplanGit` case:

- new `allows_completion == true` while legacy says false is a hard stop;
- new Complete while legacy requires continuation/block/wait is a hard stop;
- stricter Eggplan outcomes are allowed only when attributable to documented
  subject/verification authority that legacy did not model.

Record every intentional stricter delta in closure evidence.

## 13. Completion transition race test

Add an end-to-end test that:

1. creates a Git-backed active WorkPlan with passing exact-subject evidence;
2. Eggplan assessment at S1 returns completion-eligible;
3. mutate source before CodeGG status transition;
4. S2 differs;
5. CodeGG does not complete the plan;
6. no evidence is rewritten;
7. re-assessment reports stale/missing applicability until new evidence exists.

Also test stable S1/S2 completion.

## 14. Provider trust

Do not reuse CodeGG's LLM `ProviderRegistry`; that is a different domain.

Construct Eggplan's evidence `ProviderRegistry` locally with the fixed
CodeGG-host evidence descriptor.

Trust is host code/configuration, never serialized WorkPlan evidence text.

No model/provider plugin may self-register as trusted evidence authority.

## 15. Storage and migration

No new CodeGG storage migration is expected in M002.

Reuse:

- v67 JobAttempt source subject;
- current durable JobRecord payload;
- AgentRun job+attempt link;
- existing WorkPlan schema.

If exact verification derivation proves impossible without storing an
additional execution-semantic field, STOP and register a bounded storage
follow-up rather than silently changing the M002 schema scope.

Do not alter Eggplan persisted schemas.

## 16. Failure semantics

### Current Git capture fails

For a known Git-backed workspace: return fail-closed assessment error / no
completion. Do not route to LegacyNonGit.

### Historical subject missing/drifted

EggplanGit remains selected; evidence is unavailable/stale. No legacy
satisfaction fallback.

### Verification derivation fails

Evidence remains unbound/unavailable. No ref-ID fallback.

### Eggplan bridge rejects mapping

Return bounded adapter error and preserve plan state. Do not automatically
treat the plan as complete.

### Unsupported evidence kind

Select LegacyUnsupportedEvidence for the whole assessment and record the
diagnostic.

## 17. CI-stability operational gate

The unrelated CI/test-throughput corrective currently owns the unstable hosted
baseline.

M002 implementation may proceed in parallel and may use:

- focused local tests;
- `./scripts/verify.sh quick`;
- targeted hosted runs for debugging.

M002 MUST NOT close while the repository has no trustworthy canonical green
hosted baseline.

Closure requires either:

1. CI corrective C002 has closed with its required stable hosted evidence and
   a subsequent M002 exact-head canonical run is green; or
2. the CI corrective explicitly proves the failures are unrelated and records
   an accepted equivalent canonical qualification path.

Do not cite failed run `36215541015` as M002 qualification.

## 18. Dependency pin and release policy

Pin Eggplan exactly.

Record:

- Eggplan repository URL;
- exact revision;
- packages consumed;
- transitive dependency delta.

Do not widen CodeGG's MSRV/toolchain policy for this integration unless
required and separately approved.

Future crates.io/release packaging of Git dependencies is outside M002; record
it as a distribution follow-up if relevant.

## 19. Ordered work packages

### WP1 — Pin pure Eggplan dependencies and boundary guard

Add exact-revision dependencies, prove no eggplan-repo/runtime persistence
enters CodeGG, and add a static dependency guard.

### WP2 — Verification-spec canonicalization

Implement `CodeggVerificationSpecV1` and golden digest tests for supported
native execution shapes.

### WP3 — Eggplan evidence adapter

Convert `assemble_resolved` + Job/AgentRun native records into bounded
observations with exact subject and verification binding.

### WP4 — Application-layer assessment facade

Capture current subject, construct snapshot/provider policy, invoke
`assess_codegg_snapshot`, project to `WorkPlanCompletionAssessment`, and
emit bounded diagnostics/engine identity.

### WP5 — Differential qualification

Run legacy vs Eggplan matrices, fix only non-permissive compatibility deltas,
and freeze golden cases.

### WP6 — Production migration and completion revalidation

Move arbiter/tool Git-backed supported-evidence paths to the facade; add S2
revalidation before completion transition and source guards preventing direct
legacy-assessor bypass.

### WP7 — Cross-repo closure

Recheck Eggplan head/bridge, update both repositories' planning/architecture
records, obtain trustworthy hosted qualification, and close the coordinated
M002.

## 20. Required tests

At minimum:

### Dependency/boundary

- exact Eggplan revision pin;
- no eggplan-repo/cli/projection/markdown dependency;
- codegg-core remains free of DB/Git capture caused by this integration.

### Verification digest

- deterministic Test digest;
- cwd/argv semantic change changes digest;
- irrelevant timestamp/attempt/lease changes do not;
- Shell without canonical argv fails closed;
- Python source digest participates;
- SubagentRun policy/prompt digest changes verification identity;
- ref/job ID alone cannot create a digest.

### Evidence adapter

- stable exact TestJob;
- failed TestJob;
- stale subject;
- drifted subject;
- materialization incomplete;
- missing verification spec;
- positively linked AgentRun;
- dangling AgentRun;
- observation ID/timestamp deterministic across repeated read.

### Assessment

- complete;
- actionable;
- blocked;
- user judgment;
- in flight;
- forged Satisfied ignored;
- dependency-gated;
- unsupported evidence chooses explicit legacy engine;
- non-Git chooses explicit legacy engine;
- Git capture failure never chooses legacy.

### Race

- S1/S2 stable completes;
- S1/S2 changed does not complete.

### Regression

- Goal/Todo/checkpoint/context-epoch tests;
- WorkOrder/scheduler authority;
- long-horizon trajectory;
- v67 migration/restart;
- Eggwork execution;
- subject ownership guard.

## 21. Verification commands

At minimum, using current equivalent names if repository drift requires:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test -p codegg-core --lib -- work_plan
    cargo test --test work_plan_resolved_evidence
    cargo test --test work_plan_projection_arbiter
    cargo test --test long_horizon_trajectory_qualification
    cargo test --test scheduler_authority_matrix
    cargo test --test eggwork_remote_execution
    cargo test -p codegg-core -- migration
    python3 scripts/check_execution_subject_ownership.py
    python3 scripts/check_execution_ownership.py
    bash scripts/check-core-boundary.sh
    ./scripts/verify.sh quick
    git diff --check

Add focused M002 tests/guards to this list in closure.

Hosted closure evidence must be collected only after the operational CI gate in
section 17 is satisfied.

## 22. Documentation updates

Update:

- `architecture/work_plan.md`;
- `architecture/git.md` only if current-subject assessment semantics need
  clarification;
- `architecture/testing.md` for hosted qualification dependency if needed;
- `plans/subsystems/eggplan-assessment-integration-roadmap.md`;
- `plans/registry.md`.

Document:

- application-layer ownership;
- engine selection;
- verification-spec v1;
- current-subject S1/S2 completion revalidation;
- explicit legacy compatibility boundaries;
- Eggplan dependency pin.

## 23. Acceptance criteria

M002 closes only when:

1. CodeGG consumes one immutable Eggplan revision using only pure assessment
   crates;
2. supported execution evidence receives canonical verification binding;
3. historical subjects come only from durable attempt provenance;
4. current assessment subject is governed CodeGG capture, never HEAD-only;
5. completion transitions revalidate S2 before CAS;
6. production Git-backed supported-evidence arbiter/read paths use the Eggplan
   facade;
7. unsupported/non-Git legacy paths are explicit and diagnostic;
8. no differential case becomes more permissive;
9. WorkPlanStore, Goal, Todo, checkpoint/context epoch, scheduler, worktree,
   and agent-loop ownership remain CodeGG;
10. no Eggplan repository state is created;
11. all focused/local verification is green;
12. trustworthy hosted canonical qualification is green after the CI-stability
    operational gate;
13. both repositories record exact implementation/closure revisions.

## 24. Stop conditions

Stop and report if:

- CodeGG cannot reconstruct a supported verification spec from durable native
  records;
- exact verification would require using a ref/job ID as the spec;
- Git capture failure would need a permissive legacy fallback;
- production adoption requires moving SQLite/Git/scheduler authority into
  Eggplan or codegg-core;
- any EggplanGit differential case is more permissive;
- a new persisted field/migration is required but not explicitly planned;
- current subject cannot be revalidated before completion transition;
- CI instability makes hosted closure evidence non-interpretable.

## 25. Closure evidence required

Create:

    plans/closure/eggplan-assessment-integration/003-m002-status.md

Record:

- CodeGG implementation SHA(s);
- exact Eggplan pin;
- consumed package/dependency graph;
- engine-selection matrix;
- verification-spec/digest matrix;
- subject S1/S2 race evidence;
- resolved-evidence adapter matrix;
- differential parity table with all stricter deltas;
- production call-site inventory before/after;
- ownership/static-guard outputs;
- local verification;
- exact hosted workflow IDs after CI stability is restored;
- Eggplan-side plan/registry reconciliation SHA;
- residual findings;
- disposition for M003 repository Plan binding.

## 26. Handoff notes

Do not combine M002 with:

- repository-resident Eggplan Plan binding;
- Artifact/Commit authority expansion;
- CI timing-flake corrective implementation;
- Eggwork M003 transfer optimization;
- WorkPlan storage migration.

The target is one bounded staged adoption: exact Git subject + bound execution
evidence -> Eggplan pure assessment -> existing CodeGG completion DTO and
arbiter behavior.
