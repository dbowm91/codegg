# Tool Programs Milestone 018 — Runtime Fixture Contract Alignment and DVR Unblock

Status: implemented — conditionally closed; see `plans/closure/tool-programs/018-status.md`

Repository baseline:

- current reviewed head: `9686338ad6aa8b0ff5ebfe8b07d74e1451180791`
- M006 in-scope implementation: `80e0919fb8a567eea8914c31cb2b9c0b6743efd4`
- DVR hosted-evidence update: `9686338ad6aa8b0ff5ebfe8b07d74e1451180791`

Source control documents:

- `plans/subsystems/tool-programs-roadmap.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`
- `plans/subsystems/tool-programs-runtime-fixture-closure-addendum.md`
- `plans/closure/development-verification-release/006-stop-condition.md`

Predecessor implementation:

- `plans/implementation/tool-programs/017-semantic-recovery-confirmation-and-evidence-closure.md`

Target independent closure record:

- `plans/closure/tool-programs/018-status.md`

Primary class: test-fixture correctness and verification unblock

Secondary class: final Tool Programs/DVR closure handoff

## 1. Objective

Repair the stale Tool Programs runtime integration fixture so it conforms to the production frozen-contract model without weakening any production authority, contract, broker, interpreter, or scheduler invariant.

M018 must produce all of the following outcomes:

1. the six currently failing completion tests in `tests/tool_program_runtime.rs` execute through `ToolProgramExecutor` with one real, non-empty, canonical frozen runtime contract;
2. the fixture derives `allowed_tools`, contract snapshot JSON, contract digest, authority digest, and authority grant from one shared source of truth;
3. emit-only programs complete without invoking the fixture tool;
4. empty or mismatched runtime-contract state remains rejected through focused negative tests;
5. the focused Tool Programs runtime test binary exits zero;
6. the canonical full verification gate is rerun to determine whether the separately reported projection-transport stack overflow remains reproducible after the deterministic Tool Programs failures are removed;
7. planning state accurately transfers final Tool Programs closure authority from M017 to M018 and keeps DVR M006 blocked until the complete gate is green.

This milestone is not a production-runtime redesign. The intended change is a narrow integration-test fixture correction plus exact verification and planning evidence.

## 2. Trigger and current failure

The DVR M006 stop-condition record identifies six deterministic failures in:

```text
tests/tool_program_runtime.rs
```

Failing tests:

- `emit_constant_completes`
- `for_loop_program_completes`
- `if_else_program_completes`
- `nested_loop_program_completes`
- `list_operations_program_completes`
- `string_operations_program_completes`

Current failure:

```text
runtime contract resolution failed: Tool Programs require at least one frozen runtime contract
```

The current `sample_job` fixture constructs mutually consistent empty state:

```text
execution_context.contract_snapshot_json = {"contracts":[]}
allowed_tools = []
authority_digest(..., allowed_tools = [])
build_authority_grant(..., allowed_tools = [], contract_digest = "")
```

That shape was accepted by the early M005 fixture but no longer satisfies the production contract-enforcement model established by later Tool Programs milestones. The production executor now correctly requires at least one frozen runtime contract.

The defect is therefore in the integration fixture, not in `resolve_contract_snapshot`, `ToolProgramExecutor`, `ToolBroker`, authority validation, or the interpreter.

## 3. Governing invariants

### Production contract invariants

- `resolve_contract_snapshot` must continue rejecting an empty runtime contract set.
- A Tool Program job must not execute with an empty, missing, malformed, or mismatched frozen contract snapshot.
- `allowed_tools`, the canonical contract entries, contract digest, execution-context snapshot, authority digest, and authority grant must describe the same tool set.
- A grant with a contract digest that does not match the serialized snapshot must fail.
- A snapshot containing a tool absent from `allowed_tools`, or an allowed tool absent from the snapshot, must fail through the existing typed error path.
- No fallback may synthesize a contract after admission or silently use the live registry in place of the frozen snapshot.

### Authority and effect invariants

- The fixture tool must be read-only.
- Its `ToolContract` must permit programmatic callers through the existing `DirectOrProgrammatic` policy.
- Its maximum effect must remain `read_only`.
- No shell, Git mutation, patch, write, process, network mutation, approval-sensitive, destructive, or subagent authority may be introduced.
- The fixture tool must not be added to the production default palette merely to satisfy the test.

