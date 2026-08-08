# Post-Audit Correctness, Simplification, and Footprint Milestone 005 — Routine CI and Static-Guard Simplification

Status: active

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- Milestone 005

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Primary class: verification simplification and maintenance

Dependencies:

- hard: none
- soft: M006 should reconcile final stack-related workflow environment after its root-cause correction

Target closure record:

- `plans/closure/post-audit-correctness-simplification/005-status.md`

## 1. Objective

Further reduce routine verification complexity while preserving distinct correctness signal appropriate to CodeGG's scope.

This milestone removes checks whose premise is incorrect or whose signal is duplicated by an immediately adjacent check. It preserves the existing one-job CI model and manual release posture.

## 2. Explicit non-goals

Do not:

- add CI lanes, matrices, target cross-builds, nightly jobs, coverage, fuzzing, mutation testing, benchmark gates, dependency bots, automatic cargo-audit, artifact uploads, or release automation;
- remove focused security/ownership checks merely because they are custom scripts;
- turn local `verify.sh full` into a mandatory every-edit workflow;
- make CI faster by reducing correctness to compilation only;
- change scheduler/runtime behavior;
- solve the 32 MiB stack issue by retaining it under a different environment variable name; M006 owns the root cause;
- rewrite every static guard into a Rust lint or proc macro.

## 3. Current implementation evidence

Inspect at minimum:

- `.github/workflows/ci.yml`;
- `scripts/verify.sh`;
- `scripts/check-tokio-test-flavors.py`;
- `scripts/tokio-test-flavor-baseline.txt`;
- any tests dedicated solely to the Tokio flavor scanner;
- `scripts/check_sandbox_contract.py`;
- `scripts/check_execution_ownership.py`;
- `scripts/check_yaml_parser_boundary.py`;
- `scripts/check-core-boundary.sh`;
- generated-agent validation scripts;
- `architecture/testing.md` and `AGENTS.md` CI documentation.

Known overengineering/redundancy:

1. Tokio's `#[tokio::test]` default runtime is current-thread/single-threaded. The repository's scanner/baseline documentation treats bare `#[tokio::test]` as an implicit default multithreaded runtime and carries a large historical baseline to prevent new instances. That premise is wrong; the machinery should be removed rather than maintained.
2. hosted CI runs `cargo check --workspace --all-targets --locked` immediately before `cargo clippy --workspace --all-targets --locked -- -D warnings`. Clippy already performs compilation/checking of those targets, so the hosted check duplicates signal. Local quick verification may retain `cargo check` for fast feedback.
3. sandbox and execution-ownership scripts run their own `--self-test` on every PR and are then immediately run normally. The self-test verifies the guard implementation, not the production tree. It should run when the guard itself changes or remain manually callable, not consume routine steps on every unrelated edit.
4. YAML/core-boundary guards may be retained or moved out of hosted CI only after reviewing whether they still enforce a unique invariant not already guaranteed by crate/dependency boundaries and existing tests.

## 4. Invariants that cannot regress

- routine CI remains one bounded job for PRs and pushes to main;
- formatting failures, Clippy warnings/errors, and workspace test failures remain merge-visible;
- generated builtin agent source/schema drift remains detected;
- high-value sandbox and execution-ownership invariants remain checked in routine verification unless replaced by a stronger direct mechanism;
- release publication remains manual and absent from routine CI;
- no direct dev-push expansion or matrix is introduced without separate product need;
- optional real-LSP, cross-platform, audit, example, and release checks remain available as targeted/manual verification where currently documented;
- local `scripts/verify.sh quick` remains a cheap developer feedback path;
- documentation must describe actual workflow behavior, not historical checks.

## 5. Tokio flavor removal requirements

Delete the obsolete system coherently:

- remove `scripts/check-tokio-test-flavors.py` if no other valid use remains;
- remove `scripts/tokio-test-flavor-baseline.txt`;
- remove dedicated scanner self-tests/tests/fixtures;
- remove CI and `verify.sh` invocations;
- correct `architecture/testing.md` and `AGENTS.md` statements about Tokio defaults;
- retain explicit `flavor = "multi_thread", worker_threads = N` only for tests that genuinely need it;
- do not mechanically rewrite every historical bare `#[tokio::test]` to `current_thread`; bare tests already have the desired default semantics.

If repository code intentionally requires explicit annotation as a style convention independent of runtime behavior, record that as a separate style preference and do not enforce it in routine CI unless the user explicitly requests it.

## 6. Hosted `cargo check` removal requirements

Remove the standalone workspace-check step from hosted routine CI when the immediately following Clippy command covers the same target/feature set.

Requirements:

- compare exact flags/targets/features before deletion;
- keep a standalone check locally in `verify.sh quick` for fast iteration if it is materially faster than Clippy;
- do not remove a compile variant that Clippy does not cover;
- document that Clippy is the hosted compile/type-check gate.

## 7. Guard self-test policy

For custom guards that contain `--self-test`:

