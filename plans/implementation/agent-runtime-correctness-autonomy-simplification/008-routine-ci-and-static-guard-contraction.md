# Agent Runtime Correctness, Autonomy, and Simplification M008 — Routine CI and Static-Guard Contraction

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M008

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: verification simplification and maintenance

Dependencies:

- hard: none
- soft: M001-M006 may make some static guards obsolete through stronger type/construction ownership; reconcile against the tree at implementation time

Relevant references:

- `plans/003-planning-process.md`
- `architecture/testing.md`
- `AGENTS.md`
- historical development-verification and post-audit closure records

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/008-status.md`

## 1. Objective

Reduce the remaining routine CI/static-verification apparatus to checks that provide distinct, high-value signal for CodeGG's scope.

The default posture remains one bounded GitHub Actions `verify` job, manual release cadence, and fast local `scripts/verify.sh quick`. This milestone deletes duplicate generated-agent verification and reviews custom guards for replacement by stronger Rust construction/tests where appropriate.

## 2. Explicit non-goals

Do not:

- add CI lanes, matrices, nightly jobs, coverage, fuzzing, mutation testing, benchmarks, size gates, dependency bots, scheduled cargo-audit, artifact publication, or release automation;
- remove sandbox/execution/workspace authority checks solely because they are custom scripts;
- require full local workspace tests after every edit;
- rewrite every Python/shell guard in Rust merely for language consistency;
- add path-filter complexity or a second workflow to optimize a cheap check;
- weaken branch-visible fmt/Clippy/test failures;
- turn CI into compile-only verification;
- remove optional manual feature/LSP/security commands from documentation merely because they are not routine CI.

## 3. Current implementation evidence

Inspect at minimum:

- `.github/workflows/ci.yml`;
- `scripts/verify.sh`;
- `scripts/generate_builtin_agents.py`;
- `scripts/check_builtin_agents.py`;
- `scripts/check-core-boundary.sh`;
- `scripts/check_sandbox_contract.py`;
- `scripts/check_execution_ownership.py`;
- `scripts/check_daemon_cwd_usage.py`;
- `scripts/check_project_agent_pwd_inference.py`;
- any discovery/projection/protocol static guards still run by quick/full/CI;
- `architecture/testing.md` and `AGENTS.md`;
- focused Rust tests that now cover the same invariants after M001-M006.

Known redundancy at baseline:

- `generate_builtin_agents.py --check` parses/validates agent TOML/prompt inputs and verifies generated Rust output;
- `check_builtin_agents.py` independently reparses the generated Rust using handwritten regex/string parsing and compares it back to TOML/prompt inputs, duplicating the same synchronization property while creating a second parser to maintain;
- execution-ownership guard contains both useful unique subprocess/scheduler boundary checks and a larger manifest/classification apparatus that may duplicate explicit type/construction ownership in parts of the current tree;
- prior CI simplification already removed several invalid/duplicate checks, so this pass must not reopen a broad "simplify everything" campaign.

## 4. Invariants that cannot regress

- routine CI remains one bounded `verify` job for PRs and pushes to main;
- formatting, Clippy/type checking, and workspace test failures remain visible;
- generated builtin agent source cannot drift from authoritative TOML/prompt sources;
- Python sandbox and daemon/scheduler execution bypass regressions remain detectable;
- workspace/CWD authority regressions remain detectable unless stronger construction makes the invalid state impossible and focused tests prove it;
- manual release cadence remains unchanged;
- optional/manual feature and real-LSP verification remains available where currently useful;
- no check is retained merely because a prior plan mentioned it; every routine check must have a current invariant owner;
- no check is removed merely because it is inconvenient or occasionally catches real defects.

## 5. Generated-agent verification disposition

Default target:

- retain `scripts/generate_builtin_agents.py --check` as the single source/schema/generated-output synchronization check;
- delete `scripts/check_builtin_agents.py` and all routine/local invocations if inspection confirms it enforces no distinct invariant;
- move any unique schema validation currently present only in `check_builtin_agents.py` into the generator's existing validation path before deletion;
- do not add a third checker to verify the deletion.

Required evidence:

- generator check fails when authoritative TOML/prompt inputs diverge from generated Rust;
- generator validates all fields/permissions/runtime kinds relied upon by production;
- no unique useful mismatch case is lost.

## 6. Static-guard classification model

For each guard run in CI or `verify.sh quick`, classify it as:

- **retain routine** — unique high-value invariant that is difficult to encode through Rust types/tests;
- **retain local/full only** — useful architecture maintenance check but not required on every PR;
- **replace then delete** — a focused Rust test or constructor/type change gives stronger signal with less duplicated policy;
- **delete obsolete** — premise no longer exists or is fully duplicated.

The closure record must include the table. Do not make delete decisions from script length alone.

## 7. Guard-specific review guidance

### A. Sandbox contract

Default: retain routine if it still protects a unique security boundary not fully exercised by ordinary Rust tests.

Do not weaken this merely because M003 removes CWD state.

### B. Execution ownership

Retain the narrow checks that prevent direct process spawn/scheduler bypass where Rust crate/type boundaries do not make the bypass impossible.

Review whether the broad `docs/execution-ownership.toml` path classification remains necessary for every production source location.

Preferred simplification:

- keep direct process-spawn/bypass detection;
- keep typed argv boundary checks if not compiler-enforced;
- delete manifest classifications that merely annotate already-obvious definition/test files without catching a real regression;
- replace regex policy with direct tests/types where M003/M005 introduced an explicit constructor/executor boundary.

Do not delete execution ownership wholesale unless direct bypass has become structurally impossible.

### C. Core boundary

Inspect exact prohibited dependencies/imports. If Cargo crate dependencies and visibility now make the prohibited direction impossible, remove duplicate grep checks. Retain clauses that catch source-level bypass not represented in Cargo metadata.

### D. Daemon/project CWD guards

After M003, determine whether explicit required `ExecutionContext` construction plus multi-workspace regression tests makes some CWD grep rules redundant.

Retain a narrow production `std::env::current_dir()` authority guard if it still cheaply protects an invariant that can be reintroduced without compiler failure.

Do not scan benign CLI/bootstrap CWD usage and force unnecessary allowlist churn.

### E. Projection/discovery/protocol guards

Do not broaden this milestone into their redesign. Only change their routine disposition if inspection finds direct duplicate signal or an obsolete premise.

## 8. CI command review

Confirm the current single job and exact commands before editing.

Desired routine shape remains approximately:

```text
generated-source check(s)
high-value authority/security guards
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
bounded workspace tests
```

Do not add a separate `cargo check` if Clippy already covers the same routine targets/features. Keep local `verify.sh quick` optimized for fast feedback even if its command set differs from hosted CI.

If a guard is removed from hosted CI but remains useful locally, put it in `verify.sh full` only when that script already exists as the documented broad surface; do not create new tiers.

## 9. Ordered work packages

### Work package A — Inventory current verification surface

1. record routine CI steps and `verify.sh quick/full` commands;
2. map each command to one invariant and owner;
3. identify duplicate generated-source/compile/static signal;
4. inspect recent closure history so previously rejected/removed complexity is not accidentally reintroduced.

### Work package B — Consolidate generated-agent verification

1. compare generator validation with `check_builtin_agents.py` field-by-field;
2. move any genuinely unique validation into generator code;
3. delete `check_builtin_agents.py`;
4. remove CI/verify/docs references;
5. run generator check against deliberately stale fixture/temp output if an existing self-test mechanism supports it without adding permanent machinery.

### Work package C — Classify and narrow guards

1. evaluate sandbox/execution/core/CWD/project guards;
2. for each proposed deletion, identify the stronger type/test/compiler invariant replacing it;
3. narrow execution-ownership script to high-value bypass patterns if its manifest apparatus is no longer justified;
4. remove obsolete allowlists/docs;
5. avoid adding replacement static scripts.

### Work package D — Reconcile routine CI and local verify

1. keep one hosted job;
2. remove only duplicate/obsolete steps;
3. preserve bounded Cargo jobs/test threads unless current evidence justifies changing them;
4. ensure quick remains actually quick and does not accumulate full/manual checks;
5. preserve manual release posture.

### Work package E — Documentation

Update:

- `architecture/testing.md`;
- `AGENTS.md` verification commands;
- guard-specific architecture docs only where ownership changed.

Remove stale script names and historical claims from active guidance, while leaving closure/history documents intact.

## 10. Storage, protocol, migration, and compatibility effects

Production runtime/storage/protocol: none expected.

Developer compatibility:

- deleted checker/guard commands disappear from active documentation;
- CI job name should remain `verify` unless impossible;
- branch protection should not require renaming checks;
- no supported runtime feature changes.

## 11. Verification

Because this milestone changes verification, execute the resulting surfaces directly.

At minimum:

```bash
scripts/verify.sh quick
cargo fmt --check --all
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Run the surviving custom guards directly once.

