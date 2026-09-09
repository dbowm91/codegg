# Residual Runtime Consolidation Milestone 003 — CoreDaemon Construction and Lifecycle Decomposition

Status: ready for handoff (M002 hard dependency closed via `plans/closure/residual-runtime-consolidation/002-status.md`)

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/residual-runtime-consolidation-roadmap.md#M003--CoreDaemon-construction-and-lifecycle-decomposition`

Long-term requirements: `plans/000-long-term-specification.md#42-explicit-ownership`, `#26-reliability-and-recovery`, `#29-system-invariants`.

Applicable ADRs: none. Primary class: polish.

## 1. Objective

After M002 stabilizes request-family modules, separate daemon construction, bootstrap/recovery, refresh, and shutdown implementation into explicit lifecycle modules without changing lifecycle ordering or ownership.

## 2. Why this milestone is ready

It is not dependency-ready: M002 is a hard dependency. The design contract is otherwise fixed by existing daemon lifecycle and recovery closures.

## 3. Current implementation evidence

`CoreDaemon` construction currently wires workspace/project/session stores, scheduler, provider/notification/projection/runtime dependencies and startup recovery. Prior correctness work closed singleton startup/shutdown/process lifecycle behavior. M002 will remove ordinary request-family implementation from the same file, leaving a stable lifecycle seam for this pass.

## 4. Invariants that must not regress

Singleton daemon authority; deterministic initialization order; recovery before new conflicting work; scheduler/replay/asset/provider ownership; joined shutdown; no leaked tasks/processes; same configured runtime dependencies; no second bootstrap path.

## 5. Scope

In: constructor/builder helper grouping, startup/bootstrap/recovery grouping, refresh coordinators, shutdown/join grouping, focused lifecycle tests/docs. Out: new DI container, process supervisor, schema/protocol changes, scheduler redesign, service extraction into new daemons.

## 6. Required production changes

Core: introduce responsibility modules around existing types and ordering. Storage/protocol: no changes expected. Runtime: preserve exact task creation/cancellation/join sequence and recovery semantics. Security: preserve config/secret/transport initialization boundaries. Docs: update daemon lifecycle diagrams and owners.

## 7. Ordered work packages

A — capture current construction and lifecycle ordering as tests/diagram.

B — extract dependency construction/bootstrap helpers without changing concrete ownership.

C — extract recovery/refresh and shutdown/join helpers; keep daemon entry points canonical.

D — remove dead glue/imports and reconcile docs.

## 8. Failure, cancellation, restart, and contention semantics

Construction failures must unwind without publishing a partially ready daemon. Recovery failures retain current typed behavior. Shutdown cancellation precedes joins and process cleanup as today. No lock-order changes without explicit regression evidence.

## 9. Compatibility and migration

No persisted-data or wire migration. Keep supported constructors/public paths source-compatible where practical; internal helper paths may move.

## 10. Required tests

Existing singleton/startup/shutdown/job recovery/projection transport/project activation tests plus new ordering/failure-injection coverage at extracted seams.

## 11. Required verification commands

```bash
cargo test --workspace daemon --no-fail-fast
cargo test --workspace recovery --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

`architecture/core.md`, daemon lifecycle/operations documentation, relevant core skill.

## 13. Acceptance criteria

A maintainer can find construction, startup/recovery, runtime refresh and shutdown ownership without scanning request handlers; behavior and ownership remain identical; no new framework or duplicate lifecycle exists.

## 14. Stop conditions

M002 not closed; lifecycle ordering is not understood/testable; a proposed split changes canonical ownership or requires a public protocol/schema decision.

## 15. Closure evidence required

M002 closure reference; lifecycle before/after map; exact tests including failure/restart/shutdown; compatibility statement; no-new-owner/framework confirmation.

## 16. Handoff notes

Do not optimize or reorder startup merely because extraction makes it possible. Behavioral changes discovered as desirable should become separate correctness/performance plans.
