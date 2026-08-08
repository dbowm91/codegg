# Post-Audit Correctness, Simplification, and Footprint Milestone 006 — Test Stack and Resource Root-Cause Correction

Status: active

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- Milestone 006

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Primary class: test/runtime resource correctness

Dependencies:

- hard: none
- soft: M005 final CI environment reconciliation

Target closure record:

- `plans/closure/post-audit-correctness-simplification/006-status.md`

## 1. Objective

Identify the actual source of the daemon-socket test stack overflow currently masked by global `RUST_MIN_STACK=33554432`, make the smallest coherent correction, and remove the global 32 MiB stack requirement when evidence shows normal bounded stacks are sufficient.

This milestone is root-cause work, not a general performance campaign.

## 2. Explicit non-goals

Do not:

- raise the global stack further;
- keep the workaround under a different variable without root-cause evidence;
- rewrite the daemon transport/projection subsystem broadly;
- introduce a custom async runtime, thread pool, executor, or test harness;
- split CI into resource lanes;
- add benchmark infrastructure, profilers as dependencies, flamegraph CI, or long-running stress suites;
- optimize unrelated heap/RSS behavior;
- convert every async test runtime flavor mechanically.

## 3. Current implementation evidence

Inspect at minimum:

- `.github/workflows/ci.yml` and `scripts/verify.sh` stack environment ownership;
- `architecture/testing.md` explanation for `RUST_MIN_STACK=33554432`;
- `src/core/transport/daemon_socket.rs` and projection transport helpers;
- `tests/single_daemon_lifecycle.rs` and daemon socket integration tests;
- large async functions/futures in the failing test path;
- any historical closure records that identify the original stack-overflow reproducer.

The current documentation states that daemon-socket integration tests can stack overflow without a 32 MiB minimum stack. A global 32 MiB setting is disproportionate and can hide large-future, recursion, test-thread, or debug-stack problems rather than correcting them.

## 4. Invariants that cannot regress

- daemon/client transport behavior and projection lifecycle semantics remain unchanged;
- shutdown/cancellation and cleanup remain deterministic;
- no test becomes flaky through added sleeps or races;
- the fix must target the actual stack-heavy path, not suppress overflow diagnostics;
- ordinary tests must remain usable under the repository's bounded test-thread policy;
- production stack/thread settings must not be increased globally unless evidence proves a legitimate runtime requirement;
- no loss of transport/projection test coverage;
- no new CI lane or resource-heavy diagnostic requirement.

## 5. Investigation protocol

Start by reproducing with the override removed in a focused test process, not the whole workspace.

Recommended sequence:

1. identify the smallest test binary/test name that overflows with normal stack settings;
2. run it serially with backtrace/debug logging where useful;
3. determine whether failure depends on debug build, test thread stack, Tokio worker thread, recursion, large stack local, or generated future size;
4. inspect the largest async functions/future composition on the failing path;
5. use temporary local diagnostics such as `-Zprint-type-sizes` only if available without changing repository toolchain; otherwise prefer source decomposition and ordinary profiling/debug tools;
6. reduce the reproducer before editing architecture.

Do not require nightly Rust. Temporary local diagnostic flags need not be committed.

## 6. Candidate correction order

Apply the smallest applicable fix in this order:

1. remove accidental recursion or recursively nested protocol/test helper behavior if found;
2. move unusually large fixed-size locals/buffers off the stack where semantically appropriate;
3. split a huge async state machine into smaller awaited helpers or box one narrowly identified large future when this measurably addresses stack pressure;
4. reduce test fixture stack objects or deeply nested generated values that are not representative of production;
5. scope an increased stack only to the specific test thread/process if a genuine platform/test requirement remains and source correction is not appropriate.

A narrowly scoped stack override is an acceptable fallback only when the closure record demonstrates why the stack demand is legitimate and cannot reasonably be reduced. The preferred result is no override.

## 7. Async-function decomposition constraints

If `daemon_socket.rs` is implicated:

