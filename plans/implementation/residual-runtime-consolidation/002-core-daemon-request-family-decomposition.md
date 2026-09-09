# Residual Runtime Consolidation Milestone 002 — CoreDaemon Request-Family Decomposition

Status: ready for handoff

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/residual-runtime-consolidation-roadmap.md#M002--CoreDaemon-request-family-physical-decomposition`

Long-term requirements: `plans/000-long-term-specification.md#42-explicit-ownership`, `#7-current-foundation-and-required-evolution`, `#29-system-invariants`.

Applicable ADRs: none. Primary class: polish.

## 1. Objective

Physically decompose `src/core/daemon.rs` by coherent request families while preserving `CoreDaemon` as the single composition/lifecycle authority and preserving all protocol/runtime semantics.

## 2. Why this milestone is ready

Session/project/provider/job/projection/agent/workspace services already have canonical owners. The work is a behavior-preserving extraction and has no hard dependency on M001.

## 3. Current implementation evidence

Architecture documentation describes `CoreDaemon` at roughly 7,686 lines. It owns dependency references but also contains implementation for many request families: projects/workspaces, sessions/turns, provider connections, jobs/schedules, projections, goals/assets and operational requests. Recent AgentLoop/Bash decomposition established the preferred pattern: extract coherent responsibility without a new generic coordinator.

## 4. Invariants that must not regress

One daemon state owner; one scheduler; one session/project/workspace authority; transport-derived client ownership; request/response compatibility; projection publication order; cancellation and joined teardown; no request DTO becomes authority.

## 5. Scope

In scope: request-family module extraction, narrow helper visibility, focused tests, documentation updates. Out of scope: construction/startup/shutdown extraction (M003), storage/protocol redesign, authorization feature implementation, service bus/actor/DI frameworks, arbitrary crate split.

## 6. Required production changes

### Core/domain

Group handlers by existing domain boundaries. `CoreDaemon::handle_request*` remains top-level typed dispatch and delegates to methods/modules that operate on the same daemon-owned state.

### Storage and migrations

None.

### Protocol and DTOs

No semantic changes; preserve variants, envelopes, error codes, capability negotiation and compatibility aliases.

### Runtime and concurrency

Preserve task ownership, cancellation tokens, receiver lifetimes, queue bounds and scheduler permits exactly.

### Frontend or operator surface

No intended behavior change.

### Security and authorization

Preserve existing transport-derived client/projection policy seams; do not add or bypass future auth hooks.

### Documentation and static guards

Update `architecture/core.md` with actual module ownership. A guard may constrain growth only if simple and responsibility-based, never a raw line-count gate.

## 7. Ordered work packages

A: map each `CoreRequest` variant to canonical family/owner and identify cross-family helpers.

B: extract low-coupling families first (provider/project/job/goal/assets or current equivalents), retaining daemon methods as thin delegates where compatibility helps.

C: extract session/turn/projection families carefully with transport-owner and cancellation tests.

D: reduce dispatcher/import surface, delete now-dead helpers, update docs and focused tests.

## 8. Failure, cancellation, restart, and contention semantics

All typed errors and rollback behavior remain unchanged. A moved handler must keep the same cancellation source, scheduler admission, idempotency key, lock ordering and replay/publication semantics. Restart recovery is not redesigned.

## 9. Compatibility and migration

No data migration. Internal module paths may change; public root paths should remain or receive thin re-exports when supported consumers exist.

## 10. Required tests

Request-family unit tests plus existing real-transport projection, session selection, project activation, provider connection, job/schedule, turn control and daemon lifecycle suites. Add equivalence tests around any handler whose extraction changes call shape.

## 11. Required verification commands

```bash
cargo test --workspace core --no-fail-fast
cargo test --test projection_transport_real
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

`architecture/core.md`, `.opencode/skills/core/SKILL.md`, and any module maps affected by extraction.

## 13. Acceptance criteria

Top-level daemon dispatch is concise enough to audit request-to-owner routing without navigating unrelated implementations; each extracted module has one coherent request responsibility; all canonical owners and observable semantics remain unchanged.

## 14. Stop conditions

Stop if an extraction requires new durable state, protocol semantics, scheduler authority, request-local authority derived from DTOs, or a generic coordination framework.

## 15. Closure evidence required

Before/after ownership map, moved families, focused/broad test results, compatibility notes, cancellation/transport regression evidence, and confirmation that no new state machine/store/router was introduced.

## 16. Handoff notes

Prefer boring `impl CoreDaemon` modules or narrow existing service adapters over traits invented solely for file splitting. Coordinate M001 doc edits if parallel.
