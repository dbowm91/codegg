# Agent Run, Async Delegation, and Worktree Concurrency Milestone 004 — Isolated Mutation and Structured Results

Status: closed

Implementation commit: `37b9cc9c9442fbca20fa63072581b4be1067deaf`

Closure record: `plans/closure/agent-run-worktree-concurrency/004-status.md`

Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

Source roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md#m004--automatic-mutation-isolation-child-commits-structured-results-and-integration-handoff`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#18-worktree-native-concurrency`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`

Applicable ADRs:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: capability/invariant

Hard blockers: M002 and M003 closed by their accepted closure records.

## 1. Objective

Make worktree isolation an automatic delegated-run execution property for mutation-capable concurrent children, allow an isolated child to produce a scoped Git commit/checkpoint inside its owned worktree, and return a typed `AgentRunResult` that the parent can evaluate and explicitly integrate.

The parent should delegate semantically independent implementation work without planning physical file isolation merely to avoid shared-index/working-tree collisions. Integration remains explicit and conflict-aware; this milestone does not silently merge successful child branches.

## 2. Why this milestone becomes ready after M002 and M003

M002 provides durable parent/child control, status, cancellation, and restart-safe completion delivery. M003 provides durable worktree records/leases with safe cleanup and restart reconciliation. Existing reusable foundations include:

- resolved child capability/path authority;
- typed Git mutations, state deltas, conflicts, operation-state recovery, and network/destructive risk classes;
- scheduler resource/exclusivity keys;
- test/build scheduler jobs and RunStore artifacts;
- `SubAgentReport`/terminal-output seams that can be replaced or supplemented by typed result data.

## 3. Current implementation evidence

Reconfirm before editing:

- child runtime currently receives a parent-selected explicit `workspace_root` and does not automatically request isolation;
- `execute_agent_task` applies a hard deny including `commit`, suitable for shared-workspace children but overly restrictive for an owned worktree;
- parent allowed-path scopes are inherited and narrowed, but physical worktree ownership is not yet part of that scope;
- typed Git mutation services can stage/commit and return pre/post snapshots/state deltas;
- Git network/destructive operations are separately classified and guarded;
- child terminal result is primarily text, with optional `SubAgentReport` parsed opportunistically from final text;
- scheduler supports worktree/repository mutation exclusivity and test/build jobs that can validate child work.

## 4. Invariants that must not regress

- A mutation-capable delegated run that may execute concurrently must not share the parent/sibling working tree/index unless an explicit serial compatibility mode proves no concurrency.
- Worktree allocation occurs before child `AgentLoop` construction; the loop’s immutable workspace root is the leased worktree.
- Read-only children do not gain write/Git authority merely because they reuse a parent workspace.
- Worktree ownership never widens inherited `GitWrite` or filesystem permissions.
- Child commit authority is restricted to the owned worktree and inherited Git capability.
- Push, force push, destructive history rewrite, arbitrary clean/reset, remote/config credential mutation, and parent/sibling worktree mutation remain separately denied/authorized.
- Parent integration is explicit and uses typed Git operations; completion alone never mutates the parent branch.
- A child result is machine-typed and bounded; arbitrary final prose is not the sole authority for commit/diff/test state.
- Failed/dirty/conflicted worktrees are retained according to M003 policy until safe explicit cleanup.
- Shared build caches/ports/databases remain scheduler-controlled even with separate worktrees.

## 5. Scope

### In scope

