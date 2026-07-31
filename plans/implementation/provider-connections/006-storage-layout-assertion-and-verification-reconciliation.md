# Provider Connections Milestone 006 — Storage Layout Assertion and Verification Reconciliation

Status: closed; see `plans/closure/provider-connections/006-status.md`

Repository baseline:

- current reviewed `main`: `c0aa7852685b916cd11f7dd807198e1d82729366`
- Tool Programs M018 implementation: `42354429767f706754ce7fbe1850a03d1b2d979d`
- Tool Programs M018 merge: `c0aa7852685b916cd11f7dd807198e1d82729366`
- current global storage layout: `STORAGE_LAYOUT_VERSION = 35`
- failing hosted workflow: run `30599468088`, job `91058839160`

Source control documents:

- `plans/subsystems/provider-connections-roadmap.md`
- `plans/subsystems/provider-connections-storage-verification-reconciliation-addendum.md`
- `plans/closure/development-verification-release/006-stop-condition.md`
- `plans/implementation/tool-programs/018-runtime-fixture-contract-alignment-and-dvr-unblock.md`
- `plans/subsystems/tool-programs-runtime-fixture-closure-addendum.md`

Historical provider closure record:

- `plans/closure/provider-connections/005-status.md`

Target independent closure record:

- `plans/closure/provider-connections/006-status.md`

Primary class: stale test-contract reconciliation / canonical verification unblock

Secondary class: evidence binding / closure-governance correction

## 1. Objective

Reconcile the remaining `codegg-core` provider-connection migration test with the repository's current global storage-layout contract, prove that the migration chain and provider CRUD semantics remain correct, close the residual Tool Programs fixture-isolation question, and restore truthful independent closure sequencing for Tool Programs M018 and Development Verification and Release M006.

M006 must produce all of the following outcomes:

1. determine whether `provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe` contains a stale historical layout assertion or exposes a real migration defect;
2. if the assertion is stale, bind it to the canonical current storage-layout contract rather than another copied numeric literal;
3. preserve the complete v1–v35 migration sequence, migration idempotency, provider-connection CRUD, revision safety, and all production storage semantics;
4. rerun `tests/tool_program_runtime.rs` repeatedly from the same checkout and prove that durable fixture state cannot make a later run pass or fail incorrectly;
5. replace unresolved evidence placeholders with exact implementation, merge, and hosted-run identities;
6. restore the requirement that strict Tool Programs M018, Provider Connections M006, and DVR M006 closure decisions are made by a reviewer separate from the implementation pass;
7. make `scripts/verify.sh full` exit zero on the implementation revision;
8. obtain one successful hosted `verify` job for the same implementation revision or a planning-only successor with identical executable content;
9. leave the registry with no false completed entry in the dependency-ready table and no strict closure claim before independent review.

This milestone is intentionally narrow. The expected executable correction is one provider migration test assertion and, only if required by evidence, a test-owned Tool Programs fixture-isolation correction. It is not a storage redesign, migration rewrite, CI expansion, or broad test cleanup.

## 2. Trigger and exact current failure

Tool Programs M018 removed the six stale runtime-fixture failures. The subsequent canonical full verification reached a separate `codegg-core` failure:

```text
provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe
assertion `left == right` failed
left: 35
right: 33
```

Current implementation facts:

- `crates/codegg-core/src/storage/mod.rs` declares `pub const STORAGE_LAYOUT_VERSION: i64 = 35`;
- the migration sequence advances through provider-connection lifecycle migration v33, Tool Program attempt lineage v34, and parent-session notifications v35;
- the global `migrate()` path records the current terminal layout version in `schema_version`;
- the provider test runs the global migration path twice and then asserts the historical literal `33`;
- the same test continues with provider store CRUD and revision-safety checks;
- Tool Programs M018 executable changes do not touch provider or storage code.

The likely defect is therefore a stale test assertion that confuses the provider subsystem's historical migration number with the repository-wide terminal schema version. M006 must verify that conclusion from the current migration implementation before editing.

## 3. Governing invariants

### Storage and migration invariants

