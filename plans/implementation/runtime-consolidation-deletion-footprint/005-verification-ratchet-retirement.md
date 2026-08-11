# Runtime Consolidation, Deletion, and Footprint M005 — Static Verification Ratchet Retirement and Documentation Contraction

Status: ready

Source roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Relevant references:

- `plans/003-planning-process.md`
- `architecture/testing.md`
- `docs/execution-ownership.md`
- `.github/workflows/ci.yml`
- `scripts/verify.sh`

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Primary class: simplification / verification infrastructure

Dependencies:

- hard: none;
- soft: M001-M004 may make additional migration ratchets removable; M005 may land incrementally but final closure should re-audit after those changes;
- downstream: M006 depends on the final verification surface so measurement work does not preserve obsolete tooling merely because it exists.

Target closure record:

- `plans/closure/runtime-consolidation-deletion-footprint/005-status.md`

## 1. Objective

Reduce CodeGG's accumulated static-verification and architecture-documentation machinery to the smallest set that provides distinct, correct signal for current production invariants.

Routine CI is already appropriately small: one bounded Ubuntu job, no release automation, no matrix, and no scheduled audits. This milestone MUST preserve that topology. The target is deletion of migration-only source scanners and stale implementation-mirroring documentation, not a CI redesign.

## 2. Current implementation evidence

Inspect at minimum:

- `.github/workflows/ci.yml`;
- `scripts/verify.sh`;
- `architecture/testing.md`;
- all `scripts/check_*` shell/Python guards;
- `docs/execution-ownership.toml` / `.md`;
- architecture documents changed by M001-M004;
- plan/closure records that declared temporary migration ratchets.

The repository currently contains many static scanners for concerns including execution ownership, daemon CWD usage, Git forbidden patterns, project-agent PWD inference, projection transport/publication invariants, identity-path usage, discovery invariants, provider coverage, sandbox contracts, and other historical migrations.

Some are security-relevant and should remain. Others were designed to detect transitional bypasses before a typed/module boundary became authoritative.

## 3. Guard classification contract

Every static guard reviewed by M005 must be placed in exactly one class:

### Permanent invariant guard

Retain only when:

- the invariant is security/correctness critical;
- Rust type/module visibility or focused tests cannot directly prevent the forbidden class;
- the guard is stable against normal refactoring;
- false positives/allowlists do not create substantial maintenance burden;
- it has a clear owner and focused self-test where necessary.

### Temporary migration ratchet

Delete when the migration it guarded has closed and one of the following now enforces the invariant:

- crate/module visibility;
- constructor/type system;
- canonical broker/scheduler/service boundary;
- deterministic focused tests;
- removal of the forbidden implementation entirely.

### Diagnostic/manual tool

Keep outside routine CI when it is useful for occasional maintenance but not a merge invariant, such as timing/size/audit diagnostics.

### Redundant/invalid

Delete when the premise is wrong, the check duplicates another stronger check, or it scans incidental source syntax rather than behavior/ownership.

## 4. Explicit non-goals

Do not:

- remove a security guard merely to shorten CI;
- replace deleted scanners with equally complex new scanners;
- add nextest as a required tool without measured need;
- split CI into lanes or matrices;
- add cargo-audit/cargo-deny, coverage, benchmark, fuzz, size, dependency-update, or release gates to every PR;
- weaken formatting, Clippy, workspace tests, or generated-asset synchronization without evidence;
- turn `scripts/verify.sh full` into the routine CI path;
- rewrite all architecture documentation for style.

## 5. Ordered work packages

### A. Inventory all verification mechanisms

Create a temporary review table covering:

- workflow steps;
- `verify.sh` quick/full commands;
- every `scripts/check_*` / audit script;
- machine-readable manifests consumed only by those scripts;
- test-only source annotations required solely by scanners.

For each, record:

- invariant claimed;
- last roadmap/migration that required it;
- whether routine CI invokes it;
- overlap with Rust/compiler/tests/another guard;
- false-positive/allowlist burden;
- retained/deleted/manual disposition.

The final compact disposition belongs in the closure record, not necessarily a permanent new manifest.

### B. Retire closed migration ratchets

Prioritize guards related to migrations now structurally closed by:

- explicit workspace execution context;
- canonical scheduler/job submission;
- Tool Broker authority;
- prompt/runtime-asset ownership;
- typed Git operation boundaries;
- frontend-neutral projection types.

