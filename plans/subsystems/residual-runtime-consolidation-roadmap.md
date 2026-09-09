# Residual Runtime Consolidation Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related ADRs:

- None required. This roadmap preserves existing daemon, scheduler, process, agent-run, projection, and protocol ownership. Any discovered need to change one of those canonical owners is a stop condition requiring a separate ADR.

## 1. Purpose and ownership boundary

This roadmap removes the remaining obsolete coordination/metadata surfaces identified after the September 2026 maintainability closure and physically decomposes `CoreDaemon` without creating another orchestration layer.

It owns deletion or explicit disposition of stranded local modules, correction of architecture documentation that no longer matches production behavior, and cohesion-oriented extraction of daemon request/construction code. It does not redesign the daemon, scheduler, durable run model, transport protocol, or storage authority.

## 2. Work classification

### Invariants

- `CoreDaemon` remains the single daemon composition/lifecycle authority.
- Request-family extraction MUST delegate to existing domain owners and MUST NOT create duplicate state machines or stores.
- Compatibility paths remain only when a demonstrated supported consumer exists.
- A deleted legacy coordination surface MUST NOT be replaced by another filesystem mailbox or process-global coordination mechanism.

### Capabilities

- No new user capability is claimed by this roadmap.

### Infrastructure

- Smaller request-family and construction/lifecycle modules around the existing daemon state.

### Polish

- Retirement of obsolete team/shell-session surfaces and stale architecture/skill documentation.

## 3. Non-goals

- A new service bus, actor framework, dependency-injection framework, daemon protocol, or scheduler.
- Team presence, authorization, chat, or audit implementation; those belong to dedicated roadmaps.
- Building PTY support from `shell_session`; interactive processes have a separate roadmap.
- Removing public compatibility solely to reduce line count without a consumer audit.
- New CI lanes, coverage gates, benchmarks, or size gates.

## 4. Current state

At baseline `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`, the previous maintainability roadmap is closed. Agent and Bash monoliths were decomposed and mutable search/MCP globals were removed.

Residual findings remain:

- `src/agent/team.rs` implements a file-backed team/message model that predates the durable agent-run/control system; adjacent `src/agent/teams.rs` and `src/tool/teams.rs` are not declared by their parent modules and form a stranded parallel design.
- `src/shell_session/` exposes in-memory metadata CRUD but owns no PTY or execution lifecycle; it remains publicly exported and documented despite no demonstrated production consumer.
- `src/core/daemon.rs` remains the dominant physical coordinator, documented at roughly 7,686 lines, spanning many already-distinct request families plus construction/recovery/lifecycle glue.
- `architecture/core.md` and related docs contain stale request-handler claims; other docs retain removed-tool or obsolete shell-session descriptions.

## 5. Target architecture

`CoreDaemon` remains a dependency holder, lifecycle owner, and typed top-level dispatcher. Request families are implemented in narrow modules whose only authority is the existing daemon/domain state they receive. Construction, startup recovery, refresh, and shutdown logic are similarly grouped by lifecycle responsibility.

Legacy team mailbox and metadata-only shell-session code are absent unless a concrete supported consumer is proven. Future collaboration uses canonical principals, durable agent runs, projections, project channels, and audit. Future PTYs use the interactive-process subsystem.

## 6. Dependency graph

```text
M001 residual surface retirement ---- soft ----+
                                                |
M002 request-family decomposition --------------+--> M003 construction/lifecycle decomposition
```

M001 and M002 are dependency-ready and may proceed independently with merge-conflict coordination. M003 has a hard dependency on M002 because construction/lifecycle extraction should target the stabilized daemon module layout.

## 7. Milestones

### M001 — Retire stranded coordination and metadata surfaces

Class: polish

Objective: disposition the obsolete team/mailbox and `shell_session` surfaces and reconcile documentation with production truth.

Exit conditions: no undeclared team implementation remains; `shell_session` is removed or retained only with a demonstrated supported consumer and explicit removal contract; stale architecture/skill claims are corrected; no canonical collaboration or process behavior changes.

### M002 — CoreDaemon request-family physical decomposition

Class: polish

Objective: move coherent request-family implementations out of `daemon.rs` while preserving request semantics, state ownership, cancellation, authorization seams, and protocol behavior.

Exit conditions: top-level dispatch is materially easier to audit; extracted families have focused tests; no duplicate store/service/state machine is introduced; public request/response semantics remain compatible.

### M003 — CoreDaemon construction and lifecycle decomposition

Class: polish

Objective: separate daemon construction, startup recovery/bootstrap, runtime refresh, and shutdown ownership from ordinary request dispatch after M002 stabilizes the physical seams.

Exit conditions: construction/lifecycle paths have explicit modules and tests; startup/shutdown/recovery ordering is preserved; daemon remains the sole owner; no second bootstrap framework is added.

## 8. Cross-cutting requirements

Storage/protocol migrations are not expected. Deletion work must search config, serde names, docs, tests, public re-exports, and downstream-facing examples before removal. Extraction must preserve cancellation, restart recovery, projection ordering, scheduler permits, connection ownership, and bounded task lifetime. Documentation must be updated in the same milestone that changes a surface.

## 9. Verification strategy

Use focused tests for each moved/deleted surface, then the existing bounded repository posture:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Static guards are permitted only when they cheaply enforce one durable ownership invariant.

## 10. Risks and decision points

A supposedly dead public path may have an external consumer; retain it as a thin compatibility adapter if evidence exists. Mechanical daemon splitting can make navigation worse; extraction must follow actual request/lifecycle responsibility, not arbitrary file size. Any need for new durable state, public protocol semantics, or canonical owner changes stops the milestone.

## 11. Completion definition

This roadmap closes when M001-M003 have accepted closure records, obsolete parallel surfaces are gone or explicitly justified, architecture docs describe production truth, and `CoreDaemon` is materially less physically concentrated without adding a new orchestration abstraction.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/residual-runtime-consolidation/001-retire-stranded-coordination-and-metadata-surfaces.md` | `plans/closure/residual-runtime-consolidation/001-status.md` | — |
| M002 | ready | `plans/implementation/residual-runtime-consolidation/002-core-daemon-request-family-decomposition.md` | — | — |
| M003 | blocked | `plans/implementation/residual-runtime-consolidation/003-core-daemon-construction-lifecycle-decomposition.md` | — | M002 hard dependency |