- `STORAGE_LAYOUT_VERSION` remains the canonical repository-wide terminal layout version.
- Every migration from the supported initial state through version 35 remains ordered, idempotent, and applied exactly once.
- The provider-connection migration remains represented at its historical version; later unrelated migrations may advance the global version beyond 33.
- Calling `migrate()` twice against the same database remains safe and leaves the database at the current terminal version.
- Existing provider connection rows, lifecycle metadata, secret references, revisions, timestamps, and scopes are not rewritten or weakened to satisfy a test.
- No migration is removed, renumbered, reordered, skipped, or made conditional solely to make the assertion pass.
- No production code may hard-code an older terminal version for a subsystem-local test.

### Provider correctness invariants

- Provider connection CRUD remains revision-safe.
- Stale revision updates remain rejected.
- Endpoint, TLS, scope, lifecycle, and secret-reference validation remain unchanged.
- No credentials or secret material become durable provider metadata.
- No provider selection, health, rotation, or lifecycle behavior changes in this milestone.

### Tool Programs fixture invariants

- M018's read-only frozen-contract fixture remains test-local and non-production.
- Empty and mismatched runtime contracts remain rejected.
- Emit-only programs continue to invoke no broker tool.
- Repeated test runs must not consume stale durable results from a previous identical run.
- A source digest alone must not be treated as proof of per-run isolation unless the underlying storage path is demonstrably ephemeral or cleared.
- Any correction must be test-owned; no production Tool Program behavior is changed merely to create unique test identities.

### Verification and governance invariants

- `scripts/verify.sh`, `.github/workflows/ci.yml`, test thread count, Cargo build-job limits, and `RUST_MIN_STACK` are unchanged.
- No test is ignored, deleted, filtered out, converted to `should_panic`, or moved out of the canonical workspace test command.
- Local and hosted evidence must identify the exact executable SHA.
- An implementation agent may update a milestone to `closing`; it may not grant itself strict closure.
- Existing `plans/closure/tool-programs/018-status.md` is treated as provisional implementation evidence, not independent strict approval.
- `plans/closure/provider-connections/006-status.md` and `plans/closure/development-verification-release/006-status.md` remain absent during implementation.

## 4. Scope

### In scope

- the focused test `provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe` in `crates/codegg-core/src/provider_connections.rs`;
- direct inspection of `crates/codegg-core/src/storage/mod.rs` and the current migration sequence;
- use of `STORAGE_LAYOUT_VERSION` from test code if that is the current canonical contract;
- focused provider/storage regression tests needed to establish migration and CRUD correctness;
- repeated execution and isolation review of `tests/tool_program_runtime.rs`;
- a narrow test-owned unique namespace or temporary durable location if repeated-run evidence proves the current source-digest identity insufficient;
- planning and evidence reconciliation for Provider Connections M006, Tool Programs M018, and DVR M006;
- exact local quick/full and hosted verification evidence.

### Explicitly out of scope

- changing `STORAGE_LAYOUT_VERSION` merely to match a stale test;
- renumbering, deleting, reordering, or consolidating existing migrations;
- changing production migration semantics without a demonstrated data or compatibility defect;
- broad storage refactoring, migration framework replacement, or database abstraction work;
- provider feature work, health policy changes, credential changes, endpoint changes, or Eggpool behavior changes;
- production Tool Programs, broker, scheduler, authority, interpreter, or durable-store redesign;
- broad test cleanup unrelated to the two named verification questions;
- adding retries, sleeps, global locks, or extra CI jobs;
- changing CI topology or release behavior;
- fixing projection transport unless its prior failure reappears reproducibly after the named blocker is removed;
- publishing crates or performing any release;
- creating or approving strict closure records during implementation.

## 5. Required investigation before editing

### 5.1 Freeze exact baseline evidence

Against `c0aa7852685b916cd11f7dd807198e1d82729366`, run and record:

```bash
cargo test -p codegg-core \
  provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe \
  -- --test-threads=1
```

Record:

- exact SHA;
- exact exit code;
- observed `left` and `right` values;
- whether the failure occurs before any CRUD/revision assertion;
- current `STORAGE_LAYOUT_VERSION`;
- current highest migration applied by `migrate()`.