Workspace tests may be validated by the existing hosted `verify` run if local resource cost is high; state that explicitly in closure evidence.

If `verify.sh full` is edited, run it only when practical/necessary to validate the edited branch; do not turn it into a new mandatory per-milestone gate.

## 12. Acceptance criteria

M008 closes only when:

- builtin-agent generated-source verification has one authoritative checker; the redundant handwritten generated-Rust parser is deleted if no unique invariant remains;
- every routine custom guard has a documented retain/local/replace/delete disposition;
- high-value sandbox/execution/workspace authority checks remain unless stronger direct mechanisms replace them;
- execution-ownership machinery is narrowed where classification metadata provides no distinct signal;
- no new static guard is added to enforce stylistic preferences or deleted checks;
- routine CI remains one bounded `verify` job;
- fmt, Clippy, workspace tests, and required generated/security/authority signal remain merge-visible;
- local quick verification remains small;
- release remains manual;
- resulting quick/local commands and the existing hosted verify job pass.

## 13. Stop conditions

Stop a proposed guard deletion when:

- the invariant can be violated while Rust compilation/tests still pass;
- recent history shows the guard caught a real regression class still present;
- replacing it would require significantly more test/runtime complexity than retaining the cheap guard;
- branch protection/check naming would be broken without repository-settings access.

Do not preserve the duplicate builtin-agent checker if its only value is independently reimplementing the generator parser.

## 14. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/008-status.md` must include:

- implementation commit/PR;
- before/after CI and quick-verification command lists;
- generated-agent checker comparison and deletion/migration evidence;
- static guard disposition table with replacement evidence where applicable;
- resulting quick/Clippy/surviving-guard results and hosted verify result when available;
- confirmation that no matrix, scheduled audit, artifact, coverage/benchmark/size gate, dependency bot, or release automation was added;
- unresolved developer-workflow issues by severity.
