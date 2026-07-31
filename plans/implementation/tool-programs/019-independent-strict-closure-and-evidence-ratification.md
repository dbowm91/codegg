# Tool Programs Milestone 019 — Independent Strict Closure and Evidence Ratification

Status: ready for handoff

Repository baseline reviewed:

- current reviewed `main`: `1abb2e2c3c4f8c7480fb74b780b80eb3485ff1f9`
- M018 implementation: `42354429767f706754ce7fbe1850a03d1b2d979d`
- M018 implementation/conditional-evidence head: `3bd348c470e4d52760f2a29cd66bf429d6034335`
- M018 merge: `c0aa7852685b916cd11f7dd807198e1d82729366`
- Provider M006 executable evidence: `139c832c986106f31304d845860a66b17ba17099`
- Provider merge containing green dependency evidence: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`
- green hosted implementation run: `30603541350`, job `91071065732`

Source control documents:

- `plans/subsystems/tool-programs-roadmap.md`
- `plans/subsystems/tool-programs-runtime-fixture-closure-addendum.md`
- `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md`
- `plans/implementation/tool-programs/018-runtime-fixture-contract-alignment-and-dvr-unblock.md`
- `plans/closure/tool-programs/018-status.md`
- `plans/implementation/provider-connections/006-storage-layout-assertion-and-verification-reconciliation.md`
- `plans/closure/provider-connections/006-status.md`

Target independent closure record:

- `plans/closure/tool-programs/019-status.md`

Primary class: independent Tool Programs strict-closure review

Secondary class: repeated-run evidence ratification and DVR unblock

## 1. Objective

Perform an independently attributable strict review of Tool Programs M018 after its runtime-fixture correction, repeated-run isolation evidence, and green canonical local/hosted verification became available.

M018 corrected the stale runtime fixture and produced useful conditional implementation evidence, but the same implementation branch also created its own conditional status record. Provider M006 subsequently removed the unrelated workspace blocker and recorded two-run Tool Programs evidence plus a green hosted workflow. M019 must independently determine whether those combined facts support strict Tool Programs closure.

M019 must produce all of the following outcomes:

1. independently inspect the exact M018 executable diff and merge lineage;
2. verify that the positive runtime fixture uses one real test-local, read-only, canonical frozen contract;
3. verify that allowed tools, snapshot JSON, contract digest, authority digest, authority grant, broker, registry, executor, and job payload remain internally consistent;
4. verify that emit-only programs complete without invoking the fixture tool;
5. verify that cancellation, empty-contract rejection, snapshot/tool mismatch rejection, authority integrity, and adjacent Tool Programs behavior remain fail-closed;
6. independently resolve the repeated-run isolation question from actual storage ownership rather than relying on the source-digest program-ID claim alone;
7. validate full local and hosted evidence against an accepted executable revision;
8. classify `plans/closure/tool-programs/018-status.md` as provisional implementation-authored evidence;
9. create `plans/closure/tool-programs/019-status.md` only from a reviewer that did not author M018 implementation or the M018 conditional status record;
10. unblock DVR M006 only when M019 is strictly closed and Provider M007 is also independently closed.

This is a review-only milestone. The expected executable diff is empty.

## 2. Trigger and current discrepancy

M018 replaced the obsolete empty positive fixture with a production-shaped test bundle:

- one test-local `runtime_fixture_read` tool;
- `ToolCategory::ReadOnly`;
- `ToolCallerPolicy::DirectOrProgrammatic`;
- `ToolEffectClass::ReadOnly`;
- canonical contract snapshot and digest generated through production helpers;
- one shared allowed-tool list;
- authority digest and grant derived from the same state;
- a broker, registry, and executor using the same registered tool;
- zero-call assertions for emit-only and cancelled programs;
- focused negative tests preserving empty and mismatched contract rejection.

The focused and adjacent Tool Programs targets passed. The original full gate was blocked by a stale provider migration assertion. Provider M006 later corrected that assertion, ran the Tool Programs runtime target twice sequentially, and obtained green full and hosted verification.

Strict closure remains unsupported because:

- M018's status record was authored by the implementation branch;
- the record still describes the now-resolved provider failure and missing hosted evidence;
- it overstates source-derived program IDs as the isolation mechanism even though actual process-local store ownership is the relevant evidence;
- no independently attributable strict reviewer has reconciled the implementation, repeated-run evidence, current source, and green workspace result.

M019 owns this review and nothing beyond it.

## 3. Governing invariants

### Review independence

- The M019 reviewer must not be the agent that authored commit `42354429767f706754ce7fbe1850a03d1b2d979d`.
- The M019 reviewer must not be the agent that authored the M018 conditional status content in `3bd348c470e4d52760f2a29cd66bf429d6034335`.
- The review must occur after M018 and Provider M006 have merged to `main`.
- A second pass on the same implementation branch is not independent review.
- Shared repository credentials are acceptable only if the record identifies a distinct review agent/pass with no authorship of the implementation or provisional status.

### Frozen-contract invariants

- Positive Tool Program runtime fixtures must contain at least one frozen runtime contract.
- The contract must be generated from the registered test-local tool through current production helpers.
- `allowed_tools`, canonical entries, serialized snapshot, digest, execution context, authority digest, authority grant, and payload must represent the same tool set.
- Empty, malformed, missing, or mismatched frozen contract state must fail closed.
- No live-registry fallback may replace the frozen snapshot after admission.
- No hard-coded digest copied from a test run is acceptable.

### Authority and effect invariants

- The fixture tool remains test-local.
- The fixture tool remains read-only and deterministic.
- No filesystem, process, network, environment, clock, random, write, approval-sensitive, or destructive authority is introduced.
- The production default palette is unchanged.
- Programmatic caller policy remains explicit and bounded.
- Authority-grant integrity remains verifiable.

### Execution invariants

- `emit` remains an interpreter operation rather than a broker tool call.
- Emit-only completion cases invoke the fixture tool zero times.
- Pre-cancelled and cancelled cases return `ExecutorStatus::Cancelled` and invoke no tool.
- Source persistence, parsing, IR verification, metering, and typed terminal-result behavior remain production-shaped.
- Negative fixtures fail before unintended tool execution.

### Repeated-run invariants

- A source digest alone is not accepted as proof of per-run isolation.
- The reviewer must identify every durable or process-local location touched by the runtime target.
- Process-local `ProgramStore` instances may establish cross-process isolation when source and result ownership are demonstrably reconstructed per test process.
- Filesystem source persistence must be reviewed separately from in-memory compile/result stores.
- Two sequential runs must execute the intended assertions and must not pass through stale terminal replay.
- Any demonstrated production replay defect triggers a stop condition and separate implementation plan.

### Closure invariants

- M018's status remains historical/provisional evidence.
- M019 is the strict Tool Programs closure authority.
- Tool Programs closure does not itself ratify Provider M006 or close DVR M006.
- DVR M006 remains blocked until both Tool M019 and Provider M007 are strictly closed.

## 4. Scope

### In scope

- commit and merge lineage for M018;
- `tests/tool_program_runtime.rs`;
- current `ToolProgramExecutor`, `ProgramStore`, source-store, broker, registry, and contract-helper behavior necessary to understand the fixture;
- focused Tool Programs runtime and adjacent authority/contract tests;
- two sequential runs of the runtime target;
- inspection of Provider M006 full/hosted evidence;
- fresh evidence when executable drift makes earlier runs insufficient;
- correction of Tool Programs planning and closure state;
- creation of `plans/closure/tool-programs/019-status.md` by the independent reviewer if all criteria pass.

### Explicitly out of scope

- production Tool Programs feature work;
- broker, authority, interpreter, scheduler, storage, notification, artifact, or child-job redesign;
- adding tools to the production palette;
- changing contract-resolution semantics;
- weakening empty or mismatch rejection;
- changing CI topology, resource limits, test thread count, or verification scripts;
- provider/storage implementation work;
- DVR package or release-documentation changes;
- projection transport work unless a reproducible failure reappears;
- release execution;
- cosmetic rewriting of historical M001–M018 records beyond truthful disposition notes.

## 5. Required review before disposition

### 5.1 Establish exact lineage

Record and inspect:

```text
M018 baseline:             7dffcf8ac5edb7e0d2784d7b5844d7f09a329a76
M018 implementation:       42354429767f706754ce7fbe1850a03d1b2d979d
M018 conditional evidence: 3bd348c470e4d52760f2a29cd66bf429d6034335
M018 merge:                c0aa7852685b916cd11f7dd807198e1d82729366
Provider evidence:         139c832c986106f31304d845860a66b17ba17099
Provider merge:            7d8657e60aad85f677144b1bd0e7fb5d2929faa3
M019 reviewed baseline:     1abb2e2c3c4f8c7480fb74b780b80eb3485ff1f9
```

Confirm:

- the M018 executable diff is limited to the integration fixture and required Tokio baseline maintenance;
- no production Tool Programs runtime source changed in M018;
- the merge retained the intended fixture behavior;
- later executable changes affecting Tool Programs are identified explicitly;
- planning-only descendants are distinguished from executable drift.

### 5.2 Audit the canonical runtime fixture

Inspect the current `RuntimeFixture` and prove:

1. the fixture tool is declared only in test code;
2. the tool is registered in a test-local registry;
3. the broker is constructed from that registry;
4. `resolve_contract_snapshot` reads the same registered tool;
5. canonical JSON and digest use production helpers;
6. the executor receives the same broker and registry;
7. `sample_job` receives the fixture state rather than hand-authoring positive contract JSON;
8. allowed tools, execution context, grant, authority digest, and payload agree;
9. no required positive-path digest is empty;
10. the consistency test checks the meaningful fields and grant integrity.

### 5.3 Audit positive execution behavior

Inspect and run the six historical completion cases:

- `emit_constant_completes`;
- `for_loop_program_completes`;
- `if_else_program_completes`;
- `nested_loop_program_completes`;
- `list_operations_program_completes`;
- `string_operations_program_completes`.

Confirm each:

- executes through `ToolProgramExecutor`;
- completes successfully;
- invokes the fixture tool zero times;
- does not depend on a fallback or live contract synthesis;
- exercises current interpreter/source/IR paths.

### 5.4 Audit failure and cancellation behavior

Confirm:

- pre-cancelled execution returns `Cancelled`;
- cancellation does not become a contract-resolution failure;
- the fixture tool remains uncalled;
- an empty frozen contract fails with the stable typed failure class/message fragment;
- an allowed-tools/snapshot mismatch fails before interpreter/tool execution;
- grant integrity is retained;
- adjacent authority and broker tests remain green.

### 5.5 Resolve repeated-run isolation from ownership

Inspect:

- `ProgramStore::new()` and its backing storage;
- `ToolProgramExecutor` construction and per-execution/per-process store ownership;
- source persistence under `ToolProgramSourceStore`;
- any result, attempt, ledger, or durable terminal-state path touched by this integration target;
- program ID, invocation key, submission key, and workspace identity use.

Run exactly:

```bash
cargo test --test tool_program_runtime -- --test-threads=1
cargo test --test tool_program_runtime -- --test-threads=1
```

Record:

- both exit codes and test totals;
- whether both runs instantiate a fresh process-local compile/runtime store;
- whether persistent source files are immutable/content-addressed and incapable of terminal-result replay;
- whether zero-call and negative assertions execute on both runs;
- whether identical source-derived program IDs can collide with any actual persistent terminal-result store in this target.

Correct the M018 narrative: source-derived IDs may avoid some stale identity mismatch, but the accepted isolation proof must identify actual store ownership. Do not change code unless a demonstrated contamination path exists.

### 5.6 Run focused and adjacent verification

At minimum run on the accepted review head:

```bash
cargo fmt --all -- --check
git diff --check
cargo test --test tool_program_runtime -- --test-threads=1
cargo test --test tool_program_read_palette -- --test-threads=1
cargo test --test tool_program_context_artifacts -- --test-threads=1
cargo test --test tool_program_m014_authority_pipeline -- --test-threads=1
cargo test --test tool_broker_integration -- --test-threads=1
scripts/verify.sh quick
```

Run `scripts/verify.sh full` when:

- executable drift exists after the last accepted full/hosted evidence; or
- the reviewer cannot prove that the earlier green run covers the accepted Tool Programs tree and workspace graph.

The reviewer may reuse exact executable evidence only with a documented tree-identity comparison.

### 5.7 Audit hosted evidence

Inspect run `30603541350`, job `91071065732` and confirm:

- successful conclusion;
- exact checkout SHA;
- successful formatting, check, Clippy, and workspace-test steps;
- Tool Programs runtime tests were included in workspace tests;
- no required step was skipped or allowed to fail.

If later executable drift affects Tool Programs or the workspace test graph, require a fresh successful hosted `verify` run for the accepted executable revision or an executable-identical planning successor.

### 5.8 Audit closure governance

Document:

- why `018-status.md` is provisional implementation evidence;
- the independent M019 reviewer role/identity;
- confirmation of no M018 implementation/status authorship;
- the exact review branch and closure commit;
- all high/medium findings and their disposition.

## 6. Allowed dispositions

### Disposition A — Strict Tool Programs closure

Use only when all technical, evidence, and independence criteria pass.

Required actions:

- create `plans/closure/tool-programs/019-status.md`;
- retain `018-status.md` as historical provisional evidence;
- mark Tool Programs closed through M019;
- update roadmap/addendum and registry;
- remove M019 from ready work;
- mark Tool M019 dependency satisfied for DVR M006;
- do not alter Provider M007 disposition.

### Disposition B — Conditional closure

Use when the implementation is correct but one bounded evidence item remains.

Required actions:

- create a conditional M019 record naming the missing evidence;
- keep Tool Programs `closing`;
- keep DVR M006 blocked;
- register no production plan for an evidence-only gap;
- identify the exact evidence follow-up.

### Disposition C — Rejection and corrective implementation

Use when review finds a real runtime, authority, contract, cancellation, replay, or storage defect.

Required actions:

- do not claim strict closure;
- record the exact defect and reproduction;
- register one narrow Tool Programs corrective implementation plan;
- preserve valid M018 implementation portions;
- keep DVR M006 blocked;
- do not absorb provider or DVR work.

## 7. Ordered work packages

### Work package A — Independence and lineage freeze

Actions:

1. identify the independent reviewer;
2. record all M018 and evidence SHAs;
3. compare implementation, conditional-evidence, merge, and current review heads;
4. classify executable drift.

Acceptance:

- no M018 implementation/status authorship by the reviewer;
- exact lineage is recorded;
- no conflict or later change is assumed irrelevant without inspection.

### Work package B — Contract and authority review

Actions:

1. inspect fixture tool registration and contract generation;
2. trace all coupled contract and authority fields;
3. inspect consistency and negative tests;
4. verify no production palette/runtime change.

Acceptance:

- one canonical source of truth is demonstrated;
- all fail-closed invariants remain intact;
- no hidden fallback or hand-authored positive digest exists.

### Work package C — Execution and cancellation review

Actions:

1. inspect six completion cases;
2. inspect call counter assertions;
3. inspect cancellation behavior;
4. inspect empty and mismatch rejection;
5. run the focused target.

Acceptance:

- all cases pass;
- positive emit-only cases call no tool;
- failure cases call no tool;
- cancellation remains typed and early.

### Work package D — Repeated-run isolation proof

Actions:

1. map process-local and durable stores;
2. run the target twice sequentially;
3. prove or reject cross-run terminal replay;
4. correct evidence language without changing code when ownership already proves isolation.

Acceptance:

- two runs pass independently;
- actual storage ownership, not source digest alone, proves isolation;
- any real contamination triggers the stop condition.

### Work package E — Adjacent and canonical evidence

Actions:

1. run adjacent Tool Programs/broker targets;
2. run quick verification;
3. verify or rerun full verification;
4. inspect or obtain hosted evidence.

Acceptance:

- evidence proves the accepted executable revision;
- no unrelated failure is hidden;
- no test or CI weakening occurs.

### Work package F — Independent closure disposition

Actions:

1. classify M018 evidence as provisional history;
2. create M019 status only when justified;
3. reconcile Tool Programs planning state;
4. update DVR dependency state without closing DVR.

Acceptance:

- M019 is the strict closure authority;
- no self-review claim remains;
- DVR remains blocked until Provider M007 also closes.

## 8. Required closure evidence matrix

The M019 status record must contain at least:

| Requirement | Source/command | Revision | Result | Notes |
|---|---|---|---|---|
| Reviewer independence | reviewer declaration and lineage | review SHA | pass/fail | no M018 authorship |
| M018 executable scope | commit comparison | `4235442` | pass/fail | test/baseline only |
| Canonical contract bundle | source inspection | review SHA | pass/fail | one source of truth |
| Emit-only zero calls | six completion tests | review SHA | pass/fail | exact tests |
| Cancellation zero calls | cancellation tests | review SHA | pass/fail | typed status |
| Empty contract rejection | focused negative test | review SHA | pass/fail | fail closed |
| Snapshot/tool mismatch | focused negative test | review SHA | pass/fail | fail before execution |
| Repeated-run isolation | two sequential runs + ownership map | review SHA | pass/fail | actual store proof |
| Adjacent Tool Programs | named targets | review SHA | pass/fail | exact totals |
| Quick verification | `scripts/verify.sh quick` | review SHA | pass/fail | exit code |
| Full verification | run or identity proof | accepted executable SHA | pass/fail | exact rationale |
| Hosted verify | run/job | accepted executable SHA | pass/fail | all steps green |
| Planning ownership | M018/M019 records and registry | closure SHA | pass/fail | M019 strict authority |

No evidence row may be marked pass when its command or source inspection is incomplete.

## 9. Registry and planning updates

Before M019 completes:

- Tool Programs status: `closing`;
- current Tool Programs milestone: M019 ready/active;
- M018 status: provisional implementation evidence;
- Provider M007: independent and separately tracked;
- DVR M006: blocked on Provider M007 and Tool M019.

After strict M019 closure:

- Tool Programs status: `closed`;
- current milestone: M019 closed;
- recently closed table links `plans/closure/tool-programs/019-status.md`;
- M019 is removed from dependency-ready work;
- DVR remains blocked only on Provider M007 if still open;
- no new Tool Programs milestone is registered absent a demonstrated defect.

## 10. Stop conditions

Stop and report rather than broadening scope when:

- the fixture contract bundle is internally inconsistent;
- positive tests rely on live-registry fallback;
- emit-only programs call the fixture tool;
- empty or mismatched contracts no longer fail closed;
- cancellation semantics regress;
- repeated runs demonstrate actual terminal-result contamination;
- production Tool Programs changes are required;
- later executable drift lacks green local/hosted evidence;
- independent reviewer attribution cannot be established;
- CI/test resources or exclusions are proposed;
- provider, projection, or DVR implementation is proposed.

For a stop condition:

1. preserve valid evidence;
2. record exact SHA, command, exit code, test, and minimal output;
3. classify the owning Tool Programs boundary;
4. register at most one narrow corrective implementation plan;
5. leave Tool M019 and DVR M006 non-closed.

## 11. Completion definition

M019 is strictly complete only when:

- an independently attributable reviewer has inspected M018 after merge;
- the canonical frozen-contract fixture is correct and test-local;
- authority, effect, and broker invariants remain intact;
- six completion cases pass with zero fixture calls;
- cancellation and negative cases pass with zero fixture calls;
- repeated-run isolation is proven from actual store ownership and two sequential runs;
- focused and adjacent tests are green;
- quick, full, and hosted evidence prove the accepted executable revision;
- `018-status.md` remains historical/provisional;
- `plans/closure/tool-programs/019-status.md` records the independent strict decision;
- no unresolved high- or medium-severity Tool Programs finding remains;
- DVR is not closed prematurely.

## 12. Handoff guidance

Assign M019 to a reviewer that did not implement or conditionally close M018.

Begin with source and ownership inspection. Do not edit production code. The expected output is an independent evidence record and planning reconciliation. Any executable change indicates a real defect and requires a separately registered corrective implementation milestone rather than expansion of this review plan.