- classify delegated run as read-only versus mutation-capable from resolved capability/tool surface/effect policy, not model-provided claims;
- default allocation policy: concurrent mutation-capable child -> new managed worktree lease; read-only child -> inherit parent workspace/worktree where safe;
- bind leased `WorktreeId`/path/base commit into durable run execution context before loop construction;
- restrict child path permissions to the leased worktree even when parent scope is broader;
- conditionally remove the blanket child `commit` hard deny only for an owned worktree plus inherited `GitWrite` authority;
- provide a bounded checkpoint/commit operation appropriate to child execution, using existing Git mutation service;
- define/persist `AgentRunResult` and validation-result DTOs;
- capture base commit, result commit, changed paths, repository state, validation jobs/results, findings, artifacts, conflict/dirty state, retryability/recovery hint;
- provide explicit parent integration/handoff operations using existing typed merge/rebase/cherry-pick or commit application strategy selected by current Git architecture;
- preserve worktree after failed validation/conflict for inspection;
- update prompts/tool descriptions so children/parents understand isolation is automatic and do not waste turns creating worktrees manually.

### Explicitly out of scope

- automatic merge of every successful child;
- unrestricted child push/network Git;
- child-created arbitrary persistent branches outside managed naming/lifecycle;
- free-form conflict auto-resolution beyond ordinary agent edits in the relevant owned/integration worktree;
- group joins/fan-out policies beyond single-run compatibility (M005);
- arbitrary rewind;
- remote worktrees.

## 6. Required production changes

### Delegation/run preparation policy

Add a host-owned policy function based on the fully resolved child execution surface, for example:

```text
ReadOnly -> inherit parent execution root if policy permits
MutationCapable + isolated run available -> acquire WorktreeLease
MutationCapable + isolation unavailable -> queue/fail explicitly; do not silently fall back to shared concurrent mutation
```

Do not determine mutability by agent name alone. Use resolved capability/effect/tool authority. If classification is ambiguous, choose isolation for safety when the child can write filesystem/Git state.

The scheduler/run executor should acquire the worktree during `Preparing` before starting the model. Preparation failure leaves a typed run failure/retryable state and no child model call.

### Execution context/path authority

For isolated children:

- set `workspace_root` to leased worktree path at construction;
- attach `WorktreeId`, repository/base commit, and lease generation to run execution context;
- intersect path permissions with the leased root;
- ensure shell/Git/file/snapshot/LSP/test commands resolve against the same leased root;
- reject attempts to target parent/sibling absolute paths even if the original parent workspace was broader.

### Child Git policy

Replace blanket `commit` denial with a contextual policy:

- no owned worktree -> child commit denied;
- owned worktree but no inherited `GitWrite` -> denied;
- owned worktree + `GitWrite` -> stage/commit may be allowed through normal permission/risk policy;
- push/network/destructive history remain independently classified and do not become allowed by the owned-worktree condition.

Commit must use hardened noninteractive Git policy. If hooks/signing require unsupported interaction, return a typed failure and leave the worktree intact rather than bypassing repository policy silently.

### Checkpoint and result contract

Add a typed result domain, roughly:

```rust
pub struct AgentRunResult {
    pub run_id: AgentRunId,
    pub status: AgentRunResultStatus,
    pub summary: String,
    pub worktree_id: Option<WorktreeId>,
    pub base_commit: Option<String>,
    pub result_commit: Option<String>,
    pub changed_paths: Vec<String>,
    pub validation: Vec<ValidationResultRef>,
    pub findings: Vec<AgentRunFinding>,
    pub artifacts: Vec<ArtifactRef>,
    pub repository_state: RunRepositoryState,
    pub retryability: Retryability,
    pub recovery_hint: Option<String>,
}
```

Keep collections/strings bounded and store large diffs/logs as artifacts. Populate changed paths/commit/state from Git services rather than trusting child prose.

A run may complete successfully without a commit when it is read-only or explicitly returns analysis. A mutating implementation run should normally checkpoint/commit before reporting success unless the task/policy states otherwise.

### Validation integration

Allow child validation to submit existing scheduler-backed test/build/lint/format jobs scoped to the child worktree. Record job/run/artifact references and concise outcomes in `AgentRunResult`.

Do not duplicate TestRunner semantics in the agent-run service.

### Explicit integration/handoff

Provide a typed parent-side operation that accepts a completed result/run and verifies:

