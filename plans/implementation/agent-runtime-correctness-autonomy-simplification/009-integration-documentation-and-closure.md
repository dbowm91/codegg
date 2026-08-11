# Agent Runtime Correctness, Autonomy, and Simplification M009 — Integration, Documentation, and Closure

Status: blocked

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M009

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: integration and closure

Dependencies:

- hard: M001 MCP authority/provenance/tool-surface correctness closed
- hard: M002 textual tool-call repair safety closed
- hard: M003 workspace-bound AgentLoop construction closed
- hard: M004 turn identity/accounting/lifecycle correctness closed
- hard: M005 recovery/autonomy state-machine simplification closed
- hard: M006 prompt/control-policy consolidation closed
- hard: M007 measured binary/upstream dependency review closed
- hard: M008 routine CI/static-guard contraction closed

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- all architecture documents changed by M001-M008

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md`

## 1. Objective

Perform one final integration, documentation, minimal broad verification, and closure pass after M001-M008 land.

This milestone does not introduce new product architecture. It proves that the corrected authority/workspace/turn semantics and the simplified recovery/prompt/verification machinery work together on the accepted final tree, captures the final footprint/upstream state, and closes or explicitly classifies every remaining audit finding.

## 2. Explicit non-goals

Do not:

- use closure as an excuse for another broad refactor;
- reopen closed M001-M008 scope without new concrete evidence;
- add new CI lanes, matrices, coverage, fuzzing, benchmark/size gates, scheduled dependency checks, artifacts, or release automation;
- require duplicate local full-workspace runs when the existing hosted verify run provides equivalent final-tree evidence;
- introduce new features or protocol/storage migrations;
- force a binary-size improvement if M007 correctly concluded that no worthwhile no-feature-loss candidate exists;
- create corrective plans for low-severity deferred ideas unless they are real closure blockers.

## 3. Preconditions

Before starting M009:

- each M001-M008 plan has a closure record with accepted disposition;
- no milestone remains `active`, `closing`, or blocked on an internal dependency;
- any conditional external evidence is explicitly identified and evaluated for whether it actually blocks this workstream;
- current `main` or the integration branch contains all accepted implementation commits;
- there are no unresolved critical/high findings in the source closure records.

If these conditions are not met, do not simulate closure by running broad tests. Resolve the specific predecessor first.

## 4. Integration invariants to re-check

### Authority

- unknown/raw MCP tools do not receive blanket approval;
- textual tool-call repair never bypasses resolved tool surface or permission checks;
- recovery cannot restore user/profile-denied tools;
- Tool Program/broker authorization remains consistent with ADR-0001;
- execution provenance remains truthful after recovery/refactor integration.

### Workspace isolation

- every production loop/snapshot/path decision remains rooted in explicit execution context;
- no M005/M006 refactor reintroduced process-global CWD authority;
- concurrent workspace tests remain green.

### Turn/autonomy correctness

- current-turn prompt drives turn-local heuristics;
- session-origin goal remains stable where intended;
- goal accounting uses exact deltas across autonomous continuations;
- exactly one `AgentFinished` occurs per agent-loop terminal lifecycle;
- one recovery state machine owns malformed/no-progress/soft-stop decisions;
- strong-model normal paths do not incur unnecessary synthetic bootstrap/provider turns;
- fragile-model compatibility fixtures still work through explicit adapter policy.

### Prompt/context correctness

- startup behavior is compiled once and fingerprinted;
- plan mode advertises only actual resolved capabilities;
- runtime-asset snapshot semantics remain immutable;
- dynamic steering/recovery/todo/notification controls remain correctly late/volatile;
- prompt simplification did not alter permission/tool authority.

### Footprint/verification posture

- accepted dependency changes preserve supported features;
- final default release size is recorded consistently with M007 baseline methodology;
- plugin/Wasmtime upstream security disposition remains current enough for closure or is rechecked if the implementation interval materially changed;
- routine CI is still one bounded verify job;
- release remains manual.

## 5. Required documentation reconciliation

Review active architecture/developer docs, not historical closure files, for stale statements.

At minimum inspect:

- `architecture/agent.md`;
- `architecture/tool.md`;
- `architecture/permission.md`;
- `architecture/provider.md`;
- `architecture/cache-aware-context.md`;
- `architecture/goal.md`;
- `architecture/core.md`;
- `architecture/testing.md`;
- `AGENTS.md`;
- model-profile/config documentation if changed;
- plugin docs if Wasmtime requirements changed.

Required reconciliations:

- no stale description of blanket MCP auto-approval;
- no stale claim that arbitrary text-tool parsing is generic behavior;
- no stale synthetic bootstrap/retry-counter description after M005;
- no legacy prompt assembler described as production after M006;
- snapshot/workspace ownership matches M003;
- terminal event and goal accounting ownership match M004;
- CI command/guard list matches M008 exactly enough for developer use;
- deferred ideas are not presented as current requirements.

## 6. Closure test matrix

Run focused cross-milestone regression tests first. Prefer deterministic test names/fixtures introduced by M001-M006 rather than recreating behavior manually.

Minimum integrated cases:

```text
raw unknown MCP mutation -> Ask
repaired textual mutation -> same Ask path
structured-only model JSON example -> no execution
fragile-model textual fixture -> repaired call -> normal permission path
workspace A/B concurrent turns -> independent snapshot/path roots
multi-turn session -> current turn drives research/routing
2+ autonomous continuations -> exact tool/token accounting
successful turn -> one AgentFinished with usage/stop reason
failed/cancelled turn -> one correct terminal lifecycle
strong-model final answer -> no synthetic bootstrap
repeated no-progress -> bounded Stall
plan-mode prompt/tool surface consistency
startup profile contract appears once
```

Run the surviving authority/security/CWD static guards from M008 once.

## 7. Minimal broad verification

Local final integration:

```bash
scripts/verify.sh quick
```

Then run one broad compile/test surface appropriate to the final touched tree. Preferred if practical:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
```