### Execution invariants

- The six completion programs contain no tool call and must not invoke the fixture tool.
- `emit` remains an interpreter operation; it must not be reclassified as a broker tool call.
- Cancellation tests must continue returning `ExecutorStatus::Cancelled` through the normal executor cancellation path.
- Source persistence, parsing, IR verification, metering, and typed terminal-result behavior must remain production-shaped.

### Verification and scope invariants

- No production behavior may be weakened to make stale tests pass.
- No test may be ignored, removed, converted to `should_panic`, or excluded from the workspace command.
- `scripts/verify.sh`, `.github/workflows/ci.yml`, resource limits, and release documentation are not to be changed in this milestone.
- The separately reported projection-transport stack behavior is not owned by M018.
- If the projection failure remains after the Tool Programs fixture correction, stop and register one narrow projection-transport plan; do not absorb that work into M018.

## 4. Scope

### In scope

- `tests/tool_program_runtime.rs`;
- an optional narrowly named helper under `tests/support/` only if the fixture cannot remain clear and self-contained;
- focused assertions proving canonical snapshot/grant consistency;
- focused negative tests preserving empty/mismatch rejection;
- Tool Programs planning addendum and registry reconciliation;
- exact command evidence for the focused test binary and canonical verification rerun.

### Explicitly out of scope

- `src/tool/tool_program_context.rs` production semantics;
- `src/scheduler/tool_program_executor.rs` production behavior;
- `src/tool/broker.rs` authorization or validation behavior;
- interpreter opcodes, language semantics, budgets, checkpointing, recovery, notification delivery, artifact handling, or child jobs;
- adding a test-only production fallback;
- changing the default production registry or programmable palette;
- changing `scripts/verify.sh`, CI topology, `RUST_MIN_STACK`, Cargo job limits, or test thread count;
- fixing the projection transport daemon-socket stack overflow;
- broad test cleanup or unrelated Clippy edits;
- actual DVR M006 closure;
- creating `plans/closure/tool-programs/018-status.md` during implementation.

## 5. Required investigation before editing

Run the exact baseline commands and record exit codes:

```bash
cargo test --test tool_program_runtime -- --test-threads=1
RUST_MIN_STACK=33554432 CARGO_BUILD_JOBS=1 \
  cargo test --workspace --locked -- --test-threads=1
```

Confirm:

- the same six completion tests fail;
- `failed_program_returns_failed` and `cancelled_program_returns_cancelled` retain their current cancellation behavior;
- the error comes from runtime contract resolution before interpreter execution;
- no fixture tool is currently registered or frozen;
- no M018 change is already present at the implementation head.

Inspect and reuse the established canonical fixture pattern in:

```text
tests/tool_program_read_palette.rs
```

Specifically review its use of:

- `ToolRegistry`;
- `ToolBroker`;
- `Tool::contract`;
- `contract_entry`;
- `canonical_contract_json`;
- `canonical_contract_digest`;
- `ToolAuthorityGrant` contract fields.

Also inspect the current signatures of:

```text
ToolProgramExecutor::new
build_authority_grant
authority_digest
resolve_contract_snapshot
```

Do not copy an obsolete signature from a historical plan or commit.

## 6. Required design

### 6.1 Introduce one explicit runtime fixture contract

Create a local integration-test tool with a name that cannot be confused with a production tool, for example:

```text
runtime_fixture_read
```

Required characteristics:

- implements `Tool` only inside the integration test or test support module;
- `ToolCategory::ReadOnly`;
- `ToolCallerPolicy::DirectOrProgrammatic`;
- `ToolEffectClass::ReadOnly`;
- deterministic input schema;
- deterministic output schema;
- deterministic result;
- no filesystem, process, network, environment, clock, or random behavior;
- an atomic call counter or equivalent observation so emit-only tests can prove it was not invoked.

Illustrative contract shape only; use current APIs:

```rust
ToolContract {
    name: "runtime_fixture_read".into(),
    caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
    effect_class: ToolEffectClass::ReadOnly,
    output_schema: Some(json!({
        "type": "object",
        "properties": {"value": {"type": "string"}},
        "required": ["value"]
    })),
    ..ToolContract::legacy(tool_name, input_schema)
}
```

