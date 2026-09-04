# Architecture Convergence M006 — Command Pipeline Convergence

Status: blocked

Repository baseline: `3c4890035513cd4d74430b6f64523c8be676024e`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Hard dependency:

- M004 AgentLoop coordinator reduction must close.

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.7-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Primary class: infrastructure / polish

## 1. Objective

Converge command parsing, intent classification, planning, routing, authorization handoff, dispatch, and outcome mapping into one typed production pipeline. Preserve supported commands and deterministic behavior while removing overlapping interpretations spread across `command/`, `command_intent/`, `command_planner.rs`, `command_routing.rs`, and `command_outcome.rs`.

This milestone is not “delete all layers.” Distinct phases may remain when they represent genuinely distinct contracts. The requirement is one canonical data flow with no second planner/router interpretation path.

## 2. Explicit non-goals

M006 must not:

- redesign model tool-call dispatch;
- create a new generic workflow/planner engine;
- replace typed daemon requests with stringly command routing;
- change authorization policy;
- remove user-visible commands solely to simplify internals;
- add natural-language intent inference beyond existing behavior;
- change AgentLoop lifecycle ownership established by M004;
- add new verification infrastructure.

## 3. Current implementation evidence to inspect

Inspect at least:

- `src/command/`;
- `src/command_intent/`;
- `src/command_planner.rs`;
- `src/command_routing.rs`;
- `src/command_outcome.rs`;
- command entry points from TUI/CLI/agent runtime;
- command-to-CoreRequest/tool mappings;
- permission/authorization checks;
- command result/projection mapping;
- tests and static routing guards;
- legacy compatibility commands or aliases.

Create a table of all supported command families and the phases they currently traverse.

## 4. Required pipeline contract

The final production path should be conceptually equivalent to:

```text
raw command/input
      |
      v
parse + normalize
      |
      v
typed intent
      |
      v
context/authority validation
      |
      v
typed dispatch target
      |
      v
execution/daemon request
      |
      v
typed command outcome
      |
      v
projection/UI rendering
```

A phase may be skipped for commands that are already typed at entry, but the pipeline must not reparse/reinterpret a typed intent into a second independent planner result.

## 5. Ordered work packages

### WP1 — Command-family inventory

List all production command families, aliases, entry points, and current routing layers. Mark duplicate parsing/classification and dead compatibility paths.

### WP2 — Canonical intent type

Select or refine one typed intent representation. It must carry only semantic command intent and validated arguments, not UI state or execution authority.

### WP3 — Collapse planner/router duplication

Where planner and router both decide the same target, consolidate into one deterministic mapping. Preserve a separate planning phase only where it materially adds bounded semantic transformation that cannot be represented as intent normalization.

### WP4 — Typed dispatch boundary

Ensure dispatch converts canonical intent into existing typed CoreRequest/tool/service calls. Authorization remains at its canonical daemon/tool boundary; command routing may preflight but must not become the sole security check.

### WP5 — Outcome convergence

Map execution results into one typed command outcome family. Remove parallel boolean/string success/error conventions where `CommandOutcome` or existing typed results already represent them.

### WP6 — Compatibility cleanup

Remove aliases/legacy routes only when current caller/docs evidence permits it. Otherwise keep a thin normalization adapter into the canonical intent.

### WP7 — Documentation

Update command architecture/help documentation with the canonical pipeline and ownership boundaries.

## 6. Storage, protocol, migration, compatibility

No storage schema change is expected. No external protocol change is expected unless a currently stringly command path is already part of a public protocol and needs an additive typed variant. Prefer preserving protocol and changing internal routing only.

## 7. Security and failure semantics

Authorization must continue to execute at the canonical daemon/tool boundary. A normalized command must not gain authority because it came through a trusted UI path.

Invalid/ambiguous commands must fail before execution with typed diagnostics. Cancellation semantics belong to the dispatched execution owner and must pass through unchanged.

## 8. Verification

Focused tests must cover representative command families, aliases, invalid inputs, authorization denial, dispatch mapping, and outcome mapping. Add parity tests that compare old expected routing semantics for the supported command set.

Then run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not add a new command-routing harness if existing unit/integration coverage can express the invariant.

## 9. Acceptance criteria

M006 is complete only when:

- every supported command family traverses one documented typed pipeline;
- planner/router overlap is removed or justified by distinct contracts;
- aliases normalize into the canonical intent rather than fork execution;
- authorization remains canonical and unchanged;
- outcome mapping is typed and singular;
- dead compatibility routes are removed where safe;
- focused and quick verification pass.

## 10. Stop conditions

Stop if the work requires changing public command semantics, authorization architecture, or AgentLoop lifecycle. Register separate capability work rather than widening this maintenance milestone.

## 11. Closure evidence required

Record:

- command-family routing matrix before/after;
- implementation commits;
- deleted/retained aliases and rationale;
- parity/authorization tests;
- architecture documentation update;
- verification outcomes and residual findings.