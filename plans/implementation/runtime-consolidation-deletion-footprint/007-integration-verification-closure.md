# Runtime Consolidation, Deletion, and Footprint M007 — Integration, Verification, and Strict Closure

Status: closed — exact-candidate closure accepted by M009

Source roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/003-planning-process.md`
- `architecture/agent.md`
- `architecture/tool.md`
- `architecture/scheduler.md`
- `architecture/testing.md`
- closure records for M001-M006

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Primary class: integration / closure

Dependencies:

- hard: M001-M006 closed (M001 is now closed; M002-M006 remain outstanding);
- operational: one ordinary existing hosted `CI / verify` run on the exact accepted final candidate;
- no external platform evidence beyond currently supported routine verification is introduced by this milestone.

Target closure record:

- `plans/closure/runtime-consolidation-deletion-footprint/007-status.md`

## 1. Objective

Perform one integration pass over the completed consolidation work, reconcile architecture documentation and planning state, capture final footprint/verification evidence, and determine whether this roadmap can close without another corrective cycle.

M007 should contain almost no production refactor. Any substantive defect discovered here should normally become a narrowly scoped corrective plan rather than being hidden inside closure work.

## 2. Required predecessor evidence

Before implementation begins, confirm closure records exist for:

- M001 scheduler compatibility deletion/convergence;
- M002 structured outcome/recovery convergence;
- M003 AgentLoop decomposition;
- M004 prompt/provider/history legacy deletion;
- M005 verification-ratchet retirement;
- M006 measured dependency/binary cleanup.

Each predecessor record must identify unresolved findings by severity. M007 must not mark the roadmap closed while a critical/high/medium in-scope correctness defect remains unresolved.

## 3. Integration audit

Inspect current repository reality rather than assuming predecessor checklists landed correctly.

At minimum verify these end-state statements directly:

### Scheduling

- production daemon construction has one durable scheduler ownership path;
- no independent background timer loop persists/dispatches production scheduled work;
- no UUID-string-to-`u64` scheduling identity conversion remains;
- compatibility schedule APIs, if retained, adapt to durable services.

### Structured execution and autonomy

- Tool Broker/structured execution results remain authoritative internally;
- known denial/timeout/cancellation/protocol/tool-error statuses do not depend on arbitrary output parsing;
- repeat/equivalence detection uses correct action/effect identity;
- volatile output novelty alone cannot keep a no-effect mutation loop alive indefinitely;
- recovery remains bounded and transport retry remains separate.

### Agent loop ownership

- generic turn orchestration is materially smaller and delegates tool execution/context/provider/recovery ownership;
- no extracted helper simply recreated the same multi-domain god object;
- tool/context/provider boundaries pass structured types rather than rendered strings.

### Prompt/history

- production prompt compilation has one PromptCompiler/runtime-asset path;
- no production process-CWD instruction authority remains;
- remote instructions are refresh/snapshot-owned;
- effective system content is represented before compiler fingerprinting;
- provider-only message repair is projection/adapter-owned where feasible.

### Verification/documentation

- every routine static guard provides unique signal;
- migration ratchets removed in M005 have direct replacement evidence or their forbidden path no longer exists;
- routine CI remains one bounded job;
- architecture docs describe durable ownership/invariants rather than obsolete implementation inventories.

### Footprint/dependencies

- final size/feature measurements are recorded on the consolidated tree;
- no capability was lost for size reduction;
- dependency changes are evidence-backed and MSRV/topology remain intentional.

## 4. Explicit non-goals

Do not:

- create new feature work;
- fix unrelated low-severity cleanup discovered during audit unless it is trivial and required for truthful docs/verification;
- add new CI/release infrastructure for closure evidence;
- rerun every historical test command from prior roadmaps;
- rewrite predecessor closure records to conceal deviations;
- declare closure based only on commits or compilation.

## 5. Required broad verification

Use the repository's existing verification contract. Expected minimum:

```bash
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
cargo check -p codegg --locked --features server,plugins,lsp-test-support
git diff --check
```

If `scripts/verify.sh full` remains the canonical wrapper and already performs the appropriate commands after M005, use it instead of duplicating equivalent commands, while still recording the actual commands/output.

Run focused end-to-end tests for the changed boundaries where not included by default workspace tests, especially retained schedule compatibility and production-feature Tool Broker/MCP paths.

Do not run real external LSP servers unless a predecessor specifically changed real-server compatibility and the environment already provides them.

## 6. Hosted evidence

Obtain one normal existing GitHub Actions `CI / verify` run on the exact final candidate through the repository's standard push/PR trigger.

Record:

- exact commit SHA;
- workflow run ID;
- job ID;
- final conclusion;
- whether the run covered the same default workspace verification contract expected by the repository.

Do not add `workflow_dispatch`, a new branch trigger, matrix, scheduled workflow, artifact upload, or release trigger merely to obtain evidence.

If hosted CI fails due to an in-scope defect, fix through a corrective plan when substantive. If failure is demonstrably unrelated/external, record it truthfully and classify closure accordingly rather than claiming green evidence.

## 7. Final measurement evidence

Carry forward M006 measurements and record final values on the exact accepted M007 candidate:

- default release binary size;
- documented production-feature release size where practical;
- direct dependency/feature summary sufficient to explain material changes;
- before-roadmap vs after-roadmap source-size/deletion summary for the major consolidation surfaces;
- `src/agent/loop.rs` coarse before/after size or LOC;
- number/disposition of static guards removed/retained as contextual evidence, not a target metric.

Do not add these measurements as CI thresholds.

## 8. Documentation and registry reconciliation

Update only current-state documents:

- subsystem roadmap statuses/milestones;
- `plans/registry.md`;
- `architecture/agent.md`;
- `architecture/tool.md` if final structured execution ownership changed wording;
- `architecture/scheduler.md`;
- `architecture/testing.md`;
- other docs directly made stale by M001-M006.

Do not duplicate detailed historical implementation evidence into architecture docs. Closure records and Git preserve history.

When all criteria pass:

- mark roadmap `closed`;
- mark M007 closed;
- move the roadmap out of active registry rows or mark it closed according to current registry convention;
- preserve predecessor implementation/closure records for traceability.

## 9. Closure record requirements

`plans/closure/runtime-consolidation-deletion-footprint/007-status.md` MUST contain:

1. final candidate SHA and implementation commit range/PRs;
2. requirement-to-evidence matrix covering roadmap exit conditions;
3. predecessor closure disposition M001-M006;
4. focused and broad test/guard commands with outcomes;
5. hosted CI run/job evidence;
6. scheduling authority evidence;
7. structured outcome/recovery evidence;
8. AgentLoop ownership/decomposition evidence;
9. prompt/runtime-asset/history ownership evidence;
10. verification guard disposition summary;
11. binary/dependency measurement summary;
12. compatibility/security/storage/protocol assessment;
13. unresolved findings classified critical/high/medium/low/deferred;
14. recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 10. Explicit acceptance criteria

M007 and the roadmap may close only when:

1. M001-M006 each have accepted closure records with no unresolved critical/high/medium in-scope defect.
2. Production scheduling has one durable owner and no independent legacy timer/persistence/dispatch loop.
3. The known legacy scheduling ID defect is structurally impossible.
4. Structured tool status/effect facts are authoritative through recovery wherever known.
5. Recovery equivalence/progress cannot be reset indefinitely by unrelated or volatile text novelty.
6. Semantic recovery remains bounded and does not hide non-idempotent mutation retries.
7. `AgentLoop` has materially reduced multi-domain policy ownership and no replacement god module was created.
8. PromptCompiler/runtime assets are the sole production prompt compilation authority.
9. Production active-turn prompt behavior cannot fall back to process-global CWD.
10. Provider compatibility does not mutate canonical history solely for wire grammar when projection can satisfy the requirement.
11. Static guards retained in routine CI each have distinct current-value justification; closed migration ratchets do not remain unexplained.
12. Routine CI remains one bounded job and release remains manual.
13. Final binary/dependency measurements are captured and show truthful before/after evidence; no feature reduction or major dependency rewrite was used to force a size result.
14. Architecture docs reflect current ownership and contain no known stale predecessor names/field inventories in the touched areas.
15. Focused tests, the broad existing workspace verification contract, production-feature compile check, and `git diff --check` pass on the accepted candidate.
16. One ordinary hosted `CI / verify` run passes on the exact accepted candidate.
17. No new CI matrix/lane, scheduled audit, size gate, dependency bot, release automation, workflow-dispatch mechanism, or fixed release cadence was introduced.
18. The final closure record includes a complete requirement-to-evidence matrix and unresolved finding severity classification.

## 11. Stop and corrective-pass conditions

M007 MUST NOT absorb substantive newly discovered defects.

Create a narrow corrective plan when any of the following is found:

- a second production scheduling owner remains;
- structured execution status is still destroyed at a central boundary;
- AgentLoop extraction introduced a new authority/cancellation bug;
- prompt deletion broke runtime asset or adapter correctness;
- removal of a static guard reopened a security/authorization bypass;
- a footprint change removed supported capability;
- broad/hosted verification fails for an in-scope reason.

If only external operational evidence is missing while all implementation evidence is green, conditional closure may be considered only if the planning process and current repository policy permit it. Do not fabricate evidence.