Do not add the fixture tool to `ToolRegistry::with_defaults()` or any production registration path.

### 6.2 Build one shared `RuntimeFixture`

Prefer a small helper object that owns all coupled test state, conceptually:

```rust
struct RuntimeFixture {
    executor: ToolProgramExecutor,
    allowed_tools: Vec<String>,
    contract_snapshot_json: String,
    contract_digest: String,
    call_count: Arc<AtomicUsize>,
}
```

Construction sequence:

1. create a test-local registry;
2. register the fixture read-only tool;
3. create the broker from that same registry;
4. resolve the fixture tool's current `ToolContract` from that registry;
5. convert it with `contract_entry`;
6. serialize exactly those entries with `canonical_contract_json`;
7. compute exactly those entries with `canonical_contract_digest`;
8. create `ToolProgramExecutor::new` with that broker and registry;
9. retain the single allowed tool name and call counter.

The fixture must not separately hand-author JSON, digest strings, or tool lists.

### 6.3 Make `sample_job` consume the fixture source of truth

Refactor `sample_job` to receive either `&RuntimeFixture` or an explicit immutable contract bundle derived from it.

Populate all related fields from the same bundle:

```text
execution_context.contract_snapshot_json
allowed_tools
authority_digest input tool list
build_authority_grant input tool list
build_authority_grant contract digest
JobPayload::ToolProgram.allowed_tools
authority_grant_json
```

Required consistency:

- the contract snapshot contains exactly `runtime_fixture_read`;
- the allowed tool list contains exactly the same name;
- the authority grant contains the canonical snapshot and digest expected by production validation;
- the authority digest is computed with the same tool list;
- no empty string is used for a required contract digest;
- the execution-context and grant policy revisions remain stable and deterministic.

Do not hard-code a digest copied from a test run.

### 6.4 Preserve emit-only semantics

For each of the six completion tests:

- construct a valid `RuntimeFixture`;
- construct the job from the fixture;
- execute through `ToolProgramExecutor`;
- assert `ExecutorStatus::Completed`;
- assert the fixture tool call counter remains zero.

This proves the contract exists for admission and execution integrity but is not used by programs that only evaluate language operations and call `emit`.

### 6.5 Preserve cancellation semantics

Update the cancellation tests to use the valid fixture contract as well.

Required assertions:

- pre-cancelled context returns `ExecutorStatus::Cancelled`;
- the fixture tool call counter remains zero;
- no contract-resolution error replaces cancellation;
- no source, child-job, or broker side effect occurs.

Rename `failed_program_returns_failed` if its actual contract is cancellation rather than failure. A truthful name such as `pre_cancelled_program_returns_cancelled` is preferable. Do not change the expected production status merely to retain the historical name.

### 6.6 Add focused negative contract tests

Add at least the following tests to the same integration target:

#### Empty frozen contract remains rejected

Construct the historical empty shape intentionally and assert:

- execution does not complete;
- the terminal status is the existing failure status;
- the summary contains the typed contract-resolution failure class or stable message fragment;
- no tool is invoked.

This prevents a future fixture cleanup from weakening production enforcement.

#### Allowed-tools/snapshot mismatch remains rejected

Construct one narrow mismatch, such as:

```text
snapshot = [runtime_fixture_read]
allowed_tools = [different_fixture_name]
```

or the inverse, according to the existing production error boundary.

Assert typed failure and zero tool calls.

#### Digest mismatch remains rejected

Only add this test if it can be expressed without duplicating broad authority coverage already present elsewhere. Mutate the fixture contract digest after canonical generation and assert validation rejects it before interpreter execution.

Do not create redundant permutations when existing M013–M017 tests already own the same invariant.

## 7. Ordered work packages

### Work package A — Freeze baseline evidence

Record:

- baseline SHA;
- exact six failing tests;
- exact failure summary;
- focused test exit code;
- workspace test exit code;
- whether the daemon-socket overflow is reached before or after the Tool Programs binary.

Acceptance:

- evidence is tied to one exact SHA;
- no failure is inferred solely from the DVR stop-condition document.

### Work package B — Build the canonical runtime fixture

Implement the read-only fixture tool and shared contract bundle.

Acceptance:

- one canonical tool entry is generated from the registered tool;
- snapshot and digest are generated with production helper functions;
- executor, broker, and registry share the same registry instance;
- no production registry is modified.