If M007 changed optional plugin/server/LSP feature manifests, additionally run only the affected feature compile/test commands recorded by M007. Do not run every feature permutation by default.

Hosted evidence:

- use the existing single `verify` workflow on the actual final merge candidate;
- one passing hosted run is sufficient;
- do not create a new workflow/closure lane.

If local broad tests are prohibitively resource-heavy and hosted `verify` executes the same relevant test surface, the closure record may rely on hosted evidence after local quick/focused tests. State this explicitly rather than duplicating expensive work ceremonially.

## 8. Final footprint/upstream evidence

Using M007's documented host/toolchain methodology where feasible:

- capture final default release binary bytes;
- compare with M007 pre-change baseline and any accepted M007 milestone measurement;
- explain changes caused by M001-M006 code deletion/refactor separately from dependency changes only if measurable;
- do not claim causation from size difference without isolated evidence;
- recheck exact Wasmtime/plugin lock against current applicable security fixes if enough time/upstream change has occurred since M007 to make the earlier check stale.

No binary-size target is required. The closure requirement is truthful measurement and no feature reduction.

## 9. Closure finding review

Read every M001-M008 closure record and classify residual items:

- critical/high: workstream cannot close; create a narrow corrective implementation plan;
- medium: normally corrective pass required unless clearly external/operational and product-safe with explicit user acceptance;
- low/info: may be deferred with rationale and ownership;
- speculative future improvement: keep unregistered unless the user/product roadmap explicitly activates it.

Do not create a new plan merely to collect unavailable external evidence unless the implementation is otherwise incomplete and the evidence materially affects safety/correctness.

## 10. Registry and roadmap updates

On successful closure:

- mark M001-M009 closed in the subsystem roadmap with links to closure records;
- mark the subsystem closed in `plans/registry.md`;
- remove its rows from dependency-ready/blocked active work;
- add only the latest relevant entries to recently closed sections so the registry remains compact;
- preserve historical implementation/closure files in place according to planning process;
- do not edit canonical long-term documents unless a genuine direction change was accepted during implementation.

If a corrective pass is required, keep the subsystem active and register only that bounded corrective plan.

## 11. Acceptance criteria

M009 closes only when:

- M001-M008 have accepted closure records;
- integrated authority tests prove external/text-repaired tools use the same permission boundary;
- integrated workspace-isolation tests prove no process-CWD authority regression;
- current-turn/accounting/terminal lifecycle regressions remain fixed;
- M005 recovery is bounded and no duplicate historical recovery path remains active;
- M006 startup prompt/control composition has one authoritative path;
- M007 final measurement/upstream safety evidence is recorded without feature loss;
- M008 routine CI remains one bounded job with redundant guards/checkers removed as accepted;
- active architecture/testing/developer docs match the final implementation;
- focused integration tests and `scripts/verify.sh quick` pass;
- one broad final test/compile surface passes locally or through equivalent existing hosted verify evidence;
- the existing hosted `verify` workflow passes on the actual final merge candidate;
- no unresolved critical/high findings remain;
- any medium finding has an explicit corrective disposition rather than being silently deferred;
- `plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md` contains the complete evidence matrix.

## 12. Stop conditions

Do not mark closed if:

- any M001-M008 closure record is missing or conditional on unresolved internal correctness work;
- final integration reveals permission bypass, cross-workspace leakage, double accounting, duplicate terminal events, unbounded recovery, or generic prose execution;
- documentation materially contradicts production behavior;
- the final hosted verify run is failing due to a repository defect;
- a security-relevant dependency remains below a known applicable fixed version without an accepted mitigation.

If hosted failure is clearly external infrastructure after all repository steps pass, record the exact external condition and decide conditional closure according to existing planning policy; do not invent a new CI system to obtain a green badge.

## 13. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md` must include:

- final implementation commits/PRs for M001-M008;
- requirement-to-evidence matrix covering every roadmap exit condition;
- cross-milestone authority/workspace/autonomy integration test results;
- quick verification result;
- broad local or equivalent hosted test result;
- hosted `verify` run ID/result on the final candidate;
- final CI/guard command shape;
- final default release size and M007 comparison context;
- current plugin/Wasmtime upstream security disposition if applicable;
- documentation reconciliation list;
- unresolved finding table by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked;
- registry/roadmap status updates performed.