Inspect the completed hosted run `30599468088`, job `91058839160`, and bind the first failed workspace test to the same assertion if confirmed. Do not infer hosted behavior solely from local output.

### 5.2 Inspect the global migration contract

Read the current migration dispatcher and answer explicitly:

1. Is `schema_version.version` defined as the repository-wide terminal layout version?
2. Does `migrate()` apply v34 and v35 after the provider v33 migration?
3. Is the provider test calling the global `migrate()` function rather than a provider-only migration helper?
4. Does a second `migrate()` call leave the version unchanged at the current terminal version?
5. Are the provider tables and lifecycle columns still present after v34/v35?
6. Does any supported upgrade path legitimately terminate at 33 on current code?

Expected conclusion: a test of the global migration function should compare against `STORAGE_LAYOUT_VERSION`, not the historical provider migration number. If any answer contradicts that expectation, stop before editing and document the actual migration defect.

### 5.3 Inspect existing storage tests

Search current tests for:

- assertions against `STORAGE_LAYOUT_VERSION`;
- other copied terminal-version literals;
- tests that intentionally assert a historical intermediate version;
- current migration idempotency and compatibility patterns.

Use the smallest established repository pattern. Do not introduce a helper abstraction solely for one assertion.

### 5.4 Inspect Tool Programs durable test isolation

Determine exactly which durable locations are touched by `tests/tool_program_runtime.rs`:

- source storage;
- compiled/verified program records;
- result or attempt records;
- workspace-relative state;
- any default database or cache path.

Run the focused target twice sequentially without cleaning the checkout:

```bash
cargo test --test tool_program_runtime -- --test-threads=1
cargo test --test tool_program_runtime -- --test-threads=1
```

Determine whether:

- both runs execute the intended code paths rather than replaying terminal records;
- the identical source-derived program IDs collide across runs;
- the test already uses an ephemeral database or isolated storage root;
- observable call counters and negative assertions prove fresh execution.

Do not change the fixture merely because a theoretical collision exists. Change it only if the actual storage path permits cross-run contamination or the current evidence cannot prove isolation.

## 6. Required design

### 6.1 Reconcile the migration assertion to the canonical contract

If investigation confirms a stale assertion, replace the historical terminal-version literal with the canonical current contract, preferably:

```rust
assert_eq!(version, crate::storage::STORAGE_LAYOUT_VERSION);
```

or the shortest equivalent import consistent with module visibility and repository style.

Required properties:

- the test still invokes the production global `migrate()` path twice;
- the test still verifies provider CRUD and revision safety after migration;
- the expected value tracks the canonical storage contract;
- no production migration code changes;
- no copied literal `35` is introduced unless module visibility makes the constant inaccessible and the plan record explains why.

A copied `35` is inferior because it recreates the same maintenance defect. Prefer the canonical constant.

### 6.2 Preserve historical migration meaning

Do not rewrite documentation to imply the provider migration itself moved from v33 to v35. The correct distinction is:

- v33 introduced the relevant provider-connection storage shape;
- v34 and v35 introduced later repository-wide storage changes;
- current global migration completion is v35.

Any comment added to the test should be brief and only clarify that it exercises the global migration contract.

### 6.3 Add no redundant test matrix

The existing focused test already covers:

- two migration invocations;
- terminal schema version;
- provider creation;
- retrieval;
- revision update;
- stale revision rejection.

Do not split or duplicate it unless investigation demonstrates that the failure currently masks a separate assertion that needs isolation. Prefer preserving the existing test with the truthful expected version.

### 6.4 Close the Tool Programs repeated-run question

If both sequential runs prove fresh, isolated behavior:

- record the concrete storage isolation mechanism;
- retain the M018 executable code unchanged;
- correct planning language that claimed source-digest IDs alone guaranteed isolation.

If cross-run contamination is demonstrated or cannot be excluded because the test uses persistent shared storage:

- introduce a test-owned per-process or per-run namespace using an existing temporary-directory/UUID facility;
- bind program ID, invocation key, submission key, workspace path, and any durable test root consistently where needed;
- preserve deterministic program source and contract content;
- retain all positive, negative, zero-call, and cancellation assertions;
- do not change production storage lookup or replay semantics.

Acceptance is behavioral freshness, not merely a different string format.

### 6.5 Restore closure governance

Reconcile planning state as follows:

- Tool Programs M018: `implemented; strict closure pending independent review and green canonical local/hosted evidence`;
- existing `plans/closure/tool-programs/018-status.md`: provisional conditional implementation evidence, not an independent strict closure decision;
- Provider Connections M006: `ready`, then `active`, then `closing` after implementation;
- DVR M006: remains `blocked` until full and hosted verification are green;
- the implementation pass must not create or approve:
  - `plans/closure/provider-connections/006-status.md`;
  - `plans/closure/development-verification-release/006-status.md`;
  - a strict/closed disposition in `plans/closure/tool-programs/018-status.md`.

The eventual reviewer may update existing M018 evidence into an independently reviewed strict closure only after examining the implementation and green gates.

## 7. Ordered work packages

### Work package A — Baseline and ownership freeze

Actions:

1. record the baseline SHA and focused failure;
2. inspect the hosted workspace-test failure;
3. confirm the current migration constant and sequence;
4. confirm M018 is executable-code complete but not independently closed;
5. confirm no newer commit already fixes the provider assertion.

Acceptance:

- every finding is tied to an exact SHA or workflow ID;
- the provider assertion is assigned to Provider Connections M006;
- no unrelated failure is absorbed.

### Work package B — Migration contract reconciliation

Actions:

1. prove whether the global terminal version must be 35;
2. update only the stale assertion if the production chain is correct;
3. preserve all provider CRUD/revision assertions;
4. run the focused test.

Acceptance:

```bash
cargo test -p codegg-core \
  provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe \
  -- --test-threads=1
```

exits zero, and:

- expected version derives from `STORAGE_LAYOUT_VERSION`;
- two migration calls remain;
- provider CRUD and stale-revision rejection execute;
- no production storage file changes unless a real defect was first demonstrated and the plan was amended.

### Work package C — Bounded codegg-core regression

Run:

```bash
cargo test -p codegg-core --locked -- --test-threads=1
```

Inspect failures rather than broadening automatically.

Acceptance:

- provider/storage tests pass;
- no migration compatibility regression appears;
- any unrelated failure becomes an explicit stop condition rather than an opportunistic patch.

### Work package D — Tool Programs repeated-run isolation evidence

Run the M018 target twice sequentially from the same checkout and durable environment.

Acceptance:

- both runs exit zero;
- all 13 tests run on both invocations;
- emit-only call counters remain zero;
- empty and mismatch tests fail closed as expected;
- fresh execution is demonstrated by the actual storage topology or by a narrow test-owned unique namespace;
- planning no longer claims that source digest alone guarantees per-run uniqueness unless that claim is proven.

### Work package E — Planning and evidence correction

Update:

- `plans/closure/development-verification-release/006-stop-condition.md` with exact M018 implementation/merge SHAs, hosted workflow evidence, and the Provider M006 link;
- `plans/implementation/tool-programs/018-runtime-fixture-contract-alignment-and-dvr-unblock.md` with truthful `implemented/closing` governance;
- `plans/subsystems/tool-programs-runtime-fixture-closure-addendum.md` with provisional-evidence and independent-review language;
- `plans/subsystems/provider-connections-storage-verification-reconciliation-addendum.md` with M006 status;
- `plans/registry.md` with Provider M006 as the sole ready/active handoff and M018 removed from the dependency-ready table.

Acceptance:

- no `pending M018 commit` placeholder remains;
- no completed plan appears as dependency-ready;
- exact SHAs and workflow IDs are recorded;
- strict closure remains independent.

### Work package F — Canonical local gates

Run in order:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test -p codegg-core \
  provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe \
  -- --test-threads=1