### Work package C — Align job/grant construction

Refactor `sample_job` so all contract-related values derive from the fixture.

Acceptance:

- no `{"contracts":[]}` remains in positive-path fixture construction;
- no positive-path `allowed_tools: Vec::new()` remains;
- no required contract digest is an empty string;
- grant and execution-context snapshot agree;
- source and authority digests remain deterministic.

### Work package D — Correct and extend the integration tests

Repair the six completion tests, preserve cancellation tests, and add the narrow negative tests.

Acceptance:

```bash
cargo test --test tool_program_runtime -- --test-threads=1
```

exits zero with:

- all six completion tests passing;
- cancellation tests passing;
- empty-contract rejection passing;
- mismatch rejection passing;
- no ignored tests;
- no fixture tool calls from emit-only programs.

### Work package E — Run adjacent Tool Programs regression checks

Run focused authority/contract tests that could detect accidental weakening. At minimum inspect and run the relevant targets among:

```bash
cargo test --test tool_program_read_palette -- --test-threads=1
cargo test --test tool_program_context_artifacts -- --test-threads=1
cargo test --test tool_program_m014_authority_pipeline -- --test-threads=1
cargo test --test tool_broker_integration -- --test-threads=1
```

Run only the necessary bounded set; do not invent a new test matrix.

Acceptance:

- frozen-contract and broker-authority tests remain green;
- no production behavior change is required;
- no test-only bypass becomes production reachable.

### Work package F — Re-run the canonical verification boundary

Run in order:

```bash
scripts/verify.sh quick
scripts/verify.sh full
```

Interpretation:

#### If `scripts/verify.sh full` exits zero

- push the implementation revision;
- obtain one successful hosted `verify` run for the same revision or its planning-only successor;
- update planning state to `closing`;
- leave `plans/closure/tool-programs/018-status.md` absent for an independent reviewer;
- DVR M006 may then proceed to independent closure review if no other blocker remains.

#### If the Tool Programs runtime failures are gone but the projection daemon-socket stack overflow remains

- do not modify projection code or verification resource settings;
- capture the exact command, test, signal/exit code, and minimal output;
- confirm the Tool Programs test target is green;
- mark M018 implementation landed/closing or conditionally complete as appropriate;
- register one subsequent projection-transport corrective plan as the sole next dependency-ready handoff;
- keep DVR M006 blocked.

#### If another Tool Programs failure appears

- determine whether it is directly caused by the fixture correction;
- fix only a narrow fixture-caused regression within this milestone;
- otherwise stop and document the new owning boundary.

### Work package G — Planning reconciliation

After implementation:

- mark this plan `implemented` or `closing`, never `closed`;
- mark the Tool Programs runtime-fixture addendum `closing`;
- update `plans/registry.md` with exact evidence;
- keep M017 as a conditionally accepted predecessor implementation;
- keep `plans/closure/tool-programs/017-status.md` absent;
- keep `plans/closure/tool-programs/018-status.md` absent until separate review;
- update the DVR M006 blocker text to distinguish cleared Tool Programs failure from any remaining projection failure.

The implementation agent must not write the final closure record.

## 8. Required verification commands

```bash
# Formatting and compilation for changed test code
cargo fmt --check --all
cargo check --workspace --all-targets --locked

# Primary corrected target
cargo test --test tool_program_runtime -- --test-threads=1

# Adjacent authority/contract regression coverage
cargo test --test tool_program_read_palette -- --test-threads=1
cargo test --test tool_program_context_artifacts -- --test-threads=1
cargo test --test tool_program_m014_authority_pipeline -- --test-threads=1
cargo test --test tool_broker_integration -- --test-threads=1

# Canonical verification boundary
scripts/verify.sh quick
scripts/verify.sh full
```

Use `CARGO_BUILD_JOBS=1`, `RUST_MIN_STACK=33554432`, and `--test-threads=1` through the existing canonical script. Do not create a parallel verification wrapper.

## 9. Evidence matrix

The implementation report must include:

| Evidence | Required content |
|---|---|
| Baseline | exact pre-change SHA and six failing tests |
| Changed files | every modified file with scope justification |
| Production diff | explicit confirmation that production runtime files are unchanged, or exact approved reason if investigation proves otherwise |
| Fixture contract | tool name, caller policy, effect class, canonical snapshot-generation path |
| Consistency | allowed tools, snapshot, digest, authority digest, and grant all derived from one bundle |
| Positive tests | all completion/cancellation test names and exit status |
| Negative tests | empty, mismatch, and optional digest rejection evidence |
| Broker usage | proof emit-only programs caused zero fixture-tool calls |
| Adjacent regressions | exact selected targets and results |
| Quick gate | command, SHA, exit code |
| Full gate | command, SHA, exit code, first failure if nonzero |
| Hosted gate | run ID, SHA, conclusion, and failing step if available |
| Remaining blocker | exact projection evidence if still present |
| Planning | registry/addendum status and confirmation closure files remain absent |

## 10. Explicit acceptance criteria

M018 implementation is acceptable only when all applicable criteria below are satisfied:

1. The six historical completion tests pass through `ToolProgramExecutor`.
2. Positive fixture construction contains at least one frozen runtime contract.
3. The contract is derived from a registered test-local read-only tool.
4. The executor uses a broker and registry built from the same registry instance.
5. `allowed_tools` exactly matches the frozen contract tool set.
6. `contract_snapshot_json` is generated by `canonical_contract_json`.
7. `contract_digest` is generated by `canonical_contract_digest`.
8. The authority digest uses the same allowed tool list.
9. The authority grant uses the same tool list and contract digest.
10. No positive path uses an empty contract digest or `{"contracts":[]}`.
11. Emit-only programs invoke no broker tool.
12. Cancellation behavior remains typed and unchanged.
13. Empty runtime-contract state remains rejected.
14. A tool-list/snapshot mismatch remains rejected.
15. No production contract enforcement is weakened.
16. No production default tool registration is changed.
17. No test is ignored, deleted, or excluded.
18. The focused runtime target exits zero.
19. Selected adjacent authority/contract tests exit zero.
20. `scripts/verify.sh quick` exits zero.
21. `scripts/verify.sh full` is run honestly and its result is recorded.
22. The original Tool Programs failure is absent from full/hosted logs after the correction.
23. Any remaining projection failure is not absorbed into this milestone.
24. No CI topology or verification resource change is introduced.
25. Planning state identifies M018 as the active Tool Programs closure owner.
26. Neither `017-status.md` nor `018-status.md` is created by the implementation agent.
27. A separate reviewer is required for final M018/Tool Programs closure.
28. DVR M006 remains blocked until the entire canonical full and hosted gates pass.

## 11. Stop conditions

Stop implementation and report rather than broadening scope if:

- making the fixture valid appears to require weakening `resolve_contract_snapshot`;
- the only proposed fix is allowing empty contracts for programs that currently make no tool calls;
- production registration of the fixture tool is proposed;
- current APIs cannot build a canonical contract bundle without a production design change;
- a new protocol, storage migration, scheduler path, or authority model is required;
- the Tool Programs tests expose a genuine production defect beyond fixture construction;
- the projection transport stack issue is the only remaining failure after the focused fix.

## 12. Smaller-model execution guidance

Work in this order:

1. reproduce only `tests/tool_program_runtime.rs`;
2. inspect the existing canonical snapshot helper pattern;
3. build one local read-only fixture tool;
4. centralize the contract bundle;
5. update positive jobs;
6. add negative tests;
7. run the focused target until green;
8. run the bounded adjacent targets;
9. run quick and full once;
10. stop at the first unrelated failure and record it precisely;
11. update planning state without self-closing.

Do not begin by editing production code. Do not increase test resources. Do not add retries or sleeps. Do not broaden the test palette. Do not convert the fixture into a reusable production abstraction.

## 13. Independent closure requirements

A separate reviewer may create `plans/closure/tool-programs/018-status.md` only after implementation lands.

Strict Tool Programs closure requires the reviewer to confirm:

- the fixture correction is production-faithful;
- empty and mismatched contracts remain rejected;
- no authority surface expanded;
- M017 notification/recovery implementation remains intact;
- focused and adjacent Tool Programs tests pass;
- no unresolved high or medium Tool Programs finding remains;
- planning documents agree.

DVR M006 closure is separate and additionally requires complete local `scripts/verify.sh full` and hosted `verify` success. M018 closure alone must not be represented as DVR closure if the projection-transport blocker remains.