- retain the self-test implementation when it is useful for editing the guard;
- remove unconditional routine CI invocation of the self-test immediately before the guard;
- optionally have guard-specific unit tests run when the guard changes only if existing GitHub workflow capabilities allow this without introducing a second lane or complex path-filter logic; default preference is simply manual/local self-test;
- keep the normal production-tree guard in routine CI when it protects a high-value invariant.

Do not add path-filter orchestration solely to avoid a sub-second self-test; deletion from routine CI is enough.

## 8. Remaining guard review

Classify each routine static guard as one of:

- **retain hosted** — unique high-value security/ownership/generated-source invariant;
- **local quick/full only** — useful maintenance check but not required for every PR;
- **delete** — duplicate/obsolete premise.

Default disposition to evaluate:

- generated-agent source/schema: retain hosted;
- sandbox contract: retain hosted regular guard, self-test local;
- execution ownership: retain hosted regular guard, self-test local;
- core boundary: retain hosted only if crate/compiler boundaries do not already catch the prohibited dependency/import cases;
- YAML parser boundary: local/full or delete if codegg-config dependency ownership and source structure make accidental bypass sufficiently obvious/compile-constrained;
- Tokio flavor: delete.

Any change from these defaults must be justified by concrete repository evidence, not general CI preference.

## 9. Ordered work packages

### Work package A — Inventory exact routine steps

1. record current workflow steps and commands;
2. map each to the invariant it claims to enforce;
3. identify duplicate command/feature coverage;
4. confirm the upstream Tokio default from current official documentation and repository's locked Tokio behavior.

### Work package B — Remove invalid Tokio machinery

1. delete scanner/baseline/tests;
2. remove all invocations;
3. correct testing docs;
4. run relevant script/doc searches to ensure no stale claims remain.

### Work package C — Remove duplicate hosted compile step

1. compare check and Clippy flags;
2. delete hosted `cargo check` when fully subsumed;
3. preserve local quick check;
4. ensure workflow remains simple/readable.

### Work package D — Simplify guard invocation

1. remove routine guard self-test steps;
2. classify YAML/core-boundary checks against actual unique signal;
3. move low-value checks to `verify.sh full` or delete only when safe;
4. retain normal high-value sandbox/execution guards.

### Work package E — Documentation reconciliation

1. update `architecture/testing.md`;
2. update `AGENTS.md` command/CI guidance;
3. remove references to deleted baseline/scanner;
4. preserve manual release and optional targeted verification guidance.

## 10. Storage, protocol, migration, and compatibility effects

Production runtime/storage/protocol: none.

Developer compatibility:

- CI check names should remain stable if possible; keep the job named `verify` unless there is a compelling reason otherwise;
- local commands may lose obsolete scanner invocations;
- no supported production feature changes.

## 11. Focused verification

Because this milestone edits verification itself, run the resulting commands directly:

```bash
scripts/verify.sh quick
```

Then run the commands that remain in hosted CI locally when practical, but do not create a second full local verification requirement if they are prohibitively expensive. At minimum validate workflow syntax and run:

```bash
cargo fmt --check --all
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Workspace tests may be left to the existing hosted `verify` run if local resource cost is high and the closure record says so explicitly.

Run deleted-guard searches to prove stale invocations/docs are gone.

## 12. Static guards

This milestone should reduce, not add, static guards.

Do not add a new script to assert that deleted scripts remain deleted. Git history and workflow review are sufficient.

## 13. Acceptance criteria

M005 closes only when:

- Tokio-flavor scanner/baseline machinery based on the incorrect runtime premise is removed coherently;
- documentation correctly states Tokio test defaults and when explicit multithread flavor is required;
- hosted standalone `cargo check` is removed if Clippy covers the same targets/features;
- custom guard self-tests are no longer unconditional routine CI steps;
- high-value sandbox/execution/generated-agent checks remain or have stronger direct replacements;
- YAML/core-boundary checks have an explicit retain/move/delete disposition based on unique signal;
- CI remains one bounded `verify` job with manual release posture;
- no matrix, new lane, audit automation, artifact, benchmark, coverage, or release machinery is added;
- `scripts/verify.sh quick` remains functional and the resulting workflow passes its existing hosted run.

## 14. Stop conditions

Stop a proposed deletion when:

- the adjacent check does not actually cover the same target/feature set;
- a guard catches a real class of regressions not enforced by types/crate boundaries/tests;
- branch protection depends on a check/job name that would be changed and repository settings cannot be reconciled.

Do not keep invalid Tokio machinery merely because deleting it touches many documentation lines.

## 15. Required closure evidence

`plans/closure/post-audit-correctness-simplification/005-status.md` must include:

- implementation commit/PR;
- before/after routine CI step list;
- removed/moved/retained guard table with rationale;
- explicit upstream Tokio-default evidence used for the decision;
- local quick/Clippy results and hosted verify result when available;
- confirmation that routine release/audit/matrix complexity was not reintroduced;
- unresolved developer-workflow issues by severity.