- split by existing lifecycle boundaries rather than arbitrary line count;
- avoid duplicating projection subscription/cleanup logic;
- do not change lock ordering or cancellation semantics merely to shrink future size;
- preserve typed response/error behavior;
- prefer private helpers that make ownership clearer even if binary size is neutral;
- measure the reproducer after each coherent decomposition so unnecessary refactors can be reverted.

Do not transform the whole handler into trait-object state machines unless direct evidence requires it.

## 8. Ordered work packages

### Work package A — Reproduce and localize

1. remove `RUST_MIN_STACK` only for the focused command;
2. reproduce the overflow deterministically;
3. identify the exact test path/function/future/thread;
4. document the minimal reproducer in implementation notes.

### Work package B — Root-cause correction

1. inspect recursion, large locals, future composition, and test fixture structure;
2. implement the smallest correction from the candidate order;
3. rerun the minimal reproducer at normal stack size;
4. verify related daemon/projection tests.

### Work package C — Remove global workaround

1. remove `RUST_MIN_STACK=33554432` from CI/verification environment if the affected suite passes without it;
2. remove stale documentation describing it as required;
3. if a narrowly scoped override remains necessary, apply it only at the smallest justified test boundary and document the measured reason.

### Work package D — Resource regression check

1. run affected tests serially under normal stack settings;
2. run `scripts/verify.sh quick`;
3. allow the existing hosted workspace test job to provide the broad integration signal;
4. do not add a stack-size regression scanner or benchmark.

## 9. Storage, protocol, migration, and compatibility effects

Storage: none.

Protocol: no change expected.

Migration: none.

Compatibility:

- daemon/socket behavior must remain wire-compatible;
- test execution should become less dependent on host-global stack configuration;
- no production user-visible behavior change expected.

## 10. Focused verification

Use the exact minimal reproducer discovered in work package A, then related tests such as:

```bash
cargo test --test single_daemon_lifecycle -- --test-threads=1
cargo test --lib core::transport::daemon_socket -- --test-threads=1
scripts/verify.sh quick
```

Run with `RUST_MIN_STACK` unset for the acceptance evidence.

If the failure occurs only in one integration test binary, run that binary fully with normal stack size. The full workspace test suite may be left to hosted CI rather than duplicated locally.

## 11. Static guards

Do not add one.

Stack consumption is not usefully enforced by regex or a fixed CI threshold. The regression test is that the actual previously failing test binary passes without the global override.

## 12. Acceptance criteria

M006 closes only when:

- the original stack-overflow reproducer is explicitly identified;
- the root cause is documented at function/test/future level rather than described only as "Rust needs more stack";
- the smallest coherent source/test correction is implemented;
- the reproducer and related transport tests pass with `RUST_MIN_STACK` unset;
- the global 32 MiB override is removed from routine CI and documentation;
- if any scoped override remains, closure evidence proves why it is legitimate and limits it to the smallest boundary;
- daemon/projection lifecycle, cancellation, protocol, and coverage are preserved;
- no new CI lane, profiler dependency, benchmark suite, or broad transport rewrite is introduced;
- `scripts/verify.sh quick` and existing hosted verification pass on the final tree.

## 13. Stop conditions

Stop and report if:

- the overflow cannot be reproduced from the documented historical test path;
- the issue is compiler/toolchain-specific and disappears on the repository-supported current toolchain without source changes;
- correcting it requires a transport/projection protocol redesign;
- evidence shows a legitimate production thread stack requirement rather than a test-only issue.

In the non-reproducible case, remove the global override only after a focused and hosted run proves it unnecessary; record that as evidence-driven cleanup rather than claiming a root cause that was not observed.

## 14. Required closure evidence

`plans/closure/post-audit-correctness-simplification/006-status.md` must include:

- implementation commit/PR;
- minimal failing/reproducer command before the fix, when reproducible;
- identified root cause;
- exact correction;
- same command passing with stack override unset;
- related focused test and quick verification outcomes;
- final CI environment disposition;
- unresolved resource concerns by severity.