Delete the guard, its dedicated allowlist/manifest fields, source annotations, and documentation together when they no longer provide unique signal.

Do not leave orphaned `// execution-ownership:`-style annotations if their only consumer is removed.

### C. Simplify retained guards

For permanent guards:

- narrow them to the invariant rather than file/path inventories where possible;
- prefer compiler-visible deny-by-construction boundaries;
- remove custom parsers/allowlists if normal TOML/Rust/test infrastructure already expresses the requirement;
- run self-tests locally when changing guard logic, but do not automatically make every self-test a routine CI step.

If `check_execution_ownership.py` remains, specifically review whether its manifest, regex inventory, annotations, and custom TOML parsing can contract after M001/M003. Retain only the portion that catches genuinely possible scheduler/process bypasses not prevented structurally.

### D. Keep routine CI small and non-duplicative

The expected default CI shape remains approximately:

- generated source/schema synchronization;
- only high-value permanent boundary/security guards;
- formatting;
- workspace Clippy;
- bounded workspace tests.

If a guard is deleted from CI but remains useful diagnostically, document its local invocation rather than creating a second workflow.

Do not add a separate `cargo check` step before Clippy if Clippy already provides the needed compilation signal for the same target/features.

### E. Contract architecture documentation

Review architecture docs touched by this roadmap for stale implementation mirrors.

Delete or replace:

- field-by-field struct inventories;
- exact test counts;
- obsolete type names/transition states;
- duplicated command catalogs already in scripts/help;
- historical implementation narratives better preserved in plans/closure/Git.

Retain:

- ownership boundaries;
- invariants;
- protocols/dataflow;
- lifecycle/failure semantics;
- operator-relevant constraints;
- links to canonical source/commands.

Specifically ensure `architecture/agent.md`, `architecture/scheduler.md`, and `architecture/testing.md` describe the post-M001-M004 system rather than predecessors.

### F. Verification behavior tests

For every deleted routine guard, identify the replacement evidence:

- compiler impossibility;
- visibility boundary;
- focused unit/integration/property test;
- deletion of forbidden path;
- another retained guard with stricter scope.

Do not require a one-for-one new test if the forbidden implementation is literally gone and cannot be reached.

## 6. Security review

Before deleting any guard related to:

- permission/authorization;
- sandbox/process execution;
- scheduler bypass;
- credential disclosure;
- path/workspace authority;
- projection disclosure;
- Git mutation safety;

prove equivalent or stronger enforcement. If proof is ambiguous, retain the guard and classify it permanent pending a future narrower refactor.

Deletion count is not a goal; unique signal per unit complexity is the goal.

## 7. Verification

Run affected guard self-tests/focused tests while editing them, then:

```bash
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

If `.github/workflows/ci.yml` changes, validate YAML/shell syntax with available local tools but do not add another workflow validator dependency solely for this pass.

Hosted CI evidence is centralized in M007 unless M005 changes the routine workflow itself; in that case one normal hosted run on the M005 candidate is required to prove the simplified workflow executes.

## 8. Explicit acceptance criteria

M005 is complete only when:

1. Every retained routine static guard has a documented unique invariant and owner.
2. Migration-only guards whose forbidden path is now deleted or structurally impossible are removed with their obsolete annotations/manifests/docs.
3. No deleted high-value security invariant loses coverage without equivalent or stronger structural/test enforcement.
4. Routine CI remains a single bounded job.
5. No CI matrix, scheduled workflow, artifact workflow, coverage/benchmark/size gate, automatic dependency bot, release automation, or fixed cadence is added.
6. Routine CI does not perform duplicate compilation work without distinct signal.
7. Manual diagnostics remain local/manual when they are not merge invariants.
8. `architecture/agent.md`, `architecture/scheduler.md`, and `architecture/testing.md` describe ownership/invariants rather than stale field inventories or historical migration detail.
9. Exact test totals and other rapidly stale implementation metrics are not treated as architecture contracts.
10. `scripts/verify.sh quick` and workspace Clippy pass after deletion.
11. If the routine workflow changes, one ordinary hosted CI run proves the final workflow is valid.
12. The closure record contains a guard disposition table: retained permanent, retained manual, deleted migration, deleted redundant/invalid.
13. No new static scanner is introduced merely to prove that old scanners were deleted.

## 9. Stop conditions

Do not delete a guard when its invariant remains possible to violate and no stronger direct enforcement exists. Classify it permanent and record the reason instead of forcing a deletion target.