- worktree lease/result belongs to the expected repository/root lineage;
- base/result commit still exist;
- parent/integration target state is known;
- no prohibited operation is required implicitly.

Use existing typed Git merge/rebase/cherry-pick services. Return structured outcomes including fast-forward/completed/conflict/rejected and recovery hints. Do not auto-resolve conflicts outside normal agent/user flow.

### Prompt/tool surface

Update parent contract to state:

- write-capable delegated children are isolated automatically;
- parent need not pre-partition paths solely for Git collision avoidance;
- child returns a commit/result handle;
- integration is explicit.

Update child contract to state:

- work only inside assigned execution root;
- checkpoint/commit when implementation is complete if allowed;
- do not push/integrate.

Avoid large prompt additions; prefer concise runtime metadata and tool descriptions.

## 7. Ordered work packages

### A — Mutation classification and preparation fixture

Add production-shaped tests showing two write-capable children currently share/serialize a workspace and define the new classification/preparation contract.

Acceptance evidence:

- read-only and mutating surfaces classify deterministically;
- ambiguous writable surface chooses isolation.

### B — Automatic worktree binding

Acquire M003 lease during run preparation and construct child loop against it.

Acceptance evidence:

- two concurrent mutation children have distinct `WorktreeId`, path, `.git` pointer/index;
- child file writes appear only in its worktree;
- parent working tree remains unchanged before integration.

### C — Contextual Git commit authority

Remove blanket hard deny only under owned-worktree/inherited-authority checks.

Acceptance evidence:

- isolated authorized child can commit;
- read-only/shared child cannot;
- push/force/reset broad destructive operations remain denied unless existing independent policy explicitly allows them.

### D — Structured result/checkpoint

Populate durable result from actual Git/status/validation/artifact state.

Acceptance evidence:

- successful implementation returns base/result commit and changed paths without parsing final prose;
- failed/no-commit/conflicted cases are represented explicitly.

### E — Validation jobs

Route child tests/build/lint/format through existing scheduler jobs with worktree-scoped execution context and attach concise result refs.

Acceptance evidence:

- validation does not run in parent workspace;
- cancellation of child/run cancels owned validation where appropriate.

### F — Integration/handoff

Add explicit typed integrate operation and conflict reporting.

Acceptance evidence:

- clean child commit can be integrated explicitly;
- conflicting child result returns conflict state/recovery hint and preserves both worktrees;
- completion without integration leaves parent branch untouched.

### G — Docs/prompts/cleanup semantics

Document isolation contract and ensure M003 worktree retention/cleanup respects result/integration state.

## 8. Failure, cancellation, restart, and contention semantics

- Worktree preparation failure: no model execution; run returns typed preparation failure and M003 cleans/reconciles safely.
- Child cancellation with dirty changes: run terminalizes cancelled/interrupted; worktree is retained dirty for inspection unless explicit cleanup policy says otherwise.
- Child completes textually but commit/checkpoint fails: do not report fully successful implementation; return partial/failed result with dirty state and recovery hint.
- Commit succeeds but result persistence fails: restart reconstructs result commit/state from durable run/worktree/journal/Git facts; never repeat the commit blindly.
- Validation timeout/failure: represent in result; whether run is failed or completed-with-failing-validation must follow task policy and be explicit.
- Parent changes while child runs: integration compares current parent target with child base/result and uses typed Git outcome; do not assume fast-forward.
- Two child integrations contend: scheduler repository mutation exclusivity serializes integration operations; conflicts are Git-semantic, not filesystem races.
- Daemon restart: M001/M002/M003 reconcile run/control/worktree state first; completed commit remains an artifact and is not recreated.

## 9. Compatibility and migration

- Existing serialized/shared mutation behavior may remain as an explicit compatibility fallback only for non-concurrent legacy paths; daemon concurrent mutation must not silently use it.
- Existing read-only subagent behavior should remain cheap and not force worktree creation.
- `SubAgentReport` may remain as a presentation adapter populated from `AgentRunResult`; JSON parsing of arbitrary final text should no longer be required for machine result state.
- Existing Git tool/action names remain; contextual run/worktree checks are additive authority constraints.
- Existing manual worktrees remain unaffected.