cargo test -p codegg-core --locked -- --test-threads=1
cargo test --test tool_program_runtime -- --test-threads=1
cargo test --test tool_program_runtime -- --test-threads=1
scripts/verify.sh quick
scripts/verify.sh full
```

Use the resource limits already encoded by the repository. Do not create another wrapper or parallel matrix.

Acceptance:

- every command exits zero;
- `scripts/verify.sh full` reaches completion;
- neither the provider assertion nor the old Tool Programs failures appears;
- no projection stack failure appears;
- evidence is captured against the implementation SHA.

### Work package G — Hosted verification

After executable changes are committed and pushed:

1. obtain one hosted `CI / verify` run for the implementation SHA or a planning-only successor;
2. verify every step, including `Workspace tests`, is successful;
3. record run ID, job ID, SHA, conclusion, and all step conclusions;
4. do not use an older run from before the provider correction as closure evidence.

Acceptance:

- hosted run conclusion is `success`;
- executable tree matches the locally verified implementation;
- no skipped or allowed-failure workspace test step substitutes for success.

### Work package H — Implementation disposition and independent handoff

Implementation agent actions:

- mark Provider Connections M006 `closing`;
- mark Tool Programs M018 `closing — implementation complete, independent review pending`;
- keep DVR M006 `closing/blocked pending independent closure review` until all evidence is recorded;
- leave strict closure records uncreated or unapproved as specified;
- provide the exact implementation SHA and evidence matrix to the reviewer.

Independent reviewer actions:

1. review the executable diff and verify scope containment;
2. inspect migration semantics rather than accepting the test result alone;
3. confirm repeated-run Tool Programs isolation;
4. confirm local full and hosted green evidence on the same executable revision;
5. create `plans/closure/provider-connections/006-status.md` if no high/medium provider finding remains;
6. independently upgrade Tool Programs M018 to strict closure if no high/medium Tool Programs finding remains;
7. independently create `plans/closure/development-verification-release/006-status.md` only after all DVR criteria are satisfied;
8. reconcile the registry in a later review commit.

## 8. Required verification evidence matrix

The implementation report must contain:

| Evidence | Required content |
|---|---|
| Baseline | exact `c0aa785...` SHA and focused 35-vs-33 failure |
| Hosted baseline | run `30599468088`, job `91058839160`, exact failing step/output |
| Migration contract | current constant, ordered migrations v33/v34/v35, meaning of `schema_version` |
| Changed executable files | every changed source/test file and why it is in scope |
| Production diff | explicit statement whether any production storage code changed |
| Focused provider test | command, SHA, test count, exit code |
| codegg-core suite | command, SHA, result, first failure if nonzero |
| M018 repeated runs | two commands, test counts, storage-isolation explanation |
| Formatting/check | commands and exit codes |
| Quick gate | command, SHA, exit code |
| Full gate | command, SHA, exit code and final summary |
| Hosted gate | run ID, job ID, SHA, conclusion, step list |
| Governance | exact status of M018, Provider M006, and DVR M006; closure files absent/pending |
| Registry | Provider M006 sole handoff; no completed M018 dependency-ready row |

## 9. Explicit acceptance criteria

Provider Connections M006 implementation is acceptable only when all applicable criteria are true:

1. The focused baseline failure is reproduced and bound to `c0aa785...`.
2. The hosted baseline failure is inspected directly.
3. The global migration contract is documented correctly.
4. The provider v33 migration is not confused with the global terminal version.
5. The test's expected terminal version derives from `STORAGE_LAYOUT_VERSION` unless an exact documented visibility constraint prevents it.
6. No copied replacement literal silently substitutes `35` without justification.
7. The global storage version is not lowered.
8. Existing migrations are not renumbered, reordered, removed, or skipped.
9. Migration remains idempotent across two calls.
10. Provider connection CRUD executes after migration.
11. Revision-safe update succeeds with the current revision.
12. Stale revision update remains rejected.
13. No provider production behavior changes.
14. No credential or secret-storage behavior changes.
15. The focused provider test exits zero.
16. The complete `codegg-core` test suite exits zero.
17. `tests/tool_program_runtime.rs` exits zero twice sequentially.
18. Both M018 runs execute all expected tests.
19. Repeated-run isolation is proven from actual storage behavior or corrected with test-owned isolation.
20. No production Tool Programs behavior changes.
21. Empty and mismatch runtime-contract rejection remains green.
22. No test is ignored, deleted, or excluded.
23. No CI topology, resource limit, test thread, or stack-size change is made.
24. Formatting and workspace check pass.
25. `scripts/verify.sh quick` exits zero.
26. `scripts/verify.sh full` exits zero.
27. The old Tool Programs failure is absent from full logs.
28. The provider 35-vs-33 failure is absent from full logs.
29. The projection stack failure does not reproduce; if it does, implementation stops and registers an owning plan.
30. One hosted `verify` run is green on the same executable revision.
31. The M006 stop-condition contains exact SHAs and no placeholder.
32. M018 is represented as implementation-complete but awaiting independent strict review.
33. Existing `018-status.md` is not treated as independent approval merely because it exists.
34. Provider M006 is the sole ready/active implementation handoff during execution.
35. The implementation pass does not create or approve Provider M006 strict closure.
36. The implementation pass does not create DVR M006 closure.
37. A separate reviewer performs final closure decisions.
38. No unresolved high or medium finding remains before strict closure.

## 10. Stop conditions

Stop and report rather than broadening scope if any of the following occurs:

- the database legitimately remains at version 33 after the current global migration path;
- v34 or v35 is not applied, is applied out of order, or corrupts provider tables;
- changing the assertion exposes data loss, non-idempotency, or a real migration compatibility defect;
- a production migration change appears necessary;
- the codegg-core suite exposes a different subsystem failure;
- repeated Tool Programs runs show a production replay/storage defect rather than test isolation;
- fixing Tool Programs isolation would require production scheduler, broker, or storage changes;
- the projection daemon-socket stack failure reappears reproducibly;
- local full verification and hosted verification fail for different reasons;
- a proposed fix changes CI resources or excludes tests;
- release or unrelated product work is proposed.

For a stop condition:

1. preserve the narrow work already proven correct;
2. record exact SHA, command, exit code, test, and minimal output;
3. update the blocker record truthfully;
4. register at most one next owning corrective plan;
5. do not self-close M006, M018, or DVR M006.

## 11. Smaller-model execution guidance

Execute in this order:

1. reproduce the one provider test;
2. read `STORAGE_LAYOUT_VERSION` and the migration sequence;
3. decide stale assertion versus real defect;
4. make the smallest test correction only if the production chain is correct;
5. rerun the focused provider test;
6. run the codegg-core suite;
7. run the Tool Programs target twice;
8. correct only demonstrated test-isolation weakness;
9. update exact planning evidence and governance language;
10. run quick and full once each;
11. push executable changes;
12. inspect hosted CI;
13. stop at any unrelated failure;
14. hand exact evidence to an independent reviewer.

Do not begin by editing migration production code. Do not add a new migration. Do not change version 35. Do not increase test resources. Do not rewrite the verification pipeline. Do not create a broad closure milestone.

## 12. Closure requirements

### Provider Connections M006

A separate reviewer may create `plans/closure/provider-connections/006-status.md` only after:

- the implementation is committed;
- focused and package tests are green;
- migration semantics are independently inspected;
- local full and hosted gates are green;
- no provider high/medium finding remains.

### Tool Programs M018

A separate reviewer may upgrade M018 to strict closure only after:

- the fixture implementation is reviewed independently;
- two-run isolation evidence is accepted;
- full and hosted gates are green;
- no Tool Programs high/medium finding remains.

The implementation-authored conditional record may be retained as evidence history, but it is not itself independent approval.

### Development Verification and Release M006

A separate reviewer may create `plans/closure/development-verification-release/006-status.md` only after:

- Provider Connections M006 is independently closed;
- Tool Programs M018 is independently closed;
- `scripts/verify.sh quick` and `scripts/verify.sh full` are green on the accepted executable revision;
- hosted `verify` is green on that revision or an executable-identical planning successor;
- the DVR package inventory, release documentation, guards, and CI contract remain truthful;
- no unresolved high or medium finding remains.

Strict closure of any one milestone must not be represented as strict closure of the others.