## 10. Required tests

### Focused unit tests

- mutation/read-only classification;
- worktree/path authority intersection;
- child commit policy matrix;
- result DTO bounds/serialization;
- integration eligibility checks.

### Integration tests

- two parallel children edit overlapping file names in separate worktrees without physical collision;
- parent tree remains untouched until explicit integration;
- authorized child commit round trip;
- read-only child reuse;
- test/build runs scoped to child worktree;
- clean explicit integration;
- merge/cherry-pick conflict report.

### Restart and recovery tests

- restart after worktree prepared before child start;
- restart with dirty child;
- restart after child commit before result delivery;
- restart after result persistence before integration;
- no duplicate commit on resume.

### Contention and cancellation tests

- multiple children same repository/base;
- child cancellation during validation/commit boundary;
- simultaneous integration requests;
- shared build-cache exclusivity remains enforced.

### Security and negative tests

- child tries parent/sibling path;
- child commit without lease/GitWrite;
- child push/force-reset attempt;
- stale lease generation performing commit/integration;
- forged result/run/repository relation.

## 11. Required verification commands

Expected focused shape after blockers close:

```bash
cargo test --lib agent
cargo test --lib git
cargo test --test worktree
cargo test --test scheduler_contention
cargo test --test scheduler_cancellation
cargo fmt --all -- --check
```

Add narrowly named production-shaped integration tests and run them directly. Run existing scheduler/Git ownership guards when affected. One quick broad pass at closure; no new CI machinery.

## 12. Documentation updates

- `architecture/agent.md` — isolated child construction/result contract.
- `architecture/worktree.md` — automatic delegated-run lease consumer.
- `architecture/git.md` — contextual child commit and explicit integration policy.
- `architecture/scheduler.md` — validation/integration exclusivity.
- agent/subagent prompt contracts and task tool documentation.
- source roadmap milestone status after closure.

## 13. Acceptance criteria

1. Concurrent mutation-capable delegated runs automatically receive distinct managed worktrees before model execution.
2. Read-only children can reuse a parent root without gaining mutation authority.
3. Isolated child filesystem/Git execution is confined to its leased root.
4. Child commit is allowed only with owned lease plus inherited `GitWrite`; push/history rewrite remains independent.
5. Successful mutating child returns machine-derived base/result commit, changed paths, validation evidence, artifacts, and repository state.
6. Child final prose is not the sole source of machine result state.
7. Parent branch is unchanged until an explicit typed integration operation.
8. Integration conflicts return structured Git outcome/recovery information and preserve inspectable worktrees.
9. Cancellation/restart does not delete dirty work or duplicate commits.
10. Focused isolation/security/recovery tests pass.

## 14. Stop conditions

Stop if:

- M002/M003 are not closed;
- automatic isolation would require constructing child loops before worktree ownership is durable;
- enabling commit would bypass existing Git permission/risk classification;
- integration requires ad-hoc shell Git rather than existing typed services;
- the implementation begins automatically merging without an explicit parent/user operation;
- preserving repository-required signing/hooks would require silently disabling them;
- scope expands into run groups or remote worktrees.

## 15. Closure evidence required

- implementation/review commits;
- two-concurrent-mutating-child isolation fixture;
- child commit authority matrix;
- structured result examples backed by actual Git/test facts;
- explicit integration success/conflict fixtures;
- cancellation/restart/dirty retention evidence;
- security negative tests for path/push/history boundaries;
- exact verification results and unresolved findings.

## 16. Handoff notes

The goal is to reduce parent planning overhead, not to create an elaborate branching workflow manager. Let Git worktrees solve physical isolation, let existing typed Git operations solve integration, and keep the agent-run layer focused on ownership and result contracts.
